import type { APIRoute } from "astro";
import { GITHUB_TOKEN, GITHUB_REPO } from "astro:env/server";

export const prerender = false;

// ─────────────────────────────────────────────
// Feature voting backed by the SAME GitHub Issues as user feedback:
//   * Candidate features = open issues labeled "enhancement"
//     (i.e. feedback of type=feature, from website form / app / vote page)
//   * Vote count = GitHub 👍 reactions on the issue
//                + anonymous website votes (JSON records in ONE tracking issue,
//                  per-IP dedup — a single bot token cannot emit one reaction
//                  per visitor, so site votes are recorded separately and summed)
// GET needs only 2 GitHub API calls: list issues (reactions included) +
// list tracking comments. POST (vote/unvote/propose) patches the cached view.
// ─────────────────────────────────────────────

const VOTES_ISSUE_TITLE = "[FluxDown] Feature Vote Records";
const FEATURE_LABEL = "enhancement";

const CACHE_TTL = 30_000; // 30 s

// Rate limits (per IP)
const VOTE_RATE_WINDOW = 60_000; // 1 min
const VOTE_RATE_MAX = 20;
const PROPOSE_RATE_WINDOW = 10 * 60_000; // 10 min
const PROPOSE_RATE_MAX = 3;

const TITLE_MAX = 80;
const DESC_MAX = 1000;

// ─────────────────────────────────────────────
// GitHub helpers
// ─────────────────────────────────────────────

function ghHeaders(): Record<string, string> {
  return {
    Authorization: `Bearer ${GITHUB_TOKEN}`,
    Accept: "application/vnd.github+json",
    "X-GitHub-Api-Version": "2022-11-28",
    "Content-Type": "application/json",
  };
}

interface GHIssue {
  number: number;
  title: string;
  body: string | null;
  created_at: string;
  state?: string;
  comments?: number;
  labels?: Array<{ name: string } | string>;
  reactions?: { "+1"?: number };
  pull_request?: unknown;
}

interface GHComment {
  id: number;
  body: string;
}

function labelNames(issue: GHIssue): string[] {
  return (issue.labels ?? []).map((l) => (typeof l === "string" ? l : l.name));
}

/** Fetch with simple retry (up to 3 attempts, back-off). */
async function fetchWithRetry(
  url: string,
  init: RequestInit,
  attempts = 3,
): Promise<Response> {
  let lastErr: unknown;
  for (let i = 0; i < attempts; i++) {
    try {
      return await fetch(url, init);
    } catch (err) {
      lastErr = err;
      if (i < attempts - 1) {
        await new Promise((r) => setTimeout(r, 500 * (i + 1)));
      }
    }
  }
  throw lastErr;
}

/**
 * Fetch comments for an issue (paginated, capped at 10 pages = 1000 records
 * to bound worst-case API usage). Returns [] on error.
 */
async function fetchAllComments(issueNumber: number): Promise<GHComment[]> {
  const all: GHComment[] = [];
  let page = 1;
  while (page <= 10) {
    let res: Response;
    try {
      res = await fetchWithRetry(
        `https://api.github.com/repos/${GITHUB_REPO}/issues/${issueNumber}/comments?per_page=100&page=${page}`,
        { headers: ghHeaders() },
      );
    } catch {
      break;
    }
    if (!res.ok) break;
    const batch: GHComment[] = await res.json();
    if (!Array.isArray(batch) || batch.length === 0) break;
    all.push(...batch);
    if (batch.length < 100) break;
    page++;
  }
  return all;
}

/**
 * Fetch open feature-suggestion issues (label "enhancement", max 3 pages).
 * These are the same issues the feedback tracker lists — one source of truth.
 * Throws when even the FIRST page cannot be fetched (rate limit / outage) so
 * callers never cache an empty list; later-page failures degrade gracefully.
 */
async function fetchFeatureIssues(): Promise<GHIssue[]> {
  const features: GHIssue[] = [];
  for (let page = 1; page <= 3; page++) {
    let res: Response;
    try {
      res = await fetchWithRetry(
        `https://api.github.com/repos/${GITHUB_REPO}/issues?state=open&labels=${FEATURE_LABEL}&per_page=100&page=${page}`,
        { headers: ghHeaders() },
      );
    } catch (err) {
      if (page === 1) throw err;
      break;
    }
    if (!res.ok) {
      if (page === 1) {
        throw new Error(
          `list issues failed: ${res.status}${isRateLimitedResponse(res) ? " (rate limited)" : ""}`,
        );
      }
      break;
    }
    const issues: GHIssue[] = await res.json();
    if (!Array.isArray(issues)) break;
    for (const issue of issues) {
      if (issue.pull_request) continue;
      if (issue.title === VOTES_ISSUE_TITLE) continue;
      features.push(issue);
    }
    if (issues.length < 100) break;
  }
  return features;
}

