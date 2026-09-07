/// RSS 过滤规则的 **Dart 侧镜像**，仅供「过滤规则」Tab 的实时预览使用。
///
/// 真正决定下载与否的是引擎 `native/engine/src/rss/filter.rs`；这里必须与它
/// **逐条等价**，否则预览会骗人——预览可信是本功能相对 qBittorrent 的核心
/// 差异化（设计文档 P2），一旦漂移就自毁招牌。
///
/// 对齐清单（改动任一侧都要同步另一侧）：
/// 1. 判定顺序固定：包含 → 排除 → 体积下限 → 体积上限 → 剧集去重，先命中先返回；
/// 2. 空表达式恒真；非法正则**放行**（宁重勿漏）；
/// 3. 关键词语法：`|` 分隔或项，空格分隔与项，大小写不敏感；
/// 4. 体积上下限只对**已知大小**（`> 0`）生效；
/// 5. 剧集识别四格式，`NxM` 限「季 ≤ 2 位、集 ≤ 3 位」避免 `1920x1080` 误判；
///    识别失败即放行。
library;

import '../bindings/bindings.dart';

/// 条目被过滤掉的稳定原因码（与 `RejectReason::code()` 一一对应）。
class RssReasonCode {
  static const notIncluded = 'not_included';
  static const excluded = 'excluded';
  static const tooSmall = 'too_small';
  static const tooLarge = 'too_large';
  static const dupEpisode = 'dup_episode';
  static const seedSkipped = 'seed_skipped';
}

/// 条目状态码（与 `RssItemStatus::as_i32()` 一一对应）。
class RssItemStatusCode {
  static const isNew = 0;
  static const downloaded = 1;
  static const ignored = 2;
  static const filtered = 3;
  static const duplicateEpisode = 4;
  static const seedSkipped = 5;
}

/// 一条订阅的过滤规则（预览用的可变投影）。
class RssFilterRule {
  String include;
  String exclude;
  bool useRegex;
  bool smartEpisode;
  int sizeMinBytes;
  int sizeMaxBytes;

  RssFilterRule({
    this.include = '',
    this.exclude = '',
    this.useRegex = false,
    this.smartEpisode = false,
    this.sizeMinBytes = 0,
    this.sizeMaxBytes = 0,
  });
}

/// 一条预览结论。[reason] 为 null 表示命中（将下载）。
class RssVerdict {
  final String? reason;
  final String episodeKey;

  const RssVerdict.accept(this.episodeKey) : reason = null;
  const RssVerdict.reject(this.reason, {this.episodeKey = ''});

  bool get accepted => reason == null;
}

/// 解析体积字面量为字节数：`200M` / `2G` / `1.5 GB` / `1024`（1024 进制）。
/// 空串与无法解析的输入统一返回 null = 不限（与 Rust `parse_size` 同语义）。
int? parseRssSize(String input) {
  final text = input.trim();
  if (text.isEmpty) return null;
  final m = RegExp(
    r'^([0-9]+(?:\.[0-9]+)?)\s*([kmgtKMGT]?)[bB]?$',
  ).firstMatch(text);
  if (m == null) return null;
  final value = double.tryParse(m.group(1) ?? '');
  if (value == null) return null;
  const kib = 1024.0;
  final mult = switch ((m.group(2) ?? '').toUpperCase()) {
    'K' => kib,
    'M' => kib * kib,
    'G' => kib * kib * kib,
    'T' => kib * kib * kib * kib,
    _ => 1.0,
  };
  final bytes = value * mult;
  if (!bytes.isFinite || bytes < 0) return null;
  return bytes.toInt();
}

/// 字节数 → 体积字面量（0 = 空串 = 不限），与 [parseRssSize] 构成往返。
String formatRssSize(int bytes) {
  if (bytes <= 0) return '';
  const units = [
    (1024 * 1024 * 1024 * 1024, 'T'),
    (1024 * 1024 * 1024, 'G'),
    (1024 * 1024, 'M'),
    (1024, 'K'),
  ];
  for (final (scale, suffix) in units) {
    if (bytes % scale == 0) return '${bytes ~/ scale}$suffix';
  }
  return '$bytes';
}

