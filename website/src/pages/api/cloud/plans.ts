/**
 * GET /api/cloud/plans — FluxCloud 公开套餐目录同源代理。
 *
 * 定价页需要展示 FluxCloud 的动态套餐列表（含限量阶梯活动的当前档位价与余量，
 * 以及下架但仍勾选官网展示的套餐；后者 purchasable=false，前端隐藏购买入口）。
 * 浏览器只连本站，服务端出口转发 FluxCloud `GET /api/v1/plans/catalog`（无鉴权公开端点），
 * 短缓存吸收流量：活动余量类数据允许 60 秒陈旧。
 *
 * 上游不可达 / 出错时返回 502，前端降级为静态占位卡片。
 */

import type { APIRoute } from "astro";
import { FLUXCLOUD_API_BASE } from "astro:env/server";

export const prerender = false;

export const GET: APIRoute = async () => {
  const base = (FLUXCLOUD_API_BASE ?? "").replace(/\/+$/, "");
  if (!base) {
    return new Response("Not Configured", { status: 502 });
  }

  let upstream: Response;
  try {
    upstream = await fetch(`${base}/api/v1/plans/catalog`, {
      signal: AbortSignal.timeout(10_000),
    });
  } catch {
    return new Response("Upstream Unreachable", { status: 502 });
  }
  if (!upstream.ok) {
    return new Response("Upstream Error", { status: 502 });
  }

  return new Response(upstream.body, {
    status: 200,
    headers: {
      "Content-Type": "application/json; charset=utf-8",
      "Cache-Control": "public, max-age=60, stale-while-revalidate=300",
      "X-Proxy-Source": "fluxcloud",
    },
  });
};
