// 文件分类：侧边栏「分类」区块的数据源与匹配规则。
//
// 分类列表存在引擎 config 表的 `custom_categories` 键里（JSON 数组），桌面端可以
// 增删改、排序、隐藏其中任意一项。本模块只解析与消费这份数据——用户在桌面端建的
// 「设计稿」「字幕」之类分类，刷新后就出现在 Web 侧边栏并能正常筛选。
//
// 键不存在时（全新安装、或用户从未动过分类）回落到与引擎同一套内置分类，行为与
// 没有这个功能时完全一致。

import {
  Archive,
  Bookmark,
  Box,
  Code,
  Cpu,
  Database,
  Disc,
  File,
  FileText,
  Film,
  Folders,
  Gamepad2,
  Globe,
  HardDrive,
  Image,
  Library,
  Music,
  Package2,
  Pen,
  Printer,
  Smartphone,
  Subtitles,
  Type,
  Zap,
} from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import { t, type I18nKey } from './i18n'

/** 内置分类的类型标签；`null` = 用户自定义分类。 */
export type BuiltinType = 'all' | 'video' | 'audio' | 'document' | 'image' | 'program' | 'archive' | 'other'

export interface Category {
  id: string
  /** 内置分类此字段是占位英文名，展示一律走 [categoryLabel]。 */
  name: string
  icon: string
  matchMode: 'extension' | 'regex'
  /** 不含点号、已小写。 */
  extensions: string[]
  regexPattern: string
  position: number
  visible: boolean
  isBuiltin: boolean
  builtinType: BuiltinType | null
  /** 分类专属保存目录，空串 = 用全局默认目录。 */
  saveDir: string
}

const ICONS: Record<string, LucideIcon> = {
  folders: Folders,
  film: Film,
  music: Music,
  fileText: FileText,
  image: Image,
  archive: Archive,
  file: File,
  code: Code,
  database: Database,
  gamepad: Gamepad2,
  globe: Globe,
  bookmark: Bookmark,
  box: Box,
  cpu: Cpu,
  disc: Disc,
  font: Type,
  hardDrive: HardDrive,
  library: Library,
  package2: Package2,
  pen: Pen,
  printer: Printer,
  smartphone: Smartphone,
  subtitles: Subtitles,
  type: Type,
  zap: Zap,
}

// 内置分类显示名。这些文案不只是标签：`categoryDirUnder` 拿它当目录名，桌面
// （assets/i18n 的 categoryVideo/categoryAudio/…）与这里必须**逐字一致**，
// 否则两端「一键分类目录」会在同一台机器上建出两套目录（Document vs Documents）。
const BUILTIN_LABEL: Record<BuiltinType, I18nKey> = {
  all: 'type.all',
  video: 'type.video',
  audio: 'type.audio',
  document: 'type.document',
  image: 'type.image',
  program: 'type.program',
  archive: 'type.archive',
  other: 'type.other',
}

/** 内置分类的扩展名表与引擎保持一致；用户改过之后以 config 里的为准。 */
const BUILTIN: Category[] = [
  builtin('_all', 'all', 'folders', 0, []),
  builtin('_video', 'video', 'film', 1, [
    'mp4', 'mkv', 'avi', 'mov', 'wmv', 'flv', 'webm', 'ts',
    'm4v', 'rmvb', 'rm', '3gp', 'vob', 'mpg', 'mpeg',
  ]),
  builtin('_audio', 'audio', 'music', 2, [
    'mp3', 'flac', 'wav', 'aac', 'ogg', 'wma', 'm4a', 'opus', 'ape', 'aiff',
  ]),
  builtin('_document', 'document', 'fileText', 3, [
    'pdf', 'doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx', 'txt',
    'csv', 'rtf', 'epub', 'mobi', 'md', 'odt', 'ods', 'odp',
  ]),
  builtin('_image', 'image', 'image', 4, [
    'jpg', 'jpeg', 'png', 'gif', 'bmp', 'webp', 'svg', 'ico',
    'tiff', 'tif', 'psd', 'raw', 'heic', 'avif',
  ]),
  builtin('_program', 'program', 'package2', 5, [
    'exe', 'msi', 'msix', 'appx', 'apk', 'dmg', 'pkg', 'deb',
    'rpm', 'appimage', 'snap', 'flatpak',
  ]),
  builtin('_archive', 'archive', 'archive', 6, [
    'zip', 'rar', '7z', 'tar', 'gz', 'bz2', 'xz', 'zst',
    'iso', 'cab', 'lz', 'lzma',
  ]),
  builtin('_other', 'other', 'file', 100, []),
]

