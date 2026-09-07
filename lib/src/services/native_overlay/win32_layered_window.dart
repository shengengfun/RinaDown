/// Win32 分层窗口基础设施 — Toast 与悬浮球共用。
///
/// 抽取自 win32_toast_window.dart 的已验证模式：
/// - 窗口类注册守卫（参数化类名，DefWindowProcW 直通，零 Dart 原生回调）
/// - premultiplied BGRA → 32bpp top-down DIB
/// - UpdateLayeredWindow 整图贴入
///
/// 消费方各自保留状态机与 hit-test 语义。
library;

import 'dart:ffi';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';

import '../log_service.dart';
import '../win32_toast/win32_bindings.dart';

const _tag = 'LayeredWin';

/// 已注册的窗口类名集合（进程级守卫，防重复注册）。
final Set<String> _registeredClasses = {};

/// 注册分层窗口类（幂等）。WndProc = DefWindowProcW 原生指针。
///
/// 失败 throw [StateError]（沿用 toast 规范）；成功 logInfo。
void ensureLayeredWindowClass(String className) {
  if (_registeredClasses.contains(className)) return;

  final hInstance = getModuleHandleW(nullptr);
  final classNamePtr = className.toNativeUtf16();
  final cursorPtr = Pointer<Utf16>.fromAddress(32512); // IDC_ARROW

  final wndClass = calloc<WNDCLASSEXW>();
  try {
    wndClass.ref.cbSize = sizeOf<WNDCLASSEXW>();
    wndClass.ref.style = 0;
    // 直接使用 DefWindowProcW 的原生函数指针 — 不经过 Dart VM
    wndClass.ref.lpfnWndProc = defWindowProcWPtr;
    wndClass.ref.cbClsExtra = 0;
    wndClass.ref.cbWndExtra = 0;
    wndClass.ref.hInstance = hInstance;
    wndClass.ref.hIcon = 0;
    wndClass.ref.hCursor = loadCursorW(0, cursorPtr);
    wndClass.ref.hbrBackground = 0; // 分层窗口，无背景刷
    wndClass.ref.lpszMenuName = nullptr;
    wndClass.ref.lpszClassName = classNamePtr;
    wndClass.ref.hIconSm = 0;

    final atom = registerClassExW(wndClass);
    if (atom == 0) {
      final err = getLastError();
      // 1410 = ERROR_CLASS_ALREADY_EXISTS：类表是进程级的，Dart 热重启后
      // isolate 重建导致守卫集合清空，但 OS 侧类仍在 — 视为已注册继续。
      if (err != 1410) {
        throw StateError('RegisterClassExW failed for $className, error=$err');
      }
      logInfo(_tag, 'window class already exists (reused): $className');
    } else {
      logInfo(_tag, 'window class registered: $className, atom=$atom');
    }
    _registeredClasses.add(className);
  } finally {
    calloc.free(wndClass);
    calloc.free(classNamePtr);
  }
}

/// premultiplied BGRA 像素 → 32bpp top-down DIB。返回 HBITMAP。
int createDibFromBgra(int width, int height, Uint8List bgraPremultiplied) {
  final bmi = calloc<BITMAPINFOHEADER>();
  final ppvBits = calloc<Pointer<Void>>();
  try {
    bmi.ref
      ..biSize = sizeOf<BITMAPINFOHEADER>()
      ..biWidth = width
      ..biHeight = -height // 负值 = top-down 行序
      ..biPlanes = 1
      ..biBitCount = 32
      ..biCompression = BI_RGB;

    final hBitmap = createDIBSection(0, bmi, DIB_RGB_COLORS, ppvBits, 0, 0);
    if (hBitmap == 0 || ppvBits.value == nullptr) {
      throw StateError('CreateDIBSection failed');
    }

    final dst = ppvBits.value
        .cast<Uint8>()
        .asTypedList(bgraPremultiplied.length);
    dst.setAll(0, bgraPremultiplied);
    return hBitmap;
  } finally {
    calloc.free(bmi);
    calloc.free(ppvBits);
  }
}

/// 把 DIB 贴入分层窗口（UpdateLayeredWindow，per-pixel alpha + 整窗 alpha）。
///
/// [memDC] 为调用方持有的兼容 DC；位置/尺寸为物理像素。
void pushLayeredBitmap({
  required int hwnd,
  required int memDC,
  required int hBitmap,
  required int screenX,
  required int screenY,
  required int width,
  required int height,
  int alpha = 255,
}) {
  final old = selectObject(memDC, hBitmap);

  final ptDst = calloc<POINT>();
  final size = calloc<SIZE>();
  final ptSrc = calloc<POINT>();
  final blend = calloc<BLENDFUNCTION>();
  try {
    ptDst.ref
      ..x = screenX
      ..y = screenY;
    size.ref
      ..cx = width
      ..cy = height;
    ptSrc.ref
      ..x = 0
      ..y = 0;
    blend.ref
      ..BlendOp = AC_SRC_OVER
      ..BlendFlags = 0
      ..SourceConstantAlpha = alpha
      ..AlphaFormat = AC_SRC_ALPHA;

    updateLayeredWindow(
      hwnd,
      0,
      ptDst,
      size,
      memDC,
      ptSrc,
      0,
      blend,
      ULW_ALPHA,
    );
  } finally {
    selectObject(memDC, old);
    calloc.free(ptDst);
    calloc.free(size);
    calloc.free(ptSrc);
    calloc.free(blend);
  }
}
