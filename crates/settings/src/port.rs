//! 设置能力的宿主端口：由 app 注入单一 agent 会话，本 crate 只知道方法名与 JSON。

use std::pin::Pin;

pub type PortFuture<T> =
    Pin<Box<dyn Future<Output = Result<T, fluxdown_protocol::RpcErrorData>> + Send + 'static>>;

pub trait SettingsPort: Send + Sync {
    fn call(
        &self,
        method: &'static str,
        params: serde_json::Value,
    ) -> PortFuture<serde_json::Value>;
}
