//! 可重放事件帧与全量快照契约。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::agent::{
    AgentPreferencesDto, AgentSessionDto, CloudDevice, GatewayStatusDto, PendingCaptureDto,
    RemoteTaskDto, SyncStatusDto,
};
use crate::daemon::{
    ComponentStatusDto, DaemonConfigSnapshot, DaemonRuntimeStatsDto, GroupDto, LinkDeviceInfo,
    PluginDto, QueueDto, QueuePositionDto, RssSourceDto, SelectionRequestDto, TaskDto,
    WebhookDeliveryDto, WsServerMsg,
};

/// daemon 的完整物化投影。
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct DaemonSnapshot {
    pub tasks: Vec<TaskDto>,
    pub queues: Vec<QueueDto>,
    pub queue_positions: Vec<QueuePositionDto>,
    pub groups: Vec<GroupDto>,
    pub config: DaemonConfigSnapshot,
    pub rss_sources: Vec<RssSourceDto>,
    pub rss_item_revisions: BTreeMap<String, u64>,
    pub plugins: Vec<PluginDto>,
    pub components: Vec<ComponentStatusDto>,
    pub webhook_deliveries: Vec<WebhookDeliveryDto>,
    pub priority: Vec<String>,
    pub runtime_stats: DaemonRuntimeStatsDto,
    pub pending_selections: Vec<SelectionRequestDto>,
}

/// agent 的完整物化投影。下载事实只来自嵌套 daemon 快照。
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AgentSnapshot {
    pub daemon: DaemonSnapshot,
    #[serde(default)]
    pub daemon_connected: bool,
    pub session: Option<AgentSessionDto>,
    pub sync: SyncStatusDto,
    pub preferences: AgentPreferencesDto,
    pub gateway: GatewayStatusDto,
    pub cloud_devices: Vec<CloudDevice>,
    pub linked_devices: Vec<LinkDeviceInfo>,
    pub remote_tasks: Vec<RemoteTaskDto>,
    pub pending_captures: Vec<PendingCaptureDto>,
}

/// `system.snapshot` 的服务角色对应主体。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "role", content = "snapshot", rename_all = "camelCase")]
pub enum SnapshotBody {
    Daemon(Box<DaemonSnapshot>),
    Agent(Box<AgentSnapshot>),
}

/// 带原子事件位置的全量快照。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub epoch: String,
    pub sequence: u64,
    pub body: SnapshotBody,
}

/// daemon 状态变化事件。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(
    tag = "type",
    content = "data",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum DaemonEvent {
    SnapshotReplaced(DaemonSnapshot),
    Engine(WsServerMsg),
    TaskChanged(TaskDto),
    TaskDeleted {
        task_id: String,
    },
    QueuesChanged(Vec<QueueDto>),
    GroupsChanged(Vec<GroupDto>),
    ConfigChanged(DaemonConfigSnapshot),
    RssChanged {
        source_id: String,
        item_revision: u64,
    },
    PluginsChanged(Vec<PluginDto>),
    ComponentsChanged(Vec<ComponentStatusDto>),
    WebhooksChanged(Vec<WebhookDeliveryDto>),
    RuntimeStatsChanged(DaemonRuntimeStatsDto),
    SelectionPending(SelectionRequestDto),
    SelectionResolved {
        request_id: String,
    },
}

/// agent 自有或转发的状态变化事件。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(
    tag = "type",
    content = "data",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AgentEvent {
    Daemon(DaemonEvent),
    DaemonSnapshotReplaced(DaemonSnapshot),
    DaemonConnectionChanged(bool),
    SessionChanged(Box<Option<AgentSessionDto>>),
    SyncChanged(SyncStatusDto),
    PreferencesChanged(AgentPreferencesDto),
    GatewayChanged(GatewayStatusDto),
    CloudDevicesChanged(Vec<CloudDevice>),
    LinkedDevicesChanged(Vec<LinkDeviceInfo>),
    RemoteTasksChanged(Vec<RemoteTaskDto>),
    PendingCapturesChanged(Vec<PendingCaptureDto>),
}

/// `service.event` notification 的事件主体。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "service", content = "event", rename_all = "camelCase")]
pub enum ServiceEvent {
    Daemon(DaemonEvent),
    Agent(AgentEvent),
}

/// 单个严格递增的服务事件帧。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct EventFrame {
    pub epoch: String,
    pub sequence: u64,
    pub event: ServiceEvent,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{DaemonEvent, EventFrame, ServiceEvent, Snapshot, SnapshotBody};

    #[test]
    fn snapshot_carries_atomic_epoch_and_sequence() -> Result<(), serde_json::Error> {
        let snapshot = Snapshot {
            epoch: "epoch-a".to_owned(),
            sequence: 41,
            body: SnapshotBody::Daemon(Box::default()),
        };
        let wire = serde_json::to_value(&snapshot)?;
        assert_eq!(wire["epoch"], "epoch-a");
        assert_eq!(wire["sequence"], 41);
        assert_eq!(wire["body"]["role"], "daemon");
        assert!(wire["body"]["snapshot"]["tasks"].is_array());
        Ok(())
    }

    #[test]
    fn event_frame_uses_strict_monotonic_cursor_shape() -> Result<(), serde_json::Error> {
        let frame = EventFrame {
            epoch: "epoch-a".to_owned(),
            sequence: 42,
            event: ServiceEvent::Daemon(DaemonEvent::TaskDeleted {
                task_id: "task-1".to_owned(),
            }),
        };
        let wire = serde_json::to_value(frame)?;
        assert_eq!(
            wire,
            json!({
                "epoch": "epoch-a",
                "sequence": 42,
                "event": {
                    "service": "daemon",
                    "event": {
                        "type": "taskDeleted",
                        "data": { "taskId": "task-1" }
                    }
                }
            })
        );
        Ok(())
    }
}
