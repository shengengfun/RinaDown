//! daemon 物化投影、事件游标与广播的单一同步边界。

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use fluxdown_protocol::{
    DaemonEvent, DaemonSnapshot, EventFrame, ServiceEvent, Snapshot, SnapshotBody, WsServerMsg,
};
use tokio::sync::broadcast;
use uuid::Uuid;

#[derive(Debug)]
struct EventState {
    epoch: String,
    sequence: u64,
    snapshot: DaemonSnapshot,
    download_speeds: HashMap<String, i64>,
    upload_speeds: HashMap<String, i64>,
}

/// 线性化 daemon 快照与增量事件。
#[derive(Clone)]
pub struct DaemonEventHub {
    state: Arc<Mutex<EventState>>,
    events: broadcast::Sender<EventFrame>,
}

impl DaemonEventHub {
    /// 创建新的事件 epoch。
    #[must_use]
    pub fn new(snapshot: DaemonSnapshot, capacity: usize) -> Self {
        let (events, _) = broadcast::channel(capacity);
        Self {
            state: Arc::new(Mutex::new(EventState {
                epoch: Uuid::new_v4().to_string(),
                sequence: 0,
                snapshot,
                download_speeds: HashMap::new(),
                upload_speeds: HashMap::new(),
            })),
            events,
        }
    }

    /// 先订阅广播，再原子克隆快照；调用方丢弃不大于快照 sequence 的帧。
    #[must_use]
    pub fn subscribe_and_snapshot(&self) -> (broadcast::Receiver<EventFrame>, Snapshot) {
        let receiver = self.events.subscribe();
        let state = lock_or_recover(&self.state);
        let snapshot = Snapshot {
            epoch: state.epoch.clone(),
            sequence: state.sequence,
            body: SnapshotBody::Daemon(Box::new(state.snapshot.clone())),
        };
        (receiver, snapshot)
    }

    /// 原子读取当前快照与游标。
    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        let state = lock_or_recover(&self.state);
        Snapshot {
            epoch: state.epoch.clone(),
            sequence: state.sequence,
            body: SnapshotBody::Daemon(Box::new(state.snapshot.clone())),
        }
    }

    /// 先更新物化投影，再递增 sequence 并广播对应帧。
    pub fn publish(&self, event: DaemonEvent) -> EventFrame {
        let frame = {
            let mut state = lock_or_recover(&self.state);
            apply_event(&mut state.snapshot, &event);
            apply_runtime_stats(&mut state, &event);
            state.sequence = state.sequence.saturating_add(1);
            EventFrame {
                epoch: state.epoch.clone(),
                sequence: state.sequence,
                event: ServiceEvent::Daemon(event),
            }
        };
        let _ = self.events.send(frame.clone());
        frame
    }

    /// 原子替换投影并发布替换事件。
    pub fn replace_snapshot(&self, snapshot: DaemonSnapshot) -> EventFrame {
        self.publish(DaemonEvent::SnapshotReplaced(snapshot))
    }
}

fn apply_event(snapshot: &mut DaemonSnapshot, event: &DaemonEvent) {
    match event {
        DaemonEvent::SnapshotReplaced(replacement) => *snapshot = replacement.clone(),
        DaemonEvent::Engine(message) => apply_engine_message(snapshot, message),
        DaemonEvent::TaskChanged(task) => {
            if let Some(existing) = snapshot
                .tasks
                .iter_mut()
                .find(|existing| existing.task_id == task.task_id)
            {
                *existing = task.clone();
            } else {
                snapshot.tasks.push(task.clone());
            }
        }
        DaemonEvent::TaskDeleted { task_id } => {
            snapshot.tasks.retain(|task| task.task_id != *task_id);
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
            snapshot.webhook_deliveries.clone_from(deliveries);
        }
        DaemonEvent::RuntimeStatsChanged(stats) => snapshot.runtime_stats.clone_from(stats),
        DaemonEvent::SelectionPending(request) => {
            snapshot
                .pending_selections
                .retain(|pending| pending.request_id != request.request_id);
            snapshot.pending_selections.push(request.clone());
        }
        DaemonEvent::SelectionResolved { request_id } => {
            snapshot
                .pending_selections
                .retain(|pending| pending.request_id != *request_id);
        }
    }
}

