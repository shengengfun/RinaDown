/// 文件类型图标 —— 全项目唯一映射源。
///
/// 取代此前四处各自渲染 `Text(task.fileExtension)` 的做法：长扩展名
/// （`torrent` / `appimage` / `appinstaller`）在 24~40px 方块里必然换行或溢出，
/// 且纯文字在列表中无法快速扫读。
///
/// 分层策略：
///   1. `_extIcons` 精确表 —— 仅收录「比分类更有信息量」的扩展名
///      （iso→光盘、apk→手机、torrent→磁力、srt→字幕、ttf→字形…）。
///   2. 未命中则回落 [FileCategory] 语义图标，与侧栏分类图标同一套符号，
///      保证「点击侧栏『视频』」与「列表行图标」视觉连贯。
///
/// 颜色严格由 [FileCategory] 决定（见 [fileTypeColor]），刻意不引入第二套
/// 分类：精确表只影响字形，不影响着色，`other` 一律跟随 `textSecondary`。
library;

import 'package:flutter/widgets.dart';
import 'package:shadcn_ui/shadcn_ui.dart';

import '../models/download_task.dart';
import '../theme/app_colors.dart';

/// 扩展名 → 图标的精确覆盖表。
///
/// 只有当某扩展名的专属图标比它所属分类的通用图标更达意时才登记；
/// 否则留空走分类回落，避免表无限膨胀。
///
/// 注意：`ts` 故意缺席 —— 在下载器语境下它是 MPEG-TS 视频切片
/// （已在 `FileCategory._videoExts` 中），不是 TypeScript 源码。
const Map<String, IconData> _extIcons = {
  // ── 光盘 / 磁盘镜像 ──
  'iso': LucideIcons.disc,
  'img': LucideIcons.disc,
  'dmg': LucideIcons.disc,
  'vhd': LucideIcons.disc,
  'vhdx': LucideIcons.disc,
  'cue': LucideIcons.disc,

  // ── 桌面可执行 / 安装包 ──
  'exe': LucideIcons.appWindow,
  'msi': LucideIcons.appWindow,
  'msix': LucideIcons.appWindow,
  'appx': LucideIcons.appWindow,
  'appinstaller': LucideIcons.appWindow,

  // ── 移动端安装包 ──
  'apk': LucideIcons.smartphone,
  'xapk': LucideIcons.smartphone,
  'apks': LucideIcons.smartphone,
  'ipa': LucideIcons.smartphone,

  // ── Linux / macOS 分发包 ──
  'deb': LucideIcons.package,
  'rpm': LucideIcons.package,
  'pkg': LucideIcons.package,
  'appimage': LucideIcons.package,
  'snap': LucideIcons.package,
  'flatpak': LucideIcons.package,

  // ── 下载器一等公民 ──
  'torrent': LucideIcons.magnet,

  // ── 文档细分 ──
  'xls': LucideIcons.fileSpreadsheet,
  'xlsx': LucideIcons.fileSpreadsheet,
  'ods': LucideIcons.fileSpreadsheet,
  'csv': LucideIcons.fileSpreadsheet,
  'tsv': LucideIcons.fileSpreadsheet,
  'ppt': LucideIcons.presentation,
  'pptx': LucideIcons.presentation,
  'odp': LucideIcons.presentation,
  'epub': LucideIcons.bookOpen,
  'mobi': LucideIcons.bookOpen,
  'azw': LucideIcons.bookOpen,
  'azw3': LucideIcons.bookOpen,

  // ── 字幕 / 歌词 ──
  'srt': LucideIcons.captions,
  'ass': LucideIcons.captions,
  'ssa': LucideIcons.captions,
  'vtt': LucideIcons.captions,
  'sub': LucideIcons.captions,
  'lrc': LucideIcons.captions,

  // ── 字体 ──
  'ttf': LucideIcons.type,
  'otf': LucideIcons.type,
  'ttc': LucideIcons.type,
  'woff': LucideIcons.type,
  'woff2': LucideIcons.type,

  // ── 数据库 ──
  'db': LucideIcons.database,
  'sql': LucideIcons.database,
  'mdb': LucideIcons.database,
  'sqlite': LucideIcons.database,
  'sqlite3': LucideIcons.database,

  // ── 结构化数据 / 源码 ──
  'json': LucideIcons.fileJson,
  'xml': LucideIcons.fileCode,
  'yaml': LucideIcons.fileCode,
  'yml': LucideIcons.fileCode,
  'toml': LucideIcons.fileCode,
  'ini': LucideIcons.fileCode,
  'conf': LucideIcons.fileCode,
  'html': LucideIcons.fileCode,
  'htm': LucideIcons.fileCode,
  'css': LucideIcons.fileCode,
  'scss': LucideIcons.fileCode,
  'js': LucideIcons.fileCode,
  'mjs': LucideIcons.fileCode,
  'jsx': LucideIcons.fileCode,
  'tsx': LucideIcons.fileCode,
  'py': LucideIcons.fileCode,
  'rs': LucideIcons.fileCode,
  'go': LucideIcons.fileCode,
  'java': LucideIcons.fileCode,
  'kt': LucideIcons.fileCode,
  'swift': LucideIcons.fileCode,
  'c': LucideIcons.fileCode,
  'cpp': LucideIcons.fileCode,
  'cc': LucideIcons.fileCode,
  'h': LucideIcons.fileCode,
  'hpp': LucideIcons.fileCode,
  'cs': LucideIcons.fileCode,
  'rb': LucideIcons.fileCode,
  'php': LucideIcons.fileCode,
  'dart': LucideIcons.fileCode,
  'lua': LucideIcons.fileCode,
  'sh': LucideIcons.fileCode,
  'bat': LucideIcons.fileCode,
  'cmd': LucideIcons.fileCode,
  'ps1': LucideIcons.fileCode,
};

