//! 连接所有权的交互选择订阅、终局与默认回退。

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

use fluxdown_protocol::{
    DaemonEvent, SelectionKind, SelectionOutcome, SelectionRequestDto, SelectionResolutionDto,
};
use tokio::sync::oneshot;

use crate::event_hub::DaemonEventHub;

const RESOLVED_HISTORY_LIMIT: usize = 1024;

#[derive(Clone, Copy, Eq, PartialEq)]
enum SelectionType {
    Hls,
    Bt,
    Variant,
}

struct PendingSelection {
    kind: SelectionType,
    task_id: String,
    default_choice: SelectionOutcome,
    sender: oneshot::Sender<SelectionOutcome>,
}

#[derive(Default)]
struct SelectionState {
    subscribers: HashSet<String>,
    pending: HashMap<String, PendingSelection>,
    resolved: VecDeque<String>,
}

/// 新选择请求的等待方式。
pub enum SelectionWait {
    Immediate(SelectionOutcome),
    Pending(oneshot::Receiver<SelectionOutcome>),
}

/// 选择协议错误。
#[derive(Debug, thiserror::Error)]
pub enum SelectionError {
    #[error("selection request not found")]
    NotFound,
    #[error("selection request was already resolved")]
    Conflict,
    #[error("selection outcome does not match request kind")]
    InvalidOutcome,
    #[error("HLS selection cannot be cancelled")]
    HlsCancel,
}

/// 所有 WebSocket 连接共享的选择状态。
#[derive(Clone)]
pub struct DaemonSelection {
    state: Arc<Mutex<SelectionState>>,
    events: DaemonEventHub,
}

impl DaemonSelection {
    #[must_use]
    pub fn new(events: DaemonEventHub) -> Self {
        Self {
            state: Arc::new(Mutex::new(SelectionState::default())),
            events,
        }
    }

    /// 将一个已完成 hello 的连接登记为交互选择消费者。
    pub fn subscribe(&self, connection_id: String) {
        lock_or_recover(&self.state)
            .subscribers
            .insert(connection_id);
    }

    /// 移除连接；最后一个订阅者离开时所有待选择立即采用各自默认值。
    pub fn unsubscribe(&self, connection_id: &str) {
        let resolved_ids = {
            let mut state = lock_or_recover(&self.state);
            state.subscribers.remove(connection_id);
            if !state.subscribers.is_empty() {
                return;
            }
            let pending = std::mem::take(&mut state.pending);
            let mut ids = Vec::with_capacity(pending.len());
            for (id, selection) in pending {
                let _ = selection.sender.send(selection.default_choice);
                remember_resolved(&mut state.resolved, id.clone());
                ids.push(id);
            }
            ids
        };
        for request_id in resolved_ids {
            self.events
                .publish(DaemonEvent::SelectionResolved { request_id });
        }
    }

    /// 发布待选择请求；无订阅者时不发布并立即采用默认值。
    pub fn begin(&self, request: SelectionRequestDto) -> SelectionWait {
        let mut state = lock_or_recover(&self.state);
        if state.subscribers.is_empty() {
            return SelectionWait::Immediate(request.default_choice);
        }
        let (sender, receiver) = oneshot::channel();
        let kind = selection_type(&request.kind);
        let request_id = request.request_id.clone();
        state.pending.insert(
            request_id,
            PendingSelection {
                kind,
                task_id: request.task_id.clone(),
                default_choice: request.default_choice.clone(),
                sender,
            },
        );
        drop(state);
        self.events.publish(DaemonEvent::SelectionPending(request));
        SelectionWait::Pending(receiver)
    }

