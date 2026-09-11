import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:rina_down/src/services/platform_utils.dart';

/// 更名迁移（`%LOCALAPPDATA%\FluxDown` → `…\RinaDown`）行为测试。
/// 与 native/engine/src/data_dir.rs 的 `legacy_layout_*` 覆盖同一组语义。
void main() {
  late Directory root;
  late String baseDir;
  late String oldDir;
  late String newDir;
  final sep = Platform.pathSeparator;

  setUp(() {
    root = Directory.systemTemp.createTempSync('rinadown_legacy_migrate_');
    // baseDir 扮演 %LOCALAPPDATA%：旧目录与新目录都是它的子目录。
    baseDir = root.path;
    oldDir = '$baseDir${sep}FluxDown';
    newDir = '$baseDir${sep}RinaDown';
  });

  tearDown(() {
    try {
      root.deleteSync(recursive: true);
    } catch (_) {}
  });

  void write(String path, String content) {
    final file = File(path);
    file.parent.createSync(recursive: true);
    file.writeAsStringSync(content);
  }

  String read(String path) => File(path).readAsStringSync();

  test('新目录不存在：整目录搬走，旧文件夹消失且库改成新名', () {
    write('$oldDir${sep}flux_down.db', 'db');
    write('$oldDir${sep}flux_down.db-wal', 'wal');
    write('$oldDir${sep}settings.json', '{}');
    write('$oldDir${sep}plugins${sep}a.js', '//p');

    migrateLegacyAppData(baseDir, newDir);

    expect(Directory(oldDir).existsSync(), isFalse);
    expect(read('$newDir${sep}rina_down.db'), 'db');
    expect(read('$newDir${sep}rina_down.db-wal'), 'wal');
    expect(read('$newDir${sep}settings.json'), '{}');
    expect(File('$newDir${sep}plugins${sep}a.js').existsSync(), isTrue);
    // 旧库名不得留在新目录里（留着等于新版找不到库，白搬）。
    expect(File('$newDir${sep}flux_down.db').existsSync(), isFalse);
  });

  test('新目录已存在：逐项补齐，不覆盖既有新数据', () {
    write('$oldDir${sep}flux_down.db', 'old-db');
    write('$oldDir${sep}bin${sep}ffmpeg.exe', 'bin');
    write('$newDir${sep}settings.json', 'new');

    migrateLegacyAppData(baseDir, newDir);

    expect(read('$newDir${sep}rina_down.db'), 'old-db');
    expect(File('$newDir${sep}bin${sep}ffmpeg.exe').existsSync(), isTrue);
    // 目标已存在 → 不覆盖；旧目录剩下的条目原地不动。
    expect(read('$newDir${sep}settings.json'), 'new');
    expect(Directory(oldDir).existsSync(), isTrue);
  });

  test('新库已在：旧库原地保留，不覆盖新库', () {
    write('$oldDir${sep}flux_down.db', 'old-db');
    write('$newDir${sep}rina_down.db', 'new-db');

    migrateLegacyAppData(baseDir, newDir);

    expect(read('$newDir${sep}rina_down.db'), 'new-db');
    expect(File('$oldDir${sep}flux_down.db').existsSync(), isTrue);
  });

  test('没有旧目录：完全 no-op，不创建新目录', () {
    migrateLegacyAppData(baseDir, newDir);
    expect(Directory(newDir).existsSync(), isFalse);
  });

  test('幂等：重复调用不炸、内容不变', () {
    write('$oldDir${sep}flux_down.db', 'db');
    migrateLegacyAppData(baseDir, newDir);
    migrateLegacyAppData(baseDir, newDir);
    expect(read('$newDir${sep}rina_down.db'), 'db');
  });
}
