//! CDN 云端先验下发与 daemon 样本租约上传工作器。

use std::collections::BTreeMap;
use std::time::Duration;

use fluxdown_protocol::{
    AgentEvent, CdnConfigApplyParams, CdnReportAckParams, CdnReportLeaseDto, DaemonEvent,
    ServiceEvent, WsServerMsg,
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::cloud::{CloudApi, CloudError};
use crate::daemon_client::DaemonClient;
use crate::event_hub::AgentEventHub;

const CONFIG_INTERVAL: Duration = Duration::from_secs(12 * 60 * 60);
const REPORT_INTERVAL: Duration = Duration::from_secs(30 * 60);
const REPORT_BATCH_GAP: Duration = Duration::from_millis(1_200);
const COMPLETION_DELAY: Duration = Duration::from_secs(10);
const REPORT_BATCH_SIZE: usize = 64;

pub struct CdnWorker {
    cloud: CloudApi,
    daemon: DaemonClient,
    events: AgentEventHub,
}

impl CdnWorker {
    #[must_use]
    pub fn new(cloud: CloudApi, daemon: DaemonClient, events: AgentEventHub) -> Self {
        Self {
            cloud,
            daemon,
            events,
        }
    }

    pub async fn run(self, cancel: CancellationToken) {
        let mut config_tick = tokio::time::interval(CONFIG_INTERVAL);
        let mut report_tick = tokio::time::interval(REPORT_INTERVAL);
        let (mut events, _) = self.events.subscribe_and_snapshot();
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = config_tick.tick() => {
                    if let Err(error) = self.refresh_config().await {
                        trace_cloud_error("CDN config refresh", &error);
                    }
                }
                _ = report_tick.tick() => {
                    if let Err(error) = self.upload_reports().await {
                        trace_cloud_error("CDN report upload", &error);
                    }
                }
                event = events.recv() => {
                    if let Ok(frame) = event
                        && is_task_completion(&frame.event)
                    {
                        let worker = self.clone();
                        let trigger_cancel = cancel.clone();
                        tokio::spawn(async move {
                            tokio::select! {
                                _ = trigger_cancel.cancelled() => {}
                                _ = tokio::time::sleep(COMPLETION_DELAY) => {
                                    if let Err(error) = worker.upload_reports().await {
                                        trace_cloud_error("completion CDN report upload", &error);
                                    }
                                }
                            }
                        });
                    }
                }
            }
        }
    }

    async fn refresh_config(&self) -> Result<(), WorkerError> {
        let config = self.cloud.cdn_config().await?;
        let mut values = BTreeMap::new();
        if let Some(resolvers) = config.get("resolvers") {
            values.insert(
                "cdn_resolver_endpoints".to_owned(),
                serde_json::to_string(resolvers)?,
            );
        }
        if let Some(subnets) = config
            .get("ecs_subnets")
            .or_else(|| config.get("ecsSubnets"))
        {
            values.insert(
                "cdn_ecs_subnets".to_owned(),
                serde_json::to_string(subnets)?,
            );
        }
        if let Some(hints) = config.get("hints_base").or_else(|| config.get("hintsBase")) {
            values.insert(
                "cdn_hints_base".to_owned(),
                hints
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| hints.to_string()),
            );
        }
        if values.is_empty() {
            return Ok(());
        }
        let _: Value = self
            .daemon
            .call(
                fluxdown_protocol::method::DAEMON_CDN_CONFIG_APPLY,
                Some(CdnConfigApplyParams { values }),
            )
            .await
            .map_err(WorkerError::Daemon)?;
        Ok(())
    }

    async fn upload_reports(&self) -> Result<(), WorkerError> {
        let lease: Option<CdnReportLeaseDto> = self
            .daemon
            .call::<Value, Option<CdnReportLeaseDto>>(
                fluxdown_protocol::method::DAEMON_CDN_REPORTS_PEEK,
                None,
            )
            .await
            .map_err(WorkerError::Daemon)?;
        let Some(lease) = lease else {
            return Ok(());
        };
        for (index, batch) in lease.samples.chunks(REPORT_BATCH_SIZE).enumerate() {
            if index > 0 {
                tokio::time::sleep(REPORT_BATCH_GAP).await;
            }
            self.cloud.cdn_report(&json!({ "samples": batch })).await?;
        }
        let _: Value = self
            .daemon
            .call(
                fluxdown_protocol::method::DAEMON_CDN_REPORTS_ACK,
                Some(CdnReportAckParams {
                    batch_id: lease.batch_id,
                }),
            )
            .await
            .map_err(WorkerError::Daemon)?;
        Ok(())
    }
}

impl Clone for CdnWorker {
    fn clone(&self) -> Self {
        Self {
            cloud: self.cloud.clone(),
            daemon: self.daemon.clone(),
            events: self.events.clone(),
        }
    }
}

fn is_task_completion(event: &ServiceEvent) -> bool {
    matches!(
        event,
        ServiceEvent::Agent(AgentEvent::Daemon(DaemonEvent::Engine(
            WsServerMsg::TaskProgress { status: 3, .. }
        )))
    )
}

fn trace_cloud_error(context: &str, error: &WorkerError) {
    match error {
        WorkerError::Cloud(error) if error.status == Some(401) => {
            tracing::debug!(%context, "CDN worker skipped without cloud login");
        }
        _ => tracing::warn!(%context, error = %error, "CDN worker failed"),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WorkerError {
    #[error(transparent)]
    Cloud(#[from] CloudError),
    #[error("daemon CDN RPC failed: {0:?}")]
    Daemon(fluxdown_protocol::RpcErrorData),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
