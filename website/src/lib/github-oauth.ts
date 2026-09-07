/**
 * GitHub OAuth 登录会话（定价页投票/讨论需真实 GitHub 用户）。
 *
 * 无 scope 授权：只取公开身份（id/login/avatar），不索取任何仓库权限。
 * 会话 = HMAC-SHA256 签名的 HttpOnly cookie（无服务端存储，重启不失效）。
 */
import { createHmac, randomBytes, timingSafeEqual } from "node:crypto";
import type { AstroCookies } from "astro";
import {
  GITHUB_OAUTH_CLIENT_ID,
  GITHUB_OAUTH_CLIENT_SECRET,
} from "astro:env/server";

export const SESSION_COOKIE = "fluxdown_gh_session";
export const STATE_COOKIE = "fluxdown_gh_oauth_state";

const SESSION_TTL_S = 30 * 24 * 3600; // 30 days

export interface SessionUser {
  id: number;
  login: string;
  avatar: string;
}

export function oauthConfigured(): boolean {
  return Boolean(GITHUB_OAUTH_CLIENT_ID && GITHUB_OAUTH_CLIENT_SECRET);
}

export function oauthClientId(): string {
  return GITHUB_OAUTH_CLIENT_ID ?? "";
}

export function oauthClientSecret(): string {
  return GITHUB_OAUTH_CLIENT_SECRET ?? "";
}

/**
 * OAuth 回调地址。生产环境 node standalone 跑在反向代理后面，
 * `url.origin` 会解析成 localhost，必须以 astro.config 的 `site` 为准；
 * 本地开发（另建指向 127.0.0.1 的 OAuth App）才用请求 origin。
 */
export function oauthCallbackUrl(url: URL, site: URL | undefined): string {
  const origin = import.meta.env.PROD && site ? site.origin : url.origin;
  return `${origin}/api/auth/github/callback`;
}

export function randomState(): string {
  return randomBytes(16).toString("hex");
}

function sign(payload: string): string {
  return createHmac("sha256", oauthClientSecret()).update(payload).digest("base64url");
}

const COOKIE_OPTS = {
  httpOnly: true,
  sameSite: "lax" as const,
  path: "/",
  secure: import.meta.env.PROD,
};

export function createSession(cookies: AstroCookies, user: SessionUser): void {
  const payload = Buffer.from(
    JSON.stringify({ ...user, exp: Math.floor(Date.now() / 1000) + SESSION_TTL_S }),
  ).toString("base64url");
  cookies.set(SESSION_COOKIE, `${payload}.${sign(payload)}`, {
    ...COOKIE_OPTS,
    maxAge: SESSION_TTL_S,
  });
}

export function getSessionUser(cookies: AstroCookies): SessionUser | null {
  if (!oauthConfigured()) return null;
  const raw = cookies.get(SESSION_COOKIE)?.value;
  if (!raw) return null;

  const dot = raw.lastIndexOf(".");
  if (dot <= 0) return null;
  const payload = raw.slice(0, dot);
  const sig = raw.slice(dot + 1);

  const expected = sign(payload);
  const a = Buffer.from(sig);
  const b = Buffer.from(expected);
  if (a.length !== b.length || !timingSafeEqual(a, b)) return null;

  try {
    const data = JSON.parse(Buffer.from(payload, "base64url").toString("utf8"));
    if (typeof data.id !== "number" || typeof data.login !== "string") return null;
    if (typeof data.exp !== "number" || data.exp * 1000 < Date.now()) return null;
    return {
      id: data.id,
      login: data.login,
      avatar: typeof data.avatar === "string" ? data.avatar : "",
    };
  } catch {
    return null;
  }
}

export function clearSession(cookies: AstroCookies): void {
  cookies.delete(SESSION_COOKIE, { path: "/" });
}

/** returnTo 只允许站内相对路径，防开放跳转 */
export function safeReturnTo(raw: string | null): string {
  if (raw && raw.startsWith("/") && !raw.startsWith("//")) return raw;
  return "/pricing";
}
