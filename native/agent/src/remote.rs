//! 跨设备任务全量投影、本地执行路由、绑定评分与缺失宽限。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use fluxdown_protocol::{
    AgentEvent, CreateTaskRequest, DaemonCreateTaskParams, RemoteTaskDto, RemoteTaskStatus,
};
use futures_util::StreamExt;
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::cloud::{CloudApi, CloudError};
use crate::daemon_client::DaemonClient;
use crate::event_hub::AgentEventHub;
use crate::state::{AgentState, StateStore};

const MISSING_GRACE_ROUNDS: u8 = 3;
const SSE_IDLE_TIMEOUT: Duration = Duration::from_secs(75);
const REPORT_INTERVAL: Duration = Duration::from_secs(1);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const RETRY_DELAYS: [Duration; 3] = [
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(60),
];

pub struct RemoteTaskService {
    cloud: CloudApi,
    daemon: Arc<DaemonClient>,
    events: AgentEventHub,
    state: Arc<Mutex<AgentState>>,
    store: Arc<StateStore>,
    bindings: Mutex<HashMap<String, String>>,
    missing_rounds: Mutex<HashMap<String, u8>>,
    confirmed_commands: Mutex<HashSet<String>>,
    reported_statuses: Mutex<HashMap<String, RemoteTaskStatus>>,
    local_missing_rounds: Mutex<HashMap<String, u8>>,
}