/** GitHub rate-limit / abuse responses (403 with remaining=0, or 429). */
function isRateLimitedResponse(res: Response): boolean {
  return (
    res.status === 429 ||
    (res.status === 403 && res.headers.get("x-ratelimit-remaining") === "0")
  );
}

// The tracking issue never moves once discovered — cache its number for the
// lifetime of the serverless instance to save up to 3 list calls per request.
let votesIssueNumberCache: number | null = null;

/** Find or lazily create the single votes-tracking issue. */
async function findOrCreateVotesIssue(): Promise<number> {
  if (votesIssueNumberCache !== null) return votesIssueNumberCache;
  // Search open issues for the tracking issue by exact title.
  for (let page = 1; page <= 3; page++) {
    let res: Response;
    try {
      res = await fetchWithRetry(
        `https://api.github.com/repos/${GITHUB_REPO}/issues?state=open&per_page=100&page=${page}`,
        { headers: ghHeaders() },
      );
    } catch {
      break;
    }
    if (!res.ok) break;
    const issues: GHIssue[] = await res.json();
    if (!Array.isArray(issues)) break;
    const hit = issues.find((i) => i.title === VOTES_ISSUE_TITLE);
    if (hit) {
      votesIssueNumberCache = hit.number;
      return hit.number;
    }
    if (issues.length < 100) break;
  }

  const res = await fetchWithRetry(
    `https://api.github.com/repos/${GITHUB_REPO}/issues`,
    {
      method: "POST",
      headers: ghHeaders(),
      body: JSON.stringify({
        title: VOTES_ISSUE_TITLE,
        body: [
          "## FluxDown Feature Vote Records",
          "",
          "This issue stores all feature vote comments.",
          "Each comment is a JSON record: `{ featureId, ip, action, date }`.",
          "**Do not close or rename this issue.**",
        ].join("\n"),
      }),
    },
  );

  if (!res.ok) {
    const text = await res.text();
    throw new Error(`Failed to create votes issue: ${res.status} ${text}`);
  }
  const created: GHIssue = await res.json();
  votesIssueNumberCache = created.number;
  return created.number;
}

// ─────────────────────────────────────────────
// Vote record helpers
// ─────────────────────────────────────────────

interface VoteRecord {
  featureId: number;
  ip: string;
  action: "vote" | "unvote";
  date: string;
}

function parseVoteComment(body: string): VoteRecord | null {
  const m = body.match(/```json\s*([\s\S]*?)```/);
  if (!m) return null;
  try {
    const d = JSON.parse(m[1]);
    if (
      typeof d.featureId === "number" &&
      typeof d.ip === "string" &&
      (d.action === "vote" || d.action === "unvote")
    ) {
      return d as VoteRecord;
    }
  } catch {
    // malformed
  }
  return null;
}

function buildVoteCommentBody(record: VoteRecord): string {
  return [
    "### Feature Vote Record",
    "",
    "```json",
    JSON.stringify(record, null, 2),
    "```",
    "",
    `- **Feature:** #${record.featureId}`,
    `- **Action:** ${record.action}`,
    `- **Date:** ${record.date}`,
  ].join("\n");
}

/** Net website votes for a feature. Per-IP replay: last action wins. */
function countVotes(records: VoteRecord[], featureId: number): number {
  const perIp = new Map<string, "vote" | "unvote">();
  for (const r of records) {
    if (r.featureId === featureId) perIp.set(r.ip, r.action);
  }
  let count = 0;
  for (const a of perIp.values()) {
    if (a === "vote") count++;
  }
  return count;
}

// ─────────────────────────────────────────────
// Vote-records cache (shared by GET and vote handler)
//
// Every vote/unvote appends one tracking comment, so re-reading ALL comments
// on every request is the biggest API cost. Cache the parsed records for
// CACHE_TTL and append new records in place after each successful write —
// the cached view stays exact for this instance; TTL expiry reconciles
// cross-instance drift with GitHub as the source of truth.
// ─────────────────────────────────────────────

let recordsCache: { records: VoteRecord[]; timestamp: number } | null = null;

async function loadVoteRecords(): Promise<VoteRecord[]> {
  if (recordsCache && Date.now() - recordsCache.timestamp < CACHE_TTL) {
    return recordsCache.records;
  }
  const votesIssueNumber = await findOrCreateVotesIssue();
  const comments = await fetchAllComments(votesIssueNumber);
  const records = comments
    .map((c) => parseVoteComment(c.body))
    .filter((r): r is VoteRecord => r !== null);
  recordsCache = { records, timestamp: Date.now() };
  return records;
}

