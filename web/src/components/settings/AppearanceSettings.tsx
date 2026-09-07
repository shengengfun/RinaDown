// 外观：主题模式 + 强调色 + 语言 + 界面区块显隐。
// 主题/强调色纯前端（useTheme）；语言与区块显隐写穿服务器 config 表，后者与桌面
// 客户端共用同一批键，改这里桌面端下次读配置也跟着变。
import { ACCENT_PRESETS, useTheme } from '../../lib/theme'
import type { ThemeMode } from '../../lib/theme'
import { cn } from '../../lib/cn'
import { LANGUAGE_CONFIG_KEY, useI18n } from '../../lib/i18n'
import type { Locale } from '../../lib/i18n'
import { LOCALES } from '../../lib/locales'
import { SetRow, SetSelect, SetSwitch } from './controls'
import { boolEntry, readBool, readTriBool, SECTION_KEY, useConfigMutation, useConfigQuery, type UiSection } from '../../lib/config'

const LANGUAGE_OPTIONS: { value: Locale; label: string }[] = LOCALES.map(({ code, name }) => ({
  value: code,
  label: name,
}))

export function AppearanceSettings() {
  const { mode, setMode, accent, setAccent } = useTheme()
  const { t, locale, setLocale } = useI18n()
  const mutation = useConfigMutation()
  const { data: config } = useConfigQuery()

  // 区块开关行。设备区是三态（未设置 = 有设备才显示），开关按「当前是否强制显示」
  // 回显：一旦用户拨动，语义就固化成显式的强制显示/隐藏。
  const SECTIONS: { key: UiSection; title: string; desc: string }[] = [
    { key: 'status', title: t('set.appearance.secStatus'), desc: t('set.appearance.secStatusDesc') },
    { key: 'category', title: t('set.appearance.secCategory'), desc: t('set.appearance.secCategoryDesc') },
    { key: 'queues', title: t('set.appearance.secQueues'), desc: t('set.appearance.secQueuesDesc') },
    { key: 'rss', title: t('set.appearance.secRss'), desc: t('set.appearance.secRssDesc') },
    { key: 'device', title: t('set.appearance.secDevice'), desc: t('set.appearance.secDeviceDesc') },
  ]
  const sectionOn = (k: UiSection) =>
    k === 'device' ? (readTriBool(config, SECTION_KEY.device) ?? false) : readBool(config, SECTION_KEY[k])

  const MODE_OPTIONS: { value: ThemeMode; label: string }[] = [
    { value: 'light', label: t('set.appearance.light') },
    { value: 'dark', label: t('set.appearance.dark') },
    { value: 'system', label: t('set.appearance.system') },
  ]

  function onLanguageChange(v: string) {
    setLocale(v as Locale)
    mutation.mutate({ [LANGUAGE_CONFIG_KEY]: v })
  }

  return (
    <>
      <h2 className="set-title">{t('set.appearance')}</h2>
      <p className="set-desc">{t('set.appearance.desc')}</p>
      <div className="set-group">
        <SetRow title={t('set.appearance.themeMode')}>
          <SetSelect value={mode} onValueChange={(v) => setMode(v as ThemeMode)} options={MODE_OPTIONS} />
        </SetRow>
        <SetRow
          title={t('set.appearance.accent')}
          desc={ACCENT_PRESETS.map((p) => t(p.nameKey)).join(' / ')}
        >
          <div className="color-dots">
            {ACCENT_PRESETS.map((p, i) => (
              <button
                key={p.nameKey}
                type="button"
                aria-label={t(p.nameKey)}
                className={cn('color-dot', i === accent && 'active')}
                style={{ background: p.light }}
                onClick={() => setAccent(i)}
              />
            ))}
          </div>
        </SetRow>
        <SetRow title={t('set.appearance.language')} desc={t('set.appearance.languageDesc')}>
          <SetSelect value={locale} onValueChange={onLanguageChange} options={LANGUAGE_OPTIONS} />
        </SetRow>
      </div>

      <p className="mb-1 mt-6 text-[12.5px] font-semibold text-text2">{t('set.appearance.sections')}</p>
      <p className="set-desc" style={{ marginBottom: 10 }}>
        {t('set.appearance.sectionsDesc')}
      </p>
      <div className="set-group">
        {SECTIONS.map((s) => (
          <SetRow key={s.key} title={s.title} desc={s.desc}>
            <SetSwitch
              checked={sectionOn(s.key)}
              onCheckedChange={(v) => mutation.mutate(boolEntry(SECTION_KEY[s.key], v))}
            />
          </SetRow>
        ))}
      </div>
    </>
  )
}
