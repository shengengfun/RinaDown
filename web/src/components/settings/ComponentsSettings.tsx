// 组件：ffmpeg / yt-dlp 托管安装管理。
// 状态探测 GET /api/v1/components/<name>，可安装版本 GET .../versions，
// 安装/更新 POST .../install（后台执行，经 WS componentProgress/componentResult 推送），
// 卸载 POST .../uninstall。手动路径复用既有 config 端点（PUT /api/v1/config，
// 键 component.ffmpeg.path / component.ytdlp.path，与 native/engine
// `components::CONFIG_FFMPEG_PATH`/`CONFIG_YTDLP_PATH` 一致）。
//
// 优先级：手动路径 > 托管安装 > 系统 PATH（与引擎 resolve_ffmpeg/resolve_ytdlp 语义一致）。
import { useState, type ReactNode } from 'react'
import { RefreshCw, Trash2 } from 'lucide-react'
import { cn } from '../../lib/cn'
import { confirmDialog } from '../../lib/confirm'
import { fmtBytes } from '../../lib/format'
import { translateBackendMessage, useI18n } from '../../lib/i18n'
import type { ComponentFfmpegStatus, ComponentYtdlpStatus, FfmpegSource } from '../../lib/types'
import { componentProgressStore, componentResultStore, useStore } from '../../lib/ws'
import { CopyButton } from '../CopyButton'
import {
  useFfmpegStatusQuery,
  useFfmpegVersionsQuery,
  useInstallFfmpegMutation,
  useInstallYtdlpMutation,
  useUninstallFfmpegMutation,
  useUninstallYtdlpMutation,
  useYtdlpStatusQuery,
  useYtdlpVersionsQuery,
} from '../../hooks/useComponents'
import { useConfigMutation, useConfigQuery } from '../../lib/config'
import { SetRow, SetSelect, TextFieldRow } from './controls'

type ComponentName = 'ffmpeg' | 'ytdlp'

/** config 键：手动指定路径，须与 native/engine `CONFIG_FFMPEG_PATH`/`CONFIG_YTDLP_PATH` 保持一致。 */
const CONFIG_PATH_KEY: Record<ComponentName, string> = {
  ffmpeg: 'component.ffmpeg.path',
  ytdlp: 'component.ytdlp.path',
}

const SOURCE_TONE: Record<FfmpegSource, 'accent' | 'neutral' | 'danger'> = {
  manual: 'neutral',
  managed: 'accent',
  system: 'neutral',
  none: 'danger',
}

function SourceBadge({ source }: { source: FfmpegSource }) {
  const { t } = useI18n()
  const tone = SOURCE_TONE[source]
  return (
    <span
      className={cn(
        'rounded-full px-2 py-0.5 text-[11px] font-medium',
        tone === 'accent' && 'bg-accent-weak text-accent',
        tone === 'neutral' && 'bg-surface2 text-text3',
        tone === 'danger' && 'bg-danger/10 text-danger',
      )}
    >
      {t(`components.source.${source}`)}
    </span>
  )
}

/** 路径展示行：等宽字体 + 溢出省略，非空时附复制按钮（对齐 SecuritySettings.AddrRow）。 */
function PathRow({ title, desc, value, empty }: { title: string; desc?: string; value: string; empty: string }) {
  return (
    <SetRow title={title} desc={desc}>
      <div className="token-box" style={{ flex: 1, minWidth: 0 }}>
        <span
          style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}
          title={value || undefined}
        >
          {value || empty}
        </span>
        {value ? <CopyButton value={value} /> : null}
      </div>
    </SetRow>
  )
}

interface ComponentCardProps {
  component: ComponentName
  status: ComponentFfmpegStatus | ComponentYtdlpStatus | undefined
  statusLoading: boolean
  statusError: boolean
  versionsQuery: ReturnType<typeof useFfmpegVersionsQuery>
  installMut: ReturnType<typeof useInstallFfmpegMutation>
  uninstallMut: ReturnType<typeof useUninstallFfmpegMutation>
}

