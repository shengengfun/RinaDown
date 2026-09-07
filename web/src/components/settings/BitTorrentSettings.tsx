// BitTorrent：librqbit 引擎参数（服务器 config 表）。
import { useQueryClient } from '@tanstack/react-query'
import { RefreshCw, RotateCcw } from 'lucide-react'
import { useRef, useState, type ReactNode } from 'react'
import { api } from '../../lib/api'
import { translateBackendMessage, useI18n } from '../../lib/i18n'
import type { ConfigMap } from '../../lib/types'
import { NumberInput, SetRow, SetSelect, SetSwitch } from './controls'

// 「重置为默认」用的自定义 Tracker 列表（与桌面 settings_page.dart `_kDefaultTrackers` 同步）。
const DEFAULT_TRACKERS = [
  'udp://tracker.dler.com:6969/announce',
  'udp://admin.52ywp.com:6969/announce',
  'udp://tracker.dler.org:6969/announce',
  'https://tracker.moeblog.cn:443/announce',
  'http://nyaa.tracker.wf:7777/announce',
  'https://tr.zukizuki.org:443/announce',
  'udp://tracker.opentrackr.org:1337/announce',
  'udp://open.dstud.io:6969/announce',
  'udp://tracker-udp.gbitt.info:80/announce',
  'udp://open.stealth.si:80/announce',
  'udp://tracker.torrent.eu.org:451/announce',
  'udp://exodus.desync.com:6969/announce',
  'udp://explodie.org:6969/announce',
  'udp://tracker.srv00.com:6969/announce',
  'udp://tracker.qu.ax:6969/announce',
  'udp://opentracker.io:6969/announce',
  'udp://tracker.bittor.pw:1337/announce',
  'udp://tracker.theoks.net:6969/announce',
  'udp://tracker.opentorrent.top:6969/announce',
  'udp://open.demonoid.ch:6969/announce',
  'udp://tracker.t-1.org:6969/announce',
  'https://tracker.ghostchu-services.top:443/announce',
  'https://tracker.bt4g.com:443/announce',
  'https://1337.abcvg.info:443/announce',
  'http://tracker.bt4g.com:2095/announce',
].join('\n')

// 默认订阅源（与桌面 `_kDefaultTrackerSubUrls` / 引擎 `default_subscription_urls` 同步）。
const DEFAULT_SUB_URLS = [
  'https://trackerslist.com/best.txt',
  'https://ngosang.github.io/trackerslist/trackers_best.txt',
].join('\n')

function countLines(s: string): number {
  return s.split('\n').filter((l) => l.trim().length > 0).length
}

