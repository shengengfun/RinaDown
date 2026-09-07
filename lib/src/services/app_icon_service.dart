import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/foundation.dart';
import 'package:flutter/painting.dart';
import 'package:flutter/services.dart' show MethodChannel, rootBundle;
import 'package:path/path.dart' as p;
import 'package:window_manager/window_manager.dart';

import '../bindings/bindings.dart';
import 'ico_codec.dart';
import 'kv_store.dart';
import 'log_service.dart';
import 'platform_utils.dart';
import 'tray_service.dart';
import 'win32_window_icon.dart';

const _tag = 'AppIconService';

/// 应用图标选择。
enum AppIconChoice {
  /// 默认图标（exe 资源 / app_icon.ico，Linux 为打包的 hicolor PNG）。
  defaultIcon,

  /// 内置备选图标「闪电」（assets/logo/fluxdown_bolt.png）。
  bolt,

  /// 用户导入的自定义图标。
  custom,
}

/// macOS 主窗口原生通道 —— 与 tray_service.dart 的 `_macWindowChannel`
/// 同名同通道（macos/Runner/MainFlutterWindow.swift 里只注册一次
/// handler，两个 Dart 文件各自持有一份 MethodChannel 实例是安全的，
/// 二者只是同一个平台通道名的独立句柄）。这里只用得到 `setAppIcon`。
const _macWindowChannel = MethodChannel('com.fluxdown/window');

/// 动态应用图标服务（Windows/Linux/macOS 均生效，实现方式因平台而异）。
///
/// 管理窗口/任务栏/托盘图标 **以及桌面快捷方式图标** 在「默认」「内置闪电」
/// 「自定义」之间的切换：
/// - 默认图标来自打包资源（Windows：exe 资源 + CMake install 的
///   `app_icon.ico`；Linux：CMake install 的
///   `data/icons/hicolor/256x256/apps/com.fluxdown.app.png`；macOS：
///   `Assets.xcassets/AppIcon`，运行时不持有可读路径，靠原生 `nil` 复位）；
/// - 内置「闪电」图标由打包资源 [builtinBoltAsset] 在应用时渲染，缓存在
///   数据目录 `icons/bolt_icon.<ext>`；
/// - 自定义图标由用户选择的图片（png/jpg/webp/bmp/ico）转换后持久化在
///   数据目录 `icons/custom_icon.<ext>`；
///   Windows 用多尺寸 `.ico`（WM_SETICON / .lnk IconLocation 都吃这个
///   容器），Linux/macOS 用单张 256px `.png`（`gtk_window_set_icon_from_file`
///   / `NSImage` 直接吃 PNG，不需要 ICO 容器）；
/// - 运行时窗口/任务栏图标：Windows 走自研 `win32_window_icon.dart`
///   （256px `WM_SETICON`；window_manager.setIcon 硬编码 16/32px，DPI
///   缩放下任务栏静默不更新），Linux 用 `window_manager.setIcon`
///   （`gtk_window_set_icon_from_file`），macOS 没有等价的 window_manager
///   实现，经 `com.fluxdown/window` 原生通道调用
///   `NSApp.applicationIconImage`；
/// - **持久化的「快捷方式」图标**（桌面 / 开始菜单 / 任务栏固定 /
///   Finder / Dock）——运行时替换窗口图标不会触碰这些静态引用，三个平台
///   分别处理：
///   - Windows：发 `UpdateShortcutIcons` 信号给 Rust
///     (`native/hub/src/shortcut_icon.rs`)，用 COM 重写 `.lnk` 的
///     `IconLocation` 并 `SHChangeNotify`；
///   - Linux：覆盖用户级 XDG 图标主题路径
///     `$XDG_DATA_HOME/icons/hicolor/256x256/apps/com.fluxdown.app.png`
///     ——查找优先级高于系统安装的同名图标，`.desktop` 的
///     `Icon=com.fluxdown.app` 因此在应用菜单/Dash固定/任务栏等处生效，
///     无需 root；
///   - macOS：经原生通道调用 `NSWorkspace.setIcon(forFile:)` 给 `.app`
///     bundle 设置 Finder 自定义图标覆盖（不修改已签名 bundle 的实际
///     内容），Finder / Dock（含未运行时的固定图标）据此显示；Launchpad
///     直接从 LaunchServices 读取 bundle 真实图标，不经过这层覆盖，是
///     已知的平台限制。
/// - 托盘图标：Windows/Linux 由 [TrayService.setCustomIcon] 跟随；macOS
///   菜单栏图标固定用系统模板图（HIG 惯例，随亮暗色自动着色），不随本
///   服务切换；
/// - 选择持久化在 [KvStore]，每次启动 [init] 时重新应用。
class AppIconService extends ChangeNotifier {
  AppIconService._();
  static final AppIconService instance = AppIconService._();

