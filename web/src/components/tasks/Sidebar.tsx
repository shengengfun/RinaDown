// 侧边栏（220px）：品牌 + 全局速度、分类 / 设备 / 队列 / RSS 四个可折叠区块、连接徽标、反馈入口。
//
// 四个区块的显隐与折叠状态存在引擎 config 表里（见 lib/config.ts 的键表），与桌面客户端
// 是同一份数据：在任一端右键区块标题「隐藏此区块」，另一端下次读配置也随之隐藏。四个全隐
// 时整条侧边栏收起（见 routes/tasks.tsx），此时只剩设置页能把它们请回来。

import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import * as ContextMenu from '@radix-ui/react-context-menu'
import * as Dialog from '@radix-ui/react-dialog'
import { ArrowUpCircle, ChevronDown, EyeOff, Globe, List, Loader2, LogOut, Monitor, MessageCircle, Pause, Play, Plus, RefreshCw, Radio, Smartphone, Trash2, X } from 'lucide-react'
import type { ReactNode } from 'react'
import { AddLocalDeviceDialog } from '../dialogs/add-local-device'
import { api } from '../../lib/api'
import { categoryIcon, categoryIdOf, categoryLabel, parseCategories, visibleCategories, ALL_CATEGORY } from '../../lib/categories'
import { cloudApi } from '../../lib/cloud/client'
import { cloudDeviceId, useCloudSession } from '../../lib/cloud/session'
import { deviceLabel } from '../../lib/cloud/deviceLabel'
import { useRemoteTasks } from '../../lib/cloud/useRemoteTasks'
import { linkApi } from '../../lib/link'
import { clearCredentials, getBase } from '../../lib/auth'
import { cn } from '../../lib/cn'
import {
  boolEntry,
  CATEGORIES_KEY,
  EXPANDED_KEY,
  readBool,
  readTriBool,
  SECTION_KEY,
  useConfigMutation,
  useConfigQuery,
} from '../../lib/config'
import { fmtSpeed, fmtTime, queueDisplayName } from '../../lib/format'
import { useI18n } from '../../lib/i18n'
import { connStore, disconnectWs, useGlobalSpeed, useStore } from '../../lib/ws'
import { useUpdateCheck } from '../../lib/update'
import { confirmDialog } from '../../lib/confirm'
import { sourceDisplayName } from '../../lib/rss-filter'
import type { ConfigMap, RssSourceDto } from '../../lib/types'
import {
  beginRssFetch,
  useDeleteRssSourceMutation,
  useRefreshRssSourceMutation,
  useRssFetching,
  useRssSourcesQuery,
} from '../../hooks/useRss'
import { ColResizer, SIDEBAR_W } from './ColResizer'
import { useTasksUi } from './context'
import { QueueManagerDialog } from './queue-manager-dialog'
import { RssCreateDialog, RssManagerDialog } from './rss-manager-dialog'
import { useViewTasks } from './useViewTasks'

