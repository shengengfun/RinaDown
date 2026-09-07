/// 组件管理状态：ffmpeg + yt-dlp 共用同一套归一化状态与生命周期管理
/// （[ComponentController]），具体信号类型差异下沉到各自子类
/// （[FfmpegController]/[YtdlpController]）。
///
/// ffmpeg/yt-dlp 均为可选的外部工具，由官方源按需下载，不随安装包分发。
library;

import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:rinf/rinf.dart';

import '../bindings/bindings.dart';
import '../services/log_service.dart';

/// config 键：手动指定的 ffmpeg 路径。须与 Rust 端
/// `fluxdown_engine::components::CONFIG_FFMPEG_PATH` 保持一致。
const kFfmpegManualPathConfigKey = 'component.ffmpeg.path';

/// config 键：手动指定的 yt-dlp 路径。须与 Rust 端
/// `fluxdown_engine::components::CONFIG_YTDLP_PATH` 保持一致。
const kYtdlpManualPathConfigKey = 'component.ytdlp.path';

/// 组件（ffmpeg/yt-dlp 等）状态管理的公共基类。
///
/// 复刻 [PluginProvider]（见 `plugin_provider.dart`）的 ChangeNotifier +
/// rinf 信号订阅模式：`requestStatus()`/`requestVersions()` 主动拉取，
/// 写操作（install/uninstall/saveManualPath）均为单向
/// `.sendSignalToRust()`，结果经具体组件的信号类型异步回流，由子类转换
/// 后写入本类的归一化字段（[applyStatus]/[applyVersions]/
/// [applyProgress]/[applyResult]）。手动路径的当前值借道全局
/// [ConfigLoaded] 信号读取（由应用启动时既有的 `RequestConfig` 触发），
/// 不重复发起整表配置拉取。
///
/// 子类需要：提供 [logTag]/[manualPathConfigKey]；实现
/// [startListening]/[cancelSubscriptions] 建立/释放 4 条信号流订阅并把
/// 收到的信号转换成 [applyStatus] 等归一化调用；实现
/// [sendRequestStatus]/[sendRequestVersions]/[sendInstall]/
/// [sendUninstall] 4 个写操作；并在自己的构造函数体内调用
/// [initListening]（基类构造函数不做虚方法派发，避免子类字段尚未就绪时
/// 被回调）。
abstract class ComponentController extends ChangeNotifier {
  String _source = '';
  String _path = '';
  String _version = '';
  String _managedVersion = '';
  String _systemPath = '';
  bool _hasStatus = false;
  bool _managedSupportedRaw = true;
  bool _statusLoading = false;

  String _manualPath = '';

  List<String> _versions = [];
  String _latestStable = '';
  bool _versionsLoading = false;
  String _versionsError = '';

  bool _installing = false;
  int _downloadedBytes = 0;
  int _totalBytes = 0;

  bool? _lastResultOk;
  String _lastResultMessage = '';
  int _installResultSeq = 0;

  bool _disposed = false;

  StreamSubscription<RustSignalPack<ConfigLoaded>>? _configSub;

  /// 日志标签（如 'ffmpeg'/'yt-dlp'），用于区分本实例的日志输出。
  String get logTag;

  /// config 键：手动指定路径。须与对应 Rust 端常量保持一致。
  String get manualPathConfigKey;

  /// 建立本组件的信号流订阅（状态/版本/进度/结果 4 条；[ConfigLoaded]
  /// 由基类统一管理，无需在此订阅）。
  @protected
  void startListening();

  /// 释放 [startListening] 建立的订阅。
  @protected
  void cancelSubscriptions();

  /// 发起状态探测信号。
  @protected
  void sendRequestStatus();

  /// 发起可安装版本列表信号。
  @protected
  void sendRequestVersions();

  /// 发起安装（或更新/重装）信号。
  @protected
  void sendInstall(String version);

  /// 发起卸载信号。
  @protected
  void sendUninstall();

