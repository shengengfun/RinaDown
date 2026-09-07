//! 把任务落盘产物（文件或文件夹）放进系统剪贴板，使其能在文件管理器里直接
//! `Ctrl+V` 粘贴出一份拷贝——与「复制下载地址」不同，剪贴板里放的是**文件
//! 引用**而非文本。
//!
//! | 平台    | 载体                                                                     |
//! |---------|--------------------------------------------------------------------------|
//! | Windows | `CF_HDROP`（`DROPFILES` 头 + 双 NUL 结尾宽字符路径列表），Explorer 与第三方 FM 通用 |
//! | macOS   | `osascript` 写入 «class furl»（`set the clipboard to POSIX file …`），Finder 可直接粘贴 |
//! | Linux   | `wl-copy`(Wayland) / `xclip`(X11)：GNOME 系写 `x-special/gnome-copied-files`，其余写 `text/uri-list` |
//!
//! **文件与文件夹不需要分别处理**：三种载体放的都是「路径引用」，粘贴端按路径
//! 本身在磁盘上的类型决定是复制单个文件还是整棵目录。调用方只要给出落盘对象的
//! 绝对路径（`save_dir/file_name`——BT 全选多文件种子时它就是种子根目录，其余
//! 协议与 BT 单文件时是那个文件），本模块用 `fs::metadata` 判定类型并回报，供
//! UI 区分「已复制文件夹 / 已复制文件」。
//!
//! 错误一律返回稳定的短码，由 Dart 侧映射为本地化提示：
//! - `not_found`   — 路径已不存在（外部删除/移动）
//! - `no_tool`     — Linux 上既没有 `wl-copy` 也没有 `xclip`
//! - `unsupported` — 该平台没有桌面剪贴板概念（移动端）
//! - `os:<detail>` — 系统调用/子进程失败，`<detail>` 仅供日志与反馈

use std::path::Path;

use crate::logger::log_info;

/// 把 `path` 指向的文件或文件夹放进系统剪贴板。
///
/// 返回 `Ok(true)` 表示放进剪贴板的是**文件夹**，`Ok(false)` 表示是单个文件。
/// 错误码见模块文档。
pub fn copy_path(path: &str) -> Result<bool, String> {
    let target = Path::new(path);
    let is_dir = match std::fs::metadata(target) {
        Ok(meta) => meta.is_dir(),
        Err(e) => {
            log_info!("[clipboard] copy target unavailable: {path} ({e})");
            return Err("not_found".to_string());
        }
    };
    platform_copy(target)?;
    log_info!(
        "[clipboard] copied {} to clipboard: {path}",
        if is_dir { "dir" } else { "file" }
    );
    Ok(is_dir)
}

// ---------------------------------------------------------------------------
// Windows：CF_HDROP
// ---------------------------------------------------------------------------

/// Win32 `CF_HDROP` 剪贴板格式号。windows-sys 把这个常量放在 `Win32_System_Ole`
/// feature 下，为一个 `u16` 拉进整个 Ole 模块不值当，按头文件取字面值。
#[cfg(target_os = "windows")]
const CF_HDROP: u32 = 15;

/// Windows：构造 `CF_HDROP` 数据块（`DROPFILES` 头 + 宽字符路径列表）。
///
/// 布局（Win32 约定）：头紧跟路径列表，列表每条以 NUL 结尾、整体再补一个 NUL
/// 收尾；`pFiles` 是列表相对块首的字节偏移，`fWide = TRUE` 表示列表是 UTF-16。
/// 文件夹与文件同一种编码，粘贴端自行区分。
#[cfg(target_os = "windows")]
fn build_hdrop(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::UI::Shell::DROPFILES;

    let header = size_of::<DROPFILES>();
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain([0, 0]) // 本条路径结束 + 列表结束
        .collect();

    let mut block = Vec::with_capacity(header + wide.len() * size_of::<u16>());
    block.extend_from_slice(&(header as u32).to_le_bytes()); // pFiles
    block.extend_from_slice(&0i32.to_le_bytes()); // pt.x
    block.extend_from_slice(&0i32.to_le_bytes()); // pt.y
    block.extend_from_slice(&0i32.to_le_bytes()); // fNC = FALSE
    block.extend_from_slice(&1i32.to_le_bytes()); // fWide = TRUE（UTF-16 列表）
    debug_assert_eq!(block.len(), header);
    for unit in wide {
        block.extend_from_slice(&unit.to_le_bytes());
    }
    block
}

