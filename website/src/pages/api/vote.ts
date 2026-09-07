import type { APIRoute } from "astro";
import { GITHUB_TOKEN, GITHUB_REPO } from "astro:env/server";

export const prerender = false;

const ISSUE_TITLE = "[Vote] Community Platform Poll";
const ISSUE_LABEL = "vote";
const VALID_OPTIONS = ["wechat", "qq", "official-account"];

const rateLimitMap = new Map<string, { count: number; resetAt: number }>();
const RATE_LIMIT_WINDOW = 60_000;
const RATE_LIMIT_MAX = 5;

function isRateLimited(ip: string): boolean {
  const now = Date.now();
  const entry = rateLimitMap.get(ip);
  if (!entry || now > entry.resetAt) {
    rateLimitMap.set(ip, { count: 1, resetAt: now + RATE_LIMIT_WINDOW });
    return false;
  }
  entry.count += 1;
  return entry.count > RATE_LIMIT_MAX;
}

setInterval(() => {
  const now = Date.now();
  for (const [ip, entry] of rateLimitMap) {
    if (now > entry.resetAt) rateLimitMap.delete(ip);
  }
}, 5 * 60_000);

const ghHeaders = {
  Authorization: `Bearer ${GITHUB_TOKEN}`,
  Accept: "application/vnd.github+json",
  "X-GitHub-Api-Version": "2022-11-28",
  "Content-Type": "application/json",
};

interface VoteComment {
  option: string;
  ip: string;
  date: string;
}

async function findOrCreateIssue(): Promise<number> {
  const searchRes = await fetch(
    `https://api.github.com/repos/${GITHUB_REPO}/issues?labels=${ISSUE_LABEL}&state=open&per_page=1`,
    { headers: ghHeaders },
  );

  if (searchRes.ok) {
    const issues = await searchRes.json();
    if (Array.isArray(issues) && issues.length > 0) {
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
          "## Community Platform Poll",
          "",
          "This issue collects votes from users about which community platform to create.",
          "",
          "**Options:** WeChat Group, QQ Group, Official Account",
          "",
          "**Do not close this issue.** Each comment below represents one vote.",
          "",
          "### Data format",
          "```json",
          '{ "option": "wechat|qq|official-account", "ip": "...", "date": "..." }',
          "```",
        ].join("\n"),
        labels: [ISSUE_LABEL],
      }),
    },
  );

  if (!createRes.ok) {
    const text = await createRes.text();
    throw new Error(`Failed to create vote issue: ${createRes.status} ${text}`);
  }

  const created = await createRes.json();
  return created.number;
}

interface GitHubComment {
  body: string;
}

async function fetchAllComments(issueNumber: number): Promise<GitHubComment[]> {
  const all: GitHubComment[] = [];
  let page = 1;

  while (true) {
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

function parseVoteFromComment(body: string): VoteComment | null {
  const jsonMatch = body.match(/```json\s*([\s\S]*?)```/);
  if (!jsonMatch) return null;
  try {
    const data = JSON.parse(jsonMatch[1]);
    if (data.option && data.ip) return data as VoteComment;
  } catch {
    // malformed JSON
  }
  return null;
}

interface VoteResultsCache {
  data: { results: Record<string, number>; total: number; voted: null };
  timestamp: number;
}

let resultsCache: VoteResultsCache | null = null;
const RESULTS_CACHE_TTL = 30_000;

export const GET: APIRoute = async () => {
  if (!GITHUB_TOKEN) {
    return new Response(JSON.stringify({ error: "Server misconfigured" }), {
      status: 500,
      headers: { "Content-Type": "application/json" },
    });
  }

  try {
    if (
      resultsCache &&
      Date.now() - resultsCache.timestamp < RESULTS_CACHE_TTL
    ) {
      return new Response(JSON.stringify(resultsCache.data), {
        status: 200,
        headers: {
          "Content-Type": "application/json",
          "Cache-Control": "public, s-maxage=30, stale-while-revalidate=60",
        },
      });
    }

    const issueNumber = await findOrCreateIssue();
    const comments = await fetchAllComments(issueNumber);

    const results: Record<string, number> = {};
    for (const opt of VALID_OPTIONS) {
      results[opt] = 0;
    }

    let total = 0;

    for (const comment of comments) {
      const vote = parseVoteFromComment(comment.body);
      if (vote && VALID_OPTIONS.includes(vote.option)) {
        results[vote.option] = (results[vote.option] || 0) + 1;
        total++;
      }
    }

    const data = { results, total, voted: null };
    resultsCache = { data, timestamp: Date.now() };

    return new Response(JSON.stringify(data), {
      status: 200,
      headers: {
        "Content-Type": "application/json",
        "Cache-Control": "public, s-maxage=30, stale-while-revalidate=60",
      },
    });
  } catch (err) {
    console.error("Failed to fetch vote results:", err);
    return new Response(JSON.stringify({ error: "Failed to fetch results" }), {
      status: 500,
      headers: { "Content-Type": "application/json" },
    });
  }
};

export const POST: APIRoute = async ({ request, clientAddress }) => {
  const ip = clientAddress || "unknown";

  if (isRateLimited(ip)) {
    return new Response(JSON.stringify({ error: "Too many requests" }), {
      status: 429,
      headers: { "Content-Type": "application/json" },
    });
  }

  if (!GITHUB_TOKEN) {
    return new Response(JSON.stringify({ error: "Server misconfigured" }), {
      status: 500,
      headers: { "Content-Type": "application/json" },
    });
  }

  let body: { option?: string };
  try {
    body = await request.json();
  } catch {
    return new Response(JSON.stringify({ error: "Invalid JSON body" }), {
      status: 400,
      headers: { "Content-Type": "application/json" },
    });
  }

  const { option } = body;

  if (!option || !VALID_OPTIONS.includes(option)) {
    return new Response(
      JSON.stringify({
        error: `Invalid option. Must be: ${VALID_OPTIONS.join(", ")}`,
      }),
      { status: 400, headers: { "Content-Type": "application/json" } },
    );
  }

  try {
    const issueNumber = await findOrCreateIssue();
    const comments = await fetchAllComments(issueNumber);

    for (const comment of comments) {
      const vote = parseVoteFromComment(comment.body);
      if (vote && vote.ip === ip) {
        return new Response(
          JSON.stringify({ success: true, message: "already_voted" }),
          { status: 200, headers: { "Content-Type": "application/json" } },
        );
      }
    }

    const data = { option, ip, date: new Date().toISOString() };
    const commentBody = [
      "### Vote",
      "",
      "```json",
      JSON.stringify(data, null, 2),
      "```",
      "",
      `- **Option:** ${option}`,
      `- **Date:** ${data.date}`,
    ].join("\n");

    const commentRes = await fetch(
      `https://api.github.com/repos/${GITHUB_REPO}/issues/${issueNumber}/comments`,
      {
        method: "POST",
        headers: ghHeaders,
        body: JSON.stringify({ body: commentBody }),
      },
    );

    if (!commentRes.ok) {
      const text = await commentRes.text();
      console.error(`Failed to add vote: ${commentRes.status}`, text);
      return new Response(JSON.stringify({ error: "Failed to submit vote" }), {
        status: 502,
        headers: { "Content-Type": "application/json" },
      });
    }

    resultsCache = null;

    return new Response(JSON.stringify({ success: true, message: "voted" }), {
      status: 201,
      headers: { "Content-Type": "application/json" },
    });
  } catch (err) {
    console.error("Vote error:", err);
    return new Response(JSON.stringify({ error: "Internal server error" }), {
      status: 500,
      headers: { "Content-Type": "application/json" },
    });
  }
};
