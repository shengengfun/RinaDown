// Webhook 端点对话框 + 投递日志抽屉的渲染冒烟测试。
//
// 存在的理由是一次真实事故：`ShadDialog.actions` 用
// `Flex(mainAxisSize: min)` 排布，传进去的子项拿到**无界主轴约束**，页脚
// Row 里的 `Spacer`/`Flexible` 因此抛 RenderFlex 断言 → 整帧被打断 →
// MouseTracker 卡在 device-update 相位，之后每个指针事件都断言刷屏，
// 对话框根本不出现。`flutter analyze` 对此一无所知，只有真 pump 一次才捉得住。
//
// 主题管线与 manifest_select_dialog_test.dart 同构（FluxThemeScope +
// ShadTheme + WidgetsApp），不引 MaterialApp / 完整 ShadApp。
// 不点「保存端点 / 发送测试」——那会触发 rinf 信号，需要原生 runtime。

import 'package:flutter/gestures.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flux_down/src/i18n/locale_provider.dart';
import 'package:flux_down/src/models/download_queue.dart';
import 'package:flux_down/src/models/webhook_endpoint.dart';
import 'package:flux_down/src/models/webhook_provider.dart';
import 'package:flux_down/src/theme/app_theme.dart';
import 'package:flux_down/src/theme/flux_theme_tokens.dart';
import 'package:flux_down/src/widgets/webhook_brand_marks.dart';
import 'package:flux_down/src/widgets/webhook_delivery_panel.dart';
import 'package:flux_down/src/widgets/webhook_endpoint_dialog.dart';
import 'package:flux_down/src/widgets/webhook_endpoint_list.dart';
import 'package:shadcn_ui/shadcn_ui.dart';

List<DownloadQueue> _queues() => const [
  DownloadQueue(
    queueId: kMainQueueId,
    name: '主队列',
    speedLimitKbps: 0,
    maxConcurrent: 0,
    defaultSaveDir: '',
    position: 0,
  ),
];

Widget _harness(Widget home) {
  final tokens = FluxThemeTokens.defaultDark();
  final theme = buildThemeFromTokens(tokens);
  return LocaleScope(
    s: S.of('zh'),
    child: FluxThemeScope(
      tokens: tokens,
      child: ShadTheme(
        data: theme,
        child: Directionality(
          textDirection: TextDirection.ltr,
          child: DefaultTextStyle(
            style: theme.textTheme.p.copyWith(
              color: theme.colorScheme.foreground,
            ),
            child: ShadToaster(
              child: ShadSonner(
                child: WidgetsApp(
                  color: theme.colorScheme.primary,
                  debugShowCheckedModeBanner: false,
                  home: home,
                  pageRouteBuilder: <T>(
                    RouteSettings settings,
                    WidgetBuilder builder,
                  ) {
                    return PageRouteBuilder<T>(
                      settings: settings,
                      pageBuilder: (context, _, _) => builder(context),
                    );
                  },
                ),
              ),
            ),
          ),
        ),
      ),
    ),
  );
}

/// 桌面级窗口尺寸：对话框 820 宽，默认 800×600 测试视口会挤出 overflow，
/// 那是测试环境噪音而非产品缺陷。
Future<void> _useDesktopViewport(WidgetTester tester) async {
  tester.view.physicalSize = const Size(1600, 1000);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
}

Future<void> _openWith(
  WidgetTester tester,
  void Function(BuildContext context) open,
) async {
  await tester.pumpWidget(
    _harness(
      Builder(
        builder: (context) => ShadButton(
          onPressed: () => open(context),
          child: const Text('open'),
        ),
      ),
    ),
  );
  await tester.tap(find.byType(ShadButton));
  await tester.pumpAndSettle();
}

