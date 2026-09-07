import type { APIRoute } from "astro";
import { verifyNotifySign, markPaid, payConfigError } from "@/lib/pay";

export const prerender = false;

/* ============================================================
   Async payment callback (gateway -> us).
   - POST application/x-www-form-urlencoded
   - Fields: amount, attach, biz_order_no, nonce, out_trade_no,
     paid_at, provider_trade_no, status(=paid), timestamp, sign
   - MUST verify HMAC sign before trusting anything.
   - MUST return 2xx with body containing "success", else the
     gateway retries.
   Configure this URL on the pay app: https://<host>/api/pay/notify
   ============================================================ */

// Plain text "success" ack — gateway treats 2xx + "success" as delivered.
function ack(): Response {
  return new Response("success", {
    status: 200,
    headers: { "Content-Type": "text/plain" },
  });
}

function fail(status: number, msg: string): Response {
  return new Response(msg, {
    status,
    headers: { "Content-Type": "text/plain" },
  });
}

export const POST: APIRoute = async ({ request }) => {
  // Without a secret we cannot verify — reject so the gateway retries later.
  if (payConfigError()) {
    return fail(503, "unavailable");
  }

  let params: Record<string, string>;
  try {
    const form = await request.formData();
    params = {};
    for (const [k, v] of form.entries()) {
      params[k] = typeof v === "string" ? v : "";
    }
  } catch {
    return fail(400, "invalid form");
  }

  if (!verifyNotifySign(params)) {
    // Opaque rejection; never reveal expected signature.
    return fail(401, "invalid sign");
  }

  if (params.status === "paid" && params.out_trade_no) {
    // `amount` is integer cents per the gateway contract.
    const amountCents = Math.round(Number(params.amount));
    markPaid(
      params.out_trade_no,
      Number.isFinite(amountCents) && amountCents > 0 ? amountCents : 0,
    );
  }

  // Acknowledge any verified callback so the gateway stops retrying.
  return ack();
};
