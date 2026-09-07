// RSS 过滤规则引擎（前端副本）—— 管理对话框「过滤规则」Tab 的实时预览用。
//
// 逐条对齐 native/engine/src/rss/filter.rs：预览一旦与引擎分叉就是在骗人，
// 所以这里不做任何「差不多就行」的简化，包括原型 docs/rss_ui_preview.html 之后
// 三处有意的收紧：
//   1. `NxM` 剧集格式限 `\b(\d{1,2})x(\d{1,3})\b`，避免把 1920x1080 当成第 1080 集；
//   2. 体积上下限只对 `enclosureLength > 0` 的条目生效（未知大小放行）；
//   3. 归一番名按 Unicode 字符而非 UTF-16 码元取前 24 个。
// 判定顺序固定为 包含 → 排除 → 体积下限 → 体积上限 → 剧集去重，先命中先返回：
// 一个条目只有一个原因。
//
// 引擎只产出稳定原因码，面向用户的文案一律走 reasonLabel() 查 i18n——原码永不上屏。

import { t, type I18nKey } from './i18n'
import type { RssItemDto, RssSourceDto } from './types'

/** 条目被过滤掉的原因（引擎 RejectReason::code 的完整值域）。 */
export type RssRejectReason = 'not_included' | 'excluded' | 'too_small' | 'too_large' | 'dup_episode'

/** 落在 `RssItemDto.reason` 上的全部原因码：过滤原因 + 首轮播种跳过。 */
export type RssReasonCode = RssRejectReason | 'seed_skipped'

const REASON_KEYS: Record<RssReasonCode, I18nKey> = {
  not_included: 'rss.reasonNotIncluded',
  excluded: 'rss.reasonExcluded',
  too_small: 'rss.reasonTooSmall',
  too_large: 'rss.reasonTooLarge',
  dup_episode: 'rss.reasonDupEpisode',
  seed_skipped: 'rss.reasonSeedSkipped',
}

/** 原因码 → 人读文案。空码返回空串；未知码回落通用文案（绝不把原码上屏）。 */
export function reasonLabel(code: string): string {
  if (!code) return ''
  const key = REASON_KEYS[code as RssReasonCode]
  return key ? t(key) : t('rss.reasonUnknown')
}

/** 同上，重复剧集额外带出集号（剧集键形如 `归一番名#12`；无集号则退化为纯文案）。 */
export function reasonText(code: string, episodeKey = ''): string {
  const label = reasonLabel(code)
  if (code !== 'dup_episode' || !label) return label
  const ep = episodeKey.slice(episodeKey.lastIndexOf('#') + 1)
  return episodeKey.includes('#') && ep ? `${label}${t('rss.dupEpisodeAt', { ep })}` : label
}

// ---------------------------------------------------------------------------
// Unicode 词边界
// ---------------------------------------------------------------------------

// Rust regex 的 `\b` 是 Unicode 感知的（`\w` 含 CJK 等字母），JS 的 `\b` 只认 ASCII。
// 直接照抄 `\b` 会让「番名 - 02话」这类标题在两端分叉（JS 认边界、Rust 不认），
// 故用前后瞻手工还原 Rust `\w` 的定义。
const WORD_CHAR = '[\\p{Alphabetic}\\p{M}\\p{Nd}\\p{Pc}\\p{Join_Control}]'
const NOT_AFTER_WORD = `(?<!${WORD_CHAR})`
const NOT_BEFORE_WORD = `(?!${WORD_CHAR})`

// ---------------------------------------------------------------------------
// 体积字面量
// ---------------------------------------------------------------------------

const SIZE_RE = /^([0-9]+(?:\.[0-9]+)?)\s*([kmgt]?)b?$/i
const SIZE_MULT: Record<string, number> = {
  '': 1,
  K: 1024,
  M: 1024 ** 2,
  G: 1024 ** 3,
  T: 1024 ** 4,
}
/** 引擎的溢出上界写作 `i64::MAX as f64`，该转换落到 2^63——此处用同一个可精确
 *  表示的值，避免写 i64::MAX 字面量反而被 f64 舍入成别的数。 */
const I64_MAX_AS_F64 = 2 ** 63

/** 解析体积字面量为字节数：`200M` / `2G` / `1.5 GB` / `1024`（1024 进制）。
 *  空串与无法解析的输入统一返回 null = 不限。 */
export function parseSize(input: string): number | null {
  const text = input.trim()
  if (!text) return null
  const caps = SIZE_RE.exec(text)
  if (!caps) return null
  const value = Number.parseFloat(caps[1])
  const bytes = value * (SIZE_MULT[caps[2].toUpperCase()] ?? 1)
  if (!Number.isFinite(bytes) || bytes < 0 || bytes > I64_MAX_AS_F64) return null
  return Math.trunc(bytes)
}

