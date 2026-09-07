// RSS 订阅管理/新建对话框 —— 对齐 queue-manager-dialog.tsx 的三 Tab 手绘（qtab-bar）结构：
// - 基本：名称 / 订阅链接（可「验证」拉 feed 标题与条目预览）/ 启用 / 目标队列 /
//   保存目录 / 抓取间隔 / 自动下载 / 新任务以暂停创建；
// - 过滤规则：包含·排除关键词 + 体积上下限 + 正则·智能去重开关，下半屏对已缓存条目
//   实时试跑规则（lib/rss-filter.ts，与引擎 filter.rs 逐条对齐——预览分叉就是骗人）；
// - 高级：Cookie / UA / 独立代理 / 每轮最多新建任务数 / 携带 Referer / 自动下载通知。
// 三 Tab 经「确定」一次提交（新建 → createRssSource，管理 → updateRssSource）。
// 表单值在 open 边沿由整体重挂载（key）从订阅当前快照初始化。

import { useState, type ReactNode } from 'react'
import * as Dialog from '@radix-ui/react-dialog'
import { useQuery } from '@tanstack/react-query'
import { CheckCircle2, CircleAlert, Loader2, Settings2, X } from 'lucide-react'
import { OverflowTooltip } from '../OverflowTooltip'
import { api } from '../../lib/api'
import { cn } from '../../lib/cn'
import { fmtBytes, queueDisplayName } from '../../lib/format'
import { useI18n, type I18nKey } from '../../lib/i18n'
import { formatSize, parseSize, previewRule, reasonText, sourceDisplayName } from '../../lib/rss-filter'
import type { RssItemDto, RssSourceDto } from '../../lib/types'
import { SelectField } from '../dialogs/select-field'
import { FsPicker } from '../dialogs/fs-picker'
import { SetSwitch } from '../settings/controls'
import {
  beginRssFetch,
  useCreateRssSourceMutation,
  useRssItemsQuery,
  useUpdateRssSourceMutation,
  useValidateRssFeedMutation,
} from '../../hooks/useRss'
import { useTasksUi } from './context'

/** 抓取间隔下拉：分钟数 + 文案键（`{n}` 由渲染处代入）。 */
const INTERVAL_OPTIONS: { minutes: number; key: I18nKey; n: number }[] = [
  { minutes: 10, key: 'rss.intervalMinutes', n: 10 },
  { minutes: 30, key: 'rss.intervalMinutes', n: 30 },
  { minutes: 60, key: 'rss.intervalHours', n: 1 },
  { minutes: 120, key: 'rss.intervalHours', n: 2 },
  { minutes: 360, key: 'rss.intervalHours', n: 6 },
  { minutes: 720, key: 'rss.intervalHours', n: 12 },
  { minutes: 1440, key: 'rss.intervalHours', n: 24 },
]

const TAB_KEYS: I18nKey[] = ['rss.tabBasic', 'rss.tabFilter', 'rss.tabAdvanced']

/** 引擎侧的新建默认值（RssSourceInfo::default）：间隔 30 分钟、每轮 20 条、
 *  启用/自动下载/携带 Referer/通知均开，智能去重默认关。 */
const NEW_SOURCE: RssSourceDto = {
  sourceId: '',
  url: '',
  name: '',
  enabled: true,
  autoDownload: true,
  startPaused: false,
  queueId: '',
  saveDir: '',
  intervalMinutes: 30,
  includePattern: '',
  excludePattern: '',
  useRegex: false,
  smartEpisode: false,
  sizeMinBytes: 0,
  sizeMaxBytes: 0,
  sendReferer: true,
  notifyOnDownload: true,
  maxPerFetch: 20,
  cookies: '',
  userAgent: '',
  proxyUrl: '',
  lastFetchAt: 0,
  lastSuccessAt: 0,
  lastError: '',
  failCount: 0,
  seeded: false,
  position: 0,
  unreadCount: 0,
}

