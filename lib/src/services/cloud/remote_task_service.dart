// FluxCloud 跨设备任务协同客户端 —— 单例 + ChangeNotifier（同 ConfigSyncService
// 的单例 + SSE 长连风格）。一个服务承担三种角色，互不冲突：
//
//   查看端：SSE 接收 task.progress（批量增量）/ task.status / presence，增量更新
//           本地 [_remoteTasks] 快照，~300ms 合并后注入 DownloadController，驱动
//           设备区混排 + 进度回流 UI（绝不逐事件重建列表）。
//   接收端：SSE 收到 task.dispatch（目标为本机）或重连全量拉取时发现离线
//           期间积压的同类记录 → 经 DownloadController 建本地任务执行，回
//           reportTaskStatus(accepted)；task.command（目标本机）→ 暂停/
//           恢复/取消对应本地任务。
//   执行端：Timer 1s 采样「由下发产生的本地任务」→ 批量 reportProgress（仅活跃）+
//           状态转换即时 reportTaskStatus。节流批量是性能关键（对标迅雷云中转，
//           进度只走内存 + SSE，绝不高频请求/落库）。
//
// 数据面永远直连本地引擎执行；云端仅做连接与下发/进度中转，不取回文件。

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';

import '../../models/download_controller.dart';
import '../../models/download_task.dart';
import '../../models/settings_provider.dart';
import '../log_service.dart';
import 'cloud_auth_service.dart';
import 'cloud_client.dart';
import 'cloud_models.dart';
import 'device_identity.dart';

const _tag = 'RemoteTask';
const _kEventsPath = '/api/v1/tasks/events';
const _kSseIdleTimeout = Duration(seconds: 75);
const _kReportInterval = Duration(seconds: 1);
const _kFlushDebounce = Duration(milliseconds: 300);
const _kPresenceDebounce = Duration(seconds: 2);

/// presence 心跳间隔（C1+C2 契约）：服务端 sweeper 每 30s 扫一次、90s 未
/// 收到心跳判定离线，30s 心跳给了整整一轮 sweep 周期的冗余。
const _kHeartbeatInterval = Duration(seconds: 30);
const _kRetryDelays = [
  Duration(seconds: 5),
  Duration(seconds: 15),
  Duration(seconds: 60),
];

/// 单条 rid 重建绑定连续失败的尝试次数上限：超过即放弃（退回“丢绑定”的
/// 旧行为），避免真正匹配不到本地任务的 rid（比如用户手动删了落地任务，
/// 但服务端仍是非终态）永远留在 _pendingRebind 里，让 DownloadController
/// 每次 notifyListeners（下载中每秒多次）都触发一次 O(pendingRebind ×
/// localTasks) 全表扫描。
const _kMaxRebindAttempts = 20;

/// 本地任务在 [DownloadController.localTasks] 里连续缺失多少轮才判定为
/// "已被删除"（C9 第 3 条）：DownloadController 未暴露显式的强制刷新
/// 入口，1s 一轮采样期间引擎任务表可能存在短暂过渡态（刚重启/刚批量
/// 操作），第一次查不到不能当场判死刑，靠计数换一段宽限窗口。
const _kMissGraceRounds = 3;

/// 本地任务确认已被删除后回报服务端的错误文案（C9 第 5 条）：这是回传
/// 给服务端存库的数据字段，不是 UI 文案，固定英文常量，不走 i18n。
const _kLocalTaskGoneError =
    'local task no longer exists on the executing device';

/// [RemoteTaskService._safeReportStatus] 的上报结果，驱动"确认式推进"
/// （C9 第 1 条）：只有 [ok] 才能推进去重表/settled/解绑，[fatal] 直接
/// 解绑放弃（服务端已明确拒绝，重试无意义），[retry] 什么都不改，让
/// 下一轮 [RemoteTaskService._reportTick] 用同一个 wire 值自然重试。
enum _ReportOutcome { ok, retry, fatal }

/// 跨设备任务协同服务单例。home_page 在 providers 就绪后调 [attach] 一次。
class RemoteTaskService extends ChangeNotifier {
  RemoteTaskService._();

  static final RemoteTaskService instance = RemoteTaskService._();

  /// 查看端快照：remoteTaskId → RemoteTask（进度经 SSE 增量 copyWith）。
  final Map<String, RemoteTask> _remoteTasks = {};

  /// 只读快照，供设置页/调试查看（侧栏走 DownloadController 混排，不直接读这里）。
  Map<String, RemoteTask> get remoteTasks => Map.unmodifiable(_remoteTasks);

  /// 执行端映射：本地 taskId → remoteTaskId（下发任务落地本机后建立）。重启/
  /// 重登后此表在内存里是空的，靠 [_pendingRebind] + [_rebindPendingLocalTasks]
  /// 从服务端全量快照回填，不引入任何本地持久化。
  final Map<String, String> _localToRemote = {};

