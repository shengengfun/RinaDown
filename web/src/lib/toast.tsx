// 全局 toast 通知 —— 模块级队列 + 单个宿主组件（main.tsx 只挂一次）。
//
// 用轻量外部 store（复用 lib/ws.ts 的 Store）而不是 Context：需要弹提示的地方
// 大多不是组件——mutation 的 per-call 回调、WS 分派、网络层——直接 import 一个
// 函数调用才不用把 hook 一层层往下传。
//
// 语义定位：toast 只承载「已经发生、无需回应」的一次性反馈；需要用户决策的
// 一律走 components/dialogs/confirm-dialog.tsx，别把两者混为一谈。

import { useEffect } from 'react'
import { cn } from './cn'
import { Store, useStore } from './ws'

export type ToastKind = 'info' | 'error'

export interface ToastItem {
  id: number
  text: string
  kind: ToastKind
}

/** 同屏上限：再多就把屏幕底部糊成一堵墙，最老的先走。 */
const MAX_VISIBLE = 3
/** 自动消失时长。报错留得久些——用户得把错误文本读完才知道下一步做什么。 */
const DURATION_MS: Record<ToastKind, number> = { info: 3000, error: 4500 }

const toastStore = new Store<ToastItem[]>([])

/** 计时器按 id 索引：手动关闭或被挤出队列时要能精确撤销那一个，
 *  否则回调会在 id 复用/队列变动后打到别的条目上。 */
const timers = new Map<number, ReturnType<typeof setTimeout>>()
let nextId = 1

function clearTimer(id: number) {
  const handle = timers.get(id)
  if (handle === undefined) return
  clearTimeout(handle)
  timers.delete(id)
}

function dismiss(id: number) {
  clearTimer(id)
  toastStore.set((prev) => prev.filter((it) => it.id !== id))
}

/** 弹一条通知。任何位置（含非组件代码）可直接调用。 */
export function toast(text: string, kind: ToastKind = 'info') {
  const id = nextId++
  toastStore.set((prev) => {
    const next = [...prev, { id, text, kind }]
    // 溢出的老条目连同计时器一起丢弃，避免 timers 里留下指向已移除条目的孤儿。
    while (next.length > MAX_VISIBLE) clearTimer(next.shift()!.id)
    return next
  })
  timers.set(
    id,
    setTimeout(() => dismiss(id), DURATION_MS[kind]),
  )
}

/** 通知宿主：全局挂一个即可（见 main.tsx）。 */
export function ToastHost() {
  const items = useStore(toastStore)

  // 宿主卸载（根重挂 / HMR）时撤掉所有在飞的计时器并清空队列，
  // 否则它们会继续对着不再渲染的 store 空转。
  useEffect(
    () => () => {
      for (const handle of timers.values()) clearTimeout(handle)
      timers.clear()
      toastStore.set([])
    },
    [],
  )

  // 容器常驻：aria-live 区域必须在内容变化之前就存在于 DOM 里，
  // 条件渲染整个容器会让屏幕阅读器错过第一条通知。
  return (
    <div className="wtoast-host" role="status" aria-live="polite">
      {items.map((it) => (
        <button
          key={it.id}
          type="button"
          className={cn('wtoast', it.kind === 'error' && 'err')}
          title={it.text}
          onClick={() => dismiss(it.id)}
        >
          {it.text}
        </button>
      ))}
    </div>
  )
}
