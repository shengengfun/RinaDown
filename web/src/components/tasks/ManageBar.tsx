// 批量管理条：全选 / 已选计数 / 批量暂停恢复 / 清理失效 / 删除（可选连文件）。
// 仅在 manageMode 时渲染内容（所有 hooks 必须先于该判断无条件调用，满足
// Rules of Hooks）。动作集与桌面端管理栏（lib/src/widgets/task_tab_bar.dart）
// 对齐，暂停/恢复是 web 侧额外保留的两项。

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { BrushCleaning, FileX, Pause, Play, Trash2, X } from 'lucide-react'
import { api } from '../../lib/api'
import { parseCategories, visibleCategories } from '../../lib/categories'
import { CATEGORIES_KEY, useConfigQuery } from '../../lib/config'
import { confirmDialog } from '../../lib/confirm'
import { useI18n } from '../../lib/i18n'
import { groupDisplayName } from '../../lib/task-group'
import { filterTasks } from './filters'
import { useTasksUi } from './context'
import { useViewTasks } from './useViewTasks'

/** 批量 REST 的并发上限。「全选」可以是上千个任务，`Promise.all` 全量并发会
 *  打满浏览器的同源连接池，且让服务端 actor 的命令队列瞬时堆满。 */
const BATCH_CONCURRENCY = 8

/** 以固定并发跑完整批 id；任一请求失败会向调用方冒泡（与单条动作一致）。 */
async function runBatch(ids: string[], fn: (id: string) => Promise<unknown>): Promise<void> {
  let next = 0
  const worker = async () => {
    for (let i = next++; i < ids.length; i = next++) {
      const id = ids[i]
      if (id !== undefined) await fn(id)
    }
  }
  await Promise.all(Array.from({ length: Math.min(BATCH_CONCURRENCY, ids.length) }, worker))
}

export function ManageBar() {
  const { t } = useI18n()
  const { manageMode, setManageMode, selected, setSelected, statusTab, categoryFilter, queueFilter, search } = useTasksUi()
  const tasks = useViewTasks()
  const { data: groups = [] } = useQuery({ queryKey: ['groups'], queryFn: api.listGroups })
  const { data: config } = useConfigQuery()
  const qc = useQueryClient()
  const invalidate = () => qc.invalidateQueries({ queryKey: ['tasks'] })

  const batchPause = useMutation({
    mutationFn: (ids: string[]) => runBatch(ids, (id) => api.pauseTask(id)),
    onSuccess: invalidate,
  })
  const batchContinue = useMutation({
    mutationFn: (ids: string[]) => runBatch(ids, (id) => api.continueTask(id)),
    onSuccess: invalidate,
  })
  const batchDelete = useMutation({
    mutationFn: ({ ids, deleteFiles }: { ids: string[]; deleteFiles: boolean }) =>
      runBatch(ids, (id) => api.deleteTask(id, deleteFiles)),
    onSuccess: () => {
      invalidate()
      setSelected(new Set())
    },
  })
  // 失效任务两批的磁盘语义不同（对齐桌面 deleteStaleTasks）：文件已消失的
  // 只删记录；失败的任务可能留着谁也续不上的残片，记录一删就成孤儿文件。
  const cleanStale = useMutation({
    mutationFn: async ({ missing, failed }: { missing: string[]; failed: string[] }) => {
      await runBatch(missing, (id) => api.deleteTask(id, false))
      await runBatch(failed, (id) => api.deleteTask(id, true))
    },
    onSuccess: () => {
      invalidate()
      setSelected(new Set())
    },
  })

  if (!manageMode) return null

  const groupNameByGroupId = new Map(groups.map((g) => [g.groupId, groupDisplayName(g).toLowerCase()]))
  const categories = visibleCategories(parseCategories(config?.[CATEGORIES_KEY]))
  const visible = filterTasks(tasks, { statusTab, categoryFilter, categories, queueFilter, search, groupNameByGroupId })
  const allSelected = visible.length > 0 && visible.every((t) => selected.has(t.taskId))

  // 失效任务的作用域与「全选」一致：当前筛选后的可见列表，而非全量任务。
  const staleMissing = visible.filter((t) => t.status === 3 && t.fileMissing).map((t) => t.taskId)
  const staleFailed = visible.filter((t) => t.status === 4).map((t) => t.taskId)
  const staleCount = staleMissing.length + staleFailed.length

  function toggleAll(checked: boolean) {
    setSelected(checked ? new Set(visible.map((t) => t.taskId)) : new Set())
  }

  async function confirmDelete(deleteFiles: boolean) {
    const ids = Array.from(selected)
    if (ids.length === 0) return
    const ok = await confirmDialog({
      title: t(deleteFiles ? 'manage.deleteWithFilesTitle' : 'manage.deleteTitle'),
      message: t(deleteFiles ? 'manage.deleteWithFilesMsg' : 'manage.deleteMsg', { n: ids.length }),
      danger: true,
    })
    if (ok) batchDelete.mutate({ ids, deleteFiles })
  }

  return (
    <div className="manage-bar on">
      <label className="mcheck">
        <input type="checkbox" checked={allSelected} onChange={(e) => toggleAll(e.target.checked)} />
        <i />
        {t('common.selectAll')}
      </label>
      <span className="msel-pill">{t('manage.selected', { n: selected.size })}</span>
      <span className="flex1" />
      <button
        type="button"
        className="mbtn"
        disabled={selected.size === 0}
        onClick={() => batchPause.mutate(Array.from(selected))}
      >
        <Pause size={14} />
        {t('common.pause')}
      </button>
      <button
        type="button"
        className="mbtn"
        disabled={selected.size === 0}
        onClick={() => batchContinue.mutate(Array.from(selected))}
      >
        <Play size={14} />
        {t('common.resume')}
      </button>
      <button
        type="button"
        className="mbtn warn"
        disabled={staleCount === 0}
        onClick={async () => {
          if (
            await confirmDialog({
              title: t('manage.cleanStaleTitle'),
              message: t('manage.cleanStaleMsg', { n: staleCount }),
              danger: true,
            })
          )
            cleanStale.mutate({ missing: staleMissing, failed: staleFailed })
        }}
      >
        <BrushCleaning size={14} />
        {t('manage.cleanStale')}
      </button>
      <button type="button" className="mbtn" disabled={selected.size === 0} onClick={() => confirmDelete(false)}>
        <Trash2 size={14} />
        {t('manage.deleteTasks')}
      </button>
      <button type="button" className="mbtn danger" disabled={selected.size === 0} onClick={() => confirmDelete(true)}>
        <FileX size={14} />
        {t('manage.deleteWithFiles')}
      </button>
      <span className="vsep" />
      <button type="button" className="btn primary sm" onClick={() => setManageMode(false)}>
        <X size={14} />
        {t('common.cancel')}
      </button>
    </div>
  )
}
