//! daemon 进程装配、引擎启动顺序与控制面生命周期。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use fluxdown_engine::db::{Db, EngineWriteGuard};
use fluxdown_engine::download_manager;
use fluxdown_engine::events::EventSink;
use fluxdown_engine::proxy_config::ProxyConfig;
use fluxdown_engine::selection::HostSelection;
use fluxdown_engine::{Engine, EngineConfig};
use fluxdown_protocol::{DaemonConfigSnapshot, DaemonSnapshot};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::actor::EngineReceivers;
#[cfg(feature = "plugins")]
use crate::actor::PluginEvent;
use crate::blob_store::BlobStore;
use crate::config::{DaemonConfig, bt_config_from_map};
use crate::event_hub::{DaemonEngineEventSink, DaemonEventHub};
use crate::http::{load_or_create_bearer, serve};
use crate::selection::DaemonSelection;
use crate::service::DaemonService;

/// 运行 daemon 直到收到取消信号。
pub async fn run(
    cancel: CancellationToken,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let process_config = DaemonConfig::from_env()?;
    let data_dir =
        fluxdown_engine::data_dir::resolve_data_dir(process_config.data_dir_override.as_deref())?;
    fluxdown_engine::logger::init_with_dir(&data_dir)?;
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    let (boot_db, write_guard) = open_database(&process_config, &data_dir).await?;
    let default_save_dir = fluxdown_engine::user_dirs::download_dir_or_cwd();
    boot_db.init_default_config(&default_save_dir).await?;
    let all_config = boot_db.get_all_config().await?;

    let snapshot = initial_snapshot(&boot_db, &all_config).await?;
    let events = DaemonEventHub::new(snapshot, 1024);
    let selections = DaemonSelection::new(events.clone());
    let sink: Arc<dyn EventSink> = Arc::new(DaemonEngineEventSink(events.clone()));
    let selector: Arc<dyn HostSelection> = Arc::new(selections.clone());

    let save_dir = configured_save_dir(&all_config, default_save_dir);
    let mut engine = Engine::from_db(
        EngineConfig {
            max_concurrent: usize_config(&all_config, "max_concurrent_tasks", 5),
            speed_limit_bps: u64_config(&all_config, "speed_limit_bytes", 0),
            upload_limit_bps: u64_config(&all_config, "upload_limit_bytes", 0),
            default_save_dir: save_dir.clone(),
            app_data_dir: data_dir.to_string_lossy().into_owned(),
            bt_config: bt_config_from_map(&all_config),
            proxy_config: ProxyConfig::from_config_map(&all_config),
            user_agent: all_config
                .get("global_user_agent")
                .cloned()
                .unwrap_or_default(),
            data_dir_override: Some(data_dir.clone()),
            database_url: process_config.database_url.clone(),
        },
        boot_db,
        write_guard,
        sink.clone(),
        selector,
    )
    .await?;
    apply_manager_settings(&mut engine, &all_config);

    if let Some(progress) = engine.manager.take_progress_rx() {
        tokio::spawn(download_manager::progress_reporter(
            progress,
            engine.db.clone(),
            sink,
        ));
    }
    let service_db = engine.db.clone();
    #[cfg(any(feature = "plugins", feature = "components"))]
    let service_data_dir = data_dir.clone();
    #[cfg(feature = "plugins")]
    let service_plugin_manager = engine.manager.plugin_manager();
    let receivers = take_engine_receivers(&mut engine)?;
    let (actor, mut actor_task) = crate::actor::spawn_actor(
        engine,
        receivers,
        selections.clone(),
        events.clone(),
        cancel.clone(),
    );
    let maintenance_actor = actor.clone();
    let startup_config = all_config.clone();
    let startup_maintenance_task = tokio::spawn(async move {
        if config_enabled(&startup_config, "bt_tracker_sub_enabled", true) {
            let _ = maintenance_actor
                .execute(crate::actor::ActorOperation::RefreshTrackerSubscription)
                .await;
        }
        if config_enabled(&startup_config, "ed2k_server_sub_enabled", true) {
            let _ = maintenance_actor
                .execute(crate::actor::ActorOperation::RefreshEd2kServerSubscription)
                .await;
        }
        if config_enabled(&startup_config, "ed2k_enable_kad", true) {
            let _ = maintenance_actor
                .execute(crate::actor::ActorOperation::RefreshEd2kNodes)
                .await;
        }
    });

    let bearer =
        load_or_create_bearer(&data_dir, process_config.token_file_override.as_deref()).await?;
    let blobs = Arc::new(BlobStore::open(data_dir.join("daemon-blobs")).await?);
    let hello = crate::service_hello(uuid::Uuid::new_v4().to_string(), runtime_capabilities(true));
    let service = Arc::new(DaemonService::new(
        hello,
        events,
        selections,
        blobs.clone(),
        actor.clone(),
        service_db,
        #[cfg(any(feature = "plugins", feature = "components"))]
        service_data_dir,
        #[cfg(feature = "plugins")]
        service_plugin_manager,
    ));
    service
        .initialize_dynamic_projection()
        .await
        .map_err(|error| std::io::Error::other(error.message))?;
    let listener = TcpListener::bind(process_config.bind_addr).await?;
    tracing::info!(address = %process_config.bind_addr, "fluxdownd control plane listening");

    let sweep_task = spawn_blob_sweeper(blobs.clone(), cancel.clone());
    let result = serve(listener, service, bearer, cancel.clone()).await;
    let _ = tokio::time::timeout(Duration::from_secs(10), actor.shutdown()).await;
    cancel.cancel();
    let _ = sweep_task.await;
    if tokio::time::timeout(Duration::from_secs(10), &mut actor_task)
        .await
        .is_err()
    {
        actor_task.abort();
        let _ = actor_task.await;
    }
    if !startup_maintenance_task.is_finished() {
        startup_maintenance_task.abort();
    }
    let _ = startup_maintenance_task.await;
    if let Err(error) = blobs.cleanup_all().await {
        tracing::warn!(error = %error, "daemon temporary cleanup failed");
    }
    result.map_err(Into::into)
}

