// FluxDown 本地设备互联（局域网配对）客户端服务 —— 单例 + ChangeNotifier
// （同 CloudAuthService/RemoteTaskService 的单例风格）。
//
// 与 FluxCloud 账户体系无关：不登录账号，双方在同一局域网内即可直接配对，
// 数据面/控制面均走 Rust 端 LinkManager（native/engine/src/link/），本服务
// 只负责：
//   - 发 LinkCommand（发现/探测/配对/名册管理）；
//   - 收 LinkEvent，按 kind 分发更新本地状态并 notifyListeners；
//   - 把生成信号类型转换为 [LocalDevice]/[LocalDiscoveredPeer]/
//     [PairingChallenge] 等领域模型（见 link_models.dart），UI 层不直接
//     依赖 bindings 生成类型。
//
// 传输方式的可扩展性说明见 Rust 侧 native/engine/src/link/transport.rs（v1 只做局域网直连）。

import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:rinf/rinf.dart';

import '../../bindings/bindings.dart';
import '../log_service.dart';
import 'link_models.dart';

const _tag = 'LocalPairing';

/// 本地设备互联服务单例。宿主页面在 providers 就绪后调 [attach] 一次
/// （同 RemoteTaskService.attach 的接线时机）。
class LocalPairingService extends ChangeNotifier {
  LocalPairingService._();

  static final LocalPairingService instance = LocalPairingService._();

  /// 入站配对本机决策窗口：与后端 PairingResponder::handle_confirm 的
  /// LOCAL_DECISION_TIMEOUT（60s）保持一致，且**锚点是 hello 到达时刻**，
  /// 不是「用户开始核对的时刻」——引擎收到发起方 hello 后立即广播
  /// `incomingPairing`，随后收到 confirm 时按 `entry.created +
  /// LOCAL_DECISION_TIMEOUT` 判断决策窗口是否已过，不会等 confirm 抵达才
  /// 重新起算 60s（发起方核对 SAS 花掉的时间同样计入这 60s）。UI 倒计时
  /// 必须用同一锚点，否则会和引擎实际窗口对不上：过去这里从「弹窗打开」
  /// 起算，若发起方核对超过 60s 才点确认，响应方弹窗早已自动关闭。
  static const Duration incomingPairingDecisionWindow = Duration(seconds: 60);

  /// 本地设备互联在当前平台是否可用。
  ///
  /// 产品决策：移动端（Android/iOS）不做本地设备互联——Rust 侧 `hub_link` cfg
  /// 在 android/ios 上不编译（见 native/hub/build.rs），`LinkCommand` 分发器
  /// 在这两个平台上退化为空闭包（native/hub/src/actors/download_actor.rs），
  /// 发了命令也没有处理方；因此本服务在移动端整体不生效——[attach] 不订阅
  /// 信号，所有对外命令方法均为空操作。
  bool get supported => !(Platform.isAndroid || Platform.isIOS);

  bool _attached = false;
  StreamSubscription<RustSignalPack<LinkEvent>>? _sub;

  /// 局域网内发现的未配对设备（发现阶段增量 upsert，按 [LocalDiscoveredPeer.dedupeKey] 去重）。
  List<LocalDiscoveredPeer> _discoveredPeers = const [];
  List<LocalDiscoveredPeer> get discoveredPeers => _discoveredPeers;

  /// 已配对（受信任）的本地设备名册。
  List<LocalDevice> _localDevices = const [];
  List<LocalDevice> get localDevices => _localDevices;

  /// 是否已有至少一台已配对设备（供侧栏/设置页判断是否展示本地设备区）。
  bool get hasLocalDevices => _localDevices.isNotEmpty;

  /// 当前待用户核验的配对挑战（SAS 短数字），为 null 表示当前没有进行中的配对。
  PairingChallenge? _pendingChallenge;
  PairingChallenge? get pendingChallenge => _pendingChallenge;

  /// 入站配对请求（本机作为被添加方）：对端已通过一次性码发起 hello，等待
  /// 本机用户核对 SAS 后批准/拒绝，为 null 表示当前没有挂起的入站请求。
  IncomingPairing? _incomingPairing;
  IncomingPairing? get incomingPairing => _incomingPairing;

