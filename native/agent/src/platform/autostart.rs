//! 开机自启：登录时以 `--minimized` 拉起同级 `fluxdown-desktop`。
//!
//! - Windows：`HKCU\Software\Microsoft\Windows\CurrentVersion\Run` 的 `FluxDown`
//!   值（与 Flutter 时代 `launch_at_startup` 及 `installer/windows/setup.iss`
//!   的 `RemoveAutostartRunValue` 使用同一值名）。
//! - macOS：`~/Library/LaunchAgents/dev.zerx.fluxdown.desktop.plist`（RunAtLoad）。
//! - Linux：`~/.config/autostart/fluxdown.desktop`（XDG autostart）。
//!
//! “已启用”要求条目指向当前桌面程序；程序移动/升级后旧条目视为未启用，
//! 用户重新开启即覆盖为新路径。

#[cfg(target_os = "windows")]
mod inner {
    use std::path::Path;

    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};

    use crate::platform::{PlatformError, registry_executable};

    const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    const VALUE_NAME: &str = "FluxDown";

    pub fn supported(desktop: Option<&Path>) -> bool {
        desktop.is_some()
    }

    fn command_line(exe: &str) -> String {
        format!("\"{exe}\" --minimized")
    }

    pub fn is_enabled(desktop: &Path) -> bool {
        let Ok(exe) = registry_executable(Some(desktop)) else {
            return false;
        };
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let Ok(key) = hkcu.open_subkey_with_flags(RUN_KEY, KEY_READ) else {
            return false;
        };
        key.get_value::<String, _>(VALUE_NAME)
            .is_ok_and(|value| value.eq_ignore_ascii_case(&command_line(&exe)))
    }

    pub fn enable(desktop: &Path) -> Result<(), PlatformError> {
        let exe = registry_executable(Some(desktop))?;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (key, _) = hkcu.create_subkey_with_flags(RUN_KEY, KEY_WRITE)?;
        key.set_value(VALUE_NAME, &command_line(&exe))?;
        tracing::info!(exe, "enabled autostart");
        Ok(())
    }

    pub fn disable() -> Result<(), PlatformError> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = match hkcu.open_subkey_with_flags(RUN_KEY, KEY_WRITE) {
            Ok(key) => key,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        match key.delete_value(VALUE_NAME) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        tracing::info!("disabled autostart");
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod inner {
    use std::path::{Path, PathBuf};

    use crate::platform::PlatformError;

    const LABEL: &str = "dev.zerx.fluxdown.desktop";

    pub fn supported(desktop: Option<&Path>) -> bool {
        desktop.is_some()
    }

    fn plist_path() -> Result<PathBuf, PlatformError> {
        let base = directories::BaseDirs::new()
            .ok_or(PlatformError::Unsupported("home directory unavailable"))?;
        Ok(base
            .home_dir()
            .join("Library")
            .join("LaunchAgents")
            .join(format!("{LABEL}.plist")))
    }

    fn xml_escape(value: &str) -> String {
        let mut out = String::with_capacity(value.len());
        for ch in value.chars() {
            match ch {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                other => out.push(other),
            }
        }
        out
    }

    fn plist_body(desktop: &Path) -> String {
        let program = xml_escape(&desktop.display().to_string());
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
             \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\">\n\
             <dict>\n\
             \t<key>Label</key>\n\
             \t<string>{LABEL}</string>\n\
             \t<key>ProgramArguments</key>\n\
             \t<array>\n\
             \t\t<string>{program}</string>\n\
             \t\t<string>--minimized</string>\n\
             \t</array>\n\
             \t<key>RunAtLoad</key>\n\
             \t<true/>\n\
             \t<key>ProcessType</key>\n\
             \t<string>Interactive</string>\n\
             </dict>\n\
             </plist>\n"
        )
    }

    pub fn is_enabled(desktop: &Path) -> bool {
        let Ok(path) = plist_path() else {
            return false;
        };
        std::fs::read_to_string(path)
            .is_ok_and(|content| content.contains(&xml_escape(&desktop.display().to_string())))
    }

    pub fn enable(desktop: &Path) -> Result<(), PlatformError> {
        let path = plist_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, plist_body(desktop))?;
        tracing::info!(path = %path.display(), "enabled autostart");
        Ok(())
    }

    pub fn disable() -> Result<(), PlatformError> {
        let path = plist_path()?;
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        tracing::info!(path = %path.display(), "disabled autostart");
        Ok(())
    }
}

#[cfg(target_os = "linux")]
mod inner {
    use std::path::{Path, PathBuf};

    use crate::platform::PlatformError;

    pub fn supported(desktop: Option<&Path>) -> bool {
        desktop.is_some()
    }

    fn entry_path() -> Result<PathBuf, PlatformError> {
        let base = directories::BaseDirs::new()
            .ok_or(PlatformError::Unsupported("home directory unavailable"))?;
        Ok(base.config_dir().join("autostart").join("fluxdown.desktop"))
    }

    /// 按 Desktop Entry 规范为 `Exec` 引用并转义参数。
    fn exec_quote(value: &str) -> String {
        let mut out = String::with_capacity(value.len() + 2);
        out.push('"');
        for ch in value.chars() {
            if matches!(ch, '"' | '`' | '$' | '\\') {
                out.push('\\');
            }
            out.push(ch);
        }
        out.push('"');
        out
    }

    fn entry_body(desktop: &Path) -> String {
        let exec = exec_quote(&desktop.display().to_string());
        format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=FluxDown\n\
             Comment=Free IDM-alternative download manager\n\
             Exec={exec} --minimized\n\
             Icon=com.fluxdown.app\n\
             Terminal=false\n\
             X-GNOME-Autostart-enabled=true\n"
        )
    }

    pub fn is_enabled(desktop: &Path) -> bool {
        let Ok(path) = entry_path() else {
            return false;
        };
        std::fs::read_to_string(path)
            .is_ok_and(|content| content.contains(&exec_quote(&desktop.display().to_string())))
    }

    pub fn enable(desktop: &Path) -> Result<(), PlatformError> {
        let path = entry_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, entry_body(desktop))?;
        tracing::info!(path = %path.display(), "enabled autostart");
        Ok(())
    }

    pub fn disable() -> Result<(), PlatformError> {
        let path = entry_path()?;
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        tracing::info!(path = %path.display(), "disabled autostart");
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

    pub fn is_enabled(_desktop: &Path) -> bool {
        false
    }

    pub fn enable(_desktop: &Path) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported(
            "autostart is not supported on this platform",
        ))
    }

    pub fn disable() -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported(
            "autostart is not supported on this platform",
        ))
    }
}

pub use inner::{disable, enable, is_enabled, supported};
