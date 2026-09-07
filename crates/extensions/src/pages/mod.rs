//! 扩展分类的两个子页：插件（已安装 / 安装 / 市场）与受管组件（ffmpeg / yt-dlp）。

use fluxdown_ui_i18n::Translator;
use gpui_component::Theme;

pub mod managed_components;
pub mod plugins;

/// 一帧内不变的渲染输入：文案、主题与本地服务连接状态。
#[derive(Clone, Copy)]
pub(crate) struct Frame<'a> {
    pub translator: &'a Translator,
    pub theme: &'a Theme,
    pub stale: bool,
}
