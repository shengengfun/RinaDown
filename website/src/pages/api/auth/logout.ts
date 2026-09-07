import type { APIRoute } from "astro";
import { clearSession, safeReturnTo } from "@/lib/github-oauth";

export const prerender = false;

/** GET /api/auth/logout?returnTo=/pricing — 清会话并跳回 */
export const GET: APIRoute = async ({ url, cookies, redirect }) => {
  clearSession(cookies);
  return redirect(safeReturnTo(url.searchParams.get("returnTo")), 302);
};
