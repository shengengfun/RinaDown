//! 配置同步 ownership、pull-before-push、dirty/tombstone 与 self-echo 状态机。

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use fluxdown_protocol::{AgentEvent, DaemonConfigPatch, RpcErrorData};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::{Mutex, Notify};
use tokio_util::sync::CancellationToken;

use crate::cloud::{CloudApi, CloudError};
use crate::daemon_client::DaemonClient;
use crate::event_hub::AgentEventHub;
use crate::state::{AgentState, StateStore};
use fluxdown_protocol::{SettingOwner, setting_spec, validate_value, value_to_daemon_config};

pub use fluxdown_protocol::SettingOwner as SyncOwner;

#[must_use]
pub fn owner_for_key(key: &str) -> SyncOwner {
    if matches!(
        key,
        "cdn_node_health" | "auto_route_health" | "cdn_pending_reports" | "domain_conn_caps"
    ) {
        return SyncOwner::Excluded;
    }
    setting_spec(key)
        .map(|spec| spec.owner)
        .unwrap_or(SettingOwner::Preferences)
}

const SSE_IDLE_TIMEOUT: Duration = Duration::from_secs(75);
const LOCAL_DEBOUNCE: Duration = Duration::from_millis(600);
const RETRY_DELAYS: [Duration; 4] = [
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(60),
    Duration::from_secs(300),
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PullResult {
    revision: u64,
    #[serde(default)]
    resync: bool,
    #[serde(default)]
    items: Vec<SyncItem>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncItem {
    key: String,
    value: Value,
    #[serde(default)]
    deleted: bool,
    version: u64,
    #[serde(default)]
    device_id: String,
}

pub struct SyncService {
    cloud: CloudApi,
    daemon: Arc<DaemonClient>,
    events: AgentEventHub,
    state: Arc<Mutex<AgentState>>,
    store: Arc<StateStore>,
    gate: Mutex<()>,
    wake: Notify,
}

impl SyncService {
    #[must_use]
    pub fn new(
        cloud: CloudApi,
        daemon: Arc<DaemonClient>,
        events: AgentEventHub,
        state: Arc<Mutex<AgentState>>,
        store: Arc<StateStore>,
    ) -> Self {
        Self {
            cloud,
            daemon,
            events,
            state,
            store,
            gate: Mutex::new(()),
            wake: Notify::new(),
        }
    }

    #[must_use]
    pub async fn status(&self) -> fluxdown_protocol::SyncStatusDto {
        self.state.lock().await.sync.clone()
    }

    pub async fn set_enabled(&self, enabled: bool) -> Result<(), SyncError> {
        let mut state = self.state.lock().await;
        state.sync.enabled = enabled;
        self.store.save(&state).await?;
        self.events
            .publish(AgentEvent::SyncChanged(state.sync.clone()));
        drop(state);
        self.wake.notify_one();
        Ok(())
    }

    pub async fn run(self: Arc<Self>, cancel: CancellationToken) {
        let mut retry_attempt = 0_usize;
        loop {
            if cancel.is_cancelled() {
                return;
            }
            let enabled = self.state.lock().await.sync.enabled;
            if !enabled || !self.cloud.is_authenticated().await {
                tokio::select! {
                    _ = cancel.cancelled() => return,
                    _ = self.wake.notified() => {},
                    _ = tokio::time::sleep(Duration::from_secs(30)) => {},
                }
                continue;
            }
            if let Err(error) = self.sync_now().await {
                self.record_error(&error).await;
                let delay = RETRY_DELAYS[retry_attempt.min(RETRY_DELAYS.len() - 1)];
                retry_attempt = retry_attempt.saturating_add(1);
                tokio::select! {
                    _ = cancel.cancelled() => return,
                    _ = self.wake.notified() => {},
                    _ = tokio::time::sleep(delay) => {},
                }
                continue;
            }
            retry_attempt = 0;
            let device_id = self.state.lock().await.device_id.clone();
            match self.cloud.sync_events(&device_id).await {
                Ok(response) => {
                    if let Err(error) = self.consume_events(response, &cancel).await {
                        self.record_error(&error).await;
                    }
                }
                Err(error) => self.record_error(&SyncError::Cloud(error)).await,
            }
            let delay = RETRY_DELAYS[retry_attempt.min(RETRY_DELAYS.len() - 1)];
            retry_attempt = retry_attempt.saturating_add(1);
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = self.wake.notified() => {},
                _ = tokio::time::sleep(delay) => {},
            }
        }
    }

    async fn consume_events(
        &self,
        response: reqwest::Response,
        cancel: &CancellationToken,
    ) -> Result<(), SyncError> {
        let mut stream = response.bytes_stream();
        let mut buffer = Vec::<u8>::new();
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return Ok(()),
                _ = self.wake.notified() => {
                    tokio::time::sleep(LOCAL_DEBOUNCE).await;
                    self.sync_now().await?;
                }
                chunk = tokio::time::timeout(SSE_IDLE_TIMEOUT, stream.next()) => {
                    let chunk = chunk
                        .map_err(|_| SyncError::Protocol("sync SSE idle timeout".to_owned()))?
                        .ok_or_else(|| SyncError::Protocol("sync SSE disconnected".to_owned()))?
                        .map_err(|error| SyncError::Protocol(format!("sync SSE read failed: {error:#}")))?;
                    buffer.extend_from_slice(&chunk);
                    while let Some(newline) = buffer.iter().position(|byte| *byte == b'\n') {
                        let line = buffer.drain(..=newline).collect::<Vec<_>>();
                        let line = std::str::from_utf8(&line)
                            .map_err(|error| SyncError::Protocol(error.to_string()))?
                            .trim();
                        if let Some(payload) = line.strip_prefix("data:") {
                            let event = serde_json::from_str::<Value>(payload.trim())
                                .map_err(|error| SyncError::Protocol(error.to_string()))?;
                            if event.get("kind").and_then(Value::as_str) == Some("cdn_config") {
                                continue;
                            }
                            if let Some(revision) =
                                event.get("revision").and_then(Value::as_u64)
                            {
                                let current = self.state.lock().await.sync.revision;
                                if revision != current {
                                    self.sync_now().await?;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    async fn record_error(&self, error: &SyncError) {
        let mut state = self.state.lock().await;
        state.sync.last_error = Some(error.to_string());
        let _ = self.store.save(&state).await;
        self.events
            .publish(AgentEvent::SyncChanged(state.sync.clone()));
    }

    /// 执行一次严格 pull-before-push 同步。
    pub async fn sync_now(&self) -> Result<(), SyncError> {
        let _gate = self.gate.lock().await;
        let (since, device_id) = {
            let state = self.state.lock().await;
            (state.sync.revision, state.device_id.clone())
        };
        let pull_value = self.cloud.sync_pull(since, &device_id).await?;
        let pull = serde_json::from_value::<PullResult>(pull_value)
            .map_err(|error| SyncError::Protocol(error.to_string()))?;

        let mut daemon_changes = BTreeMap::new();
        {
            let state = self.state.lock().await;
            for item in &pull.items {
                let Some(spec) = setting_spec(&item.key) else {
                    continue;
                };
                if spec.owner == SyncOwner::Daemon
                    && !item.deleted
                    && !state
                        .sync_entries
                        .get(&item.key)
                        .is_some_and(|entry| entry.dirty)
                {
                    daemon_changes.insert(
                        spec.storage_key.to_owned(),
                        value_to_daemon_config(spec, &item.value).map_err(SyncError::Protocol)?,
                    );
                }
            }
        }
        if !daemon_changes.is_empty() {
            let revision = daemon_revision(&self.events);
            self.daemon
                .call::<DaemonConfigPatch, Value>(
                    fluxdown_protocol::method::DAEMON_CONFIG_PATCH,
                    Some(DaemonConfigPatch {
                        expected_revision: revision,
                        values: daemon_changes,
                    }),
                )
                .await
                .map_err(SyncError::Daemon)?;
        }

        let remote_keys = pull
            .items
            .iter()
            .map(|item| item.key.clone())
            .collect::<HashSet<_>>();
        let (dirty_to_push, sent_entries) = {
            let mut state = self.state.lock().await;
            if pull.resync {
                state.sync.revision = 0;
                for (key, entry) in &mut state.sync_entries {
                    if setting_spec(key).is_some() && !remote_keys.contains(key) {
                        entry.dirty = true;
                    }
                }
            }
            for item in pull.items {
                apply_pull_item(&mut state, &device_id, item)?;
            }
            state.sync.revision = pull.revision;
            state.sync_pulled = true;
            let sent_entries = state
                .sync_entries
                .iter()
                .filter(|(_, entry)| entry.dirty)
                .map(|(key, entry)| {
                    (
                        key.clone(),
                        (entry.value.clone(), entry.deleted, entry.version),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            let payload = sent_entries
                .iter()
                .map(|(key, (value, deleted, version))| {
                    serde_json::json!({
                        "key": key,
                        "value": value,
                        "deleted": deleted,
                        "version": version,
                    })
                })
                .collect::<Vec<_>>();
            (payload, sent_entries)
        };

        if !dirty_to_push.is_empty() {
            let response = self
                .cloud
                .sync_push(&serde_json::json!({
                    "deviceId": device_id,
                    "items": &dirty_to_push,
                }))
                .await?;
            let revision = response
                .get("revision")
                .and_then(Value::as_u64)
                .ok_or_else(|| SyncError::Protocol("sync push returned no revision".to_owned()))?;
            let mut state = self.state.lock().await;
            for (key, (value, deleted, version)) in &sent_entries {
                if let Some(entry) = state.sync_entries.get_mut(key)
                    && entry.value == *value
                    && entry.deleted == *deleted
                    && entry.version == *version
                {
                    entry.dirty = false;
                }
            }
            state.sync.revision = revision;
        }

        let mut state = self.state.lock().await;
        state.sync.dirty_keys = state
            .sync_entries
            .iter()
            .filter(|(_, entry)| entry.dirty)
            .map(|(key, _)| key.clone())
            .collect();
        state.sync.last_error = None;
        self.store.save(&state).await?;
        self.events
            .publish(AgentEvent::PreferencesChanged(state.preferences.clone()));
        self.events
            .publish(AgentEvent::SyncChanged(state.sync.clone()));
        Ok(())
    }

    /// 本地变更立即置 dirty；真正 push 等下一次 pull 成功后执行。
    pub async fn mark_local(
        &self,
        key: String,
        value: Value,
        deleted: bool,
    ) -> Result<(), SyncError> {
        let owner = owner_for_key(&key);
        if owner == SyncOwner::Excluded {
            return Ok(());
        }
        if !deleted && let Some(spec) = setting_spec(&key) {
            validate_value(spec.key, &value).map_err(SyncError::Protocol)?;
        }
        if owner == SyncOwner::Daemon
            && !deleted
            && let Some(spec) = setting_spec(&key)
        {
            let revision = daemon_revision(&self.events);
            self.daemon
                .call::<DaemonConfigPatch, Value>(
                    fluxdown_protocol::method::DAEMON_CONFIG_PATCH,
                    Some(DaemonConfigPatch {
                        expected_revision: revision,
                        values: BTreeMap::from([(
                            spec.storage_key.to_owned(),
                            value_to_daemon_config(spec, &value).map_err(SyncError::Protocol)?,
                        )]),
                    }),
                )
                .await
                .map_err(SyncError::Daemon)?;
        }
        let mut state = self.state.lock().await;
        let entry = state.sync_entries.entry(key.clone()).or_default();
        entry.value = value.clone();
        entry.deleted = deleted;
        entry.dirty = true;
        if matches!(owner, SyncOwner::Agent | SyncOwner::Preferences) {
            if deleted {
                state.preferences.values.remove(&key);
            } else {
                state.preferences.values.insert(key.clone(), value);
            }
            state.preferences.revision = state.preferences.revision.saturating_add(1);
        }
        state.sync.dirty_keys = state
            .sync_entries
            .iter()
            .filter(|(_, entry)| entry.dirty)
            .map(|(key, _)| key.clone())
            .collect();
        self.store.save(&state).await?;
        self.events
            .publish(AgentEvent::PreferencesChanged(state.preferences.clone()));
        self.events
            .publish(AgentEvent::SyncChanged(state.sync.clone()));
        drop(state);
        self.wake.notify_one();
        Ok(())
    }

    pub async fn set_local_preference(
        &self,
        key: String,
        value: Value,
        deleted: bool,
    ) -> Result<(), SyncError> {
        let mut state = self.state.lock().await;
        if deleted {
            state.preferences.values.remove(&key);
        } else {
            state.preferences.values.insert(key, value);
        }
        state.preferences.revision = state.preferences.revision.saturating_add(1);
        self.store.save(&state).await?;
        self.events
            .publish(AgentEvent::PreferencesChanged(state.preferences.clone()));
        Ok(())
    }
}

fn apply_pull_item(
    state: &mut AgentState,
    local_device: &str,
    item: SyncItem,
) -> Result<(), SyncError> {
    if !item.deleted
        && let Some(spec) = setting_spec(&item.key)
    {
        validate_value(spec.key, &item.value).map_err(SyncError::Protocol)?;
    }
    let entry = state.sync_entries.entry(item.key.clone()).or_default();
    if item.device_id == local_device {
        entry.version = entry.version.max(item.version);
        if entry.value == item.value && entry.deleted == item.deleted {
            entry.dirty = false;
        }
        return Ok(());
    }
    if entry.dirty {
        entry.version = entry.version.max(item.version);
        return Ok(());
    }
    entry.value = item.value.clone();
    entry.deleted = item.deleted;
    entry.version = item.version;
    match owner_for_key(&item.key) {
        SyncOwner::Agent | SyncOwner::Preferences => {
            if item.deleted {
                state.preferences.values.remove(&item.key);
            } else {
                state.preferences.values.insert(item.key, item.value);
            }
        }
        SyncOwner::Daemon | SyncOwner::Excluded => {}
    }
    Ok(())
}

fn daemon_revision(events: &AgentEventHub) -> u64 {
    match events.snapshot().body {
        fluxdown_protocol::SnapshotBody::Agent(agent) => agent.daemon.config.revision,
        fluxdown_protocol::SnapshotBody::Daemon(_) => 0,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error(transparent)]
    Cloud(#[from] CloudError),
    #[error("daemon sync apply failed: {0:?}")]
    Daemon(RpcErrorData),
    #[error("sync protocol error: {0}")]
    Protocol(String),
    #[error(transparent)]
    State(#[from] crate::state::StateError),
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::Router;
    use axum::extract::State;
    use axum::http::{HeaderMap, StatusCode, header};
    use axum::response::IntoResponse;
    use axum::routing::get;
    use serde_json::json;
    use tokio_util::sync::CancellationToken;

    use super::{SyncItem, SyncOwner, SyncService, apply_pull_item, owner_for_key};
    use crate::state::{AgentState, CloudCredentials, PersistedSyncEntry, StateStore};

    #[test]
    fn ownership_table_routes_and_excludes_engine_learning_keys() {
        assert_eq!(
            owner_for_key("download.max_concurrent_tasks"),
            SyncOwner::Daemon
        );
        assert_eq!(owner_for_key("download.keep_awake"), SyncOwner::Agent);
        assert_eq!(
            owner_for_key("appearance.theme_mode"),
            SyncOwner::Preferences
        );
        assert_eq!(owner_for_key("cdn_node_health"), SyncOwner::Excluded);
    }

    #[test]
    fn dirty_local_value_wins_and_self_echo_only_confirms_equal_value() {
        let mut state = AgentState::default();
        state.sync_entries.insert(
            "appearance.theme_mode".to_owned(),
            PersistedSyncEntry {
                value: json!("dark"),
                version: 1,
                dirty: true,
                deleted: false,
            },
        );
        apply_pull_item(
            &mut state,
            "device-1",
            SyncItem {
                key: "appearance.theme_mode".to_owned(),
                value: json!("light"),
                deleted: false,
                version: 2,
                device_id: "device-2".to_owned(),
            },
        )
        .expect("apply remote item");
        assert_eq!(
            state.sync_entries["appearance.theme_mode"].value,
            json!("dark")
        );
        assert!(state.sync_entries["appearance.theme_mode"].dirty);
        apply_pull_item(
            &mut state,
            "device-1",
            SyncItem {
                key: "appearance.theme_mode".to_owned(),
                value: json!("dark"),
                deleted: false,
                version: 3,
                device_id: "device-1".to_owned(),
            },
        )
        .expect("apply self echo");
        assert!(!state.sync_entries["appearance.theme_mode"].dirty);
    }

    #[derive(Default)]
    struct SyncMockState {
        pulls: AtomicUsize,
        events: AtomicUsize,
    }

    async fn mock_pull(State(state): State<Arc<SyncMockState>>) -> impl IntoResponse {
        state.pulls.fetch_add(1, Ordering::SeqCst);
        axum::Json(json!({"revision": 0, "resync": false, "items": []}))
    }

    async fn mock_events(
        State(state): State<Arc<SyncMockState>>,
        headers: HeaderMap,
    ) -> impl IntoResponse {
        assert_eq!(
            headers
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer access")
        );
        state.events.fetch_add(1, Ordering::SeqCst);
        (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/event-stream")],
            "data: {\"revision\":0}\n\n",
        )
    }

    #[tokio::test]
    async fn enabled_worker_reconnects_authenticated_sse_after_disconnect() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind sync mock");
        let address = listener.local_addr().expect("sync mock address");
        let mock = Arc::new(SyncMockState::default());
        let app = Router::new()
            .route("/api/v1/sync/items", get(mock_pull))
            .route("/api/v1/sync/events", get(mock_events))
            .with_state(mock.clone());
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let dir = std::env::temp_dir().join(format!(
            "fluxdown_sync_worker_{}_{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let store = Arc::new(
            StateStore::open(dir.clone())
                .await
                .expect("sync state store"),
        );
        let initial = AgentState {
            device_id: "device-1".to_owned(),
            sync: fluxdown_protocol::SyncStatusDto {
                enabled: true,
                ..Default::default()
            },
            credentials: Some(CloudCredentials {
                access_token: "access".to_owned(),
                refresh_token: "refresh".to_owned(),
                expires_at_unix: i64::MAX,
                session: None,
            }),
            ..Default::default()
        };
        store.save(&initial).await.expect("save sync state");
        let state = Arc::new(tokio::sync::Mutex::new(initial));
        let cloud_client = crate::cloud::CloudClient::new(
            format!("http://{address}"),
            state.clone(),
            store.clone(),
        )
        .expect("sync cloud client");
        let events =
            crate::event_hub::AgentEventHub::new(fluxdown_protocol::AgentSnapshot::default());
        let service = Arc::new(SyncService::new(
            crate::cloud::CloudApi::new(cloud_client),
            Arc::new(crate::daemon_client::DaemonClient::disconnected()),
            events,
            state,
            store.clone(),
        ));
        let cancel = CancellationToken::new();
        let worker = tokio::spawn(service.run(cancel.clone()));
        tokio::time::timeout(std::time::Duration::from_secs(8), async {
            while mock.pulls.load(Ordering::SeqCst) < 2 || mock.events.load(Ordering::SeqCst) < 2 {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("sync worker reconnected SSE");
        cancel.cancel();
        worker.await.expect("join sync worker");
        drop(store);
        let _ = tokio::fs::remove_dir_all(dir).await;
    }
}
