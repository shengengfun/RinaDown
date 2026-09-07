//! agent 进程装配、daemon 单会话与 UI Gateway 生命周期。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fluxdown_protocol::{AgentSnapshot, ServiceEvent};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;

use crate::api_host::AgentApiHost;
use crate::daemon_client::{DaemonClient, DaemonClientConfig, DaemonClientEvent};
use crate::event_hub::AgentEventHub;
use crate::gateway::{GatewayService, load_or_create_bearer};
use crate::state::{AgentState, StateStore};
use crate::supervisor::DaemonSupervisor;

pub async fn run(
    cancel: CancellationToken,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let paths = AgentPaths::resolve()?;
    let store = Arc::new(StateStore::open(paths.agent_data_dir.clone()).await?);
    let mut state = store.load().await?;
    initialize_device_identity(&mut state, &store).await?;

    // 先绑定 UI Gateway：实际端口进入状态与快照，后续 Doctor/兼容 API 都据此探测。
    let override_bind = std::env::var("FLUXDOWN_AGENT_BIND").ok();
    let listener = TcpListener::bind(gateway_bind_address(
        state.gateway.lan_enabled,
        override_bind.as_deref(),
    )?)
    .await?;
    let bound = listener.local_addr()?;
    if state.gateway.port != bound.port() {
        state.gateway.port = bound.port();
        store.save(&state).await?;
    }
    tracing::info!(address = %bound, "fluxdown-agent gateway listening");

    let supervisor = Arc::new(DaemonSupervisor::new());
    let daemon_bearer = load_daemon_bearer(&paths, &supervisor).await?;
    let daemon_config = DaemonClientConfig {
        rpc_url: paths.daemon_rpc_url.clone(),
        bearer: daemon_bearer,
    };
    let (daemon, mut daemon_events) = DaemonClient::start(daemon_config.clone(), supervisor)?;
    let daemon = Arc::new(daemon);
    daemon
        .wait_ready(Duration::from_secs(30))
        .await
        .map_err(|error| {
            std::io::Error::other(format!("daemon startup failed: {:?}", error.code))
        })?;
    let initial_daemon =
        match tokio::time::timeout(Duration::from_secs(5), daemon_events.recv()).await {
            Ok(Some(DaemonClientEvent::Snapshot(snapshot))) => snapshot,
            Ok(Some(_)) | Ok(None) | Err(_) => {
                return Err(std::io::Error::other("daemon returned no initial snapshot").into());
            }
        };

    let initial = AgentSnapshot {
        daemon: initial_daemon,
        daemon_connected: true,
        session: state
            .credentials
            .as_ref()
            .and_then(|credentials| credentials.session.clone()),
        sync: state.sync.clone(),
        preferences: state.preferences.clone(),
        gateway: state.gateway.clone(),
        linked_devices: crate::link::public_devices(&state),
        remote_tasks: state.remote_tasks.clone(),
        ..AgentSnapshot::default()
    };
    let events = AgentEventHub::new(initial);
    let event_task = spawn_daemon_projection(daemon_events, events.clone(), cancel.clone());
    let effects_task = tokio::spawn(
        crate::background_effects::BackgroundEffects::new(events.clone()).run(cancel.clone()),
    );

    let shared_state = Arc::new(tokio::sync::Mutex::new(state));
    let analytics_task = tokio::spawn(
        crate::analytics::AnalyticsWorker::new(shared_state.clone(), store.clone())?
            .run(cancel.clone()),
    );
    crate::link::migrate_legacy_state(&daemon, &shared_state, &store, &events).await?;
    let (api_config, api_switches, api_token) = {
        let state = shared_state.lock().await;
        let switches = Arc::new(fluxdown_api::server::ApiRuntimeSwitches::new(
            state.gateway.takeover_enabled,
            state.gateway.jsonrpc_enabled,
            state.gateway.api_enabled,
            state.gateway.mcp_enabled,
            state.gateway.cors_enabled,
        ));
        let config = compatibility_api_config(&state).with_runtime_switches(switches.clone());
        let token = config.token.clone();
        (config, switches, token)
    };
    let cloud_client = crate::cloud::CloudClient::new(
        std::env::var("FLUXCLOUD_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8720".to_owned()),
        shared_state.clone(),
        store.clone(),
    )?;
    let auth = Arc::new(crate::cloud::CloudAuthService::new(
        cloud_client.clone(),
        events.clone(),
    ));
    let cloud_api = crate::cloud::CloudApi::new(cloud_client);
    let sync = Arc::new(crate::sync::SyncService::new(
        cloud_api.clone(),
        daemon.clone(),
        events.clone(),
        shared_state.clone(),
        store.clone(),
    ));
    let sync_task = tokio::spawn(sync.clone().run(cancel.clone()));
    let remote = Arc::new(crate::remote::RemoteTaskService::new(
        cloud_api.clone(),
        daemon.clone(),
        events.clone(),
        shared_state.clone(),
        store.clone(),
    ));
    let remote_task = tokio::spawn(remote.clone().run(cancel.clone()));
    let cdn_task = tokio::spawn(
        crate::cdn_worker::CdnWorker::new(
            cloud_api.clone(),
            daemon.as_ref().clone(),
            events.clone(),
        )
        .run(cancel.clone()),
    );
    let capture = Arc::new(crate::capture::CaptureService::new(
        daemon.clone(),
        events.clone(),
    ));
    let blobs = Arc::new(crate::capture::DaemonBlobClient::new(&daemon_config)?);
    let mut nmh_task = tokio::spawn(
        crate::nmh::NmhService::new(daemon.clone(), capture.clone()).run(cancel.clone()),
    );
    let diagnostics = Arc::new(crate::diagnostics::DiagnosticsService::new(
        daemon.clone(),
        daemon_config,
        events.clone(),
        shared_state.clone(),
        store.clone(),
        api_switches.clone(),
    ));
    let update = Arc::new(crate::update::UpdateService::new(env!(
        "CARGO_PKG_VERSION"
    ))?);
    let cloud = Arc::new(cloud_api);
    let gateway_service = Arc::new(GatewayService::new(
        daemon.clone(),
        events.clone(),
        auth,
        cloud,
        sync,
        remote,
        capture.clone(),
        blobs,
        diagnostics,
        update,
        shared_state,
        store.clone(),
        api_switches,
        api_token,
    ));
    let api_host = Arc::new(AgentApiHost::new(daemon, events, capture));
    let bearer = load_or_create_bearer(
        store.data_dir(),
        std::env::var_os("FLUXDOWN_AGENT_TOKEN_FILE")
            .as_deref()
            .map(Path::new),
    )
    .await?;
    let (result, nmh_completed): (Result<(), Box<dyn std::error::Error + Send + Sync>>, bool) = tokio::select! {
        gateway = crate::gateway::serve(
            listener,
            gateway_service,
            api_host,
            api_config,
            bearer,
            cancel.clone(),
        ) => (gateway.map_err(Into::into), false),
        nmh = &mut nmh_task => {
            let error = match nmh {
                Ok(Ok(())) => std::io::Error::other("NMH IPC service stopped unexpectedly"),
                Ok(Err(error)) => error,
                Err(error) => std::io::Error::other(error.to_string()),
            };
            (Err(Box::new(error)), true)
        }
    };
    cancel.cancel();
    let _ = event_task.await;
    let _ = cdn_task.await;
    let _ = sync_task.await;
    let _ = remote_task.await;
    let _ = effects_task.await;
    let _ = analytics_task.await;
    if !nmh_completed {
        let _ = nmh_task.await;
    }
    result
}