/** 表单态：体积与条数用文本框承载（`200M` / `2G` 字面量经 parseSize 往返）。 */
interface RssForm {
  name: string
  url: string
  enabled: boolean
  autoDownload: boolean
  startPaused: boolean
  queueId: string
  saveDir: string
  intervalMinutes: number
  includePattern: string
  excludePattern: string
  useRegex: boolean
  smartEpisode: boolean
  sizeMin: string
  sizeMax: string
  cookies: string
  userAgent: string
  proxyUrl: string
  maxPerFetch: string
  sendReferer: boolean
  notifyOnDownload: boolean
}

function formOf(s: RssSourceDto): RssForm {
  return {
    name: s.name,
    url: s.url,
    enabled: s.enabled,
    autoDownload: s.autoDownload,
    startPaused: s.startPaused,
    queueId: s.queueId,
    saveDir: s.saveDir,
    intervalMinutes: s.intervalMinutes > 0 ? s.intervalMinutes : 30,
    includePattern: s.includePattern,
    excludePattern: s.excludePattern,
    useRegex: s.useRegex,
    smartEpisode: s.smartEpisode,
    sizeMin: formatSize(s.sizeMinBytes),
    sizeMax: formatSize(s.sizeMaxBytes),
    cookies: s.cookies,
    userAgent: s.userAgent,
    proxyUrl: s.proxyUrl,
    maxPerFetch: String(s.maxPerFetch > 0 ? s.maxPerFetch : 20),
    sendReferer: s.sendReferer,
    notifyOnDownload: s.notifyOnDownload,
  }
}

/** 管理既有订阅。`trigger` 省略时用侧边栏 hover 操作簇里的齿轮按钮；条目流的
 *  「检查配置」入口传自己的按钮进来复用同一份表单。 */
export function RssManagerDialog({ source, trigger }: { source: RssSourceDto; trigger?: ReactNode }) {
  const { t } = useI18n()
  const [open, setOpen] = useState(false)
  return (
    <Dialog.Root open={open} onOpenChange={setOpen}>
      <Dialog.Trigger asChild>
        {trigger ?? (
          <button type="button" className="icon-btn sm" title={t('rss.manage')} onClick={(e) => e.stopPropagation()}>
            <Settings2 size={13} />
          </button>
        )}
      </Dialog.Trigger>
      <RssDialogContent key={String(open)} source={source} open={open} setOpen={setOpen} />
    </Dialog.Root>
  )
}

/** 新建订阅：触发器在侧边栏区块标题的「＋」上，故由外部控制开合。 */
export function RssCreateDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (v: boolean) => void }) {
  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <RssDialogContent key={String(open)} open={open} setOpen={onOpenChange} />
    </Dialog.Root>
  )
}

