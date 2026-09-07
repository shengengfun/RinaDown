import 'dart:convert';
import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:launch_at_startup/launch_at_startup.dart';

import 'package:flux_down/src/bindings/bindings.dart';
import 'package:flux_down/src/models/settings_provider.dart';
import 'package:flux_down/src/i18n/locale_provider.dart';
import 'package:flux_down/src/models/plugin_provider.dart';
import 'package:flux_down/src/pages/settings_page.dart';
import 'package:flux_down/src/services/cloud/cloud_auth_service.dart';
import 'package:flux_down/src/services/kv_store.dart';
import 'package:flux_down/src/theme/app_theme.dart';
import 'package:flux_down/src/theme/flux_theme_tokens.dart';
import 'package:shadcn_ui/shadcn_ui.dart';

Widget _wrap(Widget child) {
  final tokens = FluxThemeTokens.defaultDark();
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
              home: child,
              pageRouteBuilder: <T>(settings, builder) => PageRouteBuilder<T>(
                settings: settings,
                pageBuilder: (context, _, _) => builder(context),
              ),
            ),
          ),
        ),
      ),
    ),
  );
}

void main() {
  final binding = TestWidgetsFlutterBinding.ensureInitialized();
  setUpAll(I18nStore.load);

  launchAtStartup.setup(
    appName: 'FluxDownTest',
    appPath: Platform.resolvedExecutable,
  );
  binding.defaultBinaryMessenger.setMockMethodCallHandler(
    const MethodChannel('launch_at_startup'),
    (call) async => call.method == 'launchAtStartupIsEnabled' ? false : null,
  );

  void load(SettingsProvider settings, String value) {
    try {
      settings.applyLoadedConfig([
        ConfigEntry(key: 'referral_feature_enabled', value: value),
        ConfigEntry(key: 'program_category_migrated', value: 'true'),
      ]);
    } on ArgumentError {
      // rinf native library is unavailable in the test VM.
    }
  }

  test('referral feature stays hidden until explicitly enabled', () {
    final settings = SettingsProvider(enableFileAssoc: false);
    addTearDown(settings.dispose);

    expect(settings.referralFeatureEnabled, isFalse);

    load(settings, 'true');
    expect(settings.referralFeatureEnabled, isTrue);

    load(settings, 'false');
    expect(settings.referralFeatureEnabled, isFalse);
  });

  testWidgets('sidebar follows the persisted referral preference', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(1218, 772);
    addTearDown(tester.view.reset);
    final dir = Directory.systemTemp.createTempSync('referral_pref_test');
    final storeFile = File('${dir.path}${Platform.pathSeparator}settings.json');
    KvStore.instance.debugReset();
    KvStore.instance.debugInitPortable(storeFile);
    addTearDown(() {
      KvStore.instance.debugReset();
      dir.deleteSync(recursive: true);
    });
    await KvStore.instance.setString('cloud_access_token', 'access');
    await KvStore.instance.setString('cloud_refresh_token', 'refresh');
    await KvStore.instance.setString(
      'cloud_user',
      jsonEncode({
        'id': 'user-1',
        'email': 'user@example.com',
        'nickname': 'Tester',
        'plan': 'free',
        'status': 'active',
        'createdAt': '2026-08-17T00:00:00Z',
      }),
    );
    await KvStore.instance.flush();
    expect(CloudAuthService.instance.isLoggedIn, isTrue);

    final settings = SettingsProvider(enableFileAssoc: false);
    final plugins = PluginProvider();
    addTearDown(settings.dispose);
    addTearDown(plugins.dispose);

    await tester.pumpWidget(
      _wrap(
        SettingsPage(
          onBack: () {},
          settingsProvider: settings,
          pluginProvider: plugins,
        ),
      ),
    );
    expect(find.byKey(const ValueKey('settings-nav-referral')), findsNothing);

    try {
      settings.setReferralFeatureEnabled(true);
    } on ArgumentError {
      // rinf native library is unavailable in the test VM.
    }
    await tester.pump();
    expect(find.byKey(const ValueKey('settings-nav-referral')), findsOneWidget);

    try {
      settings.setReferralFeatureEnabled(false);
    } on ArgumentError {
      // rinf native library is unavailable in the test VM.
    }
    await tester.pump();
    expect(find.byKey(const ValueKey('settings-nav-referral')), findsNothing);
  });
}