  /// 执行端待关联：下发任务 url → 待领取的 remoteTaskId 队列（FIFO）。
  /// createTask 是 rinf 单向信号（fire-and-forget，见 _acceptDispatch 旁注：
  /// 拿不到新任务 id），只能按 url 回找——同 url 可并发下发多条（重试失败
  /// 任务、多设备各下发一次同 URL），故用队列而非单值，_onControllerChanged
  /// 按本地任务出现顺序 FIFO 领取，避免后一次下发覆盖前一次的绑定。
  final Map<String, List<String>> _awaitingLocal = {};

  /// 执行端待重建绑定：本机为 toDevice 且非终态、但尚未匹配到本地任务的
  /// remoteTaskId 集合。每次 [_pullAll] 全量拉取后重算，见 [_refreshPendingRebind]。
  final Set<String> _pendingRebind = {};

  /// 重建绑定尝试失败次数：rid → 已尝试次数，仅用于 [_kMaxRebindAttempts]
  /// 放弃判断，随 [_pendingRebind] 增删同步维护。
  final Map<String, int> _pendingRebindAttempts = {};

  /// 执行端已上报的最近状态：remoteTaskId → wire 状态（去重，仅转换才上报）。
  final Map<String, String> _lastStatus = {};

  /// 本地任务在 [DownloadController.localTasks] 里连续缺失的轮次计数：
  /// 本地 taskId → 已缺失轮数，见 [_kMissGraceRounds]/[_handleMissingLocal]。
  final Map<String, int> _missCount = {};

  String get _deviceId => DeviceIdentity.deviceId();

  bool _running = false;
  bool _stopped = true;
  bool _authAttached = false;
  bool _controllerAttached = false;

  /// [_reportTick] 重入防护：定时器不能叠层——上一轮上报（含网络往返）
  /// 还没跑完时，下一轮 tick 直接跳过，避免同一 rid 被并发上报两次不同
  /// 状态，把 [_lastStatus] 的"仅转换才上报"去重打穿。
  bool _ticking = false;

  HttpClient? _sseHttp;
  StreamSubscription<String>? _sseSub;
  Timer? _sseWatchdog;
  Timer? _reportTimer;
  Timer? _flushTimer;
  Timer? _presenceTimer;
  Timer? _heartbeatTimer;
  Timer? _retryTimer;
  int _retryAttempt = 0;

  // ── 接线 ─────────────────────────────────────────────────────────────

  /// home_page 在 providers 创建后调用一次：挂账户/控制器监听，登录即启动。
  Future<void> attach() async {
    if (!_authAttached) {
      _authAttached = true;
      CloudAuthService.instance.addListener(_onAuthChanged);
    }
    _tryAttachController();
    if (CloudAuthService.instance.isLoggedIn) {
      await start();
    }
  }

  /// 补挂 DownloadController 监听，幂等（已挂过直接跳过）。home_page 只调用
  /// 一次 [attach]，此时 ctrl 可能尚未就绪（providers 初始化顺序竞态）；
  /// 不静默失败——记一条 WARN，并保证下次真正跑到这里（[start] 每次登录/
  /// 重登都会先调用它）时能补挂上，不需要额外的重试定时器。
  void _tryAttachController() {
    if (_controllerAttached) return;
    final ctrl = DownloadController.globalInstance;
    if (ctrl == null) {
      LogService.instance.log(
        _tag,
        'WARN: attach: DownloadController.globalInstance not ready yet, will retry on next start()',
      );
      return;
    }
    _controllerAttached = true;
    ctrl.addListener(_onControllerChanged);
  }

  void _onAuthChanged() {
    if (CloudAuthService.instance.isLoggedIn) {
      if (!_running) unawaited(start());
    } else {
      stop();
    }
  }

  // ── 生命周期 ─────────────────────────────────────────────────────────

  Future<void> start() async {
    if (_running || !CloudAuthService.instance.isLoggedIn) return;
    _tryAttachController();
    _running = true;
    _stopped = false;
    _retryAttempt = 0;
    _reportTimer?.cancel();
    _reportTimer = Timer.periodic(_kReportInterval, (_) => _reportTick());
    await _syncAndConnect();
  }

  void stop() {
    _stopped = true;
    _running = false;
    _ticking = false;
    _cancelRetry();
    _reportTimer?.cancel();
    _reportTimer = null;
    _flushTimer?.cancel();
    _flushTimer = null;
    _presenceTimer?.cancel();
    _presenceTimer = null;
    _closeSse();
    _remoteTasks.clear();
    _localToRemote.clear();
    _awaitingLocal.clear();
    _pendingRebind.clear();
    _pendingRebindAttempts.clear();
    _lastStatus.clear();
    _missCount.clear();
    DownloadController.globalInstance?.updateRemoteTasks(const []);
    notifyListeners();
  }

