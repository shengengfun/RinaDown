// 中央任务列表：过滤 → 显示已完成开关 → 7 维分桶（智能/日期/状态/类型/队列/站点/不分组）
// → 桶内 6 键排序 → （仅列表形态）在组行后插入目录分段行+成员行 → 用
// @tanstack/react-virtual 虚拟滚动。网格形态为 bento 行装箱卡片网格（GridCard.tsx，
// 组卡跨 2 列；对齐桌面 _buildGridBody：最小卡宽 210 / 间距 10 / 卡高 138），
// 不支持组内展开（对齐桌面「网格降级」）。分桶/排序纯函数见 lib/list-sections.ts，
// 视图偏好（形态/密度/分组/排序/显示开关/列）见 lib/view-prefs.ts，按状态页签独立记忆。
//
// 组聚合：groupId 非空但组列表查不到（孤儿成员）与无 groupId 的任务一律按普通任务平铺
// 兜底。状态/分类/队列筛选 + 显示已完成开关作用于成员；搜索词命中组名时整组（含全部
// 成员）可见，命中成员文件名时组行+命中成员可见（组行仅在有可见成员时出现，计数按可见
// 成员）。组行本身不参与既有多选批量。

import { useEffect, useRef, useState, type CSSProperties, type MouseEvent, type PointerEvent as ReactPointerEvent } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useVirtualizer } from '@tanstack/react-virtual'
import { ChevronDown, ChevronRight, Folder } from 'lucide-react'
import { api } from '../../lib/api'
import { parseCategories, visibleCategories } from '../../lib/categories'
import { CATEGORIES_KEY, useConfigQuery } from '../../lib/config'
import { cloudDeviceId } from '../../lib/cloud/session'
import type { RemoteTask } from '../../lib/cloud/types'
import { useRemoteTasks } from '../../lib/cloud/useRemoteTasks'
import { cn } from '../../lib/cn'
import { fmtBytes } from '../../lib/format'
import { useI18n } from '../../lib/i18n'
import { bucketEntities, compareSectionEntities, orderSections, type SectionEntity } from '../../lib/list-sections'
import { compressPathChain, dirKey, flattenGroupMembers, groupDisplayName } from '../../lib/task-group'
import { useViewPrefs } from '../../lib/view-prefs'
import type { GroupDto } from '../../lib/types'
import { GroupRow } from './GroupRow'
import { SeedingSummaryBar } from './SeedingSummaryBar'
import { GroupGridCard, TaskGridCard } from './GridCard'
import { RemoteTaskRow } from './RemoteTaskRow'
import { TaskRow } from './TaskRow'
import { filterTasks } from './filters'
import { useTasksUi } from './context'
import { useViewTasks, type ViewTask } from './useViewTasks'

type FlatItem =
  | { kind: 'sectionhead'; key: string; title: string; count: number }
  | { kind: 'row'; task: ViewTask }
  | { kind: 'grouprow'; group: GroupDto; members: ViewTask[] }
  | { kind: 'groupdir'; groupId: string; path: string; fileCount: number; totalBytes: number }
  | { kind: 'groupmember'; task: ViewTask }
  | { kind: 'gridrow'; entities: SectionEntity<ViewTask>[] }
  | { kind: 'remoterow'; task: RemoteTask }

// 行的估算尺寸取自 design.css：.task-row/.grow min-height 64 + margin-bottom 4（虚拟
// 滚动下 margin 不参与相邻元素排布，需并入 estimateSize 才能还原视觉间距）。
// 组行与任务行等高（§4.6 组不豁免行高节奏）：舒适 64+4、紧凑 44+4 单行化
// （meta/计数并入行内、进度条移行底或隐藏）。
const SECTION_HEAD_SIZE = 32
const ROW_SIZE = 68
const ROW_COMPACT_SIZE = 48
const GROW_SIZE = 68
const GROW_COMPACT_SIZE = 48
const GDIR_SIZE = 28
const GDIR_COMPACT_SIZE = 24
const GRID_CARD_HEIGHT = 138
const GRID_GAP = 10
const GRID_ROW_SIZE = GRID_CARD_HEIGHT + GRID_GAP
const GRID_CARD_MIN_WIDTH = 210
// 框选（marquee）调参：拖动位移超过该阈值才激活，避免吞掉普通点击；激活后指针进入
// 容器上下边缘这个宽度区间即自动滚动，速度按贴近边缘程度在 [MIN,MAX] 间线性插值。
const MARQUEE_ACTIVATE_PX = 4
const MARQUEE_EDGE_PX = 28
const MARQUEE_SCROLL_STEP_MIN = 4
const MARQUEE_SCROLL_STEP_MAX = 18

