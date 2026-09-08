/// 悬浮球统一服务（方案 S0.5）— 进程生命周期，与 TrayService 同级。
///
/// ## 职责
/// - enable()/disable() 状态机（幂等，布尔在途锁）
/// - 平台分发：Windows = FFI 分层窗口；macOS/Linux = MethodChannel 推位图
/// - 载荷分发：URL → QuickDownloadDialog 预填；.torrent → torrent 流程；
///   其他本地文件 → 丢弃并 logInfo
/// - 挂起/唤醒：Dart 心跳 Timer 检测墙钟跳变 >30s → 全量重绘 + 坐标校验
///
/// ## MethodChannel 协议（A6，`com.fluxdown/floating_ball`）
/// Dart→原生：pushBitmap / showBall / hideBall / destroyBall / registerDropTarget
/// 原生→Dart：onDropPayload / onBallClicked / onBallMoved / onCapability
library;

import 'dart:async';
import 'dart:io';
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:window_manager/window_manager.dart';

import '../../i18n/locale_provider.dart';
import '../../models/download_controller.dart';
import '../../models/download_task.dart';
import '../../models/settings_provider.dart';
import '../../theme/theme_provider.dart';
import '../../widgets/quick_download_dialog.dart';
import '../app_icon_service.dart';
import '../log_service.dart';
import '../tray_service.dart';
import 'floating_ball_controller.dart';
import 'floating_ball_renderer.dart';
import 'wayland_degradation_service.dart';
import 'win32_ball_window.dart';

const _tag = 'FloatBall';

/// URL 识别正则（同 new_download_dialog.dart 先例）
final _urlRegex = RegExp(r'(https?|ftp)://\S+', caseSensitive: false);

/// Linux 能力探测结果
enum LinuxBallCapability { unknown, x11, wayland }

class FloatingBallService {
  FloatingBallService._();
  static final instance = FloatingBallService._();

  static const _channel = MethodChannel('com.fluxdown/floating_ball');

  SettingsProvider? _settings;
  ThemeProvider? _theme;
  GlobalKey<NavigatorState>? _navigatorKey;
  FloatingBallController? _controller;

  bool _enabled = false;
  bool _transitioning = false; // 在途锁（防快速反复切换交叠）
  LinuxBallCapability _linuxCapability = LinuxBallCapability.unknown;

  // 原生窗口当前可见性（仅下 activeOnly 模式下有意义；避免重复 show/hide 调用）。
  bool _nativeVisible = false;

  // 唤醒检测（S0.5：墙钟跳变 >30s = 唤醒）
  Timer? _heartbeat;
  DateTime _lastBeat = DateTime.now();

  // 位图缓存（A7 缓存键：variant + themeGeneration + dpiScale）
  final Map<String, BallImage> _staticCache = {};

  // ── hover 卡片（Windows 展开「正在下载」列表）──
  bool _hovering = false; // 光标在球/卡片上（来自 Win32BallWindow.onHoverChanged）
  bool _cardOpen = false; // 卡片是否展示中
  bool _collapsePending = false; // 收起过渡在途（防中间态帧盖错几何）
  List<String> _cardNames = const []; // 卡片上次渲染的文件名（去重/顺序比较用）
  List<String> _cardTaskIds = const []; // 卡片当前可见行的 taskId（与行序一致）

  /// 有效设置实例 — 优先 globalInstance（HomePage 主实例，设置页读写的
  /// 就是它）；init 注入的 _settingsForExternal 仅作 fallback。
  /// 双实例不同步是设置页 switch 不刷新的根因（用户实测反馈）。
  SettingsProvider? get _effectiveSettings =>
      SettingsProvider.globalInstance ?? _settings;

  /// Linux Wayland 会话下悬浮球不可用（S3.4 降级）。
  bool get isDegraded =>
      Platform.isLinux && _linuxCapability == LinuxBallCapability.wayland;

  LinuxBallCapability get linuxCapability => _linuxCapability;

  /// 初始化 — 必须在 SettingsProvider 配置加载完成后调用（S0.5 初始化钩子）。
  void init({
    required SettingsProvider settings,
    required ThemeProvider theme,
    required GlobalKey<NavigatorState> navigatorKey,
  }) {
    _settings = settings;
    _theme = theme;
    _navigatorKey = navigatorKey;
    _channel.setMethodCallHandler(_onNativeCall);

    if (settings.floatingBallEnabled) {
      // Linux 需先等 onCapability；Windows/macOS 直接启用
      if (Platform.isLinux) {
        _requestLinuxCapability();
      } else {
        enable();
      }
    }
    logInfo(_tag, 'init done, enabled=${settings.floatingBallEnabled}');
  }

