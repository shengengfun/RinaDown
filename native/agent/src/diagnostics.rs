//! agent Doctor 探测、就地修复命令与日志导出。
//!
//! 每个检查项是一次只读探测：NMH 中继/清单/浏览器注册、本进程 IPC 与网关监听、
//! 兼容 HTTP API、daemon RPC、URL scheme 与 `.torrent` 关联、日志目录可写。
//! 修复动作是显式的第二步（`repair`），探测本身从不改动系统。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fluxdown_protocol::{
    DiagnosticCheckDto, DiagnosticLevel, DiagnosticRepairParams, DiagnosticsReportDto,
    LogExportParams, LogExportResult, LogPathsDto, PlatformIntegrationDto,
};
use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::daemon_client::{DaemonClient, DaemonClientConfig};
use crate::event_hub::AgentEventHub;
use crate::state::{AgentState, StateStore};

/// 检查项 id；UI 据此映射 `doctorCheck{Camel}` 文案与修复按钮。
const CHECK_NMH_BINARY: &str = "nmh_binary";
const CHECK_NMH_MANIFEST: &str = "nmh_manifest";
const CHECK_NMH_BROWSER: &str = "nmh_browser";
const CHECK_APP_LISTENER: &str = "app_listener";
const CHECK_LOCAL_SERVER: &str = "local_server";
const CHECK_DAEMON: &str = "daemon";
const CHECK_URL_PROTOCOL: &str = "url_protocol";
const CHECK_TORRENT_ASSOCIATION: &str = "torrent_association";
const CHECK_LOG_DIR: &str = "log_dir";

/// 提示码；UI 映射 `doctorHint{Camel}`。
const HINT_REINSTALL_APP: &str = "reinstall_app";
const HINT_REREGISTER_NMH: &str = "reregister_nmh";
const HINT_RESTART_APP: &str = "restart_app";
const HINT_ENABLE_LOCAL_SERVER: &str = "enable_local_server";
const HINT_CHECK_FIREWALL: &str = "check_firewall";
const HINT_ENABLE_PROTOCOL: &str = "enable_protocol";
const HINT_CHECK_DISK: &str = "check_disk";

/// 修复动作；UI 映射 `doctorAction{Camel}`，并作为 `repair` 的 `action`。
pub const ACTION_REREGISTER: &str = "reregister";
pub const ACTION_ENABLE_SERVICE: &str = "enable_service";
pub const ACTION_REGISTER: &str = "register";
pub const ACTION_OPEN_LOG_DIR: &str = "open_log_dir";
pub const ACTION_REFRESH_TRACKERS: &str = "refreshTrackers";
pub const ACTION_REFRESH_ED2K_SERVERS: &str = "refreshEd2kServers";
/// `register` 动作的 `.torrent` 关联目标。
pub const TARGET_TORRENT: &str = "torrent";

const URL_SCHEMES: [&str; 3] = ["fluxdown", "magnet", "ed2k"];

/// 回环/IPC 探测超时：慢回答本身就是结论。
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const DAEMON_TIMEOUT: Duration = Duration::from_secs(5);
/// NMH 中继日志导出上限。
const NMH_LOG_EXPORT_BYTES: u64 = 4 * 1024 * 1024;

pub struct DiagnosticsService {
    daemon: Arc<DaemonClient>,
    daemon_config: DaemonClientConfig,
    events: AgentEventHub,
    state: Arc<Mutex<AgentState>>,
    store: Arc<StateStore>,
    api_switches: Arc<fluxdown_api::server::ApiRuntimeSwitches>,
}

impl DiagnosticsService {
    #[must_use]
    pub fn new(
        daemon: Arc<DaemonClient>,
        daemon_config: DaemonClientConfig,
        events: AgentEventHub,
        state: Arc<Mutex<AgentState>>,
        store: Arc<StateStore>,
        api_switches: Arc<fluxdown_api::server::ApiRuntimeSwitches>,
    ) -> Self {
        Self {
            daemon,
            daemon_config,
            events,
            state,
            store,
            api_switches,
        }
    }

