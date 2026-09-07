// 底部状态栏 —— 对齐桌面端 lib/src/widgets/status_bar.dart：
// 左簇：状态点(空闲/下载中) / ↓↑ 实时速度 / 活跃·暂停·总计 / 视图作用域摘要；
// 右簇快捷设置：限速 Popover(下载+上传，写 config 与设置页同源) / 代理模式快切 /
// 磁盘剩余与版本(web 特有保留)。桌面的「完成后关机」关的是本机，headless 服务器
// 无对应 API，不移植；「反馈」为桌面对话框，同样不移植。

import { useEffect, useState } from 'react'
import * as DropdownMenu from '@radix-ui/react-dropdown-menu'
import { useNavigate } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { ArrowDown, ArrowUp, Check, ChevronUp, Circle, FlaskConical, Gauge, Globe, HardDrive } from 'lucide-react'
import { api } from '../../lib/api'
import { cn } from '../../lib/cn'
import { confirmDialog } from '../../lib/confirm'
import { useConfigQuery, useConfigMutation } from '../../lib/config'
import { fmtBytes, fmtSpeed } from '../../lib/format'
import { useI18n } from '../../lib/i18n'
import { useViewPrefs } from '../../lib/view-prefs'
import { useGlobalSpeed } from '../../lib/ws'
import { useTasksUi } from './context'
import { useViewTasks } from './useViewTasks'

// 预设限速值（与桌面 _kPresets 一致；kbs 为 KB/s）
const PRESETS = [
  { label: '128 KB/s', kbs: 128 },
  { label: '512 KB/s', kbs: 512 },
  { label: '1 MB/s', kbs: 1024 },
  { label: '2 MB/s', kbs: 2048 },
  { label: '5 MB/s', kbs: 5120 },
]

/** 限速区块（下载/上传共用）：标题 + 开关、预设 chips、自定义 KB/s 输入。 */
function LimitSection({
  title,
  bytes,
  onCommit,
}: {
  title: string
  bytes: number
  onCommit: (bytes: number) => void
}) {
  const { t } = useI18n()
  const limited = bytes > 0
  const [text, setText] = useState(limited ? String(Math.round(bytes / 1024)) : '')
  useEffect(() => setText(bytes > 0 ? String(Math.round(bytes / 1024)) : ''), [bytes])

  const toggle = () => {
    if (limited) {
      onCommit(0)
    } else {
      const kbs = Number(text) > 0 ? Math.round(Number(text)) : 512
      onCommit(kbs * 1024)
    }
  }
  const commitCustom = () => {
    const kbs = Math.round(Number(text))
    if (Number.isFinite(kbs) && kbs > 0 && kbs * 1024 !== bytes) onCommit(kbs * 1024)
  }

  return (
    <div className="view-section">
      <div className="view-section-head">
        <span>{title}</span>
        <button type="button" className={cn('sbq-switch', limited && 'on')} role="switch" aria-checked={limited} onClick={toggle}>
          <i />
        </button>
      </div>
      <div className="view-chips">
        {PRESETS.map((p) => (
          <button
            key={p.kbs}
            type="button"
            className={cn('view-chip', limited && bytes === p.kbs * 1024 && 'active')}
            onClick={() => onCommit(p.kbs * 1024)}
          >
            {p.label}
          </button>
        ))}
      </div>
      <div className="mt-2 flex items-center gap-2">
        <input
          className="text-input"
          style={{ width: 110 }}
          inputMode="numeric"
          placeholder={t('statusbar.customRateHint')}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onBlur={commitCustom}
          onKeyDown={(e) => {
            if (e.key === 'Enter') commitCustom()
          }}
        />
        <span className="text-[12px] text-text3">KB/s</span>
      </div>
    </div>
  )
}

