/// 离屏光栅化基础设施 — 供 Win32 Toast / 悬浮球等原生覆盖窗口共用。
///
/// 把任意 Flutter widget 在**主引擎**里离屏光栅化为位图字节，
/// 与主窗口共享同一套主题 token、字体与渲染管线。
///
/// 输出格式：
/// - [rgbaToPremultipliedBgra]：premultiplied BGRA，`UpdateLayeredWindow` 要求；
/// - straight-alpha RGBA（`rasterizeWidgetRgba`）：macOS CGImage / Linux cairo
///   由原生层自行转换（A6 协议约定）。
library;

import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';

/// RGBA（straight alpha）→ BGRA（premultiplied）— UpdateLayeredWindow 要求
Uint8List rgbaToPremultipliedBgra(Uint8List rgba) {
  final out = Uint8List(rgba.length);
  for (var i = 0; i < rgba.length; i += 4) {
    final r = rgba[i];
    final g = rgba[i + 1];
    final b = rgba[i + 2];
    final a = rgba[i + 3];
    out[i] = (b * a) ~/ 255;
    out[i + 1] = (g * a) ~/ 255;
    out[i + 2] = (r * a) ~/ 255;
    out[i + 3] = a;
  }
  return out;
}

/// 离屏光栅化 widget 并返回 straight-alpha RGBA 字节。
///
/// 返回 (width, height, rgba)。
Future<(int, int, Uint8List)> rasterizeWidgetRgba(
  Widget widget, {
  required Size logicalSize,
  required double scale,
}) async {
  final ui.Image image = await rasterizeWidget(
    widget,
    logicalSize: logicalSize,
    scale: scale,
  );
  try {
    final byteData = await image.toByteData(format: ui.ImageByteFormat.rawRgba);
    if (byteData == null) {
      throw StateError('toByteData returned null');
    }
    return (image.width, image.height, byteData.buffer.asUint8List());
  } finally {
    image.dispose();
  }
}

/// 离屏光栅化任意 widget（不上屏、不进 widget tree）。
///
/// 手工组装 BuildOwner + RenderView 管线 — 与 `screenshot` 包同原理，
/// 在主引擎主 isolate 内同步 layout/paint，仅 toImage 为异步。
Future<ui.Image> rasterizeWidget(
  Widget widget, {
  required Size logicalSize,
  required double scale,
}) async {
  final boundary = RenderRepaintBoundary();
  final pipelineOwner = PipelineOwner();
  final buildOwner = BuildOwner(focusManager: FocusManager());

  final renderView = RenderView(
    view: WidgetsBinding.instance.platformDispatcher.views.first,
    configuration: ViewConfiguration(
      logicalConstraints: BoxConstraints.tight(logicalSize),
      physicalConstraints: BoxConstraints.tight(logicalSize * scale),
      devicePixelRatio: scale,
    ),
    child: RenderPositionedBox(
      alignment: Alignment.center,
      child: boundary,
    ),
  );

  pipelineOwner.rootNode = renderView;
  renderView.prepareInitialFrame();

  final rootElement =
      RenderObjectToWidgetAdapter<RenderBox>(
        container: boundary,
        child: Directionality(
          textDirection: TextDirection.ltr,
          child: MediaQuery(
            data: MediaQueryData(
              size: logicalSize,
              devicePixelRatio: scale,
            ),
            child: widget,
          ),
        ),
      ).attachToRenderTree(buildOwner);

  try {
    buildOwner.buildScope(rootElement);
    buildOwner.finalizeTree();

    pipelineOwner.flushLayout();
    pipelineOwner.flushCompositingBits();
    pipelineOwner.flushPaint();

    return await boundary.toImage(pixelRatio: scale);
  } finally {
    // 卸载 element tree，释放 render objects
    rootElement.update(
      RenderObjectToWidgetAdapter<RenderBox>(container: boundary),
    );
    buildOwner.buildScope(rootElement);
    buildOwner.finalizeTree();
  }
}
