// i18n 翻译表加载与语言自动发现。
//
// 翻译源文件为 assets/i18n/<locale>.json（en 为源语言，社区经 Weblate 贡献
// 其他语言）。可用语言由 AssetManifest 在运行时自动发现：新增 <locale>.json
// 资产（重新构建后）即自动出现在语言选择器，无需改代码。
//
// 键级回退：缺失或空串的键回退英文，再回退键名本身。

import 'dart:convert';

import 'package:flutter/services.dart';

import '../services/log_service.dart';

const _tag = 'I18nStore';
const _assetDir = 'assets/i18n/';

class I18nStore {
  I18nStore._();

  /// locale code -> 键值表。en 表兼作全局兜底。
  static final Map<String, Map<String, String>> _tables = {};

  /// 已发现的语言代码（en、zh 置顶，其余按代码排序）。
  static List<String> available = const ['en', 'zh'];

  /// 启动时调用（main / popup 两个引擎入口都需要）。
  /// 从 AssetManifest 发现并加载全部 assets/i18n/*.json。
  static Future<void> load() async {
    try {
      final manifest = await AssetManifest.loadFromAssetBundle(rootBundle);
      final paths = manifest
          .listAssets()
          .where((p) => p.startsWith(_assetDir) && p.endsWith('.json'))
          .toList();
      for (final path in paths) {
        final code = path
            .substring(_assetDir.length, path.length - '.json'.length)
            .toLowerCase();
        try {
          final raw = await rootBundle.loadString(path);
          final map = (jsonDecode(raw) as Map<String, dynamic>).map(
            (k, v) => MapEntry(k, v as String),
          );
          _tables[code] = map;
        } catch (e) {
          logError(_tag, 'failed to load $path', e);
        }
      }
      if (_tables.isNotEmpty) {
        final codes = _tables.keys.toList()
          ..sort((a, b) {
            int rank(String c) => c == 'en'
                ? 0
                : c == 'zh'
                ? 1
                : 2;
            final r = rank(a) - rank(b);
            return r != 0 ? r : a.compareTo(b);
          });
        available = codes;
      }
      logInfo(_tag, 'loaded ${_tables.length} locales: $available');
    } catch (e, stack) {
      logError(_tag, 'i18n load failed', e, stack);
    }
  }

  /// 将任意 locale 标识（如 "zh_CN"、"ja-JP"、"en"）解析为可用语言代码。
  /// 精确匹配优先，其次主语言前缀匹配，最后回退 en。
  static String resolve(String locale) {
    final lower = locale.toLowerCase().replaceAll('_', '-');
    if (available.contains(lower)) return lower;
    final prefix = lower.split('-').first;
    if (available.contains(prefix)) return prefix;
    for (final code in available) {
      if (code.split('-').first == prefix) return code;
    }
    return 'en';
  }

  /// 查表 + `{name}` 占位插值；空串视为未翻译，键级回退英文。
  static String lookup(
    String locale,
    String key, [
    Map<String, Object?>? args,
  ]) {
    var value = _tables[locale]?[key];
    if (value == null || value.isEmpty) value = _tables['en']?[key];
    if (value == null || value.isEmpty) value = key;
    if (args != null) {
      args.forEach((k, v) {
        value = value!.replaceAll('{$k}', '$v');
      });
    }
    return value!;
  }

  /// 语言自称（选择器显示用）；语言文件缺失该键时回退语言代码。
  static String nativeName(String code) {
    final v = _tables[code]?['languageNativeName'];
    return (v == null || v.isEmpty) ? code : v;
  }
}
