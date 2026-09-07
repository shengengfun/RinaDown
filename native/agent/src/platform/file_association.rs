//! `.torrent` 文件关联。
//!
//! Windows 通过 HKCU 注册表指向同级 `fluxdown-desktop.exe`（与 Inno Setup
//! 安装器写入的结构一致）：
//! ```text
//! HKCU\Software\Classes\.torrent                                → "FluxDown.TorrentFile"
//! HKCU\Software\Classes\FluxDown.TorrentFile                    → "BitTorrent File"
//! HKCU\Software\Classes\FluxDown.TorrentFile\DefaultIcon        → "<exe>,0"
//! HKCU\Software\Classes\FluxDown.TorrentFile\shell\open\command → "\"<exe>\" \"%1\""
//! ```
//! 运行期直接写 winreg，不在安装器 [Registry] 跟踪范围内，因此
//! `installer/windows/setup.iss` 卸载时显式执行 `RemoveTorrentAssociation`
//! ——两处需保持同步。
//!
//! Linux 通过 `xdg-mime` 把 `application/x-bittorrent` 交给打包的
//! `com.fluxdown.app.desktop`；macOS 通过 Launch Services 把
//! `org.bittorrent.torrent` UTI（`macos/Runner/Info.plist` 已声明）交给当前
//! `.app` bundle。

#[cfg(target_os = "windows")]
mod inner {
    use std::path::Path;

    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};

    use crate::platform::{PlatformError, registry_executable};

    const PROG_ID: &str = "FluxDown.TorrentFile";
    const PROG_DESC: &str = "BitTorrent File";
    const EXT: &str = ".torrent";

    pub fn supported(desktop: Option<&Path>) -> bool {
        desktop.is_some()
    }

    /// `.torrent` 当前是否关联到 FluxDown。
    ///
    /// 只比较 `HKCU\Software\Classes\.torrent` 默认值是否为
    /// `FluxDown.TorrentFile`，不比较命令中的 exe 路径：安装器与运行进程的路径
    /// 表示可能不同（UNC 前缀、大小写、短名），ProgID 足以确认归属。
    pub fn is_associated() -> bool {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let Ok(ext_key) =
            hkcu.open_subkey_with_flags(format!("Software\\Classes\\{EXT}"), KEY_READ)
        else {
            return false;
        };
        ext_key
            .get_value::<String, _>("")
            .is_ok_and(|prog_id| prog_id == PROG_ID)
    }

    /// 把 `.torrent` 关联到桌面程序。
    pub fn associate(desktop: Option<&Path>) -> Result<(), PlatformError> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let exe = registry_executable(desktop)?;

        let (ext_key, _) =
            hkcu.create_subkey_with_flags(format!("Software\\Classes\\{EXT}"), KEY_WRITE)?;
        ext_key.set_value("", &PROG_ID)?;

        let (prog_key, _) =
            hkcu.create_subkey_with_flags(format!("Software\\Classes\\{PROG_ID}"), KEY_WRITE)?;
        prog_key.set_value("", &PROG_DESC)?;

        let (icon_key, _) = hkcu.create_subkey_with_flags(
            format!("Software\\Classes\\{PROG_ID}\\DefaultIcon"),
            KEY_WRITE,
        )?;
        icon_key.set_value("", &format!("\"{exe}\",0"))?;

        let (cmd_key, _) = hkcu.create_subkey_with_flags(
            format!("Software\\Classes\\{PROG_ID}\\shell\\open\\command"),
            KEY_WRITE,
        )?;
        cmd_key.set_value("", &format!("\"{exe}\" \"%1\""))?;

        crate::platform::windows_shell::notify_association_changed();
        tracing::info!(exe, "associated .torrent with FluxDown");
        Ok(())
    }

    /// 移除 FluxDown 的 `.torrent` 关联；其他程序的关联不受影响。
    pub fn disassociate() -> Result<(), PlatformError> {
        if !is_associated() {
            tracing::info!(".torrent not associated to FluxDown, skipping removal");
            return Ok(());
        }
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let classes = hkcu.open_subkey_with_flags("Software\\Classes", KEY_WRITE)?;
        let _ = classes.delete_subkey_all(EXT);
        let _ = classes.delete_subkey_all(PROG_ID);
        crate::platform::windows_shell::notify_association_changed();
        tracing::info!("removed .torrent association");
        Ok(())
    }
}

#[cfg(target_os = "linux")]
mod inner {
    use std::path::Path;

    use crate::platform::PlatformError;
    use crate::platform::xdg::{DESKTOP_ENTRY, query_default_is_fluxdown, remove_default};

    const MIME_TYPE: &str = "application/x-bittorrent";

    pub fn supported(_desktop: Option<&Path>) -> bool {
        true
    }

    /// `.torrent` 当前是否关联到 FluxDown。
    pub fn is_associated() -> bool {
        query_default_is_fluxdown(MIME_TYPE)
    }

