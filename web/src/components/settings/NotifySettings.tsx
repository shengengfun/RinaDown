// 通知设置：自托管 Webhook 任务事件推送。
// 系统托盘通知（notify_on_complete）是桌面 App 专属，headless 无从弹出，本页不呈现。
// 端点表就是 config 键 `webhook.endpoints` 的 JSON 数组，读写都走 /api/v1/config；
// 投递日志/预设目录/测试投递才需要 /api/v1/webhooks/*（引擎内存态，DB 查不到）。

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Plus, ScrollText } from 'lucide-react'
import { useState } from 'react'
import { api } from '../../lib/api'
import { cn } from '../../lib/cn'
import { confirmDialog } from '../../lib/confirm'
import { useI18n } from '../../lib/i18n'
import { toast } from '../../lib/toast'
import type { ConfigMap, WebhookDelivery, WebhookEndpoint } from '../../lib/types'
import { SetSwitch } from './controls'
import { Spinner } from './Spinner'
import { WebhookDeliveryDialog } from './WebhookDeliveryDialog'
import { eventLabel, PresetTile, WebhookEndpointDialog } from './WebhookEndpointDialog'

const ENDPOINTS_KEY = 'webhook.endpoints'

/** 坏配置不该炸掉整页——解析失败按空表处理（与引擎/桌面端一致）。 */
function parseEndpoints(raw: string | undefined): WebhookEndpoint[] {
  if (!raw || raw.trim() === '') return []
  try {
    const parsed: unknown = JSON.parse(raw)
    return Array.isArray(parsed) ? (parsed as WebhookEndpoint[]) : []
  } catch {
    return []
  }
}

/** URL 中段掩码：token 常在路径里，列表页不该把它整条摊开。 */
function maskUrl(url: string): string {
  if (url.length <= 44) return url
  return `${url.slice(0, 30)}•••${url.slice(-8)}`
}

export function NotifySettings({
  config,
  mutate,
}: {
  config: ConfigMap
  mutate: (entries: ConfigMap) => void
}) {
  const { t } = useI18n()
  const qc = useQueryClient()
  const endpoints = parseEndpoints(config[ENDPOINTS_KEY])

  const { data: queues = [] } = useQuery({ queryKey: ['queues'], queryFn: api.listQueues })
  // 不轮询：投递日志由 WS `webhookDeliveriesChanged` 推（引擎侧 500ms 节流），
  // 与任务/队列同范式。这里只负责首屏那一次拉取。
  const { data: webhookData } = useQuery({
    queryKey: ['webhookDeliveries'],
    queryFn: api.webhookDeliveries,
  })
  const deliveries: WebhookDelivery[] = webhookData?.deliveries ?? []
  const presets = webhookData?.presets ?? []

  const [editing, setEditing] = useState<WebhookEndpoint | null>(null)
  const [dialogOpen, setDialogOpen] = useState(false)
  const [logOpen, setLogOpen] = useState(false)
  const [logFilter, setLogFilter] = useState('')

  const invalidateLog = () => void qc.invalidateQueries({ queryKey: ['webhookDeliveries'] })

  const testOne = useMutation({
    mutationFn: (endpoint: WebhookEndpoint) => api.testWebhook(endpoint),
    onSuccess: (res) => {
      toast(
        res.success
          ? t('set.notify.testOk', { status: res.statusCode || 'OK', ms: res.latencyMs })
          : t('set.notify.testFail', { error: res.error || String(res.statusCode) }),
        res.success ? 'info' : 'error',
      )
      invalidateLog()
    },
    onError: (err: Error) => toast(err.message, 'error'),
  })

  function persist(next: WebhookEndpoint[]) {
    mutate({ [ENDPOINTS_KEY]: JSON.stringify(next) })
  }

  function upsert(endpoint: WebhookEndpoint) {
    const index = endpoints.findIndex((e) => e.id === endpoint.id)
    const next = index >= 0 ? endpoints.map((e) => (e.id === endpoint.id ? endpoint : e)) : [...endpoints, endpoint]
    persist(next)
    setDialogOpen(false)
  }

  async function remove(endpoint: WebhookEndpoint) {
    const ok = await confirmDialog({
      title: t('set.notify.deleteTitle'),
      message: t('set.notify.deleteMsg', { name: endpoint.name || endpoint.url }),
      danger: true,
    })
    if (ok) persist(endpoints.filter((e) => e.id !== endpoint.id))
  }

  function openLog(endpointId: string) {
    setLogFilter(endpointId)
    setLogOpen(true)
    invalidateLog()
  }

  return (
    <>
      <h2 className="set-title">{t('set.notify')}</h2>
      <p className="set-desc">{t('set.notify.desc')}</p>

      <div className="mb-2 flex items-center gap-2">
        <h3 className="flex-1 font-semibold text-[12.5px] text-text2">{t('set.notify.webhookGroup')}</h3>
        <button type="button" className="btn ghost sm" onClick={() => openLog('')}>
          <ScrollText size={13} />
          {t('set.notify.deliveryLog')}
        </button>
        <button
          type="button"
          className="btn primary sm"
          onClick={() => {
            setEditing(null)
            setDialogOpen(true)
          }}
        >
          <Plus size={13} />
          {t('set.notify.addEndpoint')}
        </button>
      </div>

      {endpoints.length === 0 ? (
        <div className="rounded-xl border border-line bg-surface p-4">
          <p className="text-[13px] text-text">{t('set.notify.emptyTitle')}</p>
          <p className="mt-1 text-[11.5px] text-text3 leading-relaxed">{t('set.notify.emptyDesc')}</p>
          <button
            type="button"
            className="btn primary sm mt-3"
            onClick={() => {
              setEditing(null)
              setDialogOpen(true)
            }}
          >
            {t('set.notify.addEndpoint')}
          </button>
        </div>
      ) : (
        <div className="flex flex-col gap-2">
          {endpoints.map((e) => (
            <EndpointRow
              key={e.id}
              endpoint={e}
              latest={deliveries.find((d) => d.endpointId === e.id)}
              testing={testOne.isPending && testOne.variables?.id === e.id}
              onToggle={(v) => persist(endpoints.map((x) => (x.id === e.id ? { ...x, enabled: v } : x)))}
              onTest={() => {
                // 慢网络下用户会连点，每一下都是一条真实对外请求。
                if (!testOne.isPending) testOne.mutate(e)
              }}
              onLogs={() => openLog(e.id)}
              onEdit={() => {
                setEditing(e)
                setDialogOpen(true)
              }}
              onDelete={() => void remove(e)}
            />
          ))}
        </div>
      )}

      <p className="mt-2 text-[10.5px] text-text3 leading-relaxed">{t('set.notify.semantics')}</p>

      {dialogOpen ? (
        <WebhookEndpointDialog
          key={editing?.id ?? 'new'}
          open={dialogOpen}
          onOpenChange={setDialogOpen}
          presets={presets}
          queues={queues}
          initial={editing}
          onSave={upsert}
        />
      ) : null}

      <WebhookDeliveryDialog
        open={logOpen}
        onOpenChange={setLogOpen}
        deliveries={deliveries}
        endpoints={endpoints}
        endpointId={logFilter}
        onEndpointIdChange={setLogFilter}
        onRefresh={invalidateLog}
      />
    </>
  )
}

