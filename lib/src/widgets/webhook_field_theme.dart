// Webhook 配置面的紧凑表单密度。
//
// 这些界面（端点对话框、推送记录抽屉）是**配置面**：一屏里挤着预设网格、
// 名称/URL、事件 chips、队列过滤、高级区，字段多而单个字段不重要。默认
// 12/8 的输入内边距在这里显得笨重，缩一档能让整屏少滚动一次。
//
// 只覆盖 input / select 的内边距与字号，不动配色与边框 —— 主题仍由
// `ShadTheme.of(context)` 决定，13 套配色照常适配。

import 'package:flutter/widgets.dart';
import 'package:shadcn_ui/shadcn_ui.dart';

/// 输入/下拉的紧凑内边距（默认 12/8）。
const EdgeInsets kWebhookFieldPadding = EdgeInsets.symmetric(
  horizontal: 10,
  vertical: 5,
);

/// 焦点环画在边框**外面**（`secondaryFocusedBorder`，约 3px）。字段贴着
/// 滚动视口边缘时那一圈会被裁掉，看起来像"边框超出宽度"。给容器留出这点
/// 余量即可。
const double kWebhookFocusRingSlack = 4;

/// 给子树套一层紧凑的 input / select 密度。
class WebhookFieldTheme extends StatelessWidget {
  const WebhookFieldTheme({super.key, required this.child});

  final Widget child;

  @override
  Widget build(BuildContext context) {
    final base = ShadTheme.of(context);
    final fieldStyle = base.textTheme.muted.copyWith(fontSize: 12.5);
    return ShadTheme(
      data: base.copyWith(
        inputTheme: base.inputTheme.copyWith(
          padding: kWebhookFieldPadding,
          style: fieldStyle,
          placeholderStyle: fieldStyle,
        ),
        selectTheme: base.selectTheme.copyWith(padding: kWebhookFieldPadding),
      ),
      child: child,
    );
  }
}
