//! RSS 条目的规则过滤与智能剧集去重——**全部为纯函数**，是 `rss` 子系统的
//! 单测主战场。
//!
//! 语义与可交互原型 `docs/rss_ui_preview.html` 内嵌的 JS 规则引擎逐条对齐
//! （`parseSize` / `matchKw` / `epKey` / `calc`），原型即行为规格。三处**有意
//! 的收紧**已在各函数文档中标注：
//! 1. `NxM` 剧集格式限定 `季<=99 且 集<=999`，避免把 `1920x1080` 当成
//!    「第 1080 集」；
//! 2. 体积上下限只对**已知大小**（`> 0`）的条目生效——未知大小放行，与
//!    「宁可重复不可漏下」的整体取向一致；
//! 3. 标题截断按 Unicode 字符而非 UTF-16 码元计数。
//!
//! 判定顺序固定为 包含 → 排除 → 体积下限 → 体积上限 → 剧集去重，与原型
//! `calc()` 的 if/else 链一致：**先命中先返回**，一个条目只有一个原因。

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;

use crate::rss::model::RssItemStatus;

/// 一条订阅的过滤规则（[`crate::rss::model::RssSourceInfo`] 的投影）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilterRule {
    /// 包含关键词表达式（空 = 全部通过）。
    pub include: String,
    /// 排除关键词表达式（空 = 不排除）。
    pub exclude: String,
    /// 按正则解析 `include`/`exclude`。
    pub use_regex: bool,
    /// 启用智能剧集去重。
    pub smart_episode: bool,
    /// 体积下限（字节，0 = 不限）。
    pub size_min_bytes: i64,
    /// 体积上限（字节，0 = 不限）。
    pub size_max_bytes: i64,
}

/// 条目被过滤掉的原因。
///
/// 引擎只产出**稳定原因码**（[`RejectReason::code`]），面向用户的文案由各端
/// i18n 负责——引擎不内嵌任何自然语言。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    /// 未命中包含关键词。
    NotIncluded,
    /// 命中排除关键词。
    Excluded,
    /// 小于体积下限。
    TooSmall,
    /// 超过体积上限。
    TooLarge,
    /// 同源同集已下过（智能剧集去重）。
    DuplicateEpisode,
}

impl RejectReason {
    /// 稳定原因码（落 `rss_items.reason` 列，宿主据此查 i18n 表）。
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::NotIncluded => "not_included",
            Self::Excluded => "excluded",
            Self::TooSmall => "too_small",
            Self::TooLarge => "too_large",
            Self::DuplicateEpisode => "dup_episode",
        }
    }

    /// 该原因对应的条目状态。
    #[must_use]
    pub fn item_status(self) -> RssItemStatus {
        match self {
            Self::DuplicateEpisode => RssItemStatus::DuplicateEpisode,
            _ => RssItemStatus::Filtered,
        }
    }
}

/// 单条目的判定结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// 通过全部规则，应当下载。`episode_key` 非空时已登记进去重集合。
    Accept {
        /// 智能剧集归一键（空 = 未识别或未启用去重）。
        episode_key: String,
    },
    /// 被规则拦下。
    Reject {
        /// 拦截原因。
        reason: RejectReason,
        /// 触发拦截时识别到的剧集键（仅 [`RejectReason::DuplicateEpisode`]
        /// 非空，供 UI 显示「第 N 集已命中」）。
        episode_key: String,
    },
}

impl Verdict {
    /// 是否应当为该条目创建下载任务。
    #[must_use]
    pub fn accepted(&self) -> bool {
        matches!(self, Self::Accept { .. })
    }
}

/// 预编译后的规则——正则只编译一次，供一整轮条目复用。
///
/// **非法正则一律放行**（编译失败等价于「不过滤」），与原型
/// `matchKw` 的 `catch(e){return true}` 一致：用户写错正则时宁可多下，
/// 也不静默漏下（qBittorrent `episodeFilter` 写错即静默失配是明确的反面
/// 教训，见设计文档 §1.1）。
#[derive(Debug)]
pub struct CompiledRule {
    include: Matcher,
    exclude: Matcher,
    smart_episode: bool,
    size_min_bytes: i64,
    size_max_bytes: i64,
}

