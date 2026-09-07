import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:path/path.dart' as p;
import 'package:tray_manager/tray_manager.dart';
import 'package:window_manager/window_manager.dart';

import '../i18n/locale_provider.dart';
import '../models/settings_provider.dart';
import 'floating_ball/floating_ball_service.dart';
import 'log_service.dart';

const _tag = 'TrayService';

/// macOS 主窗口恢复通道（对应 macos/Runner/MainFlutterWindow.swift）。
const _macWindowChannel = MethodChannel('com.fluxdown/window');

/// 从托盘/最小化/关闭到托盘恢复主窗口并置于前台。
///
/// macOS：走原生 `com.fluxdown/window` → AppDelegate.restoreMainWindow，
/// 与点击 Dock 图标相同的可靠激活序列（ignoringOtherApps: true）。
/// window_manager 的 show()/focus() 在 App 非前台时用
/// ignoringOtherApps: false，macOS 13+ 常无法把窗口带到前台，
/// 导致「关掉窗口后再点击打不开、需退出重开」。
/// 其它平台：沿用 window_manager show() + focus()。
Future<void> restoreMainWindow() async {
  if (Platform.isMacOS) {
    try {
      await _macWindowChannel.invokeMethod<void>('restore');
      return;
    } catch (e, stack) {
      logError(_tag, 'native restore failed, falling back', e, stack);
    }
    try {
      await windowManager.setSkipTaskbar(false);
    } catch (e, stack) {
      logError(_tag, 'failed to restore macOS regular policy', e, stack);
    }
  }
  await windowManager.show();
  await windowManager.focus();
}

/// macOS 应用菜单原生动作（Hide/Hide Others/Show All/Zoom/前置/全屏）。
///
/// Flutter 的 `PlatformMenuItem` 无法绑定 AppKit 标准 selector，
/// 统一经 `com.fluxdown/window` 通道转发到
/// macos/Runner/MainFlutterWindow.swift 执行等效原生调用。
Future<void> macMenuAction(String method) async {
  try {
    await _macWindowChannel.invokeMethod<void>(method);
  } catch (e, stack) {
    logError(_tag, 'macMenuAction($method) failed', e, stack);
  }
}

/// 系统托盘服务 — 管理托盘图标、菜单和事件
class TrayService with TrayListener {
  TrayService._();
  static final TrayService instance = TrayService._();

  bool _initialized = false;

  /// 是否正在退出过程中 — 防止重入和退出期间操作窗口
  bool _isExiting = false;

  // Windows 系统托盘图标路径（深/浅色任务栏各一套）
  String? _winTrayDarkPath; // 白色箭头 — 适配深色任务栏
  String? _winTrayLightPath; // 深蓝色箭头 — 适配浅色任务栏
  // Linux 默认托盘图标路径（init() 中计算一次，无深浅色变体）
  String? _linuxDefaultTrayIconPath;
  // 当前有效的深/浅色状态，初始值跟随系统，后续由 setIsDark() 驱动
  bool _isDark = false;
  // 自定义应用图标覆盖（由 AppIconService 驱动）：非 null 时 Windows/Linux
  // 托盘固定使用该图标文件，忽略深/浅色变体。macOS 不支持覆盖——菜单栏图标
  // 固定用系统模板图（HIG 惯例，随亮暗色自动着色）。
  String? _customIconPath;

  /// 应用退出回调 — 由外部（如 _FluxDownAppState）设置以实现优雅退出。
  /// 回调中应等待待处理通知、销毁托盘、再销毁窗口。
  Future<void> Function()? onExitApp;

