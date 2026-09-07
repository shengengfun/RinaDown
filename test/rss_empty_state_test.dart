// 条目流空态的三分支契约。
//
// 空列表有三种截然不同的处境，用同一段文案糊过去等于什么都没说：
//   1. 正在抓（含新建订阅后引擎自动跑的首轮）→ 转圈 + 「正在抓取…」，等着就行；
//   2. 抓失败                                → 警示图标 + 失败原因 + 重试/检查配置；
//   3. 抓成功但源里没条目                     → 引导文案。
// 回归防线：曾经三种情况都渲染同一句「首轮抓取进行中……」，网络不好或订阅配错时
// 用户面对一片空白，既不知道该等还是该动手，也没有任何入口可去。

import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flux_down/src/bindings/bindings.dart';
import 'package:flux_down/src/i18n/locale_provider.dart';
import 'package:flux_down/src/models/rss_provider.dart';
import 'package:flux_down/src/theme/app_theme.dart';
import 'package:flux_down/src/theme/flux_theme_tokens.dart';
import 'package:flux_down/src/widgets/rss_item_list.dart';
import 'package:shadcn_ui/shadcn_ui.dart';

/// 只替换 UI 读到的三个入口。真实 provider 的数据全部来自 rinf 信号，测试里
/// 没有 Rust 侧可推；改成覆写读取面，构造函数照常跑（订阅一条永不出数的流）。
class _StubRssProvider extends RssProvider {
  _StubRssProvider({required this.source, required this.fetching});

  final RssSourceEntry source;
  final bool fetching;

  @override
  RssSourceEntry? get selectedSource => source;

  @override
  List<RssItemEntry> get selectedItems => const [];

  @override
  bool isRefreshing(String sourceId) => fetching;
}

RssSourceEntry _source({
  int lastFetchAt = 0,
  int lastSuccessAt = 0,
  String lastError = '',
  int failCount = 0,
}) => RssSourceEntry(
  sourceId: 's1',
  url: 'https://mikanani.me/RSS/Bangumi?bangumiId=3600',
  name: '',
  enabled: true,
  autoDownload: true,
  startPaused: false,
  queueId: '',
  saveDir: '',
  intervalMinutes: 30,
  includePattern: '',
  excludePattern: '',
  useRegex: false,
  smartEpisode: true,
  sizeMinBytes: 0,
  sizeMaxBytes: 0,
  sendReferer: false,
  notifyOnDownload: true,
  maxPerFetch: 20,
  cookies: '',
  userAgent: '',
  proxyUrl: '',
  lastFetchAt: lastFetchAt,
  lastSuccessAt: lastSuccessAt,
  lastError: lastError,
  failCount: failCount,
  seeded: false,
  position: 0,
  unreadCount: 0,
);

Widget _harness(Widget home) {
  final tokens = FluxThemeTokens.defaultLight();
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
            style: theme.textTheme.p,
            child: WidgetsApp(
              color: theme.colorScheme.primary,
              debugShowCheckedModeBanner: false,
              home: home,
              pageRouteBuilder: <T>(RouteSettings s, WidgetBuilder b) =>
                  PageRouteBuilder<T>(
                    settings: s,
                    pageBuilder: (context, _, _) => b(context),
                  ),
            ),
          ),
        ),
      ),
    ),
  );
}

Future<void> _pump(
  WidgetTester tester, {
  required RssSourceEntry source,
  required bool fetching,
  List<String> managed = const [],
}) async {
  final provider = _StubRssProvider(source: source, fetching: fetching);
  addTearDown(provider.dispose);
  await tester.pumpWidget(
    _harness(
      SizedBox(
        width: 900,
        height: 600,
        child: RssItemList(
          provider: provider,
          onOpenTask: (_) {},
          onManage: managed.add,
        ),
      ),
    ),
  );
  await tester.pump();
}