/** 反向格式化：字节数 → 体积字面量（0 = 空串 = 不限）。与 parseSize 构成往返，
 *  供对话框把 sizeMinBytes/sizeMaxBytes 回填进文本框。 */
export function formatSize(bytes: number): string {
  if (bytes <= 0) return ''
  const units: [number, string][] = [
    [1024 ** 4, 'T'],
    [1024 ** 3, 'G'],
    [1024 ** 2, 'M'],
    [1024, 'K'],
  ]
  for (const [scale, suffix] of units) {
    if (bytes % scale === 0) return `${bytes / scale}${suffix}`
  }
  return String(bytes)
}

// ---------------------------------------------------------------------------
// 智能剧集去重
// ---------------------------------------------------------------------------

const SEASON_EP = /s(\d+)e(\d+)/iu
// 季号限 1-2 位、集号限 1-3 位：`1920x1080` 这类分辨率不再被误判。
const CROSS_EP = new RegExp(`${NOT_AFTER_WORD}(\\d{1,2})x(\\d{1,3})${NOT_BEFORE_WORD}`, 'u')
const DASH_EP = new RegExp(`-\\s*(\\d{2,3})${NOT_BEFORE_WORD}`, 'u')
const CJK_EP = /第\s*(\d+)\s*[话話集]/u
/** Rust 侧集号解析为 u32，溢出即换下一个匹配器——此处照同样的门槛放行。 */
const U32_MAX = 4_294_967_295

const EPISODE_MATCHERS: [RegExp, number][] = [
  [SEASON_EP, 2],
  [CROSS_EP, 2],
  [DASH_EP, 1],
  [CJK_EP, 1],
]

const BRACKETS = /\[[^\]]*\]/gu
const EP_TOKENS = new RegExp(
  `s\\d+e\\d+|第\\s*\\d+\\s*[话話集]|-\\s*\\d{2,3}${NOT_BEFORE_WORD}|${NOT_AFTER_WORD}\\d{1,2}x\\d{1,3}${NOT_BEFORE_WORD}`,
  'giu',
)
const NOISE = /[\s/～~]+/gu

/** 归一番名：去掉 `[...]` 字幕组/规格标签 → 去掉集号片段 → 去掉空白与 `/`、`～`、`~`
 *  → 取前 24 个 Unicode 字符。 */
export function normalizedSeries(title: string): string {
  let text = title
  for (const re of [BRACKETS, EP_TOKENS, NOISE]) text = text.replace(re, '')
  return Array.from(text).slice(0, 24).join('')
}

/** 从标题提取「番名归一键 + 集号」，识别 `S01E02` / `1x02` / `- 02` / `第02话`。
 *  识别失败返回 null = 放行（宁可重复不可漏下）。 */
export function episodeKey(title: string): string | null {
  for (const [re, group] of EPISODE_MATCHERS) {
    const caps = re.exec(title)
    if (!caps) continue
    const episode = Number.parseInt(caps[group], 10)
    if (!Number.isFinite(episode) || episode > U32_MAX) continue
    return `${normalizedSeries(title)}#${episode}`
  }
  return null
}

// ---------------------------------------------------------------------------
// 规则编译与判定
// ---------------------------------------------------------------------------

/** 一条订阅的过滤规则（RssSourceDto 的投影）。 */
export interface RssFilterRule {
  include: string
  exclude: string
  useRegex: boolean
  smartEpisode: boolean
  /** 体积下限（字节，0 = 不限）。 */
  sizeMinBytes: number
  /** 体积上限（字节，0 = 不限）。 */
  sizeMaxBytes: number
}

type Matcher =
  /** 空表达式或非法正则：恒真。 */
  | { kind: 'any' }
  | { kind: 'regex'; re: RegExp }
  /** 外层 = `|` 分隔的或项，内层 = 空格分隔的与项（全小写）。 */
  | { kind: 'keywords'; alts: string[][] }

function buildMatcher(expr: string, useRegex: boolean): Matcher {
  const text = expr.trim()
  if (!text) return { kind: 'any' }
  if (useRegex) {
    // 非法正则一律放行：用户写错时宁可多下，也不静默漏下（qBittorrent
    // episodeFilter 写错即静默失配是明确的反面教训）。
    try {
      return { kind: 'regex', re: new RegExp(text, 'i') }
    } catch {
      return { kind: 'any' }
    }
  }
  return {
    kind: 'keywords',
    alts: text.split('|').map((alt) => alt.split(/\s+/).filter(Boolean).map((w) => w.toLowerCase())),
  }
}

