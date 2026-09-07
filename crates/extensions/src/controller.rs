//! 扩展能力的视图模型：插件 / 组件 / 手动路径的快照投影与宿主 RPC 调用。

use std::{collections::HashMap, sync::Arc};

use fluxdown_protocol::{
    AgentEvent, AgentSnapshot, ApplicationErrorCode, ComponentKind, ComponentStatusDto,
    DaemonConfigSnapshot, DaemonEvent, PluginDto, RpcErrorData, ServiceEvent, WsServerMsg, method,
};

use crate::{ExtensionsPort, PortFuture};

/// 受管组件的固定展示顺序（与 Flutter 组件页一致）。
pub const COMPONENT_KINDS: [ComponentKind; 2] = [ComponentKind::Ffmpeg, ComponentKind::Ytdlp];

/// 组件在 [`COMPONENT_KINDS`] 中的下标；view 端按此索引存放每个组件的 UI 状态。
pub fn component_slot(kind: ComponentKind) -> usize {
    match kind {
        ComponentKind::Ffmpeg => 0,
        ComponentKind::Ytdlp => 1,
    }
}

/// 引擎事件与插件 `missingComponents` 中使用的组件线名。
pub fn component_wire_name(kind: ComponentKind) -> &'static str {
    match kind {
        ComponentKind::Ffmpeg => "ffmpeg",
        ComponentKind::Ytdlp => "ytdlp",
    }
}

/// 手动指定可执行文件路径的 daemon 配置键（与引擎 `CONFIG_*_PATH` 一致）。
pub fn manual_path_key(kind: ComponentKind) -> &'static str {
    match kind {
        ComponentKind::Ffmpeg => "component.ffmpeg.path",
        ComponentKind::Ytdlp => "component.ytdlp.path",
    }
}

fn component_kind_from_wire(name: &str) -> Option<ComponentKind> {
    COMPONENT_KINDS
        .into_iter()
        .find(|kind| component_wire_name(*kind) == name)
}

/// 组件状态的类型无关只读视图（ffmpeg / yt-dlp 字段集合完全相同）。
#[derive(Clone, Copy)]
pub struct ComponentSummary<'a> {
    pub source: &'a str,
    pub path: &'a str,
    pub version: &'a str,
    pub managed_version: &'a str,
    pub system_path: &'a str,
    pub managed_supported: bool,
}

impl ComponentSummary<'_> {
    pub fn has_managed(&self) -> bool {
        !self.managed_version.is_empty()
    }
}

pub fn component_summary(status: &ComponentStatusDto) -> ComponentSummary<'_> {
    match status {
        ComponentStatusDto::Ffmpeg(status) => ComponentSummary {
            source: &status.source,
            path: &status.path,
            version: &status.version,
            managed_version: &status.managed_version,
            system_path: &status.system_path,
            managed_supported: status.managed_supported,
        },
        ComponentStatusDto::Ytdlp(status) => ComponentSummary {
            source: &status.source,
            path: &status.path,
            version: &status.version,
            managed_version: &status.managed_version,
            system_path: &status.system_path,
            managed_supported: status.managed_supported,
        },
    }
}

fn status_kind(status: &ComponentStatusDto) -> ComponentKind {
    match status {
        ComponentStatusDto::Ffmpeg(_) => ComponentKind::Ffmpeg,
        ComponentStatusDto::Ytdlp(_) => ComponentKind::Ytdlp,
    }
}

/// 引擎推送的组件安装进度 / 结果（RPC 响应之外的旁路信号）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtensionsSignal {
    ComponentProgress {
        kind: ComponentKind,
        downloaded_bytes: i64,
        total_bytes: i64,
    },
    ComponentResult {
        kind: ComponentKind,
        ok: bool,
        message: String,
    },
    /// 插件被熔断器自动禁用；携带插件展示名（列表已同步更新）。
    PluginAutoDisabled { name: String },
}

pub struct ExtensionsController {
    port: Arc<dyn ExtensionsPort>,
    plugins: Vec<PluginDto>,
    components: Vec<ComponentStatusDto>,
    config_revision: u64,
    manual_paths: [String; 2],
    stale: bool,
}

