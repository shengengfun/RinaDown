//! 插件安装 —— zip 解压（防 zip-slip / 防压缩炸弹 / 单层剥壳）与目录拷贝。
//!
//! zip 安全：每个成员路径规整后必须落在目标目录内（拒 `..`/绝对路径）；累计解压
//! ≤50MB 且条目 ≤200（防压缩炸弹）；zip 内 manifest.json 须在根，单层子目录包裹时
//! 自动剥壳。

use std::io::Read;
use std::path::{Component, Path, PathBuf};

use super::manifest::PluginManifest;
use super::runtime::PluginError;

/// 累计解压上限（50MB）。
const MAX_TOTAL_UNCOMPRESSED: u64 = 50 * 1024 * 1024;
/// 条目数上限。
const MAX_ENTRIES: usize = 200;

/// 从 zip 字节安装到 `<root>/<identity>/`，返回 identity。
pub fn install_from_zip(root: &Path, bytes: &[u8]) -> Result<String, PluginError> {
    let tmp = root.join(format!(".tmp_install_{}", uuid::Uuid::new_v4()));
    let result = (|| {
        std::fs::create_dir_all(&tmp)
            .map_err(|e| PluginError::ManifestInvalid(format!("创建临时目录失败: {e}")))?;
        extract_zip(bytes, &tmp)?;
        // 剥壳：若根无 manifest.json 但唯一子目录含，则以该子目录为根。
        let src_root = resolve_pkg_root(&tmp)?;
        let manifest = read_manifest(&src_root)?;
        let identity = manifest.identity.clone();
        // 拷贝到最终目录（先清旧）。
        let dest = root.join(&identity);
        if dest.exists() {
            std::fs::remove_dir_all(&dest)
                .map_err(|e| PluginError::ManifestInvalid(format!("清理旧插件失败: {e}")))?;
        }
        copy_dir(&src_root, &dest)?;
        Ok(identity)
    })();
    // 清理临时目录（无论成败）。
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

/// 从目录安装（不剥壳，`path` 须直接含 manifest.json）。
pub fn install_from_dir(root: &Path, path: &Path) -> Result<String, PluginError> {
    let manifest = read_manifest(path)?;
    let identity = manifest.identity.clone();
    let dest = root.join(&identity);
    std::fs::create_dir_all(root)
        .map_err(|e| PluginError::ManifestInvalid(format!("创建插件根目录失败: {e}")))?;
    if dest.exists() {
        std::fs::remove_dir_all(&dest)
            .map_err(|e| PluginError::ManifestInvalid(format!("清理旧插件失败: {e}")))?;
    }
    copy_dir(path, &dest)?;
    Ok(identity)
}

fn read_manifest(dir: &Path) -> Result<PluginManifest, PluginError> {
    let bytes = std::fs::read(dir.join("manifest.json"))
        .map_err(|e| PluginError::ManifestInvalid(format!("读取 manifest.json 失败: {e}")))?;
    let manifest = PluginManifest::parse(&bytes)?;
    manifest.validate()?;
    Ok(manifest)
}

/// 解压 zip 到 `dest`，逐条防穿越 + 累计体积/条目上限。
fn extract_zip(bytes: &[u8], dest: &Path) -> Result<(), PluginError> {
    let reader = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(reader)
        .map_err(|e| PluginError::ManifestInvalid(format!("zip 打开失败: {e}")))?;
    if zip.len() > MAX_ENTRIES {
        return Err(PluginError::ManifestInvalid(format!(
            "zip 条目数 {} 超过上限 {MAX_ENTRIES}",
            zip.len()
        )));
    }
    let mut total: u64 = 0;
    for i in 0..zip.len() {
        let mut file = zip
            .by_index(i)
            .map_err(|e| PluginError::ManifestInvalid(format!("读取 zip 条目失败: {e}")))?;
        let Some(enclosed) = file.enclosed_name() else {
            return Err(PluginError::ManifestInvalid(
                "zip 含非法路径（zip-slip）".to_string(),
            ));
        };
        // 二次防护：规整后必须落在 dest 内。
        let out_path = dest.join(&enclosed);
        if !is_within(dest, &out_path) {
            return Err(PluginError::ManifestInvalid(
                "zip 成员越界（zip-slip）".to_string(),
            ));
        }
        if file.is_dir() {
            std::fs::create_dir_all(&out_path)
                .map_err(|e| PluginError::ManifestInvalid(format!("创建目录失败: {e}")))?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| PluginError::ManifestInvalid(format!("创建父目录失败: {e}")))?;
        }
        // 累计**实际**解压字节上限：以剩余预算 +1 限流本条目读取，读完累加实际字节
        // 再判上限。杜绝「每条目声明极小、实际膨胀」的压缩炸弹（reviewer finding 6）。
        let remaining = MAX_TOTAL_UNCOMPRESSED.saturating_sub(total);
        let mut limited = (&mut file).take(remaining + 1);
        let mut buf = Vec::new();
        limited
            .read_to_end(&mut buf)
            .map_err(|e| PluginError::ManifestInvalid(format!("解压读取失败: {e}")))?;
        total = total.saturating_add(buf.len() as u64);
        if total > MAX_TOTAL_UNCOMPRESSED {
            return Err(PluginError::ManifestInvalid(format!(
                "解压总量超过 {MAX_TOTAL_UNCOMPRESSED} 字节上限（疑压缩炸弹）"
            )));
        }
        std::fs::write(&out_path, &buf)
            .map_err(|e| PluginError::ManifestInvalid(format!("写入解压文件失败: {e}")))?;
    }
    Ok(())
}

/// `child` 规整后是否落在 `base` 内。
fn is_within(base: &Path, child: &Path) -> bool {
    // 逐组件校验：不允许任何 ParentDir/RootDir/Prefix 逃逸。
    let rel = match child.strip_prefix(base) {
        Ok(r) => r,
        Err(_) => return false,
    };
    for comp in rel.components() {
        match comp {
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return false,
            _ => {}
        }
    }
    true
}

/// 剥壳：根含 manifest.json 直接返回；否则若唯一子目录含 manifest.json 则返回它。
fn resolve_pkg_root(tmp: &Path) -> Result<PathBuf, PluginError> {
    if tmp.join("manifest.json").is_file() {
        return Ok(tmp.to_path_buf());
    }
    let mut subdirs = Vec::new();
    let mut has_root_files = false;
    if let Ok(rd) = std::fs::read_dir(tmp) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                subdirs.push(p);
            } else {
                has_root_files = true;
            }
        }
    }
    if !has_root_files && subdirs.len() == 1 && subdirs[0].join("manifest.json").is_file() {
        return Ok(subdirs[0].clone());
    }
    Err(PluginError::ManifestInvalid(
        "zip 内未找到根 manifest.json".to_string(),
    ))
}

/// 递归拷贝目录（安装到最终位置）。
fn copy_dir(src: &Path, dest: &Path) -> Result<(), PluginError> {
    std::fs::create_dir_all(dest)
        .map_err(|e| PluginError::ManifestInvalid(format!("创建目标目录失败: {e}")))?;
    let rd = std::fs::read_dir(src)
        .map_err(|e| PluginError::ManifestInvalid(format!("读取源目录失败: {e}")))?;
    for entry in rd.flatten() {
        let from = entry.path();
        let Some(name) = from.file_name() else {
            continue;
        };
        let to = dest.join(name);
        if from.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)
                .map_err(|e| PluginError::ManifestInvalid(format!("拷贝文件失败: {e}")))?;
        }
    }
    Ok(())
}