    /// 运行全部探测并生成报告。
    pub async fn run(&self) -> Result<DiagnosticsReportDto, DiagnosticsError> {
        let gateway = self.state.lock().await.gateway.clone();
        let data_dir = self.store.data_dir().to_path_buf();
        let sync_probe = tokio::task::spawn_blocking(move || probe_sync(&data_dir))
            .await
            .map_err(join_error)?;
        let daemon = self.probe_daemon().await;
        let listener = probe_listener(gateway.port).await;
        let local_server = probe_local_server(&gateway).await;

        let mut checks = sync_probe.nmh;
        checks.push(listener);
        checks.push(local_server);
        checks.push(daemon.check);
        checks.extend(sync_probe.shell);
        checks.push(sync_probe.log_dir);

        let attention = checks
            .iter()
            .filter(|check| matches!(check.level, DiagnosticLevel::Warn | DiagnosticLevel::Error))
            .count();
        tracing::info!(checks = checks.len(), attention, "doctor run completed");

        Ok(DiagnosticsReportDto {
            generated_at_unix_ms: unix_ms(),
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
            platform: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
            agent_data_dir: self.store.data_dir().display().to_string(),
            daemon_connected: daemon.connected,
            checks,
        })
    }

    /// 执行修复动作；成功返回 `{ok:true}` 或 daemon RPC 的返回值。
    pub async fn repair(&self, params: &DiagnosticRepairParams) -> Result<Value, DiagnosticsError> {
        match params.action.as_str() {
            ACTION_REREGISTER => {
                spawn_blocking_io(crate::nmh::registry::register).await?;
                Ok(json!({ "ok": true }))
            }
            ACTION_ENABLE_SERVICE => {
                self.enable_service().await?;
                Ok(json!({ "ok": true }))
            }
            ACTION_REGISTER => {
                let target = params.target.trim().to_owned();
                if target == TARGET_TORRENT {
                    spawn_blocking_platform(|| crate::platform::set_file_association(true)).await?;
                } else if URL_SCHEMES.contains(&target.as_str()) {
                    spawn_blocking_platform(move || {
                        crate::platform::set_url_protocol(&target, true)
                    })
                    .await?;
                } else {
                    return Err(DiagnosticsError::InvalidAction(format!(
                        "register target: {target}"
                    )));
                }
                Ok(json!({ "ok": true }))
            }
            ACTION_OPEN_LOG_DIR => {
                let target = if params.target.trim().is_empty() {
                    self.store.data_dir().to_path_buf()
                } else {
                    PathBuf::from(params.target.trim())
                };
                spawn_blocking_platform(move || crate::platform::open_path(&target, false)).await?;
                Ok(json!({ "ok": true }))
            }
            ACTION_REFRESH_TRACKERS => {
                self.daemon_call(fluxdown_protocol::method::DAEMON_BT_TRACKER_SUBSCRIPTION_REFRESH)
                    .await
            }
            ACTION_REFRESH_ED2K_SERVERS => {
                self.daemon_call(fluxdown_protocol::method::DAEMON_ED2K_SERVER_SUBSCRIPTION_REFRESH)
                    .await
            }
            other => Err(DiagnosticsError::InvalidAction(other.to_owned())),
        }
    }

    /// agent 与 daemon 的日志目录；daemon 不可达时其目录为空串。
    pub async fn log_paths(&self) -> LogPathsDto {
        let describe = self.daemon_describe().await;
        crate::log_export::log_paths(self.store.data_dir(), daemon_log_dir(describe.as_ref()))
    }

    /// 打包 agent 摘要、Doctor 报告、daemon 快照与两侧日志到 `.zip`。
    pub async fn export_logs(
        &self,
        params: &LogExportParams,
    ) -> Result<LogExportResult, DiagnosticsError> {
        if params.target_path.trim().is_empty() {
            return Err(DiagnosticsError::InvalidAction(
                "exportLogs requires targetPath".to_owned(),
            ));
        }
        let target = crate::log_export::resolve_target(&params.target_path);
        let mut zip = crate::log_export::ZipWriter::new();

        let report = self.run().await?;
        zip.add("agent/diagnostics.json", &pretty_json(&report));
        zip.add(
            "agent/summary.json",
            &pretty_json(&self.agent_summary().await),
        );

        let describe = self.daemon_describe().await;
        match &describe {
            Some(describe) => zip.add("daemon/describe.json", &pretty_json(describe)),
            None => zip.add(
                "daemon/describe.json",
                b"{\"error\":\"daemon unreachable\"}\n",
            ),
        }
        match self.fetch_daemon_export().await {
            Ok(bytes) => zip.add("daemon/snapshot.json", &bytes),
            Err(error) => {
                tracing::warn!(error = %error, "daemon log export unavailable");
                zip.add(
                    "daemon/snapshot.json",
                    format!("{{\"error\":{}}}\n", Value::from(error.to_string())).as_bytes(),
                );
            }
        }

        for (name, bytes) in crate::log_export::collect_log_files(self.store.data_dir()).await {
            zip.add(&format!("agent/logs/{name}"), &bytes);
        }
        if let Some(dir) = daemon_log_dir(describe.as_ref()) {
            for (name, bytes) in crate::log_export::collect_log_files(Path::new(dir)).await {
                zip.add(&format!("daemon/logs/{name}"), &bytes);
            }
        }
        if let Some(path) = crate::log_export::nmh_relay_log_path()
            && let Some(bytes) = crate::log_export::read_tail(&path, NMH_LOG_EXPORT_BYTES).await
        {
            zip.add("nmh/fluxdown_nmh.log", &bytes);
        }

        let bytes = zip.finish();
        Ok(crate::log_export::write_atomic(&target, &bytes).await?)
    }

