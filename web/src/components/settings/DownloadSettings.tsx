// 下载：保存位置 / 行为 / 连接与性能 / 失败自动重试 / 高级 / 站点认证（服务器 config 表）。
// 分组与组内顺序以桌面端 settings_page.dart 的 _DownloadContent 为基准（镜像契约）。
import type { ReactNode } from 'react'
import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api } from '../../lib/api'
import { confirmDialog } from '../../lib/confirm'
import { queueDisplayName } from '../../lib/format'
import { useI18n } from '../../lib/i18n'
import type { ConfigMap } from '../../lib/types'
import { FsPicker } from '../dialogs/fs-picker'
import { UA_PRESETS } from '../../lib/ua-presets'
import { NumberFieldRow, SetRow, SetSelect, SetSwitch, TextInput } from './controls'
import { SiteAuthCredentials } from './SiteAuthCredentials'

const KB = 1024

const CUSTOM = '__custom__'

/** 主队列 id：`default_queue_id` 为空或指向已删除队列时的展示兜底。 */
const MAIN_QUEUE = 'main'

/** 解析引擎持久化的域名连接上限数据（首行 v1 版本标记），返回未过期条数。 */
function parseConnPolicyCount(raw: string): number {
  const lines = raw.split('\n')
  if (lines[0]?.trim() !== 'v1') return 0
  const nowSecs = Math.floor(Date.now() / 1000)
  const ttlSecs = 24 * 3600
  let count = 0
  for (const line of lines.slice(1)) {
    const parts = line.split('\t')
    if (parts.length !== 3 || !parts[0]) continue
    const cap = Number(parts[1])
    const ts = Number(parts[2])
    if (!Number.isFinite(cap) || cap < 1 || !Number.isFinite(ts)) continue
    if (nowSecs - ts < ttlSecs) count++
  }
  return count
}

/** 分组段：桌面 _SettingsGroup 的 title + 卡片，在 web 侧是小标题 + `.set-group`。
 *  两者包在同一个 `.set-section` 里，宽屏分列时标题不会与自己的卡片被拆到两列。 */
function SetSection({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="set-section">
      <h3 className="mb-2 font-semibold text-[12.5px] text-text2">{title}</h3>
      <div className="set-group">{children}</div>
    </section>
  )
}

