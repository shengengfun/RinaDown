// 下载完成提示音。
//
// 两条来源：
// 1. **内置合成音**（默认，零版权依赖）：参照迅雷等下载器「下载完成」的
//    听觉反馈，两声渐强的「叮咚」。PCM 由本文件生成，播放走
//    `powershell -WindowStyle Hidden` + .NET SoundPlayer（进程分离、不阻塞 UI）。
// 2. **用户音效**：`<数据目录>/sounds/` 下的音频文件，用户把 wav/mp3/ogg…
//    丢进去即可被扫描到、并在「设置 → 通知」里选择。播放走 Windows MCI
//    （`winmm.dll` 的 `mciSendStringW`，零新增依赖）；系统解不了的格式
//    （如缺解码器的 ogg）自动回退内置合成音并记一条日志。
//
// 实现刻意不引入任何音频插件/新依赖。

import 'dart:async';
import 'dart:ffi';
import 'dart:io';
import 'dart:isolate';
import 'dart:math' as math;
import 'dart:typed_data';

import 'package:ffi/ffi.dart';
import 'package:flutter/services.dart' show rootBundle;
import 'package:path/path.dart' as p;

import 'log_service.dart';
import 'platform_utils.dart';

const _tag = 'CompletionSound';

/// 单次完成提示音（幂等可并发，内部互斥重建文件）。
class CompletionSound {
  CompletionSound._();

  static final CompletionSound instance = CompletionSound._();

  /// 设置值：空字符串 = 内置合成音。
  static const String builtinId = '';

  /// 首次运行时释放到用户音效目录的内置音效（asset → 文件名）。
  static const List<String> _bundledSounds = ['duniang.mp3', 'moulei.mp3'];

  /// 扫描时接受的音频扩展名（也决定同名多扩展时的优先次序）。
  static const List<String> supportedExtensions = [
    'wav',
    'mp3',
    'ogg',
    'oga',
    'opus',
    'flac',
    'm4a',
    'aac',
    'wma',
    'mka',
  ];

  /// 渲染线程互斥：合成/写盘只做一次；播放按需多开（系统级短音）。
  bool _prepared = false;
  String? _wavPath;
  bool _playing = false;

  /// 内置音效释放只做一次（见 [ensureReady]）。
  Future<void>? _seeded;

  // ─────────────────────────────────────────────
  // 音效目录
  // ─────────────────────────────────────────────

  /// 用户音效目录：`<数据目录>/sounds`。可直接往里拖文件。
  Directory get soundsDir => Directory(p.join(resolveDataDir(), 'sounds'));

  /// 确保音效目录存在，并在**目录本就不存在**时释放内置音效。
  ///
  /// 只在「目录不存在」时释放（而不是逐个文件检查）：用户把某个内置音效
  /// 删掉后不再被塞回来；把整个目录删掉则恢复出厂音效。
  Future<void> ensureReady() async {
    _seeded ??= _seedBundledSounds();
    await _seeded;
  }

  Future<void> _seedBundledSounds() async {
    try {
      final dir = soundsDir;
      if (dir.existsSync()) return;
      await dir.create(recursive: true);
      for (final name in _bundledSounds) {
        try {
          final data = await rootBundle.load('assets/sounds/$name');
          await File(p.join(dir.path, name)).writeAsBytes(
            data.buffer.asUint8List(data.offsetInBytes, data.lengthInBytes),
            flush: true,
          );
        } catch (e) {
          logError(_tag, 'seed bundled sound "$name" failed: $e');
        }
      }
    } catch (e) {
      logError(_tag, 'ensure sounds dir failed: $e');
    }
  }

  /// 扫描音效目录，返回可选音效 id（= 文件名去掉扩展名，按名排序）。
  ///
  /// 同名不同扩展只取第一个（按 [supportedExtensions] 次序），避免下拉里
  /// 出现两个一样的名字。
  List<String> listSoundNames() {
    try {
      final dir = soundsDir;
      if (!dir.existsSync()) return const [];
      final files = dir.listSync().whereType<File>().toList()
        ..sort((a, b) => p.basename(a.path).compareTo(p.basename(b.path)));
      final seen = <String>{};
      final out = <String>[];
      for (final f in files) {
        final ext = p.extension(f.path).toLowerCase().replaceFirst('.', '');
        if (!supportedExtensions.contains(ext)) continue;
        final id = p.basenameWithoutExtension(f.path);
        if (id.isEmpty || !seen.add(id)) continue;
        out.add(id);
      }
      return out;
    } catch (e) {
      logError(_tag, 'scan sounds dir failed: $e');
      return const [];
    }
  }

