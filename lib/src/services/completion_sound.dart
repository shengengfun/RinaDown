// 下载完成提示音（本地合成，零版权依赖）。参照迅雷等下载器「下载完成」
// 的听觉反馈：两声渐强的「叮咚」。Windows 上把合成 WAV 落盘后用系统
// SoundPlayer 播放；其它平台目前为 no-op（桌面目标当前以 Windows 为准）。
//
// 实现刻意不引入任何音频插件/新依赖：PCM 由本文件生成，播放走
// `powershell -WindowStyle Hidden` 调 .NET SoundPlayer（进程分离、不阻塞 UI）。

import 'dart:async';
import 'dart:io';
import 'dart:math' as math;
import 'dart:typed_data';

import 'log_service.dart';

const _tag = 'CompletionSound';

/// 单次完成提示音（幂等可并发，内部互斥重建文件）。
class CompletionSound {
  CompletionSound._();

  static final CompletionSound instance = CompletionSound._();

  /// 渲染线程互斥：合成/写盘只做一次；播放按需多开（系统级短音）。
  bool _prepared = false;
  String? _wavPath;
  bool _playing = false;

  Future<void> _ensureWav() async {
    if (_prepared && _wavPath != null) return;
    try {
      final dir = Directory.systemTemp;
      final file = File('${dir.path}${Platform.pathSeparator}fluxdown_done.wav');
      final bytes = _synthesize();
      await file.writeAsBytes(bytes, flush: true);
      _wavPath = file.path;
      _prepared = true;
    } catch (e) {
      logError(_tag, 'wav write failed: $e');
    }
  }

  /// 播放下载完成提示音（Windows）。失败静默。
  Future<void> play() async {
    if (!Platform.isWindows) return;
    if (_playing) return; // 极速连续完成时只响一次
    await _ensureWav();
    final path = _wavPath;
    if (path == null) return;
    _playing = true;
    try {
      await Process.run(
        'powershell',
        [
          '-NoProfile',
          '-WindowStyle',
          'Hidden',
          '-Command',
          "(New-Object Media.SoundPlayer '$path').PlaySync()",
        ],
        runInShell: false,
      );
    } catch (e) {
      logError(_tag, 'play failed: $e');
    } finally {
      _playing = false;
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
