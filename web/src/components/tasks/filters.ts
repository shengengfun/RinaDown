// 任务筛选/计数的纯函数 —— Sidebar/StatusTabs/TaskList/ManageBar 共用同一套语义。
// 状态 Tab 计数的统计对象始终是「全量任务」，不叠加其余筛选维度；只有 filterTasks
// （渲染任务列表用）会同时叠加 tab + 分类 + 队列 + 搜索四个维度。

import { passesCategory, type Category } from '../../lib/categories'
import { isSeeding } from '../../lib/seeding'
import type { TaskStatus } from '../../lib/types'
import type { ViewTask } from './useViewTasks'

export type StatusTab = 'all' | 'downloading' | 'completed' | 'paused' | 'error' | 'seeding'

/** 下载中 Tab 归并 pending(0) / downloading(1) / preparing(5)；其余 Tab 各对应单一状态码。 */
const TAB_STATUSES: Record<Exclude<StatusTab, 'all' | 'seeding'>, readonly TaskStatus[]> = {
  downloading: [0, 1, 5],
  completed: [3],
  paused: [2],
  error: [4],
}

/** 做种 Tab 看 (status, seedingStatus) 组合，需要整个任务对象。 */
export function matchesStatusTabTask(tab: StatusTab, t: ViewTask): boolean {
  if (tab === 'all') return true
  if (tab === 'seeding') return isSeeding(t)
  return TAB_STATUSES[tab].includes(t.status)
}

export function countByStatusTab(tasks: ViewTask[], tab: StatusTab): number {
  return tab === 'all' ? tasks.length : tasks.filter((t) => matchesStatusTabTask(tab, t)).length
}

export interface TaskFilters {
  statusTab: StatusTab
  /** 分类筛选值（`ALL_CATEGORY` = 不筛选）。 */
  categoryFilter: string
  /** 当前可见分类表：判定任务归属需要整张表（先命中先归属 + 兜底「其他」）。 */
  categories: Category[]
  queueFilter: string
  search: string
  /** groupId → 展示名小写形式；搜索词命中组名时该组全部成员视为命中，即使各自
   *  文件名不匹配（TopBar 搜索匹配组名）。未传等价于仅按文件名过滤。 */
  groupNameByGroupId?: Map<string, string>
}

/** 任务列表实际渲染用的组合过滤（tab + 分类 + 队列 + 搜索，搜索额外匹配所属组名）。 */
export function filterTasks(tasks: ViewTask[], f: TaskFilters): ViewTask[] {
  const q = f.search.trim().toLowerCase()
  return tasks.filter((t) => {
    if (!matchesStatusTabTask(f.statusTab, t)) return false
    if (!passesCategory(t, f.categoryFilter, f.categories)) return false
    if (f.queueFilter !== 'all' && t.queueId !== f.queueFilter) return false
    if (q) {
      const groupName = t.groupId ? f.groupNameByGroupId?.get(t.groupId) : undefined
      const matchesGroup = groupName !== undefined && groupName.includes(q)
      if (!matchesGroup && !t.fileName.toLowerCase().includes(q)) return false
    }
    return true
  })
}