function builtin(id: string, type: BuiltinType, icon: string, position: number, extensions: string[]): Category {
  return {
    id,
    name: type,
    icon,
    matchMode: 'extension',
    extensions,
    regexPattern: '',
    position,
    visible: true,
    isBuiltin: true,
    builtinType: type,
    saveDir: '',
  }
}

/** 解析 config 里的分类列表。空串/非法 JSON/空数组一律回落内置表——分类是导航骨架，
 *  宁可显示默认的八项，也不能让侧边栏整块空掉。 */
export function parseCategories(raw: string | undefined): Category[] {
  if (!raw) return BUILTIN
  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch {
    return BUILTIN
  }
  if (!Array.isArray(parsed) || parsed.length === 0) return BUILTIN
  const out: Category[] = []
  for (const item of parsed) {
    if (typeof item !== 'object' || item === null) continue
    const o = item as Record<string, unknown>
    const id = typeof o.id === 'string' ? o.id : ''
    if (!id) continue
    out.push({
      id,
      name: typeof o.name === 'string' ? o.name : '',
      icon: typeof o.icon === 'string' ? o.icon : 'file',
      matchMode: o.matchMode === 'regex' ? 'regex' : 'extension',
      extensions: Array.isArray(o.extensions)
        ? o.extensions.map((e) => String(e).toLowerCase())
        : [],
      regexPattern: typeof o.regexPattern === 'string' ? o.regexPattern : '',
      position: typeof o.position === 'number' ? o.position : 0,
      visible: o.visible !== false,
      isBuiltin: o.isBuiltin === true,
      builtinType: typeof o.builtinType === 'string' ? (o.builtinType as BuiltinType) : null,
      saveDir: typeof o.saveDir === 'string' ? o.saveDir : '',
    })
  }
  return out.length > 0 ? out : BUILTIN
}

/** 可见分类，按 position 排序。 */
export function visibleCategories(all: Category[]): Category[] {
  return all.filter((c) => c.visible).sort((a, b) => a.position - b.position)
}

export function categoryLabel(c: Category): string {
  return c.isBuiltin && c.builtinType ? t(BUILTIN_LABEL[c.builtinType]) : c.name
}

export function categoryIcon(c: Category): LucideIcon {
  return ICONS[c.icon] ?? File
}

/** 按图标标识取组件（设置页图标选择器用）；未知标识退到通用文件图标。 */
export function categoryIconByKey(key: string): LucideIcon {
  return ICONS[key] ?? File
}

/** 从任务的文件名/链接取出用于匹配的名字。文件名尚未确定（解析中）时退到 URL 末段。 */
function effectiveName(fileName: string, url: string): string {
  return fileName || url.split('?')[0].split('/').pop() || ''
}

function matchesCategory(c: Category, name: string): boolean {
  if (c.builtinType === 'all' || c.builtinType === 'other') return false
  if (c.matchMode === 'regex') {
    if (!c.regexPattern) return false
    try {
      return new RegExp(c.regexPattern, 'iu').test(name)
    } catch {
      // 正则写错时一律不命中，而不是抛异常打断整张列表的渲染。
      return false
    }
  }
  const dot = name.lastIndexOf('.')
  if (dot < 0) return false
  return c.extensions.includes(name.slice(dot + 1).toLowerCase())
}

/**
 * 任务归属的分类 id。按 position 顺序先命中先归属；一个都不命中就落到 `other`。
 *
 * 磁力链与 m3u8 直播流在文件名落定前没有扩展名可判，若只看扩展名它们会全部堆进
 * 「其他」——按链接形态直接判成视频。
 */
export function categoryIdOf(task: { fileName: string; url: string }, cats: Category[]): string {
  const normals = cats.filter((c) => c.builtinType !== 'all' && c.builtinType !== 'other')
  const name = effectiveName(task.fileName, task.url)
  const hasExt = name.lastIndexOf('.') > 0
  if (!hasExt && /^magnet:|\.m3u8(\?|$)/.test(task.url)) {
    const video = normals.find((c) => c.builtinType === 'video')
    if (video) return video.id
  }
  for (const c of normals) {
    if (matchesCategory(c, name)) return c.id
  }
  return cats.find((c) => c.builtinType === 'other')?.id ?? ''
}

