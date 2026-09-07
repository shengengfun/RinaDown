// 自定义分类管理（config 键 `custom_categories`）：新增 / 重命名 / 改匹配规则 /
// 显隐 / 重置 / 删除 / 上下移动排序。
//
// 归属「通用」分区 —— 设置分类以桌面端 settings_page 为基准（通用 → 自定义分类）。
// 写出的 JSON 与桌面 `CustomCategory.toJson()` 逐字段一致（见 lib/categories.ts 的
// serializeCategories），两端共用引擎 config 表的同一行。
import { useState } from 'react'
import { ChevronDown, ChevronUp, Eye, EyeOff, FolderTree, FolderX, Pencil, Plus, RotateCcw, Trash2 } from 'lucide-react'
import {
  CATEGORY_ICON_KEYS,
  applyCategoryDirs,
  categoryDirUnder,
  categoryDirsApplied,
  categoryIcon,
  categoryIconByKey,
  categoryLabel,
  clearCategoryDirs,
  defaultCategories,
  defaultCategoryOf,
  formatExtensionsInput,
  isSpecialBuiltin,
  isValidRegex,
  newCategoryId,
  parseCategories,
  parseExtensionsInput,
  repositionCategories,
  serializeCategories,
  type Category,
} from '../../lib/categories'
import { cn } from '../../lib/cn'
import { CATEGORIES_KEY } from '../../lib/config'
import { confirmDialog } from '../../lib/confirm'
import { useI18n } from '../../lib/i18n'
import type { ConfigMap } from '../../lib/types'
import { FsPicker } from '../dialogs/fs-picker'
import { SetRow } from './controls'

/** 分类编辑器草稿。`target = null` 表示新增，否则编辑该分类。 */
interface CategoryEditor {
  target: Category | null
  name: string
  icon: string
  matchMode: 'extension' | 'regex'
  extensions: string
  regexPattern: string
  saveDir: string
  error: string
}

