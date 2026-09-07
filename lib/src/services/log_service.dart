import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:isolate';
import 'dart:typed_data';

import 'package:flutter/foundation.dart';
import 'package:path/path.dart' as p;
import 'platform_utils.dart';

/// 文件日志服务 — 将日志写入数据目录的 logs/ 子目录，按日期分文件。
///
/// 日志目录由 [resolveDataDir] 决定：
/// - Windows 便携版: exe 同级 logs/
/// - Windows 安装版: %LOCALAPPDATA%/FluxDown/logs/
/// - Linux: ~/.local/share/fluxdown/logs/
/// - macOS: ~/Library/Application Support/fluxdown/logs/
///
/// 使用缓冲写入 + 定时刷盘，兼顾性能和崩溃前日志完整度。
///
/// 自动分割与清理（与 Rust 端 logger.rs 协议一致）：
/// - 单文件超过 2MB 自动分割为 `fluxdown_YYYY-MM-DD.N.log` 分卷；
/// - 日志总大小超过上限（默认 10MB，可在设置中调整）时按
///   （日期, 分卷序号）从最旧开始删除；
/// - 清理只做目录遍历 + stat，不读文件内容，内存占用极小。
///
/// 单例，应在 app 启动最早期调用 [init]。
class LogService {
  LogService._();
  static final LogService instance = LogService._();

  /// 测试专用实例：日志目录显式指定，且不启动定时刷盘定时器
  /// （`writeStringSync` 本身就是直写 syscall，测试可直接读文件校验）。
  /// 生产代码一律用 [instance] + [init]。
  @visibleForTesting
  factory LogService.forTest(Directory logDir) {
    final service = LogService._();
    service._initialized = true;
    service._logDir = logDir;
    service._rotateSink();
    return service;
  }

  RandomAccessFile? _raf;
  String? _currentDateTag;
  Timer? _flushTimer;
  bool _initialized = false;
  bool _degraded = false;
  int _failureCount = 0;
  String? _lastError;
  int _errorSequence = 0;

  /// 自上次 flush 以来是否有新数据写入
  bool _dirty = false;

  /// 日志保留天数
  static const int _retentionDays = 7;

  /// 单个日志文件大小上限，超过则自动分割到新分卷
  static const int _maxFileBytes = 2 * 1024 * 1024;

  /// 日志目录总大小默认上限（可由设置覆盖，见 [maxTotalBytes]）
  static const int _defaultMaxTotalBytes = 10 * 1024 * 1024;

  /// 日志文件名格式：fluxdown_YYYY-MM-DD.log 或 fluxdown_YYYY-MM-DD.N.log
  static final RegExp _logNamePattern = RegExp(
    r'^fluxdown_(\d{4}-\d{2}-\d{2})(?:\.(\d+))?\.log$',
  );

  int _maxTotalBytes = _defaultMaxTotalBytes;

  /// 当前日期内的分卷序号（0 = 无序号的首个文件）
  int _currentPart = 0;

  /// 当前文件大小。每次写入前经 [_writeRaw] 的 `lengthSync()` 重新读取，
  /// 因此始终等于文件真实长度。
  int _fileSize = 0;

  /// 日志目录总大小上限（字节）。设置后立即执行一次超量清理。
  set maxTotalBytes(int bytes) {
    if (bytes < 1024 * 1024) return;
    if (_maxTotalBytes == bytes) return;
    _maxTotalBytes = bytes;
    if (_initialized) _enforceTotalSize();
  }

  int get maxTotalBytes => _maxTotalBytes;

  /// 日志 writer 是否已成功初始化。
  bool get initialized => _initialized;

  /// 本次进程生命周期内是否发生过日志基础设施失败。
  bool get degraded => _degraded;

  /// 本次进程生命周期内累计日志基础设施失败次数。
  int get failureCount => _failureCount;

  /// 最近一次日志基础设施失败。
  String? get lastError => _lastError;

  /// 日志目录
  late final Directory _logDir;

  /// 暴露日志目录路径，供导出日志等功能使用。
  Directory get logDir => _logDir;

