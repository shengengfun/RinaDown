// RSS 条目流：侧边栏选中某订阅时接管中央主区（与任务列表互斥，见 routes/tasks.tsx）。
// 视觉与交互逐条对齐桌面 `lib/src/widgets/rss_item_list.dart`——同一个产品不该有两套
// 条目列表：订阅头与动作合并成一行、条目是平铺行而非卡片、动作 hover 才出现。
//
// 状态与可用操作（RssItemDto.status，值域见 lib/types.ts）：
//   0 新       → 「下载」「忽略」
//   1 已下载   → chip 可点，跳到对应任务（taskId 回链）；动作为「重新下载」
//   2/3/4/5    → 「下载」（download 会绕过规则强制建任务）
// reason 是稳定原因码，一律经 lib/rss-filter.ts 的 reasonText() 映射为人读文案。

import { useState } from 'react'
import { ArrowDownWideNarrow, ArrowUpNarrowWide, CircleAlert, RefreshCw, CheckCheck, Loader2, Radio, Search, Settings2 } from 'lucide-react'
import { OverflowTooltip } from '../OverflowTooltip'
import { ApiError } from '../../lib/api'
import { cn } from '../../lib/cn'
import { fmtBytes, fmtRelativeUnix, fmtShortTime } from '../../lib/format'
import { useI18n, type I18nKey } from '../../lib/i18n'
import { reasonText, sourceDisplayName } from '../../lib/rss-filter'
import { toast } from '../../lib/toast'
import type { RssItemDto, RssSourceDto } from '../../lib/types'
import {
  beginRssFetch,
  useRefreshRssSourceMutation,
  useRssFetching,
  useRssItemActionMutation,
  useRssItemsQuery,
} from '../../hooks/useRss'
import { useTasksUi } from './context'
import { RssManagerDialog } from './rss-manager-dialog'

/** 状态 → chip 配色（对齐任务行的 done/err/pause 语义色）。 */
const CHIP_CLASS: Record<number, string> = {
  0: 'new',
  1: 'done',
  2: 'skip',
  3: 'skip',
  4: 'dup',
  5: 'skip',
}

/** 会进 chip 的原因码：只有「规则为什么拦下它」值得补一句。
 *  `seed_skipped` 不在内——chip 本身已经写着「历史条目」，再跟一句
 *  「首轮抓取的历史条目」是同义反复。 */
const RULE_REASONS = new Set(['not_included', 'excluded', 'too_small', 'too_large', 'dup_episode'])

/** 抓取间隔摘要：整小时的间隔说「每 N 小时」，否则说分钟（1440 分钟不好读）。 */
function intervalText(minutes: number, t: (k: I18nKey, p?: Record<string, string | number>) => string): string {
  return minutes >= 60 && minutes % 60 === 0
    ? t('rss.everyHours', { n: minutes / 60 })
    : t('rss.everyMinutes', { n: minutes })
}

/** 失败提示用的错误文案：ApiError 的 message 已由 api.ts 本地化过，
 *  直接 String(err) 会多出一层 "ApiError:" 前缀，用户读到的是噪音。 */
function errorText(err: unknown): string {
  if (err instanceof ApiError) return err.message
  return err instanceof Error ? err.message : String(err)
}