export function CategoriesSettings({
  config,
  mutate,
}: {
  config: ConfigMap
  mutate: (entries: ConfigMap) => void
}) {
  const { t } = useI18n()
  const cats = parseCategories(config[CATEGORIES_KEY])
  const [editor, setEditor] = useState<CategoryEditor | null>(null)

  function commit(next: Category[]) {
    mutate({ [CATEGORIES_KEY]: serializeCategories(next) })
  }

  /** 上/下移一格后按数组下标重写 position（对齐桌面 reorderCustomCategories）。 */
  function move(index: number, delta: number) {
    const to = index + delta
    if (to < 0 || to >= cats.length) return
    const next = [...cats]
    const [item] = next.splice(index, 1)
    next.splice(to, 0, item)
    commit(repositionCategories(next))
  }

  function toggleVisible(cat: Category) {
    commit(cats.map((c) => (c.id === cat.id ? { ...c, visible: !c.visible } : c)))
  }

  /** 内置分类恢复出厂配置，仅保留当前排序位置。 */
  function resetOne(cat: Category) {
    if (!cat.builtinType) return
    const fresh = defaultCategoryOf(cat.builtinType)
    if (!fresh) return
    commit(cats.map((c) => (c.id === cat.id ? { ...fresh, position: c.position } : c)))
  }

  async function removeOne(cat: Category) {
    const ok = await confirmDialog({
      title: t('set.general.categoryDelete'),
      message: t('set.general.categoryDeleteConfirm'),
      confirmLabel: t('set.general.categoryDelete'),
      danger: true,
    })
    if (!ok) return
    if (editor?.target?.id === cat.id) setEditor(null)
    commit(cats.filter((c) => c.id !== cat.id))
  }

  async function resetAll() {
    const ok = await confirmDialog({
      title: t('set.general.categoryResetAll'),
      message: t('set.general.categoryResetAllConfirm'),
      danger: true,
    })
    if (!ok) return
    setEditor(null)
    commit(defaultCategories())
  }

  // 一键分类目录：两态。尚未全部指向「默认下载目录 / 分类名」时是「应用」，
  // 已全部指向时变成「清除」——与桌面设置页同一枚按钮、同一套语义。
  const baseDir = (config.default_save_dir ?? '').trim()
  const dirsApplied = categoryDirsApplied(cats, baseDir)

  async function toggleCategoryDirs() {
    if (baseDir === '') return
    if (dirsApplied) {
      const ok = await confirmDialog({
        title: t('set.general.categoryDirsClear'),
        message: t('set.general.categoryDirsClearConfirm'),
        danger: true,
      })
      if (!ok) return
      setEditor(null)
      commit(clearCategoryDirs(cats))
      return
    }
    // 举例用第一个可推导出目录的分类，让用户先看清实际会建在哪。
    let sample = ''
    for (const cat of cats) {
      if (cat.builtinType === 'all') continue
      const dir = categoryDirUnder(baseDir, categoryLabel(cat))
      if (dir !== '') {
        sample = dir
        break
      }
    }
    const ok = await confirmDialog({
      title: t('set.general.categoryDirsApply'),
      message: t('set.general.categoryDirsApplyConfirm', { example: sample }),
    })
    if (!ok) return
    setEditor(null)
    commit(applyCategoryDirs(cats, baseDir))
  }

  function openEditor(target: Category | null) {
    setEditor({
      target,
      name: target?.name ?? '',
      icon: target?.icon ?? 'file',
      matchMode: target?.matchMode ?? 'extension',
      extensions: formatExtensionsInput(target?.extensions ?? []),
      regexPattern: target?.regexPattern ?? '',
      saveDir: target?.saveDir ?? '',
      error: '',
    })
  }

  /** 校验规则逐条对齐桌面 CategoryEditDialog._save。 */
  function saveEditor() {
    if (!editor) return
    const target = editor.target
    const builtin = target?.isBuiltin ?? false
    const name = editor.name.trim()
    if (name === '' && !builtin) {
      setEditor({ ...editor, error: t('set.general.categoryNameRequired') })
      return
    }
    // 'all' / 'other' 没有显式规则，沿用原值不做校验。
    let extensions = target?.extensions ?? []
    let regexPattern = target?.regexPattern ?? ''
    if (!isSpecialBuiltin(target)) {
      if (editor.matchMode === 'extension') {
        extensions = parseExtensionsInput(editor.extensions)
        if (extensions.length === 0 && !builtin) {
          setEditor({ ...editor, error: t('set.general.categoryExtensionsRequired') })
          return
        }
        regexPattern = ''
      } else {
        regexPattern = editor.regexPattern.trim()
        if (regexPattern !== '' && !isValidRegex(regexPattern)) {
          setEditor({ ...editor, error: t('set.general.categoryRegexInvalid') })
          return
        }
        extensions = []
      }
    }
    const saved: Category = {
      id: target?.id ?? newCategoryId(),
      name,
      icon: editor.icon,
      matchMode: editor.matchMode,
      extensions,
      regexPattern,
      // 新分类排在末尾（桌面同值），下次拖动排序时统一重写。
      position: target?.position ?? 999,
      visible: target?.visible ?? true,
      isBuiltin: builtin,
      builtinType: target?.builtinType ?? null,
      saveDir: editor.saveDir.trim(),
    }
    commit(target ? cats.map((c) => (c.id === saved.id ? saved : c)) : [...cats, saved])
    setEditor(null)
  }

  /** 列表副标题：匹配规则 + 已设置的分类保存目录（一键分类目录后能直接看到落点）。 */
  function subtitle(cat: Category): string {
    const rule = matchSummary(cat)
    if (cat.saveDir === '') return rule
    return rule === '' ? cat.saveDir : `${rule}  ·  ${cat.saveDir}`
  }

  function matchSummary(cat: Category): string {
    if (cat.builtinType === 'all') return t('set.general.categoryAllDesc')
    if (cat.builtinType === 'other') return t('set.general.categoryOtherDesc')
    if (cat.matchMode === 'extension' && cat.extensions.length > 0) {
      return cat.extensions.map((e) => `.${e}`).join(', ')
    }
    if (cat.matchMode === 'regex' && cat.regexPattern !== '') return cat.regexPattern
    return ''
  }

  const special = isSpecialBuiltin(editor?.target ?? null)
  const editorForm = editor ? (
    <div className="set-row stack">
      <label className="text-[11.5px] font-medium text-text2" htmlFor="cat-name">
        {t('set.general.categoryName')}
      </label>
      <input
        id="cat-name"
        className="text-input"
        style={{ width: '100%' }}
        spellCheck={false}
        placeholder={t('set.general.categoryNameHint')}
        value={editor.name}
        onChange={(e) => setEditor({ ...editor, name: e.target.value, error: '' })}
      />
      <span className="text-[11.5px] font-medium text-text2">{t('set.general.categoryIcon')}</span>
      <div className="flex flex-wrap gap-1">
        {CATEGORY_ICON_KEYS.map((key) => {
          const Icon = categoryIconByKey(key)
          return (
            <button
              key={key}
              type="button"
              className={cn('icon-btn sm', editor.icon === key && 'active')}
              aria-label={key}
              aria-pressed={editor.icon === key}
              onClick={() => setEditor({ ...editor, icon: key })}
            >
              <Icon size={15} />
            </button>
          )
        })}
      </div>
      {!special && (
        <>
          <span className="text-[11.5px] font-medium text-text2">
            {t('set.general.categoryMatchMode')}
          </span>
          <div className="flex gap-2">
            <button
              type="button"
              className={cn('btn sm', editor.matchMode === 'extension' ? 'primary' : 'ghost')}
              onClick={() => setEditor({ ...editor, matchMode: 'extension', error: '' })}
            >
              {t('set.general.categoryMatchExtension')}
            </button>
            <button
              type="button"
              className={cn('btn sm', editor.matchMode === 'regex' ? 'primary' : 'ghost')}
              onClick={() => setEditor({ ...editor, matchMode: 'regex', error: '' })}
            >
              {t('set.general.categoryMatchRegex')}
            </button>
          </div>
          {editor.matchMode === 'extension' ? (
            <>
              <label className="text-[11.5px] font-medium text-text2" htmlFor="cat-ext">
                {t('set.general.categoryExtensions')}
              </label>
              <input
                id="cat-ext"
                className="text-input"
                style={{ width: '100%' }}
                spellCheck={false}
                placeholder={t('set.general.categoryExtensionsHint')}
                value={editor.extensions}
                onChange={(e) => setEditor({ ...editor, extensions: e.target.value, error: '' })}
              />
            </>
          ) : (
            <>
              <label className="text-[11.5px] font-medium text-text2" htmlFor="cat-regex">
                {t('set.general.categoryRegex')}
              </label>
              <input
                id="cat-regex"
                className="text-input font-mono"
                style={{ width: '100%' }}
                spellCheck={false}
                placeholder={t('set.general.categoryRegexHint')}
                value={editor.regexPattern}
                onChange={(e) => setEditor({ ...editor, regexPattern: e.target.value, error: '' })}
              />
            </>
          )}
        </>
      )}
      {/* 「全部文件」等同于全局默认目录，不提供分类保存目录 */}
      {editor.target?.builtinType !== 'all' && (
        <>
          <label className="text-[11.5px] font-medium text-text2" htmlFor="cat-savedir">
            {t('set.general.categorySaveDir')}
          </label>
          <span className="text-[11px] text-text3">{t('set.general.categorySaveDirDesc')}</span>
          <div className="dir-row">
            <input
              id="cat-savedir"
              className="text-input"
              style={{ width: '100%' }}
              spellCheck={false}
              placeholder={t('set.general.categorySaveDirHint')}
              value={editor.saveDir}
              onChange={(e) => setEditor({ ...editor, saveDir: e.target.value })}
            />
            <FsPicker value={editor.saveDir} onChange={(p) => setEditor({ ...editor, saveDir: p })} />
          </div>
        </>
      )}
      {editor.error ? <span className="text-[11.5px] text-danger">{editor.error}</span> : null}
      <div className="flex items-center justify-end gap-2">
        <button type="button" className="btn ghost sm" onClick={() => setEditor(null)}>
          {t('common.cancel')}
        </button>
        <button type="button" className="btn primary sm" onClick={saveEditor}>
          {t('common.confirm')}
        </button>
      </div>
    </div>
  ) : null

  return (
    <>
      <h2 className="set-title mt-6">{t('set.general.categories')}</h2>
      <p className="set-desc">{t('set.general.categoriesDesc')}</p>
      <div className="mb-2 flex items-center justify-end gap-2">
        <button
          type="button"
          className="btn ghost sm"
          disabled={baseDir === ''}
          onClick={() => void toggleCategoryDirs()}
        >
          {dirsApplied ? <FolderX size={13} /> : <FolderTree size={13} />}
          {dirsApplied ? t('set.general.categoryDirsClear') : t('set.general.categoryDirsApply')}
        </button>
        <button type="button" className="btn ghost sm" onClick={() => void resetAll()}>
          <RotateCcw size={13} />
          {t('set.general.categoryResetAll')}
        </button>
        <button type="button" className="btn primary sm" onClick={() => openEditor(null)}>
          <Plus size={13} />
          {t('set.general.categoryAdd')}
        </button>
      </div>
      {/* 分类行较宽（图标 + 名称 + 后缀列表 + 一排操作按钮），宽屏下整行铺满而不参与分列。 */}
      <div className="set-group set-wide">
        {cats.map((cat, i) => {
          const Icon = categoryIcon(cat)
          // 「全部文件」不可编辑/删除；其余内置项可编辑、可重置、可删除。
          const locked = cat.builtinType === 'all'
          return (
            <div key={cat.id}>
              <SetRow
                title={
                  <span className="flex items-center gap-1.5">
                    <Icon size={14} className="text-accent" />
                    <span className={cn(!cat.visible && 'text-text3')}>{categoryLabel(cat)}</span>
                    {cat.isBuiltin ? (
                      <span className="rounded bg-accent-weak px-1 py-px text-[9px] text-accent">
                        {t('set.general.categoryBuiltin')}
                      </span>
                    ) : null}
                    {!cat.visible ? <EyeOff size={11} className="text-text3" /> : null}
                  </span>
                }
                desc={subtitle(cat)}
              >
                <div className="flex items-center gap-1">
                  <button
                    type="button"
                    className="icon-btn sm"
                    disabled={i === 0}
                    title={t('set.general.categoryMoveUp')}
                    aria-label={t('set.general.categoryMoveUp')}
                    onClick={() => move(i, -1)}
                  >
                    <ChevronUp size={14} />
                  </button>
                  <button
                    type="button"
                    className="icon-btn sm"
                    disabled={i === cats.length - 1}
                    title={t('set.general.categoryMoveDown')}
                    aria-label={t('set.general.categoryMoveDown')}
                    onClick={() => move(i, 1)}
                  >
                    <ChevronDown size={14} />
                  </button>
                  <button
                    type="button"
                    className="icon-btn sm"
                    title={t('set.general.categoryToggleVisible')}
                    aria-label={t('set.general.categoryToggleVisible')}
                    onClick={() => toggleVisible(cat)}
                  >
                    {cat.visible ? <Eye size={14} /> : <EyeOff size={14} />}
                  </button>
                  {!locked ? (
                    <button
                      type="button"
                      className="icon-btn sm"
                      title={t('set.general.categoryEdit')}
                      aria-label={t('set.general.categoryEdit')}
                      onClick={() => openEditor(cat)}
                    >
                      <Pencil size={14} />
                    </button>
                  ) : null}
                  {cat.isBuiltin && !locked ? (
                    <button
                      type="button"
                      className="icon-btn sm"
                      title={t('set.general.categoryReset')}
                      aria-label={t('set.general.categoryReset')}
                      onClick={() => resetOne(cat)}
                    >
                      <RotateCcw size={14} />
                    </button>
                  ) : null}
                  {!locked ? (
                    <button
                      type="button"
                      className="icon-btn sm danger"
                      title={t('set.general.categoryDelete')}
                      aria-label={t('set.general.categoryDelete')}
                      onClick={() => void removeOne(cat)}
                    >
                      <Trash2 size={14} />
                    </button>
                  ) : null}
                </div>
              </SetRow>
              {editor?.target?.id === cat.id ? editorForm : null}
            </div>
          )
        })}
        {editor && editor.target === null ? editorForm : null}
      </div>
    </>
  )
}