    async fn daemon_call(&self, method: &str) -> Result<Value, DiagnosticsError> {
        self.daemon
            .call::<Value, Value>(method, None)
            .await
            .map_err(DiagnosticsError::Daemon)
    }

    async fn daemon_describe(&self) -> Option<Value> {
        tokio::time::timeout(
            DAEMON_TIMEOUT,
            self.daemon
                .call::<Value, Value>(fluxdown_protocol::method::DAEMON_DIAGNOSTICS_DESCRIBE, None),
        )
        .await
        .ok()
        .and_then(Result::ok)
    }

    async fn probe_daemon(&self) -> DaemonProbe {
        let endpoint = &self.daemon_config.rpc_url;
        let ping = tokio::time::timeout(
            DAEMON_TIMEOUT,
            self.daemon
                .call::<Value, Value>(fluxdown_protocol::method::SYSTEM_PING, None),
        )
        .await;
        match ping {
            Ok(Ok(_)) => {
                let mut detail = format!("{endpoint} → pong");
                if let Some(describe) = self.daemon_describe().await {
                    let field = |key: &str| describe.get(key).and_then(Value::as_u64).unwrap_or(0);
                    let version = describe
                        .pointer("/service/version")
                        .and_then(Value::as_str)
                        .unwrap_or("?");
                    detail.push_str(&format!(
                        " (v{version}; tasks={}, queues={}, configRevision={})",
                        field("tasks"),
                        field("queues"),
                        field("configRevision")
                    ));
                }
                DaemonProbe {
                    connected: true,
                    check: check(CHECK_DAEMON, "", DiagnosticLevel::Ok, detail, "", None),
                }
            }
            Ok(Err(error)) => DaemonProbe {
                connected: false,
                check: check(
                    CHECK_DAEMON,
                    "",
                    DiagnosticLevel::Error,
                    format!("{endpoint} — {:?}", error.code),
                    HINT_RESTART_APP,
                    None,
                ),
            },
            Err(_) => DaemonProbe {
                connected: false,
                check: check(
                    CHECK_DAEMON,
                    "",
                    DiagnosticLevel::Error,
                    format!("{endpoint} — ping timed out"),
                    HINT_RESTART_APP,
                    None,
                ),
            },
        }
    }

    /// 与 `agent.gateway.patch` 同一条路径：持久化 → 运行时开关 → 广播 `GatewayChanged`。
    async fn enable_service(&self) -> Result<(), DiagnosticsError> {
        let mut state = self.state.lock().await;
        state.gateway.api_enabled = true;
        let gateway = state.gateway.clone();
        self.store.save(&state).await?;
        drop(state);
        self.api_switches.update(
            gateway.takeover_enabled,
            gateway.jsonrpc_enabled,
            gateway.api_enabled,
            gateway.mcp_enabled,
            gateway.cors_enabled,
        );
        self.events
            .publish(fluxdown_protocol::AgentEvent::GatewayChanged(gateway));
        Ok(())
    }

    /// 不含 token、凭据与设备私钥的 agent 状态摘要。
    async fn agent_summary(&self) -> Value {
        let state = self.state.lock().await;
        json!({
            "appVersion": env!("CARGO_PKG_VERSION"),
            "platform": format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
            "dataDir": self.store.data_dir().display().to_string(),
            "daemonRpcUrl": self.daemon_config.rpc_url,
            "deviceId": state.device_id,
            "deviceName": state.device_name,
            "devicePlatform": state.platform,
            "signedIn": state.credentials.is_some(),
            "gateway": state.gateway,
            "sync": state.sync,
            "preferences": state.preferences,
            "linkedDevices": state.linked_devices.len(),
            "remoteTasks": state.remote_tasks.len(),
            "ipcEndpoint": crate::nmh::ipc_endpoint(),
        })
    }