  static const _kCustomEnabled = 'app_icon_custom'; // 旧版 bool 键（迁移用）
  static const _kChoiceKey = 'app_icon_choice';

  /// 内置备选图标「闪电」的打包资源路径（UI 预览也直接引用）。
  static const builtinBoltAsset = 'assets/logo/fluxdown_bolt.png';

  /// Windows ICO 容器渲染的正方形尺寸集合。
  static const _icoSizes = [16, 24, 32, 48, 64, 128, 256];

  /// Linux/macOS 单文件持久化图标的正方形尺寸——两者运行时都直接吃 PNG，
  /// 不需要 Windows ICO 那种多尺寸容器；256px 与 Linux 打包默认图标
  /// （CMakeLists.txt 的 hicolor/256x256）保持一致。
  static const _flatIconSize = 256;

  AppIconChoice _choice = AppIconChoice.defaultIcon;

  /// 预览文件内容版本号 — 每次导入自增，供 UI 作为 Image key 破除缓存。
  int _previewRevision = 0;

  /// 当前的图标选择。
  AppIconChoice get choice => _choice;

  /// 当前是否启用自定义图标。
  bool get isCustom => _choice == AppIconChoice.custom;

  /// 当前是否启用内置「闪电」图标。
  bool get isBolt => _choice == AppIconChoice.bolt;

  int get previewRevision => _previewRevision;

  String get _iconsDir => p.join(resolveDataDir(), 'icons');

  /// Windows 用 `.ico`（WM_SETICON / .lnk IconLocation 都吃这个容器）；
  /// Linux/macOS 用单尺寸 `.png`。
  String get _iconExt => Platform.isWindows ? 'ico' : 'png';
  String get _customIconPath => p.join(_iconsDir, 'custom_icon.$_iconExt');
  String get _boltIconPath => p.join(_iconsDir, 'bolt_icon.$_iconExt');
  String get _previewPath => p.join(_iconsDir, 'custom_icon_preview.png');

  /// 自定义图标文件是否已存在（曾成功导入过）。
  bool get hasCustomIcon => File(_customIconPath).existsSync();

  /// 自定义图标的预览 PNG 路径；不存在时返回 `null`。
  String? get previewPngPath {
    final f = File(_previewPath);
    return f.existsSync() ? f.path : null;
  }

  /// 打包默认图标的绝对路径 —— 仅 Windows/Linux 使用（重置运行时窗口
  /// 图标时需要一个真实文件路径）；macOS 靠原生 `setAppIcon(null)` 复位，
  /// 不需要知道 bundle 内部路径。
  static String get _defaultIconPath {
    final exeDir = File(Platform.resolvedExecutable).parent.path;
    if (Platform.isWindows) return p.join(exeDir, 'app_icon.ico');
    // Linux bundle 布局（linux/CMakeLists.txt 的 install 规则；portable
    // tar.gz / AppImage / deb / arch 打包都直接复用同一份 flutter build
    // bundle 输出，因此这个相对路径在所有分发形式下一致）：
    //   <exeDir>/data/icons/hicolor/256x256/apps/com.fluxdown.app.png
    return p.join(
      exeDir,
      'data',
      'icons',
      'hicolor',
      '256x256',
      'apps',
      'com.fluxdown.app.png',
    );
  }

  /// Linux 用户级 XDG 图标主题覆盖路径。
  ///
  /// 写入此文件会让桌面环境（GNOME/KDE 等）的图标主题查找优先命中它，
  /// 覆盖系统安装的 `/usr/share/icons/hicolor/256x256/apps/com.fluxdown.app.png`
  /// ——`.desktop` 的 `Icon=com.fluxdown.app` 因此在应用菜单、Dash/任务栏
  /// 固定图标、窗口切换器等"快捷方式"位置生效，且无需 root 权限。这是
  /// Linux 侧对应 Windows 改写 `.lnk` 的角色。
  static String get _linuxIconOverridePath {
    final xdgDataHome = Platform.environment['XDG_DATA_HOME'];
    final base = (xdgDataHome != null && xdgDataHome.isNotEmpty)
        ? xdgDataHome
        : p.join(Platform.environment['HOME'] ?? '', '.local', 'share');
    return p.join(
      base,
      'icons',
      'hicolor',
      '256x256',
      'apps',
      'com.fluxdown.app.png',
    );
  }

