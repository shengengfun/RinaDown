import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:launch_at_startup/launch_at_startup.dart';

import 'package:flux_down/src/bindings/bindings.dart';
import 'package:flux_down/src/models/settings_provider.dart';

/// The "Take over magnet links" toggle mirrors the ed2k opt-out mechanism
/// (see ed2k_assoc_toggle_test.dart), with one difference in defaults: the
/// scheme is auto-registered at startup by Rust unless the persisted opt-out
/// (`magnet_assoc_user_disabled`) is set or another client (qBittorrent, …)
/// already claims it. On the Dart side that means a user-requested OFF must
/// win over any live status Rust reports back, and the opt-out must survive
/// a restart so the startup auto-registration stays vetoed.
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

  // Calls SettingsProvider.setMagnetProtocolAssociation tolerating the
  // ArgumentError from `sendSignalToRust` (the Rust dylib is not loaded under
  // `flutter test`). All in-memory state mutations complete before the
  // fire-and-forget signal send, so the observable state is unaffected.
  void userToggles(SettingsProvider settings, bool enable) {
    try {
      settings.setMagnetProtocolAssociation(enable);
    } on ArgumentError {
      // rinf native library unavailable in the test VM.
    }
  }

  test('user toggle-off sticks even when the live query still reports magnet '
      'as registered', () {
    final settings = SettingsProvider(enableFileAssoc: false);
    addTearDown(settings.dispose);

    // Startup auto-registration succeeded → Rust reports registered.
    settings.handleMagnetProtocolStatus(true);
    expect(settings.magnetProtocolAssociated, isTrue);

    userToggles(settings, false);
    expect(settings.magnetProtocolAssociated, isFalse);

    settings.handleMagnetProtocolStatus(true);
    expect(
      settings.magnetProtocolAssociated,
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
      ConfigEntry(key: 'magnet_assoc_user_disabled', value: 'true'),
    ]);
    settings.handleMagnetProtocolStatus(true);
    expect(settings.magnetProtocolAssociated, isFalse);
  });

  test('magnet and ed2k associations are independent switches', () {
    final settings = SettingsProvider(enableFileAssoc: false);
    addTearDown(settings.dispose);

    settings.handleMagnetProtocolStatus(true);
    settings.handleEd2kProtocolStatus(false);

    expect(settings.magnetProtocolAssociated, isTrue);
    expect(settings.ed2kProtocolAssociated, isFalse);
  });

  test('without an opt-out the live status remains authoritative', () {
    final settings = SettingsProvider(enableFileAssoc: false);
    addTearDown(settings.dispose);

    // Another client (qBittorrent) owns magnet: → startup auto-registration
    // skipped (non-preemptive) → Rust reports false.
    settings.handleMagnetProtocolStatus(false);
    expect(settings.magnetProtocolAssociated, isFalse);

    // User takes it over explicitly; Rust confirms.
    userToggles(settings, true);
    settings.handleMagnetProtocolStatus(true);
    expect(settings.magnetProtocolAssociated, isTrue);

    // The other client takes it back → reflect reality.
    settings.handleMagnetProtocolStatus(false);
    expect(settings.magnetProtocolAssociated, isFalse);
  });
}