  /// 拉全量（断线重连/首启用）→ 建 SSE 流。任何失败走退避重连。
  Future<void> _syncAndConnect() async {
    if (_stopped) return;
    try {
      await _pullAll();
      if (_stopped) return;
      await _connectSse();
      if (_stopped) {
        _closeSse();
        return;
      }
      _retryAttempt = 0;
      // SSE 已建立：主动刷新设备名册，让「本机在线」立刻反映到 UI。
      // 依赖服务端回发的自身 presence 事件在重连重叠期（引用计数未过 0↔1）不广播，
      // 不能只靠事件驱动。
      _schedulePresenceRefresh();
      _startHeartbeat();
    } catch (e, stack) {
      if (_stopped) return;
      logError(_tag, 'sync/connect failed', e, stack);
      _scheduleRetry();
    }
  }

  Future<void> _pullAll() async {
    final list = await CloudClient.instance.remoteTasks();
    _remoteTasks
      ..clear()
      ..addEntries(list.map((r) => MapEntry(r.id, r)));
    _acceptOfflineDispatches();
    _refreshPendingRebind();
    _pushToController();
    notifyListeners();
  }

  /// 离线/断连期间收到的下发：SSE 未连接时 task.dispatch 事件根本不会
  /// 触发 [_acceptDispatch]（那只在 [_onEvent] 里响应实时事件），全量
  /// 快照里就会遗留 status==pending 且 toDevice==本机的记录——放任不管，
  /// 这类任务既不会被执行（没有事件重放），又会被 [_pushToController]
  /// 的“本机目标”过滤规则挡在展示层之外，用户完全看不到、也等不到它
  /// 开始下载。这里补一次接受，让离线期间的下发重连后自动开始，与设备
  /// 一直在线时语义一致；[_acceptDispatch] 内部的幂等保证同时覆盖这里
  /// 和 SSE 实时路径，不会重复接受同一条下发。
  void _acceptOfflineDispatches() {
    for (final r in _remoteTasks.values) {
      if (r.toDevice == _deviceId && r.status == RemoteTaskStatus.pending) {
        _acceptDispatch(r);
      }
    }
  }

  // ── SSE 事件流（仿 ConfigSyncService._connectSse）─────────────────────

  Future<void> _connectSse() async {
    _closeSse();
    final client = HttpClient()..connectionTimeout = const Duration(seconds: 10);
    final deviceId = Uri.encodeQueryComponent(_deviceId);
    final uri = Uri.parse('${CloudApiConfig.baseUrl}$_kEventsPath?deviceId=$deviceId');
    HttpClientResponse res;
    try {
      final req = await client.getUrl(uri);
      req.headers.set('Accept', 'text/event-stream');
      req.headers.set('Authorization', 'Bearer ${CloudClient.instance.accessToken}');
      res = await req.close();
    } catch (e) {
      client.close(force: true);
      throw CloudApiException(
        code: 'network_error',
        message: 'SSE 连接失败：$e',
        status: 0,
      );
    }
    if (res.statusCode != 200) {
      final body = await res.transform(utf8.decoder).join();
      client.close(force: true);
      throw CloudApiException(
        code: res.statusCode == 401 ? 'unauthorized' : 'sse_error',
        message: body.trim().isNotEmpty ? body.trim() : 'HTTP ${res.statusCode}',
        status: res.statusCode,
      );
    }
    _sseHttp = client;
    _resetWatchdog();
    _sseSub = res
        .transform(utf8.decoder)
        .transform(const LineSplitter())
        .listen(
          _onSseLine,
          onDone: _onSseDisconnected,
          onError: (_) => _onSseDisconnected(),
        );
  }

  void _onSseLine(String line) {
    _resetWatchdog();
    if (!line.startsWith('data:')) return;
    final payload = line.substring('data:'.length).trim();
    if (payload.isEmpty) return;
    try {
      final json = jsonDecode(payload) as Map<String, dynamic>;
      _onEvent(json);
    } catch (e, stack) {
      logError(_tag, 'sse payload parse failed: $payload', e, stack);
    }
  }