  /// [_incomingPairing] 的决策截止时间点（事件到达时刻 +
  /// [incomingPairingDecisionWindow]）。只暴露绝对时间点供 UI 按
  /// `deadline - now` 算倒计时剩余，不用 Timer.periodic 每秒自减——后者在
  /// 系统卡顿/后台限速时会跑偏。Web 侧同语义见 web/src/lib/ws.ts 的
  /// incomingPairingStore.at。
  DateTime? _incomingPairingDeadline;
  DateTime? get incomingPairingDeadline => _incomingPairingDeadline;

  /// 最近一次错误消息（`LinkEvent{kind:"error"}`），供 UI 弹 toast/内联提示。
  String? _lastError;
  String? get lastError => _lastError;

  /// 本机当前生成的配对码（`LinkEvent{kind:"code"}`），供本机作为「被配对方」
  /// 时展示给用户在对端输入。
  String? _generatedCode;
  String? get generatedCode => _generatedCode;

  /// [_generatedCode] 的过期时间点，由 `LinkEvent{kind:"code"}.ttlSeconds`
  /// 换算得出；只暴露绝对时间点供 UI 自行跑 Ticker 做倒计时展示，service 内部
  /// 不起 Timer。
  DateTime? _codeExpiresAt;
  DateTime? get codeExpiresAt => _codeExpiresAt;

  /// 配对码是否已过期。
  bool get codeExpired =>
      _codeExpiresAt != null && DateTime.now().isAfter(_codeExpiresAt!);

  /// 是否正在进行局域网发现。
  bool _discovering = false;
  bool get discovering => _discovering;

  /// [dispatchTask] 按 fingerprint 归属结果用的等待表：`dispatched`/
  /// `error{action:"dispatch"}` 事件只带 fingerprint、不带请求级 token，
  /// 只能以「同一 fingerprint 同一时刻至多一次在途下发」为前提按
  /// fingerprint 归属——旧实现靠「调用前清 lastError/lastDispatchedTaskId，
  /// 谁先变化就归谁」，并发下发到不同设备时会互相抢答对方的结果。
  final Map<String, Completer<void>> _dispatchWaiters = {};

  // ── 接线 ─────────────────────────────────────────────────────────────

  /// 宿主页面在 providers 创建后调用一次：订阅 LinkEvent 信号流。幂等。
  Future<void> attach() async {
    if (!supported) return;
    if (_attached) return;
    _attached = true;
    _startListening();
  }

  void _startListening() {
    _sub = LinkEvent.rustSignalStream.listen(_onLinkEvent);
  }

  // ── Dart → Rust 命令 ─────────────────────────────────────────────────

  /// 开始局域网发现（mDNS/广播，具体机制由 Rust 端实现）。
  void startDiscovery() {
    if (!supported) return;
    _discovering = true;
    // 只清错误，不清 [_pendingChallenge]——切 Tab 离开发现页再切回会重新调用
    // 本方法，若在此清空会把已到达但用户还未处理的配对挑战静默吞掉。
    _lastError = null;
    // Rust 端每次 start_discovery 都会清空自己的发现快照；Dart 侧若只增不减，
    // 已关机/离网的设备会永久滞留在列表里，用户点了必然超时失败。
    _discoveredPeers = const [];
    notifyListeners();
    _send(action: 'startDiscovery');
  }

  /// 停止局域网发现。
  void stopDiscovery() {
    if (!supported) return;
    _discovering = false;
    notifyListeners();
    _send(action: 'stopDiscovery');
  }

  /// 探测指定地址是否为可配对的 FluxDown 设备（手动输入地址场景）。
  void probe({required String host, required int port}) {
    if (!supported) return;
    _send(action: 'probe', host: host, port: port);
  }

  /// 发起配对：向目标设备发送本机身份 + 用户输入的配对码，等待对端下发
  /// `LinkEvent{kind:"pairingChallenge"}`。
  void beginPairing({
    required String host,
    required int port,
    required String code,
  }) {
    if (!supported) return;
    _lastError = null;
    notifyListeners();
    _send(action: 'beginPairing', host: host, port: port, code: code);
  }