  /// 初始化日志服务。应在 main() 最开始调用。
  void init() {
    if (_initialized) return;
    _initialized = true;

    _logDir = _resolveLogDir();
    try {
      if (!_logDir.existsSync()) {
        _logDir.createSync(recursive: true);
      }
    } catch (e, stack) {
      _recordFailure('create log directory', e, stack);
      _initialized = false;
      return;
    }

    try {
      _rotateSink();
    } catch (e, stack) {
      _recordFailure('open log file', e, stack);
      _initialized = false;
      return;
    }

    // 启动时清理 7 天前的旧日志文件
    _cleanupOldLogs();

    // 每 2 秒刷盘一次，确保崩溃前有足够日志。
    // 仅在有新数据写入时才调用 flushSync，避免空闲时无谓的磁盘 I/O。
    _flushTimer = Timer.periodic(const Duration(seconds: 2), (_) {
      if (!_dirty) return;
      try {
        _raf?.flushSync();
        _dirty = false;
      } catch (e, stack) {
        _recordFailure('flush', e, stack);
      }
    });
  }

  /// 把 [text] 追加到当前日志文件尾。
  ///
  /// **必须经本方法写，不要直接 `_raf.writeStringSync`。**
  /// Rust 端（native/engine/src/logger.rs）用真正的 `O_APPEND` 写同一个文件，
  /// 而 dart:io 的 `FileMode.append` 并不带 `O_APPEND`——它只是
  /// `open(O_RDWR|O_CREAT)` 之后做一次 `lseek(SEEK_END)`
  /// （dart-sdk runtime/bin/file_macos.cc / file_win.cc）。这个 fd 的写位置
  /// 只随 Dart 自己的写入前进，Rust 每追加 N 字节，Dart 的下一次写就从落后
  /// N 字节处开始，**原地覆盖掉 Rust 刚写的内容**。Dart 写入量远大于 Rust，
  /// 结果是 Rust 侧日志几乎全军覆没（实测：macOS 用户 6 个会话 3400+ 行里
  /// 一条 Rust 日志都没有，Windows 上也只有会话末尾 Dart 停写后的残余）。
  ///
  /// 因此每次写前用 `lengthSync()` 读真实长度并在落后时重新 seek。
  /// 残留竞态仅剩「seek 与 write 之间 Rust 恰好追加一行」，窗口在微秒级，
  /// 最坏影响是偶发一行被覆盖，而不是当前的整体丢失。
  void _writeRaw(String text) {
    final raf = _raf;
    if (raf == null) {
      throw StateError('log file is not open');
    }
    final length = raf.lengthSync();
    if (length != _fileSize) {
      raf.setPositionSync(length);
    }
    final bytes = utf8.encode(text);
    raf.writeFromSync(bytes);
    _fileSize = length + bytes.length;
    _dirty = true;
  }

  /// 写一条 INFO 日志。[tag] 是稳定组件名，[message] 是事件内容。
  void log(String tag, String message) {
    _writeEntry('INFO', tag, message);
  }

  /// 记录错误、错误 ID 与完整堆栈，并立即刷盘。
  void error(String tag, String message, [Object? err, StackTrace? stack]) {
    final errorId =
        'dart-${DateTime.now().microsecondsSinceEpoch.toRadixString(36)}-${_errorSequence++}';
    final detail = StringBuffer(message);
    if (err != null) detail.write('\nexception: $err');
    if (stack != null) detail.write('\nstackTrace:\n$stack');
    _writeEntry('ERROR', tag, detail.toString(), errorId: errorId, flush: true);
  }

  void _writeEntry(
    String level,
    String tag,
    String message, {
    String? errorId,
    bool flush = false,
  }) {
    if (!_initialized) {
      // ignore: avoid_print
      print('[logger-uninitialized] $level [$tag] $message');
      return;
    }
    try {
      _rotateSink();
      final now = DateTime.now();
      final ts =
          '${_pad2(now.hour)}:${_pad2(now.minute)}:${_pad2(now.second)}.${_pad3(now.millisecond)}';
      final component = tag
          .replaceAll('\\', '\\\\')
          .replaceAll('"', '\\"')
          .replaceAll('\n', '\\n');
      final fields = errorId == null
          ? 'component="$component"'
          : 'component="$component" error_id="$errorId"';
      final prefix = '$ts ${level.padRight(5)} flutter{$fields}: ';
      for (final line in message.split('\n')) {
        _writeRaw('$prefix$line\n');
      }
      _maybeRollBySize();
      if (flush) {
        _raf?.flushSync();
        _dirty = false;
      }
      if (kDebugMode) {
        // ignore: avoid_print
        print('$prefix$message');
      }
    } catch (e, stack) {
      _recordFailure('write $level event', e, stack);
    }
  }

