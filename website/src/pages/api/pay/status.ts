import type { APIRoute } from "astro";
import { isPaid } from "@/lib/pay";

export const prerender = false;

const JSON_HEADERS = { "Content-Type": "application/json", "Cache-Control": "no-store" };

/* Frontend polls this to learn when an order's async callback arrived. */
export const GET: APIRoute = async ({ url }) => {
  const outTradeNo = url.searchParams.get("outTradeNo");
  if (!outTradeNo) {
    return new Response(JSON.stringify({ error: "outTradeNo required" }), {
      status: 400,
      headers: JSON_HEADERS,
    });
  }
  return new Response(JSON.stringify({ paid: isPaid(outTradeNo) }), {
    status: 200,
    headers: JSON_HEADERS,
  });
};