fn spawn_daemon_projection(
    mut daemon_events: tokio::sync::mpsc::Receiver<DaemonClientEvent>,
    events: AgentEventHub,
    cancel: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                event = daemon_events.recv() => {
                    let Some(event) = event else { break; };
                    match event {
                        DaemonClientEvent::Snapshot(snapshot) => {
                            events.replace_daemon_snapshot(snapshot);
                            events.publish(
                                fluxdown_protocol::AgentEvent::DaemonConnectionChanged(true),
                            );
                        }
                        DaemonClientEvent::Event(frame) => {
                            if let ServiceEvent::Daemon(event) = frame.event {
                                events.apply_daemon_event(event);
                            }
                        }
                        DaemonClientEvent::Stale => {
                            events.publish(fluxdown_protocol::AgentEvent::DaemonConnectionChanged(
                                false,
                            ));
                            tracing::warn!("daemon connection is stale; commands are read-only until snapshot replacement");
                        }
                        DaemonClientEvent::Fatal(error) => {
                            tracing::error!(code = ?error.code, "fatal daemon connection error");
                            cancel.cancel();
                            events.publish(fluxdown_protocol::AgentEvent::DaemonConnectionChanged(
                                false,
                            ));
                            break;
                        }
                    }
                }
            }
        }
    })
}

