// Tests for DeviceIdentity (lib/src/services/cloud/device_identity.dart) ——
// 只覆盖纯逻辑部分：Dart Platform.operatingSystem → 契约 devicePlatform 枚举
// 字符串的映射、"自定义设备名优先于探测默认名"的解析优先级、同名设备可区分标签，
// 以及首次生成的 deviceId 必须在 deviceId() 返回前落盘（丢了就是云端一台幽灵设备）。
// 平台探测（Platform.isXxx / device_info_plus）依赖真实运行时环境，不在此覆盖。

import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:flux_down/src/services/cloud/cloud_models.dart';
import 'package:flux_down/src/services/cloud/device_identity.dart';
import 'package:flux_down/src/services/kv_store.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  group('platformFor', () {
    test('maps contract-covered operating systems to their wire values', () {
      expect(DeviceIdentity.platformFor('windows'), 'windows');
      expect(DeviceIdentity.platformFor('macos'), 'macos');
      expect(DeviceIdentity.platformFor('linux'), 'linux');
      expect(DeviceIdentity.platformFor('android'), 'android');
      expect(DeviceIdentity.platformFor('ios'), 'ios');
    });

    test('returns null for platforms not covered by the contract', () {
      expect(DeviceIdentity.platformFor('fuchsia'), isNull);
      expect(DeviceIdentity.platformFor('web'), isNull);
      expect(DeviceIdentity.platformFor(''), isNull);
    });
  });

  group('iosDisplayName', () {
    test('prefers the utsname machine code when present', () {
      expect(
        DeviceIdentity.iosDisplayName(machine: 'iPhone15,2', name: "Zero's iPhone"),
        'iPhone15,2',
      );
    });

    test('falls back to the user-assigned device name when machine is empty', () {
      expect(
        DeviceIdentity.iosDisplayName(machine: '  ', name: "Zero's iPhone"),
        "Zero's iPhone",
      );
    });
  });

  group('resolvedName priority', () {
    late Directory dir;
    late File file;

    setUp(() {
      dir = Directory.systemTemp.createTempSync('device_identity_test_');
      file = File('${dir.path}/settings.json');
      KvStore.instance.debugReset();
      KvStore.instance.debugInitPortable(file);
    });

    tearDown(() {
      KvStore.instance.debugReset();
      dir.deleteSync(recursive: true);
    });

    test('a user-set custom name always wins over the probed default', () async {
      await DeviceIdentity.setCustomName('我的主机');
      expect(await DeviceIdentity.resolvedName(), '我的主机');
    });

    test('blank custom names are treated as unset', () async {
      await DeviceIdentity.setCustomName('   ');
      expect(DeviceIdentity.customName(), isNull);
    });
  });

  group('deviceId persistence', () {
    late Directory dir;
    late File file;

    setUp(() {
      dir = Directory.systemTemp.createTempSync('device_id_test_');
      file = File('${dir.path}/settings.json');
      KvStore.instance.debugReset();
      KvStore.instance.debugInitPortable(file);
    });

    tearDown(() {
      KvStore.instance.debugReset();
      dir.deleteSync(recursive: true);
    });

    test('the first generated id is on disk before the call returns', () {
      final id = DeviceIdentity.deviceId();
      expect(id, isNotEmpty);
      // 同步读盘：便携后端的 400ms 防抖已被 flush 取消，此刻断电也不会丢 ID。
      expect(file.readAsStringSync(), contains(id));
      // 进程内缓存：重复调用永远拿到同一个 ID，不会各自生成一个。
      expect(DeviceIdentity.deviceId(), id);
    });
  });

  group('deviceLabel', () {
    test('keeps a unique name clean', () {
      final all = [_dev('DESKTOP-A', 'c0db0311'), _dev('iPhone', 'ff10ab22')];
      expect(deviceLabel(all[0], all), 'DESKTOP-A');
    });

    test('appends a short code to every device sharing a name', () {
      final all = [
        _dev('DESKTOP-A', 'c0db0311'),
        _dev('DESKTOP-A', 'ff10ab22'),
        _dev('iPhone', '77deadbe'),
      ];
      expect(deviceLabel(all[0], all), 'DESKTOP-A · c0db03');
      expect(deviceLabel(all[1], all), 'DESKTOP-A · ff10ab');
      expect(deviceLabel(all[2], all), 'iPhone');
    });

    test('falls back to the short code when the server sent no name', () {
      final all = [_dev('', 'c0db0311')];
      expect(deviceLabel(all[0], all), 'c0db03');
    });

    test('uses the whole id when it is shorter than the short code', () {
      final all = [_dev('', 'abc')];
      expect(deviceLabel(all[0], all), 'abc');
    });
  });
}

CloudDevice _dev(String name, String deviceId) => CloudDevice(
  id: 'row-$deviceId',
  deviceId: deviceId,
  name: name,
  createdAt: '',
  lastSeenAt: '',
);