impl ExtensionsController {
    pub fn new(port: Arc<dyn ExtensionsPort>) -> Self {
        Self {
            port,
            plugins: Vec::new(),
            components: Vec::new(),
            config_revision: 0,
            manual_paths: [String::new(), String::new()],
            stale: true,
        }
    }

    pub fn replace_snapshot(&mut self, snapshot: &AgentSnapshot) {
        self.plugins.clone_from(&snapshot.daemon.plugins);
        self.components.clone_from(&snapshot.daemon.components);
        self.apply_config(&snapshot.daemon.config);
        self.stale = false;
    }

    /// 应用事件；组件安装进度 / 结果以信号形式回传给 view 处理。
    pub fn apply_event(&mut self, event: &ServiceEvent) -> Option<ExtensionsSignal> {
        let ServiceEvent::Agent(event) = event else {
            return None;
        };
        match event {
            AgentEvent::DaemonSnapshotReplaced(snapshot) => {
                self.plugins.clone_from(&snapshot.plugins);
                self.components.clone_from(&snapshot.components);
                self.apply_config(&snapshot.config);
                self.stale = false;
            }
            AgentEvent::DaemonConnectionChanged(connected) => self.stale = !connected,
            AgentEvent::Daemon(DaemonEvent::PluginsChanged(plugins)) => {
                self.plugins.clone_from(plugins);
            }
            AgentEvent::Daemon(DaemonEvent::ComponentsChanged(components)) => {
                self.components.clone_from(components);
            }
            AgentEvent::Daemon(DaemonEvent::ConfigChanged(config)) => self.apply_config(config),
            AgentEvent::Daemon(DaemonEvent::Engine(WsServerMsg::ComponentProgress {
                component,
                downloaded_bytes,
                total_bytes,
            })) => {
                return component_kind_from_wire(component).map(|kind| {
                    ExtensionsSignal::ComponentProgress {
                        kind,
                        downloaded_bytes: *downloaded_bytes,
                        total_bytes: *total_bytes,
                    }
                });
            }
            AgentEvent::Daemon(DaemonEvent::Engine(WsServerMsg::ComponentResult {
                component,
                ok,
                message,
            })) => {
                return component_kind_from_wire(component).map(|kind| {
                    ExtensionsSignal::ComponentResult {
                        kind,
                        ok: *ok,
                        message: message.clone(),
                    }
                });
            }
            // 与 agent 快照投影一致：熔断禁用直接改本地列表，不等下一次 PluginsChanged。
            AgentEvent::Daemon(DaemonEvent::Engine(WsServerMsg::PluginAutoDisabled {
                identity,
                reason,
            })) => {
                let plugin = self
                    .plugins
                    .iter_mut()
                    .find(|plugin| plugin.identity == *identity)?;
                plugin.enabled = false;
                plugin.disabled_reason.clone_from(reason);
                return Some(ExtensionsSignal::PluginAutoDisabled {
                    name: plugin.name.clone(),
                });
            }
            _ => {}
        }
        None
    }

    pub fn mark_stale(&mut self) {
        self.stale = true;
    }

    pub fn apply_config(&mut self, config: &DaemonConfigSnapshot) {
        self.config_revision = config.revision;
        for kind in COMPONENT_KINDS {
            let value = config
                .values
                .get(manual_path_key(kind))
                .map(|value| value.trim())
                .unwrap_or_default();
            let slot = &mut self.manual_paths[component_slot(kind)];
            if slot != value {
                slot.clear();
                slot.push_str(value);
            }
        }
    }

    /// 用单个组件的最新状态替换列表中的同类条目（`daemon.component.get` 回流）。
    pub fn apply_component_status(&mut self, status: ComponentStatusDto) {
        let kind = status_kind(&status);
        match self
            .components
            .iter_mut()
            .find(|existing| status_kind(existing) == kind)
        {
            Some(existing) => *existing = status,
            None => self.components.push(status),
        }
    }

    pub fn plugins(&self) -> &[PluginDto] {
        &self.plugins
    }

    pub fn plugin(&self, identity: &str) -> Option<&PluginDto> {
        self.plugins
            .iter()
            .find(|plugin| plugin.identity == identity)
    }

    pub fn components(&self) -> &[ComponentStatusDto] {
        &self.components
    }

    pub fn component(&self, kind: ComponentKind) -> Option<&ComponentStatusDto> {
        self.components
            .iter()
            .find(|status| status_kind(status) == kind)
    }