#[derive(Debug)]
enum Matcher {
    /// 空表达式或非法正则：恒真。
    Any,
    /// 正则匹配（已带 `(?i)`）。
    Regex(Box<Regex>),
    /// 关键词匹配：外层 `Vec` = `|` 分隔的或项，内层 = 空格分隔的与项（全小写）。
    Keywords(Vec<Vec<String>>),
}

impl Matcher {
    fn build(expr: &str, use_regex: bool) -> Self {
        let expr = expr.trim();
        if expr.is_empty() {
            return Self::Any;
        }
        if use_regex {
            return match Regex::new(&format!("(?i){expr}")) {
                Ok(re) => Self::Regex(Box::new(re)),
                Err(_) => Self::Any,
            };
        }
        Self::Keywords(
            expr.split('|')
                .map(|alt| alt.split_whitespace().map(str::to_lowercase).collect())
                .collect(),
        )
    }

    /// `lowered` 为调用方预先小写化的标题（避免逐 alt 重复分配）。
    fn is_match(&self, title: &str, lowered: &str) -> bool {
        match self {
            Self::Any => true,
            Self::Regex(re) => re.is_match(title),
            // 空的与项集合视为命中（`"a||b"` 中的空段与原型 `''.split()` 同义）。
            Self::Keywords(alts) => alts
                .iter()
                .any(|words| words.iter().all(|w| lowered.contains(w.as_str()))),
        }
    }

    /// 表达式是否为空（`Any` 也可能来自非法正则，此处只关心「有没有配」）。
    fn is_any(&self) -> bool {
        matches!(self, Self::Any)
    }
}

impl CompiledRule {
    /// 编译一条规则。
    #[must_use]
    pub fn new(rule: &FilterRule) -> Self {
        Self {
            include: Matcher::build(&rule.include, rule.use_regex),
            exclude: Matcher::build(&rule.exclude, rule.use_regex),
            smart_episode: rule.smart_episode,
            size_min_bytes: rule.size_min_bytes.max(0),
            size_max_bytes: rule.size_max_bytes.max(0),
        }
    }

    /// 判定一个条目。
    ///
    /// `size` 为 enclosure 声明大小（`<= 0` = 未知，跳过体积判定）。
    /// `seen_episodes` 是**同源**已占用的剧集键集合（调用方需预先用历史
    /// 已下条目播种）；命中 `Accept` 且识别出剧集键时就地登记，使同一轮内
    /// 的后续同集条目被判为重复。
    pub fn evaluate(&self, title: &str, size: i64, seen_episodes: &mut HashSet<String>) -> Verdict {
        let lowered = title.to_lowercase();
        if !self.include.is_match(title, &lowered) {
            return Verdict::Reject {
                reason: RejectReason::NotIncluded,
                episode_key: String::new(),
            };
        }
        if !self.exclude.is_any() && self.exclude.is_match(title, &lowered) {
            return Verdict::Reject {
                reason: RejectReason::Excluded,
                episode_key: String::new(),
            };
        }
        if size > 0 {
            if self.size_min_bytes > 0 && size < self.size_min_bytes {
                return Verdict::Reject {
                    reason: RejectReason::TooSmall,
                    episode_key: String::new(),
                };
            }
            if self.size_max_bytes > 0 && size > self.size_max_bytes {
                return Verdict::Reject {
                    reason: RejectReason::TooLarge,
                    episode_key: String::new(),
                };
            }
        }
        let mut key = String::new();
        if self.smart_episode
            && let Some(k) = episode_key(title)
        {
            if seen_episodes.contains(&k) {
                return Verdict::Reject {
                    reason: RejectReason::DuplicateEpisode,
                    episode_key: k,
                };
            }
            seen_episodes.insert(k.clone());
            key = k;
        }
        Verdict::Accept { episode_key: key }
    }
}