    /// 把 FluxDown 注册为 `.torrent` 默认处理程序。
    ///
    /// 要求打包的 `com.fluxdown.app.desktop` 已安装到 XDG applications 目录。
    pub fn associate(_desktop: Option<&Path>) -> Result<(), PlatformError> {
        let status = std::process::Command::new("xdg-mime")
            .args(["default", DESKTOP_ENTRY, MIME_TYPE])
            .status()?;
        if !status.success() {
            return Err(PlatformError::Failed(format!(
                "xdg-mime default exited with {status}"
            )));
        }
        tracing::info!("associated .torrent with FluxDown");
        Ok(())
    }

    /// 移除用户级覆盖，把 `.torrent` 交还系统默认。
    pub fn disassociate() -> Result<(), PlatformError> {
        remove_default(MIME_TYPE)?;
        tracing::info!("removed .torrent association");
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod inner {
    use std::path::Path;

    use crate::platform::PlatformError;
    use crate::platform::macos_cf::{
        CFStringRef, CfOwned, cf_string, cf_to_string, main_bundle_id,
    };

    /// Info.plist 中声明的 `.torrent` UTI。
    const TORRENT_UTI: &str = "org.bittorrent.torrent";
    /// `kLSRolesAll`。
    const LS_ROLES_ALL: u32 = 0xFFFF_FFFF;

    #[link(name = "CoreServices", kind = "framework")]
    unsafe extern "C" {
        fn LSCopyDefaultRoleHandlerForContentType(
            content_type: CFStringRef,
            role: u32,
        ) -> CFStringRef;
        fn LSSetDefaultRoleHandlerForContentType(
            content_type: CFStringRef,
            role: u32,
            handler_bundle_id: CFStringRef,
        ) -> i32;
    }

    pub fn supported(_desktop: Option<&Path>) -> bool {
        main_bundle_id().is_some()
    }

    /// `.torrent` 当前是否关联到本 bundle。
    pub fn is_associated() -> bool {
        let Ok(uti) = cf_string(TORRENT_UTI) else {
            return false;
        };
        // SAFETY: `uti.raw()` 是有效 CFStringRef；返回的 handler 引用归我们所有，
        // 由 `CfOwned` 释放。
        let handler = CfOwned::new(unsafe {
            LSCopyDefaultRoleHandlerForContentType(uti.raw(), LS_ROLES_ALL)
        });
        let Some(handler_id) = cf_to_string(handler.raw()) else {
            return false;
        };
        main_bundle_id().is_some_and(|mine| handler_id.eq_ignore_ascii_case(&mine))
    }

    /// 把本 bundle 设为 `.torrent` 默认处理程序。
    ///
    /// bundle 首次被系统扫描或启动时即已向 Launch Services 登记其 Info.plist
    /// 中声明的 UTI。
    pub fn associate(_desktop: Option<&Path>) -> Result<(), PlatformError> {
        let bundle_id = main_bundle_id().ok_or(PlatformError::Unsupported(
            "fluxdown-agent is not running inside an app bundle",
        ))?;
        let uti = cf_string(TORRENT_UTI)?;
        let id = cf_string(&bundle_id)?;
        // SAFETY: 两个 CFStringRef 在调用期间存活；函数不接管所有权。
        let status =
            unsafe { LSSetDefaultRoleHandlerForContentType(uti.raw(), LS_ROLES_ALL, id.raw()) };
        if status != 0 {
            return Err(PlatformError::Failed(format!(
                "LSSetDefaultRoleHandlerForContentType failed (OSStatus={status})"
            )));
        }
        tracing::info!(bundle_id, "associated .torrent with FluxDown");
        Ok(())
    }

    /// 把 `.torrent` 交还系统默认；仅在当前由本 bundle 持有时执行。
    pub fn disassociate() -> Result<(), PlatformError> {
        if !is_associated() {
            tracing::info!(".torrent not associated to FluxDown, skipping removal");
            return Ok(());
        }
        let uti = cf_string(TORRENT_UTI)?;
        let empty = cf_string("")?;
        // SAFETY: 两个 CFStringRef 在调用期间存活。
        let status =
            unsafe { LSSetDefaultRoleHandlerForContentType(uti.raw(), LS_ROLES_ALL, empty.raw()) };
        if status != 0 {
            return Err(PlatformError::Failed(format!(
                "LSSetDefaultRoleHandlerForContentType (clear) failed (OSStatus={status})"
            )));
        }
        tracing::info!("removed .torrent association");
        Ok(())
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
mod inner {
    use std::path::Path;

    use crate::platform::PlatformError;

    pub fn supported(_desktop: Option<&Path>) -> bool {
        false
    }

    pub fn is_associated() -> bool {
        false
    }

    pub fn associate(_desktop: Option<&Path>) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported(
            "file association is not supported on this platform",
        ))
    }

    pub fn disassociate() -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported(
            "file association is not supported on this platform",
        ))
    }
}

pub use inner::{associate, disassociate, is_associated, supported};