  /// 子类须在自己的构造函数体内调用一次：建立信号订阅并从缓存加载手动
  /// 路径。放在构造函数体而非基类构造函数中，是为了避免在子类自身字段
  /// （如各信号流的 [StreamSubscription] 字段）完成声明前发生虚方法回调。
  @protected
  void initListening() {
    logInfo('Components', '$logTag constructor');
    startListening();
    _loadManualPathFromCache();
    _configSub = ConfigLoaded.rustSignalStream.listen(_onConfigLoaded);
  }

  @override
  void dispose() {
    logInfo('Components', '$logTag dispose');
    _disposed = true;
    cancelSubscriptions();
    _configSub?.cancel();
    super.dispose();
  }

  /// 防止信号在 controller 已释放后回调触发 "used after being disposed" 异常。
  void _safeNotifyListeners() {
    if (!_disposed) notifyListeners();
  }

  // ---------------------------------------------------------------------------
  // Getters（归一化状态，跨组件通用）
  // ---------------------------------------------------------------------------

  /// 是否已收到过至少一次状态回流。
  bool get hasStatus => _hasStatus;

  /// 生效来源（'manual'/'managed'/'system'/'none'）；[hasStatus] 为
  /// false 时为空串。
  String get source => _source;

  /// 生效路径；[hasStatus] 为 false 时为空串。
  String get path => _path;

  /// 生效版本号；[hasStatus] 为 false 时为空串。
  String get version => _version;

  /// 当前托管安装的版本号（未安装为空串）。
  String get managedVersion => _managedVersion;

  /// 系统 PATH 探测到的路径（未找到为空串）。
  String get systemPath => _systemPath;

  bool get statusLoading => _statusLoading;

  /// 当前平台是否提供托管安装（如 macOS 的 ffmpeg 为 false）。状态回流
  /// 前默认 `true`，避免首帧误隐藏托管安装区；真正判定在状态到达后。
  bool get managedSupported => _hasStatus ? _managedSupportedRaw : true;

  /// 用户当前保存的手动路径（空 = 未设置）。与生效路径独立——手动路径
  /// 失效（文件不存在）时生效来源会回退，但此值仍展示用户的原始输入。
  String get manualPath => _manualPath;

  List<String> get versions => List.unmodifiable(_versions);
  String get latestStable => _latestStable;
  bool get versionsLoading => _versionsLoading;
  String get versionsError => _versionsError;

  bool get installing => _installing;
  int get downloadedBytes => _downloadedBytes;
  int get totalBytes => _totalBytes;

  /// 最近一次安装/卸载操作是否成功；尚未收到过结果时为 null。
  bool? get lastResultOk => _lastResultOk;

  /// 最近一次安装/卸载操作的结果消息。
  String get lastResultMessage => _lastResultMessage;

  /// 随每次安装/卸载结果信号单调递增，供调用方判断“是否是新结果”。
  int get installResultSeq => _installResultSeq;

  // ---------------------------------------------------------------------------
  // 子类信号处理回调写入归一化状态
  // ---------------------------------------------------------------------------

  @protected
  void applyStatus({
    required String source,
    required String path,
    required String version,
    required String managedVersion,
    required String systemPath,
    required bool managedSupported,
  }) {
    _hasStatus = true;
    _source = source;
    _path = path;
    _version = version;
    _managedVersion = managedVersion;
    _systemPath = systemPath;
    _managedSupportedRaw = managedSupported;
    _statusLoading = false;
    logInfo(
      'Components',
      '$logTag status: source=$source version=$version path=$path',
    );
    _safeNotifyListeners();
  }

  @protected
  void applyVersions({
    required bool ok,
    required String message,
    required List<String> versions,
    required String latestStable,
  }) {
    _versionsLoading = false;
    if (ok) {
      _versions = versions;
      _latestStable = latestStable;
      _versionsError = '';
    } else {
      _versions = [];
      _latestStable = '';
      _versionsError = message;
    }
    logInfo('Components', '$logTag versions: ok=$ok count=${versions.length}');
    _safeNotifyListeners();
  }

  @protected
  void applyProgress({
    required int downloadedBytes,
    required int totalBytes,
  }) {
    _installing = true;
    _downloadedBytes = downloadedBytes;
    _totalBytes = totalBytes;
    _safeNotifyListeners();
  }

