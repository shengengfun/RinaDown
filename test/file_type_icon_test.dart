import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flux_down/src/i18n/locale_provider.dart';
import 'package:flux_down/src/models/download_task.dart';
import 'package:flux_down/src/theme/app_colors.dart';
import 'package:flux_down/src/theme/app_theme.dart';
import 'package:flux_down/src/theme/flux_theme_tokens.dart';
import 'package:flux_down/src/widgets/bt_file_selection_shared.dart'
    show btFileIcon;
import 'package:flux_down/src/widgets/file_type_icon.dart';
import 'package:shadcn_ui/shadcn_ui.dart';

Widget _wrap(Widget child) {
  final tokens = FluxThemeTokens.defaultDark();
  final theme = buildThemeFromTokens(tokens);
  return FluxThemeScope(
    tokens: tokens,
    child: ShadTheme(
      data: theme,
      child: LocaleScope(
        s: S.of('zh'),
        child: Directionality(
          textDirection: TextDirection.ltr,
          child: DefaultTextStyle(
            style: theme.textTheme.p,
            child: Center(child: child),
          ),
        ),
      ),
    ),
  );
}

void main() {
  group('fileTypeIcon', () {
    test('精确表优先于分类回落', () {
      // iso 属 archive，但光盘字形比通用压缩包更达意
      expect(fileTypeIcon('iso'), LucideIcons.disc);
      expect(fileTypeIcon('zip'), LucideIcons.archive);
      // 三种安装包同属 program，字形必须各不相同
      expect(fileTypeIcon('exe'), LucideIcons.appWindow);
      expect(fileTypeIcon('apk'), LucideIcons.smartphone);
      expect(fileTypeIcon('deb'), LucideIcons.package);
      expect(fileTypeIcon('torrent'), LucideIcons.magnet);
    });

    test('ts 是 MPEG-TS 视频切片，不得被当作 TypeScript 源码', () {
      expect(fileTypeIcon('ts'), LucideIcons.film);
      expect(fileTypeIcon('tsx'), LucideIcons.fileCode);
    });

    test('未命中精确表时回落分类字形', () {
      expect(fileTypeIcon('mkv'), fileCategoryIcon(FileCategory.video));
      expect(fileTypeIcon('flac'), fileCategoryIcon(FileCategory.audio));
      expect(fileTypeIcon('png'), fileCategoryIcon(FileCategory.image));
      expect(fileTypeIcon('txt'), fileCategoryIcon(FileCategory.document));
    });

    test('大小写不敏感；无扩展名落到通用文件字形', () {
      expect(fileTypeIcon('EXE'), fileTypeIcon('exe'));
      expect(fileTypeIcon('MkV'), fileTypeIcon('mkv'));
      // DownloadTask.fileExtension 对无扩展名的文件返回 '?'
      expect(fileTypeIcon('?'), LucideIcons.file);
      expect(fileTypeIcon('zzzz'), LucideIcons.file);
    });
  });

  group('btFileIcon', () {
    test('从路径尾部取扩展名，与 fileTypeIcon 同源', () {
      expect(btFileIcon('Example/Season 1/Episode 1.mkv'), fileTypeIcon('mkv'));
      expect(btFileIcon(r'Example\pack.iso'), fileTypeIcon('iso'));
    });

    test('目录名含点、或文件无扩展名时不误判', () {
      expect(btFileIcon('v1.2.3/README'), LucideIcons.file);
      expect(btFileIcon('folder.d/data.'), LucideIcons.file);
    });
  });

  group('FileTypeIconTile', () {
    testWidgets('渲染图标而非扩展名文字，长扩展名不再进入布局', (tester) async {
      await tester.pumpWidget(
        _wrap(
          const FileTypeIconTile(
            ext: 'appinstaller',
            size: 34,
            borderRadius: BorderRadius.zero,
          ),
        ),
      );

      expect(find.text('appinstaller'), findsNothing);
      final icon = tester.widget<Icon>(find.byType(Icon));
      expect(icon.icon, LucideIcons.appWindow);
      expect(tester.takeException(), isNull);
    });

    testWidgets('着色按分类：video 专属色，program 回落中性', (tester) async {
      await tester.pumpWidget(
        _wrap(
          const Row(
            children: [
              FileTypeIconTile(
                key: ValueKey('v'),
                ext: 'mkv',
                size: 34,
                borderRadius: BorderRadius.zero,
              ),
              FileTypeIconTile(
                key: ValueKey('p'),
                ext: 'exe',
                size: 34,
                borderRadius: BorderRadius.zero,
              ),
            ],
          ),
        ),
      );

      final ctx = tester.element(find.byKey(const ValueKey('v')));
      final c = AppColors.of(ctx);
      Icon iconOf(String key) => tester.widget<Icon>(
        find.descendant(
          of: find.byKey(ValueKey(key)),
          matching: find.byType(Icon),
        ),
      );

      expect(iconOf('v').color, AppColors.categoryVideo);
      expect(iconOf('p').color, c.textSecondary);
    });
  });
}
