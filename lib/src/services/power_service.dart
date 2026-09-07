import 'dart:async';
import 'dart:ffi';
import 'dart:io';

import '../models/download_controller.dart';
import '../models/download_task.dart';
import '../models/settings_provider.dart';
import 'log_service.dart';

const _tag = 'PowerService';

// =============================================================================
// Win32 SetThreadExecutionState 绑定（kernel32，零第三方依赖）
// kernel32.dll 属于 Windows KnownDLLs，恒定从 System32 加载，无劫持面。
// =============================================================================

/// ES_CONTINUOUS — 状态持续生效直到下一次调用清除
const int _esContinuous = 0x80000000;

/// ES_SYSTEM_REQUIRED — 阻止系统进入睡眠
const int _esSystemRequired = 0x00000001;

/// ES_DISPLAY_REQUIRED — 阻止显示器关闭（息屏被阻止后自动锁屏亦不会触发）
const int _esDisplayRequired = 0x00000002;

typedef _SetThreadExecutionStateNative = Uint32 Function(Uint32 esFlags);
typedef _SetThreadExecutionStateDart = int Function(int esFlags);

/// 下载期间阻止系统睡眠/息屏的跨平台服务。
///
/// - Windows: `SetThreadExecutionState`（线程级；Dart root isolate 固定跑在
///   同一 OS 线程上，acquire/release 天然同线程可抵消；进程退出自动清除）
/// - macOS: `/usr/bin/caffeinate -di -w <pid>` 子进程（宿主退出自动释放）
/// - Linux: `/usr/bin/systemd-inhibit --what=idle:sleep -- /bin/cat` 子进程
///   （stdin 管道随宿主死亡 EOF → cat 退出 → inhibitor 自动释放）
///
/// 通过 [bind] 监听 [DownloadController] 与 [SettingsProvider]：
/// 有活跃传输且设置开启时持锁，其余时刻释放。
///
/// 健壮性设计：
/// - `_wanted`（目标状态）与 `_applied`（平台调用已确认状态）分离，
///   平台调用失败不吞状态，后续信号自动触发重试；
/// - 平台调用统一最小间隔 1s、失败退避 15s，防止状态抖动或快速开关
///   设置导致 spawn/kill 循环；
/// - 持锁工具缺失（非 systemd 发行版等）→ 永久降级为 no-op，不影响下载；
/// - 释放做 3s 防抖，任务队列衔接间隙不闪烁电源状态。
class PowerService {
  PowerService._();

  static final PowerService instance = PowerService._();

  DownloadController? _controller;
  SettingsProvider? _settings;

  /// 目标状态：是否应当持有唤醒锁
  bool _wanted = false;

  /// 已确认状态：平台调用成功生效的状态
  bool _applied = false;

  /// 持锁工具不可用（如 caffeinate/systemd-inhibit 缺失）→ 永久 no-op
  bool _platformUnsupported = false;

  /// 下一次允许发起平台调用的时间（限流/失败退避）
  DateTime _nextAttemptAt = DateTime.fromMillisecondsSinceEpoch(0);

  /// 限流等待期间的补偿泵计时器
  Timer? _pumpTimer;

  /// 释放防抖计时器
  Timer? _releaseTimer;

  /// 平台调用串行化链，防止 acquire/release 交错
  Future<void> _serial = Future<void>.value();

  /// macOS/Linux 持锁子进程
  Process? _inhibitProcess;

  /// Windows kernel32 函数指针（惰性解析，非 Windows 平台恒为 null）
  _SetThreadExecutionStateDart? _setThreadExecutionState;

  static const Duration _releaseDebounce = Duration(seconds: 3);
  static const Duration _minOpInterval = Duration(seconds: 1);
  static const Duration _failureBackoff = Duration(seconds: 15);

  /// 唤醒锁是否已实际生效（平台调用确认后的状态）。
  /// 注意：持锁工具缺失导致永久降级（[_platformUnsupported]）后，
  /// 该值退化为「目标状态」——不代表系统级锁真实存在。
  bool get isHeld => _applied;

  /// 绑定下载控制器与设置。重复调用先解绑旧实例。
  void bind(DownloadController controller, SettingsProvider settings) {
    unbind();
    _controller = controller;
    _settings = settings;
    controller.addListener(_reevaluate);
    settings.addListener(_reevaluate);
    _reevaluate();
  }