    /// 接受第一个合法终局；重复或迟到答复返回 conflict。
    pub fn resolve(&self, resolution: SelectionResolutionDto) -> Result<(), SelectionError> {
        let selection = {
            let mut state = lock_or_recover(&self.state);
            let Some(pending) = state.pending.get(&resolution.request_id) else {
                if state.resolved.contains(&resolution.request_id) {
                    return Err(SelectionError::Conflict);
                }
                return Err(SelectionError::NotFound);
            };
            validate_outcome(pending.kind, &resolution.outcome)?;
            let Some(selection) = state.pending.remove(&resolution.request_id) else {
                return Err(SelectionError::Conflict);
            };
            remember_resolved(&mut state.resolved, resolution.request_id.clone());
            selection
        };
        let _ = selection.sender.send(resolution.outcome);
        self.events.publish(DaemonEvent::SelectionResolved {
            request_id: resolution.request_id,
        });
        Ok(())
    }

    /// 兼容引擎按 task ID 投递答案的 trait；仍由同一 first-wins 终局处理。
    fn resolve_for_task(
        &self,
        task_id: &str,
        kind: SelectionType,
        outcome: SelectionOutcome,
    ) -> Result<(), SelectionError> {
        let request_id = {
            let state = lock_or_recover(&self.state);
            state
                .pending
                .iter()
                .find(|(_, pending)| pending.task_id == task_id && pending.kind == kind)
                .map(|(request_id, _)| request_id.clone())
                .ok_or(SelectionError::NotFound)?
        };
        self.resolve(SelectionResolutionDto {
            request_id,
            outcome,
        })
    }

    /// 超时后移除 pending 并返回默认值；迟到答复随后得到 conflict。
    pub fn timeout(&self, request_id: &str) -> Option<SelectionOutcome> {
        let default_choice = {
            let mut state = lock_or_recover(&self.state);
            let selection = state.pending.remove(request_id)?;
            remember_resolved(&mut state.resolved, request_id.to_owned());
            selection.default_choice
        };
        self.events.publish(DaemonEvent::SelectionResolved {
            request_id: request_id.to_owned(),
        });
        Some(default_choice)
    }

    /// 将仍待处理的选择全部按默认值终结，供 daemon 关闭路径调用。
    pub fn resolve_all_defaults(&self) {
        let ids = {
            let mut state = lock_or_recover(&self.state);
            let pending = std::mem::take(&mut state.pending);
            let mut ids = Vec::with_capacity(pending.len());
            for (id, selection) in pending {
                let _ = selection.sender.send(selection.default_choice);
                remember_resolved(&mut state.resolved, id.clone());
                ids.push(id);
            }
            ids
        };
        for request_id in ids {
            self.events
                .publish(DaemonEvent::SelectionResolved { request_id });
        }
    }
}

const DEFAULT_BT_SELECTION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

#[async_trait::async_trait]
impl fluxdown_engine::selection::HostSelection for DaemonSelection {
    async fn select_hls_quality(
        &self,
        task_id: &str,
        options: &[fluxdown_engine::model::HlsQualityOption],
        timeout: std::time::Duration,
    ) -> fluxdown_engine::selection::SelectionOutcome<i32> {
        let default_index = options
            .iter()
            .enumerate()
            .max_by_key(|(_, option)| option.bandwidth)
            .map_or(0, |(index, _)| index as i32);
        let request_id = uuid::Uuid::new_v4().to_string();
        let request = SelectionRequestDto {
            request_id: request_id.clone(),
            task_id: task_id.to_owned(),
            kind: SelectionKind::Hls {
                options: options
                    .iter()
                    .cloned()
                    .map(fluxdown_engine_protocol::hls_quality_option_to_dto)
                    .collect(),
            },
            default_choice: SelectionOutcome::Hls {
                index: default_index,
            },
            deadline_unix_ms: deadline_unix_ms(timeout),
        };
        match self.begin(request) {
            SelectionWait::Immediate(_) => {
                fluxdown_engine::selection::SelectionOutcome::NoSelectorConfigured(default_index)
            }
            SelectionWait::Pending(receiver) => match tokio::time::timeout(timeout, receiver).await
            {
                Ok(Ok(SelectionOutcome::Hls { index })) => {
                    fluxdown_engine::selection::SelectionOutcome::UserChose(index)
                }
                _ => {
                    let _ = self.timeout(&request_id);
                    fluxdown_engine::selection::SelectionOutcome::TimedOutDefaulted(default_index)
                }
            },
        }
    }

