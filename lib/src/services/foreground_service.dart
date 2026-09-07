import 'dart:async';
import 'dart:io';

import 'package:flutter_foreground_task/flutter_foreground_task.dart';

import '../i18n/translations.dart';
import '../models/download_controller.dart';
import '../models/download_task.dart';
import 'log_service.dart';

const _tag = 'ForegroundSvc';

/// TaskHandler 入口回调（必须是顶层 / 静态函数，并标注 vm:entry-point）。
///
/// FluxDown 的下载引擎（Rust via Rinf）运行在主 isolate 内，前台服务的唯一
/// 职责是**保活进程**——切换到其他应用时系统不杀进程，下载得以继续。因此
/// 后台 isolate 不承载任何下载逻辑，[_KeepAliveTaskHandler] 为空实现。
@pragma('vm:entry-point')
void foregroundServiceCallback() {
  FlutterForegroundTask.setTaskHandler(_KeepAliveTaskHandler());
}

/// 空 TaskHandler：仅用于满足插件在后台 isolate 的存活要求。
///
/// 所有通知内容更新由主 isolate 经 [ForegroundServiceManager.updateNotification]
/// 直接调用 `updateService` 完成，无需在此处理事件。
class _KeepAliveTaskHandler extends TaskHandler {
  @override
  Future<void> onStart(DateTime timestamp, TaskStarter starter) async {}

  @override
  void onRepeatEvent(DateTime timestamp) {}

  @override
  Future<void> onDestroy(DateTime timestamp, bool isTimeout) async {}
}

/// Android 前台服务管理器（移动端后台持续下载 + 任务栏常驻通知）。
///
/// ## 为什么需要
///
/// 下载引擎跑在 App 进程内。App 切到后台后，Android 8.0+ 的后台执行限制与
/// 12+ 的电池优化会冻结甚至杀死无前台服务的进程，导致下载中断。前台服务
/// （`foregroundServiceType=dataSync`）声明"应用正在进行数据同步"，系统据此
/// 保活进程，任务栏显示一条常驻通知。
///
/// ## 通知策略
///
/// 通知随下载状态动态更新（活跃任务数 + 全局速度），空闲时回落到静态文案。
/// 更新走主 isolate 的 `updateService`，随 [DownloadController] 变化节流刷新。
///
/// ## 非 Android 平台
///
/// iOS 无前台服务概念，桌面端也不需要此 Android 保活机制；所有公开入口直接
/// 返回，不请求通知权限、不启动后台服务。
///
/// ## 用法
///
/// ```dart
/// // 移动端 App 启动时（main.dart）
/// ForegroundServiceManager.initCommunicationPort();
///
/// // 根组件 initState 内
/// await ForegroundServiceManager.instance.start(controller, s);
///
/// // 根组件 dispose 内
/// await ForegroundServiceManager.instance.stop();
/// ```
class ForegroundServiceManager {
  ForegroundServiceManager._();

  static final ForegroundServiceManager instance = ForegroundServiceManager._();

  static const int _serviceId = 4271; // 固定 service id，避免多实例冲突

  DownloadController? _controller;
  S? _strings;
  bool _started = false;
  int _lifecycleGeneration = 0;
  Future<void> _lifecycleOperation = Future<void>.value();

  /// 上次写入通知的文案，去重避免无谓的跨进程调用。
  String _lastTitle = '';
  String _lastText = '';

  /// 节流：最快 1s 刷新一次通知，避免高频进度回调打爆 IPC。
  DateTime _lastUpdate = DateTime.fromMillisecondsSinceEpoch(0);
  static const Duration _minInterval = Duration(seconds: 1);

  /// 初始化 TaskHandler ↔ 主 isolate 通信端口。必须在 `runApp` 之前调用。
  static void initCommunicationPort() {
    if (!_isAndroid) return;
    FlutterForegroundTask.initCommunicationPort();
  }

  static bool get _isAndroid => Platform.isAndroid;

  /// 请求通知权限与电池优化豁免，初始化并启动 Android 前台服务。
  ///
  /// [controller] 用于订阅下载状态以动态刷新通知；[s] 提供本地化文案。
  Future<void> start(DownloadController controller, S s) {
    if (!_isAndroid) return Future<void>.value();
    final generation = ++_lifecycleGeneration;
    _strings = s;
    _lifecycleOperation = _lifecycleOperation.then(
      (_) => _start(generation, controller),
    );
    return _lifecycleOperation;
  }

  Future<void> _start(int generation, DownloadController controller) async {
    if (generation != _lifecycleGeneration) return;
    _controller?.removeListener(_onControllerChanged);
    _controller = controller;

    try {
      await _requestPermissions();
      if (generation != _lifecycleGeneration) return;
      final currentStrings = _strings;
      if (currentStrings == null) return;
      _init(currentStrings);
      await _startService(controller, currentStrings);
      if (generation != _lifecycleGeneration) return;
      final latestStrings = _strings;
      if (latestStrings == null) return;
      _init(latestStrings);
      _started = true;
      controller.addListener(_onControllerChanged);
      _refreshNotification(force: true);
      logInfo(_tag, 'foreground service started');
    } catch (e, st) {
      if (generation == _lifecycleGeneration) {
        _controller = null;
        _strings = null;
        _started = false;
      }
      logError(_tag, 'failed to start foreground service', e, st);
    }
  }

