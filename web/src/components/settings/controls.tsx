// 设置页通用行内控件 —— 对齐 design.css 的 .set-row/.set-info/.switch/.select/.text-input。
// Radix Switch/Select 复用原型视觉（轨道/滑块/下拉），文本类控件失焦提交避免逐字符请求。

import * as SelectPrimitive from '@radix-ui/react-select'
import * as SwitchPrimitive from '@radix-ui/react-switch'
import { Check, ChevronDown, ChevronUp } from 'lucide-react'
import { useEffect, useState, type ReactNode } from 'react'
import { cn } from '../../lib/cn'

export function SetRow({
  title,
  desc,
  children,
  align = 'center',
}: {
  title: ReactNode
  desc?: string
  children: ReactNode
  align?: 'center' | 'start'
}) {
  return (
    <div className="set-row" style={align === 'start' ? { alignItems: 'flex-start' } : undefined}>
      <div className="set-info">
        <b>{title}</b>
        {desc ? <span>{desc}</span> : null}
      </div>
      {children}
    </div>
  )
}

export function SetSwitch({
  checked,
  onCheckedChange,
  disabled,
}: {
  checked: boolean
  onCheckedChange: (v: boolean) => void
  disabled?: boolean
}) {
  return (
    <SwitchPrimitive.Root
      checked={checked}
      onCheckedChange={onCheckedChange}
      disabled={disabled}
      className="switch"
    >
      <SwitchPrimitive.Thumb className="switch-thumb" />
    </SwitchPrimitive.Root>
  )
}

export interface SelectOption {
  value: string
  label: string
}

/** Radix Select 不允许 Item 的 value 为空字符串（会被当作"清空 → 显示 placeholder"），
 *  用哨兵值在边界处映射（"" ↔ EMPTY_VALUE），使值为空串的选项也能正常回显选中态。 */
const EMPTY_VALUE = '__empty__'

export function SetSelect({
  value,
  onValueChange,
  options,
  width = 220,
  placeholder,
}: {
  value: string
  onValueChange: (v: string) => void
  options: SelectOption[]
  width?: number
  placeholder?: string
}) {
  return (
    <SelectPrimitive.Root
      value={value === '' ? EMPTY_VALUE : value}
      onValueChange={(v) => onValueChange(v === EMPTY_VALUE ? '' : v)}
    >
      <SelectPrimitive.Trigger className="select" style={{ width, flexShrink: 0 }}>
        <SelectPrimitive.Value placeholder={placeholder} />
        <SelectPrimitive.Icon>
          <ChevronDown />
        </SelectPrimitive.Icon>
      </SelectPrimitive.Trigger>
      <SelectPrimitive.Portal>
        <SelectPrimitive.Content
          position="popper"
          sideOffset={6}
          className="select-pop"
          style={{
            minWidth: 'var(--radix-select-trigger-width)',
            maxHeight: 'min(320px, var(--radix-select-content-available-height))',
            boxShadow: 'var(--shadow)',
          }}
        >
          <SelectPrimitive.ScrollUpButton className="select-scroll">
            <ChevronUp className="h-3.5 w-3.5" />
          </SelectPrimitive.ScrollUpButton>
          <SelectPrimitive.Viewport>
            {options.map((o) => (
              <SelectPrimitive.Item
                key={o.value}
                value={o.value === '' ? EMPTY_VALUE : o.value}
                className="select-item"
              >
                <SelectPrimitive.ItemText>{o.label}</SelectPrimitive.ItemText>
                <SelectPrimitive.ItemIndicator className="select-item-check">
                  <Check className="h-3.5 w-3.5" />
                </SelectPrimitive.ItemIndicator>
              </SelectPrimitive.Item>
            ))}
          </SelectPrimitive.Viewport>
          <SelectPrimitive.ScrollDownButton className="select-scroll">
            <ChevronDown className="h-3.5 w-3.5" />
          </SelectPrimitive.ScrollDownButton>
        </SelectPrimitive.Content>
      </SelectPrimitive.Portal>
    </SelectPrimitive.Root>
  )
}

/** 受控文本输入：本地编辑态，失焦时若与已提交值不同才回调提交。 */
export function TextInput({
  value,
  onCommit,
  placeholder,
  password,
  className,
}: {
  value: string
  onCommit: (v: string) => void
  placeholder?: string
  password?: boolean
  className?: string
}) {
  const [text, setText] = useState(value)
  useEffect(() => setText(value), [value])
  return (
    <input
      className={cn('text-input', className)}
      type={password ? 'password' : 'text'}
      spellCheck={false}
      placeholder={placeholder}
      value={text}
      onChange={(e) => setText(e.target.value)}
      onBlur={() => {
        if (text !== value) onCommit(text)
      }}
    />
  )
}

export function NumberInput({
  value,
  onCommit,
  min,
  max,
  className,
}: {
  value: number
  onCommit: (v: number) => void
  min?: number
  max?: number
  className?: string
}) {
  const [text, setText] = useState(String(value))
  useEffect(() => setText(String(value)), [value])
  return (
    <input
      className={cn('text-input', className)}
      type="number"
      min={min}
      max={max}
      value={text}
      onChange={(e) => setText(e.target.value)}
      onBlur={() => {
        const n = Number(text)
        if (Number.isFinite(n) && n !== value) onCommit(n)
        else setText(String(value))
      }}
    />
  )
}

export function TextAreaInput({
  value,
  onCommit,
  rows = 4,
  placeholder,
  width = '100%',
}: {
  value: string
  onCommit: (v: string) => void
  rows?: number
  placeholder?: string
  width?: number | string
}) {
  const [text, setText] = useState(value)
  useEffect(() => setText(value), [value])
  return (
    <textarea
      className="text-input area"
      style={{ width }}
      spellCheck={false}
      rows={rows}
      placeholder={placeholder}
      value={text}
      onChange={(e) => setText(e.target.value)}
      onBlur={() => {
        if (text !== value) onCommit(text)
      }}
    />
  )
}

export function TextFieldRow({
  title,
  desc,
  value,
  onCommit,
  placeholder,
  password,
}: {
  title: ReactNode
  desc?: string
  value: string
  onCommit: (v: string) => void
  placeholder?: string
  password?: boolean
}) {
  return (
    <SetRow title={title} desc={desc}>
      <TextInput value={value} onCommit={onCommit} placeholder={placeholder} password={password} />
    </SetRow>
  )
}

export function NumberFieldRow({
  title,
  desc,
  value,
  onCommit,
  min,
  max,
  short = true,
}: {
  title: ReactNode
  desc?: string
  value: number
  onCommit: (v: number) => void
  min?: number
  max?: number
  short?: boolean
}) {
  return (
    <SetRow title={title} desc={desc}>
      <NumberInput value={value} onCommit={onCommit} min={min} max={max} className={short ? 'short' : undefined} />
    </SetRow>
  )
}

export function TextAreaFieldRow({
  title,
  desc,
  value,
  onCommit,
  placeholder,
}: {
  title: ReactNode
  desc?: string
  value: string
  onCommit: (v: string) => void
  placeholder?: string
}) {
  return (
    <div className="set-row stack">
      <div className="set-info">
        <b>{title}</b>
        {desc ? <span>{desc}</span> : null}
      </div>
      <TextAreaInput value={value} onCommit={onCommit} placeholder={placeholder} />
    </div>
  )
}