    /// `daemon.diagnostics.prepareLogExport` → `GET /exports/{id}`（一次性 blob）。
    async fn fetch_daemon_export(&self) -> Result<Vec<u8>, DiagnosticsError> {
        let prepared = tokio::time::timeout(
            DAEMON_TIMEOUT,
            self.daemon.call::<Value, Value>(
                fluxdown_protocol::method::DAEMON_DIAGNOSTICS_PREPARE_LOG_EXPORT,
                None,
            ),
        )
        .await
        .map_err(|_| DiagnosticsError::Export("prepareLogExport timed out".to_owned()))?
        .map_err(DiagnosticsError::Daemon)?;
        let export_id = prepared
            .get("exportId")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| {
                DiagnosticsError::Export("prepareLogExport returned no exportId".to_owned())
            })?;
        let url = daemon_export_url(&self.daemon_config.rpc_url, export_id)?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(DAEMON_TIMEOUT)
            .build()
            .map_err(|error| DiagnosticsError::Export(error.to_string()))?;
        let response = client
            .get(url)
            .bearer_auth(&self.daemon_config.bearer)
            .send()
            .await
            .map_err(|error| DiagnosticsError::Export(error.to_string()))?;
        if !response.status().is_success() {
            return Err(DiagnosticsError::Export(format!(
                "daemon export returned {}",
                response.status()
            )));
        }
        response
            .bytes()
            .await
            .map(|bytes| bytes.to_vec())
            .map_err(|error| DiagnosticsError::Export(error.to_string()))
    }
}

struct DaemonProbe {
    connected: bool,
    check: DiagnosticCheckDto,
}

/// 无需 `await` 的探测（文件、注册表、平台集成状态），在一次 `spawn_blocking` 内完成。
struct SyncProbe {
    nmh: Vec<DiagnosticCheckDto>,
    shell: Vec<DiagnosticCheckDto>,
    log_dir: DiagnosticCheckDto,
}

fn probe_sync(data_dir: &Path) -> SyncProbe {
    SyncProbe {
        nmh: nmh_checks(&crate::nmh::registry::diagnose()),
        shell: shell_checks(&crate::platform::integration_status()),
        log_dir: probe_log_dir(data_dir),
    }
}

fn check(
    id: &str,
    target: &str,
    level: DiagnosticLevel,
    detail: String,
    hint: &str,
    repair: Option<(&str, &str)>,
) -> DiagnosticCheckDto {
    DiagnosticCheckDto {
        id: id.to_owned(),
        target: target.to_owned(),
        level,
        detail,
        hint: hint.to_owned(),
        repair: repair.map(|(action, target)| DiagnosticRepairParams {
            action: action.to_owned(),
            target: target.to_owned(),
        }),
    }
}

/// `nmh_binary`、`nmh_manifest`、每个浏览器一条 `nmh_browser`。
fn nmh_checks(diagnosis: &crate::nmh::registry::NmhDiagnosis) -> Vec<DiagnosticCheckDto> {
    let mut checks = Vec::with_capacity(2 + diagnosis.targets.len());
    if diagnosis.exe_path.is_empty() {
        checks.push(check(
            CHECK_NMH_BINARY,
            "",
            DiagnosticLevel::Error,
            diagnosis.exe_error.clone(),
            HINT_REINSTALL_APP,
            None,
        ));
    } else {
        checks.push(check(
            CHECK_NMH_BINARY,
            "",
            DiagnosticLevel::Ok,
            diagnosis.exe_path.clone(),
            "",
            None,
        ));
    }
    checks.push(manifest_check(
        &diagnosis.chromium_manifest,
        &diagnosis.firefox_manifest,
    ));
    for target in &diagnosis.targets {
        let (level, detail, hint, repair) = if !target.installed {
            (
                DiagnosticLevel::Info,
                format!("{} (browser not installed)", target.location),
                "",
                None,
            )
        } else if target.ok {
            (DiagnosticLevel::Ok, target.location.clone(), "", None)
        } else {
            (
                DiagnosticLevel::Error,
                format!("{} — {}", target.location, target.issue),
                HINT_REREGISTER_NMH,
                Some((ACTION_REREGISTER, "")),
            )
        };
        checks.push(check(
            CHECK_NMH_BROWSER,
            &target.label,
            level,
            detail,
            hint,
            repair,
        ));
    }
    checks
}

