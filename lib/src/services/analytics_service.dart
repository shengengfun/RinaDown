import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:math';

import '../i18n/locale_provider.dart';
import '../models/settings_provider.dart';
import 'kv_store.dart';
import 'log_service.dart';

const _tag = 'Analytics';

/// Application version injected at build time (same define as UpdateService).
const _appVersion = String.fromEnvironment('APP_VERSION', defaultValue: 'dev');

/// Analytics ingest App-Key injected at build time. Empty = analytics disabled
/// entirely (dev builds / forks without a key).
const _appKey = String.fromEnvironment('ANALYTICS_APP_KEY', defaultValue: '');

/// Analytics ingest endpoint.
const _endpoint =
    'https://ops.zerx.dev/api/zerx.v1.AnalyticsIngestService/TrackEvents';

/// KvStore key: persistent per-machine anonymous device id (UUID v4),
/// used as the wire `sessionId`. 一机一个：升级/覆盖安装保留；卸载器会删除
/// KvStore 落盘文件（installer/windows/setup.iss [UninstallDelete]），
/// 因此「卸载后重装」会生成新 ID 并被统计为新安装。
const _kDeviceId = 'analytics_device_id';

/// KvStore key: whether the one-time install event has been reported.
const _kInstallReported = 'analytics_install_reported';

/// KvStore key: last local date (yyyy-MM-dd) an `app_active` event was sent.
const _kLastActiveDay = 'analytics_last_active_day';

/// KvStore key: whether an external integration request (browser extension /
/// takeover / RPC) has ever reached this install. Reported as the
/// `extensionConnected` prop — a boolean only, no request content.
const _kExtConnected = 'analytics_ext_connected';

/// 匿名统计服务。
///
/// 隐私边界（硬性约束，改动前必读）：
/// - 只发送两类事件：`app_installed`（首装一次性）与 `app_active`（每日一次）。
/// - **禁止**采集任何与下载任务相关的信息（URL、文件名、大小、协议、速度等）。
/// - `sessionId` 是本机持久匿名设备 ID（服务端按 DISTINCT session_id 去重，
///   支撑新装/日活的精确统计）；除此之外无任何账号/硬件指纹信息。
/// - `app_installed` 不受开关控制（首装时用户尚无机会修改设置，保证安装量可统计）；
///   `app_active` 受设置页「匿名使用统计」开关控制，关闭后不再发送。
/// - `props` 只允许应用级配置维度（开机自启/更新渠道/界面语言/扩展是否连接过）。
class AnalyticsService {
  AnalyticsService._();

  static final AnalyticsService instance = AnalyticsService._();

  bool _started = false;

  /// 外部集成（浏览器扩展/接管/RPC）首次到达时打标，供 `extensionConnected`
  /// 维度统计扩展渗透率。只记 bool，不含任何请求内容。
  static void markExtensionConnected() {
    if (KvStore.instance.getBool(_kExtConnected) ?? false) return;
    unawaited(KvStore.instance.setBool(_kExtConnected, true));
  }

  /// 在配置加载完成后上报启动相关事件。可在 `requestConfig()` 后立即调用；
  /// 内部会等待 [SettingsProvider.loaded] 为 true 再评估开关。
  void init(SettingsProvider settings) {
    if (_started) return;
    _started = true;

    if (_appKey.isEmpty) {
      logInfo(_tag, 'no app key configured, analytics disabled');
      return;
    }

    if (settings.loaded) {
      unawaited(_reportStartup(settings));
      return;
    }
    void onLoaded() {
      if (!settings.loaded) return;
      settings.removeListener(onLoaded);
      unawaited(_reportStartup(settings));
    }

    settings.addListener(onLoaded);
  }

