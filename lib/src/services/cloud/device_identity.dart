// 设备身份 —— 持久 deviceId/deviceName/devicePlatform 三元组 + appVersion（契约
// v1.1 新增），对应契约「请求中的设备信息」。所有发令牌的 FluxCloud 接口都要带上
// 这些字段。
//
// deviceId 首次调用时随机生成（UUID v4）并立即落盘 kv_store，之后进程内走内存
// 缓存、跨进程走 kv_store，正常使用下保持不变；只有清空 kv 存储、便携版与安装版
// 互换、或更换数据目录时会重新生成（对服务端等同于一台新设备）。生成方式同
// analytics_service.dart 的匿名设备 ID，避免为此单引入 uuid 包依赖。
// deviceName 默认取本机名（桌面）/设备型号（移动），用户可在「账户」设置中改名；
// 探测失败时留空交服务端按 devicePlatform 兜底（见契约注释）。
// appVersion 直接复用 update_service.dart 的构建期版本号（--dart-define
// APP_VERSION 注入），不为此单独引入依赖或新的版本获取途径。

import 'dart:async';
import 'dart:io';
import 'dart:math';

import 'package:device_info_plus/device_info_plus.dart';

import '../kv_store.dart';
import '../log_service.dart';
import '../update_service.dart';
import 'cloud_models.dart';

const _tag = 'DeviceIdentity';
const _kDeviceIdKey = 'cloud_device_id';
const _kDeviceNameKey = 'cloud_device_name';

class DeviceIdentity {
  DeviceIdentity._();

  /// 进程内缓存：首次解析后全程复用，保证并发/重入调用不会各自生成一个 UUID。
  static String? _cachedDeviceId;

  /// 持久客户端设备 ID（UUID v4）；首次访问生成并立即落盘，
  /// 是服务端 devices 表识别"同一设备"的唯一依据。
  static String deviceId() {
    final cached = _cachedDeviceId;
    if (cached != null) return cached;
    final existing = KvStore.instance.getString(_kDeviceIdKey);
    if (existing != null && existing.isNotEmpty) {
      return _cachedDeviceId = existing;
    }
    final id = _uuidV4();
    _cachedDeviceId = id;
    unawaited(_persistDeviceId(id));
    return id;
  }

  /// 落盘首次生成的 deviceId。丢失它的代价是云端凭空多出一台同名幽灵设备、
  /// 白占套餐设备额度，所以这里不接受便携后端 400ms 防抖窗口内的进程退出：
  /// setString 同步更新缓存并登记防抖，紧跟的 flush 取消防抖并同步写文件——
  /// 两个调用都在第一个 await 之前同步执行完，[deviceId] 返回时文件已落盘。
  /// 安装后端 flush 为 no-op，写入由 SharedPreferences 自身完成。
  static Future<void> _persistDeviceId(String id) async {
    try {
      final written = KvStore.instance.setString(_kDeviceIdKey, id);
      final flushed = KvStore.instance.flush();
      await written;
      await flushed;
    } catch (e, stack) {
      logError(_tag, 'failed to persist device id', e, stack);
    }
  }

  /// 纯函数：Dart `Platform.operatingSystem` 取值 → 契约 devicePlatform 枚举字符串
  /// （windows|macos|linux|android|ios），未覆盖的平台（如 web/fuchsia）返回 null。
  /// 抽成纯函数便于单测，不依赖运行时 Platform 判断。
  static String? platformFor(String operatingSystem) => switch (operatingSystem) {
    'windows' => 'windows',
    'macos' => 'macos',
    'linux' => 'linux',
    'android' => 'android',
    'ios' => 'ios',
    _ => null,
  };

  /// 当前设备的 devicePlatform 取值。
  static String? platform() => platformFor(Platform.operatingSystem);

  /// 请求携带用的客户端版本号（契约 v1.1 appVersion，可空）：直接复用
  /// update_service.dart 的构建期版本号，本地开发未注入 --dart-define 时为
  /// 'dev'（同「当前版本」展示口径，不额外区分）。
  static String? appVersion() {
    final v = UpdateService.instance.currentVersion;
    return v.isEmpty ? null : v;
  }

