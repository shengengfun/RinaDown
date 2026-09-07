import { createHmac, randomBytes, timingSafeEqual } from "crypto";

/* ============================================================
   Payment gateway client (zerx pay, ConnectRPC).
   - Server-side only: PAY_APP_SECRET must never reach the browser.
   - Signature contract (verified against ops gateway source):
     snake_case keys, drop empties + "sign", ASCII-sorted,
     joined "k=v&k=v", HMAC-SHA256(secret), lowercase hex.
   ============================================================ */

import { PAY_GATEWAY_URL, PAY_APP_ID, PAY_APP_SECRET } from "astro:env/server";

const GATEWAY_URL = PAY_GATEWAY_URL ?? "";
const APP_ID = PAY_APP_ID ?? "";
const APP_SECRET = PAY_APP_SECRET ?? "";

const CREATE_PATH = "/zerx.v1.PayGatewayService/CreateOrder";
const QUERY_PATH = "/zerx.v1.PayGatewayService/QueryOrder";

export interface PayConfigError {
  ok: false;
  error: string;
}

export interface CreateOrderResult {
  outTradeNo: string;
  bizOrderNo: string;
  codeUrl: string;
  status: string;
  amount: string;
}

export interface QueryOrderResult {
  outTradeNo: string;
  bizOrderNo: string;
  status: string; // pending/paid/closed/refunded/failed
  amount: string;
}

/** Returns null when env is fully configured, otherwise an error string. */
export function payConfigError(): string | null {
  if (!GATEWAY_URL) return "PAY_GATEWAY_URL not set";
  if (!APP_ID) return "PAY_APP_ID not set";
  if (!APP_SECRET) return "PAY_APP_SECRET not set";
  return null;
}

/** Build the HMAC-SHA256 signature over snake_case params (gateway contract). */
function sign(params: Record<string, string>): string {
  const msg = Object.keys(params)
    .filter((k) => k !== "sign" && params[k] !== "" && params[k] != null)
    .sort()
    .map((k) => `${k}=${params[k]}`)
    .join("&");
  return createHmac("sha256", APP_SECRET).update(msg).digest("hex");
}

function nonce(): string {
  return randomBytes(8).toString("hex");
}

async function rpc<T>(path: string, body: Record<string, unknown>): Promise<T> {
  const res = await fetch(GATEWAY_URL + path, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "Connect-Protocol-Version": "1",
    },
    body: JSON.stringify(body),
  });
  const text = await res.text();
  let json: unknown;
  try {
    json = text ? JSON.parse(text) : {};
  } catch {
    throw new Error(`gateway non-JSON response (${res.status}): ${text.slice(0, 200)}`);
  }
  if (!res.ok) {
    const err = json as { code?: string; message?: string };
    throw new Error(err.message || err.code || `gateway error ${res.status}`);
  }
  return json as T;
}

/**
 * Create a payment order. `amountCents` is integer cents (>=1).
 * `bizOrderNo` must be unique per attempt.
 */
export async function createOrder(opts: {
  bizOrderNo: string;
  subject: string;
  amountCents: number;
  attach?: string;
}): Promise<CreateOrderResult> {
  const timestamp = Math.floor(Date.now() / 1000);
  const n = nonce();
  const signParams: Record<string, string> = {
    amount: String(opts.amountCents),
    app_id: APP_ID,
    biz_order_no: opts.bizOrderNo,
    nonce: n,
    subject: opts.subject,
    timestamp: String(timestamp),
  };
  if (opts.attach) signParams.attach = opts.attach;

  const body: Record<string, unknown> = {
    appId: APP_ID,
    bizOrderNo: opts.bizOrderNo,
    subject: opts.subject,
    amount: opts.amountCents,
    timestamp,
    nonce: n,
    sign: sign(signParams),
  };
  if (opts.attach) body.attach = opts.attach;

  return rpc<CreateOrderResult>(CREATE_PATH, body);
}

