/**
 * GET /api/plugins/:path — fluxdown-plugin-index 仓库静态资源同源代理。
 *
 * 插件市场页需加载插件索引仓库的 index.json（及可选的 per-version 分片），
 * 直连 raw.githubusercontent.com 在大陆访问极慢。改为经本站服务端中转：
 * 浏览器只连本站（省 DNS/TLS 且大陆可达），服务端出口拉 raw 并回传，
 * 配合 Cache-Control 让 CDN / 浏览器缓存吸收后续请求。
 *
 * 安全：白名单路径（仅 index.json 与 plugins/<id>/<version>.json），
 * 拒绝路径穿越，不做开放代理。
 */

import type { APIRoute } from "astro";

export const prerender = false;

const RAW_BASE = "https://raw.githubusercontent.com/zerx-lab/fluxdown-plugin-index/main";

/** 仅允许 index.json 或 plugins/<pluginId>/<version>.json（pluginId 形如 author@name） */
const ALLOWED_PATH = /^(index\.json|plugins\/[\w.@-]+\/[\w.-]+\.json)$/;

export const GET: APIRoute = async ({ params }) => {
  const path = params.path ?? "";
  if (!ALLOWED_PATH.test(path)) {
    return new Response("Not Found", { status: 404 });
  }

  let upstream: Response;
  try {
    upstream = await fetch(`${RAW_BASE}/${path}`, {
      signal: AbortSignal.timeout(15_000),
    });
  } catch {
    return new Response("Upstream Unreachable", { status: 502 });
  }
  if (!upstream.ok) {
    return new Response("Upstream Error", { status: upstream.status === 404 ? 404 : 502 });
  }

  // index.json 短缓存（新插件上架 5 分钟内可见）；per-version 分片内容随版本号
  // 变化、路径稳定（append-only），长缓存 + SWR 让 CDN 边缘直接命中。
  const cacheControl =
    path === "index.json"
      ? "public, max-age=300, stale-while-revalidate=3600"
      : "public, max-age=86400, stale-while-revalidate=604800";

  return new Response(upstream.body, {
    status: 200,
    headers: {
      "Content-Type": "application/json; charset=utf-8",
      "Cache-Control": cacheControl,
      "X-Proxy-Source": "raw.githubusercontent.com",
    },
  });
};
