// 跨设备任务行：展示执行端回报的状态/进度，并让发起端能暂停/恢复/取消（POST
// /tasks/{id}/command）。真实状态永远由执行端经 SSE task.status 回报——这里的按钮
// 只负责发指令 + 等结果，绝不本地伪造下一状态；命令失败时按钮态原样弹回，等用户
// 再次点击或状态自然收敛。视觉与交互复用本地任务行（TaskRow.tsx）的 task-act 范式，
// 不另创控件样式。

import { useMutation } from '@tanstack/react-query'
import { Download, Loader2, Pause, Play, X } from 'lucide-react'
import type { ReactNode } from 'react'
import { cloudApi } from '../../lib/cloud/client'
import { CloudApiError, type RemoteTask, type RemoteTaskAction, type RemoteTaskStatus } from '../../lib/cloud/types'
import { cn } from '../../lib/cn'
import { confirmDialog } from '../../lib/confirm'
import { fmtBytes, fmtSpeed } from '../../lib/format'
import { useI18n, type I18nKey } from '../../lib/i18n'
import { toast } from '../../lib/toast'
import type { ViewDensity } from '../../lib/view-prefs'

/** 跨设备任务状态 → 文案键（mdc §1.1 状态机）。 */
const REMOTE_STATUS_KEY: Record<RemoteTaskStatus, I18nKey> = {
  pending: 'remote.status.pending',
  accepted: 'remote.status.accepted',
  downloading: 'remote.status.downloading',
  paused: 'remote.status.paused',
  completed: 'remote.status.completed',
  failed: 'remote.status.failed',
  canceled: 'remote.status.canceled',
}

/** 命令失败的错误码 → 文案：离线/终态冲突有专门措辞，其余原样带出服务端 message
 *  （契约里这些 message 本就是给人看的中文句子，不走 translateBackendMessage）。 */
function commandErrorText(t: (key: I18nKey, params?: Record<string, string | number>) => string, err: unknown): string {
  if (err instanceof CloudApiError) {
    if (err.code === 'target_device_offline') return t('remote.cmdErrorOffline')
    if (err.code === 'task_state_conflict') return t('remote.cmdErrorConflict')
    return t('remote.cmdErrorGeneric', { error: err.message })
  }
  return t('remote.cmdErrorGeneric', { error: err instanceof Error ? err.message : String(err) })
}

function RemoteTaskActions({
  task,
  busyAction,
  onPause,
  onResume,
  onCancel,
}: {
  task: RemoteTask
  busyAction: RemoteTaskAction | null
  onPause: () => void
  onResume: () => void
  onCancel: () => void
}) {
  const { t } = useI18n()
  const disabled = busyAction !== null
  const spinnerOr = (action: RemoteTaskAction, icon: ReactNode) => (busyAction === action ? <Loader2 size={15} className="animate-spin" /> : icon)

  const cancelBtn = (
    <button type="button" className="task-act" title={t('remote.cancel')} disabled={disabled} onClick={onCancel}>
      {spinnerOr('cancel', <X size={15} />)}
    </button>
  )

  if (task.status === 'downloading')
    return (
      <>
        <button type="button" className="task-act" title={t('remote.pause')} disabled={disabled} onClick={onPause}>
          {spinnerOr('pause', <Pause size={15} />)}
        </button>
        {cancelBtn}
      </>
    )
  if (task.status === 'paused')
    return (
      <>
        <button type="button" className="task-act" title={t('remote.resume')} disabled={disabled} onClick={onResume}>
          {spinnerOr('resume', <Play size={15} />)}
        </button>
        {cancelBtn}
      </>
    )
  if (task.status === 'pending' || task.status === 'accepted') return cancelBtn
  return null
}

export function RemoteTaskRow({ task, density = 'comfortable' }: { task: RemoteTask; density?: ViewDensity }) {
  const { t } = useI18n()
  const isCompact = density === 'compact'

  const commandMut = useMutation({
    mutationFn: (action: RemoteTaskAction) => cloudApi.commandTask(task.id, action),
    onError: (err) => toast(commandErrorText(t, err), 'error'),
  })
  const busyAction = commandMut.isPending ? (commandMut.variables ?? null) : null

  async function cancel() {
    if (await confirmDialog({ title: t('remote.cancelTitle'), message: t('remote.cancelMsg'), danger: true })) commandMut.mutate('cancel')
  }

  const statusKey = REMOTE_STATUS_KEY[task.status]

  return (
    <div className={cn('task-row', isCompact && 'compact')}>
      <span className="trow-icon">
        <Download size={19} />
      </span>
      <div className="trow-main">
        <div className="trow-name">
          <b>{task.fileName || task.url}</b>
        </div>
        <div className="trow-meta">
          <span>{statusKey ? t(statusKey) : task.status}</span>
          {task.status === 'downloading' && (
            <>
              <span>
                {' '}
                · {fmtBytes(task.downloadedBytes)}
                {task.totalBytes ? ` / ${fmtBytes(task.totalBytes)}` : ''}
              </span>
              <span> · {fmtSpeed(task.speed)}</span>
            </>
          )}
          {task.error && <span className="text-danger"> · {task.error}</span>}
        </div>
        <div className="trow-bar">
          <i style={{ width: `${Math.round((task.progress || 0) * 100)}%` }} />
        </div>
      </div>
      <div className="trow-side">
        <span className="trow-pct">{Math.round((task.progress || 0) * 100)}%</span>
        <RemoteTaskActions
          task={task}
          busyAction={busyAction}
          onPause={() => commandMut.mutate('pause')}
          onResume={() => commandMut.mutate('resume')}
          onCancel={() => void cancel()}
        />
      </div>
    </div>
  )
}