  void _onEvent(Map<String, dynamic> json) {
    switch (json['type'] as String?) {
      case 'task.dispatch':
        final r = RemoteTask.fromJson(json);
        _remoteTasks[r.id] = r;
        _scheduleFlush();
        if (r.toDevice == _deviceId && r.status == RemoteTaskStatus.pending) {
          _acceptDispatch(r);
        }
      case 'task.progress':
        final items = json['items'] as List<dynamic>? ?? const [];
        var changed = false;
        for (final it in items) {
          if (it is! Map<String, dynamic>) continue;
          final id = it['taskId'] as String?;
          if (id == null) continue;
          final cur = _remoteTasks[id];
          if (cur == null) continue;
          _remoteTasks[id] = cur.copyWith(
            status: cur.status.isTerminal
                ? cur.status
                : RemoteTaskStatus.downloading,
            downloadedBytes: (it['downloadedBytes'] as num?)?.toInt(),
            speed: (it['speed'] as num?)?.toInt(),
            progress: (it['progress'] as num?)?.toDouble(),
          );
          changed = true;
        }
        if (changed) _scheduleFlush();
      case 'task.status':
        final r = RemoteTask.fromJson(json);
        _remoteTasks[r.id] = r;
        _scheduleFlush();
      case 'task.command':
        final target =
            json['targetDevice'] as String? ?? json['toDevice'] as String?;
        if (target == _deviceId) _applyCommand(json);
      case 'presence':
        _schedulePresenceRefresh();
      case 'session.revoked':
        // 服务端吊销会话（全部或指定本机）：立即被动登出。
        // auth 状态变化经 _onAuthChanged → stop() 断 SSE，presence 随之离线。
        final target = json['deviceId'] as String?;
        if (target == null || target == _deviceId) {
          unawaited(CloudAuthService.instance.onRemoteSessionRevoked());
        }
    }
  }

  void _resetWatchdog() {
    _sseWatchdog?.cancel();
    _sseWatchdog = Timer(_kSseIdleTimeout, () {
      logInfo(_tag, 'sse idle ${_kSseIdleTimeout.inSeconds}s, reconnecting');
      _onSseDisconnected();
    });
  }

  void _onSseDisconnected() {
    if (_stopped) return;
    _closeSse();
    _scheduleRetry();
  }

  void _closeSse() {
    _heartbeatTimer?.cancel();
    _heartbeatTimer = null;
    _sseWatchdog?.cancel();
    _sseWatchdog = null;
    _sseSub?.cancel();
    _sseSub = null;
    _sseHttp?.close(force: true);
    _sseHttp = null;
  }

  void _scheduleRetry() {
    _cancelRetry();
    if (_stopped) return;
    final idx = _retryAttempt.clamp(0, _kRetryDelays.length - 1);
    _retryAttempt = idx + 1;
    _retryTimer = Timer(_kRetryDelays[idx], () {
      if (_stopped) return;
      unawaited(_syncAndConnect());
    });
  }

  void _cancelRetry() {
    _retryTimer?.cancel();
    _retryTimer = null;
  }

  // ── 查看端：合并推送给 DownloadController ─────────────────────────────

  /// ~300ms 合并窗口：高频进度事件只触发一次列表重建 + notify（性能关键）。
  void _scheduleFlush() {
    _flushTimer ??= Timer(_kFlushDebounce, () {
      _flushTimer = null;
      _pushToController();
      notifyListeners();
    });
  }

  void _pushToController() {
    // 只把目标为"其他设备"的远程任务交给展示层：toDevice==本机的下发任务
    // 在 _acceptDispatch 后已经落地成一条真实本地任务（ctrl.localTasks 里
    // 有）——SSE 实时路径和 _pullAll 里对离线期间下发的补接（见
    // _acceptOfflineDispatches）都会先走 _acceptDispatch 再到这里，这个
    // 前提对两条路径同时成立，不会有"目标本机但既未接受也不可见"的记录。
    // _remoteTasks 全量保留（上报逻辑需要），这里过滤避免执行端"全部任务"
    // 视图里同一条下发任务显示两份（真实本地任务 + 远程镜像）。
    final list = _remoteTasks.values
        .where((r) => r.toDevice != _deviceId)
        .map(_asDownloadTask)
        .toList();
    DownloadController.globalInstance?.updateRemoteTasks(list);
  }

  void _schedulePresenceRefresh() {
    _presenceTimer ??= Timer(_kPresenceDebounce, () {
      _presenceTimer = null;
      unawaited(CloudAuthService.instance.refreshDevices());
    });
  }

  /// SSE 连接建立后每 [_kHeartbeatInterval] 调一次 /tasks/presence（C1+C9
  /// 第 6 条）：presence 租约靠心跳续期，SSE 本身出错（网络中间设备静默
  /// 丢包等）不一定会立刻触发 onError/onDone，心跳独立兜底。[_closeSse]
  /// 统一负责取消（断连/重连/登出/stop() 都会经过它）。
  void _startHeartbeat() {
    _heartbeatTimer?.cancel();
    _heartbeatTimer = Timer.periodic(_kHeartbeatInterval, (_) {
      unawaited(_safePingPresence());
    });
  }

