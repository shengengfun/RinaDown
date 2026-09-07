import 'dart:convert';
import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:launch_at_startup/launch_at_startup.dart';

import 'package:flux_down/src/bindings/bindings.dart';
import 'package:flux_down/src/models/download_controller.dart';
import 'package:flux_down/src/models/settings_provider.dart';
import 'package:flux_down/src/services/external_download_service.dart';
import 'package:flux_down/src/services/popup_window_service.dart';
import 'package:flux_down/src/theme/theme_provider.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  launchAtStartup.setup(
    appName: 'FluxDownTest',
    appPath: Platform.resolvedExecutable,
  );

  const channel = MethodChannel('fluxdown/popup_host');

  testWidgets(
    'cold-start request waits for config and the first queue snapshot',
    (tester) async {
      final showPayloads = <String>[];
      tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(channel, (
        call,
      ) async {
        switch (call.method) {
          case 'show':
            showPayloads.add(call.arguments as String);
            return true;
          case 'close':
            return null;
        }
        return null;
      });

      final navKey = GlobalKey<NavigatorState>();
      await tester.pumpWidget(
        WidgetsApp(
          color: const Color(0xFF000000),
          navigatorKey: navKey,
          onGenerateRoute: (_) =>
              PageRouteBuilder(pageBuilder: (_, _, _) => const SizedBox()),
        ),
      );

      final settings = SettingsProvider(enableFileAssoc: false);
      final popup = PopupWindowService.instance;
      popup.init(themeProvider: ThemeProvider(), navigatorKey: navKey);
      ExternalDownloadService.init(
        settingsProvider: settings,
        navigatorKey: navKey,
      );
      DownloadController? controller;
      addTearDown(() async {
        ExternalDownloadService.shutdown();
        await popup.close();
        controller?.dispose();
        settings.dispose();
        tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
          channel,
          null,
        );
      });

      ExternalDownloadService.handleLocalRequest(
        url: 'https://example.com/archive.zip',
        filename: 'archive.zip',
      );
      await tester.pump();
      expect(showPayloads, isEmpty, reason: 'config is not loaded yet');

      settings.applyLoadedConfig([
        ConfigEntry(key: 'default_save_dir', value: r'C:\Users\zero\Downloads'),
        ConfigEntry(key: 'program_category_migrated', value: 'true'),
      ]);
      await tester.pump();
      expect(
        showPayloads,
        isEmpty,
        reason: 'the download controller is not available yet',
      );

      controller = DownloadController(requestInitialState: false);
      await tester.pump();
      expect(
        showPayloads,
        isEmpty,
        reason: 'an empty list is not an authoritative AllQueues snapshot',
      );

      controller.applyLoadedQueues(const <QueueInfo>[]);
      await tester.pump();
      await tester.pump();

      expect(showPayloads, hasLength(1));
      final payload = jsonDecode(showPayloads.single) as Map<String, dynamic>;
      final request = payload['req'] as Map<String, dynamic>;
      final environment = payload['env'] as Map<String, dynamic>;
      expect(request['saveDir'], r'C:\Users\zero\Downloads');
      expect(environment['queues'], isEmpty);

      await popup.close();
      await tester.pump();
    },
  );
}
