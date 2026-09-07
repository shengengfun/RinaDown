import 'dart:io';

import 'package:flutter/widgets.dart';
import '../services/kv_store.dart';
import '../services/log_service.dart';
export 'i18n_store.dart';
export 'translations.dart';
import 'i18n_store.dart';
import 'translations.dart';

/// KvStore key
const _kAppLocale = 'app_locale';

/// 语言偏好值: 'system' 或任意已发现的 locale 代码（'zh'、'en'、'ja'…）
const kLocaleSystem = 'system';

/// 获取系统语言并解析为已发现的可用语言（精确 → 前缀 → en）。
String _resolveSystemLocale() =>
    I18nStore.resolve(Platform.localeName); // e.g. "zh_CN", "en_US", "ja_JP"

/// 全局 locale 实例 — 供无 context 场景使用（models, services, tray 等）。
/// 随 [LocaleNotifier] 变更自动更新。
S currentS = S.of(_resolveSystemLocale());

/// 当前实际 locale code（'zh'、'en'、'ja'…）
String currentLocale = _resolveSystemLocale();

/// 全局 LocaleNotifier 单例 — 在 main() 中创建并初始化
late final LocaleNotifier localeNotifier;

/// 运行时语言管理器
///
/// 支持三种模式: 跟随系统 / 中文 / 英文。
/// 持久化到 [KvStore]，变更时 notifyListeners 触发 UI 重建。
class LocaleNotifier extends ChangeNotifier {
  /// 用户选择的语言偏好: 'system' 或 locale 代码
  String _preference = kLocaleSystem;

  String get preference => _preference;
  S get s => currentS;

  /// 启动时调用，从 [KvStore] 恢复语言偏好（同步读取，已在 main 早期载入）。
  Future<void> init() async {
    try {
      final saved = KvStore.instance.getString(_kAppLocale);
      if (saved != null &&
          (saved == kLocaleSystem || I18nStore.available.contains(saved))) {
        _preference = saved;
      }
    } catch (e, stack) {
      logError('LocaleNotifier', 'init failed, using system locale', e, stack);
    }
    _applyLocale();
    // 静默加载，不触发 rebuild（main.dart 会在 init 完成后才 runApp）
  }

  /// 设置语言偏好并立即生效
  void setLocale(String pref) {
    if (_preference == pref) return;
    _preference = pref;
    _applyLocale();
    notifyListeners();
    _persist();
  }

  /// 根据偏好计算实际 locale 并更新全局变量
  void _applyLocale() {
    if (_preference == kLocaleSystem) {
      currentLocale = _resolveSystemLocale();
    } else {
      currentLocale = I18nStore.resolve(_preference);
    }
    currentS = S.of(currentLocale);
  }

  /// 写入语言偏好（fire-and-forget）
  Future<void> _persist() async {
    await KvStore.instance.setString(_kAppLocale, _preference);
  }
}

/// InheritedWidget 用于在 widget tree 中传递 S 实例
class LocaleScope extends InheritedWidget {
  final S s;

  const LocaleScope({super.key, required this.s, required super.child});

  static S of(BuildContext context) {
    final scope = context.dependOnInheritedWidgetOfExactType<LocaleScope>();
    return scope?.s ?? currentS;
  }

  @override
  bool updateShouldNotify(LocaleScope oldWidget) =>
      s.locale != oldWidget.s.locale;
}
