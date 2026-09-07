//! 跨平台可用磁盘空间查询。零新依赖:Windows 用已启用的 windows-sys
//! `Win32_Storage_FileSystem` feature(`GetDiskFreeSpaceExW`),Unix 用既有
//! `libc::statvfs`。两处 unsafe FFI,先例:`segment_coordinator.rs` 的
//! `unsafe libc::fallocate` 预分配。
//!
//! 消费者:HLS remux 与 DASH mux 的 ENOSPC 预检(两者的中间产物会让磁盘
//! 峰值达到 ≈2x 源体积,预检不足时走各自既有的优雅降级路径)。

use std::path::{Path, PathBuf};

/// HLS/DASH 预检共用安全余量:mux/remux 产物 ≈ 源体积(重封装/流复制,
/// 无转码),余量覆盖容器开销与预检-写入间隙的并发磁盘消耗。
pub const PRECHECK_MARGIN: u64 = 64 * 1024 * 1024;

/// `dir`(须存在)所在卷的可用字节数。
///
/// 失败返回 `None`(= 未知,调用方乐观放行——预检是优化;真正的安全网
/// 是既有的写失败降级路径)。
///
/// # Examples
///
/// ```
/// let avail = fluxdown_engine::disk_space::available_space(&std::env::temp_dir());
/// assert!(avail.is_some_and(|a| a > 0));
/// ```
pub fn available_space(dir: &Path) -> Option<u64> {
    available_space_impl(dir)
}

#[cfg(windows)]
fn available_space_impl(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = dir
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut free_to_caller: u64 = 0;
    // SAFETY: `wide` 是 NUL 结尾 UTF-16 缓冲,调用期间存活;第 2 参指向栈上
    // u64 出参(ULARGE_INTEGER 同布局);后两个 out 指针按 API 约定传 null
    // 表示不取该值。
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free_to_caller,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(free_to_caller)
}

// Apple 平台专用:`statvfs` 在 libc 的 apple 绑定中 `fsblkcnt_t = c_uint`
// (u32,全 apple 家族共用),`f_bavail` 在可用块数 > u32::MAX 时回绕
// (4K frsize 下 ≈17.6TB 的大卷会误报极小可用空间 → 预检误拒)。改用
// 64 位的 `statfs`(libc 绑定链接 `statfs$INODE64` 变体,
// `f_bavail: u64` + `f_bsize: u32`)。`target_vendor = "apple"` 覆盖
// macos/ios/tvos/watchos/visionos,与 libc apple 模块的门控一致。
#[cfg(target_vendor = "apple")]
fn available_space_impl(dir: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c_path = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    // SAFETY: `c_path` 是合法 NUL 结尾 C 字符串;`stat` 为栈上出参,仅在
    // statfs 返回 0(成功、结构已完整写入)后读取其字段。
    let mut stat: libc::statfs = unsafe { std::mem::zeroed() };
    let ret = unsafe { libc::statfs(c_path.as_ptr(), &mut stat) };
    (ret == 0).then(|| stat.f_bavail.saturating_mul(stat.f_bsize as u64))
}

#[cfg(all(unix, not(target_vendor = "apple")))]
fn available_space_impl(dir: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c_path = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    // SAFETY: `c_path` 是合法 NUL 结尾 C 字符串;`stat` 为栈上出参,仅在
    // statvfs 返回 0(成功、结构已完整写入)后读取其字段。
    // 64-bit target 上 fsblkcnt_t/f_frsize 均为 64 位,无截断;32-bit
    // glibc 旧 ABI / 32-bit Android 上 fsblkcnt_t 为 u32,存在与 apple
    // 相同的大卷截断风险——FluxDown 仅支持 64 位桌面端,不在支持矩阵内。
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    let ret = unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) };
    (ret == 0).then(|| (stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64))
}

#[cfg(not(any(windows, unix)))]
fn available_space_impl(_dir: &Path) -> Option<u64> {
    None
}

/// [`available_space`] 的异步包装:`spawn_blocking` + 3s 超时。
///
/// 网络盘/慢速挂载点上系统调用可能阻塞数秒,直接在 async worker 上同步调
/// 用会拖慢其他任务(项目先例 BUG-BT-COMPLETION-MOVE-BLOCKING);超时或
/// 任务失败一律返回 `None`(视为"未知",调用方乐观放行)。
pub async fn available_space_checked(dir: PathBuf) -> Option<u64> {
    let fut = tokio::task::spawn_blocking(move || available_space(&dir));
    match tokio::time::timeout(std::time::Duration::from_secs(3), fut).await {
        Ok(Ok(v)) => v,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::available_space;

    #[test]
    fn available_space_reports_positive_for_temp_dir() {
        let avail = available_space(&std::env::temp_dir());
        match avail {
            Some(a) => assert!(a > 0, "temp dir volume reports zero free bytes"),
            None => panic!("available_space failed for temp_dir"),
        }
    }

    #[test]
    fn available_space_returns_none_for_missing_path() {
        let missing = std::env::temp_dir().join("fluxdown_definitely_missing_dir_xyz");
        // Windows GetDiskFreeSpaceExW 对不存在路径失败;Unix statvfs 同样
        // 报 ENOENT。两平台均应得到 None 而非 panic。
        assert!(available_space(&missing).is_none());
    }
}