/// 解析体积字面量为字节数：`200M` / `2G` / `1.5 GB` / `1024`（**1024 进制**，
/// 与 CLI 的 `K/M/G/T` 解析同惯例）。
///
/// 空串、纯空白与无法解析的输入统一返回 `None` = **不限**（对应原型
/// `parseSize` 的 `null`/`NaN` 两种落点在 `calc()` 中被同等跳过）。
///
/// # Examples
///
/// ```
/// use fluxdown_engine::rss::filter::parse_size;
///
/// assert_eq!(parse_size("200M"), Some(200 * 1024 * 1024));
/// assert_eq!(parse_size("1.5 GB"), Some(1_610_612_736));
/// assert_eq!(parse_size(""), None);
/// assert_eq!(parse_size("很大"), None);
/// ```
#[must_use]
pub fn parse_size(input: &str) -> Option<i64> {
    static SIZE_RE: LazyLock<Option<Regex>> =
        LazyLock::new(|| Regex::new(r"(?i)^([0-9]+(?:\.[0-9]+)?)\s*([kmgt]?)b?$").ok());
    let text = input.trim();
    if text.is_empty() {
        return None;
    }
    let caps = SIZE_RE.as_ref()?.captures(text)?;
    let value: f64 = caps.get(1)?.as_str().parse().ok()?;
    let unit = caps.get(2)?.as_str().to_ascii_uppercase();
    let mult: f64 = match unit.as_str() {
        "K" => 1024.0,
        "M" => 1024.0 * 1024.0,
        "G" => 1024.0 * 1024.0 * 1024.0,
        "T" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => 1.0,
    };
    let bytes = value * mult;
    if !bytes.is_finite() || bytes < 0.0 || bytes > i64::MAX as f64 {
        return None;
    }
    Some(bytes as i64)
}

/// 反向格式化：字节数 → 体积字面量（`0` = 空串 = 不限）。UI 回填输入框用，
/// 与 [`parse_size`] 构成往返。
///
/// # Examples
///
/// ```
/// use fluxdown_engine::rss::filter::{format_size, parse_size};
///
/// assert_eq!(format_size(0), "");
/// assert_eq!(format_size(200 * 1024 * 1024), "200M");
/// assert_eq!(parse_size(&format_size(3 * 1024 * 1024 * 1024)), Some(3 * 1024 * 1024 * 1024));
/// ```
#[must_use]
pub fn format_size(bytes: i64) -> String {
    if bytes <= 0 {
        return String::new();
    }
    const UNITS: [(i64, &str); 4] = [
        (1024 * 1024 * 1024 * 1024, "T"),
        (1024 * 1024 * 1024, "G"),
        (1024 * 1024, "M"),
        (1024, "K"),
    ];
    for (scale, suffix) in UNITS {
        if bytes % scale == 0 {
            return format!("{}{suffix}", bytes / scale);
        }
    }
    bytes.to_string()
}

