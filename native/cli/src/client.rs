//! Typed HTTP 客户端 —— 复用 [`fluxdown_api`] 的路径常量与 wire 类型，
//! 保证请求地址/JSON 结构与服务端永不漂移。
//!
//! 面向运行中的 FluxDown App（本机 API 服务，默认 `127.0.0.1:17800`）或
//! headless server。所有管理 API 强制 token 鉴权。

use std::time::Duration;

use fluxdown_api::auth::TOKEN_HEADER;
use fluxdown_api::routes;
use fluxdown_api::service::UNKNOWN_ENDPOINT_MESSAGE;
use fluxdown_protocol::daemon::{
    ApiInfo, CreateTaskRequest, CreatedTask, QueueDto, RssItemDto, RssSourceDto, TaskDto,
};
use reqwest::{Client, Method, StatusCode};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::exit::ExitCode;

/// 客户端错误，携带最贴近的 aria2 退出码。
#[derive(Debug)]
pub struct ClientError {
    pub message: String,
    pub exit: ExitCode,
}

impl ClientError {
    /// 构造一个携带 aria2 退出码的客户端错误。
    ///
    /// # Examples
    ///
    /// ```
    /// use fluxdown_cli::client::ClientError;
    /// use fluxdown_cli::exit::ExitCode;
    ///
    /// let e = ClientError::new("boom", ExitCode::Unknown);
    /// assert_eq!(e.message, "boom");
    /// assert_eq!(e.exit, ExitCode::Unknown);
    /// ```
    pub fn new(message: impl Into<String>, exit: ExitCode) -> Self {
        Self {
            message: message.into(),
            exit,
        }
    }
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ClientError {}

/// 服务端统一错误体 `{"success":false,"message":...}` 的 message 字段。
#[derive(serde::Deserialize)]
struct ServerError {
    message: Option<String>,
}

/// `POST /api/v1/rss` 的应答体。服务端沿用 `CreatedTask`/`CreateGroupResponse`
/// 的 `{"<资源>Id"}` 惯例，这里只取 id，无需引入完整 DTO。
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreatedRssSource {
    source_id: String,
}

/// 无请求体时传给 [`ApiClient::send_json`] 的占位值：`body` 已泛型化，
/// 裸 `None` 推断不出类型。
const NO_BODY: Option<&()> = None;

/// FluxDown 管理 API 客户端。
pub struct ApiClient {
    http: Client,
    base: String,
    token: String,
}

impl ApiClient {
    /// 构造客户端。
    ///
    /// - `base`：服务基址（如 `http://127.0.0.1:17800`），尾部斜杠自动去除。
    /// - `token`：管理 API token（`FLUXDOWN_TOKEN`）。
    /// - `timeout`：单请求超时。
    pub fn new(base: &str, token: &str, timeout: Duration) -> Result<Self, ClientError> {
        let http = Client::builder()
            .timeout(timeout)
            // CLI 只连本机 API 服务，绝不走系统代理（否则本地回环被代理拦截误报）。
            .no_proxy()
            .build()
            .map_err(|e| {
                ClientError::new(
                    format!("failed to build HTTP client: {e}"),
                    ExitCode::Unknown,
                )
            })?;
        Ok(Self {
            http,
            base: base.trim_end_matches('/').to_string(),
            token: token.to_string(),
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    /// 将 reqwest 传输层错误映射为带退出码的 [`ClientError`]。
    fn transport_err(&self, e: &reqwest::Error) -> ClientError {
        if e.is_timeout() {
            ClientError::new(
                format!("request timed out: {}", self.base),
                ExitCode::Timeout,
            )
        } else if e.is_connect() {
            ClientError::new(
                format!("cannot connect to {} — is FluxDown running?", self.base),
                ExitCode::Network,
            )
        } else {
            ClientError::new(format!("network error: {e}"), ExitCode::Network)
        }
    }

    /// 依 HTTP 状态码映射退出码，并尽力提取服务端 message。
    async fn status_err(&self, resp: reqwest::Response) -> ClientError {
        let code = resp.status();
        let body = resp.text().await.unwrap_or_default();
        let msg = serde_json::from_str::<ServerError>(&body)
            .ok()
            .and_then(|e| e.message)
            .unwrap_or_else(|| body.trim().to_string());
        // 管理 API 分组未启用时，`/api/v1/*` 命中服务端 404 fallback（message
        // 为 UNKNOWN_ENDPOINT_MESSAGE），而非资源不存在。给出可操作提示，区别于
        // 「任务 ID 不存在」的普通 not found，指引用户去开启开关。
        if code == StatusCode::NOT_FOUND && msg == UNKNOWN_ENDPOINT_MESSAGE {
            return ClientError::new(
                format!(
                    "management API is not enabled on {} — turn on \
                     the Management API in the desktop app (Settings → Local API Service), \
                     or run a headless server (where it is always on), then set FLUXDOWN_TOKEN",
                    self.base
                ),
                ExitCode::NotFound,
            );
        }
        let (exit, prefix) = match code {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                (ExitCode::Auth, "authentication failed")
            }
            StatusCode::NOT_FOUND => (ExitCode::NotFound, "not found"),
            StatusCode::BAD_REQUEST => (ExitCode::BadRequest, "bad request"),
            StatusCode::SERVICE_UNAVAILABLE => (ExitCode::Network, "service unavailable"),
            _ => (ExitCode::Unknown, "request failed"),
        };
        let detail = if msg.is_empty() {
            format!("{prefix} (HTTP {})", code.as_u16())
        } else {
            format!("{prefix}: {msg}")
        };
        ClientError::new(detail, exit)
    }

    /// 发送请求并将成功响应反序列化为 `T`。
    ///
    /// `body` 泛型化以承载任意 wire 请求体（`?Sized` 允许直接传 `&str` 等）；
    /// 无请求体时传 [`NO_BODY`]。
    async fn send_json<T: DeserializeOwned, B: Serialize + ?Sized>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<T, ClientError> {
        let mut req = self
            .http
            .request(method, self.url(path))
            .header(TOKEN_HEADER, &self.token);
        if let Some(b) = body {
            req = req.json(b);
        }
        let resp = req.send().await.map_err(|e| self.transport_err(&e))?;
        if !resp.status().is_success() {
            return Err(self.status_err(resp).await);
        }
        resp.json::<T>().await.map_err(|e| {
            ClientError::new(format!("failed to parse response: {e}"), ExitCode::Unknown)
        })
    }

    /// 发送请求，忽略响应体（用于 pause/continue/delete 等 ack 端点）。
    async fn send_unit(&self, method: Method, path: &str) -> Result<(), ClientError> {
        let resp = self
            .http
            .request(method, self.url(path))
            .header(TOKEN_HEADER, &self.token)
            .send()
            .await
            .map_err(|e| self.transport_err(&e))?;
        if !resp.status().is_success() {
            return Err(self.status_err(resp).await);
        }
        Ok(())
    }

    /// `GET /ping`（无鉴权）。返回原始 JSON 值。
    pub async fn ping(&self) -> Result<serde_json::Value, ClientError> {
        let resp = self
            .http
            .get(self.url(routes::PING))
            .send()
            .await
            .map_err(|e| self.transport_err(&e))?;
        if !resp.status().is_success() {
            return Err(self.status_err(resp).await);
        }
        resp.json::<serde_json::Value>().await.map_err(|e| {
            ClientError::new(
                format!("failed to parse ping response: {e}"),
                ExitCode::Unknown,
            )
        })
    }

    /// `GET /api/v1/info`。
    pub async fn info(&self) -> Result<ApiInfo, ClientError> {
        self.send_json(Method::GET, routes::API_INFO, NO_BODY).await
    }

    /// `GET /api/v1/tasks?status=N`（`status` 为 `None` 时列出全部）。
    pub async fn list_tasks(&self, status: Option<i32>) -> Result<Vec<TaskDto>, ClientError> {
        let path = match status {
            Some(s) => format!("{}?status={s}", routes::API_TASKS),
            None => routes::API_TASKS.to_string(),
        };
        self.send_json(Method::GET, &path, NO_BODY).await
    }

    /// `GET /api/v1/tasks/{id}`。
    pub async fn get_task(&self, id: &str) -> Result<TaskDto, ClientError> {
        self.send_json(Method::GET, &routes::task_path(id), NO_BODY)
            .await
    }

    /// `POST /api/v1/tasks`。
    pub async fn create_task(&self, req: &CreateTaskRequest) -> Result<CreatedTask, ClientError> {
        self.send_json(Method::POST, routes::API_TASKS, Some(req))
            .await
    }

    /// `DELETE /api/v1/tasks/{id}?deleteFiles=<bool>`。
    pub async fn delete_task(&self, id: &str, delete_files: bool) -> Result<(), ClientError> {
        let path = format!("{}?deleteFiles={delete_files}", routes::task_path(id));
        self.send_unit(Method::DELETE, &path).await
    }

    /// `PUT /api/v1/tasks/{id}/pause`。
    pub async fn pause_task(&self, id: &str) -> Result<(), ClientError> {
        self.send_unit(Method::PUT, &routes::task_pause_path(id))
            .await
    }

    /// `PUT /api/v1/tasks/{id}/continue`。
    pub async fn resume_task(&self, id: &str) -> Result<(), ClientError> {
        self.send_unit(Method::PUT, &routes::task_continue_path(id))
            .await
    }

    /// `PUT /api/v1/tasks/pause`（暂停全部）。
    pub async fn pause_all(&self) -> Result<(), ClientError> {
        self.send_unit(Method::PUT, routes::API_TASKS_PAUSE).await
    }

    /// `PUT /api/v1/tasks/continue`（恢复全部）。
    pub async fn resume_all(&self) -> Result<(), ClientError> {
        self.send_unit(Method::PUT, routes::API_TASKS_CONTINUE)
            .await
    }

    /// `GET /api/v1/queues`。
    pub async fn list_queues(&self) -> Result<Vec<QueueDto>, ClientError> {
        self.send_json(Method::GET, routes::API_QUEUES, NO_BODY)
            .await
    }

    /// `GET /api/v1/rss`。
    pub async fn list_rss_sources(&self) -> Result<Vec<RssSourceDto>, ClientError> {
        self.send_json(Method::GET, routes::API_RSS, NO_BODY).await
    }

    /// `POST /api/v1/rss`，返回新订阅 id。
    pub async fn create_rss_source(&self, req: &RssSourceDto) -> Result<String, ClientError> {
        let created: CreatedRssSource = self
            .send_json(Method::POST, routes::API_RSS, Some(req))
            .await?;
        Ok(created.source_id)
    }

    /// `DELETE /api/v1/rss/{id}`。
    pub async fn delete_rss_source(&self, id: &str) -> Result<(), ClientError> {
        self.send_unit(Method::DELETE, &routes::rss_source_path(id))
            .await
    }

    /// `POST /api/v1/rss/{id}/refresh`。
    pub async fn refresh_rss_source(&self, id: &str) -> Result<(), ClientError> {
        self.send_unit(Method::POST, &routes::rss_refresh_path(id))
            .await
    }

    /// `GET /api/v1/rss/{id}/items`。
    pub async fn list_rss_items(&self, id: &str) -> Result<Vec<RssItemDto>, ClientError> {
        self.send_json(Method::GET, &routes::rss_items_path(id), NO_BODY)
            .await
    }
}