  // ===========================================================================
  // enable / disable 状态机
  // ===========================================================================

  Future<void> enable() async {
    if (_enabled || _transitioning) return;
    final settings = _settings;
    final theme = _theme;
    if (settings == null || theme == null) {
      logError(_tag, 'enable() before init()');
      return;
    }
    final downloads = DownloadController.globalInstance;
    if (downloads == null) {
      logInfo(_tag, 'enable() deferred: DownloadController not ready');
      return;
    }
    if (isDegraded) {
      logInfo(_tag, 'enable() skipped: wayland degraded mode');
      return;
    }
    _transitioning = true;
    try {
      // 0. 预解码 logo（幂等；idle 态球心图标，跟随应用图标自定义）
      await ensureBallLogoLoaded();
      AppIconService.instance.addListener(_onAppIconChanged);

      // 1. 订阅数据层
      _controller = FloatingBallController(downloads: downloads, theme: theme)
        ..addListener(_onDataChanged);

      // 2. 读坐标（哨兵 -1 → 默认停靠）→ 校验
      final (x, y) = _resolvePosition();

      // 3. 目标可见性：activeOnly 且当前无活跃任务 → 建资源但不显示（避免闪烁）
      final wantVisible =
          !_activeOnly || (_controller?.state.isActive ?? false);

      // 4. 创建窗口
      if (Platform.isWindows) {
        final win = Win32BallWindow.instance
          ..onClicked = _onBallClicked
          ..onMoved = _onBallMoved
          ..onContextMenu = _onBallContextMenu
          ..onHoverChanged = _onBallHoverChanged
          ..onCardPressed = _onBallCardPressed
          ..onCardRowPressed = _onHoverCardRowPressed
          ..onDpiChanged = (_) => _rerenderAll();
        win.create(x: x, y: y);
        await _renderAndPush(); // 未 show 也先备好位图，show 瞬间无空白帧
        if (wantVisible) win.show();
        // 5. C++ 侧注册 IDropTarget（S1.2）。失败仅降级（球保留展示/
        // 点击/拖动，仅拖放不可用）—— 不拆整球。隐藏态注册无害（显示后即可用）。
        try {
          await _channel.invokeMethod('registerDropTarget', {
            'hwnd': Win32BallWindow.instance.hwnd,
          });
        } catch (e) {
          logError(_tag, 'registerDropTarget failed (drop disabled)', e);
        }
      } else {
        // 非 Windows：showBall 兼建窗+显示；隐藏态整体跳过，re-show 时再建。
        if (wantVisible) {
          await _channel.invokeMethod('showBall', {
            'x': x.toDouble(),
            'y': y.toDouble(),
          });
          await _renderAndPush();
        }
      }
      _nativeVisible = wantVisible;

      // 6. 心跳（唤醒检测）
      _startHeartbeat();

      _enabled = true;
      logInfo(_tag, 'enabled at ($x,$y)');
    } catch (e, stack) {
      logError(_tag, 'enable failed', e, stack);
      _teardown();
    } finally {
      _transitioning = false;
    }
  }

  Future<void> disable() async {
    if (!_enabled || _transitioning) return;
    _transitioning = true;
    try {
      _teardown();
      logInfo(_tag, 'disabled');
    } finally {
      _transitioning = false;
    }
  }

  /// 应用退出收口（S4.4 — _performGracefulExit 调用）。
  void destroy() {
    _teardown();
    _channel.setMethodCallHandler(null);
    logInfo(_tag, 'destroy: done');
  }

  void _teardown() {
    _heartbeat?.cancel();
    _heartbeat = null;
    _waveTicker?.cancel();
    _waveTicker = null;
    _displayLevel = 0;
    _downloadWasActive = false;
    AppIconService.instance.removeListener(_onAppIconChanged);
    _controller?.removeListener(_onDataChanged);
    _controller?.dispose();
    _controller = null;
    _staticCache.clear();
    _hovering = false;
    _cardOpen = false;
    _collapsePending = false;
    _cardNames = const [];
    _cardTaskIds = const [];
    if (Platform.isWindows) {
      // RevokeDragDrop 在 C++ destroyBall 分支处理
      if (Win32BallWindow.instance.isCreated) {
        unawaited(
          _channel.invokeMethod('unregisterDropTarget').catchError((Object e) {
            logError(_tag, 'unregisterDropTarget failed', e);
            return null;
          }),
        );
      }
      Win32BallWindow.instance.destroy();
    } else {
      unawaited(
        _channel.invokeMethod('destroyBall').catchError((Object e) {
          logError(_tag, 'destroyBall failed', e);
          return null;
        }),
      );
    }
    _nativeVisible = false;
    _enabled = false;
  }

