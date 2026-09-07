
/// Windows 运行时窗口/任务栏图标（WM_SETICON），替代 `windowManager.setIcon`。
///
/// # 为什么不用 window_manager
///
/// window_manager 0.5.1 的 `SetIcon` 把 ICON_SMALL/ICON_BIG 硬编码加载为
/// 16/32px。Windows 任务栏对 `WM_SETICON` 递进来的图标有一个随 UI 缩放
/// 变化的最小尺寸阈值：低于阈值时只更新窗口标题栏装饰，任务栏按钮
/// **静默不更新**（回落到窗口类图标，即 Runner.rc 编进 exe 的默认图标）。
/// 高分屏（缩放 ≥125%）必踩中，而 FluxDown 是 TitleBarStyle.hidden 无
/// 标题栏——16/32px 的图标在任何可见位置都看不到效果。递 256px 图标则
/// 标题栏与任务栏都正确生效（Windows 自行降采样），托盘图标之所以一直
/// 正常，是因为 tray_manager 走 Shell_NotifyIcon 直递 HICON，不经此路径。
///
/// # 安全边界（对照 shortcut_icon.rs 的历史 bug）
///
/// `WM_SETICON` 只作用于**本进程自己的主窗口句柄**，不写任何 `.lnk`、
/// 注册表或 shell 持久状态，进程退出即消失——不存在改坏资源管理器/
/// 其他应用图标的可能。持久化的快捷方式图标仍由 Rust 侧
/// `native/hub/src/shortcut_icon.rs`（fail-closed 目标校验）独立处理。
library;

import 'package:ffi/ffi.dart';

import 'win32_toast/win32_bindings.dart';

/// `WM_SETICON`，from `winuser.h`。
const _wmSetIcon = 0x0080;

/// `ICON_SMALL` / `ICON_BIG`（`WM_SETICON` 的 wParam）。
const _iconSmall = 0;
const _iconBig = 1;

/// `IMAGE_ICON` / `LR_LOADFROMFILE`（LoadImageW 参数）。
const _imageIcon = 1;
const _lrLoadFromFile = 0x0010;

/// 加载尺寸。经验证的任务栏安全值：256px 在任何 DPI 缩放下都高于
/// 任务栏的静默忽略阈值；LoadImage 会从 .ico 容器选最接近的条目缩放，
/// 因此即使用户导入的 .ico 没有 256px 条目也能得到有效 HICON。
const _iconSide = 256;

/// 主窗口定位方式与 `windows/runner/main.cpp` 的单实例逻辑
/// （`FindWindow(kFlutterWindowClass, kWindowTitle)`）保持一致：
/// 快捷下载弹窗与主窗口共享窗口类，必须用标题区分。
const _mainWindowClass = 'FLUTTER_RUNNER_WIN32_WINDOW';
const _mainWindowTitle = 'FluxDown';

/// 把 [icoPath]（多尺寸 .ico）设为主窗口的运行时窗口/任务栏/Alt-Tab
/// 图标。成功返回 true；找不到主窗口或图标加载失败返回 false（不改动
/// 当前图标）。上一次通过本函数设置的 HICON 会被回收，无 GDI 句柄泄漏。
bool setMainWindowIconWin32(String icoPath) {
  final classPtr = _mainWindowClass.toNativeUtf16();
  final titlePtr = _mainWindowTitle.toNativeUtf16();
  final int hwnd;
  try {
    hwnd = findWindowW(classPtr, titlePtr);
  } finally {
    calloc.free(classPtr);
    calloc.free(titlePtr);
  }
  if (hwnd == 0) return false;
  return setWindowIconWin32(hwnd, icoPath);
}

/// 内核：把 [icoPath] 挂到任意 [hwnd] 的 ICON_SMALL/ICON_BIG 槽位。
/// 加载失败返回 false 且不改动窗口当前图标。
bool setWindowIconWin32(int hwnd, String icoPath) {
  // small/big 各持有独立 HICON：窗口在两个槽位分别持有引用，独立句柄
  // 才能在下次切换时按槽位无歧义地 DestroyIcon。
  final pathPtr = icoPath.toNativeUtf16();
  final int small;
  final int big;
  try {
    small = loadImageW(0, pathPtr, _imageIcon, _iconSide, _iconSide, _lrLoadFromFile);
    big = loadImageW(0, pathPtr, _imageIcon, _iconSide, _iconSide, _lrLoadFromFile);
  } finally {
    calloc.free(pathPtr);
  }
  if (small == 0 || big == 0) {
    if (small != 0) destroyIcon(small);
    if (big != 0) destroyIcon(big);
    return false;
  }

  // WM_SETICON 返回该槽位之前的 HICON（从未设置过则为 0；窗口类图标
  // 不会被返回），旧句柄由我们创建、由我们回收。
  final oldSmall = sendMessageW(hwnd, _wmSetIcon, _iconSmall, small);
  final oldBig = sendMessageW(hwnd, _wmSetIcon, _iconBig, big);
  if (oldSmall != 0) destroyIcon(oldSmall);
  if (oldBig != 0 && oldBig != oldSmall) destroyIcon(oldBig);
  return true;
}