function EndpointRow({
  endpoint,
  latest,
  testing,
  onToggle,
  onTest,
  onLogs,
  onEdit,
  onDelete,
}: {
  endpoint: WebhookEndpoint
  latest: WebhookDelivery | undefined
  testing: boolean
  onToggle: (v: boolean) => void
  onTest: () => void
  onLogs: () => void
  onEdit: () => void
  onDelete: () => void
}) {
  const { t } = useI18n()

  // 健康状态 = 投递日志的第一层：不点日志也知道端点死活。
  let healthTone = 'text-text3'
  let healthText = t('set.notify.healthNone')
  let dotColor = 'var(--text3)'
  if (!endpoint.enabled) {
    healthText = t('set.notify.healthDisabled')
  } else if (latest?.success) {
    healthTone = 'text-text3'
    dotColor = 'var(--success)'
    healthText = t('set.notify.healthOk', { detail: `${latest.statusCode} · ${latest.latencyMs}ms` })
  } else if (latest) {
    healthTone = 'text-danger'
    dotColor = 'var(--danger)'
    healthText = t('set.notify.healthFail', {
      detail:
        latest.statusCode > 0
          ? `${latest.statusCode} · ${t('set.notify.attempts', { n: latest.attempts })}`
          : latest.error,
    })
  }

  return (
    <div
      className={cn(
        'group flex items-center gap-3 rounded-xl border border-line bg-surface px-4 py-3',
        !endpoint.enabled && 'opacity-60',
      )}
    >
      <PresetTile presetId={endpoint.preset} />
      <div className="min-w-0 flex-1">
        <p className="truncate text-[13px] text-text">{endpoint.name || endpoint.url}</p>
        <p className="truncate font-mono text-[10.5px] text-text3">{maskUrl(endpoint.url)}</p>
      </div>
      <div className="flex flex-shrink-0 items-center gap-1.5 group-hover:hidden">
        {endpoint.events.map((ev) => (
          <span key={ev} className="rounded-full border border-line px-2 text-[10px] text-text3">
            {eventLabel(t, ev)}
          </span>
        ))}
      </div>
      <div className="hidden flex-shrink-0 items-center gap-1.5 group-hover:flex">
        <button type="button" className="btn ghost sm" disabled={testing} onClick={onTest}>
          {testing ? <Spinner /> : null}
          {testing ? t('set.notify.testing') : t('set.notify.test')}
        </button>
        <button type="button" className="btn ghost sm" onClick={onLogs}>
          {t('set.notify.deliveryLog')}
        </button>
        <button type="button" className="btn ghost sm" onClick={onEdit}>
          {t('set.notify.edit')}
        </button>
        {/* 与桌面端一致：四个行内操作都是中性色，删除的危险信号交给二次确认弹窗。
            （`.btn.ghost` 自带 color，写 `text-danger` 也压不过它。） */}
        <button type="button" className="btn ghost sm" onClick={onDelete}>
          {t('set.notify.delete')}
        </button>
      </div>
      <div className={cn('flex w-[180px] flex-shrink-0 items-center justify-end gap-1.5 text-[11px]', healthTone)}>
        <span className="size-1.5 flex-shrink-0 rounded-full" style={{ background: dotColor }} />
        <span className="truncate">{healthText}</span>
      </div>
      <SetSwitch checked={endpoint.enabled} onCheckedChange={onToggle} />
    </div>
  )
}