  /// 启动时恢复持久化的图标选择。需在 `windowManager.ensureInitialized`
  /// 与 `TrayService.init` 之后调用。
  Future<void> init() async {
    try {
      final prefs = KvStore.instance;
      var choice = _readChoice(prefs);
      if (choice == AppIconChoice.custom && !hasCustomIcon) {
        choice = AppIconChoice.defaultIcon;
        await prefs.setString(_kChoiceKey, _choiceTag(choice));
        logInfo(_tag, 'custom icon file missing, falling back to default');
      }
      _choice = choice;
      switch (choice) {
        case AppIconChoice.defaultIcon:
          break;
        case AppIconChoice.bolt:
          await _buildBoltIcon();
          await _applyIcon(_boltIconPath);
          logInfo(_tag, 'restored bolt app icon: $_boltIconPath');
        case AppIconChoice.custom:
          await _applyIcon(_customIconPath);
          logInfo(_tag, 'restored custom app icon: $_customIconPath');
      }
    } catch (e, stack) {
      logError(_tag, 'init failed', e, stack);
    }
  }

  /// 读取持久化选择；无新键时从旧版 bool 键迁移。
  AppIconChoice _readChoice(KvStore prefs) {
    final tag = prefs.getString(_kChoiceKey);
    if (tag != null) {
      return AppIconChoice.values.firstWhere(
        (c) => _choiceTag(c) == tag,
        orElse: () => AppIconChoice.defaultIcon,
      );
    }
    // 旧版本仅有 bool 键：true=自定义
    final legacyCustom = prefs.getBool(_kCustomEnabled) ?? false;
    return legacyCustom ? AppIconChoice.custom : AppIconChoice.defaultIcon;
  }

  static String _choiceTag(AppIconChoice c) => switch (c) {
    AppIconChoice.defaultIcon => 'default',
    AppIconChoice.bolt => 'bolt',
    AppIconChoice.custom => 'custom',
  };

  /// 切回默认应用图标。
  Future<void> useDefault() async {
    try {
      if (Platform.isWindows) {
        await _applyWindowsRuntimeIcon(_defaultIconPath);
        await TrayService.instance.setCustomIcon(null);
        UpdateShortcutIcons(iconPath: _defaultIconPath).sendSignalToRust();
      } else if (Platform.isLinux) {
        await windowManager.setIcon(_defaultIconPath);
        await TrayService.instance.setCustomIcon(null);
        await _removeLinuxIconOverride();
      } else if (Platform.isMacOS) {
        await _macWindowChannel.invokeMethod<void>('setAppIcon', {
          'iconPath': null,
        });
      }
    } catch (e, stack) {
      // 应用失败不阻塞持久化：图标文件有效时下次启动仍可生效
      logError(_tag, 'useDefault: failed to apply icon', e, stack);
    }
    _choice = AppIconChoice.defaultIcon;
    await _persist();
    notifyListeners();
  }

  /// 切换到内置「闪电」图标。图标文件每次应用时从打包资源重建，
  /// 保证应用升级后资源更新不会残留旧缓存。
  Future<void> useBolt() async {
    try {
      await _buildBoltIcon();
      await _applyIcon(_boltIconPath);
    } catch (e, stack) {
      logError(_tag, 'useBolt: failed to apply icon', e, stack);
    }
    _choice = AppIconChoice.bolt;
    await _persist();
    notifyListeners();
  }

  /// 切换到已导入的自定义图标。[hasCustomIcon] 为 false 时无操作。
  Future<void> useCustom() async {
    if (!hasCustomIcon) return;
    try {
      await _applyIcon(_customIconPath);
    } catch (e, stack) {
      logError(_tag, 'useCustom: failed to apply icon', e, stack);
    }
    _choice = AppIconChoice.custom;
    await _persist();
    notifyListeners();
  }

