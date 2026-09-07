/// 溢出才弹全名的省略文本。
///
/// 解决两个问题：
///
/// 1. **列表行文件名被省略号截断后无从查看全名。** 无条件套 tooltip 会让短
///    文件名也弹一个内容完全相同的气泡（纯噪音），因此先用 [TextPainter] 在
///    当前约束下试排一次，只有确实超出 [maxLines] 时才挂气泡。试排与 Text 自身
///    layout 同阶，虚拟化列表下每帧仅可见行参与计算。
///
/// 2. **`ShadTooltip` 直接包裸 `Text` 永远不弹。** 它不自带 `MouseRegion`，而是
///    把 `hoverStrategies` 注入 `ShadTheme`，等 child 内部的 `ShadGestureDetector`
///    回调 `onHoverChange`（shadcn_ui `tooltip.dart` 的 `build` 配合
///    `gesture_detector.dart` 的 `MouseRegion`）。child 是 `Text` / `Container`
///    这类朴素 widget 时没有那一层，`waitDuration` 形同虚设。
///
///    补一层 `ShadGestureDetector` 看似最省事，但它会按 theme 默认策略
///    （`hover: {onTapDown, …}`）注册 tap 识别器并赢下手势竞技场，**吞掉列表行
///    的点击选中**（已由 `test/overflow_tooltip_text_test.dart` 的穿透用例证实）。
///    因此这里自持 [ShadTooltipController]，用朴素 [MouseRegion] + [Timer] 驱动
///    显隐：不碰手势层，延迟语义也完全由本组件掌握。
library;

import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/widgets.dart';
import 'package:shadcn_ui/shadcn_ui.dart';

/// 悬浮多久弹出全名。
const Duration kOverflowTooltipDelay = Duration(milliseconds: 500);

/// 气泡自身的宽度上限。超过就换行——RSS 条目标题、种子文件名动辄上百字符，
/// 不设上限时 `ShadPortal` 只按窗口宽度 loosen 约束，气泡会拉成横跨整个窗口的
/// 一长条；而一旦真的比窗口还宽，`positionDependentBox` 会把它居中放置，
/// 两端同时被裁掉——恰好把用户想看的结尾也裁没了。
const double kOverflowTooltipMaxWidth = 420;

/// 气泡与窗口左右边缘的最小留白。窗口比 [kOverflowTooltipMaxWidth] 还窄时
/// （小窗 / 移动端）按窗口宽度收紧，保证气泡整体仍在窗口内。
const double kOverflowTooltipViewportMargin = 16;

class OverflowTooltipText extends StatefulWidget {
  /// 显示文本，同时也是未指定 [tooltip] 时的气泡内容。
  final String text;

  /// 气泡内容；省略则用 [text]（想额外带上目录等信息时才需显式传）。
  final String? tooltip;

  final TextStyle? style;
  final int maxLines;

  /// 悬浮到弹出的等待时长。
  final Duration delay;

  const OverflowTooltipText(
    this.text, {
    super.key,
    this.tooltip,
    this.style,
    this.maxLines = 1,
    this.delay = kOverflowTooltipDelay,
  });

  @override
  State<OverflowTooltipText> createState() => _OverflowTooltipTextState();
}

class _OverflowTooltipTextState extends State<OverflowTooltipText> {
  final ShadTooltipController _controller = ShadTooltipController();
  Timer? _timer;

  @override
  void didUpdateWidget(OverflowTooltipText oldWidget) {
    super.didUpdateWidget(oldWidget);
    // 列表行会复用 State：文本换了就把上一条的气泡和倒计时一并作废，
    // 否则滚动过程中可能弹出属于旧任务的文件名。
    if (oldWidget.text != widget.text || oldWidget.tooltip != widget.tooltip) {
      _timer?.cancel();
      _timer = null;
      if (_controller.isOpen) _controller.hide();
    }
  }

  @override
  void dispose() {
    _timer?.cancel();
    _controller.dispose();
    super.dispose();
  }

  void _scheduleShow() {
    _timer?.cancel();
    _timer = Timer(widget.delay, () {
      if (mounted) _controller.show();
    });
  }

  void _hide() {
    _timer?.cancel();
    _timer = null;
    if (_controller.isOpen) _controller.hide();
  }

  @override
  Widget build(BuildContext context) {
    final effectiveStyle = DefaultTextStyle.of(context).style.merge(
      widget.style,
    );
    return LayoutBuilder(
      builder: (context, constraints) {
        final content = Text(
          widget.text,
          maxLines: widget.maxLines,
          overflow: TextOverflow.ellipsis,
          style: widget.style,
        );
        // 无界约束下 ellipsis 不会触发，也无从判断溢出 —— 原样返回。
        if (!constraints.hasBoundedWidth) return content;

        final painter = TextPainter(
          text: TextSpan(text: widget.text, style: effectiveStyle),
          maxLines: widget.maxLines,
          textDirection: Directionality.of(context),
          textScaler: MediaQuery.textScalerOf(context),
        )..layout(maxWidth: constraints.maxWidth);
        final overflowed = painter.didExceedMaxLines;
        painter.dispose();
        if (!overflowed) {
          // 只撤倒计时：ShadTooltip 随本分支一并从树上摘除，portal 自然消失。
          // 此处在 build 期间，不能调 _controller.hide()（notifyListeners →
          // setState during build）。
          _timer?.cancel();
          _timer = null;
          return content;
        }

        return ShadTooltip(
          controller: _controller,
          // 无入场动画：已经等了 500ms，再叠 200ms 淡入只是把等待拖长；
          // 提示要么不出现，要么立刻在那儿（同 task_list / rss_item_list）。
          effects: const [],
          builder: (context) {
            final available =
                MediaQuery.sizeOf(context).width -
                kOverflowTooltipViewportMargin * 2;
            return ConstrainedBox(
              constraints: BoxConstraints(
                // 下限兜底：窗口窄到 32px 以内时 available 会变成 0 甚至负数，
                // 直接传给 BoxConstraints 会让气泡塌成一条竖线。
                maxWidth: math.max(
                  120,
                  math.min(kOverflowTooltipMaxWidth, available),
                ),
              ),
              child: Text(widget.tooltip ?? widget.text),
            );
          },
          child: MouseRegion(
            onEnter: (_) => _scheduleShow(),
            onExit: (_) => _hide(),
            child: content,
          ),
        );
      },
    );
  }
}
