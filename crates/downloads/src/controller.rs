use std::{
    collections::{BTreeMap, HashMap},
    future::Future,
    pin::Pin,
    sync::Arc,
};

use fluxdown_protocol::{
    AgentEvent, AgentSnapshot, DaemonEvent, DaemonSnapshot, QueueDto, RemoteTaskDto, ServiceEvent,
    TaskDto, WsServerMsg,
};

use crate::model::DownloadTaskView;

/// 本机偏好：新建下载对话框上次使用的保存目录（设备本地，不进云同步）。
pub const LAST_SAVE_DIR_PREF: &str = "download.last_save_dir";
/// 偏好：新建下载默认沿用上次保存目录。
pub const REMEMBER_LAST_SAVE_DIR_PREF: &str = "download.remember_last_save_dir";

pub type PortFuture<T> =
    Pin<Box<dyn Future<Output = Result<T, fluxdown_protocol::RpcErrorData>> + Send + 'static>>;

pub enum DownloadsCommand {
    Create(Box<fluxdown_protocol::DaemonCreateTaskParams>),
    Pause {
        task_id: String,
    },
    Resume {
        task_id: String,
    },
    Rename {
        task_id: String,
        file_name: String,
    },
    Delete {
        task_id: String,
        delete_files: bool,
    },
    PauseAll,
    ResumeAll,
    Queue {
        method: &'static str,
        params: serde_json::Value,
    },
    Group {
        method: &'static str,
        params: serde_json::Value,
    },
    ResolveSelection(fluxdown_protocol::SelectionResolutionDto),
    RemoteDispatch(serde_json::Value),
    RemoteCommand(serde_json::Value),
    OpenTask {
        task_id: String,
    },
    RevealTask {
        task_id: String,
    },
    /// 本机 `.torrent` 文件：agent 读取、上传 blob 后按捕获路径建任务。
    SubmitTorrentFile {
        path: String,
    },
    /// 记录本次保存目录到 [`LAST_SAVE_DIR_PREF`]（设备本地偏好）。
    RememberSaveDir {
        save_dir: String,
    },
}

pub enum DownloadsResult {
    Unit,
    Value(serde_json::Value),
}

pub trait DownloadsPort: Send + Sync {
    fn execute(&self, command: DownloadsCommand) -> PortFuture<DownloadsResult>;
}

pub struct DownloadsController {
    port: Arc<dyn DownloadsPort>,
    local: Vec<TaskDto>,
    remote: Vec<RemoteTaskDto>,
    tasks: Vec<DownloadTaskView>,
    live_speeds: HashMap<String, i64>,
    pending_selections: Vec<fluxdown_protocol::SelectionRequestDto>,
    queues: Vec<QueueDto>,
    config: BTreeMap<String, String>,
    runtime_save_dir: String,
    preferences: BTreeMap<String, serde_json::Value>,
    stale: bool,
}

impl DownloadsController {
    #[must_use]
    pub fn new(port: Arc<dyn DownloadsPort>) -> Self {
        Self {
            port,
            local: Vec::new(),
            remote: Vec::new(),
            tasks: Vec::new(),
            live_speeds: HashMap::new(),
            pending_selections: Vec::new(),
            queues: Vec::new(),
            config: BTreeMap::new(),
            runtime_save_dir: String::new(),
            preferences: BTreeMap::new(),
            stale: true,
        }
    }

    pub fn replace_snapshot(&mut self, snapshot: &AgentSnapshot) {
        self.live_speeds.clear();
        self.local.clone_from(&snapshot.daemon.tasks);
        self.remote.clone_from(&snapshot.remote_tasks);
        self.pending_selections
            .clone_from(&snapshot.daemon.pending_selections);
        self.absorb_daemon_context(&snapshot.daemon);
        self.preferences.clone_from(&snapshot.preferences.values);
        self.stale = false;
        self.rebuild();
    }

