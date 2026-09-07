// 快速下载表单站点凭据自动回填回归测试 — 行为契约：
// - 单条 URL 命中已保存站点（键与引擎 site_auth::site_key 同构）时，
//   HTTP 认证两框自动填入该站点的用户名/密码（用户可见、可改）；
// - URL 切到另一命中站点：自动值跟随更新；切到无凭据站点：自动值清空；
// - 用户手动编辑过认证输入后，URL 再变化也不再覆盖；
// - 「为此网站保存」开关不因回填被拨动。
//
// 主题管线与 quick_download_form_append_test.dart 相同（popup_app.dart
// 的最小 context 依赖），另放大测试视口以容纳展开的高级选项区。
import 'package:flutter/material.dart' show TextField;
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flux_down/src/theme/app_theme.dart';
import 'package:flux_down/src/theme/flux_theme_tokens.dart';
import 'package:flux_down/src/widgets/quick_download_form.dart';
import 'package:shadcn_ui/shadcn_ui.dart';

/// 凭据表固定的最小宿主：example.com 与 other.net:8080 各有一条凭据。
class _FakeHost implements QuickDownloadFormHost {
  @override
  List<QuickQueueOption> get queues => const [];

  @override
  List<QuickDeviceOption> get devices => const [];

  @override
  int get defaultSegments => 0;

  @override
  String get lastDialogThreads => '';

  @override
  String get siteAuthCredentials =>
      '{"example.com":{"user":"u1","pass":"p1"},'
      '"other.net:8080":{"user":"u2","pass":"p2"}}';

  @override
  Future<String?> pickDirectory({
    required String dialogTitle,
    String? initialDirectory,
  }) async => null;
}

Widget _wrapForm(QuickDownloadForm form) {
  final tokens = FluxThemeTokens.defaultDark();
  final theme = buildThemeFromTokens(tokens);
  return FluxThemeScope(
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
                home: SingleChildScrollView(child: form),
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
  );
}

/// URL 输入框（表单里唯一的裸 [TextField]，其余输入都是 ShadInput）。
TextEditingController _urlBox(WidgetTester tester) =>
    tester.widget<TextField>(find.byType(TextField)).controller!;

/// 认证密码框 — 表单里唯一 obscureText 的 [ShadInput]。
Finder get _passInput => find.byWidgetPredicate(
  (w) => w is ShadInput && w.obscureText,
);

/// 认证用户名框 — 密码框所在 Row 里的另一个 [ShadInput]（按控制器文本
/// 无法定位空值场景，改用与密码框同 Row 的结构关系）。
Finder get _userInput => find.descendant(
  of: find.ancestor(of: _passInput, matching: find.byType(Row)).first,
  matching: find.byWidgetPredicate((w) => w is ShadInput && !w.obscureText),
);

String _text(WidgetTester tester, Finder f) =>
    tester.widget<ShadInput>(f).controller!.text;

Future<void> _pumpForm(WidgetTester tester) async {
  tester.view.physicalSize = const Size(1000, 2600);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    _wrapForm(
      QuickDownloadForm(
        initialUrl: '',
        initialFileName: '',
        initialSaveDir: r'C:\downloads',
        defaultQueueId: '',
        initialCookies: '',
        host: _FakeHost(),
        onSubmit: (_) {},
        onCancel: () {},
      ),
    ),
  );
  await tester.pump();
  // 展开高级选项（折叠态入口是唯一的 chevronRight 图标行）
  await tester.tap(find.byIcon(LucideIcons.chevronRight).first);
  await tester.pump();
}

Future<void> _setUrl(WidgetTester tester, String url) async {
  _urlBox(tester).text = url;
  await tester.pump();
}

void main() {
  testWidgets('命中站点自动回填，切换 URL 跟随更新/清空，开关不被拨动', (
    tester,
  ) async {
    await _pumpForm(tester);
    expect(_text(tester, _userInput), '');
    expect(_text(tester, _passInput), '');

    // 命中 example.com → 回填
    await _setUrl(tester, 'https://example.com/file.zip');
    expect(_text(tester, _userInput), 'u1');
    expect(_text(tester, _passInput), 'p1');

    // 显式默认端口不入键，仍命中同一站点
    await _setUrl(tester, 'https://example.com:443/other.zip');
    expect(_text(tester, _userInput), 'u1');

    // 切到另一命中站点（非默认端口显式入键）→ 自动值跟随更新
    await _setUrl(tester, 'http://other.net:8080/x');
    expect(_text(tester, _userInput), 'u2');
    expect(_text(tester, _passInput), 'p2');

    // 切到无凭据站点 → 自动值清空
    await _setUrl(tester, 'https://nocred.example.org/y');
    expect(_text(tester, _userInput), '');
    expect(_text(tester, _passInput), '');

    // 「为此网站保存」（及其他）开关全程未被拨动
    expect(
      tester.widgetList<ShadSwitch>(find.byType(ShadSwitch)),
      everyElement(
        isA<ShadSwitch>().having((w) => w.value, 'value', false),
      ),
    );
  });

  testWidgets('手动编辑后不再覆盖，也不随 URL 切换清空', (tester) async {
    await _pumpForm(tester);
    await _setUrl(tester, 'https://example.com/file.zip');
    expect(_text(tester, _userInput), 'u1');

    // 手动改用户名 → 脏标记
    await tester.enterText(_userInput, 'manual');
    await tester.pump();

    // 切到另一命中站点：不覆盖
    await _setUrl(tester, 'http://other.net:8080/x');
    expect(_text(tester, _userInput), 'manual');
    expect(_text(tester, _passInput), 'p1');

    // 切到无凭据站点：也不清空
    await _setUrl(tester, 'https://nocred.example.org/y');
    expect(_text(tester, _userInput), 'manual');
    expect(_text(tester, _passInput), 'p1');
  });

  testWidgets('批量多条 URL 时不回填', (tester) async {
    await _pumpForm(tester);
    await _setUrl(
      tester,
      'https://example.com/a.zip\nhttps://example.com/b.zip',
    );
    // 批量路径认证区隐藏，控制器也不应被写入
    expect(_passInput, findsNothing);
    await _setUrl(tester, 'https://example.com/a.zip');
    expect(_text(tester, _userInput), 'u1');
  });
}
