import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:rinf/rinf.dart';

import '../bindings/bindings.dart';
import '../services/log_service.dart';

/// RSS 订阅状态（订阅列表 + 当前选中订阅的条目流 + feed 验证结果）。
///
/// 照 [PluginProvider] 的 ChangeNotifier + rinf 信号订阅模式：构造时建立订阅，
/// 读经 `Request*` 主动拉取，写操作一律单向 `.sendSignalToRust()`，结果经
/// [AllRssSources] / [RssItemsSnapshot] / [RssValidateResult] 异步回流。
///
/// **条目只缓存「当前选中的那一个订阅」**：条目流每源最多 500 条，全量常驻会
/// 让内存随订阅数线性膨胀，而 UI 一次只看得到一个订阅。
class RssProvider extends ChangeNotifier {
  List<RssSourceEntry> _sources = [];
  final Map<String, List<RssItemEntry>> _items = {};

  /// 侧边栏当前选中的订阅（空 = 未选中，主区显示任务列表）。
  String _selectedSourceId = '';

  RssValidateResult? _lastValidateResult;
  int _validateSeq = 0;

  /// 最近一次自动下载的条目标题（供宿主弹一条合批通知）。
  List<String> _lastNotifyTitles = const [];
  int _notifySeq = 0;

  /// 正在抓取中的订阅 → 发起刷新那一刻它的 `lastFetchAt`。
  ///
  /// 抓取是 off-actor 的，从点「立即抓取」到结果回来常有好几秒；没有进行中
  /// 状态用户就分不清「在跑」还是「点了没反应」。用 `lastFetchAt` 前进作为
  /// 完成判据（引擎在成功与失败两条路径上都会回写它并广播）。
  final Map<String, int> _refreshingSince = {};

  /// 正在建任务的条目 → 派发那一刻的 [_itemStamp]（`'sourceId\0guid'` 为键）。
  ///
  /// 与 [_refreshingSince] 同构：完成判据是引擎把结果写回了这条条目，指纹
  /// 一变就解除。
  final Map<String, String> _downloadingItems = {};

  /// 刚提交、等着在下一份订阅快照里认领的 feed 地址（空 = 无待认领）。
  ///
  /// [CreateRssSource] 是单向信号，新订阅的 id 只会随 [AllRssSources] 回流。
  /// 引擎建完订阅**立刻就抓了一轮**，但 UI 这边此刻既不知道 id 也就无从表达
  /// 「在抓」——用户点完「订阅」进去看到的是一条名字还是主机名、写着「尚未
  /// 抓取」的空列表，分不清是在跑、卡住了还是已经失败。先把 URL 记下来，
  /// 快照一到就按 URL 认领，同步点亮抓取态并选中它。
  String _pendingCreateUrl = '';

  /// 我方主动 [RequestRssItems] 的待回执账本。
  final _selfItemsRequests = PendingRequestLedger();

  bool _disposed = false;

  StreamSubscription<RustSignalPack<AllRssSources>>? _sourcesSub;
  StreamSubscription<RustSignalPack<RssItemsSnapshot>>? _itemsSub;
  StreamSubscription<RustSignalPack<RssValidateResult>>? _validateSub;

  RssProvider() {
    logInfo('Rss', 'constructor');
    _startListening();
  }

  @override
  void dispose() {
    logInfo('Rss', 'dispose');
    _disposed = true;
    _sourcesSub?.cancel();
    _itemsSub?.cancel();
    _validateSub?.cancel();
    super.dispose();
  }

  void _safeNotifyListeners() {
    if (!_disposed) notifyListeners();
  }

  // ---------------------------------------------------------------------------
  // Getters
  // ---------------------------------------------------------------------------

  List<RssSourceEntry> get sources => List.unmodifiable(_sources);

  /// 是否有任何订阅（决定侧边栏区块是否给「空态引导」）。
  bool get hasSources => _sources.isNotEmpty;

  /// 抓取异常的订阅数（`lastError` 非空），供状态栏汇总。
  int get unhealthyCount => _sources.where((s) => s.lastError.isNotEmpty).length;

  String get selectedSourceId => _selectedSourceId;

  RssSourceEntry? get selectedSource {
    for (final s in _sources) {
      if (s.sourceId == _selectedSourceId) return s;
    }
    return null;
  }

  /// 当前选中订阅的条目流（新→旧）。未拉取时为空表。
  List<RssItemEntry> get selectedItems =>
      List.unmodifiable(_items[_selectedSourceId] ?? const []);

  RssValidateResult? get lastValidateResult => _lastValidateResult;

