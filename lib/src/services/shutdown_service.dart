import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';

import '../models/download_controller.dart';
import 'log_service.dart';

const _tag = 'ShutdownService';

/// 「任务完成后自动关机」跨平台服务。
///
/// 设计约束：
/// - 状态**纯内存持有，绝不持久化** —— 应用重启后自动关机恒为关闭状态；
/// - 仅当存在活跃任务（下载中/等待中/准备中/恢复中）时才允许开启；
/// - 全部活跃任务结束后进入倒计时（分钟数可自定义），倒计时结束执行关机；
/// - 倒计时期间有新任务变为活跃 → 自动取消倒计时、保持待命，
///   待任务再次全部完成后重新计时；
/// - 任意时刻可通过 [cancel] 取消（含倒计时中）。
///
/// 平台实现：
/// - Windows: `shutdown /s /f /t 0`（/f 强制关闭应用 —— 无人值守场景下
///   不能被前台应用的「阻止关机」对话框卡住）
/// - macOS: `osascript -e 'tell application "System Events" to shut down'`
///   （无需 sudo 的正规关机路径）
/// - Linux: `systemctl poweroff`，失败回退 `loginctl poweroff`
///   （均走 logind，桌面会话内无需 root）
class ShutdownService extends ChangeNotifier {
  ShutdownService._();

  static final ShutdownService instance = ShutdownService._();

  /// 状态来源（生产环境为 [DownloadController]）
  Listenable? _source;

  /// 活跃任务数读取器
  int Function()? _activeCount;

  /// 关机执行器 —— 测试可注入以避免真实关机；null = 平台默认实现
  @visibleForTesting
  Future<void> Function()? debugShutdownExecutor;

  /// 用户已开启「完成后关机」（待命状态）
  bool _armed = false;

  /// 完成后延迟关机的分钟数（0 = 立即关机）
  int _delayMinutes = 5;

  /// 倒计时剩余秒数；-1 = 未进入倒计时
  int _remainingSeconds = -1;

  Timer? _countdownTimer;

  /// 关机命令已发出（防重复触发）
  bool _shutdownIssued = false;

  // ---------------------------------------------------------------------------
  // 状态查询
  // ---------------------------------------------------------------------------

  /// 是否已开启（待命或倒计时中）
  bool get isArmed => _armed;

  /// 是否处于倒计时阶段
  bool get isCountingDown => _remainingSeconds >= 0;

  /// 倒计时剩余秒数（未倒计时返回 -1）
  int get remainingSeconds => _remainingSeconds;

  /// 完成后延迟分钟数
  int get delayMinutes => _delayMinutes;

  /// 当前是否允许开启 —— 必须有活跃任务
  bool get canArm => (_activeCount?.call() ?? 0) > 0;

  /// 倒计时剩余时间格式化为 mm:ss
  String get remainingText {
    final s = _remainingSeconds < 0 ? 0 : _remainingSeconds;
    final mm = (s ~/ 60).toString().padLeft(2, '0');
    final ss = (s % 60).toString().padLeft(2, '0');
    return '$mm:$ss';
  }

  // ---------------------------------------------------------------------------
  // 生命周期
  // ---------------------------------------------------------------------------

  /// 绑定下载控制器。重复调用先解绑旧实例。
  void bind(DownloadController controller) {
    bindSource(controller, () => controller.activeCount);
  }

  /// 绑定任意活跃任务数来源（供测试注入；生产路径经 [bind]）。
  @visibleForTesting
  void bindSource(Listenable source, int Function() activeCount) {
    unbind();
    _source = source;
    _activeCount = activeCount;
    source.addListener(_reevaluate);
  }

  /// 解绑并重置全部状态（应用退出/页面销毁时调用）。
  void unbind() {
    _source?.removeListener(_reevaluate);
    _source = null;
    _activeCount = null;
    _stopCountdown();
    _armed = false;
    _shutdownIssued = false;
  }

  // ---------------------------------------------------------------------------
  // 用户操作
  // ---------------------------------------------------------------------------

