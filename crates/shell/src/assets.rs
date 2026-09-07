use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

/// FluxDown 品牌图标路径。
pub const APP_LOGO_PATH: &str = "fluxdown/logo.png";
const APP_LOGO: &[u8] = include_bytes!("../../../assets/logo/fluxdown_logo.png");

/// 窗口 shell 拥有的嵌入资源。
pub struct ShellAssets;

impl AssetSource for ShellAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok((path == APP_LOGO_PATH).then_some(Cow::Borrowed(APP_LOGO)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(if APP_LOGO_PATH.starts_with(path) {
            vec![APP_LOGO_PATH.into()]
        } else {
            Vec::new()
        })
    }
}

#[cfg(test)]
mod tests {
    use gpui::AssetSource;

    use super::{APP_LOGO, APP_LOGO_PATH, ShellAssets};

    #[test]
    fn shell_assets_preserve_brand_icon() -> gpui::Result<()> {
        assert_eq!(ShellAssets.load(APP_LOGO_PATH)?.as_deref(), Some(APP_LOGO));
        Ok(())
    }
}
