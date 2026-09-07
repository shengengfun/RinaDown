import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:launch_at_startup/launch_at_startup.dart';

import 'package:flux_down/src/bindings/bindings.dart';
import 'package:flux_down/src/models/settings_provider.dart';

/// The "Associate ed2k links" toggle shares the opt-out mechanism of the
/// .torrent association (see torrent_assoc_toggle_test.dart): on Linux the
/// live `xdg-mime query default x-scheme-handler/ed2k` still resolves
/// FluxDown after the per-user override is dropped whenever FluxDown is the
/// only installed app declaring the handler, so a user-requested OFF must
/// win over the status Rust reports back.
void main() {
  final binding = TestWidgetsFlutterBinding.ensureInitialized();

  // SettingsProvider's constructor syncs auto-startup state over the
  // `launch_at_startup` method channel; mock it so the async sync completes.
  launchAtStartup.setup(
    appName: 'FluxDownTest',
    appPath: Platform.resolvedExecutable,
  );
  binding.defaultBinaryMessenger.setMockMethodCallHandler(
    const MethodChannel('launch_at_startup'),
    (call) async => call.method == 'launchAtStartupIsEnabled' ? false : null,
  );

  // Calls SettingsProvider.setEd2kProtocolAssociation tolerating the
  // ArgumentError from `sendSignalToRust` (the Rust dylib is not loaded under
  // `flutter test`). All in-memory state mutations complete before the
  // fire-and-forget signal send, so the observable state is unaffected.
  void userToggles(SettingsProvider settings, bool enable) {
    try {
      settings.setEd2kProtocolAssociation(enable);
    } on ArgumentError {
      // rinf native library unavailable in the test VM.
    }
  }

  test('user toggle-off sticks even when the live query still reports ed2k '
      'as registered', () {
    final settings = SettingsProvider(enableFileAssoc: false);
    addTearDown(settings.dispose);

    userToggles(settings, true);
    settings.handleEd2kProtocolStatus(true);
    expect(settings.ed2kProtocolAssociated, isTrue);

    userToggles(settings, false);
    expect(settings.ed2kProtocolAssociated, isFalse);

    settings.handleEd2kProtocolStatus(true);
    expect(
      settings.ed2kProtocolAssociated,
      isFalse,
      reason: 'a user-requested OFF must not be clobbered by a live query '
          'that resolves back to FluxDown',
    );
  });

  test('persisted opt-out survives restart (config reload + startup query)',
      () {
    final settings = SettingsProvider(enableFileAssoc: false);
    addTearDown(settings.dispose);

    settings.applyLoadedConfig([
      ConfigEntry(key: 'ed2k_assoc_user_disabled', value: 'true'),
    ]);
    settings.handleEd2kProtocolStatus(true);
    expect(settings.ed2kProtocolAssociated, isFalse);
  });

  test('the two associations are independent switches', () {
    final settings = SettingsProvider(enableFileAssoc: false);
    addTearDown(settings.dispose);

    try {
      settings.setFileAssociation(true);
    } on ArgumentError {
      // rinf native library unavailable in the test VM.
    }
    settings.handleFileAssociationStatus(true);
    settings.handleEd2kProtocolStatus(false);

    expect(settings.torrentAssociated, isTrue);
    expect(settings.ed2kProtocolAssociated, isFalse);
  });

  test('without an opt-out the live status remains authoritative', () {
    final settings = SettingsProvider(enableFileAssoc: false);
    addTearDown(settings.dispose);

    // Another client (eMule) owns ed2k:// → Rust reports false.
    settings.handleEd2kProtocolStatus(false);
    expect(settings.ed2kProtocolAssociated, isFalse);

    // User claims it; Rust confirms.
    userToggles(settings, true);
    settings.handleEd2kProtocolStatus(true);
    expect(settings.ed2kProtocolAssociated, isTrue);

    // The other client takes it back → reflect reality.
    settings.handleEd2kProtocolStatus(false);
    expect(settings.ed2kProtocolAssociated, isFalse);
  });
}
