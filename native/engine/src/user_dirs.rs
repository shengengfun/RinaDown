//! 用户主目录下的系统标准目录解析（当前只有「下载」目录）。
//!
//! **一律走系统 API，禁止 `$HOME/Downloads` 拼接**——下载目录是可迁移的：
//!
//! | 平台 | 数据来源（[`directories::UserDirs`] 内部实现） |
//! |---|---|
//! | Windows | `SHGetKnownFolderPath(FOLDERID_Downloads)`，读注册表 User Shell Folders；用户在「属性 → 位置」里改到 `D:\Downloads` 后立即生效 |
//! | Linux/BSD | XDG user-dirs（`~/.config/user-dirs.dirs` 的 `XDG_DOWNLOAD_DIR`），本地化目录名（如 `~/下载`）也能正确解析 |
//! | macOS | `$HOME/Downloads`（系统固定，不可迁移） |
//!
//! 只有在系统未给出答案时（无桌面环境的容器 / headless、缺 `user-dirs.dirs`
//! 配置的最小化 Linux）才回退到 `$HOME/Downloads` 拼接，最后回退 `"."`。

use std::path::PathBuf;

use directories::UserDirs;

/// 系统「下载」目录。
///
/// 返回 `None` 表示系统 API 与 `$HOME` 回退都无法给出路径（无主目录的
/// 服务账户等），调用方自行决定回退策略；多数调用方直接用
/// [`download_dir_or_cwd`]。
///
/// 注意：返回的路径**不保证存在**（用户可能删掉了 Downloads 文件夹），
/// 引擎落盘前会自行创建目录。
///
/// # Examples
///
/// ```
/// use fluxdown_engine::user_dirs::download_dir;
///
/// // 有主目录的环境下必有值；CI 容器里可能为 None。
/// if let Some(dir) = download_dir() {
///     assert!(dir.is_absolute());
/// }
/// ```
pub fn download_dir() -> Option<PathBuf> {
    let dirs = UserDirs::new()?;
    if let Some(d) = dirs.download_dir() {
        return Some(d.to_path_buf());
    }
    // Linux 最小化环境：无 `user-dirs.dirs` 配置时 XDG 查询为空，退回约定俗成
    // 的 `$HOME/Downloads`（此时系统本就没有「正确答案」可言）。
    Some(dirs.home_dir().join("Downloads"))
}

/// [`download_dir`] 的字符串形式，解析失败回退 `"."`（相对当前工作目录，
/// 与历史行为一致）。
///
/// # Examples
///
/// ```
/// use fluxdown_engine::user_dirs::download_dir_or_cwd;
///
/// assert!(!download_dir_or_cwd().is_empty());
/// ```
pub fn download_dir_or_cwd() -> String {
    download_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| ".".to_string())
}