/// 从标题提取「番名归一键 + 集号」，识别四类命名：
/// `S01E02` / `1x02` / `- 02` / `第02话`（含 `話`/`集`）。
///
/// 返回 `<归一番名>#<集号>`；**识别失败返回 `None` = 放行**（宁可重复不可
/// 漏下，与 qBittorrent 智能过滤误杀的取向相反，见设计文档 §2.2）。
///
/// 归一番名 = 去掉 `[...]` 字幕组/规格标签 → 去掉集号片段 → 去掉空白与
/// `/`、`～`、`~` → 取前 24 个字符。
///
/// # Examples
///
/// ```
/// use fluxdown_engine::rss::filter::episode_key;
///
/// // 同一集的不同字幕组版本归一到同一个键
/// let a = episode_key("[ANi] 幼女战记 2 - 02 [1080P][Baha][WEB-DL]");
/// let b = episode_key("[桜都字幕组] 幼女战记 2 - 02 [720P][简体内嵌]");
/// assert_eq!(a, b);
/// assert!(a.is_some());
///
/// // 分辨率不会被误当成季集号
/// assert_eq!(episode_key("Some Movie 1920x1080 BluRay"), None);
/// ```
#[must_use]
pub fn episode_key(title: &str) -> Option<String> {
    static SEASON_EP: LazyLock<Option<Regex>> =
        LazyLock::new(|| Regex::new(r"(?i)s(\d+)e(\d+)").ok());
    // 季号限 1-2 位、集号限 1-3 位：`1920x1080` 这类分辨率不再被误判。
    static CROSS_EP: LazyLock<Option<Regex>> =
        LazyLock::new(|| Regex::new(r"\b(\d{1,2})x(\d{1,3})\b").ok());
    static DASH_EP: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"-\s*(\d{2,3})\b").ok());
    static CJK_EP: LazyLock<Option<Regex>> =
        LazyLock::new(|| Regex::new(r"第\s*(\d+)\s*[话話集]").ok());

    let episode = [
        (SEASON_EP.as_ref(), 2usize),
        (CROSS_EP.as_ref(), 2),
        (DASH_EP.as_ref(), 1),
        (CJK_EP.as_ref(), 1),
    ]
    .into_iter()
    .find_map(|(re, group)| {
        let caps = re?.captures(title)?;
        caps.get(group)?.as_str().parse::<u32>().ok()
    })?;

    Some(format!("{}#{episode}", normalized_series(title)))
}