export function RssItemList({ source }: { source: RssSourceDto }) {
  const { t } = useI18n()
  const { setRssFilter, selectTask } = useTasksUi()
  const [search, setSearch] = useState('')
  // 排序方向。默认新→旧：引擎回来的快照本来就是这个顺序（DB `ORDER BY pub_date DESC`），
  // 追番看的永远是最新一集。
  const [oldestFirst, setOldestFirst] = useState(false)
  const { data: items = [], isPending } = useRssItemsQuery(source.sourceId)
  const refresh = useRefreshRssSourceMutation()
  // 抓取是异步派发：mutation 的 isPending 只覆盖那一次 POST，真正的完成信号来自
  // 引擎回写 lastFetchAt 后的广播（见 hooks/useRss.ts 的 rssFetchingStore）。
  const fetching = useRssFetching(source.sourceId) || refresh.isPending
  const action = useRssItemActionMutation()
  // 「正在处理」精确到条目：整列按钮一起变灰会让人以为点错了行。
  const busyGuid = action.isPending ? (action.variables?.req.guid ?? '') : ''

  // 建任务是异步的：POST 返回只代表引擎收到了请求，而用户点完往往立刻移开鼠标，
  // 行内 spinner 一闪而过等于没反馈——结果一律用全局 toast 说清楚。
  const act = (item: RssItemDto, kind: 'download' | 'ignore') =>
    action.mutate(
      { sourceId: source.sourceId, req: { guid: item.guid, action: kind } },
      {
        onSuccess: () =>
          toast(kind === 'download' ? t('rss.toastQueued', { title: item.title }) : t('rss.toastIgnored')),
        onError: (err) => toast(t('rss.toastFailed', { error: errorText(err) }), 'error'),
      },
    )

  /** 「已下载」chip：回到任务列表并选中回链任务（条目流与任务列表互斥占用主区）。 */
  function jumpToTask(taskId: string) {
    if (!taskId) return
    setRssFilter(null)
    selectTask(taskId)
  }

  const q = search.trim().toLowerCase()
  const filtered = q ? items.filter((i) => i.title.toLowerCase().includes(q)) : items
  // 缺发布时间（pubDate === 0）的条目一律沉底，不让它们污染时间轴两端；同一集不同
  // 字幕组常常同秒发布，靠 JS sort 的稳定性（ES2019 起有保证）钉住原顺序，行才不会
  // 每次重渲染乱跳。桌面侧 `List.sort` 不稳定，故那边额外用原始下标做 tie-break。
  const visible = [...filtered].sort((a, b) => {
    if ((a.pubDate === 0) !== (b.pubDate === 0)) return a.pubDate === 0 ? 1 : -1
    return oldestFirst ? a.pubDate - b.pubDate : b.pubDate - a.pubDate
  })
  const unread = items.filter((i) => i.status === 0).length
  const unhealthy = !!source.lastError

  return (
    <div className="rss-pane">
      {/* 订阅头 = 身份 + 健康度 + 这条订阅的动作，一行装下（见 design.css .rss-head）。 */}
      <header className="rss-head">
        <span className={cn('rss-head-icon', unhealthy && 'err')}>
          <Radio size={15} />
        </span>
        <div className="rss-head-main">
          <div className="rss-head-name">
            <OverflowTooltip as="b" text={sourceDisplayName(source)} />
            {unread > 0 && <span className="rss-chip new">{t('rss.unreadCount', { count: unread })}</span>}
          </div>
          {/* 一行说清「多久抓一次 / 上次怎么样 / 抓到了往哪放」。 */}
          <div className={cn('rss-head-meta', unhealthy && 'err')}>
            <span>{intervalText(source.intervalMinutes || 30, t)}</span>
            <span className="sep">·</span>
            <span>
              {fetching
                ? t('rss.refreshing')
                : unhealthy
                  ? t('rss.lastErrorAt', { error: source.lastError, count: source.failCount })
                  : source.lastSuccessAt > 0
                    ? t('rss.lastSuccessAt', { time: fmtRelativeUnix(source.lastSuccessAt) })
                    : t('rss.neverFetched')}
            </span>
            <span className="sep">·</span>
            <span>{source.autoDownload ? t('rss.modeAuto') : t('rss.modeCollect')}</span>
          </div>
        </div>

        <div className="rss-head-acts">
          {/* 排序：图标即当前方向（箭头朝下 = 新在上），点一下翻转。 */}
          <button
            type="button"
            className="icon-btn sm"
            title={oldestFirst ? t('rss.sortOldest') : t('rss.sortNewest')}
            onClick={() => setOldestFirst((v) => !v)}
          >
            {oldestFirst ? <ArrowUpNarrowWide size={15} /> : <ArrowDownWideNarrow size={15} />}
          </button>
          <button
            type="button"
            className="icon-btn sm"
            title={t('rss.markAllRead')}
            disabled={action.isPending || unread === 0}
            onClick={() =>
              action.mutate(
                { sourceId: source.sourceId, req: { guid: '', action: 'readAll' } },
                {
                  onSuccess: () => toast(t('rss.toastAllRead')),
                  onError: (err) => toast(t('rss.toastFailed', { error: errorText(err) }), 'error'),
                },
              )
            }
          >
            <CheckCheck size={15} />
          </button>
          {/* 抓取常要好几秒：没有进行中反馈用户会反复点或以为没生效。 */}
          <button
            type="button"
            className="icon-btn sm"
            title={fetching ? t('rss.refreshing') : t('rss.refreshNow')}
            disabled={fetching}
            onClick={() => { beginRssFetch(source.sourceId, source.lastFetchAt); refresh.mutate(source.sourceId) }}
          >
            {fetching ? <Loader2 size={15} className="animate-spin" /> : <RefreshCw size={15} />}
          </button>
        </div>

        <div className="search rss-search">
          <Search size={13} />
          <input
            type="text"
            placeholder={t('rss.searchPlaceholder')}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Escape') {
                setSearch('')
                e.currentTarget.blur()
              }
            }}
          />
        </div>
      </header>

      <div className="rss-scroll">
        {items.length === 0 ? (
          // 空列表有三种截然不同的处境，用同一段文案糊过去等于什么都没说：
          // 正在抓（等着就行）／抓失败了（要动手改配置）／抓成功但源里没东西。
          <div className={cn('rss-empty', unhealthy && !fetching && 'err')}>
            {fetching ? <Loader2 size={30} className="animate-spin" /> : unhealthy ? <CircleAlert size={30} /> : <Radio size={30} />}
            <b>{fetching ? t('rss.emptyFetching') : unhealthy ? t('rss.stateError') : isPending ? t('common.loading') : t('rss.emptyTitle')}</b>
            <span>{fetching ? t('rss.emptyFetchingHint') : unhealthy ? t('rss.emptyErrorHint') : t('rss.emptyHint')}</span>
            {unhealthy && !fetching && (
              <div className="rss-empty-acts">
                <button
                  type="button"
                  className="btn ghost sm"
                  onClick={() => { beginRssFetch(source.sourceId, source.lastFetchAt); refresh.mutate(source.sourceId) }}
                >
                  <RefreshCw size={13} />
                  {t('common.retry')}
                </button>
                <RssManagerDialog
                  source={source}
                  trigger={
                    <button type="button" className="btn ghost sm">
                      <Settings2 size={13} />
                      {t('rss.checkConfig')}
                    </button>
                  }
                />
              </div>
            )}
          </div>
        ) : visible.length === 0 ? (
          <p className="empty-tip">{t('rss.noMatch', { query: search.trim() })}</p>
        ) : (
          visible.map((item) => (
            <RssRow
              key={item.guid}
              item={item}
              busy={busyGuid === item.guid}
              onAct={act}
              onJump={jumpToTask}
            />
          ))
        )}
      </div>
    </div>
  )
}

