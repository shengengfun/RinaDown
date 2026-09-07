// 单个插件的设置对话框 —— 点击卡片上的「设置」齿轮按钮弹出，按 widget 分发 controls.tsx
// 行组件渲染全部受支持控件（text/password/textarea/number/toggle/select/folder），提交前做
// required/pattern/min-max/select 成员前置校验（全部通过才发起 PUT，避免 all-or-nothing 请求半路失败）。

import { useEffect, useRef, useState } from 'react'
import * as Dialog from '@radix-ui/react-dialog'
import { Check, ClipboardCopy, Settings2, X } from 'lucide-react'
import { copyText } from '../../lib/copy'
import type { I18nKey } from '../../lib/i18n'
import { useI18n } from '../../lib/i18n'
import type { PluginDto, SettingFieldDto } from '../../lib/types'
import { FsPicker } from '../dialogs/fs-picker'
import { NumberFieldRow, SetRow, SetSelect, SetSwitch, TextAreaFieldRow, TextFieldRow } from './controls'

export interface PluginSettingsDialogProps {
  plugin: PluginDto
  saving?: boolean
  /** 校验通过后提交；`onDone` 供成功回调（用于关闭对话框）。 */
  onSave: (entries: Record<string, string>, onDone: () => void) => void
}

/** 单条设置项前置校验：required → number/min/max → pattern → select 成员。首个失败项即返回。 */
function validateField(field: SettingFieldDto, raw: string): I18nKey | null {
  const value = raw.trim()
  if (field.required && value === '') return 'plugins.err.required'
  if (value === '') return null
  if (field.type === 'number') {
    const n = Number(value)
    if (!Number.isFinite(n)) return 'plugins.err.number'
    if (field.min !== null && n < field.min) return 'plugins.err.min'
    if (field.max !== null && n > field.max) return 'plugins.err.max'
  }
  if (field.pattern) {
    let ok = true
    try {
      ok = new RegExp(field.pattern).test(value)
    } catch {
      ok = true // 插件提供的正则非法：不阻塞提交
    }
    if (!ok) return 'plugins.err.pattern'
  }
  if (field.widget === 'select' && field.options.length > 0 && !field.options.some((o) => o.value === value)) {
    return 'plugins.err.select'
  }
  return null
}