/// Windows：把 `CF_HDROP` 数据块挂上剪贴板。
#[cfg(target_os = "windows")]
fn platform_copy(path: &Path) -> Result<(), String> {
    use windows_sys::Win32::Foundation::GlobalFree;
    use windows_sys::Win32::System::DataExchange::{EmptyClipboard, SetClipboardData};
    use windows_sys::Win32::System::Memory::{
        GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock,
    };

    let block = build_hdrop(path);

    let _clipboard = ClipboardGuard::open()?;

    // SAFETY: 剪贴板已由 _clipboard 打开并在本函数作用域内保持打开。
    if unsafe { EmptyClipboard() } == 0 {
        return Err(last_error("EmptyClipboard"));
    }

    // SAFETY: 申请 block.len() 字节可移动全局内存；失败时返回空句柄。
    let hmem = unsafe { GlobalAlloc(GMEM_MOVEABLE, block.len()) };
    if hmem.is_null() {
        return Err(last_error("GlobalAlloc"));
    }

    // SAFETY: hmem 刚由 GlobalAlloc 返回且非空；锁定得到同样大小的可写区。
    let dst = unsafe { GlobalLock(hmem) };
    if dst.is_null() {
        let err = last_error("GlobalLock");
        // SAFETY: hmem 尚未交给系统，所有权仍在本函数。
        unsafe { GlobalFree(hmem) };
        return Err(err);
    }

    // SAFETY: dst 指向恰好 block.len() 字节的可写内存，与本地 block 不重叠。
    unsafe {
        std::ptr::copy_nonoverlapping(block.as_ptr(), dst.cast::<u8>(), block.len());
        GlobalUnlock(hmem);
    }

    // SAFETY: 剪贴板打开中；成功后 hmem 所有权移交系统（不得再 Free），
    // 失败则仍归本函数释放。
    if unsafe { SetClipboardData(CF_HDROP, hmem) }.is_null() {
        let err = last_error("SetClipboardData");
        // SAFETY: SetClipboardData 失败意味着所有权未移交。
        unsafe { GlobalFree(hmem) };
        return Err(err);
    }
    Ok(())
}

/// `OpenClipboard`/`CloseClipboard` 的 RAII 配对：Win32 同一时刻只允许一个进程
/// 持有剪贴板，忘记关闭会把整个桌面的复制粘贴卡死。
#[cfg(target_os = "windows")]
struct ClipboardGuard;

#[cfg(target_os = "windows")]
impl ClipboardGuard {
    /// 打开剪贴板。失败几乎总是「别的进程正持有」（输入法、剪贴板管理器都会
    /// 短暂占用），退避重试若干次而不是直接报错。
    fn open() -> Result<Self, String> {
        use windows_sys::Win32::System::DataExchange::OpenClipboard;

        for attempt in 0..8u64 {
            // SAFETY: 传 NULL 表示把剪贴板关联到当前任务而非某个窗口，文档允许。
            if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
                return Ok(Self);
            }
            std::thread::sleep(std::time::Duration::from_millis(10 * (attempt + 1)));
        }
        Err(last_error("OpenClipboard"))
    }
}

#[cfg(target_os = "windows")]
impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        use windows_sys::Win32::System::DataExchange::CloseClipboard;

        // SAFETY: 本 guard 存在即代表 OpenClipboard 成功过，恰好配对一次关闭。
        unsafe { CloseClipboard() };
    }
}

/// 把最近一次 Win32 错误码包成稳定的 `os:` 短码。
#[cfg(target_os = "windows")]
fn last_error(op: &str) -> String {
    use windows_sys::Win32::Foundation::GetLastError;

    // SAFETY: GetLastError 无参数、无副作用，读取当前线程的错误码。
    let code = unsafe { GetLastError() };
    format!("os:{op} failed (GetLastError={code})")
}

// ---------------------------------------------------------------------------
// macOS：osascript 写 «class furl»
// ---------------------------------------------------------------------------

/// macOS：`set the clipboard to POSIX file "…"` 会把一个 «class furl»（文件
/// 引用）放进 `NSPasteboard`，Finder 与多数 App 粘贴时按文件处理；文件夹同理。
///
/// 走 osascript 而非直接 FFI `NSPasteboard`：后者要经 Objective-C 运行时
/// `objc_msgSend`（需为每种签名手写 extern 声明），为一个冷路径动作不值当。
#[cfg(target_os = "macos")]
fn platform_copy(path: &Path) -> Result<(), String> {
    use std::process::Command;

    // AppleScript 字符串字面量里只有反斜杠与双引号需要转义。
    let escaped = path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let script = format!("set the clipboard to POSIX file \"{escaped}\"");
    let status = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .status()
        .map_err(|e| format!("os:osascript spawn failed ({e})"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("os:osascript exited with {status}"))
    }
}