    pub fn apply_event(&mut self, event: &ServiceEvent) {
        let ServiceEvent::Agent(event) = event else {
            return;
        };
        match event {
            AgentEvent::DaemonSnapshotReplaced(snapshot) => {
                self.live_speeds.clear();
                self.local.clone_from(&snapshot.tasks);
                self.absorb_daemon_context(snapshot);
                self.stale = false;
            }
            AgentEvent::DaemonConnectionChanged(connected) => self.stale = !connected,
            AgentEvent::RemoteTasksChanged(tasks) => self.remote.clone_from(tasks),
            AgentEvent::PreferencesChanged(preferences) => {
                self.preferences.clone_from(&preferences.values);
                return;
            }
            AgentEvent::Daemon(event) => self.apply_daemon_event(event),
            _ => return,
        }
        self.rebuild();
    }

    pub fn mark_stale(&mut self) {
        self.stale = true;
    }

    #[must_use]
    pub fn is_stale(&self) -> bool {
        self.stale
    }

    #[must_use]
    pub(crate) fn pending_selection(&self) -> Option<&fluxdown_protocol::SelectionRequestDto> {
        self.pending_selections.first()
    }
    pub(crate) fn tasks(&self) -> &[DownloadTaskView] {
        &self.tasks
    }

    /// daemon 队列清单（快照顺序）。
    #[must_use]
    pub(crate) fn queues(&self) -> &[QueueDto] {
        &self.queues
    }

    /// daemon 公开配置（字符串编码）。
    #[must_use]
    pub(crate) fn config(&self) -> &BTreeMap<String, String> {
        &self.config
    }

    /// 配置值（缺省为空串），已去首尾空白。
    #[must_use]
    pub(crate) fn config_str(&self, key: &str) -> &str {
        self.config.get(key).map_or("", |value| value.trim())
    }

    /// 当前生效的保存目录：配置 `default_save_dir`，为空时回退 daemon 运行时目录。
    #[must_use]
    pub(crate) fn effective_save_dir(&self) -> &str {
        match self.config_str("default_save_dir") {
            "" => &self.runtime_save_dir,
            configured => configured,
        }
    }

    /// agent 偏好值。
    #[must_use]
    pub(crate) fn preference(&self, key: &str) -> Option<&serde_json::Value> {
        self.preferences.get(key)
    }

    pub fn execute(&self, command: DownloadsCommand) -> PortFuture<DownloadsResult> {
        if self.stale {
            return Box::pin(async {
                Err(fluxdown_protocol::RpcErrorData::new(
                    fluxdown_protocol::ApplicationErrorCode::Unavailable,
                    true,
                ))
            });
        }
        self.port.execute(command)
    }

    fn absorb_daemon_context(&mut self, snapshot: &DaemonSnapshot) {
        self.queues.clone_from(&snapshot.queues);
        self.config.clone_from(&snapshot.config.values);
        self.runtime_save_dir
            .clone_from(&snapshot.runtime_stats.save_dir);
    }

    fn apply_daemon_event(&mut self, event: &DaemonEvent) {
        match event {
            DaemonEvent::SnapshotReplaced(snapshot) => {
                self.live_speeds.clear();
                self.local.clone_from(&snapshot.tasks);
                self.absorb_daemon_context(snapshot);
            }
            DaemonEvent::QueuesChanged(queues) => self.queues.clone_from(queues),
            DaemonEvent::ConfigChanged(config) => self.config.clone_from(&config.values),
            DaemonEvent::RuntimeStatsChanged(stats) => {
                self.runtime_save_dir.clone_from(&stats.save_dir);
            }
            DaemonEvent::TaskChanged(task) => upsert(&mut self.local, task.clone()),
            DaemonEvent::TaskDeleted { task_id } => {
                self.local.retain(|task| task.task_id != *task_id);
                self.live_speeds.remove(task_id);
            }
            DaemonEvent::Engine(WsServerMsg::TasksSnapshot { tasks }) => {
                self.local.clone_from(tasks);
                self.live_speeds
                    .retain(|task_id, _| tasks.iter().any(|task| task.task_id == *task_id));
            }
            DaemonEvent::Engine(WsServerMsg::TaskProgress {
                task_id,
                status,
                downloaded_bytes,
                total_bytes,
                file_name,
                speed,
                ..
            }) => {
                self.live_speeds
                    .insert(task_id.clone(), if *status == 1 { *speed } else { 0 });
                if let Some(task) = self.local.iter_mut().find(|task| task.task_id == *task_id) {
                    task.status = *status;
                    task.downloaded_bytes = *downloaded_bytes;
                    task.total_bytes = *total_bytes;
                    if !file_name.is_empty() {
                        task.file_name.clone_from(file_name);
                    }
                }
            }
            DaemonEvent::Engine(WsServerMsg::TaskMetaProbed {
                task_id,
                file_name,
                total_bytes,
            }) => {
                if let Some(task) = self.local.iter_mut().find(|task| task.task_id == *task_id) {
                    if !file_name.is_empty() {
                        task.file_name.clone_from(file_name);
                    }
                    task.total_bytes = *total_bytes;
                }
            }
            DaemonEvent::SelectionPending(request) => {
                self.pending_selections
                    .retain(|pending| pending.request_id != request.request_id);
                self.pending_selections.push(request.clone());
            }
            DaemonEvent::SelectionResolved { request_id } => {
                self.pending_selections
                    .retain(|pending| pending.request_id != *request_id);
            }
            _ => {}
        }
    }

