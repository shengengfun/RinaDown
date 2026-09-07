//! 设置视图模型：agent 快照投影、乐观本地覆盖、防抖合并写回与动作调用。
//!
//! 页面闭包只经 `Entity<SettingsStore>` 读写；视图 observe 本实体重绘。

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

use fluxdown_protocol::{
    AgentEvent, AgentPreferencesDto, AgentSnapshot, ApplicationErrorCode, ComponentStatusDto,
    ConnPolicySummaryDto, DaemonConfigPatch, DaemonConfigSnapshot, DaemonEvent,
    DiagnosticsReportDto, GatewayPatchParams, GatewayStatusDto, PlatformIntegrationDto, PluginDto,
    QueueDto, RpcErrorData, ServiceEvent, SettingOwner, SiteAuthEntryDto, SyncStatusDto,
    UpdateCheckResultDto, WebhookDeliveryDto, method, setting_spec, setting_value_kind,
    value_to_daemon_config,
};
use gpui::{Context, SharedString};
use serde_json::{Value, json};

use crate::port::{PortFuture, SettingsPort};

/// 本地编辑到写回 RPC 的合并窗口。
const FLUSH_DEBOUNCE: Duration = Duration::from_millis(250);
/// daemon 修订冲突时的自动重试上限。
const MAX_CONFLICT_RETRIES: u8 = 3;

/// 设置写回失败的 UI 可展示分类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsErrorKind {
    Disconnected,
    Conflict,
    InvalidArgument,
    Failed,
}

impl SettingsErrorKind {
    #[must_use]
    pub fn from_rpc(error: &RpcErrorData) -> Self {
        match error.code {
            ApplicationErrorCode::Unavailable => Self::Disconnected,
            ApplicationErrorCode::Conflict => Self::Conflict,
            ApplicationErrorCode::InvalidArgument => Self::InvalidArgument,
            _ => Self::Failed,
        }
    }

    /// 对应 `assets/i18n` 的既有键。
    #[must_use]
    pub fn i18n_key(self) -> &'static str {
        match self {
            Self::Disconnected => "localServiceDisconnected",
            Self::Conflict => "localServiceConflict",
            Self::InvalidArgument => "localServiceInvalidArgument",
            Self::Failed => "localServiceActionFailed",
        }
    }
}

/// 一次设置写回的可展示错误。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsError {
    pub kind: SettingsErrorKind,
    /// 校验失败时的具体说明（引擎/协议给出的英文原文）；无则为空。
    pub detail: SharedString,
}

pub struct SettingsStore {
    port: Arc<dyn SettingsPort>,
    daemon: DaemonConfigSnapshot,
    gateway: GatewayStatusDto,
    preferences: AgentPreferencesDto,
    sync: SyncStatusDto,
    queues: Vec<QueueDto>,
    plugins: Vec<PluginDto>,
    components: Vec<ComponentStatusDto>,
    webhook_deliveries: Vec<WebhookDeliveryDto>,
    session: Option<fluxdown_protocol::AgentSessionDto>,
    daemon_connected: bool,
    stale: bool,

    /// 已编辑、尚未发送的 daemon 原始键（不经云同步目录）。
    pending_daemon: BTreeMap<String, String>,
    /// 已发送、等待回执的 daemon 原始键。
    inflight_daemon: BTreeMap<String, String>,
    /// 已编辑、尚未发送的偏好；`bool` = 是否进入云同步。
    pending_prefs: BTreeMap<String, (Value, bool)>,
    inflight_prefs: BTreeMap<String, (Value, bool)>,
    flush_scheduled: bool,
    flush_inflight: bool,
    conflict_retries: u8,