async fn initialize_device_identity(
    state: &mut AgentState,
    store: &StateStore,
) -> Result<(), crate::state::StateError> {
    let mut changed = false;
    let first_run = state.device_id.is_empty();
    if first_run {
        state.device_id = uuid::Uuid::new_v4().to_string();
        state.gateway.takeover_enabled = true;
        state.gateway.jsonrpc_enabled = true;
        changed = true;
    }
    let valid_name = {
        let length = state.device_name.trim().chars().count();
        (1..=64).contains(&length)
    };
    if !valid_name {
        state.device_name = std::env::var("HOSTNAME")
            .ok()
            .filter(|name| (1..=64).contains(&name.trim().chars().count()))
            .unwrap_or_else(|| "FluxDown".to_owned());
        changed = true;
    }
    if state.platform.is_empty() {
        state.platform = std::env::consts::OS.to_owned();
        changed = true;
    }
    if state.credentials.as_ref().is_some_and(|credentials| {
        credentials.access_token.is_empty()
            || credentials.refresh_token.is_empty()
            || credentials.session.is_none()
    }) {
        state.credentials = None;
        changed = true;
    }
    if changed {
        store.save(state).await?;
    }
    Ok(())
}

async fn load_daemon_bearer(
    paths: &AgentPaths,
    supervisor: &DaemonSupervisor,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    if let Ok(token) = tokio::fs::read_to_string(&paths.daemon_token_file).await
        && !token.trim().is_empty()
    {
        return Ok(token.trim().to_owned());
    }
    let address = daemon_socket_address(&paths.daemon_rpc_url)?;
    match TcpStream::connect(address).await {
        Ok(_) => {
            return Err("daemon is listening but its bearer token file is unavailable".into());
        }
        Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => {
            supervisor.ensure_running().await?;
        }
        Err(error) => return Err(error.into()),
    }
    for _ in 0..100 {
        if let Ok(token) = tokio::fs::read_to_string(&paths.daemon_token_file).await
            && !token.trim().is_empty()
        {
            return Ok(token.trim().to_owned());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err("daemon token file was not created after supervised launch".into())
}

fn compatibility_api_config(state: &AgentState) -> fluxdown_api::server::ApiServerConfig {
    let config = HashMap::from([
        ("local_server_enabled".to_owned(), "true".to_owned()),
        (
            "local_server_takeover_enabled".to_owned(),
            state.gateway.takeover_enabled.to_string(),
        ),
        (
            "local_server_jsonrpc_enabled".to_owned(),
            state.gateway.jsonrpc_enabled.to_string(),
        ),
        (
            "local_server_api_enabled".to_owned(),
            state.gateway.api_enabled.to_string(),
        ),
        (
            "local_server_mcp_enabled".to_owned(),
            state.gateway.mcp_enabled.to_string(),
        ),
        (
            "local_server_lan_enabled".to_owned(),
            state.gateway.lan_enabled.to_string(),
        ),
        (
            "local_server_cors_allow_all".to_owned(),
            state.gateway.cors_enabled.to_string(),
        ),
        (
            "local_server_token".to_owned(),
            state.gateway_user_token.clone(),
        ),
        (
            "local_server_port".to_owned(),
            state.gateway.port.to_string(),
        ),
    ]);
    fluxdown_api::server::ApiServerConfig::from_config_map(&config, env!("CARGO_PKG_VERSION"))
}

/// UI Gateway 监听地址：`FLUXDOWN_AGENT_BIND` 覆盖时必须是回环；否则按持久化的
/// `lan_enabled` 选择 `0.0.0.0` / `127.0.0.1`，端口固定 17800。
fn gateway_bind_address(
    lan_enabled: bool,
    override_bind: Option<&str>,
) -> Result<std::net::SocketAddr, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(value) = override_bind {
        let bind = value.parse::<std::net::SocketAddr>()?;
        if !bind.ip().is_loopback() {
            return Err("FLUXDOWN_AGENT_BIND must be loopback".into());
        }
        return Ok(bind);
    }
    let ip = if lan_enabled {
        std::net::Ipv4Addr::UNSPECIFIED
    } else {
        std::net::Ipv4Addr::LOCALHOST
    };
    Ok(std::net::SocketAddr::new(ip.into(), DEFAULT_GATEWAY_PORT))
}

const DEFAULT_GATEWAY_PORT: u16 = 17800;

struct AgentPaths {
    agent_data_dir: PathBuf,
    daemon_token_file: PathBuf,
    daemon_rpc_url: String,
}

impl AgentPaths {
    fn resolve() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let project = directories::ProjectDirs::from("dev", "zerx", "FluxDown")
            .ok_or("could not resolve application data directory")?;
        let root = std::env::var_os("FLUXDOWN_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| project.data_dir().to_owned());
        let agent_data_dir = std::env::var_os("FLUXDOWN_AGENT_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("agent"));
        // daemon 的 bearer 落在 engine 数据目录（`fluxdown_engine::data_dir::resolve_data_dir`），
        // 与 agent 自己的 ProjectDirs 根不同；未显式指定时必须按同一规则推导，否则永远等不到 token。
        let daemon_token_file = std::env::var_os("FLUXDOWN_DAEMON_TOKEN_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::var_os("FLUXDOWN_DATA_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(engine_data_dir)
                    .join("daemon.token")
            });
        let daemon_rpc_url = std::env::var("FLUXDOWN_DAEMON_URL")
            .unwrap_or_else(|_| "ws://127.0.0.1:17801/rpc".to_owned());
        Ok(Self {
            agent_data_dir,
            daemon_token_file,
            daemon_rpc_url,
        })
    }
}

/// 镜像 `fluxdown_engine::data_dir::resolve_data_dir_inner` 的默认目录（agent 不依赖 engine）。
fn engine_data_dir() -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let home = std::env::var_os("HOME").unwrap_or_else(|| ".".into());
                PathBuf::from(home).join(".local").join("share")
            });
        base.join("fluxdown")
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME").unwrap_or_else(|| ".".into());
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("fluxdown")
    }
    #[cfg(target_os = "windows")]
    {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."));
        if exe_dir.join("portable").exists() {
            return exe_dir.join("portable_data");
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(local).join("FluxDown");
        }
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return PathBuf::from(appdata).join("FluxDown");
        }
        exe_dir
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        PathBuf::from(".")
    }
}

