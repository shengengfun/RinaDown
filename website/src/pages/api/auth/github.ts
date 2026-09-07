import type { APIRoute } from "astro";
import {
  STATE_COOKIE,
  oauthCallbackUrl,
  oauthConfigured,
  oauthClientId,
  randomState,
  safeReturnTo,
} from "@/lib/github-oauth";

export const prerender = false;

/** GET /api/auth/github?returnTo=/pricing — 跳转 GitHub 授权页（无 scope，仅公开身份） */
export const GET: APIRoute = async ({ url, site, cookies, redirect }) => {
  if (!oauthConfigured()) {
    return new Response("GitHub OAuth not configured", { status: 500 });
  }

  const state = randomState();
  const returnTo = safeReturnTo(url.searchParams.get("returnTo"));

  cookies.set(STATE_COOKIE, `${state}|${returnTo}`, {
    httpOnly: true,
    sameSite: "lax",
    path: "/",
    maxAge: 600,
    secure: import.meta.env.PROD,
  });

  const params = new URLSearchParams({
    client_id: oauthClientId(),
    redirect_uri: oauthCallbackUrl(url, site),
    state,
  });

  return redirect(`https://github.com/login/oauth/authorize?${params}`, 302);
};
