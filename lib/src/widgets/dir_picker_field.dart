import 'package:flutter/widgets.dart';
import 'package:shadcn_ui/shadcn_ui.dart';

import '../i18n/locale_provider.dart';
import '../services/recent_dirs.dart';
import '../theme/app_colors.dart';
import '../theme/app_metrics.dart';

/// 目录选择器一体化控件。
///
/// 外观是一个输入框，路径文本在左，浏览按钮嵌入右侧，
/// 中间用竖分隔线分开，视觉上是一个整体。
///
/// 传 [onPathSelected] 时，在字段**正下方**直接排布最近用过的目录
/// （一行小标签，随输入框一起显示），点选即回填；不再用二级弹窗。
class DirPickerField extends StatelessWidget {
  final String path;
  final String? placeholder;
  final bool enabled;
  final VoidCallback? onTap;

  /// 选择最近目录时的回调（null = 不在下方显示最近目录）。
  final ValueChanged<String>? onPathSelected;

  const DirPickerField({
    super.key,
    required this.path,
    this.placeholder,
    this.enabled = true,
    this.onTap,
    this.onPathSelected,
  });

  /// 取目录名的最后一段做小标签（`D:\a\b\下载` → `下载`）。
  static String _leaf(String dir) {
    final clean = dir.endsWith('\\') || dir.endsWith('/')
        ? dir.substring(0, dir.length - 1)
        : dir;
    final idx = clean.lastIndexOf(RegExp(r'[\\/]'));
    if (idx >= 0 && idx < clean.length - 1) return clean.substring(idx + 1);
    return clean;
  }

  /// 字段下方最近目录小标签行。
  Widget _recentBar(BuildContext context) {
    final c = AppColors.of(context);
    final s = LocaleScope.of(context);
    final onPick = onPathSelected;
    final dirs = RecentDirs.instance.items;
    if (onPick == null || dirs.isEmpty) return const SizedBox.shrink();
    return Padding(
      padding: const EdgeInsets.only(left: 12, top: 6),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            s.recentDirs,
            style: TextStyle(fontSize: 10.5, color: c.textMuted),
          ),
          const SizedBox(height: 4),
          SingleChildScrollView(
            scrollDirection: Axis.horizontal,
            child: Row(
              children: [
                for (var i = 0; i < dirs.length; i++) ...[
                  if (i > 0) const SizedBox(width: 6),
                  MouseRegion(
                    cursor: SystemMouseCursors.click,
                    child: GestureDetector(
                      onTap: () => onPick(dirs[i]),
                      child: Container(
                        padding: const EdgeInsets.symmetric(
                          horizontal: 8,
                          vertical: 3,
                        ),
                        decoration: BoxDecoration(
                          color: c.surface2,
                          borderRadius: BorderRadius.circular(10),
                          border: Border.all(color: c.border, width: 0.5),
                        ),
                        child: Row(
                          mainAxisSize: MainAxisSize.min,
                          children: [
                            Icon(
                              LucideIcons.folder,
                              size: 11,
                              color: c.textSecondary,
                            ),
                            const SizedBox(width: 4),
                            ConstrainedBox(
                              constraints: const BoxConstraints(maxWidth: 220),
                              child: Text(
                                _leaf(dirs[i]),
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: TextStyle(
                                  fontSize: 11,
                                  color: c.textPrimary,
                                ),
                              ),
                            ),
                          ],
                        ),
                      ),
                    ),
                  ),
                ],
              ],
            ),
          ),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final m = AppMetrics.of(context);
    final s = LocaleScope.of(context);
    final hasPath = path.isNotEmpty;
    final displayText = hasPath ? path : (placeholder ?? s.selectSaveDir);

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        GestureDetector(
          onTap: enabled ? onTap : null,
          child: MouseRegion(
            cursor: enabled
                ? SystemMouseCursors.click
                : SystemMouseCursors.basic,
            child: Container(
              // 与全局 input/select 字段同一套视觉：32 高（对齐按钮
              // buttonHeightMd）、inputBg 填充、inputBorder 边框、radiusInput 圆角。
              height: 32,
              decoration: BoxDecoration(
                color: c.inputBg,
                borderRadius: m.brInput,
                border: Border.all(color: c.inputBorder, width: 1),
              ),
              child: Row(
                children: [
                  // 路径文本
                  Expanded(
                    child: Padding(
                      padding: const EdgeInsets.symmetric(horizontal: 12),
                      child: Text(
                        displayText,
                        style: TextStyle(
                          fontSize: 13,
                          color: hasPath ? c.textPrimary : c.textMuted,
                        ),
                        overflow: TextOverflow.ellipsis,
                        maxLines: 1,
                      ),
                    ),
                  ),
                  // 竖分隔线
                  Container(width: 1, height: 20, color: c.border),
                  // 浏览按钮区域
                  Padding(
                    padding: const EdgeInsets.symmetric(horizontal: 10),
                    child: Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Icon(
                          LucideIcons.folderOpen,
                          size: 14,
                          color: enabled
                              ? c.textSecondary
                              : m.disabled(c.textMuted),
                        ),
                        const SizedBox(width: 5),
                        Text(
                          s.browse,
                          style: TextStyle(
                            fontSize: 12.5,
                            color: enabled
                                ? c.textSecondary
                                : m.disabled(c.textMuted),
                          ),
                        ),
                      ],
                    ),
                  ),
                ],
              ),
            ),
          ),
        ),
        // 最近目录小标签直接排布在字段下方
        if (enabled && onPathSelected != null) _recentBar(context),
      ],
    );
  }
}