/** 单个组件（ffmpeg/yt-dlp）状态/安装/卸载卡片 —— 两者结构完全对称，仅文案与端点不同。 */
function ComponentCard({
  component,
  status,
  statusLoading,
  statusError,
  versionsQuery,
  installMut,
  uninstallMut,
}: ComponentCardProps) {
  const { t } = useI18n()
  const { data: config, isLoading: configLoading } = useConfigQuery()
  const configMut = useConfigMutation()
  const [selectedVersion, setSelectedVersion] = useState('')

  const configKey = CONFIG_PATH_KEY[component]
  const binName = t(`components.${component}`)
  const progress = useStore(componentProgressStore)[component]
  const result = useStore(componentResultStore)
  const installing = installMut.isPending || progress !== undefined

  const manualPath = config?.[configKey] ?? ''
  const manualPathPlaceholder =
    component === 'ytdlp' ? t('components.ytdlpManualPathPlaceholder') : t('components.manualPathPlaceholder')
  const installDesc = component === 'ytdlp' ? t('components.ytdlpInstallDesc') : t('components.installDesc')
  const managedUnsupportedText =
    component === 'ytdlp' ? t('components.ytdlpManagedUnsupported') : t('components.managedUnsupported')

  async function uninstall() {
    const ok = await confirmDialog({
      title: t('components.uninstallTitle', { bin: binName }),
      message: t('components.uninstallMsg', { bin: binName }),
      danger: true,
    })
    if (ok) uninstallMut.mutate()
  }

  const pct =
    progress && progress.totalBytes > 0 ? Math.round((progress.downloadedBytes / progress.totalBytes) * 100) : null

  let versionsBody: ReactNode
  if (status && !status.managedSupported) {
    // 平台不支持托管安装（macOS 等为 false）：静态引导，不展示版本选择/安装按钮，
    // 也不发起版本拉取，避免反复弹「不支持安装」。
    versionsBody = (
      <SetRow title={t('components.install')} align="start">
        <p className="text-[12px] text-text3">{managedUnsupportedText}</p>
      </SetRow>
    )
  } else if (versionsQuery.isLoading) {
    // 与加载完成后的行同 title + desc：卡片高度不变，宽屏分列时不会因为
    // 版本列表到货而重新均衡列高（首屏抖动）。
    versionsBody = (
      <SetRow title={t('components.install')} desc={installDesc}>
        <span className="set-value">{t('common.loading')}</span>
      </SetRow>
    )
  } else if (versionsQuery.isError || !versionsQuery.data || versionsQuery.data.versions.length === 0) {
    const reason = versionsQuery.error
      ? translateBackendMessage(versionsQuery.error.message)
      : t('components.versionsEmpty')
    versionsBody = (
      <SetRow title={t('components.install')} align="start">
        <div className="flex flex-col items-start gap-2">
          <p className="text-[12px] text-text3">{t('components.versionsFailed', { error: reason, bin: binName })}</p>
          <button
            type="button"
            className="btn ghost sm flex-shrink-0"
            disabled={versionsQuery.isFetching}
            onClick={() => void versionsQuery.refetch()}
          >
            <RefreshCw size={14} className={versionsQuery.isFetching ? 'animate-spin' : undefined} />
            {t('common.retry')}
          </button>
        </div>
      </SetRow>
    )
  } else {
    const data = versionsQuery.data
    versionsBody = (
      <SetRow title={t('components.install')} desc={installDesc}>
        <div className="flex flex-shrink-0 items-center gap-2">
          <SetSelect
            value={selectedVersion}
            onValueChange={setSelectedVersion}
            width={230}
            options={[
              { value: '', label: t('components.latestStable', { version: data.latestStable }) },
              ...data.versions.map((v) => ({ value: v, label: v })),
            ]}
          />
          <button
            type="button"
            className="btn ghost sm flex-shrink-0"
            disabled={installing}
            onClick={() => installMut.mutate(selectedVersion || undefined)}
          >
            <RefreshCw size={14} />
            {installing
              ? t('components.installing')
              : status?.managedVersion
                ? t('components.update')
                : t('components.installNow')}
          </button>
        </div>
      </SetRow>
    )
  }

  return (
    // 整个组件块（标题 + 状态卡 + 安装卡）是一列内的整体，宽屏多列时不拆开，
    // 避免「状态卡很高、安装卡很矮」在另一列留下大片空白。
    <section className="set-section">
      <h3 className="set-title" style={{ fontSize: 13, marginTop: 0 }}>
        {binName}
      </h3>
      <p className="set-desc">{t(`components.${component}Desc`)}</p>

      <div className="set-group">
        {statusLoading || configLoading ? (
          // 骨架与加载完成后的行数一致（状态 / 生效路径 / 版本 / 系统 PATH），
          // 卡片高度不跳变，宽屏分列时首屏不抖。
          <>
            <SetRow title={t('components.status')}>
              <span className="set-value">{t('common.loading')}</span>
            </SetRow>
            <PathRow title={t('components.effectivePath')} value="" empty={t('common.loading')} />
            <SetRow title={t('components.version')}>
              <span className="set-value">{t('common.loading')}</span>
            </SetRow>
            <PathRow
              title={t('components.systemPath')}
              desc={t('components.systemPathDesc', { bin: binName })}
              value=""
              empty={t('common.loading')}
            />
          </>
        ) : statusError || !status ? (
          <SetRow title={t('components.status')}>
            <span className="set-value text-danger">{t('set.loadFailed')}</span>
          </SetRow>
        ) : (
          <>
            <SetRow title={t('components.status')}>
              <SourceBadge source={status.source} />
            </SetRow>
            <PathRow title={t('components.effectivePath')} value={status.path} empty={t('components.none')} />
            <SetRow title={t('components.version')}>
              <span className="set-value">{status.version || t('components.unknown')}</span>
            </SetRow>
            <PathRow
              title={t('components.systemPath')}
              desc={t('components.systemPathDesc', { bin: binName })}
              value={status.systemPath}
              empty={t('components.none')}
            />
          </>
        )}
        <TextFieldRow
          title={t('components.manualPath')}
          desc={t('components.manualPathDesc')}
          value={manualPath}
          placeholder={manualPathPlaceholder}
          onCommit={(v) => configMut.mutate({ [configKey]: v.trim() })}
        />
      </div>

      <div className="set-group">
        {versionsBody}
        {progress && (
          <div className="d-progress" style={{ padding: '0 16px 14px' }}>
            <div className="d-progress-num">
              <b>{pct !== null ? `${pct}%` : '…'}</b>
              <span>
                {fmtBytes(progress.downloadedBytes)}
                {progress.totalBytes > 0 ? ` / ${fmtBytes(progress.totalBytes)}` : ''}
              </span>
            </div>
            <div className="d-bar">
              <i style={{ width: pct !== null ? `${pct}%` : '100%' }} />
            </div>
          </div>
        )}
        {result && result.component === component && (
          <p
            className={cn('text-[12px]', result.ok ? 'text-success' : 'text-danger')}
            style={{ padding: '0 16px 14px' }}
          >
            {result.ok ? t('components.installOk') : translateBackendMessage(result.message)}
          </p>
        )}
        {status?.managedVersion ? (
          <SetRow
            title={t('components.uninstall')}
            desc={t('components.uninstallDesc', { version: status.managedVersion })}
          >
            <button
              type="button"
              className="btn ghost sm flex-shrink-0 text-danger"
              disabled={uninstallMut.isPending}
              onClick={() => void uninstall()}
            >
              <Trash2 size={14} />
              {t('components.uninstall')}
            </button>
          </SetRow>
        ) : null}
      </div>
    </section>
  )
}

