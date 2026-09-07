//! Windows 下 BT staging 文件的 sparse 存储包装。
//!
//! librqbit 的 `FilesystemStorage` 在初检结束后对选中文件 `set_len` 到完整
//! 大小。NTFS 上非 sparse 文件的 `set_len` 会立即预留全部簇——磁盘可用
//! 空间一次性掉完；且此后每次在高偏移写入都按 valid-data-length 语义把
//! `[VDL, offset)` 之间零填充。BT 的 piece 到达顺序近乎随机，首个高偏移
//! piece 会触发对整个前缀的零写，大文件种子上这是数量级的写放大，也是
//! 下载速率周期性跌落的直接来源。
//!
//! 本包装在 `FilesystemStorage::init` 创建文件之后、任何 `set_len`/写入
//! 之前给每个非 padding 文件打上 `FSCTL_SET_SPARSE`：sparse 文件的
//! `set_len` 只改逻辑大小不预留簇，随机偏移写入没有 VDL 零填充，未写
//! 区域读出为零。全部数据写完后文件实际占用与逻辑大小一致，只留下一个
//! 无害的 sparse 属性；同卷 rename 保留该属性，跨卷 copy 落成普通文件。
//!
//! ext4 / APFS 的 `set_len` 天然稀疏，非 Windows 平台不编译本模块。
//!
//! 代价与取舍：sparse 文件放弃了「预分配即占位」带来的两个次要收益——
//! 提前暴露 ENOSPC（改为写入时逐 piece 暴露，librqbit 按致命错误处理，
//! 任务进错误态可重试）与簇的连续性（NTFS 按写入顺序分配，BT 随机写
//! 本就难以连续）。换来的是即点即下、零预留等待、零 VDL 写放大。

use std::any::TypeId;
use std::path::{Path, PathBuf};

use librqbit::storage::filesystem::{FilesystemStorage, FilesystemStorageFactory};
use librqbit::storage::{BoxStorageFactory, StorageFactory, TorrentStorage};
use librqbit::{ManagedTorrentShared, TorrentMetadata};

use crate::logger::log_info;

/// 构造注入 `AddTorrentOptions.storage_factory` 的 sparse 包装工厂。
///
/// `output_folder` 与 `AddTorrentOptions.output_folder` 同值（本任务的
/// staging 目录）：`init` 之后按 torrent 布局拼出各文件路径打 sparse 标记。
pub fn sparse_fs_factory(output_folder: PathBuf) -> BoxStorageFactory {
    Box::new(SparseFsFactory {
        inner: FilesystemStorageFactory::default(),
        output_folder,
    })
}

struct SparseFsFactory {
    inner: FilesystemStorageFactory,
    output_folder: PathBuf,
}

impl StorageFactory for SparseFsFactory {
    // `BoxStorageFactory` 要求关联 Storage 为 `Box<dyn TorrentStorage>`。
    type Storage = Box<dyn TorrentStorage>;

    fn create(
        &self,
        shared: &ManagedTorrentShared,
        metadata: &TorrentMetadata,
    ) -> anyhow::Result<Self::Storage> {
        Ok(Box::new(SparseFsStorage {
            inner: self.inner.create(shared, metadata)?,
            output_folder: self.output_folder.clone(),
        }))
    }

    fn is_type_id(&self, type_id: TypeId) -> bool {
        // 伪装为 `FilesystemStorageFactory` 以通过 JSON session 持久化的
        // TypeId 白名单，否则 add_torrent 整体失败。约定与安全性论证同
        // `bt_partfile::FsMasqueradeFactory`：session.json 在每次启动时
        // 先于会话创建被整体清除，其恢复路径从不消费这些条目；本包装
        // 之下就是真正的 FilesystemStorage，即便被按默认 storage 恢复，
        // 行为也一致（仅少一个 sparse 标记）。
        type_id == TypeId::of::<FilesystemStorageFactory>() || type_id == TypeId::of::<Self>()
    }

    fn clone_box(&self) -> BoxStorageFactory {
        Box::new(Self {
            inner: self.inner,
            output_folder: self.output_folder.clone(),
        })
    }
}

/// 委托 `FilesystemStorage` 的存储实现，仅在 `init` 后追加 sparse 标记。
pub struct SparseFsStorage {
    inner: FilesystemStorage,
    output_folder: PathBuf,
}