fn apply_runtime_stats(state: &mut EventState, event: &DaemonEvent) {
    match event {
        DaemonEvent::SnapshotReplaced(_) => {
            state.download_speeds.clear();
            state.upload_speeds.clear();
        }
        DaemonEvent::TaskDeleted { task_id } => {
            state.download_speeds.remove(task_id);
            state.upload_speeds.remove(task_id);
        }
        DaemonEvent::TaskChanged(task) if !matches!(task.status, 1 | 5) => {
            state.download_speeds.remove(&task.task_id);
            state.upload_speeds.remove(&task.task_id);
        }
        DaemonEvent::Engine(WsServerMsg::TasksSnapshot { tasks }) => {
            state.download_speeds.retain(|task_id, _| {
                tasks
                    .iter()
                    .any(|task| task.task_id == *task_id && matches!(task.status, 1 | 5))
            });
            state.upload_speeds.retain(|task_id, _| {
                tasks
                    .iter()
                    .any(|task| task.task_id == *task_id && matches!(task.status, 1 | 5))
            });
        }
        DaemonEvent::Engine(WsServerMsg::TaskProgress {
            task_id,
            status,
            speed,
            upload_speed,
            ..
        }) => {
            if matches!(status, 1 | 5) {
                state
                    .download_speeds
                    .insert(task_id.clone(), (*speed).max(0));
                state
                    .upload_speeds
                    .insert(task_id.clone(), (*upload_speed).max(0));
            } else {
                state.download_speeds.remove(task_id);
                state.upload_speeds.remove(task_id);
            }
        }
        _ => {}
    }
    state.snapshot.runtime_stats.active_tasks = u32::try_from(
        state
            .snapshot
            .tasks
            .iter()
            .filter(|task| matches!(task.status, 1 | 5))
            .count(),
    )
    .unwrap_or(u32::MAX);
    state.snapshot.runtime_stats.pending_tasks = u32::try_from(
        state
            .snapshot
            .tasks
            .iter()
            .filter(|task| task.status == 0)
            .count(),
    )
    .unwrap_or(u32::MAX);
    state.snapshot.runtime_stats.total_download_bps = state
        .download_speeds
        .values()
        .fold(0_i64, |total, speed| total.saturating_add(*speed));
    state.snapshot.runtime_stats.total_upload_bps = state
        .upload_speeds
        .values()
        .fold(0_i64, |total, speed| total.saturating_add(*speed));
    if let Some(save_dir) = state
        .snapshot
        .config
        .values
        .get("default_save_dir")
        .filter(|path| !path.trim().is_empty())
    {
        state.snapshot.runtime_stats.save_dir.clone_from(save_dir);
    }
}

fn apply_engine_message(snapshot: &mut DaemonSnapshot, message: &WsServerMsg) {
    match message {
        WsServerMsg::TasksSnapshot { tasks } => snapshot.tasks.clone_from(tasks),
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
                return;
            }
            if let Some(task) = snapshot
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
        WsServerMsg::QueuesChanged { queues } => snapshot.queues.clone_from(queues),
        WsServerMsg::QueuePositionsChanged { positions } => {
            snapshot.queue_positions.clone_from(positions);
        }
        WsServerMsg::GroupsChanged { groups } => snapshot.groups.clone_from(groups),
        WsServerMsg::RssSourcesChanged { sources } => snapshot.rss_sources.clone_from(sources),
        WsServerMsg::RssItemsChanged { source_id, .. } => {
            let revision = snapshot
                .rss_item_revisions
                .entry(source_id.clone())
                .or_default();
            *revision = revision.saturating_add(1);
        }
        WsServerMsg::WebhookDeliveriesChanged { deliveries } => {
            snapshot.webhook_deliveries.clone_from(deliveries);
        }
        WsServerMsg::FileMissingChanged { updates } => {
            for update in updates {
                if let Some(task) = snapshot
                    .tasks
                    .iter_mut()
                    .find(|task| task.task_id == update.task_id)
                {
                    task.file_missing = update.missing;
                }
            }
        }
        WsServerMsg::PriorityTaskChanged {
            priority_task_id, ..
        } => {
            snapshot.priority.clear();
            if !priority_task_id.is_empty() {
                snapshot.priority.push(priority_task_id.clone());
            }
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
        _ => {}
    }
}