    async fn select_bt_files(
        &self,
        task_id: &str,
        files: &[fluxdown_engine::model::BtFileEntry],
        timeout: Option<std::time::Duration>,
    ) -> fluxdown_engine::selection::SelectionOutcome<Vec<i32>> {
        let effective_timeout = timeout.unwrap_or(DEFAULT_BT_SELECTION_TIMEOUT);
        let request_id = uuid::Uuid::new_v4().to_string();
        let request = SelectionRequestDto {
            request_id: request_id.clone(),
            task_id: task_id.to_owned(),
            kind: SelectionKind::Bt {
                files: files
                    .iter()
                    .cloned()
                    .map(fluxdown_engine_protocol::bt_file_entry_to_dto)
                    .collect(),
            },
            default_choice: SelectionOutcome::Bt {
                indices: Vec::new(),
            },
            deadline_unix_ms: deadline_unix_ms(effective_timeout),
        };
        match self.begin(request) {
            SelectionWait::Immediate(_) => {
                fluxdown_engine::selection::SelectionOutcome::NoSelectorConfigured(Vec::new())
            }
            SelectionWait::Pending(receiver) => {
                match tokio::time::timeout(effective_timeout, receiver).await {
                    Ok(Ok(SelectionOutcome::Bt { indices })) => {
                        fluxdown_engine::selection::SelectionOutcome::UserChose(indices)
                    }
                    Ok(Ok(SelectionOutcome::Cancelled)) => {
                        fluxdown_engine::selection::SelectionOutcome::UserChose(vec![-1])
                    }
                    _ => {
                        let _ = self.timeout(&request_id);
                        fluxdown_engine::selection::SelectionOutcome::TimedOutDefaulted(Vec::new())
                    }
                }
            }
        }
    }

    async fn select_resolve_variant(
        &self,
        task_id: &str,
        options: &[fluxdown_engine::model::ResolveVariantOption],
        default_index: i32,
        timeout: std::time::Duration,
    ) -> fluxdown_engine::selection::SelectionOutcome<i32> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let request = SelectionRequestDto {
            request_id: request_id.clone(),
            task_id: task_id.to_owned(),
            kind: SelectionKind::Variant {
                options: options
                    .iter()
                    .cloned()
                    .map(fluxdown_engine_protocol::resolve_variant_option_to_dto)
                    .collect(),
            },
            default_choice: SelectionOutcome::Variant {
                index: default_index,
            },
            deadline_unix_ms: deadline_unix_ms(timeout),
        };
        match self.begin(request) {
            SelectionWait::Immediate(_) => {
                fluxdown_engine::selection::SelectionOutcome::NoSelectorConfigured(default_index)
            }
            SelectionWait::Pending(receiver) => match tokio::time::timeout(timeout, receiver).await
            {
                Ok(Ok(SelectionOutcome::Variant { index })) => {
                    fluxdown_engine::selection::SelectionOutcome::UserChose(index)
                }
                Ok(Ok(SelectionOutcome::Cancelled)) => {
                    fluxdown_engine::selection::SelectionOutcome::UserChose(-1)
                }
                _ => {
                    let _ = self.timeout(&request_id);
                    fluxdown_engine::selection::SelectionOutcome::TimedOutDefaulted(default_index)
                }
            },
        }
    }

    fn provide_hls_selection(&self, task_id: &str, selected_index: i32) {
        let _ = self.resolve_for_task(
            task_id,
            SelectionType::Hls,
            SelectionOutcome::Hls {
                index: selected_index,
            },
        );
    }

    fn provide_bt_selection(&self, task_id: &str, selected_indices: Vec<i32>) {
        let _ = self.resolve_for_task(
            task_id,
            SelectionType::Bt,
            SelectionOutcome::Bt {
                indices: selected_indices,
            },
        );
    }

    fn provide_variant_selection(&self, task_id: &str, selected_index: i32) {
        let _ = self.resolve_for_task(
            task_id,
            SelectionType::Variant,
            SelectionOutcome::Variant {
                index: selected_index,
            },
        );
    }
}

