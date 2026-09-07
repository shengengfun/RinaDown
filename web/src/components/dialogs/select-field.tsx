// Radix Select 的通用值选择字段封装 —— 抽自 new-download.tsx，供 new-download.tsx /
// manifest-select.tsx 共用（避免同一 Select 外观出现两套实现）。

import { Fragment } from 'react'
import * as Select from '@radix-ui/react-select'
import { Check, ChevronDown } from 'lucide-react'

/** Radix Select 不允许 Item 的 value 为空字符串，用哨兵值代表"未设置/默认"语义。 */
const EMPTY_VALUE = '__default__'

interface Option {
  value: string
  label: string
  group?: string
}

/** 把相邻同 `group` 的选项聚成一块。`Select.Label` 必须包在 `Select.Group` 里
 *  （Radix 运行时断言，静态类型查不出来），所以不能在扁平 map 里就地插标题——
 *  必须先分块，再整块包 `Group`。分块只按「相邻」判定，与调用方「同组选项须相邻」
 *  的约定一致：同名分组若被隔开，会得到两块各带一次标题，不会静默合并。 */
function chunkByGroup(options: Option[]): { group?: string; items: Option[] }[] {
  const chunks: { group?: string; items: Option[] }[] = []
  for (const option of options) {
    const last = chunks[chunks.length - 1]
    if (last && last.group === option.group) last.items.push(option)
    else chunks.push({ group: option.group, items: [option] })
  }
  return chunks
}

export function SelectField({
  value,
  onChange,
  options,
  ariaLabel,
}: {
  value: string
  onChange: (v: string) => void
  /** group：可选分组标题；同一分组的选项须相邻，组内第一项前渲染一次标题（如"云设备"/
   *  "本地设备"，见 new-download.tsx 设备选择器）。 */
  options: Option[]
  ariaLabel: string
}) {
  const renderItem = (o: Option) => (
    <Select.Item key={o.value || EMPTY_VALUE} value={o.value === '' ? EMPTY_VALUE : o.value} className="select-item">
      <Select.ItemText>{o.label}</Select.ItemText>
      <Select.ItemIndicator className="select-item-check">
        <Check size={14} />
      </Select.ItemIndicator>
    </Select.Item>
  )
  return (
    <Select.Root value={value === '' ? EMPTY_VALUE : value} onValueChange={(v) => onChange(v === EMPTY_VALUE ? '' : v)}>
      <Select.Trigger className="select w-full" aria-label={ariaLabel}>
        <Select.Value className="min-w-0 flex-1 truncate text-left" />
        <Select.Icon className="shrink-0 text-text3">
          <ChevronDown size={14} />
        </Select.Icon>
      </Select.Trigger>
      <Select.Portal>
        <Select.Content position="popper" sideOffset={6} className="select-pop" style={{ width: 'var(--radix-select-trigger-width)' }}>
          <Select.Viewport className="max-h-64">
            {chunkByGroup(options).map((chunk, i) =>
              chunk.group ? (
                <Select.Group key={`${chunk.group}-${i}`}>
                  <Select.Label className="select-group-label">{chunk.group}</Select.Label>
                  {chunk.items.map(renderItem)}
                </Select.Group>
              ) : (
                <Fragment key={`ungrouped-${i}`}>{chunk.items.map(renderItem)}</Fragment>
              ),
            )}
          </Select.Viewport>
        </Select.Content>
      </Select.Portal>
    </Select.Root>
  )
}
