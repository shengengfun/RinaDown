//! 下载完成通知与活动下载期间的系统保持唤醒。

use std::collections::HashMap;

use fluxdown_protocol::{AgentSnapshot, SnapshotBody};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::event_hub::AgentEventHub;

pub struct BackgroundEffects {
    events: AgentEventHub,
}

impl BackgroundEffects {
    #[must_use]
    pub fn new(events: AgentEventHub) -> Self {
        Self { events }
    }

    pub async fn run(self, cancel: CancellationToken) {
        let (mut receiver, snapshot) = self.events.subscribe_and_snapshot();
        let mut current = agent_snapshot(snapshot);
        let mut statuses = current
            .daemon
            .tasks
            .iter()
            .map(|task| (task.task_id.clone(), task.status))
            .collect::<HashMap<_, _>>();
        let mut awake = None;
        reconcile_awake(&current, &mut awake).await;
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return,
                event = receiver.recv() => {
                    match event {
                        Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {
                            current = agent_snapshot(self.events.snapshot());
                            notify_new_completions(&current, &mut statuses).await;
                            reconcile_awake(&current, &mut awake).await;
                        }
                        Err(broadcast::error::RecvError::Closed) => return,
                    }
                }
            }
        }
    }
}

fn agent_snapshot(snapshot: fluxdown_protocol::Snapshot) -> AgentSnapshot {
    match snapshot.body {
        SnapshotBody::Agent(snapshot) => *snapshot,
        SnapshotBody::Daemon(_) => AgentSnapshot::default(),
    }
}

async fn notify_new_completions(snapshot: &AgentSnapshot, statuses: &mut HashMap<String, i32>) {
    let enabled = preference_bool(snapshot, "download.notify_on_complete");
    let mut next = HashMap::with_capacity(snapshot.daemon.tasks.len());
    for task in &snapshot.daemon.tasks {
        let previous = statuses.get(&task.task_id).copied();
        if enabled && task.status == 3 && previous.is_some_and(|status| status != 3) {
            let file_name = task.file_name.clone();
            tokio::task::spawn_blocking(move || {
                let _ = notify_rust::Notification::new()
                    .summary("FluxDown")
                    .body(&file_name)
                    .show();
            });
        }
        next.insert(task.task_id.clone(), task.status);
    }
    *statuses = next;
}

async fn reconcile_awake(snapshot: &AgentSnapshot, awake: &mut Option<keepawake::KeepAwake>) {
    let should_hold = preference_bool(snapshot, "download.keep_awake")
        && snapshot
            .daemon
            .tasks
            .iter()
            .any(|task| matches!(task.status, 1 | 5));
    if should_hold && awake.is_none() {
        match tokio::task::spawn_blocking(|| {
            keepawake::Builder::default()
                .idle(true)
                .sleep(true)
                .reason("FluxDown active download")
                .app_name("FluxDown")
                .app_reverse_domain("dev.zerx.fluxdown")
                .create()
        })
        .await
        {
            Ok(Ok(guard)) => *awake = Some(guard),
            Ok(Err(error)) => tracing::warn!(error = %error, "could not inhibit sleep"),
            Err(error) => tracing::warn!(error = %error, "keep-awake worker failed"),
        }
    } else if !should_hold {
        *awake = None;
    }
}

fn preference_bool(snapshot: &AgentSnapshot, key: &str) -> bool {
    snapshot
        .preferences
        .values
        .get(key)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use fluxdown_protocol::{AgentPreferencesDto, AgentSnapshot};
    use serde_json::json;

    use super::preference_bool;

    #[test]
    fn namespaced_agent_preferences_drive_background_effects() {
        let mut snapshot = AgentSnapshot {
            preferences: AgentPreferencesDto::default(),
            ..AgentSnapshot::default()
        };
        snapshot
            .preferences
            .values
            .insert("download.keep_awake".to_owned(), json!(true));
        assert!(preference_bool(&snapshot, "download.keep_awake"));
        assert!(!preference_bool(&snapshot, "download.notify_on_complete"));
    }
}
