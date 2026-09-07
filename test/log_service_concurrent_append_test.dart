// 回归：Dart 与 Rust 两端写同一个日志文件时，Rust 侧写入必须存活。
//
// dart:io 的 `FileMode.append` 并不带 `O_APPEND`——它只是
// `open(O_RDWR|O_CREAT)` 之后做一次 `lseek(SEEK_END)`（dart-sdk
// runtime/bin/file_macos.cc / file_win.cc）。LogService 全程持有同一个
// RandomAccessFile，写位置只随自身写入前进；而 Rust 端
// (native/engine/src/logger.rs) 用真正的 O_APPEND 恒定写在真实 EOF。
// 修复前，Rust 每追加 N 字节，Dart 的下一次写就从落后 N 字节处开始并
// 原地覆盖掉它——实测 macOS 用户 6 个会话 3400+ 行日志里一条 Rust 日志
// 都没有，直接废掉了 Rust 侧的可诊断性（issue #244）。

import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:flux_down/src/services/log_service.dart';

void main() {
  late Directory dir;

  setUp(() {
    dir = Directory.systemTemp.createTempSync('fluxdown_log_concurrent');
  });

  tearDown(() {
    try {
      dir.deleteSync(recursive: true);
    } catch (_) {}
  });

  /// 目录里唯一那个 fluxdown_*.log。
  File logFile() => dir.listSync().whereType<File>().firstWhere(
    (f) => f.path.endsWith('.log'),
  );

  /// 模拟 Rust 端：每次重新 open 并落到真实 EOF，语义等价于 O_APPEND。
  void appendLikeRust(File file, String text) {
    file.writeAsStringSync(text, mode: FileMode.append, flush: true);
  }

  test('外部追加写入者的内容不会被 Dart 侧覆盖，且两端顺序保持', () {
    final service = LogService.forTest(dir);
    final file = logFile();

    service.log('T', 'dart-1');
    appendLikeRust(file, '[rust] rust-1\n');
    service.log('T', 'dart-2');
    appendLikeRust(file, '[rust] rust-2\n');
    service.log('T', 'dart-3');

    final content = file.readAsStringSync();

    // 修复前 rust-1 / rust-2 会被紧随其后的 Dart 写入整行覆盖。
    expect(content, contains('[rust] rust-1'));
    expect(content, contains('[rust] rust-2'));
    expect(content, contains('flutter{component="T"}: dart-1'));
    expect(content, contains('flutter{component="T"}: dart-2'));
    expect(content, contains('flutter{component="T"}: dart-3'));

    // 交错顺序 = 实际写入顺序，说明没有任何一端在错误偏移上落笔。
    final offsets = [
      content.indexOf('flutter{component="T"}: dart-1'),
      content.indexOf('[rust] rust-1'),
      content.indexOf('flutter{component="T"}: dart-2'),
      content.indexOf('[rust] rust-2'),
      content.indexOf('flutter{component="T"}: dart-3'),
    ];
    expect(offsets, orderedEquals(List<int>.from(offsets)..sort()));
  });

  test('外部追加后文件长度等于两端写入量之和（没有覆盖丢字节）', () {
    final service = LogService.forTest(dir);
    final file = logFile();

    final baseline = file.lengthSync(); // session header
    service.log('T', 'x');
    final afterDart = file.lengthSync();
    final dartLineBytes = afterDart - baseline;
    expect(dartLineBytes, greaterThan(0));

    const rustLine = '[rust] y\n';
    appendLikeRust(file, rustLine);
    service.log('T', 'x'); // 与第一条等长

    expect(file.lengthSync(), baseline + dartLineBytes * 2 + rustLine.length);
  });

  test('错误日志包含稳定组件、错误 ID，并为堆栈每行保留结构前缀', () {
    final service = LogService.forTest(dir);
    final file = logFile();

    service.error(
      'Worker',
      'operation failed',
      StateError('boom'),
      StackTrace.fromString('frame-one\nframe-two'),
    );

    final errorLines = file
        .readAsLinesSync()
        .where((line) => line.contains('ERROR flutter{component="Worker"'))
        .toList();
    expect(errorLines, hasLength(5));
    final ids = errorLines
        .map((line) => RegExp(r'error_id="([^"]+)"').firstMatch(line)?.group(1))
        .toSet();
    expect(ids.length, 1);
    expect(ids.single, isNotNull);
    expect(
      errorLines.any((line) => line.endsWith(': operation failed')),
      isTrue,
    );
    expect(errorLines.any((line) => line.endsWith(': frame-two')), isTrue);
  });
}