/** 桌面 _formatSpeed 同款：>=1MB/s 用 MB（整则不带小数），否则取整 KB/s。 */
function fmtLimit(bytes: number): string {
  if (bytes >= 1024 * 1024) {
    const mb = bytes / (1024 * 1024)
    const rounded = Math.round(mb)
    return rounded === mb ? `${rounded} MB/s` : `${mb.toFixed(1)} MB/s`
  }
  return `${Math.round(bytes / 1024)} KB/s`
}

/** 限速快捷 chip：无限制 / ↓X · ↑Y，点击弹出双区限速面板。 */
function SpeedLimitChip() {
  const { t } = useI18n()
  const { data: config } = useConfigQuery()
  const mutation = useConfigMutation()
  const dl = Number(config?.speed_limit_bytes ?? 0)
  const ul = Number(config?.upload_limit_bytes ?? 0)
  const active = dl > 0 || ul > 0

  let label: string
  if (dl > 0 && ul > 0) label = `↓${fmtLimit(dl)} · ↑${fmtLimit(ul)}`
  else if (ul > 0) label = `↑${fmtLimit(ul)}`
  else if (dl > 0) label = fmtLimit(dl)
  else label = t('statusbar.limitUnlimited')

  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild>
        <button type="button" className={cn('sb-item sbq-chip', active && 'accent')}>
          <Gauge size={13} />
          {label}
          <ChevronUp size={11} />
        </button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content className="view-panel show" style={{ width: 240 }} side="top" align="end" sideOffset={8}>
          <LimitSection
            title={t('set.download.speedLimit')}
            bytes={dl}
            onCommit={(b) => mutation.mutate({ speed_limit_bytes: String(b) })}
          />
          <LimitSection
            title={t('set.download.uploadLimit')}
            bytes={ul}
            onCommit={(b) => mutation.mutate({ upload_limit_bytes: String(b) })}
          />
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  )
}

/** 代理模式快切 chip：四模式 + 跳设置（与设置页 onModeChange 同一 CDN 互斥语义）。 */
function ProxyModeChip() {
  const { t } = useI18n()
  const { data: config } = useConfigQuery()
  const navigate = useNavigate()
  const mutation = useConfigMutation()
  const mode = config?.proxy_mode ?? 'none'
  const host = (config?.proxy_host ?? '').trim()
  const port = (config?.proxy_port ?? '').trim()
  const proxyType = config?.proxy_type ?? 'socks5'
  const manualUrl = host ? `${proxyType}://${host}${port ? `:${port}` : ''}` : ''
  const cdnMultiEnabled = (config?.cdn_multi_enabled ?? '0') === '1'

  const MODE_LABEL: Record<string, string> = {
    none: t('set.proxy.none'),
    system: t('set.proxy.system'),
    manual: t('set.proxy.manual'),
    auto: t('set.proxy.auto'),
  }

  /** 与 ProxySettings.onModeChange 对齐：切到 system/manual 且多 CDN 开启 → 确认互斥。 */
  async function select(v: string) {
    if (v === mode) return
    if (v === 'none' || v === 'auto' || !cdnMultiEnabled) {
      mutation.mutate({ proxy_mode: v })
      return
    }
    const ok = await confirmDialog({
      title: t('set.proxy.cdnMultiConfirmTitle'),
      message: t('set.proxy.cdnMultiConfirmDesc'),
      confirmLabel: t('set.proxy.cdnMultiConfirmEnable'),
    })
    if (ok) mutation.mutate({ cdn_multi_enabled: '0', proxy_mode: v })
  }

  const options: { value: string; subtitle?: string }[] = [
    { value: 'none' },
    { value: 'system' },
    { value: 'manual', subtitle: manualUrl || t('statusbar.proxyNotConfigured') },
    { value: 'auto' },
  ]

  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild>
        <button type="button" className={cn('sb-item sbq-chip', mode !== 'none' && 'accent')}>
          <Globe size={13} />
          {MODE_LABEL[mode] ?? MODE_LABEL.none}
          <ChevronUp size={11} />
        </button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content className="view-panel show" style={{ width: 240 }} side="top" align="end" sideOffset={8}>
          <b>{t('set.proxy.mode')}</b>
          {options.map((o) => (
            <DropdownMenu.Item
              key={o.value}
              className={cn('sbq-option', mode === o.value && 'active')}
              onSelect={() => void select(o.value)}
            >
              <span className="flex-1">
                {MODE_LABEL[o.value]}
                {o.subtitle && <em className="block text-[11px] not-italic text-text3">{o.subtitle}</em>}
              </span>
              {mode === o.value && <Check size={13} />}
            </DropdownMenu.Item>
          ))}
          <DropdownMenu.Separator className="ctx-sep" />
          <DropdownMenu.Item className="sbq-option muted" onSelect={() => navigate({ to: '/settings' })}>
            {t('statusbar.configureInSettings')}
          </DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  )
}

