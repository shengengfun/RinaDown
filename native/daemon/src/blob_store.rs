//! daemon 私有的一次性二进制 blob 与导出文件存储。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use tokio::sync::Mutex;
use uuid::Uuid;

/// blob 默认有效期。
pub const BLOB_TTL: Duration = Duration::from_secs(10 * 60);

/// blob 类型；ID 前缀阻止跨端点误用。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlobKind {
    Torrent,
    Plugin,
    Logs,
}

impl BlobKind {
    fn prefix(self) -> &'static str {
        match self {
            Self::Torrent => "torrent",
            Self::Plugin => "plugin",
            Self::Logs => "logs",
        }
    }
}

#[derive(Clone, Debug)]
struct BlobEntry {
    kind: BlobKind,
    path: PathBuf,
    expires_at: SystemTime,
}

/// blob 操作错误。
#[derive(Debug, thiserror::Error)]
pub enum BlobError {
    #[error("blob not found")]
    NotFound,
    #[error("blob kind does not match endpoint")]
    WrongKind,
    #[error("blob expired")]
    Expired,
    #[error("blob I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

/// 进程私有、带 TTL 的 blob 存储。
pub struct BlobStore {
    root: PathBuf,
    entries: Mutex<HashMap<String, BlobEntry>>,
}

impl BlobStore {
    /// 创建存储并清理上次异常退出遗留的所有过期文件。
    pub async fn open(root: PathBuf) -> Result<Self, BlobError> {
        tokio::fs::create_dir_all(&root).await?;
        set_private_dir_permissions(&root).await?;
        let store = Self {
            root,
            entries: Mutex::new(HashMap::new()),
        };
        store.sweep_filesystem().await?;
        Ok(store)
    }

    /// 保存新 blob 并返回带类型前缀的 ID。
    pub async fn put(&self, kind: BlobKind, bytes: &[u8]) -> Result<String, BlobError> {
        let id = format!("{}:{}", kind.prefix(), Uuid::new_v4());
        let path = self.path_for_id(&id);
        tokio::fs::write(&path, bytes).await?;
        set_private_file_permissions(&path).await?;
        self.entries.lock().await.insert(
            id.clone(),
            BlobEntry {
                kind,
                path,
                expires_at: SystemTime::now() + BLOB_TTL,
            },
        );
        Ok(id)
    }

    /// 读取 blob；调用方完成业务提交后再调用 [`Self::consume`]。
    pub async fn read(&self, id: &str, expected_kind: BlobKind) -> Result<Vec<u8>, BlobError> {
        let entry = self.entry(id, expected_kind).await?;
        Ok(tokio::fs::read(entry.path).await?)
    }

    /// 业务提交成功后删除 blob，使其只能成功消费一次。
    pub async fn consume(&self, id: &str, expected_kind: BlobKind) -> Result<(), BlobError> {
        let entry = self.entry(id, expected_kind).await?;
        self.entries.lock().await.remove(id);
        match tokio::fs::remove_file(entry.path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(BlobError::Io(error)),
        }
    }

    /// 删除内存索引中已过期的 blob。
    pub async fn sweep(&self) -> Result<(), BlobError> {
        let now = SystemTime::now();
        let expired = {
            let entries = self.entries.lock().await;
            entries
                .iter()
                .filter(|(_, entry)| entry.expires_at <= now)
                .map(|(id, entry)| (id.clone(), entry.path.clone()))
                .collect::<Vec<_>>()
        };
        for (id, path) in expired {
            self.entries.lock().await.remove(&id);
            if let Err(error) = tokio::fs::remove_file(path).await
                && error.kind() != std::io::ErrorKind::NotFound
            {
                return Err(BlobError::Io(error));
            }
        }
        Ok(())
    }

    /// daemon 关闭时删除所有临时 blob 与导出。
    pub async fn cleanup_all(&self) -> Result<(), BlobError> {
        let paths = {
            let mut entries = self.entries.lock().await;
            entries
                .drain()
                .map(|(_, entry)| entry.path)
                .collect::<Vec<_>>()
        };
        for path in paths {
            if let Err(error) = tokio::fs::remove_file(path).await
                && error.kind() != std::io::ErrorKind::NotFound
            {
                return Err(BlobError::Io(error));
            }
        }
        let mut directory = tokio::fs::read_dir(&self.root).await?;
        while let Some(entry) = directory.next_entry().await? {
            if entry.metadata().await?.is_file() {
                tokio::fs::remove_file(entry.path()).await?;
            }
        }
        Ok(())
    }

    async fn entry(&self, id: &str, expected_kind: BlobKind) -> Result<BlobEntry, BlobError> {
        let Some(entry) = self.entries.lock().await.get(id).cloned() else {
            return Err(BlobError::NotFound);
        };
        if entry.kind != expected_kind || !id.starts_with(expected_kind.prefix()) {
            return Err(BlobError::WrongKind);
        }
        if entry.expires_at <= SystemTime::now() {
            self.entries.lock().await.remove(id);
            let _ = tokio::fs::remove_file(entry.path).await;
            return Err(BlobError::Expired);
        }
        Ok(entry)
    }

    async fn sweep_filesystem(&self) -> Result<(), BlobError> {
        let mut entries = tokio::fs::read_dir(&self.root).await?;
        while let Some(entry) = entries.next_entry().await? {
            let metadata = entry.metadata().await?;
            let stale = metadata
                .modified()
                .ok()
                .and_then(|modified| modified.checked_add(BLOB_TTL))
                .is_none_or(|expires| expires <= SystemTime::now());
            if stale && metadata.is_file() {
                tokio::fs::remove_file(entry.path()).await?;
            }
        }
        Ok(())
    }

    fn path_for_id(&self, id: &str) -> PathBuf {
        self.root.join(id.replace(':', "_"))
    }
}

#[cfg(unix)]
async fn set_private_dir_permissions(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).await
}

#[cfg(not(unix))]
async fn set_private_dir_permissions(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}

#[cfg(unix)]
async fn set_private_file_permissions(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).await
}

#[cfg(not(unix))]
async fn set_private_file_permissions(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{BlobError, BlobKind, BlobStore};

    #[tokio::test]
    async fn blob_kind_and_single_use_are_enforced() {
        let root = std::env::temp_dir().join(format!(
            "fluxdown_blob_test_{}_{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let store = BlobStore::open(root.clone()).await.expect("open store");
        let id = store
            .put(BlobKind::Torrent, b"torrent-bytes")
            .await
            .expect("put");
        assert!(matches!(
            store.read(&id, BlobKind::Plugin).await,
            Err(BlobError::WrongKind)
        ));
        assert_eq!(
            store.read(&id, BlobKind::Torrent).await.expect("read"),
            b"torrent-bytes"
        );
        store
            .consume(&id, BlobKind::Torrent)
            .await
            .expect("consume");
        assert!(matches!(
            store.read(&id, BlobKind::Torrent).await,
            Err(BlobError::NotFound)
        ));
        let _ = std::fs::remove_dir_all(root);
    }
}