export function Sidebar() {
  const { t } = useI18n()
  const tasks = useViewTasks()
  const { data: queues = [] } = useQuery({ queryKey: ['queues'], queryFn: api.listQueues })
  const { categoryFilter, setCategoryFilter, queueFilter, setQueueFilter, deviceFilter, setDeviceFilter, rssFilter, setRssFilter, sidebarOpen, setSidebarOpen } = useTasksUi()
  const speed = useGlobalSpeed()
  const conn = useStore(connStore)
  const update = useUpdateCheck()
  const qc = useQueryClient()
  const navigate = useNavigate()
  const [logoutOpen, setLogoutOpen] = useState(false)
  const [rssCreateOpen, setRssCreateOpen] = useState(false)
  const session = useCloudSession()
  const myDeviceId = cloudDeviceId()
  const { data: config } = useConfigQuery()
  const configMut = useConfigMutation()
  const { data: cloudDevices = [] } = useQuery({
    queryKey: ['cloud', 'devices'],
    queryFn: () => cloudApi.devices().then((r) => r.devices),
    enabled: session.status === 'authenticated',
    staleTime: 10_000,
  })
  // 展示只列远端（不含本机），但 deviceLabel 判重名基准用 cloudDevices（含本机）——
  // 见 deviceLabel.ts 的硬约定。
  const remoteDevices = cloudDevices.filter((d) => d.deviceId !== myDeviceId)
  const { remoteTasks, onlineDeviceIds } = useRemoteTasks()
  // 本地设备(link)小节：仅展示已配对设备（在线圆点），不参与 deviceFilter 任务过滤——
  // 本地设备的任务运行在对端，本端看不到其进度，点击没有意义。
  const { data: linkDevices = [] } = useQuery({
    queryKey: ['link', 'devices'],
    queryFn: () => linkApi.devices().then((r) => r.devices),
    staleTime: 10_000,
    retry: false,
  })
  const showLinkSection = linkDevices.length > 0

  // 区块显隐。设备区是三态：未设置 = 有别的设备才显示（渐进披露），显式 true/false 则强制。
  //
  // 它**不要求登录云账户**：本机始终是一台设备，局域网直连配对也不经账号；
  // 云端登录只决定「远程设备」这一小节有没有内容。把整区挂在登录状态上，会让
  // 打开开关的用户对着空白发懵。
  const showCategory = readBool(config, SECTION_KEY.category)
  const showQueues = readBool(config, SECTION_KEY.queues)
  const showRss = readBool(config, SECTION_KEY.rss)
  const hasAnyDevice = remoteDevices.length > 0 || showLinkSection
  const showDeviceSection = readTriBool(config, SECTION_KEY.device) ?? hasAnyDevice

  // 写配置先本地落一帧再发请求：折叠箭头等交互的反馈必须是即时的，等一轮往返
  // 才动会像点了没反应。服务端回执后 invalidate 会用权威值覆盖这一帧。
  function writeConfig(entries: ConfigMap) {
    qc.setQueryData<ConfigMap>(['config'], (prev) => ({ ...(prev ?? {}), ...entries }))
    configMut.mutate(entries)
  }
  const expanded = (k: keyof typeof EXPANDED_KEY) => readBool(config, EXPANDED_KEY[k])
  const toggleExpanded = (k: keyof typeof EXPANDED_KEY) => writeConfig(boolEntry(EXPANDED_KEY[k], !expanded(k)))
  const hideSection = (k: keyof typeof SECTION_KEY) => writeConfig(boolEntry(SECTION_KEY[k], false))

  const categories = visibleCategories(parseCategories(config?.[CATEGORIES_KEY]))

  function logout() {
    setLogoutOpen(false)
    disconnectWs()
    clearCredentials()
    qc.clear()
    navigate({ to: '/login' })
  }

  const createQueue = useMutation({
    mutationFn: (name: string) => api.createQueue({ name }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['queues'] }),
  })
  const deleteQueue = useMutation({
    mutationFn: (id: string) => api.deleteQueue(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['queues'] }),
  })
  const startQueue = useMutation({
    mutationFn: (id: string) => api.startQueue(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['queues'] }),
  })
  const stopQueue = useMutation({
    mutationFn: (id: string) => api.stopQueue(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['queues'] }),
  })
  const { data: rssSources = [] } = useRssSourcesQuery()

  function addQueue() {
    const name = window.prompt(t('sidebar.newQueuePrompt'))
    if (name?.trim()) createQueue.mutate(name.trim())
  }

  const host = (() => {
    const base = getBase()
    if (!base) return location.host
    try {
      return new URL(base).host
    } catch {
      return base
    }
  })()
  const connText =
    conn.status === 'connected'
      ? conn.rttMs != null
        ? t('sidebar.connectedRtt', { rtt: conn.rttMs })
        : t('sidebar.connected')
      : conn.status === 'connecting'
        ? t('sidebar.connecting')
        : t('sidebar.disconnected')

  // 四个区块全隐时整条侧边栏（含拖拽把手）不渲染：留一条只剩品牌与连接徽标的
  // 空栏白占 220px。设置页的「界面区块」是把它们请回来的唯一入口。
  if (!showCategory && !showQueues && !showRss && !showDeviceSection) return null

  return (
    <>
    <aside className={cn('sidebar', sidebarOpen && 'open')}>
      <div className="side-brand">
        <span className="side-logo">
          <svg viewBox="30 30 452 452" role="img" xmlns="http://www.w3.org/2000/svg">
            <rect x="56" y="56" width="400" height="400" rx="88" fill="#3B82F6" />
            <path
              d="M 226 131 Q 226 119 238 119 L 274 119 Q 286 119 286 131 L 286 296 L 331 251 Q 340 242 349 251 L 363 265 Q 372 274 363 283 L 265 381 Q 256 390 247 381 L 149 283 Q 140 274 149 265 L 163 251 Q 172 242 181 251 L 226 296 Z"
              fill="#F2F4F8"
            />
          </svg>
        </span>
        <div className="side-brand-text">
          <b>FluxDown</b>
          <span>↓ {speed > 0 ? fmtSpeed(speed) : t('sidebar.idle')}</span>
        </div>
      </div>

      {/* RSS 条目流占着主区时，任务侧的三个区块都不是「当前所在位置」——它们只是
          回到任务列表的入口，此刻高亮任何一项都是在指向看不见的东西。 */}
      <div className="side-scroll">
        {showCategory && (
          <>
            <SectionLabel
              title={t('sidebar.fileTypes')}
              expanded={expanded('category')}
              onToggle={() => toggleExpanded('category')}
              onHide={() => hideSection('category')}
            />
            {expanded('category') && (
              <nav className="side-nav">
                {categories.map((c) => {
                  const Icon = categoryIcon(c)
                  const count =
                    c.builtinType === 'all'
                      ? tasks.length
                      : tasks.filter((task) => categoryIdOf(task, categories) === c.id).length
                  const active = c.builtinType === 'all' ? categoryFilter === ALL_CATEGORY : categoryFilter === c.id
                  return (
                    <button
                      key={c.id}
                      type="button"
                      className={cn('side-item', !rssFilter && active && 'active')}
                      onClick={() => {
                        setCategoryFilter(c.builtinType === 'all' ? ALL_CATEGORY : c.id)
                        setRssFilter(null)
                        setSidebarOpen(false)
                      }}
                    >
                      <Icon size={15} />
                      <span>{categoryLabel(c)}</span>
                      <em>{count || ''}</em>
                    </button>
                  )
                })}
              </nav>
            )}
          </>
        )}

        {showQueues && (
          <>
            <SectionLabel
              title={t('sidebar.queues')}
              expanded={expanded('queues')}
              onToggle={() => toggleExpanded('queues')}
              onHide={() => hideSection('queues')}
              onAdd={addQueue}
              addTitle={t('sidebar.newQueue')}
            />
            {expanded('queues') && (
              <nav className="side-nav">
                {queues.map((q) => {
                  const count = tasks.filter((t) => t.queueId === q.queueId).length
                  const builtin = q.queueId === 'main' || q.queueId === 'later'
                  const displayName = queueDisplayName(q)
                  return (
                    <div key={q.queueId} className="queue-row">
                      <button
                        type="button"
                        className={cn('side-item', !rssFilter && queueFilter === q.queueId && 'active')}
                        onClick={() => { setQueueFilter((f) => (f === q.queueId ? 'all' : q.queueId)); setRssFilter(null); setSidebarOpen(false) }}
                      >
                        <List size={15} />
                        <i
                          className={cn('queue-dot', q.isRunning && 'on')}
                          title={q.isRunning ? t('sidebar.queueRunning') : t('sidebar.queueStopped')}
                        />
                        <span>{displayName}</span>
                        <em>{count || ''}</em>
                      </button>
                      <div className="queue-actions">
                        <button
                          type="button"
                          className="icon-btn sm"
                          title={q.isRunning ? t('sidebar.stopQueue') : t('sidebar.startQueue')}
                          onClick={(e) => {
                            e.stopPropagation()
                            if (q.isRunning) stopQueue.mutate(q.queueId)
                            else startQueue.mutate(q.queueId)
                          }}
                        >
                          {q.isRunning ? <Pause size={13} /> : <Play size={13} />}
                        </button>
                        <QueueManagerDialog queue={q} queueName={displayName} />
                        {!builtin && (
                          <button
                            type="button"
                            className="icon-btn sm"
                            title={t('sidebar.deleteQueue')}
                            onClick={async (e) => {
                              e.stopPropagation()
                              if (await confirmDialog({ title: t('sidebar.deleteQueue'), message: t('sidebar.deleteQueueMsg', { name: displayName }), danger: true }))
                                deleteQueue.mutate(q.queueId)
                            }}
                          >
                            <Trash2 size={13} />
                          </button>
                        )}
                      </div>
                    </div>
                  )
                })}
              </nav>
            )}
          </>
        )}

        {showRss && (
          <>
            <SectionLabel
              title={t('sidebar.rss')}
              expanded={expanded('rss')}
              onToggle={() => toggleExpanded('rss')}
              onHide={() => hideSection('rss')}
              onAdd={() => setRssCreateOpen(true)}
              addTitle={t('rss.newSource')}
            />
            {expanded('rss') && (
              <nav className="side-nav">
                {rssSources.map((s) => (
                  <RssSourceRow key={s.sourceId} source={s} />
                ))}
              </nav>
            )}
          </>
        )}

        {/* 设备区固定排在最末：它是「去哪台机器下载」的切换器，不是内容导航，
            混在分类/队列中间会打断从上到下「筛什么 → 看什么」的阅读顺序。 */}
        {showDeviceSection && (
          <>
            <SectionLabel
              title={t('sidebar.devices')}
              expanded={expanded('device')}
              onToggle={() => toggleExpanded('device')}
              onHide={() => hideSection('device')}
            />
            {expanded('device') && (
              <nav className="side-nav">
                <button
                  type="button"
                  className={cn('side-item', !rssFilter && deviceFilter === null && 'active')}
                  onClick={() => { setDeviceFilter(null); setRssFilter(null); setSidebarOpen(false) }}
                >
                  <Globe size={15} />
                  <span>{t('sidebar.allDevices')}</span>
                </button>
                <button
                  type="button"
                  className={cn('side-item', !rssFilter && deviceFilter === myDeviceId && 'active')}
                  onClick={() => { setDeviceFilter(myDeviceId); setRssFilter(null); setSidebarOpen(false) }}
                >
                  <Monitor size={15} />
                  <i className="queue-dot on" title={t('link.online')} />
                  <span>{t('cloud.deviceCurrent')}</span>
                  <em>{tasks.length || ''}</em>
                </button>
                {remoteDevices.map((d) => {
                  const Icon = d.platform === 'android' || d.platform === 'ios' ? Smartphone : Monitor
                  const count = remoteTasks.filter((rt) => rt.toDevice === d.deviceId).length
                  // devices 查询是 10s 缓存的快照，SSE presence 是实时的：取并集，
                  // 免得刚上线的设备在导航里还挂着灰点。
                  const online = (d.isOnline ?? false) || onlineDeviceIds.has(d.deviceId)
                  return (
                    <button
                      key={d.id}
                      type="button"
                      className={cn('side-item', !rssFilter && deviceFilter === d.deviceId && 'active')}
                      onClick={() => { setDeviceFilter(d.deviceId); setRssFilter(null); setSidebarOpen(false) }}
                    >
                      <Icon size={15} />
                      <i className={cn('queue-dot', online && 'on')} title={online ? t('link.online') : t('link.offline')} />
                      <span>{deviceLabel(d, cloudDevices)}</span>
                      <em>{count || ''}</em>
                    </button>
                  )
                })}
                {showLinkSection && (
                  <>
                    <p className="side-sublabel">{t('sidebar.directDevices')}</p>
                    {linkDevices.map((d) => {
                      const Icon = d.platform === 'android' || d.platform === 'ios' ? Smartphone : Monitor
                      return (
                        <div key={d.fingerprint} className="side-item">
                          <Icon size={15} />
                          <i className={cn('queue-dot', d.online && 'on')} title={d.online ? t('link.online') : t('link.offline')} />
                          <span>{d.name || '-'}</span>
                        </div>
                      )
                    })}
                  </>
                )}
                {/* 配对入口常驻：局域网直连不经账号，没有它这一区在「只有本机」时
                    就是一条死路——用户看得到设备概念却没有添加设备的地方。 */}
                <AddLocalDeviceDialog
                  trigger={
                    <button type="button" className="side-item">
                      <Plus size={15} />
                      <span>{t('link.addDevice')}</span>
                    </button>
                  }
                />
              </nav>
            )}
          </>
        )}
      </div>

      <div className="side-bottom">
        <div className="conn-badge" title={host}>
          <i className="dot" style={{ background: conn.status === 'connected' ? 'var(--success)' : 'var(--text3)' }} />
          <div className="conn-text">
            <b>{host}</b>
            <span>{connText}</span>
          </div>
          <button type="button" className="icon-btn sm ml-auto shrink-0" title={t('sidebar.logoutTitle')} onClick={() => setLogoutOpen(true)}>
            <LogOut size={13} />
          </button>
        </div>
        <a className="side-feedback" href="https://github.com/zerx-lab/FluxDown/issues" target="_blank" rel="noreferrer">
          <MessageCircle size={14} />
          {t('sidebar.feedback')}
        </a>
        {update.hasUpdate && update.releaseUrl ? (
          <a className="side-feedback" style={{ color: 'var(--accent)' }} href={update.releaseUrl} target="_blank" rel="noreferrer">
            <ArrowUpCircle size={14} />
            {t('sidebar.newVersion', { version: `v${update.latest}` })}
          </a>
        ) : update.current ? (
          <span className="side-feedback" style={{ cursor: 'default' }}>
            {t('sidebar.version', { version: `v${update.current}` })}
          </span>
        ) : null}
      </div>

      <Dialog.Root open={logoutOpen} onOpenChange={setLogoutOpen}>
        <Dialog.Portal>
          <Dialog.Overlay className="wbackdrop show" />
          <Dialog.Content className="dialog sm show">
            <header className="dlg-head">
              <Dialog.Title asChild>
                <b>{t('sidebar.logoutTitle')}</b>
              </Dialog.Title>
              <Dialog.Close asChild>
                <button type="button" className="icon-btn sm" aria-label={t('common.close')}>
                  <X size={16} />
                </button>
              </Dialog.Close>
            </header>
            <div className="dlg-body">
              <Dialog.Description className="dlg-sub">{t('sidebar.logoutMsg')}</Dialog.Description>
            </div>
            <footer className="dlg-foot">
              <Dialog.Close asChild>
                <button type="button" className="btn ghost">
                  {t('common.cancel')}
                </button>
              </Dialog.Close>
              <button type="button" className="btn danger" onClick={logout}>
                {t('sidebar.logoutTitle')}
              </button>
            </footer>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>

      <RssCreateDialog open={rssCreateOpen} onOpenChange={setRssCreateOpen} />
    </aside>
    <ColResizer cssVar="--sidebar-w" conf={SIDEBAR_W} />
    </>
  )
}