  @protected
  void applyResult({required bool ok, required String message}) {
    _installing = false;
    _downloadedBytes = 0;
    _totalBytes = 0;
    _lastResultOk = ok;
    _lastResultMessage = message;
    _installResultSeq++;
    logInfo('Components', '$logTag install result: ok=$ok message=$message');
    _safeNotifyListeners();
  }

  // ---------------------------------------------------------------------------
  // 手动路径（借道全局 ConfigLoaded 信号）
  // ---------------------------------------------------------------------------

  void _loadManualPathFromCache() {
    final cached = ConfigLoaded.latestRustSignal?.message;
    if (cached != null) _applyManualPathFromEntries(cached.entries);
  }

  void _onConfigLoaded(RustSignalPack<ConfigLoaded> pack) {
    _applyManualPathFromEntries(pack.message.entries);
    _safeNotifyListeners();
  }

  void _applyManualPathFromEntries(List<ConfigEntry> entries) {
    for (final e in entries) {
      if (e.key == manualPathConfigKey) {
        _manualPath = e.value;
        return;
      }
    }
  }

  // ---------------------------------------------------------------------------
  // 写操作（均为单向信号，结果经上述信号异步回流）
  // ---------------------------------------------------------------------------

  /// 请求当前状态（进入组件设置分类时调用）。
  void requestStatus() {
    logInfo('Components', '$logTag requestStatus');
    _statusLoading = true;
    _safeNotifyListeners();
    sendRequestStatus();
  }

  /// 请求可安装版本列表（懒加载：首次展开安装区时调用）。
  void requestVersions() {
    logInfo('Components', '$logTag requestVersions');
    _versionsLoading = true;
    _versionsError = '';
    _safeNotifyListeners();
    sendRequestVersions();
  }

  /// 安装（或更新/重装）托管组件。[version] 空 = 最新稳定版。
  void install(String version) {
    logInfo('Components', '$logTag install: version=$version');
    _installing = true;
    _downloadedBytes = 0;
    _totalBytes = 0;
    _safeNotifyListeners();
    sendInstall(version);
  }

  /// 卸载托管组件（手动/系统路径不受影响）。
  void uninstall() {
    logInfo('Components', '$logTag uninstall');
    sendUninstall();
  }

  /// 保存手动指定路径（空串 = 清除）；写入后重新探测状态。
  void saveManualPath(String path) {
    logInfo('Components', '$logTag saveManualPath: $path');
    _manualPath = path;
    _safeNotifyListeners();
    SaveConfig(key: manualPathConfigKey, value: path).sendSignalToRust();
    requestStatus();
  }
}

/// ffmpeg 组件状态管理。行为等价于重构前的 `ComponentsProvider`。
class FfmpegController extends ComponentController {
  StreamSubscription<RustSignalPack<FfmpegStatusReport>>? _statusSub;
  StreamSubscription<RustSignalPack<FfmpegVersionList>>? _versionsSub;
  StreamSubscription<RustSignalPack<FfmpegInstallProgress>>? _progressSub;
  StreamSubscription<RustSignalPack<FfmpegInstallResult>>? _resultSub;

  FfmpegController() {
    initListening();
  }

  @override
  String get logTag => 'ffmpeg';

  @override
  String get manualPathConfigKey => kFfmpegManualPathConfigKey;

  @override
  void startListening() {
    _statusSub = FfmpegStatusReport.rustSignalStream.listen(_onStatus);
    _versionsSub = FfmpegVersionList.rustSignalStream.listen(_onVersions);
    _progressSub = FfmpegInstallProgress.rustSignalStream.listen(_onProgress);
    _resultSub = FfmpegInstallResult.rustSignalStream.listen(_onResult);
  }

  @override
  void cancelSubscriptions() {
    _statusSub?.cancel();
    _versionsSub?.cancel();
    _progressSub?.cancel();
    _resultSub?.cancel();
  }