    // ── 按需加载的动作结果 ──
    integration: Option<PlatformIntegrationDto>,
    diagnostics: Option<DiagnosticsReportDto>,
    site_auth: Vec<SiteAuthEntryDto>,
    conn_policy: Option<ConnPolicySummaryDto>,
    update_check: Option<UpdateCheckResultDto>,
    busy: BTreeSet<&'static str>,
    last_error: Option<SettingsError>,
    /// 最近一次动作的成功提示（i18n 键）。
    last_notice: Option<&'static str>,
    /// 页面级临时值（测试结果等），不持久化、不发送。
    transient: BTreeMap<&'static str, Value>,
}

impl SettingsStore {
    #[must_use]
    pub fn new(port: Arc<dyn SettingsPort>) -> Self {
        Self {
            port,
            daemon: DaemonConfigSnapshot::default(),
            gateway: GatewayStatusDto::default(),
            preferences: AgentPreferencesDto::default(),
            sync: SyncStatusDto::default(),
            queues: Vec::new(),
            plugins: Vec::new(),
            components: Vec::new(),
            webhook_deliveries: Vec::new(),
            session: None,
            daemon_connected: false,
            stale: true,
            pending_daemon: BTreeMap::new(),
            inflight_daemon: BTreeMap::new(),
            pending_prefs: BTreeMap::new(),
            inflight_prefs: BTreeMap::new(),
            flush_scheduled: false,
            flush_inflight: false,
            conflict_retries: 0,
            integration: None,
            diagnostics: None,
            site_auth: Vec::new(),
            conn_policy: None,
            update_check: None,
            busy: BTreeSet::new(),
            last_error: None,
            last_notice: None,
            transient: BTreeMap::new(),
        }
    }

    // ───────────────────────── 快照与事件 ─────────────────────────

    pub fn replace_snapshot(&mut self, snapshot: &AgentSnapshot, cx: &mut Context<Self>) {
        self.daemon.clone_from(&snapshot.daemon.config);
        self.gateway.clone_from(&snapshot.gateway);
        self.preferences.clone_from(&snapshot.preferences);
        self.sync.clone_from(&snapshot.sync);
        self.queues.clone_from(&snapshot.daemon.queues);
        self.plugins.clone_from(&snapshot.daemon.plugins);
        self.components.clone_from(&snapshot.daemon.components);
        self.webhook_deliveries
            .clone_from(&snapshot.daemon.webhook_deliveries);
        self.session.clone_from(&snapshot.session);
        self.daemon_connected = snapshot.daemon_connected;
        self.stale = false;
        self.overlay_local_edits();
        self.last_error = None;
        cx.notify();
    }

    pub fn apply_event(&mut self, event: &ServiceEvent, cx: &mut Context<Self>) {
        let ServiceEvent::Agent(event) = event else {
            return;
        };
        match event {
            AgentEvent::Daemon(DaemonEvent::ConfigChanged(config)) => {
                self.daemon.clone_from(config);
                self.overlay_local_edits();
            }
            AgentEvent::Daemon(DaemonEvent::QueuesChanged(queues)) => {
                self.queues.clone_from(queues)
            }
            AgentEvent::Daemon(DaemonEvent::PluginsChanged(plugins)) => {
                self.plugins.clone_from(plugins)
            }
            AgentEvent::Daemon(DaemonEvent::ComponentsChanged(components)) => {
                self.components.clone_from(components)
            }
            AgentEvent::Daemon(DaemonEvent::WebhooksChanged(deliveries)) => {
                self.webhook_deliveries.clone_from(deliveries)
            }
            AgentEvent::GatewayChanged(gateway) => self.gateway.clone_from(gateway),
            AgentEvent::PreferencesChanged(preferences) => {
                self.preferences.clone_from(preferences);
                self.overlay_local_edits();
            }
            AgentEvent::SyncChanged(sync) => self.sync.clone_from(sync),
            AgentEvent::SessionChanged(session) => self.session.clone_from(session.as_ref()),
            AgentEvent::DaemonSnapshotReplaced(snapshot) => {
                self.daemon.clone_from(&snapshot.config);
                self.queues.clone_from(&snapshot.queues);
                self.plugins.clone_from(&snapshot.plugins);
                self.components.clone_from(&snapshot.components);
                self.webhook_deliveries
                    .clone_from(&snapshot.webhook_deliveries);
                self.overlay_local_edits();
                self.daemon_connected = true;
            }
            AgentEvent::DaemonConnectionChanged(connected) => self.daemon_connected = *connected,
            _ => return,
        }
        cx.notify();
    }

