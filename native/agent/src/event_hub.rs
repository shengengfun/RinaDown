//! agent 自有状态与 daemon 缓存的原子投影和事件序列。

use std::sync::{Arc, Mutex, MutexGuard};

use fluxdown_protocol::{
    AgentEvent, AgentSnapshot, DaemonEvent, DaemonSnapshot, EventFrame, ServiceEvent, Snapshot,
    SnapshotBody, WsServerMsg,
};
use tokio::sync::broadcast;
use uuid::Uuid;

struct AgentEventState {
    epoch: String,
    sequence: u64,
    snapshot: AgentSnapshot,
}

/// agent 物化投影的唯一同步边界。
#[derive(Clone)]
pub struct AgentEventHub {
    state: Arc<Mutex<AgentEventState>>,
    events: broadcast::Sender<EventFrame>,
}

impl AgentEventHub {
    #[must_use]
    pub fn new(snapshot: AgentSnapshot) -> Self {
        let (events, _) = broadcast::channel(1024);
        Self {
            state: Arc::new(Mutex::new(AgentEventState {
                epoch: Uuid::new_v4().to_string(),
                sequence: 0,
                snapshot,
            })),
            events,
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        let state = lock_or_recover(&self.state);
        Snapshot {
            epoch: state.epoch.clone(),
            sequence: state.sequence,
            body: SnapshotBody::Agent(Box::new(state.snapshot.clone())),
        }
    }

    #[must_use]
    pub fn subscribe_and_snapshot(&self) -> (broadcast::Receiver<EventFrame>, Snapshot) {
        let receiver = self.events.subscribe();
        (receiver, self.snapshot())
    }

    /// daemon 增量先更新缓存，再发布 agent sequence。
    pub fn apply_daemon_event(&self, event: DaemonEvent) -> EventFrame {
        self.publish(AgentEvent::Daemon(event))
    }

    /// daemon 重连快照原子替换并发布替换事件。
    pub fn replace_daemon_snapshot(&self, snapshot: DaemonSnapshot) -> EventFrame {
        self.publish(AgentEvent::DaemonSnapshotReplaced(snapshot))
    }

    pub fn publish(&self, event: AgentEvent) -> EventFrame {
        let frame = {
            let mut state = lock_or_recover(&self.state);
            apply_event(&mut state.snapshot, &event);
            state.sequence = state.sequence.saturating_add(1);
            EventFrame {
                epoch: state.epoch.clone(),
                sequence: state.sequence,
                event: ServiceEvent::Agent(event),
            }
        };
        let _ = self.events.send(frame.clone());
        frame
    }
}

fn apply_event(snapshot: &mut AgentSnapshot, event: &AgentEvent) {
    match event {
        AgentEvent::Daemon(event) => apply_daemon_event(&mut snapshot.daemon, event),
        AgentEvent::DaemonSnapshotReplaced(daemon) => snapshot.daemon.clone_from(daemon),
        AgentEvent::DaemonConnectionChanged(connected) => snapshot.daemon_connected = *connected,
        AgentEvent::SessionChanged(session) => snapshot.session.clone_from(session.as_ref()),
        AgentEvent::SyncChanged(sync) => snapshot.sync.clone_from(sync),
        AgentEvent::PreferencesChanged(preferences) => snapshot.preferences.clone_from(preferences),
        AgentEvent::GatewayChanged(gateway) => snapshot.gateway.clone_from(gateway),
        AgentEvent::CloudDevicesChanged(devices) => snapshot.cloud_devices.clone_from(devices),
        AgentEvent::LinkedDevicesChanged(devices) => snapshot.linked_devices.clone_from(devices),
        AgentEvent::RemoteTasksChanged(tasks) => snapshot.remote_tasks.clone_from(tasks),
        AgentEvent::PendingCapturesChanged(captures) => {
            snapshot.pending_captures.clone_from(captures)
        }
    }
}

fn apply_daemon_event(snapshot: &mut DaemonSnapshot, event: &DaemonEvent) {
    match event {
        DaemonEvent::SnapshotReplaced(replacement) => snapshot.clone_from(replacement),
        DaemonEvent::Engine(message) => apply_engine_message(snapshot, message),
        DaemonEvent::TaskChanged(task) => {
            if let Some(existing) = snapshot
                .tasks
                .iter_mut()
                .find(|item| item.task_id == task.task_id)
            {
                existing.clone_from(task);
            } else {
                snapshot.tasks.push(task.clone());
            }
        }
        DaemonEvent::TaskDeleted { task_id } => {
            snapshot.tasks.retain(|task| task.task_id != *task_id)
        }
        DaemonEvent::QueuesChanged(queues) => snapshot.queues.clone_from(queues),
        DaemonEvent::GroupsChanged(groups) => snapshot.groups.clone_from(groups),
        DaemonEvent::ConfigChanged(config) => snapshot.config.clone_from(config),
        DaemonEvent::RssChanged {
            source_id,
            item_revision,
        } => {
            snapshot
                .rss_item_revisions
                .insert(source_id.clone(), *item_revision);
        }
        DaemonEvent::PluginsChanged(plugins) => snapshot.plugins.clone_from(plugins),
        DaemonEvent::ComponentsChanged(components) => snapshot.components.clone_from(components),
        DaemonEvent::WebhooksChanged(deliveries) => {
            snapshot.webhook_deliveries.clone_from(deliveries)
        }
        DaemonEvent::RuntimeStatsChanged(stats) => snapshot.runtime_stats.clone_from(stats),
        DaemonEvent::SelectionPending(request) => {
            snapshot
                .pending_selections
                .retain(|item| item.request_id != request.request_id);
            snapshot.pending_selections.push(request.clone());
        }
        DaemonEvent::SelectionResolved { request_id } => {
            snapshot
                .pending_selections
                .retain(|item| item.request_id != *request_id);
        }
    }
}

fn apply_engine_message(snapshot: &mut DaemonSnapshot, message: &WsServerMsg) {
    match message {
        WsServerMsg::TasksSnapshot { tasks } => snapshot.tasks.clone_from(tasks),
        WsServerMsg::QueuesChanged { queues } => snapshot.queues.clone_from(queues),
        WsServerMsg::QueuePositionsChanged { positions } => {
            snapshot.queue_positions.clone_from(positions)
        }
        WsServerMsg::GroupsChanged { groups } => snapshot.groups.clone_from(groups),
        WsServerMsg::RssSourcesChanged { sources } => snapshot.rss_sources.clone_from(sources),
        WsServerMsg::WebhookDeliveriesChanged { deliveries } => {
            snapshot.webhook_deliveries.clone_from(deliveries)
        }
        WsServerMsg::TaskMetaProbed {
            task_id,
            file_name,
            total_bytes,
        } => {
            if let Some(task) = snapshot
                .tasks
                .iter_mut()
                .find(|task| task.task_id == *task_id)
            {
                if !file_name.is_empty() {
                    task.file_name.clone_from(file_name);
                }
                task.total_bytes = *total_bytes;
            }
        }
        WsServerMsg::TaskQueueChanged { task_id, queue_id } => {
            if let Some(task) = snapshot
                .tasks
                .iter_mut()
                .find(|task| task.task_id == *task_id)
            {
                task.queue_id.clone_from(queue_id);
            }
        }
        WsServerMsg::TaskRouteChanged { task_id, route } => {
            if let Some(task) = snapshot
                .tasks
                .iter_mut()
                .find(|task| task.task_id == *task_id)
            {
                task.auto_route.clone_from(route);
            }
        }
        WsServerMsg::RssItemsChanged { source_id, .. } => {
            let revision = snapshot
                .rss_item_revisions
                .entry(source_id.clone())
                .or_default();
            *revision = revision.saturating_add(1);
        }
        WsServerMsg::PluginAutoDisabled { identity, reason } => {
            if let Some(plugin) = snapshot
                .plugins
                .iter_mut()
                .find(|plugin| plugin.identity == *identity)
            {
                plugin.enabled = false;
                plugin.disabled_reason.clone_from(reason);
            }
        }
        WsServerMsg::TaskProgress {
            task_id,
            status,
            downloaded_bytes,
            total_bytes,
            file_name,
            save_dir,
            url,
            error_message,
            uploaded_bytes,
            seeding_status,
            seeding_message,
            seeding_time_secs,
            ..
        } => {
            if *status == 4 && error_message == "deleted" {
                snapshot.tasks.retain(|task| task.task_id != *task_id);
            } else if let Some(task) = snapshot
                .tasks
                .iter_mut()
                .find(|task| task.task_id == *task_id)
            {
                task.status = *status;
                task.downloaded_bytes = *downloaded_bytes;
                task.total_bytes = *total_bytes;
                if !file_name.is_empty() {
                    task.file_name.clone_from(file_name);
                }
                if !save_dir.is_empty() {
                    task.save_dir.clone_from(save_dir);
                }
                if !url.is_empty() {
                    task.url.clone_from(url);
                }
                task.error_message.clone_from(error_message);
                task.uploaded_bytes = *uploaded_bytes;
                task.seeding_status = *seeding_status;
                task.seeding_message.clone_from(seeding_message);
                task.seeding_time_secs = *seeding_time_secs;
            }
        }
        _ => {}
    }
}

fn lock_or_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use fluxdown_protocol::{
        AgentSnapshot, DaemonEvent, DaemonSnapshot, SnapshotBody, TaskDto, WsServerMsg,
    };