impl TorrentStorage for SparseFsStorage {
    fn init(
        &mut self,
        shared: &ManagedTorrentShared,
        metadata: &TorrentMetadata,
    ) -> anyhow::Result<()> {
        self.inner.init(shared, metadata)?;
        // 文件此刻已创建/打开（0 字节或带既有数据），尚未 set_len——
        // 正是打 sparse 标记的窗口。对已有数据的文件重复打标记是幂等
        // 操作，既有分配不受影响。
        for fi in metadata.file_infos.iter().filter(|f| !f.attrs.padding) {
            let path = self.output_folder.join(&fi.relative_filename);
            if let Err(e) = mark_sparse(&path) {
                // 失败仅记日志：非 sparse 只是回到「全量预留 + VDL 零填充」
                // 的旧行为，不影响数据正确性（FAT32/exFAT 不支持 sparse）。
                log_info!("[BT] failed to mark '{}' sparse: {}", path.display(), e);
            }
        }
        Ok(())
    }

    fn pread_exact(&self, file_id: usize, offset: u64, buf: &mut [u8]) -> anyhow::Result<()> {
        self.inner.pread_exact(file_id, offset, buf)
    }

    fn pwrite_all(&self, file_id: usize, offset: u64, buf: &[u8]) -> anyhow::Result<()> {
        self.inner.pwrite_all(file_id, offset, buf)
    }

    fn remove_file(&self, file_id: usize, filename: &Path) -> anyhow::Result<()> {
        self.inner.remove_file(file_id, filename)
    }

    fn remove_directory_if_empty(&self, path: &Path) -> anyhow::Result<()> {
        self.inner.remove_directory_if_empty(path)
    }

    fn ensure_file_length(&self, file_id: usize, length: u64) -> anyhow::Result<()> {
        self.inner.ensure_file_length(file_id, length)
    }

    fn take(&self) -> anyhow::Result<Box<dyn TorrentStorage>> {
        // 暂停时 librqbit 用 take() 把存储换成占位并取走句柄集；sparse
        // 标记是文件属性、init 期已落盘，之后的存储对象无需再带包装。
        // `on_piece_completed` 不覆写：FilesystemStorage 亦为默认 no-op。
        self.inner.take()
    }
}

/// 给已存在的文件打 NTFS sparse 标记。
///
/// `FSCTL_SET_SPARSE` 不带输入缓冲区即「置位」。属性是文件级持久标记：
/// 之后的 `set_len` 扩展不预留簇，任意偏移写入没有 VDL 零填充。
fn mark_sparse(path: &Path) -> std::io::Result<()> {
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::System::IO::DeviceIoControl;
    use windows_sys::Win32::System::Ioctl::FSCTL_SET_SPARSE;

    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?;
    let mut returned: u32 = 0;
    // SAFETY: `file` 是本函数持有的合法文件句柄，生命周期覆盖整个调用；
    // FSCTL_SET_SPARSE 允许空输入/输出缓冲区（NULL + 0 长度）；
    // lpBytesReturned 指向栈上有效的 u32；同步调用，OVERLAPPED 为 NULL。
    let ok = unsafe {
        DeviceIoControl(
            file.as_raw_handle() as _,
            FSCTL_SET_SPARSE,
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const FILE_ATTRIBUTE_SPARSE_FILE: u32 = 0x200;

    fn unique_test_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "fluxdown_bt_sparse_test_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn file_attributes(path: &Path) -> u32 {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: wide 以 NUL 结尾且在调用期间有效。
        unsafe { windows_sys::Win32::Storage::FileSystem::GetFileAttributesW(wide.as_ptr()) }
    }

    #[test]
    fn mark_sparse_sets_attribute_and_survives_set_len() {
        let dir = unique_test_dir("attr");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("payload.bin");
        std::fs::write(&path, b"").unwrap();

        mark_sparse(&path).unwrap();
        assert_ne!(
            file_attributes(&path) & FILE_ATTRIBUTE_SPARSE_FILE,
            0,
            "sparse attribute must be set after mark_sparse"
        );

        // set_len 之后属性保留，逻辑大小生效。
        let f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        f.set_len(64 * 1024 * 1024).unwrap();
        drop(f);
        assert_eq!(
            std::fs::metadata(&path).unwrap().len(),
            64 * 1024 * 1024,
            "logical size must follow set_len"
        );
        assert_ne!(
            file_attributes(&path) & FILE_ATTRIBUTE_SPARSE_FILE,
            0,
            "sparse attribute must survive set_len"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mark_sparse_missing_file_reports_error() {
        let dir = unique_test_dir("missing");
        std::fs::create_dir_all(&dir).unwrap();
        let err = mark_sparse(&dir.join("no_such_file.bin")).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
