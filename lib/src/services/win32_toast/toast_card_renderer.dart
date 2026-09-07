/// Win32 Toast 卡片离屏渲染器。
///
/// 把通知卡片作为 Flutter widget 在**主引擎**里离屏光栅化为
/// premultiplied BGRA 位图，供 `UpdateLayeredWindow`（per-pixel alpha）
/// 整图贴到 Win32 分层窗口。与主窗口共享同一套主题 token、字体与
/// 渲染管线 — UI 观感与 App 内完全一致。
///
/// 渲染发生在主 isolate、显示之前一次性完成（4 张 hover 变体），
/// Toast 生命周期内 tick 只做位图切换，无异步渲染竞态。
library;

import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:shadcn_ui/shadcn_ui.dart';

import '../../theme/app_colors.dart';
import '../../theme/flux_theme_tokens.dart';
import '../../widgets/file_type_icon.dart';
import '../native_overlay/offscreen_rasterizer.dart';

// =============================================================================
// 布局常量（逻辑像素）— Win32 侧按 DPI scale 换算命中区域
// =============================================================================

/// 卡片尺寸
const double kToastCardW = 340;
const double kToastCardH = 150;

/// 卡片四周的阴影出血区（窗口尺寸 = 卡片 + 2×出血）
const double kToastShadowPad = 20;

/// 窗口逻辑尺寸
const double kToastWindowW = kToastCardW + kToastShadowPad * 2;
const double kToastWindowH = kToastCardH + kToastShadowPad * 2;

/// 命中区域（窗口逻辑坐标，含出血偏移）
/// 关闭按钮：卡片右上角 34×34 区域
const kToastHitClose = Rect.fromLTWH(
  kToastShadowPad + kToastCardW - 38,
  kToastShadowPad,
  38,
  36,
);

/// 底部动作条高度（分割线以下）
const double _actionBarH = 45;

/// 打开文件夹按钮：动作条左半
const kToastHitFolder = Rect.fromLTWH(
  kToastShadowPad,
  kToastShadowPad + kToastCardH - _actionBarH,
  kToastCardW / 2,
  _actionBarH,
);

/// 打开文件按钮：动作条右半
const kToastHitFile = Rect.fromLTWH(
  kToastShadowPad + kToastCardW / 2,
  kToastShadowPad + kToastCardH - _actionBarH,
  kToastCardW / 2,
  _actionBarH,
);

// =============================================================================
// 渲染结果
// =============================================================================

/// hover 变体索引，与 Win32 侧 `hoveredButton`（0..3）一一对应
enum ToastVariant { base, hoverClose, hoverFolder, hoverFile }

/// 单张变体位图 — premultiplied BGRA，自底向上翻转前的 top-down 行序
class ToastCardImage {
  final int width;
  final int height;
  final Uint8List bgraPremultiplied;

  const ToastCardImage(this.width, this.height, this.bgraPremultiplied);
}

/// 渲染输入
class ToastCardSpec {
  final String title;
  final String fileName;
  final String fileExt;

  /// 单文件 → 文件大小；批量 → "等 N 个文件"
  final String subtitle;
  final String openFolderLabel;
  final String openFileLabel;
  final FluxThemeTokens tokens;

  const ToastCardSpec({
    required this.title,
    required this.fileName,
    required this.fileExt,
    required this.subtitle,
    required this.openFolderLabel,
    required this.openFileLabel,
    required this.tokens,
  });
}

// =============================================================================
// 离屏渲染
// =============================================================================

/// 渲染全部 4 张 hover 变体。`scale` 为目标显示器 DPI/96。
Future<List<ToastCardImage>> renderToastCardVariants(
  ToastCardSpec spec, {
  required double scale,
}) async {
  final images = <ToastCardImage>[];
  for (final variant in ToastVariant.values) {
    images.add(await _renderVariant(spec, variant, scale));
  }
  return images;
}

Future<ToastCardImage> _renderVariant(
  ToastCardSpec spec,
  ToastVariant variant,
  double scale,
) async {
  final ui.Image image = await rasterizeWidget(
    _ToastCard(spec: spec, variant: variant),
    logicalSize: const Size(kToastWindowW, kToastWindowH),
    scale: scale,
  );
  try {
    final byteData = await image.toByteData(
      format: ui.ImageByteFormat.rawRgba,
    );
    if (byteData == null) {
      throw StateError('toByteData returned null');
    }
    return ToastCardImage(
      image.width,
      image.height,
      rgbaToPremultipliedBgra(byteData.buffer.asUint8List()),
    );
  } finally {
    image.dispose();
  }
}

// =============================================================================
// 卡片 widget — 与主窗口内旧版通知卡片同款设计语言
// =============================================================================

class _ToastCard extends StatelessWidget {
  final ToastCardSpec spec;
  final ToastVariant variant;

  const _ToastCard({required this.spec, required this.variant});

