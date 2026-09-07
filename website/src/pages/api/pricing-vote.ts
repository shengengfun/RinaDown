import type { APIRoute } from "astro";
import { GITHUB_TOKEN, GITHUB_REPO } from "astro:env/server";
import { getSessionUser, oauthConfigured, type SessionUser } from "@/lib/github-oauth";

export const prerender = false;

// ─────────────────────────────────────────────
// Pricing poll + discussion, recorded in ONE GitHub tracking issue:
//   * vote records    → { kind: "vote", plan, option, login, userId, ip, date }
//   * discussion post → { kind: "comment", login, userId, avatar, message, ip, date }
// Each record is one issue comment carrying a ```json block.
// Voting/commenting requires GitHub OAuth login (see lib/github-oauth.ts);
// one vote per GitHub user per plan — the FIRST vote wins, replays return
// "already_voted".
// ─────────────────────────────────────────────

const ISSUE_TITLE = "[Vote] Pricing Poll";
const ISSUE_LABEL = "pricing-vote";

const PLANS = ["lifetime", "subscription"] as const;
type Plan = (typeof PLANS)[number];

const VALID_OPTIONS: Record<Plan, string[]> = {
  lifetime: ["lt-69", "lt-99", "lt-129", "lt-199"],
  subscription: ["sub-3", "sub-6", "sub-10", "sub-15plus"],
};

const MESSAGE_MAX = 500;
const COMMENTS_SHOWN = 50;

// Rate limits (per GitHub user)
const voteRateMap = new Map<string, { count: number; resetAt: number }>();
const commentRateMap = new Map<string, { count: number; resetAt: number }>();
const VOTE_RATE_WINDOW = 60_000; // 1 min
const VOTE_RATE_MAX = 5;
const COMMENT_RATE_WINDOW = 10 * 60_000; // 10 min
const COMMENT_RATE_MAX = 3;

function isRateLimited(
  map: Map<string, { count: number; resetAt: number }>,
  key: string,
  windowMs: number,
  max: number,
): boolean {
  const now = Date.now();
  const entry = map.get(key);
  if (!entry || now > entry.resetAt) {
    map.set(key, { count: 1, resetAt: now + windowMs });
    return false;
  }
  entry.count += 1;
  return entry.count > max;
}

setInterval(() => {
  const now = Date.now();
  for (const map of [voteRateMap, commentRateMap]) {
    for (const [key, entry] of map) {
      if (now > entry.resetAt) map.delete(key);
    }
  }
}, 5 * 60_000);

const ghHeaders = {
  Authorization: `Bearer ${GITHUB_TOKEN}`,
  Accept: "application/vnd.github+json",
  "X-GitHub-Api-Version": "2022-11-28",
  "Content-Type": "application/json",
};

// ─────────────────────────────────────────────
// Records
// ─────────────────────────────────────────────

interface VoteRecord {
  kind: "vote";
  plan: Plan;
  option: string;
  login: string;
  userId: number;
  ip: string;
  date: string;
}

interface CommentRecord {
  kind: "comment";
  login: string;
  userId: number;
  avatar: string;
  message: string;
  ip: string;
  date: string;
}

type PollRecord = VoteRecord | CommentRecord;

function parseRecord(body: string): PollRecord | null {
  const jsonMatch = body.match(/```json\s*([\s\S]*?)```/);
  if (!jsonMatch) return null;
  try {
    const data = JSON.parse(jsonMatch[1]);
    if (
      data.kind === "vote" &&
      data.plan &&
      data.option &&
      typeof data.userId === "number"
    ) {
      return data as VoteRecord;
    }
    if (
      data.kind === "comment" &&
      typeof data.message === "string" &&
      typeof data.login === "string"
    ) {
      return data as CommentRecord;
    }
  } catch {
    // malformed JSON
  }
  return null;
}

// ─────────────────────────────────────────────
// GitHub helpers
// ─────────────────────────────────────────────

let issueNumberCache: number | null = null;

async function findOrCreateIssue(): Promise<number> {
  if (issueNumberCache !== null) return issueNumberCache;

  const searchRes = await fetch(
    `https://api.github.com/repos/${GITHUB_REPO}/issues?labels=${ISSUE_LABEL}&state=open&per_page=1`,
    { headers: ghHeaders },
  );

  if (searchRes.ok) {
    const issues = await searchRes.json();
    if (Array.isArray(issues) && issues.length > 0) {
      issueNumberCache = issues[0].number;
      return issues[0].number;
    }
  }

  const createRes = await fetch(
    `https://api.github.com/repos/${GITHUB_REPO}/issues`,
    {
      method: "POST",
      headers: ghHeaders,
      body: JSON.stringify({
        title: ISSUE_TITLE,
        body: [
          "## Pricing Poll & Discussion",
          "",
          "This issue records community votes and discussion about FluxDown premium pricing",
          "(lifetime one-time purchase vs subscription). Final prices are TBD.",
          "",
          "**Do not close this issue.** Each comment below is one vote or one discussion post,",
          "submitted by a GitHub-authenticated user from https://fluxdown.zerx.dev/pricing.",
          "",
          "### Data format",
          "```json",
          '{ "kind": "vote", "plan": "lifetime|subscription", "option": "...", "login": "...", "userId": 0, "ip": "...", "date": "..." }',
          "```",
          "```json",
          '{ "kind": "comment", "login": "...", "userId": 0, "avatar": "...", "message": "...", "ip": "...", "date": "..." }',
          "```",
        ].join("\n"),
        labels: [ISSUE_LABEL],
      }),
    },
  );

  if (!createRes.ok) {
    const text = await createRes.text();
    throw new Error(`Failed to create pricing issue: ${createRes.status} ${text}`);
  }

  const created = await createRes.json();
  issueNumberCache = created.number;
  return created.number;
}

