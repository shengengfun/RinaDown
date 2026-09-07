// 安全与访问：local_server_* 配置组 + 令牌管理 + WS 会话状态。
import { useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { CircleHelp, Eye, EyeOff, RefreshCw } from 'lucide-react'
import { api } from '../../lib/api'
import { CopyButton } from '../CopyButton'
import { useI18n } from '../../lib/i18n'
import type { ConfigMap } from '../../lib/types'
import { connStore, useStore } from '../../lib/ws'
import { alertDialog } from '../../lib/confirm'
import { updateStoredToken } from '../../lib/auth'
import { randomAccessKey, validateAccessKey } from '../../lib/token-policy'
import { SetRow, SetSwitch, TextInput } from './controls'

export function SecuritySettings({
  config,
  mutate,
}: {
  config: ConfigMap
  mutate: (entries: ConfigMap) => void
}) {
  const { t } = useI18n()
  const token = config.local_server_token ?? ''
  const [showToken, setShowToken] = useState(false)
  const takeover = (config.local_server_takeover_enabled ?? 'true') === 'true'
  const jsonrpc = (config.local_server_jsonrpc_enabled ?? 'true') === 'true'
  const mcp = (config.local_server_mcp_enabled ?? 'true') === 'true'
  const corsAllowAll = (config.local_server_cors_allow_all ?? 'false') === 'true'
  const origin = window.location.origin
  const conn = useStore(connStore)
  const qc = useQueryClient()
  const { data: stats } = useQuery({ queryKey: ['stats'], queryFn: api.stats, refetchInterval: 5000 })

  // 密钥在服务器端**立即生效**：本地凭证必须等落库成功后再改，否则请求失败
  // 却把本地 token 换掉，用户会被自己踢出去。这里不走通用 `mutate`（fire-and-
  // forget，拿不到成败），直接调 API 后失效 config 查询。
  async function saveToken(next: string) {
    const v = next.trim()
    if (v === token) return
    const issue = validateAccessKey(v)
    if (issue) {
      void alertDialog({ message: t(`setup.rule.${issue}`) })
      return
    }
    try {
      await api.putConfig({ local_server_token: v })
      updateStoredToken(v)
      await qc.invalidateQueries({ queryKey: ['config'] })
      void alertDialog({ message: t('set.sec.tokenSaved') })
    } catch (e) {
      void alertDialog({ message: e instanceof Error ? e.message : t('set.sec.tokenSaveFailed') })
    }
  }

  function randomToken() {
    void saveToken(randomAccessKey())
  }

  return (
    <>
      <h2 className="set-title">{t('set.security')}</h2>
      <p className="set-desc">{t('set.sec.desc')}</p>
      <div className="set-group">
        <SetRow title={t('set.sec.token')} desc={t('set.sec.tokenDesc')}>
          <div className="token-box">
            <TextInput
              value={token}
              onCommit={(v) => void saveToken(v)}
              password={!showToken}
              placeholder={t('set.sec.tokenPlaceholder')}
            />
            <button
              type="button"
              title={showToken ? t('set.sec.hideToken') : t('set.sec.showToken')}
              onClick={() => setShowToken((s) => !s)}
            >
              {showToken ? <EyeOff /> : <Eye />}
            </button>
            <CopyButton value={token} title={t('set.sec.copyToken')} />
            <button type="button" title={t('set.sec.genToken')} onClick={randomToken}>
              <RefreshCw />
            </button>
          </div>
        </SetRow>
      </div>
      <div className="set-group">
        <SetRow title={t('set.sec.takeover')} desc={t('set.sec.takeoverDesc')}>
          <SetSwitch checked={takeover} onCheckedChange={(v) => mutate({ local_server_takeover_enabled: String(v) })} />
        </SetRow>
        <AddrRow value={origin} copyTitle={t('set.sec.copyAddr')} />
      </div>
      <div className="set-group">
        <SetRow title={t('set.sec.jsonrpc')} desc={t('set.sec.jsonrpcDesc')}>
          <SetSwitch checked={jsonrpc} onCheckedChange={(v) => mutate({ local_server_jsonrpc_enabled: String(v) })} />
        </SetRow>
        <AddrRow value={`${origin}/jsonrpc`} copyTitle={t('set.sec.copyAddr')} />
      </div>
      <div className="set-group">
        <SetRow title={t('set.sec.api')} desc={t('set.sec.apiDesc')}>
          <SetSwitch checked disabled onCheckedChange={() => {}} />
        </SetRow>
        <AddrRow value={`${origin}/api/v1`} copyTitle={t('set.sec.copyAddr')} />
      </div>
      <div className="set-group">
        <SetRow title={t('set.sec.mcp')} desc={t('set.sec.mcpDesc')}>
          <SetSwitch checked={mcp} onCheckedChange={(v) => mutate({ local_server_mcp_enabled: String(v) })} />
        </SetRow>
        <AddrRow value={`${origin}/mcp`} copyTitle={t('set.sec.copyAddr')} />
      </div>
      <div className="set-group">
        <SetRow
          title={
            <>
              {t('set.sec.cors')}
              <span className="set-help" title={t('set.sec.corsHelp')}>
                <CircleHelp />
              </span>
            </>
          }
          desc={t('set.sec.corsDesc')}
        >
          <SetSwitch checked={corsAllowAll} onCheckedChange={(v) => mutate({ local_server_cors_allow_all: String(v) })} />
        </SetRow>
      </div>
      <div className="set-group">
        <SetRow
          title={t('set.sec.ws')}
          desc={conn.status === 'connected' ? t('set.sec.wsConnected', { rtt: conn.rttMs ?? '—' }) : t('set.sec.wsDisconnected')}
        >
          <span className="set-value">{stats ? t('set.sec.wsSessions', { n: stats.wsClients }) : '—'}</span>
        </SetRow>
      </div>
    </>
  )
}

/** 端点地址行：等宽字体地址 + 复制按钮。 */
function AddrRow({ value, copyTitle }: { value: string; copyTitle: string }) {
  return (
    <div className="set-row">
      <div className="token-box" style={{ flex: 1 }}>
        <span style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{value}</span>
        <CopyButton value={value} title={copyTitle} />
      </div>
    </div>
  )
}
