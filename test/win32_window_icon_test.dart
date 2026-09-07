/// win32_window_icon.dart 的行为测试（仅 Windows 主机）。
///
/// 契约：
/// 1. `setWindowIconWin32` 从多尺寸 .ico 加载 HICON 并经 WM_SETICON 挂到
///    窗口的 ICON_SMALL/ICON_BIG 槽位（WM_GETICON 可读回非零句柄）——
///    这是任务栏图标动态切换的唯一运行时通路，手写 FFI 签名错一个参数
///    就是运行时崩溃，必须有真实调用覆盖；
/// 2. 重复设置轮换句柄（旧 HICON 被回收，新句柄挂上）；
/// 3. 图标文件无效时返回 false 且不改动窗口当前图标（防止把任务栏
///    图标清成空白）。
///
/// 测试窗口用私有窗口类（非 FLUTTER_RUNNER_WIN32_WINDOW），不会与
/// 开发机上正在运行的 FluxDown 实例发生任何交互。
@TestOn('windows')
library;

import 'dart:ffi' show nullptr;

import 'package:ffi/ffi.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flux_down/src/services/native_overlay/win32_layered_window.dart'
    show ensureLayeredWindowClass;
import 'package:flux_down/src/services/win32_toast/win32_bindings.dart';
import 'package:flux_down/src/services/win32_window_icon.dart';

/// `WM_GETICON`，from `winuser.h`。
const _wmGetIcon = 0x007F;
const _iconSmall = 0;
const _iconBig = 1;

/// 打包默认图标，仓库内固定 fixture（flutter test 的 cwd = 包根）。
const _icoFixture = 'windows/runner/resources/app_icon.ico';

/// 创建一个不显示的最小测试窗口，返回 HWND。
int _createHiddenTestWindow() {
  const className = 'FluxDownIconTest_v1';
  ensureLayeredWindowClass(className);
  final classPtr = className.toNativeUtf16();
  final titlePtr = 'icon test'.toNativeUtf16();
  try {
    final hwnd = createWindowExW(
      0,
      classPtr,
      titlePtr,
      WS_POPUP,
      0,
      0,
      1,
      1,
      0,
      0,
      getModuleHandleW(nullptr),
      nullptr,
    );
    expect(hwnd, isNot(0), reason: 'CreateWindowExW 失败');
    return hwnd;
  } finally {
    calloc.free(classPtr);
    calloc.free(titlePtr);
  }
}

int _getIcon(int hwnd, int slot) => sendMessageW(hwnd, _wmGetIcon, slot, 0);

void main() {
  test('从 .ico 挂载图标：两个槽位均可读回非零 HICON', () {
    final hwnd = _createHiddenTestWindow();
    addTearDown(() => destroyWindow(hwnd));

    expect(setWindowIconWin32(hwnd, _icoFixture), isTrue);
    expect(_getIcon(hwnd, _iconSmall), isNot(0));
    expect(_getIcon(hwnd, _iconBig), isNot(0));
  });

  test('重复设置轮换句柄：新 HICON 挂上，槽位内容变化', () {
    final hwnd = _createHiddenTestWindow();
    addTearDown(() => destroyWindow(hwnd));

    expect(setWindowIconWin32(hwnd, _icoFixture), isTrue);
    final firstSmall = _getIcon(hwnd, _iconSmall);
    final firstBig = _getIcon(hwnd, _iconBig);

    expect(setWindowIconWin32(hwnd, _icoFixture), isTrue);
    expect(_getIcon(hwnd, _iconSmall), isNot(firstSmall));
    expect(_getIcon(hwnd, _iconBig), isNot(firstBig));
  });

  test('图标文件不存在：返回 false 且不改动已挂图标', () {
    final hwnd = _createHiddenTestWindow();
    addTearDown(() => destroyWindow(hwnd));

    expect(setWindowIconWin32(hwnd, _icoFixture), isTrue);
    final small = _getIcon(hwnd, _iconSmall);
    final big = _getIcon(hwnd, _iconBig);

    expect(setWindowIconWin32(hwnd, r'Z:\no\such\icon.ico'), isFalse);
    expect(_getIcon(hwnd, _iconSmall), small);
    expect(_getIcon(hwnd, _iconBig), big);
  });
}