export function PluginSettingsDialog({ plugin, saving, onSave }: PluginSettingsDialogProps) {
  const { t } = useI18n()
  const [open, setOpen] = useState(false)
  const [values, setValues] = useState<Record<string, string>>(() => ({ ...plugin.settingsValues }))
  const [errors, setErrors] = useState<Partial<Record<string, I18nKey>>>({})

  // 每次打开都从插件当前已保存值重置表单（丢弃上次未提交的编辑）。
  useEffect(() => {
    if (open) {
      setValues({ ...plugin.settingsValues })
      setErrors({})
    }
  }, [open, plugin])

  function valueOf(field: SettingFieldDto): string {
    return values[field.key] ?? field.default ?? ''
  }

  function setValue(key: string, v: string) {
    setValues((prev) => ({ ...prev, [key]: v }))
    setErrors((prev) => {
      if (!(key in prev)) return prev
      const next = { ...prev }
      delete next[key]
      return next
    })
  }

  function submit() {
    const nextErrors: Partial<Record<string, I18nKey>> = {}
    for (const field of plugin.settings) {
      const err = validateField(field, valueOf(field))
      if (err) nextErrors[field.key] = err
    }
    setErrors(nextErrors)
    if (Object.keys(nextErrors).length > 0) return
    onSave(values, () => setOpen(false))
  }

  return (
    <Dialog.Root open={open} onOpenChange={setOpen}>
      <Dialog.Trigger asChild>
        <button type="button" className="icon-btn sm text-text3" title={t('plugins.configure')} aria-label={t('plugins.configure')}>
          <Settings2 size={14} />
        </button>
      </Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Overlay className="wbackdrop show" />
        <Dialog.Content
          asChild
          onPointerDownOutside={(e) => {
            // 表单对话框：点击外部不关闭（防误触丢失编辑，兼根治 Radix Select-in-Dialog
            // 展开时点内部元素被误判为 outside 而连带关闭的已知问题）。关闭路径：✕ / 取消 / Esc。
            e.preventDefault()
          }}
        >
          <div className="dialog show">
            <header className="dlg-head">
              <Dialog.Title asChild>
                <b>{t('plugins.settingsTitle', { name: plugin.name })}</b>
              </Dialog.Title>
              <Dialog.Close asChild>
                <button type="button" className="icon-btn sm" aria-label={t('common.close')}>
                  <X size={16} />
                </button>
              </Dialog.Close>
            </header>
            <Dialog.Description className="sr-only">{plugin.description || plugin.name}</Dialog.Description>
            <div className="dlg-body">
              <div className="set-group" style={{ marginBottom: 0 }}>
                {plugin.settings.map((field) => (
                  <div className="plugin-field" key={field.key}>
                    <SettingFieldRow field={field} value={valueOf(field)} onChange={(v) => setValue(field.key, v)} />
                    {field.helperScript && <HelperScriptButton field={field} />}
                    {errors[field.key] && <p className="px-4 pb-2 text-[11px] text-danger">{t(errors[field.key]!)}</p>}
                  </div>
                ))}
              </div>
            </div>
            <footer className="dlg-foot">
              <Dialog.Close asChild>
                <button type="button" className="btn ghost">
                  {t('common.cancel')}
                </button>
              </Dialog.Close>
              <button type="button" className="btn primary" disabled={saving} onClick={submit}>
                {saving ? t('common.loading') : t('plugins.saveSettings')}
              </button>
            </footer>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}

function SettingFieldRow({
  field,
  value,
  onChange,
}: {
  field: SettingFieldDto
  value: string
  onChange: (v: string) => void
}) {
  const { t } = useI18n()
  const title = (
    <>
      {field.title || field.key}
      {field.required && <span className="ml-0.5 text-danger">*</span>}
    </>
  )
  const desc = field.description || undefined
  switch (field.widget) {
    case 'password':
      return <TextFieldRow title={title} desc={desc} value={value} onCommit={onChange} password />
    case 'textarea':
      return <TextAreaFieldRow title={title} desc={desc} value={value} onCommit={onChange} />
    case 'number':
      return (
        <NumberFieldRow
          title={title}
          desc={desc}
          value={Number(value || '0')}
          onCommit={(n) => onChange(String(n))}
          min={field.min ?? undefined}
          max={field.max ?? undefined}
        />
      )
    case 'toggle':
      return (
        <SetRow title={title} desc={desc}>
          <SetSwitch checked={value === 'true'} onCheckedChange={(v) => onChange(v ? 'true' : 'false')} />
        </SetRow>
      )
    case 'select':
      return (
        <SetRow title={title} desc={desc}>
          <SetSelect
            value={value}
            onValueChange={onChange}
            options={field.options}
            placeholder={t('plugins.selectPlaceholder')}
          />
        </SetRow>
      )
    case 'folder':
      return (
        <SetRow title={title} desc={desc}>
          <div className="dir-row" style={{ width: 260, flexShrink: 0 }}>
            <input
              className="text-input"
              spellCheck={false}
              placeholder={t('plugins.folderPlaceholder')}
              value={value}
              onChange={(e) => onChange(e.target.value)}
            />
            <FsPicker value={value} onChange={onChange} />
          </div>
        </SetRow>
      )
    case 'text':
    default:
      return <TextFieldRow title={title} desc={desc} value={value} onCommit={onChange} />
  }
}

/** 字段级辅助脚本复制按钮：仅复制文本到剪贴板（绝不执行），供用户粘贴到目标
 *  网站的开发者工具 Console 运行（典型用途：提取 cookie）。 */
function HelperScriptButton({ field }: { field: SettingFieldDto }) {
  const { t } = useI18n()
  const [copied, setCopied] = useState(false)
  const timer = useRef<number | undefined>(undefined)
  useEffect(() => () => window.clearTimeout(timer.current), [])
  return (
    <div className="px-4 pb-2">
      <button
        type="button"
        className="btn ghost sm"
        onClick={() => {
          copyText(field.helperScript ?? '')
          setCopied(true)
          window.clearTimeout(timer.current)
          timer.current = window.setTimeout(() => setCopied(false), 2500)
        }}
      >
        {copied ? <Check size={13} /> : <ClipboardCopy size={13} />}
        {copied ? t('plugins.helperCopied') : field.helperLabel || t('plugins.copyHelper')}
      </button>
    </div>
  )
}
