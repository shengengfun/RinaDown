import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

// =============================================================================
// FilePickerService._validateDirectory is private, so we test it indirectly
// through a minimal public wrapper. Instead, we replicate the same logic here
// and verify its correctness — the actual integration is covered by the fact
// that pickDirectory/saveFile call _validateDirectory before the OS dialog.
//
// For openFolder we test the path→directory resolution logic (the part before
// the platform-specific branch), using real temp directories on macOS/Linux.
// =============================================================================

/// Replicates FilePickerService._validateDirectory for unit testing.
/// This MUST be kept in sync with the private implementation.
Future<String?> validateDirectory(String? dir) async {
  if (dir == null || dir.isEmpty) return null;

  if (await Directory(dir).exists()) return dir;

  var current = Directory(dir);
  for (var i = 0; i < 20; i++) {
    final parent = current.parent;
    if (parent.path == current.path) break;
    if (await parent.exists()) return parent.path;
    current = parent;
  }

  return null;
}

/// Replicates openFolder's directory resolution logic (platform-independent
/// part only) so we can verify it on macOS without triggering Win32 FFI.
Future<String> resolveOpenFolderDir(String filePath) async {
  final type = await FileSystemEntity.type(filePath);
  switch (type) {
    case FileSystemEntityType.file:
      return File(filePath).parent.path;
    case FileSystemEntityType.directory:
      return filePath;
    default:
      final leaf = filePath.split(Platform.pathSeparator).last;
      return leaf.contains('.') ? File(filePath).parent.path : filePath;
  }
}

void main() {
  // ---------------------------------------------------------------------------
  // _validateDirectory tests
  // ---------------------------------------------------------------------------
  group('validateDirectory', () {
    test('null input returns null', () async {
      expect(await validateDirectory(null), isNull);
    });

    test('empty string returns null', () async {
      expect(await validateDirectory(''), isNull);
    });

    test('existing directory returns same path', () async {
      final tmp = await Directory.systemTemp.createTemp('picker_test_');
      try {
        final result = await validateDirectory(tmp.path);
        expect(result, equals(tmp.path));
      } finally {
        await tmp.delete(recursive: true);
      }
    });

    test('non-existent child falls back to existing parent', () async {
      final tmp = await Directory.systemTemp.createTemp('picker_test_');
      try {
        final nonExistent = '${tmp.path}${Platform.pathSeparator}a${Platform.pathSeparator}b${Platform.pathSeparator}c';
        final result = await validateDirectory(nonExistent);
        expect(result, equals(tmp.path));
      } finally {
        await tmp.delete(recursive: true);
      }
    });

    test('deeply nested non-existent path still finds ancestor', () async {
      final tmp = await Directory.systemTemp.createTemp('picker_test_');
      try {
        final deep = List.generate(10, (i) => 'level$i').join(Platform.pathSeparator);
        final nonExistent = '${tmp.path}${Platform.pathSeparator}$deep';
        final result = await validateDirectory(nonExistent);
        expect(result, equals(tmp.path));
      } finally {
        await tmp.delete(recursive: true);
      }
    });

    test('existing subdirectory returns exact subdirectory', () async {
      final tmp = await Directory.systemTemp.createTemp('picker_test_');
      final sub = await Directory('${tmp.path}${Platform.pathSeparator}sub').create();
      try {
        final nonExistent = '${sub.path}${Platform.pathSeparator}gone';
        final result = await validateDirectory(nonExistent);
        expect(result, equals(sub.path));
      } finally {
        await tmp.delete(recursive: true);
      }
    });

    test('system temp directory itself passes validation', () async {
      final result = await validateDirectory(Directory.systemTemp.path);
      expect(result, equals(Directory.systemTemp.path));
    });
  });

  // ---------------------------------------------------------------------------
  // openFolder directory resolution tests
  // ---------------------------------------------------------------------------
  group('resolveOpenFolderDir', () {
    test('file path resolves to parent directory', () async {
      final tmp = await Directory.systemTemp.createTemp('folder_test_');
      final file = File('${tmp.path}${Platform.pathSeparator}test.txt');
      await file.writeAsString('hello');
      try {
        final dir = await resolveOpenFolderDir(file.path);
        expect(dir, equals(tmp.path));
      } finally {
        await tmp.delete(recursive: true);
      }
    });

    test('directory path resolves to itself', () async {
      final tmp = await Directory.systemTemp.createTemp('folder_test_');
      try {
        final dir = await resolveOpenFolderDir(tmp.path);
        expect(dir, equals(tmp.path));
      } finally {
        await tmp.delete(recursive: true);
      }
    });

    test('non-existent path with extension treated as file (resolves to parent)', () async {
      final tmp = Directory.systemTemp.path;
      final fakePath = '$tmp${Platform.pathSeparator}nonexistent.mp4';
      final dir = await resolveOpenFolderDir(fakePath);
      expect(dir, equals(tmp));
    });

    test('non-existent path without extension treated as directory (returns as-is)', () async {
      final fakePath = '${Directory.systemTemp.path}${Platform.pathSeparator}some_folder';
      final dir = await resolveOpenFolderDir(fakePath);
      expect(dir, equals(fakePath));
    });

    test('non-existent path with CJK characters and extension resolves to parent', () async {
      final tmp = Directory.systemTemp.path;
      final fakePath = '$tmp${Platform.pathSeparator}下载文件.zip';
      final dir = await resolveOpenFolderDir(fakePath);
      expect(dir, equals(tmp));
    });

    test('non-existent path with spaces resolves correctly', () async {
      final tmp = Directory.systemTemp.path;
      final fakePath = '$tmp${Platform.pathSeparator}my folder with spaces';
      final dir = await resolveOpenFolderDir(fakePath);
      // No extension → treated as directory → returned as-is
      expect(dir, equals(fakePath));
    });
  });

  // openFolder 自身已下沉到 Rust（reveal_file.rs），Dart 仅 sendSignalToRust，
  // 无平台分支可测；上面 resolveOpenFolderDir 的逻辑现在由 Rust 端 fs::metadata
  // 重新实现，但保留这些 Dart 测试作为旧逻辑的回归基线。
}
