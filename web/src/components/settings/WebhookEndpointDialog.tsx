// Webhook 端点添加/编辑对话框 —— 左栏表单（预设网格 → 名称/URL → 事件订阅 →
// 队列过滤 → 高级折叠区），右栏实时载荷预览，页脚「发送测试」内联反馈。
// 与桌面端 `lib/src/widgets/webhook_endpoint_dialog.dart` 同构。

import * as Dialog from '@radix-ui/react-dialog'
import { ChevronRight, X } from 'lucide-react'
import { useMemo, useState } from 'react'
import { api } from '../../lib/api'
import { cn } from '../../lib/cn'
import { translateBackendMessage, useI18n } from '../../lib/i18n'
import type { I18nKey } from '../../lib/i18n'
import type { QueueDto, WebhookEndpoint, WebhookPreset } from '../../lib/types'
import { WEBHOOK_EVENTS } from '../../lib/types'
import { BRAND_PATHS } from './brandMarks'
import { SetSelect, SetSwitch } from './controls'

/**
 * 服务预设的品牌标记（只是视觉标识，行为一律来自引擎下发的预设元数据）。
 *
 * 有官方矢量标的走 {@link BRAND_PATHS} 真 logo；bark / serverchan 官方只有
 * 位图应用图标，custom 不是品牌 —— 这三个回退字标。
 */
export const PRESET_MARKS: Record<string, { glyph: string; color: string }> = {
  ntfy: { glyph: 'n', color: '#34d399' },
  gotify: { glyph: 'G', color: '#60a5fa' },
  bark: { glyph: 'B', color: '#f87171' },
  serverchan: { glyph: '酱', color: '#fb923c' },
  telegram: { glyph: 'T', color: '#38bdf8' },
  discord: { glyph: 'D', color: '#818cf8' },
  slack: { glyph: '#', color: '#f472b6' },
  custom: { glyph: '{}', color: '#a1a1aa' },
}

export function PresetTile({ presetId, size = 30 }: { presetId: string; size?: number }) {
  const mark = PRESET_MARKS[presetId] ?? PRESET_MARKS.custom
  const vector = BRAND_PATHS[presetId]
  return (
    <span
      className="flex flex-shrink-0 items-center justify-center rounded-lg border font-semibold"
      style={{
        width: size,
        height: size,
        fontSize: size * 0.44,
        color: mark.color,
        background: `${mark.color}1c`,
        borderColor: `${mark.color}44`,
      }}
    >
      {vector ? (
        <svg viewBox="0 0 24 24" width={size * 0.56} height={size * 0.56} aria-hidden>
          <path fill="currentColor" d={vector} />
        </svg>
      ) : (
        mark.glyph
      )}
    </span>
  )
}

/** 事件 wire 名 → 本地化短标签。 */
export function eventLabel(t: (k: I18nKey) => string, wire: string): string {
  const key = `set.notify.event.${wire}` as I18nKey
  const label = t(key)
  return label === key ? wire : label
}

export function newEndpointId(): string {
  return `wh_${Date.now().toString(16)}${Math.floor(Math.random() * 0xffffffff).toString(16)}`
}

function newSecret(): string {
  const bytes = new Uint8Array(16)
  crypto.getRandomValues(bytes)
  return `whsec_${Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('')}`
}

// ── 预览：占位符替换 ──────────────────────────────────────────────────
// 与引擎 `render_template` 同规则：占位符是**不含嵌套 `{`** 的 `{…}` 段，
// 未知段原样保留 —— JSON 字面量因此不会被破坏。

const SAMPLE_VARS: Record<string, string> = {
  '{event}': 'task.completed',
  '{event.title}': 'Download completed',
  '{event.summary}': 'ubuntu-24.04.2-desktop-amd64.iso · 6.0 GB',
  '{timestamp}': '2026-07-17T12:34:56Z',
  '{instance.app}': 'fluxdown',
  '{instance.version}': '0.1.44',
  '{instance.host}': 'fluxdown',
  '{task.id}': '00000000-0000-4000-8000-000000000000',
  '{task.fileName}': 'ubuntu-24.04.2-desktop-amd64.iso',
  '{task.url}': 'https://releases.ubuntu.com/24.04/ubuntu.iso',
  '{task.saveDir}': '/downloads',
  '{task.totalBytes}': '6442450944',
  '{task.totalBytesHuman}': '6.0 GB',
  '{task.status}': '3',
  '{task.errorMessage}': '',
  '{queue.id}': 'main',
  '{queue.name}': 'Main',
  '{ntfy.topic}': 'my-topic',
}