  /// 随每次 [RssValidateResult] 单调递增，供对话框判断「是不是我等的那次」。
  int get validateSeq => _validateSeq;

  List<String> get lastNotifyTitles => _lastNotifyTitles;

  /// 随每次「有条目被自动下载」单调递增，供 HomePage 去重弹通知。
  int get notifySeq => _notifySeq;

  // ---------------------------------------------------------------------------
  // 信号订阅
  // ---------------------------------------------------------------------------

  void _startListening() {
    _sourcesSub = AllRssSources.rustSignalStream.listen(_onSources);
    _itemsSub = RssItemsSnapshot.rustSignalStream.listen(_onItems);
    _validateSub = RssValidateResult.rustSignalStream.listen(_onValidate);
  }

  void _onSources(RustSignalPack<AllRssSources> pack) {
    _sources = pack.message.sources;
    // 选中的订阅被删除（或从别的端删掉）→ 收回选中态，避免主区停在空壳上。
    if (_selectedSourceId.isNotEmpty &&
        !_sources.any((s) => s.sourceId == _selectedSourceId)) {
      _selectedSourceId = '';
    }
    _refreshingSince.removeWhere(
      (id, since) => rssFetchSettled(
        _sources.where((s) => s.sourceId == id).firstOrNull,
        since,
      ),
    );
    _claimPendingCreate();
    logInfo('Rss', 'sources: ${_sources.length}');
    _safeNotifyListeners();
  }

  /// 认领刚创建的订阅：选中它、拉条目流，并在首轮抓取尚未落地时点亮抓取态。
  ///
  /// 判「还没落地」看的是**有没有结果**（成功时间与错误都空），而不是
  /// `lastFetchAt == 0`：引擎在派发那一刻就乐观地把 `lastFetchAt` 置成了当前
  /// 时间（防止 due 判定把失败源变成每 tick 重试的死循环），拿它当判据会漏掉
  /// 「已派发、结果未回」这一整段——恰恰是要显示转圈的那一段。
  void _claimPendingCreate() {
    if (_pendingCreateUrl.isEmpty) return;
    final created = _sources
        .where((s) => s.url == _pendingCreateUrl)
        .firstOrNull;
    if (created == null) return;
    _pendingCreateUrl = '';
    _selectedSourceId = created.sourceId;
    _requestItemsTracked(created.sourceId);
    if (rssFirstFetchPending(created)) {
      _refreshingSince[created.sourceId] = created.lastFetchAt;
      Timer(
        const Duration(seconds: 45),
        () => _clearRefreshing(created.sourceId),
      );
    }
  }

  void _onItems(RustSignalPack<RssItemsSnapshot> pack) {
    final msg = pack.message;
    _items[msg.sourceId] = msg.items;
    // 手动下载的完成判据：引擎真的动过这条条目（状态或任务回链变了）。
    if (_downloadingItems.isNotEmpty) {
      for (final item in msg.items) {
        final key = _itemKey(msg.sourceId, item.guid);
        final before = _downloadingItems[key];
        if (before != null && before != _itemStamp(item)) {
          _downloadingItems.remove(key);
        }
      }
    }
    // 只有**抓取落地后的广播**才意味着这一轮结束；我们自己 RequestRssItems 的
    // 回执什么都不意味着，它会在毫秒内返回，正好赶在首轮抓取之前，不区分就会
    // 立刻把刚点亮的「抓取中」熄掉，用户看到的仍是一片空列表。
    if (!_selfItemsRequests.consume(msg.sourceId)) {
      // 同秒二次刷新时 lastFetchAt 可能不变，这里是第二重解除。
      _refreshingSince.remove(msg.sourceId);
    }
    if (msg.notifyTitles.isNotEmpty) {
      _lastNotifyTitles = msg.notifyTitles;
      _notifySeq++;
    }
    logInfo(
      'Rss',
      'items: source=${msg.sourceId} count=${msg.items.length} '
          'notify=${msg.notifyTitles.length}',
    );
    _safeNotifyListeners();
  }

  void _onValidate(RustSignalPack<RssValidateResult> pack) {
    _lastValidateResult = pack.message;
    _validateSeq++;
    logInfo(
      'Rss',
      'validate: url=${pack.message.url} items=${pack.message.items.length} '
          'error=${pack.message.error}',
    );
    _safeNotifyListeners();
  }

  // ---------------------------------------------------------------------------
  // 读
  // ---------------------------------------------------------------------------

