import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flux_down/src/popup/popup_app.dart';
import 'package:flux_down/src/popup/popup_payload.dart';
import 'package:flux_down/src/theme/flux_theme_tokens.dart';
import 'package:flux_down/src/widgets/context_menu.dart';
import 'package:flux_down/src/widgets/quick_download_form.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  const channel = MethodChannel('fluxdown/popup_child');

  QuickPopupPayload payload(int requestId) => QuickPopupPayload(
    requestId: requestId,
    url: 'https://example.com/file.zip',
    filename: 'file.zip',
    fileSize: 1024,
    mimeType: 'application/zip',
    saveDir: r'C:\Downloads',
    cookies: '',
    locale: 'en',
    tokensJson: FluxThemeTokens.defaultDark().toJson(),
    defaultSegments: 0,
    lastDialogThreads: '',
    defaultQueueId: '',
    queues: const [],
  );

  Future<void> deliverPayload(
    WidgetTester tester,
    QuickPopupPayload payload,
  ) async {
    await tester.binding.defaultBinaryMessenger.handlePlatformMessage(
      channel.name,
      const StandardMethodCodec().encodeMethodCall(
        MethodCall('setPayload', payload.toJsonString()),
      ),
      (_) {},
    );
    await tester.pump();
  }

  testWidgets('new payload removes overlay entries from the previous session', (
    tester,
  ) async {
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
      channel,
      (_) async => null,
    );
    addTearDown(
      () => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
        channel,
        null,
      ),
    );

    await tester.pumpWidget(const QuickPopupApp());
    await tester.pump();
    await deliverPayload(tester, payload(1));

    final formContext = tester.element(find.byType(QuickDownloadForm));
    showContextMenu(
      formContext,
      Offset.zero,
      items: [
        ContextMenuItem(
          icon: const IconData(0xe000),
          label: 'stale overlay marker',
          color: const Color(0xFFFFFFFF),
          action: () {},
        ),
      ],
    );
    await tester.pump();
    expect(find.text('stale overlay marker'), findsOneWidget);

    await deliverPayload(tester, payload(2));

    expect(
      find.text('stale overlay marker'),
      findsNothing,
      reason: 'a new popup session must not retain hit-test overlays',
    );
  });
}
