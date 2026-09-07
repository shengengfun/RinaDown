//! FluxDown 下载核心进程的应用边界。
//!
//! 本 crate 只承载本地下载、任务管理与下载功能装配；账户、云同步和 UI 状态属于
//! `fluxdown_agent` 或具体客户端。

pub mod actor;
pub mod blob_store;
pub mod config;
pub mod event_hub;
pub mod http;
pub mod rpc;
pub mod runtime;
pub mod selection;
pub mod service;

use fluxdown_protocol::{ServiceHello, ServiceRole};

/// 下载核心进程名。
pub const SERVICE_NAME: &str = "fluxdownd";

/// 返回下载核心用于协议协商的稳定身份。
#[must_use]
pub fn service_hello(instance_id: impl Into<String>, capabilities: Vec<String>) -> ServiceHello {
    ServiceHello::new(
        ServiceRole::Daemon,
        SERVICE_NAME,
        env!("CARGO_PKG_VERSION"),
        instance_id,
        capabilities,
    )
}

#[cfg(test)]
mod tests {
    use fluxdown_protocol::{PROTOCOL_VERSION, ServiceRole};

    use super::{SERVICE_NAME, service_hello};

    #[test]
    fn identifies_as_download_daemon() {
        let hello = service_hello("daemon-instance", Vec::new());

        assert_eq!(hello.role, ServiceRole::Daemon);
        assert_eq!(hello.service_name, SERVICE_NAME);
        assert_eq!(hello.protocol_version, PROTOCOL_VERSION);
        assert_eq!(hello.instance_id, "daemon-instance");
    }
}
