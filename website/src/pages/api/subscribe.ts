/**
 * POST /api/subscribe
 *
 * 平台可用性邮箱订阅。使用 GitHub Issue 单条聚合模式：
 * - 查找带 `subscription` 标签的 Issue（没有则创建）
 * - 检查已有评论是否存在相同邮箱（去重）
 * - 新增一条结构化评论
 *
 * 请求体:
 * {
 *   email: string,
 *   platform: "web" | "macos" | "linux" | "mobile",
 * }
 *
 * 语言从请求头 Accept-Language 自动检测。
 */

import type { APIRoute } from "astro";
import { GITHUB_TOKEN, GITHUB_REPO } from "astro:env/server";

export const prerender = false;

const ISSUE_TITLE = "[Subscription] Platform Availability Notifications";
const ISSUE_LABEL = "subscription";

// ---------- Rate Limit ----------

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

// ---------- GitHub helpers ----------

function ghHeaders() {
  return {
    Authorization: `Bearer ${GITHUB_TOKEN}`,
    Accept: "application/vnd.github+json",
    "X-GitHub-Api-Version": "2022-11-28",
    "Content-Type": "application/json",
  };
}

/** 查找或创建聚合 Issue，返回 issue number */
async function findOrCreateIssue(): Promise<number> {
  // 搜索已有 Issue
  const searchRes = await fetch(
    `https://api.github.com/repos/${GITHUB_REPO}/issues?labels=${ISSUE_LABEL}&state=open&per_page=1`,
    { headers: ghHeaders() },
  );

  if (searchRes.ok) {
    const issues = await searchRes.json();
    if (Array.isArray(issues) && issues.length > 0) {
      return issues[0].number;
    }
  }

  // 不存在，创建新 Issue
  const createRes = await fetch(
    `https://api.github.com/repos/${GITHUB_REPO}/issues`,
    {
      method: "POST",
      headers: ghHeaders(),
      body: JSON.stringify({
        title: ISSUE_TITLE,
        body: [
          "## Platform Availability Subscription Registry",
          "",
          "This issue collects email subscriptions from users who want to be notified when FluxDown becomes available on their platform.",
          "",
          "**Do not close this issue.** Each comment below represents one subscriber.",
          "",
          "### Data format",
          "```json",
          '{ "email": "...", "platform": "web|macos|linux|mobile", "locale": "zh|en", "date": "..." }',
          "```",
        ].join("\n"),
        labels: [ISSUE_LABEL],
      }),
    },
  );

  if (!createRes.ok) {
    const text = await createRes.text();
    throw new Error(
      `Failed to create subscription issue: ${createRes.status} ${text}`,
    );
  }

  const created = await createRes.json();
  return created.number;
}

/** 检查邮箱是否已订阅（遍历 Issue 评论） */
async function isAlreadySubscribed(
  issueNumber: number,
  email: string,
): Promise<boolean> {
  let page = 1;
  const lowerEmail = email.toLowerCase();

  while (true) {
    const res = await fetch(
      `https://api.github.com/repos/${GITHUB_REPO}/issues/${issueNumber}/comments?per_page=100&page=${page}`,
      { headers: ghHeaders() },
    );

    if (!res.ok) break;

    const comments = await res.json();
    if (!Array.isArray(comments) || comments.length === 0) break;

    for (const comment of comments) {
      // 评论 body 中包含 JSON，尝试从中提取 email
      const body: string = comment.body || "";
      if (body.toLowerCase().includes(`"email": "${lowerEmail}"`)) {
        return true;
      }
      // 也匹配无空格的格式
      if (body.toLowerCase().includes(`"email":"${lowerEmail}"`)) {
        return true;
      }
    }

    if (comments.length < 100) break;
    page++;
  }

  return false;
}

// ---------- Locale detection ----------

function detectLocale(acceptLang: string | null): string {
  if (!acceptLang) return "en";
  if (acceptLang.startsWith("zh")) return "zh";
  return "en";
}

// ---------- Email validation ----------

const EMAIL_RE = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

// ---------- Handler ----------

export const POST: APIRoute = async ({ request, clientAddress }) => {
  const ip = clientAddress || "unknown";

  if (isRateLimited(ip)) {
    return new Response(
      JSON.stringify({ error: "Too many requests. Please try again later." }),
      { status: 429, headers: { "Content-Type": "application/json" } },
    );
  }

  let body: { email?: string; platform?: string };
  try {
    body = await request.json();
  } catch {
    return new Response(JSON.stringify({ error: "Invalid JSON body" }), {
      status: 400,
      headers: { "Content-Type": "application/json" },
    });
  }

  const { email, platform } = body;

  // Validate
  if (!email || !platform) {
    return new Response(
      JSON.stringify({ error: "Missing required fields: email, platform" }),
      { status: 400, headers: { "Content-Type": "application/json" } },
    );
  }

  if (!EMAIL_RE.test(email)) {
    return new Response(JSON.stringify({ error: "Invalid email address" }), {
      status: 400,
      headers: { "Content-Type": "application/json" },
    });
  }

  if (!["web", "macos", "linux", "mobile"].includes(platform)) {
    return new Response(
      JSON.stringify({
        error: "Invalid platform. Must be: web, macos, linux or mobile",
      }),
      { status: 400, headers: { "Content-Type": "application/json" } },
    );
  }

  const locale = detectLocale(request.headers.get("accept-language"));

  try {
    // 1. 查找/创建聚合 Issue
    const issueNumber = await findOrCreateIssue();

    // 2. 去重检查
    const exists = await isAlreadySubscribed(issueNumber, email);
    if (exists) {
      return new Response(
        JSON.stringify({ success: true, message: "already_subscribed" }),
        { status: 200, headers: { "Content-Type": "application/json" } },
      );
    }

    // 3. 添加评论
    const data = {
      email: email.toLowerCase().trim(),
      platform,
      locale,
      date: new Date().toISOString(),
    };

    const commentBody = [
      `### New Subscription`,
      "",
      "```json",
      JSON.stringify(data, null, 2),
      "```",
      "",
      `- **Email:** ${data.email}`,
      `- **Platform:** ${data.platform}`,
      `- **Language:** ${data.locale}`,
      `- **Date:** ${data.date}`,
    ].join("\n");

    const commentRes = await fetch(
      `https://api.github.com/repos/${GITHUB_REPO}/issues/${issueNumber}/comments`,
      {
        method: "POST",
        headers: ghHeaders(),
        body: JSON.stringify({ body: commentBody }),
      },
    );

    if (!commentRes.ok) {
      const text = await commentRes.text();
      console.error(`Failed to add comment: ${commentRes.status}`, text);
      return new Response(JSON.stringify({ error: "Failed to subscribe" }), {
        status: 502,
        headers: { "Content-Type": "application/json" },
      });
    }

    return new Response(
      JSON.stringify({ success: true, message: "subscribed" }),
      { status: 201, headers: { "Content-Type": "application/json" } },
    );
  } catch (err) {
    console.error("Subscribe error:", err);
    return new Response(JSON.stringify({ error: "Internal server error" }), {
      status: 500,
      headers: { "Content-Type": "application/json" },
    });
  }
};
