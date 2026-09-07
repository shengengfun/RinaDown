// Regression tests for the Win32 Toast offscreen card renderer
// (lib/src/services/win32_toast/toast_card_renderer.dart).
//
// Deliberately NOT using `testWidgets()`: `_rasterizeWidget` hand-assembles a
// BuildOwner/RenderView/RenderRepaintBoundary pipeline and awaits the real
// async `RenderRepaintBoundary.toImage()`. `testWidgets()` runs its callback
// inside a FakeAsync zone (so `pump()` can fast-forward time), and that zone
// never lets the real `toImage()` future resolve, hanging the test. Plain
// `test()` after `TestWidgetsFlutterBinding.ensureInitialized()` runs in the
// real event loop while still providing the `PlatformDispatcher`/test view
// that `_rasterizeWidget` reads via `WidgetsBinding.instance`, which is all
// the renderer needs — no widget tree pumping involved.
//
// Out of scope: win32_toast_window.dart (pure Win32 FFI; not testable off
// Windows-with-native-bindings and not part of this renderer's contract).

import 'package:flutter_test/flutter_test.dart';
import 'package:flux_down/src/services/win32_toast/toast_card_renderer.dart';
import 'package:flux_down/src/theme/flux_theme_tokens.dart';

ToastCardSpec _spec() => ToastCardSpec(
  title: 'Download complete',
  fileName: 'archive.zip',
  fileExt: 'zip',
  subtitle: '12.4 MB',
  openFolderLabel: 'Open folder',
  openFileLabel: 'Open file',
  tokens: FluxThemeTokens.defaultDark(),
);

int _alphaAt(ToastCardImage img, int x, int y) {
  final idx = (y * img.width + x) * 4;
  return img.bgraPremultiplied[idx + 3];
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  // Rendering is expensive (4 offscreen rasterizations per call); render
  // once per scale and share the immutable output across assertions below.
  late List<ToastCardImage> variants1x;
  late List<ToastCardImage> variants15x;

  setUpAll(() async {
    variants1x = await renderToastCardVariants(_spec(), scale: 1.0);
    variants15x = await renderToastCardVariants(_spec(), scale: 1.5);
  });

  group('renderToastCardVariants — pipeline integrity (scale 1.0)', () {
    test('renders exactly one image per ToastVariant', () {
      expect(variants1x.length, ToastVariant.values.length);
    });

    test('each variant is a 380x190 buffer with a full BGRA byte plane', () {
      for (final img in variants1x) {
        expect(img.width, 380);
        expect(img.height, 190);
        expect(img.bgraPremultiplied.length, img.width * img.height * 4);
      }
    });
  });

  group('renderToastCardVariants — DPI scaling', () {
    test('scale 1.5 scales physical pixel dimensions proportionally', () {
      for (final img in variants15x) {
        expect(img.width, 570);
        expect(img.height, 285);
        expect(img.bgraPremultiplied.length, img.width * img.height * 4);
      }
    });
  });

  group('renderToastCardVariants — premultiplied BGRA invariant', () {
    test('every pixel satisfies B<=A, G<=A, R<=A in every variant', () {
      for (final img in variants1x) {
        final bytes = img.bgraPremultiplied;
        String? violation;
        for (var i = 0; i < bytes.length && violation == null; i += 4) {
          final b = bytes[i];
          final g = bytes[i + 1];
          final r = bytes[i + 2];
          final a = bytes[i + 3];
          if (b > a || g > a || r > a) {
            final px = i ~/ 4;
            violation =
                'pixel (${px % img.width},${px ~/ img.width}): '
                'B=$b G=$g R=$r A=$a';
          }
        }
        expect(
          violation,
          isNull,
          reason: 'non-premultiplied pixel found: $violation',
        );
      }
    });
  });

  group('renderToastCardVariants — alpha shape', () {
    test('the four window corners are fully transparent', () {
      final base = variants1x[ToastVariant.base.index];
      final corners = <(int, int)>[
        (0, 0),
        (base.width - 1, 0),
        (0, base.height - 1),
        (base.width - 1, base.height - 1),
      ];
      for (final (x, y) in corners) {
        expect(
          _alphaAt(base, x, y),
          0,
          reason: 'corner ($x,$y) should be outside the shadow bleed',
        );
      }
    });

    test('the card center is fully opaque', () {
      final base = variants1x[ToastVariant.base.index];
      expect(_alphaAt(base, 190, 95), 255);
    });
  });

  group('renderToastCardVariants — hover variant differences', () {
    test('all four hover variants render distinct pixel buffers', () {
      for (var i = 0; i < variants1x.length; i++) {
        for (var j = i + 1; j < variants1x.length; j++) {
          expect(
            variants1x[i].bgraPremultiplied,
            isNot(equals(variants1x[j].bgraPremultiplied)),
            reason:
                '${ToastVariant.values[i]} should render differently from '
                '${ToastVariant.values[j]}',
          );
        }
      }
    });

    test('base vs hoverFile: differing pixels stay inside kToastHitFile', () {
      final base = variants1x[ToastVariant.base.index];
      final hoverFile = variants1x[ToastVariant.hoverFile.index];
      var diffCount = 0;
      for (var y = 0; y < base.height; y++) {
        for (var x = 0; x < base.width; x++) {
          final idx = (y * base.width + x) * 4;
          var differs = false;
          for (var k = 0; k < 4; k++) {
            if (base.bgraPremultiplied[idx + k] !=
                hoverFile.bgraPremultiplied[idx + k]) {
              differs = true;
              break;
            }
          }
          if (!differs) continue;
          diffCount++;
          expect(
            kToastHitFile.contains(Offset(x.toDouble(), y.toDouble())),
            isTrue,
            reason: 'diff pixel ($x,$y) falls outside kToastHitFile',
          );
        }
      }
      // Guard against a vacuous pass: the loop above only ever asserts on
      // pixels that differ, so if rendering regressed to identical output
      // it would silently produce zero assertions.
      expect(diffCount, greaterThan(0));
    });
  });

  group('hit region constants', () {
    test('each hit rect lies fully within the window bounds', () {
      for (final r in [kToastHitClose, kToastHitFolder, kToastHitFile]) {
        expect(r.left, greaterThanOrEqualTo(0));
        expect(r.top, greaterThanOrEqualTo(0));
        expect(r.right, lessThanOrEqualTo(kToastWindowW));
        expect(r.bottom, lessThanOrEqualTo(kToastWindowH));
      }
    });

    test('hit rects are pairwise non-overlapping', () {
      expect(kToastHitClose.overlaps(kToastHitFolder), isFalse);
      expect(kToastHitClose.overlaps(kToastHitFile), isFalse);
      expect(kToastHitFolder.overlaps(kToastHitFile), isFalse);
    });
  });
}
