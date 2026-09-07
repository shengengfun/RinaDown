import 'dart:async';
import 'dart:io';

import 'package:file_selector/file_selector.dart';

/// 封装 [file_selector] 调用，提供统一的文件/目录选择服务。
///
/// `file_selector` 是 Flutter 官方推荐的文件选择包：
/// - **Windows**: 使用 C++ platform channel 直接调用 `IFileDialog` COM 接口，
///   在主线程执行，不存在 Dart FFI isolate 中的 COM 初始化竞态问题。
/// - **Linux**: 使用 GTK native file dialog 或 xdg-desktop-portal，
///   覆盖 GNOME、KDE、Sway、Hyprland 等主流桌面环境。
///
/// 所有异常统一以 [FilePickerException] 向上抛出。
class FilePickerService {
  FilePickerService._();

  // ─────────────────────────────────────────────
  // 公开 API
  // ─────────────────────────────────────────────

  /// 选取目录。
  ///
  /// 返回用户选中的目录路径，用户取消时返回 `null`。
  /// 失败时抛出 [FilePickerException]。
  static Future<String?> pickDirectory({
    required String dialogTitle,
    String? initialDirectory,
  }) async {
    try {
      final validatedDir = await _validateDirectory(initialDirectory);
      return await getDirectoryPath(
        initialDirectory: validatedDir,
        confirmButtonText: dialogTitle,
      );
    } catch (e) {
      if (e is FilePickerException) rethrow;
      throw FilePickerException(_classifyError(e), cause: e);
    }
  }

  /// 选取单个或多个文件。
  ///
  /// [allowedExtensions] 为允许的文件扩展名列表（不含点号），为 `null` 或空
  /// 时表示不限制文件类型。
  ///
  /// 返回 [XFile] 列表，用户取消时返回 `null`。
  /// 失败时抛出 [FilePickerException]。
  static Future<List<XFile>?> pickFiles({
    required String dialogTitle,
    List<String>? allowedExtensions,
    bool allowMultiple = false,
  }) async {
    try {
      final typeGroup = XTypeGroup(
        label: dialogTitle,
        extensions: allowedExtensions,
      );
      final groups = <XTypeGroup>[typeGroup];

      if (allowMultiple) {
        final files = await openFiles(acceptedTypeGroups: groups);
        return files.isEmpty ? null : files;
      } else {
        final file = await openFile(acceptedTypeGroups: groups);
        return file == null ? null : [file];
      }
    } catch (e) {
      if (e is FilePickerException) rethrow;
      throw FilePickerException(_classifyError(e), cause: e);
    }
  }

  /// 保存文件对话框。
  ///
  /// 返回用户选择的保存路径，用户取消时返回 `null`。
  /// 失败时抛出 [FilePickerException]。
  static Future<String?> saveFile({
    required String dialogTitle,
    String? fileName,
    String? initialDirectory,
    List<String>? allowedExtensions,
  }) async {
    try {
      final validatedDir = await _validateDirectory(initialDirectory);
      final typeGroup = XTypeGroup(
        label: dialogTitle,
        extensions: allowedExtensions,
      );

      final location = await getSaveLocation(
        acceptedTypeGroups: <XTypeGroup>[typeGroup],
        initialDirectory: validatedDir,
        suggestedName: fileName,
      );

      return location?.path;
    } catch (e) {
      if (e is FilePickerException) rethrow;
      throw FilePickerException(_classifyError(e), cause: e);
    }
  }

  // ─────────────────────────────────────────────
  // 内部辅助
  // ─────────────────────────────────────────────

  /// 验证 [dir] 是否为实际存在的目录。
  ///
  /// 在 Windows 上，`IFileDialog::SetFolder` 在目录不存在时会抛出 COM 错误
  /// (HRESULT)，导致文件选择器无法打开。这里预先检查：
  /// - 路径为 `null` 或空字符串 → 返回 `null`（让系统决定初始目录）
  /// - 路径存在且是目录 → 原样返回
  /// - 路径不存在或不是目录 → 逐级向上查找第一个存在的祖先目录
  /// - 所有祖先都不存在 → 返回 `null`
  static Future<String?> _validateDirectory(String? dir) async {
    if (dir == null || dir.isEmpty) return null;

    // 快速检查：路径存在且是目录
    if (await Directory(dir).exists()) return dir;

    // 逐级向上找到第一个存在的祖先目录。
    // 用户可能手动输入了路径或目录被删除/移动/在外部驱动器上。
    var current = Directory(dir);
    for (var i = 0; i < 20; i++) {
      final parent = current.parent;
      // 到达根目录仍无法解析 — 放弃
      if (parent.path == current.path) break;
      if (await parent.exists()) return parent.path;
      current = parent;
    }

    // 所有祖先都不存在，交给系统选择默认目录
    return null;
  }

  static FilePickerFailReason _classifyError(Object e) {
    final msg = e.toString().toLowerCase();

    if (msg.contains('dbus') ||
        msg.contains('portal') ||
        msg.contains('org.freedesktop')) {
      return FilePickerFailReason.noDialogTool;
    }

    if (msg.contains('gtk') || msg.contains('display')) {
      return FilePickerFailReason.noDialogTool;
    }

    if (msg.contains('hresult') ||
        msg.contains('hr =') ||
        msg.contains('windowsexception')) {
      return FilePickerFailReason.nativeDialogFailed;
    }

    return FilePickerFailReason.unknown;
  }
}

enum FilePickerFailReason {
  timeout,
  noDialogTool,
  comInitFailed,
  nativeDialogFailed,
  unknown,
}

class FilePickerException implements Exception {
  const FilePickerException(this.reason, {this.cause});
  final FilePickerFailReason reason;
  final Object? cause;
  @override
  String toString() => 'FilePickerException($reason, cause: $cause)';
}
