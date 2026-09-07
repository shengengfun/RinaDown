/**
 * GET /api/issues/:number
 *
 * 获取单个 GitHub Issue 详情 + 评论列表。
 *
 * 返回格式:
 * {
 *   issue: { number, title, state, labels, created_at, updated_at, comments_count, user, body },
 *   comments: [ { id, user, body, created_at, updated_at, reactions } ]
 * }
 */

import type { APIRoute } from "astro";
import { GITHUB_TOKEN, GITHUB_REPO } from "astro:env/server";

export const prerender = false;

// ── 缓存（每个 Issue 独立缓存） ──
interface CacheEntry {
  data: IssueDetail;
  timestamp: number;
}

const detailCache = new Map<number, CacheEntry>();
const CACHE_TTL = 60_000; // 1 分钟

// ── 类型 ──

interface GitHubLabel {
  id: number;
  name: string;
  color: string;
}

interface GitHubUser {
  login: string;
  avatar_url: string;
  html_url: string;
}

interface GitHubReactions {
  "+1": number;
  "-1": number;
  laugh: number;
  hooray: number;
  confused: number;
  heart: number;
  rocket: number;
  eyes: number;
}

interface GitHubComment {
  id: number;
  user: GitHubUser;
  body: string;
  created_at: string;
  updated_at: string;
  reactions: GitHubReactions;
}

interface GitHubIssue {
  number: number;
  title: string;
  state: string;
  state_reason: string | null; // "completed" | "not_planned" | "reopened" | null
  labels: GitHubLabel[];
  created_at: string;
  updated_at: string;
  comments: number;
  user: GitHubUser;
  body: string | null;
  reactions: GitHubReactions;
}

/**
 * 归一化的关闭原因:
 *   "completed"   - 已完成
 *   "not_planned" - 不采纳
 *   "duplicate"   - 重复项
 *   null          - 未关闭
 */
type CloseReason = "completed" | "not_planned" | "duplicate" | null;

function deriveCloseReason(issue: GitHubIssue): CloseReason {
  if (issue.state !== "closed") return null;
  if (issue.labels.some((l) => l.name.toLowerCase() === "duplicate"))
    return "duplicate";
  if (issue.state_reason === "completed") return "completed";
  if (issue.state_reason === "not_planned") return "not_planned";
  return "completed";
}

/** 从 feedback body 中解析的结构化元数据 */
interface ParsedMetadata {
  type: string | null; // feature / bug / other
  contact: string | null; // 联系方式（可选）
  source: string | null; // 来源
  submitted_at: string | null; // 提交时间 ISO
}

interface IssueDetail {
  issue: {
    number: number;
    title: string;
    state: string;
    close_reason: CloseReason;
    labels: { name: string; color: string }[];
    created_at: string;
    updated_at: string;
    comments_count: number;
    user: { login: string; avatar_url: string };
    /** 纯正文描述（去掉 ## 标题和 --- 之后的元数据区） */
    description: string;
    /** 原始 body（非 feedback 格式时回退使用） */
    body_raw: string;
    /** 结构化元数据（仅 feedback 格式存在，IP 已隐藏） */
    metadata: ParsedMetadata | null;
    /** 是否为 website feedback 格式的 body */
    is_feedback_format: boolean;
    reactions: GitHubReactions;
  };
  comments: {
    id: number;
    user: { login: string; avatar_url: string };
    body: string;
    created_at: string;
    updated_at: string;
    reactions: GitHubReactions;
  }[];
}

const defaultHeaders = {
  Authorization: `Bearer ${GITHUB_TOKEN}`,
  Accept: "application/vnd.github+json",
  "X-GitHub-Api-Version": "2022-11-28",
};

// ── Body 解析 ──

/**
 * 检测并解析 Website Feedback 格式的 body。
 *
 * 格式:
 *   ## Feature Request
 *
 *   用户描述内容
 *
 *   ---
 *
 *   **Type:** feature
 *   **Contact:** xxx（可选）
 *   **Source:** Website feedback form
 *   **Submitted:** 2026-02-11T14:25:53.194Z
 *   **IP:** `172.69.22.27`
 *
 * 返回 { description, metadata } 或 null（非 feedback 格式）。
 */
