//! agent 官方 UI Gateway：单一 RPC 会话、agent 快照与 daemon 透明转发。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::extract::ws::{CloseFrame, Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use fluxdown_protocol::method;
use fluxdown_protocol::{
    ApplicationErrorCode, RpcErrorData, RpcErrorObject, RpcNotification, RpcRequest, RpcResponse,
    ServiceHello, ServiceRole, validate_first_request,
};
use futures_util::StreamExt;
use reqwest::Method;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::api_host::AgentApiHost;
use crate::capture::{BlobError, BlobKind, CaptureError, CaptureService, DaemonBlobClient};
use crate::cloud::{CloudApi, CloudAuthService, CloudError};
use crate::daemon_client::DaemonClient;
use crate::diagnostics::{DiagnosticsError, DiagnosticsService};
use crate::event_hub::AgentEventHub;
use crate::platform::PlatformError;
use crate::remote::{RemoteError, RemoteTaskService};
use crate::sync::SyncService;
use crate::update::{UpdateError, UpdateService};

/// daemon `/blobs/*` 请求体上限（与 `fluxdown_daemon::http::REQUEST_BODY_LIMIT` 一致）。
const BLOB_UPLOAD_LIMIT: u64 = 4 * 1024 * 1024;

pub struct GatewayService {
    daemon: Arc<DaemonClient>,
    events: AgentEventHub,
    auth: Arc<CloudAuthService>,
    cloud: Arc<CloudApi>,
    sync: Arc<SyncService>,
    remote: Arc<RemoteTaskService>,
    capture: Arc<CaptureService>,
    blobs: Arc<DaemonBlobClient>,
    diagnostics: Arc<DiagnosticsService>,
    update: Arc<UpdateService>,
    state: Arc<tokio::sync::Mutex<crate::state::AgentState>>,
    store: Arc<crate::state::StateStore>,
    api_switches: Arc<fluxdown_api::server::ApiRuntimeSwitches>,
    api_token: fluxdown_api::auth::TokenCell,
    hello: ServiceHello,
    selection_clients: AtomicUsize,
}