function renderTemplate(template: string, formEscape: boolean): string {
  let out = ''
  let i = 0
  while (i < template.length) {
    if (template[i] !== '{') {
      const start = i
      while (i < template.length && template[i] !== '{') i++
      out += template.slice(start, i)
      continue
    }
    let j = i + 1
    while (j < template.length && template[j] !== '}' && template[j] !== '{') j++
    if (j >= template.length || template[j] === '{') {
      out += '{'
      i++
      continue
    }
    const key = template.slice(i, j + 1)
    const value = SAMPLE_VARS[key]
    if (value === undefined) out += key
    else if (formEscape) out += encodeURIComponent(value)
    else out += JSON.stringify(value).slice(1, -1)
    i = j + 1
  }
  return out
}

const ENVELOPE_SAMPLE = {
  schemaVersion: 1,
  event: 'task.completed',
  deliveryId: '5f2a91c7-8b3e-4d10-a6f4-c2d90b7e13aa',
  timestamp: '2026-07-17T12:34:56Z',
  instance: { app: 'fluxdown', version: '0.1.44', host: 'fluxdown' },
  queue: { id: 'main', name: 'Main' },
  task: {
    id: '00000000-0000-4000-8000-000000000000',
    fileName: 'ubuntu-24.04.2-desktop-amd64.iso',
    url: 'https://releases.ubuntu.com/24.04/ubuntu.iso',
    saveDir: '/downloads',
    totalBytes: 6442450944,
    status: 3,
    errorMessage: '',
  },
}

// ── 对话框 ────────────────────────────────────────────────────────────

type TestState = { status: 'idle' | 'pending' | 'ok' | 'err'; detail?: string }
type HeaderRow = { key: string; value: string }

export interface WebhookEndpointDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  presets: WebhookPreset[]
  queues: QueueDto[]
  /** null = 新建。 */
  initial: WebhookEndpoint | null
  onSave: (endpoint: WebhookEndpoint) => void
}

