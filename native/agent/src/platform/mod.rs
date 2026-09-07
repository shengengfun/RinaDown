//! agent 桌面系统集成：任务文件打开/定位、官方桌面进程唤起、开机自启、
//! `.torrent` 关联与 URL scheme 注册。
//!
//! 所有注册的目标都是官方桌面程序而非 agent 自身：Windows 指向同级
//! `fluxdown-desktop.exe`，macOS 指向 agent 所在的 `.app` bundle，Linux 指向
//! 打包的 `com.fluxdown.app.desktop`。全部函数同步阻塞，RPC 侧需放入
//! `spawn_blocking`。

mod autostart;
mod file_association;
#[cfg(target_os = "macos")]
mod macos_cf;
mod protocol_registry;

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};

use fluxdown_protocol::PlatformIntegrationDto;

static DESKTOP_LAUNCHED: AtomicBool = AtomicBool::new(false);

const DESKTOP_EXECUTABLE_NAME: &str = if cfg!(windows) {
    "fluxdown-desktop.exe"
} else {
    "fluxdown-desktop"
};

/// 与 agent 同目录的官方桌面程序；文件不存在时返回 `None`。
#[must_use]
pub fn desktop_executable() -> Option<PathBuf> {
    let path = std::env::current_exe()
        .ok()?
        .with_file_name(DESKTOP_EXECUTABLE_NAME);
    path.is_file().then_some(path)
}

pub fn open_task(task: &fluxdown_protocol::TaskDto) -> Result<(), PlatformError> {
    launch_path(&PathBuf::from(&task.save_dir).join(&task.file_name), false)
}

pub fn reveal_task(task: &fluxdown_protocol::TaskDto) -> Result<(), PlatformError> {
    launch_path(&PathBuf::from(&task.save_dir).join(&task.file_name), true)
}

/// 用系统默认程序打开 `path`；`reveal` 为 true 时改为在文件管理器中定位。
pub fn open_path(path: &Path, reveal: bool) -> Result<(), PlatformError> {
    launch_path(path, reveal)
}

/// 首个待确认捕获在无 UI 时只拉起一次同级桌面程序。
pub fn launch_desktop_once() -> Result<(), PlatformError> {
    if DESKTOP_LAUNCHED.swap(true, Ordering::AcqRel) {
        return Ok(());
    }
    let executable = desktop_executable().ok_or(PlatformError::Unsupported(
        "fluxdown-desktop is not installed next to fluxdown-agent",
    ))?;
    let mut command = std::process::Command::new(executable);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    set_no_console_window(&mut command);
    command.spawn()?;
    Ok(())
}

/// 当前系统集成状态快照。
#[must_use]
pub fn integration_status() -> PlatformIntegrationDto {
    let desktop = desktop_executable();
    let target = desktop.as_deref();
    let url_protocols = protocol_registry::SCHEMES
        .iter()
        .map(|scheme| {
            (
                scheme.scheme.to_owned(),
                protocol_registry::is_registered(*scheme, target),
            )
        })
        .collect();
    PlatformIntegrationDto {
        autostart_supported: autostart::supported(target),
        autostart_enabled: target.is_some_and(autostart::is_enabled),
        file_association_supported: file_association::supported(target),
        torrent_associated: file_association::is_associated(),
        url_protocol_supported: protocol_registry::supported(target),
        url_protocols,
        desktop_executable: desktop
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
    }
}

pub fn set_autostart(enabled: bool) -> Result<(), PlatformError> {
    if !enabled {
        return autostart::disable();
    }
    let desktop = desktop_executable().ok_or(PlatformError::Unsupported(
        "fluxdown-desktop is not installed next to fluxdown-agent",
    ))?;
    autostart::enable(&desktop)
}

pub fn set_file_association(enabled: bool) -> Result<(), PlatformError> {
    if enabled {
        file_association::associate(desktop_executable().as_deref())
    } else {
        file_association::disassociate()
    }
}

/// `scheme` 只接受 `magnet` / `ed2k` / `fluxdown`。
pub fn set_url_protocol(scheme: &str, enabled: bool) -> Result<(), PlatformError> {
    let scheme = protocol_registry::from_name(scheme)
        .ok_or_else(|| PlatformError::InvalidScheme(scheme.to_owned()))?;
    let desktop = desktop_executable();
    if enabled {
        protocol_registry::register(scheme, desktop.as_deref())
    } else {
        protocol_registry::unregister(scheme, desktop.as_deref())
    }
}