function RssRow({
  item,
  busy,
  onAct,
  onJump,
}: {
  item: RssItemDto
  busy: boolean
  onAct: (item: RssItemDto, kind: 'download' | 'ignore') => void
  onJump: (taskId: string) => void
}) {
  const { t } = useI18n()
  const label =
    item.status === 0
      ? t('rss.statusNew')
      : item.status === 1
        ? t('rss.statusDownloaded')
        : item.status === 2
          ? t('rss.statusIgnored')
          : item.status === 4
            ? t('rss.statusDupEpisode')
            : item.status === 5
              ? t('rss.statusSeedSkipped')
              : t('rss.statusFiltered')
  const why = RULE_REASONS.has(item.reason) ? reasonText(item.reason, item.episodeKey) : ''
  const chipText = why ? `${label} · ${why}` : label
  const pub = fmtShortTime(item.pubDate)

  return (
    <div className="rss-row">
      <div className="rss-row-main">
        <div className="rss-row-name">
          <OverflowTooltip as="b" text={item.title} />
        </div>
        <div className="rss-row-meta">
          {pub && <span>{pub}</span>}
          {item.enclosureLength > 0 && <span>{fmtBytes(item.enclosureLength)}</span>}
        </div>
      </div>
      <div className="rss-row-status">
        {item.status === 1 && item.taskId ? (
          <button type="button" className="rss-chip done clickable" title={t('rss.jumpToTask')} onClick={() => onJump(item.taskId)}>
            {chipText}
          </button>
        ) : (
          <span className={cn('rss-chip', CHIP_CLASS[item.status])} title={chipText}>{chipText}</span>
        )}
      </div>
      <div className={cn('rss-row-ops', busy && 'busy')}>
        {busy ? (
          // 这一行正在被引擎处理（抓种子 / 建任务）：只留一个禁用态按钮，
          // 此刻「忽略」既没意义也容易误点。
          <button type="button" className="btn plain sm" disabled>
            <Loader2 size={12} className="animate-spin" />
            {t('rss.actionPreparing')}
          </button>
        ) : (
          <>
            {/* 被规则拦下的条目按钮同样只写「下载」：chip 已经把原因说清楚了，
                按钮再来一句「仍要下载」是在替用户犹豫。已下载的则给「重新下载」——
                任务可能被删了、下崩了，挡住重下只会逼用户去别处找种子。 */}
            <button type="button" className="btn plain sm" onClick={() => onAct(item, 'download')}>
              {t(item.status === 1 ? 'rss.actionRedownload' : 'rss.download')}
            </button>
            {item.status === 0 && (
              <button type="button" className="btn plain sm" onClick={() => onAct(item, 'ignore')}>
                {t('rss.ignore')}
              </button>
            )}
          </>
        )}
      </div>
    </div>
  )
}
