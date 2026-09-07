/**
 * POST /api/cloud/order-lookup — 网页购买第一步：账号身份确认（同源代理）。
 * 转发 FluxCloud `POST /api/v1/orders/web/lookup`（公开端点，per-IP 限频）。
 */

import type { APIRoute } from "astro";
import { cloudBase, forwardJson } from "@/lib/cloud-proxy";

export const prerender = false;

export const POST: APIRoute = async ({ request, clientAddress }) => {
  const base = cloudBase();
  if (!base) return new Response("Not Configured", { status: 502 });
  const body = await request.text();
  return forwardJson(`${base}/api/v1/orders/web/lookup`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "X-Forwarded-For": clientAddress,
    },
    body,
  });
};