  /// 开启「完成后关机」。无活跃任务时拒绝（返回 false）。
  bool arm({int? minutes}) {
    if (!canArm) return false;
    if (minutes != null) _delayMinutes = _clampMinutes(minutes);
    _armed = true;
    _shutdownIssued = false;
    logInfo(_tag, 'armed: shutdown ${_delayMinutes}min after downloads done');
    notifyListeners();
    // 防御：极端情况下 arm 与任务结束同帧发生
    _reevaluate();
    return true;
  }

  /// 取消自动关机（含倒计时中）。
  void cancel() {
    if (!_armed && !isCountingDown) return;
    _armed = false;
    _stopCountdown();
    logInfo(_tag, 'auto-shutdown cancelled by user');
    notifyListeners();
  }

  /// 更新延迟分钟数（0 = 完成后立即关机）。若正处于倒计时，按新时长重新计时
  /// —— 倒计时中设为 0 会立即执行关机。
  void setDelayMinutes(int minutes) {
    final m = _clampMinutes(minutes);
    if (m == _delayMinutes) return;
    _delayMinutes = m;
    if (isCountingDown) {
      _startCountdown();
    } else {
      notifyListeners();
    }
  }

  int _clampMinutes(int m) => m < 0 ? 0 : (m > 24 * 60 ? 24 * 60 : m);

  // ---------------------------------------------------------------------------
  // 状态机
  // ---------------------------------------------------------------------------

  void _reevaluate() {
    if (!_armed || _shutdownIssued) return;
    final active = _activeCount?.call() ?? 0;
    if (active > 0) {
      // 倒计时中有新任务变为活跃 → 取消倒计时，保持待命
      if (isCountingDown) {
        _stopCountdown();
        logInfo(_tag, 'countdown cancelled: new active task detected');
        notifyListeners();
      }
      return;
    }
    // 活跃任务归零 → 进入倒计时（0 分钟 = 立即关机）
    if (!isCountingDown) {
      _startCountdown();
      logInfo(_tag, 'all downloads done, shutdown in ${_delayMinutes}min');
    }
  }

  void _startCountdown() {
    _countdownTimer?.cancel();
    // 立即关机：不进入倒计时阶段
    if (_delayMinutes <= 0) {
      _remainingSeconds = -1;
      _issueShutdown();
      return;
    }
    _remainingSeconds = _delayMinutes * 60;
    _countdownTimer = Timer.periodic(const Duration(seconds: 1), (_) {
      _remainingSeconds--;
      if (_remainingSeconds <= 0) {
        _stopCountdown();
        _issueShutdown();
      }
      notifyListeners();
    });
    notifyListeners();
  }

  void _stopCountdown() {
    _countdownTimer?.cancel();
    _countdownTimer = null;
    _remainingSeconds = -1;
  }

  // ---------------------------------------------------------------------------
  // 平台关机
  // ---------------------------------------------------------------------------

  Future<void> _issueShutdown() async {
    if (_shutdownIssued) return;
    _shutdownIssued = true;
    _armed = false;
    notifyListeners();
    logInfo(_tag, 'issuing system shutdown...');
    final executor = debugShutdownExecutor;
    if (executor != null) {
      await executor();
      return;
    }
    try {
      if (Platform.isWindows) {
        await _run('shutdown', ['/s', '/f', '/t', '0']);
      } else if (Platform.isMacOS) {
        await _run('osascript', [
          '-e',
          'tell application "System Events" to shut down',
        ]);
      } else if (Platform.isLinux) {
        final ok = await _run('systemctl', ['poweroff']);
        if (!ok) await _run('loginctl', ['poweroff']);
      } else {
        logError(_tag, 'shutdown unsupported on this platform');
      }
    } catch (e, st) {
      logError(_tag, 'failed to issue shutdown', e, st);
    }
  }

  /// 执行关机命令，返回是否成功（exit code 0）。
  Future<bool> _run(String executable, List<String> args) async {
    try {
      final result = await Process.run(executable, args);
      if (result.exitCode != 0) {
        logError(
          _tag,
          '$executable exited ${result.exitCode}: ${result.stderr}',
        );
        return false;
      }
      logInfo(_tag, '$executable succeeded');
      return true;
    } on ProcessException catch (e, st) {
      logError(_tag, 'failed to spawn $executable', e, st);
      return false;
    }
  }
}
