use std::sync::Arc;

use fluxdown_ui_downloads::{DownloadsCommand, DownloadsPort, DownloadsResult, PortFuture};
use serde_json::{Value, json};

use crate::agent_client::AgentClient;

pub struct AgentDownloadsPort {
    client: Arc<AgentClient>,
}

impl AgentDownloadsPort {
    #[must_use]
    pub fn new(client: Arc<AgentClient>) -> Self {
        Self { client }
    }
}

impl DownloadsPort for AgentDownloadsPort {
    fn execute(&self, command: DownloadsCommand) -> PortFuture<DownloadsResult> {
        let client = self.client.clone();
        Box::pin(async move {
            let (method, params) = match command {
                DownloadsCommand::Create(params) => (
                    fluxdown_protocol::method::DAEMON_TASK_CREATE.to_owned(),
                    serialize(params)?,
                ),
                DownloadsCommand::Pause { task_id } => (
                    fluxdown_protocol::method::DAEMON_TASK_PAUSE.to_owned(),
                    json!({ "taskId": task_id }),
                ),
                DownloadsCommand::Resume { task_id } => (
                    fluxdown_protocol::method::DAEMON_TASK_RESUME.to_owned(),
                    json!({ "taskId": task_id }),
                ),
                DownloadsCommand::Rename { task_id, file_name } => (
                    fluxdown_protocol::method::DAEMON_TASK_RENAME.to_owned(),
                    json!({ "taskId": task_id, "fileName": file_name }),
                ),
                DownloadsCommand::Delete {
                    task_id,
                    delete_files,
                } => (
                    fluxdown_protocol::method::DAEMON_TASK_DELETE.to_owned(),
                    json!({ "taskId": task_id, "deleteFiles": delete_files }),
                ),
                DownloadsCommand::PauseAll => (
                    fluxdown_protocol::method::DAEMON_TASK_PAUSE_ALL.to_owned(),
                    json!({}),
                ),
                DownloadsCommand::ResumeAll => (
                    fluxdown_protocol::method::DAEMON_TASK_RESUME_ALL.to_owned(),
                    json!({}),
                ),
                DownloadsCommand::Queue { method, params }
                | DownloadsCommand::Group { method, params } => (method.to_owned(), params),
                DownloadsCommand::ResolveSelection(params) => (
                    fluxdown_protocol::method::DAEMON_SELECTION_RESOLVE.to_owned(),
                    serialize(params)?,
                ),
                DownloadsCommand::RemoteDispatch(params) => (
                    fluxdown_protocol::method::AGENT_REMOTE_DISPATCH.to_owned(),
                    params,
                ),
                DownloadsCommand::RemoteCommand(params) => (
                    fluxdown_protocol::method::AGENT_REMOTE_COMMAND.to_owned(),
                    params,
                ),
                DownloadsCommand::OpenTask { task_id } => (
                    fluxdown_protocol::method::AGENT_PLATFORM_OPEN_TASK.to_owned(),
                    json!({ "taskId": task_id }),
                ),
                DownloadsCommand::RevealTask { task_id } => (
                    fluxdown_protocol::method::AGENT_PLATFORM_REVEAL_TASK.to_owned(),
                    json!({ "taskId": task_id }),
                ),
                DownloadsCommand::SubmitTorrentFile { path } => (
                    fluxdown_protocol::method::AGENT_CAPTURE_SUBMIT_TORRENT_FILE.to_owned(),
                    json!({ "path": path, "silent": true }),
                ),
                DownloadsCommand::RememberSaveDir { save_dir } => (
                    fluxdown_protocol::method::AGENT_PREFERENCES_PATCH.to_owned(),
                    json!({
                        "values": { fluxdown_ui_downloads::LAST_SAVE_DIR_PREF: save_dir },
                        "sync": false,
                    }),
                ),
            };
            let value: Value = client.call(&method, Some(params)).await?;
            Ok(if value == json!({ "ok": true }) {
                DownloadsResult::Unit
            } else {
                DownloadsResult::Value(value)
            })
        })
    }
}

fn serialize<T: serde::Serialize>(value: T) -> Result<Value, fluxdown_protocol::RpcErrorData> {
    serde_json::to_value(value).map_err(|_| {
        fluxdown_protocol::RpcErrorData::new(
            fluxdown_protocol::ApplicationErrorCode::Internal,
            false,
        )
    })
}
