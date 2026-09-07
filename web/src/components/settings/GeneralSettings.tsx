// 通用：匿名统计 + 自定义分类管理（服务器 config 表）。
// 引擎连接/重试参数归「下载」分区 —— 设置分类以桌面端 settings_page 为基准。
import { useI18n } from '../../lib/i18n'
import type { ConfigMap } from '../../lib/types'
import { CategoriesSettings } from './CategoriesSettings'
import { SetRow, SetSwitch } from './controls'

export function GeneralSettings({
  config,
  mutate,
}: {
  config: ConfigMap
  mutate: (entries: ConfigMap) => void
}) {
  const { t } = useI18n()
  const analyticsEnabled = (config.analytics_enabled ?? 'true') === 'true'

  return (
    <>
      <h2 className="set-title">{t('set.general')}</h2>
      <p className="set-desc">{t('set.general.desc')}</p>
      <div className="set-group">
        <SetRow title={t('set.general.analytics')} desc={t('set.general.analyticsDesc')}>
          <SetSwitch
            checked={analyticsEnabled}
            onCheckedChange={(v) => mutate({ analytics_enabled: String(v) })}
          />
        </SetRow>
      </div>
      <CategoriesSettings config={config} mutate={mutate} />
    </>
  )
}