async fn open_database(
    config: &DaemonConfig,
    data_dir: &std::path::Path,
) -> Result<(Db, EngineWriteGuard), fluxdown_engine::db::DbError> {
    match &config.database_url {
        Some(url) => Db::connect_exclusive(url, data_dir).await,
        None => Db::open_exclusive(data_dir).await,
    }
}

async fn initial_snapshot(
    db: &Db,
    config: &HashMap<String, String>,
) -> Result<DaemonSnapshot, fluxdown_engine::db::DbError> {
    Ok(DaemonSnapshot {
        tasks: db
            .load_all_tasks()
            .await?
            .into_iter()
            .map(fluxdown_engine_protocol::task_info_to_dto)
            .collect(),
        queues: db
            .load_all_queues()
            .await?
            .into_iter()
            .map(fluxdown_engine_protocol::queue_info_to_dto)
            .collect(),
        groups: db
            .load_all_groups()
            .await?
            .into_iter()
            .map(fluxdown_engine_protocol::group_info_to_dto)
            .collect(),
        config: DaemonConfigSnapshot {
            revision: config
                .get("daemon_config_revision")
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0),
            values: crate::config::public_config_values(config),
        },
        rss_sources: db
            .load_all_rss_sources()
            .await?
            .into_iter()
            .map(fluxdown_engine_protocol::rss_source_info_to_dto)
            .collect(),
        ..DaemonSnapshot::default()
    })
}

fn take_engine_receivers(engine: &mut Engine) -> Result<EngineReceivers, std::io::Error> {
    let done = engine
        .manager
        .take_done_rx()
        .ok_or_else(|| std::io::Error::other("done receiver already taken"))?;
    let retry = engine
        .manager
        .take_retry_rx()
        .ok_or_else(|| std::io::Error::other("retry receiver already taken"))?;
    let missing_cleanup = engine
        .manager
        .take_missing_cleanup_rx()
        .ok_or_else(|| std::io::Error::other("missing cleanup receiver already taken"))?;
    let (plugin_tx, plugin) = tokio::sync::mpsc::unbounded_channel();
    #[cfg(feature = "plugins")]
    {
        let mut resolve = engine
            .manager
            .take_resolve_rx()
            .ok_or_else(|| std::io::Error::other("resolve receiver already taken"))?;
        let resolve_tx = plugin_tx.clone();
        tokio::spawn(async move {
            while let Some(outcome) = resolve.recv().await {
                if resolve_tx
                    .send(PluginEvent::Resolve(Box::new(outcome)))
                    .is_err()
                {
                    break;
                }
            }
        });
        let mut retry_events = engine
            .manager
            .take_plugin_retry_rx()
            .ok_or_else(|| std::io::Error::other("plugin retry receiver already taken"))?;
        let retry_tx = plugin_tx.clone();
        tokio::spawn(async move {
            while let Some((task_id, delay_ms)) = retry_events.recv().await {
                if retry_tx
                    .send(PluginEvent::Retry { task_id, delay_ms })
                    .is_err()
                {
                    break;
                }
            }
        });
    }
    drop(plugin_tx);
    Ok(EngineReceivers {
        done,
        retry,
        plugin,
        missing_cleanup,
    })
}