export function ComponentsSettings() {
  const { t } = useI18n()

  const { data: ffmpegStatus, isLoading: ffmpegStatusLoading, isError: ffmpegStatusError } = useFfmpegStatusQuery()
  const ffmpegVersionsQuery = useFfmpegVersionsQuery(ffmpegStatus?.managedSupported === true)
  const installFfmpegMut = useInstallFfmpegMutation()
  const uninstallFfmpegMut = useUninstallFfmpegMutation()

  const { data: ytdlpStatus, isLoading: ytdlpStatusLoading, isError: ytdlpStatusError } = useYtdlpStatusQuery()
  const ytdlpVersionsQuery = useYtdlpVersionsQuery(ytdlpStatus?.managedSupported === true)
  const installYtdlpMut = useInstallYtdlpMutation()
  const uninstallYtdlpMut = useUninstallYtdlpMutation()

  return (
    <div className="set-panel">
      <p className="set-desc">{t('set.components.desc')}</p>

      <ComponentCard
        component="ffmpeg"
        status={ffmpegStatus}
        statusLoading={ffmpegStatusLoading}
        statusError={ffmpegStatusError}
        versionsQuery={ffmpegVersionsQuery}
        installMut={installFfmpegMut}
        uninstallMut={uninstallFfmpegMut}
      />
      <ComponentCard
        component="ytdlp"
        status={ytdlpStatus}
        statusLoading={ytdlpStatusLoading}
        statusError={ytdlpStatusError}
        versionsQuery={ytdlpVersionsQuery}
        installMut={installYtdlpMut}
        uninstallMut={uninstallYtdlpMut}
      />
    </div>
  )
}