  /// 音效 id → 绝对路径（找不到返回 null）。
  String? resolveSoundPath(String id) {
    if (id.isEmpty) return null;
    for (final ext in supportedExtensions) {
      final f = File(p.join(soundsDir.path, '$id.$ext'));
      if (f.existsSync()) return f.path;
    }
    return null;
  }

  // ─────────────────────────────────────────────
  // 播放
  // ─────────────────────────────────────────────

  /// 播放下载完成提示音。`soundId` 为空 → 内置合成音；指定音效不存在或
  /// 本机解不了 → 回退内置合成音。失败静默。
  Future<void> play({String soundId = builtinId}) async {
    if (!Platform.isWindows) return;
    if (_playing) return; // 极速连续完成时只响一次
    _playing = true;
    try {
      if (soundId.isNotEmpty) {
        final path = resolveSoundPath(soundId);
        if (path != null && await _playFile(path)) return;
        // 本机解不了（缺解码器的 ogg / 文件损坏）——回退内置音并留痕，
        // 用户看日志就知道该换格式，而不是「选了却没声音」。
        logError(_tag, 'sound "$soundId" unplayable, using built-in chime');
      }
      await _playBuiltin();
    } catch (e) {
      logError(_tag, 'play failed: $e');
    } finally {
      _playing = false;
    }
  }

  /// 内置合成音（写盘 + SoundPlayer）。
  Future<void> _playBuiltin() async {
    await _ensureWav();
    final path = _wavPath;
    if (path == null) return;
    try {
      await Process.run('powershell', [
        '-NoProfile',
        '-WindowStyle',
        'Hidden',
        '-Command',
        "(New-Object Media.SoundPlayer '$path').PlaySync()",
      ], runInShell: false);
    } catch (e) {
      logError(_tag, 'builtin play failed: $e');
    }
  }

  /// 用 Windows MCI 播放一个音频文件；返回 false = 本机解不了这个格式。
  ///
  /// `play <alias> wait` 是阻塞调用，故整段跑在独立 isolate 里（不占 UI
  /// 线程），并按 `status length` 限时——用户放一首 5 分钟的歌也不会挂住。
  Future<bool> _playFile(String path) {
    return Isolate.run(() => _playFileSync(path));
  }

  Future<void> _ensureWav() async {
    if (_prepared && _wavPath != null) return;
    try {
      final dir = Directory.systemTemp;
      final file = File('${dir.path}${Platform.pathSeparator}rinadown_done.wav');
      final bytes = _synthesize();
      await file.writeAsBytes(bytes, flush: true);
      _wavPath = file.path;
      _prepared = true;
    } catch (e) {
      logError(_tag, 'wav write failed: $e');
    }
  }