  /// 用户自定义设备名；未设置时返回 null。
  static String? customName() {
    final v = KvStore.instance.getString(_kDeviceNameKey);
    return (v != null && v.trim().isNotEmpty) ? v.trim() : null;
  }

  /// 持久化用户自定义设备名。调用方需先校验 1-64 字符（同服务端 PATCH /devices 规则）。
  static Future<void> setCustomName(String name) =>
      KvStore.instance.setString(_kDeviceNameKey, name.trim());

  /// 探测本机默认设备名：桌面取主机名，移动端取机型；探测失败返回空串，
  /// 由服务端按 devicePlatform 生成本地化默认名（见契约）。
  static Future<String> defaultName() async {
    try {
      if (Platform.isWindows || Platform.isMacOS || Platform.isLinux) {
        return Platform.localHostname.trim();
      }
      if (Platform.isAndroid) {
        final info = await DeviceInfoPlugin().androidInfo;
        final brand = info.brand.trim();
        final model = info.model.trim();
        if (brand.isNotEmpty && !model.toLowerCase().contains(brand.toLowerCase())) {
          return '$brand $model'.trim();
        }
        return model;
      }
      if (Platform.isIOS) {
        final info = await DeviceInfoPlugin().iosInfo;
        return iosDisplayName(machine: info.utsname.machine, name: info.name);
      }
    } catch (e, stack) {
      logError(_tag, 'defaultName probe failed', e, stack);
    }
    return '';
  }

  /// 纯函数：iOS 设备名优先取 utsname.machine（如 "iPhone15,2"，机型代号），
  /// 探测不到则回退用户设置的设备昵称（info.name，如 "老王的 iPhone"）。
  static String iosDisplayName({required String machine, required String name}) {
    final m = machine.trim();
    if (m.isNotEmpty) return m;
    return name.trim();
  }

  /// 请求携带用的设备名：优先用户自定义，否则探测默认名，探测也失败则返回 null
  /// （由服务端按 devicePlatform 兜底，见契约）。
  static Future<String?> resolvedName() async {
    final custom = customName();
    if (custom != null) return custom;
    final probed = await defaultName();
    return probed.isEmpty ? null : probed;
  }

  static String _uuidV4() {
    final rng = Random.secure();
    final bytes = List<int>.generate(16, (_) => rng.nextInt(256));
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant 10
    final h = bytes.map((b) => b.toRadixString(16).padLeft(2, '0')).join();
    return '${h.substring(0, 8)}-${h.substring(8, 12)}-'
        '${h.substring(12, 16)}-${h.substring(16, 20)}-${h.substring(20)}';
  }
}

/// 同名设备可区分标签：[device] 的名称在 [all] 中重复出现时追加 deviceId 前 6 位
/// 短码（形如 `DESKTOP-RSQ4S1D · c0db03`）。服务端不禁止重名，同型号手机或同一
/// 批装机的办公机在列表里长得一模一样，用户无从判断该把任务下发给哪台；名称唯一
/// 时保持原样，不用短码污染绝大多数场景。名称为空时直接用短码兜底。
///
/// 硬约定：[all] 必须传入含本机在内的全量设备列表——本机与某台远端同名也要能
/// 被短码区分，否则设置页/侧栏/新建下载三处入口对同一台远端设备算出的重名结果
/// 会不一致，导致同一设备在不同页面显示不同的名字。
String deviceLabel(CloudDevice device, List<CloudDevice> all) {
  final short = _deviceShortCode(device.deviceId);
  final name = device.name.trim();
  if (name.isEmpty) return short;
  var seen = 0;
  for (final other in all) {
    if (other.name.trim() != name) continue;
    seen++;
    if (seen > 1) return '$name · $short';
  }
  return name;
}

String _deviceShortCode(String deviceId) =>
    deviceId.length <= 6 ? deviceId : deviceId.substring(0, 6);