export function StatusBar() {
  const { t } = useI18n()
  const tasks = useViewTasks()
  const speed = useGlobalSpeed()
  const { data: stats } = useQuery({ queryKey: ['stats'], queryFn: api.stats, refetchInterval: 30_000 })
  const { statusTab } = useTasksUi()
  const prefs = useViewPrefs(statusTab)
  const active = tasks.filter((t) => t.status === 0 || t.status === 1 || t.status === 5).length
  const paused = tasks.filter((t) => t.status === 2).length
  const seedingCount = tasks.filter((t) => t.status === 3 && (t.seedingStatus === 1 || t.seedingStatus === 8)).length
  const uploadSpeed = tasks.reduce((s, t) => (t.seedingStatus === 1 ? s + t.uploadSpeed : s), 0)
  const visibleForScope = prefs.showCompleted ? tasks : tasks.filter((t) => t.status !== 3)
  const scopeCount = visibleForScope.length
  const scopeSizeBytes = visibleForScope.reduce((s, t) => s + t.totalBytes, 0)
  const hiddenCompleted = prefs.showCompleted ? 0 : tasks.filter((t) => t.status === 3).length

  return (
    <footer className="statusbar">
      <span className="sb-item">
        <Circle size={8} fill={active > 0 ? 'var(--ok)' : 'none'} color={active > 0 ? 'var(--ok)' : 'currentColor'} />
        <span className="sb-status-label">{active > 0 ? t('statusbar.downloading') : t('statusbar.idle')}</span>
      </span>
      <span className="sb-item accent">
        <ArrowDown size={13} />
        <b>{fmtSpeed(speed)}</b>
      </span>
      {(seedingCount > 0 || uploadSpeed > 0) && (
        <span className="sb-item accent">
          <ArrowUp size={13} />
          <b>{fmtSpeed(uploadSpeed)}</b>
        </span>
      )}
      <span className="sb-item sb-sum">{t('statusbar.summary', { active, paused, total: tasks.length })}</span>
      <span className="sb-item sb-scope">
        {t('view.scopeSummary', { count: scopeCount, size: fmtBytes(scopeSizeBytes) })}
        {hiddenCompleted > 0 && t('view.scopeHidden', { count: hiddenCompleted })}
      </span>
      {stats?.demoMode && (
        <span className="sb-item accent" title={t('statusbar.demoTitle', { url: stats.demoUrl })}>
          <FlaskConical size={13} />
          {t('statusbar.demoMode')}
        </span>
      )}
      <span className="flex1" />
      <SpeedLimitChip />
      <ProxyModeChip />
      <span className="sb-item sb-disk" title={t('statusbar.diskTitle')}>
        <HardDrive size={13} />
        <span className="sb-ellip">
          {stats
            ? t('statusbar.diskFree', { dir: stats.saveDir, free: stats.diskFreeBytes != null ? fmtBytes(stats.diskFreeBytes) : t('common.unknown') })
            : '—'}
        </span>
      </span>
      <span className="sb-item sb-ver">FluxDown Server {stats?.serverVersion ?? '—'}</span>
    </footer>
  )
}
