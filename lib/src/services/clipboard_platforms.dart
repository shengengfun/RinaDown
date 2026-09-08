// FluxDown 剪贴板「可解析音视频链接」监听 — 平台定义与匹配（纯数据，无 UI 依赖）。
//
// 平台以 host 后缀白名单识别（含裸域与子域），避免把非视频站点误判为可解析
// 视频。平台 id 稳定，用于设置持久化（config 键 `clipboard_parse_platforms`）。

class ClipboardPlatformDef {
  const ClipboardPlatformDef(this.id, this.hosts);

  final String id;

  /// 允许的 host 集合（小写；匹配 `host == h` 或 `host.endsWith('.$h')`）。
  final List<String> hosts;
}

/// 内置平台（顺序即设置页展示顺序）。
const List<ClipboardPlatformDef> kClipboardPlatforms = [
  ClipboardPlatformDef('douyin', ['douyin.com', 'iesdouyin.com']),
  ClipboardPlatformDef('bilibili', ['bilibili.com', 'b23.tv']),
  ClipboardPlatformDef('youtube', ['youtube.com', 'youtu.be']),
  ClipboardPlatformDef('xigua', ['ixigua.com']),
  ClipboardPlatformDef('xiaohongshu', ['xiaohongshu.com', 'xhslink.com']),
  ClipboardPlatformDef('twitter', ['x.com', 'twitter.com']),
  ClipboardPlatformDef('weibo', ['weibo.com']),
];

ClipboardPlatformDef? clipboardPlatformById(String id) {
  for (final def in kClipboardPlatforms) {
    if (def.id == id) return def;
  }
  return null;
}

/// 默认开启的平台（避免新装用户被过度打扰）。
const Set<String> kDefaultClipboardParsePlatforms = {
  'douyin',
  'bilibili',
  'youtube',
};

/// 从文本中提取首个命中内置平台的 http(s) URL，返回 (平台 id, URL)。
/// 无命中返回 null。
(String, String)? matchClipboardPlatformUrl(String text) {
  if (text.isEmpty) return null;
  final re = RegExp(
    r'https?://[^\s<>"，。！？、；：]+',
    caseSensitive: false,
  );
  for (final m in re.allMatches(text)) {
    final url = m.group(0)!;
    final host = Uri.tryParse(url)?.host.toLowerCase() ?? '';
    if (host.isEmpty) continue;
    for (final def in kClipboardPlatforms) {
      for (final h in def.hosts) {
        if (host == h || host.endsWith('.$h')) return (def.id, url);
      }
    }
  }
  return null;
}
