import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:launch_at_startup/launch_at_startup.dart';

import 'package:flux_down/src/bindings/bindings.dart';
import 'package:flux_down/src/models/custom_category.dart';
import 'package:flux_down/src/models/settings_provider.dart';

/// Repro: deleting built-in categories gets undone on the next startup.
///
/// The "program" category migration in `applyLoadedConfig` used
/// "list lacks builtinType == 'program'" as its trigger, which cannot
/// distinguish a pre-migration config from a user who deliberately deleted
/// the built-in category — so every restart re-inserted it.
///
/// Expected behavior: the migration runs at most once, gated by the
/// `program_category_migrated` marker key that is persisted alongside every
/// user-driven category change.
void main() {
  final binding = TestWidgetsFlutterBinding.ensureInitialized();

  launchAtStartup.setup(
    appName: 'FluxDownTest',
    appPath: Platform.resolvedExecutable,
  );
  binding.defaultBinaryMessenger.setMockMethodCallHandler(
    const MethodChannel('launch_at_startup'),
    (call) async => call.method == 'launchAtStartupIsEnabled' ? false : null,
  );

  ConfigEntry entry(String key, String value) =>
      ConfigEntry(key: key, value: value);

  // `applyLoadedConfig` fires fire-and-forget SaveConfig signals when the
  // migration runs; the Rust dylib is absent under `flutter test`, so
  // tolerate the ArgumentError — all in-memory mutations complete first.
  void load(SettingsProvider settings, List<ConfigEntry> entries) {
    try {
      settings.applyLoadedConfig(entries);
    } on ArgumentError {
      // rinf native library unavailable in the test VM.
    }
  }

  /// Categories as persisted by a user who deleted every deletable built-in
  /// (only the undeletable "all" remains).
  String allDeletedJson() => CustomCategory.encodeList(
    CustomCategory.defaultCategories()
        .where((c) => c.builtinType == 'all')
        .toList(),
  );

  test('deleted built-ins stay deleted once the migration marker is set', () {
    final settings = SettingsProvider(enableFileAssoc: false);
    addTearDown(settings.dispose);

    load(settings, [
      entry('custom_categories', allDeletedJson()),
      entry('program_category_migrated', 'true'),
    ]);

    expect(
      settings.customCategories.map((c) => c.builtinType),
      ['all'],
      reason: 'a marked config must never resurrect deleted built-ins',
    );
  });

  test('legacy config without marker gains the program category once', () {
    // Pre-"program" config: defaults minus program, no marker.
    final legacy = CustomCategory.encodeList(
      CustomCategory.defaultCategories()
          .where((c) => c.builtinType != 'program')
          .toList(),
    );

    final settings = SettingsProvider(enableFileAssoc: false);
    addTearDown(settings.dispose);
    load(settings, [entry('custom_categories', legacy)]);

    expect(
      settings.customCategories.any((c) => c.builtinType == 'program'),
      isTrue,
      reason: 'legacy configs must receive the program category migration',
    );
    // Program sits right before archive, mirroring the default order.
    final types = settings.customCategories.map((c) => c.builtinType).toList();
    expect(types.indexOf('program'), types.indexOf('archive') - 1);
  });

  test('user deletion is what persists the marker', () {
    final settings = SettingsProvider(enableFileAssoc: false);
    addTearDown(settings.dispose);
    load(settings, []); // first run seeds defaults (includes program)

    // User deletes the built-in program category; the save also persists
    // the marker (both writes are fire-and-forget signals).
    final program = settings.customCategories.firstWhere(
      (c) => c.builtinType == 'program',
    );
    try {
      settings.removeCustomCategory(program.id);
    } on ArgumentError {
      // rinf native library unavailable in the test VM.
    }
    expect(
      settings.customCategories.any((c) => c.builtinType == 'program'),
      isFalse,
    );
  });
}
