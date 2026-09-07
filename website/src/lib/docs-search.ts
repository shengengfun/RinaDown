/**
 * 文档客户端搜索引擎(零依赖)。
 *
 * 结合两种匹配:
 *  - 模糊匹配(fuzzy):子序列打分,容忍拼写不连续/缩写,主要作用于标题与小标题;
 *  - 全文匹配(full-text):在正文纯文本中做子串命中,并抽取带高亮的片段。
 *
 * 中英文通用:CJK 无词边界,子序列与子串匹配对单字仍有效;英文额外奖励词首命中。
 */

export interface SearchDoc {
  slug: string;
  href: string;
  title: string;
  section: string;
  description: string;
  headings: string[];
  text: string;
}

/** 片段中的一段:是否为命中高亮。 */
export interface SnippetPart {
  text: string;
  hit: boolean;
}

export interface SearchResult {
  doc: SearchDoc;
  score: number;
  /** 命中来源(用于结果区分显示):title / heading / text。 */
  matchedHeading?: string;
  snippet: SnippetPart[];
}

/**
 * 模糊子序列打分:query 的每个字符按序出现在 target 中即命中。
 * 返回 [0,1] 相对分,连续命中与词首命中加权更高;不匹配返回 -1。
 */
function fuzzyScore(query: string, target: string): number {
  if (!query) return 0;
  const q = query.toLowerCase();
  const t = target.toLowerCase();
  let qi = 0;
  let score = 0;
  let streak = 0;
  let prevIdx = -2;
  for (let ti = 0; ti < t.length && qi < q.length; ti++) {
    if (t[ti] === q[qi]) {
      streak = ti === prevIdx + 1 ? streak + 1 : 1;
      let pt = 1 + streak * 0.5; // 连续命中越长权重越高
      const prevCh = ti > 0 ? t[ti - 1] : " ";
      if (prevCh === " " || prevCh === "-" || prevCh === "/") pt += 2; // 词首
      score += pt;
      prevIdx = ti;
      qi++;
    }
  }
  if (qi < q.length) return -1; // 未能覆盖全部 query 字符
  // 归一化:满分 ~ 每字符按最高权重命中;再对目标长度轻微惩罚(短目标更相关)
  const maxPer = 3.5;
  const norm = score / (q.length * maxPer);
  const lenPenalty = Math.min(target.length, 60) / 600;
  return Math.max(0, Math.min(1, norm - lenPenalty));
}

/** 全文子串命中位置(小写),未命中返回 -1。 */
function substringIndex(query: string, target: string): number {
  if (!query) return -1;
  return target.toLowerCase().indexOf(query.toLowerCase());
}

/**
 * 从正文中抽取命中片段并标出高亮,窗口约 `radius` 字符;未命中返回空片段。
 */
function extractSnippet(
  text: string,
  query: string,
  radius = 60,
): SnippetPart[] {
  const idx = substringIndex(query, text);
  if (idx < 0) return [];
  const start = Math.max(0, idx - radius);
  const end = Math.min(text.length, idx + query.length + radius);
  const before = (start > 0 ? "…" : "") + text.slice(start, idx);
  const hit = text.slice(idx, idx + query.length);
  const after = text.slice(idx + query.length, end) + (end < text.length ? "…" : "");
  return [
    { text: before, hit: false },
    { text: hit, hit: true },
    { text: after, hit: false },
  ];
}

const TITLE_WEIGHT = 6;
const HEADING_WEIGHT = 3.5;
const DESC_WEIGHT = 2;
const TEXT_FULL_WEIGHT = 4; // 正文全文子串命中(强信号)
const TEXT_FUZZY_WEIGHT = 1.2;

/**
 * 在文档集合上执行搜索,返回按相关性降序的结果。
 * @param limit 最多返回条数(默认 20)。
 */
export function searchDocs(
  docs: SearchDoc[],
  rawQuery: string,
  limit = 20,
): SearchResult[] {
  const query = rawQuery.trim();
  if (query.length === 0) return [];

  const results: SearchResult[] = [];

  for (const doc of docs) {
    let score = 0;
    let matchedHeading: string | undefined;
    let snippet: SnippetPart[] = [];

    // 标题:模糊 + 子串双通道取高
    const titleFuzzy = fuzzyScore(query, doc.title);
    const titleSub = substringIndex(query, doc.title) >= 0 ? 1 : 0;
    if (titleFuzzy > 0) score += titleFuzzy * TITLE_WEIGHT;
    if (titleSub) score += TITLE_WEIGHT;

    // 描述
    if (substringIndex(query, doc.description) >= 0) score += DESC_WEIGHT;

    // 小标题:取最佳命中,记录用于结果标注
    let bestHeadingScore = 0;
    for (const h of doc.headings) {
      const hs = fuzzyScore(query, h);
      const sub = substringIndex(query, h) >= 0 ? 1 : 0;
      const combined = Math.max(hs, sub);
      if (combined > bestHeadingScore) {
        bestHeadingScore = combined;
        matchedHeading = h;
      }
    }
    if (bestHeadingScore > 0) score += bestHeadingScore * HEADING_WEIGHT;

    // 正文全文:子串命中给强信号并抽取片段
    const textIdx = substringIndex(query, doc.text);
    if (textIdx >= 0) {
      score += TEXT_FULL_WEIGHT;
      snippet = extractSnippet(doc.text, query);
    } else {
      // 无子串命中时,对正文做一次弱模糊,兜底召回
      const tf = fuzzyScore(query, doc.text.slice(0, 400));
      if (tf > 0) score += tf * TEXT_FUZZY_WEIGHT;
    }

    if (score > 0) {
      // 无正文片段时,用描述或正文开头兜底展示
      if (snippet.length === 0) {
        const fallback = (doc.description || doc.text).slice(0, 140);
        if (fallback) snippet = [{ text: fallback + (doc.text.length > 140 ? "…" : ""), hit: false }];
      }
      results.push({ doc, score, matchedHeading, snippet });
    }
  }

  results.sort((a, b) => b.score - a.score || a.doc.title.localeCompare(b.doc.title));
  return results.slice(0, limit);
}
