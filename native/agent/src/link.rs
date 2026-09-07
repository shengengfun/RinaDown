//! daemon legacy 设备链接身份的一次性导入与 agent 私有信任状态。

use std::sync::Arc;

use base64::Engine as _;
use ed25519_dalek::SigningKey;
use fluxdown_protocol::{
    AgentEvent, GatewayMigrationExport, LinkDeviceInfo, LinkMigrationExport, MigrationAckParams,
};
use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::daemon_client::DaemonClient;
use crate::event_hub::AgentEventHub;
use crate::state::{AgentState, StateStore};

pub async fn migrate_legacy_state(
    daemon: &DaemonClient,
    state: &Arc<Mutex<AgentState>>,
    store: &StateStore,
    events: &AgentEventHub,
) -> Result<(), LinkMigrationError> {
    migrate_link(daemon, state, store, events).await?;
    migrate_gateway(daemon, state, store, events).await
}

async fn migrate_link(
    daemon: &DaemonClient,
    state: &Arc<Mutex<AgentState>>,
    store: &StateStore,
    events: &AgentEventHub,
) -> Result<(), LinkMigrationError> {
    if state.lock().await.link_migration_revision.is_some() {
        return Ok(());
    }
    let export = daemon
        .call::<Value, LinkMigrationExport>(
            fluxdown_protocol::method::DAEMON_MIGRATION_LINK_EXPORT,
            None,
        )
        .await;
    let export = match export {
        Ok(export) => export,
        Err(error) if error.code == fluxdown_protocol::ApplicationErrorCode::NotFound => {
            ensure_identity(state, store).await?;
            return Ok(());
        }
        Err(error) => return Err(LinkMigrationError::Daemon(error)),
    };
    {
        let mut state = state.lock().await;
        state.link_identity = if export.identity.is_null() {
            Some(generate_identity())
        } else {
            Some(export.identity)
        };
        state.linked_devices = export.roster;
        state.link_migration_revision = Some(export.revision);
        store.save(&state).await?;
        events.publish(AgentEvent::LinkedDevicesChanged(public_devices(&state)));
    }
    let _: Value = daemon
        .call(
            fluxdown_protocol::method::DAEMON_MIGRATION_LINK_ACK,
            Some(MigrationAckParams {
                revision: export.revision,
            }),
        )
        .await
        .map_err(LinkMigrationError::Daemon)?;
    Ok(())
}

async fn migrate_gateway(
    daemon: &DaemonClient,
    state: &Arc<Mutex<AgentState>>,
    store: &StateStore,
    events: &AgentEventHub,
) -> Result<(), LinkMigrationError> {
    if state.lock().await.gateway_migration_revision.is_some() {
        return Ok(());
    }
    let export = daemon
        .call::<Value, GatewayMigrationExport>(
            fluxdown_protocol::method::DAEMON_MIGRATION_GATEWAY_EXPORT,
            None,
        )
        .await;
    let export = match export {
        Ok(export) => export,
        Err(error) if error.code == fluxdown_protocol::ApplicationErrorCode::NotFound => {
            return Ok(());
        }
        Err(error) => return Err(LinkMigrationError::Daemon(error)),
    };
    {
        let mut state = state.lock().await;
        state.gateway.takeover_enabled = export.takeover_enabled;
        state.gateway.jsonrpc_enabled = export.jsonrpc_enabled;
        state.gateway.api_enabled = export.api_enabled;
        state.gateway.mcp_enabled = export.mcp_enabled;
        state.gateway.cors_enabled = export.cors_enabled;
        state.gateway.user_token_configured = export.user_token_configured;
        state.gateway_user_token.clone_from(&export.user_token);
        state.gateway_migration_revision = Some(export.revision);
        store.save(&state).await?;
        events.publish(AgentEvent::GatewayChanged(state.gateway.clone()));
    }
    let _: Value = daemon
        .call(
            fluxdown_protocol::method::DAEMON_MIGRATION_GATEWAY_ACK,
            Some(MigrationAckParams {
                revision: export.revision,
            }),
        )
        .await
        .map_err(LinkMigrationError::Daemon)?;
    Ok(())
}

async fn ensure_identity(
    state: &Arc<Mutex<AgentState>>,
    store: &StateStore,
) -> Result<(), LinkMigrationError> {
    let mut state = state.lock().await;
    if state.link_identity.is_none() {
        state.link_identity = Some(generate_identity());
        store.save(&state).await?;
    }
    Ok(())
}

fn generate_identity() -> Value {
    let secret = rand::random::<[u8; 32]>();
    let signing = SigningKey::from_bytes(&secret);
    json!({
        "secretB64": base64::engine::general_purpose::STANDARD.encode(secret),
        "publicB64": base64::engine::general_purpose::STANDARD.encode(signing.verifying_key().to_bytes()),
    })
}

#[must_use]
pub fn public_devices(state: &AgentState) -> Vec<LinkDeviceInfo> {
    state
        .linked_devices
        .iter()
        .filter_map(|value| {
            Some(LinkDeviceInfo {
                fingerprint: value.get("fingerprint")?.as_str()?.to_owned(),
                name: value
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                platform: value
                    .get("platform")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                online: false,
                paired_at: value.get("pairedAt").and_then(Value::as_i64).unwrap_or(0),
                last_seen_at: value.get("lastSeenAt").and_then(Value::as_i64).unwrap_or(0),
            })
        })
        .collect()
}

#[derive(Debug, thiserror::Error)]
pub enum LinkMigrationError {
    #[error("daemon migration RPC failed: {0:?}")]
    Daemon(fluxdown_protocol::RpcErrorData),
    #[error(transparent)]
    State(#[from] crate::state::StateError),
}
