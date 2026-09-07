//! 桌面进程启动参数、单实例锁与外部链接接入。
//!
//! 开机自启以 `--minimized` 拉起；系统把 `magnet:` / `ed2k:` / `fluxdown:` 链接或
//! 直链交给本进程时，统一经 `agent.capture.submit` 交由 agent 建任务：
//! 主实例自己提交，后续实例提交后立即退出，因此不依赖任何进程间通道。

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

/// 已解析的命令行。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LaunchOptions {
    /// 启动后最小化主窗口（自启动场景）。
    pub minimized: bool,
    /// 需要交给 agent 的外部链接。
    pub urls: Vec<String>,
    /// 需要经 agent 上传后建任务的本机 `.torrent` 文件。
    pub torrent_files: Vec<PathBuf>,
}

impl LaunchOptions {
    #[must_use]
    pub fn from_args(args: impl IntoIterator<Item = String>) -> Self {
        let mut options = Self::default();
        for arg in args {
            match arg.as_str() {
                "--minimized" | "--start-minimized" => options.minimized = true,
                value if is_capture_url(value) => options.urls.push(value.to_owned()),
                value if value.starts_with("--") => {}
                value => {
                    if let Some(path) = torrent_path(value) {
                        options.torrent_files.push(path);
                    }
                }
            }
        }
        options
    }
}

/// `.torrent` 路径或 `file://` URL → 本机路径。
#[must_use]
pub fn torrent_path(value: &str) -> Option<PathBuf> {
    let raw = value.strip_prefix("file://").map_or(value, |rest| rest);
    let decoded = percent_decode(raw);
    if !decoded.to_ascii_lowercase().ends_with(".torrent") {
        return None;
    }
    let path = PathBuf::from(decoded);
    path.is_file().then_some(path)
}

/// 判定参数是否为可直接建任务的链接（与 agent 捕获入口一致）。
#[must_use]
pub fn is_capture_url(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "magnet:",
        "ed2k://",
        "fluxdown:",
        "http://",
        "https://",
        "ftp://",
        "ftps://",
    ]
    .iter()
    .any(|scheme| lower.starts_with(scheme))
}

/// `fluxdown:` 协议 → 实际下载链接：`fluxdown://download?url=<encoded>` 或
/// `fluxdown:<url>`；其他 scheme 原样返回。
#[must_use]
pub fn normalize_capture_url(value: &str) -> String {
    let lower = value.to_ascii_lowercase();
    if !lower.starts_with("fluxdown:") {
        return value.to_owned();
    }
    let rest = &value["fluxdown:".len()..];
    let rest = rest.trim_start_matches('/');
    if let Some(query) = rest.strip_prefix("download?") {
        for pair in query.split('&') {
            if let Some(encoded) = pair.strip_prefix("url=") {
                return percent_decode(encoded);
            }
        }
    }
    percent_decode(rest)
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = &value[index + 1..index + 3];
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| value.to_owned())
}

/// 单实例锁：持有期间文件锁不释放；第二个进程 `try_acquire` 失败。
pub struct InstanceLock {
    _file: File,
}

impl InstanceLock {
    /// 尝试成为主实例。`None` 表示已有实例在运行。
    pub fn try_acquire(dir: &Path) -> Result<Option<Self>, std::io::Error> {
        std::fs::create_dir_all(dir)?;
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(dir.join("desktop.lock"))?;
        match file.try_lock() {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(std::fs::TryLockError::Error(error)) => Err(error),
        }
    }
}

/// 锁与队列文件所在目录：与 agent 数据目录同级。
#[must_use]
pub fn instance_dir(agent_token_path: &Path) -> PathBuf {
    agent_token_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::env::temp_dir().join("fluxdown"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flags_and_urls() {
        let options = LaunchOptions::from_args(
            [
                "--minimized",
                "magnet:?xt=urn:btih:abc",
                "/tmp/x.torrent",
                "--foo",
            ]
            .map(str::to_owned),
        );
        assert!(options.minimized);
        assert_eq!(options.urls, vec!["magnet:?xt=urn:btih:abc"]);
        assert!(options.torrent_files.is_empty());
        let file = std::env::temp_dir().join(format!("fluxdown-{}.torrent", std::process::id()));
        std::fs::write(&file, b"d8:announce0:e").expect("write");
        let options = LaunchOptions::from_args([file.display().to_string()]);
        assert_eq!(options.torrent_files, vec![file.clone()]);
        assert_eq!(
            torrent_path(&format!("file://{}", file.display())),
            Some(file.clone())
        );
        let _ = std::fs::remove_file(file);
    }

    #[test]
    fn normalizes_fluxdown_scheme() {
        assert_eq!(
            normalize_capture_url("fluxdown://download?url=https%3A%2F%2Fa.b%2Fc"),
            "https://a.b/c"
        );
        assert_eq!(
            normalize_capture_url("fluxdown:https://a.b/c"),
            "https://a.b/c"
        );
        assert_eq!(normalize_capture_url("magnet:?x"), "magnet:?x");
    }

    #[test]
    fn lock_is_exclusive() {
        let dir = std::env::temp_dir().join(format!("fluxdown-lock-{}", std::process::id()));
        let first = InstanceLock::try_acquire(&dir).expect("lock");
        assert!(first.is_some());
        let second = InstanceLock::try_acquire(&dir).expect("lock");
        assert!(second.is_none());
        drop(first);
        let _ = std::fs::remove_dir_all(dir);
    }
}
