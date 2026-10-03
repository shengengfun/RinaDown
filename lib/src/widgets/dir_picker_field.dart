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
/// 传 [onPathSelected] 时，路径文本右侧多一个「最近目录」下拉触发器
/// （历史图标 + 下箭头）：点开是一列最近用过的目录，点选即回填并收起；
/// 无最近目录时在下拉里给出空态提示。不再占用输入框下方的一行小标签。
class DirPickerField extends StatefulWidget {
  final String path;
  final String? placeholder;
  final bool enabled;
  final VoidCallback? onTap;

  /// 选择最近目录时的回调（null = 不显示最近目录下拉）。
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

  @override
  State<DirPickerField> createState() => _DirPickerFieldState();
}

class _DirPickerFieldState extends State<DirPickerField> {
  final _popoverController = ShadPopoverController();

  bool get _showRecent => widget.enabled && widget.onPathSelected != null;

  void _pick(String dir) {
    _popoverController.hide();
    widget.onPathSelected?.call(dir);
  }

  /// 「最近目录」下拉内容：标题 + 目录行（叶子名 + 完整路径）/ 空态。
  Widget _recentPopover(BuildContext context) {
    final c = AppColors.of(context);
    final s = LocaleScope.of(context);
    final dirs = RecentDirs.instance.items;
    return SizedBox(
      width: 280,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisSize: MainAxisSize.min,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(12, 10, 12, 6),
            child: Text(
              s.recentDirs,
              style: TextStyle(
                fontSize: 12,
                fontWeight: FontWeight.w600,
                color: c.textPrimary,
              ),
            ),
          ),
          if (dirs.isEmpty)
            Padding(
              padding: const EdgeInsets.fromLTRB(12, 2, 12, 12),
              child: Text(
                s.recentDirsEmpty,
                style: TextStyle(fontSize: 12, color: c.textMuted),
              ),
            )
          else
            for (final dir in dirs)
              _RecentDirRow(
                leaf: DirPickerField._leaf(dir),
                full: dir,
                onTap: () => _pick(dir),
              ),
          const SizedBox(height: 6),
        ],
      ),
    );
  }

  /// 路径文本右侧的下拉触发器（历史图标 + 下箭头）。
  Widget _recentTrigger(BuildContext context) {
    final c = AppColors.of(context);
    final color = widget.enabled ? c.textSecondary : c.textDisabled;
    return MouseRegion(
      cursor: widget.enabled
          ? SystemMouseCursors.click
          : SystemMouseCursors.basic,
      child: GestureDetector(
        onTap: widget.enabled ? _popoverController.toggle : null,
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 8),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(LucideIcons.history, size: 13, color: color),
              const SizedBox(width: 2),
              Icon(LucideIcons.chevronDown, size: 11, color: color),
            ],
          ),
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final m = AppMetrics.of(context);
    final s = LocaleScope.of(context);
    final hasPath = widget.path.isNotEmpty;
    final displayText =
        hasPath ? widget.path : (widget.placeholder ?? s.selectSaveDir);

    final field = GestureDetector(
      onTap: widget.enabled ? widget.onTap : null,
      child: MouseRegion(
        cursor: widget.enabled
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
              // 最近目录下拉触发器
              if (_showRecent) _recentTrigger(context),
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
                      color: widget.enabled
                          ? c.textSecondary
                          : m.disabled(c.textMuted),
                    ),
                    const SizedBox(width: 5),
                    Text(
                      s.browse,
                      style: TextStyle(
                        fontSize: 12.5,
                        color: widget.enabled
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
    );

    if (!_showRecent) return field;

    return ShadPopover(
      controller: _popoverController,
      // RinaDown 弹出层无进出场动画(rule: shad-overlay-no-animation)。
      effects: const [],
      // 锚在字段下方左对齐（childAlignment 作用于 overlay、overlayAlignment
      // 作用于触发器的锚点——用固定方向避免 Auto 在弹窗顶部时向上翻转裁出屏外）。
      anchor: const ShadAnchor(
        childAlignment: Alignment.topLeft,
        overlayAlignment: Alignment.bottomLeft,
        offset: Offset(0, 6),
      ),
      padding: EdgeInsets.zero,
      popover: (ctx) => _recentPopover(ctx),
      child: field,
    );
  }
}

/// 单条最近目录行：叶子名（主）+ 完整路径（副，溢出省略）。hover 给底色反馈。
class _RecentDirRow extends StatefulWidget {
  final String leaf;
  final String full;
  final VoidCallback onTap;

  const _RecentDirRow({
    required this.leaf,
    required this.full,
    required this.onTap,
  });

  @override
  State<_RecentDirRow> createState() => _RecentDirRowState();
}

class _RecentDirRowState extends State<_RecentDirRow> {
  bool _hovered = false;

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final m = AppMetrics.of(context);
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      onEnter: (_) => setState(() => _hovered = true),
      onExit: (_) => setState(() => _hovered = false),
      child: GestureDetector(
        onTap: widget.onTap,
        // 即时状态切换（不用 AnimatedContainer，见 rule: no-lerp-from-transparent）。
        child: Container(
          margin: const EdgeInsets.symmetric(horizontal: 6),
          padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 6),
          decoration: BoxDecoration(
            color: _hovered ? c.surface2 : null,
            borderRadius: m.brSm,
          ),
          child: Row(
            children: [
              Icon(LucideIcons.folder, size: 13, color: c.textSecondary),
              const SizedBox(width: 8),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      widget.leaf,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(fontSize: 12.5, color: c.textPrimary),
                    ),
                    const SizedBox(height: 1),
                    Text(
                      widget.full,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(fontSize: 10.5, color: c.textMuted),
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