  /// 停止服务并解除监听。递增 generation 可取消仍在等待授权的 start。
  Future<void> stop() {
    if (!_isAndroid) return Future<void>.value();
    _lifecycleGeneration += 1;
    _controller?.removeListener(_onControllerChanged);
    _controller = null;
    _strings = null;
    _started = false;
    _lifecycleOperation = _lifecycleOperation.then((_) => _stopService());
    return _lifecycleOperation;
  }

  /// 语言切换后刷新已缓存的本地化文案与通知频道配置。
  void updateStrings(S s) {
    if (!_isAndroid) return;
    _strings = s;
    if (_started) {
      _init(s);
      _refreshNotification(force: true);
    }
  }

  Future<void> _requestPermissions() async {
    final NotificationPermission perm =
        await FlutterForegroundTask.checkNotificationPermission();
    if (perm != NotificationPermission.granted) {
      await FlutterForegroundTask.requestNotificationPermission();
    }

    // 电池优化豁免显著提升后台存活率；用户可拒绝，失败不阻断启动。
    final bool ignoring =
        await FlutterForegroundTask.isIgnoringBatteryOptimizations;
    if (!ignoring) {
      await FlutterForegroundTask.requestIgnoreBatteryOptimization();
    }
  }

  void _init(S s) {
    FlutterForegroundTask.init(
      androidNotificationOptions: AndroidNotificationOptions(
        channelId: 'fluxdown_download_service',
        channelName: s.fgServiceChannelName,
        channelDescription: s.fgServiceChannelDesc,
        channelImportance: NotificationChannelImportance.LOW,
        priority: NotificationPriority.LOW,
        onlyAlertOnce: true,
        showWhen: false,
      ),
      iosNotificationOptions: const IOSNotificationOptions(
        showNotification: false,
        playSound: false,
      ),
      foregroundTaskOptions: ForegroundTaskOptions(
        eventAction: ForegroundTaskEventAction.nothing(),
        autoRunOnBoot: false,
        autoRunOnMyPackageReplaced: true,
        allowWakeLock: true,
        allowWifiLock: true,
      ),
    );
  }

  Future<void> _startService(DownloadController controller, S s) async {
    final (title, text) = _composeContent(controller, s);
    _lastTitle = title;
    _lastText = text;
    if (await FlutterForegroundTask.isRunningService) {
      await FlutterForegroundTask.restartService();
    } else {
      await FlutterForegroundTask.startService(
        serviceId: _serviceId,
        notificationTitle: title,
        notificationText: text,
        callback: foregroundServiceCallback,
      );
    }
  }

  Future<void> _stopService() async {
    try {
      await FlutterForegroundTask.stopService();
    } catch (e, st) {
      logError(_tag, 'failed to stop foreground service', e, st);
    }
  }

  void _onControllerChanged() => _refreshNotification();

  void _refreshNotification({bool force = false}) {
    if (!_started) return;
    final controller = _controller;
    final strings = _strings;
    if (controller == null || strings == null) return;
    final (title, text) = _composeContent(controller, strings);
    // 活跃 → 空闲 的状态翻转必须绕过节流立即刷新：完成帧常落在 1 秒节流
    // 窗口内，若不强制，归零后的空闲文案会被吞掉且无后续事件兜底。
    final becameIdle = controller.downloadingCount == 0 && title != _lastTitle;
    final now = DateTime.now();
    if (!force && !becameIdle && now.difference(_lastUpdate) < _minInterval) {
      return;
    }
    _lastUpdate = now;

    if (!force && title == _lastTitle && text == _lastText) return;
    _lastTitle = title;
    _lastText = text;

    unawaited(_pushNotification(title, text));
  }

  Future<void> _pushNotification(String title, String text) async {
    try {
      await FlutterForegroundTask.updateService(
        notificationTitle: title,
        notificationText: text,
      );
    } catch (e, st) {
      logError(_tag, 'updateService failed', e, st);
    }
  }

  /// 组装通知标题/正文：与主界面左上角保持一致，仅以真正下载中的任务数
  /// 判定活跃；无下载任务即回落到空闲静态文案。
  (String, String) _composeContent(DownloadController controller, S strings) {
    final downloading = controller.downloadingCount;
    if (downloading > 0) {
      final speed =
          '${DownloadTask.formatBytes(controller.totalDownloadSpeed)}/s';
      return (
        strings.fgServiceActiveTitle(downloading),
        strings.fgServiceActiveText(speed),
      );
    }
    return (strings.fgServiceIdleTitle, strings.fgServiceIdleText);
  }
}
