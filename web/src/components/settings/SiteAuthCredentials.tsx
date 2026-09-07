// 已保存的网站凭据（config 键 site_auth_credentials）：列出/新增/编辑/删除。
// 归属「下载」分区 —— 设置分类以桌面端 settings_page 为基准（下载 → 已保存的网站凭据）。
import { useState } from 'react'
import { Pencil, Search } from 'lucide-react'
import { useI18n } from '../../lib/i18n'
import type { ConfigMap } from '../../lib/types'
import { filterSiteAuth, normalizeSiteKey, parseSiteAuthStore } from '../../lib/site-auth'
import { SetRow } from './controls'

/** 站点凭据编辑器状态：site=null 表示新增（需要输入站点），否则编辑该站点。 */
interface SiteAuthEditor {
  site: string | null
  siteInput: string
  user: string
  pass: string
}

export function SiteAuthCredentials({
  config,
  mutate,
}: {
  config: ConfigMap
  mutate: (entries: ConfigMap) => void
}) {
  const { t } = useI18n()
  const siteAuth = parseSiteAuthStore(config.site_auth_credentials)
  const siteAuthSites = Object.keys(siteAuth).sort()

  const [authEditor, setAuthEditor] = useState<SiteAuthEditor | null>(null)
  const [query, setQuery] = useState('')

  function openAuthEditor(site: string | null) {
    setAuthEditor({
      site,
      siteInput: '',
      user: site ? (siteAuth[site]?.user ?? '') : '',
      pass: site ? (siteAuth[site]?.pass ?? '') : '',
    })
  }

  /** 保存 = 更新/新增该站点键后整表 JSON 写回；新增遇同键即覆盖（编辑语义）。 */
  function saveAuthEditor() {
    if (!authEditor) return
    const key = authEditor.site ?? normalizeSiteKey(authEditor.siteInput)
    if (!key || !authEditor.user.trim()) return
    const next = { ...siteAuth, [key]: { user: authEditor.user.trim(), pass: authEditor.pass } }
    mutate({ site_auth_credentials: JSON.stringify(next) })
    setAuthEditor(null)
  }

  const authEditorCanSave =
    authEditor != null &&
    authEditor.user.trim() !== '' &&
    (authEditor.site !== null || normalizeSiteKey(authEditor.siteInput) !== null)

  const authEditorForm = authEditor ? (
    <div className="set-row stack">
      {authEditor.site === null ? (
        <input
          className="text-input"
          style={{ width: '100%' }}
          spellCheck={false}
          placeholder={t('set.siteAuth.sitePlaceholder')}
          value={authEditor.siteInput}
          onChange={(e) => setAuthEditor({ ...authEditor, siteInput: e.target.value })}
        />
      ) : (
        <b className="text-[13px]">{authEditor.site}</b>
      )}
      <input
        className="text-input"
        style={{ width: '100%' }}
        spellCheck={false}
        placeholder={t('set.siteAuth.username')}
        value={authEditor.user}
        onChange={(e) => setAuthEditor({ ...authEditor, user: e.target.value })}
      />
      <input
        className="text-input"
        style={{ width: '100%' }}
        type="password"
        autoComplete="new-password"
        placeholder={t('set.siteAuth.password')}
        value={authEditor.pass}
        onChange={(e) => setAuthEditor({ ...authEditor, pass: e.target.value })}
      />
      <div className="flex items-center justify-end gap-2">
        <button type="button" className="btn ghost sm" onClick={() => setAuthEditor(null)}>
          {t('common.cancel')}
        </button>
        <button type="button" className="btn ghost sm" disabled={!authEditorCanSave} onClick={saveAuthEditor}>
          {t('set.siteAuth.save')}
        </button>
      </div>
    </div>
  ) : null

  const filtered = filterSiteAuth(siteAuth, query)

  return (
    // 标题 + 卡片同段：宽屏分列时整块留在一列，标题不会被甩到列底的空白里。
    <section className="set-section">
      <h2 className="set-title mt-6">{t('set.siteAuth')}</h2>
      <p className="set-desc">{t('set.siteAuth.desc')}</p>
      <div className="set-group">
        {siteAuthSites.length > 0 ? (
          <div className="set-row">
            <div className="flex w-full items-center gap-3">
              <div className="relative flex-1">
                <Search
                  size={13}
                  className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-text3"
                />
                <input
                  className="text-input"
                  style={{ width: '100%', paddingLeft: 30 }}
                  type="search"
                  spellCheck={false}
                  placeholder={t('set.siteAuth.searchPlaceholder')}
                  aria-label={t('set.siteAuth.searchPlaceholder')}
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                />
              </div>
              <span className="shrink-0 text-xs text-text3">
                {query.trim()
                  ? t('set.siteAuth.countFiltered', {
                      m: filtered.length,
                      n: siteAuthSites.length,
                    })
                  : t('set.siteAuth.count', { n: siteAuthSites.length })}
              </span>
            </div>
          </div>
        ) : null}
        {siteAuthSites.length === 0 ? (
          <p className="set-note">{t('set.siteAuth.empty')}</p>
        ) : filtered.length === 0 ? (
          <p className="set-note">{t('set.siteAuth.noMatch')}</p>
        ) : (
          // 数百条凭据时容器内滚动，不撑高设置页。
          <div className="max-h-96 overflow-y-auto">
            {filtered.map(([site, entry]) => (
              <div key={site}>
                <SetRow title={site} desc={entry.user ?? ''}>
                  <div className="flex items-center gap-1">
                    <button
                      type="button"
                      className="icon-btn sm"
                      title={t('set.siteAuth.edit')}
                      aria-label={t('set.siteAuth.edit')}
                      onClick={() => openAuthEditor(site)}
                    >
                      <Pencil size={14} />
                    </button>
                    <button
                      type="button"
                      className="btn ghost sm"
                      onClick={() => {
                        const next = { ...siteAuth }
                        delete next[site]
                        if (authEditor?.site === site) setAuthEditor(null)
                        mutate({ site_auth_credentials: JSON.stringify(next) })
                      }}
                    >
                      {t('common.delete')}
                    </button>
                  </div>
                </SetRow>
                {authEditor?.site === site ? authEditorForm : null}
              </div>
            ))}
          </div>
        )}
        <div className="set-row">
          <button type="button" className="btn ghost sm" onClick={() => openAuthEditor(null)}>
            {t('set.siteAuth.add')}
          </button>
        </div>
        {authEditor && authEditor.site === null ? authEditorForm : null}
      </div>
    </section>
  )
}
