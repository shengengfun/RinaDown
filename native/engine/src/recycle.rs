//! 用户数据最终文件的删除策略。
//!
//! Windows：优先移入系统回收站（`SHFileOperationW` + `FOF_ALLOWUNDO`），使
//! 「删除任务并删除文件」后仍能在回收站找回（误删保护）。非 Windows 平台没有
//! 跨桌面统一的回收站 API，维持原有硬删语义。
//!
//! 只应作用于**用户最终数据文件**（HTTP/FTP/HLS/DASH 的单文件最终产物）；
//! 临时文件 `.fdownloading`、sidecar、BT staging/parts 等内部工件一律硬删，
//! 不进回收站。

use std::path::Path;

/// 删除用户最终数据文件（文件或目录），`NotFound` 视为成功。
///
/// Windows：静默移入回收站（`FOF_ALLOWUNDO | FOF_SILENT | FOF_NOCONFIRMATION |
/// FOF_NOERRORUI`）。若调用后目标仍存在（例如位于网络盘等回收站不可用场景），
/// 回退为硬删，保证不滞留文件。
///
/// 非 Windows：等价于原有 `tokio::fs` 硬删。
pub async fn remove_user_file(path: &Path) -> std::io::Result<()> {
    remove_user_file_impl(path).await
}

#[cfg(windows)]
async fn remove_user_file_impl(path: &Path) -> std::io::Result<()> {
    // SHFileOperationW 是同步阻塞 Win32 调用（可能触发磁盘枚举），置于
    // spawn_blocking，避免阻塞 current_thread 引擎的 async worker。
    let owned = path.to_path_buf();
    tokio::task::spawn_blocking(move || recycle_windows(&owned))
        .await
        .map_err(|e| std::io::Error::other(format!("recycle join error: {e}")))?
}

#[cfg(not(windows))]
async fn remove_user_file_impl(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        tokio::fs::remove_dir_all(path).await
    } else {
        tokio::fs::remove_file(path).await
    }
}

#[cfg(windows)]
fn recycle_windows(path: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::{
        FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, SHFILEOPSTRUCTW,
        SHFileOperationW,
    };

    if !path.exists() {
        return Ok(());
    }

    // pFrom 需要双 NUL 结尾的 UTF-16 缓冲。
    let from: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .chain(std::iter::once(0))
        .collect();

    let mut op = SHFILEOPSTRUCTW {
        hwnd: std::ptr::null_mut(),
        wFunc: FO_DELETE,
        // PCWSTR = *const u16（windows-sys 0.59 为裸指针别名），直接赋指针。
        pFrom: from.as_ptr(),
        pTo: std::ptr::null(),
        // FOF_* 常量在 windows-sys 中为 u32（FILEOPERATION_FLAGS），结构体
        // fFlags 字段为 u16，这里按 Windows 头文件语义强转（值均 < 0xFFFF）。
        fFlags: (FOF_ALLOWUNDO | FOF_SILENT | FOF_NOCONFIRMATION | FOF_NOERRORUI) as u16,
        fAnyOperationsAborted: 0,
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: std::ptr::null(),
    };

    // SAFETY: `op` 各字段已初始化且生命周期覆盖本次调用；pFrom 指向双 NUL
    // 结尾的 UTF-16 缓冲；无回调、无进度 UI（FOF_NOERRORUI/SILENT），错误由
    // 返回值上报而非弹窗。
    let ret = unsafe { SHFileOperationW(&mut op) };

    if ret == 0 && !path.exists() {
        return Ok(());
    }
    // 失败或目标仍在（回收站不可用/被占用等）：回退硬删，避免残留。
    if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}