  /// 设置开关变更入口（设置页/托盘菜单调用）。
  void setEnabled(bool value) {
    _effectiveSettings?.setFloatingBallEnabled(value);
    if (value) {
      if (Platform.isLinux && _linuxCapability == LinuxBallCapability.unknown) {
        _requestLinuxCapability();
      } else {
        unawaited(enable());
      }
    } else {
      unawaited(disable());
    }
  }

  // ===========================================================================
  // 渲染管线
  // ===========================================================================
  void _onDataChanged() {
    if (!_enabled) return;
    // 下载活跃上升沿：水位归零，波浪自底上浮
    final active = _controller?.state.isActive ?? false;
    if (active && !_downloadWasActive) _displayLevel = 0;
    _downloadWasActive = active;
    _applyActiveOnlyVisibility();
    if (!_nativeVisible) return; // 隐藏态无需渲染
    unawaited(_refreshAfterDataChange());
  }

  /// 数据变化后的渲染编排：
  /// - 卡片展开中 → 只刷新/收起卡片（不推小球帧，避免盖掉卡片窗口）
  /// - 否则 → 正常小球渲染（卡片只在悬停进入的上升沿打开，避免数据抖动
  ///   在悬停中反复开合）
  Future<void> _refreshAfterDataChange() async {
    if (_collapsePending) return; // 收起过渡在途：跳过，避免中间态帧盖错几何
    if (_cardOpen) {
      await _renderHoverCardOrCollapse();
      return;
    }
    await _renderAndPush();
  }

  /// 是否处于「仅下载时显示」模式。
  bool get _activeOnly => _effectiveSettings?.floatingBallActiveOnly ?? false;

  /// 依据 activeOnly 设置与当前活跃态，决定原生窗口显隐（幂等）。
  ///
  /// 关闭 activeOnly → 始终显示；开启 → 有活跃任务才显示。
  void _applyActiveOnlyVisibility() {
    if (!_enabled) return;
    final shouldShow = !_activeOnly || (_controller?.state.isActive ?? false);
    if (shouldShow == _nativeVisible) return;
    if (shouldShow) {
      _showNative();
    } else {
      _hideNative();
    }
  }

  /// 设置页切换 activeOnly 后重新评估可见性（隐藏→显示时补一帧渲染）。
  void refreshVisibility() {
    if (!_enabled) return;
    final wasVisible = _nativeVisible;
    _applyActiveOnlyVisibility();
    if (_nativeVisible && !wasVisible) unawaited(_renderAndPush());
  }

  void _showNative() {
    if (Platform.isWindows) {
      Win32BallWindow.instance.show();
    } else {
      final (x, y) = _resolvePosition();
      unawaited(
        _channel.invokeMethod('showBall', {
          'x': x.toDouble(),
          'y': y.toDouble(),
        }).catchError((Object e) {
          logError(_tag, 'showBall failed', e);
          return null;
        }),
      );
    }
    _nativeVisible = true;
  }

  void _hideNative() {
    if (Platform.isWindows) {
      final win = Win32BallWindow.instance;
      // 隐藏前若卡片展开，静默复位几何（隐藏态无图可贴，几何交下次渲染修正）
      if (win.isCardOpen) {
        win.resetCardGeometry();
        _cardOpen = false;
        _cardNames = const [];
      }
      win.hide();
    } else {
      unawaited(
        _channel.invokeMethod('hideBall').catchError((Object e) {
          logError(_tag, 'hideBall failed', e);
          return null;
        }),
      );
    }
    _nativeVisible = false;
  }

  /// 应用图标切换（设置-外观）→ 重载球心 logo 并整体重绘
  void _onAppIconChanged() {
    unawaited(_refreshLogo());
  }

  Future<void> _refreshLogo() async {
    await ensureBallLogoLoaded();
    if (!_enabled) return;
    await _rerenderAll();
  }

  Future<void> _rerenderAll() async {
    // 整体重绘前先收起卡片（主题/DPI/唤醒等触发，几何与缓存一并失效）
    if (_cardOpen) await _collapseHoverCard();
    _staticCache.clear();
    await _renderAndPush();
  }

  bool _renderInFlight = false;
  bool _renderQueued = false;