  Future<void> _reportStartup(SettingsProvider settings) async {
    // 首装事件：不受开关控制，成功后永久标记，失败则下次启动重试。
    // 设备 ID 与该标记同生命周期（同一 KvStore 文件），卸载删除后两者
    // 一起消失 → 重装 = 新 ID + 重发首装事件；服务端再按 ID 去重兜底。
    if (!(KvStore.instance.getBool(_kInstallReported) ?? false)) {
      final ok = await _track('app_installed', settings);
      if (ok) await KvStore.instance.setBool(_kInstallReported, true);
    }

    // 每日活跃事件：受开关控制，本地自然日去重。
    if (!settings.analyticsEnabled) {
      logInfo(_tag, 'analytics disabled by user, skip app_active');
      return;
    }
    final today = _localDay(DateTime.now());
    if (KvStore.instance.getString(_kLastActiveDay) == today) return;
    final ok = await _track('app_active', settings);
    if (ok) await KvStore.instance.setString(_kLastActiveDay, today);
  }

  /// 持久设备 ID：首次访问时生成并落盘。
  String _deviceId() {
    final existing = KvStore.instance.getString(_kDeviceId);
    if (existing != null && existing.isNotEmpty) return existing;
    final id = _uuidV4();
    unawaited(KvStore.instance.setString(_kDeviceId, id));
    return id;
  }

  /// 发送单个事件。只携带系统级匿名属性与应用配置维度，恒不抛出。
  Future<bool> _track(String eventName, SettingsProvider settings) async {
    final payload = jsonEncode({
      'events': [
        {
          'sessionId': _deviceId(),
          'eventName': eventName,
          'systemProps': {
            'osName': _osName(),
            'osVersion': Platform.operatingSystemVersion,
            'appVersion': _appVersion,
            'locale': Platform.localeName,
            // dev 构建走 ops 的 debug 分流，不污染正式统计（仪表盘 debug 开关查看）
            'isDebug': _appVersion == 'dev',
          },
          'props': {
            'edition': 'desktop',
            'autoStartup': settings.autoStartup,
            'updateChannel': settings.updateChannel,
            'appLanguage': currentLocale,
            'extensionConnected':
                KvStore.instance.getBool(_kExtConnected) ?? false,
          },
        },
      ],
    });

    final client = HttpClient()
      ..connectionTimeout = const Duration(seconds: 10);
    try {
      final request = await client
          .postUrl(Uri.parse(_endpoint))
          .timeout(const Duration(seconds: 15));
      request.headers.set('Content-Type', 'application/json');
      request.headers.set('App-Key', _appKey);
      final bytes = utf8.encode(payload);
      request.contentLength = bytes.length;
      request.add(bytes);
      final response = await request.close().timeout(
        const Duration(seconds: 15),
      );
      await response.drain<void>();
      final ok = response.statusCode >= 200 && response.statusCode < 300;
      if (!ok) {
        logInfo(_tag, '$eventName rejected: HTTP ${response.statusCode}');
      } else {
        logInfo(_tag, '$eventName sent');
      }
      return ok;
    } catch (e) {
      // 网络失败静默降级 —— 统计缺失优于打扰用户。
      logInfo(_tag, '$eventName failed: $e');
      return false;
    } finally {
      client.close();
    }
  }

  static String _osName() {
    if (Platform.isWindows) return 'Windows';
    if (Platform.isMacOS) return 'macOS';
    if (Platform.isLinux) return 'Linux';
    if (Platform.isAndroid) return 'Android';
    if (Platform.isIOS) return 'iOS';
    return Platform.operatingSystem;
  }

  static String _localDay(DateTime t) =>
      '${t.year.toString().padLeft(4, '0')}-'
      '${t.month.toString().padLeft(2, '0')}-'
      '${t.day.toString().padLeft(2, '0')}';

  static String _uuidV4() {
    final rng = Random.secure();
    final bytes = List<int>.generate(16, (_) => rng.nextInt(256));
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant 10
    final h = bytes
        .map((b) => b.toRadixString(16).padLeft(2, '0'))
        .join();
    return '${h.substring(0, 8)}-${h.substring(8, 12)}-'
        '${h.substring(12, 16)}-${h.substring(16, 20)}-${h.substring(20)}';
  }
}
