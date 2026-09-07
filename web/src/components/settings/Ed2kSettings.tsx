// eD2K：电驴 / eMule 引擎参数（服务器 config 表），对齐桌面 settings_page.dart 的
// `_Ed2kBasicContent`（常规）+ `_Ed2kServersContent`（服务器）两个 Tab，web 侧按分组卡片铺开。
import { useQueryClient } from '@tanstack/react-query'
import { RefreshCw, RotateCcw } from 'lucide-react'
import { useState } from 'react'
import { api } from '../../lib/api'
import { translateBackendMessage, useI18n } from '../../lib/i18n'
import type { ConfigMap } from '../../lib/types'
import { NumberInput, SetRow, SetSwitch } from './controls'

// 内置默认服务器（与桌面 `_kDefaultEd2kServers` / 引擎 db.rs 的 ed2k_server_list 默认值同步）。
const DEFAULT_SERVERS = [
  '176.123.5.89:4725',
  '45.82.80.155:5687',
  '85.121.5.137:4232',
  '176.123.2.239:4232',
  '145.239.2.134:4661',
  '91.208.162.87:4232',
  '37.15.61.236:4232',
]

// 默认 server.met 订阅地址（与引擎 `server_subscription::DEFAULT_SERVER_MET_URLS` 同步）。
const DEFAULT_MET_URLS = ['http://upd.emule-security.org/server.met', 'https://www.shortypower.org/server.met']

/** 逗号或换行分隔 → 去空去重后的条目（判重按 trim 后的小写形式，与桌面 `_Ed2kServerEditor._save` 一致）。 */
function cleanServers(raw: string): string[] {
  const seen = new Set<string>()
  const items: string[] = []
  for (const line of raw.split(/[\n,]/)) {
    const v = line.trim()
    if (!v) continue
    if (!seen.has(v.toLowerCase())) {
      seen.add(v.toLowerCase())
      items.push(v)
    }
  }
  return items
}

/** 订阅地址清洗：按行去空去重（判重忽略大小写与结尾斜杠，与桌面 `_Ed2kServerSubEditor._save` 一致）。 */
function cleanUrls(raw: string): string[] {
  const seen = new Set<string>()
  const items: string[] = []
  for (const line of raw.split('\n')) {
    const v = line.trim()
    if (!v) continue
    const key = v.toLowerCase().replace(/\/+$/, '')
    if (!seen.has(key)) {
      seen.add(key)
      items.push(v)
    }
  }
  return items
}