/** `lowered` 为调用方预先小写化的标题（避免逐 alt 重复分配）。 */
function matches(matcher: Matcher, title: string, lowered: string): boolean {
  switch (matcher.kind) {
    case 'any':
      return true
    case 'regex':
      return matcher.re.test(title)
    // 空的与项集合视为命中（`"a||b"` 中的空段）。
    case 'keywords':
      return matcher.alts.some((words) => words.every((w) => lowered.includes(w)))
  }
}

/** 预编译后的规则——正则只编译一次，供一整轮条目复用。 */
export interface CompiledRssRule {
  include: Matcher
  exclude: Matcher
  smartEpisode: boolean
  sizeMinBytes: number
  sizeMaxBytes: number
}

export function compileRule(rule: RssFilterRule): CompiledRssRule {
  return {
    include: buildMatcher(rule.include, rule.useRegex),
    exclude: buildMatcher(rule.exclude, rule.useRegex),
    smartEpisode: rule.smartEpisode,
    sizeMinBytes: Math.max(0, rule.sizeMinBytes),
    sizeMaxBytes: Math.max(0, rule.sizeMaxBytes),
  }
}

/** 单条目的判定结论。 */
export type RssVerdict =
  | { accepted: true; episodeKey: string }
  | { accepted: false; reason: RssRejectReason; episodeKey: string }

/**
 * 判定一个条目。`size <= 0` = 未知大小，跳过体积判定；`seen` 是**同源**已占用的
 * 剧集键集合，命中 Accept 且识别出剧集键时就地登记，使同一轮内的后续同集条目
 * 被判为重复。
 */
export function evaluate(rule: CompiledRssRule, title: string, size: number, seen: Set<string>): RssVerdict {
  const lowered = title.toLowerCase()
  if (!matches(rule.include, title, lowered)) {
    return { accepted: false, reason: 'not_included', episodeKey: '' }
  }
  if (rule.exclude.kind !== 'any' && matches(rule.exclude, title, lowered)) {
    return { accepted: false, reason: 'excluded', episodeKey: '' }
  }
  if (size > 0) {
    if (rule.sizeMinBytes > 0 && size < rule.sizeMinBytes) {
      return { accepted: false, reason: 'too_small', episodeKey: '' }
    }
    if (rule.sizeMaxBytes > 0 && size > rule.sizeMaxBytes) {
      return { accepted: false, reason: 'too_large', episodeKey: '' }
    }
  }
  let key = ''
  if (rule.smartEpisode) {
    const k = episodeKey(title)
    if (k !== null) {
      if (seen.has(k)) return { accepted: false, reason: 'dup_episode', episodeKey: k }
      seen.add(k)
      key = k
    }
  }
  return { accepted: true, episodeKey: key }
}

/** 一条预览结果：条目 + 判定。 */
export interface RssPreviewRow {
  item: RssItemDto
  verdict: RssVerdict
}

/**
 * 对一批已缓存条目整轮试跑规则。去重集合每次从空开始（不播种历史已下条目），
 * 因此预览回答的是「若这批条目现在重新抓一遍会怎样」——与引擎单轮内的语义一致。
 */
export function previewRule(rule: RssFilterRule, items: RssItemDto[]): RssPreviewRow[] {
  const compiled = compileRule(rule)
  const seen = new Set<string>()
  return items.map((item) => ({ item, verdict: evaluate(compiled, item.title, item.enclosureLength, seen) }))
}

/** 订阅源 → 过滤规则投影（对话框未保存时用表单值另行构造）。 */
export function ruleOf(source: RssSourceDto): RssFilterRule {
  return {
    include: source.includePattern,
    exclude: source.excludePattern,
    useRegex: source.useRegex,
    smartEpisode: source.smartEpisode,
    sizeMinBytes: source.sizeMinBytes,
    sizeMaxBytes: source.sizeMaxBytes,
  }
}

/**
 * 订阅显示名：未命名时退到 feed **主机名**而不是整条 URL——真实 feed 链接常带
 * token/passkey，铺在侧边栏既顶掉行内其它元素，也把凭证摊在屏幕上。
 * URL 解析不了（用户手输了半截）才退回原串。
 *
 * 侧边栏 / 条目流头部 / 管理对话框标题 / 删除确认文案共用这一处，避免同一条订阅
 * 在四个地方叫四个名字。
 */
export function sourceDisplayName(source: RssSourceDto): string {
  if (source.name) return source.name
  try {
    return new URL(source.url).host || source.url
  } catch {
    return source.url
  }
}
