// 任务行右键菜单。纯展示 + 回调 props，不持有任何 mutation（由 TaskRow 统一持有并下发），
// 对齐 design/web/app.js ctxItems()。

import * as ContextMenu from '@radix-ui/react-context-menu'
import { ChevronRight, Copy, Download, Link2, ListOrdered, Pause, Pencil, Play, RotateCcw, Trash2, Zap } from 'lucide-react'
import type { ReactNode } from 'react'
import { taskFileUrl } from '../../lib/api'
import { confirmDialog } from '../../lib/confirm'
import { openRenameTask } from '../../lib/dialogs'
import { copyText } from '../../lib/copy'
import { queueDisplayName, taskShareUrl } from '../../lib/format'
import { useI18n } from '../../lib/i18n'
import type { QueueDto } from '../../lib/types'
import { isSeeding, isSeedingStopped } from '../../lib/seeding'
import type { ViewTask } from './useViewTasks'

export function TaskContextMenu({
  task: t,
  queues,
  onPause,
  onContinue,
  onBoost,
  onDelete,
  onMove,
  children,
}: {
  task: ViewTask
  queues: QueueDto[]
  onPause: () => void
  onContinue: () => void
  onBoost: () => void
  onDelete: (deleteFiles: boolean) => void
  onMove: (queueId: string) => void
  children: ReactNode
}) {
  const { t: tr } = useI18n()
  // BT 任务（磁力/种子）不支持重命名（多文件语义）；下载中/准备中亦不可改名，
  // 与引擎 rename_task 的 bt-unsupported / task-active 拒绝条件对齐。
  const isBt = t.url.startsWith('magnet:') || t.url.startsWith('torrent-file://') || t.url.endsWith('.torrent')
  const canRename = !isBt && t.status !== 1 && t.status !== 5
  return (
    <ContextMenu.Root>
      {/* 右键只弹菜单，不选中/不打开详情面板（对齐需求：仅左键单击打开）。 */}
      <ContextMenu.Trigger asChild>
        {children}
      </ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Content className="ctxmenu show">
          {(t.status === 1 || t.status === 5) && (
            <ContextMenu.Item className="ctx-item" onSelect={onPause}>
              <Pause size={14} />
              {tr('task.pause')}
            </ContextMenu.Item>
          )}
          {(t.status === 2 || t.status === 0) && (
            <ContextMenu.Item className="ctx-item" onSelect={onContinue}>
              <Play size={14} />
              {tr('task.resume')}
            </ContextMenu.Item>
          )}
          {t.status === 4 && (
            <ContextMenu.Item className="ctx-item" onSelect={onContinue}>
              <RotateCcw size={14} />
              {tr('task.retry')}
            </ContextMenu.Item>
          )}
          {t.status !== 3 && (
            <ContextMenu.Item className="ctx-item" onSelect={onBoost}>
              <Zap size={14} />
              {tr('task.boost')}
            </ContextMenu.Item>
          )}
          {/* 做种中 → 暂停（停止做种）；停止态 → 继续做种（复用 pause/continue API，对齐桌面 §3）。 */}
          {isSeeding(t) && (
            <ContextMenu.Item className="ctx-item" onSelect={onPause}>
              <Pause size={14} />
              {tr('task.pause')}
            </ContextMenu.Item>
          )}
          {isSeedingStopped(t) && (
            <ContextMenu.Item className="ctx-item" onSelect={onContinue}>
              <Play size={14} />
              {tr('task.resumeSeeding')}
            </ContextMenu.Item>
          )}
          {t.status === 3 && !t.fileMissing && (
            <ContextMenu.Item
              className="ctx-item"
              onSelect={() => {
                location.href = taskFileUrl(t.taskId)
              }}
            >
              <Download size={14} />
              {tr('task.saveToLocal')}
            </ContextMenu.Item>
          )}
          <ContextMenu.Item className="ctx-item" onSelect={() => copyText(taskShareUrl(t))}>
            <Copy size={14} />
            {tr('task.copyUrl')}
          </ContextMenu.Item>
          <ContextMenu.Item className="ctx-item" onSelect={() => copyText(`${t.saveDir}/${t.fileName}`)}>
            <Link2 size={14} />
            {tr('task.copyPath')}
          </ContextMenu.Item>
          {canRename && (
            <ContextMenu.Item className="ctx-item" onSelect={() => openRenameTask({ taskId: t.taskId, fileName: t.fileName })}>
              <Pencil size={14} />
              {tr('task.rename')}
            </ContextMenu.Item>
          )}
          {queues.filter((q) => q.queueId !== t.queueId).length > 0 && (
            <ContextMenu.Sub>
              <ContextMenu.SubTrigger className="ctx-item">
                <ListOrdered size={14} />
                {tr('task.moveToQueue')}
                <ChevronRight size={13} style={{ marginLeft: 'auto' }} />
              </ContextMenu.SubTrigger>
              <ContextMenu.Portal>
                <ContextMenu.SubContent className="ctxmenu show">
                  {queues
                    .filter((q) => q.queueId !== t.queueId)
                    .map((q) => (
                      <ContextMenu.Item key={q.queueId} className="ctx-item" onSelect={() => onMove(q.queueId)}>
                        {queueDisplayName(q)}
                      </ContextMenu.Item>
                    ))}
                </ContextMenu.SubContent>
              </ContextMenu.Portal>
            </ContextMenu.Sub>
          )}
          <ContextMenu.Separator className="ctx-sep" />
          <ContextMenu.Item className="ctx-item danger" onSelect={() => onDelete(false)}>
            <Trash2 size={14} />
            {tr('task.delete')}
          </ContextMenu.Item>
          <ContextMenu.Item
            className="ctx-item danger"
            onSelect={async () => {
              if (await confirmDialog({ title: tr('task.deleteTitle'), message: tr('task.deleteWithFilesMsg'), danger: true })) onDelete(true)
            }}
          >
            <Trash2 size={14} />
            {tr('task.deleteWithFiles')}
          </ContextMenu.Item>
        </ContextMenu.Content>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  )
}
