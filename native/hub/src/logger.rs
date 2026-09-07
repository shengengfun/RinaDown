//! 日志 shim —— 转发到 `fluxdown_engine::logger`。
//!
//! 实际实现已随其它零 rinf 耦合的模块迁移到 `fluxdown_engine`(引擎需要
//! 独立于 hub 记录自身日志)。此处保留 `crate::logger::*` 路径,使 hub 内
//! App-shell 专属文件(`native_messaging.rs`/`http_takeover.rs`/
//! `updater.rs`/`reveal_file.rs`/`file_association.rs`/
//! `protocol_registry.rs`/`nmh_registry.rs`/`actors/`)现有的
//! `use crate::logger::log_info;` 等导入零改动继续工作。
pub use fluxdown_engine::logger::*;