fn apply_manager_settings(engine: &mut Engine, config: &HashMap<String, String>) {
    engine
        .manager
        .set_default_segments(i32_config(config, "default_segments", 0));
    engine
        .manager
        .set_auto_max_connections(i32_config(config, "auto_max_connections", 16));
    engine
        .manager
        .set_cdn_multi_enabled(bool_config(config, "cdn_multi_enabled", false));
    engine
        .manager
        .set_cdn_max_nodes(i32_config(config, "cdn_max_nodes", 0).clamp(0, 8));
    engine
        .manager
        .set_max_auto_retries(i32_config(config, "max_auto_retries", 3));
    engine
        .manager
        .set_auto_retry_delay_secs(u64_config(config, "auto_retry_delay_secs", 2));
    engine
        .manager
        .set_use_server_time(bool_config(config, "use_server_time", false));
    engine.manager.set_file_exists_overwrite(
        config
            .get("file_exists_behavior")
            .is_some_and(|value| value == "overwrite"),
    );
    engine.manager.set_missing_file_auto_delete(
        config
            .get("file_missing_action")
            .is_some_and(|value| value == "delete"),
    );
}

fn configured_save_dir(config: &HashMap<String, String>, fallback: String) -> String {
    config
        .get("default_save_dir")
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .unwrap_or(fallback)
}

fn bool_config(config: &HashMap<String, String>, key: &str, fallback: bool) -> bool {
    config
        .get(key)
        .map_or(fallback, |value| value == "true" || value == "1")
}

fn i32_config(config: &HashMap<String, String>, key: &str, fallback: i32) -> i32 {
    config
        .get(key)
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(fallback)
}

fn u64_config(config: &HashMap<String, String>, key: &str, fallback: u64) -> u64 {
    config
        .get(key)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(fallback)
}

fn usize_config(config: &HashMap<String, String>, key: &str, fallback: usize) -> usize {
    config
        .get(key)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(fallback)
}

fn spawn_blob_sweeper(
    blobs: Arc<BlobStore>,
    cancel: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = interval.tick() => {
                    if let Err(error) = blobs.sweep().await {
                        tracing::warn!(error = %error, "daemon blob sweep failed");
                    }
                }
            }
        }
    })
}

/// 只宣称已完成初始化的能力。
#[must_use]
pub fn runtime_capabilities(engine_initialized: bool) -> Vec<String> {
    if !engine_initialized {
        return Vec::new();
    }
    let capabilities = vec![
        fluxdown_protocol::method::CAPABILITY_DAEMON_TASKS.to_owned(),
        fluxdown_protocol::method::CAPABILITY_DAEMON_QUEUES.to_owned(),
        fluxdown_protocol::method::CAPABILITY_DAEMON_GROUPS.to_owned(),
        fluxdown_protocol::method::CAPABILITY_DAEMON_CONFIG.to_owned(),
        fluxdown_protocol::method::CAPABILITY_DAEMON_RSS.to_owned(),
        fluxdown_protocol::method::CAPABILITY_DAEMON_WEBHOOKS.to_owned(),
        fluxdown_protocol::method::CAPABILITY_DAEMON_SELECTIONS.to_owned(),
        fluxdown_protocol::method::CAPABILITY_DAEMON_FILES.to_owned(),
    ];
    #[cfg(feature = "plugins")]
    let capabilities = {
        let mut enabled = capabilities;
        enabled.push(fluxdown_protocol::method::CAPABILITY_DAEMON_PLUGINS.to_owned());
        enabled
    };
    #[cfg(feature = "components")]
    let capabilities = {
        let mut enabled = capabilities;
        enabled.push(fluxdown_protocol::method::CAPABILITY_DAEMON_COMPONENTS.to_owned());
        enabled
    };
    capabilities
}

fn config_enabled(config: &HashMap<String, String>, key: &str, default: bool) -> bool {
    config
        .get(key)
        .map(|value| matches!(value.as_str(), "true" | "1"))
        .unwrap_or(default)
}