/** 区块标题：折叠箭头 + 标题（整体可点折叠）+ 可选的「新建」按钮；右键给出隐藏入口。
 *  隐藏做成右键而不是常驻按钮——它是低频的一次性布置动作，常驻只会在每个标题右边
 *  多挂一个几乎不点的图标。 */
function SectionLabel({
  title,
  expanded,
  onToggle,
  onHide,
  onAdd,
  addTitle,
}: {
  title: ReactNode
  expanded: boolean
  onToggle: () => void
  onHide: () => void
  onAdd?: () => void
  addTitle?: string
}) {
  const { t } = useI18n()
  return (
    <ContextMenu.Root>
      <ContextMenu.Trigger asChild>
        <p className="side-label row">
          <button type="button" className="side-label-toggle" onClick={onToggle}>
            <ChevronDown size={12} className={cn('side-chevron', !expanded && 'collapsed')} />
            {title}
          </button>
          {onAdd && (
            <button type="button" className="side-add" title={addTitle} onClick={onAdd}>
              <Plus size={13} />
            </button>
          )}
        </p>
      </ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Content className="ctxmenu show">
          <ContextMenu.Item className="ctx-item" onSelect={onHide}>
            <EyeOff size={14} />
            {t('sidebar.hideSection')}
          </ContextMenu.Item>
        </ContextMenu.Content>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  )
}