    pub fn mark_stale(&mut self, cx: &mut Context<Self>) {
        self.stale = true;
        self.pending_daemon.clear();
        self.inflight_daemon.clear();
        self.pending_prefs.clear();
        self.inflight_prefs.clear();
        self.last_error = Some(SettingsError {
            kind: SettingsErrorKind::Disconnected,
            detail: SharedString::default(),
        });
        cx.notify();
    }

    /// 服务端快照到达时把尚未回执的本地编辑重新盖上去，避免输入框回跳。
    fn overlay_local_edits(&mut self) {
        for (key, value) in self
            .inflight_daemon
            .iter()
            .chain(self.pending_daemon.iter())
        {
            self.daemon.values.insert(key.clone(), value.clone());
        }
        for (key, (value, _)) in self.inflight_prefs.iter().chain(self.pending_prefs.iter()) {
            self.preferences.values.insert(key.clone(), value.clone());
        }
    }

    // ───────────────────────── 只读投影 ─────────────────────────

    #[must_use]
    pub fn is_read_only(&self) -> bool {
        self.stale
    }
    #[must_use]
    pub fn daemon_connected(&self) -> bool {
        self.daemon_connected && !self.stale
    }
    #[must_use]
    pub fn gateway(&self) -> &GatewayStatusDto {
        &self.gateway
    }
    #[must_use]
    pub fn sync_status(&self) -> &SyncStatusDto {
        &self.sync
    }
    #[must_use]
    pub fn session(&self) -> Option<&fluxdown_protocol::AgentSessionDto> {
        self.session.as_ref()
    }
    #[must_use]
    pub fn queues(&self) -> &[QueueDto] {
        &self.queues
    }
    #[must_use]
    pub fn plugins(&self) -> &[PluginDto] {
        &self.plugins
    }
    #[must_use]
    pub fn components(&self) -> &[ComponentStatusDto] {
        &self.components
    }
    #[must_use]
    pub fn webhook_deliveries(&self) -> &[WebhookDeliveryDto] {
        &self.webhook_deliveries
    }
    #[must_use]
    pub fn integration(&self) -> Option<&PlatformIntegrationDto> {
        self.integration.as_ref()
    }
    #[must_use]
    pub fn diagnostics(&self) -> Option<&DiagnosticsReportDto> {
        self.diagnostics.as_ref()
    }
    #[must_use]
    pub fn site_auth(&self) -> &[SiteAuthEntryDto] {
        &self.site_auth
    }
    #[must_use]
    pub fn conn_policy(&self) -> Option<&ConnPolicySummaryDto> {
        self.conn_policy.as_ref()
    }
    #[must_use]
    pub fn update_check(&self) -> Option<&UpdateCheckResultDto> {
        self.update_check.as_ref()
    }
    #[must_use]
    pub fn transient(&self, key: &str) -> Option<&Value> {
        self.transient.get(key)
    }
    pub fn set_transient(&mut self, key: &'static str, value: Value, cx: &mut Context<Self>) {
        self.transient.insert(key, value);
        cx.notify();
    }
    #[must_use]
    pub fn is_busy(&self, action: &str) -> bool {
        self.busy.contains(action)
    }
    #[must_use]
    pub fn last_error(&self) -> Option<&SettingsError> {
        self.last_error.as_ref()
    }
    #[must_use]
    pub fn last_notice(&self) -> Option<&'static str> {
        self.last_notice
    }
    pub fn clear_feedback(&mut self, cx: &mut Context<Self>) {
        if self.last_error.take().is_some() | self.last_notice.take().is_some() {
            cx.notify();
        }
    }

    // ───────────────────────── daemon 配置 ─────────────────────────

    /// daemon 原始字符串值；缺省取协议目录默认值。
    #[must_use]
    pub fn daemon_str(&self, key: &str) -> String {
        self.daemon
            .values
            .get(key)
            .cloned()
            .unwrap_or_else(|| fluxdown_protocol::daemon_config_default(key).to_owned())
    }
    #[must_use]
    pub fn daemon_bool(&self, key: &str) -> bool {
        matches!(self.daemon_str(key).as_str(), "true" | "1")
    }
    #[must_use]
    pub fn daemon_i64(&self, key: &str) -> i64 {
        self.daemon_str(key).trim().parse().unwrap_or_else(|_| {
            fluxdown_protocol::daemon_config_default(key)
                .parse()
                .unwrap_or(0)
        })
    }
    #[must_use]
    pub fn daemon_f64(&self, key: &str) -> f64 {
        self.daemon_str(key).trim().parse().unwrap_or_else(|_| {
            fluxdown_protocol::daemon_config_default(key)
                .parse()
                .unwrap_or(0.0)
        })
    }

    /// 写入一个 daemon 配置键（wire 字符串）。
    ///
    /// 键若在云同步目录内则经 `agent.preferences.patch` 走同步链路；
    /// 否则直接进入防抖合并的 `daemon.config.patch`。
    pub fn set_daemon(&mut self, key: &str, value: impl Into<String>, cx: &mut Context<Self>) {
        if self.stale {
            self.set_error(SettingsErrorKind::Disconnected, "", cx);
            return;
        }
        let value = value.into();
        let normalized = match fluxdown_protocol::normalize_daemon_config_value(key, &value) {
            Ok(normalized) => normalized,
            Err(error) => {
                self.set_error(SettingsErrorKind::InvalidArgument, error.to_string(), cx);
                return;
            }
        };
        if self.daemon.values.get(key) == Some(&normalized) {
            return;
        }
        self.daemon
            .values
            .insert(key.to_owned(), normalized.clone());
        if let Some(spec) = fluxdown_protocol::SYNC_SETTING_SPECS
            .iter()
            .find(|spec| spec.owner == SettingOwner::Daemon && spec.storage_key == key)
        {
            let json = daemon_string_to_json(spec.key, &normalized);
            self.pending_prefs.insert(spec.key.to_owned(), (json, true));
        } else {
            self.pending_daemon.insert(key.to_owned(), normalized);
        }
        self.schedule_flush(cx);
        cx.notify();
    }
    pub fn set_daemon_bool(&mut self, key: &str, value: bool, cx: &mut Context<Self>) {
        self.set_daemon(key, value.to_string(), cx);
    }
    pub fn set_daemon_i64(&mut self, key: &str, value: i64, cx: &mut Context<Self>) {
        self.set_daemon(key, value.to_string(), cx);
    }
    pub fn set_daemon_f64(&mut self, key: &str, value: f64, cx: &mut Context<Self>) {
        self.set_daemon(key, value.to_string(), cx);
    }

    // ───────────────────────── agent 偏好 ─────────────────────────

    #[must_use]
    pub fn pref(&self, key: &str) -> Option<&Value> {
        self.preferences.values.get(key)
    }
    #[must_use]
    pub fn pref_bool(&self, key: &str, default: bool) -> bool {
        self.pref(key).and_then(Value::as_bool).unwrap_or(default)
    }
    #[must_use]
    pub fn pref_str(&self, key: &str, default: &str) -> String {
        self.pref(key)
            .and_then(Value::as_str)
            .map_or_else(|| default.to_owned(), str::to_owned)
    }
    #[must_use]
    pub fn pref_i64(&self, key: &str, default: i64) -> i64 {
        self.pref(key).and_then(Value::as_i64).unwrap_or(default)
    }
    #[must_use]
    pub fn pref_f64(&self, key: &str, default: f64) -> f64 {
        self.pref(key).and_then(Value::as_f64).unwrap_or(default)
    }

    /// 写入偏好。云同步目录内的键自动进入同步；其余键为设备本地。
    pub fn set_pref(&mut self, key: &str, value: Value, cx: &mut Context<Self>) {
        if self.stale {
            self.set_error(SettingsErrorKind::Disconnected, "", cx);
            return;
        }
        let synced = match setting_spec(key) {
            Some(spec) => {
                if let Err(error) = fluxdown_protocol::validate_value(spec.key, &value) {
                    self.set_error(SettingsErrorKind::InvalidArgument, error, cx);
                    return;
                }
                if spec.owner == SettingOwner::Daemon {
                    // daemon 键的读侧是 daemon 快照；写侧仍经同步链路。
                    if let Ok(wire) = value_to_daemon_config(spec, &value) {
                        self.daemon.values.insert(spec.storage_key.to_owned(), wire);
                    }
                }
                spec.owner != SettingOwner::Excluded
            }
            None => false,
        };
        if self.preferences.values.get(key) == Some(&value) {
            return;
        }
        self.preferences
            .values
            .insert(key.to_owned(), value.clone());
        self.pending_prefs.insert(key.to_owned(), (value, synced));
        self.schedule_flush(cx);
        cx.notify();
    }
    pub fn set_pref_bool(&mut self, key: &str, value: bool, cx: &mut Context<Self>) {
        self.set_pref(key, Value::Bool(value), cx);
    }
    pub fn set_pref_str(&mut self, key: &str, value: impl Into<String>, cx: &mut Context<Self>) {
        self.set_pref(key, Value::String(value.into()), cx);
    }
    pub fn set_pref_i64(&mut self, key: &str, value: i64, cx: &mut Context<Self>) {
        self.set_pref(key, Value::from(value), cx);
    }

    // ───────────────────────── 网关 ─────────────────────────

    pub fn patch_gateway(&mut self, patch: GatewayPatchParams, cx: &mut Context<Self>) {
        if self.stale {
            self.set_error(SettingsErrorKind::Disconnected, "", cx);
            return;
        }
        if let Some(value) = patch.takeover_enabled {
            self.gateway.takeover_enabled = value;
        }
        if let Some(value) = patch.jsonrpc_enabled {
            self.gateway.jsonrpc_enabled = value;
        }
        if let Some(value) = patch.api_enabled {
            self.gateway.api_enabled = value;
        }
        if let Some(value) = patch.mcp_enabled {
            self.gateway.mcp_enabled = value;
        }
        if let Some(value) = patch.cors_enabled {
            self.gateway.cors_enabled = value;
        }
        if let Some(value) = patch.lan_enabled {
            self.gateway.lan_enabled = value;
        }
        let params = serde_json::to_value(patch).unwrap_or_else(|_| json!({}));
        self.call_with(
            "gateway",
            method::AGENT_GATEWAY_PATCH,
            params,
            cx,
            |this, result, cx| {
                if let Ok(value) = result
                    && let Ok(gateway) = serde_json::from_value::<GatewayStatusDto>(value)
                {
                    this.gateway = gateway;
                    this.reveal_gateway_token(cx);
                    cx.notify();
                }
            },
        );
    }

    /// 用户 token 只在本机 UI 按需读取，不进快照；结果放入 `transient("gateway_user_token")`。
    pub fn reveal_gateway_token(&mut self, cx: &mut Context<Self>) {
        self.call_with(
            "gatewayToken",
            method::AGENT_GATEWAY_REVEAL_TOKEN,
            json!({}),
            cx,
            |this, result, cx| {
                if let Ok(value) = result
                    && let Some(token) = value.get("userToken").and_then(Value::as_str)
                {
                    this.set_transient("gateway_user_token", Value::String(token.to_owned()), cx);
                }
            },
        );
    }

    // ───────────────────────── 通用动作 ─────────────────────────

    /// 发起一次 RPC；`action` 用于 `is_busy`，完成后回调更新状态。
    pub fn call_with(
        &mut self,
        action: &'static str,
        method: &'static str,
        params: Value,
        cx: &mut Context<Self>,
        on_done: impl FnOnce(&mut Self, Result<Value, RpcErrorData>, &mut Context<Self>) + 'static,
    ) {
        if self.stale {
            self.set_error(SettingsErrorKind::Disconnected, "", cx);
            return;
        }
        self.busy.insert(action);
        self.last_error = None;
        self.last_notice = None;
        cx.notify();
        let future = self.port.call(method, params);
        cx.spawn(async move |this, cx| {
            let result = future.await;
            let _ = this.update(cx, |this, cx| {
                this.busy.remove(action);
                if let Err(error) = &result {
                    this.last_error = Some(SettingsError {
                        kind: SettingsErrorKind::from_rpc(error),
                        detail: SharedString::default(),
                    });
                }
                on_done(this, result, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// 无结果关心的动作；成功后展示 `notice` 提示（i18n 键）。
    pub fn call_simple(
        &mut self,
        action: &'static str,
        method: &'static str,
        params: Value,
        notice: Option<&'static str>,
        cx: &mut Context<Self>,
    ) {
        self.call_with(action, method, params, cx, move |this, result, _| {
            if result.is_ok() {
                this.last_notice = notice;
            }
        });
    }

    pub fn raw_call(&self, method: &'static str, params: Value) -> PortFuture<Value> {
        self.port.call(method, params)
    }

    pub fn load_integration(&mut self, cx: &mut Context<Self>) {
        self.call_with(
            "integration",
            method::AGENT_PLATFORM_INTEGRATION_GET,
            json!({}),
            cx,
            Self::absorb_integration,
        );
    }
    pub fn set_autostart(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.call_with(
            "integration",
            method::AGENT_PLATFORM_SET_AUTOSTART,
            json!({ "enabled": enabled }),
            cx,
            Self::absorb_integration,
        );
    }
    pub fn set_file_association(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.call_with(
            "integration",
            method::AGENT_PLATFORM_SET_FILE_ASSOCIATION,
            json!({ "enabled": enabled }),
            cx,
            Self::absorb_integration,
        );
    }
    pub fn set_url_protocol(&mut self, scheme: &str, enabled: bool, cx: &mut Context<Self>) {
        self.call_with(
            "integration",
            method::AGENT_PLATFORM_SET_URL_PROTOCOL,
            json!({ "scheme": scheme, "enabled": enabled }),
            cx,
            Self::absorb_integration,
        );
    }
    fn absorb_integration(&mut self, result: Result<Value, RpcErrorData>, _cx: &mut Context<Self>) {
        if let Ok(value) = result
            && let Ok(dto) = serde_json::from_value::<PlatformIntegrationDto>(value)
        {
            self.integration = Some(dto);
        }
    }

    pub fn run_diagnostics(&mut self, cx: &mut Context<Self>) {
        self.call_with(
            "diagnostics",
            method::AGENT_DIAGNOSTICS_RUN,
            json!({}),
            cx,
            |this, result, _| {
                if let Ok(value) = result
                    && let Ok(report) = serde_json::from_value::<DiagnosticsReportDto>(value)
                {
                    this.diagnostics = Some(report);
                }
            },
        );
    }
    pub fn repair_diagnostics(
        &mut self,
        params: fluxdown_protocol::DiagnosticRepairParams,
        cx: &mut Context<Self>,
    ) {
        let params = serde_json::to_value(params).unwrap_or_else(|_| json!({}));
        self.call_with(
            "diagnostics",
            method::AGENT_DIAGNOSTICS_REPAIR,
            params,
            cx,
            |this, result, cx| {
                if result.is_ok() {
                    this.run_diagnostics(cx);
                }
            },
        );
    }

    pub fn load_site_auth(&mut self, cx: &mut Context<Self>) {
        self.call_with(
            "siteAuth",
            method::DAEMON_SITE_AUTH_LIST,
            json!({}),
            cx,
            Self::absorb_site_auth,
        );
    }
    pub fn delete_site_auth(&mut self, site: &str, cx: &mut Context<Self>) {
        self.call_with(
            "siteAuth",
            method::DAEMON_SITE_AUTH_DELETE,
            json!({ "site": site }),
            cx,
            Self::absorb_site_auth,
        );
    }
    pub fn clear_site_auth(&mut self, cx: &mut Context<Self>) {
        self.call_with(
            "siteAuth",
            method::DAEMON_SITE_AUTH_CLEAR,
            json!({}),
            cx,
            Self::absorb_site_auth,
        );
    }
    fn absorb_site_auth(&mut self, result: Result<Value, RpcErrorData>, _cx: &mut Context<Self>) {
        if let Ok(value) = result
            && let Ok(entries) = serde_json::from_value::<Vec<SiteAuthEntryDto>>(value)
        {
            self.site_auth = entries;
        }
    }

    pub fn load_conn_policy(&mut self, cx: &mut Context<Self>) {
        self.call_with(
            "connPolicy",
            method::DAEMON_CONFIG_CONN_POLICY,
            json!({}),
            cx,
            Self::absorb_conn_policy,
        );
    }
    pub fn clear_conn_policy(&mut self, cx: &mut Context<Self>) {
        self.call_with(
            "connPolicy",
            method::DAEMON_CONFIG_CLEAR_CONN_POLICY,
            json!({}),
            cx,
            Self::absorb_conn_policy,
        );
    }
    fn absorb_conn_policy(&mut self, result: Result<Value, RpcErrorData>, _cx: &mut Context<Self>) {
        if let Ok(value) = result
            && let Ok(summary) = serde_json::from_value::<ConnPolicySummaryDto>(value)
        {
            self.conn_policy = Some(summary);
        }
    }

    pub fn check_update(&mut self, channel: Option<String>, cx: &mut Context<Self>) {
        self.call_with(
            "update",
            method::AGENT_UPDATE_CHECK,
            json!({ "channel": channel }),
            cx,
            |this, result, _| {
                if let Ok(value) = result
                    && let Ok(dto) = serde_json::from_value::<UpdateCheckResultDto>(value)
                {
                    this.update_check = Some(dto);
                }
            },
        );
    }

    /// 退出前把尚未发送的编辑立即打成 RPC future 交给调用方等待（不再走防抖）。
    #[must_use]
    pub fn drain_pending_calls(&mut self) -> Vec<PortFuture<Value>> {
        if self.stale || (self.pending_daemon.is_empty() && self.pending_prefs.is_empty()) {
            return Vec::new();
        }
        let daemon_values = std::mem::take(&mut self.pending_daemon);
        let prefs = std::mem::take(&mut self.pending_prefs);
        self.build_flush_calls(daemon_values, prefs)
    }

    // ───────────────────────── 写回泵 ─────────────────────────

    fn schedule_flush(&mut self, cx: &mut Context<Self>) {
        if self.flush_scheduled {
            return;
        }
        self.flush_scheduled = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FLUSH_DEBOUNCE).await;
            let _ = this.update(cx, |this, cx| {
                this.flush_scheduled = false;
                this.flush(cx);
            });
        })
        .detach();
    }

    fn flush(&mut self, cx: &mut Context<Self>) {
        if self.flush_inflight || self.stale {
            return;
        }
        if self.pending_daemon.is_empty() && self.pending_prefs.is_empty() {
            return;
        }
        self.flush_inflight = true;
        let daemon_values = std::mem::take(&mut self.pending_daemon);
        let prefs = std::mem::take(&mut self.pending_prefs);
        self.inflight_daemon = daemon_values.clone();
        self.inflight_prefs = prefs.clone();

        let calls = self.build_flush_calls(daemon_values, prefs);

        cx.spawn(async move |this, cx| {
            let mut first_error: Option<RpcErrorData> = None;
            for call in calls {
                if let Err(error) = call.await
                    && first_error.is_none()
                {
                    first_error = Some(error);
                }
            }
            let _ =
                this.update(cx, |this, cx| {
                    this.flush_inflight = false;
                    match first_error {
                        None => {
                            this.inflight_daemon.clear();
                            this.inflight_prefs.clear();
                            this.conflict_retries = 0;
                            if this.last_error.as_ref().is_some_and(|error| {
                                error.kind != SettingsErrorKind::InvalidArgument
                            }) {
                                this.last_error = None;
                            }
                        }
                        Some(error)
                            if error.code == ApplicationErrorCode::Conflict
                                && this.conflict_retries < MAX_CONFLICT_RETRIES =>
                        {
                            this.conflict_retries += 1;
                            let inflight = std::mem::take(&mut this.inflight_daemon);
                            for (key, value) in inflight {
                                this.pending_daemon.entry(key).or_insert(value);
                            }
                            let inflight = std::mem::take(&mut this.inflight_prefs);
                            for (key, value) in inflight {
                                this.pending_prefs.entry(key).or_insert(value);
                            }
                            this.schedule_flush(cx);
                        }
                        Some(error) => {
                            this.inflight_daemon.clear();
                            this.inflight_prefs.clear();
                            this.conflict_retries = 0;
                            this.last_error = Some(SettingsError {
                                kind: SettingsErrorKind::from_rpc(&error),
                                detail: SharedString::default(),
                            });
                        }
                    }
                    if !this.pending_daemon.is_empty() || !this.pending_prefs.is_empty() {
                        this.schedule_flush(cx);
                    }
                    cx.notify();
                });
        })
        .detach();
    }

    fn build_flush_calls(
        &self,
        daemon_values: BTreeMap<String, String>,
        prefs: BTreeMap<String, (Value, bool)>,
    ) -> Vec<PortFuture<Value>> {
        let mut calls: Vec<PortFuture<Value>> = Vec::new();
        if !daemon_values.is_empty() {
            let patch = DaemonConfigPatch {
                expected_revision: self.daemon.revision,
                values: daemon_values,
            };
            let params = serde_json::to_value(patch).unwrap_or_else(|_| json!({}));
            calls.push(self.port.call(method::DAEMON_CONFIG_PATCH, params));
        }
        let (synced, local): (BTreeMap<_, _>, BTreeMap<_, _>) = prefs
            .into_iter()
            .partition::<BTreeMap<String, (Value, bool)>, _>(|(_, (_, synced))| *synced);
        if !synced.is_empty() {
            let values: serde_json::Map<String, Value> = synced
                .into_iter()
                .map(|(key, (value, _))| (key, value))
                .collect();
            calls.push(
                self.port
                    .call(method::AGENT_PREFERENCES_PATCH, json!({ "values": values })),
            );
        }
        if !local.is_empty() {
            let values: serde_json::Map<String, Value> = local
                .into_iter()
                .map(|(key, (value, _))| (key, value))
                .collect();
            calls.push(self.port.call(
                method::AGENT_PREFERENCES_PATCH,
                json!({ "values": values, "sync": false }),
            ));
        }
        calls
    }

    fn set_error(
        &mut self,
        kind: SettingsErrorKind,
        detail: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        self.last_error = Some(SettingsError {
            kind,
            detail: SharedString::from(detail.into()),
        });
        cx.notify();
    }
}

/// daemon wire 字符串 → 云同步目录键的 JSON 值。
fn daemon_string_to_json(spec_key: &str, wire: &str) -> Value {
    match setting_value_kind(spec_key) {
        fluxdown_protocol::SettingValueKind::Boolean => Value::Bool(matches!(wire, "true" | "1")),
        fluxdown_protocol::SettingValueKind::Integer => {
            wire.parse::<i64>().map_or(Value::Null, Value::from)
        }
        fluxdown_protocol::SettingValueKind::Float => wire
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map_or(Value::Null, Value::Number),
        fluxdown_protocol::SettingValueKind::String => Value::String(wire.to_owned()),
    }
}