/** 分类筛选是否放行该任务。`ALL_CATEGORY` 表示不筛选。 */
export const ALL_CATEGORY = 'all'

export function passesCategory(
  task: { fileName: string; url: string },
  filter: string,
  cats: Category[],
): boolean {
  if (filter === ALL_CATEGORY) return true
  const target = cats.find((c) => c.id === filter)
  if (!target || target.builtinType === 'all') return true
  return categoryIdOf(task, cats) === target.id
}

// ---------------------------------------------------------------------------
// 分类编辑（设置页「自定义分类」）
// ---------------------------------------------------------------------------
//
// 序列化格式与桌面端 `CustomCategory.toJson()` 逐字段一致（同一行 DB，两端互读）：
// id / name / icon / matchMode / extensions / regexPattern / position / visible /
// isBuiltin / builtinType / saveDir。字段名或取值一旦偏离，桌面端读到的分类就会
// 退化成默认值。

/** 图标选择器的候选集，顺序与桌面 `CategoryIcon` 枚举一致。 */
export const CATEGORY_ICON_KEYS: string[] = Object.keys(ICONS)

/** 内置分类的出厂快照（深拷贝，调用方可安全改写）。 */
export function defaultCategories(): Category[] {
  return BUILTIN.map((c) => ({ ...c, extensions: [...c.extensions] }))
}

/** 某个内置类型的出厂配置；未知类型返回 undefined。 */
export function defaultCategoryOf(type: BuiltinType): Category | undefined {
  const found = BUILTIN.find((c) => c.builtinType === type)
  return found ? { ...found, extensions: [...found.extensions] } : undefined
}

/** 写回 config 的 JSON。字段顺序对齐桌面 `toJson()`，便于两端 diff。 */
export function serializeCategories(cats: Category[]): string {
  return JSON.stringify(
    cats.map((c) => ({
      id: c.id,
      name: c.name,
      icon: c.icon,
      matchMode: c.matchMode,
      extensions: c.extensions,
      regexPattern: c.regexPattern,
      position: c.position,
      visible: c.visible,
      isBuiltin: c.isBuiltin,
      builtinType: c.builtinType,
      saveDir: c.saveDir,
    })),
  )
}

/** 新建分类的 id：与桌面同构（微秒时间戳的 36 进制），跨端不会撞键。 */
export function newCategoryId(): string {
  return (Date.now() * 1000).toString(36)
}

/** 扩展名输入归一化：逗号（含全角）/空白分隔，去点号、转小写、丢空项。 */
export function parseExtensionsInput(raw: string): string[] {
  return raw
    .split(/[,，\s]+/)
    .map((e) => e.trim().replaceAll('.', '').toLowerCase())
    .filter((e) => e !== '')
}

/** 扩展名回填输入框的展示形式（与桌面 `extensions.join(', ')` 一致）。 */
export function formatExtensionsInput(extensions: string[]): string {
  return extensions.join(', ')
}

export function isValidRegex(pattern: string): boolean {
  try {
    new RegExp(pattern)
    return true
  } catch {
    return false
  }
}

/** 'all' 与 'other' 没有显式匹配规则（全匹配 / 兜底），编辑时隐藏规则区。 */
export function isSpecialBuiltin(c: Category | null): boolean {
  return c?.isBuiltin === true && (c.builtinType === 'all' || c.builtinType === 'other')
}

/** 重排后按数组下标重写 position，保证排序稳定（对齐桌面 reorderCustomCategories）。 */
export function repositionCategories(cats: Category[]): Category[] {
  return cats.map((c, i) => ({ ...c, position: i }))
}

// ---------------------------------------------------------------------------
// 分类保存目录：一键分类目录 + 新建下载时的目录解析
// ---------------------------------------------------------------------------
//
// 桌面镜像：`lib/src/models/custom_category.dart` 的 sanitizeCategoryDirName /
// categoryDirUnder，以及 `settings_provider.dart` 的 resolveCategorySaveDir。
// 同一台机器上桌面与 Web 一键出来的目录必须逐字一致，改一处就要改另一处。