impl RemoteTaskService {
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
            bindings: Mutex::new(HashMap::new()),
            missing_rounds: Mutex::new(HashMap::new()),
            confirmed_commands: Mutex::new(HashSet::new()),
            reported_statuses: Mutex::new(HashMap::new()),
            local_missing_rounds: Mutex::new(HashMap::new()),
        }
    }

    pub async fn run(self: Arc<Self>, cancel: CancellationToken) {
        let mut retry_attempt = 0_usize;
        loop {
            if cancel.is_cancelled() {
                return;
            }
            if !self.cloud.is_authenticated().await {
                tokio::select! {
                    _ = cancel.cancelled() => return,
                    _ = tokio::time::sleep(Duration::from_secs(30)) => {},
                }
                continue;
            }
            if let Err(error) = self.refresh_snapshot().await {
                tracing::warn!(error = %error, "remote task snapshot refresh failed");
                let delay = RETRY_DELAYS[retry_attempt.min(RETRY_DELAYS.len() - 1)];
                retry_attempt = retry_attempt.saturating_add(1);
                tokio::select! {
                    _ = cancel.cancelled() => return,
                    _ = tokio::time::sleep(delay) => {},
                }
                continue;
            }
            self.rebuild_bindings().await;
            if let Err(error) = self.accept_pending_dispatches().await {
                tracing::warn!(error = %error, "remote pending dispatch acceptance failed");
            }
            let device_id = self.local_device_id().await;
            match self.cloud.remote_events(&device_id).await {
                Ok(response) => {
                    retry_attempt = 0;
                    if let Err(error) = self.cloud.ping_presence().await {
                        tracing::warn!(error = %error, "initial remote presence heartbeat failed");
                    }
                    if let Err(error) = self.consume_events(response, &cancel).await {
                        tracing::warn!(error = %error, "remote task SSE disconnected");
                    }
                }
                Err(error) => tracing::warn!(error = %error, "remote task SSE connect failed"),
            }
            let delay = RETRY_DELAYS[retry_attempt.min(RETRY_DELAYS.len() - 1)];
            retry_attempt = retry_attempt.saturating_add(1);
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = tokio::time::sleep(delay) => {},
            }
        }
    }

    async fn consume_events(
        &self,
        response: reqwest::Response,
        cancel: &CancellationToken,
    ) -> Result<(), RemoteError> {
        let mut stream = response.bytes_stream();
        let mut buffer = Vec::<u8>::new();
        let mut report_tick = tokio::time::interval(REPORT_INTERVAL);
        let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
        report_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return Ok(()),
                _ = report_tick.tick() => {
                    if let Err(error) = self.report_local_progress().await {
                        tracing::warn!(error = %error, "remote progress report failed");
                    }
                }
                _ = heartbeat.tick() => {
                    if let Err(error) = self.cloud.ping_presence().await {
                        tracing::warn!(error = %error, "remote presence heartbeat failed");
                    }
                }
                chunk = tokio::time::timeout(SSE_IDLE_TIMEOUT, stream.next()) => {
                    let chunk = chunk
                        .map_err(|_| RemoteError::Protocol("remote SSE idle timeout".to_owned()))?
                        .ok_or_else(|| RemoteError::Protocol("remote SSE disconnected".to_owned()))?
                        .map_err(|error| RemoteError::Protocol(format!("remote SSE read failed: {error:#}")))?;
                    buffer.extend_from_slice(&chunk);
                    while let Some(newline) = buffer.iter().position(|byte| *byte == b'\n') {
                        let line = buffer.drain(..=newline).collect::<Vec<_>>();
                        let line = std::str::from_utf8(&line)
                            .map_err(|error| RemoteError::Protocol(error.to_string()))?
                            .trim();
                        if let Some(payload) = line.strip_prefix("data:") {
                            let event = serde_json::from_str::<Value>(payload.trim())?;
                            self.apply_remote_event(event).await?;
                        }
                    }
                }
            }
        }
    }

    async fn apply_remote_event(&self, event: Value) -> Result<(), RemoteError> {
        match event
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "task.dispatch" | "task.status" => {
                let task = serde_json::from_value::<RemoteTaskDto>(event)?;
                let mut tasks = self.tasks().await;
                if let Some(existing) = tasks.iter_mut().find(|existing| existing.id == task.id) {
                    existing.clone_from(&task);
                } else {
                    tasks.push(task);
                }
                self.replace_tasks(tasks).await?;
                self.rebuild_bindings().await;
                self.accept_pending_dispatches().await?;
            }
            "task.progress" => {
                let items = event
                    .get("items")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let mut tasks = self.tasks().await;
                for item in items {
                    let Some(id) = item.get("id").and_then(Value::as_str) else {
                        continue;
                    };
                    let Some(task) = tasks.iter_mut().find(|task| task.id == id) else {
                        continue;
                    };
                    if !task.status.is_terminal() {
                        task.status = RemoteTaskStatus::Downloading;
                    }
                    if let Some(value) = item.get("downloadedBytes").and_then(Value::as_i64) {
                        task.downloaded_bytes = value;
                    }
                    if let Some(value) = item.get("speed").and_then(Value::as_i64) {
                        task.speed = value;
                    }
                    if let Some(value) = item.get("progress").and_then(Value::as_f64) {
                        task.progress = value;
                    }
                }
                self.replace_tasks(tasks).await?;
            }
            "task.command" => {
                let target = event
                    .get("toDevice")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let local_device = self.local_device_id().await;
                if target == local_device {
                    let task_id = event
                        .get("taskId")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let action = event
                        .get("action")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if let Some(task) = self
                        .tasks()
                        .await
                        .into_iter()
                        .find(|task| task.id == task_id)
                    {
                        let command_id = event
                            .get("commandId")
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                            .unwrap_or_else(|| format!("{task_id}:{action}"));
                        self.command(command_id, &task, &local_device, action)
                            .await?;
                    }
                }
            }
            "presence" => {
                let value = self.cloud.devices(&self.local_device_id().await).await?;
                let devices = value
                    .get("devices")
                    .or_else(|| value.get("value"))
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .map(serde_json::from_value)
                    .collect::<Result<Vec<_>, _>>()?;
                self.events
                    .publish(AgentEvent::CloudDevicesChanged(devices));
            }
            "session.revoked" => {
                let target = event.get("deviceId").and_then(Value::as_str);
                let local = self.local_device_id().await;
                if target.is_none_or(|target| target == local) {
                    self.cloud.clear_session().await?;
                    self.events
                        .publish(AgentEvent::SessionChanged(Box::new(None)));
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn replace_tasks(&self, tasks: Vec<RemoteTaskDto>) -> Result<(), RemoteError> {
        let mut state = self.state.lock().await;
        state.remote_tasks.clone_from(&tasks);
        self.store.save(&state).await?;
        drop(state);
        self.events.publish(AgentEvent::RemoteTasksChanged(tasks));
        Ok(())
    }

    async fn rebuild_bindings(&self) {
        let local_device = self.local_device_id().await;
        let local_tasks = daemon_tasks(&self.events);
        let remote_tasks = self.tasks().await;
        let mut bindings = self.bindings.lock().await;
        let mut claimed = bindings.values().cloned().collect::<HashSet<_>>();
        for remote in &remote_tasks {
            if remote.to_device != local_device
                || remote.status == RemoteTaskStatus::Pending
                || remote.status.is_terminal()
                || bindings.contains_key(&remote.id)
            {
                continue;
            }
            if let Some(local) = local_tasks
                .iter()
                .filter(|task| !claimed.contains(&task.task_id))
                .filter_map(|task| {
                    let score = Self::rebind_score(remote, task);
                    (score >= 11).then_some((score, task))
                })
                .max_by_key(|(score, _)| *score)
                .map(|(_, task)| task)
            {
                bindings.insert(remote.id.clone(), local.task_id.clone());
                claimed.insert(local.task_id.clone());
            }
        }
    }

    async fn accept_pending_dispatches(&self) -> Result<(), RemoteError> {
        let local_device = self.local_device_id().await;
        let tasks = self.tasks().await;
        for task in tasks.into_iter().filter(|task| {
            task.to_device == local_device && task.status == RemoteTaskStatus::Pending
        }) {
            if self.bindings.lock().await.contains_key(&task.id) {
                continue;
            }
            let request = serde_json::from_value::<CreateTaskRequest>(json!({
                "url": task.url,
                "fileName": task.file_name,
                "saveDir": task.save_dir.clone().unwrap_or_default()
            }))?;
            let result = self
                .daemon
                .call::<DaemonCreateTaskParams, Value>(
                    fluxdown_protocol::method::DAEMON_TASK_CREATE,
                    Some(DaemonCreateTaskParams {
                        request,
                        torrent_blob_id: None,
                        unattended: true,
                    }),
                )
                .await
                .map_err(RemoteError::Daemon)?;
            let local_id = result
                .get("taskId")
                .and_then(Value::as_str)
                .ok_or_else(|| RemoteError::Protocol("daemon returned no taskId".to_owned()))?
                .to_owned();
            self.bindings.lock().await.insert(task.id.clone(), local_id);
            self.cloud
                .report_remote_status(&task.id, &json!({"status": "accepted"}))
                .await?;
            self.reported_statuses
                .lock()
                .await
                .insert(task.id, RemoteTaskStatus::Accepted);
        }
        Ok(())
    }

    async fn report_local_progress(&self) -> Result<(), RemoteError> {
        let local_tasks = daemon_tasks(&self.events);
        let bindings = self.bindings.lock().await.clone();
        let mut progress = Vec::new();
        for (remote_id, local_id) in bindings {
            let Some(task) = local_tasks.iter().find(|task| task.task_id == local_id) else {
                let mut misses = self.local_missing_rounds.lock().await;
                let count = misses.entry(remote_id.clone()).or_default();
                *count = count.saturating_add(1);
                if *count >= MISSING_GRACE_ROUNDS {
                    drop(misses);
                    self.report_status_if_changed(
                        &remote_id,
                        RemoteTaskStatus::Failed,
                        Some("local task disappeared"),
                        None,
                    )
                    .await?;
                    self.bindings.lock().await.remove(&remote_id);
                }
                continue;
            };
            self.local_missing_rounds.lock().await.remove(&remote_id);
            let status = local_status(task.status);
            self.report_status_if_changed(
                &remote_id,
                status,
                (!task.error_message.is_empty()).then_some(task.error_message.as_str()),
                Some(task),
            )
            .await?;
            if matches!(task.status, 1 | 5) {
                progress.push(json!({
                    "taskId": remote_id,
                    "downloadedBytes": task.downloaded_bytes,
                    "speed": 0,
                    "progress": if task.total_bytes > 0 {
                        task.downloaded_bytes as f64 / task.total_bytes as f64
                    } else {
                        0.0
                    }
                }));
            }
        }
        if !progress.is_empty() {
            self.cloud
                .report_remote_progress(&json!({"items": progress}))
                .await?;
        }
        Ok(())
    }

    async fn report_status_if_changed(
        &self,
        remote_id: &str,
        status: RemoteTaskStatus,
        error: Option<&str>,
        task: Option<&fluxdown_protocol::TaskDto>,
    ) -> Result<(), RemoteError> {
        if self.reported_statuses.lock().await.get(remote_id) == Some(&status) {
            return Ok(());
        }
        let body = json!({
            "status": remote_status_wire(status),
            "totalBytes": task.map(|task| task.total_bytes),
            "fileName": task.map(|task| task.file_name.clone()),
            "error": error,
        });
        match self.cloud.report_remote_status(remote_id, &body).await {
            Ok(_) => {
                self.reported_statuses
                    .lock()
                    .await
                    .insert(remote_id.to_owned(), status);
                if status.is_terminal() {
                    self.bindings.lock().await.remove(remote_id);
                }
                Ok(())
            }
            Err(error) if matches!(error.status, Some(404 | 409)) => {
                self.bindings.lock().await.remove(remote_id);
                Ok(())
            }
            Err(error) => Err(RemoteError::Cloud(error)),
        }
    }

    /// 首次或重连后用完整 `/tasks/remote` 快照替换投影。
    pub async fn refresh_snapshot(&self) -> Result<Vec<RemoteTaskDto>, RemoteError> {
        let value = self.cloud.remote_tasks().await?;
        let tasks = parse_task_list(value)?;
        let merged = self.apply_missing_grace(tasks).await;
        let mut state = self.state.lock().await;
        state.remote_tasks.clone_from(&merged);
        self.store.save(&state).await?;
        self.events
            .publish(AgentEvent::RemoteTasksChanged(merged.clone()));
        Ok(merged)
    }

    pub async fn local_device_id(&self) -> String {
        self.state.lock().await.device_id.clone()
    }

    pub async fn tasks(&self) -> Vec<RemoteTaskDto> {
        self.state.lock().await.remote_tasks.clone()
    }

    /// 本机目标直接创建一个 daemon 任务；远端目标交给 FluxCloud。
    pub async fn dispatch(
        &self,
        to_device: &str,
        local_device: &str,
        url: String,
        file_name: String,
        save_dir: Option<String>,
    ) -> Result<Value, RemoteError> {
        if to_device == local_device {
            let request = serde_json::from_value::<CreateTaskRequest>(json!({
                "url": url,
                "fileName": file_name,
                "saveDir": save_dir.unwrap_or_default()
            }))?;
            let result = self
                .daemon
                .call::<DaemonCreateTaskParams, Value>(
                    fluxdown_protocol::method::DAEMON_TASK_CREATE,
                    Some(DaemonCreateTaskParams {
                        request,
                        torrent_blob_id: None,
                        unattended: false,
                    }),
                )
                .await
                .map_err(RemoteError::Daemon)?;
            return Ok(result);
        }
        Ok(self
            .cloud
            .dispatch_remote(&json!({
                "deviceId": local_device,
                "toDevice": to_device,
                "url": url,
                "fileName": file_name,
                "saveDir": save_dir,
            }))
            .await?)
    }

    /// 命令去重只在已确认成功后推进；失败保留可重试状态。
    pub async fn command(
        &self,
        command_id: String,
        task: &RemoteTaskDto,
        local_device: &str,
        action: &str,
    ) -> Result<(), RemoteError> {
        if self.confirmed_commands.lock().await.contains(&command_id) {
            return Ok(());
        }
        if task.to_device == local_device {
            let local_id = self
                .bindings
                .lock()
                .await
                .get(&task.id)
                .cloned()
                .unwrap_or_else(|| task.id.clone());
            let method = match action {
                "pause" => fluxdown_protocol::method::DAEMON_TASK_PAUSE,
                "resume" => fluxdown_protocol::method::DAEMON_TASK_RESUME,
                "delete" | "cancel" => fluxdown_protocol::method::DAEMON_TASK_DELETE,
                _ => return Err(RemoteError::InvalidAction(action.to_owned())),
            };
            let _: Value = self
                .daemon
                .call(method, Some(json!({ "taskId": local_id })))
                .await
                .map_err(RemoteError::Daemon)?;
        } else {
            self.cloud
                .command_remote(&task.id, &json!({ "action": action }))
                .await?;
        }
        self.confirmed_commands.lock().await.insert(command_id);
        Ok(())
    }

    /// URL 为主键，文件名与保存目录提供稳定加分。
    #[must_use]
    pub fn rebind_score(remote: &RemoteTaskDto, local: &fluxdown_protocol::TaskDto) -> i32 {
        if remote.url != local.url && remote.url != local.origin_url {
            return -1;
        }
        let mut score = 10;
        if !remote.file_name.is_empty() && remote.file_name == local.file_name {
            score += 4;
        }
        if remote
            .save_dir
            .as_deref()
            .is_some_and(|dir| dir == local.save_dir)
        {
            score += 2;
        }
        score
    }

    async fn apply_missing_grace(&self, incoming: Vec<RemoteTaskDto>) -> Vec<RemoteTaskDto> {
        let previous = self.state.lock().await.remote_tasks.clone();
        let incoming_ids = incoming
            .iter()
            .map(|task| task.id.clone())
            .collect::<HashSet<_>>();
        let mut rounds = self.missing_rounds.lock().await;
        let mut merged = incoming;
        for task in previous {
            if incoming_ids.contains(task.id.as_str()) || task.status.is_terminal() {
                rounds.remove(&task.id);
                continue;
            }
            let round = rounds.entry(task.id.clone()).or_default();
            *round = round.saturating_add(1);
            if *round < MISSING_GRACE_ROUNDS {
                merged.push(task);
            } else {
                rounds.remove(&task.id);
                self.bindings.lock().await.remove(&task.id);
            }
        }
        merged
    }
}

fn daemon_tasks(events: &AgentEventHub) -> Vec<fluxdown_protocol::TaskDto> {
    match events.snapshot().body {
        fluxdown_protocol::SnapshotBody::Agent(snapshot) => snapshot.daemon.tasks,
        fluxdown_protocol::SnapshotBody::Daemon(_) => Vec::new(),
    }
}

fn local_status(status: i32) -> RemoteTaskStatus {
    match status {
        1 => RemoteTaskStatus::Downloading,
        2 => RemoteTaskStatus::Paused,
        3 => RemoteTaskStatus::Completed,
        4 => RemoteTaskStatus::Failed,
        _ => RemoteTaskStatus::Accepted,
    }
}

fn remote_status_wire(status: RemoteTaskStatus) -> &'static str {
    match status {
        RemoteTaskStatus::Pending => "pending",
        RemoteTaskStatus::Accepted => "accepted",
        RemoteTaskStatus::Downloading => "downloading",
        RemoteTaskStatus::Paused => "paused",
        RemoteTaskStatus::Completed => "completed",
        RemoteTaskStatus::Failed => "failed",
        RemoteTaskStatus::Canceled => "canceled",
    }
}

trait RemoteStatusExt {
    fn is_terminal(&self) -> bool;
}

impl RemoteStatusExt for RemoteTaskStatus {
    fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Canceled)
    }
}