  // 迅雷风格波浪进度动画状态（active 态由计时器持续驱动重推位图）
  Timer? _waveTicker;
  double _wavePhase = 0;
  double _displayLevel = 0;
  bool _downloadWasActive = false;
  static const Duration _waveFramePeriod = Duration(milliseconds: 55);
  static const double _wavePhaseStep = 0.30;
  static const double _levelEase = 0.16;

  /// 渲染当前状态一帧并推送（串行化：进行中则排队一次）。
  Future<void> _renderAndPush() async {
    _syncWaveTicker();
    if (_renderInFlight) {
      _renderQueued = true;
      return;
    }
    _renderInFlight = true;
    try {
      do {
        _renderQueued = false;
        await _renderOnce();
      } while (_renderQueued);
    } finally {
      _renderInFlight = false;
    }
  }

  /// 启停波浪动画计时器（幂等）：active + 可见 + 非拖放态 + 卡片未展开时运行。
  /// （卡片展开期小球在卡片内定格，波浪暂停以省去整帧重推。）
  void _syncWaveTicker() {
    final shouldAnimate = _enabled &&
        _nativeVisible &&
        !_dragHover &&
        !_cardOpen &&
        (_controller?.state.isActive ?? false);
    if (shouldAnimate) {
      _waveTicker ??= Timer.periodic(_waveFramePeriod, (_) => _onWaveTick());
    } else {
      _waveTicker?.cancel();
      _waveTicker = null;
    }
  }

  /// 波浪帧：推进相位、缓动水位向目标进度靠拢，重推一帧。
  void _onWaveTick() {
    if (!_enabled ||
        !_nativeVisible ||
        _dragHover ||
        _cardOpen ||
        !(_controller?.state.isActive ?? false)) {
      _waveTicker?.cancel();
      _waveTicker = null;
      return;
    }
    _wavePhase += _wavePhaseStep;
    final progress = _controller?.state.activeSpec?.aggregateProgress;
    // 进度未知：水位在中位缓慢起伏，表达“进行中”
    final target = progress ?? (0.5 + 0.14 * math.sin(_wavePhase * 0.45));
    _displayLevel += (target - _displayLevel) * _levelEase;
    unawaited(_renderAndPush());
  }

  /// 渲染当前状态一帧并推送。含卡片收起等重绘入口复用 [pushToNative]。
  Future<void> _renderOnce() async {
    final image = await _buildCurrentBallImage();
    if (image == null) return;
    await pushToNative(image);
  }

  /// 组装当前正常小球位图（不推送）。
  ///
  /// 变体裁决：拖放目标 → 贴边收起？→ active 波浪 / idle。返回 null 表示
  /// 数据未就绪（controller/theme 缺失）。
  Future<BallImage?> _buildCurrentBallImage() async {
    final controller = _controller;
    final theme = _theme;
    if (controller == null || theme == null) return null;

    final scale = Platform.isWindows
        ? Win32BallWindow.instance.scale
        : (_navigatorKey?.currentContext != null
              ? MediaQuery.of(_navigatorKey!.currentContext!).devicePixelRatio
              : 1.0);
    final dark = _isDarkNoContext();
    final tokens = theme.tokensFor(dark: dark);
    final st = controller.state;

    final BallImage image;
    if (_dragHover) {
      image = await _staticVariant(
        BallVariant.dragTarget,
        tokens,
        scale,
        controller.themeGeneration,
      );
    } else if (st.isActive) {
      // 动态层不缓存（数据驱动，每帧内容不同）
      image = await renderBallImage(
        variant: BallVariant.active,
        tokens: tokens,
        scale: scale,
        activeSpec: st.activeSpec,
        wavePhase: _wavePhase,
        waveLevel: _displayLevel,
      );
    } else {
      image = await _staticVariant(
        BallVariant.idle,
        tokens,
        scale,
        controller.themeGeneration,
      );
    }
    return image;
  }

  /// 把一帧位图推到原生层（Windows：分层窗口；其他：MethodChannel）。
  Future<void> pushToNative(BallImage image) async {
    if (Platform.isWindows) {
      Win32BallWindow.instance.pushImage(image);
    } else {
      final scale = _navigatorKey?.currentContext != null
          ? MediaQuery.of(_navigatorKey!.currentContext!).devicePixelRatio
          : 1.0;
      await _channel.invokeMethod('pushBitmap', {
        'bytes': image.rgba,
        'width': image.width,
        'height': image.height,
        'scale': scale,
      });
    }
  }