    fn rebuild(&mut self) {
        self.tasks = self
            .local
            .iter()
            .map(|task| {
                DownloadTaskView::local_with_speed(
                    task,
                    self.live_speeds.get(&task.task_id).copied(),
                )
            })
            .chain(self.remote.iter().map(DownloadTaskView::remote))
            .collect();
    }
}

fn upsert(tasks: &mut Vec<TaskDto>, task: TaskDto) {
    if let Some(existing) = tasks
        .iter_mut()
        .find(|existing| existing.task_id == task.task_id)
    {
        *existing = task;
    } else {
        tasks.push(task);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;

    use super::{
        DaemonEvent, DownloadsCommand, DownloadsController, DownloadsPort, DownloadsResult,
        PortFuture, WsServerMsg,
    };

    struct NullPort;

    impl DownloadsPort for NullPort {
        fn execute(&self, _command: DownloadsCommand) -> PortFuture<DownloadsResult> {
            Box::pin(async { Ok(DownloadsResult::Unit) })
        }
    }

    #[test]
    fn metadata_probe_replaces_loading_row_name_and_size() {
        let task = serde_json::from_value::<fluxdown_protocol::TaskDto>(json!({
            "taskId":"task-1","url":"https://example.com/download","fileName":"",
            "saveDir":"/tmp","status":0,"downloadedBytes":0,"totalBytes":0,
            "errorMessage":"","createdAt":"1","proxyUrl":"","queueId":"main","checksum":""
        }))
        .expect("probing task");
        let mut controller = DownloadsController::new(Arc::new(NullPort));
        controller.local.push(task);
        controller.rebuild();
        assert!(controller.tasks()[0].metadata_pending);

        controller.apply_daemon_event(&DaemonEvent::Engine(WsServerMsg::TaskMetaProbed {
            task_id: "task-1".to_owned(),
            file_name: "resolved.bin".to_owned(),
            total_bytes: 4096,
        }));
        controller.rebuild();

        let task = &controller.tasks()[0];
        assert_eq!(task.name, "resolved.bin");
        assert_eq!(task.size_bytes, 4096);
        assert!(!task.metadata_pending);

        controller.apply_daemon_event(&DaemonEvent::Engine(WsServerMsg::TaskProgress {
            task_id: "task-1".to_owned(),
            status: 1,
            downloaded_bytes: 1024,
            total_bytes: 4096,
            speed: 1024,
            upload_speed: 0,
            file_name: "resolved.bin".to_owned(),
            save_dir: "/tmp".to_owned(),
            url: "https://example.com/download".to_owned(),
            error_message: String::new(),
            uploaded_bytes: 0,
            seeding_status: 0,
            seeding_message: String::new(),
            seeding_time_secs: 0,
        }));
        controller.rebuild();

        let task = &controller.tasks()[0];
        assert_eq!(task.speed_bytes_per_second, Some(1024));
        assert_eq!(task.eta_seconds, Some(3));
        assert_eq!(task.progress, 0.25);
    }
}