  Future<void> _safePingPresence() async {
    try {
      await CloudClient.instance.pingPresence();
    } catch (e, stack) {
      logError(_tag, 'pingPresence failed', e, stack);
    }
  }

  // ── 接收端：把下发任务落到本地引擎执行 ───────────────────────────────

  /// 幂等：同一 rid 只会被真正接受一次（[_isAlreadyAccepted] 挡重复调用）。
  /// 调用方有两处——[_onEvent] 的 task.dispatch 分支（设备在线时的实时
  /// 路径）和 [_acceptOfflineDispatches]（重连补发离线期间的下发）——
  /// 网络抖动可能让同一条 rid 被触发多次（比如上一次 accepted 状态上报
  /// 还没落到服务端、又发生一次重连），不加这道防线会对同一条下发重复
  /// createTask，在本机长出两个真实任务。
  /// 接单失败（本地建任务同步抛异常）时必须回报 failed，否则云端任务
  /// 永远挂在 pending，发起端再也等不到结果（C9 第 4 条）。带 saveDir
  /// 先降级重试一次再判定失败（C9 第 4 条附带条款）：发起端下发的
  /// saveDir 是它自己本机的路径，跨设备/跨平台（Windows 盘符在其他
  /// 平台上不存在等）大概率非法，去掉后退回本机默认目录更可能成功。
  void _acceptDispatch(RemoteTask r) {
    if (_isAlreadyAccepted(r.id)) return;
    final ctrl = DownloadController.globalInstance;
    if (ctrl == null) return;
    final saveDir = _effectiveSaveDir(r);
    try {
      ctrl.createTask(url: r.url, saveDir: saveDir, fileName: r.fileName);
    } catch (e, stack) {
      if (saveDir.isEmpty) {
        logError(_tag, 'acceptDispatch createTask failed: ${r.id}', e, stack);
        unawaited(_safeReportStatus(r.id, 'failed', error: '$e'));
        return;
      }
      logError(
        _tag,
        'acceptDispatch createTask failed with saveDir, retrying without it: ${r.id}',
        e,
        stack,
      );
      try {
        ctrl.createTask(url: r.url, saveDir: '', fileName: r.fileName);
      } catch (e2, stack2) {
        logError(_tag, 'acceptDispatch createTask retry failed: ${r.id}', e2, stack2);
        unawaited(_safeReportStatus(r.id, 'failed', error: '$e2'));
        return;
      }
    }
    // createTask 是 rinf 单向信号（fire-and-forget），Dart 侧拿不到新任务的
    // 同步 id（调查过 download_controller.dart：签名是 void，id 由 Rust 异步
    // 生成后才经 AllTasks 信号回流），只能追加进 url 对应的待关联队列，靠
    // _onControllerChanged 按本地任务出现顺序 FIFO 领取——同 url 并发下发
    // 多条时不会互相覆盖绑定。
    _awaitingLocal.putIfAbsent(r.url, () => []).add(r.id);
    unawaited(_safeReportStatus(r.id, 'accepted'));
  }

  /// [rid] 是否已经在本次运行时里被接受过（进了待关联队列或已绑定），供
  /// [_acceptDispatch] 判断是否需要跳过——纯内存态，重启后自然清零，与
  /// 类头注释里“不引入本地持久化”的设计一致。
  bool _isAlreadyAccepted(String rid) =>
      _localToRemote.containsValue(rid) ||
      _awaitingLocal.values.any((queue) => queue.contains(rid));

  /// 下发任务的落地保存目录：远程指定则用远程值，否则退回本机默认目录。
  /// [_rebindPendingLocalTasks] 回找匹配时要用同一套推导，否则重启后重建
  /// 绑定的 saveDir 比对会失真（本地任务落地时实际写的是这个值，不是原始
  /// r.saveDir）。
  String _effectiveSaveDir(RemoteTask r) =>
      (r.saveDir != null && r.saveDir!.isNotEmpty)
          ? r.saveDir!
          : (SettingsProvider.globalInstance?.effectiveDefaultSaveDir ?? '');

  void _applyCommand(Map<String, dynamic> json) {
    final rid = json['taskId'] as String? ?? json['id'] as String?;
    final action = json['action'] as String?;
    if (rid == null || action == null) return;
    String? localId;
    for (final e in _localToRemote.entries) {
      if (e.value == rid) {
        localId = e.key;
        break;
      }
    }
    if (localId == null) return;
    final ctrl = DownloadController.globalInstance;
    switch (action) {
      case 'pause':
        ctrl?.pauseTask(localId);
      case 'resume':
        ctrl?.resumeTask(localId);
      case 'cancel':
        ctrl?.cancelTask(localId);
    }
  }

  // ── 执行端：关联本地任务 + 1s 批量上报进度/状态 ──────────────────────