  Future<BallImage> _staticVariant(
    BallVariant variant,
    tokens,
    double scale,
    int themeGen,
  ) async {
    final key = '${variant.name}#$themeGen@$scale';
    final cached = _staticCache[key];
    if (cached != null) return cached;
    // 主题/DPI 变更 → 整体失效（A7 缓存键裁决）
    _staticCache.removeWhere((k, _) => !k.endsWith('#$themeGen@$scale'));
    final image = await renderBallImage(
      variant: variant,
      tokens: tokens,
      scale: scale,
    );
    _staticCache[key] = image;
    return image;
  }

  // ===========================================================================
  // hover 卡片（Windows「正在下载」文件列表）
  // ===========================================================================

  /// 取当前正在下载/准备/续传任务的 (taskId, fileName) 条目（下载列表数据源，
  /// 同 detail_panel；顺序稳定，卡片行命中测试据此映射任务）。
  List<({String id, String name})> _activeHoverEntries() {
    final downloads = DownloadController.globalInstance;
    if (downloads == null) return const [];
    final entries = <({String id, String name})>[];
    for (final t in downloads.tasks) {
      if (t.status != TaskStatus.downloading &&
          t.status != TaskStatus.preparing &&
          t.status != TaskStatus.resuming) {
        continue;
      }
      final name = t.fileName.trim();
      if (name.isEmpty) continue;
      entries.add((id: t.id, name: name));
    }
    return entries;
  }

  static bool _sameNameList(List<String> a, List<String> b) {
    if (a.length != b.length) return false;
    for (var i = 0; i < a.length; i++) {
      if (a[i] != b[i]) return false;
    }
    return true;
  }

  /// 卡片可展开条件不可用的场景：贴边收起（只剩边条）或贴边滑出/收起
  /// 动画进行中（锚点随动画移动，若此时开卡会把球"冻"在半路）。
  /// 贴边但已完整展开、可正常悬停的球允许弹卡。
  static bool _hoverCardBlocked(Win32BallWindow win) =>
      win.isCollapsed || win.isAnimating;

  /// 悬停进入 → 尝试展开卡片（上升沿打开：数据变化不在此自动重开）。
  Future<void> _onBallHoverChanged(bool hovering) async {
    if (_hovering == hovering) return;
    _hovering = hovering;
    if (!_enabled || !Platform.isWindows) return;
    if (hovering) {
      await _showHoverCard();
    } else {
      await _collapseHoverCard();
    }
  }

  /// 卡片展开态下按下 → 收起卡片（悬停仍在，不重开）。
  void _onBallCardPressed() {
    if (!_enabled) return;
    unawaited(_collapseHoverCard());
  }

  /// 卡片展开态下在某文件行内点击 → 暂停该任务（不收起卡片；任务离活跃集后
  /// 由数据变化驱动刷新/自动收起）。
  void _onHoverCardRowPressed(int rowIndex) {
    if (!_enabled) return;
    if (rowIndex < 0 || rowIndex >= _cardTaskIds.length) return;
    final taskId = _cardTaskIds[rowIndex];
    logInfo(_tag, 'hover card row $rowIndex pressed → pause task $taskId');
    DownloadController.globalInstance?.pauseTask(taskId);
  }

  /// 尝试打开 hover 卡片。条件：Windows + 已建窗 + 非贴边 + 可见 +
  /// 存在下载中文件 + 仍处于悬停。不满足即静默返回。
  Future<void> _showHoverCard() async {
    if (!_enabled || _cardOpen || _collapsePending || !Platform.isWindows) {
      return;
    }
    final win = Win32BallWindow.instance;
    if (!win.isCreated || !_nativeVisible) return;
    if (_hoverCardBlocked(win)) return; // 收起态/动画中不弹卡

    final entries = _activeHoverEntries();
    if (entries.isEmpty) return;
    final allNames = [for (final e in entries) e.name];

    final visibleRows = math.min(entries.length, kHoverCardMaxRows);
    final visible = entries.length > visibleRows
        ? entries.sublist(0, visibleRows)
        : entries;
    final cardSize = hoverCardLogicalSize(visibleRows: visible.length);
    final layout = win.prepareHoverCardLayout(cardSize);
    final image = await _buildHoverCardFrame(
      layout: layout,
      headerText: currentS.fgServiceActiveTitle(entries.length),
      fileNames: [for (final e in visible) e.name],
    );
    if (image == null) return;

    // 渲染间隙状态可能已变：重新校验再提交，避免贴过期帧
    if (!_enabled || !_nativeVisible) return;
    if (!win.isCreated) return;
    if (_hoverCardBlocked(win)) return;
    if (!_hovering || _cardOpen) return;

    win.presentHoverCard(
      image: image,
      layout: layout,
      rowCount: visible.length,
    );
    _cardOpen = true;
    _cardNames = allNames;
    _cardTaskIds = [for (final e in visible) e.id];
    _syncWaveTicker(); // 卡片展开 → 暂停波浪动画
    logInfo(_tag, 'hover card open: ${entries.length} file(s)');
  }

