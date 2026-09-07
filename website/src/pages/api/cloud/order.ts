/**
 * POST /api/cloud/order — 网页购买下单（同源代理）。
 * GET  /api/cloud/order?orderNo=&account= — 订单状态轮询。
 *
 * 转发 FluxCloud `POST /api/v1/orders/web` 与 `GET /api/v1/orders/web/{orderNo}`，
 * 透传状态码与错误体；IP 透传说明见 order-lookup.ts。
 */

import type { APIRoute } from "astro";
import { cloudBase, forwardJson } from "@/lib/cloud-proxy";

export const prerender = false;

export const POST: APIRoute = async ({ request, clientAddress }) => {
  const base = cloudBase();
  if (!base) return new Response("Not Configured", { status: 502 });
  const body = await request.text();
  return forwardJson(`${base}/api/v1/orders/web`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "X-Forwarded-For": clientAddress,
    },
    body,
  });
};

export const GET: APIRoute = async ({ url, clientAddress }) => {
  const base = cloudBase();
  if (!base) return new Response("Not Configured", { status: 502 });
  const orderNo = url.searchParams.get("orderNo") ?? "";
  const account = url.searchParams.get("account") ?? "";
  if (!orderNo || !account) {
    return new Response(JSON.stringify({ code: "validation_error" }), {
      status: 422,
      headers: { "Content-Type": "application/json; charset=utf-8" },
    });
  }
  const target = `${base}/api/v1/orders/web/${encodeURIComponent(orderNo)}?account=${encodeURIComponent(account)}`;
  return forwardJson(target, {
    method: "GET",
    headers: { "X-Forwarded-For": clientAddress },
  });
};
