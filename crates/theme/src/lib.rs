//! FluxDown GPUI 客户端的主题 schema、Flutter 默认值与运行时装配。
//!
//! 本 crate 是主题单一入口：完整保存 gpui-base 的颜色、圆角、间距、排版和
//! 阴影 token；将 legacy 可表达部分同步给 gpui-component，并把全部 token
//! 投影给应用自有 Base 组件。业务 feature 不在本 crate 中。

mod appearance;
mod definition;
mod manager;

pub use appearance::*;
pub use definition::*;
pub use gpui_base::{
    ColorTokens, RadiusTokens, SemanticThemeTokens, ShadowTokens, SpacingTokens, TextStyleToken,
    TypographyTokens,
};
pub use gpui_component::ThemeMode;
pub use manager::*;