  /// 解绑监听并请求释放锁（状态来源消失后不应继续持锁）。
  void unbind() {
    _controller?.removeListener(_reevaluate);
    _settings?.removeListener(_reevaluate);
    _controller = null;
    _settings = null;
    _releaseTimer?.cancel();
    _releaseTimer = null;
    _wanted = false;
    _pump();
  }

  /// 应用退出时调用：解绑并立即释放唤醒锁（绕过限流），等待状态收敛。
  Future<void> shutdown() async {
    unbind();
    // 最多两轮：第一轮 force 释放；若其内部失败又追加了限流重试 job，
    // 第二轮再 force 一次并等待，保证 await 返回时已收敛或已尽力。
    for (var i = 0; i < 2; i++) {
      _pumpTimer?.cancel();
      _pumpTimer = null;
      _pump(force: true);
      await _serial;
      if (_applied == _wanted || _platformUnsupported) break;
    }
  }

  /// 是否存在应当保持唤醒的任务。
  /// 仅统计正在传输的状态；排队中的 pending 不阻止睡眠。
  bool get _hasActiveTransfer {
    final c = _controller;
    if (c == null) return false;
    return c.tasks.any(
      (t) =>
          t.status == TaskStatus.downloading ||
          t.status == TaskStatus.preparing ||
          t.status == TaskStatus.resuming,
    );
  }

  void _reevaluate() {
    final enabled = _settings?.keepAwakeWhileDownloading ?? false;
    final wantHeld = enabled && _hasActiveTransfer;

    if (wantHeld) {
      _releaseTimer?.cancel();
      _releaseTimer = null;
      _setWanted(true);
      return;
    }

    // 设置被关闭或已解绑：立即释放
    if (!enabled) {
      _releaseTimer?.cancel();
      _releaseTimer = null;
      _setWanted(false);
      return;
    }

    // 设置开启但无活跃传输：防抖释放，避免任务衔接间隙反复切换
    if (_wanted && _releaseTimer == null) {
      _releaseTimer = Timer(_releaseDebounce, () {
        _releaseTimer = null;
        final en = _settings?.keepAwakeWhileDownloading ?? false;
        if (!en || !_hasActiveTransfer) _setWanted(false);
      });
    }
  }

  void _setWanted(bool wanted) {
    if (_wanted == wanted && _applied == wanted) return;
    _wanted = wanted;
    _pump();
  }

  /// 推动实际状态向目标状态收敛。所有平台调用经 [_serial] 串行化；
  /// 未到限流窗口时挂一次性计时器稍后重试，多次调用自动合并。
  void _pump({bool force = false}) {
    _serial = _serial.then((_) async {
      if (_applied == _wanted) {
        // 已收敛：顺手取消残留的重试计时器（快速切回原状态的场景）
        _pumpTimer?.cancel();
        _pumpTimer = null;
        return;
      }
      if (_platformUnsupported) {
        // 平台工具缺失：视作 no-op 生效，避免无意义重试
        _applied = _wanted;
        return;
      }

      if (!force) {
        final wait = _nextAttemptAt.difference(DateTime.now());
        if (wait > Duration.zero) {
          _pumpTimer ??= Timer(wait, () {
            _pumpTimer = null;
            _pump();
          });
          return;
        }
      }
      _pumpTimer?.cancel();
      _pumpTimer = null;

      final target = _wanted;
      var ok = false;
      try {
        ok = target ? await _platformAcquire() : await _platformRelease();
      } catch (e, st) {
        logError(_tag, 'power transition failed (target=$target)', e, st);
      }

      if (ok) {
        _applied = target;
        _nextAttemptAt = DateTime.now().add(_minOpInterval);
        logInfo(_tag, target ? 'wake lock acquired' : 'wake lock released');
      } else {
        _nextAttemptAt = DateTime.now().add(_failureBackoff);
        logError(
          _tag,
          'power transition unsuccessful (target=$target), '
          'retry after ${_failureBackoff.inSeconds}s',
        );
      }
      // 执行期间目标可能又变了，或本次失败需要重试 → 再泵一次（受限流约束）
      if (_applied != _wanted) _pump();
    });
  }