  /// 卡片已展开时的刷新/收起：文件名集变化 → 重渲染；文件清空/离开 →
  /// 收起。仅当 [win.isCardOpen] 为真时由数据变化驱动调用。
  Future<void> _renderHoverCardOrCollapse() async {
    if (!_cardOpen || !_enabled || !Platform.isWindows) return;
    final win = Win32BallWindow.instance;
    if (!win.isCreated || _hoverCardBlocked(win) || !_hovering ||
        !_nativeVisible) {
      await _collapseHoverCard();
      return;
    }
    final entries = _activeHoverEntries();
    if (entries.isEmpty) {
      await _collapseHoverCard();
      return;
    }
    final allNames = [for (final e in entries) e.name];
    if (_sameNameList(_cardNames, allNames)) return; // 无实质变化

    final visibleRows = math.min(entries.length, kHoverCardMaxRows);
    final visible = entries.length > visibleRows
        ? entries.sublist(0, visibleRows)
        : entries;
    final cardSize = hoverCardLogicalSize(visibleRows: visible.length);
    final layout = win.prepareHoverCardLayout(cardSize);
    final image = await _buildHoverCardFrame(
      layout: layout,
      headerText: currentS.fgServiceActiveTitle(entries.length),
      fileNames: [for (final e in visible) e.name],
    );
    if (image == null) return;

    if (!_enabled || !_hovering || !_nativeVisible) return;
    if (!win.isCreated || _hoverCardBlocked(win)) return;
    if (!_cardOpen || _sameNameList(_cardNames, allNames)) return; // 已被并发收/刷

    win.presentHoverCard(
      image: image,
      layout: layout,
      rowCount: visible.length,
    );
    _cardNames = allNames;
    _cardTaskIds = [for (final e in visible) e.id];
    logInfo(_tag, 'hover card refreshed: ${entries.length} file(s)');
  }

  /// 收起卡片：渲染一帧正常小球后原子性恢复球-only 几何并贴入。
  Future<void> _collapseHoverCard() async {
    if (!_enabled || !_cardOpen || _collapsePending) return;
    _collapsePending = true;
    try {
      _cardOpen = false;
      _cardNames = const [];
      _cardTaskIds = const [];
      if (!Platform.isWindows) return;
      final win = Win32BallWindow.instance;
      if (!win.isCreated) return;

      final image = await _buildCurrentBallImage();
      if (image == null) return;
      if (!_enabled || _cardOpen) return; // 收起期间被重新打开 → 保留卡片
      win.dismissHoverCard(image);
      _syncWaveTicker(); // 恢复波浪动画（如仍活跃）
      logInfo(_tag, 'hover card collapsed');
    } finally {
      _collapsePending = false;
    }
  }

  /// 组装「球 + 文件列表卡片」整帧（含当前 active 小球定格）。
  Future<BallImage?> _buildHoverCardFrame({
    required HoverCardLayout layout,
    required String headerText,
    required List<String> fileNames,
  }) async {
    final theme = _theme;
    final controller = _controller;
    if (theme == null) return null;
    if (!Platform.isWindows) return null;
    final scale = Win32BallWindow.instance.scale;
    final dark = _isDarkNoContext();
    final tokens = theme.tokensFor(dark: dark);
    final st = controller?.state;
    final ballSpec = (st != null && st.isActive) ? st.activeSpec : null;
    return renderHoverCardImage(
      tokens: tokens,
      scale: scale,
      layout: layout,
      headerText: headerText,
      fileNames: fileNames,
      ballSpec: ballSpec,
      wavePhase: _wavePhase,
      waveLevel: _displayLevel,
    );
  }

  bool _isDarkNoContext() {
    final ctx = _navigatorKey?.currentContext;
    if (ctx != null && ctx.mounted) {
      return _theme?.isDark(ctx) ?? true;
    }
    return true;
  }

  // ===========================================================================
  // 交互回调
  // ===========================================================================

  bool _dragHover = false;

  void _onBallClicked() {
    unawaited(_restoreMainWindow());
  }