function formatUpdatedAt(unixSecs: number): string {
  const d = new Date(unixSecs * 1000)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`
}

/** 展开式多行编辑器：本地草稿态 + 显式「确定 / 取消」（与 BitTorrentSettings 的 Tracker 编辑器同一交互）。 */
function LineListEditor({
  value,
  rows,
  placeholder,
  onSave,
  onCancel,
}: {
  value: string
  rows: number
  placeholder: string
  onSave: (v: string) => void
  onCancel: () => void
}) {
  const { t } = useI18n()
  const [draft, setDraft] = useState(value)
  const dirty = draft !== value
  return (
    <div className="flex flex-col gap-2">
      <textarea
        className="text-input area"
        style={{ width: '100%' }}
        spellCheck={false}
        rows={rows}
        placeholder={placeholder}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
      />
      <div className="flex items-center justify-end gap-2">
        <button type="button" className="btn ghost sm" onClick={onCancel}>
          {t('common.cancel')}
        </button>
        <button type="button" className="btn primary sm" disabled={!dirty} onClick={() => onSave(draft)}>
          {t('common.confirm')}
        </button>
      </div>
    </div>
  )
}

export function Ed2kSettings({
  config,
  mutate,
}: {
  config: ConfigMap
  mutate: (entries: ConfigMap) => void
}) {
  const { t } = useI18n()
  const qc = useQueryClient()

  const enableKad = (config.ed2k_enable_kad ?? 'true') === 'true'
  const enableUpnp = (config.ed2k_enable_upnp ?? 'true') === 'true'
  const listenPort = Number(config.ed2k_listen_port ?? '0')

  // 服务器列表存储为逗号分隔（引擎 `parse_server_list` 只按 ',' 切分），编辑区按行展示。
  const servers = cleanServers(config.ed2k_server_list ?? '')
  const subEnabled = (config.ed2k_server_sub_enabled ?? 'true') === 'true'
  // 订阅地址存储为按行分隔（引擎默认值即 "\n" 连接）。
  const subUrls = config.ed2k_server_sub_urls ?? ''
  // 订阅缓存同样是逗号分隔的 ip:port 列表。
  const subCount = cleanServers(config.ed2k_server_sub_cache ?? '').length
  const subUpdatedAt = Number(config.ed2k_server_sub_updated_at ?? '0')

  const [listExpanded, setListExpanded] = useState(false)
  const [subExpanded, setSubExpanded] = useState(false)
  const [refreshing, setRefreshing] = useState(false)
  const [refreshError, setRefreshError] = useState('')

  async function runRefresh() {
    setRefreshing(true)
    setRefreshError('')
    try {
      const res = await api.refreshEd2kServerSub()
      if (!res.success) setRefreshError(res.error || t('set.ed2k.subUpdateFailed'))
    } catch (err) {
      setRefreshError(err instanceof Error ? translateBackendMessage(err.message) : t('set.ed2k.subUpdateFailed'))
    } finally {
      setRefreshing(false)
      // 缓存 / 时间戳已在服务器写回，重取 config 刷新计数与更新时间。
      qc.invalidateQueries({ queryKey: ['config'] })
    }
  }

  const subStatus = refreshing
    ? t('set.ed2k.subUpdating')
    : subUpdatedAt > 0
      ? `${t('set.ed2k.subStatus', { n: subCount })} · ${t('set.ed2k.subUpdatedAt', { time: formatUpdatedAt(subUpdatedAt) })}`
      : t('set.ed2k.subNever')

  return (
    <>
      <h2 className="set-title">{t('set.ed2k')}</h2>
      <p className="set-desc">{t('set.ed2k.desc')}</p>

      {/* 常规：Kad / UPnP / 监听端口。生效提示只针对这一组，与卡片同段，
          宽屏分列时不会被甩到别的列底部。 */}
      <section className="set-section">
        <div className="set-group">
          <SetRow title={t('set.ed2k.kad')} desc={t('set.ed2k.kadDesc')}>
            <SetSwitch checked={enableKad} onCheckedChange={(v) => mutate({ ed2k_enable_kad: String(v) })} />
          </SetRow>
          <SetRow title={t('set.ed2k.upnp')} desc={t('set.ed2k.upnpDesc')}>
            <SetSwitch checked={enableUpnp} onCheckedChange={(v) => mutate({ ed2k_enable_upnp: String(v) })} />
          </SetRow>
          <SetRow title={t('set.ed2k.listenPort')} desc={t('set.ed2k.listenPortDesc')}>
            <NumberInput
              value={listenPort}
              min={0}
              max={65535}
              className="short"
              onCommit={(n) => mutate({ ed2k_listen_port: String(Math.min(65535, Math.max(0, Math.round(n)))) })}
            />
          </SetRow>
        </div>
        <p className="set-note">{t('set.ed2k.applyNote')}</p>
      </section>

      {/* 服务器列表 */}
      <div className="set-group">
        <div className="set-row stack">
          <div className="set-info">
            <b>{t('set.ed2k.serverList')}</b>
            <span>{t('set.ed2k.serverListDesc')}</span>
          </div>
          <div className="flex items-center gap-3">
            <span className="text-[12px] text-text3">{t('set.ed2k.serverCount', { n: servers.length })}</span>
            <div className="ml-auto flex items-center gap-2">
              <button
                type="button"
                className="btn ghost sm"
                onClick={() => mutate({ ed2k_server_list: DEFAULT_SERVERS.join(',') })}
              >
                <RotateCcw /> {t('set.ed2k.reset')}
              </button>
              <button type="button" className="btn ghost sm" onClick={() => setListExpanded((v) => !v)}>
                {listExpanded ? t('set.ed2k.collapse') : t('set.ed2k.manage')}
              </button>
            </div>
          </div>
          {listExpanded ? (
            <LineListEditor
              value={servers.join('\n')}
              rows={8}
              placeholder={'176.123.5.89:4725'}
              onSave={(v) => {
                mutate({ ed2k_server_list: cleanServers(v).join(',') })
                setListExpanded(false)
              }}
              onCancel={() => setListExpanded(false)}
            />
          ) : null}
        </div>
      </div>

      {/* 服务器订阅（server.met） */}
      <div className="set-group">
        <div className="set-row stack">
          <div className="set-info">
            <b>{t('set.ed2k.serverSub')}</b>
            <span>{t('set.ed2k.serverSubDesc')}</span>
          </div>
          <div className="flex items-center gap-3">
            <span className="text-[12px] text-text3">{subStatus}</span>
            <div className="ml-auto">
              <SetSwitch checked={subEnabled} onCheckedChange={(v) => mutate({ ed2k_server_sub_enabled: String(v) })} />
            </div>
          </div>
          {subEnabled ? (
            <>
              {refreshError ? (
                <span className="text-[12px] text-danger">
                  {t('set.ed2k.subUpdateFailed')}: {refreshError}
                </span>
              ) : null}
              <div className="flex items-center justify-end gap-2">
                <button type="button" className="btn ghost sm" disabled={refreshing} onClick={runRefresh}>
                  <RefreshCw className={refreshing ? 'animate-spin' : undefined} />
                  {refreshing ? t('set.ed2k.subUpdating') : t('set.ed2k.subUpdateNow')}
                </button>
                <button
                  type="button"
                  className="btn ghost sm"
                  onClick={() => mutate({ ed2k_server_sub_urls: DEFAULT_MET_URLS.join('\n') })}
                >
                  <RotateCcw /> {t('set.ed2k.reset')}
                </button>
                <button type="button" className="btn ghost sm" onClick={() => setSubExpanded((v) => !v)}>
                  {subExpanded ? t('set.ed2k.collapse') : t('set.ed2k.manage')}
                </button>
              </div>
              {subExpanded ? (
                <LineListEditor
                  value={subUrls}
                  rows={4}
                  placeholder={'http://upd.emule-security.org/server.met'}
                  onSave={(v) => {
                    mutate({ ed2k_server_sub_urls: cleanUrls(v).join('\n') })
                    setSubExpanded(false)
                  }}
                  onCancel={() => setSubExpanded(false)}
                />
              ) : null}
            </>
          ) : null}
        </div>
      </div>
    </>
  )
}