/// 归一番名（[`episode_key`] 的前半段，单独抽出便于单测与复用）。
fn normalized_series(title: &str) -> String {
    static BRACKETS: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"\[[^\]]*\]").ok());
    static EP_TOKENS: LazyLock<Option<Regex>> = LazyLock::new(|| {
        Regex::new(r"(?i)s\d+e\d+|第\s*\d+\s*[话話集]|-\s*\d{2,3}\b|\b\d{1,2}x\d{1,3}\b").ok()
    });
    static NOISE: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"[\s/～~]+").ok());

    let mut text = title.to_string();
    for re in [BRACKETS.as_ref(), EP_TOKENS.as_ref(), NOISE.as_ref()]
        .into_iter()
        .flatten()
    {
        text = re.replace_all(&text, "").into_owned();
    }
    text.chars().take(24).collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::collections::HashSet;

    use super::{
        CompiledRule, FilterRule, RejectReason, Verdict, episode_key, format_size, parse_size,
    };

    const MB: i64 = 1024 * 1024;

    fn rule() -> FilterRule {
        FilterRule::default()
    }

    #[test]
    fn parse_size_handles_units_and_garbage() {
        assert_eq!(parse_size("1024"), Some(1024));
        assert_eq!(parse_size("200M"), Some(200 * MB));
        assert_eq!(parse_size("200 mb"), Some(200 * MB));
        assert_eq!(parse_size("2G"), Some(2 * 1024 * MB));
        assert_eq!(parse_size("1.5G"), Some(1536 * MB));
        assert_eq!(parse_size("1T"), Some(1024 * 1024 * MB));
        // 空 / 空白 / 垃圾输入 → 不限（绝不 panic，绝不误判为 0 字节上限）
        assert_eq!(parse_size(""), None);
        assert_eq!(parse_size("   "), None);
        assert_eq!(parse_size("abc"), None);
        assert_eq!(parse_size("12X"), None);
        assert_eq!(parse_size("-5M"), None);
    }

    #[test]
    fn format_size_round_trips_through_parse_size() {
        for bytes in [1024, 200 * MB, 3 * 1024 * MB, 7 * 1024 * 1024 * MB] {
            assert_eq!(parse_size(&format_size(bytes)), Some(bytes), "{bytes}");
        }
        assert_eq!(format_size(0), "");
        assert_eq!(format_size(-1), "");
        // 非整倍数退化为裸字节，仍可往返
        assert_eq!(parse_size(&format_size(1_234_567)), Some(1_234_567));
    }

    #[test]
    fn empty_rule_accepts_everything() {
        let compiled = CompiledRule::new(&rule());
        let mut seen = HashSet::new();
        assert!(
            compiled
                .evaluate("anything at all", 0, &mut seen)
                .accepted()
        );
    }

    #[test]
    fn keyword_include_is_or_of_ands_case_insensitive() {
        let compiled = CompiledRule::new(&FilterRule {
            include: "1080P 简体|1080P CHT".to_string(),
            ..rule()
        });
        let mut seen = HashSet::new();
        assert!(
            compiled
                .evaluate("[ANi] X - 02 [1080p][CHT]", 0, &mut seen)
                .accepted()
        );
        assert!(
            compiled
                .evaluate("[Sakurato] X - 02 [1080P][简体]", 0, &mut seen)
                .accepted()
        );
        // 与项不齐 → 不命中
        assert_eq!(
            compiled.evaluate("[Sakurato] X - 02 [720P][简体]", 0, &mut seen),
            Verdict::Reject {
                reason: RejectReason::NotIncluded,
                episode_key: String::new()
            }
        );
    }

    #[test]
    fn exclude_beats_include_and_reports_its_own_reason() {
        let compiled = CompiledRule::new(&FilterRule {
            include: "1080P".to_string(),
            exclude: "720P|HEVC".to_string(),
            ..rule()
        });
        let mut seen = HashSet::new();
        match compiled.evaluate("[X] Y - 02 [1080P][HEVC-10bit]", 0, &mut seen) {
            Verdict::Reject { reason, .. } => assert_eq!(reason, RejectReason::Excluded),
            v => panic!("expected exclude rejection, got {v:?}"),
        }
    }

    #[test]
    fn invalid_regex_lets_items_through_instead_of_silently_dropping_them() {
        let compiled = CompiledRule::new(&FilterRule {
            include: "[unclosed".to_string(),
            use_regex: true,
            ..rule()
        });
        let mut seen = HashSet::new();
        assert!(
            compiled.evaluate("whatever", 0, &mut seen).accepted(),
            "a broken regex must not silently filter every item"
        );
    }

    #[test]
    fn regex_mode_is_case_insensitive() {
        let compiled = CompiledRule::new(&FilterRule {
            include: r"\b(1080|2160)p\b".to_string(),
            use_regex: true,
            ..rule()
        });
        let mut seen = HashSet::new();
        assert!(
            compiled
                .evaluate("Show 2160P WEB-DL", 0, &mut seen)
                .accepted()
        );
        assert!(
            !compiled
                .evaluate("Show 720p WEB-DL", 0, &mut seen)
                .accepted()
        );
    }

    #[test]
    fn size_bounds_apply_only_to_known_sizes() {
        let compiled = CompiledRule::new(&FilterRule {
            size_min_bytes: 200 * MB,
            size_max_bytes: 2 * 1024 * MB,
            ..rule()
        });
        let mut seen = HashSet::new();
        assert!(compiled.evaluate("ok", 500 * MB, &mut seen).accepted());
        match compiled.evaluate("small", 10 * MB, &mut seen) {
            Verdict::Reject { reason, .. } => assert_eq!(reason, RejectReason::TooSmall),
            v => panic!("expected too_small, got {v:?}"),
        }
        match compiled.evaluate("huge", 40 * 1024 * MB, &mut seen) {
            Verdict::Reject { reason, .. } => assert_eq!(reason, RejectReason::TooLarge),
            v => panic!("expected too_large, got {v:?}"),
        }
        // 未知大小（enclosure 没给 length）必须放行，否则整站无 length 的
        // feed 会在配了体积下限后被全量误杀。
        assert!(compiled.evaluate("unknown size", 0, &mut seen).accepted());
    }

    #[test]
    fn episode_key_recognizes_all_four_formats() {
        assert!(episode_key("Show Name S01E02 1080p").is_some());
        assert!(episode_key("Show Name 1x02 1080p").is_some());
        assert!(episode_key("[ANi] Show - 02 [1080P]").is_some());
        assert!(episode_key("[字幕组] 番名 第02话 [1080P]").is_some());
    }

    #[test]
    fn episode_key_normalizes_across_release_groups() {
        let ani = episode_key("[ANi] 幼女战记 2 - 02 [1080P][Baha][WEB-DL][AAC AVC][CHT][MP4]");
        let sakura = episode_key("[桜都字幕组] 幼女战记 2 - 02 [720P][简体内嵌]");
        assert_eq!(ani, sakura);
        assert!(ani.unwrap().ends_with("#2"));
    }

    #[test]
    fn episode_key_returns_none_when_unrecognized() {
        // 合集 / 剧场版 / 无集号命名一律放行
        assert_eq!(
            episode_key("[VCB-Studio] 紫罗兰永恒花园 [Ma10p_1080p][合集]"),
            None
        );
        assert_eq!(
            episode_key("[FRDS] Dune Part Three 2026 2160p WEB-DL DDP5.1 Atmos HDR H.265"),
            None
        );
    }

    #[test]
    fn episode_key_does_not_mistake_resolution_for_season_episode() {
        // 十年老 bug（qB#24397 类误杀）的定点防守：1920x1080 不是第 1080 集。
        assert_eq!(episode_key("Some Movie 1920x1080 BluRay"), None);
        assert_eq!(episode_key("Clip 3840x2160"), None);
        // 但真正的 1x02 仍要识别
        assert!(episode_key("Show 1x02").is_some());
    }

    #[test]
    fn smart_dedup_keeps_first_and_rejects_later_duplicates() {
        let compiled = CompiledRule::new(&FilterRule {
            smart_episode: true,
            ..rule()
        });
        let mut seen = HashSet::new();
        assert!(
            compiled
                .evaluate("[ANi] 幼女战记 2 - 02 [1080P]", 0, &mut seen)
                .accepted()
        );
        match compiled.evaluate("[桜都字幕组] 幼女战记 2 - 02 [720P]", 0, &mut seen) {
            Verdict::Reject {
                reason,
                episode_key,
            } => {
                assert_eq!(reason, RejectReason::DuplicateEpisode);
                assert!(episode_key.ends_with("#2"));
            }
            v => panic!("expected duplicate rejection, got {v:?}"),
        }
        // 不同集互不影响
        assert!(
            compiled
                .evaluate("[ANi] 幼女战记 2 - 03 [1080P]", 0, &mut seen)
                .accepted()
        );
    }

    #[test]
    fn smart_dedup_is_off_by_default() {
        let compiled = CompiledRule::new(&rule());
        let mut seen = HashSet::new();
        assert!(compiled.evaluate("[A] X - 02", 0, &mut seen).accepted());
        assert!(
            compiled.evaluate("[B] X - 02", 0, &mut seen).accepted(),
            "dedup must not engage unless smart_episode is on"
        );
        assert!(seen.is_empty());
    }

    #[test]
    fn unrecognized_episodes_are_never_deduped() {
        let compiled = CompiledRule::new(&FilterRule {
            smart_episode: true,
            ..rule()
        });
        let mut seen = HashSet::new();
        // 两条都识别不出集号 → 都放行（识别失败即放行）
        assert!(compiled.evaluate("[VCB] 合集 A", 0, &mut seen).accepted());
        assert!(compiled.evaluate("[VCB] 合集 A", 0, &mut seen).accepted());
    }

    #[test]
    fn rejection_maps_to_the_right_item_status() {
        use crate::rss::model::RssItemStatus;
        assert_eq!(
            RejectReason::Excluded.item_status(),
            RssItemStatus::Filtered
        );
        assert_eq!(
            RejectReason::TooLarge.item_status(),
            RssItemStatus::Filtered
        );
        assert_eq!(
            RejectReason::DuplicateEpisode.item_status(),
            RssItemStatus::DuplicateEpisode
        );
    }
}