// 控制字符是有意匹配的：文件系统不接受它们，与桌面同规剔除。
// eslint-disable-next-line no-control-regex
const INVALID_DIR_CHARS = /[\\/:*?"<>|\u0000-\u001f]/g

/** 分类显示名 → 目录名：非法字符换空格、压缩空白、去掉 Windows 会丢弃的结尾点/空格。 */
export function sanitizeCategoryDirName(label: string): string {
  let out = label.replace(INVALID_DIR_CHARS, ' ').replace(/\s+/g, ' ').trim()
  while (out !== '' && (out.endsWith('.') || out.endsWith(' '))) out = out.slice(0, -1)
  return out
}

/** 目标机器的路径分隔符。宿主可能是 Linux 服务器而浏览器在 Windows，只能从目录本身反推。 */
function separatorOf(base: string): string {
  return /^[a-zA-Z]:[\\/]/.test(base) || (base.includes('\\') && !base.includes('/')) ? '\\' : '/'
}

/** 「默认下载目录 / 分类名」。目录为空或分类名净化后为空时返回 ''（调用方跳过）。 */
export function categoryDirUnder(baseDir: string, label: string): string {
  let root = baseDir.trim()
  if (root === '') return ''
  const folder = sanitizeCategoryDirName(label)
  if (folder === '') return ''
  const sep = separatorOf(root)
  while (root.length > 1 && (root.endsWith('/') || root.endsWith('\\'))) root = root.slice(0, -1)
  // 根目录（"/" 或 "\"）本身就带分隔符，直接拼名字。
  if (root.endsWith('/') || root.endsWith('\\')) return `${root}${folder}`
  return `${root}${sep}${folder}`
}

/** 一键：每个分类（「全部文件」除外，它等同于全局默认目录）指向同名子目录。 */
export function applyCategoryDirs(cats: Category[], baseDir: string): Category[] {
  return cats.map((c) => {
    if (c.builtinType === 'all') return c
    const dir = categoryDirUnder(baseDir, categoryLabel(c))
    return dir === '' || dir === c.saveDir ? c : { ...c, saveDir: dir }
  })
}

/** 清除所有分类的保存目录，回到「一切都落默认下载目录」。 */
export function clearCategoryDirs(cats: Category[]): Category[] {
  return cats.map((c) => (c.saveDir === '' ? c : { ...c, saveDir: '' }))
}

/** 每个可设目录的分类是否都已指向「默认下载目录 / 分类名」（一键按钮的两态判定）。 */
export function categoryDirsApplied(cats: Category[], baseDir: string): boolean {
  let any = false
  for (const c of cats) {
    if (c.builtinType === 'all') continue
    const dir = categoryDirUnder(baseDir, categoryLabel(c))
    if (dir === '') continue
    any = true
    if (c.saveDir !== dir) return false
  }
  return any
}

/** URL 末段派生的文件名（必须含 '.'），取不到返回 ''。 */
function fileNameFromUrl(url: string): string {
  try {
    const path = new URL(url, 'http://x/').pathname
    const last = decodeURIComponent(path.split('/').pop() ?? '')
    return last.includes('.') ? last : ''
  } catch {
    return ''
  }
}

/**
 * 按分类规则解析保存目录：普通分类按 position 先命中先用，都不命中再看「其他」。
 * 没有任何分类设了目录（默认状态）时返回 ''，由调用方回退全局默认目录。
 *
 * [cats] 需传已过滤可见 + 按 position 排序的列表（`visibleCategories`）。
 * 文件名为空或不含扩展名时用 URL 末段兜底 —— 与桌面
 * `SettingsProvider.resolveCategorySaveDir` 逐条对齐。
 */
export function resolveCategorySaveDir(fileName: string, url: string, cats: Category[]): string {
  let name = fileName
  if ((name === '' || !name.includes('.')) && url !== '') {
    const derived = fileNameFromUrl(url)
    if (derived !== '') name = derived
  }
  if (name === '') return ''
  const normals = cats.filter((c) => c.builtinType !== 'all' && c.builtinType !== 'other')
  for (const c of normals) {
    if (c.saveDir !== '' && matchesCategory(c, name)) return c.saveDir
  }
  const other = cats.find((c) => c.builtinType === 'other')
  if (other && other.saveDir !== '' && !normals.some((c) => matchesCategory(c, name))) {
    return other.saveDir
  }
  return ''
}