  void _recordFailure(String operation, Object error, [StackTrace? stack]) {
    _degraded = true;
    _failureCount++;
    _lastError = '$operation: $error';
    // The persistent sink is unavailable; stderr is the only independent
    // emergency sink. Never call log()/error() from here.
    // ignore: avoid_print
    print(
      '[LogService] persistent logging degraded: $_lastError'
      '${stack == null ? '' : '\n$stack'}',
    );
  }

  /// 将所有日志文件打包为 ZIP 压缩包保存到 [zipPath]。
  ///
  /// 打包前会先刷盘，确保最新日志已写入文件。
  /// [sanitize] 为 true（默认）时，导出前对日志内容进行脱敏处理，
  /// 移除 URL 认证凭证、代理密码、用户路径、设备 ID 等敏感信息。
  /// 返回打包的文件数量。
  Future<int> exportLogs(String zipPath, {bool sanitize = true}) async {
    // 先刷盘，确保最新日志已写入
    try {
      _raf?.flushSync();
      _dirty = false;
    } catch (e, stack) {
      _recordFailure('flush before export', e, stack);
    }

    if (!_logDir.existsSync()) return 0;

    final logFiles = <File>[];
    for (final entity in _logDir.listSync()) {
      if (entity is! File) continue;
      final name = p.basename(entity.path);
      if (!name.startsWith('fluxdown_') || !name.endsWith('.log')) continue;
      logFiles.add(entity);
    }
    if (logFiles.isEmpty) return 0;

    // 按（日期, 分卷序号）排序
    logFiles.sort(_compareLogFiles);

    final zipBytes = _buildZip(logFiles, sanitize: sanitize);
    await File(zipPath).writeAsBytes(zipBytes);
    return logFiles.length;
  }

  /// 读取当天（最新分卷）日志文本内容，用于随反馈一并提交。
  ///
  /// 已做脱敏处理并按 [maxChars] 截断（保留末尾，日志越新越靠后）。
  /// 无日志时返回空字符串。
  Future<String> readTodayLog({int maxChars = 25000}) async {
    try {
      _raf?.flushSync();
      _dirty = false;
    } catch (e, stack) {
      _recordFailure('flush before reading logs', e, stack);
    }

    if (!_logDir.existsSync()) return '';

    final now = DateTime.now();
    final dateTag = '${now.year}-${_pad2(now.month)}-${_pad2(now.day)}';

    // 收集当天全部分卷，按序号排序后拼接。
    final parts = <({int part, File file})>[];
    try {
      for (final entity in _logDir.listSync()) {
        if (entity is! File) continue;
        final m = _logNamePattern.firstMatch(p.basename(entity.path));
        if (m == null || m.group(1) != dateTag) continue;
        parts.add((part: int.parse(m.group(2) ?? '0'), file: entity));
      }
    } catch (_) {
      return '';
    }
    if (parts.isEmpty) return '';
    parts.sort((a, b) => a.part.compareTo(b.part));

    final buffer = StringBuffer();
    for (final f in parts) {
      try {
        buffer.write(f.file.readAsStringSync());
      } catch (_) {}
    }
    var content = _sanitizeLogContent(buffer.toString());
    if (content.length > maxChars) {
      content = content.substring(content.length - maxChars);
    }
    return content;
  }

  /// 计算日志目录的总大小（字节）。
  int get logDirSizeBytes {
    if (!_logDir.existsSync()) return 0;
    int total = 0;
    for (final entity in _logDir.listSync()) {
      if (entity is! File) continue;
      final name = p.basename(entity.path);
      if (!name.startsWith('fluxdown_') || !name.endsWith('.log')) continue;
      try {
        total += entity.lengthSync();
      } catch (_) {}
    }
    return total;
  }

