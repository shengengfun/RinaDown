import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:rinf/rinf.dart';

import '../bindings/bindings.dart';
import '../services/log_service.dart';

/// 插件系统状态（已安装列表 + 插件市场索引）。
///
/// 复刻 [SettingsProvider] 的 ChangeNotifier + rinf 信号订阅模式：构造时
/// 建立信号订阅，`requestPlugins()`/`requestMarket()` 主动拉取，写操作
/// （install/uninstall/setEnabled/saveSettings/ignoreRetry/installMarket）
/// 均为单向 `.sendSignalToRust()`，结果经 [PluginOpResult] 异步回流。
class PluginProvider extends ChangeNotifier {
  List<PluginInfoSignal> _plugins = [];
  List<MarketEntrySignal> _marketEntries = [];
  bool _marketLoading = false;
  String _marketError = '';

  PluginOpResult? _lastOpResult;
  int _opResultSeq = 0;

  PluginAutoDisabledNotice? _lastAutoDisabledNotice;
  int _autoDisabledSeq = 0;

  bool _disposed = false;

  StreamSubscription<RustSignalPack<PluginList>>? _pluginListSub;
  StreamSubscription<RustSignalPack<PluginOpResult>>? _opResultSub;
  StreamSubscription<RustSignalPack<PluginAutoDisabledNotice>>?
  _autoDisabledSub;
  StreamSubscription<RustSignalPack<MarketIndexLoaded>>? _marketSub;

  PluginProvider() {
    logInfo('Plugin', 'constructor');
    _startListening();
  }

  @override
  void dispose() {
    logInfo('Plugin', 'dispose');
    _disposed = true;
    _pluginListSub?.cancel();
    _opResultSub?.cancel();
    _autoDisabledSub?.cancel();
    _marketSub?.cancel();
    super.dispose();
  }

  /// 防止信号在 Provider 已释放后回调触发 "used after being disposed" 异常。
  void _safeNotifyListeners() {
    if (!_disposed) notifyListeners();
  }

  // ---------------------------------------------------------------------------
  // Getters
  // ---------------------------------------------------------------------------

  List<PluginInfoSignal> get plugins => List.unmodifiable(_plugins);
  List<MarketEntrySignal> get marketEntries => List.unmodifiable(_marketEntries);
  bool get marketLoading => _marketLoading;
  String get marketError => _marketError;

  /// 最近一次插件写操作的结果（install/uninstall/set_enabled/save_settings…）。
  PluginOpResult? get lastOpResult => _lastOpResult;

  /// 随每次 [PluginOpResult] 信号单调递增，供调用方判断"是否是新结果"。
  int get opResultSeq => _opResultSeq;

  /// 最近一次熔断器自动禁用通知。
  PluginAutoDisabledNotice? get lastAutoDisabledNotice =>
      _lastAutoDisabledNotice;

  /// 随每次 [PluginAutoDisabledNotice] 信号单调递增。
  int get autoDisabledSeq => _autoDisabledSeq;

  // ---------------------------------------------------------------------------
  // 信号订阅
  // ---------------------------------------------------------------------------

  void _startListening() {
    _pluginListSub = PluginList.rustSignalStream.listen(_onPluginList);
    _opResultSub = PluginOpResult.rustSignalStream.listen(_onOpResult);
    _autoDisabledSub = PluginAutoDisabledNotice.rustSignalStream.listen(
      _onAutoDisabled,
    );
    _marketSub = MarketIndexLoaded.rustSignalStream.listen(_onMarketIndex);
  }

  void _onPluginList(RustSignalPack<PluginList> pack) {
    _plugins = pack.message.plugins;
    logInfo('Plugin', 'plugin list: ${_plugins.length} plugins');
    _safeNotifyListeners();
  }

  void _onOpResult(RustSignalPack<PluginOpResult> pack) {
    _lastOpResult = pack.message;
    _opResultSeq++;
    logInfo(
      'Plugin',
      'op result: op=${pack.message.op} identity=${pack.message.identity} '
          'ok=${pack.message.ok} failedKey=${pack.message.failedKey}',
    );
    _safeNotifyListeners();
  }

  void _onAutoDisabled(RustSignalPack<PluginAutoDisabledNotice> pack) {
    _lastAutoDisabledNotice = pack.message;
    _autoDisabledSeq++;
    logInfo(
      'Plugin',
      'auto disabled: identity=${pack.message.identity} '
          'reason=${pack.message.reason}',
    );
    _safeNotifyListeners();
  }

  void _onMarketIndex(RustSignalPack<MarketIndexLoaded> pack) {
    _marketLoading = false;
    if (pack.message.ok) {
      _marketEntries = pack.message.entries;
      _marketError = '';
    } else {
      _marketError = pack.message.message;
    }
    logInfo(
      'Plugin',
      'market index loaded: ok=${pack.message.ok} '
          'entries=${pack.message.entries.length}',
    );
    _safeNotifyListeners();
  }

  // ---------------------------------------------------------------------------
  // 写操作（均为单向信号，结果经 PluginOpResult / MarketIndexLoaded 异步回流）
  // ---------------------------------------------------------------------------

  /// 请求当前插件列表（app 启动时 / 进入插件设置分类时调用）。
  void requestPlugins() {
    logInfo('Plugin', 'requestPlugins');
    const RequestPlugins().sendSignalToRust();
  }

  /// 安装插件。传 [zipBytes] 走 zip 上传模式；传 [dirPath] + [devMode] 走
  /// 开发目录模式，两者互斥（由调用方决定填哪一组）。
  void install({Uint8List? zipBytes, String dirPath = '', bool devMode = false}) {
    logInfo(
      'Plugin',
      'install: dirPath=$dirPath devMode=$devMode '
          'zipBytes=${zipBytes?.length ?? 0} bytes',
    );
    InstallPlugin(
      zipBytes: zipBytes ?? Uint8List(0),
      dirPath: dirPath,
      devMode: devMode,
    ).sendSignalToRust();
  }

  void uninstall(String identity) {
    logInfo('Plugin', 'uninstall: $identity');
    UninstallPlugin(identity: identity).sendSignalToRust();
  }

  void setEnabled(String identity, bool enabled) {
    logInfo('Plugin', 'setEnabled: $identity=$enabled');
    SetPluginEnabled(identity: identity, enabled: enabled).sendSignalToRust();
  }

  void saveSettings(String identity, Map<String, String> values) {
    logInfo('Plugin', 'saveSettings: $identity (${values.length} entries)');
    SavePluginSettings(
      identity: identity,
      entries: [
        for (final e in values.entries) ConfigEntry(key: e.key, value: e.value),
      ],
    ).sendSignalToRust();
  }

  /// 逃生舱：清除失败任务的插件解析器绑定并恢复下载（跳过插件重新解析）。
  void ignoreRetry(String taskId) {
    logInfo('Plugin', 'ignoreRetry: $taskId');
    IgnorePluginRetry(taskId: taskId).sendSignalToRust();
  }

  /// 请求插件市场索引（进入市场区域时调用）。
  void requestMarket() {
    logInfo('Plugin', 'requestMarket');
    _marketLoading = true;
    _marketError = '';
    _safeNotifyListeners();
    const RequestMarketIndex().sendSignalToRust();
  }

  void installMarket(String pluginId) {
    logInfo('Plugin', 'installMarket: $pluginId');
    InstallMarketPlugin(pluginId: pluginId).sendSignalToRust();
  }
}
