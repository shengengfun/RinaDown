// FluxDown smoke tests — 仅测试纯 Dart 层，不依赖 libhub.so / native 插件。
//
// 背景：FluxDownApp 渲染时会构造 DownloadController 和 SettingsProvider，
// 二者在构造函数中立即调用 sendSignalToRust()，而测试环境没有构建 Rust 共享库，
// 导致 DynamicLibrary.open('libhub.so') 抛出 ArgumentError。
//
// 正确做法：不修改正式代码来迁就测试，而是让 smoke test 只覆盖
// 可以在测试环境中运行的纯 Dart 层逻辑。
import 'package:flutter_test/flutter_test.dart';
import 'package:flux_down/src/i18n/locale_provider.dart';
import 'package:flux_down/src/theme/theme_provider.dart';

void main() {
  // I18nStore 从资产加载翻译表（assets/i18n/*.json），需要测试绑定。
  TestWidgetsFlutterBinding.ensureInitialized();
  setUpAll(I18nStore.load);

  // ─────────────────────────────────────────────
  // ThemeProvider 初始状态
  // ─────────────────────────────────────────────

  group('ThemeProvider', () {
    test('内置主题列表非空', () {
      // builtinThemes 是顶层 final 变量，不属于 ThemeProvider 实例
      expect(builtinThemes, isNotEmpty);
    });

    test('默认暗色主题 ID 存在于内置列表', () {
      final provider = ThemeProvider();
      final ids = builtinThemes.map((e) => e.id).toList();
      expect(ids, contains(provider.selectedDarkTheme));
    });

    test('默认亮色主题 ID 存在于内置列表', () {
      final provider = ThemeProvider();
      final ids = builtinThemes.map((e) => e.id).toList();
      expect(ids, contains(provider.selectedLightTheme));
    });

    test('初始无导入主题', () {
      final provider = ThemeProvider();
      expect(provider.importedThemes, isEmpty);
    });
  });

  // ─────────────────────────────────────────────
  // LocaleNotifier 初始状态
  // ─────────────────────────────────────────────

  group('LocaleNotifier', () {
    test('构造后 preference 为 system', () {
      final notifier = LocaleNotifier();
      expect(notifier.preference, equals(kLocaleSystem));
    });

    test('currentLocale 为 zh 或 en', () {
      // currentLocale 是顶层变量，由 _resolveSystemLocale() 初始化
      expect(currentLocale, anyOf(equals('zh'), equals('en')));
    });

    test('locale 常量值正确', () {
      expect(kLocaleSystem, equals('system'));
    });

    test('I18nStore 自动发现内置语言', () {
      expect(I18nStore.available, containsAll(['zh', 'en']));
      expect(I18nStore.available.first, equals('en'));
      expect(I18nStore.resolve('zh_CN'), equals('zh'));
      expect(I18nStore.resolve('ko'), equals('en'));
      expect(I18nStore.nativeName('zh'), equals('简体中文'));
    });
  });

  // ─────────────────────────────────────────────
  // i18n 翻译系统
  // ─────────────────────────────────────────────

  group('S 翻译', () {
    test('中文实例返回中文字符串', () {
      final s = S.of('zh');
      expect(s.newDownload, equals('新建下载'));
      expect(s.cancel, equals('取消'));
      expect(s.settings, equals('设置'));
      expect(s.browse, equals('浏览'));
    });

    test('英文实例返回英文字符串', () {
      final s = S.of('en');
      expect(s.newDownload, equals('New Download'));
      expect(s.cancel, equals('Cancel'));
      expect(s.settings, equals('Settings'));
      expect(s.browse, equals('Browse'));
    });

    test('不支持的语言 fallback 到英文', () {
      final s = S.of('ja');
      expect(s.cancel, equals('Cancel'));
    });

    test('file picker 错误提示字符串非空', () {
      for (final locale in ['zh', 'en']) {
        final s = S.of(locale);
        expect(s.filePickerErrorTimeout, isNotEmpty);
        expect(s.filePickerErrorNoTool, isNotEmpty);
        expect(s.filePickerErrorNative, isNotEmpty);
        expect(s.filePickerErrorGeneric, isNotEmpty);
      }
    });

    test('参数化字符串正确插值', () {
      final s = S.of('zh');
      expect(s.urlCount(3), equals('3 个链接'));
      expect(s.startBatchDownload(5), equals('下载 5 个文件'));
      expect(s.importTxtFound(10), equals('已导入 10 个链接'));

      final en = S.of('en');
      expect(en.urlCount(3), equals('3 URLs'));
      expect(en.startBatchDownload(5), equals('Download 5 files'));
    });
  });
}
