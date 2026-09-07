//! FluxCloud HTTPS 客户端、并发 401 单飞刷新与一次重放。

use std::sync::Arc;
use std::time::Duration;

use reqwest::{Method, StatusCode};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::sync::Mutex;

use super::models::{AuthResponse, CloudErrorBody, RefreshRequest};
use crate::state::{AgentState, CloudCredentials, StateStore};

#[derive(Clone)]
pub struct CloudClient {
    base_url: String,
    http: reqwest::Client,
    stream_http: reqwest::Client,
    state: Arc<Mutex<AgentState>>,
    store: Arc<StateStore>,
    refresh: Arc<Mutex<()>>,
}

impl CloudClient {
    pub fn new(
        base_url: String,
        state: Arc<Mutex<AgentState>>,
        store: Arc<StateStore>,
    ) -> Result<Self, CloudError> {
        validate_base_url(&base_url)?;
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(15))
            .pool_idle_timeout(Duration::from_secs(15))
            .build()
            .map_err(|error| CloudError::transport(error.to_string()))?;
        let stream_http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .pool_idle_timeout(Duration::from_secs(90))
            .build()
            .map_err(|error| CloudError::transport(error.to_string()))?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            http,
            stream_http,
            state,
            store,
            refresh: Arc::new(Mutex::new(())),
        })
    }

    /// 无登录端点调用。
    pub async fn public<P: Serialize, R: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&P>,
    ) -> Result<R, CloudError> {
        let body = body
            .map(serde_json::to_value)
            .transpose()
            .map_err(|error| CloudError::invalid(error.to_string()))?;
        let response = self.send_once(method, path, body, None).await?;
        decode(response).await
    }

    /// 登录端点调用；401 共享一次刷新并只重放一次原请求。
    pub async fn authenticated<P: Serialize, R: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&P>,
    ) -> Result<R, CloudError> {
        let body = body
            .map(serde_json::to_value)
            .transpose()
            .map_err(|error| CloudError::invalid(error.to_string()))?;
        let attempted = self.access_token().await?;
        let response = self
            .send_once(method.clone(), path, body.clone(), Some(&attempted))
            .await?;
        if response.status() != StatusCode::UNAUTHORIZED {
            return decode(response).await;
        }

        let _guard = self.refresh.lock().await;
        let current = self.access_token().await?;
        let replay_token = if current != attempted {
            current
        } else {
            self.refresh_session().await?
        };
        let replay = self
            .send_once(method, path, body, Some(&replay_token))
            .await?;
        decode(replay).await
    }

    /// 显式登录/注册成功后，先原子持久化令牌轮换再返回无令牌会话。
    pub(crate) async fn persist_auth(
        &self,
        auth: AuthResponse,
    ) -> Result<fluxdown_protocol::AgentSessionDto, CloudError> {
        let session = auth.session();
        let mut state = self.state.lock().await;
        state.credentials = Some(CloudCredentials {
            access_token: auth.access_token,
            refresh_token: auth.refresh_token,
            expires_at_unix: now_unix().saturating_add(auth.expires_in),
            session: Some(session.clone()),
        });
        self.store
            .save(&state)
            .await
            .map_err(|error| CloudError::transport(error.to_string()))?;
        Ok(session)
    }

    /// 资料修改成功后更新无令牌会话并原子持久化。
    pub(crate) async fn persist_profile(
        &self,
        profile: fluxdown_protocol::CloudProfile,
    ) -> Result<fluxdown_protocol::AgentSessionDto, CloudError> {
        let mut state = self.state.lock().await;
        let session = state
            .credentials
            .as_mut()
            .and_then(|credentials| credentials.session.as_mut())
            .ok_or_else(CloudError::unauthorized)?;
        session.user = profile.user;
        session.entitlements = profile.entitlements;
        session.current_plan = profile.current_plan;
        let updated = session.clone();
        self.store
            .save(&state)
            .await
            .map_err(|error| CloudError::transport(error.to_string()))?;
        Ok(updated)
    }

    /// 仅显式退出或已确认撤销时清除完整会话。
    pub async fn clear_session(&self) -> Result<(), CloudError> {
        let mut state = self.state.lock().await;
        state.credentials = None;
        self.store
            .save(&state)
            .await
            .map_err(|error| CloudError::transport(error.to_string()))
    }

    async fn refresh_session(&self) -> Result<String, CloudError> {
        let refresh_token = {
            let state = self.state.lock().await;
            state
                .credentials
                .as_ref()
                .ok_or_else(CloudError::unauthorized)?
                .refresh_token
                .clone()
        };
        let response = self
            .send_once(
                Method::POST,
                "/api/v1/auth/refresh",
                Some(
                    serde_json::to_value(RefreshRequest {
                        refresh_token: &refresh_token,
                    })
                    .map_err(|error| CloudError::invalid(error.to_string()))?,
                ),
                None,
            )
            .await?;
        if matches!(
            response.status(),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ) {
            self.clear_session().await?;
            return Err(CloudError::unauthorized());
        }
        let auth = decode::<AuthResponse>(response).await?;
        let access_token = auth.access_token.clone();
        self.persist_auth(auth).await?;
        Ok(access_token)
    }

    async fn access_token(&self) -> Result<String, CloudError> {
        self.state
            .lock()
            .await
            .credentials
            .as_ref()
            .map(|credentials| credentials.access_token.clone())
            .filter(|token| !token.is_empty())
            .ok_or_else(CloudError::unauthorized)
    }

    pub(crate) async fn refresh_token(&self) -> Result<String, CloudError> {
        let state = self.state.lock().await;
        state
            .credentials
            .as_ref()
            .map(|credentials| credentials.refresh_token.clone())
            .filter(|token| !token.is_empty())
            .ok_or_else(CloudError::unauthorized)
    }

    pub(crate) async fn is_authenticated(&self) -> bool {
        self.state
            .lock()
            .await
            .credentials
            .as_ref()
            .is_some_and(|credentials| {
                !credentials.access_token.is_empty() && !credentials.refresh_token.is_empty()
            })
    }

    pub(crate) async fn authenticated_stream(
        &self,
        path: &str,
    ) -> Result<reqwest::Response, CloudError> {
        let access_token = self.access_token().await?;
        let response = self.send_stream_once(path, &access_token).await?;
        if response.status() != StatusCode::UNAUTHORIZED {
            return ensure_success(response).await;
        }
        let replay_token = self.refresh_session().await?;
        ensure_success(self.send_stream_once(path, &replay_token).await?).await
    }

    async fn send_stream_once(
        &self,
        path: &str,
        bearer: &str,
    ) -> Result<reqwest::Response, CloudError> {
        let (device_id, device_name, platform) = {
            let state = self.state.lock().await;
            (
                state.device_id.clone(),
                state.device_name.clone(),
                state.platform.clone(),
            )
        };
        self.stream_http
            .get(format!("{}{}", self.base_url, path))
            .bearer_auth(bearer)
            .header("Accept", "text/event-stream")
            .header("X-FluxDown-Device-Id", device_id)
            .header("X-FluxDown-Device-Name", device_name)
            .header("X-FluxDown-Platform", platform)
            .header("X-FluxDown-Version", env!("CARGO_PKG_VERSION"))
            .send()
            .await
            .map_err(|error| CloudError::transport(format!("{error:#}")))
    }

    async fn send_once(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        bearer: Option<&str>,
    ) -> Result<reqwest::Response, CloudError> {
        let (device_id, device_name, platform) = {
            let state = self.state.lock().await;
            (
                state.device_id.clone(),
                state.device_name.clone(),
                state.platform.clone(),
            )
        };
        let mut request = self
            .http
            .request(method, format!("{}{}", self.base_url, path))
            .header("X-FluxDown-Device-Id", device_id)
            .header("X-FluxDown-Device-Name", device_name)
            .header("X-FluxDown-Platform", platform)
            .header("X-FluxDown-Version", env!("CARGO_PKG_VERSION"));
        if let Some(token) = bearer {
            request = request.bearer_auth(token);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        request
            .send()
            .await
            .map_err(|error| CloudError::transport(format!("{error:#}")))
    }
}