void main() {
  // 不加载翻译表时 `S` 会退化成返回键名（`webhookEventCreated` 之类的长串），
  // 行宽断言会被这种假文案带偏 —— 真实运行永远有表。
  setUpAll(() async {
    TestWidgetsFlutterBinding.ensureInitialized();
    await I18nStore.load();
  });

  testWidgets('新建端点对话框能装配、渲染，且页脚不触发无界约束断言', (tester) async {
    await _useDesktopViewport(tester);
    final webhook = WebhookProvider();
    addTearDown(webhook.dispose);

    await _openWith(tester, (context) {
      showWebhookEndpointDialog(
        context: context,
        webhook: webhook,
        queues: _queues(),
      );
    });

    expect(tester.takeException(), isNull);
    final s = S.of('zh');
    expect(find.text(s.webhookDialogAddTitle), findsOneWidget);
    // 页脚三颗按钮都在（`Spacer` 曾在这里炸掉整帧）。
    expect(find.text(s.webhookSendTest), findsOneWidget);
    expect(find.text(s.webhookSaveEndpoint), findsOneWidget);
    // 右栏实时预览：默认无模板 → custom 信封。
    expect(find.textContaining('schemaVersion'), findsOneWidget);
  });

  /// 带着上一次的测试态再开对话框，不能在构建途中通知监听者。
  ///
  /// 回归守卫：`initState` 跑在 build 相位，里面 `resetTestState()` 一旦
  /// 通知监听者，外层 `ListenableBuilder` 就会在构建途中 `markNeedsBuild`，
  /// 抛 "setState() or markNeedsBuild() called during build" 并打断整帧。
  testWidgets('残留测试态下打开对话框不在 build 相位通知', (tester) async {
    await _useDesktopViewport(tester);
    final webhook = WebhookProvider();
    addTearDown(webhook.dispose);

    // 把在途测试态做脏：`testEndpoint` 先置状态、最后才发信号，而 widget
    // test 里没有原生 runtime，信号那一步必抛 —— 状态已经留下了。
    try {
      webhook.testEndpoint(
        const WebhookEndpoint(id: 'e1', name: 'stale', url: 'https://x.dev/h'),
      );
    } catch (_) {
      // 发信号失败是预期的，这里只要它留下的脏状态。
    }
    expect(
      webhook.testing,
      isTrue,
      reason: '前提没成立：testEndpoint 不再先置状态，这条守卫就失效了',
    );

    var notifiedDuringBuild = 0;
    void spy() => notifiedDuringBuild++;
    webhook.addListener(spy);
    addTearDown(() => webhook.removeListener(spy));

    await _openWith(tester, (context) {
      showWebhookEndpointDialog(
        context: context,
        webhook: webhook,
        queues: _queues(),
      );
    });

    expect(tester.takeException(), isNull);
    expect(notifiedDuringBuild, 0, reason: 'initState 不得通知监听者');
    // 脏状态确实被清了，页脚不会闪上一次的「发送中…」。
    expect(webhook.testing, isFalse);
    expect(find.text(S.of('zh').webhookSendTest), findsOneWidget);
  });

  testWidgets('编辑既有端点：回填字段并自动展开高级区', (tester) async {
    await _useDesktopViewport(tester);
    final webhook = WebhookProvider();
    addTearDown(webhook.dispose);

    await _openWith(tester, (context) {
      showWebhookEndpointDialog(
        context: context,
        webhook: webhook,
        queues: _queues(),
        initial: const WebhookEndpoint(
          id: 'e1',
          name: '家庭 ntfy',
          preset: 'ntfy',
          url: 'https://ntfy.sh/my-topic',
          events: ['task.completed'],
          signSecret: 'whsec_abc',
          headers: {'Authorization': 'Bearer x'},
        ),
      );
    });

    expect(tester.takeException(), isNull);
    final s = S.of('zh');
    expect(find.text(s.webhookDialogEditTitle), findsOneWidget);
    expect(find.text('家庭 ntfy'), findsOneWidget);
    // 有自定义头/签名 → 高级区默认展开，否则用户会以为配置丢了。
    expect(find.text(s.webhookAddHeader), findsOneWidget);
    expect(find.text(s.webhookRegenerate), findsOneWidget);
  });

  testWidgets('推送记录抽屉空态能渲染，页脚按钮齐全', (tester) async {
    await _useDesktopViewport(tester);
    final webhook = WebhookProvider();
    addTearDown(webhook.dispose);

    await _openWith(tester, (context) {
      showWebhookDeliveryPanel(context: context, webhook: webhook);
    });

    expect(tester.takeException(), isNull);
    final s = S.of('zh');
    expect(find.text(s.webhookLogEmpty), findsOneWidget);
    expect(find.text(s.webhookLogSimulate), findsOneWidget);
    expect(find.text(s.webhookLogClear), findsOneWidget);
    // 一个目标都没有时不显示筛选下拉（没得筛）。
    expect(find.text(s.webhookLogFilterLabel), findsNothing);
  });

  testWidgets('推送记录抽屉：有目标时给出筛选下拉，且显示名字而非 ID', (tester) async {
    await _useDesktopViewport(tester);
    final webhook = WebhookProvider();
    addTearDown(webhook.dispose);

    await _openWith(tester, (context) {
      showWebhookDeliveryPanel(
        context: context,
        webhook: webhook,
        endpoints: const [
          WebhookEndpoint(
            id: 'wh_19fa72d86e93e408015',
            name: '家庭 ntfy',
            preset: 'ntfy',
          ),
          WebhookEndpoint(id: 'wh_b', name: 'Discord', preset: 'discord'),
        ],
        // 行内「日志」入口：打开即预选这一条。
        endpointId: 'wh_19fa72d86e93e408015',
      );
    });

    expect(tester.takeException(), isNull);
    final s = S.of('zh');
    expect(find.text(s.webhookLogFilterLabel), findsOneWidget);
    // 回归：预选项必须显示名字，不是 `wh_19fa72d86e93e408015`。
    expect(find.text('家庭 ntfy'), findsOneWidget);
    expect(find.textContaining('wh_19fa72'), findsNothing);
  });

  /// 抽屉铺满窗口高度，页脚贴底。
  ///
  /// 回归守卫：`ShadSheet` 的 `child` 槽位拿到的是**无界**高度（内部是可滚动
  /// 列），一度写死 560px —— 于是窗口越高，页脚越是浮在半空。面板高度按视口
  /// 算，这条断言把那个减掉的 chrome 常量钉住。
  testWidgets('推送记录抽屉铺满抽屉高度，页脚贴底', (tester) async {
    const viewportHeight = 1000.0;
    tester.view.physicalSize = const Size(1400, viewportHeight);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);
    final webhook = WebhookProvider();
    addTearDown(webhook.dispose);

    await _openWith(tester, (context) {
      showWebhookDeliveryPanel(context: context, webhook: webhook);
    });

    expect(tester.takeException(), isNull);
    final footerBottom = tester
        .getRect(find.text(S.of('zh').webhookLogClear))
        .bottom;
    // 页脚离窗口底部不该超过一圈内边距；写死 560 时这里是 640（差 360）。
    expect(
      viewportHeight - footerBottom,
      lessThan(48),
      reason: '页脚必须贴在抽屉底部，而不是浮在内容高度那一截',
    );
  });

  _listTests();
  _brandMarkTests();
}

