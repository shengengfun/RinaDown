// #screen-main —— 三栏任务界面：侧边栏 + 中央任务列表 + 详情面板。
// 左右两栏支持拖拽调宽（见 components/tasks/ColResizer.tsx）。

import { useEffect, useRef, type CSSProperties } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { GlobalDialogs } from '../components/dialogs'
import { DetailPanel } from '../components/tasks/DetailPanel'
import { GroupDetailPanel } from '../components/tasks/GroupDetailPanel'
import { ManageBar } from '../components/tasks/ManageBar'
import { RssItemList } from '../components/tasks/RssItemList'
import { ColResizer, DETAIL_W, loadWidth, SIDEBAR_W } from '../components/tasks/ColResizer'
import { Sidebar } from '../components/tasks/Sidebar'
import { StatusBar } from '../components/tasks/StatusBar'
import { StatusTabs } from '../components/tasks/StatusTabs'
import { TaskList } from '../components/tasks/TaskList'
import { TasksUiProvider, useTasksUi } from '../components/tasks/context'
import { TopBar } from '../components/tasks/TopBar'
import { useRssSourcesQuery } from '../hooks/useRss'
import { api } from '../lib/api'
import { readBool, SECTION_KEY, useConfigQuery } from '../lib/config'
import { connectWs } from '../lib/ws'

export function TasksScreen() {
  const qc = useQueryClient()
  useEffect(() => {
    connectWs(qc)
  }, [qc])

  // 预取 + 与子组件共享同一份 Query 缓存（WS 消息直接 setQueryData 到这些 key）。
  useQuery({ queryKey: ['tasks'], queryFn: api.listTasks })
  useQuery({ queryKey: ['queues'], queryFn: api.listQueues })
  useQuery({ queryKey: ['groups'], queryFn: api.listGroups })
  useQuery({ queryKey: ['stats'], queryFn: api.stats, refetchInterval: 30_000 })

  // 初始宽度只读一次（拖拽期间由把手直接写 DOM，不回流 React state）。
  const initialWidths = useRef({ sidebar: loadWidth(SIDEBAR_W), detail: loadWidth(DETAIL_W) })

  return (
    <TasksUiProvider>
      <section
        className="wscreen active"
        id="screen-main"
        style={{ '--sidebar-w': `${initialWidths.current.sidebar}px`, '--detail-w': `${initialWidths.current.detail}px` } as CSSProperties}
      >
        <Sidebar />
        <SideBackdrop />
        <CenterPane />
        <ColResizer cssVar="--detail-w" conf={DETAIL_W} invert className="dresize" />
        <DetailPanel />
        <GroupDetailPanel />
      </section>
      <GlobalDialogs />
    </TasksUiProvider>
  )
}

/** 中央主区：默认任务列表；侧边栏选中某 RSS 订阅时整块换成条目流（两者互斥）。 */
function CenterPane() {
  const { rssFilter } = useTasksUi()
  const { data: sources = [] } = useRssSourcesQuery()
  const { data: config } = useConfigQuery()
  const source = rssFilter ? sources.find((s) => s.sourceId === rssFilter) : undefined
  return (
    <div className="center">
      <TopBar />
      {source ? (
        <RssItemList source={source} />
      ) : (
        <>
          <ManageBar />
          {readBool(config, SECTION_KEY.status) && <StatusTabs />}
          <TaskList />
        </>
      )}
      {/* 状态栏两种主区共用：全局速度/连接状态与「当前在看什么」无关，切到订阅时
          抽走底栏只会让整块布局跳一下。 */}
      <StatusBar />
    </div>
  )
}

/** 移动端抽屉侧边栏的遮罩：仅在小屏且抽屉展开时可见（CSS 控制），点击收起。 */
function SideBackdrop() {
  const { sidebarOpen, setSidebarOpen } = useTasksUi()
  if (!sidebarOpen) return null
  return <div className="side-backdrop" onClick={() => setSidebarOpen(false)} />
}
