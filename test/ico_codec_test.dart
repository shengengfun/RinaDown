import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:flux_down/src/services/ico_codec.dart';

/// Builds a minimal fake "PNG" blob: real PNG magic (8 bytes) followed by a
/// trailing [marker] byte so blobs from different entries are distinguishable
/// when compared byte-for-byte.
Uint8List _fakePng(int marker, {int length = 12}) {
  assert(length >= 9);
  final bytes = Uint8List(length);
  bytes[0] = 0x89;
  bytes[1] = 0x50;
  bytes[2] = 0x4E;
  bytes[3] = 0x47;
  bytes[4] = 0x0D;
  bytes[5] = 0x0A;
  bytes[6] = 0x1A;
  bytes[7] = 0x0A;
  bytes[length - 1] = marker;
  return bytes;
}

/// A hand-specified ICONDIRENTRY payload used to build ICO files the public
/// [buildIcoFromPngs] API cannot produce (e.g. non-PNG payloads), so
/// [extractLargestPngEntry]'s defensive paths can be exercised directly.
class _RawEntry {
  const _RawEntry({
    required this.width,
    required this.height,
    required this.payload,
  });

  final int width;
  final int height;
  final Uint8List payload;
}

/// Hand-builds an ICO file byte-for-byte, independent of
/// [buildIcoFromPngs], preserving entry order as given (no sorting).
Uint8List _buildRawIco(List<_RawEntry> entries) {
  final headerLen = 6 + 16 * entries.length;
  var total = headerLen;
  for (final e in entries) {
    total += e.payload.length;
  }
  final out = Uint8List(total);
  final bd = ByteData.sublistView(out);
  bd.setUint16(0, 0, Endian.little); // reserved
  bd.setUint16(2, 1, Endian.little); // type = 1 (icon)
  bd.setUint16(4, entries.length, Endian.little);

  var offset = headerLen;
  for (var i = 0; i < entries.length; i++) {
    final e = entries[i];
    final base = 6 + 16 * i;
    out[base] = e.width;
    out[base + 1] = e.height;
    bd.setUint32(base + 8, e.payload.length, Endian.little); // bytes in res
    bd.setUint32(base + 12, offset, Endian.little); // image offset
    out.setRange(offset, offset + e.payload.length, e.payload);
    offset += e.payload.length;
  }
  return out;
}

