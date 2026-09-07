import type { APIRoute } from "astro";
import {
  STATE_COOKIE,
  createSession,
  oauthCallbackUrl,
  oauthConfigured,
  oauthClientId,
  oauthClientSecret,
  safeReturnTo,
} from "@/lib/github-oauth";

export const prerender = false;

/** GET /api/auth/github/callback — code 换 token、取用户身份、落会话 cookie */
export const GET: APIRoute = async ({ url, site, cookies, redirect }) => {
  if (!oauthConfigured()) {
    return new Response("GitHub OAuth not configured", { status: 500 });
  }

  const stateCookie = cookies.get(STATE_COOKIE)?.value ?? "";
  cookies.delete(STATE_COOKIE, { path: "/" });

  const sep = stateCookie.indexOf("|");
  const expectedState = sep > 0 ? stateCookie.slice(0, sep) : "";
  const returnTo = safeReturnTo(sep > 0 ? stateCookie.slice(sep + 1) : null);
  const failed = `${returnTo}${returnTo.includes("?") ? "&" : "?"}auth_error=1`;

  const code = url.searchParams.get("code");
  const state = url.searchParams.get("state");
  if (!code || !state || !expectedState || state !== expectedState) {
    return redirect(failed, 302);
  }

  try {
    const tokenRes = await fetch("https://github.com/login/oauth/access_token", {
      method: "POST",
      headers: { "Content-Type": "application/json", Accept: "application/json" },
      body: JSON.stringify({
        client_id: oauthClientId(),
        client_secret: oauthClientSecret(),
        code,
        redirect_uri: oauthCallbackUrl(url, site),
      }),
    });
    const token = tokenRes.ok ? (await tokenRes.json()).access_token : null;
    if (!token) return redirect(failed, 302);

    const userRes = await fetch("https://api.github.com/user", {
      headers: {
        Authorization: `Bearer ${token}`,
        Accept: "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
      },
    });
    if (!userRes.ok) return redirect(failed, 302);

    const user = await userRes.json();
    if (typeof user.id !== "number" || typeof user.login !== "string") {
      return redirect(failed, 302);
    }

    createSession(cookies, {
      id: user.id,
      login: user.login,
      avatar: typeof user.avatar_url === "string" ? user.avatar_url : "",
    });

    return redirect(returnTo, 302);
  } catch (err) {
    console.error("GitHub OAuth callback error:", err);
    return redirect(failed, 302);
  }
};
