/// 行/卡片操作按钮（[TaskActionButton]）的延迟气泡行为。
///
/// `ShadTooltip` 不自带 hover 检测，按钮自持 controller + Timer 驱动显隐
/// （见 task_list_item.dart 的说明），因此这里必须用真实 mouse pointer 验证：
/// 300ms 前不弹、到点才弹、移开即撤，且气泡逻辑不吞按钮自身的点击。
library;

import 'package:flutter/gestures.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flux_down/src/i18n/locale_provider.dart';
import 'package:flux_down/src/theme/app_theme.dart';
import 'package:flux_down/src/theme/flux_theme_tokens.dart';
import 'package:flux_down/src/widgets/task_list_item.dart';
import 'package:shadcn_ui/shadcn_ui.dart';

Widget _wrap(Widget child) {
  final tokens = FluxThemeTokens.defaultLight();
  final theme = buildThemeFromTokens(tokens);
  return FluxThemeScope(
    tokens: tokens,
    child: ShadTheme(
      data: theme,
      child: LocaleScope(
        s: S.of('zh'),
        child: Directionality(
          textDirection: TextDirection.ltr,
          child: DefaultTextStyle(
            style: theme.textTheme.p.copyWith(
              color: theme.colorScheme.foreground,
            ),
            child: WidgetsApp(
              color: theme.colorScheme.primary,
              debugShowCheckedModeBanner: false,
              home: Center(child: child),
              pageRouteBuilder: <T>(RouteSettings settings, WidgetBuilder b) {
                return PageRouteBuilder<T>(
                  settings: settings,
                  pageBuilder: (context, _, _) => b(context),
                );
              },
            ),
          ),
        ),
      ),
    ),
  );
}

Future<TestGesture> _hover(WidgetTester tester, Finder finder) async {
  final gesture = await tester.createGesture(kind: PointerDeviceKind.mouse);
  await gesture.addPointer(location: Offset.zero);
  addTearDown(gesture.removePointer);
  await tester.pump();
  await gesture.moveTo(tester.getCenter(finder));
  await tester.pump();
  return gesture;
}

void main() {
  testWidgets('悬浮 300ms 后才弹出提示，移开即撤', (tester) async {
    await tester.pumpWidget(
      _wrap(
        const TaskActionButton(icon: LucideIcons.fileOutput, tooltip: '打开文件'),
      ),
    );
    final button = find.byType(TaskActionButton);

    final gesture = await _hover(tester, button);
    expect(find.text('打开文件'), findsNothing, reason: '刚进入不应立刻弹');

    await tester.pump(const Duration(milliseconds: 299));
    expect(find.text('打开文件'), findsNothing, reason: '未到 300ms 不应弹');
    await tester.pump(const Duration(milliseconds: 2));
    await tester.pumpAndSettle();
    expect(find.text('打开文件'), findsOneWidget);

    await gesture.moveTo(Offset.zero);
    await tester.pumpAndSettle();
    expect(find.text('打开文件'), findsNothing);
  });

  testWidgets('未传 tooltip 时不挂气泡', (tester) async {
    await tester.pumpWidget(
      _wrap(const TaskActionButton(icon: LucideIcons.folderOpen)),
    );
    await _hover(tester, find.byType(TaskActionButton));
    await tester.pump(const Duration(seconds: 1));
    expect(find.byType(ShadTooltip), findsNothing);
  });

  testWidgets('气泡逻辑不影响点击，且点下即撤已弹出的气泡', (tester) async {
    var taps = 0;
    await tester.pumpWidget(
      _wrap(
        TaskActionButton(
          icon: LucideIcons.fileOutput,
          tooltip: '打开文件',
          onTap: () => taps++,
        ),
      ),
    );
    final button = find.byType(TaskActionButton);

    await _hover(tester, button);
    await tester.pump(const Duration(milliseconds: 300));
    await tester.pumpAndSettle();
    expect(find.text('打开文件'), findsOneWidget);

    await tester.tap(button);
    await tester.pumpAndSettle();
    expect(taps, 1);
    expect(find.text('打开文件'), findsNothing, reason: '点下应立刻撤掉气泡');
  });
}