  /// 日志文件数量。
  int get logFileCount {
    if (!_logDir.existsSync()) return 0;
    int count = 0;
    for (final entity in _logDir.listSync()) {
      if (entity is! File) continue;
      final name = p.basename(entity.path);
      if (!name.startsWith('fluxdown_') || !name.endsWith('.log')) continue;
      count++;
    }
    return count;
  }

  /// 关闭日志服务
  Future<void> dispose() async {
    _flushTimer?.cancel();
    _flushTimer = null;
    try {
      _raf?.flushSync();
      _raf?.closeSync();
    } catch (_) {}
    _raf = null;
    _initialized = false;
  }

  // ── 内部 ──

  /// 按日期切换日志文件（全同步，无 IOSink 异步问题）
  void _rotateSink() {
    final now = DateTime.now();
    final dateTag = '${now.year}-${_pad2(now.month)}-${_pad2(now.day)}';
    if (dateTag == _currentDateTag && _raf != null) return;

    _closeRaf();
    _currentDateTag = dateTag;
    _currentPart = _scanActivePart(dateTag);
    _openCurrentFile();

    _writeRaw(
      '\n'
      '====== FluxDown log session started at $now ======\n'
      '  pid: $pid\n'
      '  exe: ${Platform.resolvedExecutable}\n'
      '  isolate: ${Isolate.current.debugName}\n'
      '\n',
    );
    _enforceTotalSize();
  }

  void _closeRaf() {
    try {
      _raf?.flushSync();
      _raf?.closeSync();
    } catch (e, stack) {
      _recordFailure('close log file', e, stack);
    }
    _raf = null;
  }

  /// 打开当前 (_currentDateTag, _currentPart) 对应的日志文件（append 模式）。
  void _openCurrentFile() {
    final file = File(_filePath(_currentDateTag!, _currentPart));
    _raf = file.openSync(mode: FileMode.append);
    _fileSize = _raf!.lengthSync();
  }

  String _filePath(String dateTag, int part) {
    final suffix = part == 0 ? '' : '.$part';
    return '${_logDir.path}${Platform.pathSeparator}fluxdown_$dateTag$suffix.log';
  }

  /// 找到 [dateTag] 当天已有的最大分卷序号；若该分卷已写满则返回下一个序号。
  /// Rust 端可能已创建更高序号的分卷，两端通过该扫描收敛到同一文件。
  int _scanActivePart(String dateTag) {
    int? maxPart;
    try {
      for (final entity in _logDir.listSync()) {
        if (entity is! File) continue;
        final m = _logNamePattern.firstMatch(p.basename(entity.path));
        if (m == null || m.group(1) != dateTag) continue;
        final part = int.parse(m.group(2) ?? '0');
        maxPart = maxPart == null || part > maxPart ? part : maxPart;
      }
    } catch (_) {}
    if (maxPart == null) return 0;
    try {
      final size = File(_filePath(dateTag, maxPart)).lengthSync();
      if (size >= _maxFileBytes) return maxPart + 1;
    } catch (_) {}
    return maxPart;
  }

  /// 自动分割：当前文件超过单文件上限时切换到新分卷并触发总量清理。
  /// [_fileSize] 由 [_writeRaw] 每次写入时按真实文件长度刷新，无需再额外校准。
  void _maybeRollBySize() {
    if (_fileSize < _maxFileBytes || _currentDateTag == null) return;

    _closeRaf();
    final next = _scanActivePart(_currentDateTag!);
    // 防御：保证分卷序号单调递增，避免重新打开已写满的文件
    _currentPart = next > _currentPart ? next : _currentPart + 1;
    _openCurrentFile();
    _dirty = true;
    _enforceTotalSize();
  }