/// 关键词/正则匹配。空表达式恒真；非法正则**放行**。
bool matchRssKeywords(String title, String expr, bool useRegex) {
  final e = expr.trim();
  if (e.isEmpty) return true;
  if (useRegex) {
    try {
      return RegExp(e, caseSensitive: false).hasMatch(title);
    } on FormatException {
      return true;
    }
  }
  final lowered = title.toLowerCase();
  return e
      .split('|')
      .any(
        (alt) => alt
            .split(RegExp(r'\s+'))
            .where((w) => w.isNotEmpty)
            .every((w) => lowered.contains(w.toLowerCase())),
      );
}

final _seasonEp = RegExp(r's(\d+)e(\d+)', caseSensitive: false);
// 季号限 1-2 位、集号限 1-3 位：`1920x1080` 这类分辨率不再被误判为季集号。
final _crossEp = RegExp(r'\b(\d{1,2})x(\d{1,3})\b');
final _dashEp = RegExp(r'-\s*(\d{2,3})\b');
final _cjkEp = RegExp(r'第\s*(\d+)\s*[话話集]');
final _brackets = RegExp(r'\[[^\]]*\]');
final _epTokens = RegExp(
  r's\d+e\d+|第\s*\d+\s*[话話集]|-\s*\d{2,3}\b|\b\d{1,2}x\d{1,3}\b',
  caseSensitive: false,
);
final _noise = RegExp(r'[\s/～~]+');

/// 从标题提取 `<归一番名>#<集号>`；识别失败返回 null = 放行。
String? rssEpisodeKey(String title) {
  int? episode;
  for (final (re, group) in [
    (_seasonEp, 2),
    (_crossEp, 2),
    (_dashEp, 1),
    (_cjkEp, 1),
  ]) {
    final m = re.firstMatch(title);
    if (m == null) continue;
    episode = int.tryParse(m.group(group) ?? '');
    if (episode != null) break;
  }
  if (episode == null) return null;
  final base = title
      .replaceAll(_brackets, '')
      .replaceAll(_epTokens, '')
      .replaceAll(_noise, '');
  final chars = base.characters24();
  return '$chars#$episode';
}

extension on String {
  /// 取前 24 个 Unicode 码点（Rust 侧同为 `chars().take(24)`）。
  String characters24() {
    final runes = this.runes.toList();
    if (runes.length <= 24) return this;
    return String.fromCharCodes(runes.take(24));
  }
}

/// 对一批条目跑一遍规则，返回与输入等长的判定序列。
///
/// [takenEpisodeKeys] 为该源**已被占用**的剧集键（引擎侧 = status ∈ {新,已下载}
/// 的条目）；预览时传当前列表里 status ∈ {0,1} 的条目键即可近似。
List<RssVerdict> evaluateRssBatch(
  List<RssItemEntry> items,
  RssFilterRule rule, {
  Set<String>? takenEpisodeKeys,
}) {
  final seen = <String>{...?takenEpisodeKeys};
  return [
    for (final item in items) _evaluateOne(item.title, item.enclosureLength, rule, seen),
  ];
}

RssVerdict _evaluateOne(
  String title,
  int size,
  RssFilterRule rule,
  Set<String> seen,
) {
  if (!matchRssKeywords(title, rule.include, rule.useRegex)) {
    return const RssVerdict.reject(RssReasonCode.notIncluded);
  }
  if (rule.exclude.trim().isNotEmpty &&
      matchRssKeywords(title, rule.exclude, rule.useRegex)) {
    return const RssVerdict.reject(RssReasonCode.excluded);
  }
  if (size > 0) {
    if (rule.sizeMinBytes > 0 && size < rule.sizeMinBytes) {
      return const RssVerdict.reject(RssReasonCode.tooSmall);
    }
    if (rule.sizeMaxBytes > 0 && size > rule.sizeMaxBytes) {
      return const RssVerdict.reject(RssReasonCode.tooLarge);
    }
  }
  if (rule.smartEpisode) {
    final key = rssEpisodeKey(title);
    if (key != null) {
      if (seen.contains(key)) {
        return RssVerdict.reject(RssReasonCode.dupEpisode, episodeKey: key);
      }
      seen.add(key);
      return RssVerdict.accept(key);
    }
  }
  return const RssVerdict.accept('');
}
