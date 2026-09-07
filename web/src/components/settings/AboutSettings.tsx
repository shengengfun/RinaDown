// 关于：版本信息 + 更新渠道 + 更新检测 + 退出登录。
import { useQuery } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { Download } from 'lucide-react'
import { api, logsExportUrl } from '../../lib/api'
import { clearCredentials } from '../../lib/auth'
import { fmtBytes } from '../../lib/format'
import { useI18n } from '../../lib/i18n'
import type { ConfigMap } from '../../lib/types'
import { useUpdateCheck } from '../../lib/update'
import { disconnectWs } from '../../lib/ws'
import { CopyButton } from '../CopyButton'
import { SetRow, SetSelect } from './controls'

/** 日志总大小上限可选项（MB），与桌面端一致；缺省 10 MB。 */
const LOG_MAX_SIZE_OPTIONS = [5, 10, 20, 50, 100]

export function AboutSettings({
  config,
  mutate,
}: {
  config?: ConfigMap
  mutate: (entries: ConfigMap) => void
}) {
  const navigate = useNavigate()
  const { t } = useI18n()
  const { data: info, isLoading } = useQuery({ queryKey: ['info'], queryFn: api.info })
  const { data: logs } = useQuery({ queryKey: ['logs'], queryFn: api.logs })
  const logDir = logs?.dir ?? ''
  const fileCount = logs?.files.length ?? 0
  const totalSize = logs?.files.reduce((sum, f) => sum + f.size, 0) ?? 0
  const update = useUpdateCheck()
  const channel = config?.web_update_channel === 'frontier' ? 'frontier' : 'stable'
  const logMaxSizeMb = Number(config?.log_max_size_mb ?? '10') || 10
  const logStatus = !logs
    ? t('common.loading')
    : logs.initialized && !logs.degraded
      ? t('set.about.logStatusHealthy')
      : t('set.about.logStatusDegraded')
  const logStatusDesc = !logs
    ? undefined
    : logs.initialized && !logs.degraded
      ? t('set.about.logStatusHealthyDesc')
      : t('set.about.logStatusDegradedDesc', {
          count: logs.failureCount,
          error: logs.lastError ?? t('common.unknown'),
        })

  function logout() {
    clearCredentials()
    disconnectWs()
    navigate({ to: '/login' })
  }

  return (
    <>
      <h2 className="set-title">{t('set.about')}</h2>
      <p className="set-desc">FluxDown Server — Downloads, Supercharged.</p>
      <div className="set-group">
        <SetRow title={t('set.about.version')}>
          <span className="set-value">{isLoading ? t('common.loading') : info ? `${info.name} ${info.version}` : '—'}</span>
        </SetRow>
        <SetRow title={t('set.about.channel')} desc={t('set.about.channelDesc')}>
          <SetSelect
            value={channel}
            onValueChange={(v) => mutate({ web_update_channel: v })}
            options={[
              { value: 'stable', label: t('set.about.channelStable') },
              { value: 'frontier', label: t('set.about.channelFrontier') },
            ]}
            width={160}
          />
        </SetRow>
        {update.hasUpdate && update.releaseUrl ? (
          <SetRow title={t('set.about.newVersion', { version: `v${update.latest}` })}>
            <a className="btn primary sm" href={update.releaseUrl} target="_blank" rel="noreferrer">
              {t('set.about.getUpdate')}
            </a>
          </SetRow>
        ) : update.latest && update.current ? (
          <SetRow title={t('set.about.upToDate')}>
            <span className="set-value">v{update.latest}</span>
          </SetRow>
        ) : null}
      </div>
      <div className="set-group">
        <SetRow title={t('set.about.logDir')} desc={t('set.about.logDirDesc')}>
          <div className="token-box" style={{ flex: 1, minWidth: 0 }}>
            <span
              style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}
              title={logDir || undefined}
            >
              {logDir || t('common.loading')}
            </span>
            {logDir ? <CopyButton value={logDir} /> : null}
          </div>
        </SetRow>
        <SetRow title={t('set.about.logStatus')} desc={logStatusDesc}>
          <span className="set-value">{logStatus}</span>
        </SetRow>
        <SetRow
          title={t('set.about.logExport')}
          desc={t('set.about.logExportDesc', { count: fileCount, size: fmtBytes(totalSize) })}
        >
          {fileCount > 0 ? (
            <a className="btn ghost sm" href={logsExportUrl()} download>
              <Download />
              {t('set.about.logExportBtn')}
            </a>
          ) : (
            <button type="button" className="btn ghost sm" disabled>
              <Download />
              {t('set.about.logExportBtn')}
            </button>
          )}
        </SetRow>
        <SetRow title={t('set.about.logMaxSize')} desc={t('set.about.logMaxSizeDesc')}>
          <SetSelect
            value={String(logMaxSizeMb)}
            onValueChange={(v) => mutate({ log_max_size_mb: v })}
            options={LOG_MAX_SIZE_OPTIONS.map((mb) => ({ value: String(mb), label: `${mb} MB` }))}
            placeholder={`${logMaxSizeMb} MB`}
            width={160}
          />
        </SetRow>
      </div>
      {/* 退出登录卡与页尾语同段：宽屏分列时页尾语跟在卡片下面，不会孤零零悬在空白里。 */}
      <section className="set-section">
        <div className="set-group">
          <SetRow title={t('set.about.logout')} desc={t('set.about.logoutDesc')}>
            <button type="button" className="btn danger sm" onClick={logout}>
              {t('set.about.logout')}
            </button>
          </SetRow>
        </div>
        <p className="set-desc" style={{ marginTop: 14 }}>
          {t('set.about.tagline')}
        </p>
      </section>
    </>
  )
}