  /// 总大小超量清理：按（日期, 分卷序号）从最旧开始删除，
  /// 直到总大小回到 [_maxTotalBytes] 内。当前活跃文件不删除。
  void _enforceTotalSize() {
    try {
      final files = <({String date, int part, File file, int size})>[];
      int total = 0;
      for (final entity in _logDir.listSync()) {
        if (entity is! File) continue;
        final m = _logNamePattern.firstMatch(p.basename(entity.path));
        if (m == null) continue;
        int size = 0;
        try {
          size = entity.lengthSync();
        } catch (_) {}
        files.add((
          date: m.group(1)!,
          part: int.parse(m.group(2) ?? '0'),
          file: entity,
          size: size,
        ));
        total += size;
      }
      if (total <= _maxTotalBytes) return;

      files.sort((a, b) {
        final c = a.date.compareTo(b.date);
        return c != 0 ? c : a.part.compareTo(b.part);
      });
      final activePath = _currentDateTag == null
          ? null
          : _filePath(_currentDateTag!, _currentPart);
      for (final f in files) {
        if (total <= _maxTotalBytes) break;
        if (f.file.path == activePath) continue;
        try {
          f.file.deleteSync();
          total -= f.size;
        } catch (e, stack) {
          _recordFailure('delete oversized log ${f.file.path}', e, stack);
        }
      }
    } catch (e, stack) {
      _recordFailure('enforce total log size', e, stack);
    }
  }

  /// 按（日期, 分卷序号）比较两个日志文件，用于导出排序。
  static int _compareLogFiles(File a, File b) {
    final ma = _logNamePattern.firstMatch(p.basename(a.path));
    final mb = _logNamePattern.firstMatch(p.basename(b.path));
    if (ma == null || mb == null) {
      return p.basename(a.path).compareTo(p.basename(b.path));
    }
    final c = ma.group(1)!.compareTo(mb.group(1)!);
    if (c != 0) return c;
    return int.parse(
      ma.group(2) ?? '0',
    ).compareTo(int.parse(mb.group(2) ?? '0'));
  }

  /// 解析日志目录：委托 platform_utils.resolveDataDir()，加 /logs 后缀。
  ///
  /// - Linux: ~/.local/share/fluxdown/logs
  /// - macOS: ~/Library/Application Support/fluxdown/logs
  /// - Windows 便携版: exe 同级 logs/
  /// - Windows 安装版: %LOCALAPPDATA%/FluxDown/logs
  static Directory _resolveLogDir() {
    final dataDir = resolveDataDir();
    return Directory('$dataDir${Platform.pathSeparator}logs');
  }

  /// 清理超过 [_retentionDays] 天的 fluxdown_*.log 文件。
  void _cleanupOldLogs() {
    try {
      if (!_logDir.existsSync()) return;
      final cutoff = DateTime.now().subtract(Duration(days: _retentionDays));
      for (final entity in _logDir.listSync()) {
        if (entity is! File) continue;
        final name = entity.uri.pathSegments.last;
        if (!name.startsWith('fluxdown_') || !name.endsWith('.log')) continue;
        try {
          final modified = entity.lastModifiedSync();
          if (modified.isBefore(cutoff)) {
            entity.deleteSync();
          }
        } catch (e, stack) {
          _recordFailure('delete expired log ${entity.path}', e, stack);
        }
      }
    } catch (e, stack) {
      _recordFailure('clean expired logs', e, stack);
    }
  }

  static String _pad2(int n) => n.toString().padLeft(2, '0');
  static String _pad3(int n) => n.toString().padLeft(3, '0');
}

/// 全局快捷方法
void logInfo(String tag, String message) =>
    LogService.instance.log(tag, message);

void logError(String tag, String message, [Object? err, StackTrace? stack]) =>
    LogService.instance.error(tag, message, err, stack);

// ══════════════════════════════════════════════════
//  ZIP 构建（纯 Dart 标准库，零外部依赖）
// ══════════════════════════════════════════════════

final List<int> _crc32Table = () {
  final table = List<int>.filled(256, 0);
  for (int i = 0; i < 256; i++) {
    int c = i;
    for (int j = 0; j < 8; j++) {
      c = (c & 1) != 0 ? (0xEDB88320 ^ (c >>> 1)) : (c >>> 1);
    }
    table[i] = c;
  }
  return table;
}();

int _crc32(List<int> data) {
  int crc = 0xFFFFFFFF;
  for (final b in data) {
    crc = _crc32Table[(crc ^ b) & 0xFF] ^ (crc >>> 8);
  }
  return (crc ^ 0xFFFFFFFF) & 0xFFFFFFFF;
}