function formatUpdatedAt(unixSecs: number): string {
  const d = new Date(unixSecs * 1000)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`
}

// ── 做种（对齐桌面端 BT 设置「做种」Tab，键全部热读，改动即时生效）──

const UNIT_FACTOR: Record<string, number> = { days: 1440, hours: 60, minutes: 1 }

function unitFactor(unit: string): number {
  return UNIT_FACTOR[unit] ?? 1
}

/** 条件行：开关 + 说明 + 数值（可选时间单位）。禁用时写 0，启用时恢复本会话缓存值或默认值。 */
function SeedConditionRow({
  enabled,
  label,
  onToggle,
  children,
}: {
  enabled: boolean
  label: string
  onToggle: (v: boolean) => void
  children: ReactNode
}) {
  return (
    <div className="flex items-center gap-2">
      <SetSwitch checked={enabled} onCheckedChange={onToggle} />
      <span className="flex-1 text-[12px] text-text3">{label}</span>
      {children}
    </div>
  )
}

function SeedingSettings({
  config,
  mutate,
}: {
  config: ConfigMap
  mutate: (entries: ConfigMap) => void
}) {
  const { t } = useI18n()
  const seedEnabled = (config.bt_seed_enabled ?? '1') !== '0'
  const autoReseed = (config.bt_auto_reseed ?? '1') !== '0'
  const maxActive = Number(config.bt_seed_max_active ?? '0')

  // 与桌面 SettingsProvider 同一编码：值 > 0 = 条件启用；0 = 禁用。
  const ratio = Number(config.bt_seed_ratio_limit ?? '0')
  const postRatio = Number(config.bt_seed_post_ratio_limit ?? '0')
  const timeMinutes = Number(config.bt_seed_time_limit_minutes ?? '0')
  const inactiveMinutes = Number(config.bt_seed_inactive_time_limit_minutes ?? '0')
  const timeUnit = config.bt_seed_time_limit_unit ?? 'minutes'
  const inactiveUnit = config.bt_seed_inactive_time_limit_unit ?? 'minutes'
  const operator = config.bt_seed_limit_operator === 'and' ? 'and' : 'or'
  const thenAction = ['delete', 'delete_files'].includes(config.bt_seed_then_action ?? '')
    ? (config.bt_seed_then_action as string)
    : 'stop'

  // 本会话内记住关闭前的值，重新启用时恢复（与桌面 *_Cached 语义一致）；默认值同桌面。
  const ratioCache = useRef(1.0)
  const postRatioCache = useRef(1.0)
  const timeCache = useRef(72 * 60)
  const inactiveCache = useRef(30)
  if (ratio > 0) ratioCache.current = ratio
  if (postRatio > 0) postRatioCache.current = postRatio
  if (timeMinutes > 0) timeCache.current = timeMinutes
  if (inactiveMinutes > 0) inactiveCache.current = inactiveMinutes

  const unitOptions = [
    { value: 'minutes', label: t('set.bt.timeUnitMinutes') },
    { value: 'hours', label: t('set.bt.timeUnitHours') },
    { value: 'days', label: t('set.bt.timeUnitDays') },
  ]

  return (
    <>
      <div className="set-group">
        <SetRow title={t('set.bt.seedEnabled')} desc={t('set.bt.seedEnabledDesc')}>
          <SetSwitch checked={seedEnabled} onCheckedChange={(v) => mutate({ bt_seed_enabled: v ? '1' : '0' })} />
        </SetRow>
        <SetRow title={t('set.bt.seedMaxActive')} desc={t('set.bt.seedMaxActiveDesc')}>
          <NumberInput
            value={maxActive}
            min={0}
            max={999}
            className="short"
            onCommit={(n) => mutate({ bt_seed_max_active: String(Math.min(999, Math.max(0, Math.round(n)))) })}
          />
        </SetRow>
        <SetRow title={t('set.bt.autoReseed')} desc={t('set.bt.autoReseedDesc')}>
          <SetSwitch checked={autoReseed} onCheckedChange={(v) => mutate({ bt_auto_reseed: v ? '1' : '0' })} />
        </SetRow>
      </div>

      <div className="set-group">
        <div className="set-row stack">
          <div className="set-info">
            <b>{t('set.bt.seedingTitle')}</b>
          </div>
          <SeedConditionRow
            enabled={ratio > 0}
            label={t('set.bt.seedRatioLimit')}
            onToggle={(v) => mutate({ bt_seed_ratio_limit: v ? String(ratioCache.current) : '0' })}
          >
            <NumberInput
              value={ratio > 0 ? ratio : ratioCache.current}
              min={0}
              className="short"
              onCommit={(n) => {
                if (n > 0) mutate({ bt_seed_ratio_limit: String(n) })
              }}
            />
          </SeedConditionRow>
          <SeedConditionRow
            enabled={postRatio > 0}
            label={t('set.bt.seedPostRatioLimit')}
            onToggle={(v) => mutate({ bt_seed_post_ratio_limit: v ? String(postRatioCache.current) : '0' })}
          >
            <NumberInput
              value={postRatio > 0 ? postRatio : postRatioCache.current}
              min={0}
              className="short"
              onCommit={(n) => {
                if (n > 0) mutate({ bt_seed_post_ratio_limit: String(n) })
              }}
            />
          </SeedConditionRow>
          <SeedConditionRow
            enabled={timeMinutes > 0}
            label={t('set.bt.seedTimeLimit')}
            onToggle={(v) => mutate({ bt_seed_time_limit_minutes: v ? String(timeCache.current) : '0' })}
          >
            <NumberInput
              value={Math.floor((timeMinutes > 0 ? timeMinutes : timeCache.current) / unitFactor(timeUnit))}
              min={0}
              className="short"
              onCommit={(n) => {
                const minutes = Math.round(n) * unitFactor(timeUnit)
                if (minutes > 0) mutate({ bt_seed_time_limit_minutes: String(minutes) })
              }}
            />
            <SetSelect
              width={92}
              value={timeUnit}
              onValueChange={(v) => mutate({ bt_seed_time_limit_unit: v })}
              options={unitOptions}
            />
          </SeedConditionRow>
          <SeedConditionRow
            enabled={inactiveMinutes > 0}
            label={t('set.bt.seedInactiveTimeLimit')}
            onToggle={(v) => mutate({ bt_seed_inactive_time_limit_minutes: v ? String(inactiveCache.current) : '0' })}
          >
            <NumberInput
              value={Math.floor((inactiveMinutes > 0 ? inactiveMinutes : inactiveCache.current) / unitFactor(inactiveUnit))}
              min={0}
              className="short"
              onCommit={(n) => {
                const minutes = Math.round(n) * unitFactor(inactiveUnit)
                if (minutes > 0) mutate({ bt_seed_inactive_time_limit_minutes: String(minutes) })
              }}
            />
            <SetSelect
              width={92}
              value={inactiveUnit}
              onValueChange={(v) => mutate({ bt_seed_inactive_time_limit_unit: v })}
              options={unitOptions}
            />
          </SeedConditionRow>
          <div className="flex items-center gap-2">
            <span className="flex-1 text-[12px] text-text3">{t('set.bt.seedConditionsOperator')}</span>
            <SetSelect
              width={140}
              value={operator}
              onValueChange={(v) => mutate({ bt_seed_limit_operator: v })}
              options={[
                { value: 'or', label: t('set.bt.seedOperatorOr') },
                { value: 'and', label: t('set.bt.seedOperatorAnd') },
              ]}
            />
          </div>
          <div className="flex items-center gap-2">
            <span className="flex-1 text-[12px] text-text3">{t('set.bt.seedThenAction')}</span>
            <SetSelect
              width={140}
              value={thenAction}
              onValueChange={(v) => mutate({ bt_seed_then_action: v })}
              options={[
                { value: 'stop', label: t('set.bt.seedStopSeeding') },
                { value: 'delete', label: t('set.bt.seedDeleteTask') },
                { value: 'delete_files', label: t('set.bt.seedDeleteTaskAndFiles') },
              ]}
            />
          </div>
        </div>
      </div>
    </>
  )
}

/** 展开式多行编辑器：本地草稿态 + 显式「确定 / 取消」，避免失焦静默提交让用户无从判断是否保存。 */
function TrackerEditor({
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

export function BitTorrentSettings({
  config,
  mutate,
}: {
  config: ConfigMap
  mutate: (entries: ConfigMap) => void
}) {
  const { t } = useI18n()
  const qc = useQueryClient()
  const dht = (config.bt_enable_dht ?? 'true') === 'true'
  const upnp = (config.bt_enable_upnp ?? 'true') === 'true'
  const portStart = Number(config.bt_port_start ?? '6881')
  const portEnd = Number(config.bt_port_end ?? '6889')
  const trackers = config.bt_custom_trackers ?? ''
  const subEnabled = (config.bt_tracker_sub_enabled ?? 'true') === 'true'
  const subUrls = config.bt_tracker_sub_urls ?? ''
  const subCount = countLines(config.bt_tracker_sub_cache ?? '')
  const subUpdatedAt = Number(config.bt_tracker_sub_updated_at ?? '0')

  const [listExpanded, setListExpanded] = useState(false)
  const [subExpanded, setSubExpanded] = useState(false)
  const [refreshing, setRefreshing] = useState(false)
  const [refreshError, setRefreshError] = useState('')

  async function runRefresh() {
    setRefreshing(true)
    setRefreshError('')
    try {
      const res = await api.refreshTrackerSub()
      if (!res.success) setRefreshError(res.error || t('set.bt.subUpdateFailed'))
    } catch (err) {
      setRefreshError(err instanceof Error ? translateBackendMessage(err.message) : t('set.bt.subUpdateFailed'))
    } finally {
      setRefreshing(false)
      // 缓存 / 时间戳已在服务器写回，重取 config 刷新计数与更新时间。
      qc.invalidateQueries({ queryKey: ['config'] })
    }
  }

  const subStatus = refreshing
    ? t('set.bt.subUpdating')
    : subUpdatedAt > 0
      ? `${t('set.bt.subStatus', { n: subCount })} · ${t('set.bt.subUpdatedAt', { time: formatUpdatedAt(subUpdatedAt) })}`
      : t('set.bt.subNever')

  return (
    <>
      <h2 className="set-title">{t('set.bt')}</h2>
      <p className="set-desc">{t('set.bt.desc')}</p>
      {/* 常规：DHT / UPnP / 端口。重启提示只针对这一组，与卡片同段，
          宽屏分列时不会被甩到别的列底部。 */}
      <section className="set-section">
        <div className="set-group">
          <SetRow title={t('set.bt.dht')} desc={t('set.bt.dhtDesc')}>
            <SetSwitch checked={dht} onCheckedChange={(v) => mutate({ bt_enable_dht: String(v) })} />
          </SetRow>
          <SetRow title={t('set.bt.upnp')} desc={t('set.bt.upnpDesc')}>
            <SetSwitch checked={upnp} onCheckedChange={(v) => mutate({ bt_enable_upnp: String(v) })} />
          </SetRow>
          <SetRow title={t('set.bt.ports')} desc={t('set.bt.portsDesc')}>
            <div className="flex items-center gap-2">
              <NumberInput value={portStart} min={1} className="short" onCommit={(n) => mutate({ bt_port_start: String(n) })} />
              <span className="text-text3">–</span>
              <NumberInput value={portEnd} min={1} className="short" onCommit={(n) => mutate({ bt_port_end: String(n) })} />
            </div>
          </SetRow>
        </div>
        <p className="set-note">{t('set.bt.restartNote')}</p>
      </section>

      {/* Tracker 列表 */}
      <div className="set-group">
        <div className="set-row stack">
          <div className="set-info">
            <b>{t('set.bt.trackers')}</b>
            <span>{t('set.bt.trackersDesc')}</span>
          </div>
          <div className="flex items-center gap-3">
            <span className="text-[12px] text-text3">{t('set.bt.trackersCount', { n: countLines(trackers) })}</span>
            <div className="ml-auto flex items-center gap-2">
              <button type="button" className="btn ghost sm" onClick={() => mutate({ bt_custom_trackers: DEFAULT_TRACKERS })}>
                <RotateCcw /> {t('set.bt.reset')}
              </button>
              <button type="button" className="btn ghost sm" onClick={() => setListExpanded((v) => !v)}>
                {listExpanded ? t('set.bt.collapse') : t('set.bt.manage')}
              </button>
            </div>
          </div>
          {listExpanded ? (
            <TrackerEditor
              value={trackers}
              rows={8}
              placeholder={'udp://tracker.opentrackr.org:1337/announce'}
              onSave={(v) => {
                mutate({ bt_custom_trackers: v })
                setListExpanded(false)
              }}
              onCancel={() => setListExpanded(false)}
            />
          ) : null}
        </div>
      </div>

      {/* Tracker 订阅 */}
      <div className="set-group">
        <div className="set-row stack">
          <div className="set-info">
            <b>{t('set.bt.trackerSub')}</b>
            <span>{t('set.bt.trackerSubDesc')}</span>
          </div>
          <div className="flex items-center gap-3">
            <span className="text-[12px] text-text3">{subStatus}</span>
            <div className="ml-auto">
              <SetSwitch checked={subEnabled} onCheckedChange={(v) => mutate({ bt_tracker_sub_enabled: String(v) })} />
            </div>
          </div>
          {subEnabled ? (
            <>
              {refreshError ? (
                <span className="text-[12px] text-danger">
                  {t('set.bt.subUpdateFailed')}: {refreshError}
                </span>
              ) : null}
              <div className="flex items-center justify-end gap-2">
                <button type="button" className="btn ghost sm" disabled={refreshing} onClick={runRefresh}>
                  <RefreshCw className={refreshing ? 'animate-spin' : undefined} />
                  {refreshing ? t('set.bt.subUpdating') : t('set.bt.subUpdateNow')}
                </button>
                <button type="button" className="btn ghost sm" onClick={() => mutate({ bt_tracker_sub_urls: DEFAULT_SUB_URLS })}>
                  <RotateCcw /> {t('set.bt.reset')}
                </button>
                <button type="button" className="btn ghost sm" onClick={() => setSubExpanded((v) => !v)}>
                  {subExpanded ? t('set.bt.collapse') : t('set.bt.manage')}
                </button>
              </div>
              {subExpanded ? (
                <TrackerEditor
                  value={subUrls}
                  rows={4}
                  placeholder={'https://trackerslist.com/best.txt'}
                  onSave={(v) => {
                    mutate({ bt_tracker_sub_urls: v })
                    setSubExpanded(false)
                  }}
                  onCancel={() => setSubExpanded(false)}
                />
              ) : null}
            </>
          ) : null}
        </div>
      </div>

      <SeedingSettings config={config} mutate={mutate} />
    </>
  )
}