  void _onControllerChanged() {
    if (_awaitingLocal.isEmpty && _pendingRebind.isEmpty) return;
    final ctrl = DownloadController.globalInstance;
    if (ctrl == null) return;
    // 先领取本次会话内刚下发、待和本地新任务配对的队列（FIFO，见
    // _awaitingLocal 字段注释）；同 url 多条时按本地任务出现顺序依次认领，
    // 不会互相覆盖绑定。
    if (_awaitingLocal.isNotEmpty) {
      for (final t in ctrl.localTasks) {
        if (_localToRemote.containsKey(t.id)) continue;
        final queue = _awaitingLocal[t.url];
        if (queue == null || queue.isEmpty) continue;
        final rid = queue.removeAt(0);
        _localToRemote[t.id] = rid;
        if (queue.isEmpty) _awaitingLocal.remove(t.url);
      }
    }
    // 再尝试消化重启/重登后遗留的待重建绑定（见 _refreshPendingRebind）。
    if (_pendingRebind.isNotEmpty) _rebindPendingLocalTasks();
  }

  /// 计算「本机待重建绑定」集合：toDevice==本机、已被接受但尚未绑定的远程
  /// 任务（accepted/downloading/paused，即非终态里排除 pending）。pending
  /// 特意排除——它意味着"还没被接受"，本机根本没有对应的本地任务，重新
  /// 接受是 [_acceptOfflineDispatches]/[_onEvent] 的 task.dispatch 分支的
  /// 职责，不是这里；混进来会和 _awaitingLocal 抢同一个 rid，制造重复
  /// 绑定。
  /// [_localToRemote]/[_awaitingLocal] 都是纯内存态，重启/重登后为空——不
  /// 引入持久化存储，靠每次全量拉取后用"服务端快照 + 本地任务表回找"重建，
  /// 已绑定的排除在外，不会打扰仍然有效的既有绑定（覆盖初次启动与断线
  /// 重连两种场景，见 [_pullAll]）。
  void _refreshPendingRebind() {
    final bound = _localToRemote.values.toSet();
    _pendingRebind
      ..clear()
      ..addAll(
        _remoteTasks.values
            .where(
              (r) =>
                  r.toDevice == _deviceId &&
                  r.status != RemoteTaskStatus.pending &&
                  !r.status.isTerminal &&
                  !bound.contains(r.id),
            )
            .map((r) => r.id),
      );
    // 与 _pendingRebind 同步剪掉不再待重建的 rid 的尝试计数（已绑定/已
    // 终态/服务端已不再返回），避免这张计数表跟着历史 rid 无限增长。
    _pendingRebindAttempts.removeWhere(
      (rid, _) => !_pendingRebind.contains(rid),
    );
    if (_pendingRebind.isNotEmpty) _rebindPendingLocalTasks();
  }

  /// 用服务端全量记录回找本地任务表重建 [_localToRemote]：按 url 匹配，
  /// 同 url 有多个候选时叠加 fileName/saveDir 精确匹配收窄；一个本地任务
  /// 只绑定一次（已被占用的候选排除，避免多条同 url 远程任务抢同一条本地
  /// 任务）。候选必须至少命中 fileName 或 saveDir 其中一项（score>=1）才
  /// 会被采用——score==0 只代表 url 相同，常见于用户自己新建了一条同链接
  /// /资源包的本地下载，与下发完全无关；误绑会双向出事：[_applyCommand]
  /// 把发起端的暂停/取消打到用户自己的任务上，[_reportTick] 把用户自己
  /// 任务的进度当成远程任务上报。都不命中就宁可不绑（退化为"丢绑定"，
  /// 比误绑安全）。ctrl.localTasks 此刻可能还没加载完（引擎任务表尚未从
  /// Rust 拉回），匹配不到的 rid 留在 [_pendingRebind] 里，等
  /// [_onControllerChanged] 下次触发（本地任务表变化）再试；每次落空计入
  /// [_pendingRebindAttempts]，达到 [_kMaxRebindAttempts] 后放弃（同样退化
  /// 为丢绑定），避免真正匹配不到本地任务的 rid（比如用户手动删了落地
  /// 任务）永远占着 [_pendingRebind]，让每次 notifyListeners 都白付一遍
  /// O(pendingRebind × localTasks) 扫描。
  void _rebindPendingLocalTasks() {
    if (_pendingRebind.isEmpty) return;
    final ctrl = DownloadController.globalInstance;
    if (ctrl == null || ctrl.localTasks.isEmpty) return;
    final claimed = _localToRemote.keys.toSet();
    final resolved = <String>[];
    final gaveUp = <String>[];
    for (final rid in _pendingRebind) {
      final r = _remoteTasks[rid];
      if (r == null) {
        // 快照里已经没有这条记录，没有数据可比对，直接放弃。
        gaveUp.add(rid);
        continue;
      }
      final saveDir = _effectiveSaveDir(r);
      DownloadTask? best;
      // 起点设为 0（而非 -1）：score==0 的候选严格不会替换 best，见上方
      // 类注释里的最低分阀值说明。
      var bestScore = 0;
      for (final t in ctrl.localTasks) {
        if (claimed.contains(t.id) || t.url != r.url) continue;
        final score =
            (t.fileName == r.fileName ? 1 : 0) + (t.saveDir == saveDir ? 1 : 0);
        if (score > bestScore) {
          best = t;
          bestScore = score;
        }
      }
      if (best != null) {
        _localToRemote[best.id] = rid;
        claimed.add(best.id);
        resolved.add(rid);
        continue;
      }
      final attempts = (_pendingRebindAttempts[rid] ?? 0) + 1;
      if (attempts >= _kMaxRebindAttempts) {
        gaveUp.add(rid);
      } else {
        _pendingRebindAttempts[rid] = attempts;
      }
    }
    for (final rid in resolved) {
      _pendingRebindAttempts.remove(rid);
    }
    for (final rid in gaveUp) {
      _pendingRebindAttempts.remove(rid);
    }
    _pendingRebind
      ..removeAll(resolved)
      ..removeAll(gaveUp);
  }