void _writeU16(BytesBuilder b, int v) {
  b.addByte(v & 0xFF);
  b.addByte((v >> 8) & 0xFF);
}

void _writeU32(BytesBuilder b, int v) {
  b.addByte(v & 0xFF);
  b.addByte((v >> 8) & 0xFF);
  b.addByte((v >> 16) & 0xFF);
  b.addByte((v >> 24) & 0xFF);
}

class _ZipCentralEntry {
  final List<int> nameBytes;
  final int crc;
  final int compressedSize;
  final int uncompressedSize;
  final int localOffset;
  final int dosTime;
  final int dosDate;

  _ZipCentralEntry({
    required this.nameBytes,
    required this.crc,
    required this.compressedSize,
    required this.uncompressedSize,
    required this.localOffset,
    required this.dosTime,
    required this.dosDate,
  });
}

Uint8List _buildZip(List<File> files, {bool sanitize = false}) {
  final out = BytesBuilder(copy: false);
  final centralEntries = <_ZipCentralEntry>[];

  for (final file in files) {
    final name = p.basename(file.path);
    final nameBytes = utf8.encode(name);
    final rawData = file.readAsBytesSync();
    // 脱敏处理：在压缩前对文本内容替换敏感信息
    final data = sanitize ? _sanitizeLogBytes(rawData) : rawData;
    final crc = _crc32(data);
    final compressed = ZLibEncoder(raw: true, level: 6).convert(data);

    // DOS 日期时间
    final mod = file.lastModifiedSync();
    final dosTime = (mod.hour << 11) | (mod.minute << 5) | (mod.second ~/ 2);
    final dosDate = ((mod.year - 1980) << 9) | (mod.month << 5) | mod.day;

    final localOffset = out.length;

    // Local file header
    _writeU32(out, 0x04034b50); // signature
    _writeU16(out, 20); // version needed
    _writeU16(out, 0); // flags
    _writeU16(out, 8); // compression: deflate
    _writeU16(out, dosTime);
    _writeU16(out, dosDate);
    _writeU32(out, crc);
    _writeU32(out, compressed.length);
    _writeU32(out, data.length); // 使用脱敏后的实际大小
    _writeU16(out, nameBytes.length);
    _writeU16(out, 0); // extra length
    out.add(nameBytes);
    out.add(compressed);

    centralEntries.add(
      _ZipCentralEntry(
        nameBytes: nameBytes,
        crc: crc,
        compressedSize: compressed.length,
        uncompressedSize: data.length, // 使用脱敏后的实际大小
        localOffset: localOffset,
        dosTime: dosTime,
        dosDate: dosDate,
      ),
    );
  }

  final centralStart = out.length;

  for (final e in centralEntries) {
    _writeU32(out, 0x02014b50); // signature
    _writeU16(out, 20); // version made by
    _writeU16(out, 20); // version needed
    _writeU16(out, 0); // flags
    _writeU16(out, 8); // compression: deflate
    _writeU16(out, e.dosTime);
    _writeU16(out, e.dosDate);
    _writeU32(out, e.crc);
    _writeU32(out, e.compressedSize);
    _writeU32(out, e.uncompressedSize);
    _writeU16(out, e.nameBytes.length);
    _writeU16(out, 0); // extra length
    _writeU16(out, 0); // comment length
    _writeU16(out, 0); // disk number
    _writeU16(out, 0); // internal attributes
    _writeU32(out, 0); // external attributes
    _writeU32(out, e.localOffset);
    out.add(e.nameBytes);
  }

  final centralSize = out.length - centralStart;

  // End of central directory
  _writeU32(out, 0x06054b50);
  _writeU16(out, 0); // disk number
  _writeU16(out, 0); // central dir start disk
  _writeU16(out, centralEntries.length);
  _writeU16(out, centralEntries.length);
  _writeU32(out, centralSize);
  _writeU32(out, centralStart);
  _writeU16(out, 0); // comment length

  return out.toBytes();
}

// ══════════════════════════════════════════════════
//  日志脱敏（导出时保护敏感信息）
// ══════════════════════════════════════════════════

