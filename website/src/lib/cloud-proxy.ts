/**
 * FluxCloud 同源代理共用小件：base 解析 + JSON 透传转发。
 *
 * 透传上游状态码与 JSON 错误体（前端按 code 映射文案）。转发时附带访客真实 IP
 * （X-Forwarded-For），FluxCloud 侧需将本站出口 IP 加入 FLUXCLOUD_TRUSTED_PROXIES
 * 才能按访客限频，否则全站共享一个限频桶。
 */

import { FLUXCLOUD_API_BASE } from "astro:env/server";

export function cloudBase(): string {
  return (FLUXCLOUD_API_BASE ?? "").replace(/\/+$/, "");
}

export async function forwardJson(
  upstreamUrl: string,
  init: RequestInit,
): Promise<Response> {
  let upstream: Response;
  try {
    upstream = await fetch(upstreamUrl, {
      ...init,
      signal: AbortSignal.timeout(20_000),
    });
  } catch {
    return new Response(JSON.stringify({ code: "upstream_unreachable" }), {
      status: 502,
      headers: { "Content-Type": "application/json; charset=utf-8" },
    });
  }
  return new Response(upstream.body, {
    status: upstream.status,
    headers: {
      "Content-Type": "application/json; charset=utf-8",
      "Cache-Control": "no-store",
    },
  });
}
