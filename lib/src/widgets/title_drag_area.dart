import 'package:flutter/widgets.dart';
import 'package:window_manager/window_manager.dart';

/// 替代 [DragToMoveArea] 的自定义标题栏拖拽区域，解决子组件点击延迟问题。
///
/// 原版 [DragToMoveArea] 在同一个 [GestureDetector] 上同时注册了
/// `onPanStart` 和 `onDoubleTap`。Flutter 的手势竞技场在检测到
/// `DoubleTapGestureRecognizer` 时，必须等待 300ms（kDoubleTapTimeout）
/// 来判断当前点击是否为双击的第一下，导致所有子组件（如文本输入框）的
/// 焦点获取被延迟 ~300ms。
///
/// 本组件将两个关注点分离：
/// - **拖拽**：`onPanStart` — 无竞技场延迟
/// - **双击最大化**：通过 `onTap` 手动追踪时间戳实现，不使用
///   `DoubleTapGestureRecognizer`，彻底避免延迟
class TitleDragArea extends StatefulWidget {
  const TitleDragArea({super.key, required this.child});

  final Widget child;

  @override
  State<TitleDragArea> createState() => _TitleDragAreaState();
}

class _TitleDragAreaState extends State<TitleDragArea> {
  DateTime _lastTapTime = DateTime(0);

  /// 300ms 内连续两次 tap 视为双击
  static const _doubleTapTimeout = Duration(milliseconds: 300);

  Future<void> _toggleMaximize() async {
    if (await windowManager.isMaximized()) {
      await windowManager.unmaximize();
    } else {
      await windowManager.maximize();
    }
  }

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      behavior: HitTestBehavior.translucent,
      onPanStart: (_) => windowManager.startDragging(),
      onTap: () {
        // 手动追踪双击：仅当空白区域被点击时 onTap 才会触发
        // （子组件如 ShadInput 会赢得手势竞技场，onTap 不会触发）
        final now = DateTime.now();
        if (now.difference(_lastTapTime) < _doubleTapTimeout) {
          _toggleMaximize();
          _lastTapTime = DateTime(0);
        } else {
          _lastTapTime = now;
        }
      },
      child: widget.child,
    );
  }
}