/// Chromium 与 Firefox 两份清单都要存在；缺一份是安装未完成或清理工具误删的典型症状。
fn manifest_check(chromium: &str, firefox: &str) -> DiagnosticCheckDto {
    let missing: Vec<&str> = [("chromium", chromium), ("firefox", firefox)]
        .into_iter()
        .filter(|(_, path)| path.is_empty() || !Path::new(path).exists())
        .map(|(label, _)| label)
        .collect();
    let detail = format!("chromium: {chromium}\nfirefox: {firefox}");
    if missing.is_empty() {
        check(
            CHECK_NMH_MANIFEST,
            "",
            DiagnosticLevel::Ok,
            detail,
            "",
            None,
        )
    } else {
        check(
            CHECK_NMH_MANIFEST,
            "",
            DiagnosticLevel::Error,
            format!("{detail}\nmissing: {}", missing.join(", ")),
            HINT_REREGISTER_NMH,
            Some((ACTION_REREGISTER, "")),
        )
    }
}

/// `url_protocol`×3 与 `torrent_association`。
///
/// `fluxdown://` 是深链入口，缺失报 warn；`magnet`/`ed2k`/`.torrent` 是可选项，只报 info，
/// 否则用户会习惯性忽略整页。
fn shell_checks(integration: &PlatformIntegrationDto) -> Vec<DiagnosticCheckDto> {
    let mut checks = Vec::with_capacity(URL_SCHEMES.len() + 1);
    for scheme in URL_SCHEMES {
        let registered = integration
            .url_protocols
            .get(scheme)
            .copied()
            .unwrap_or(false);
        let (level, detail, hint, repair) = if !integration.url_protocol_supported {
            (
                DiagnosticLevel::Info,
                "not supported on this platform".to_owned(),
                "",
                None,
            )
        } else if registered {
            (
                DiagnosticLevel::Ok,
                "registered for this build".to_owned(),
                "",
                None,
            )
        } else if scheme == "fluxdown" {
            (
                DiagnosticLevel::Warn,
                "not registered".to_owned(),
                HINT_ENABLE_PROTOCOL,
                Some((ACTION_REGISTER, scheme)),
            )
        } else {
            (
                DiagnosticLevel::Info,
                "not registered".to_owned(),
                HINT_ENABLE_PROTOCOL,
                Some((ACTION_REGISTER, scheme)),
            )
        };
        checks.push(check(
            CHECK_URL_PROTOCOL,
            scheme,
            level,
            detail,
            hint,
            repair,
        ));
    }
    let torrent = if !integration.file_association_supported {
        check(
            CHECK_TORRENT_ASSOCIATION,
            "",
            DiagnosticLevel::Info,
            "not supported on this platform".to_owned(),
            "",
            None,
        )
    } else if integration.torrent_associated {
        check(
            CHECK_TORRENT_ASSOCIATION,
            "",
            DiagnosticLevel::Ok,
            ".torrent opens with FluxDown".to_owned(),
            "",
            None,
        )
    } else {
        check(
            CHECK_TORRENT_ASSOCIATION,
            "",
            DiagnosticLevel::Info,
            ".torrent not associated".to_owned(),
            HINT_ENABLE_PROTOCOL,
            Some((ACTION_REGISTER, TARGET_TORRENT)),
        )
    };
    checks.push(torrent);
    checks
}

/// 磁盘满或无权限的日志目录会吞掉 bug 报告需要的证据，所以真的写一次。
fn probe_log_dir(dir: &Path) -> DiagnosticCheckDto {
    let dir_str = dir.display().to_string();
    let (files, total) = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().ends_with(".log"))
                .filter_map(|entry| entry.metadata().ok())
                .fold((0_usize, 0_u64), |(count, bytes), metadata| {
                    (count + 1, bytes + metadata.len())
                })
        })
        .unwrap_or((0, 0));
    let summary = format!(
        "{dir_str} ({files} log files, {:.1} MB)",
        total as f64 / (1024.0 * 1024.0)
    );
    let repair = Some((ACTION_OPEN_LOG_DIR, dir_str.as_str()));
    if !dir.is_dir() {
        return check(
            CHECK_LOG_DIR,
            "",
            DiagnosticLevel::Error,
            format!("{summary} — directory missing"),
            HINT_CHECK_DISK,
            repair,
        );
    }
    let probe_file = dir.join(".doctor_write_probe");
    match std::fs::write(&probe_file, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe_file);
            check(CHECK_LOG_DIR, "", DiagnosticLevel::Ok, summary, "", repair)
        }
        Err(error) => check(
            CHECK_LOG_DIR,
            "",
            DiagnosticLevel::Error,
            format!("{summary} — not writable: {error}"),
            HINT_CHECK_DISK,
            repair,
        ),
    }
}