  /// 确认/拒绝当前挂起的配对挑战（SAS 核验通过后调用）。没有挂起挑战时忽略。
  void confirmPairing(bool accept) {
    if (!supported) return;
    final challenge = _pendingChallenge;
    if (challenge == null) {
      logInfo(_tag, 'confirmPairing skipped: no pending challenge');
      return;
    }
    _send(action: 'confirmPairing', token: challenge.token, accept: accept);
    if (!accept) {
      // Rust 端拒绝路径（LinkManager.pair_confirm 在 !accept 时早退）不会发
      // 任何能清掉挑战态的事件——'devices' 分支也不清 challenge；不在这里
      // 立即本地清空的话，SAS 弹窗点「取消」后会永久卡在同一组失效数字上。
      _pendingChallenge = null;
      notifyListeners();
    }
  }

  /// 批准/拒绝当前挂起的入站配对请求（本机作为被添加方，核对
  /// [IncomingPairing.sas] 与对端屏幕显示一致后调用）。没有挂起请求时忽略。
  void approveIncoming(bool accept) {
    if (!supported) return;
    final pending = _incomingPairing;
    if (pending == null) {
      logInfo(_tag, 'approveIncoming skipped: no pending incoming pairing');
      return;
    }
    _send(
      action: 'approveIncoming',
      sessionId: pending.sessionId,
      accept: accept,
    );
    // 乐观清除：避免等待后端确认期间弹窗一直卡着；真实结果经后续 paired /
    // pairingRejected / error 事件另行反馈给用户。
    _incomingPairing = null;
    _incomingPairingDeadline = null;
    notifyListeners();
  }

  /// 本机决策倒计时归零、用户未表态时调用：只清本地待核验态并触发关闭
  /// （notifyListeners 驱动弹窗监听器 pop 自己），不回传任何决策指令。
  ///
  /// 为什么不再像过去那样发 approveIncoming(false)：引擎侧决策窗口现在与
  /// 本倒计时同锚在 hello 到达时刻（[incomingPairingDecisionWindow]），归
  /// 零意味着引擎那头此刻同样判定超时——发起方等待 confirm 时会直接收到
  /// PairingTimeout，不需要本机主动回传拒绝；主动回传反而会让发起方看到
  /// 「对方拒绝了配对」这种失实结论（没有人做出拒绝决策，只是没人来得及
  /// 看）。Web 侧同语义：归零只清 store，不调 approveIncoming API。
  ///
  /// [sessionId] 由调用方（弹窗）在打开时快照：若当前 [_incomingPairing]
  /// 已经是别的会话，说明本轮超时的会话早已被新入站请求顶替，不能误清。
  void dismissIncomingPairingTimeout(String sessionId) {
    if (!supported) return;
    if (_incomingPairing?.sessionId != sessionId) return;
    _incomingPairing = null;
    _incomingPairingDeadline = null;
    notifyListeners();
  }

  /// 生成本机配对码（本机作为被配对方时调用），结果经
  /// `LinkEvent{kind:"code"}` 回流到 [generatedCode]/[codeExpiresAt]。
  void generateCode() {
    if (!supported) return;
    _send(action: 'generateCode');
  }

  /// 停止 mDNS 广播（本机不再出现在对端「发现」列表里）——**不撤销**已
  /// 生成的配对码：Rust 端 `LinkManager::stop_advertising` 只丢弃 mDNS
  /// advertiser，`PairingResponder` 里的码记录完全不受影响，依旧活到
  /// `CODE_TTL`（120s，见 native/engine/src/link/pairing.rs）——对端只要
  /// 记得这个码，仍可在有效期内直接输入 IP:端口完成配对。因此只在码确实
  /// 已经过期时才清空本地 [generatedCode]/[codeExpiresAt]；码还没过期时
  /// 调用本方法只停广播、不清本地展示态，否则 UI 会冒充码已失效，和
  /// 「2 分钟内有效」的文案自相矛盾。
  void stopAdvertising() {
    if (!supported) return;
    _send(action: 'stopAdvertising');
    if (!codeExpired) return;
    _generatedCode = null;
    _codeExpiresAt = null;
    notifyListeners();
  }

  /// 刷新已配对设备名册。
  void refreshDevices() {
    if (!supported) return;
    _send(action: 'listDevices');
  }

  /// 解除与指定设备的配对关系。
  void removeDevice(String fingerprint) {
    if (!supported) return;
    _send(action: 'removeDevice', fingerprint: fingerprint);
  }

