//! GPUI 下载能力页面、领域组件与视图状态。
//!
//! 本 crate 不负责窗口导航或应用初始化；宿主在 composition root 中创建
//! [`DownloadView`] 并作为路由内容注入 shell。

mod assets;
mod components;
mod controller;
mod model;
mod pages;
mod strings;

pub use assets::*;
pub use controller::{
    DownloadsCommand, DownloadsController, DownloadsPort, DownloadsResult, LAST_SAVE_DIR_PREF,
    PortFuture, REMEMBER_LAST_SAVE_DIR_PREF,
};
pub use pages::downloads::*;
pub use pages::new_download::*;