/// 浏览器中继拨号的 IPC 端点是否应答 `pong`，以及网关 TCP 端口是否可连。
async fn probe_listener(port: u16) -> DiagnosticCheckDto {
    let ipc_endpoint = crate::nmh::ipc_endpoint();
    let ipc = crate::nmh::probe_ipc(PROBE_TIMEOUT).await;
    let tcp_address = format!("127.0.0.1:{port}");
    let tcp =
        match tokio::time::timeout(PROBE_TIMEOUT, tokio::net::TcpStream::connect(&tcp_address))
            .await
        {
            Ok(result) => result,
            Err(_) => Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "connect timed out",
            )),
        };
    let mut detail = match &ipc {
        Ok(reply) => format!("ipc {ipc_endpoint} → {reply}"),
        Err(error) => format!("ipc {ipc_endpoint} — {error}"),
    };
    detail.push('\n');
    match &tcp {
        Ok(_) => detail.push_str(&format!("gateway {tcp_address} → connected")),
        Err(error) => detail.push_str(&format!("gateway {tcp_address} — {error}")),
    }
    if ipc.is_ok() && tcp.is_ok() {
        check(
            CHECK_APP_LISTENER,
            "",
            DiagnosticLevel::Ok,
            detail,
            "",
            None,
        )
    } else {
        check(
            CHECK_APP_LISTENER,
            "",
            DiagnosticLevel::Error,
            detail,
            HINT_RESTART_APP,
            None,
        )
    }
}

/// 兼容 HTTP API：任一功能开关打开才算启用，然后探活 `/ping`。
async fn probe_local_server(gateway: &fluxdown_protocol::GatewayStatusDto) -> DiagnosticCheckDto {
    let enabled = gateway.api_enabled
        || gateway.takeover_enabled
        || gateway.jsonrpc_enabled
        || gateway.mcp_enabled;
    if !enabled {
        return check(
            CHECK_LOCAL_SERVER,
            "",
            DiagnosticLevel::Info,
            "disabled in settings".to_owned(),
            HINT_ENABLE_LOCAL_SERVER,
            Some((ACTION_ENABLE_SERVICE, "")),
        );
    }
    let url = format!(
        "http://127.0.0.1:{}{}",
        gateway.port,
        fluxdown_api::routes::PING
    );
    // `.no_proxy()`：系统代理会吞掉回环探测，把健康的服务误报为不可达。
    let client = match reqwest::Client::builder()
        .no_proxy()
        .timeout(PROBE_TIMEOUT)
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return check(
                CHECK_LOCAL_SERVER,
                "",
                DiagnosticLevel::Error,
                format!("{url} — client build failed: {error}"),
                "",
                None,
            );
        }
    };
    let flags = format!(
        "api={}, takeover={}, jsonrpc={}, mcp={}, lan={}",
        gateway.api_enabled,
        gateway.takeover_enabled,
        gateway.jsonrpc_enabled,
        gateway.mcp_enabled,
        gateway.lan_enabled
    );
    match client.get(&url).send().await {
        Ok(response) if response.status().is_success() => check(
            CHECK_LOCAL_SERVER,
            "",
            DiagnosticLevel::Ok,
            format!("{url} → {} ({flags})", response.status()),
            "",
            None,
        ),
        Ok(response) => check(
            CHECK_LOCAL_SERVER,
            "",
            DiagnosticLevel::Error,
            format!("{url} → {} ({flags})", response.status()),
            HINT_CHECK_FIREWALL,
            None,
        ),
        Err(error) => check(
            CHECK_LOCAL_SERVER,
            "",
            DiagnosticLevel::Error,
            format!("{url} — {error} ({flags})"),
            HINT_CHECK_FIREWALL,
            None,
        ),
    }
}

fn daemon_log_dir(describe: Option<&Value>) -> Option<&str> {
    describe?
        .get("logDir")
        .and_then(Value::as_str)
        .filter(|dir| !dir.is_empty())
}

/// `ws://host:port/rpc` → `http://host:port/exports/{id}`。
fn daemon_export_url(rpc_url: &str, export_id: &str) -> Result<reqwest::Url, DiagnosticsError> {
    let mut url = reqwest::Url::parse(rpc_url)
        .map_err(|error| DiagnosticsError::Export(format!("daemon URL invalid: {error}")))?;
    let scheme = match url.scheme() {
        "wss" | "https" => "https",
        _ => "http",
    };
    url.set_scheme(scheme)
        .map_err(|()| DiagnosticsError::Export("daemon URL scheme not switchable".to_owned()))?;
    url.set_path(&format!("/exports/{export_id}"));
    url.set_query(None);
    Ok(url)
}

