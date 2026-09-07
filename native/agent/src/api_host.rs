//! engine-neutral `ApiHost`：读取 agent 投影并把下载写操作转发给 daemon。

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use fluxdown_api::service::{ApiError, ApiHost};
use fluxdown_protocol::method;
use fluxdown_protocol::{
    CreateTaskRequest, DaemonConfigPatch, DaemonCreateTaskParams, DownloadRequest, QueueDto,
    TaskDto,
};
use serde_json::{Value, json};

use crate::capture::CaptureService;
use crate::daemon_client::DaemonClient;
use crate::event_hub::AgentEventHub;

pub struct AgentApiHost {
    daemon: Arc<DaemonClient>,
    events: AgentEventHub,
    capture: Arc<CaptureService>,
}

impl AgentApiHost {
    #[must_use]
    pub fn new(
        daemon: Arc<DaemonClient>,
        events: AgentEventHub,
        capture: Arc<CaptureService>,
    ) -> Self {
        Self {
            daemon,
            events,
            capture,
        }
    }

    async fn unit(&self, method_name: &str, params: Value) -> Result<(), ApiError> {
        let _: Value = self
            .daemon
            .call(method_name, Some(params))
            .await
            .map_err(api_error)?;
        Ok(())
    }

    fn daemon_snapshot(&self) -> fluxdown_protocol::DaemonSnapshot {
        let snapshot = self.events.snapshot();
        match snapshot.body {
            fluxdown_protocol::SnapshotBody::Agent(agent) => agent.daemon,
            fluxdown_protocol::SnapshotBody::Daemon(_) => {
                unreachable!("agent event hub returned daemon root snapshot")
            }
        }
    }
}

#[async_trait]
impl ApiHost for AgentApiHost {
    async fn list_tasks(&self) -> Result<Vec<TaskDto>, ApiError> {
        Ok(self.daemon_snapshot().tasks)
    }

    async fn get_task(&self, task_id: &str) -> Result<Option<TaskDto>, ApiError> {
        Ok(self
            .daemon_snapshot()
            .tasks
            .into_iter()
            .find(|task| task.task_id == task_id))
    }

    async fn create_task(&self, request: CreateTaskRequest) -> Result<String, ApiError> {
        let result: serde_json::Value = self
            .daemon
            .call(
                method::DAEMON_TASK_CREATE,
                Some(DaemonCreateTaskParams {
                    request,
                    torrent_blob_id: None,
                    unattended: false,
                }),
            )
            .await
            .map_err(api_error)?;
        result
            .get("taskId")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| ApiError::Internal("daemon returned no taskId".to_owned()))
    }

    async fn delete_task(&self, task_id: &str, delete_files: bool) -> Result<(), ApiError> {
        self.unit(
            method::DAEMON_TASK_DELETE,
            json!({ "taskId": task_id, "deleteFiles": delete_files }),
        )
        .await
    }

    async fn pause_task(&self, task_id: &str) -> Result<(), ApiError> {
        self.unit(method::DAEMON_TASK_PAUSE, json!({ "taskId": task_id }))
            .await
    }

    async fn continue_task(&self, task_id: &str) -> Result<(), ApiError> {
        self.unit(method::DAEMON_TASK_RESUME, json!({ "taskId": task_id }))
            .await
    }

    async fn rename_task(&self, task_id: &str, file_name: &str) -> Result<(), ApiError> {
        self.unit(
            method::DAEMON_TASK_RENAME,
            json!({ "taskId": task_id, "fileName": file_name }),
        )
        .await
    }

    async fn pause_all(&self) -> Result<(), ApiError> {
        self.unit(method::DAEMON_TASK_PAUSE_ALL, json!({})).await
    }

    async fn continue_all(&self) -> Result<(), ApiError> {
        self.unit(method::DAEMON_TASK_RESUME_ALL, json!({})).await
    }

    async fn list_queues(&self) -> Result<Vec<QueueDto>, ApiError> {
        Ok(self.daemon_snapshot().queues)
    }

    async fn submit_external(&self, request: DownloadRequest) -> Result<(), ApiError> {
        self.capture
            .submit(request, false)
            .await
            .map(|_| ())
            .map_err(|error| ApiError::Internal(error.to_string()))
    }

    async fn get_config(&self) -> Result<HashMap<String, String>, ApiError> {
        Ok(self.daemon_snapshot().config.values.into_iter().collect())
    }

    async fn apply_config(&self, changes: HashMap<String, String>) -> Result<(), ApiError> {
        let snapshot = self.daemon_snapshot().config;
        let _: Value = self
            .daemon
            .call(
                method::DAEMON_CONFIG_PATCH,
                Some(DaemonConfigPatch {
                    expected_revision: snapshot.revision,
                    values: changes.into_iter().collect(),
                }),
            )
            .await
            .map_err(api_error)?;
        Ok(())
    }
}

fn api_error(error: fluxdown_protocol::RpcErrorData) -> ApiError {
    match error.code {
        fluxdown_protocol::ApplicationErrorCode::Unauthorized => ApiError::Unauthorized,
        fluxdown_protocol::ApplicationErrorCode::NotFound => ApiError::NotFound,
        fluxdown_protocol::ApplicationErrorCode::Conflict => {
            ApiError::Conflict("conflict".to_owned())
        }
        fluxdown_protocol::ApplicationErrorCode::InvalidArgument => {
            ApiError::BadRequest("invalid argument".to_owned())
        }
        fluxdown_protocol::ApplicationErrorCode::Unavailable
        | fluxdown_protocol::ApplicationErrorCode::Timeout => ApiError::Unavailable,
        _ => ApiError::Internal("daemon RPC failed".to_owned()),
    }
}
