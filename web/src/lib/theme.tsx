// 主题（浅/深/跟随系统）+ 强调色，localStorage 持久化，纯前端。

import { createContext, useCallback, useContext, useEffect, useMemo, useState } from 'react'

export type ThemeMode = 'light' | 'dark' | 'system'

/** 预设强调色（虹咲学园角色应援色，与桌面端 `AppColorScheme` 同基线、同顺序）；
 *  名称走 i18n，不在此写死文案。 */
export const ACCENT_PRESETS = [
  { nameKey: 'set.appearance.accentAyumu', color: '#ED7D95' },
  { nameKey: 'set.appearance.accentKasumi', color: '#E7D600' },
  { nameKey: 'set.appearance.accentShizuku', color: '#01B7ED' },
  { nameKey: 'set.appearance.accentKarin', color: '#485EC6' },
  { nameKey: 'set.appearance.accentAi', color: '#FF5800' },
  { nameKey: 'set.appearance.accentKanata', color: '#A664A0' },
  { nameKey: 'set.appearance.accentSetsuna', color: '#D81C2F' },
  { nameKey: 'set.appearance.accentEmma', color: '#84C36E' },
  { nameKey: 'set.appearance.accentRina', color: '#9CA5B9' },
  { nameKey: 'set.appearance.accentShioriko', color: '#37B484' },
  { nameKey: 'set.appearance.accentMia', color: '#A9A898' },
  { nameKey: 'set.appearance.accentLanzhu', color: '#F69992' },
  { nameKey: 'set.appearance.accentYu', color: '#1D1D1D' },
] as const

/** 默认强调色索引（钟岚珠 F69992），与桌面端 `AppColorScheme.fallback` 一致。 */
const DEFAULT_ACCENT = 11

const MODE_KEY = 'rinadown.theme'
const ACCENT_KEY = 'rinadown.accent'

interface ThemeCtx {
  mode: ThemeMode
  setMode: (m: ThemeMode) => void
  accent: number
  setAccent: (i: number) => void
}

const Ctx = createContext<ThemeCtx>({ mode: 'system', setMode: () => {}, accent: DEFAULT_ACCENT, setAccent: () => {} })

function systemDark(): boolean {
  return window.matchMedia('(prefers-color-scheme: dark)').matches
}

function apply(mode: ThemeMode, accent: number) {
  const dark = mode === 'dark' || (mode === 'system' && systemDark())
  document.documentElement.classList.toggle('dark', dark)
  const preset = ACCENT_PRESETS[accent] ?? ACCENT_PRESETS[DEFAULT_ACCENT]
  document.documentElement.style.setProperty('--accent', preset.color)
}

export function ThemeProvider({ children }: { children: React.ReactNode }) {
  const [mode, setModeState] = useState<ThemeMode>(
    () => (localStorage.getItem(MODE_KEY) as ThemeMode) || 'system',
  )
  const [accent, setAccentState] = useState<number>(() =>
    parseInt(localStorage.getItem(ACCENT_KEY) ?? String(DEFAULT_ACCENT), 10),
  )

  useEffect(() => {
    apply(mode, accent)
    if (mode !== 'system') return
    const mq = window.matchMedia('(prefers-color-scheme: dark)')
    const onChange = () => apply(mode, accent)
    mq.addEventListener('change', onChange)
    return () => mq.removeEventListener('change', onChange)
  }, [mode, accent])

  const setMode = useCallback((m: ThemeMode) => {
    localStorage.setItem(MODE_KEY, m)
    setModeState(m)
  }, [])
  const setAccent = useCallback((i: number) => {
    localStorage.setItem(ACCENT_KEY, String(i))
    setAccentState(i)
  }, [])

  const value = useMemo(() => ({ mode, setMode, accent, setAccent }), [mode, setMode, accent, setAccent])
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>
}

export function useTheme() {
  return useContext(Ctx)
}
