//! FluxCloud 登录/验证/退出与无令牌本地会话投影。

use fluxdown_protocol::{AgentEvent, AgentLoginResult};
use reqwest::Method;
use serde::Serialize;
use serde_json::Value;

use super::client::{CloudClient, CloudError};
use super::models::AuthResponse;
use crate::event_hub::AgentEventHub;

#[derive(Clone)]
pub struct CloudAuthService {
    client: CloudClient,
    events: AgentEventHub,
}

impl CloudAuthService {
    #[must_use]
    pub fn new(client: CloudClient, events: AgentEventHub) -> Self {
        Self { client, events }
    }

    pub async fn login<P: Serialize>(&self, request: &P) -> Result<AgentLoginResult, CloudError> {
        self.authenticate("/api/v1/auth/login", request).await
    }

    pub async fn login_verify<P: Serialize>(
        &self,
        request: &P,
    ) -> Result<AgentLoginResult, CloudError> {
        self.authenticate("/api/v1/auth/login/verify", request)
            .await
    }

    pub async fn register<P: Serialize>(
        &self,
        request: &P,
    ) -> Result<AgentLoginResult, CloudError> {
        self.authenticate("/api/v1/auth/register", request).await
    }

    pub async fn register_verify<P: Serialize>(
        &self,
        request: &P,
    ) -> Result<AgentLoginResult, CloudError> {
        self.authenticate("/api/v1/auth/register/verify", request)
            .await
    }

    pub async fn send_code<P: Serialize>(&self, request: &P) -> Result<Value, CloudError> {
        self.client
            .public(Method::POST, "/api/v1/auth/code/send", Some(request))
            .await
    }

    pub async fn verify_code<P: Serialize>(
        &self,
        request: &P,
    ) -> Result<AgentLoginResult, CloudError> {
        self.authenticate("/api/v1/auth/code/verify", request).await
    }

    pub async fn logout(&self) -> Result<(), CloudError> {
        let refresh_token = self.client.refresh_token().await?;
        let remote: Result<Value, CloudError> = self
            .client
            .authenticated(
                Method::POST,
                "/api/v1/auth/logout",
                Some(&serde_json::json!({ "refreshToken": refresh_token })),
            )
            .await;
        self.client.clear_session().await?;
        self.events
            .publish(AgentEvent::SessionChanged(Box::new(None)));
        remote.map(|_| ())
    }

    async fn authenticate<P: Serialize>(
        &self,
        path: &str,
        request: &P,
    ) -> Result<AgentLoginResult, CloudError> {
        let value: Value = self
            .client
            .public(Method::POST, path, Some(request))
            .await?;
        if value.get("accessToken").is_some() {
            let auth = serde_json::from_value::<AuthResponse>(value)
                .map_err(|error| CloudError::invalid_response(error.to_string()))?;
            let session = self.client.persist_auth(auth).await?;
            self.events
                .publish(AgentEvent::SessionChanged(Box::new(Some(session.clone()))));
            Ok(AgentLoginResult::Ok {
                session: Box::new(session),
            })
        } else {
            Ok(AgentLoginResult::DeviceVerificationRequired {
                ttl_seconds: value.get("ttlSeconds").and_then(Value::as_u64).unwrap_or(0),
                will_replace_devices: value
                    .get("willReplaceDevices")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            })
        }
    }
}

impl CloudError {
    pub(crate) fn invalid_response(message: String) -> Self {
        Self {
            status: None,
            code: Some("invalidResponse".to_owned()),
            message,
            retryable: false,
        }
    }
}