function RssDialogContent({
  source,
  open,
  setOpen,
}: {
  /** 缺省 = 新建。 */
  source?: RssSourceDto
  open: boolean
  setOpen: (v: boolean) => void
}) {
  const { t } = useI18n()
  const [tab, setTab] = useState<0 | 1 | 2>(0)
  const [form, setForm] = useState<RssForm>(() => formOf(source ?? NEW_SOURCE))
  const [urlError, setUrlError] = useState('')
  const [validated, setValidated] = useState<{ feedTitle: string; items: RssItemDto[]; error: string } | null>(null)
  const { data: queues = [] } = useQuery({ queryKey: ['queues'], queryFn: api.listQueues })
  // 条目流只在对话框打开时才拉：本组件为每条订阅常驻挂载（Portal 才是按需的），
  // 否则侧边栏有几条订阅就会在首屏打出几个 items 请求。
  const { data: cachedItems = [] } = useRssItemsQuery(open ? (source?.sourceId ?? '') : '')
  const { setRssFilter } = useTasksUi()
  const create = useCreateRssSourceMutation()
  const update = useUpdateRssSourceMutation()
  const validate = useValidateRssFeedMutation()
  const saving = create.isPending || update.isPending

  // 表单不设「随 source 重置」的 effect：引擎每轮抓取都会推 rssSourcesChanged
  // （lastFetchAt 变了），跟着重置会把用户正在编辑的内容冲掉。改由 open 变化时
  // 换 key 整体重挂载——useState 初值即当次打开时的订阅快照。

  const patch = (p: Partial<RssForm>) => setForm((f) => ({ ...f, ...p }))

  function buildDto(): RssSourceDto {
    return {
      ...(source ?? NEW_SOURCE),
      name: form.name.trim(),
      url: form.url.trim(),
      enabled: form.enabled,
      autoDownload: form.autoDownload,
      startPaused: form.startPaused,
      queueId: form.queueId,
      saveDir: form.saveDir.trim(),
      intervalMinutes: form.intervalMinutes,
      includePattern: form.includePattern,
      excludePattern: form.excludePattern,
      useRegex: form.useRegex,
      smartEpisode: form.smartEpisode,
      sizeMinBytes: parseSize(form.sizeMin) ?? 0,
      sizeMaxBytes: parseSize(form.sizeMax) ?? 0,
      sendReferer: form.sendReferer,
      notifyOnDownload: form.notifyOnDownload,
      maxPerFetch: Math.min(100, Math.max(1, Number(form.maxPerFetch.trim()) || 20)),
      cookies: form.cookies,
      userAgent: form.userAgent.trim(),
      proxyUrl: form.proxyUrl.trim(),
    }
  }

  async function submit() {
    if (!form.url.trim()) {
      setUrlError(t('rss.urlRequired'))
      setTab(0)
      return
    }
    const dto = buildDto()
    if (source) {
      await update.mutateAsync({ sourceId: source.sourceId, req: dto })
    } else {
      // 引擎建完订阅会立刻抓一轮。这里同步把它标成「抓取中」并切到它的条目流：
      // 否则用户看到的是一条名字还是主机名、写着「尚未抓取」的空列表，既分不清
      // 是在跑还是已经失败，也不知道该等还是该去检查配置。
      const { sourceId } = await create.mutateAsync(dto)
      beginRssFetch(sourceId, 0)
      setRssFilter(sourceId)
    }
    setOpen(false)
  }

  /** 验证是一次纯诊断调用：抓取失败也是 200，错误进 error 字段照常展示。 */
  async function runValidate() {
    const url = form.url.trim()
    if (!url) {
      setUrlError(t('rss.urlRequired'))
      return
    }
    setUrlError('')
    const res = await validate.mutateAsync({
      url,
      cookies: form.cookies,
      userAgent: form.userAgent.trim(),
      proxyUrl: form.proxyUrl.trim(),
    })
    setValidated({ feedTitle: res.feedTitle, items: res.items, error: res.error })
    // 名称留空时用 feed 标题回填（与引擎侧的回填规则一致）。
    if (!res.error && !form.name.trim() && res.feedTitle) patch({ name: res.feedTitle })
  }

  // 预览样本：既有订阅用已缓存条目，新建订阅用刚验证回来的条目。
  const previewItems = cachedItems.length > 0 ? cachedItems : (validated?.items ?? [])
  const rows = previewRule(
    {
      include: form.includePattern,
      exclude: form.excludePattern,
      useRegex: form.useRegex,
      smartEpisode: form.smartEpisode,
      sizeMinBytes: parseSize(form.sizeMin) ?? 0,
      sizeMaxBytes: parseSize(form.sizeMax) ?? 0,
    },
    previewItems,
  )
  const hitCount = rows.filter((r) => r.verdict.accepted).length

  const queueOptions = [
    { value: '', label: t('rss.queueDefault') },
    ...queues.map((q) => ({ value: q.queueId, label: queueDisplayName(q) })),
  ]

  return (
    <Dialog.Portal>
      <Dialog.Overlay className="wbackdrop show" />
      <Dialog.Content asChild onClick={(e) => e.stopPropagation()} onPointerDownOutside={(e) => e.preventDefault()}>
        <div className="dialog show" style={{ width: 520 }}>
          <header className="dlg-head">
            <Dialog.Title asChild>
              <b className="flex items-center gap-2">
                {source ? sourceDisplayName(source) : t('rss.newSource')}
                {source && (
                  <span className={cn('run-badge', source.enabled && !source.lastError && 'on')}>
                    <i className={cn('queue-dot', source.enabled && !source.lastError && 'on')} />
                    {source.lastError ? t('rss.stateError') : source.enabled ? t('rss.stateEnabled') : t('rss.stateDisabled')}
                  </span>
                )}
              </b>
            </Dialog.Title>
            <Dialog.Close asChild>
              <button type="button" className="icon-btn sm" aria-label={t('common.close')}>
                <X size={16} />
              </button>
            </Dialog.Close>
          </header>
          <Dialog.Description className="sr-only">{t('rss.manage')}</Dialog.Description>
          <div className="dlg-body">
            <div className="qtab-bar">
              {TAB_KEYS.map((key, i) => (
                <button key={key} type="button" className={cn('qtab', tab === i && 'active')} onClick={() => setTab(i as 0 | 1 | 2)}>
                  {t(key)}
                </button>
              ))}
            </div>

            {tab === 0 && (
              <>
                <label className="field-label" htmlFor="rss-url">{t('rss.urlLabel')}</label>
                <div className="dir-row">
                  <input
                    id="rss-url"
                    className="text-input"
                    spellCheck={false}
                    value={form.url}
                    placeholder={t('rss.urlHint')}
                    onChange={(e) => { patch({ url: e.target.value }); setUrlError(''); setValidated(null) }}
                  />
                  <button type="button" className="btn ghost" disabled={validate.isPending} onClick={() => void runValidate()}>
                    {validate.isPending ? <Loader2 size={13} className="animate-spin" /> : <CheckCircle2 size={13} />}
                    {t('rss.validate')}
                  </button>
                </div>
                {urlError && <p className="sched-summary warn"><CircleAlert size={13} /><span>{urlError}</span></p>}
                {validated && (
                  <p className={cn('sched-summary', validated.error && 'warn')}>
                    {validated.error ? <CircleAlert size={13} /> : <CheckCircle2 size={13} />}
                    <span>
                      {validated.error
                        ? t('rss.validateFailed', { error: validated.error })
                        : t('rss.validateOk', { title: validated.feedTitle || t('common.unknown'), count: validated.items.length })}
                    </span>
                  </p>
                )}
                <div className="grid2">
                  <div>
                    <label className="field-label" htmlFor="rss-name">{t('rss.nameLabel')}</label>
                    <input
                      id="rss-name"
                      className="text-input"
                      value={form.name}
                      placeholder={t('rss.nameHint')}
                      onChange={(e) => patch({ name: e.target.value })}
                    />
                  </div>
                  <div>
                    <label className="field-label">{t('rss.interval')}</label>
                    <SelectField
                      value={String(form.intervalMinutes)}
                      onChange={(v) => patch({ intervalMinutes: Number(v) })}
                      options={INTERVAL_OPTIONS.map((o) => ({ value: String(o.minutes), label: t(o.key, { n: o.n }) }))}
                      ariaLabel={t('rss.interval')}
                    />
                  </div>
                </div>
                <div className="grid2">
                  <div>
                    <label className="field-label">{t('rss.targetQueue')}</label>
                    <SelectField value={form.queueId} onChange={(v) => patch({ queueId: v })} options={queueOptions} ariaLabel={t('rss.targetQueue')} />
                  </div>
                  <div>
                    <label className="field-label" htmlFor="rss-dir">{t('rss.saveDir')}</label>
                    <div className="dir-row">
                      <input
                        id="rss-dir"
                        className="text-input"
                        spellCheck={false}
                        value={form.saveDir}
                        placeholder={t('rss.saveDirHint')}
                        onChange={(e) => patch({ saveDir: e.target.value })}
                      />
                      <FsPicker value={form.saveDir} onChange={(v) => patch({ saveDir: v })} />
                    </div>
                  </div>
                </div>
                <div className="set-row" style={{ padding: '4px 0' }}>
                  <div className="set-info">
                    <b>{t('rss.enabled')}</b>
                    <span>{t('rss.enabledDesc')}</span>
                  </div>
                  <SetSwitch checked={form.enabled} onCheckedChange={(v) => patch({ enabled: v })} />
                </div>
                <div className="set-row" style={{ padding: '4px 0' }}>
                  <div className="set-info">
                    <b>{t('rss.autoDownload')}</b>
                    <span>{t('rss.autoDownloadDesc')}</span>
                  </div>
                  <SetSwitch checked={form.autoDownload} onCheckedChange={(v) => patch({ autoDownload: v })} />
                </div>
                <div className="set-row" style={{ padding: '4px 0' }}>
                  <div className="set-info">
                    <b>{t('rss.startPaused')}</b>
                    <span>{t('rss.startPausedDesc')}</span>
                  </div>
                  <SetSwitch checked={form.startPaused} onCheckedChange={(v) => patch({ startPaused: v })} />
                </div>
              </>
            )}

            {tab === 1 && (
              <>
                <div className="grid2">
                  <div>
                    <label className="field-label" htmlFor="rss-inc">{t('rss.include')}</label>
                    <input
                      id="rss-inc"
                      className="text-input"
                      spellCheck={false}
                      value={form.includePattern}
                      placeholder={t('rss.includeHint')}
                      onChange={(e) => patch({ includePattern: e.target.value })}
                    />
                  </div>
                  <div>
                    <label className="field-label" htmlFor="rss-exc">{t('rss.exclude')}</label>
                    <input
                      id="rss-exc"
                      className="text-input"
                      spellCheck={false}
                      value={form.excludePattern}
                      placeholder={t('rss.excludeHint')}
                      onChange={(e) => patch({ excludePattern: e.target.value })}
                    />
                  </div>
                </div>
                <div className="grid2">
                  <div>
                    <label className="field-label" htmlFor="rss-min">{t('rss.sizeMin')}</label>
                    <input
                      id="rss-min"
                      className="text-input"
                      spellCheck={false}
                      value={form.sizeMin}
                      placeholder="200M"
                      onChange={(e) => patch({ sizeMin: e.target.value })}
                    />
                  </div>
                  <div>
                    <label className="field-label" htmlFor="rss-max">{t('rss.sizeMax')}</label>
                    <input
                      id="rss-max"
                      className="text-input"
                      spellCheck={false}
                      value={form.sizeMax}
                      placeholder="2G"
                      onChange={(e) => patch({ sizeMax: e.target.value })}
                    />
                  </div>
                </div>
                <div className="set-row" style={{ padding: '4px 0' }}>
                  <div className="set-info">
                    <b>{t('rss.useRegex')}</b>
                    <span>{t('rss.useRegexDesc')}</span>
                  </div>
                  <SetSwitch checked={form.useRegex} onCheckedChange={(v) => patch({ useRegex: v })} />
                </div>
                <div className="set-row" style={{ padding: '4px 0' }}>
                  <div className="set-info">
                    <b>{t('rss.smartEpisode')}</b>
                    <span>{t('rss.smartEpisodeDesc')}</span>
                  </div>
                  <SetSwitch checked={form.smartEpisode} onCheckedChange={(v) => patch({ smartEpisode: v })} />
                </div>
                <div className="rss-preview">
                  <div className="rss-preview-head">
                    <span>{t('rss.previewTitle', { count: previewItems.length })}</span>
                    {previewItems.length > 0 && (
                      <span>
                        <b className="ok">{t('rss.previewHit', { count: hitCount })}</b>
                        {' · '}
                        {t('rss.previewMiss', { count: previewItems.length - hitCount })}
                      </span>
                    )}
                  </div>
                  {previewItems.length === 0 ? (
                    <p className="qorder-empty">{t('rss.previewEmpty')}</p>
                  ) : (
                    <div className="rss-preview-list">
                      {rows.map(({ item, verdict }) => (
                        <div key={item.guid} className={cn('rss-preview-row', !verdict.accepted && 'miss')}>
                          <span className={cn('rss-mark', verdict.accepted ? 'y' : 'n')}>
                            {verdict.accepted ? t('rss.willDownload') : t('rss.willFilter')}
                          </span>
                          <OverflowTooltip className="rss-preview-name" text={item.title} />
                          <span className="rss-preview-why">
                            {verdict.accepted
                              ? item.enclosureLength > 0
                                ? fmtBytes(item.enclosureLength)
                                : ''
                              : reasonText(verdict.reason, verdict.episodeKey)}
                          </span>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              </>
            )}

            {tab === 2 && (
              <>
                <label className="field-label" htmlFor="rss-cookies">{t('rss.cookies')}</label>
                <textarea
                  id="rss-cookies"
                  className="text-input area"
                  rows={2}
                  spellCheck={false}
                  value={form.cookies}
                  placeholder="key=value; key2=value2"
                  onChange={(e) => patch({ cookies: e.target.value })}
                />
                <p className="field-hint">{t('rss.cookiesDesc')}</p>
                <div className="grid2">
                  <div>
                    <label className="field-label" htmlFor="rss-ua">{t('rss.userAgent')}</label>
                    <input
                      id="rss-ua"
                      className="text-input"
                      spellCheck={false}
                      value={form.userAgent}
                      placeholder={t('rss.userAgentHint')}
                      onChange={(e) => patch({ userAgent: e.target.value })}
                    />
                  </div>
                  <div>
                    <label className="field-label" htmlFor="rss-proxy">{t('rss.proxyUrl')}</label>
                    <input
                      id="rss-proxy"
                      className="text-input"
                      spellCheck={false}
                      value={form.proxyUrl}
                      placeholder="socks5://127.0.0.1:1080"
                      onChange={(e) => patch({ proxyUrl: e.target.value })}
                    />
                  </div>
                </div>
                <label className="field-label" htmlFor="rss-max-fetch">{t('rss.maxPerFetch')}</label>
                <input
                  id="rss-max-fetch"
                  className="text-input"
                  inputMode="numeric"
                  style={{ width: 120 }}
                  value={form.maxPerFetch}
                  onChange={(e) => patch({ maxPerFetch: e.target.value })}
                />
                <p className="field-hint">{t('rss.maxPerFetchDesc')}</p>
                <div className="set-row" style={{ padding: '4px 0', marginTop: 6 }}>
                  <div className="set-info">
                    <b>{t('rss.sendReferer')}</b>
                    <span>{t('rss.sendRefererDesc')}</span>
                  </div>
                  <SetSwitch checked={form.sendReferer} onCheckedChange={(v) => patch({ sendReferer: v })} />
                </div>
                <div className="set-row" style={{ padding: '4px 0' }}>
                  <div className="set-info">
                    <b>{t('rss.notifyOnDownload')}</b>
                    <span>{t('rss.notifyOnDownloadDesc')}</span>
                  </div>
                  <SetSwitch checked={form.notifyOnDownload} onCheckedChange={(v) => patch({ notifyOnDownload: v })} />
                </div>
              </>
            )}
          </div>
          <footer className="dlg-foot">
            <Dialog.Close asChild>
              <button type="button" className="btn ghost">{t('common.cancel')}</button>
            </Dialog.Close>
            <button type="button" className="btn primary" disabled={saving} onClick={() => void submit()}>
              {saving ? t('common.loading') : source ? t('common.confirm') : t('rss.subscribe')}
            </button>
          </footer>
        </div>
      </Dialog.Content>
    </Dialog.Portal>
  )
}