fn deadline_unix_ms(timeout: std::time::Duration) -> i64 {
    let deadline = std::time::SystemTime::now()
        .checked_add(timeout)
        .unwrap_or(std::time::SystemTime::now());
    deadline
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn selection_type(kind: &SelectionKind) -> SelectionType {
    match kind {
        SelectionKind::Hls { .. } => SelectionType::Hls,
        SelectionKind::Bt { .. } => SelectionType::Bt,
        SelectionKind::Variant { .. } => SelectionType::Variant,
    }
}

fn validate_outcome(kind: SelectionType, outcome: &SelectionOutcome) -> Result<(), SelectionError> {
    match (kind, outcome) {
        (SelectionType::Hls, SelectionOutcome::Hls { .. })
        | (SelectionType::Bt, SelectionOutcome::Bt { .. })
        | (SelectionType::Variant, SelectionOutcome::Variant { .. }) => Ok(()),
        (SelectionType::Hls, SelectionOutcome::Cancelled) => Err(SelectionError::HlsCancel),
        (SelectionType::Bt | SelectionType::Variant, SelectionOutcome::Cancelled) => Ok(()),
        _ => Err(SelectionError::InvalidOutcome),
    }
}

fn remember_resolved(resolved: &mut VecDeque<String>, request_id: String) {
    resolved.push_back(request_id);
    while resolved.len() > RESOLVED_HISTORY_LIMIT {
        resolved.pop_front();
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
        DaemonSnapshot, SelectionKind, SelectionOutcome, SelectionRequestDto,
        SelectionResolutionDto,
    };

    use super::{DaemonSelection, SelectionError, SelectionWait};
    use crate::event_hub::DaemonEventHub;

    fn manager() -> DaemonSelection {
        DaemonSelection::new(DaemonEventHub::new(DaemonSnapshot::default(), 16))
    }

    fn hls_request(id: &str) -> SelectionRequestDto {
        SelectionRequestDto {
            request_id: id.to_owned(),
            task_id: "task-1".to_owned(),
            kind: SelectionKind::Hls {
                options: Vec::new(),
            },
            default_choice: SelectionOutcome::Hls { index: 0 },
            deadline_unix_ms: i64::MAX,
        }
    }

    #[test]
    fn zero_subscribers_short_circuits_to_default() {
        let manager = manager();
        assert!(matches!(
            manager.begin(hls_request("r0")),
            SelectionWait::Immediate(SelectionOutcome::Hls { index: 0 })
        ));
    }

    #[tokio::test]
    async fn first_valid_resolution_wins_and_late_reply_conflicts() {
        let manager = manager();
        manager.subscribe("connection-1".to_owned());
        let SelectionWait::Pending(receiver) = manager.begin(hls_request("r1")) else {
            panic!("subscribed request did not wait");
        };
        assert!(matches!(
            manager.resolve(SelectionResolutionDto {
                request_id: "r1".to_owned(),
                outcome: SelectionOutcome::Cancelled,
            }),
            Err(SelectionError::HlsCancel)
        ));
        manager
            .resolve(SelectionResolutionDto {
                request_id: "r1".to_owned(),
                outcome: SelectionOutcome::Hls { index: 2 },
            })
            .expect("valid resolution");
        assert_eq!(
            receiver.await.expect("selection result"),
            SelectionOutcome::Hls { index: 2 }
        );
        assert!(matches!(
            manager.resolve(SelectionResolutionDto {
                request_id: "r1".to_owned(),
                outcome: SelectionOutcome::Hls { index: 1 },
            }),
            Err(SelectionError::Conflict)
        ));
    }

    #[tokio::test]
    async fn last_subscriber_leaving_resolves_defaults() {
        let manager = manager();
        manager.subscribe("connection-1".to_owned());
        let SelectionWait::Pending(receiver) = manager.begin(hls_request("r2")) else {
            panic!("subscribed request did not wait");
        };
        manager.unsubscribe("connection-1");
        assert_eq!(
            receiver.await.expect("default result"),
            SelectionOutcome::Hls { index: 0 }
        );
    }
}