  /// 右键菜单（Windows：原生 TrackPopupMenuEx；macOS/Linux：原生层自行弹出，
  /// 经 onMenuAction 回传）。
  void _onBallContextMenu() {
    if (!Platform.isWindows) return;
    final s = currentS;
    final selected = Win32BallWindow.instance.showContextMenu([
      (1, s.resumeAll),
      (2, s.pauseAll),
      (0, ''),
      (3, s.trayShowWindow),
      (4, s.hideFloatingBall),
      (0, ''),
      (5, s.trayExit),
    ]);
    _dispatchMenuAction(selected);
  }

  void _dispatchMenuAction(int id) {
    final downloads = DownloadController.globalInstance;
    switch (id) {
      case 1:
        downloads?.resumeAll();
      case 2:
        downloads?.pauseAll();
      case 3:
        unawaited(_restoreMainWindow());
      case 4:
        setEnabled(false);
        unawaited(TrayService.instance.refreshMenu()); // 同步托盘复选状态
      case 5:
        // 复用托盘退出链路（onExitApp → _performGracefulExit）
        final exit = TrayService.instance.onExitApp;
        if (exit != null) unawaited(exit());
      default:
        break; // 0 = 取消
    }
  }

  Future<void> _restoreMainWindow() async {
    try {
      final visible = await windowManager.isVisible();
      await restoreMainWindow();
      logInfo(_tag, 'main window restored (wasVisible=$visible)');
    } catch (e, stack) {
      logError(_tag, 'restore main window failed', e, stack);
    }
  }

  void _onBallMoved(double x, double y) {
    // 转场期垃圾坐标防护（S0.5：-32000 类值不落盘）
    if (x < -500 || x > 20000 || y < -500 || y > 20000) {
      logInfo(_tag, 'onBallMoved: rejected garbage coords ($x,$y)');
      return;
    }
    _effectiveSettings?.setFloatingBallPosition(x, y);
    logInfo(_tag, 'ball moved to ($x,$y)');
  }

  (int, int) _resolvePosition() {
    final s = _effectiveSettings!;
    if (Platform.isWindows) {
      if (s.floatingBallX < 0 || s.floatingBallY < 0) {
        return Win32BallWindow.defaultDockPosition();
      }
      return Win32BallWindow.clampToWorkArea(
        s.floatingBallX.round(),
        s.floatingBallY.round(),
      );
    }
    // macOS/Linux：原生层负责落屏校验，Dart 只传原始值（-1 = 原生默认停靠）
    return (s.floatingBallX.round(), s.floatingBallY.round());
  }

  // ===========================================================================
  // 原生 → Dart（A6 协议）
  // ===========================================================================

  Future<dynamic> _onNativeCall(MethodCall call) async {
    switch (call.method) {
      case 'onDropPayload':
        final args = (call.arguments as Map).cast<String, dynamic>();
        final kind = args['kind'] as String? ?? 'text';
        final values = (args['values'] as List<dynamic>? ?? const [])
            .cast<String>();
        _handleDropPayload(kind, values);
      case 'onBallClicked':
        _onBallClicked();
      case 'onBallMoved':
        final args = (call.arguments as Map).cast<String, dynamic>();
        _onBallMoved(
          (args['x'] as num).toDouble(),
          (args['y'] as num).toDouble(),
        );
      case 'onDragEnter':
        _dragHover = true;
        if (Platform.isWindows) {
          final win = Win32BallWindow.instance;
          win.expandIfCollapsed();
          if (_cardOpen) {
            // 外部拖放进入：收起卡片并直接以 dragTarget 帧复位球-only 几何
            _cardOpen = false;
            _cardNames = const [];
            final img = await _buildCurrentBallImage();
            if (img != null && _enabled && _dragHover && win.isCreated) {
              win.dismissHoverCard(img);
              _syncWaveTicker();
            }
          } else {
            unawaited(_renderAndPush());
          }
        } else {
          unawaited(_renderAndPush());
        }
      case 'onDragLeave':
        _dragHover = false;
        unawaited(_renderAndPush());
      case 'onContextMenuRequested':
        // macOS/Linux：原生检测到右键 → Dart 组装 i18n 菜单 → 原生弹出
        final s = currentS;
        unawaited(
          _channel
              .invokeMethod('showContextMenu', {
                'items': [
                  {'id': 1, 'label': s.resumeAll},
                  {'id': 2, 'label': s.pauseAll},
                  {'id': 0, 'label': ''},
                  {'id': 3, 'label': s.trayShowWindow},
                  {'id': 4, 'label': s.hideFloatingBall},
                  {'id': 0, 'label': ''},
                  {'id': 5, 'label': s.trayExit},
                ],
              })
              .catchError((Object e) {
                logError(_tag, 'showContextMenu failed', e);
                return null;
              }),
        );
      case 'onMenuAction':
        final args = (call.arguments as Map).cast<String, dynamic>();
        _dispatchMenuAction((args['id'] as num?)?.toInt() ?? 0);
      case 'onCapability':
        final args = (call.arguments as Map).cast<String, dynamic>();
        final mode = args['mode'] as String? ?? 'wayland';
        _linuxCapability = mode == 'x11'
            ? LinuxBallCapability.x11
            : LinuxBallCapability.wayland;
        logInfo(_tag, 'linux capability: $mode');
        if (isDegraded) {
          // Wayland：激活降级通道（托盘 setTitle 速度 + 剪贴板形态③）
          final s = _settings;
          final nav = _navigatorKey;
          if (s != null && nav != null) {
            WaylandDegradationService.instance.activate(
              settings: s,
              navigatorKey: nav,
            );
          }
        } else if (_effectiveSettings?.floatingBallEnabled == true) {
          unawaited(enable());
        }
      default:
        logInfo(_tag, 'unknown native call: ${call.method}');
    }
    return null;
  }

