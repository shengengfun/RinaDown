// 站点 HTTP 认证凭据（config 键 site_auth_credentials）的共享工具：站点键规则必须与
// 引擎 site_auth::site_key 逐字一致 —— 仅 http/https，键 = 小写 host，端口显式且非协议
// 默认（http:80 / https:443）才追加 `:port`。new URL().port 对默认端口本就返回 ''。

/** config 键 site_auth_credentials 的 JSON 形态：站点键（host[:port]）→ 明文凭据。 */
export interface SiteAuthEntry {
  user: string
  pass: string
}

/** 解析凭据表 JSON；损坏/缺失按空表处理，仅影响展示与回填，不主动覆写。 */
export function parseSiteAuthStore(raw: string | undefined): Record<string, SiteAuthEntry> {
  if (!raw) return {}
  try {
    const v: unknown = JSON.parse(raw)
    if (v && typeof v === 'object' && !Array.isArray(v)) return v as Record<string, SiteAuthEntry>
  } catch {
    // 损坏的 JSON 按空处理。
  }
  return {}
}

/** 从下载 URL 提取站点键：仅 http/https，其余协议或解析失败返回 null。 */
export function siteKeyFromUrl(url: string): string | null {
  try {
    const u = new URL(url.trim())
    if (u.protocol !== 'http:' && u.protocol !== 'https:') return null
    if (!u.hostname) return null
    return (u.port ? `${u.hostname}:${u.port}` : u.hostname).toLowerCase()
  } catch {
    return null
  }
}

/** 站点输入归一化：含 `://` 走 URL 解析取 host[:port]（协议默认端口省略），
 *  否则按裸 host[:port] 处理；统一小写，无法解析返回 null。 */
export function normalizeSiteKey(raw: string): string | null {
  const s = raw.trim()
  if (!s) return null
  if (s.includes('://')) {
    try {
      const u = new URL(s)
      if (!u.hostname) return null
      // URL 已把协议默认端口归一为空串（http:80 / https:443）。
      return (u.port ? `${u.hostname}:${u.port}` : u.hostname).toLowerCase()
    } catch {
      return null
    }
  }
  return s.toLowerCase()
}

/** 凭据列表模糊过滤 + 站点字典序排序：查询串按空白拆词，**每个词**都必须是
 *  `站点 + ' ' + 用户名` 的子串（大小写不敏感）。空查询返回全部。
 *  规则与桌面端 `filterSiteAuth`（Dart）保持一致。 */
export function filterSiteAuth(
  store: Record<string, SiteAuthEntry>,
  query: string,
): [string, SiteAuthEntry][] {
  const entries = Object.entries(store).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
  const terms = query.toLowerCase().split(/\s+/).filter(Boolean)
  if (terms.length === 0) return entries
  return entries.filter(([site, e]) => {
    const haystack = `${site} ${e.user ?? ''}`.toLowerCase()
    return terms.every((t) => haystack.includes(t))
  })
}
