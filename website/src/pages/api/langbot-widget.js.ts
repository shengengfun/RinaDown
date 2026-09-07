/**
 * GET /api/langbot-widget.js
 *
 * LangBot 嵌入脚本的同域代理。上游 widget.js 由 LangBot 后端在响应里注入 CONFIG，
 * 其中 `baseUrl` 取自该实例的「公开访问地址」配置——当前被写成了
 * `http://127.0.0.1:5300`，导致浏览器端所有 API / WebSocket / logo 请求打到访客本机
 * （控制台 ERR_CONNECTION_REFUSED）。
 *
 * 本路由拉取上游脚本并把内网地址整体重写为 UPSTREAM_ORIGIN，同时进程内缓存 1 小时。
 * 一旦 LangBot 侧把公开地址配对，重写自动变成 no-op，无需回滚本文件。
 *
 * 为什么代理而不是把 js 复制进 public/：复制会冻结版本，上游修 bug / 加功能都拿不到，
 * 且 CONFIG 里的 botUuid、turnstileSiteKey 变更需要人工同步。代理只固定「地址」这一项。
 */

import type { APIRoute } from "astro";
import { getCached, setCached } from "../../lib/api-cache";

export const prerender = false;

const BOT_UUID = "52070191-7c5a-4b81-afb2-28870589375e";
const UPSTREAM_ORIGIN = "https://bot.zerx.dev";
const UPSTREAM_URL = `${UPSTREAM_ORIGIN}/api/v1/embed/${BOT_UUID}/widget.js`;

/** 上游注入的内网地址（含无端口写法），全部重写为公网 origin */
const BAD_ORIGIN = /https?:\/\/(?:127\.0\.0\.1|localhost|0\.0\.0\.0)(?::\d+)?/g;

const CACHE_KEY = "langbot-widget-v3";
const CACHE_TTL = 3_600_000; // 1 小时

// 为右下角社区入口留出空间，避免机器人气泡遮挡 Telegram / QQ 按钮。
function rewriteWidget(script: string): string {
  const rewritten = script.replace(BAD_ORIGIN, UPSTREAM_ORIGIN);
  return rewritten.replace(
    /\.lb-bubble\s*\{/g,
    ".lb-bubble { display: none !important;",
  );
}

export const GET: APIRoute = async () => {
  const cached = getCached<string>(CACHE_KEY, CACHE_TTL);
  if (cached !== null) return respond(cached);

  let upstream: Response;
  try {
    upstream = await fetch(UPSTREAM_URL, { signal: AbortSignal.timeout(10_000) });
  } catch {
    return new Response("/* langbot widget upstream unreachable */", {
      status: 502,
      headers: { "content-type": "application/javascript; charset=utf-8" },
    });
  }

  if (!upstream.ok) {
    return new Response(`/* langbot widget upstream ${upstream.status} */`, {
      status: 502,
      headers: { "content-type": "application/javascript; charset=utf-8" },
    });
  }

  const script = rewriteWidget(await upstream.text());
  setCached(CACHE_KEY, script);
  return respond(script);
};

function respond(script: string): Response {
  return new Response(script, {
    status: 200,
    headers: {
      "content-type": "application/javascript; charset=utf-8",
      "cache-control": "public, max-age=3600",
    },
  });
}