interface GitHubComment {
  body: string;
}

async function fetchAllComments(issueNumber: number): Promise<GitHubComment[]> {
  const all: GitHubComment[] = [];
  let page = 1;

  // Cap at 10 pages (1000 records) to bound worst-case API usage.
  while (page <= 10) {
    const res = await fetch(
      `https://api.github.com/repos/${GITHUB_REPO}/issues/${issueNumber}/comments?per_page=100&page=${page}`,
      { headers: ghHeaders },
    );

    if (!res.ok) break;

    const comments: GitHubComment[] = await res.json();
    if (!Array.isArray(comments) || comments.length === 0) break;

    all.push(...comments);

    if (comments.length < 100) break;
    page++;
  }

  return all;
}

async function postRecord(
  issueNumber: number,
  record: PollRecord,
  readable: string[],
): Promise<boolean> {
  const body = [
    record.kind === "vote" ? "### Vote" : "### Comment",
    "",
    "```json",
    JSON.stringify(record, null, 2),
    "```",
    "",
    ...readable,
  ].join("\n");

  const res = await fetch(
    `https://api.github.com/repos/${GITHUB_REPO}/issues/${issueNumber}/comments`,
    {
      method: "POST",
      headers: ghHeaders,
      body: JSON.stringify({ body }),
    },
  );

  if (!res.ok) {
    const text = await res.text();
    console.error(`Failed to post pricing record: ${res.status}`, text);
  }
  return res.ok;
}

// ─────────────────────────────────────────────
// Records cache (30 s TTL); tallies and viewer state derive per request.
// ─────────────────────────────────────────────

let recordsCache: { records: PollRecord[]; timestamp: number } | null = null;
const RECORDS_CACHE_TTL = 30_000;

async function loadRecords(): Promise<PollRecord[]> {
  if (recordsCache && Date.now() - recordsCache.timestamp < RECORDS_CACHE_TTL) {
    return recordsCache.records;
  }
  const issueNumber = await findOrCreateIssue();
  const comments = await fetchAllComments(issueNumber);
  const records: PollRecord[] = [];
  for (const c of comments) {
    const r = parseRecord(c.body);
    if (r) records.push(r);
  }
  recordsCache = { records, timestamp: Date.now() };
  return records;
}

/** Keep the cached view exact after a successful write (TTL reconciles drift). */
function appendRecordToCache(record: PollRecord): void {
  recordsCache?.records.push(record);
}

/** First vote wins per (user, plan). Returns option or null. */
function userVote(records: PollRecord[], userId: number, plan: Plan): string | null {
  for (const r of records) {
    if (r.kind === "vote" && r.plan === plan && r.userId === userId) return r.option;
  }
  return null;
}

// ─────────────────────────────────────────────
// GET /api/pricing-vote — tallies + discussion + viewer state
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

export const GET: APIRoute = async ({ cookies }) => {
  if (!GITHUB_TOKEN) {
    return json({ error: "Server misconfigured" }, 500);
  }

  try {
    const records = await loadRecords();

    const results: Record<Plan, Record<string, number>> = {
      lifetime: {},
      subscription: {},
    };
    const totals: Record<Plan, number> = { lifetime: 0, subscription: 0 };
    for (const plan of PLANS) {
      for (const opt of VALID_OPTIONS[plan]) results[plan][opt] = 0;
    }

    const seenVoters: Record<Plan, Set<number>> = {
      lifetime: new Set(),
      subscription: new Set(),
    };
    const discussion: { login: string; avatar: string; message: string; date: string }[] = [];

    for (const record of records) {
      if (record.kind === "vote") {
        const plan = record.plan;
        if (!PLANS.includes(plan) || !VALID_OPTIONS[plan].includes(record.option)) continue;
        if (seenVoters[plan].has(record.userId)) continue; // first vote wins
        seenVoters[plan].add(record.userId);
        results[plan][record.option] += 1;
        totals[plan] += 1;
      } else {
        discussion.push({
          login: record.login,
          avatar: typeof record.avatar === "string" ? record.avatar : "",
          message: record.message,
          date: record.date ?? "",
        });
      }
    }

    const user = getSessionUser(cookies);
    const viewer = user
      ? {
          login: user.login,
          avatar: user.avatar,
          votes: {
            lifetime: userVote(records, user.id, "lifetime"),
            subscription: userVote(records, user.id, "subscription"),
          },
        }
      : null;

    return json(
      {
        results,
        totals,
        comments: discussion.slice(-COMMENTS_SHOWN).reverse(),
        issueUrl: `https://github.com/${GITHUB_REPO}/issues/${await findOrCreateIssue()}`,
        viewer,
        loginEnabled: oauthConfigured(),
      },
      200,
      // viewer differs per cookie — keep shared caches out of the way
      { "Cache-Control": "private, max-age=15" },
    );
  } catch (err) {
    console.error("Failed to fetch pricing poll:", err);
    return json({ error: "Failed to fetch results" }, 500);
  }
};