/// 文件分类 → Lucide 图标。
///
/// 与侧栏分类项、搜索下拉、移动端列表共用同一套符号；改这里即全局生效。
IconData fileCategoryIcon(FileCategory category) => switch (category) {
  FileCategory.video => LucideIcons.film,
  FileCategory.audio => LucideIcons.music,
  FileCategory.document => LucideIcons.fileText,
  FileCategory.image => LucideIcons.image,
  FileCategory.program => LucideIcons.package2,
  FileCategory.archive => LucideIcons.archive,
  FileCategory.all => LucideIcons.layoutGrid,
  FileCategory.other => LucideIcons.file,
};

/// 扩展名 → Lucide 图标：精确表优先，未命中回落分类图标。
///
/// [ext] 不带点，大小写不敏感；`DownloadTask.fileExtension` 对无扩展名的
/// 文件返回 `'?'`，会自然落到 [FileCategory.other] 的通用文件图标。
IconData fileTypeIcon(String ext) {
  final e = ext.toLowerCase();
  return _extIcons[e] ?? fileCategoryIcon(FileCategory.fromExtension(e));
}

/// 文件分类 → (tile 背景色, 前景色)。
///
/// 色值来自 manifest 原型规格（`manifest.js` 的 `MF_EXT_TYPE` + `styles.css`
/// 的 `.mf-ftile.t-*`），此处是全项目唯一定义点 —— manifest 浏览列表原本自带
/// 一份同名私有实现，已收敛到这里。
///
/// `program` / `other` / `all` 没有专属色，回退中性（`surface2` + 次级文字色），
/// 靠字形而非颜色区分。
(Color bg, Color fg) fileCategoryTileColors(FileCategory category, AppColors c) =>
    switch (category) {
      FileCategory.video => (const Color(0x24A855F7), AppColors.categoryVideo),
      FileCategory.audio => (const Color(0x2406B6D4), AppColors.categoryAudio),
      FileCategory.document => (c.accentBg, c.accent),
      FileCategory.image => (const Color(0x2422C55E), AppColors.green),
      FileCategory.archive => (const Color(0x24F59E0B), AppColors.amber),
      FileCategory.program || FileCategory.other || FileCategory.all => (
        c.surface2,
        c.textSecondary,
      ),
    };

/// 扩展名 → 图标前景色。方块底色保持 `surface2` 时使用（任务列表/详情/通知）。
Color fileTypeColor(String ext, AppColors c) =>
    fileCategoryTileColors(FileCategory.fromExtension(ext.toLowerCase()), c).$2;

/// 任务行 / 卡片 / 详情面板共用的文件类型图标方块。
///
/// 底色沿用 `surface2`，只有字形着色 —— 保持既有克制的视觉调性，
/// 同时让类型在扫读时一眼可辨。
class FileTypeIconTile extends StatelessWidget {
  /// 不带点的扩展名，一般直接传 `task.fileExtension`。
  final String ext;

  /// 方块边长。
  final double size;

  /// 圆角，由调用方从 `AppMetrics` 取，避免此处硬编码度量。
  final BorderRadius borderRadius;

  /// 字形尺寸；默认取边长的 52%（24→12、34→18、40→21）。
  final double? iconSize;

  const FileTypeIconTile({
    super.key,
    required this.ext,
    required this.size,
    required this.borderRadius,
    this.iconSize,
  });

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Container(
      width: size,
      height: size,
      decoration: BoxDecoration(color: c.surface2, borderRadius: borderRadius),
      child: Center(
        child: Icon(
          fileTypeIcon(ext),
          size: iconSize ?? (size * 0.52).roundToDouble(),
          color: fileTypeColor(ext, c),
        ),
      ),
    );
  }
}