  /// 初始化系统托盘图标和菜单
  Future<void> init() async {
    logInfo(_tag, 'init called, _initialized=$_initialized');
    if (_initialized) return;
    _initialized = true;

    // 图标路径：Windows/Linux 使用绝对文件系统路径，macOS 使用 Flutter asset key
    // CMakeLists.txt 已配置将图标文件复制到 exe 同级目录
    final exeDir = File(Platform.resolvedExecutable).parent.path;
    final String iconPath;
    final bool isTemplate;
    if (Platform.isWindows) {
      // Windows: 根据系统亮暗模式选择初始托盘图标
      //   tray_win_dark.ico  = 白色箭头（深色任务栏）
      //   tray_win_light.ico = 深蓝色箭头（浅色任务栏）
      // CMakeLists.txt 已将两个文件复制到 exe 同级目录
      // 初始值使用系统亮度；启动后由 _FluxDownAppState 通过 setIsDark() 修正为 app 主题
      _winTrayDarkPath = p.join(exeDir, 'tray_win_dark.ico');
      _winTrayLightPath = p.join(exeDir, 'tray_win_light.ico');
      _isDark =
          WidgetsBinding.instance.platformDispatcher.platformBrightness ==
          Brightness.dark;
      iconPath = _effectiveTrayIconPath();
      isTemplate = false;
    } else if (Platform.isMacOS) {
      // macOS: tray_manager 使用 rootBundle.load() 加载，需要 Flutter asset key
      // 使用单色模板图标，macOS 自动适配亮色/暗色菜单栏
      iconPath = 'assets/logo/tray_iconTemplate.png';
      isTemplate = true;
    } else {
      // Linux: exe is at <prefix>/bin/, flutter_assets at <prefix>/data/flutter_assets/
      _linuxDefaultTrayIconPath = p.join(
        exeDir,
        'data',
        'flutter_assets',
        'assets',
        'logo',
        'fluxdown_logo.png',
      );
      iconPath = _effectiveTrayIconPath();
      isTemplate = false;
    }

    logInfo(_tag, 'setting icon: $iconPath (isTemplate=$isTemplate)');
    await trayManager.setIcon(iconPath, isTemplate: isTemplate);
    // setToolTip is not implemented on Linux
    if (!Platform.isLinux) {
      await trayManager.setToolTip('FluxDown');
    }

    final menu = Menu(
      items: [
        MenuItem(key: 'show_window', label: currentS.trayShowWindow),
        MenuItem.checkbox(
          key: 'toggle_ball',
          label: currentS.trayShowFloatingBall,
          checked:
              SettingsProvider.globalInstance?.floatingBallEnabled ?? false,
        ),
        MenuItem.separator(),
        MenuItem(key: 'exit_app', label: currentS.trayExit),
      ],
    );
    await trayManager.setContextMenu(menu);
    trayManager.addListener(this);
    logInfo(_tag, 'init done');
  }

  /// 刷新托盘菜单文字（语言切换后调用）
  Future<void> refreshMenu() async {
    if (!_initialized) return;
    logInfo(_tag, 'refreshMenu called');
    final menu = Menu(
      items: [
        MenuItem(key: 'show_window', label: currentS.trayShowWindow),
        MenuItem.checkbox(
          key: 'toggle_ball',
          label: currentS.trayShowFloatingBall,
          checked:
              SettingsProvider.globalInstance?.floatingBallEnabled ?? false,
        ),
        MenuItem.separator(),
        MenuItem(key: 'exit_app', label: currentS.trayExit),
      ],
    );
    await trayManager.setContextMenu(menu);
    logInfo(_tag, 'refreshMenu done');
  }

  /// 销毁托盘图标
  Future<void> destroy() async {
    logInfo(_tag, 'destroy called, _initialized=$_initialized');
    trayManager.removeListener(this);
    await trayManager.destroy();
    _initialized = false;
    logInfo(_tag, 'destroy done');
  }

  // ─────────────────────────────────────────────
  // Windows 深/浅色切换 + Windows/Linux 自定义图标覆盖
  // ─────────────────────────────────────────────

  /// 返回当前生效的托盘图标路径（自定义覆盖优先，否则按平台取默认值）。
  String _effectiveTrayIconPath() {
    final custom = _customIconPath;
    if (custom != null) return custom;
    if (Platform.isWindows) {
      return _isDark ? (_winTrayDarkPath ?? '') : (_winTrayLightPath ?? '');
    }
    return _linuxDefaultTrayIconPath ?? '';
  }

  /// 由外部（_FluxDownAppState）在应用主题或系统亮度变化时调用，
  /// 将托盘图标切换为与 app 当前生效主题一致的深/浅色版本。
  Future<void> setIsDark(bool isDark) async {
    if (!Platform.isWindows) return;
    if (_isDark == isDark) return; // 无变化，跳过
    _isDark = isDark;
    if (_customIconPath != null) return; // 自定义图标覆盖中，深浅色不影响托盘
    if (!_initialized || _isExiting) return;
    final newPath = _effectiveTrayIconPath();
    logInfo(_tag, 'setIsDark($isDark) → $newPath');
    try {
      await trayManager.setIcon(newPath, isTemplate: false);
    } catch (e, stack) {
      logError(_tag, 'setIsDark: failed to update tray icon', e, stack);
    }
  }

  /// 设置或清除自定义托盘图标覆盖（由 AppIconService 调用）。
  /// [path] 为 null 时恢复默认图标。macOS 菜单栏图标固定用系统模板图
  /// （HIG 惯例，随亮暗色自动着色），不随 App 图标切换，直接忽略。
  Future<void> setCustomIcon(String? path) async {
    if (Platform.isMacOS) return;
    if (_customIconPath == path) return; // 无变化，跳过
    _customIconPath = path;
    if (!_initialized || _isExiting) return;
    final newPath = _effectiveTrayIconPath();
    logInfo(_tag, 'setCustomIcon($path) → $newPath');
    try {
      await trayManager.setIcon(newPath, isTemplate: false);
    } catch (e, stack) {
      logError(_tag, 'setCustomIcon: failed to update tray icon', e, stack);
    }
  }

