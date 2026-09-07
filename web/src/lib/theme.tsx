// 主题（浅/深/跟随系统）+ 强调色，localStorage 持久化，纯前端。

import { createContext, useCallback, useContext, useEffect, useMemo, useState } from 'react'

export type ThemeMode = 'light' | 'dark' | 'system'

/** 预设强调色（对齐桌面端色彩方案的常用项）；名称走 i18n，不在此写死文案。 */
export const ACCENT_PRESETS = [
  { nameKey: 'set.appearance.accentBlue', light: '#2e6bf6', dark: '#4d82f8' },
  { nameKey: 'set.appearance.accentGreen', light: '#16a34a', dark: '#22c55e' },
  { nameKey: 'set.appearance.accentPurple', light: '#7c3aed', dark: '#8b5cf6' },
  { nameKey: 'set.appearance.accentOrange', light: '#ea580c', dark: '#f97316' },
  { nameKey: 'set.appearance.accentRose', light: '#e11d48', dark: '#f43f5e' },
] as const

const MODE_KEY = 'fluxdown.theme'
const ACCENT_KEY = 'fluxdown.accent'

interface ThemeCtx {
  mode: ThemeMode
  setMode: (m: ThemeMode) => void
  accent: number
  setAccent: (i: number) => void
}

const Ctx = createContext<ThemeCtx>({ mode: 'system', setMode: () => {}, accent: 0, setAccent: () => {} })

function systemDark(): boolean {
  return window.matchMedia('(prefers-color-scheme: dark)').matches
}

function apply(mode: ThemeMode, accent: number) {
  const dark = mode === 'dark' || (mode === 'system' && systemDark())
  document.documentElement.classList.toggle('dark', dark)
  const preset = ACCENT_PRESETS[accent] ?? ACCENT_PRESETS[0]
  document.documentElement.style.setProperty('--accent', dark ? preset.dark : preset.light)
}

export function ThemeProvider({ children }: { children: React.ReactNode }) {
  const [mode, setModeState] = useState<ThemeMode>(
    () => (localStorage.getItem(MODE_KEY) as ThemeMode) || 'system',
  )
  const [accent, setAccentState] = useState<number>(() =>
    parseInt(localStorage.getItem(ACCENT_KEY) ?? '0', 10),
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