export function TaskList() {
  const { t } = useI18n()
  const {
    statusTab,
    categoryFilter,
    queueFilter,
    deviceFilter,
    search,
    foldedSections,
    toggleSectionFold,
    manageMode,
    expandedGroups,
    scrollTarget,
    clearScrollTarget,
    collapsedDirs,
    toggleDirCollapsed,
    clearSelection,
    selected,
    setSelected,
    enterManageMode,
    visibleTaskOrderRef,
  } = useTasksUi()
  const prefs = useViewPrefs(statusTab)
  const tasks = useViewTasks()
  const { data: queues = [] } = useQuery({ queryKey: ['queues'], queryFn: api.listQueues })
  const { data: groups = [] } = useQuery({ queryKey: ['groups'], queryFn: api.listGroups })
  const { data: config } = useConfigQuery()
  const { remoteTasks } = useRemoteTasks()
  const isRemoteDeviceFilter = deviceFilter !== null && deviceFilter !== cloudDeviceId()
  const remoteTasksForDevice = isRemoteDeviceFilter ? remoteTasks.filter((rt) => rt.toDevice === deviceFilter) : []
  const parentRef = useRef<HTMLDivElement>(null)
  const contentRef = useRef<HTMLDivElement>(null)
  const [containerWidth, setContainerWidth] = useState(960)
  // 框选拖拽会话：pointerdown 时建立，pointerup/cancel 时清空；不用 state 是因为
  // pointermove 高频触发，state 更新走 setSelected/setMarqueeRect 已够，会话本身
  // 只需在事件间存活，不需要触发渲染。
  const dragRef = useRef<{
    pointerId: number
    ctrlAtStart: boolean
    base: Set<string>
    startX: number
    startY: number
    clientX: number
    clientY: number
    active: boolean
    rafId: number | null
  } | null>(null)
  // 框选松手后短暂拦截紧随其后的 click（否则会被当成点在行/空白上，
  // 触发详情面板打开或清空选中）；容器 onClickCapture 消费一次即复位。
  const suppressClickRef = useRef(false)
  const [marqueeRect, setMarqueeRect] = useState<{ left: number; top: number; width: number; height: number } | null>(null)

  useEffect(() => {
    const el = parentRef.current
    if (!el) return
    // RO 的初始回调依赖渲染帧（后台标签页/无帧环境可能不派发）——先同步量一次
    // 内容盒宽度，保证首帧列数就正确；RO 只负责后续尺寸变化。
    const cs = getComputedStyle(el)
    const initial = el.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight)
    if (initial > 0) setContainerWidth(initial)
    const ro = new ResizeObserver((entries) => {
      const w = entries[0]?.contentRect.width
      if (w) setContainerWidth(w)
    })
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  const groupsById = new Map(groups.map((g) => [g.groupId, g]))
  const groupNameByGroupId = new Map(groups.map((g) => [g.groupId, groupDisplayName(g).toLowerCase()]))
  const categories = visibleCategories(parseCategories(config?.[CATEGORIES_KEY]))
  const filteredByDims = filterTasks(tasks, { statusTab, categoryFilter, categories, queueFilter, search, groupNameByGroupId })
  const filtered = prefs.showCompleted ? filteredByDims : filteredByDims.filter((task) => task.status !== 3)

  // 按 groupId 聚合：仅存在于 groups 列表中的组才聚合为活卡片。
  const membersByGroup = new Map<string, ViewTask[]>()
  const flatTasks: ViewTask[] = []
  for (const task of filtered) {
    const g = task.groupId ? groupsById.get(task.groupId) : undefined
    if (!g) {
      flatTasks.push(task)
      continue
    }
    const arr = membersByGroup.get(g.groupId)
    if (arr) arr.push(task)
    else membersByGroup.set(g.groupId, [task])
  }
  const entities: SectionEntity<ViewTask>[] = flatTasks.map((task) => ({ kind: 'task', task }))
  for (const [groupId, members] of membersByGroup) entities.push({ kind: 'group', group: groupsById.get(groupId)!, members })

  const bucketed = bucketEntities(entities, prefs.groupBy, queues)
  for (const section of bucketed) {
    section.entities.sort((a, b) => compareSectionEntities(prefs.sortKey, prefs.sortDir, a, b, groupDisplayName))
  }
  const sections = orderSections(bucketed, prefs.sortKey, prefs.sortDir, groupDisplayName)

  const isGrid = prefs.form === 'grid'
  const cardsPerRow = Math.max(1, Math.floor((containerWidth + GRID_GAP) / (GRID_CARD_MIN_WIDTH + GRID_GAP)))

  const flat: FlatItem[] = []
  if (isRemoteDeviceFilter) {
    for (const rt of remoteTasksForDevice) flat.push({ kind: 'remoterow', task: rt })
  } else {
    for (const section of sections) {
      if (section.title !== null) flat.push({ kind: 'sectionhead', key: section.key, title: section.title, count: section.entities.length })
      if (foldedSections.has(section.key)) continue

      if (isGrid) {
        // 行装箱：task 占 1 槽，group 占 2 槽，贪心凑满 cardsPerRow 即换行（网格无组内展开机制）。
        let row: SectionEntity<ViewTask>[] = []
        let used = 0
        for (const e of section.entities) {
          const cost = e.kind === 'group' ? Math.min(2, cardsPerRow) : 1
          if (used + cost > cardsPerRow && row.length > 0) {
            flat.push({ kind: 'gridrow', entities: row })
            row = []
            used = 0
          }
          row.push(e)
          used += cost
        }
        if (row.length > 0) flat.push({ kind: 'gridrow', entities: row })
        continue
      }

      for (const e of section.entities) {
        if (e.kind === 'task') {
          flat.push({ kind: 'row', task: e.task })
          continue
        }
        flat.push({ kind: 'grouprow', group: e.group, members: e.members })
        if (!expandedGroups.has(e.group.groupId)) continue
        const groupId = e.group.groupId
        const isDirCollapsed = (path: string) => collapsedDirs.has(dirKey(groupId, path))
        for (const m of flattenGroupMembers(e.members, e.group.saveDir, isDirCollapsed)) {
          if (m.kind === 'dir') flat.push({ kind: 'groupdir', groupId, path: m.path, fileCount: m.fileCount, totalBytes: m.totalBytes })
          else flat.push({ kind: 'groupmember', task: m.task })
        }
      }
    }
  }

  // 可参与多选的可见顺序（Shift 范围选择/框选用）：任务组行/折叠分组内成员/远程行
  // 都不参与，与 flat 装配规则保持一致——每次渲染后写入，供事件回调读取最新值。
  useEffect(() => {
    const order: string[] = []
    for (const item of flat) {
      if (item.kind === 'row' || item.kind === 'groupmember') order.push(item.task.taskId)
      else if (item.kind === 'gridrow') {
        for (const e of item.entities) if (e.kind === 'task') order.push(e.task.taskId)
      }
    }
    visibleTaskOrderRef.current = order
  })

  // 紧凑档行高（design §4.4：任务/组行 44+4、目录行 24）；网格形态密度不适用。
  const isCompact = prefs.density === 'compact'
  const virtualizer = useVirtualizer({
    count: flat.length,
    getScrollElement: () => parentRef.current,
    // estimateSize 不在 react-virtual 的 measurements memo deps 里——把形态/密度
    // 编进 item key，切换时强制整表重算，否则沿用旧行高导致行错位。
    getItemKey: (i) => `${prefs.form}:${prefs.density}:${i}`,
    estimateSize: (i) => {
      const item = flat[i]
      if (item.kind === 'sectionhead') return SECTION_HEAD_SIZE
      if (item.kind === 'gridrow') return GRID_ROW_SIZE
      if (item.kind === 'grouprow') return isCompact ? GROW_COMPACT_SIZE : GROW_SIZE
      if (item.kind === 'groupdir') return isCompact ? GDIR_COMPACT_SIZE : GDIR_SIZE
      return isCompact ? ROW_COMPACT_SIZE : ROW_SIZE
    },
    overscan: 8,
  })

  // 失败直达（组计数行点击）：目标组/目录已由 jumpToGroupMember 展开，这里只负责滚动；
  // 找不到（已被筛选隐藏，或当前为网格形态无展开机制）时静默清空，避免悬挂状态。
  useEffect(() => {
    if (!scrollTarget) return
    const index = flat.findIndex((item) => item.kind === 'groupmember' && item.task.taskId === scrollTarget)
    if (index >= 0) virtualizer.scrollToIndex(index, { align: 'center' })
    clearScrollTarget()
  }, [scrollTarget])

  // 点空白退出选中。虚拟列表在行与滚动容器之间还夹着「撑高层 + 每项绝对定位包裹层」，
  // 用 e.target === e.currentTarget 会把落在这两层上的点击漏判成点在行上；改为向上寻找
  // 可选中元素，找不到就一律视为空白，行/卡片自身的点击照常冒泡到 selectTask 不受影响。
  // .ctxmenu：右键菜单经 Radix Portal 渲染在 body 下，但 React 合成事件沿组件树冒泡，
  // 菜单项点击会到达这里且 DOM target 不在任何行内——不加会被误判成空白点击关掉详情面板。
  function onScrollAreaClick(e: MouseEvent<HTMLDivElement>) {
    if (!(e.target as HTMLElement).closest('.task-row, .grow, .gcard, .group-head, .gdir-row, .ctxmenu')) clearSelection()
  }

  // 卸载时若还在拖拽中，取消挂着的自动滚动 rAF，避免泄漏。
  useEffect(() => {
    return () => {
      const rafId = dragRef.current?.rafId
      if (rafId !== null && rafId !== undefined) cancelAnimationFrame(rafId)
    }
  }, [])

  function isInteractiveTarget(target: EventTarget | null) {
    return target instanceof HTMLElement && target.closest('button, input, a, select, textarea, .mcheck, .ctxmenu') !== null
  }

  // 客户端坐标 → 内容坐标：相对撑高层（`contentRef`）左上角，与虚拟项的 `vi.start`
  // 同一坐标系——撑高层随滚动整体位移，这里直接减它的 boundingRect 天然抵消滚动。
  function contentPoint(clientX: number, clientY: number) {
    const rect = contentRef.current?.getBoundingClientRect()
    if (!rect) return { x: 0, y: 0 }
    return { x: clientX - rect.left, y: clientY - rect.top }
  }

  // 框选命中：纵向用 virtualizer.measurementsCache 与矩形做区间相交——它覆盖全量
  // flat（懒物化，见 lazy-measurements.ts），不依赖行是否已经渲染进 DOM；横向
  // row/groupmember 视为整行命中，gridrow 按行内槽位坐标算（group 卡占 2 槽但
  // 跳过不参与多选，网格降级）。
  function computeMarqueeHits(x1: number, y1: number, x2: number, y2: number): Set<string> {
    const top = Math.min(y1, y2)
    const bottom = Math.max(y1, y2)
    const left = Math.min(x1, x2)
    const right = Math.max(x1, x2)
    const hits = new Set<string>()
    const cellWidth = isGrid ? (containerWidth - (cardsPerRow - 1) * GRID_GAP) / cardsPerRow : 0
    for (let i = 0; i < flat.length; i++) {
      const m = virtualizer.measurementsCache[i]
      if (!m || m.end < top || m.start > bottom) continue
      const item = flat[i]
      if (item.kind === 'row' || item.kind === 'groupmember') {
        hits.add(item.task.taskId)
      } else if (item.kind === 'gridrow') {
        let used = 0
        for (const e of item.entities) {
          const cost = e.kind === 'group' ? Math.min(2, cardsPerRow) : 1
          const slotLeft = used * (cellWidth + GRID_GAP)
          const slotRight = slotLeft + cellWidth * cost + GRID_GAP * (cost - 1)
          if (e.kind === 'task' && slotRight >= left && slotLeft <= right) hits.add(e.task.taskId)
          used += cost
        }
      }
    }
    return hits
  }

  // 按当前拖拽矩形重算选中态 + 可视框；随拖动实时增减（矩形缩小会取消命中，不是只增）。
  function applyMarqueeRect(startX: number, startY: number, curX: number, curY: number, base: Set<string> | null) {
    const hits = computeMarqueeHits(startX, startY, curX, curY)
    setSelected(base ? new Set([...base, ...hits]) : hits)
    setMarqueeRect({ left: Math.min(startX, curX), top: Math.min(startY, curY), width: Math.abs(curX - startX), height: Math.abs(curY - startY) })
  }

  // 指针贴近容器可视上/下边缘 MARQUEE_EDGE_PX 内时持续滚动，速度按贴近程度插值；
  // 每帧都用最新 lastClient 坐标重算矩形，滚动位移不会让已选中范围跟丢。
  function marqueeAutoScrollTick() {
    const drag = dragRef.current
    const el = parentRef.current
    if (!drag || !drag.active || !el) return
    const rect = el.getBoundingClientRect()
    const distTop = drag.clientY - rect.top
    const distBottom = rect.bottom - drag.clientY
    let dy = 0
    if (distTop < MARQUEE_EDGE_PX) {
      const proximity = 1 - Math.max(0, distTop) / MARQUEE_EDGE_PX
      dy = -(MARQUEE_SCROLL_STEP_MIN + proximity * (MARQUEE_SCROLL_STEP_MAX - MARQUEE_SCROLL_STEP_MIN))
    } else if (distBottom < MARQUEE_EDGE_PX) {
      const proximity = 1 - Math.max(0, distBottom) / MARQUEE_EDGE_PX
      dy = MARQUEE_SCROLL_STEP_MIN + proximity * (MARQUEE_SCROLL_STEP_MAX - MARQUEE_SCROLL_STEP_MIN)
    }
    if (dy !== 0) {
      el.scrollTop += dy
      const cur = contentPoint(drag.clientX, drag.clientY)
      applyMarqueeRect(drag.startX, drag.startY, cur.x, cur.y, drag.ctrlAtStart ? drag.base : null)
    }
    drag.rafId = requestAnimationFrame(marqueeAutoScrollTick)
  }

  function onMarqueePointerDown(e: ReactPointerEvent<HTMLDivElement>) {
    if (isRemoteDeviceFilter || e.button !== 0 || isInteractiveTarget(e.target)) return
    const { x, y } = contentPoint(e.clientX, e.clientY)
    dragRef.current = {
      pointerId: e.pointerId,
      ctrlAtStart: e.ctrlKey || e.metaKey,
      base: new Set(selected),
      startX: x,
      startY: y,
      clientX: e.clientX,
      clientY: e.clientY,
      active: false,
      rafId: null,
    }
  }

  function onMarqueePointerMove(e: ReactPointerEvent<HTMLDivElement>) {
    const drag = dragRef.current
    if (!drag || drag.pointerId !== e.pointerId) return
    drag.clientX = e.clientX
    drag.clientY = e.clientY
    const { x, y } = contentPoint(e.clientX, e.clientY)
    if (!drag.active) {
      if (Math.hypot(x - drag.startX, y - drag.startY) <= MARQUEE_ACTIVATE_PX) return
      drag.active = true
      // 到这里才捕获指针：pointerdown 就捕获会让 Chrome 把兼容 click 一并重定向到
      // 容器，行/卡片的 onClick 永远收不到；激活后捕获只为拖出窗口时仍收到 move/up，
      // 而框选松手后的 click 本来就会被 suppressClickRef 吞掉，重定向无副作用。
      e.currentTarget.setPointerCapture(e.pointerId)
      enterManageMode()
      drag.rafId = requestAnimationFrame(marqueeAutoScrollTick)
    }
    applyMarqueeRect(drag.startX, drag.startY, x, y, drag.ctrlAtStart ? drag.base : null)
  }

  function endMarqueeDrag(e: ReactPointerEvent<HTMLDivElement>) {
    const drag = dragRef.current
    if (!drag || drag.pointerId !== e.pointerId) return
    if (drag.rafId !== null) cancelAnimationFrame(drag.rafId)
    // 激活过（真正框选过）才吞掉紧随的 click；纯点击（未过阈值）不受影响，原有
    // 单击/双击/右键行为照常。
    if (drag.active) suppressClickRef.current = true
    dragRef.current = null
    setMarqueeRect(null)
    if (e.currentTarget.hasPointerCapture(e.pointerId)) e.currentTarget.releasePointerCapture(e.pointerId)
  }

  // 框选松手后的 click 先打到这里（捕获阶段先于冒泡阶段的 onScrollAreaClick）：
  // 吞掉这一次，避免被当成点在行/空白上误触发详情面板或清空选中。
  function onScrollAreaClickCapture(e: MouseEvent<HTMLDivElement>) {
    if (suppressClickRef.current) {
      suppressClickRef.current = false
      e.preventDefault()
      e.stopPropagation()
    }
  }

  return (
    <div
      className={cn('task-scroll', manageMode && 'manage', isGrid && 'grid-form', marqueeRect && 'marqueeing')}
      ref={parentRef}
      onClick={onScrollAreaClick}
      onClickCapture={onScrollAreaClickCapture}
      onPointerDown={onMarqueePointerDown}
      onPointerMove={onMarqueePointerMove}
      onPointerUp={endMarqueeDrag}
      onPointerCancel={endMarqueeDrag}
    >
      {statusTab === 'seeding' && <SeedingSummaryBar />}
      {flat.length === 0 ? (
        <p className="empty-tip">{t('list.empty')}</p>
      ) : (
        <div ref={contentRef} style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
          {marqueeRect && (
            <div
              className="task-marquee"
              style={{ left: marqueeRect.left, top: marqueeRect.top, width: marqueeRect.width, height: marqueeRect.height }}
            />
          )}
          {virtualizer.getVirtualItems().map((vi) => {
            const item = flat[vi.index]
            return (
              <div key={vi.key} style={{ position: 'absolute', top: 0, left: 0, right: 0, transform: `translateY(${vi.start}px)` }}>
                {item.kind === 'sectionhead' && (
                  <div className={cn('group-head', foldedSections.has(item.key) && 'folded')} onClick={() => toggleSectionFold(item.key)}>
                    <ChevronDown size={12} />
                    {item.title} <em>· {item.count}</em>
                  </div>
                )}
                {item.kind === 'row' && (
                  <TaskRow task={item.task} queues={queues} density={prefs.density} protocolBadges={prefs.protocolBadges} columns={prefs.columns} />
                )}
                {item.kind === 'grouprow' && <GroupRow group={item.group} members={item.members} density={prefs.density} />}
                {item.kind === 'groupdir' && (
                  <div className={cn('gdir-row', isCompact && 'compact')} onClick={() => toggleDirCollapsed(item.groupId, item.path)}>
                    {collapsedDirs.has(dirKey(item.groupId, item.path)) ? <ChevronRight size={11} /> : <ChevronDown size={11} />}
                    <Folder size={12} />
                    <span className="ellip">{compressPathChain(item.path)}</span>
                    <span className="gdir-meta">{t('group.dirMeta', { count: item.fileCount, size: fmtBytes(item.totalBytes) })}</span>
                  </div>
                )}
                {item.kind === 'groupmember' && (
                  <div className="grow-member">
                    <TaskRow task={item.task} queues={queues} density={prefs.density} protocolBadges={prefs.protocolBadges} columns={prefs.columns} />
                  </div>
                )}
                {item.kind === 'remoterow' && <RemoteTaskRow task={item.task} density={prefs.density} />}
                {item.kind === 'gridrow' && (
                  <div className="grid-row" style={{ '--grid-cols': cardsPerRow } as CSSProperties}>
                    {item.entities.map((e) =>
                      e.kind === 'task' ? (
                        <div className="grid-cell" key={e.task.taskId}>
                          <TaskGridCard task={e.task} queues={queues} protocolBadges={prefs.protocolBadges} />
                        </div>
                      ) : (
                        <div className="grid-cell grid-cell-wide" style={{ gridColumn: `span ${Math.min(2, cardsPerRow)}` }} key={e.group.groupId}>
                          <GroupGridCard group={e.group} members={e.members} />
                        </div>
                      ),
                    )}
                  </div>
                )}
              </div>
            )
          })}
        </div>
      )}
    </div>
  )
}
