// 竖向拖拽分隔条：侧边栏 180–320 / 详情面板 240–420，localStorage 持久化。
// 宽度经 CSS 变量挂在外层 `.wscreen` 上，拖拽把手直接写 DOM（不走 React state，
// 拖动期间零重渲染），松手才落盘。

import { useRef, type PointerEvent as ReactPointerEvent } from 'react'

export interface PanelWidthConf {
  key: string
  def: number
  min: number
  max: number
}

export const SIDEBAR_W: PanelWidthConf = { key: 'fluxdown.sidebarWidth', def: 220, min: 180, max: 320 }
export const DETAIL_W: PanelWidthConf = { key: 'fluxdown.detailWidth', def: 340, min: 240, max: 420 }

export function loadWidth(c: PanelWidthConf): number {
  const v = Number(localStorage.getItem(c.key))
  return Number.isFinite(v) && v >= c.min && v <= c.max ? v : c.def
}

/** `invert`：右侧面板向左拖为加宽（delta 取反）。 */
export function ColResizer({ cssVar, conf, invert, className }: { cssVar: string; conf: PanelWidthConf; invert?: boolean; className?: string }) {
  const ref = useRef<HTMLDivElement>(null)
  const drag = useRef<{ startX: number; startW: number } | null>(null)
  const screenOf = () => ref.current?.closest<HTMLElement>('.wscreen') ?? null

  const onPointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    const sec = screenOf()
    if (!sec || !ref.current) return
    const cur = parseFloat(getComputedStyle(sec).getPropertyValue(cssVar))
    drag.current = { startX: e.clientX, startW: Number.isFinite(cur) ? cur : conf.def }
    ref.current.setPointerCapture(e.pointerId)
    ref.current.classList.add('active')
  }
  const onPointerMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    const sec = screenOf()
    if (!drag.current || !sec) return
    const delta = e.clientX - drag.current.startX
    const w = Math.min(conf.max, Math.max(conf.min, drag.current.startW + (invert ? -delta : delta)))
    sec.style.setProperty(cssVar, `${w}px`)
  }
  const onPointerUp = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!drag.current || !ref.current) return
    drag.current = null
    ref.current.releasePointerCapture(e.pointerId)
    ref.current.classList.remove('active')
    const sec = screenOf()
    const w = sec ? parseFloat(getComputedStyle(sec).getPropertyValue(cssVar)) : NaN
    if (Number.isFinite(w)) localStorage.setItem(conf.key, String(Math.round(w)))
  }

  return (
    <div
      ref={ref}
      className={`col-resizer ${className ?? ''}`}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
    />
  )
}
