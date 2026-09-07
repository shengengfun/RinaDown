// 重命名任务文件对话框 —— 由 renameTaskStore（TaskContextMenu 触发）驱动开关。
// 成功后引擎经 WS 推送任务列表更新；此处照既有任务操作惯例再 invalidate ['tasks'] 兜底。
// 失败时把服务端透传的稳定错误码映射为本地化文案，未识别错误走兜底键展示原文。

import { useEffect, useState } from 'react'
import * as Dialog from '@radix-ui/react-dialog'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { X } from 'lucide-react'
import { api, ApiError } from '../../lib/api'
import { renameTaskStore } from '../../lib/dialogs'
import type { I18nKey } from '../../lib/i18n'
import { useI18n } from '../../lib/i18n'
import { useStore } from '../../lib/ws'

/** 引擎稳定错误码 → 文案键（见 native/engine rename_task 契约）。 */
const RENAME_ERROR_KEYS: Record<string, I18nKey> = {
  'invalid-name': 'task.renameErrInvalidName',
  'task-active': 'task.renameErrTaskActive',
  'bt-unsupported': 'task.renameErrBtUnsupported',
  'target-exists': 'task.renameErrTargetExists',
  'not-found': 'task.renameErrNotFound',
}

export function RenameTaskDialog() {
  const { t } = useI18n()
  const payload = useStore(renameTaskStore)
  const open = payload !== null
  const qc = useQueryClient()
  const [name, setName] = useState('')
  const [error, setError] = useState('')

  // 每次新请求到达时，输入框预填当前文件名并清空上次错误。
  useEffect(() => {
    if (!payload) return
    setName(payload.fileName)
    setError('')
  }, [payload])

  const renameMut = useMutation({
    mutationFn: ({ taskId, fileName }: { taskId: string; fileName: string }) => api.renameTask(taskId, fileName),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['tasks'] })
      renameTaskStore.set(null)
    },
    onError: (err: Error) => {
      // 任务不存在时服务端按既有惯例回 404 + "not found"（且 message 可能已被
      // translateBackendMessage 本地化），按 HTTP 状态映射而非匹配字符串。
      if (err instanceof ApiError && err.status === 404) {
        setError(t('task.renameErrNotFound'))
        return
      }
      const key = RENAME_ERROR_KEYS[err.message.trim()]
      setError(key ? t(key) : t('task.renameErrUnknown', { error: err.message }))
    },
  })

  function cancel() {
    renameTaskStore.set(null)
  }

  function confirm() {
    if (!payload) return
    const trimmed = name.trim()
    if (!trimmed) {
      setError(t('task.renameErrInvalidName'))
      return
    }
    if (trimmed === payload.fileName) {
      renameTaskStore.set(null)
      return
    }
    setError('')
    renameMut.mutate({ taskId: payload.taskId, fileName: trimmed })
  }

  return (
    <Dialog.Root
      open={open}
      onOpenChange={(o) => {
        if (!o) cancel()
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="wbackdrop show" />
        <Dialog.Content className="dialog sm show" aria-describedby={undefined}>
          <header className="dlg-head">
            <Dialog.Title asChild>
              <b>{t('task.renameTitle')}</b>
            </Dialog.Title>
            <Dialog.Close asChild>
              <button type="button" className="icon-btn sm" aria-label={t('common.close')}>
                <X size={16} />
              </button>
            </Dialog.Close>
          </header>
          <form
            onSubmit={(e) => {
              e.preventDefault()
              confirm()
            }}
          >
            <div className="dlg-body">
              <label className="field-label" htmlFor="rename-filename">
                {t('task.renameLabel')}
              </label>
              <input
                id="rename-filename"
                className="text-input"
                type="text"
                spellCheck={false}
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder={t('task.renamePlaceholder')}
                autoFocus
                onFocus={(e) => {
                  // 预填名默认全选，便于直接输入替换；保留后缀可自行调整光标。
                  e.target.select()
                }}
              />
              {error && <p className="mt-2 text-xs break-all text-danger">{error}</p>}
            </div>
            <footer className="dlg-foot">
              <Dialog.Close asChild>
                <button type="button" className="btn ghost">
                  {t('common.cancel')}
                </button>
              </Dialog.Close>
              <button type="submit" className="btn primary" disabled={renameMut.isPending || !name.trim()}>
                {t('task.rename')}
              </button>
            </footer>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