  // ---------------------------------------------------------------------------
  // 平台实现（返回 true = 调用成功生效）
  // ---------------------------------------------------------------------------

  Future<bool> _platformAcquire() async {
    if (Platform.isWindows) {
      return _setWindowsState(
        _esContinuous | _esSystemRequired | _esDisplayRequired,
      );
    }
    if (Platform.isMacOS) {
      // -d 阻止显示器睡眠，-i 阻止系统 idle 睡眠，-w pid 宿主退出自动释放
      return _spawnInhibitor('/usr/bin/caffeinate', ['-di', '-w', '$pid']);
    }
    if (Platform.isLinux) {
      return _spawnInhibitor('/usr/bin/systemd-inhibit', [
        '--what=idle:sleep',
        '--who=FluxDown',
        '--why=Downloading files',
        '--mode=block',
        '--',
        '/bin/cat',
      ]);
    }
    return true; // 其他平台 no-op
  }

  Future<bool> _platformRelease() async {
    if (Platform.isWindows) {
      return _setWindowsState(_esContinuous);
    }
    final p = _inhibitProcess;
    _inhibitProcess = null;
    if (p != null) {
      // 先温和关闭 stdin（cat 收到 EOF 自然退出），再兜底 kill
      try {
        await p.stdin.close();
      } catch (_) {}
      p.kill(ProcessSignal.sigterm);
      // 回收退出码，避免僵尸进程；1s 超时兜底 SIGKILL
      try {
        await p.exitCode.timeout(const Duration(seconds: 1));
      } on TimeoutException {
        p.kill(ProcessSignal.sigkill);
      }
    }
    return true;
  }

  bool _setWindowsState(int flags) {
    final fn = _resolveWin32();
    if (fn == null) return false;
    // MSDN：成功返回「上一执行状态」，失败返回 0。但该线程首次调用时
    // 上一状态本身合法为 0，与失败态不可区分（Chromium 亦完全忽略返回值）。
    // 故不据此判失败，仅留日志辅助排障。
    final prev = fn(flags);
    if (prev == 0) {
      logInfo(_tag, 'SetThreadExecutionState returned 0 (first call or error)');
    }
    return true;
  }

  _SetThreadExecutionStateDart? _resolveWin32() {
    if (_setThreadExecutionState != null) return _setThreadExecutionState;
    try {
      final kernel32 = DynamicLibrary.open('kernel32.dll');
      _setThreadExecutionState = kernel32.lookupFunction<
        _SetThreadExecutionStateNative,
        _SetThreadExecutionStateDart
      >('SetThreadExecutionState');
    } catch (e, st) {
      logError(_tag, 'failed to resolve SetThreadExecutionState', e, st);
      _platformUnsupported = true;
    }
    return _setThreadExecutionState;
  }

  Future<bool> _spawnInhibitor(String executable, List<String> args) async {
    // 已有存活的持锁进程则视作成功（防御性；正常路径 release 先行）
    if (_inhibitProcess != null) return true;
    try {
      final p = await Process.start(executable, args);
      _inhibitProcess = p;
      // 排空输出防止管道缓冲填满；进程意外退出时清理引用
      p.stdout.drain<void>().ignore();
      p.stderr.drain<void>().ignore();
      p.exitCode.then((code) {
        if (identical(_inhibitProcess, p)) {
          _inhibitProcess = null;
          if (_applied) {
            logError(
              _tag,
              'inhibitor process exited unexpectedly, code=$code',
            );
            _applied = false; // 实际锁已丢失，下次 _reevaluate 重新获取
            _pump();
          }
        }
      }).ignore();
      return true;
    } on ProcessException catch (e, st) {
      // errorCode 2 = ENOENT：二进制不存在（如非 systemd 发行版）→ 永久降级
      if (e.errorCode == 2) {
        _platformUnsupported = true;
        logError(_tag, '$executable not found, keep-awake disabled', e, st);
      } else {
        logError(_tag, 'failed to spawn $executable', e, st);
      }
      return false;
    } catch (e, st) {
      logError(_tag, 'failed to spawn $executable', e, st);
      return false;
    }
  }
}
