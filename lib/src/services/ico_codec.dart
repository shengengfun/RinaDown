import 'dart:typed_data';

/// ICO 文件编解码 — 纯字节操作，无 dart:ui / 平台依赖，可独立单测。
///
/// ICO 结构：ICONDIR(6B) + ICONDIRENTRY(16B)×N + 图像数据。
/// 本模块只处理 PNG 压缩条目（Vista+ 的 LoadImage 原生支持，
/// 项目内 tray_win_dark.ico 即为此格式，由 scripts/gen_icons.ts 生成）。

/// PNG 文件魔数（前 4 字节）。
const _pngMagic = [0x89, 0x50, 0x4E, 0x47];

/// 一个待写入 ICO 的 PNG 压缩图像条目。
///
/// [size] 为正方形边长（像素），[png] 为完整 PNG 文件字节。
class IcoPngEntry {
  final int size;
  final Uint8List png;

  const IcoPngEntry({required this.size, required this.png});
}

/// 由 PNG 压缩条目构建完整 ICO 文件字节。
///
/// 条目按 [IcoPngEntry.size] 升序写入（ICO 惯例）。
/// 256 及以上尺寸的宽高字节按规范写 0。
///
/// ```dart
/// final ico = buildIcoFromPngs([IcoPngEntry(size: 16, png: png16)]);
/// ```
Uint8List buildIcoFromPngs(List<IcoPngEntry> entries) {
  if (entries.isEmpty) {
    throw ArgumentError('entries must not be empty');
  }
  final sorted = List<IcoPngEntry>.of(entries)
    ..sort((a, b) => a.size.compareTo(b.size));
  final count = sorted.length;
  final headerLen = 6 + 16 * count;
  var total = headerLen;
  for (final e in sorted) {
    total += e.png.length;
  }
  final out = Uint8List(total);
  final bd = ByteData.view(out.buffer);
  bd.setUint16(0, 0, Endian.little); // reserved
  bd.setUint16(2, 1, Endian.little); // type = 1 (icon)
  bd.setUint16(4, count, Endian.little);
  var offset = headerLen;
  for (var i = 0; i < count; i++) {
    final e = sorted[i];
    final base = 6 + 16 * i;
    final dim = e.size >= 256 ? 0 : e.size;
    out[base] = dim; // width
    out[base + 1] = dim; // height
    out[base + 2] = 0; // color count (真彩色为 0)
    out[base + 3] = 0; // reserved
    bd.setUint16(base + 4, 1, Endian.little); // color planes
    bd.setUint16(base + 6, 32, Endian.little); // bits per pixel
    bd.setUint32(base + 8, e.png.length, Endian.little); // bytes in resource
    bd.setUint32(base + 12, offset, Endian.little); // image offset
    out.setRange(offset, offset + e.png.length, e.png);
    offset += e.png.length;
  }
  return out;
}

/// 校验字节是否为 ICO 文件（ICONDIR 头：reserved=0, type=1, count>0）。
bool looksLikeIco(Uint8List bytes) {
  if (bytes.length < 6) return false;
  final bd = ByteData.sublistView(bytes);
  return bd.getUint16(0, Endian.little) == 0 &&
      bd.getUint16(2, Endian.little) == 1 &&
      bd.getUint16(4, Endian.little) > 0;
}

/// 从 ICO 字节中提取尺寸最大的 PNG 压缩条目（用于 UI 预览）。
///
/// BMP(DIB) 压缩条目 Flutter 无法解码，跳过；无任何 PNG 条目或
/// 结构损坏时返回 `null`。
Uint8List? extractLargestPngEntry(Uint8List ico) {
  if (!looksLikeIco(ico)) return null;
  final bd = ByteData.sublistView(ico);
  final count = bd.getUint16(4, Endian.little);
  if (ico.length < 6 + 16 * count) return null;
  Uint8List? best;
  var bestSize = -1;
  for (var i = 0; i < count; i++) {
    final base = 6 + 16 * i;
    // 宽度字节为 0 表示 256
    final width = ico[base] == 0 ? 256 : ico[base];
    final len = bd.getUint32(base + 8, Endian.little);
    final offset = bd.getUint32(base + 12, Endian.little);
    if (len < 4 || offset + len > ico.length) continue;
    final isPng =
        ico[offset] == _pngMagic[0] &&
        ico[offset + 1] == _pngMagic[1] &&
        ico[offset + 2] == _pngMagic[2] &&
        ico[offset + 3] == _pngMagic[3];
    if (isPng && width > bestSize) {
      bestSize = width;
      best = Uint8List.sublistView(ico, offset, offset + len);
    }
  }
  return best;
}
