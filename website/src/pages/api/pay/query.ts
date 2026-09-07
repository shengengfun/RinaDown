import type { APIRoute } from "astro";
import { queryOrder, payConfigError } from "@/lib/pay";

export const prerender = false;

const JSON_HEADERS = { "Content-Type": "application/json", "Cache-Control": "no-store" };

export const GET: APIRoute = async ({ url }) => {
  const cfgErr = payConfigError();
  if (cfgErr) {
    console.error("[pay/query] config error:", cfgErr);
    return new Response(JSON.stringify({ error: "payment unavailable" }), {
      status: 503,
      headers: JSON_HEADERS,
    });
  }

  const outTradeNo = url.searchParams.get("outTradeNo");
  if (!outTradeNo) {
    return new Response(JSON.stringify({ error: "outTradeNo required" }), {
      status: 400,
      headers: JSON_HEADERS,
    });
  }

  try {
    const order = await queryOrder(outTradeNo);
    return new Response(
      JSON.stringify({ status: order.status, amount: order.amount }),
      { status: 200, headers: JSON_HEADERS },
    );
  } catch (err) {
    console.error("[pay/query] gateway error:", err);
    return new Response(JSON.stringify({ error: "failed to query order" }), {
      status: 502,
      headers: JSON_HEADERS,
    });
  }
};
