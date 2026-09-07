//! FluxDown CLI
//!
//! 复用 [`fluxdown_api`] 的路径常量与 wire 类型作 typed HTTP 客户端，
//! 与运行中的 FluxDown App / headless server 通信（本机 API 服务，
//! 默认 `127.0.0.1:17800`，管理 API 强制 token 鉴权）。
//!
//! 库 crate 仅导出可测试的纯逻辑单元（格式化、退出码、客户端、持久化配置），
//! CLI 入口在 `main.rs`。

pub mod client;
pub mod config;
pub mod exit;
pub mod format;