  /// 每秒一轮：先上报状态转换（成功才推进去重表/解绑，见 [_applyReportOutcome]），
  /// 再批量上报活跃任务的进度。[_ticking] 防重入——见字段注释。
  Future<void> _reportTick() async {
    if (_ticking) return;
    if (_localToRemote.isEmpty) return;
    final ctrl = DownloadController.globalInstance;
    if (ctrl == null) return;
    _ticking = true;
    try {
      final byId = {for (final t in ctrl.localTasks) t.id: t};
      final reports = <ProgressReport>[];
      for (final entry in _localToRemote.entries.toList()) {
        final localId = entry.key;
        final rid = entry.value;
        final t = byId[localId];
        if (t == null) {
          await _handleMissingLocal(localId, rid);
          continue;
        }
        _missCount.remove(localId);
        final wire = _localStatusToWire(t.status);
        if (_lastStatus[rid] != wire) {
          final outcome = await _safeReportStatus(
            rid,
            wire,
            totalBytes: t.totalBytes > 0 ? t.totalBytes : null,
            fileName: t.fileName.isNotEmpty ? t.fileName : null,
            error: t.status == TaskStatus.error ? t.errorMessage : null,
          );
          _applyReportOutcome(outcome, localId: localId, rid: rid, wire: wire);
        }
        if (t.status == TaskStatus.downloading) {
          reports.add(
            ProgressReport(
              taskId: rid,
              downloadedBytes: t.downloadedBytes,
              speed: t.speed,
              progress: t.totalBytes > 0 ? t.downloadedBytes / t.totalBytes : 0,
            ),
          );
        }
      }
      if (reports.isNotEmpty) {
        await _safeReportProgress(reports);
      }
    } finally {
      _ticking = false;
    }
  }

  /// 本地任务在 [ctrl.localTasks] 里查不到时的宽限判定（C9 第 3 条）：
  /// DownloadController 未暴露显式的"强制刷新本地任务表"入口，1s 一轮
  /// 采样期间引擎任务表可能存在短暂过渡态（刚重启/刚批量操作），第一次
  /// 查不到不能当场判死刑，连续 [_kMissGraceRounds] 轮仍缺失才真正判定
  /// 为"已被用户删除"并回报固定错误文案的终态失败。
  Future<void> _handleMissingLocal(String localId, String rid) async {
    final misses = (_missCount[localId] ?? 0) + 1;
    _missCount[localId] = misses;
    if (misses < _kMissGraceRounds) return;
    const wire = 'failed';
    if (_lastStatus[rid] == wire) return;
    final outcome = await _safeReportStatus(rid, wire, error: _kLocalTaskGoneError);
    _applyReportOutcome(outcome, localId: localId, rid: rid, wire: wire);
  }

  /// [_reportTick]/[_handleMissingLocal] 共用的"确认式推进"落地点（C9 第 1
  /// 条）：只有上报真正成功（[_ReportOutcome.ok]）才写 [_lastStatus]、才
  /// 在终态时解绑；[_ReportOutcome.fatal]（服务端已明确拒绝：409 状态冲突/
  /// 403 设备不匹配/404）直接解绑放弃，重试没有意义；[_ReportOutcome.retry]
  /// 什么都不改，让下一轮 tick 用同一个 wire 值自然重试——这正是修复
  /// BLOCKER 的关键：绝不能在上报成功之前就"看起来已经报过"。
  void _applyReportOutcome(
    _ReportOutcome outcome, {
    required String localId,
    required String rid,
    required String wire,
  }) {
    switch (outcome) {
      case _ReportOutcome.ok:
        _lastStatus[rid] = wire;
        if (RemoteTaskStatus.fromWire(wire).isTerminal) {
          _localToRemote.remove(localId);
          _lastStatus.remove(rid);
          _missCount.remove(localId);
        }
      case _ReportOutcome.fatal:
        _localToRemote.remove(localId);
        _lastStatus.remove(rid);
        _missCount.remove(localId);
      case _ReportOutcome.retry:
        break;
    }
  }