  /// 显示窗口并聚焦
  Future<void> _showWindow() async {
    logInfo(_tag, '_showWindow called, _isExiting=$_isExiting');
    // 退出过程中不再操作窗口，避免在已 destroyed 的窗口上调用导致崩溃
    if (_isExiting) {
      logInfo(_tag, '_showWindow skipped (isExiting)');
      return;
    }
    try {
      // 诊断日志：窗口前置状态（macOS 最小化恢复问题排查用）
      final isMinimized = await windowManager.isMinimized();
      final isVisible = await windowManager.isVisible();
      logInfo(
        _tag,
        'window state before show: isMinimized=$isMinimized, '
        'isVisible=$isVisible',
      );
      logInfo(_tag, 'calling restoreMainWindow()...');
      await restoreMainWindow();
      logInfo(_tag, '_showWindow done');
    } catch (e, stack) {
      logError(_tag, '_showWindow error', e, stack);
    }
  }

  /// 隐藏窗口到托盘。
  ///
  /// macOS 原生路径会在同一 AppKit 调用中先隐藏主窗口，再切换 accessory
  /// activation policy，使 Dock 与应用菜单消失；通道不可用时降级到
  /// window_manager 的等价操作。其它平台仅隐藏窗口。
  Future<void> hideToTray() async {
    logInfo(_tag, 'hideToTray called, _isExiting=$_isExiting');
    if (_isExiting) {
      logInfo(_tag, 'hideToTray skipped (isExiting)');
      return;
    }
    try {
      if (Platform.isMacOS) {
        try {
          await _macWindowChannel.invokeMethod<void>('hideToTray');
          logInfo(_tag, 'hideToTray done (native)');
          return;
        } catch (e, stack) {
          logError(_tag, 'native hideToTray failed, falling back', e, stack);
        }
      }
      await windowManager.hide();
      if (Platform.isMacOS) {
        await windowManager.setSkipTaskbar(true);
      }
      logInfo(_tag, 'hideToTray done');
    } catch (e, stack) {
      logError(_tag, 'hideToTray error', e, stack);
    }
  }

  // ─────────────────────────────────────────────
  // TrayListener 回调
  // ─────────────────────────────────────────────

  @override
  void onTrayIconMouseDown() {
    logInfo(_tag, 'onTrayIconMouseDown, _isExiting=$_isExiting');
    // 退出中不响应托盘点击
    if (_isExiting) return;
    _showWindow();
  }

  @override
  void onTrayIconRightMouseDown() {
    logInfo(_tag, 'onTrayIconRightMouseDown, _isExiting=$_isExiting');
    if (_isExiting) return;
    trayManager.popUpContextMenu();
  }

  @override
  void onTrayIconRightMouseUp() {}

  @override
  void onTrayMenuItemClick(MenuItem menuItem) {
    logInfo(
      _tag,
      'onTrayMenuItemClick: key=${menuItem.key}, _isExiting=$_isExiting',
    );
    if (_isExiting) return;
    switch (menuItem.key) {
      case 'show_window':
        _showWindow();
      case 'toggle_ball':
        final enabled =
            SettingsProvider.globalInstance?.floatingBallEnabled ?? false;
        FloatingBallService.instance.setEnabled(!enabled);
        refreshMenu(); // 同步复选状态
      case 'exit_app':
        _handleExit();
    }
  }

  /// 请求优雅退出（供 macOS 应用菜单「退出」等外部入口复用，
  /// 与托盘「退出」走同一完整清理流程，含防重入）。
  Future<void> requestExit() => _handleExit();

  /// 优雅退出 — 通过回调通知上层执行完整清理流程
  Future<void> _handleExit() async {
    logInfo(_tag, '_handleExit called, _isExiting=$_isExiting');
    // 防止重入：多次点击退出不会重复执行
    if (_isExiting) return;
    _isExiting = true;

    try {
      if (onExitApp != null) {
        logInfo(_tag, 'calling onExitApp callback...');
        await onExitApp!();
        logInfo(_tag, 'onExitApp callback done');
      } else {
        // 兜底：如果没有设置回调，直接退出（但先清理托盘）
        logInfo(_tag, 'no onExitApp callback, direct destroy');
        await destroy();
        await windowManager.destroy();
      }
    } catch (e, stack) {
      logError(_tag, '_handleExit error', e, stack);
      // 出错也尝试强制退出
      try {
        await windowManager.destroy();
      } catch (_) {}
    }
  }
}