function parseFeedbackBody(body: string): {
  description: string;
  metadata: ParsedMetadata;
} | null {
  // 以独立的 --- 分割线为界
  const sepIdx = body.indexOf("\n---\n");
  if (sepIdx < 0) return null;

  const contentPart = body.slice(0, sepIdx);
  const metaPart = body.slice(sepIdx + 5); // 跳过 "\n---\n"

  // 必须含有 **Source:** Website feedback form 才确认是 feedback 格式
  if (!metaPart.includes("**Source:**")) return null;

  // 提取描述（去掉 ## 标题行）
  const description = contentPart.replace(/^##\s+.*$/gm, "").trim();

  // 从元数据区提取字段
  const extract = (key: string): string | null => {
    const regex = new RegExp(`\\*\\*${key}:\\*\\*\\s*\`?([^\`\\n]+)\`?`, "i");
    const match = metaPart.match(regex);
    return match ? match[1].trim() : null;
  };

  const metadata: ParsedMetadata = {
    type: extract("Type"),
    contact: extract("Contact"),
    source: extract("Source"),
    submitted_at: extract("Submitted"),
    // IP 故意不提取 — 隐藏
  };

  return { description, metadata };
}

/**
 * 解析网站访客回复评论，剥离包装元数据，只返回实际内容。
 *
 * 格式:
 *   > 💬 Website visitor reply
 *
 *   实际回复内容
 *
 *   ---
 *
 *   **Source:** Website reply
 *   **Time:** 2026-02-15T13:20:27.295Z
 *
 * 非此格式的评论原样返回。
 */
function parseVisitorComment(body: string): string {
  if (!body.includes("Website visitor reply")) return body;

  let content = body.replace(
    /^>\s*\uD83D\uDCAC\s*Website visitor reply\s*\n*/u,
    "",
  );

  const sepIdx = content.indexOf("\n---\n");
  if (sepIdx >= 0) {
    content = content.slice(0, sepIdx);
  }

  return content.trim();
}

async function fetchIssueDetail(issueNumber: number): Promise<IssueDetail> {
  // 检查缓存
  const cached = detailCache.get(issueNumber);
  if (cached && Date.now() - cached.timestamp < CACHE_TTL) {
    return cached.data;
  }

  // 并行请求 Issue 详情和评论
  const [issueRes, commentsRes] = await Promise.all([
    fetch(`https://api.github.com/repos/${GITHUB_REPO}/issues/${issueNumber}`, {
      headers: defaultHeaders,
    }),
    fetch(
      `https://api.github.com/repos/${GITHUB_REPO}/issues/${issueNumber}/comments?per_page=100`,
      { headers: defaultHeaders },
    ),
  ]);

  if (!issueRes.ok) {
    if (issueRes.status === 404) {
      throw new Error("NOT_FOUND");
    }
    throw new Error(`GitHub API ${issueRes.status}: ${await issueRes.text()}`);
  }

  if (!commentsRes.ok) {
    throw new Error(
      `GitHub API comments ${commentsRes.status}: ${await commentsRes.text()}`,
    );
  }

  const issue: GitHubIssue = await issueRes.json();
  const comments: GitHubComment[] = await commentsRes.json();

  // 解析 body
  const rawBody = issue.body || "";
  const parsed = parseFeedbackBody(rawBody);

  const result: IssueDetail = {
    issue: {
      number: issue.number,
      title: issue.title
        .replace(
          /^[\u{1F300}-\u{1FAF6}\u{2600}-\u{27BF}\u{FE00}-\u{FE0F}\u{200D}\u{20E3}\u{E0020}-\u{E007F}]+\s*/u,
          "",
        )
        .replace(/^\[Website Feedback\]\s*/i, ""),
      state: issue.state,
      close_reason: deriveCloseReason(issue),
      labels: issue.labels
        .filter(
          (l) =>
            l.name !== "user-feedback" && l.name.toLowerCase() !== "duplicate",
        )
        .map((l) => ({ name: l.name, color: l.color })),
      created_at: issue.created_at,
      updated_at: issue.updated_at,
      comments_count: issue.comments,
      user: {
        login: issue.user.login,
        avatar_url: issue.user.avatar_url,
      },
      description: parsed ? parsed.description : rawBody,
      body_raw: rawBody,
      metadata: parsed ? parsed.metadata : null,
      is_feedback_format: parsed !== null,
      reactions: issue.reactions,
    },
    comments: comments.map((c) => ({
      id: c.id,
      user: {
        login: c.user.login,
        avatar_url: c.user.avatar_url,
      },
      body: parseVisitorComment(c.body),
      created_at: c.created_at,
      updated_at: c.updated_at,
      reactions: c.reactions,
    })),
  };

  detailCache.set(issueNumber, { data: result, timestamp: Date.now() });
  return result;
}

export const GET: APIRoute = async ({ params }) => {
  const numberStr = params.number;

  if (!numberStr) {
    return new Response(JSON.stringify({ error: "Missing issue number" }), {
      status: 400,
      headers: { "Content-Type": "application/json" },
    });
  }

  const issueNumber = parseInt(numberStr, 10);
  if (isNaN(issueNumber) || issueNumber <= 0) {
    return new Response(JSON.stringify({ error: "Invalid issue number" }), {
      status: 400,
      headers: { "Content-Type": "application/json" },
    });
  }

  if (!GITHUB_TOKEN) {
    return new Response(
      JSON.stringify({ error: "Server misconfigured: missing GITHUB_TOKEN" }),
      { status: 500, headers: { "Content-Type": "application/json" } },
    );
  }

  try {
    const detail = await fetchIssueDetail(issueNumber);

    return new Response(JSON.stringify(detail), {
      status: 200,
      headers: {
        "Content-Type": "application/json",
        "Cache-Control": "public, s-maxage=60, stale-while-revalidate=120",
      },
    });
  } catch (err) {
    if (err instanceof Error && err.message === "NOT_FOUND") {
      return new Response(JSON.stringify({ error: "Issue not found" }), {
        status: 404,
        headers: { "Content-Type": "application/json" },
      });
    }

    console.error("Failed to fetch issue detail:", err);
    return new Response(
      JSON.stringify({
        error: "Failed to fetch issue detail",
        detail: String(err),
      }),
      { status: 500, headers: { "Content-Type": "application/json" } },
    );
  }
};
