//! GPUI 设置能力页面与设置分区。
//!
//! 设置通过共享的翻译 Entity 和主题全局状态更新 UI，不依赖其他业务能力；
//! 全部读写经 [`SettingsPort`] 注入的单一 agent 会话。

mod port;
mod sections;
mod store;
mod view;

pub use port::{PortFuture, SettingsPort};
pub use store::{SettingsError, SettingsErrorKind, SettingsStore};
pub use view::{SettingsContentSlots, SettingsView};

/// 将 FluxDown locale 映射为 gpui-component 支持的 locale。
pub fn component_locale(locale: &str) -> &str {
    if locale == "zh" { "zh-CN" } else { "en" }
}