async fn ensure_success(response: reqwest::Response) -> Result<reqwest::Response, CloudError> {
    if response.status().is_success() {
        Ok(response)
    } else {
        Err(response_error(response).await)
    }
}

async fn response_error(response: reqwest::Response) -> CloudError {
    let status = response.status();
    let retryable = status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS;
    let body = response.json::<CloudErrorBody>().await.ok();
    CloudError {
        status: Some(status.as_u16()),
        code: body.as_ref().and_then(|body| body.code.clone()),
        message: body
            .and_then(|body| body.message)
            .unwrap_or_else(|| format!("FluxCloud HTTP {status}")),
        retryable,
    }
}

async fn decode<R: DeserializeOwned>(response: reqwest::Response) -> Result<R, CloudError> {
    let status = response.status();
    if status.is_success() {
        return response
            .json::<R>()
            .await
            .map_err(|error| CloudError::transport(format!("{error:#}")));
    }
    Err(response_error(response).await)
}

fn validate_base_url(base_url: &str) -> Result<(), CloudError> {
    let url =
        reqwest::Url::parse(base_url).map_err(|error| CloudError::invalid(error.to_string()))?;
    let loopback = url.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if url.scheme() == "https" || (loopback && url.scheme() == "http") {
        Ok(())
    } else {
        Err(CloudError::invalid(
            "FluxCloud URL must use HTTPS outside loopback".to_owned(),
        ))
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs().min(i64::MAX as u64) as i64)
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct CloudError {
    pub status: Option<u16>,
    pub code: Option<String>,
    pub message: String,
    pub retryable: bool,
}

impl CloudError {
    fn unauthorized() -> Self {
        Self {
            status: Some(401),
            code: Some("unauthorized".to_owned()),
            message: "authentication required".to_owned(),
            retryable: false,
        }
    }

    fn transport(message: String) -> Self {
        Self {
            status: None,
            code: None,
            message,
            retryable: true,
        }
    }

    fn invalid(message: String) -> Self {
        Self {
            status: None,
            code: Some("invalidArgument".to_owned()),
            message,
            retryable: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::Router;
    use axum::extract::State;
    use axum::http::{HeaderMap, StatusCode, header};
    use axum::response::{IntoResponse, Response};
    use axum::routing::{get, post};
    use serde_json::{Value, json};
    use tokio::sync::Mutex;

    use super::CloudClient;
    use crate::state::{AgentState, CloudCredentials, StateStore};

    #[derive(Clone)]
    struct MockState {
        refreshes: Arc<AtomicUsize>,
    }

    async fn protected(headers: HeaderMap) -> Response {
        if headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            == Some("Bearer new-access")
        {
            axum::Json(json!({ "ok": true })).into_response()
        } else {
            StatusCode::UNAUTHORIZED.into_response()
        }
    }

    async fn refresh(State(state): State<MockState>) -> Response {
        state.refreshes.fetch_add(1, Ordering::SeqCst);
        axum::Json(json!({
            "accessToken": "new-access",
            "refreshToken": "new-refresh",
            "expiresIn": 3600,
            "user": {
                "id": "u1",
                "email": "user@example.com",
                "nickname": "User",
                "plan": "free",
                "status": "active",
                "createdAt": ""
            },
            "entitlements": {},
            "device": {
                "id": "row1",
                "deviceId": "device1",
                "name": "Desktop",
                "createdAt": "",
                "lastSeenAt": ""
            }
        }))
        .into_response()
    }

    async fn reject_refresh() -> StatusCode {
        StatusCode::UNAUTHORIZED
    }

    #[tokio::test]
    async fn concurrent_unauthorized_requests_share_one_refresh_and_replay() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock cloud");
        let address = listener.local_addr().expect("mock address");
        let refreshes = Arc::new(AtomicUsize::new(0));
        let app = Router::new()
            .route("/api/v1/test", get(protected))
            .route("/api/v1/auth/refresh", post(refresh))
            .with_state(MockState {
                refreshes: refreshes.clone(),
            });
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let dir = std::env::temp_dir().join(format!(
            "fluxdown_cloud_test_{}_{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let store = Arc::new(StateStore::open(dir.clone()).await.expect("state store"));
        let state = Arc::new(Mutex::new(AgentState {
            device_id: "device1".to_owned(),
            device_name: "Desktop".to_owned(),
            platform: "linux".to_owned(),
            credentials: Some(CloudCredentials {
                access_token: "old-access".to_owned(),
                refresh_token: "refresh".to_owned(),
                expires_at_unix: 0,
                session: None,
            }),
            ..AgentState::default()
        }));
        {
            let state = state.lock().await;
            store.save(&state).await.expect("save state");
        }
        let client =
            CloudClient::new(format!("http://{address}"), state, store).expect("cloud client");
        let first =
            client.authenticated::<Value, Value>(reqwest::Method::GET, "/api/v1/test", None);
        let second =
            client.authenticated::<Value, Value>(reqwest::Method::GET, "/api/v1/test", None);
        let (first, second) = tokio::join!(first, second);
        assert_eq!(first.expect("first replay")["ok"], true);
        assert_eq!(second.expect("second replay")["ok"], true);
        assert_eq!(refreshes.load(Ordering::SeqCst), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn revoked_refresh_clears_credentials_in_memory_and_on_disk() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind revoked cloud mock");
        let address = listener.local_addr().expect("revoked cloud address");
        let app = Router::new()
            .route("/api/v1/test", get(protected))
            .route("/api/v1/auth/refresh", post(reject_refresh));
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let dir = std::env::temp_dir().join(format!(
            "fluxdown_cloud_revoked_test_{}_{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let store = Arc::new(
            StateStore::open(dir.clone())
                .await
                .expect("revoked state store"),
        );
        let state = Arc::new(Mutex::new(AgentState {
            device_id: "device1".to_owned(),
            credentials: Some(CloudCredentials {
                access_token: "revoked-access".to_owned(),
                refresh_token: "revoked-refresh".to_owned(),
                expires_at_unix: 0,
                session: None,
            }),
            ..AgentState::default()
        }));
        {
            let state = state.lock().await;
            store.save(&state).await.expect("save revoked state");
        }
        let client = CloudClient::new(format!("http://{address}"), state.clone(), store.clone())
            .expect("revoked cloud client");

        let error = client
            .authenticated::<Value, Value>(reqwest::Method::GET, "/api/v1/test", None)
            .await
            .expect_err("revoked refresh must fail");
        assert_eq!(error.status, Some(StatusCode::UNAUTHORIZED.as_u16()));
        assert!(state.lock().await.credentials.is_none());
        assert!(
            store
                .load()
                .await
                .expect("reload revoked state")
                .credentials
                .is_none()
        );
        drop(client);
        drop(state);
        drop(store);
        let _ = tokio::fs::remove_dir_all(dir).await;
    }
}