fn pretty_json<T: serde::Serialize>(value: &T) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(value).unwrap_or_default();
    bytes.push(b'\n');
    bytes
}

fn unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

async fn spawn_blocking_io<F>(task: F) -> Result<(), DiagnosticsError>
where
    F: FnOnce() -> Result<(), std::io::Error> + Send + 'static,
{
    tokio::task::spawn_blocking(task)
        .await
        .map_err(join_error)?
        .map_err(DiagnosticsError::Io)
}

async fn spawn_blocking_platform<F>(task: F) -> Result<(), DiagnosticsError>
where
    F: FnOnce() -> Result<(), crate::platform::PlatformError> + Send + 'static,
{
    tokio::task::spawn_blocking(task)
        .await
        .map_err(join_error)?
        .map_err(DiagnosticsError::Platform)
}

fn join_error(error: tokio::task::JoinError) -> DiagnosticsError {
    DiagnosticsError::Io(std::io::Error::other(format!(
        "blocking task failed: {error}"
    )))
}

#[derive(Debug, thiserror::Error)]
pub enum DiagnosticsError {
    #[error("invalid diagnostic repair: {0}")]
    InvalidAction(String),
    #[error("daemon diagnostic RPC failed: {0:?}")]
    Daemon(fluxdown_protocol::RpcErrorData),
    #[error(transparent)]
    State(#[from] crate::state::StateError),
    #[error("diagnostic I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Platform(#[from] crate::platform::PlatformError),
    #[error("log export failed: {0}")]
    Export(String),
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use fluxdown_protocol::{DiagnosticLevel, PlatformIntegrationDto};

    use super::{
        ACTION_ENABLE_SERVICE, ACTION_OPEN_LOG_DIR, ACTION_REGISTER, ACTION_REREGISTER,
        HINT_CHECK_DISK, HINT_ENABLE_LOCAL_SERVER, HINT_ENABLE_PROTOCOL, HINT_REINSTALL_APP,
        HINT_REREGISTER_NMH, TARGET_TORRENT, daemon_export_url, daemon_log_dir, manifest_check,
        nmh_checks, probe_local_server, probe_log_dir, shell_checks,
    };
    use crate::nmh::registry::{NmhDiagnosis, NmhTarget};

    fn target(label: &str, installed: bool, ok: bool) -> NmhTarget {
        NmhTarget {
            label: label.to_owned(),
            location: format!("/manifests/{label}.json"),
            installed,
            ok,
            issue: if ok {
                String::new()
            } else {
                "manifest file missing".to_owned()
            },
        }
    }

    #[test]
    fn nmh_checks_map_levels_hints_and_repairs() {
        let diagnosis = NmhDiagnosis {
            exe_path: "/app/fluxdown_nmh".to_owned(),
            exe_error: String::new(),
            chromium_manifest: "/missing/chromium.json".to_owned(),
            firefox_manifest: "/missing/firefox.json".to_owned(),
            targets: vec![
                target("Chrome", true, true),
                target("Edge", true, false),
                target("Firefox", false, false),
            ],
        };
        let checks = nmh_checks(&diagnosis);
        assert_eq!(checks.len(), 5);
        assert_eq!(checks[0].id, "nmh_binary");
        assert_eq!(checks[0].level, DiagnosticLevel::Ok);
        assert_eq!(checks[1].id, "nmh_manifest");
        assert_eq!(checks[1].level, DiagnosticLevel::Error);
        assert_eq!(checks[1].hint, HINT_REREGISTER_NMH);
        assert!(checks[1].detail.contains("missing: chromium, firefox"));
        assert_eq!(checks[2].target, "Chrome");
        assert_eq!(checks[2].level, DiagnosticLevel::Ok);
        assert!(checks[2].repair.is_none());
        assert_eq!(checks[3].target, "Edge");
        assert_eq!(checks[3].level, DiagnosticLevel::Error);
        assert_eq!(checks[3].hint, HINT_REREGISTER_NMH);
        assert_eq!(
            checks[3].repair.as_ref().map(|r| r.action.as_str()),
            Some(ACTION_REREGISTER)
        );
        assert_eq!(checks[4].target, "Firefox");
        assert_eq!(checks[4].level, DiagnosticLevel::Info);
        assert!(checks[4].detail.contains("browser not installed"));
        assert!(checks[4].hint.is_empty());
    }

    #[test]
    fn missing_relay_binary_is_an_error_with_reinstall_hint() {
        let diagnosis = NmhDiagnosis {
            exe_error: "fluxdown_nmh not found".to_owned(),
            ..NmhDiagnosis::default()
        };
        let checks = nmh_checks(&diagnosis);
        assert_eq!(checks.len(), 2);
        assert_eq!(checks[0].level, DiagnosticLevel::Error);
        assert_eq!(checks[0].hint, HINT_REINSTALL_APP);
        assert_eq!(checks[0].detail, "fluxdown_nmh not found");
        let ok = manifest_check("", "");
        assert_eq!(ok.level, DiagnosticLevel::Error);
    }

    #[test]
    fn shell_checks_follow_platform_integration() {
        let mut integration = PlatformIntegrationDto {
            url_protocol_supported: true,
            file_association_supported: true,
            url_protocols: BTreeMap::from([
                ("fluxdown".to_owned(), false),
                ("magnet".to_owned(), true),
            ]),
            ..PlatformIntegrationDto::default()
        };
        let checks = shell_checks(&integration);
        assert_eq!(checks.len(), 4);
        let fluxdown = &checks[0];
        assert_eq!(fluxdown.id, "url_protocol");
        assert_eq!(fluxdown.target, "fluxdown");
        assert_eq!(fluxdown.level, DiagnosticLevel::Warn);
        assert_eq!(fluxdown.hint, HINT_ENABLE_PROTOCOL);
        assert_eq!(
            fluxdown
                .repair
                .as_ref()
                .map(|r| (r.action.as_str(), r.target.as_str())),
            Some((ACTION_REGISTER, "fluxdown"))
        );
        assert_eq!(checks[1].target, "magnet");
        assert_eq!(checks[1].level, DiagnosticLevel::Ok);
        assert!(checks[1].repair.is_none());
        assert_eq!(checks[2].target, "ed2k");
        assert_eq!(checks[2].level, DiagnosticLevel::Info);
        let torrent = &checks[3];
        assert_eq!(torrent.id, "torrent_association");
        assert_eq!(torrent.level, DiagnosticLevel::Info);
        assert_eq!(
            torrent.repair.as_ref().map(|r| r.target.as_str()),
            Some(TARGET_TORRENT)
        );

        integration.url_protocol_supported = false;
        integration.file_association_supported = false;
        let checks = shell_checks(&integration);
        assert!(checks.iter().all(|c| c.level == DiagnosticLevel::Info));
        assert!(
            checks
                .iter()
                .all(|c| c.repair.is_none() && c.hint.is_empty())
        );
    }

    #[test]
    fn log_dir_probe_reports_writability() {
        let dir = std::env::temp_dir().join(format!(
            "fluxdown_doctor_{}_{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let missing = probe_log_dir(&dir);
        assert_eq!(missing.level, DiagnosticLevel::Error);
        assert_eq!(missing.hint, HINT_CHECK_DISK);
        std::fs::create_dir_all(&dir).ok();
        std::fs::write(dir.join("agent.log"), b"line\n").ok();
        let writable = probe_log_dir(&dir);
        assert_eq!(writable.level, DiagnosticLevel::Ok);
        assert!(writable.detail.contains("1 log files"));
        assert_eq!(
            writable.repair.as_ref().map(|r| r.action.as_str()),
            Some(ACTION_OPEN_LOG_DIR)
        );
        assert!(!dir.join(".doctor_write_probe").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn disabled_local_server_offers_enable_repair() {
        let gateway = fluxdown_protocol::GatewayStatusDto::default();
        let check = probe_local_server(&gateway).await;
        assert_eq!(check.level, DiagnosticLevel::Info);
        assert_eq!(check.hint, HINT_ENABLE_LOCAL_SERVER);
        assert_eq!(
            check.repair.as_ref().map(|r| r.action.as_str()),
            Some(ACTION_ENABLE_SERVICE)
        );
    }

    #[test]
    fn daemon_export_url_and_log_dir_are_derived() {
        let url = daemon_export_url("ws://127.0.0.1:17801/rpc?x=1", "logs:abc").ok();
        assert_eq!(
            url.map(|u| u.to_string()),
            Some("http://127.0.0.1:17801/exports/logs:abc".to_owned())
        );
        let describe = serde_json::json!({ "logDir": "/data/logs" });
        assert_eq!(daemon_log_dir(Some(&describe)), Some("/data/logs"));
        assert_eq!(daemon_log_dir(Some(&serde_json::json!({}))), None);
        assert_eq!(daemon_log_dir(None), None);
    }
}