void main() {
  group('buildIcoFromPngs', () {
    test('throws ArgumentError for an empty entry list', () {
      expect(() => buildIcoFromPngs(const []), throwsArgumentError);
    });

    test(
      'writes a well-formed ICONDIR + ascending entries even when input is '
      'out of order, with size-0/256 width-height rules and correct offsets',
      () {
        final png16 = _fakePng(0x16, length: 10);
        final png256 = _fakePng(0x99, length: 20);

        // Deliberately pass 256px first to verify ascending re-sort.
        final ico = buildIcoFromPngs([
          IcoPngEntry(size: 256, png: png256),
          IcoPngEntry(size: 16, png: png16),
        ]);

        final bd = ByteData.sublistView(ico);

        // ICONDIR header (little-endian).
        expect(bd.getUint16(0, Endian.little), 0, reason: 'reserved');
        expect(bd.getUint16(2, Endian.little), 1, reason: 'type = icon');
        expect(bd.getUint16(4, Endian.little), 2, reason: 'count');

        // Entry 0 must be the 16px icon — ascending order is enforced
        // regardless of the caller's input order.
        const entry0Base = 6;
        const entry1Base = 6 + 16;

        expect(ico[entry0Base], 16, reason: '16px width byte');
        expect(ico[entry0Base + 1], 16, reason: '16px height byte');
        expect(ico[entry1Base], 0, reason: '256px width byte must be 0');
        expect(ico[entry1Base + 1], 0, reason: '256px height byte must be 0');

        final bytesInRes0 = bd.getUint32(entry0Base + 8, Endian.little);
        final offset0 = bd.getUint32(entry0Base + 12, Endian.little);
        final bytesInRes1 = bd.getUint32(entry1Base + 8, Endian.little);
        final offset1 = bd.getUint32(entry1Base + 12, Endian.little);

        expect(bytesInRes0, png16.length);
        expect(bytesInRes1, png256.length);

        // Offsets must point at the exact PNG blobs — verified by slicing
        // the actual output bytes rather than trusting internal bookkeeping.
        expect(ico.sublist(offset0, offset0 + bytesInRes0), png16);
        expect(ico.sublist(offset1, offset1 + bytesInRes1), png256);

        // First blob starts right after the header (6 + 16*2 = 38); second
        // blob is contiguous right after the first.
        expect(offset0, 6 + 16 * 2);
        expect(offset1, offset0 + png16.length);
      },
    );

    test('sorts three unordered entries ascending by size', () {
      final png16 = _fakePng(0xA1);
      final png32 = _fakePng(0xA2);
      final png256 = _fakePng(0xA3);

      final ico = buildIcoFromPngs([
        IcoPngEntry(size: 256, png: png256),
        IcoPngEntry(size: 16, png: png16),
        IcoPngEntry(size: 32, png: png32),
      ]);

      final bd = ByteData.sublistView(ico);
      expect(bd.getUint16(4, Endian.little), 3);

      final widths = [for (var i = 0; i < 3; i++) ico[6 + 16 * i]];
      // 16 -> 16, 32 -> 32, 256 -> 0 (256 is encoded as 0).
      expect(widths, [16, 32, 0]);
    });
  });

  group('looksLikeIco', () {
    test('returns true for buildIcoFromPngs output', () {
      final ico = buildIcoFromPngs([IcoPngEntry(size: 16, png: _fakePng(1))]);
      expect(looksLikeIco(ico), isTrue);
    });

    test('returns false for empty bytes', () {
      expect(looksLikeIco(Uint8List(0)), isFalse);
    });

    test('returns false for fewer than 6 bytes', () {
      expect(looksLikeIco(Uint8List.fromList([0, 0, 1, 0, 1])), isFalse);
    });

    test('returns false for a PNG file header', () {
      expect(looksLikeIco(_fakePng(0)), isFalse);
    });

    test('returns false for type=2 (cursor .cur files)', () {
      final cur = _buildRawIco([
        _RawEntry(width: 16, height: 16, payload: _fakePng(1)),
      ]);
      // Flip ICONDIR.type from 1 (icon) to 2 (cursor).
      ByteData.sublistView(cur).setUint16(2, 2, Endian.little);
      expect(looksLikeIco(cur), isFalse);
    });
  });

  group('extractLargestPngEntry', () {
    test('returns the largest PNG entry bytes from build output', () {
      final png16 = _fakePng(0x16);
      final png32 = _fakePng(0x32);
      final png256 = _fakePng(0x99, length: 30);

      final ico = buildIcoFromPngs([
        IcoPngEntry(size: 16, png: png16),
        IcoPngEntry(size: 256, png: png256),
        IcoPngEntry(size: 32, png: png32),
      ]);

      final extracted = extractLargestPngEntry(ico);
      expect(extracted, isNotNull);
      expect(extracted, png256);
    });

    test('returns null when the only entry is a non-PNG (BMP-style) blob', () {
      final bmpLikePayload = Uint8List.fromList([0x28, 0, 0, 0, 1, 2, 3, 4]);
      final ico = _buildRawIco([
        _RawEntry(width: 32, height: 32, payload: bmpLikePayload),
      ]);
      expect(extractLargestPngEntry(ico), isNull);
    });

    test('skips a non-PNG entry and returns the remaining PNG entry', () {
      final bmpLikePayload = Uint8List.fromList([0x28, 0, 0, 0, 1, 2, 3, 4]);
      final png16 = _fakePng(0x16);
      final ico = _buildRawIco([
        _RawEntry(width: 32, height: 32, payload: bmpLikePayload),
        _RawEntry(width: 16, height: 16, payload: png16),
      ]);
      expect(extractLargestPngEntry(ico), png16);
    });

    test(
      'does not throw and returns null when an entry offset+length is out '
      'of bounds (corrupted ICO)',
      () {
        final ico = _buildRawIco([
          _RawEntry(width: 16, height: 16, payload: _fakePng(1)),
        ]);
        // Corrupt the single entry's image offset to point past the buffer.
        ByteData.sublistView(
          ico,
        ).setUint32(6 + 12, ico.length + 1000, Endian.little);

        expect(() => extractLargestPngEntry(ico), returnsNormally);
        expect(extractLargestPngEntry(ico), isNull);
      },
    );

    test(
      'does not throw and returns null when the header count exceeds the '
      'entries actually present in the buffer',
      () {
        final ico = _buildRawIco([
          _RawEntry(width: 16, height: 16, payload: _fakePng(1)),
        ]);
        // Header claims 5 entries but the buffer only has room for 1.
        ByteData.sublistView(ico).setUint16(4, 5, Endian.little);

        expect(() => extractLargestPngEntry(ico), returnsNormally);
        expect(extractLargestPngEntry(ico), isNull);
      },
    );

    test('returns null for non-ICO bytes', () {
      expect(extractLargestPngEntry(_fakePng(0)), isNull);
      expect(extractLargestPngEntry(Uint8List(0)), isNull);
    });
  });
}
