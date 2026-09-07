//! FluxDown GPUI 桌面窗口 shell。
//!
//! 本 crate 只提供窗口 chrome、活动栏、路由和内容槽位；业务页面由 app
//! 创建后以 [`ShellRoute`] 注入。

mod assets;
mod strings;
mod view;

use gpui::{SharedString, WindowDecorations, WindowOptions, size};
use gpui_component::TitleBar;

pub use assets::*;
pub use view::*;

/// 构造 FluxDown 主窗口选项。
pub fn main_window_options() -> WindowOptions {
    let mut options = TitleBar::window_options();
    options.window_min_size = Some(size(gpui::px(720.), gpui::px(520.)));
    options.window_decorations = Some(WindowDecorations::Client);
    options
}

/// 构造使用 FluxDown 自定义标题栏的辅助窗口选项。
pub fn auxiliary_window_options(title: impl Into<SharedString>) -> WindowOptions {
    let mut options = TitleBar::window_options();
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.title = Some(title.into());
    }
    options.window_min_size = Some(size(gpui::px(720.), gpui::px(520.)));
    options.window_decorations = Some(WindowDecorations::Client);
    options
}

#[cfg(test)]
mod tests {
    use gpui::{point, px, size};

    use super::{auxiliary_window_options, main_window_options};

    #[test]
    fn main_window_preserves_custom_titlebar_platform_contract() {
        let options = main_window_options();

        assert!(options.app_owns_titlebar_drag);
        assert_eq!(
            options
                .titlebar
                .as_ref()
                .and_then(|titlebar| titlebar.traffic_light_position),
            Some(point(px(9.), px(9.)))
        );
        assert_eq!(
            options
                .titlebar
                .as_ref()
                .and_then(|titlebar| titlebar.title.as_deref()),
            None
        );
        assert_eq!(options.window_min_size, Some(size(px(720.), px(520.))));
        assert_eq!(
            options.window_decorations,
            Some(gpui::WindowDecorations::Client)
        );
    }

    #[test]
    fn auxiliary_window_preserves_custom_titlebar_platform_contract() {
        let options = auxiliary_window_options("Settings");

        assert!(options.app_owns_titlebar_drag);
        assert_eq!(
            options
                .titlebar
                .as_ref()
                .and_then(|titlebar| titlebar.title.as_deref()),
            Some("Settings")
        );
        assert_eq!(
            options
                .titlebar
                .as_ref()
                .and_then(|titlebar| titlebar.traffic_light_position),
            Some(point(px(9.), px(9.)))
        );
        assert_eq!(options.window_min_size, Some(size(px(720.), px(520.))));
        assert_eq!(
            options.window_decorations,
            Some(gpui::WindowDecorations::Client)
        );
    }
}