  void _requestLinuxCapability() {
    unawaited(
      _channel.invokeMethod('queryCapability').catchError((Object e) {
        logError(_tag, 'queryCapability failed', e);
        return null;
      }),
    );
  }

  // ===========================================================================
  // 载荷分发（S0.5）
  // ===========================================================================

  void _handleDropPayload(String kind, List<String> values) {
    logInfo(_tag, 'drop payload: kind=$kind count=${values.length}');
    _dragHover = false;
    unawaited(_renderAndPush());

    if (kind == 'files') {
      var accepted = 0;
      for (final path in values) {
        if (path.toLowerCase().endsWith('.torrent')) {
          final saveDir = _effectiveSettings?.effectiveDefaultSaveDir ?? '';
          if (saveDir.isEmpty) {
            logError(_tag, 'drop torrent: defaultSaveDir empty, skip');
            continue;
          }
          unawaited(DownloadController.sendTorrentFileSignal(path, saveDir));
          accepted++;
        } else {
          // 明确不支持：非 .torrent 本地文件丢弃（S0.5 裁决）
          logInfo(_tag, 'drop file ignored (not .torrent): $path');
        }
      }
      if (accepted > 0) {
        unawaited(_restoreMainWindow());
      }
      return;
    }

    // kind == text：URL 正则筛选（语义校验留 Dart，A4）
    final urls = <String>[];
    for (final text in values) {
      urls.addAll(_urlRegex.allMatches(text).map((m) => m.group(0)!));
    }
    if (urls.isEmpty) {
      logInfo(_tag, 'drop text contained no URL, ignored');
      return;
    }
    unawaited(_showQuickDialogWithUrls(urls));
  }

  Future<void> _showQuickDialogWithUrls(List<String> urls) async {
    await _restoreMainWindow();
    final ctx = _navigatorKey?.currentContext;
    if (ctx == null || !ctx.mounted) {
      logError(_tag, 'no navigator context for quick dialog');
      return;
    }
    final settings = SettingsProvider.globalInstance ?? _settings!;
    showQuickDownloadDialog(
      ctx,
      url: urls.join('\n'),
      filename: '',
      fileSize: 0,
      mimeType: '',
      cookies: '',
      defaultSaveDir: settings.effectiveDefaultSaveDir,
      defaultQueueId: settings.defaultQueueId,
    );
  }

  // ===========================================================================
  // 唤醒检测（S0.5 — 墙钟跳变）
  // ===========================================================================

  void _startHeartbeat() {
    _lastBeat = DateTime.now();
    _heartbeat?.cancel();
    _heartbeat = Timer.periodic(const Duration(seconds: 10), (_) {
      final now = DateTime.now();
      final gap = now.difference(_lastBeat);
      _lastBeat = now;
      if (gap.inSeconds > 30) {
        logInfo(_tag, 'wake detected (gap=${gap.inSeconds}s), revalidating');
        _onWake();
      }
    });
  }

  void _onWake() {
    if (!_enabled) return;
    // 坐标校验 + 全量重绘
    if (Platform.isWindows && Win32BallWindow.instance.isCreated) {
      final s = _effectiveSettings!;
      final (x, y) = Win32BallWindow.clampToWorkArea(
        s.floatingBallX < 0 ? 0 : s.floatingBallX.round(),
        s.floatingBallY < 0 ? 0 : s.floatingBallY.round(),
      );
      Win32BallWindow.instance.moveTo(x, y);
    }
    if (!_nativeVisible) return; // 隐藏态（activeOnly idle）无需重绘
    unawaited(_rerenderAll());
  }
}
