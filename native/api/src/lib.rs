//! `fluxdown_api` —— FluxDown 本机 HTTP API：契约 + axum 服务器，零 FFI 依赖。
//!
//! 本 crate 把 HTTP 传输与宿主实现拆开：
//!
//! - `fluxdown_protocol` —— 唯一 wire JSON 契约（camelCase）
//! - [`routes`] —— 路径常量
//! - [`service`] —— [`ApiHost`](service::ApiHost) trait：宿主能力契约
//! - [`server`] —— axum 服务器（探活 / 脚本接管 / aria2 / 管理 API）
//!
//! 宿主各自实现 `ApiHost`；本 crate 不依赖下载引擎。
//!
//! # Examples
//!
//! 用一个内存宿主启动服务器（宿主形态无关的最小示例）：
//!
//! ```no_run
//! use std::sync::Arc;
//! use async_trait::async_trait;
//! use fluxdown_api::server::{ApiServerConfig, spawn_api_server};
//! use fluxdown_api::service::{ApiError, ApiHost};
//! use fluxdown_protocol::daemon::{CreateTaskRequest, DownloadRequest, QueueDto, TaskDto};
//!
//! struct MyHost;
//!
//! #[async_trait]
//! impl ApiHost for MyHost {
//!     async fn list_tasks(&self) -> Result<Vec<TaskDto>, ApiError> { Ok(vec![]) }
//!     async fn get_task(&self, _id: &str) -> Result<Option<TaskDto>, ApiError> { Ok(None) }
//!     async fn create_task(&self, _req: CreateTaskRequest) -> Result<String, ApiError> {
//!         Ok("task-id".to_string())
//!     }
//!     async fn delete_task(&self, _id: &str, _files: bool) -> Result<(), ApiError> { Ok(()) }
//!     async fn pause_task(&self, _id: &str) -> Result<(), ApiError> { Ok(()) }
//!     async fn continue_task(&self, _id: &str) -> Result<(), ApiError> { Ok(()) }
//!     async fn pause_all(&self) -> Result<(), ApiError> { Ok(()) }
//!     async fn continue_all(&self) -> Result<(), ApiError> { Ok(()) }
//!     async fn list_queues(&self) -> Result<Vec<QueueDto>, ApiError> { Ok(vec![]) }
//!     async fn submit_external(&self, _req: DownloadRequest) -> Result<(), ApiError> { Ok(()) }
//! }
//!
//! # async fn run() {
//! let config = ApiServerConfig::from_config_map(&std::collections::HashMap::new(), "1.0.0");
//! let handle = spawn_api_server(Arc::new(MyHost), config);
//! // ... 配置变更时：
//! handle.shutdown();
//! # }
//! ```

mod aria2;
pub mod auth;
mod jsonrpc;
mod jsonrpc_ws;
mod mcp;
pub mod openapi;
pub mod routes;
pub mod server;
pub mod service;
mod takeover;
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