// ─────────────────────────────────────────────
// POST /api/pricing-vote — requires GitHub login
//   { action: "vote", plan, option } | { action: "comment", message }
// ─────────────────────────────────────────────

export const POST: APIRoute = async ({ request, cookies, clientAddress }) => {
  const ip = clientAddress || "unknown";

  if (!GITHUB_TOKEN) {
    return json({ error: "Server misconfigured" }, 500);
  }

  const user = getSessionUser(cookies);
  if (!user) {
    return json({ error: "auth_required" }, 401);
  }

  let body: { action?: string; plan?: string; option?: string; message?: string };
  try {
    body = await request.json();
  } catch {
    return json({ error: "Invalid JSON body" }, 400);
  }

  if (body.action === "vote") return handleVote(body, user, ip);
  if (body.action === "comment") return handleComment(body, user, ip);
  return json({ error: "Invalid action" }, 400);
};

async function handleVote(
  body: { plan?: string; option?: string },
  user: SessionUser,
  ip: string,
): Promise<Response> {
  if (isRateLimited(voteRateMap, `u${user.id}`, VOTE_RATE_WINDOW, VOTE_RATE_MAX)) {
    return json({ error: "Too many requests" }, 429);
  }

  const plan = body.plan as Plan;
  if (!PLANS.includes(plan)) {
    return json({ error: "Invalid plan" }, 400);
  }
  const option = body.option ?? "";
  if (!VALID_OPTIONS[plan].includes(option)) {
    return json({ error: "Invalid option" }, 400);
  }

  try {
    const records = await loadRecords();
    const existing = userVote(records, user.id, plan);
    if (existing !== null) {
      return json({ success: true, message: "already_voted", option: existing }, 200);
    }

    const record: VoteRecord = {
      kind: "vote",
      plan,
      option,
      login: user.login,
      userId: user.id,
      ip,
      date: new Date().toISOString(),
    };
    const issueNumber = await findOrCreateIssue();
    const ok = await postRecord(issueNumber, record, [
      `- **User:** ${user.login}`,
      `- **Plan:** ${plan}`,
      `- **Option:** ${option}`,
      `- **Date:** ${record.date}`,
    ]);

    if (!ok) return json({ error: "Failed to submit vote" }, 502);

    appendRecordToCache(record);
    return json({ success: true, message: "voted" }, 201);
  } catch (err) {
    console.error("Pricing vote error:", err);
    return json({ error: "Internal server error" }, 500);
  }
}

async function handleComment(
  body: { message?: string },
  user: SessionUser,
  ip: string,
): Promise<Response> {
  if (isRateLimited(commentRateMap, `u${user.id}`, COMMENT_RATE_WINDOW, COMMENT_RATE_MAX)) {
    return json({ error: "Too many requests" }, 429);
  }

  // eslint-disable-next-line no-control-regex
  const message = String(body.message ?? "")
    .replace(/[\u0000-\u0008\u000B-\u001F\u007F]/g, "")
    .trim()
    .slice(0, MESSAGE_MAX);

  if (!message) {
    return json({ error: "Message required" }, 400);
  }

  try {
    const issueNumber = await findOrCreateIssue();
    const record: CommentRecord = {
      kind: "comment",
      login: user.login,
      userId: user.id,
      avatar: user.avatar,
      message,
      ip,
      date: new Date().toISOString(),
    };
    // No readable echo of the message: raw user text could contain a ``` line
    // and break the fenced JSON block parsers rely on.
    const ok = await postRecord(issueNumber, record, [
      `- **User:** ${user.login}`,
      `- **Date:** ${record.date}`,
    ]);

    if (!ok) return json({ error: "Failed to submit comment" }, 502);

    appendRecordToCache(record);
    return json(
      {
        success: true,
        comment: {
          login: user.login,
          avatar: user.avatar,
          message,
          date: record.date,
        },
      },
      201,
    );
  } catch (err) {
    console.error("Pricing comment error:", err);
    return json({ error: "Internal server error" }, 500);
  }
}