  /// 上报单条状态转换，返回 [_ReportOutcome] 驱动调用方的确认式推进
  /// （见 [_applyReportOutcome]）。404/409 task_state_conflict/403
  /// task_device_mismatch 视为服务端明确拒绝，判 [_ReportOutcome.fatal]；
  /// 其余异常（网络错误、5xx、401 刷新失败等）都是暂时性故障，判
  /// [_ReportOutcome.retry]，交给下一轮 tick 自然重试。
  Future<_ReportOutcome> _safeReportStatus(
    String id,
    String status, {
    int? totalBytes,
    String? fileName,
    String? error,
  }) async {
    try {
      await CloudClient.instance.reportTaskStatus(
        id,
        status: status,
        totalBytes: totalBytes,
        fileName: fileName,
        error: error,
      );
      return _ReportOutcome.ok;
    } on CloudApiException catch (e, stack) {
      if (_isFatalReportError(e)) {
        logError(_tag, 'reportTaskStatus fatal (${e.code}): $id', e, stack);
        return _ReportOutcome.fatal;
      }
      logError(_tag, 'reportTaskStatus failed: $id', e, stack);
      return _ReportOutcome.retry;
    } catch (e, stack) {
      logError(_tag, 'reportTaskStatus failed: $id', e, stack);
      return _ReportOutcome.retry;
    }
  }

  bool _isFatalReportError(CloudApiException e) =>
      e.status == 404 ||
      (e.status == 409 && e.code == 'task_state_conflict') ||
      (e.status == 403 && e.code == 'task_device_mismatch');

  Future<void> _safeReportProgress(List<ProgressReport> items) async {
    try {
      await CloudClient.instance.reportProgress(items);
    } catch (e, stack) {
      logError(_tag, 'reportProgress failed', e, stack);
    }
  }

  // ── 状态映射 ─────────────────────────────────────────────────────────

  /// 穷举 [TaskStatus] 全部取值（switch 无 `_` 兜底）：新增枚举值时编译器
  /// 会强制在这里显式决策 wire 映射，不会被静默吞成 'accepted'——这正是
  /// 修复 canceled 曾被兜底吞掉这一 MAJOR 缺陷的根本手段，而不只是加一
  /// 个 case。pending 映射到 'accepted'：本地任务刚落地、引擎尚未真正
  /// 开始，语义上等同"已被本机接受、还没下载"。
  String _localStatusToWire(TaskStatus s) => switch (s) {
    TaskStatus.downloading ||
    TaskStatus.preparing ||
    TaskStatus.resuming => 'downloading',
    TaskStatus.paused => 'paused',
    TaskStatus.completed => 'completed',
    TaskStatus.error => 'failed',
    TaskStatus.canceled => 'canceled',
    TaskStatus.pending => 'accepted',
  };

  TaskStatus _mapStatus(RemoteTaskStatus s) => switch (s) {
    RemoteTaskStatus.downloading => TaskStatus.downloading,
    RemoteTaskStatus.paused => TaskStatus.paused,
    RemoteTaskStatus.completed => TaskStatus.completed,
    RemoteTaskStatus.failed => TaskStatus.error,
    // 对端设备主动取消：直接映射到 TaskStatus.canceled，与失败区分开来。
    RemoteTaskStatus.canceled => TaskStatus.canceled,
    _ => TaskStatus.pending,
  };

  DownloadTask _asDownloadTask(RemoteTask r) => DownloadTask(
    id: 'remote:${r.id}',
    url: r.url,
    fileName: r.fileName.isNotEmpty ? r.fileName : _fileNameFromUrl(r.url),
    saveDir: r.saveDir ?? '',
    status: _mapStatus(r.status),
    downloadedBytes: r.downloadedBytes,
    totalBytes: r.totalBytes ?? 0,
    speed: r.speed,
    errorMessage: r.error ?? '',
    deviceId: r.toDevice,
    isRemote: true,
    createdAt: DateTime.tryParse(r.createdAt),
  );

  String _fileNameFromUrl(String url) {
    try {
      final seg = Uri.parse(url).pathSegments;
      if (seg.isNotEmpty && seg.last.isNotEmpty) return seg.last;
    } catch (_) {}
    return url;
  }
}