fn parse_task_list(value: Value) -> Result<Vec<RemoteTaskDto>, RemoteError> {
    let list = value
        .get("tasks")
        .or_else(|| value.get("value"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    list.into_iter()
        .map(serde_json::from_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(RemoteError::Json)
}

#[derive(Debug, thiserror::Error)]
pub enum RemoteError {
    #[error(transparent)]
    Cloud(#[from] CloudError),
    #[error("daemon remote command failed: {0:?}")]
    Daemon(fluxdown_protocol::RpcErrorData),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    State(#[from] crate::state::StateError),
    #[error("invalid remote action: {0}")]
    InvalidAction(String),
    #[error("remote task protocol error: {0}")]
    Protocol(String),
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::Router;
    use axum::extract::State;
    use axum::http::{HeaderMap, StatusCode, header};
    use axum::response::IntoResponse;
    use axum::routing::{get, post};
    use fluxdown_protocol::{RemoteTaskDto, RemoteTaskStatus, TaskDto};
    use serde_json::json;
    use tokio_util::sync::CancellationToken;

    use super::RemoteTaskService;
    use crate::state::{AgentState, CloudCredentials, StateStore};

    #[test]
    fn rebind_score_requires_url_and_rewards_filename_and_directory() {
        let remote = serde_json::from_value::<RemoteTaskDto>(json!({
            "id": "r1", "url": "https://example.com/a", "fileName": "a.bin",
            "saveDir": "/tmp", "status": "pending"
        }))
        .expect("remote");
        let local = serde_json::from_value::<TaskDto>(json!({
            "taskId":"t1","url":"https://example.com/a","fileName":"a.bin",
            "saveDir":"/tmp","status":1,"downloadedBytes":0,"totalBytes":0,
            "errorMessage":"","createdAt":"1","proxyUrl":"","queueId":"main","checksum":""
        }))
        .expect("local");
        assert_eq!(RemoteTaskService::rebind_score(&remote, &local), 16);
        let unrelated = RemoteTaskDto {
            url: "https://other".to_owned(),
            status: RemoteTaskStatus::Pending,
            ..remote
        };
        assert_eq!(RemoteTaskService::rebind_score(&unrelated, &local), -1);
    }

    #[derive(Default)]
    struct RemoteMockState {
        snapshots: AtomicUsize,
        events: AtomicUsize,
        presence: AtomicUsize,
    }

    async fn mock_remote_snapshot(State(state): State<Arc<RemoteMockState>>) -> impl IntoResponse {
        state.snapshots.fetch_add(1, Ordering::SeqCst);
        axum::Json(json!({"tasks": []}))
    }

    async fn mock_remote_events(
        State(state): State<Arc<RemoteMockState>>,
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
            "data: {\"type\":\"noop\"}\n\n",
        )
    }

    async fn mock_presence(State(state): State<Arc<RemoteMockState>>) -> impl IntoResponse {
        state.presence.fetch_add(1, Ordering::SeqCst);
        StatusCode::NO_CONTENT
    }

    #[tokio::test]
    async fn worker_reconnects_sse_and_renews_presence_after_disconnect() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind remote mock");
        let address = listener.local_addr().expect("remote mock address");
        let mock = Arc::new(RemoteMockState::default());
        let app = Router::new()
            .route("/api/v1/tasks/remote", get(mock_remote_snapshot))
            .route("/api/v1/tasks/events", get(mock_remote_events))
            .route("/api/v1/tasks/presence", post(mock_presence))
            .with_state(mock.clone());
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let dir = std::env::temp_dir().join(format!(
            "fluxdown_remote_worker_{}_{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let store = Arc::new(
            StateStore::open(dir.clone())
                .await
                .expect("remote state store"),
        );
        let initial = AgentState {
            device_id: "device-1".to_owned(),
            credentials: Some(CloudCredentials {
                access_token: "access".to_owned(),
                refresh_token: "refresh".to_owned(),
                expires_at_unix: i64::MAX,
                session: None,
            }),
            ..Default::default()
        };
        store.save(&initial).await.expect("save remote state");
        let state = Arc::new(tokio::sync::Mutex::new(initial));
        let cloud = crate::cloud::CloudApi::new(
            crate::cloud::CloudClient::new(
                format!("http://{address}"),
                state.clone(),
                store.clone(),
            )
            .expect("remote cloud client"),
        );
        let service = Arc::new(RemoteTaskService::new(
            cloud,
            Arc::new(crate::daemon_client::DaemonClient::disconnected()),
            crate::event_hub::AgentEventHub::new(fluxdown_protocol::AgentSnapshot::default()),
            state,
            store.clone(),
        ));
        let cancel = CancellationToken::new();
        let worker = tokio::spawn(service.run(cancel.clone()));
        tokio::time::timeout(std::time::Duration::from_secs(8), async {
            while mock.snapshots.load(Ordering::SeqCst) < 2
                || mock.events.load(Ordering::SeqCst) < 2
                || mock.presence.load(Ordering::SeqCst) < 2
            {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("remote worker reconnected SSE and heartbeat");
        cancel.cancel();
        worker.await.expect("join remote worker");
        drop(store);
        let _ = tokio::fs::remove_dir_all(dir).await;
    }
}