/** Query an order's status by gateway out_trade_no. */
export async function queryOrder(outTradeNo: string): Promise<QueryOrderResult> {
  const timestamp = Math.floor(Date.now() / 1000);
  const n = nonce();
  const signParams: Record<string, string> = {
    app_id: APP_ID,
    nonce: n,
    out_trade_no: outTradeNo,
    timestamp: String(timestamp),
  };
  const body = {
    appId: APP_ID,
    outTradeNo,
    timestamp,
    nonce: n,
    sign: sign(signParams),
  };
  return rpc<QueryOrderResult>(QUERY_PATH, body);
}

/** Generate a unique biz order number for a sponsor payment. */
export function genBizOrderNo(): string {
  return `flux_${Date.now()}_${randomBytes(4).toString("hex")}`;
}

/**
 * Verify an inbound async-callback signature (gateway -> us).
 * `params` is the flat form payload (string values); the contract matches the
 * outbound order signature: snake_case keys, drop empties + "sign", ASCII-sorted,
 * "k=v&k=v", HMAC-SHA256(app_secret), lowercase hex. Constant-time compare.
 */
export function verifyNotifySign(params: Record<string, string>): boolean {
  const provided = params.sign ?? "";
  if (!provided || !APP_SECRET) return false;
  const expected = sign(params);
  const a = Buffer.from(expected.toLowerCase(), "utf8");
  const b = Buffer.from(provided.toLowerCase(), "utf8");
  if (a.length !== b.length) return false;
  return timingSafeEqual(a, b);
}

// ── Paid-order store (ephemeral, for frontend success signal) ──────
// The async callback marks an out_trade_no paid; the browser polls
// /api/pay/status to flip the UI. Intentionally in-memory + TTL:
// no persistence required for a thank-you screen.

const PAID_TTL = 30 * 60 * 1000; // 30 minutes

interface PaidOrder {
  expiry: number;
  amountCents: number; // 0 when the callback carried no usable amount
  wallPosted: boolean; // sponsor-wall comment already created
}

const paidOrders = new Map<string, PaidOrder>(); // outTradeNo -> record

function sweepPaid(now: number): void {
  for (const [k, entry] of paidOrders) {
    if (entry.expiry <= now) paidOrders.delete(k);
  }
}

function livePaid(outTradeNo: string): PaidOrder | null {
  const entry = paidOrders.get(outTradeNo);
  if (!entry) return null;
  if (entry.expiry <= Date.now()) {
    paidOrders.delete(outTradeNo);
    return null;
  }
  return entry;
}

/** Mark an order paid (called from the verified notify handler). */
export function markPaid(outTradeNo: string, amountCents = 0): void {
  const now = Date.now();
  sweepPaid(now);
  const prev = paidOrders.get(outTradeNo);
  paidOrders.set(outTradeNo, {
    expiry: now + PAID_TTL,
    amountCents: amountCents > 0 ? amountCents : (prev?.amountCents ?? 0),
    wallPosted: prev?.wallPosted ?? false,
  });
}

/** Returns true if the order was marked paid and is still within TTL. */
export function isPaid(outTradeNo: string): boolean {
  return livePaid(outTradeNo) !== null;
}

/** Paid amount in cents, or null when unknown / expired / unpaid. */
export function paidAmountCents(outTradeNo: string): number | null {
  const entry = livePaid(outTradeNo);
  return entry && entry.amountCents > 0 ? entry.amountCents : null;
}

/**
 * One-shot claim so a paid order posts to the sponsor wall at most once.
 * Returns false when the order is not paid or was already claimed.
 */
export function claimWallPost(outTradeNo: string): boolean {
  const entry = livePaid(outTradeNo);
  if (!entry || entry.wallPosted) return false;
  entry.wallPosted = true;
  return true;
}

/** Undo a wall-post claim after a failed GitHub call so retry works. */
export function releaseWallPost(outTradeNo: string): void {
  const entry = livePaid(outTradeNo);
  if (entry) entry.wallPosted = false;
}
