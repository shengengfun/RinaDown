// FluxSonner —— 轻量 toast 管理器，替代 shadcn_ui 的 ShadSonner。
//
// 动机（上游 ShadSonner 两个无法通过主题修复的缺陷）：
// 1. 它用 ClipRect(OverflowBox) 包裹每个 toast，卡片 BoxShadow 在布局边界被硬裁，
//    圆角缺口处留下带直边的背景楔形；
// 2. SonnerBoxy.layout() 把区域宽度定为 max(子项宽, 约束上限≈388)，再将子项
//    position 在 Offset(0, y) —— 硬编码左对齐，内容自适应的窄卡片位置右偏失效。
//
// 本实现用 Align + Column 直接定位（无任何裁剪层），复用 ShadToast 的渲染与主题，
// 视觉与其余 shadcn 组件保持一致。支持按 toast.alignment 分组（桌面默认右下角，
// 移动端 showMobileToast 用 topCenter）、悬停暂停自动关闭、逐条进出场动画。
import 'dart:async';

import 'package:flutter/widgets.dart';
import 'package:shadcn_ui/shadcn_ui.dart';

/// 单条 toast 的运行态：进出场动画控制器 + 自动关闭计时器。
class _ToastEntry {
  _ToastEntry({
    required this.id,
    required this.toast,
    required this.controller,
  });

  final Object id;
  final ShadToast toast;
  final AnimationController controller;
  Timer? timer;
}

class _FluxSonnerScope extends InheritedWidget {
  const _FluxSonnerScope({required super.child, required this.state});

  final FluxSonnerState state;

  @override
  bool updateShouldNotify(_FluxSonnerScope oldWidget) => state != oldWidget.state;
}

/// 挂载在 App 根部（main.dart），通过 [FluxSonner.of] 显示 toast。
class FluxSonner extends StatefulWidget {
  const FluxSonner({super.key, required this.child});

  final Widget child;

  static FluxSonnerState of(BuildContext context) {
    final scope = context.getInheritedWidgetOfExactType<_FluxSonnerScope>();
    assert(scope != null, 'FluxSonner.of() 调用处的祖先中没有挂载 FluxSonner');
    return scope!.state;
  }

  @override
  State<FluxSonner> createState() => FluxSonnerState();
}

class FluxSonnerState extends State<FluxSonner> with TickerProviderStateMixin {
  static const _animationDuration = Duration(milliseconds: 200);
  static const _defaultDuration = Duration(seconds: 5);
  static const _gap = 8.0;

  final _entries = <_ToastEntry>[];

  /// 显示 [toast]，返回其标识（同 ShadSonner.show 语义：优先 toast.id）。
  /// 相同 id 的旧条目会被立即替换，避免重复点击堆积同义提示。
  Object show(ShadToast toast) {
    final id = toast.id ?? UniqueKey();
    final existing = _entries.indexWhere((e) => e.id == id);
    if (existing != -1) {
      _removeNow(_entries[existing]);
    }

    final entry = _ToastEntry(
      id: id,
      toast: toast,
      controller: AnimationController(vsync: this, duration: _animationDuration),
    );
    setState(() => _entries.add(entry));
    entry.controller.forward();
    entry.timer = Timer(toast.duration ?? _defaultDuration, () => hide(id));
    return id;
  }

  /// 播放退场动画后移除指定 toast。
  Future<void> hide(Object? id) async {
    final entry = _entries.where((e) => e.id == id).firstOrNull;
    if (entry == null) return;
    entry.timer?.cancel();
    entry.timer = null;
    await entry.controller.reverse();
    if (!mounted) return;
    _removeNow(entry);
  }

  void _removeNow(_ToastEntry entry) {
    entry.timer?.cancel();
    entry.controller.dispose();
    setState(() => _entries.remove(entry));
  }

  /// 悬停时暂停全部自动关闭，移出后重新计时（与上游 sonner 行为一致）。
  void _pauseTimers() {
    for (final entry in _entries) {
      entry.timer?.cancel();
      entry.timer = null;
    }
  }