export function DownloadSettings({
  config,
  mutate,
}: {
  config: ConfigMap
  mutate: (entries: ConfigMap) => void
}) {
  const { t } = useI18n()
  const { data: queues = [] } = useQuery({ queryKey: ['queues'], queryFn: api.listQueues })
  const saveDir = config.default_save_dir ?? ''
  const rememberLastSaveDir = (config.remember_last_save_dir ?? 'false') === 'true'
  // 与桌面端一致：KB/s 整数展示，引擎按 B/s 存储。
  const speedKB = Math.floor(Number(config.speed_limit_bytes ?? '0') / KB)
  const uploadKB = Math.floor(Number(config.upload_limit_bytes ?? '0') / KB)
  const ua = config.global_user_agent ?? ''
  const useServerTime = (config.use_server_time ?? 'false') === 'true'
  const silentSkipSelection = (config.silent_skip_selection ?? 'false') === 'true'
  const cdnMultiEnabled = (config.cdn_multi_enabled ?? '0') === '1'
  const cdnMaxNodes = Number(config.cdn_max_nodes ?? '0')
  const proxyMode = config.proxy_mode ?? 'none'
  const fileExistsBehavior = config.file_exists_behavior ?? 'rename'
  const fileMissingAction = config.file_missing_action ?? 'keep'
  const maxConcurrent = Number(config.max_concurrent_tasks ?? '5')
  const defaultSegments = Number(config.default_segments ?? '0')
  const autoMaxConnections = Number(config.auto_max_connections ?? '16')
  const connPolicyCount = parseConnPolicyCount(config.domain_conn_caps ?? '')
  const maxRetries = Number(config.max_auto_retries ?? '3')
  const retryDelay = Number(config.auto_retry_delay_secs ?? '5')

  // 默认队列：当前值失效时展示回落主队列，但不写回设置（对齐桌面 _DefaultQueueSelector）。
  const queueIds = new Set(queues.map((q) => q.queueId))
  const storedQueueId = config.default_queue_id ?? ''
  const effectiveQueueId = queueIds.has(storedQueueId)
    ? storedQueueId
    : queueIds.has(MAIN_QUEUE)
      ? MAIN_QUEUE
      : (queues[0]?.queueId ?? '')

  /** 开启多 CDN 并发时与代理互斥（对齐桌面端 _onCdnMultiChanged）：代理已启用则
   *  弹确认框——确认「关闭代理并开启」一次写入两个键，取消则不改任何状态。
   *  Auto 模式视同可用：CDN 聚合对直连任务仍然生效，不触发互斥。 */
  async function onCdnMultiChange(v: boolean) {
    if (!v || proxyMode === 'none' || proxyMode === 'auto') {
      mutate({ cdn_multi_enabled: v ? '1' : '0' })
      return
    }
    const ok = await confirmDialog({
      title: t('set.download.cdnMultiProxyConfirmTitle'),
      message:
        proxyMode === 'system'
          ? t('set.download.cdnMultiProxyConfirmDescSystem')
          : t('set.download.cdnMultiProxyConfirmDescManual'),
      confirmLabel: t('set.download.cdnMultiProxyConfirmDisable'),
    })
    if (ok) mutate({ proxy_mode: 'none', cdn_multi_enabled: '1' })
  }

  // 自定义模式：用户在下拉里选了"自定义"，或当前值不匹配任何预设。
  const isPreset = ua === '' || UA_PRESETS.some((p) => p.value === ua)
  const [customMode, setCustomMode] = useState(!isPreset)
  const customActive = customMode || !isPreset

  // Radix Select 把 value="" 视为"未选择"，触发器会显示空白 —— 默认项用哨兵值。
  const DEFAULT = '__default__'
  const uaOptions = [
    { label: t('set.download.uaDefault'), value: DEFAULT },
    ...UA_PRESETS,
    { label: t('common.custom'), value: CUSTOM },
  ]
  const selectValue = customActive ? CUSTOM : ua === '' ? DEFAULT : ua

  return (
    <>
      <h2 className="set-title">{t('set.download')}</h2>
      <p className="set-desc">{t('set.download.desc')}</p>

      <SetSection title={t('set.download.groupSaveLocation')}>
        <SetRow title={t('set.download.saveDir')} desc={t('set.download.saveDirDesc')}>
          <div className="dir-row" style={{ width: 300, flexShrink: 0 }}>
            <TextInput value={saveDir} onCommit={(v) => mutate({ default_save_dir: v })} />
            <FsPicker value={saveDir} onChange={(p) => mutate({ default_save_dir: p })} />
          </div>
        </SetRow>
        <SetRow
          title={t('set.download.rememberLastSaveDir')}
          desc={t('set.download.rememberLastSaveDirDesc')}
        >
          <SetSwitch
            checked={rememberLastSaveDir}
            onCheckedChange={(v) => mutate({ remember_last_save_dir: String(v) })}
          />
        </SetRow>
      </SetSection>

      <SetSection title={t('set.download.groupBehavior')}>
        {/* headless 无确认弹框：接管入口（扩展远程投递/脚本）创建的任务开启后
            跳过 BT 文件/画质的 WS 选择往返，直接按默认开始下载 */}
        <SetRow
          title={t('set.download.silentSkipSelection')}
          desc={t('set.download.silentSkipSelectionDesc')}
        >
          <SetSwitch
            checked={silentSkipSelection}
            onCheckedChange={(v) => mutate({ silent_skip_selection: String(v) })}
          />
        </SetRow>
        <SetRow title={t('set.download.serverTime')} desc={t('set.download.serverTimeDesc')}>
          <SetSwitch
            checked={useServerTime}
            onCheckedChange={(v) => mutate({ use_server_time: String(v) })}
          />
        </SetRow>
        <SetRow title={t('set.download.fileExists')} desc={t('set.download.fileExistsDesc')}>
          <SetSelect
            value={fileExistsBehavior === 'overwrite' ? 'overwrite' : 'rename'}
            onValueChange={(v) => mutate({ file_exists_behavior: v })}
            options={[
              { value: 'rename', label: t('set.download.fileExistsRename') },
              { value: 'overwrite', label: t('set.download.fileExistsOverwrite') },
            ]}
            width={160}
          />
        </SetRow>
        <SetRow
          title={t('set.download.fileMissingAction')}
          desc={t('set.download.fileMissingActionDesc')}
        >
          <SetSelect
            value={fileMissingAction === 'delete' ? 'delete' : 'keep'}
            onValueChange={(v) => mutate({ file_missing_action: v })}
            options={[
              { value: 'keep', label: t('set.download.fileMissingKeep') },
              { value: 'delete', label: t('set.download.fileMissingDelete') },
            ]}
            width={200}
          />
        </SetRow>
        {queues.length > 0 && (
          <SetRow title={t('set.download.defaultQueue')} desc={t('set.download.defaultQueueDesc')}>
            <SetSelect
              value={effectiveQueueId}
              onValueChange={(v) => mutate({ default_queue_id: v })}
              options={queues.map((q) => ({ value: q.queueId, label: queueDisplayName(q) }))}
              width={200}
            />
          </SetRow>
        )}
      </SetSection>

      <SetSection title={t('set.download.groupConnection')}>
        <NumberFieldRow
          title={t('set.general.segments')}
          desc={t('set.general.segmentsDesc')}
          value={defaultSegments}
          min={0}
          onCommit={(n) => mutate({ default_segments: String(n) })}
        />
        {defaultSegments === 0 && (
          <NumberFieldRow
            title={t('set.general.autoMaxConn')}
            desc={t('set.general.autoMaxConnDesc')}
            value={autoMaxConnections}
            min={1}
            onCommit={(n) => mutate({ auto_max_connections: String(n) })}
          />
        )}
        <SetRow title={t('set.download.cdnMulti')} desc={t('set.download.cdnMultiDesc')}>
          <SetSwitch checked={cdnMultiEnabled} onCheckedChange={(v) => void onCdnMultiChange(v)} />
        </SetRow>
        {cdnMultiEnabled && (
          <NumberFieldRow
            title={t('set.download.cdnMaxNodes')}
            desc={t('set.download.cdnMaxNodesDesc')}
            value={cdnMaxNodes}
            min={0}
            max={8}
            onCommit={(n) => mutate({ cdn_max_nodes: String(Math.min(8, Math.max(0, n))) })}
          />
        )}
        <SetRow title={t('set.general.connPolicy')} desc={t('set.general.connPolicyDesc')}>
          <div className="flex items-center gap-3">
            <span className="text-xs opacity-60">
              {connPolicyCount > 0
                ? t('set.general.connPolicyCount', { count: String(connPolicyCount) })
                : t('set.general.connPolicyEmpty')}
            </span>
            <button
              type="button"
              className="btn ghost sm"
              disabled={connPolicyCount === 0}
              onClick={() => mutate({ domain_conn_caps: '' })}
            >
              {t('set.general.connPolicyClear')}
            </button>
          </div>
        </SetRow>
        <NumberFieldRow
          title={t('set.general.maxConcurrent')}
          desc={t('set.general.maxConcurrentDesc')}
          value={maxConcurrent}
          min={1}
          onCommit={(n) => mutate({ max_concurrent_tasks: String(n) })}
        />
        <NumberFieldRow
          title={t('set.download.speedLimit')}
          desc={t('set.download.speedLimitDesc')}
          value={speedKB}
          min={0}
          onCommit={(n) => mutate({ speed_limit_bytes: String(Math.max(0, Math.round(n)) * KB) })}
        />
        <NumberFieldRow
          title={t('set.download.uploadLimit')}
          desc={t('set.download.uploadLimitDesc')}
          value={uploadKB}
          min={0}
          onCommit={(n) => mutate({ upload_limit_bytes: String(Math.max(0, Math.round(n)) * KB) })}
        />
      </SetSection>

      <SetSection title={t('set.download.groupRetry')}>
        <NumberFieldRow
          title={t('set.general.retries')}
          desc={t('set.general.retriesDesc')}
          value={maxRetries}
          min={0}
          onCommit={(n) => mutate({ max_auto_retries: String(n) })}
        />
        <NumberFieldRow
          title={t('set.general.retryDelay')}
          desc={t('set.general.retryDelayDesc')}
          value={retryDelay}
          min={0}
          onCommit={(n) => mutate({ auto_retry_delay_secs: String(n) })}
        />
      </SetSection>

      <SetSection title={t('set.download.groupAdvanced')}>
        <SetRow title={t('set.download.ua')} desc={t('set.download.uaDesc')}>
          <div style={{ display: 'flex', gap: 8, alignItems: 'center', flexShrink: 0 }}>
            {customActive && (
              <div style={{ width: 220 }}>
                <TextInput
                  value={ua}
                  placeholder={t('set.download.uaCustomPlaceholder')}
                  onCommit={(v) => mutate({ global_user_agent: v.trim() })}
                />
              </div>
            )}
            <SetSelect
              width={customActive ? 130 : 220}
              value={selectValue}
              onValueChange={(v) => {
                if (v === CUSTOM) {
                  setCustomMode(true)
                } else {
                  setCustomMode(false)
                  mutate({ global_user_agent: v === DEFAULT ? '' : v })
                }
              }}
              options={uaOptions}
            />
          </div>
        </SetRow>
      </SetSection>

      <SiteAuthCredentials config={config} mutate={mutate} />
    </>
  )
}