    pub fn manual_path(&self, kind: ComponentKind) -> &str {
        &self.manual_paths[component_slot(kind)]
    }

    pub fn is_stale(&self) -> bool {
        self.stale
    }

    pub fn port(&self) -> &Arc<dyn ExtensionsPort> {
        &self.port
    }

    fn call(
        &self,
        method: &'static str,
        params: serde_json::Value,
    ) -> PortFuture<serde_json::Value> {
        if self.stale {
            return unavailable();
        }
        self.port.call(method, params)
    }

    pub fn set_plugin_enabled(
        &self,
        identity: String,
        enabled: bool,
    ) -> PortFuture<serde_json::Value> {
        self.call(
            method::DAEMON_PLUGIN_SET_ENABLED,
            serde_json::json!({ "identity": identity, "enabled": enabled }),
        )
    }

    pub fn uninstall_plugin(&self, identity: String) -> PortFuture<serde_json::Value> {
        self.call(
            method::DAEMON_PLUGIN_UNINSTALL,
            serde_json::json!({ "identity": identity }),
        )
    }

    /// 从本机 zip 安装：agent 读文件、上传 daemon blob 后转调 `daemon.plugin.install`。
    pub fn install_plugin_file(&self, path: String) -> PortFuture<serde_json::Value> {
        self.call(
            method::AGENT_PLUGIN_INSTALL_FILE,
            serde_json::json!({ "path": path }),
        )
    }

    pub fn install_plugin_dev(&self, dir_path: String) -> PortFuture<serde_json::Value> {
        self.call(
            method::DAEMON_PLUGIN_INSTALL_DEV,
            serde_json::json!({ "dirPath": dir_path }),
        )
    }

    pub fn market_list(&self) -> PortFuture<serde_json::Value> {
        self.call(method::DAEMON_PLUGIN_MARKET_LIST, serde_json::json!({}))
    }

    pub fn market_install(&self, plugin_id: String) -> PortFuture<serde_json::Value> {
        self.call(
            method::DAEMON_PLUGIN_MARKET_INSTALL,
            serde_json::json!({ "pluginId": plugin_id }),
        )
    }

    pub fn component_status(&self, kind: ComponentKind) -> PortFuture<serde_json::Value> {
        self.call(
            method::DAEMON_COMPONENT_GET,
            serde_json::json!({ "component": kind }),
        )
    }

    pub fn component_versions(&self, kind: ComponentKind) -> PortFuture<serde_json::Value> {
        self.call(
            method::DAEMON_COMPONENT_LIST_VERSIONS,
            serde_json::json!({ "component": kind }),
        )
    }

    pub fn install_component(
        &self,
        kind: ComponentKind,
        version: Option<String>,
    ) -> PortFuture<serde_json::Value> {
        self.call(
            method::DAEMON_COMPONENT_INSTALL,
            serde_json::json!({ "component": kind, "version": version }),
        )
    }

    pub fn uninstall_component(&self, kind: ComponentKind) -> PortFuture<serde_json::Value> {
        self.call(
            method::DAEMON_COMPONENT_UNINSTALL,
            serde_json::json!({ "component": kind }),
        )
    }

    /// 写手动路径（空串 = 清除）；成功返回新的 `DaemonConfigSnapshot`。
    pub fn set_manual_path(
        &self,
        kind: ComponentKind,
        path: String,
    ) -> PortFuture<serde_json::Value> {
        self.call(
            method::DAEMON_CONFIG_PATCH,
            serde_json::json!({
                "expectedRevision": self.config_revision,
                "values": { manual_path_key(kind): path.trim() },
            }),
        )
    }
}

/// 插件设置保存请求；设置对话框与控制器共用同一份参数构造。
pub fn update_plugin_settings(
    port: &Arc<dyn ExtensionsPort>,
    identity: &str,
    entries: HashMap<String, String>,
) -> PortFuture<serde_json::Value> {
    port.call(
        method::DAEMON_PLUGIN_UPDATE_SETTINGS,
        serde_json::json!({ "identity": identity, "entries": entries }),
    )
}

fn unavailable() -> PortFuture<serde_json::Value> {
    Box::pin(async { Err(RpcErrorData::new(ApplicationErrorCode::Unavailable, true)) })
}