  void _resumeTimers() {
    for (final entry in _entries) {
      entry.timer ??= Timer(
        entry.toast.duration ?? _defaultDuration,
        () => hide(entry.id),
      );
    }
  }

  @override
  void dispose() {
    for (final entry in _entries) {
      entry.timer?.cancel();
      entry.controller.dispose();
    }
    super.dispose();
  }

  Alignment _alignmentOf(ShadToast toast) => toast.alignment ?? Alignment.bottomRight;

  /// 关闭按钮：上游 ShadToast 的默认关闭按钮写死调用 ShadToaster.of(context).hide()，
  /// 在本管理器下是空操作；这里按变体前景色复刻同款按钮，改为 hide(entry.id)。
  /// 主题侧 closeIconPosition 将定位带拉伸满高（top:0, bottom:0），Center 保证
  /// 按钮在任意高度的 toast 内垂直居中。
  Widget _closeButton(_ToastEntry entry, Color foreground) {
    return Center(
      // widthFactor: 1 → 水平只占按钮自身宽度；否则 Center 会撑满 Positioned 可用宽，
      // 把按钮推到卡片水平中央并遮挡文字区域的点击。
      widthFactor: 1,
      child: ShadIconButton.ghost(
        icon: const Icon(LucideIcons.x, size: 16),
        width: 20,
        height: 20,
        padding: EdgeInsets.zero,
        foregroundColor: foreground.withValues(alpha: .5),
        hoverBackgroundColor: const Color(0x00000000),
        hoverForegroundColor: foreground,
        pressedForegroundColor: foreground,
        onPressed: () => hide(entry.id),
      ),
    );
  }

  Widget _buildEntry(_ToastEntry entry, Alignment alignment, ShadThemeData theme) {
    final curved = CurvedAnimation(
      parent: entry.controller,
      curve: Curves.easeOutCubic,
      reverseCurve: Curves.easeInCubic,
    );
    // 底部锚点上滑入场，顶部锚点下滑入场。
    final beginOffset = alignment.y > 0 ? const Offset(0, .3) : const Offset(0, -.3);
    return FadeTransition(
      opacity: curved,
      child: SlideTransition(
        position: Tween(begin: beginOffset, end: Offset.zero).animate(curved),
        child: Padding(
          padding: const EdgeInsets.only(top: _gap),
          child: ShadTheme(
            data: theme.copyWith(
              primaryToastTheme: theme.primaryToastTheme.copyWith(
                closeIcon: _closeButton(entry, theme.colorScheme.foreground),
              ),
              destructiveToastTheme: theme.destructiveToastTheme.copyWith(
                closeIcon: _closeButton(entry, theme.colorScheme.destructiveForeground),
              ),
            ),
            child: entry.toast,
          ),
        ),
      ),
    );
  }

  Widget _buildGroup(Alignment alignment, List<_ToastEntry> entries, ShadThemeData theme) {
    return Align(
      alignment: alignment,
      child: Padding(
        padding: const EdgeInsets.all(16) - const EdgeInsets.only(top: _gap),
        child: MouseRegion(
          onEnter: (_) => _pauseTimers(),
          onExit: (_) => _resumeTimers(),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: switch (alignment.x) {
              < 0 => CrossAxisAlignment.start,
              > 0 => CrossAxisAlignment.end,
              _ => CrossAxisAlignment.center,
            },
            children: [for (final entry in entries) _buildEntry(entry, alignment, theme)],
          ),
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final theme = ShadTheme.of(context);
    // 按锚点分组渲染；空白区域不拦截命中，toast 之外的点击照常穿透。
    final groups = <Alignment, List<_ToastEntry>>{};
    for (final entry in _entries) {
      groups.putIfAbsent(_alignmentOf(entry.toast), () => []).add(entry);
    }
    return _FluxSonnerScope(
      state: this,
      child: Stack(
        children: [
          widget.child,
          for (final MapEntry(key: alignment, value: entries) in groups.entries)
            _buildGroup(alignment, entries, theme),
        ],
      ),
    );
  }
}