  void _onStatus(RustSignalPack<FfmpegStatusReport> pack) {
    final r = pack.message;
    applyStatus(
      source: r.source,
      path: r.path,
      version: r.version,
      managedVersion: r.managedVersion,
      systemPath: r.systemPath,
      managedSupported: r.managedSupported,
    );
  }

  void _onVersions(RustSignalPack<FfmpegVersionList> pack) {
    final r = pack.message;
    applyVersions(
      ok: r.ok,
      message: r.message,
      versions: r.versions,
      latestStable: r.latestStable,
    );
  }

  void _onProgress(RustSignalPack<FfmpegInstallProgress> pack) {
    applyProgress(
      downloadedBytes: pack.message.downloadedBytes,
      totalBytes: pack.message.totalBytes,
    );
  }

  void _onResult(RustSignalPack<FfmpegInstallResult> pack) {
    applyResult(ok: pack.message.ok, message: pack.message.message);
  }

  @override
  void sendRequestStatus() => const RequestFfmpegStatus().sendSignalToRust();

  @override
  void sendRequestVersions() =>
      const RequestFfmpegVersions().sendSignalToRust();

  @override
  void sendInstall(String version) =>
      InstallFfmpeg(version: version).sendSignalToRust();

  @override
  void sendUninstall() => const UninstallFfmpeg().sendSignalToRust();
}

/// yt-dlp 组件状态管理，用法与 [FfmpegController] 完全对称。
///
/// yt-dlp 从 1000+ 站点提取媒体直链，供 FluxDown 插件使用；全平台
/// （含 macOS）均支持托管安装。
class YtdlpController extends ComponentController {
  StreamSubscription<RustSignalPack<YtdlpStatusReport>>? _statusSub;
  StreamSubscription<RustSignalPack<YtdlpVersionList>>? _versionsSub;
  StreamSubscription<RustSignalPack<YtdlpInstallProgress>>? _progressSub;
  StreamSubscription<RustSignalPack<YtdlpInstallResult>>? _resultSub;

  YtdlpController() {
    initListening();
  }

  @override
  String get logTag => 'yt-dlp';

  @override
  String get manualPathConfigKey => kYtdlpManualPathConfigKey;

  @override
  void startListening() {
    _statusSub = YtdlpStatusReport.rustSignalStream.listen(_onStatus);
    _versionsSub = YtdlpVersionList.rustSignalStream.listen(_onVersions);
    _progressSub = YtdlpInstallProgress.rustSignalStream.listen(_onProgress);
    _resultSub = YtdlpInstallResult.rustSignalStream.listen(_onResult);
  }

  @override
  void cancelSubscriptions() {
    _statusSub?.cancel();
    _versionsSub?.cancel();
    _progressSub?.cancel();
    _resultSub?.cancel();
  }

  void _onStatus(RustSignalPack<YtdlpStatusReport> pack) {
    final r = pack.message;
    applyStatus(
      source: r.source,
      path: r.path,
      version: r.version,
      managedVersion: r.managedVersion,
      systemPath: r.systemPath,
      managedSupported: r.managedSupported,
    );
  }

  void _onVersions(RustSignalPack<YtdlpVersionList> pack) {
    final r = pack.message;
    applyVersions(
      ok: r.ok,
      message: r.message,
      versions: r.versions,
      latestStable: r.latestStable,
    );
  }

  void _onProgress(RustSignalPack<YtdlpInstallProgress> pack) {
    applyProgress(
      downloadedBytes: pack.message.downloadedBytes,
      totalBytes: pack.message.totalBytes,
    );
  }

  void _onResult(RustSignalPack<YtdlpInstallResult> pack) {
    applyResult(ok: pack.message.ok, message: pack.message.message);
  }

  @override
  void sendRequestStatus() => const RequestYtdlpStatus().sendSignalToRust();

  @override
  void sendRequestVersions() =>
      const RequestYtdlpVersions().sendSignalToRust();

  @override
  void sendInstall(String version) =>
      InstallYtdlp(version: version).sendSignalToRust();

  @override
  void sendUninstall() => const UninstallYtdlp().sendSignalToRust();
}