/// 将引擎事件无阻塞转换并发布到 daemon 事件中心。
pub struct DaemonEngineEventSink(pub DaemonEventHub);

impl fluxdown_engine::events::EventSink for DaemonEngineEventSink {
    fn emit(&self, event: fluxdown_engine::events::EngineEvent) {
        use fluxdown_engine::events::EngineEvent;

        let message = match event {
            EngineEvent::TaskProgress {
                task_id,
                status,
                downloaded_bytes,
                total_bytes,
                speed,
                file_name,
                save_dir,
                url,
                error_message,
                upload_speed_bps,
                uploaded_bytes,
                seeding_status,
                seeding_message,
                seeding_time_secs,
                ..
            } => WsServerMsg::TaskProgress {
                task_id,
                status,
                downloaded_bytes,
                total_bytes,
                speed,
                upload_speed: upload_speed_bps,
                file_name,
                save_dir,
                url,
                error_message,
                uploaded_bytes,
                seeding_status,
                seeding_message,
                seeding_time_secs,
            },
            EngineEvent::TasksSnapshot(tasks) => WsServerMsg::TasksSnapshot {
                tasks: tasks
                    .into_iter()
                    .map(fluxdown_engine_protocol::task_info_to_dto)
                    .collect(),
            },
            EngineEvent::SegmentProgress {
                task_id,
                total_bytes,
                segment_count,
                segments,
            } => WsServerMsg::SegmentProgress {
                task_id,
                total_bytes,
                segment_count,
                segments: segments
                    .into_iter()
                    .map(fluxdown_engine_protocol::segment_detail_to_dto)
                    .collect(),
            },
            EngineEvent::TaskMetaProbed {
                task_id,
                file_name,
                total_bytes,
            } => WsServerMsg::TaskMetaProbed {
                task_id,
                file_name,
                total_bytes,
            },
            EngineEvent::QueuePositionsChanged(positions) => WsServerMsg::QueuePositionsChanged {
                positions: positions
                    .into_iter()
                    .map(fluxdown_engine_protocol::queue_position_to_dto)
                    .collect(),
            },
            EngineEvent::QueuesChanged(queues) => WsServerMsg::QueuesChanged {
                queues: queues
                    .into_iter()
                    .map(fluxdown_engine_protocol::queue_info_to_dto)
                    .collect(),
            },
            EngineEvent::TaskQueueChanged { task_id, queue_id } => {
                WsServerMsg::TaskQueueChanged { task_id, queue_id }
            }
            EngineEvent::TaskRouteChanged { task_id, route } => {
                WsServerMsg::TaskRouteChanged { task_id, route }
            }
            EngineEvent::PriorityTaskChanged {
                priority_task_id,
                auto_paused_count,
            } => WsServerMsg::PriorityTaskChanged {
                priority_task_id,
                auto_paused_count,
            },
            EngineEvent::SegmentSplit {
                task_id,
                parent_index,
                parent_new_end,
                child_index,
                child_start,
                child_end,
                is_proactive,
                total_segments,
            } => WsServerMsg::SegmentSplit {
                task_id,
                parent_index,
                parent_new_end,
                child_index,
                child_start,
                child_end,
                is_proactive,
                total_segments,
            },
            EngineEvent::TaskCdnEvent {
                task_id,
                kind,
                host,
                nodes,
                ip,
                reason,
                candidates,
                alive,
                cap,
                auto_cap,
            } => WsServerMsg::TaskCdnEvent {
                task_id,
                kind,
                host,
                nodes: nodes
                    .into_iter()
                    .map(fluxdown_engine_protocol::cdn_node_info_to_dto)
                    .collect(),
                ip,
                reason,
                candidates,
                alive,
                cap,
                auto_cap,
            },
            EngineEvent::BtDataFinished { .. } => return,
            EngineEvent::PluginAutoDisabled { identity, reason } => {
                WsServerMsg::PluginAutoDisabled { identity, reason }
            }
            EngineEvent::DuplicateTorrentDetected {
                task_id,
                existing_task_id,
                existing_name,
            } => WsServerMsg::DuplicateTorrent {
                task_id,
                existing_task_id,
                existing_name,
            },
            EngineEvent::PluginHookActivity {
                task_id,
                plugin_id,
                running,
            } => WsServerMsg::PluginHookActivity {
                task_id,
                plugin_id,
                running,
            },
            EngineEvent::GroupsChanged(groups) => WsServerMsg::GroupsChanged {
                groups: groups
                    .into_iter()
                    .map(fluxdown_engine_protocol::group_info_to_dto)
                    .collect(),
            },
            EngineEvent::RssSourcesChanged(sources) => WsServerMsg::RssSourcesChanged {
                sources: sources
                    .into_iter()
                    .map(fluxdown_engine_protocol::rss_source_info_to_dto)
                    .collect(),
            },
            EngineEvent::RssItemsChanged {
                source_id,
                items,
                notify_titles,
            } => WsServerMsg::RssItemsChanged {
                source_id,
                items: items
                    .into_iter()
                    .map(fluxdown_engine_protocol::rss_item_info_to_dto)
                    .collect(),
                notify_titles,
            },
            EngineEvent::RssFeedValidated {
                request_id,
                url,
                feed_title,
                items,
                error,
            } => WsServerMsg::RssFeedValidated {
                request_id,
                url,
                feed_title,
                items: items
                    .into_iter()
                    .map(fluxdown_engine_protocol::rss_item_info_to_dto)
                    .collect(),
                error,
            },
            EngineEvent::WebhookDeliveriesChanged(deliveries) => {
                WsServerMsg::WebhookDeliveriesChanged {
                    deliveries: deliveries
                        .into_iter()
                        .map(fluxdown_engine_protocol::webhook_delivery_to_dto)
                        .collect(),
                }
            }
            EngineEvent::FileMissingChanged(updates) => WsServerMsg::FileMissingChanged {
                updates: updates
                    .into_iter()
                    .map(
                        |(task_id, missing)| fluxdown_protocol::FileMissingUpdateDto {
                            task_id,
                            missing,
                        },
                    )
                    .collect(),
            },
            other => {
                tracing::debug!(?other, "daemon ignored unknown engine event");
                return;
            }
        };
        let updates_runtime = matches!(
            message,
            WsServerMsg::TaskProgress { .. } | WsServerMsg::TasksSnapshot { .. }
        );
        self.0.publish(DaemonEvent::Engine(message));
        if updates_runtime && let SnapshotBody::Daemon(snapshot) = self.0.snapshot().body {
            self.0
                .publish(DaemonEvent::RuntimeStatsChanged(snapshot.runtime_stats));
        }
    }
}