  /// 导入用户选择的图片并立即启用为自定义图标。
  ///
  /// - `.ico` 文件：Windows 直接拷贝整个容器（Flutter 无法解码 ICO，预览
  ///   取其中最大的 PNG 条目）；Linux/macOS 不用 ICO 容器，直接把提取出
  ///   的最大 PNG 条目当作图标文件本体。纯 BMP 条目的旧式 ICO 在
  ///   Linux/macOS 上无可用图像数据，视为不支持并抛出；
  /// - 其余格式（png/jpg/webp/bmp/gif 首帧）解码后居中等比渲染为透明底
  ///   正方形 PNG——Windows 渲染 [_icoSizes] 全套尺寸编码为 ICO，
  ///   Linux/macOS 只渲染一张 [_flatIconSize] PNG。
  ///
  /// 解码失败或 IO 错误时抛出，由调用方提示用户。
  Future<void> importAndApply(String sourcePath) async {
    final bytes = await File(sourcePath).readAsBytes();
    final Uint8List icon;
    final Uint8List? preview;
    if (looksLikeIco(bytes)) {
      if (Platform.isWindows) {
        icon = bytes;
        preview = extractLargestPngEntry(bytes);
      } else {
        final extracted = extractLargestPngEntry(bytes);
        if (extracted == null) {
          throw const FormatException(
            'legacy BMP-only .ico has no decodable image entry',
          );
        }
        icon = extracted;
        preview = extracted;
      }
    } else if (Platform.isWindows) {
      final rendered = await _renderSquarePngs(bytes, _icoSizes);
      icon = buildIcoFromPngs(rendered);
      // 预览取 256px 条目，供设置页放大查看与侧边栏 Logo 使用
      preview = rendered.firstWhere((e) => e.size == 256).png;
    } else {
      final png = await _renderSinglePng(bytes, _flatIconSize);
      icon = png;
      preview = png;
    }

    await Directory(_iconsDir).create(recursive: true);
    // 临时文件 + rename 原子替换，防止半写文件被下次启动加载。
    // LoadImage/托盘不持有文件锁，删除旧文件安全。
    final tmp = File('$_customIconPath.tmp');
    await tmp.writeAsBytes(icon, flush: true);
    final dest = File(_customIconPath);
    if (await dest.exists()) {
      await dest.delete();
    }
    await tmp.rename(_customIconPath);

    final previewFile = File(_previewPath);
    if (preview != null) {
      await previewFile.writeAsBytes(preview, flush: true);
    } else if (await previewFile.exists()) {
      await previewFile.delete();
    }
    await FileImage(previewFile).evict();
    _previewRevision++;
    logInfo(_tag, 'imported custom icon from $sourcePath');
    await useCustom();
  }

  /// 按平台把 [path]（bolt/custom 已生成好的图标文件）应用为运行时窗口/
  /// 任务栏/托盘图标，并同步持久化的「快捷方式」图标引用。
  Future<void> _applyIcon(String path) async {
    if (Platform.isWindows) {
      await _applyWindowsRuntimeIcon(path);
      await TrayService.instance.setCustomIcon(path);
      // 桌面 / 开始菜单 / 任务栏固定的 .lnk 图标是静态引用，WM_SETICON
      // 只影响当前进程窗口——交给 Rust 侧用 COM 重写 IconLocation
      // （见 native/hub/src/shortcut_icon.rs 头注释）。
      UpdateShortcutIcons(iconPath: path).sendSignalToRust();
    } else if (Platform.isLinux) {
      await windowManager.setIcon(path);
      await TrayService.instance.setCustomIcon(path);
      await _updateLinuxIconOverride(path);
    } else if (Platform.isMacOS) {
      await _macWindowChannel.invokeMethod<void>('setAppIcon', {
        'iconPath': path,
      });
    }
  }

  /// Windows 运行时窗口/任务栏/Alt-Tab 图标（见 win32_window_icon.dart
  /// 头注释：window_manager.setIcon 的 16/32px 图标在 DPI 缩放下会被
  /// 任务栏静默忽略）。主窗口定位失败时回退 window_manager——至少保住
  /// Alt-Tab 大图标，不至于完全无效。
  Future<void> _applyWindowsRuntimeIcon(String path) async {
    if (setMainWindowIconWin32(path)) return;
    logError(_tag, 'WM_SETICON via FindWindow failed, '
        'falling back to windowManager.setIcon');
    await windowManager.setIcon(path);
  }

  /// 覆盖 Linux 用户级 XDG 图标主题路径，并尽力刷新图标缓存。
  Future<void> _updateLinuxIconOverride(String sourcePngPath) async {
    try {
      final dest = File(_linuxIconOverridePath);
      await dest.parent.create(recursive: true);
      await File(sourcePngPath).copy(dest.path);
      await _refreshLinuxIconCache();
    } catch (e, stack) {
      logError(_tag, 'failed to write linux icon theme override', e, stack);
    }
  }