impl GatewayService {
    #[allow(
        clippy::too_many_arguments,
        reason = "gateway composition requires each state owner explicitly"
    )]
    #[must_use]
    pub fn new(
        daemon: Arc<DaemonClient>,
        events: AgentEventHub,
        auth: Arc<CloudAuthService>,
        cloud: Arc<CloudApi>,
        sync: Arc<SyncService>,
        remote: Arc<RemoteTaskService>,
        capture: Arc<CaptureService>,
        blobs: Arc<DaemonBlobClient>,
        diagnostics: Arc<DiagnosticsService>,
        update: Arc<UpdateService>,
        state: Arc<tokio::sync::Mutex<crate::state::AgentState>>,
        store: Arc<crate::state::StateStore>,
        api_switches: Arc<fluxdown_api::server::ApiRuntimeSwitches>,
        api_token: fluxdown_api::auth::TokenCell,
    ) -> Self {
        Self {
            daemon,
            events,
            auth,
            cloud,
            sync,
            remote,
            capture,
            blobs,
            diagnostics,
            update,
            state,
            store,
            api_switches,
            api_token,
            hello: crate::service_hello(
                Uuid::new_v4().to_string(),
                vec![
                    method::CAPABILITY_AGENT_GATEWAY.to_owned(),
                    method::CAPABILITY_AGENT_AUTH.to_owned(),
                    method::CAPABILITY_AGENT_SYNC.to_owned(),
                    method::CAPABILITY_AGENT_REMOTE_TASKS.to_owned(),
                    method::CAPABILITY_AGENT_BILLING.to_owned(),
                    method::CAPABILITY_AGENT_REFERRALS.to_owned(),
                    method::CAPABILITY_AGENT_EXTERNAL_CAPTURE.to_owned(),
                    method::CAPABILITY_AGENT_DEVICE_LINK.to_owned(),
                ],
            ),
            selection_clients: AtomicUsize::new(0),
        }
    }

    async fn call(&self, request: RpcRequest) -> RpcResponse {
        let id = request.id.clone();
        match self.dispatch(request).await {
            Ok(value) => RpcResponse::success(id, value),
            Err(data) => {
                RpcResponse::failure(id, RpcErrorObject::application("agent RPC failed", data))
            }
        }
    }

    async fn dispatch(&self, request: RpcRequest) -> Result<serde_json::Value, RpcErrorData> {
        match request.method.as_str() {
            method::SYSTEM_PING => Ok(serde_json::json!({ "ok": true })),
            method::SYSTEM_SNAPSHOT => serde_json::to_value(self.events.snapshot())
                .map_err(|_| RpcErrorData::new(ApplicationErrorCode::Internal, false)),
            method::AGENT_SESSION_GET => {
                let snapshot = self.events.snapshot();
                let session = match snapshot.body {
                    fluxdown_protocol::SnapshotBody::Agent(agent) => agent.session,
                    fluxdown_protocol::SnapshotBody::Daemon(_) => None,
                };
                serde_json::to_value(session)
                    .map_err(|_| RpcErrorData::new(ApplicationErrorCode::Internal, false))
            }
            method::AGENT_GATEWAY_GET => {
                let snapshot = self.events.snapshot();
                let gateway = match snapshot.body {
                    fluxdown_protocol::SnapshotBody::Agent(agent) => agent.gateway,
                    fluxdown_protocol::SnapshotBody::Daemon(_) => Default::default(),
                };
                serde_json::to_value(gateway)
                    .map_err(|_| RpcErrorData::new(ApplicationErrorCode::Internal, false))
            }
            method::AGENT_GATEWAY_PATCH => {
                self.gateway_patch(params_or_empty(request.params)).await
            }
            method::AGENT_GATEWAY_REVEAL_TOKEN => {
                let token = self.state.lock().await.gateway_user_token.clone();
                Ok(serde_json::json!({ "userToken": token }))
            }
            method::AGENT_AUTH_LOGIN => {
                cloud_value(self.auth.login(&params_or_empty(request.params)).await)
            }
            method::AGENT_AUTH_LOGIN_VERIFY => cloud_value(
                self.auth
                    .login_verify(&params_or_empty(request.params))
                    .await,
            ),
            method::AGENT_AUTH_REGISTER => {
                cloud_value(self.auth.register(&params_or_empty(request.params)).await)
            }
            method::AGENT_AUTH_REGISTER_VERIFY => cloud_value(
                self.auth
                    .register_verify(&params_or_empty(request.params))
                    .await,
            ),
            method::AGENT_AUTH_SEND_CODE => {
                cloud_value(self.auth.send_code(&params_or_empty(request.params)).await)
            }
            method::AGENT_AUTH_VERIFY_CODE => cloud_value(
                self.auth
                    .verify_code(&params_or_empty(request.params))
                    .await,
            ),
            method::AGENT_AUTH_LOGOUT => cloud_value(
                self.auth
                    .logout()
                    .await
                    .map(|()| serde_json::json!({ "ok": true })),
            ),
            method::AGENT_AUTH_REFRESH_PROFILE => {
                self.profile_request(Method::GET, "", None, true).await
            }
            method::AGENT_PROFILE_SEND_EMAIL_CODE => {
                self.profile_request(Method::POST, "/email/code", None, false)
                    .await
            }
            method::AGENT_PROFILE_SEND_NEW_EMAIL_CODE => {
                self.profile_request(
                    Method::POST,
                    "/email/code/new",
                    Some(params_or_empty(request.params)),
                    false,
                )
                .await
            }
            method::AGENT_PROFILE_CHANGE_EMAIL => {
                self.profile_request(
                    Method::POST,
                    "/email",
                    Some(params_or_empty(request.params)),
                    true,
                )
                .await
            }
            method::AGENT_PROFILE_RANDOM_ORIGIN_ID => {
                self.profile_request(Method::GET, "/origin-id/random", None, false)
                    .await
            }
            method::AGENT_PROFILE_CHECK_ORIGIN_ID => {
                let params = params_or_empty(request.params);
                match required_i64(&params, "value") {
                    Ok(value) => {
                        self.profile_request(
                            Method::GET,
                            &format!("/origin-id/check?value={value}"),
                            None,
                            false,
                        )
                        .await
                    }
                    Err(error) => Err(error),
                }
            }
            method::AGENT_PROFILE_CHANGE_ORIGIN_ID => {
                self.profile_request(
                    Method::PUT,
                    "/origin-id",
                    Some(params_or_empty(request.params)),
                    true,
                )
                .await
            }
            method::AGENT_PROFILE_CHANGE_NICKNAME => {
                self.profile_request(
                    Method::PUT,
                    "/nickname",
                    Some(params_or_empty(request.params)),
                    true,
                )
                .await
            }
            method::AGENT_DEVICE_LIST => self.device_list().await,
            method::AGENT_DEVICE_RENAME => {
                self.device_rename(params_or_empty(request.params)).await
            }
            method::AGENT_DEVICE_DELETE => {
                self.device_delete(params_or_empty(request.params)).await
            }
            method::AGENT_PLAN_LIST => cloud_value(self.cloud.plans().await),
            method::AGENT_ORDER_CREATE => cloud_value(
                self.cloud
                    .create_order(&params_or_empty(request.params))
                    .await,
            ),
            method::AGENT_ORDER_GET => self.order_get(params_or_empty(request.params)).await,
            method::AGENT_ORDER_LIST => cloud_value(self.cloud.orders().await),
            method::AGENT_REFERRAL_SUMMARY => {
                cloud_value(self.cloud.referral("/summary", Method::GET, None).await)
            }
            method::AGENT_REFERRAL_LIST_CODES => {
                self.referral_list_codes(params_or_empty(request.params))
                    .await
            }
            method::AGENT_REFERRAL_CREATE_CODE => cloud_value(
                self.cloud
                    .referral(
                        "/codes",
                        Method::POST,
                        Some(&params_or_empty(request.params)),
                    )
                    .await,
            ),
            method::AGENT_REFERRAL_DELETE_CODE => {
                self.referral_delete_code(params_or_empty(request.params))
                    .await
            }
            method::AGENT_REFERRAL_LIST_RECORDS => {
                self.referral_list_records(params_or_empty(request.params))
                    .await
            }
            method::AGENT_REFERRAL_VALIDATE => {
                self.referral_validate(params_or_empty(request.params))
                    .await
            }
            method::AGENT_PREFERENCES_PATCH => {
                self.preferences_patch(params_or_empty(request.params))
                    .await
            }
            method::AGENT_SYNC_GET => serde_json::to_value(self.sync.status().await)
                .map_err(|_| RpcErrorData::new(ApplicationErrorCode::Internal, false)),
            method::AGENT_SYNC_ENABLE => sync_value(self.sync.set_enabled(true).await),
            method::AGENT_SYNC_DISABLE => sync_value(self.sync.set_enabled(false).await),
            method::AGENT_SYNC_NOW => sync_value(self.sync.sync_now().await),
            method::AGENT_REMOTE_LIST => remote_value(self.remote.refresh_snapshot().await),
            method::AGENT_REMOTE_DISPATCH => {
                self.remote_dispatch(params_or_empty(request.params)).await
            }
            method::AGENT_REMOTE_COMMAND => {
                self.remote_command(params_or_empty(request.params)).await
            }
            method::AGENT_CAPTURE_SUBMIT => {
                self.capture_submit(params_or_empty(request.params)).await
            }
            method::AGENT_CAPTURE_SUBMIT_TORRENT_FILE => {
                self.capture_submit_torrent_file(params_or_empty(request.params))
                    .await
            }
            method::AGENT_CAPTURE_LIST => serde_json::to_value(self.capture.list().await)
                .map_err(|_| RpcErrorData::new(ApplicationErrorCode::Internal, false)),
            method::AGENT_CAPTURE_RESOLVE => {
                self.capture_resolve(params_or_empty(request.params)).await
            }
            method::AGENT_PLUGIN_INSTALL_FILE => {
                self.plugin_install_file(params_or_empty(request.params))
                    .await
            }
            method::AGENT_PLATFORM_OPEN_TASK => {
                self.platform_task(params_or_empty(request.params), false)
                    .await
            }
            method::AGENT_PLATFORM_REVEAL_TASK => {
                self.platform_task(params_or_empty(request.params), true)
                    .await
            }
            method::AGENT_PLATFORM_OPEN_PATH => {
                let params =
                    parse_params::<fluxdown_protocol::PlatformOpenPathParams>(request.params)?;
                platform_blocking(move || {
                    crate::platform::open_path(Path::new(&params.path), params.reveal)
                })
                .await
                .map(|()| serde_json::json!({ "ok": true }))
            }
            method::AGENT_PLATFORM_INTEGRATION_GET => {
                platform_blocking(|| Ok(crate::platform::integration_status()))
                    .await
                    .and_then(to_value)
            }
            method::AGENT_PLATFORM_SET_AUTOSTART => {
                let params =
                    parse_params::<fluxdown_protocol::PlatformToggleParams>(request.params)?;
                platform_integration_apply(move || crate::platform::set_autostart(params.enabled))
                    .await
            }
            method::AGENT_PLATFORM_SET_FILE_ASSOCIATION => {
                let params =
                    parse_params::<fluxdown_protocol::PlatformToggleParams>(request.params)?;
                platform_integration_apply(move || {
                    crate::platform::set_file_association(params.enabled)
                })
                .await
            }
            method::AGENT_PLATFORM_SET_URL_PROTOCOL => {
                let params =
                    parse_params::<fluxdown_protocol::PlatformUrlProtocolParams>(request.params)?;
                platform_integration_apply(move || {
                    crate::platform::set_url_protocol(&params.scheme, params.enabled)
                })
                .await
            }
            method::AGENT_DIAGNOSTICS_RUN => {
                diagnostics_value(self.diagnostics.run().await).and_then(to_value)
            }
            method::AGENT_DIAGNOSTICS_REPAIR => {
                let params =
                    parse_params::<fluxdown_protocol::DiagnosticRepairParams>(request.params)?;
                diagnostics_value(self.diagnostics.repair(&params).await)
            }
            method::AGENT_DIAGNOSTICS_LOG_PATHS => to_value(self.diagnostics.log_paths().await),
            method::AGENT_DIAGNOSTICS_EXPORT_LOGS => {
                let params = parse_params::<fluxdown_protocol::LogExportParams>(request.params)?;
                diagnostics_value(self.diagnostics.export_logs(&params).await).and_then(to_value)
            }
            method::AGENT_UPDATE_CHECK => self.update_check(params_or_empty(request.params)).await,
            name if name.starts_with("daemon.") => {
                self.daemon
                    .call::<serde_json::Value, serde_json::Value>(name, request.params)
                    .await
            }
            _ => Err(RpcErrorData::new(ApplicationErrorCode::Unsupported, false)),
        }
    }

    async fn profile_request(
        &self,
        method: Method,
        suffix: &str,
        body: Option<serde_json::Value>,
        persist: bool,
    ) -> Result<serde_json::Value, RpcErrorData> {
        let value = self
            .cloud
            .profile_call(method, suffix, body.as_ref())
            .await
            .map_err(cloud_error_data)?;
        if persist {
            let session = self
                .cloud
                .persist_profile(value.clone())
                .await
                .map_err(cloud_error_data)?;
            self.events
                .publish(fluxdown_protocol::AgentEvent::SessionChanged(Box::new(
                    Some(session),
                )));
        }
        Ok(value)
    }

    async fn gateway_patch(
        &self,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, RpcErrorData> {
        let patch = parse_params::<fluxdown_protocol::GatewayPatchParams>(Some(params))?;
        let mut state = self.state.lock().await;
        if let Some(value) = patch.takeover_enabled {
            state.gateway.takeover_enabled = value;
        }
        if let Some(value) = patch.jsonrpc_enabled {
            state.gateway.jsonrpc_enabled = value;
        }
        if let Some(value) = patch.api_enabled {
            state.gateway.api_enabled = value;
        }
        if let Some(value) = patch.mcp_enabled {
            state.gateway.mcp_enabled = value;
        }
        if let Some(value) = patch.cors_enabled {
            state.gateway.cors_enabled = value;
        }
        if let Some(value) = patch.lan_enabled {
            state.gateway.lan_enabled = value;
        }
        if patch.regenerate_user_token {
            state.gateway_user_token = generate_user_token();
            state.gateway.user_token_configured = true;
        } else if let Some(token) = patch.user_token {
            state.gateway_user_token = token;
            state.gateway.user_token_configured = !state.gateway_user_token.trim().is_empty();
        }
        let gateway = state.gateway.clone();
        let user_token = state.gateway_user_token.clone();
        self.store
            .save(&state)
            .await
            .map_err(|_| RpcErrorData::new(ApplicationErrorCode::Internal, false))?;
        drop(state);
        self.api_switches.update(
            gateway.takeover_enabled,
            gateway.jsonrpc_enabled,
            gateway.api_enabled,
            gateway.mcp_enabled,
            gateway.cors_enabled,
        );
        self.api_token.set(user_token);
        self.events
            .publish(fluxdown_protocol::AgentEvent::GatewayChanged(
                gateway.clone(),
            ));
        serde_json::to_value(gateway)
            .map_err(|_| RpcErrorData::new(ApplicationErrorCode::Internal, false))
    }

    async fn device_list(&self) -> Result<serde_json::Value, RpcErrorData> {
        let device_id = self.state.lock().await.device_id.clone();
        let value = self
            .cloud
            .devices(&device_id)
            .await
            .map_err(cloud_error_data)?;
        let devices = cloud_devices_from_value(&value)?;
        self.events
            .publish(fluxdown_protocol::AgentEvent::CloudDevicesChanged(devices));
        Ok(value)
    }

    async fn device_rename(
        &self,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, RpcErrorData> {
        let id = required_string(&params, "id")?;
        let name = required_string(&params, "name")?;
        if name.chars().count() > 64 {
            return Err(invalid_field("name"));
        }
        let value = self
            .cloud
            .rename_device(&id, &name)
            .await
            .map_err(cloud_error_data)?;
        let updated = serde_json::from_value::<fluxdown_protocol::CloudDevice>(value.clone())
            .map_err(|_| RpcErrorData::new(ApplicationErrorCode::Internal, false))?;
        let mut devices = agent_snapshot(&self.events)?.cloud_devices;
        if let Some(existing) = devices.iter_mut().find(|device| device.id == updated.id) {
            existing.clone_from(&updated);
        } else {
            devices.push(updated);
        }
        self.events
            .publish(fluxdown_protocol::AgentEvent::CloudDevicesChanged(devices));
        Ok(value)
    }

    async fn device_delete(
        &self,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, RpcErrorData> {
        let id = required_string(&params, "id")?;
        let mut devices = agent_snapshot(&self.events)?.cloud_devices;
        let deleting_current = devices
            .iter()
            .find(|device| device.id == id)
            .is_some_and(|device| device.is_current);
        let value = self
            .cloud
            .delete_device(&id)
            .await
            .map_err(cloud_error_data)?;
        devices.retain(|device| device.id != id);
        self.events
            .publish(fluxdown_protocol::AgentEvent::CloudDevicesChanged(devices));
        if deleting_current {
            self.cloud.clear_session().await.map_err(cloud_error_data)?;
            self.events
                .publish(fluxdown_protocol::AgentEvent::SessionChanged(Box::new(
                    None,
                )));
        }
        Ok(value)
    }

    async fn order_get(
        &self,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, RpcErrorData> {
        let order_no = required_string(&params, "orderNo")?;
        cloud_value(self.cloud.order(&order_no).await)
    }

    async fn referral_list_codes(
        &self,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, RpcErrorData> {
        let (page, page_size) = pagination(&params)?;
        cloud_value(self.cloud.referral_codes(page, page_size).await)
    }

    async fn referral_delete_code(
        &self,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, RpcErrorData> {
        let id = required_string(&params, "id")?;
        cloud_value(self.cloud.delete_referral_code(&id).await)
    }

    async fn referral_list_records(
        &self,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, RpcErrorData> {
        let (page, page_size) = pagination(&params)?;
        let search = params.get("search").and_then(serde_json::Value::as_str);
        cloud_value(self.cloud.referral_records(page, page_size, search).await)
    }

    async fn referral_validate(
        &self,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, RpcErrorData> {
        let code = required_string(&params, "code")?;
        let plan_code = required_string(&params, "planCode")?;
        cloud_value(self.cloud.validate_referral(&code, &plan_code).await)
    }

    async fn preferences_patch(
        &self,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, RpcErrorData> {
        let values = params
            .get("values")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| RpcErrorData {
                code: ApplicationErrorCode::InvalidArgument,
                retryable: false,
                field: Some("values".to_owned()),
                revision: None,
            })?;
        let sync = params
            .get("sync")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);
        for (key, value) in values {
            let result = if sync {
                self.sync
                    .mark_local(key.clone(), value.clone(), false)
                    .await
            } else {
                self.sync
                    .set_local_preference(key.clone(), value.clone(), false)
                    .await
            };
            result.map_err(|error| match error {
                crate::sync::SyncError::Daemon(error) => error,
                crate::sync::SyncError::Cloud(error) => {
                    RpcErrorData::new(ApplicationErrorCode::Unavailable, error.retryable)
                }
                crate::sync::SyncError::Protocol(_) | crate::sync::SyncError::State(_) => {
                    RpcErrorData::new(ApplicationErrorCode::Internal, false)
                }
            })?;
        }
        Ok(serde_json::json!({ "ok": true }))
    }

    async fn capture_submit(
        &self,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, RpcErrorData> {
        let request_value = params
            .get("request")
            .cloned()
            .unwrap_or_else(|| params.clone());
        let request =
            serde_json::from_value::<fluxdown_protocol::DownloadRequest>(request_value)
                .map_err(|_| RpcErrorData::new(ApplicationErrorCode::InvalidArgument, false))?;
        let silent = params
            .get("silent")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        capture_value(self.capture.submit(request, silent).await)
    }

    async fn capture_resolve(
        &self,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, RpcErrorData> {
        let transaction_id = required_string(&params, "transactionId")?;
        let accepted = params
            .get("accepted")
            .and_then(serde_json::Value::as_bool)
            .ok_or_else(|| RpcErrorData {
                code: ApplicationErrorCode::InvalidArgument,
                retryable: false,
                field: Some("accepted".to_owned()),
                revision: None,
            })?;
        capture_value(self.capture.resolve(&transaction_id, accepted).await)
    }

    /// 读取本机 `.torrent`，上传 daemon blob 后走捕获路径建任务。
    async fn capture_submit_torrent_file(
        &self,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, RpcErrorData> {
        let path = PathBuf::from(required_string(&params, "path")?);
        let silent = params
            .get("silent")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let bytes = read_upload_file(&path).await?;
        let blob_id = blob_value(self.blobs.upload(BlobKind::Torrent, bytes).await)?;
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let filename = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let request = fluxdown_protocol::DownloadRequest {
            url: format!("torrent-file://{file_name}"),
            filename,
            save_dir: String::new(),
            referrer: String::new(),
            cookies: String::new(),
            headers: None,
            file_size: None,
            mime_type: None,
            method: None,
            body: None,
            audio_url: None,
        };
        capture_value(self.capture.create_torrent(request, blob_id, silent).await)
    }

    /// 读取本机插件包，上传 daemon blob 后转 `daemon.plugin.install`。
    async fn plugin_install_file(
        &self,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, RpcErrorData> {
        let path = PathBuf::from(required_string(&params, "path")?);
        let bytes = read_upload_file(&path).await?;
        let blob_id = blob_value(self.blobs.upload(BlobKind::Plugin, bytes).await)?;
        self.daemon
            .call::<serde_json::Value, serde_json::Value>(
                method::DAEMON_PLUGIN_INSTALL,
                Some(serde_json::json!({ "blobId": blob_id })),
            )
            .await
    }

    async fn update_check(
        &self,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, RpcErrorData> {
        let params = parse_params::<fluxdown_protocol::UpdateCheckParams>(Some(params))?;
        let channel = match params.channel {
            Some(channel) => channel,
            None => self
                .state
                .lock()
                .await
                .preferences
                .values
                .get("general.update_channel")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("stable")
                .to_owned(),
        };
        match self.update.check(&channel).await {
            Ok(result) => to_value(result),
            Err(UpdateError::InvalidChannel(_)) => Err(invalid_field("channel")),
            Err(UpdateError::Http(_) | UpdateError::Status(_)) => {
                Err(RpcErrorData::new(ApplicationErrorCode::Unavailable, true))
            }
            Err(UpdateError::Decode(_)) => {
                Err(RpcErrorData::new(ApplicationErrorCode::Internal, false))
            }
        }
    }

    async fn platform_task(
        &self,
        params: serde_json::Value,
        reveal: bool,
    ) -> Result<serde_json::Value, RpcErrorData> {
        let task_id = required_string(&params, "taskId")?;
        let task: fluxdown_protocol::TaskDto = self
            .daemon
            .call(
                fluxdown_protocol::method::DAEMON_TASK_GET,
                Some(serde_json::json!({ "taskId": task_id })),
            )
            .await?;
        let result = if reveal {
            crate::platform::reveal_task(&task)
        } else {
            crate::platform::open_task(&task)
        };
        result
            .map(|()| serde_json::json!({ "ok": true }))
            .map_err(|_| RpcErrorData::new(ApplicationErrorCode::Internal, false))
    }

    async fn remote_dispatch(
        &self,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, RpcErrorData> {
        let to_device = required_string(&params, "toDevice")?;
        let url = required_string(&params, "url")?;
        let file_name = params
            .get("fileName")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let save_dir = params
            .get("saveDir")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        let local_device = self.remote.local_device_id().await;
        remote_value(
            self.remote
                .dispatch(&to_device, &local_device, url, file_name, save_dir)
                .await,
        )
    }

    async fn remote_command(
        &self,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, RpcErrorData> {
        let task_id = required_string(&params, "taskId")?;
        let action = required_string(&params, "action")?;
        let command_id = params
            .get("commandId")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{task_id}:{action}"));
        let task = self
            .remote
            .tasks()
            .await
            .into_iter()
            .find(|task| task.id == task_id)
            .ok_or_else(|| RpcErrorData::new(ApplicationErrorCode::NotFound, false))?;
        let local_device = self.remote.local_device_id().await;
        remote_value(
            self.remote
                .command(command_id, &task, &local_device, &action)
                .await
                .map(|()| serde_json::json!({ "ok": true })),
        )
    }

    async fn add_selection_client(&self) {
        if self.selection_clients.fetch_add(1, Ordering::AcqRel) == 0 {
            let _ = self
                .daemon
                .call::<serde_json::Value, serde_json::Value>(
                    method::DAEMON_SELECTION_SUBSCRIBE,
                    Some(serde_json::json!({})),
                )
                .await;
        }
    }

    async fn remove_selection_client(&self) {
        let previous = self
            .selection_clients
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                Some(count.saturating_sub(1))
            })
            .unwrap_or(0);
        if previous == 1 {
            let _ = self
                .daemon
                .call::<serde_json::Value, serde_json::Value>(
                    method::DAEMON_SELECTION_UNSUBSCRIBE,
                    Some(serde_json::json!({})),
                )
                .await;
        }
    }
}

fn required_string(params: &serde_json::Value, field: &str) -> Result<String, RpcErrorData> {
    params
        .get(field)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| RpcErrorData {
            code: ApplicationErrorCode::InvalidArgument,
            retryable: false,
            field: Some(field.to_owned()),
            revision: None,
        })
}

fn required_i64(params: &serde_json::Value, field: &str) -> Result<i64, RpcErrorData> {
    params
        .get(field)
        .and_then(serde_json::Value::as_i64)
        .ok_or_else(|| RpcErrorData {
            code: ApplicationErrorCode::InvalidArgument,
            retryable: false,
            field: Some(field.to_owned()),
            revision: None,
        })
}

fn pagination(params: &serde_json::Value) -> Result<(u32, u32), RpcErrorData> {
    let page = params
        .get("page")
        .map_or(Some(1), serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| invalid_field("page"))?;
    let page_size = params
        .get("pageSize")
        .map_or(Some(20), serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| (1..=100).contains(value))
        .ok_or_else(|| invalid_field("pageSize"))?;
    Ok((page, page_size))
}

fn invalid_field(field: &str) -> RpcErrorData {
    RpcErrorData {
        code: ApplicationErrorCode::InvalidArgument,
        retryable: false,
        field: Some(field.to_owned()),
        revision: None,
    }
}

fn agent_snapshot(
    events: &AgentEventHub,
) -> Result<fluxdown_protocol::AgentSnapshot, RpcErrorData> {
    match events.snapshot().body {
        fluxdown_protocol::SnapshotBody::Agent(snapshot) => Ok(*snapshot),
        fluxdown_protocol::SnapshotBody::Daemon(_) => {
            Err(RpcErrorData::new(ApplicationErrorCode::Internal, false))
        }
    }
}

fn cloud_devices_from_value(
    value: &serde_json::Value,
) -> Result<Vec<fluxdown_protocol::CloudDevice>, RpcErrorData> {
    let devices = value
        .get("devices")
        .or_else(|| value.get("value"))
        .unwrap_or(value)
        .clone();
    serde_json::from_value(devices)
        .map_err(|_| RpcErrorData::new(ApplicationErrorCode::Internal, false))
}

fn capture_value<T: serde::Serialize>(
    result: Result<T, CaptureError>,
) -> Result<serde_json::Value, RpcErrorData> {
    match result {
        Ok(value) => serde_json::to_value(value)
            .map_err(|_| RpcErrorData::new(ApplicationErrorCode::Internal, false)),
        Err(CaptureError::Full) => Err(RpcErrorData::new(ApplicationErrorCode::Unavailable, true)),
        Err(CaptureError::NotFound) => {
            Err(RpcErrorData::new(ApplicationErrorCode::NotFound, false))
        }
        Err(CaptureError::Daemon(error)) => Err(error),
        Err(CaptureError::Json(_)) => Err(RpcErrorData::new(
            ApplicationErrorCode::InvalidArgument,
            false,
        )),
        Err(CaptureError::Platform(_)) => {
            Err(RpcErrorData::new(ApplicationErrorCode::Internal, false))
        }
    }
}

fn diagnostics_value<T>(result: Result<T, DiagnosticsError>) -> Result<T, RpcErrorData> {
    result.map_err(|error| match error {
        DiagnosticsError::InvalidAction(_) => {
            RpcErrorData::new(ApplicationErrorCode::InvalidArgument, false)
        }
        DiagnosticsError::Platform(error) => platform_error_data(error),
        DiagnosticsError::Daemon(error) => error,
        DiagnosticsError::State(_) | DiagnosticsError::Io(_) | DiagnosticsError::Export(_) => {
            RpcErrorData::new(ApplicationErrorCode::Internal, false)
        }
    })
}

fn parse_params<T: serde::de::DeserializeOwned>(
    params: Option<serde_json::Value>,
) -> Result<T, RpcErrorData> {
    serde_json::from_value(params_or_empty(params))
        .map_err(|_| RpcErrorData::new(ApplicationErrorCode::InvalidArgument, false))
}

fn to_value<T: serde::Serialize>(value: T) -> Result<serde_json::Value, RpcErrorData> {
    serde_json::to_value(value)
        .map_err(|_| RpcErrorData::new(ApplicationErrorCode::Internal, false))
}

/// 在阻塞线程上执行同步 OS 集成调用。
async fn platform_blocking<T: Send + 'static>(
    action: impl FnOnce() -> Result<T, PlatformError> + Send + 'static,
) -> Result<T, RpcErrorData> {
    tokio::task::spawn_blocking(action)
        .await
        .map_err(|_| RpcErrorData::new(ApplicationErrorCode::Internal, false))?
        .map_err(platform_error_data)
}

/// 应用系统集成变更后返回最新 `PlatformIntegrationDto`。
async fn platform_integration_apply(
    action: impl FnOnce() -> Result<(), PlatformError> + Send + 'static,
) -> Result<serde_json::Value, RpcErrorData> {
    platform_blocking(move || {
        action()?;
        Ok(crate::platform::integration_status())
    })
    .await
    .and_then(to_value)
}

fn platform_error_data(error: PlatformError) -> RpcErrorData {
    match error {
        PlatformError::InvalidScheme(_) => invalid_field("scheme"),
        PlatformError::Unsupported(_) => {
            RpcErrorData::new(ApplicationErrorCode::Unsupported, false)
        }
        PlatformError::Io(_) | PlatformError::Failed(_) => {
            RpcErrorData::new(ApplicationErrorCode::Internal, false)
        }
    }
}

/// 读取待上传的本机文件；必须是存在的普通文件且不超过 daemon 请求体上限。
async fn read_upload_file(path: &Path) -> Result<Vec<u8>, RpcErrorData> {
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|_| invalid_field("path"))?;
    if !metadata.is_file() || metadata.len() > BLOB_UPLOAD_LIMIT {
        return Err(invalid_field("path"));
    }
    tokio::fs::read(path)
        .await
        .map_err(|_| RpcErrorData::new(ApplicationErrorCode::Internal, false))
}

fn blob_value(result: Result<String, BlobError>) -> Result<String, RpcErrorData> {
    result.map_err(|error| match error {
        BlobError::Status(401 | 403) => {
            RpcErrorData::new(ApplicationErrorCode::Unauthorized, false)
        }
        BlobError::Status(413) => invalid_field("path"),
        BlobError::Http(_) | BlobError::Status(_) => {
            RpcErrorData::new(ApplicationErrorCode::Unavailable, true)
        }
        BlobError::Url(_) | BlobError::Decode => {
            RpcErrorData::new(ApplicationErrorCode::Internal, false)
        }
    })
}

/// 32 字节随机用户 token 的十六进制表示。
fn generate_user_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn remote_value<T: serde::Serialize>(
    result: Result<T, RemoteError>,
) -> Result<serde_json::Value, RpcErrorData> {
    match result {
        Ok(value) => serde_json::to_value(value)
            .map_err(|_| RpcErrorData::new(ApplicationErrorCode::Internal, false)),
        Err(RemoteError::Daemon(error)) => Err(error),
        Err(RemoteError::Cloud(error)) if error.status == Some(401) => {
            Err(RpcErrorData::new(ApplicationErrorCode::Unauthorized, false))
        }
        Err(RemoteError::Cloud(error)) => Err(RpcErrorData::new(
            ApplicationErrorCode::Unavailable,
            error.retryable,
        )),
        Err(RemoteError::InvalidAction(_)) | Err(RemoteError::Json(_)) => Err(RpcErrorData::new(
            ApplicationErrorCode::InvalidArgument,
            false,
        )),
        Err(RemoteError::State(_)) | Err(RemoteError::Protocol(_)) => {
            Err(RpcErrorData::new(ApplicationErrorCode::Internal, false))
        }
    }
}

fn params_or_empty(params: Option<serde_json::Value>) -> serde_json::Value {
    params.unwrap_or_else(|| serde_json::json!({}))
}

fn cloud_value<T: serde::Serialize>(
    result: Result<T, CloudError>,
) -> Result<serde_json::Value, RpcErrorData> {
    result.map_err(cloud_error_data).and_then(|value| {
        serde_json::to_value(value)
            .map_err(|_| RpcErrorData::new(ApplicationErrorCode::Internal, false))
    })
}

fn cloud_error_data(error: CloudError) -> RpcErrorData {
    let code = match (error.status, error.code.as_deref()) {
        (Some(401 | 403), _) => ApplicationErrorCode::Unauthorized,
        (Some(404), _) => ApplicationErrorCode::NotFound,
        (Some(409), _) => ApplicationErrorCode::Conflict,
        (_, Some("invalidArgument")) => ApplicationErrorCode::InvalidArgument,
        _ if error.retryable => ApplicationErrorCode::Unavailable,
        _ => ApplicationErrorCode::Internal,
    };
    RpcErrorData::new(code, error.retryable)
}

fn sync_value(
    result: Result<(), crate::sync::SyncError>,
) -> Result<serde_json::Value, RpcErrorData> {
    result
        .map(|()| serde_json::json!({ "ok": true }))
        .map_err(|error| match error {
            crate::sync::SyncError::Daemon(error) => error,
            crate::sync::SyncError::Cloud(error) if error.status == Some(401) => {
                RpcErrorData::new(ApplicationErrorCode::Unauthorized, false)
            }
            crate::sync::SyncError::Cloud(error) => {
                RpcErrorData::new(ApplicationErrorCode::Unavailable, error.retryable)
            }
            crate::sync::SyncError::Protocol(_) | crate::sync::SyncError::State(_) => {
                RpcErrorData::new(ApplicationErrorCode::Internal, false)
            }
        })
}

#[derive(Clone)]
struct GatewayState {
    service: Arc<GatewayService>,
    bearer: Arc<str>,
    cancel: CancellationToken,
}

/// 在同一 loopback listener 合并兼容 API 与官方 `/rpc`。
pub async fn serve(
    listener: TcpListener,
    service: Arc<GatewayService>,
    api_host: Arc<AgentApiHost>,
    api_config: fluxdown_api::server::ApiServerConfig,
    bearer: String,
    cancel: CancellationToken,
) -> Result<(), std::io::Error> {
    let state = GatewayState {
        service,
        bearer: Arc::from(bearer),
        cancel: cancel.clone(),
    };
    let rpc = Router::new()
        .route("/rpc", get(rpc_upgrade))
        .with_state(state);
    let app = fluxdown_api::server::api_router(api_host, api_config).merge(rpc);
    axum::serve(listener, app)
        .with_graceful_shutdown(cancel.cancelled_owned())
        .await
}

pub async fn load_or_create_bearer(
    data_dir: &Path,
    override_path: Option<&Path>,
) -> Result<String, std::io::Error> {
    let path = override_path
        .map(Path::to_path_buf)
        .unwrap_or_else(|| data_dir.join("agent.token"));
    if let Ok(value) = tokio::fs::read_to_string(&path).await
        && !value.trim().is_empty()
    {
        return Ok(value.trim().to_owned());
    }
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let parent = path.parent().unwrap_or(data_dir);
    tokio::fs::create_dir_all(parent).await?;
    crate::state::set_private_dir_permissions(parent).await?;
    let temp = temporary_path(&path);
    use tokio::io::AsyncWriteExt;
    let mut file = tokio::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)
        .await?;
    file.write_all(token.as_bytes()).await?;
    file.write_all(b"\n").await?;
    file.sync_all().await?;
    drop(file);
    crate::state::set_private_file_permissions(&temp).await?;
    crate::state::apply_windows_acl(&temp)
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    tokio::fs::rename(temp, path).await?;
    Ok(token)
}

async fn rpc_upgrade(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    if !authorized(&headers, &state.bearer) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    upgrade
        .on_upgrade(move |socket| run_socket(socket, state.service, state.cancel))
        .into_response()
}

async fn run_socket(
    mut socket: WebSocket,
    service: Arc<GatewayService>,
    cancel: CancellationToken,
) {
    let mut ready = false;
    let mut selection_client = false;
    let mut events = None;
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                let _ = socket.send(Message::Close(Some(CloseFrame {
                    code: 1001,
                    reason: "agent-shutdown".into(),
                }))).await;
                break;
            }
            incoming = socket.next() => {
                let Some(Ok(Message::Text(text))) = incoming else { break; };
                let request = match serde_json::from_str::<RpcRequest>(&text) {
                    Ok(request) => request,
                    Err(error) => {
                        let response = RpcResponse::parse_failure(error.to_string());
                        if send_response(&mut socket, response).await.is_err() { break; }
                        continue;
                    }
                };
                if !ready {
                    let id = request.id.clone();
                    match validate_first_request(&request, ServiceRole::Agent) {
                        Ok(hello) => {
                            ready = true;
                            selection_client = hello.capabilities.iter().any(|capability| capability == method::CAPABILITY_CLIENT_SELECTIONS);
                            if selection_client { service.add_selection_client().await; }
                            let (receiver, _) = service.events.subscribe_and_snapshot();
                            events = Some(receiver);
                            let result = match serde_json::to_value(&service.hello) {
                                Ok(result) => result,
                                Err(_) => break,
                            };
                            if send_response(&mut socket, RpcResponse::success(id, result)).await.is_err() { break; }
                        }
                        Err(data) => {
                            let response = RpcResponse::failure(id, RpcErrorObject::application("hello rejected", data));
                            if send_response(&mut socket, response).await.is_err() { break; }
                        }
                    }
                    continue;
                }
                if send_response(&mut socket, service.call(request).await).await.is_err() { break; }
            }
            event = receive_event(&mut events), if events.is_some() => {
                match event {
                    Ok(frame) => {
                        let Ok(params) = serde_json::to_value(frame) else { break; };
                        let notification = RpcNotification::new(method::SERVICE_EVENT, Some(params));
                        let Ok(text) = serde_json::to_string(&notification) else { break; };
                        if socket.send(Message::Text(text.into())).await.is_err() { break; }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        let _ = socket.send(Message::Close(Some(CloseFrame { code: 4009, reason: "event-gap".into() }))).await;
                        break;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
    if selection_client {
        service.remove_selection_client().await;
    }
}

async fn send_response(socket: &mut WebSocket, response: RpcResponse) -> Result<(), ()> {
    let text = serde_json::to_string(&response).map_err(|_| ())?;
    socket
        .send(Message::Text(text.into()))
        .await
        .map_err(|_| ())
}

async fn receive_event(
    receiver: &mut Option<tokio::sync::broadcast::Receiver<fluxdown_protocol::EventFrame>>,
) -> Result<fluxdown_protocol::EventFrame, tokio::sync::broadcast::error::RecvError> {
    match receiver {
        Some(receiver) => receiver.recv().await,
        None => std::future::pending().await,
    }
}

fn authorized(headers: &HeaderMap, expected: &str) -> bool {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return false;
    };
    let Ok(value) = value.to_str() else {
        return false;
    };
    value
        .strip_prefix("Bearer ")
        .is_some_and(|value| constant_time_eq(value.as_bytes(), expected.as_bytes()))
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

fn temporary_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("agent.token");
    path.with_file_name(format!(".{name}.{}.tmp", Uuid::new_v4()))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use axum::http::{HeaderMap, HeaderValue, header};
    use fluxdown_protocol::{
        AgentSnapshot, ApplicationErrorCode, RequestId, RpcRequest, RpcResponse,
    };

    use super::{GatewayService, authorized, load_or_create_bearer};

    #[tokio::test]
    async fn service_bearer_is_exact_stable_and_private() {
        let dir = std::env::temp_dir().join(format!(
            "fluxdown_agent_token_{}_{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        tokio::fs::create_dir_all(&dir)
            .await
            .expect("create token dir");
        let first = load_or_create_bearer(&dir, None)
            .await
            .expect("create token");
        let second = load_or_create_bearer(&dir, None)
            .await
            .expect("reload token");
        assert_eq!(first, second);
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {first}")).expect("header"),
        );
        assert!(authorized(&headers, &first));
        assert!(!authorized(&headers, "different"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join("agent.token"))
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    struct TestGateway {
        service: GatewayService,
        state: Arc<tokio::sync::Mutex<crate::state::AgentState>>,
        store: Arc<crate::state::StateStore>,
        dir: std::path::PathBuf,
    }

    impl TestGateway {
        async fn new(label: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "fluxdown_agent_{label}_{}_{}",
                std::process::id(),
                uuid::Uuid::new_v4()
            ));
            let store = Arc::new(
                crate::state::StateStore::open(dir.clone())
                    .await
                    .expect("open agent test store"),
            );
            let state = Arc::new(tokio::sync::Mutex::new(crate::state::AgentState::default()));
            let events = crate::event_hub::AgentEventHub::new(AgentSnapshot::default());
            let daemon = Arc::new(crate::daemon_client::DaemonClient::disconnected());
            let cloud_client = crate::cloud::CloudClient::new(
                "http://127.0.0.1:9".to_owned(),
                state.clone(),
                store.clone(),
            )
            .expect("build agent test cloud client");
            let cloud_api = crate::cloud::CloudApi::new(cloud_client.clone());
            let auth = Arc::new(crate::cloud::CloudAuthService::new(
                cloud_client,
                events.clone(),
            ));
            let sync = Arc::new(crate::sync::SyncService::new(
                cloud_api.clone(),
                daemon.clone(),
                events.clone(),
                state.clone(),
                store.clone(),
            ));
            let remote = Arc::new(crate::remote::RemoteTaskService::new(
                cloud_api.clone(),
                daemon.clone(),
                events.clone(),
                state.clone(),
                store.clone(),
            ));
            let capture = Arc::new(crate::capture::CaptureService::new(
                daemon.clone(),
                events.clone(),
            ));
            let daemon_config = crate::daemon_client::DaemonClientConfig {
                rpc_url: "ws://127.0.0.1:9/rpc".to_owned(),
                bearer: String::new(),
            };
            let blobs = Arc::new(
                crate::capture::DaemonBlobClient::new(&daemon_config).expect("blob client"),
            );
            let api_switches = Arc::new(fluxdown_api::server::ApiRuntimeSwitches::new(
                false, false, false, false, false,
            ));
            let diagnostics = Arc::new(crate::diagnostics::DiagnosticsService::new(
                daemon.clone(),
                daemon_config,
                events.clone(),
                state.clone(),
                store.clone(),
                api_switches.clone(),
            ));
            let update = Arc::new(
                crate::update::UpdateService::new(env!("CARGO_PKG_VERSION"))
                    .expect("update service"),
            );
            let service = GatewayService::new(
                daemon,
                events,
                auth,
                Arc::new(cloud_api),
                sync,
                remote,
                capture,
                blobs,
                diagnostics,
                update,
                state.clone(),
                store.clone(),
                api_switches,
                fluxdown_api::auth::TokenCell::new(""),
            );
            Self {
                service,
                state,
                store,
                dir,
            }
        }

        async fn call(&self, method_name: &str, params: serde_json::Value) -> RpcResponse {
            self.service
                .call(RpcRequest::new(
                    RequestId::Integer(1),
                    method_name,
                    Some(params),
                ))
                .await
        }

        async fn finish(self) {
            let Self {
                service,
                state,
                store,
                dir,
            } = self;
            drop(service);
            drop(state);
            drop(store);
            let _ = tokio::fs::remove_dir_all(dir).await;
        }
    }

    #[tokio::test]
    async fn gateway_patch_persists_lan_flag_and_regenerates_token() {
        let harness = TestGateway::new("gateway_patch").await;
        let response = harness
            .call(
                fluxdown_protocol::method::AGENT_GATEWAY_PATCH,
                serde_json::json!({ "lanEnabled": true, "regenerateUserToken": true }),
            )
            .await;
        let RpcResponse::Success(success) = response else {
            panic!("gateway patch failed: {response:?}");
        };
        assert_eq!(success.result["lanEnabled"], serde_json::json!(true));
        assert_eq!(
            success.result["userTokenConfigured"],
            serde_json::json!(true)
        );
        assert!(success.result.get("userToken").is_none());

        let first_token = harness.state.lock().await.gateway_user_token.clone();
        assert_eq!(first_token.len(), 64);
        assert!(first_token.bytes().all(|byte| byte.is_ascii_hexdigit()));
        let persisted = harness.store.load().await.expect("reload state");
        assert!(persisted.gateway.lan_enabled);
        assert_eq!(persisted.gateway_user_token, first_token);

        let response = harness
            .call(
                fluxdown_protocol::method::AGENT_GATEWAY_PATCH,
                serde_json::json!({ "regenerateUserToken": true, "userToken": "ignored" }),
            )
            .await;
        assert!(matches!(response, RpcResponse::Success(_)));
        let second_token = harness.state.lock().await.gateway_user_token.clone();
        assert_ne!(second_token, first_token);
        assert_ne!(second_token, "ignored");

        let response = harness
            .call(
                fluxdown_protocol::method::AGENT_GATEWAY_PATCH,
                serde_json::json!({ "userToken": "", "lanEnabled": false }),
            )
            .await;
        let RpcResponse::Success(success) = response else {
            panic!("gateway patch failed: {response:?}");
        };
        assert_eq!(
            success.result["userTokenConfigured"],
            serde_json::json!(false)
        );
        assert_eq!(success.result["lanEnabled"], serde_json::json!(false));
        harness.finish().await;
    }

    #[tokio::test]
    async fn file_backed_methods_reject_missing_or_non_regular_paths() {
        let harness = TestGateway::new("file_paths").await;
        for method_name in [
            fluxdown_protocol::method::AGENT_PLUGIN_INSTALL_FILE,
            fluxdown_protocol::method::AGENT_CAPTURE_SUBMIT_TORRENT_FILE,
        ] {
            for path in [
                harness.dir.join("missing.bin").display().to_string(),
                harness.dir.display().to_string(),
            ] {
                let response = harness
                    .call(method_name, serde_json::json!({ "path": path }))
                    .await;
                let RpcResponse::Failure(failure) = response else {
                    panic!("{method_name} accepted {path}");
                };
                let data = failure.error.data.expect("error data");
                assert_eq!(data.code, ApplicationErrorCode::InvalidArgument);
                assert_eq!(data.field.as_deref(), Some("path"));
            }
        }
        harness.finish().await;
    }

    #[tokio::test]
    async fn every_canonical_agent_method_reaches_a_real_dispatch_branch() {
        let harness = TestGateway::new("dispatch").await;
        let service = &harness.service;

        for (index, method_name) in fluxdown_protocol::method::ALL_METHODS
            .iter()
            .copied()
            .filter(|name| name.starts_with("agent."))
            .enumerate()
        {
            // 版本检查会真的访问官方站点；用非法渠道让它在触网前返回 InvalidArgument。
            let params = if method_name == fluxdown_protocol::method::AGENT_UPDATE_CHECK {
                serde_json::json!({ "channel": "offline-test" })
            } else {
                serde_json::json!({})
            };
            let response = tokio::time::timeout(
                Duration::from_secs(2),
                service.call(RpcRequest::new(
                    RequestId::Integer(i64::try_from(index).unwrap_or(i64::MAX)),
                    method_name,
                    Some(params),
                )),
            )
            .await
            .unwrap_or_else(|_| panic!("{method_name} dispatch timed out"));
            if let RpcResponse::Failure(failure) = response
                && failure.error.data.as_ref().map(|data| data.code)
                    == Some(ApplicationErrorCode::Unsupported)
            {
                panic!("{method_name} fell through agent dispatch");
            }
        }
        harness.finish().await;
    }
}