fn daemon_socket_address(
    url: &str,
) -> Result<std::net::SocketAddr, Box<dyn std::error::Error + Send + Sync>> {
    let url = reqwest::Url::parse(url)?;
    let host = url.host_str().ok_or("daemon URL has no host")?;
    let ip = host.parse::<std::net::IpAddr>()?;
    if !ip.is_loopback() {
        return Err("daemon URL must be loopback".into());
    }
    Ok(std::net::SocketAddr::new(ip, url.port().unwrap_or(80)))
}

#[cfg(test)]
mod tests {
    use super::{compatibility_api_config, gateway_bind_address};
    use crate::state::AgentState;

    #[test]
    fn lan_flag_selects_interface_without_env_override() {
        let loopback = gateway_bind_address(false, None).expect("loopback bind");
        assert_eq!(loopback.to_string(), "127.0.0.1:17800");
        let lan = gateway_bind_address(true, None).expect("lan bind");
        assert_eq!(lan.to_string(), "0.0.0.0:17800");
    }

    #[test]
    fn env_override_wins_but_must_stay_loopback() {
        let bound = gateway_bind_address(true, Some("127.0.0.1:0")).expect("override bind");
        assert_eq!(bound.to_string(), "127.0.0.1:0");
        assert!(gateway_bind_address(false, Some("0.0.0.0:17800")).is_err());
        assert!(gateway_bind_address(false, Some("not-an-address")).is_err());
    }

    #[test]
    fn compatibility_config_mirrors_gateway_state() {
        let mut state = AgentState::default();
        state.gateway.lan_enabled = true;
        state.gateway.port = 17999;
        let config = compatibility_api_config(&state);
        assert!(config.lan_enabled);
        assert_eq!(config.port, 17999);
        assert_eq!(config.bind_addr().to_string(), "0.0.0.0:17999");
    }
}
