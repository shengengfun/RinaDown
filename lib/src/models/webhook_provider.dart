import 'dart:async';
import 'dart:convert';
import 'dart:math';

import 'package:flutter/foundation.dart';
import 'package:rinf/rinf.dart';

import '../bindings/bindings.dart';
import '../services/log_service.dart';
import 'webhook_endpoint.dart';

/// 本地投递日志上限，与引擎的 `MAX_DELIVERY_LOG` 对齐。
const int _kMaxLocalDeliveries = 1000;

/// Webhook 运行时状态：投递日志 + 服务预设目录 + 测试结果。
///
/// **端点表不在这里**——它是引擎配置（`webhook.endpoints`），归
/// [SettingsProvider] 管。本 Provider 只承载「引擎内存里、DB 查不到」的东西：
/// 环形投递日志、预设模板目录、一次性测试回执。
///
/// 复刻 [PluginProvider] 的 ChangeNotifier + rinf 信号订阅模式：
/// `refresh()` 主动拉取，写操作单向 `sendSignalToRust()`，结果异步回流。
class WebhookProvider extends ChangeNotifier {
  static const String _tag = 'Webhook';

  List<WebhookDeliveryEntry> _deliveries = const [];
  List<WebhookPresetEntry> _presets = const [];
  List<String> _variables = const [];

  WebhookTestResult? _lastTestResult;
  String _pendingTestRequestId = '';
  bool _testing = false;
  bool _disposed = false;

  /// 在途的测试投递：`requestId → endpointId`。
  ///
  /// 按端点记而不是一个全局 bool：端点行上每行都有「测试」按钮，同时点两
  /// 行是正常操作，一个 bool 会把两行一起变成转圈。
  final Map<String, String> _pendingTests = {};

  /// 「模拟一次下载完成」已点、还没拿到受理回执。
  ///
  /// 这段窗口只有几毫秒（`emit` 是同步投队列），**它不是等待投递完成**——
  /// 真正耗时的是后面的 HTTP（最多 4 次尝试 × 10s 超时 + 2/4/8s 退避）。
  bool _simulating = false;
  Timer? _simulateGuard;

  /// 受理回执说要投几个端点，就等几条新记录落库。**这才是用户眼里的
  /// 「投递中」**：失败端点要重试到 ~54s，这期间面板必须一直有动静。
  int _expectedDeliveries = 0;

  /// 发起模拟那一刻已有的记录 id。新记录 = 不在这个集合里的。
  Set<String> _deliveryBaseline = const {};
  Timer? _deliveryGuard;

  /// 上一次模拟投出去的端点数；`null` = 本次会话还没模拟过。
  /// `0` 是要给用户看的结果：没有端点订阅 `task.completed`。
  int? _lastSimulateDispatched;

  StreamSubscription<RustSignalPack<WebhookDeliveries>>? _deliveriesSub;
  StreamSubscription<RustSignalPack<WebhookDeliveriesDelta>>? _deltaSub;
  StreamSubscription<RustSignalPack<WebhookPresets>>? _presetsSub;
  StreamSubscription<RustSignalPack<WebhookTestResult>>? _testSub;
  StreamSubscription<RustSignalPack<WebhookSimulateAck>>? _simulateSub;

  WebhookProvider() {
    _deliveriesSub = WebhookDeliveries.rustSignalStream.listen(_onDeliveries);
    _deltaSub = WebhookDeliveriesDelta.rustSignalStream.listen(
      _onDeliveriesDelta,
    );
    _presetsSub = WebhookPresets.rustSignalStream.listen(_onPresets);
    _testSub = WebhookTestResult.rustSignalStream.listen(_onTestResult);
    _simulateSub = WebhookSimulateAck.rustSignalStream.listen(_onSimulateAck);
  }

  @override
  void dispose() {
    _disposed = true;
    _simulateGuard?.cancel();
    _deliveryGuard?.cancel();
    _deliveriesSub?.cancel();
    _deltaSub?.cancel();
    _presetsSub?.cancel();
    _testSub?.cancel();
    _simulateSub?.cancel();
    super.dispose();
  }

  void _safeNotifyListeners() {
    if (!_disposed) notifyListeners();
  }

  // ---------------------------------------------------------------------------
  // Getters
  // ---------------------------------------------------------------------------

  /// 投递日志，**新的在前**（引擎内存环形缓冲 100 条，重启清零）。
  List<WebhookDeliveryEntry> get deliveries => _deliveries;

  /// 服务预设目录（引擎是模板的单一事实源，客户端只做占位符替换）。
  List<WebhookPresetEntry> get presets => _presets;

  /// 可用占位符清单（`{task.fileName}` 等），供「点击插入变量」。
  List<String> get variables => _variables;

  /// 最近一次「发送测试」的结果；`null` = 尚未测过。
  WebhookTestResult? get lastTestResult => _lastTestResult;

  /// 对话框草稿的测试请求在途（页脚按钮转圈期间）。
  bool get testing => _testing;

