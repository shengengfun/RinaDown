// RSS 订阅共享的读写 hooks —— 对齐 hooks/usePlugins.ts。
// 订阅列表走 ['rss'] Query 缓存、条目流走 ['rss-items', sourceId]（WS rssSourcesChanged /
// rssItemsChanged 直接整表 setQueryData，见 lib/ws.ts）；各类写操作成功后 invalidate 相应
// 缓存兜底——引擎的广播是权威来源，invalidate 只负责补上「广播还没到」的那一帧。

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api } from '../lib/api'
import { Store, useStore } from '../lib/ws'
import type { RssItemActionRequest, RssSourceDto, RssValidateRequest } from '../lib/types'

// ---------------------------------------------------------------------------
// 「立即抓取」的进行中状态
// ---------------------------------------------------------------------------

/**
 * 正在抓取中的订阅：sourceId → 点击刷新那一刻的 `lastFetchAt` 快照。
 *
 * `POST /rss/{id}/refresh` 是**异步派发**，立刻返回、真正的抓取还要好几秒——
 * 拿 mutation 的 isPending 当判据，按钮会瞬间恢复可用，用户分不清「在跑」还是
 * 「点了没反应」。真正的完成信号是引擎回写并广播的 `lastFetchAt` 变化（成功与
 * 失败两条路径都会写），见 lib/ws.ts 的 rssSourcesChanged 分支。
 */
const rssFetchingStore = new Store<Record<string, number>>({})

/** 看门狗：判据失效（订阅被删、同秒内二次刷新导致 lastFetchAt 不变）时兜底解除，
 *  否则会永久转圈。 */
const fetchWatchdogs = new Map<string, ReturnType<typeof setTimeout>>()
const FETCH_WATCHDOG_MS = 45_000

/** 该订阅是否正在抓取。 */
export function useRssFetching(sourceId: string): boolean {
  return useStore(rssFetchingStore)[sourceId] !== undefined
}

/** 点击「立即抓取」时调用，`lastFetchAt` 传该源的当前值作为比较基准。 */
export function beginRssFetch(sourceId: string, lastFetchAt: number) {
  clearFetchWatchdog(sourceId)
  rssFetchingStore.set((prev) => ({ ...prev, [sourceId]: lastFetchAt }))
  fetchWatchdogs.set(sourceId, setTimeout(() => endRssFetch([sourceId]), FETCH_WATCHDOG_MS))
}

/** 收到全量订阅快照后结算：lastFetchAt 变过的、以及已经不在名单里的，都算结束。 */
export function settleRssFetch(sources: RssSourceDto[]) {
  const pending = rssFetchingStore.get()
  const ids = Object.keys(pending)
  if (ids.length === 0) return
  const byId = new Map(sources.map((s) => [s.sourceId, s.lastFetchAt]))
  endRssFetch(ids.filter((id) => byId.get(id) !== pending[id]))
}

/** 兜底：该源推来了条目变更，说明这一轮已经跑完。 */
export function settleRssFetchOne(sourceId: string) {
  endRssFetch([sourceId])
}

function endRssFetch(ids: string[]) {
  if (ids.length === 0) return
  for (const id of ids) clearFetchWatchdog(id)
  rssFetchingStore.set((prev) => {
    const next = { ...prev }
    let changed = false
    for (const id of ids) {
      if (id in next) {
        delete next[id]
        changed = true
      }
    }
    return changed ? next : prev
  })
}

function clearFetchWatchdog(sourceId: string) {
  const timer = fetchWatchdogs.get(sourceId)
  if (timer) clearTimeout(timer)
  fetchWatchdogs.delete(sourceId)
}

export function useRssSourcesQuery() {
  return useQuery({ queryKey: ['rss'], queryFn: api.listRssSources })
}

/** 条目流。`sourceId` 为空串（未选中订阅）时不发请求。 */
export function useRssItemsQuery(sourceId: string) {
  return useQuery({
    queryKey: ['rss-items', sourceId],
    queryFn: () => api.listRssItems(sourceId),
    enabled: sourceId !== '',
  })
}

function useInvalidateRss() {
  const qc = useQueryClient()
  return () => qc.invalidateQueries({ queryKey: ['rss'] })
}

export function useCreateRssSourceMutation() {
  const invalidate = useInvalidateRss()
  return useMutation({
    mutationFn: (req: RssSourceDto) => api.createRssSource(req),
    onSuccess: invalidate,
  })
}

export function useUpdateRssSourceMutation() {
  const invalidate = useInvalidateRss()
  return useMutation({
    mutationFn: ({ sourceId, req }: { sourceId: string; req: RssSourceDto }) =>
      api.updateRssSource(sourceId, req),
    onSuccess: invalidate,
  })
}

export function useDeleteRssSourceMutation() {
  const invalidate = useInvalidateRss()
  return useMutation({
    mutationFn: (sourceId: string) => api.deleteRssSource(sourceId),
    onSuccess: invalidate,
  })
}

export function useRefreshRssSourceMutation() {
  const invalidate = useInvalidateRss()
  return useMutation({
    mutationFn: (sourceId: string) => api.refreshRssSource(sourceId),
    onSuccess: invalidate,
  })
}

/** 条目操作（download / ignore / readAll）：条目状态与订阅未读数同时变，两份缓存都要拉。
 *  download 还会新建下载任务，故一并 invalidate ['tasks']。 */
export function useRssItemActionMutation() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ sourceId, req }: { sourceId: string; req: RssItemActionRequest }) =>
      api.rssItemAction(sourceId, req),
    onSuccess: (_data, { sourceId, req }) => {
      void qc.invalidateQueries({ queryKey: ['rss'] })
      void qc.invalidateQueries({ queryKey: ['rss-items', sourceId] })
      if (req.action === 'download') void qc.invalidateQueries({ queryKey: ['tasks'] })
    },
  })
}

/** feed 验证：只读、不落库，抓取失败也是 200（error 字段非空），故不 invalidate 任何缓存。 */
export function useValidateRssFeedMutation() {
  return useMutation({ mutationFn: (req: RssValidateRequest) => api.validateRssFeed(req) })
}