// ---------------------------------------------------------------------------
// Linux：wl-copy / xclip
// ---------------------------------------------------------------------------

/// Linux：没有系统级剪贴板服务，剪贴板内容由持有 selection 的进程提供，所以
/// 只能借道 `wl-copy`（Wayland）或 `xclip`（X11）——它们会 fork 成后台进程替
/// 我们持有内容。
///
/// MIME 选择：GNOME 系文件管理器（Nautilus/Nemo/Caja 及其衍生）认
/// `x-special/gnome-copied-files`（首行 `copy`，其后是 URI）；Dolphin/Thunar
/// 等认标准 `text/uri-list`。一次调用只能写一种类型（两个工具都不支持多
/// target），故按桌面环境择一。
#[cfg(target_os = "linux")]
fn platform_copy(path: &Path) -> Result<(), String> {
    let uri = url::Url::from_file_path(path)
        .map_err(|()| "os:path is not absolute".to_string())?
        .to_string();

    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let (mime, payload) = if is_gnome_family(&desktop) {
        ("x-special/gnome-copied-files", format!("copy\n{uri}"))
    } else {
        ("text/uri-list", format!("{uri}\r\n"))
    };

    // 会话类型决定优先级：Wayland 下 xclip 只对 XWayland 客户端可见，反之亦然。
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let wl: (&str, Vec<&str>) = ("wl-copy", vec!["--type", mime]);
    let x11: (&str, Vec<&str>) = ("xclip", vec!["-selection", "clipboard", "-t", mime]);
    let tools = if wayland { [wl, x11] } else { [x11, wl] };

    let mut last_err: Option<String> = None;
    for (bin, args) in tools {
        match feed_stdin(bin, &args, payload.as_bytes()) {
            Ok(()) => return Ok(()),
            Err(FeedError::NotFound) => continue,
            Err(FeedError::Io(e)) => last_err = Some(format!("os:{bin}: {e}")),
        }
    }
    Err(last_err.unwrap_or_else(|| "no_tool".to_string()))
}

/// 常见 GNOME 系桌面（含 Cinnamon/MATE/Budgie 等 Nautilus 衍生生态）。
/// `XDG_CURRENT_DESKTOP` 可能是 `ubuntu:GNOME` 这样的冒号分隔列表，Cinnamon
/// 还会写成带厂商前缀的 `X-Cinnamon`，两种形态都要认。
#[cfg(target_os = "linux")]
fn is_gnome_family(desktop: &str) -> bool {
    const FAMILY: &[&str] = &["GNOME", "CINNAMON", "MATE", "BUDGIE", "PANTHEON", "UNITY"];
    desktop.to_ascii_uppercase().split(':').any(|part| {
        let name = part.trim().trim_start_matches("X-");
        FAMILY.contains(&name)
    })
}

#[cfg(target_os = "linux")]
enum FeedError {
    /// 工具没装——换下一个候选。
    NotFound,
    Io(String),
}

/// 起一个剪贴板工具、把内容喂给它的 stdin 后**不等待退出**。
///
/// 这两个工具都会 fork 出后台进程持有 selection，直到有人取走或被别的复制
/// 覆盖才退出，`wait()` 会一直阻塞。这里写完即关闭管道，另起一个线程收尸，
/// 避免留下僵尸进程。
#[cfg(target_os = "linux")]
fn feed_stdin(bin: &str, args: &[&str], payload: &[u8]) -> Result<(), FeedError> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new(bin)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => FeedError::NotFound,
            _ => FeedError::Io(e.to_string()),
        })?;

    let written = match child.stdin.take() {
        Some(mut stdin) => stdin.write_all(payload).map_err(|e| e.to_string()),
        None => Err("stdin unavailable".to_string()),
    };
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    written.map_err(FeedError::Io)
}

// ---------------------------------------------------------------------------
// 其他平台（Android/iOS）：无桌面剪贴板语义
// ---------------------------------------------------------------------------

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn platform_copy(_path: &Path) -> Result<(), String> {
    Err("unsupported".to_string())
}

#[cfg(test)]
mod tests {
    use super::copy_path;

    /// 不存在的路径必须在碰任何平台 API 前就以 `not_found` 短路——UI 据此提示
    /// 「文件已不存在」而不是弹一条系统错误。
    #[test]
    fn missing_path_reports_not_found() {
        let missing = std::env::temp_dir().join("fluxdown-clipboard-missing-xyz");
        let path = missing.to_string_lossy().to_string();
        assert_eq!(copy_path(&path), Err("not_found".to_string()));
    }

