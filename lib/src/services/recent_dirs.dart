// 最近使用的下载/保存目录（最多 [maxItems] 条，新→旧）。
//
// 数据落在 KvStore（便携模式=exe 目录 settings.json，安装模式=
// SharedPreferences），与 ViewPrefs/云同步外的其它本地小偏好同源。
// 任何成功建任务的目录都会经 [push] 记录；UI（DirPickerField 的历史
// 图标 / 视频解析弹窗）读取 [items] 提供「最近 5 个目录」下拉。

import 'dart:convert';

import 'kv_store.dart';

const String kRecentDirsKey = 'recent_dirs';
const int kRecentDirsMax = 5;

/// 最近目录表（进程内缓存 + KvStore 持久化）。全部同步接口。
class RecentDirs {
  RecentDirs._();

  static final RecentDirs instance = RecentDirs._();

  List<String>? _cache;

  List<String> _load() {
    final raw = KvStore.instance.getString(kRecentDirsKey);
    if (raw == null || raw.isEmpty) return const [];
    try {
      final decoded = jsonDecode(raw);
      if (decoded is! List) return const [];
      return [
        for (final e in decoded)
          if (e is String && e.trim().isNotEmpty) e.trim(),
      ];
    } catch (_) {
      return const [];
    }
  }

  /// 最近目录（新→旧，最多 [kRecentDirsMax] 条）。
  List<String> get items => _cache ??= _load();

  /// 把一个目录顶到最近（去重、裁剪到上限、落盘）。
  void push(String dir) {
    final trimmed = dir.trim();
    if (trimmed.isEmpty) return;
    final list = List<String>.of(items)
      ..remove(trimmed)
      ..insert(0, trimmed);
    if (list.length > kRecentDirsMax) {
      list.removeRange(kRecentDirsMax, list.length);
    }
    _cache = list;
    KvStore.instance.setString(kRecentDirsKey, jsonEncode(list));
  }
}
