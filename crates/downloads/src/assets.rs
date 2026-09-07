use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

/// 下载能力在活动栏使用的图标路径。
pub const DOWNLOAD_ICON_PATH: &str = "fluxdown/icons/download.svg";
const DOWNLOAD_ICON: &[u8] = include_bytes!("../assets/download.svg");

/// 下载能力拥有的嵌入资源。
pub struct DownloadAssets;

impl AssetSource for DownloadAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok((path == DOWNLOAD_ICON_PATH).then_some(Cow::Borrowed(DOWNLOAD_ICON)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(if DOWNLOAD_ICON_PATH.starts_with(path) {
            vec![DOWNLOAD_ICON_PATH.into()]
        } else {
            Vec::new()
        })
    }
}
