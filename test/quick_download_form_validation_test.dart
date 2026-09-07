import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shadcn_ui/shadcn_ui.dart';

import 'package:flux_down/src/i18n/locale_provider.dart';
import 'package:flux_down/src/theme/app_theme.dart';
import 'package:flux_down/src/theme/flux_theme_tokens.dart';
import 'package:flux_down/src/widgets/quick_download_form.dart';
import 'package:flux_down/src/widgets/flux_sonner.dart';

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
  String get siteAuthCredentials => '';

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
            child: FluxSonner(
              child: WidgetsApp(
                color: theme.colorScheme.primary,
                debugShowCheckedModeBanner: false,
                home: form,
                pageRouteBuilder:
                    <T>(RouteSettings settings, WidgetBuilder builder) {
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

void main() {
  testWidgets('empty save directory reports validation and blocks submission', (
    tester,
  ) async {
    var submitted = false;
    await tester.pumpWidget(
      _wrapForm(
        QuickDownloadForm(
          initialUrl: 'https://example.com/archive.zip',
          initialFileName: 'archive.zip',
          initialSaveDir: '',
          defaultQueueId: '',
          initialCookies: '',
          host: _FakeHost(),
          onSubmit: (_) => submitted = true,
          onCancel: () {},
        ),
      ),
    );
    await tester.pump();

    final validationText = find.text(currentS.selectSaveDir);
    final initialValidationTextCount = validationText.evaluate().length;
    final startButton = find.text(currentS.startDownload);
    expect(startButton, findsOneWidget);

    await tester.ensureVisible(startButton);
    await tester.tap(startButton);
    await tester.pump();

    expect(submitted, isFalse);
    expect(
      validationText,
      findsNWidgets(initialValidationTextCount + 1),
      reason: 'the destructive toast must explain why submission was blocked',
    );

    await tester.pump(const Duration(seconds: 5));
  });
}