/// 端点行是「字标 + 名称/URL + chips/操作 + 健康 + 开关」的定宽拼装，
/// 六个事件全订阅时最容易把窄设置正文撑爆。这里按真实设置正文宽度 pump。
void _listTests() {
  const List<WebhookEndpoint> endpoints = [
    WebhookEndpoint(
      id: 'e1',
      name: '家庭 ntfy',
      preset: 'ntfy',
      url: 'https://ntfy.sh/a-very-long-topic-name-that-should-be-masked-here',
      // 全订阅：chips 的最坏情况。
      events: [
        'task.created',
        'task.started',
        'task.completed',
        'task.failed',
        'task.paused',
        'queue.drained',
      ],
    ),
    WebhookEndpoint(
      id: 'e2',
      name: 'Discord',
      preset: 'discord',
      url: 'https://discord.com/api/webhooks/1183/9f2',
      enabled: false,
      events: ['task.completed'],
    ),
  ];

  testWidgets('端点列表在窄设置正文里不溢出', (tester) async {
    final webhook = WebhookProvider();
    addTearDown(webhook.dispose);

    await tester.pumpWidget(
      _harness(
        Align(
          alignment: Alignment.topCenter,
          child: SizedBox(
            // 设置正文的现实下限（窗口最窄 - 侧栏 - 内边距）。
            width: 560,
            child: WebhookEndpointList(
              endpoints: endpoints,
              webhook: webhook,
              pendingDeleteId: 'e2',
              onToggle: (_, _) {},
              onTest: (_) {},
              onLogs: (_) {},
              onEdit: (_) {},
              onDelete: (_) {},
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();

    expect(tester.takeException(), isNull);
    final s = S.of('zh');
    expect(find.text('家庭 ntfy'), findsOneWidget);
    // 六个事件截断成 3 + `+3`，否则行会被撑爆。
    expect(find.text('+3'), findsOneWidget);
    // 停用端点显示「已停用」而不是空白。
    expect(find.text(s.webhookHealthDisabled), findsOneWidget);
    // 无投递记录的启用端点显示「暂无投递」。
    expect(find.text(s.webhookHealthNone), findsOneWidget);

    // hover 用更宽的行内操作换掉 chips——这是行宽的最坏情况，必须同样不溢出。
    final pointer = await tester.createGesture(kind: PointerDeviceKind.mouse);
    await pointer.addPointer(location: Offset.zero);
    addTearDown(pointer.removePointer);
    await pointer.moveTo(tester.getCenter(find.text('家庭 ntfy')));
    await tester.pumpAndSettle();

    expect(tester.takeException(), isNull);
    expect(find.text(s.webhookRowTest), findsOneWidget);
    expect(find.text(s.webhookRowEdit), findsOneWidget);
    // e2 处于二次确认窗口，但它没被 hover，操作区不显示。
    expect(find.text(s.webhookRowDeleteConfirm), findsNothing);
  });
}

/// 品牌路径是离线生成后写死在源码里的字符串。解析器只认 M/L/C/Z —— 数据
/// 一旦掺进别的命令（换个生成脚本、手改一笔），`parseBrandPath` 会静默截断，
/// 图标缺一块没人看得出来。这里按包围盒兜底：每个标都得铺满 24×24 视口的
/// 大部分，缺角立刻掉出阈值。
void _brandMarkTests() {
  test('品牌路径全部解析成铺满视口的形状', () {
    expect(kWebhookBrandPaths.keys, containsAll(['ntfy', 'gotify', 'telegram', 'discord', 'slack']));
    for (final entry in kWebhookBrandPaths.entries) {
      final box = parseBrandPath(entry.value).getBounds();
      expect(box.isEmpty, isFalse, reason: '${entry.key}: 解析出空路径');
      expect(box.width, greaterThan(12), reason: '${entry.key}: 宽度只有 ${box.width}');
      expect(box.height, greaterThan(12), reason: '${entry.key}: 高度只有 ${box.height}');
      expect(box.left, greaterThanOrEqualTo(-0.5), reason: '${entry.key}: 越出视口左边');
      expect(box.right, lessThanOrEqualTo(24.5), reason: '${entry.key}: 越出视口右边');
      expect(box.bottom, lessThanOrEqualTo(24.5), reason: '${entry.key}: 越出视口下边');
    }
  });
}
