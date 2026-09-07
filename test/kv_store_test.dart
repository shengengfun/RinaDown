// Tests for KvStore (lib/src/services/kv_store.dart) — the portable-mode
// JSON key-value facade that replaces SharedPreferences for the portable
// build (P0 fix: portable builds must never touch %APPDATA%).
//
// Only the portable backend is testable here: the installed-mode backend
// selection happens via exe-path probing inside init()/isPortableMode(),
// which cannot be injected. All tests drive the portable JSON-file backend
// through the @visibleForTesting seams `debugInitPortable`/`debugReset`.
//
// Each test starts with debugReset() for isolation, and setUp/tearDown
// manage a fresh temp directory holding the portable settings file.

import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:flux_down/src/services/kv_store.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  late Directory dir;
  late File file;

  setUp(() {
    dir = Directory.systemTemp.createTempSync('kv_store_test_');
    file = File('${dir.path}/settings.json');
    KvStore.instance.debugReset();
  });

  tearDown(() {
    KvStore.instance.debugReset();
    dir.deleteSync(recursive: true);
  });

  group('sync round trip', () {
    test('write then read back synchronously', () async {
      KvStore.instance.debugInitPortable(file);

      await KvStore.instance.setString('name', 'flux');
      await KvStore.instance.setBool('enabled', true);
      await KvStore.instance.setDouble('ratio', 3.5);

      expect(KvStore.instance.getString('name'), 'flux');
      expect(KvStore.instance.getBool('enabled'), true);
      expect(KvStore.instance.getDouble('ratio'), 3.5);
    });
  });

  group('flush + reload', () {
    test('flush persists to disk as valid JSON and reload restores values',
        () async {
      KvStore.instance.debugInitPortable(file);

      await KvStore.instance.setString('name', 'flux');
      await KvStore.instance.setBool('enabled', true);
      await KvStore.instance.setDouble('ratio', 3.5);

      await KvStore.instance.flush();

      expect(file.existsSync(), isTrue);
      final decoded = jsonDecode(file.readAsStringSync());
      expect(decoded, isA<Map<String, dynamic>>());
      final map = decoded as Map<String, dynamic>;
      expect(map['name'], 'flux');
      expect(map['enabled'], true);
      expect(map['ratio'], 3.5);

      // Simulate unplugging the drive / restarting the app: reset in-memory
      // state and re-init against the same file.
      KvStore.instance.debugReset();
      KvStore.instance.debugInitPortable(file);

      expect(KvStore.instance.getString('name'), 'flux');
      expect(KvStore.instance.getBool('enabled'), true);
      expect(KvStore.instance.getDouble('ratio'), 3.5);
    });
  });

  group('remove', () {
    test('remove clears the key and the removal persists across reload',
        () async {
      KvStore.instance.debugInitPortable(file);

      await KvStore.instance.setString('token', 'secret');
      expect(KvStore.instance.getString('token'), 'secret');

      await KvStore.instance.remove('token');
      expect(KvStore.instance.getString('token'), isNull);

      await KvStore.instance.flush();

      KvStore.instance.debugReset();
      KvStore.instance.debugInitPortable(file);

      expect(KvStore.instance.getString('token'), isNull);
    });
  });

  group('type isolation', () {
    test('a string value is invisible to getBool/getDouble', () async {
      KvStore.instance.debugInitPortable(file);

      await KvStore.instance.setString('key', 'value');

      expect(KvStore.instance.getBool('key'), isNull);
      expect(KvStore.instance.getDouble('key'), isNull);
      expect(KvStore.instance.getString('key'), 'value');
    });

    test('a double value is invisible to getString/getBool', () async {
      KvStore.instance.debugInitPortable(file);

      await KvStore.instance.setDouble('key', 1.25);

      expect(KvStore.instance.getString('key'), isNull);
      expect(KvStore.instance.getBool('key'), isNull);
      expect(KvStore.instance.getDouble('key'), 1.25);
    });

    test('a missing key returns null from every getter', () {
      KvStore.instance.debugInitPortable(file);

      expect(KvStore.instance.getString('missing'), isNull);
      expect(KvStore.instance.getBool('missing'), isNull);
      expect(KvStore.instance.getDouble('missing'), isNull);
    });
  });

  group('double precision', () {
    test('an integral double survives a flush+reload round trip as double',
        () async {
      KvStore.instance.debugInitPortable(file);

      await KvStore.instance.setDouble('width', 100.0);
      await KvStore.instance.flush();

      KvStore.instance.debugReset();
      KvStore.instance.debugInitPortable(file);

      final value = KvStore.instance.getDouble('width');
      expect(value, isA<double>());
      expect(value, 100.0);
    });
  });

  group('loading an existing file', () {
    test('debugInitPortable reads pre-existing JSON content on construction',
        () {
      file.writeAsStringSync(jsonEncode({'k': 'v'}));

      KvStore.instance.debugInitPortable(file);

      expect(KvStore.instance.getString('k'), 'v');
    });
  });

  group('malformed JSON tolerance', () {
    test('a corrupt file does not throw and yields an empty cache', () {
      file.writeAsStringSync('not json{{');

      expect(() => KvStore.instance.debugInitPortable(file), returnsNormally);

      expect(KvStore.instance.getString('k'), isNull);
      expect(KvStore.instance.getBool('k'), isNull);
      expect(KvStore.instance.getDouble('k'), isNull);
    });
  });
}
