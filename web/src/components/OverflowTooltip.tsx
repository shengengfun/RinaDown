// 溢出才弹全文的省略文本（对齐桌面 `lib/src/widgets/overflow_tooltip_text.dart`）。
//
// 为什么不用原生 `title=`：浏览器气泡延迟约 1s 不可调、不跟主题、长文本的换行策略
// 由系统决定，而 RSS 条目标题恰恰是「关键信息在末尾」的长文本。改用 Radix Tooltip，
// 延迟、样式、宽度上限全部自持。
//
// 只在**确实被省略号截断**时才挂气泡：短标题弹一个内容完全相同的气泡是纯噪音。
// 判定放在 `onOpenChange` 里而不是 render 期——`scrollWidth`/`clientWidth` 是布局
// 结果，render 时读会强制同步布局，而列表一次渲染几十行；等真要弹了再量一次，
// 且量的是当前列宽（窗口缩放后自动跟着变，不需要 ResizeObserver）。

import { useRef, useState, type ReactNode } from 'react'
import * as Tooltip from '@radix-ui/react-tooltip'

/** 悬浮多久弹出全文，与桌面端 `kOverflowTooltipDelay` 保持一致。 */
export const OVERFLOW_TOOLTIP_DELAY = 500

/** 单行 ellipsis 与多行 line-clamp 都靠这一条判定：渲染尺寸超过可视区即被截断。 */
function isTruncated(el: HTMLElement | null): boolean {
  if (!el) return false
  return el.scrollWidth > el.clientWidth + 1 || el.scrollHeight > el.clientHeight + 1
}

export function OverflowTooltip({
  text,
  tip,
  as = 'span',
  className,
  side = 'top',
  align = 'start',
}: {
  /** 显示文本，同时也是未指定 `tip` 时的气泡内容。 */
  text: string
  /** 气泡内容；省略则用 `text`。 */
  tip?: ReactNode
  /** 渲染成哪个标签——截断样式常写在 `.rss-row-name b` 这类选择器上。 */
  as?: 'span' | 'b' | 'div'
  className?: string
  side?: 'top' | 'bottom'
  align?: 'start' | 'center' | 'end'
}) {
  const ref = useRef<HTMLSpanElement>(null)
  const [open, setOpen] = useState(false)
  const Tag = as as 'span'

  return (
    <Tooltip.Root
      open={open}
      onOpenChange={(next) => {
        if (next && !isTruncated(ref.current)) return
        setOpen(next)
      }}
      delayDuration={OVERFLOW_TOOLTIP_DELAY}
    >
      <Tooltip.Trigger asChild>
        <Tag ref={ref} className={className}>
          {text}
        </Tag>
      </Tooltip.Trigger>
      <Tooltip.Portal>
        {/* 贴边留白与上下翻转交给 Radix 的碰撞检测；横向宽度上限见 design.css
            的 .ovf-tip（吃 --radix-tooltip-content-available-width），窗口比气泡
            窄时自动收紧并换行，不会横向溢出视口。 */}
        <Tooltip.Content className="ovf-tip" side={side} align={align} sideOffset={6} collisionPadding={8}>
          {tip ?? text}
        </Tooltip.Content>
      </Tooltip.Portal>
    </Tooltip.Root>
  )
}