    use super::AgentEventHub;

    #[test]
    fn daemon_snapshot_then_delta_share_agent_sequence_and_preserve_metadata()
    -> Result<(), serde_json::Error> {
        let task = serde_json::from_value::<TaskDto>(serde_json::json!({
            "taskId": "task-1",
            "url": "https://example.com/file",
            "fileName": "file.bin",
            "saveDir": "/tmp",
            "status": 0,
            "downloadedBytes": 0,
            "totalBytes": 100,
            "errorMessage": "",
            "createdAt": "1",
            "proxyUrl": "",
            "queueId": "main",
            "checksum": ""
        }))?;
        let hub = AgentEventHub::new(AgentSnapshot::default());
        let replaced = hub.replace_daemon_snapshot(DaemonSnapshot {
            tasks: vec![task],
            ..DaemonSnapshot::default()
        });
        let delta = hub.apply_daemon_event(DaemonEvent::Engine(WsServerMsg::TaskProgress {
            task_id: "task-1".to_owned(),
            status: 5,
            downloaded_bytes: 1,
            total_bytes: 100,
            speed: 0,
            file_name: String::new(),
            save_dir: String::new(),
            upload_speed: 0,
            url: String::new(),
            error_message: String::new(),
            uploaded_bytes: 0,
            seeding_status: 0,
            seeding_message: String::new(),
            seeding_time_secs: 0,
        }));
        assert_eq!(delta.sequence, replaced.sequence + 1);
        assert_eq!(delta.epoch, replaced.epoch);
        let SnapshotBody::Agent(snapshot) = hub.snapshot().body else {
            panic!("agent hub returned daemon root snapshot");
        };
        assert_eq!(snapshot.daemon.tasks[0].url, "https://example.com/file");
        assert_eq!(snapshot.daemon.tasks[0].save_dir, "/tmp");
        assert_eq!(snapshot.daemon.tasks[0].status, 5);
        Ok(())
    }
}