  /// 合成一段「叮咚」双音 PCM（44.1kHz 单声道 16-bit）。
  Uint8List _synthesize() {
    const sampleRate = 44100;
    // 两段短音：E5(659) 100ms → A5(880) 220ms，之间留 40ms 静音，尾部淡出。
    final notes = <(double, double)>[(659.0, 0.10), (880.0, 0.22)];
    const gapSec = 0.04;
    final total =
        notes.fold<double>(0, (sum, n) => sum + n.$2) + gapSec + 0.15;
    final count = (total * sampleRate).round();
    final data = Int16List(count);
    var cursor = 0;
    for (final note in notes) {
      final freq = note.$1;
      final nSamples = (note.$2 * sampleRate).round();
      for (var i = 0; i < nSamples; i++) {
        final t = i / sampleRate;
        final attack = math.min(1.0, t / 0.012);
        final release = math.min(1.0, (note.$2 - t) / 0.05);
        final env = attack * release;
        final v =
            math.sin(2 * math.pi * freq * t) * 0.28 * env +
            math.sin(4 * math.pi * freq * t) * 0.06 * env; // 基频+轻微二次谐波
        data[cursor++] = (v * 32767).round().clamp(-32768, 32767);
      }
      // 音间静音
      for (var i = 0; i < (gapSec * sampleRate).round(); i++) {
        data[cursor++] = 0;
      }
    }
    // 尾部淡出（释放尾）
    for (var i = 0; i < (0.15 * sampleRate).round() && cursor < count; i++) {
      final f = 1 - i / (0.15 * sampleRate);
      data[cursor] = (data[cursor] * f).round();
      cursor++;
    }
    // 组装 WAV（PCM, mono, 16-bit）
    final header = ByteData(44);
    header.setUint8(0, 0x52); // 'R'
    header.setUint8(1, 0x49); // 'I'
    header.setUint8(2, 0x46); // 'F'
    header.setUint8(3, 0x46); // 'F'
    header.setUint32(4, 36 + count * 2, Endian.little);
    header.setUint8(8, 0x57); // 'W'
    header.setUint8(9, 0x41); // 'A'
    header.setUint8(10, 0x56); // 'V'
    header.setUint8(11, 0x45); // 'E'
    header.setUint8(12, 0x66); // 'f'
    header.setUint8(13, 0x6d); // 'm'
    header.setUint8(14, 0x74); // 't'
    header.setUint8(15, 0x20); // ' '
    header.setUint32(16, 16, Endian.little); // PCM fmt chunk size
    header.setUint16(20, 1, Endian.little); // audio format = PCM
    header.setUint16(22, 1, Endian.little); // channels = mono
    header.setUint32(24, sampleRate, Endian.little);
    header.setUint32(28, sampleRate * 2, Endian.little); // byte rate
    header.setUint16(32, 2, Endian.little); // block align
    header.setUint16(34, 16, Endian.little); // bits per sample
    header.setUint8(36, 0x64); // 'd'
    header.setUint8(37, 0x61); // 'a'
    header.setUint8(38, 0x74); // 't'
    header.setUint8(39, 0x61); // 'a'
    header.setUint32(40, count * 2, Endian.little);

    final bytes = ByteData(44 + count * 2);
    bytes.buffer.asUint8List().setAll(0, header.buffer.asUint8List());
    final out = bytes.buffer.asUint8List();
    final pcm = data.buffer.asUint8List();
    out.setRange(44, out.length, pcm);
    return out;
  }
}

// ─────────────────────────────────────────────
// Windows MCI（winmm.dll）— 跑在独立 isolate 里的同步实现
// ─────────────────────────────────────────────

typedef _MciSendStringNative =
    Int32 Function(Pointer<Utf16>, Pointer<Utf16>, Uint32, Pointer<IntPtr>);
typedef _MciSendStringDart =
    int Function(Pointer<Utf16>, Pointer<Utf16>, int, Pointer<IntPtr>);

/// MCI 别名（同一时刻只播一个音，故固定名即可；`open` 前先 `close` 兜底）。
const String _mciAlias = 'rinadown_completion_sound';

/// 单个音效最长播放时长（毫秒）——防止用户塞长音频把 isolate 挂住。
const int _maxPlayMs = 10000;

bool _playFileSync(String path) {
  DynamicLibrary winmm;
  try {
    winmm = DynamicLibrary.open('winmm.dll');
  } catch (e) {
    logError(_tag, 'winmm.dll unavailable: $e');
    return false;
  }
  final send = winmm
      .lookupFunction<_MciSendStringNative, _MciSendStringDart>(
        'mciSendStringW',
      );

  int call(String cmd) {
    final cmdPtr = cmd.toNativeUtf16();
    final bufPtr = calloc<Uint16>(512);
    try {
      return send(cmdPtr, bufPtr.cast<Utf16>(), 512, nullptr);
    } finally {
      calloc.free(cmdPtr);
      calloc.free(bufPtr);
    }
  }

  // 上一次异常退出可能留下未关闭的别名。
  call('close $_mciAlias');

  // 路径用双引号包裹（MCI 语法）；路径里出现双引号属病态输入，直接放弃。
  if (path.contains('"')) return false;
  if (call('open "$path" alias $_mciAlias') != 0) return false;

  try {
    if (call('play $_mciAlias') != 0) return false;
    final lengthMs = _queryLengthMs(send, _mciAlias);
    final waitMs = lengthMs <= 0 ? 2000 : math.min(lengthMs + 300, _maxPlayMs);
    sleep(Duration(milliseconds: waitMs));
    return true;
  } finally {
    call('close $_mciAlias');
  }
}

/// `status <alias> length` → 毫秒（失败/未知返回 0）。
int _queryLengthMs(_MciSendStringDart send, String alias) {
  final cmd = 'status $alias length'.toNativeUtf16();
  final buf = calloc<Uint16>(64);
  try {
    if (send(cmd, buf.cast<Utf16>(), 64, nullptr) != 0) return 0;
    return int.tryParse(buf.cast<Utf16>().toDartString().trim()) ?? 0;
  } catch (_) {
    return 0;
  } finally {
    calloc.free(cmd);
    calloc.free(buf);
  }
}