  /// 「模拟一次下载完成」还没走完：受理中，或已受理但投递记录还没回来。
  ///
  /// 按钮的转圈跟着它 —— 只跟受理回执的话，转圈只闪几毫秒，而真正要等的
  /// 是后面几十秒的 HTTP。
  bool get simulating => _simulating || _expectedDeliveries > 0;

  /// 上一次模拟投出去的端点数；`null` = 还没模拟过，`0` = 无目标订阅。
  int? get lastSimulateDispatched => _lastSimulateDispatched;

  /// 该端点是否有测试投递在途（端点行按钮转圈 + 拦重复点击）。
  bool isTesting(String endpointId) =>
      endpointId.isNotEmpty && _pendingTests.containsValue(endpointId);

  /// 当前有多少条投递在途（日志面板据此显示「投递中」占位行）。
  int get pendingCount =>
      _pendingTests.length +
      (_simulating ? 1 : 0) +
      _remainingExpectedDeliveries();

  /// 按 wire 名取预设元数据；未知预设回退 `custom`，再回退 `null`。
  WebhookPresetEntry? presetById(String id) {
    for (final p in _presets) {
      if (p.id == id) return p;
    }
    for (final p in _presets) {
      if (p.id == WebhookEndpoint.kPresetCustom) return p;
    }
    return null;
  }

  /// 某端点最近一次投递记录（端点行的健康状态 = 投递日志的第一层）。
  WebhookDeliveryEntry? latestFor(String endpointId) {
    for (final d in _deliveries) {
      if (d.endpointId == endpointId) return d;
    }
    return null;
  }

  // ---------------------------------------------------------------------------
  // 写操作（单向信号）
  // ---------------------------------------------------------------------------

  /// 拉取投递日志 + 预设目录（打开通知设置页时调用）。
  void refresh() {
    const RequestWebhookDeliveries().sendSignalToRust();
  }

  void clearDeliveries() {
    const ClearWebhookDeliveries().sendSignalToRust();
  }

  /// 「模拟一次 task.completed」——按已保存端点的订阅规则走完整投递路径。
  ///
  /// 上一轮还没投完就直接忽略：一次点击一次投递，连点会在对端刷出一串
  /// 通知，而失败端点要重试到 ~54s，那期间连点尤其容易发生。
  void simulate() {
    if (simulating) return;
    _simulating = true;
    _lastSimulateDispatched = null;
    _clearExpectedDeliveries();
    // 兜底：回执理论上必到（引擎同步返回派发数）。真丢了也不能让按钮
    // 一直转，10s 后自行解锁。
    _simulateGuard?.cancel();
    _simulateGuard = Timer(const Duration(seconds: 10), () {
      if (_simulating) {
        _simulating = false;
        _safeNotifyListeners();
      }
    });
    _safeNotifyListeners();
    const SimulateWebhookEvent().sendSignalToRust();
  }

  /// 对草稿端点发一次测试投递。端点**无需先保存**。
  ///
  /// 结果经 [WebhookTestResult] 回流，用 requestId 配对——用户可能在结果
  /// 回来之前又点了一次，旧回执必须被丢弃。
  ///
  /// 同一端点已有测试在途时直接忽略：慢网络下用户会连点，每一下都真发一
  /// 个 HTTP 请求，对端会收到一串重复通知。
  void testEndpoint(WebhookEndpoint endpoint) {
    if (isTesting(endpoint.id)) return;
    final requestId = _newRequestId();
    _pendingTestRequestId = requestId;
    _pendingTests[requestId] = endpoint.id;
    _testing = true;
    _lastTestResult = null;
    _safeNotifyListeners();
    TestWebhookEndpoint(
      requestId: requestId,
      endpointJson: jsonEncode(endpoint.toJson()),
    ).sendSignalToRust();
  }

  /// 丢弃当前测试态（打开/关闭对话框时调用，防止闪现上一次的结果）。
  ///
  /// `notify: false` 供**对话框 `initState`** 使用：`initState` 跑在 build
  /// 相位，此时 notify 会让监听者在构建途中 `markNeedsBuild` 而直接抛断言。
  /// 那个场景也确实不需要通知——唯一的消费者（对话框页脚）正要首次构建，
  /// 读到的就是清干净之后的值。
  void resetTestState({bool notify = true}) {
    if (_pendingTestRequestId.isEmpty && !_testing && _lastTestResult == null) {
      return;
    }
    _pendingTestRequestId = '';
    _testing = false;
    _lastTestResult = null;
    if (notify) _safeNotifyListeners();
  }

  // ---------------------------------------------------------------------------
  // 信号回调
  // ---------------------------------------------------------------------------

  /// 整仓快照（打开面板时主动拉的那一次，以及清空之后）——整表替换。
  void _onDeliveries(RustSignalPack<WebhookDeliveries> pack) {
    _deliveries = pack.message.entries;
    _settleAfterDeliveryChange();
  }

