// 投递日志对话框 —— 「为什么没收到」的唯一产品内答案。
// 行点击展开请求头/请求体/响应片段；底部「模拟一次 task.completed」让用户
// 配完端点无需真实下载即可端到端验证。

import * as Dialog from '@radix-ui/react-dialog'
import { X } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { api } from '../../lib/api'
import { cn } from '../../lib/cn'
import { useI18n } from '../../lib/i18n'
import { toast } from '../../lib/toast'
import type { WebhookDelivery, WebhookEndpoint } from '../../lib/types'
import { Spinner } from './Spinner'
import { eventLabel } from './WebhookEndpointDialog'

function clock(ms: number): string {
  const d = new Date(ms)
  const two = (v: number) => String(v).padStart(2, '0')
  return `${two(d.getHours())}:${two(d.getMinutes())}:${two(d.getSeconds())}`
}

export function WebhookDeliveryDialog({
  open,
  onOpenChange,
  deliveries,
  endpoints,
  endpointId,
  onEndpointIdChange,
  onRefresh,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  deliveries: WebhookDelivery[]
  /** 已配置的推送目标，用于把记录里的 ID 翻成名字 + 驱动筛选下拉。 */
  endpoints: WebhookEndpoint[]
  /** 空 = 全部目标。 */
  endpointId: string
  onEndpointIdChange: (id: string) => void
  onRefresh: () => void
}) {
  const { t } = useI18n()
  const [expanded, setExpanded] = useState<string>('')
  const [simulating, setSimulating] = useState(false)
  const [noTarget, setNoTarget] = useState(false)
  /** 还差几条模拟投递没落库（受理回执给的目标数 - 已到的新记录数）。 */
  const [pending, setPending] = useState(0)
  /** 发起模拟那一刻已有的记录 id：新记录 = 不在这个集合里的。 */
  const baseline = useRef<Set<string>>(new Set())
  const busy = simulating || pending > 0
  const rows = endpointId === '' ? deliveries : deliveries.filter((d) => d.endpointId === endpointId)

  /** 目标改名后旧记录里的名字会过期，以当前配置为准，查不到（已删）再回落。 */
  function nameOf(id: string): string {
    const ep = endpoints.find((e) => e.id === id)
    if (ep) return ep.name || ep.url
    return deliveries.find((d) => d.endpointId === id)?.endpointName || id
  }

  async function clear() {
    try {
      await api.clearWebhookDeliveries()
      onRefresh()
    } catch (err) {
      toast(err instanceof Error ? err.message : String(err), 'error')
    }
  }

  /**
   * 「模拟一次下载完成」。
   *
   * `POST /simulate` 的回执只说**投出去几个**，不代表投完了 —— 真正耗时的
   * 是后面的 HTTP（最坏 4 次尝试 × 10s 超时 + 2/4/8s 退避 ≈ 54s）。所以
   * 转圈不能跟着请求走，得跟着「还差几条新记录落库」走。
   */
  async function simulate() {
    // 连点会在对端刷出一串通知：一次点击 = 一次投递。
    if (busy) return
    setSimulating(true)
    setNoTarget(false)
    setPending(0)
    try {
      const res = await api.simulateWebhook()
      // 0 = 没有目标订阅「完成」事件，干等投递记录是等不到的，直接说明白。
      setNoTarget(res.dispatched === 0)
      if (res.dispatched > 0) {
        baseline.current = new Set(deliveries.map((d) => d.deliveryId))
        setPending(res.dispatched)
      }
    } catch (err) {
      toast(err instanceof Error ? err.message : String(err), 'error')
    } finally {
      setSimulating(false)
    }
  }

  // 新记录够数就收工；兜底 75s，超时还没齐就当它丢了，不能一直转。
  useEffect(() => {
    if (pending === 0) return
    const fresh = deliveries.filter((d) => !baseline.current.has(d.deliveryId)).length
    if (fresh >= pending) {
      setPending(0)
      return
    }
    const timer = setTimeout(() => setPending(0), 75_000)
    return () => clearTimeout(timer)
  }, [deliveries, pending])

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="wbackdrop show" />
        <Dialog.Content asChild>
          <div className="dialog show fields-compact" style={{ width: 560 }}>
            <header className="dlg-head">
              <Dialog.Title asChild>
                <b>{t('set.notify.deliveryLog')}</b>
              </Dialog.Title>
              <Dialog.Close asChild>
                <button type="button" className="icon-btn sm" aria-label={t('common.close')}>
                  <X size={16} />
                </button>
              </Dialog.Close>
            </header>
            <Dialog.Description className="dlg-sub">{t('set.notify.logSubtitle')}</Dialog.Description>
            <div className="dlg-body thin-scroll" style={{ maxHeight: 460, overflowY: 'auto' }}>
              {endpoints.length > 0 ? (
                <div className="mb-3 flex items-center gap-2">
                  <span className="flex-shrink-0 text-[11.5px] text-text3">
                    {t('set.notify.logFilterLabel')}
                  </span>
                  <select
                    className="select w-[220px]"
                    value={endpointId}
                    onChange={(e) => onEndpointIdChange(e.target.value)}
                  >
                    <option value="">{t('set.notify.logFilterAll')}</option>
                    {endpoints.map((e) => (
                      <option key={e.id} value={e.id}>
                        {nameOf(e.id)}
                      </option>
                    ))}
                  </select>
                </div>
              ) : null}
              {busy ? (
                // 请求真在飞（超时 10s + 重试 3 次）。空列表配一个不动的按钮
                // 只会让人以为点坏了。
                <div className="mb-2 flex items-center gap-2 rounded-lg border border-line px-3 py-2 text-[11.5px] text-text2">
                  <Spinner />
                  {t('set.notify.logPending')}
                </div>
              ) : null}
              {rows.length === 0 && !busy ? (
                <p className="set-desc">{t('set.notify.logEmpty')}</p>
              ) : (
                <div className="flex flex-col">
                  {rows.map((d) => {
                    const open4xx = d.statusCode >= 400 && d.statusCode < 500
                    const isOpen = expanded === d.deliveryId
                    return (
                      // 详情块**不能**在 <button> 里：浏览器不让在按钮内部拖选，
                      // 而这块内容（请求头 / 载荷 / 响应）正是用户要复制走排查的。
                      // 只有头部一行是折叠开关。
                      <div key={d.deliveryId} className="border-line border-b py-2 last:border-b-0">
                        <button
                          type="button"
                          className="flex w-full items-center gap-2 text-left"
                          onClick={() => setExpanded(isOpen ? '' : d.deliveryId)}
                        >
                          <span className="font-mono text-[10.5px] text-text3">{clock(d.timestampMs)}</span>
                          <span
                            className={cn(
                              'rounded border px-1.5 font-mono text-[9.5px]',
                              d.event === 'task.completed'
                                ? 'border-success/40 bg-success/10 text-success'
                                : d.event === 'task.failed'
                                  ? 'border-danger/40 bg-danger/10 text-danger'
                                  : 'border-accent/40 bg-accent-weak text-accent',
                            )}
                          >
                            {eventLabel(t, d.event)}
                          </span>
                          <span className="min-w-0 flex-1 truncate text-[11.5px] text-text2">{d.endpointName}</span>
                          <span
                            className={cn('font-mono text-[10.5px]', d.success ? 'text-success' : 'text-danger')}
                          >
                            {d.success
                              ? `${d.statusCode} · ${d.latencyMs}ms`
                              : `${d.statusCode > 0 ? d.statusCode : d.error} · ${t('set.notify.attempts', { n: d.attempts })}`}
                          </span>
                        </button>
                        {isOpen ? (
                          <pre className="mt-2 cursor-text select-text whitespace-pre-wrap break-all rounded-lg border border-line bg-bg p-2.5 font-mono text-[10px] text-text3 leading-relaxed">
                            {`POST ${d.url}\n${d.requestHeaders}\n\n${d.requestBody}` +
                              (d.statusCode > 0 ? `\n\n← ${d.statusCode}\n${d.responseBody}` : '') +
                              (d.statusCode === 0 && d.error !== '' ? `\n\n← ${d.error}` : '') +
                              (open4xx ? `\n\n${t('set.notify.logHint4xx')}` : '')}
                          </pre>
                        ) : null}
                      </div>
                    )
                  })}
                </div>
              )}
            </div>
            <footer className="dlg-foot" style={{ justifyContent: 'flex-start' }}>
              <button type="button" className="btn plain sm" onClick={() => void clear()}>
                {t('set.notify.logClear')}
              </button>
              {/* 「模拟一次投递」光看按钮不知道会发生什么，旁边一句话说清楚。 */}
              <span
                className={cn(
                  'min-w-0 flex-1 text-[10.5px] leading-snug',
                  noTarget ? 'text-warning' : 'text-text3',
                )}
              >
                {noTarget ? t('set.notify.simulateNoTarget') : t('set.notify.logSimulateHint')}
              </span>
              <button
                type="button"
                className="btn ghost sm"
                disabled={busy}
                onClick={() => void simulate()}
              >
                {busy ? <Spinner /> : null}
                {busy ? t('set.notify.logPending') : t('set.notify.logSimulate')}
              </button>
            </footer>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