void main() {
  final s = S.of('zh');

  testWidgets('抓取中：转圈 + 抓取中文案，不显示失败出口', (tester) async {
    await tester.pumpWidget(const SizedBox.shrink());
    await _pump(tester, source: _source(), fetching: true);

    expect(find.text(s.rssEmptyFetching), findsOneWidget);
    expect(find.text(s.rssEmptyFetchingHint), findsOneWidget);
    expect(find.text(s.rssEmptyRetry), findsNothing);
    expect(find.text(s.rssEmptyTitle), findsNothing);
    // 订阅头也必须说「在抓」，而不是停在「尚未抓取」。
    expect(find.textContaining(s.rssRefreshing), findsWidgets);
    expect(find.textContaining(s.rssNeverFetched), findsNothing);
  });

  testWidgets('抓取失败：失败文案 + 重试与检查配置两个出口', (tester) async {
    final managed = <String>[];
    await _pump(
      tester,
      source: _source(lastFetchAt: 100, lastError: 'connection timed out', failCount: 3),
      fetching: false,
      managed: managed,
    );

    expect(find.text(s.rssEmptyError), findsOneWidget);
    expect(find.text(s.rssEmptyErrorHint), findsOneWidget);
    expect(find.text(s.rssEmptyRetry), findsOneWidget);
    // 「检查配置」在订阅头与空态各有一处，两处都通向同一个管理对话框。
    expect(find.text(s.rssCheckConfig), findsNWidgets(2));

    await tester.tap(find.text(s.rssCheckConfig).last);
    await tester.pump();
    expect(managed, ['s1']);
  });

  testWidgets('抓完但没条目：只给引导，不谎报进行中', (tester) async {
    await _pump(
      tester,
      source: _source(lastFetchAt: 100, lastSuccessAt: 100),
      fetching: false,
    );

    expect(find.text(s.rssEmptyTitle), findsOneWidget);
    expect(find.text(s.rssEmptyDesc), findsOneWidget);
    expect(find.text(s.rssEmptyFetching), findsNothing);
    expect(find.text(s.rssEmptyError), findsNothing);
    expect(find.text(s.rssEmptyRetry), findsNothing);
  });

  group('首轮抓取判据', () {
    test('已派发但结果未回 → 视为抓取中', () {
      // 引擎在派发那一刻就把 lastFetchAt 置成当前时间，此刻结果还没回来。
      expect(rssFirstFetchPending(_source(lastFetchAt: 1700000000)), isTrue);
      // 刚落库、尚未派发同样算「还没有结果」。
      expect(rssFirstFetchPending(_source()), isTrue);
    });

    test('已有结果（成功或失败）→ 不再算抓取中', () {
      expect(
        rssFirstFetchPending(
          _source(lastFetchAt: 1700000000, lastSuccessAt: 1700000000),
        ),
        isFalse,
      );
      expect(
        rssFirstFetchPending(
          _source(lastFetchAt: 1700000000, lastError: 'timed out'),
        ),
        isFalse,
      );
    });
  });

  group('自请求账本', () {
    test('自己请求的回执被抵消，广播不受影响', () {
      final ledger = PendingRequestLedger();
      // 没记过账的快照 = 抓取广播，必须放行（去解除抓取态）。
      expect(ledger.consume('s1'), isFalse);

      ledger.record('s1');
      expect(ledger.consume('s1'), isTrue);
      expect(ledger.consume('s1'), isFalse);
    });

    test('连续两次请求各抵消一次，多余的快照仍按广播处理', () {
      final ledger = PendingRequestLedger();
      ledger.record('s1');
      ledger.record('s1');
      expect(ledger.consume('s1'), isTrue);
      expect(ledger.consume('s1'), isTrue);
      expect(ledger.consume('s1'), isFalse);
    });

    test('账本按订阅隔离', () {
      final ledger = PendingRequestLedger();
      ledger.record('s1');
      expect(ledger.consume('s2'), isFalse);
      expect(ledger.consume('s1'), isTrue);
    });
  });

  group('抓取结束判据', () {
    test('订阅消失 → 结束（不留永远转圈的幽灵）', () {
      expect(rssFetchSettled(null, 0), isTrue);
      expect(rssFetchSettled(null, 1700000000), isTrue);
    });

    test('首轮：结果没回来之前不算结束，哪怕 lastFetchAt 已被乐观推进', () {
      expect(rssFetchSettled(_source(lastFetchAt: 1700000000), 0), isFalse);
      expect(
        rssFetchSettled(
          _source(lastFetchAt: 1700000000, lastSuccessAt: 1700000000),
          0,
        ),
        isTrue,
      );
      expect(
        rssFetchSettled(
          _source(lastFetchAt: 1700000000, lastError: 'timed out'),
          0,
        ),
        isTrue,
      );
    });

    test('后续轮次：lastFetchAt 相对发起时前进过即结束', () {
      expect(
        rssFetchSettled(_source(lastFetchAt: 100, lastSuccessAt: 100), 100),
        isFalse,
      );
      expect(
        rssFetchSettled(_source(lastFetchAt: 200, lastSuccessAt: 100), 100),
        isTrue,
      );
    });
  });
}