  /// 增量（引擎侧每有新记录就推最新一小段）——按 `deliveryId` 合并。
  ///
  /// 不能整表替换：这一段最多 100 条，而本地可能已经攒了上千条历史，
  /// 替换会把面板里翻着的旧记录一把抹掉。
  void _onDeliveriesDelta(RustSignalPack<WebhookDeliveriesDelta> pack) {
    final incoming = pack.message.entries;
    if (incoming.isEmpty) return;
    final seen = <String>{for (final d in incoming) d.deliveryId};
    final merged = <WebhookDeliveryEntry>[
      ...incoming,
      for (final d in _deliveries)
        if (!seen.contains(d.deliveryId)) d,
    ];
    // 引擎侧上限同为 1000，本地跟着裁，不让它无限涨。
    _deliveries = merged.length > _kMaxLocalDeliveries
        ? merged.sublist(0, _kMaxLocalDeliveries)
        : merged;
    _settleAfterDeliveryChange();
  }

  void _settleAfterDeliveryChange() {
    if (_expectedDeliveries > 0 && _remainingExpectedDeliveries() == 0) {
      _clearExpectedDeliveries();
    }
    _safeNotifyListeners();
  }

  /// 还差几条模拟投递没落库。
  int _remainingExpectedDeliveries() {
    if (_expectedDeliveries == 0) return 0;
    var fresh = 0;
    for (final d in _deliveries) {
      if (!_deliveryBaseline.contains(d.deliveryId)) fresh++;
    }
    final remaining = _expectedDeliveries - fresh;
    return remaining > 0 ? remaining : 0;
  }

  void _clearExpectedDeliveries() {
    _deliveryGuard?.cancel();
    _deliveryGuard = null;
    _expectedDeliveries = 0;
    _deliveryBaseline = const {};
  }

  void _onPresets(RustSignalPack<WebhookPresets> pack) {
    _presets = pack.message.presets;
    _variables = pack.message.variables;
    _safeNotifyListeners();
  }

  void _onTestResult(RustSignalPack<WebhookTestResult> pack) {
    final result = pack.message;
    // 每条回执都要销掉自己那条在途记录 —— 哪怕它已经不是对话框正在等的
    // 那一条（用户重测过），端点行的转圈也得停。
    final wasPending = _pendingTests.remove(result.requestId) != null;
    if (result.requestId != _pendingTestRequestId) {
      if (wasPending) _safeNotifyListeners();
      return; // 过期回执（用户已重测或已关窗）
    }
    _pendingTestRequestId = '';
    _testing = false;
    _lastTestResult = result;
    logInfo(
      _tag,
      'test result: success=${result.success} status=${result.statusCode} '
      'latency=${result.latencyMs}ms',
    );
    _safeNotifyListeners();
  }

  /// 模拟受理回执。回执只说「投出去几个」，**不代表投完了**——真正的等待
  /// 从这里才开始：记下基线，等 `dispatched` 条新记录落库。
  ///
  /// `dispatched == 0` = 没有端点订阅 `task.completed`，不必等（等也等不到）。
  void _onSimulateAck(RustSignalPack<WebhookSimulateAck> pack) {
    _simulateGuard?.cancel();
    _simulateGuard = null;
    final dispatched = pack.message.dispatched;
    _lastSimulateDispatched = dispatched;
    _simulating = false;
    if (dispatched > 0) {
      _deliveryBaseline = {for (final d in _deliveries) d.deliveryId};
      _expectedDeliveries = dispatched;
      // 兜底上限：单个端点最坏 4 次尝试 × 10s 超时 + 2/4/8s 退避 ≈ 54s。
      // 超过还没落库就当它丢了，宁可少显示也不能一直转。
      _deliveryGuard = Timer(const Duration(seconds: 75), () {
        if (_expectedDeliveries > 0) {
          _clearExpectedDeliveries();
          _safeNotifyListeners();
        }
      });
    }
    logInfo(_tag, 'simulate dispatched to $dispatched endpoint(s)');
    _safeNotifyListeners();
  }

  static final Random _rng = Random();

  static String _newRequestId() =>
      '${DateTime.now().microsecondsSinceEpoch}-${_rng.nextInt(1 << 32)}';
}

/// 生成 HMAC 签名密钥（`whsec_` + 32 位十六进制）。
///
/// 只用于给用户一个「够长、够随机」的起点；密钥的强度要求由接收端决定，
/// 用户随时可以改成自己的。
String generateWebhookSecret() {
  final rng = Random.secure();
  final bytes = List<int>.generate(16, (_) => rng.nextInt(256));
  final hex = bytes.map((b) => b.toRadixString(16).padLeft(2, '0')).join();
  return 'whsec_$hex';
}

/// 生成端点 ID（时间戳 + 随机数，无需 uuid 依赖）。
String generateWebhookEndpointId() {
  final rng = Random.secure();
  return 'wh_${DateTime.now().millisecondsSinceEpoch.toRadixString(16)}'
      '${rng.nextInt(1 << 32).toRadixString(16)}';
}