/// 脱敏规则列表。
///
/// 按顺序应用，每条规则包含一个正则和替换函数。
/// 规则覆盖范围：
///   1. URL 内嵌认证凭证（user:pass@host）
///   2. HTTP(S) URL 长 query string（CDN 签名、学术数据库 token 等）
///   3. Cookie 头值
///   4. Authorization 头值
///   5. 代理用户名/密码字段
///   6. Linux 用户主目录路径
///   7. Windows 用户目录路径
final _kSanitizeRules = <({RegExp pattern, String Function(Match m) replace})>[
  // 1. URL 内嵌认证凭证：scheme://user:pass@host → scheme://***@host
  //    覆盖：ftp://user:pass@host/path、http://admin:secret@proxy:8080
  (
    pattern: RegExp(r'([\w+\-.]+://)[^:/\s@]+:[^@\s]+@', caseSensitive: false),
    replace: (m) => '${m[1]}***@',
  ),

  // 2. HTTP(S) URL 长 query string（>50 字符）→ ?[QUERY_REDACTED]
  //    覆盖：知网/百度网盘签名 URL、CDN 防盗链、私人 BT tracker passkey
  //    使用非贪婪 + 向前看，不消耗 URL 后面的分隔符（逗号、括号等）
  (
    pattern: RegExp(
      r'(https?://[^?\s]{3,})\?([^\s]{50,}?)(?=[\s,)\]>]|$)',
      caseSensitive: false,
      multiLine: true,
    ),
    replace: (m) => '${m[1]}?[QUERY_REDACTED]',
  ),

  // 3. Cookie 头值：Cookie: <value> → Cookie: [REDACTED]
  (
    pattern: RegExp(r'(cookie\b[^:]*:\s*)\S+', caseSensitive: false),
    replace: (m) => '${m[1]}[REDACTED]',
  ),

  // 4. Authorization 头值：Authorization: Bearer <token> → Authorization: [REDACTED]
  //    覆盖：Bearer Token、Basic 认证、API Key 等两段式头值
  //    (?:\S+\s+)? 可选匹配 scheme（如 "Bearer "），\S+ 匹配实际凭证
  (
    pattern: RegExp(
      r'(authorization\b[^:]*:\s*)(?:\S+\s+)?\S+',
      caseSensitive: false,
    ),
    replace: (m) => '${m[1]}[REDACTED]',
  ),

  // 5. 代理用户名/密码字段（非空值）
  //    覆盖：Settings 日志 `config: proxy_password=xxx`
  //          actor 日志 `proxy config changed: proxy_password=xxx`
  (
    pattern: RegExp(
      r'(proxy[_\s]?(?:password|username)\s*[=:]\s*)\S+',
      caseSensitive: false,
    ),
    replace: (m) => '${m[1]}[REDACTED]',
  ),

  // 6. Linux 用户主目录：/home/username/ → /home/***/
  //    覆盖：save_dir、temp/dest 路径、exe 路径、图标路径
  (pattern: RegExp(r'/home/[^/\s]+/'), replace: (_) => '/home/***/'),

  // 7. Windows 用户目录：C:\Users\username\ → C:\Users\***\
  (
    pattern: RegExp(r'([A-Za-z]:\\[Uu]sers\\)[^\\\s]+\\'),
    replace: (m) => '${m[1]}***\\',
  ),
];

/// 对日志内容应用全部脱敏规则，返回脱敏后的文本。
String _sanitizeLogContent(String content) {
  for (final rule in _kSanitizeRules) {
    content = content.replaceAllMapped(rule.pattern, rule.replace);
  }
  return content;
}

/// 对日志字节内容进行脱敏，返回脱敏后的字节。
///
/// 处理流程：UTF-8 解码（allowMalformed）→ 正则替换 → UTF-8 编码
Uint8List _sanitizeLogBytes(Uint8List rawData) {
  String content;
  try {
    content = utf8.decode(rawData, allowMalformed: true);
  } catch (_) {
    // 无法解码时原样返回，不阻断导出流程
    return rawData;
  }
  return Uint8List.fromList(utf8.encode(_sanitizeLogContent(content)));
}