  /// 向已配对设备下发一个下载任务（[fingerprint] 为目标设备指纹），返回的
  /// Future 在对端确认收到（`LinkEvent{kind:"dispatched"}`）或失败
  /// （`LinkEvent{kind:"error", action:"dispatch"}`）时结算，按 fingerprint
  /// 归属结果——两个入口并发下发到不同设备时各自独立结算，不会像旧实现
  /// （靠"调用前清 lastError/lastDispatchedTaskId，谁先变化就归谁"）那样
  /// 被并发的另一次调用抢答。15s 内无回应按超时失败，避免网络丢包导致
  /// 调用方永久 await 不返回。
  ///
  /// 同一 [fingerprint] 上重复调用：新调用会顶替等待表里的旧登记，旧调用
  /// 的 Future 以超时收尾——Rust 侧没有"取消上一次下发"的概念，并发重复
  /// 下发到同一设备是调用方自己的责任，不在本方法职责内。
  Future<void> dispatchTask({
    required String fingerprint,
    required String url,
    String saveDir = '',
    String fileName = '',
  }) {
    if (!supported) {
      return Future<void>.error(
        StateError('local pairing unsupported on this platform'),
      );
    }
    final completer = Completer<void>();
    _dispatchWaiters[fingerprint] = completer;
    _send(
      action: 'dispatch',
      fingerprint: fingerprint,
      url: url,
      saveDir: saveDir,
      fileName: fileName,
    );
    return completer.future.timeout(
      const Duration(seconds: 15),
      onTimeout: () {
        // 清理时校验身份：等待期间若同一 fingerprint 又发起了新一轮下发，
        // 表里已经是新调用的 completer，这里不能误删。
        if (identical(_dispatchWaiters[fingerprint], completer)) {
          _dispatchWaiters.remove(fingerprint);
        }
        throw TimeoutException('local dispatch to $fingerprint timed out');
      },
    );
  }

  void _send({
    required String action,
    String host = '',
    int port = 0,
    String code = '',
    String token = '',
    bool accept = false,
    String fingerprint = '',
    String sessionId = '',
    String url = '',
    String saveDir = '',
    String fileName = '',
  }) {
    LinkCommand(
      action: action,
      host: host,
      port: port,
      code: code,
      token: token,
      accept: accept,
      fingerprint: fingerprint,
      sessionId: sessionId,
      url: url,
      saveDir: saveDir,
      fileName: fileName,
    ).sendSignalToRust();
  }

  // ── Rust → Dart 事件分发 ─────────────────────────────────────────────