#[cfg(target_os = "linux")]
fn launch_path(path: &Path, _reveal: bool) -> Result<(), PlatformError> {
    std::process::Command::new("xdg-open")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn launch_path(path: &Path, reveal: bool) -> Result<(), PlatformError> {
    let mut command = std::process::Command::new("open");
    if reveal {
        command.arg("-R");
    }
    command.arg(path).spawn()?;
    Ok(())
}

#[cfg(windows)]
fn launch_path(path: &Path, reveal: bool) -> Result<(), PlatformError> {
    let mut command = if reveal {
        let mut command = std::process::Command::new("explorer.exe");
        command.arg(format!("/select,{}", path.display()));
        command
    } else {
        let mut command = std::process::Command::new("cmd.exe");
        command.arg("/c").arg("start").arg("").arg(path);
        command
    };
    set_no_console_window(&mut command);
    command.spawn()?;
    Ok(())
}

#[cfg(windows)]
fn set_no_console_window(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x0800_0000);
}

#[cfg(not(windows))]
fn set_no_console_window(_command: &mut std::process::Command) {}

/// 注册表命令行使用的桌面程序路径：canonicalize 解析符号链接后去掉 `\\?\`
/// 前缀，便于与安装器写入的值比较。
#[cfg(windows)]
fn registry_executable(desktop: Option<&Path>) -> Result<String, PlatformError> {
    let path = desktop.ok_or(PlatformError::Unsupported(
        "fluxdown-desktop.exe is not installed next to fluxdown-agent",
    ))?;
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let text = canonical.to_string_lossy();
    Ok(text.strip_prefix(r"\\?\").unwrap_or(&*text).to_owned())
}

#[cfg(windows)]
mod windows_shell {
    /// `SHChangeNotify(SHCNE_ASSOCCHANGED)` 通知资源管理器关联已变化。
    ///
    /// 直接声明 FFI，避免为一个符号引入 `windows-sys` 的 `Win32_UI_Shell`。
    pub fn notify_association_changed() {
        #[link(name = "shell32")]
        unsafe extern "system" {
            fn SHChangeNotify(
                wEventId: i32,
                uFlags: u32,
                dwItem1: *const std::ffi::c_void,
                dwItem2: *const std::ffi::c_void,
            );
        }
        // SAFETY: SHCNE_ASSOCCHANGED (0x0800_0000) + SHCNF_IDLIST (0) 不读取
        // item 指针，传 null 合法。
        unsafe {
            SHChangeNotify(0x0800_0000, 0, std::ptr::null(), std::ptr::null());
        }
    }
}

#[cfg(target_os = "linux")]
mod xdg {
    use std::io::{BufRead, Write};

    use super::PlatformError;

    /// 打包安装的桌面入口（`linux/com.fluxdown.app.desktop`）。
    pub const DESKTOP_ENTRY: &str = "com.fluxdown.app.desktop";

    /// `xdg-mime query default <mime>` 是否返回 FluxDown 的桌面入口。
    pub fn query_default_is_fluxdown(mime: &str) -> bool {
        let Ok(output) = std::process::Command::new("xdg-mime")
            .args(["query", "default", mime])
            .output()
        else {
            return false;
        };
        String::from_utf8_lossy(&output.stdout)
            .to_lowercase()
            .contains("fluxdown")
    }

    /// 从 `~/.config/mimeapps.list` 删除指向 FluxDown 的 `<mime>=…` 行。
    ///
    /// xdg-mime 没有“取消默认”命令，只能直接编辑用户覆盖文件。
    pub fn remove_default(mime: &str) -> Result<(), PlatformError> {
        let base = directories::BaseDirs::new()
            .ok_or(PlatformError::Unsupported("home directory unavailable"))?;
        let path = base.config_dir().join("mimeapps.list");
        if !path.exists() {
            return Ok(());
        }
        let file = std::fs::File::open(&path)?;
        let lines = std::io::BufReader::new(file)
            .lines()
            .collect::<Result<Vec<String>, _>>()?;
        let prefix = format!("{}=", mime.to_lowercase());
        let mut out = std::fs::File::create(&path)?;
        for line in lines {
            let lower = line.to_lowercase();
            if lower.starts_with(&prefix) && lower.contains("fluxdown") {
                continue;
            }
            writeln!(out, "{line}")?;
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("platform action failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("platform integration unsupported: {0}")]
    Unsupported(&'static str),
    #[error("platform integration failed: {0}")]
    Failed(String),
    #[error("unknown URL scheme: {0}")]
    InvalidScheme(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_executable_lives_next_to_agent() {
        let current = std::env::current_exe().expect("current exe");
        let sibling = current.with_file_name(DESKTOP_EXECUTABLE_NAME);
        assert_eq!(desktop_executable(), sibling.is_file().then_some(sibling));
    }

    #[test]
    fn integration_status_reports_all_schemes() {
        let status = integration_status();
        assert_eq!(
            status.url_protocols.keys().cloned().collect::<Vec<_>>(),
            ["ed2k", "fluxdown", "magnet"]
        );
        assert!(!status.autostart_supported || !status.desktop_executable.is_empty());
    }

    #[test]
    fn unknown_scheme_is_rejected_before_touching_the_system() {
        assert!(matches!(
            set_url_protocol("javascript", true),
            Err(PlatformError::InvalidScheme(_))
        ));
    }
}
