// 翻译源文件为 JSON（en.json 为源语言，社区经 Weblate 贡献其他语言）。
// 语言注册表由 import.meta.glob 自动发现：新增 <lang>.json 即自动出现在语言下拉框，
// 无需改代码。缺失/空串的键在构建注册表时回退到英文（键级 fallback）。
import enJson from './en.json'

export type Messages = { [K in keyof typeof enJson]: string }

/** 语言自称（下拉框显示用）；未收录的语言回退显示语言代码 */
const NATIVE_NAMES: Record<string, string> = {
  en: 'English',
  zh: '简体中文',
  'zh-hant': '繁體中文',
  ja: '日本語',
  ko: '한국어',
  ru: 'Русский',
  de: 'Deutsch',
  fr: 'Français',
  es: 'Español',
  'pt-br': 'Português (Brasil)',
  it: 'Italiano',
  tr: 'Türkçe',
  pl: 'Polski',
  uk: 'Українська',
  vi: 'Tiếng Việt',
  th: 'ไทย',
  id: 'Bahasa Indonesia',
  ar: 'العربية',
}

/** 文件名 → locale 代码（小写文件名即代码，历史 web 语言码 "zh" 天然对应 zh.json） */
function fileToCode(path: string): string {
  const base = path.replace(/^.*\/(.+)\.json$/, '$1')
  return base.toLowerCase()
}

const modules = import.meta.glob<{ default: Record<string, string> }>('./*.json', {
  eager: true,
})

/** 所有已发现语言的消息表（每种语言均已合并英文兜底，空串视为未翻译） */
export const localeRegistry: Record<string, Messages> = {}
for (const [path, mod] of Object.entries(modules)) {
  const code = fileToCode(path)
  const merged: Record<string, string> = { ...enJson }
  for (const [k, v] of Object.entries(mod.default)) {
    if (v !== '' && k in merged) merged[k] = v
  }
  localeRegistry[code] = merged as Messages
}

/** 语言下拉框条目：en、zh 置顶，其余按代码排序 */
export const LOCALES: { code: string; name: string }[] = Object.keys(localeRegistry)
  .sort((a, b) => {
    const rank = (c: string) => (c === 'en' ? 0 : c === 'zh' ? 1 : 2)
    return rank(a) - rank(b) || a.localeCompare(b)
  })
  .map((code) => ({ code, name: NATIVE_NAMES[code] ?? code }))

export const en: Messages = enJson
