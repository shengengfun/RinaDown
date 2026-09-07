import type { APIRoute } from "astro";
import { createOrder, genBizOrderNo, payConfigError } from "@/lib/pay";

export const prerender = false;

const JSON_HEADERS = { "Content-Type": "application/json" };

// Reasonable bounds: ¥1 .. ¥10000 (in cents).
const MIN_CENTS = 100;
const MAX_CENTS = 1_000_000;

export const POST: APIRoute = async ({ request }) => {
  const cfgErr = payConfigError();
  if (cfgErr) {
    console.error("[pay/create] config error:", cfgErr);
    return new Response(JSON.stringify({ error: "payment unavailable" }), {
      status: 503,
      headers: JSON_HEADERS,
    });
  }

  let amountCents: number;
  let subject: string;
  try {
    const body = await request.json();
    amountCents = Math.round(Number(body?.amountCents));
    subject =
      typeof body?.subject === "string" && body.subject.trim()
        ? body.subject.trim().slice(0, 120)
        : "Support FluxDown";
  } catch {
    return new Response(JSON.stringify({ error: "invalid body" }), {
      status: 400,
      headers: JSON_HEADERS,
    });
  }

  if (!Number.isFinite(amountCents) || amountCents < MIN_CENTS || amountCents > MAX_CENTS) {
    return new Response(JSON.stringify({ error: "amount out of range" }), {
      status: 400,
      headers: JSON_HEADERS,
    });
  }

  try {
    const order = await createOrder({
      bizOrderNo: genBizOrderNo(),
      subject,
      amountCents,
    });
    return new Response(
      JSON.stringify({
        outTradeNo: order.outTradeNo,
        codeUrl: order.codeUrl,
        status: order.status,
        amount: order.amount,
      }),
      { status: 200, headers: JSON_HEADERS },
    );
  } catch (err) {
    console.error("[pay/create] gateway error:", err);
    return new Response(JSON.stringify({ error: "failed to create order" }), {
      status: 502,
      headers: JSON_HEADERS,
    });
  }
};
