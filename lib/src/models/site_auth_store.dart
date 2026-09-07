/// 站点凭据表（config 键 `site_auth_credentials`）的读取工具。
///
/// 新建下载对话框与快速下载表单共用：输入 URL 命中已保存站点时把
/// 用户名/密码**自动回填**进 HTTP 认证输入框（用户可见、可改），
/// 而不是留给引擎静默注入。设置页的凭据管理器复用同一解析 +
/// [filterSiteAuth] 模糊过滤。写入/删除逻辑在设置页，不在此文件。
library;

import 'dart:convert';

/// 从 URL 提取站点键 — 必须与引擎 `site_auth::site_key` 逐字一致：
/// 仅 http/https；键 = 小写 host；端口**显式且非默认**（http:80 /
/// https:443）才追加 `:port`。非 http(s)、无 host、解析失败返回 null。
///
/// 不依赖 Uri 对默认端口的规范化行为，显式排除 80/443。
String? siteKeyFromUrl(String url) {
  final uri = Uri.tryParse(url.trim());
  if (uri == null) return null;
  final scheme = uri.scheme.toLowerCase();
  if (scheme != 'http' && scheme != 'https') return null;
  final host = uri.host.toLowerCase();
  if (host.isEmpty) return null;
  if (!uri.hasPort) return host;
  final port = uri.port;
  final isDefault =
      (scheme == 'http' && port == 80) || (scheme == 'https' && port == 443);
  return isDefault ? host : '$host:$port';
}

/// 解析凭据表 JSON `{"host[:port]":{"user":"u","pass":"p"}}`。
/// 容错：空串 / 损坏 JSON / 非对象 / 条目结构不符 → 按空表（或跳过该条）。
Map<String, ({String user, String pass})> parseSiteAuthStore(String json) {
  if (json.trim().isEmpty) return const {};
  final Object? decoded;
  try {
    decoded = jsonDecode(json);
  } catch (_) {
    return const {};
  }
  if (decoded is! Map<String, dynamic>) return const {};
  final out = <String, ({String user, String pass})>{};
  for (final entry in decoded.entries) {
    final v = entry.value;
    if (v is! Map<String, dynamic>) continue;
    out[entry.key] = (
      user: v['user'] as String? ?? '',
      pass: v['pass'] as String? ?? '',
    );
  }
  return out;
}

/// 凭据列表模糊过滤 + 站点字典序排序。
///
/// 查询串按空白拆词，**每个词**都必须是 `站点 + ' ' + 用户名` 的子串
/// （大小写不敏感）才算命中——多词即渐进收窄，与「搜索网站或用户名」
/// 的直觉一致。空查询返回全部。
List<MapEntry<String, ({String user, String pass})>> filterSiteAuth(
  Map<String, ({String user, String pass})> store,
  String query,
) {
  final entries = store.entries.toList()
    ..sort((a, b) => a.key.compareTo(b.key));
  final terms = query
      .toLowerCase()
      .split(RegExp(r'\s+'))
      .where((t) => t.isNotEmpty)
      .toList();
  if (terms.isEmpty) return entries;
  return entries.where((e) {
    final haystack = '${e.key} ${e.value.user}'.toLowerCase();
    return terms.every((t) => haystack.contains(t));
  }).toList();
}
