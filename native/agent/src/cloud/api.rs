//! FluxCloud `/api/v1` 资源调用面。

use reqwest::Method;
use serde::Serialize;
use serde_json::Value;

use super::{CloudClient, CloudError};

#[derive(Clone)]
pub struct CloudApi {
    client: CloudClient,
}

impl CloudApi {
    #[must_use]
    pub fn new(client: CloudClient) -> Self {
        Self { client }
    }

    pub async fn profile(&self) -> Result<Value, CloudError> {
        self.authed(Method::GET, "/api/v1/me", None::<&Value>).await
    }

    pub async fn devices(&self, device_id: &str) -> Result<Value, CloudError> {
        self.authed(
            Method::GET,
            &format!("/api/v1/devices?deviceId={}", encode(device_id)),
            None::<&Value>,
        )
        .await
    }

    pub async fn rename_device(&self, id: &str, name: &str) -> Result<Value, CloudError> {
        self.authed(
            Method::PATCH,
            &format!("/api/v1/devices/{}", encode(id)),
            Some(&serde_json::json!({"name": name})),
        )
        .await
    }

    pub async fn delete_device(&self, id: &str) -> Result<Value, CloudError> {
        self.authed(
            Method::DELETE,
            &format!("/api/v1/devices/{}", encode(id)),
            None::<&Value>,
        )
        .await
    }

    pub async fn remote_tasks(&self) -> Result<Value, CloudError> {
        self.authed(Method::GET, "/api/v1/tasks/remote", None::<&Value>)
            .await
    }

    pub async fn dispatch_remote<P: Serialize>(&self, body: &P) -> Result<Value, CloudError> {
        self.authed(Method::POST, "/api/v1/tasks/dispatch", Some(body))
            .await
    }

    pub async fn command_remote<P: Serialize>(
        &self,
        id: &str,
        body: &P,
    ) -> Result<Value, CloudError> {
        self.authed(
            Method::POST,
            &format!("/api/v1/tasks/{}/command", encode(id)),
            Some(body),
        )
        .await
    }

    pub async fn report_remote_status<P: Serialize>(
        &self,
        id: &str,
        body: &P,
    ) -> Result<Value, CloudError> {
        self.authed(
            Method::POST,
            &format!("/api/v1/tasks/{}/status", encode(id)),
            Some(body),
        )
        .await
    }

    pub async fn report_remote_progress<P: Serialize>(
        &self,
        body: &P,
    ) -> Result<Value, CloudError> {
        self.authed(Method::POST, "/api/v1/tasks/progress", Some(body))
            .await
    }

    pub async fn ping_presence(&self) -> Result<Value, CloudError> {
        self.authed(Method::POST, "/api/v1/tasks/presence", None::<&Value>)
            .await
    }

    pub async fn remote_events(&self, device_id: &str) -> Result<reqwest::Response, CloudError> {
        self.client
            .authenticated_stream(&format!(
                "/api/v1/tasks/events?deviceId={}",
                encode(device_id)
            ))
            .await
    }

    pub async fn plans(&self) -> Result<Value, CloudError> {
        self.client
            .public::<Value, Value>(Method::GET, "/api/v1/plans/catalog", None)
            .await
    }

    pub async fn create_order<P: Serialize>(&self, body: &P) -> Result<Value, CloudError> {
        self.authed(Method::POST, "/api/v1/orders", Some(body))
            .await
    }

    pub async fn order(&self, order_no: &str) -> Result<Value, CloudError> {
        self.authed(
            Method::GET,
            &format!("/api/v1/orders/{}", encode(order_no)),
            None::<&Value>,
        )
        .await
    }

    pub async fn orders(&self) -> Result<Value, CloudError> {
        self.authed(Method::GET, "/api/v1/orders", None::<&Value>)
            .await
    }

    pub async fn referral(
        &self,
        suffix: &str,
        method: Method,
        body: Option<&Value>,
    ) -> Result<Value, CloudError> {
        self.authed(method, &format!("/api/v1/referral{suffix}"), body)
            .await
    }

    pub async fn referral_codes(&self, page: u32, page_size: u32) -> Result<Value, CloudError> {
        self.referral(
            &format!("/codes?page={page}&pageSize={page_size}"),
            Method::GET,
            None,
        )
        .await
    }

    pub async fn delete_referral_code(&self, id: &str) -> Result<Value, CloudError> {
        self.referral(&format!("/codes/{}", encode(id)), Method::DELETE, None)
            .await
    }

    pub async fn referral_records(
        &self,
        page: u32,
        page_size: u32,
        search: Option<&str>,
    ) -> Result<Value, CloudError> {
        let mut suffix = format!("/records?page={page}&pageSize={page_size}");
        if let Some(search) = search.filter(|value| !value.trim().is_empty()) {
            suffix.push_str("&search=");
            suffix.push_str(&encode(search.trim()));
        }
        self.referral(&suffix, Method::GET, None).await
    }

    pub async fn validate_referral(
        &self,
        code: &str,
        plan_code: &str,
    ) -> Result<Value, CloudError> {
        self.referral(
            &format!(
                "/validate?code={}&planCode={}",
                encode(code),
                encode(plan_code)
            ),
            Method::GET,
            None,
        )
        .await
    }

    pub async fn is_authenticated(&self) -> bool {
        self.client.is_authenticated().await
    }

    pub async fn sync_events(&self, device_id: &str) -> Result<reqwest::Response, CloudError> {
        self.client
            .authenticated_stream(&format!(
                "/api/v1/sync/events?deviceId={}",
                encode(device_id)
            ))
            .await
    }

    pub async fn sync_pull(&self, since: u64, device_id: &str) -> Result<Value, CloudError> {
        self.authed(
            Method::GET,
            &format!(
                "/api/v1/sync/items?since={since}&deviceId={}",
                encode(device_id)
            ),
            None::<&Value>,
        )
        .await
    }

    pub async fn sync_push<P: Serialize>(&self, body: &P) -> Result<Value, CloudError> {
        self.authed(Method::PUT, "/api/v1/sync/items", Some(body))
            .await
    }

    pub async fn cdn_config(&self) -> Result<Value, CloudError> {
        self.authed(Method::GET, "/api/v1/cdn/config", None::<&Value>)
            .await
    }

    pub async fn cdn_report<P: Serialize>(&self, body: &P) -> Result<Value, CloudError> {
        self.authed(Method::POST, "/api/v1/cdn/report", Some(body))
            .await
    }

    pub async fn profile_call<P: Serialize>(
        &self,
        method: Method,
        suffix: &str,
        body: Option<&P>,
    ) -> Result<Value, CloudError> {
        self.authed(method, &format!("/api/v1/me{suffix}"), body)
            .await
    }

    pub async fn persist_profile(
        &self,
        value: Value,
    ) -> Result<fluxdown_protocol::AgentSessionDto, CloudError> {
        let profile = serde_json::from_value::<fluxdown_protocol::CloudProfile>(value)
            .map_err(|error| CloudError::invalid_response(error.to_string()))?;
        self.client.persist_profile(profile).await
    }

    pub async fn clear_session(&self) -> Result<(), CloudError> {
        self.client.clear_session().await
    }

    async fn authed<P: Serialize>(
        &self,
        method: Method,
        path: &str,
        body: Option<&P>,
    ) -> Result<Value, CloudError> {
        self.client.authenticated(method, path, body).await
    }
}

fn encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push_str(&format!("{byte:02X}"));
        }
    }
    encoded
}