  /// 请求全部订阅（App 启动时调用）。
  void requestSources() {
    logInfo('Rss', 'requestSources');
    const RequestAllRssSources().sendSignalToRust();
  }

  /// 选中一个订阅（空串 = 取消选中，主区退回任务列表）。选中即拉取条目流。
  void select(String sourceId) {
    if (_selectedSourceId == sourceId) return;
    _selectedSourceId = sourceId;
    if (sourceId.isNotEmpty) {
      _requestItemsTracked(sourceId);
    }
    _safeNotifyListeners();
  }

  /// 主动重新拉取某订阅的条目流（管理对话框打开时用来喂预览区）。
  void requestItems(String sourceId) {
    if (sourceId.isEmpty) return;
    _requestItemsTracked(sourceId);
  }

  /// 发一次条目流请求并记账，供 [_onItems] 把回执与抓取广播区分开。
  void _requestItemsTracked(String sourceId) {
    _selfItemsRequests.record(sourceId);
    RequestRssItems(sourceId: sourceId).sendSignalToRust();
  }

  /// 已缓存的条目（管理对话框的规则预览直接吃这份，不额外请求）。
  List<RssItemEntry> itemsOf(String sourceId) =>
      List.unmodifiable(_items[sourceId] ?? const []);

  // ---------------------------------------------------------------------------
  // 写（单向信号；结果经 AllRssSources / RssItemsSnapshot 回流）
  // ---------------------------------------------------------------------------

  void create(RssSourceEntry source) {
    logInfo('Rss', 'create: ${source.url}');
    final url = source.url;
    _pendingCreateUrl = url;
    CreateRssSource(source: source).sendSignalToRust();
    // 认领窗口有上限：URL 非法等原因导致引擎压根没建成时，这条待认领记录不能
    // 一直挂着——否则日后（哪怕是另一端）出现同 URL 的订阅会被它误认领。
    Timer(const Duration(seconds: 20), () {
      if (_pendingCreateUrl == url) _pendingCreateUrl = '';
    });
  }

  void update(RssSourceEntry source) {
    logInfo('Rss', 'update: ${source.sourceId}');
    UpdateRssSource(source: source).sendSignalToRust();
  }

  void remove(String sourceId) {
    logInfo('Rss', 'delete: $sourceId');
    DeleteRssSource(sourceId: sourceId).sendSignalToRust();
    if (_selectedSourceId == sourceId) {
      _selectedSourceId = '';
      _safeNotifyListeners();
    }
  }

  void refresh(String sourceId) {
    logInfo('Rss', 'refresh: $sourceId');
    if (_refreshingSince.containsKey(sourceId)) return;
    _refreshingSince[sourceId] =
        _sources
            .where((s) => s.sourceId == sourceId)
            .map((s) => s.lastFetchAt)
            .firstOrNull ??
        0;
    RefreshRssSource(sourceId: sourceId).sendSignalToRust();
    _safeNotifyListeners();
    // 兜底解除：订阅被删、引擎丢消息或同秒二次刷新导致完成判据失效时，
    // 也不能让按钮永久转圈。
    Timer(const Duration(seconds: 45), () => _clearRefreshing(sourceId));
  }

  /// 该订阅是否正在抓取中（供按钮显示 spinner / 禁用）。
  bool isRefreshing(String sourceId) => _refreshingSince.containsKey(sourceId);

  void _clearRefreshing(String sourceId) {
    if (_refreshingSince.remove(sourceId) != null) _safeNotifyListeners();
  }

  /// 验证一个 feed 地址。[requestId] 由调用方生成，用来把结果配回自己的对话框。
  void validate({
    required String requestId,
    required String url,
    String cookies = '',
    String userAgent = '',
    String proxyUrl = '',
  }) {
    logInfo('Rss', 'validate: $url');
    ValidateRssFeed(
      requestId: requestId,
      url: url,
      cookies: cookies,
      userAgent: userAgent,
      proxyUrl: proxyUrl,
    ).sendSignalToRust();
  }

  /// 手动下载一个条目（绕过规则与剧集去重）。
  ///
  /// 派发后进入「准备中」，直到引擎把结果写回条目为止——手动下载不是一次
  /// 本地状态翻转：引擎要去抓 `.torrent`（Mikan 这类站点常要好几秒）、解析、
  /// 再建任务。期间没有任何反馈，用户只会反复点同一行。
  void downloadItem(String sourceId, String guid) {
    final key = _itemKey(sourceId, guid);
    if (_downloadingItems.containsKey(key)) return;
    final current = (_items[sourceId] ?? const <RssItemEntry>[])
        .where((i) => i.guid == guid)
        .firstOrNull;
    _downloadingItems[key] = current == null ? '' : _itemStamp(current);
    SetRssItemAction(sourceId: sourceId, guid: guid, action: 0)
        .sendSignalToRust();
    _safeNotifyListeners();
    // 兜底解除：种子抓取失败时条目会原样留在 New（引擎刻意不把 .torrent 当
    // 普通文件下下来），没有任何状态变化可等——不能让这一行永远转圈。
    Timer(const Duration(seconds: 45), () => _clearDownloading(key));
  }