function appendRecordToCache(record: VoteRecord): void {
  if (!recordsCache) return;
  recordsCache.records.push(record);
  recordsCache.timestamp = Date.now();
}

// ─────────────────────────────────────────────
// Title / description extraction (feedback issue format)
// ─────────────────────────────────────────────

/**
 * Strip the feedback/feature title decoration:
 *   "✨ [Website Feedback] xxx" / "🐛 [App Feedback] xxx" /
 *   "✨ [Feature] xxx" / legacy "[FeatureVote] xxx" → "xxx"
 */
function displayTitle(raw: string): string {
  return raw
    .replace(
      /^[^[\]]{0,6}\[[^\]]*(?:Feedback|Feature)[^\]]*\]\s*/iu,
      "",
    )
    .trim();
}

/**
 * Extract a display-friendly excerpt from a feedback-format body:
 * take the part before the metadata separator, drop markdown headings
 * and legacy meta blocks.
 */
function extractDescription(body: string | null): string {
  if (!body) return "";
  const sepIdx = body.indexOf("\n---\n");
  const content = sepIdx >= 0 ? body.slice(0, sepIdx) : body;
  const cleaned = content
    .replace(/^#{1,6}\s+.*$/gm, "")
    .replace(/<!--\s*fluxdown:feature-meta[\s\S]*?-->/g, "")
    .replace(/```json\s*[\s\S]*?```/g, "")
    .trim();
  return cleaned.length > 300 ? `${cleaned.slice(0, 300)}…` : cleaned;
}

// ─────────────────────────────────────────────
// In-memory cache
// ─────────────────────────────────────────────

interface FeatureEntry {
  id: number;
  title: string;
  description: string;
  createdAt: string;
  votes: number;
  comments: number;
}

interface ListCache {
  data: { features: FeatureEntry[]; totalVotes: number };
  timestamp: number;
}

let listCache: ListCache | null = null;

function sortEntries(entries: FeatureEntry[]): void {
  entries.sort(
    (a, b) =>
      b.votes - a.votes || Date.parse(b.createdAt) - Date.parse(a.createdAt),
  );
}

/**
 * GitHub's list APIs are eventually consistent (seconds of lag), so instead
 * of busting the cache after a write — which would re-cache stale data —
 * we patch the cached view in place and extend its freshness window.
 * TTL expiry later reconciles with GitHub as the source of truth.
 */
function patchCacheVote(featureId: number, delta: number): void {
  if (!listCache) return;
  const entry = listCache.data.features.find((f) => f.id === featureId);
  if (!entry) return;
  entry.votes = Math.max(0, entry.votes + delta);
  sortEntries(listCache.data.features);
  listCache.data.totalVotes = listCache.data.features.reduce(
    (s, e) => s + e.votes,
    0,
  );
  listCache.timestamp = Date.now();
}

function patchCachePropose(entry: FeatureEntry): void {
  // Cold cache: don't fabricate a single-item view that would hide
  // existing features for a full TTL — let the next GET fetch normally.
  if (!listCache) return;
  if (listCache.data.features.some((f) => f.id === entry.id)) return;
  listCache.data.features.push(entry);
  sortEntries(listCache.data.features);
  listCache.timestamp = Date.now();
}

// ─────────────────────────────────────────────
// Rate limiting
// ─────────────────────────────────────────────

interface RateEntry {
  count: number;
  resetAt: number;
}

const voteRateMap = new Map<string, RateEntry>();
const proposeRateMap = new Map<string, RateEntry>();

setInterval(() => {
  const now = Date.now();
  for (const map of [voteRateMap, proposeRateMap]) {
    for (const [ip, e] of map) {
      if (now > e.resetAt) map.delete(ip);
    }
  }
}, 5 * 60_000);

function isRateLimited(
  map: Map<string, RateEntry>,
  ip: string,
  windowMs: number,
  max: number,
): boolean {
  const now = Date.now();
  const entry = map.get(ip);
  if (!entry || now > entry.resetAt) {
    map.set(ip, { count: 1, resetAt: now + windowMs });
    return false;
  }
  entry.count += 1;
  return entry.count > max;
}

// ─────────────────────────────────────────────
// GET /api/feature-vote — feature list with vote counts
// ─────────────────────────────────────────────

export const GET: APIRoute = async () => {
  if (!GITHUB_TOKEN) {
    return json({ error: "Server misconfigured" }, 500);
  }

  if (listCache && Date.now() - listCache.timestamp < CACHE_TTL) {
    return json(listCache.data, 200, {
      "Cache-Control": "public, s-maxage=30, stale-while-revalidate=60",
    });
  }

  try {
    const features = await fetchFeatureIssues();

    let records: VoteRecord[] = [];
    try {
      records = await loadVoteRecords();
    } catch (err) {
      // Site votes are additive — degrade to reactions-only counts.
      console.error("[feature-vote] failed to load vote records:", err);
    }

    const entries: FeatureEntry[] = features.map((issue) => ({
      id: issue.number,
      title: displayTitle(issue.title),
      description: extractDescription(issue.body),
      createdAt: issue.created_at,
      votes:
        (issue.reactions?.["+1"] ?? 0) + countVotes(records, issue.number),
      comments: issue.comments ?? 0,
    }));

    // Sort: most votes first, then newest first
    sortEntries(entries);

    const data = {
      features: entries,
      totalVotes: entries.reduce((s, e) => s + e.votes, 0),
    };
    listCache = { data, timestamp: Date.now() };

    return json(data, 200, {
      "Cache-Control": "public, s-maxage=30, stale-while-revalidate=60",
    });
  } catch (err) {
    console.error("[feature-vote] GET error:", err);
    // Serve the last good snapshot (even past its TTL) instead of failing —
    // typical cause is a GitHub rate limit, which resolves within the hour.
    if (listCache) {
      return json(listCache.data, 200, {
        "Cache-Control": "public, s-maxage=30, stale-while-revalidate=60",
      });
    }
    return json({ error: "Failed to fetch feature list" }, 503);
  }
};

// ─────────────────────────────────────────────
// POST /api/feature-vote — vote / unvote / propose
// ─────────────────────────────────────────────

export const POST: APIRoute = async ({ request, clientAddress }) => {
  const ip = clientAddress || "unknown";

  if (!GITHUB_TOKEN) {
    return json({ error: "Server misconfigured" }, 500);
  }

  let body: {
    action?: string;
    featureId?: unknown;
    title?: unknown;
    description?: unknown;
  };
  try {
    body = await request.json();
  } catch {
    return json({ error: "Invalid JSON body" }, 400);
  }

  if (body.action === "propose") {
    return handlePropose(body, ip);
  }
  if (body.action === "vote" || body.action === "unvote") {
    return handleVote(body.action, body.featureId, ip);
  }
  return json({ error: "action must be 'vote', 'unvote' or 'propose'" }, 400);
};

async function handleVote(
  action: "vote" | "unvote",
  featureIdRaw: unknown,
  ip: string,
): Promise<Response> {
  if (isRateLimited(voteRateMap, ip, VOTE_RATE_WINDOW, VOTE_RATE_MAX)) {
    return json({ error: "Too many requests" }, 429);
  }

  const featureId = Number(featureIdRaw);
  if (!Number.isInteger(featureId) || featureId <= 0) {
    return json({ error: "featureId is required" }, 400);
  }

  // Validate: must be an existing open issue labeled as a feature suggestion
  let checkRes: Response;
  try {
    checkRes = await fetchWithRetry(
      `https://api.github.com/repos/${GITHUB_REPO}/issues/${featureId}`,
      { headers: ghHeaders() },
    );
  } catch {
    return json({ error: "Failed to validate feature" }, 502);
  }
  if (!checkRes.ok) {
    if (isRateLimitedResponse(checkRes)) {
      return json({ error: "GitHub rate limited, try again later" }, 503);
    }
    return json({ error: "Feature not found" }, 404);
  }
  const issue: GHIssue = await checkRes.json();
  if (
    issue.pull_request ||
    issue.state !== "open" ||
    !labelNames(issue).includes(FEATURE_LABEL)
  ) {
    return json({ error: "Feature not found" }, 404);
  }

  try {
    const votesIssueNumber = await findOrCreateVotesIssue();

    // Determine current state for this IP + feature (last action wins).
    // Uses the shared records cache — a fresh miss re-reads GitHub; hits
    // reuse records patched in place by previous votes on this instance.
    const records = await loadVoteRecords();
    const mine = records.filter(
      (r) => r.featureId === featureId && r.ip === ip,
    );
    const lastAction = mine.length > 0 ? mine[mine.length - 1].action : null;

    // Idempotent checks
    if (action === "vote" && lastAction === "vote") {
      return json({ success: true, message: "already_voted" }, 200);
    }
    if (action === "unvote" && lastAction !== "vote") {
      return json({ success: true, message: "not_voted" }, 200);
    }

    const record: VoteRecord = {
      featureId,
      ip,
      action,
      date: new Date().toISOString(),
    };

    const commentRes = await fetchWithRetry(
      `https://api.github.com/repos/${GITHUB_REPO}/issues/${votesIssueNumber}/comments`,
      {
        method: "POST",
        headers: ghHeaders(),
        body: JSON.stringify({ body: buildVoteCommentBody(record) }),
      },
    );

    if (!commentRes.ok) {
      const text = await commentRes.text();
      console.error(
        `[feature-vote] Failed to post vote comment: ${commentRes.status}`,
        text,
      );
      if (commentRes.status === 404) {
        // Tracking issue vanished (deleted/transferred) — drop the cached
        // number so the next attempt re-discovers or re-creates it.
        votesIssueNumberCache = null;
        recordsCache = null;
      }
      if (isRateLimitedResponse(commentRes)) {
        return json({ error: "GitHub rate limited, try again later" }, 503);
      }
      return json({ error: "Failed to record vote" }, 502);
    }

    // NOTE: 不用 bot 帐号补 👍 reaction —— 站内票已计入 tracking 记录，
    // bot reaction 会在缓存过期后与记录双重计数（单 token 也只能 +1）。
    // GitHub 侧真实用户的 👍 与站内记录相加即为总票数。

    // Patch the cached views in place (GitHub list APIs lag by seconds;
    // a plain cache-bust would just re-cache stale counts).
    appendRecordToCache(record);
    patchCacheVote(featureId, action === "vote" ? 1 : -1);

    return json(
      { success: true, message: action === "vote" ? "voted" : "unvoted" },
      201,
    );
  } catch (err) {
    console.error("[feature-vote] vote error:", err);
    return json({ error: "Internal server error" }, 500);
  }
}

async function handlePropose(
  body: { title?: unknown; description?: unknown },
  ip: string,
): Promise<Response> {
  if (
    isRateLimited(proposeRateMap, ip, PROPOSE_RATE_WINDOW, PROPOSE_RATE_MAX)
  ) {
    return json({ error: "Too many requests" }, 429);
  }

  const title = typeof body.title === "string" ? body.title.trim() : "";
  const description =
    typeof body.description === "string" ? body.description.trim() : "";

  if (!title || title.length > TITLE_MAX) {
    return json({ error: `title is required (max ${TITLE_MAX} chars)` }, 400);
  }
  if (description.length > DESC_MAX) {
    return json({ error: `description too long (max ${DESC_MAX} chars)` }, 400);
  }

  try {
    // Create the proposal as a regular feedback issue (feature type), so it
    // shows up in BOTH the feedback tracker and the vote list, and the
    // detail modal can parse its structured metadata.
    const issueBody = [
      "## Feature Request",
      "",
      "### 建议内容 / Proposal",
      "",
      description || "_(no description)_",
      "",
      "---",
      "",
      "**Type:** feature",
      "**Source:** Website feature vote",
      `**Submitted:** ${new Date().toISOString()}`,
      `**IP:** \`${ip}\``,
    ].join("\n");

    const res = await fetchWithRetry(
      `https://api.github.com/repos/${GITHUB_REPO}/issues`,
      {
        method: "POST",
        headers: ghHeaders(),
        body: JSON.stringify({
          title: `\u2728 [Website Feedback] ${title}`,
          body: issueBody,
          labels: ["user-feedback", FEATURE_LABEL],
        }),
      },
    );

    if (!res.ok) {
      const text = await res.text();
      console.error(`[feature-vote] Failed to create issue: ${res.status}`, text);
      if (isRateLimitedResponse(res)) {
        return json({ error: "GitHub rate limited, try again later" }, 503);
      }
      return json({ error: "Failed to create feature" }, 502);
    }

    const created: GHIssue = await res.json();

    // Patch the cached view so the new feature is visible immediately
    patchCachePropose({
      id: created.number,
      title,
      description:
        description.length > 300 ? `${description.slice(0, 300)}…` : description,
      createdAt: created.created_at,
      votes: 0,
      comments: 0,
    });

    return json({ success: true, featureId: created.number }, 201);
  } catch (err) {
    console.error("[feature-vote] propose error:", err);
    return json({ error: "Internal server error" }, 500);
  }
}

// ─────────────────────────────────────────────
// Helper
// ─────────────────────────────────────────────

function json(
  data: unknown,
  status: number,
  extraHeaders: Record<string, string> = {},
): Response {
  return new Response(JSON.stringify(data), {
    status,
    headers: { "Content-Type": "application/json", ...extraHeaders },
  });
}
