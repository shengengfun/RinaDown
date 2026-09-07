// 多语言 —— LocaleProvider + useI18n()。
// - 语言集合由 web/src/lib/locales/*.json 自动发现（import.meta.glob，见 locales/index.ts）；
//   社区经 Weblate 新增 <lang>.json 后无需改代码即出现在语言下拉框。en.json 为源语言/模板，
//   缺失或空串的键按键级回退到英文。
// - 解析顺序：localStorage `fluxdown.locale`（用户显式选择）→ 服务器默认语言
//   （无鉴权 `/ping` 的 language，实时求值：设置页保存的 `web_language` 优先，
//   未保存时回退部署环境 FLUXDOWN_LANG；登录页同样生效）→ 浏览器语言 → en。
// - 持久化：仅设置页的显式切换写 localStorage 并写穿服务器 config `web_language`
//   （PUT /api/v1/config）；采用服务器/浏览器默认值不落盘，服务器侧变更随时可生效。
// - 后端返回：wire message 是稳定英文契约（CLI/客户端字符串匹配），不按语言变体；
//   展示层经 translateBackendMessage() 按当前语言映射。

import { createContext, useCallback, useContext, useEffect, useMemo, useState } from 'react'
import { api } from './api'
import { localeRegistry } from './locales'
import type enJson from './locales/en.json'

/** locale 代码（"en"、"zh"…），可用集合由 locales/*.json 自动发现 */
export type Locale = string

const LOCALE_KEY = 'fluxdown.locale'
/** 服务器 config 表中的语言键（设置页写穿）。 */
export const LANGUAGE_CONFIG_KEY = 'web_language'

export type I18nKey = keyof typeof enJson

// ---------------------------------------------------------------------------
// 后端 message 本地化（wire 契约保持英文，展示层映射）
// ---------------------------------------------------------------------------

/** 后端规范英文 message → 对应翻译键。en 语言下原样透传，查不到的消息原样返回。 */
const BACKEND_KEYS: Record<string, I18nKey> = {
  'not found': 'backend.notFound',
  'task not found': 'backend.taskNotFound',
  'unknown endpoint': 'backend.unknownEndpoint',
  'app shutting down': 'backend.appShuttingDown',
  'invalid or missing token': 'backend.invalidOrMissingToken',
  'missing X-FluxDown-Client header': 'backend.missingClientHeader',
  'management API requires a token; set one in Settings > API Service': 'backend.managementApiRequiresToken',
  // 首次运行 / 密钥策略（Rust: native/server/src/config.rs::validate_access_key）
  'setup already completed': 'backend.setupAlreadyCompleted',
  'the headless server requires an access key; it cannot be cleared': 'backend.accessKeyRequired',
  'access key must be at least 8 characters': 'setup.rule.tooShort',
  'access key must be at most 128 characters': 'setup.rule.tooLong',
  'access key must contain both letters and digits': 'setup.rule.needsMix',
  'access key must not contain spaces or non-ASCII characters': 'setup.rule.badChars',
  'queue name is required': 'backend.queueNameRequired',
  'url is required': 'backend.urlRequired',
  'task is not completed': 'backend.taskNotCompleted',
  'file not found on disk': 'backend.fileNotFoundOnDisk',
  'failed to persist task': 'backend.failedToPersistTask',
  'demo mode: only the designated demo file can be downloaded': 'backend.demoModeFileOnly',
  // 见 native/engine/src/proxy_config.rs 的 PROXY_TLS_ENDPOINT_HINT。
  'the proxy endpoint did not accept a TLS handshake; if this is a mixed HTTP/SOCKS port (Clash, V2Ray) or a plain HTTP proxy, select the HTTP type instead of HTTPS':
    'backend.proxyTlsEndpointHint',
  'managed install not supported on this platform': 'backend.componentUnsupported',
  'downloaded ffmpeg failed to run; this system may use musl libc (e.g. Alpine/OpenWrt) which cannot run the official glibc build — install ffmpeg via your system package manager and set a manual path':
    'backend.componentMuslVerify',
  'downloaded yt-dlp failed to run; the binary may be incompatible with this system — install yt-dlp via your system package manager (or pip) and set a manual path':
    'backend.componentYtdlpVerify',
}

/** 按语言本地化后端返回的 message；未识别的消息原样返回。 */
export function translateBackendMessage(message: string, locale?: Locale): string {
  const loc = locale ?? currentLocale
  if (loc === 'en') return message
  const key = BACKEND_KEYS[message.trim()]
  return key ? t(key) : message
}

// ---------------------------------------------------------------------------
// t() —— 模块级当前语言（非 React 代码可直接用），Provider 负责触发重渲染
// ---------------------------------------------------------------------------

function readStoredLocale(): Locale | null {
  const v = localStorage.getItem(LOCALE_KEY)
  return v && v in localeRegistry ? v : null
}

/** 浏览器首选语言 → 支持的语言（精确匹配优先，其次主语言前缀匹配，参照 website detectLocale）。 */
function detectBrowserLocale(): Locale {
  const langs = navigator.languages ?? [navigator.language]
  const available = Object.keys(localeRegistry)
  for (const lang of langs) {
    const lower = lang.toLowerCase()
    const exact = available.find((c) => c === lower)
    if (exact) return exact
    const prefix = available.find((c) => c === lower.split('-')[0])
    if (prefix) return prefix
  }
  return 'en'
}

let currentLocale: Locale = readStoredLocale() ?? detectBrowserLocale()

export function getLocale(): Locale {
  return currentLocale
}

/** 翻译 key，支持 `{name}` 占位插值。 */
export function t(key: I18nKey, params?: Record<string, string | number>): string {
  const messages = localeRegistry[currentLocale] ?? localeRegistry.en
  let s: string = messages[key] ?? key
  if (params) {
    for (const [k, v] of Object.entries(params)) s = s.replaceAll(`{${k}}`, String(v))
  }
  return s
}

// ---------------------------------------------------------------------------
// Provider / hook
// ---------------------------------------------------------------------------

interface I18nCtx {
  locale: Locale
  setLocale: (l: Locale) => void
  t: typeof t
}

const Ctx = createContext<I18nCtx>({ locale: currentLocale, setLocale: () => {}, t })

export function I18nProvider({ children }: { children: React.ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>(currentLocale)

  useEffect(() => {
    document.documentElement.lang = locale === 'zh' ? 'zh-CN' : locale
    document.title = t('app.title')
  }, [locale])

  // 切换语言但不落盘：采用服务器/浏览器默认值时用，保持「未显式选择」状态。
  const applyLocale = useCallback((l: Locale) => {
    currentLocale = l
    setLocaleState(l)
  }, [])

  // 用户显式选择（设置页）：落盘 localStorage，此后默认值不再覆盖本浏览器。
  const setLocale = useCallback(
    (l: Locale) => {
      localStorage.setItem(LOCALE_KEY, l)
      applyLocale(l)
    },
    [applyLocale],
  )

  // 挂载时从 /ping（无鉴权）采用服务器默认语言，登录页同样生效；
  // 本地已显式选择过语言（localStorage 有值）则以本地为准。
  useEffect(() => {
    if (readStoredLocale() !== null) return
    api
      .ping()
      .then(({ language }) => {
        if (language && language in localeRegistry && readStoredLocale() === null) {
          applyLocale(language)
        }
      })
      .catch(() => {})
  }, [applyLocale])

  const value = useMemo(() => ({ locale, setLocale, t }), [locale, setLocale])
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>
}

export function useI18n() {
  return useContext(Ctx)
}