    /// MIME 选错会让粘贴在对应文件管理器里静默失效，桌面名的两种真实形态
    /// （厂商前缀列表 `ubuntu:GNOME`、`X-` 前缀 `X-Cinnamon`）都必须命中。
    #[cfg(target_os = "linux")]
    #[test]
    fn gnome_family_matches_real_desktop_names() {
        use super::is_gnome_family;
        assert!(is_gnome_family("ubuntu:GNOME"));
        assert!(is_gnome_family("X-Cinnamon"));
        assert!(is_gnome_family("MATE"));
        assert!(!is_gnome_family("KDE"));
        assert!(!is_gnome_family("XFCE"));
        assert!(!is_gnome_family(""));
    }

    /// `DROPFILES` 布局写错（偏移、`fWide`、双 NUL 收尾任一项）都会让 Explorer
    /// 把剪贴板当空的——粘贴静默失效且没有任何报错，只能靠这层断言兜住。
    #[cfg(target_os = "windows")]
    #[test]
    fn hdrop_block_has_win32_layout() {
        use std::path::Path;

        use super::build_hdrop;

        const HEADER: usize = 20; // pFiles(4) + POINT(8) + fNC(4) + fWide(4)，packed(1)
        let block = build_hdrop(Path::new(r"C:\tmp\空格 dir"));

        let p_files = u32::from_le_bytes([block[0], block[1], block[2], block[3]]) as usize;
        assert_eq!(p_files, HEADER, "pFiles 必须是路径列表相对块首的字节偏移");
        assert_eq!(&block[16..20], &1i32.to_le_bytes(), "fWide 必须为 TRUE");

        let list: Vec<u16> = block[HEADER..]
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect();
        let expected: Vec<u16> = r"C:\tmp\空格 dir".encode_utf16().collect();
        assert_eq!(&list[..expected.len()], &expected[..]);
        assert_eq!(&list[expected.len()..], &[0, 0], "路径 NUL + 列表结束 NUL");
    }

    /// 真机烟测（会覆盖当前剪贴板内容，故默认不跑）：
    /// `cargo test -p hub -- --ignored clipboard_roundtrip`
    /// 把真实目录/文件放进剪贴板后，用 Explorer 粘贴时走的同一条读取路径
    /// （`GetClipboardData(CF_HDROP)` + `DragQueryFileW`）读回来核对——BT 落成
    /// 目录与普通任务落成单文件两种形态都要过。
    #[cfg(target_os = "windows")]
    #[ignore = "覆盖系统剪贴板，仅手动烟测"]
    #[test]
    fn clipboard_roundtrip_reads_back_the_path() {
        use super::copy_path;

        let dir = std::env::temp_dir().join("fluxdown-clipboard-roundtrip");
        assert!(std::fs::create_dir_all(&dir).is_ok(), "建临时目录失败");
        let dir_path = dir.to_string_lossy().to_string();
        assert_eq!(copy_path(&dir_path), Ok(true), "目录应被识别为文件夹");
        assert_eq!(read_clipboard_hdrop(), dir_path);

        let file = dir.join("payload.bin");
        assert!(std::fs::write(&file, b"x").is_ok(), "写临时文件失败");
        let file_path = file.to_string_lossy().to_string();
        assert_eq!(copy_path(&file_path), Ok(false), "文件应被识别为文件");
        assert_eq!(read_clipboard_hdrop(), file_path);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 按 Explorer 粘贴时的同款方式把剪贴板里的单条 `CF_HDROP` 路径读回来。
    #[cfg(target_os = "windows")]
    fn read_clipboard_hdrop() -> String {
        use windows_sys::Win32::System::DataExchange::{
            CloseClipboard, GetClipboardData, OpenClipboard,
        };
        use windows_sys::Win32::UI::Shell::DragQueryFileW;

        use super::CF_HDROP;

        // SAFETY: 读回路径与写入完全对称；全程持有剪贴板直到 CloseClipboard。
        unsafe {
            assert_ne!(OpenClipboard(std::ptr::null_mut()), 0, "OpenClipboard");
            let handle = GetClipboardData(CF_HDROP);
            assert!(!handle.is_null(), "剪贴板里没有 CF_HDROP");
            let count = DragQueryFileW(handle, u32::MAX, std::ptr::null_mut(), 0);
            assert_eq!(count, 1, "应恰好有一条路径");
            let mut buf = [0u16; 512];
            let len = DragQueryFileW(handle, 0, buf.as_mut_ptr(), buf.len() as u32) as usize;
            let path = String::from_utf16_lossy(&buf[..len]);
            CloseClipboard();
            path
        }
    }
}
