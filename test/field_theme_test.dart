// 字段主题接线契约（app_theme 的 _inputTheme/_selectTheme）：
//
// 1. `inputBackground` token 必须真正落到 ShadInput 的装饰填充上——浅色
//    主题下字段与面板同白、只剩细边框的「贴纸感」正是此接线缺口造成，
//    回归意味着视觉层次再次消失。
// 2. ShadSelect 的选项弹层必须**首帧即现**（selectTheme.effects 为空）——
//    默认 150ms 渐显+缩放让高频下拉显得迟钝。断言 pump 一帧后选项已
//    完全可见，若有人重新引入动画（首帧透明/未完成），此测试即失败。
//
// 主题管线装载与 webhook_endpoint_dialog_test.dart 同构（FluxThemeScope +
// ShadTheme + WidgetsApp，不引 MaterialApp）。

import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flux_down/src/theme/app_theme.dart';
import 'package:flux_down/src/theme/flux_theme_tokens.dart';
import 'package:shadcn_ui/shadcn_ui.dart';

Widget _harness(FluxThemeTokens tokens, Widget home) {
  final theme = buildThemeFromTokens(tokens);
  return ShadTheme(
    data: theme,
    child: Directionality(
      textDirection: TextDirection.ltr,
      child: DefaultTextStyle(
        style: theme.textTheme.p.copyWith(color: theme.colorScheme.foreground),
        child: WidgetsApp(
          color: theme.colorScheme.primary,
          debugShowCheckedModeBanner: false,
          home: home,
          pageRouteBuilder: <T>(RouteSettings settings, WidgetBuilder builder) {
            return PageRouteBuilder<T>(
              settings: settings,
              pageBuilder: (context, _, _) => builder(context),
            );
          },
        ),
      ),
    ),
  );
}

void main() {
  testWidgets('浅色主题字段填充 = inputBackground token（层次感接线）', (tester) async {
    final tokens = FluxThemeTokens.defaultLight();
    await tester.pumpWidget(
      _harness(
        tokens,
        const Center(
          child: SizedBox(width: 240, child: ShadInput(placeholder: Text('p'))),
        ),
      ),
    );
    await tester.pump();

    // 字段填充经 ShadDecorator 落为 BoxDecoration/ShapeDecoration 的 color。
    final decorated = tester
        .widgetList<DecoratedBox>(
          find.descendant(
            of: find.byType(ShadInput),
            matching: find.byType(DecoratedBox),
          ),
        )
        .map((w) => w.decoration)
        .toList();
    final fillColors = decorated
        .map(
          (d) => switch (d) {
            BoxDecoration(:final color) => color,
            ShapeDecoration(:final color) => color,
            _ => null,
          },
        )
        .whereType<Color>()
        .toList();
    expect(
      fillColors,
      contains(tokens.inputBackground),
      reason: 'inputBackground token 必须落到字段填充（贴纸感回归防线）',
    );
    // 且不再是纯白——与白色面板必须有可辨层次。
    expect(tokens.inputBackground, isNot(const Color(0xFFFFFFFF)));
  });

  testWidgets('select 弹层零动画：点击即现且无渐显中间态', (tester) async {
    final tokens = FluxThemeTokens.defaultLight();
    await tester.pumpWidget(
      _harness(
        tokens,
        Center(
          child: SizedBox(
            width: 240,
            child: ShadSelect<String>(
              placeholder: const Text('pick'),
              options: const [
                ShadOption(value: 'a', child: Text('Alpha')),
                ShadOption(value: 'b', child: Text('Beta')),
              ],
              selectedOptionBuilder: (context, v) => Text(v),
            ),
          ),
        ),
      ),
    );
    await tester.pump();

    await tester.tap(find.byType(ShadSelect<String>));
    // 第一帧：overlay portal 挂载（与动画无关）；第二帧：内容呈现。
    // 若渐显动画被重新引入（默认 150ms），第二帧 opacity 仅 ~0.1，
    // 下方全不透明断言即失败——契约仍然是「点击即现，无渐显」。
    await tester.pump();
    await tester.pump();

    expect(find.text('Alpha'), findsOneWidget);
    // 渐显动画会在首帧留下 opacity < 1 的 FadeTransition/Animate 中间态；
    // effects 为空时选项子树中不应存在仍在推进的动画淡入。
    final opacities = tester
        .widgetList<FadeTransition>(
          find.ancestor(
            of: find.text('Alpha'),
            matching: find.byType(FadeTransition),
          ),
        )
        .map((w) => w.opacity.value)
        .toList();
    for (final o in opacities) {
      expect(o, 1.0, reason: 'select 弹层不得有渐显中间态（effects 已清零）');
    }
  });
}