/** 单条 RSS 订阅行。抽成组件是因为「抓取中」状态是**每源**的，只有独立组件才能
 *  各自订阅 useRssFetching。 */
function RssSourceRow({ source: s }: { source: RssSourceDto }) {
  const { t } = useI18n()
  const { rssFilter, setRssFilter, setSidebarOpen } = useTasksUi()
  const refreshRss = useRefreshRssSourceMutation()
  const deleteRss = useDeleteRssSourceMutation()
  const fetching = useRssFetching(s.sourceId) || refreshRss.isPending
  const displayName = sourceDisplayName(s)
  // 错误态优先于启用态：连续失败的订阅用警告色圆点顶出来，tooltip 给出
  // 失败原因 + 上次成功时间（否则用户只会看到「计数不涨」而不知为何）。
  const errTip = s.lastError
    ? `${s.lastError}\n${s.lastSuccessAt > 0 ? t('rss.lastSuccessAt', { time: fmtTime(s.lastSuccessAt) }) : t('rss.neverFetched')}`
    : ''

  return (
    <div className={cn('queue-row', fetching && 'rss-busy')}>
      <button
        type="button"
        className={cn('side-item', rssFilter === s.sourceId && 'active')}
        onClick={() => { setRssFilter((f) => (f === s.sourceId ? null : s.sourceId)); setSidebarOpen(false) }}
      >
        <Radio size={15} />
        <i
          className={cn('queue-dot', s.lastError ? 'warn' : s.enabled && 'on')}
          title={errTip || (s.enabled ? t('rss.stateEnabled') : t('rss.stateDisabled'))}
        />
        <span title={errTip || displayName}>{displayName}</span>
        <em>{s.unreadCount || ''}</em>
      </button>
      <div className="queue-actions">
        <button
          type="button"
          className="icon-btn sm"
          title={fetching ? t('rss.refreshing') : t('rss.refreshNow')}
          disabled={fetching}
          onClick={(e) => { e.stopPropagation(); beginRssFetch(s.sourceId, s.lastFetchAt); refreshRss.mutate(s.sourceId) }}
        >
          {fetching ? <Loader2 size={13} className="animate-spin" /> : <RefreshCw size={13} />}
        </button>
        <RssManagerDialog source={s} />
        <button
          type="button"
          className="icon-btn sm"
          title={t('rss.deleteSource')}
          onClick={async (e) => {
            e.stopPropagation()
            if (await confirmDialog({ title: t('rss.deleteSource'), message: t('rss.deleteSourceMsg', { name: displayName }), danger: true })) {
              if (rssFilter === s.sourceId) setRssFilter(null)
              deleteRss.mutate(s.sourceId)
            }
          }}
        >
          <Trash2 size={13} />
        </button>
      </div>
    </div>
  )
}
