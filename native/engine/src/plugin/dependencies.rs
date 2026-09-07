//! 插件权限 → 基础组件依赖的通用规范。
//!
//! 每个能力权限（manifest `permissions`）映射到一组它运行所需的**基础组件**
//! （设置页「组件」分类中可安装/配置的外部工具）。安装插件时宿主据此计算
//! 缺失组件并**提醒**用户先安装依赖 —— 提醒式而非阻断式：组件缺失时对应
//! `flux.*` 能力面本就 graceful（`available()` 返回 false / `run` 报错），
//! 插件安装本身仍然成功。
//!
//! 依赖表（v1）：
//! - `ffmpeg` 权限 → 需要 `ffmpeg` 组件；
//! - `ytdlp` 权限 → 需要 `ytdlp` 组件 **及** `ffmpeg` 组件（传递依赖：yt-dlp
//!   的合并/抽音/remux 依赖 ffmpeg，宿主经 `--ffmpeg-location` 注入，见
//!   [`super::bridge`] 的 yt-dlp 能力面说明）。
//!
//! 新增能力权限时在 [`required_components`] 的闭合 match 中补一臂即可，
//! 探测统一走 `components::resolve_*`（低成本存在性检查，不 spawn 进程）。

use std::path::Path;

use crate::components::{resolve_ffmpeg, resolve_ytdlp};
use crate::db::Db;
use crate::plugin::manifest::{PERMISSION_FFMPEG, PERMISSION_YTDLP};

/// ffmpeg 组件名（与设置页组件分类、`ComponentKind` 的 wire 值一致）。
pub const COMPONENT_FFMPEG: &str = "ffmpeg";
/// yt-dlp 组件名。
pub const COMPONENT_YTDLP: &str = "ytdlp";

/// 权限集合 → 去重后的所需组件列表（含传递依赖）。
///
/// 未知权限（manifest 校验已拒绝，防御性忽略）不产生组件。
/// 返回顺序稳定：按 `[ffmpeg, ytdlp]` 固定序，方便 UI 与测试断言。
///
/// ```
/// use fluxdown_engine::plugin::dependencies::required_components;
/// let perms = vec!["ytdlp".to_string()];
/// assert_eq!(required_components(&perms), vec!["ytdlp", "ffmpeg"]);
/// ```
pub fn required_components(permissions: &[String]) -> Vec<&'static str> {
    let mut ffmpeg = false;
    let mut ytdlp = false;
    for p in permissions {
        match p.as_str() {
            PERMISSION_FFMPEG => ffmpeg = true,
            PERMISSION_YTDLP => {
                ytdlp = true;
                // 传递依赖：yt-dlp 的合并/后处理依赖 ffmpeg。
                ffmpeg = true;
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    if ytdlp {
        out.push(COMPONENT_YTDLP);
    }
    if ffmpeg {
        out.push(COMPONENT_FFMPEG);
    }
    out
}

/// 探测权限集合所需组件中**当前缺失**的部分（低成本 `resolve_*` 存在性检查，
/// manual → managed → system 三级解析，不 spawn 进程）。
///
/// 返回缺失组件名列表（[`COMPONENT_FFMPEG`]/[`COMPONENT_YTDLP`]），全部就绪
/// 时为空。供安装链路在成功后计算提醒载荷。
pub async fn missing_components(db: &Db, data_dir: &Path, permissions: &[String]) -> Vec<String> {
    let mut missing = Vec::new();
    for comp in required_components(permissions) {
        let present = match comp {
            COMPONENT_FFMPEG => resolve_ffmpeg(db, data_dir).await.is_some(),
            COMPONENT_YTDLP => resolve_ytdlp(db, data_dir).await.is_some(),
            _ => true,
        };
        if !present {
            missing.push(comp.to_string());
        }
    }
    missing
}

#[cfg(test)]
mod tests {
    use super::{COMPONENT_FFMPEG, COMPONENT_YTDLP, required_components};

    #[test]
    fn empty_permissions_need_nothing() {
        assert!(required_components(&[]).is_empty());
    }

    #[test]
    fn ffmpeg_permission_needs_ffmpeg_only() {
        let perms = vec!["ffmpeg".to_string()];
        assert_eq!(required_components(&perms), vec![COMPONENT_FFMPEG]);
    }

    #[test]
    fn ytdlp_permission_needs_ytdlp_and_ffmpeg() {
        let perms = vec!["ytdlp".to_string()];
        assert_eq!(
            required_components(&perms),
            vec![COMPONENT_YTDLP, COMPONENT_FFMPEG]
        );
    }

    #[test]
    fn combined_permissions_dedupe() {
        let perms = vec!["ffmpeg".to_string(), "ytdlp".to_string()];
        assert_eq!(
            required_components(&perms),
            vec![COMPONENT_YTDLP, COMPONENT_FFMPEG]
        );
    }

    #[test]
    fn unknown_permission_ignored() {
        let perms = vec!["nonsense".to_string()];
        assert!(required_components(&perms).is_empty());
    }
}