export function WebhookEndpointDialog({
  open,
  onOpenChange,
  presets,
  queues,
  initial,
  onSave,
}: WebhookEndpointDialogProps) {
  const { t } = useI18n()
  // key={String(open)} 由调用方给 Content，打开即重挂载重置表单。
  const [name, setName] = useState(initial?.name ?? '')
  const [url, setUrl] = useState(initial?.url ?? '')
  const [presetId, setPresetId] = useState(initial?.preset ?? 'custom')
  const [events, setEvents] = useState<string[]>(initial?.events ?? ['task.completed', 'task.failed'])
  const [queueId, setQueueId] = useState(initial?.queueId ?? '')
  const [headers, setHeaders] = useState<HeaderRow[]>(
    Object.entries(initial?.headers ?? {}).map(([key, value]) => ({ key, value })),
  )
  const [template, setTemplate] = useState(initial?.bodyTemplate ?? '')
  const [signSecret, setSignSecret] = useState(initial?.signSecret ?? '')
  const [signOn, setSignOn] = useState(Boolean(initial?.signSecret))
  const [allowHttp, setAllowHttp] = useState(initial?.allowHttp ?? false)
  const [useProxy, setUseProxy] = useState(initial?.useProxy ?? false)
  const [advancedOpen, setAdvancedOpen] = useState(
    headers.length > 0 || Boolean(initial?.bodyTemplate) || Boolean(initial?.signSecret) || Boolean(initial?.allowHttp) || Boolean(initial?.useProxy),
  )
  const [urlTouched, setUrlTouched] = useState(false)
  const [test, setTest] = useState<TestState>({ status: 'idle' })

  const preset = presets.find((p) => p.id === presetId) ?? presets.find((p) => p.id === 'custom')

  const urlError = useMemo(() => {
    const raw = url.trim()
    if (raw === '') return null
    let parsed: URL
    try {
      parsed = new URL(raw)
    } catch {
      return t('set.notify.urlInvalid')
    }
    if (parsed.protocol === 'http:' && !allowHttp) return t('set.notify.urlWarnHttp')
    if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return t('set.notify.urlInvalid')
    return null
  }, [url, allowHttp, t])

  const canSave = name.trim() !== '' && url.trim() !== '' && urlError === null

  function draft(): WebhookEndpoint {
    const map: Record<string, string> = {}
    for (const h of headers) {
      const key = h.key.trim()
      if (key !== '') map[key] = h.value
    }
    return {
      id: initial?.id ?? newEndpointId(),
      name: name.trim(),
      preset: presetId,
      url: url.trim(),
      enabled: initial?.enabled ?? true,
      events: WEBHOOK_EVENTS.filter((e) => events.includes(e)),
      queueId,
      headers: map,
      bodyTemplate: template,
      signSecret: signOn ? signSecret.trim() : '',
      allowHttp,
      useProxy,
    }
  }

  async function runTest() {
    setTest({ status: 'pending' })
    try {
      const res = await api.testWebhook(draft())
      if (res.success) {
        setTest({
          status: 'ok',
          detail: t('set.notify.testOk', { status: res.statusCode || 'OK', ms: res.latencyMs }),
        })
      } else {
        setTest({
          status: 'err',
          detail: t('set.notify.testFail', { error: res.error || String(res.statusCode) }),
        })
      }
    } catch (err) {
      setTest({
        status: 'err',
        detail: t('set.notify.testFail', {
          error: err instanceof Error ? translateBackendMessage(err.message) : '',
        }),
      })
    }
  }

  const previewBody = useMemo(() => {
    const tpl = template.trim() !== '' ? template : (preset?.defaultTemplate ?? '')
    if (tpl === '') return JSON.stringify(ENVELOPE_SAMPLE, null, 2)
    const isForm = (preset?.contentType ?? '').startsWith('application/x-www-form')
    const rendered = renderTemplate(tpl, isForm)
    if (isForm) return rendered
    try {
      return JSON.stringify(JSON.parse(rendered), null, 2)
    } catch {
      return rendered
    }
  }, [template, preset])

  const firstEvent = WEBHOOK_EVENTS.find((e) => events.includes(e)) ?? 'task.completed'
  const previewHead = [
    `POST ${url.trim() || preset?.urlPlaceholder || ''}`,
    `Content-Type: ${preset?.contentType ?? 'application/json'}`,
    `X-FluxDown-Event: ${firstEvent}`,
    'X-FluxDown-Delivery: 5f2a91c7-…',
    ...(signOn ? ['X-FluxDown-Signature: t=1789647128,v1=9c41f2…'] : []),
  ].join('\n')

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="wbackdrop show" />
        <Dialog.Content asChild onPointerDownOutside={(e) => e.preventDefault()}>
          <div className="dialog show fields-compact" style={{ width: 880 }}>
            <header className="dlg-head">
              <Dialog.Title asChild>
                <b>{initial ? t('set.notify.dialogEdit') : t('set.notify.dialogAdd')}</b>
              </Dialog.Title>
              <Dialog.Close asChild>
                <button type="button" className="icon-btn sm" aria-label={t('common.close')}>
                  <X size={16} />
                </button>
              </Dialog.Close>
            </header>
            <Dialog.Description className="dlg-sub">{t('set.notify.dialogDesc')}</Dialog.Description>
            <div className="dlg-body" style={{ display: 'flex', gap: 0, padding: 0 }}>
              {/* 左：表单 */}
              <div className="thin-scroll min-w-0 flex-1 overflow-y-auto px-4 py-3">
                <FieldLabel>{t('set.notify.preset')}</FieldLabel>
                <div className="mt-2 flex flex-wrap gap-2">
                  {presets.map((p) => (
                    <button
                      key={p.id}
                      type="button"
                      onClick={() => setPresetId(p.id)}
                      className={cn(
                        'flex w-[82px] flex-col items-center gap-1.5 rounded-lg border py-2 transition-colors',
                        p.id === presetId
                          ? 'border-accent bg-accent-weak text-text'
                          : 'border-line bg-surface2 text-text2 hover:bg-hover',
                      )}
                    >
                      <PresetTile presetId={p.id} size={26} />
                      <span className="text-[11px]">{p.label}</span>
                    </button>
                  ))}
                </div>

                <div className="mt-4 flex gap-3">
                  <div className="w-[170px] flex-shrink-0">
                    <FieldLabel>{t('set.notify.name')}</FieldLabel>
                    <input
                      className="text-input mt-1.5 w-full"
                      value={name}
                      placeholder={preset?.label ?? ''}
                      spellCheck={false}
                      onChange={(e) => setName(e.target.value)}
                    />
                  </div>
                  <div className="min-w-0 flex-1">
                    <FieldLabel>{t('set.notify.url')}</FieldLabel>
                    <input
                      className="text-input mt-1.5 w-full"
                      value={url}
                      placeholder={preset?.urlPlaceholder ?? ''}
                      spellCheck={false}
                      onChange={(e) => setUrl(e.target.value)}
                      onBlur={() => setUrlTouched(true)}
                    />
                    <p className={cn('mt-1 text-[10.5px]', urlTouched && urlError ? 'text-warning' : 'text-text3')}>
                      {(urlTouched && urlError) ||
                        (presetId === 'ntfy' ? t('set.notify.urlHintNtfy') : t('set.notify.urlHint'))}
                    </p>
                  </div>
                </div>

                <div className="mt-4">
                  <FieldLabel>{t('set.notify.events')}</FieldLabel>
                  <p className={cn('mt-1 text-[11px]', events.length === 0 ? 'text-warning' : 'text-text3')}>
                    {events.length === 0 ? t('set.notify.eventsEmpty') : t('set.notify.eventsHint')}
                  </p>
                  <div className="mt-2 flex flex-wrap gap-1.5">
                    {WEBHOOK_EVENTS.map((ev) => {
                      const on = events.includes(ev)
                      return (
                        <button
                          key={ev}
                          type="button"
                          onClick={() =>
                            setEvents((prev) => (on ? prev.filter((x) => x !== ev) : [...prev, ev]))
                          }
                          className={cn(
                            'rounded-full border px-[11px] py-1 text-[11.5px] transition-colors',
                            on
                              ? 'border-accent bg-accent text-white'
                              : 'border-line bg-surface2 text-text2 hover:bg-hover',
                          )}
                        >
                          {eventLabel(t, ev)}
                        </button>
                      )
                    })}
                  </div>
                </div>

                <div className="mt-4">
                  <FieldLabel>{t('set.notify.queue')}</FieldLabel>
                  <div className="mt-1.5">
                    <SetSelect
                      value={queueId}
                      onValueChange={setQueueId}
                      options={[
                        { value: '', label: t('set.notify.queueAll') },
                        ...queues.map((q) => ({ value: q.queueId, label: q.name })),
                      ]}
                    />
                  </div>
                </div>

                <button
                  type="button"
                  className="mt-4 flex items-center gap-1.5 text-[12px] text-text2 hover:text-text"
                  onClick={() => setAdvancedOpen((v) => !v)}
                >
                  <ChevronRight size={14} className={cn('transition-transform', advancedOpen && 'rotate-90')} />
                  {t('set.notify.advanced')}
                </button>

                {advancedOpen ? (
                  <div className="mt-3 flex flex-col gap-4">
                    <div>
                      <FieldLabel>{t('set.notify.headers')}</FieldLabel>
                      <div className="mt-1.5 flex flex-col gap-1.5">
                        {headers.map((h, i) => (
                          // 行没有稳定 id，索引是唯一可用 key（增删都整体重排）。
                          // biome-ignore lint/suspicious/noArrayIndexKey: header rows have no stable id
                          <div className="flex items-center gap-2" key={i}>
                            <input
                              className="text-input w-[150px]"
                              placeholder={t('set.notify.headerName')}
                              value={h.key}
                              spellCheck={false}
                              onChange={(e) =>
                                setHeaders((prev) =>
                                  prev.map((row, idx) => (idx === i ? { ...row, key: e.target.value } : row)),
                                )
                              }
                            />
                            <input
                              className="text-input min-w-0 flex-1"
                              placeholder={t('set.notify.headerValue')}
                              value={h.value}
                              spellCheck={false}
                              onChange={(e) =>
                                setHeaders((prev) =>
                                  prev.map((row, idx) => (idx === i ? { ...row, value: e.target.value } : row)),
                                )
                              }
                            />
                            <button
                              type="button"
                              className="icon-btn sm text-text3"
                              aria-label={t('common.delete')}
                              onClick={() => setHeaders((prev) => prev.filter((_, idx) => idx !== i))}
                            >
                              <X size={13} />
                            </button>
                          </div>
                        ))}
                        <div>
                          <button
                            type="button"
                            className="btn ghost sm"
                            onClick={() => setHeaders((prev) => [...prev, { key: '', value: '' }])}
                          >
                            {t('set.notify.addHeader')}
                          </button>
                        </div>
                      </div>
                    </div>

                    <div>
                      <FieldLabel>{t('set.notify.template')}</FieldLabel>
                      <textarea
                        // `area` 把高度交还给 rows —— 不带它会吃到 `.text-input`
                        // 的固定行高，四行模板挤成一行。
                        className="text-input area mt-1.5 w-full font-mono text-[11.5px]"
                        rows={3}
                        spellCheck={false}
                        placeholder={t('set.notify.templatePlaceholder')}
                        value={template}
                        onChange={(e) => setTemplate(e.target.value)}
                      />
                      <p className="mt-1 text-[10.5px] text-text3">{t('set.notify.templateHint')}</p>
                      <div className="mt-1.5 flex flex-wrap gap-1">
                        {Object.keys(SAMPLE_VARS).map((v) => (
                          <button
                            key={v}
                            type="button"
                            className="rounded-md border border-line px-1.5 py-px font-mono text-[10px] text-text3 hover:text-text"
                            onClick={() => setTemplate((prev) => prev + v)}
                          >
                            {v}
                          </button>
                        ))}
                      </div>
                    </div>

                    <SwitchLine
                      title={t('set.notify.sign')}
                      desc={t('set.notify.signDesc')}
                      checked={signOn}
                      onChange={(v) => {
                        setSignOn(v)
                        if (v && signSecret.trim() === '') setSignSecret(newSecret())
                      }}
                    />
                    {signOn ? (
                      <div className="flex items-center gap-2">
                        <input
                          className="text-input min-w-0 flex-1 font-mono text-[11.5px]"
                          value={signSecret}
                          spellCheck={false}
                          onChange={(e) => setSignSecret(e.target.value)}
                        />
                        <button type="button" className="btn ghost sm" onClick={() => setSignSecret(newSecret())}>
                          {t('set.notify.regenerate')}
                        </button>
                      </div>
                    ) : null}

                    <SwitchLine
                      title={t('set.notify.allowHttp')}
                      desc={t('set.notify.allowHttpDesc')}
                      checked={allowHttp}
                      onChange={setAllowHttp}
                    />
                    <SwitchLine
                      title={t('set.notify.useProxy')}
                      desc={t('set.notify.useProxyDesc')}
                      checked={useProxy}
                      onChange={setUseProxy}
                    />
                  </div>
                ) : null}
              </div>

              {/* 右：实时载荷预览 */}
              <div className="flex w-[300px] flex-shrink-0 flex-col border-line border-l px-4 py-3">
                <FieldLabel>{t('set.notify.preview')}</FieldLabel>
                {/* 换行而非横向滚动：预览是给人读的，长 URL/文件名横着藏起来
                    等于没预览。`break-all` 保证超长 token 也能断开。 */}
                <pre className="thin-scroll mt-2 min-h-0 flex-1 overflow-y-auto whitespace-pre-wrap break-all rounded-lg border border-line bg-bg p-2.5 font-mono text-[10.8px] text-text2 leading-relaxed">
                  {`${previewHead}\n${'─'.repeat(28)}\n${previewBody}`}
                </pre>
                <p className="mt-2 whitespace-pre-line text-[10.5px] text-text3 leading-relaxed">
                  {t('set.notify.previewMeta')}
                </p>
              </div>
            </div>
            <footer className="dlg-foot" style={{ justifyContent: 'flex-start' }}>
              <button
                type="button"
                className="btn ghost sm"
                disabled={test.status === 'pending' || url.trim() === ''}
                onClick={() => void runTest()}
              >
                {test.status === 'pending' ? t('set.notify.testing') : t('set.notify.test')}
              </button>
              {test.status === 'ok' ? <span className="text-[12px] text-success">{test.detail}</span> : null}
              {test.status === 'err' ? <span className="text-[12px] text-danger">{test.detail}</span> : null}
              <span className="flex-1" />
              <Dialog.Close asChild>
                <button type="button" className="btn plain sm">
                  {t('common.cancel')}
                </button>
              </Dialog.Close>
              <button type="button" className="btn primary sm" disabled={!canSave} onClick={() => onSave(draft())}>
                {t('set.notify.save')}
              </button>
            </footer>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}

function FieldLabel({ children }: { children: React.ReactNode }) {
  return <span className="font-medium text-[11.5px] text-text2">{children}</span>
}

function SwitchLine({
  title,
  desc,
  checked,
  onChange,
}: {
  title: string
  desc: string
  checked: boolean
  onChange: (v: boolean) => void
}) {
  return (
    <div className="flex items-start gap-4">
      <div className="min-w-0 flex-1">
        <p className="text-[12.5px] text-text">{title}</p>
        <p className="mt-0.5 text-[11px] text-text3 leading-relaxed">{desc}</p>
      </div>
      <SetSwitch checked={checked} onCheckedChange={onChange} />
    </div>
  )
}