  /// 删除 Linux 用户级覆盖，图标主题查找回落到系统安装的默认图标。
  Future<void> _removeLinuxIconOverride() async {
    try {
      final dest = File(_linuxIconOverridePath);
      if (await dest.exists()) {
        await dest.delete();
      }
      await _refreshLinuxIconCache();
    } catch (e, stack) {
      logError(_tag, 'failed to remove linux icon theme override', e, stack);
    }
  }

  /// 尽力而为地让 Explorer 等价物（Nautilus/Dash/任务栏）尽快看到新图标。
  ///
  /// 多数现代桌面环境对 `$XDG_DATA_HOME/icons` 采用 inotify 实时监听而非
  /// 缓存文件，这个调用常常是 no-op；`gtk-update-icon-cache` 二进制缺失，
  /// 或目标目录没有 `index.theme`（用户覆盖目录通常没有）都会返回非零，
  /// 一律静默忽略——图标本身已经写入生效，缓存刷新只是锦上添花。
  Future<void> _refreshLinuxIconCache() async {
    try {
      await Process.run('gtk-update-icon-cache', [
        '-f',
        '-t',
        // .../icons/hicolor/256x256/apps → 取三层父目录 = .../icons/hicolor
        p.dirname(p.dirname(p.dirname(_linuxIconOverridePath))),
      ]);
    } catch (_) {
      // binary 缺失是预期情况（不是所有发行版都装了 gtk-update-icon-cache）。
    }
  }

  Future<void> _persist() async {
    try {
      final prefs = KvStore.instance;
      await prefs.setString(_kChoiceKey, _choiceTag(_choice));
    } catch (e, stack) {
      logError(_tag, 'failed to persist app icon setting', e, stack);
    }
  }

  /// 从打包资源渲染「闪电」图标文件并原子写入 [_boltIconPath]。
  Future<void> _buildBoltIcon() async {
    final data = await rootBundle.load(builtinBoltAsset);
    final bytes = data.buffer.asUint8List(
      data.offsetInBytes,
      data.lengthInBytes,
    );
    final Uint8List built = Platform.isWindows
        ? buildIcoFromPngs(await _renderSquarePngs(bytes, _icoSizes))
        : await _renderSinglePng(bytes, _flatIconSize);

    await Directory(_iconsDir).create(recursive: true);
    final tmp = File('$_boltIconPath.tmp');
    await tmp.writeAsBytes(built, flush: true);
    final dest = File(_boltIconPath);
    if (await dest.exists()) {
      await dest.delete();
    }
    await tmp.rename(_boltIconPath);
  }

  /// 解码源图片并渲染 [sizes] 全套正方形 PNG。
  Future<List<IcoPngEntry>> _renderSquarePngs(
    Uint8List source,
    List<int> sizes,
  ) async {
    final codec = await ui.instantiateImageCodec(source);
    final frame = await codec.getNextFrame();
    final src = frame.image;
    try {
      final out = <IcoPngEntry>[];
      for (final size in sizes) {
        out.add(
          IcoPngEntry(size: size, png: await _renderSquarePng(src, size)),
        );
      }
      return out;
    } finally {
      src.dispose();
      codec.dispose();
    }
  }

  /// 解码源图片，只渲染单张 [size]×[size] 正方形 PNG（Linux/macOS 持久化
  /// 图标用，不需要 Windows ICO 那种多尺寸容器）。
  Future<Uint8List> _renderSinglePng(Uint8List source, int size) async {
    final rendered = await _renderSquarePngs(source, [size]);
    return rendered.single.png;
  }

  /// 将 [src] 居中等比缩放绘制到 size×size 透明画布，输出 PNG 字节。
  Future<Uint8List> _renderSquarePng(ui.Image src, int size) async {
    final recorder = ui.PictureRecorder();
    final canvas = ui.Canvas(recorder);
    final side = size.toDouble();
    final scale = src.width > src.height ? side / src.width : side / src.height;
    final w = src.width * scale;
    final h = src.height * scale;
    canvas.drawImageRect(
      src,
      ui.Rect.fromLTWH(0, 0, src.width.toDouble(), src.height.toDouble()),
      ui.Rect.fromLTWH((side - w) / 2, (side - h) / 2, w, h),
      ui.Paint()..filterQuality = ui.FilterQuality.high,
    );
    final picture = recorder.endRecording();
    final image = await picture.toImage(size, size);
    picture.dispose();
    try {
      final data = await image.toByteData(format: ui.ImageByteFormat.png);
      if (data == null) {
        throw StateError('PNG encode returned null for size $size');
      }
      return data.buffer.asUint8List(data.offsetInBytes, data.lengthInBytes);
    } finally {
      image.dispose();
    }
  }
}
