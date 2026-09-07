import 'package:flutter/widgets.dart';

import '../theme/app_colors.dart';
import '../theme/app_metrics.dart';

/// 轻量 Markdown 子集渲染器：逐行解析 `#`/`##`/`###` 标题、`- ` 无序列表、
/// `1. ` 有序列表，行内加粗/代码见 [InlineMarkdown]。不支持嵌套列表、代码块、
/// 链接等完整 Markdown 语法，服务于变更日志、远程活动说明等轻量场景。
///
/// [fontScale]/[lineHeightScale] 缩放基准字号（正文 12 / H3 12 / H2 13 / H1 15）
/// 与行高（1.5），供不同调用方按场景微调而无需各自重写解析逻辑。
class MarkdownBody extends StatelessWidget {
  final String markdown;
  final double fontScale;
  final double lineHeightScale;

  const MarkdownBody({
    super.key,
    required this.markdown,
    this.fontScale = 1,
    this.lineHeightScale = 1,
  });

  static final _orderedItem = RegExp(r'^(\d+)\.\s+(.*)$');

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final bodySize = 12 * fontScale;
    final bodyHeight = 1.5 * lineHeightScale;
    final widgets = <Widget>[];

    for (final line in markdown.split('\n')) {
      final trimmed = line.trim();
      if (trimmed.isEmpty) continue;

      if (trimmed.startsWith('### ')) {
        widgets.add(
          _heading(trimmed.substring(4), 12 * fontScale, c, top: 8, bottom: 3),
        );
      } else if (trimmed.startsWith('## ')) {
        widgets.add(
          _heading(trimmed.substring(3), 13 * fontScale, c, top: 10, bottom: 4),
        );
      } else if (trimmed.startsWith('# ')) {
        widgets.add(
          _heading(trimmed.substring(2), 15 * fontScale, c, top: 12, bottom: 5),
        );
      } else if (trimmed.startsWith('- ')) {
        widgets.add(_bullet(trimmed.substring(2), bodySize, bodyHeight, c));
      } else if (_orderedItem.hasMatch(trimmed)) {
        final match = _orderedItem.firstMatch(trimmed)!;
        widgets.add(
          _ordered(match.group(1)!, match.group(2)!, bodySize, bodyHeight, c),
        );
      } else {
        widgets.add(
          Padding(
            padding: const EdgeInsets.symmetric(vertical: 2),
            child: InlineMarkdown(
              text: trimmed,
              style: TextStyle(
                fontSize: bodySize,
                color: c.textSecondary,
                height: bodyHeight,
              ),
            ),
          ),
        );
      }
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: widgets,
    );
  }

  Widget _heading(
    String text,
    double size,
    AppColors c, {
    required double top,
    required double bottom,
  }) {
    return Padding(
      padding: EdgeInsets.only(top: top, bottom: bottom),
      child: Text(
        text,
        style: TextStyle(
          fontSize: size,
          fontWeight: FontWeight.w600,
          color: c.textPrimary,
        ),
      ),
    );
  }

  Widget _bullet(String text, double size, double height, AppColors c) {
    return Padding(
      padding: const EdgeInsets.only(left: 4, top: 2, bottom: 2),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Padding(
            padding: const EdgeInsets.only(top: 5.5),
            child: Container(
              width: 4,
              height: 4,
              decoration: BoxDecoration(shape: BoxShape.circle, color: c.accent),
            ),
          ),
          const SizedBox(width: 8),
          Expanded(
            child: InlineMarkdown(
              text: text,
              style: TextStyle(fontSize: size, color: c.textSecondary, height: height),
            ),
          ),
        ],
      ),
    );
  }

  Widget _ordered(
    String number,
    String text,
    double size,
    double height,
    AppColors c,
  ) {
    return Padding(
      padding: const EdgeInsets.only(left: 4, top: 2, bottom: 2),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 18,
            child: Text(
              '$number.',
              style: TextStyle(
                fontSize: size,
                fontWeight: FontWeight.w600,
                color: c.accent,
                height: height,
              ),
            ),
          ),
          const SizedBox(width: 4),
          Expanded(
            child: InlineMarkdown(
              text: text,
              style: TextStyle(fontSize: size, color: c.textSecondary, height: height),
            ),
          ),
        ],
      ),
    );
  }
}

/// 渲染行内 Markdown：`**加粗**` 与 `` `代码` `` 两种 span；其余文本原样输出。
/// 代码 span 字号相对宿主 [style] 字号缩小 1px，跟随字号缩放自适应。
class InlineMarkdown extends StatelessWidget {
  final String text;
  final TextStyle style;

  const InlineMarkdown({super.key, required this.text, required this.style});

  static final _inline = RegExp(r'\*\*(.+?)\*\*|`([^`]+)`');

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    final m = AppMetrics.of(context);
    return Text.rich(TextSpan(children: _parse(c, m)), style: style);
  }

  List<InlineSpan> _parse(AppColors c, AppMetrics m) {
    final spans = <InlineSpan>[];
    final codeSize = (style.fontSize ?? 12) - 1;
    int lastEnd = 0;

    for (final match in _inline.allMatches(text)) {
      if (match.start > lastEnd) {
        spans.add(TextSpan(text: text.substring(lastEnd, match.start)));
      }

      if (match.group(1) != null) {
        spans.add(
          TextSpan(
            text: match.group(1),
            style: TextStyle(fontWeight: FontWeight.w600, color: c.textPrimary),
          ),
        );
      } else if (match.group(2) != null) {
        spans.add(
          WidgetSpan(
            alignment: PlaceholderAlignment.middle,
            child: Container(
              padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 1),
              decoration: BoxDecoration(color: c.surface3, borderRadius: m.brSm),
              child: Text(
                match.group(2)!,
                style: TextStyle(
                  fontSize: codeSize,
                  fontFamily: 'monospace',
                  color: c.accent,
                ),
              ),
            ),
          ),
        );
      }

      lastEnd = match.end;
    }

    if (lastEnd < text.length) {
      spans.add(TextSpan(text: text.substring(lastEnd)));
    }

    return spans;
  }
}