fn lock_or_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use fluxdown_protocol::{DaemonEvent, DaemonSnapshot, SnapshotBody, TaskDto, WsServerMsg};

    use super::DaemonEventHub;

    #[test]
    fn subscribe_snapshot_cursor_discards_prior_frames() {
        let hub = DaemonEventHub::new(DaemonSnapshot::default(), 8);
        let first = hub.publish(DaemonEvent::TaskDeleted {
            task_id: "old".to_owned(),
        });
        let (_receiver, snapshot) = hub.subscribe_and_snapshot();
        assert_eq!(snapshot.sequence, first.sequence);
        assert!(matches!(snapshot.body, SnapshotBody::Daemon(_)));
        let second = hub.publish(DaemonEvent::TaskDeleted {
            task_id: "new".to_owned(),
        });
        assert_eq!(second.sequence, snapshot.sequence + 1);
        assert_eq!(second.epoch, snapshot.epoch);
    }

    #[test]
    fn delete_sentinel_removes_task_from_atomic_projection() -> Result<(), serde_json::Error> {
        let task = serde_json::from_value::<TaskDto>(serde_json::json!({
            "taskId": "task-1",
            "url": "https://example.com/file",
            "fileName": "file",
            "saveDir": "/tmp",
            "status": 1,
            "downloadedBytes": 10,
            "totalBytes": 100,
            "errorMessage": "",
            "createdAt": "1",
            "proxyUrl": "",
            "queueId": "main",
            "checksum": ""
        }))?;
        let hub = DaemonEventHub::new(
            DaemonSnapshot {
                tasks: vec![task],
                ..DaemonSnapshot::default()
            },
            8,
        );
        let frame = hub.publish(DaemonEvent::Engine(WsServerMsg::TaskProgress {
            task_id: "task-1".to_owned(),
            status: 4,
            downloaded_bytes: 10,
            total_bytes: 100,
            speed: 0,
            file_name: "file".to_owned(),
            save_dir: "/tmp".to_owned(),
            upload_speed: 0,
            url: "https://example.com/file".to_owned(),
            error_message: "deleted".to_owned(),
            uploaded_bytes: 0,
            seeding_status: 0,
            seeding_message: String::new(),
            seeding_time_secs: 0,
        }));
        assert_eq!(frame.sequence, 1);
        let snapshot = hub.snapshot();
        let SnapshotBody::Daemon(snapshot) = snapshot.body else {
            panic!("daemon hub returned agent snapshot");
        };
        assert!(snapshot.tasks.is_empty());
        Ok(())
    }

    #[test]
    fn runtime_stats_follow_live_task_speed_and_terminal_transitions()
    -> Result<(), serde_json::Error> {
        let task = serde_json::from_value::<TaskDto>(serde_json::json!({
            "taskId": "task-live",
            "url": "https://example.com/live",
            "fileName": "live",
            "saveDir": "/tmp",
            "status": 1,
            "downloadedBytes": 10,
            "totalBytes": 100,
            "errorMessage": "",
            "createdAt": "1",
            "proxyUrl": "",
            "queueId": "main",
            "checksum": ""
        }))?;
        let hub = DaemonEventHub::new(
            DaemonSnapshot {
                tasks: vec![task],
                ..DaemonSnapshot::default()
            },
            8,
        );
        let progress = |status, speed, upload_speed| {
            DaemonEvent::Engine(WsServerMsg::TaskProgress {
                task_id: "task-live".to_owned(),
                status,
                downloaded_bytes: 10,
                total_bytes: 100,
                speed,
                upload_speed,
                file_name: "live".to_owned(),
                save_dir: "/tmp".to_owned(),
                url: "https://example.com/live".to_owned(),
                error_message: String::new(),
                uploaded_bytes: 0,
                seeding_status: 0,
                seeding_message: String::new(),
                seeding_time_secs: 0,
            })
        };
        hub.publish(progress(1, 100, 20));
        let SnapshotBody::Daemon(snapshot) = hub.snapshot().body else {
            panic!("daemon hub returned agent snapshot");
        };
        assert_eq!(snapshot.runtime_stats.active_tasks, 1);
        assert_eq!(snapshot.runtime_stats.total_download_bps, 100);
        assert_eq!(snapshot.runtime_stats.total_upload_bps, 20);

        hub.publish(progress(2, 0, 0));
        let SnapshotBody::Daemon(snapshot) = hub.snapshot().body else {
            panic!("daemon hub returned agent snapshot");
        };
        assert_eq!(snapshot.runtime_stats.active_tasks, 0);
        assert_eq!(snapshot.runtime_stats.total_download_bps, 0);
        assert_eq!(snapshot.runtime_stats.total_upload_bps, 0);
        Ok(())
    }
}