  /// 该条目是否正在建任务（供按钮显示 spinner / 禁用）。
  bool isItemDownloading(String sourceId, String guid) =>
      _downloadingItems.containsKey(_itemKey(sourceId, guid));

  void _clearDownloading(String key) {
    if (_downloadingItems.remove(key) != null) _safeNotifyListeners();
  }

  static String _itemKey(String sourceId, String guid) =>
      '$sourceId\u0000$guid';

  /// 「这条条目有没有被引擎动过」的指纹。
  ///
  /// 重新下载时 `status` 仍是 `downloaded` 不变，但 `taskId` 会换成新任务，
  /// 所以两者都要看。
  static String _itemStamp(RssItemEntry i) => '${i.status}/${i.taskId}';

  /// 忽略一个条目。
  void ignoreItem(String sourceId, String guid) {
    SetRssItemAction(sourceId: sourceId, guid: guid, action: 1)
        .sendSignalToRust();
  }

  /// 把该订阅的全部「新」条目标记为已读。
  void markAllRead(String sourceId) {
    SetRssItemAction(sourceId: sourceId, guid: '', action: 2)
        .sendSignalToRust();
  }
}

/// 「我方主动请求」的待回执账本。
///
/// 条目快照有两种来路：抓取落地后的广播（= 这一轮结束）与我们自己
/// [RequestRssItems] 的回执（什么都不代表）。两者在信号层长得一模一样，只能
/// 靠这本账区分——不区分就会被毫秒级返回的回执把刚点亮的「抓取中」熄掉。
class PendingRequestLedger {
  final Map<String, int> _counts = {};

  void record(String key) {
    _counts.update(key, (n) => n + 1, ifAbsent: () => 1);
  }

  /// 抵消一笔自请求；账上有才返回 true（此时这份快照不代表抓取结束）。
  bool consume(String key) {
    final pending = _counts[key];
    if (pending == null) return false;
    if (pending <= 1) {
      _counts.remove(key);
    } else {
      _counts[key] = pending - 1;
    }
    return true;
  }
}

/// 这条订阅是否「首轮已派发、结果还没回」。
///
/// 判据是**有没有结果**（成功时间与错误都空），不能看 `lastFetchAt`：引擎在
/// 派发那一刻就乐观地把它置成当前时间（防止 due 判定把失败源变成每 tick 重试
/// 的死循环），拿它当判据会漏掉「已派发、结果未回」这一整段——恰恰是该显示
/// 转圈的那一段。
bool rssFirstFetchPending(RssSourceEntry source) =>
    source.lastSuccessAt == 0 && source.lastError.isEmpty;

/// 这一轮抓取是否已经结束——供从每份订阅快照里回收「抓取中」标记。
///
/// [current] 为 null（订阅被删）一律结束，否则常规判据是 `lastFetchAt` 相对
/// 发起时的 [since] 前进过（成功与失败两条路径都会回写它）。
///
/// 唯一的例外是首轮（`since == 0`）：引擎在派发那一刻就乐观地把 `lastFetchAt`
/// 推到了当前时间，只比对它会在结果回来之前就把转圈熄掉，所以首轮改看「有没有
/// 结果」。
bool rssFetchSettled(RssSourceEntry? current, int since) {
  if (current == null) return true;
  if (since == 0 && rssFirstFetchPending(current)) return false;
  return current.lastFetchAt != since;
}

/// 订阅的展示名：`name` 为空时回退到 **feed 主机名**（而不是整条 URL）。
///
/// 私有 feed 的地址常常是 `https://site/RSS/MyBangumi?token=<40 字符>`，直接
/// 拿整条 URL 当侧边栏标题既放不下也毫无信息量；主机名短、可辨识，鼠标悬浮
/// 仍能看到完整地址。解析失败（用户粘了个非 URL）才退回原串。
String rssDisplayName(RssSourceEntry source) {
  if (source.name.isNotEmpty) return source.name;
  final host = Uri.tryParse(source.url)?.host ?? '';
  return host.isEmpty ? source.url : host;
}