  @override
  Widget build(BuildContext context) {
    final c = AppColors.fromTokens(spec.tokens);

    return Padding(
      padding: const EdgeInsets.all(kToastShadowPad),
      child: Container(
        width: kToastCardW,
        height: kToastCardH,
        decoration: BoxDecoration(
          color: c.dialogBg,
          borderRadius: BorderRadius.circular(12),
          border: Border.all(color: c.border),
          boxShadow: [
            BoxShadow(
              color: Colors.black.withValues(alpha: 0.18),
              blurRadius: 14,
              offset: const Offset(0, 5),
            ),
            BoxShadow(
              color: Colors.black.withValues(alpha: 0.06),
              blurRadius: 4,
              offset: const Offset(0, 1),
            ),
          ],
        ),
        child: Column(
          children: [
            // === Header ===
            Padding(
              padding: const EdgeInsets.fromLTRB(14, 10, 6, 0),
              child: Row(
                children: [
                  Container(
                    width: 18,
                    height: 18,
                    decoration: BoxDecoration(
                      color: AppColors.green.withValues(alpha: 0.12),
                      borderRadius: BorderRadius.circular(9),
                    ),
                    child: const Icon(
                      LucideIcons.check,
                      size: 11,
                      color: AppColors.green,
                    ),
                  ),
                  const SizedBox(width: 7),
                  Expanded(
                    child: Text(
                      spec.title,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                        fontFamily: 'MiSans',
                        fontSize: 13,
                        fontWeight: FontWeight.w600,
                        color: c.textPrimary,
                      ),
                    ),
                  ),
                  // 关闭按钮
                  Container(
                    width: 26,
                    height: 26,
                    decoration: BoxDecoration(
                      color: variant == ToastVariant.hoverClose
                          ? c.surface3
                          : Colors.transparent,
                      borderRadius: BorderRadius.circular(6),
                    ),
                    child: Icon(
                      LucideIcons.x,
                      size: 13,
                      color: variant == ToastVariant.hoverClose
                          ? c.textPrimary
                          : c.textMuted,
                    ),
                  ),
                ],
              ),
            ),
            // === File info ===
            Expanded(
              child: Padding(
                padding: const EdgeInsets.fromLTRB(14, 6, 14, 6),
                child: Row(
                  children: [
                    Container(
                      width: 38,
                      height: 38,
                      decoration: BoxDecoration(
                        color: c.surface2,
                        borderRadius: BorderRadius.circular(8),
                        border: Border.all(
                          color: c.border.withValues(alpha: 0.5),
                        ),
                      ),
                      child: Center(
                        child: Icon(
                          fileTypeIcon(spec.fileExt),
                          size: 19,
                          color: fileTypeColor(spec.fileExt, c),
                        ),
                      ),
                    ),
                    const SizedBox(width: 10),
                    Expanded(
                      child: Column(
                        mainAxisAlignment: MainAxisAlignment.center,
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(
                            spec.fileName,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: TextStyle(
                              fontFamily: 'MiSans',
                              fontSize: 12.5,
                              fontWeight: FontWeight.w500,
                              color: c.textPrimary,
                            ),
                          ),
                          const SizedBox(height: 2),
                          Text(
                            spec.subtitle,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: TextStyle(
                              fontFamily: 'MiSans',
                              fontSize: 11,
                              color: c.textMuted,
                            ),
                          ),
                        ],
                      ),
                    ),
                  ],
                ),
              ),
            ),
            // === Divider ===
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 14),
              child: Divider(height: 1, color: c.border),
            ),
            // === Actions ===
            Padding(
              padding: const EdgeInsets.fromLTRB(14, 8, 14, 10),
              child: Row(
                children: [
                  Expanded(
                    child: _ActionButton(
                      icon: LucideIcons.folderOpen,
                      label: spec.openFolderLabel,
                      hovered: variant == ToastVariant.hoverFolder,
                      filled: false,
                      colors: c,
                    ),
                  ),
                  const SizedBox(width: 8),
                  Expanded(
                    child: _ActionButton(
                      icon: LucideIcons.externalLink,
                      label: spec.openFileLabel,
                      hovered: variant == ToastVariant.hoverFile,
                      filled: true,
                      colors: c,
                    ),
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// 动作按钮 — 复刻 ShadButton / ShadButton.outline 视觉
/// （离屏树无 ShadTheme scope，直接用 token 手绘同款样式）
class _ActionButton extends StatelessWidget {
  final IconData icon;
  final String label;
  final bool hovered;
  final bool filled;
  final AppColors colors;

  const _ActionButton({
    required this.icon,
    required this.label,
    required this.hovered,
    required this.filled,
    required this.colors,
  });

  @override
  Widget build(BuildContext context) {
    final c = colors;
    final Color bg;
    final Color fg;
    if (filled) {
      bg = hovered ? c.accentHover : c.accent;
      fg = c.accentForeground;
    } else {
      bg = hovered ? c.hoverBg : Colors.transparent;
      fg = c.textPrimary;
    }

    return Container(
      height: 30,
      decoration: BoxDecoration(
        color: bg,
        borderRadius: BorderRadius.circular(6),
        border: filled ? null : Border.all(color: c.border),
      ),
      child: Row(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          Icon(icon, size: 13, color: filled ? fg : c.textSecondary),
          const SizedBox(width: 6),
          Text(
            label,
            style: TextStyle(
              fontFamily: 'MiSans',
              fontSize: 12,
              fontWeight: FontWeight.w500,
              color: fg,
            ),
          ),
        ],
      ),
    );
  }
}
