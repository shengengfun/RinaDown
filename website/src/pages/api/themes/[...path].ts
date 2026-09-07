/**
 * GET /api/themes/:path — fluxdown-themes 仓库静态资源同源代理。
 *
 * 主题市场页原先直连 raw.githubusercontent.com 加载 index.json / 截图 /
 * 主题 JSON，大陆访问极慢（LCP 实测 5s+）。改为经本站服务端中转：
 * 浏览器只连本站（省 DNS/TLS 且大陆可达），服务端出口拉 raw 并回传，
 * 配合 Cache-Control 让 CDN / 浏览器缓存吸收后续请求。
 *
 * 安全：白名单路径（仅 index.json 与 themes/ 下的 json/png/jpg/webp），
 * 拒绝路径穿越，不做开放代理。
 */

import type { APIRoute } from "astro";

export const prerender = false;

const RAW_BASE = "https://raw.githubusercontent.com/zerx-lab/fluxdown-themes/main";

/** 仅允许 index.json 或 themes/<id>/<file>.(json|png|jpg|jpeg|webp) */
const ALLOWED_PATH = /^(index\.json|themes\/[\w.-]+\/[\w.-]+\.(?:json|png|jpe?g|webp))$/;

const CONTENT_TYPES: Record<string, string> = {
  json: "application/json; charset=utf-8",
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  webp: "image/webp",
};

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

  const ext = path.split(".").pop() ?? "";
  // index.json 短缓存（新主题上架 5 分钟内可见）；截图/主题文件内容随版本号
  // 变化、路径稳定，长缓存 + SWR 让 CDN 边缘直接命中。
  const cacheControl =
    path === "index.json"
      ? "public, max-age=300, stale-while-revalidate=3600"
      : "public, max-age=86400, stale-while-revalidate=604800";

  return new Response(upstream.body, {
    status: 200,
    headers: {
      "Content-Type": CONTENT_TYPES[ext] ?? "application/octet-stream",
      "Cache-Control": cacheControl,
      "X-Proxy-Source": "raw.githubusercontent.com",
    },
  });
};
