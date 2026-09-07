/**
 * POST /api/sponsor/wall
 *
 * 支付成功后把赞助者的名称/留言登记到 GitHub 赞助名录 issue
 * （SPONSOR_WALL_REPO#SPONSOR_WALL_ISSUE，公开仓库置顶 issue）。
 *
 * 请求体: { outTradeNo: string, name?: string, message?: string }
 *
 * 防滥用:
 * - outTradeNo 必须在服务端被支付回调标记为已支付（内存 TTL 店内校验）
 * - 每笔订单最多登记一次（one-shot claim，GitHub 失败时回滚可重试）
 * - 基于 IP 的速率限制 + 内容长度限制 + @ 提及中和
 */

import type { APIRoute } from "astro";
import {
  GITHUB_TOKEN,
  SPONSOR_WALL_REPO,
  SPONSOR_WALL_ISSUE,
} from "astro:env/server";
import {
  isPaid,
  paidAmountCents,
  claimWallPost,
  releaseWallPost,
} from "@/lib/pay";

export const prerender = false;

const JSON_HEADERS = { "Content-Type": "application/json" };

const NAME_MAX = 30;
const MESSAGE_MAX = 300;

// ---------- Rate Limit（内存，Serverless 冷启动后重置） ----------

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
  for (const [k, v] of rateLimitMap) {
    if (now > v.resetAt) rateLimitMap.delete(k);
  }
}, 5 * 60_000);

// ---------- Sanitizers ----------

/** Collapse whitespace, drop newlines, neutralize @mentions, cap length. */
function cleanName(raw: unknown): string {
  if (typeof raw !== "string") return "";
  return raw
    .replace(/\s+/g, " ")
    .replace(/@/g, "@\u200b")
    .trim()
    .slice(0, NAME_MAX);
}

/** Keep line breaks but neutralize @mentions and cap length. */
function cleanMessage(raw: unknown): string {
  if (typeof raw !== "string") return "";
  return raw
    .replace(/\r\n?/g, "\n")
    .replace(/@/g, "@\u200b")
    .trim()
    .slice(0, MESSAGE_MAX);
}

function fmtAmount(cents: number): string {
  const yuan = cents / 100;
  return Number.isInteger(yuan) ? `¥${yuan}` : `¥${yuan.toFixed(2)}`;
}

function err(status: number, message: string): Response {
  return new Response(JSON.stringify({ error: message }), {
    status,
    headers: JSON_HEADERS,
  });
}

// ---------- Handler ----------

export const POST: APIRoute = async ({ request, clientAddress }) => {
  const ip = clientAddress || "unknown";
  if (isRateLimited(ip)) {
    return err(429, "Too many requests");
  }
  if (!GITHUB_TOKEN) {
    return err(500, "Server misconfigured");
  }

  let body: { outTradeNo?: string; name?: string; message?: string };
  try {
    body = await request.json();
  } catch {
    return err(400, "Invalid JSON body");
  }

  const outTradeNo =
    typeof body.outTradeNo === "string" ? body.outTradeNo.trim() : "";
  if (!outTradeNo || outTradeNo.length > 128) {
    return err(400, "outTradeNo required");
  }

  // 名称/留言均可留空——匿名赞助也会登记（heading 用兜底文案）。
  const name = cleanName(body.name);
  const message = cleanMessage(body.message);

  // 服务端支付校验：未支付订单不得上墙。
  if (!isPaid(outTradeNo)) {
    return err(403, "order not paid");
  }
  // 每笔订单只登记一次。
  if (!claimWallPost(outTradeNo)) {
    return err(409, "already posted");
  }

  const amount = paidAmountCents(outTradeNo);
  const date = new Intl.DateTimeFormat("en-CA", {
    timeZone: "Asia/Shanghai",
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  }).format(new Date());

  const lines: (string | null)[] = [
    `### 💖 ${name || "匿名赞助者 / Anonymous"}`,
    "",
    message
      ? message
          .split("\n")
          .map((l) => `> ${l}`)
          .join("\n")
      : null,
    message ? "" : null,
    [amount !== null ? `\`${fmtAmount(amount)}\`` : null, date]
      .filter(Boolean)
      .join(" · "),
  ];
  const commentBody = lines.filter((l) => l !== null).join("\n");

  try {
    const res = await fetch(
      `https://api.github.com/repos/${SPONSOR_WALL_REPO}/issues/${SPONSOR_WALL_ISSUE}/comments`,
      {
        method: "POST",
        headers: {
          Authorization: `Bearer ${GITHUB_TOKEN}`,
          Accept: "application/vnd.github+json",
          "X-GitHub-Api-Version": "2022-11-28",
          "Content-Type": "application/json",
        },
        body: JSON.stringify({ body: commentBody }),
      },
    );

    if (!res.ok) {
      const text = await res.text();
      console.error(`[sponsor/wall] GitHub API error: ${res.status}`, text);
      releaseWallPost(outTradeNo);
      return err(502, "Failed to post to sponsor wall");
    }

    const comment = (await res.json()) as { html_url?: string };
    return new Response(
      JSON.stringify({ success: true, url: comment.html_url ?? null }),
      { status: 201, headers: JSON_HEADERS },
    );
  } catch (e) {
    console.error("[sponsor/wall] request failed:", e);
    releaseWallPost(outTradeNo);
    return err(500, "Internal server error");
  }
};
