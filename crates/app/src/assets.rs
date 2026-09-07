use std::borrow::Cow;

use fluxdown_ui_downloads::DownloadAssets;
use fluxdown_ui_shell::ShellAssets;
use gpui::{AssetSource, Result, SharedString};

/// composition root 组合 shell、能力 crate 与 gpui-component 的资源。
pub(crate) struct DesktopAssets;

impl AssetSource for DesktopAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(asset) = ShellAssets.load(path)? {
            return Ok(Some(asset));
        }
        if let Some(asset) = DownloadAssets.load(path)? {
            return Ok(Some(asset));
        }
        gpui_component_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut assets = gpui_component_assets::Assets.list(path)?;
        assets.extend(ShellAssets.list(path)?);
        assets.extend(DownloadAssets.list(path)?);
        Ok(assets)
    }
}

#[cfg(test)]
mod tests {
    use fluxdown_ui_downloads::DOWNLOAD_ICON_PATH;
    use fluxdown_ui_shell::APP_LOGO_PATH;
    use gpui::AssetSource;

    use super::DesktopAssets;

    #[test]
    fn desktop_assets_cover_shell_capabilities_and_component_icons() -> gpui::Result<()> {
        let assets = DesktopAssets;
        assert!(assets.load(APP_LOGO_PATH)?.is_some());
        assert!(assets.load(DOWNLOAD_ICON_PATH)?.is_some());
        assert!(assets.load("icons/window-close.svg")?.is_some());
        Ok(())
    }
}