  void _onLinkEvent(RustSignalPack<LinkEvent> pack) {
    final event = pack.message;
    switch (event.kind) {
      case 'code':
        _generatedCode = event.code;
        _codeExpiresAt = DateTime.now().add(
          Duration(seconds: event.ttlSeconds),
        );
        notifyListeners();
        break;
      case 'discovered':
        _upsertDiscovered(event.discovered);
        break;
      case 'pairingChallenge':
        _pendingChallenge = PairingChallenge.fromEvent(event);
        notifyListeners();
        break;
      case 'incomingPairing':
        // 新入站请求会直接覆盖上一条尚未决策的 [_incomingPairing]（UI 不
        // 支持排队多个入站请求，见 home_page._onLocalPairingChanged）。这
        // 里不主动对被覆盖的旧会话发 approveIncoming(false)：error 事件
        // 目前只按 action 归属、不带 sessionId，两次并发 approveIncoming
        // 调用中若"拒绝旧会话"这次失败，会被误判成"当前新会话决策失败"
        // 进而错误清空刚接手的新会话。旧会话就放着在
        // [incomingPairingDecisionWindow] 内自然到期——代价是发起方最多
        // 多等 60s 才收到 PairingTimeout，换来的是不会有跨会话的状态串扰。
        //
        // 决策截止时间锚在事件到达时刻，与引擎侧
        // `entry.created + LOCAL_DECISION_TIMEOUT` 同锚点。
        _incomingPairing = IncomingPairing.fromEvent(event);
        _incomingPairingDeadline = DateTime.now().add(
          incomingPairingDecisionWindow,
        );
        notifyListeners();
        break;
      case 'paired':
        // 配对成功：清空挑战态（含入站待核验请求）与旧错误，并拉一次最新
        // 名册（含刚配对的新设备）。
        _pendingChallenge = null;
        _incomingPairing = null;
        _incomingPairingDeadline = null;
        _lastError = null;
        refreshDevices();
        notifyListeners();
        break;
      case 'pairingRejected':
        // 发起方侧：本机发起的配对被对端拒绝，挑战态必然作废。
        _pendingChallenge = null;
        // 被添加方侧：只有当事件确实指向**当前**挂起的那个入站会话时才清空。
        // 旧会话可能已被新的 hello 顶掉，无差别清空会把刚接手的新会话一起抹掉
        // （事件带 sessionId 正是为此，见 download_actor 的 approveIncoming 分支）。
        if (event.sessionId.isEmpty ||
            event.sessionId == _incomingPairing?.sessionId) {
          _incomingPairing = null;
          _incomingPairingDeadline = null;
        }
        notifyListeners();
        break;
      case 'unpaired':
        _localDevices = _localDevices
            .where((d) => d.fingerprint != event.fingerprint)
            .toList(growable: false);
        notifyListeners();
        break;
      case 'devices':
        _localDevices = event.devices
            .map(LocalDevice.fromPiece)
            .toList(growable: false);
        notifyListeners();
        break;
      case 'dispatched':
        _completeDispatch(event.fingerprint, null);
        notifyListeners();
        break;
      case 'error':
        // error 是全子系统共用的单一通道（发现失败/配对失败/入站核验
        // 失败/下发失败都走它）；event.action 标出来源命令名（空串 = 子
        // 系统级错误，见 LinkEvent.action 文档），按来源只清相关的那份
        // 状态——不能再无差别清空，否则用户正在 SAS 核对页面时，另一个
        // 入口的下发失败会把核对视图凭空摧毁。
        switch (event.action) {
          case 'startDiscovery':
          case 'stopDiscovery':
          case 'probe':
            _lastError = event.message;
            _discovering = false;
            break;
          case 'beginPairing':
          case 'confirmPairing':
            _lastError = event.message;
            _pendingChallenge = null;
            break;
          case 'approveIncoming':
            _lastError = event.message;
            _incomingPairing = null;
            _incomingPairingDeadline = null;
            break;
          case 'dispatch':
            // dispatch 走独立的 fingerprint 归属通道（见 dispatchTask /
            // _completeDispatch），不写共享的 lastError——避免污染其它
            // 正在读 lastError 内联展示的 UI（如 SAS 核对页在等
            // beginPairing/confirmPairing 的结果）。
            _completeDispatch(event.fingerprint, event.message);
            break;
          default:
            // 空串（子系统级错误）或未来新增的未知 action：只记录，不清
            // 任何一份配对态。
            _lastError = event.message;
        }
        notifyListeners();
        break;
      default:
        logInfo(_tag, 'unhandled LinkEvent kind: ${event.kind}');
    }
  }

  /// 按 [fingerprint] 结算一次 [dispatchTask] 调用：成功传
  /// `errorMessage: null`，失败传错误消息。找不到等待者时静默忽略——要么
  /// 早已因超时被移除（调用方已经拿到超时失败结果，没有人还在等这个迟到
  /// 的真实原因），要么根本不是本服务发起的下发。
  void _completeDispatch(String fingerprint, String? errorMessage) {
    final completer = _dispatchWaiters[fingerprint];
    if (completer == null || completer.isCompleted) return;
    _dispatchWaiters.remove(fingerprint);
    if (errorMessage == null) {
      completer.complete();
    } else {
      completer.completeError(Exception(errorMessage));
    }
  }

  void _upsertDiscovered(LinkDiscoveredPiece? piece) {
    if (piece == null) return;
    final peer = LocalDiscoveredPeer.fromPiece(piece);
    final next = List<LocalDiscoveredPeer>.of(_discoveredPeers);
    final idx = next.indexWhere((p) => p.dedupeKey == peer.dedupeKey);
    if (idx >= 0) {
      next[idx] = peer;
    } else {
      next.add(peer);
    }
    _discoveredPeers = next;
    notifyListeners();
  }

  @override
  void dispose() {
    unawaited(_sub?.cancel());
    _attached = false;
    super.dispose();
  }
}
