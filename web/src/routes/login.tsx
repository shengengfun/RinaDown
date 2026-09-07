// #screen-login —— 服务器地址 + 令牌登录卡片，对齐 design/web/index.html。
//
// 首次运行分叉：挂载时无鉴权探一次 `GET /api/v1/setup/status`。服务器尚未设置
// 访问密钥时，同一张卡片改渲染「设置访问密钥」向导（密钥不再由服务器自动生成
// 打到 stderr——NAS 用户根本看不到容器日志）。设置成功即用新密钥直接登录。
import { useNavigate } from '@tanstack/react-router'
import { type FormEvent, useEffect, useState } from 'react'
import { Eye, EyeOff, RefreshCw } from 'lucide-react'
import { api, ApiError } from '../lib/api'
import { saveCredentials } from '../lib/auth'
import { resyncRemoteTasks } from '../lib/cloud/useRemoteTasks'
import { translateBackendMessage, useI18n } from '../lib/i18n'
import { randomAccessKey, validateAccessKey } from '../lib/token-policy'
import { CopyButton } from '../components/CopyButton'

/** 卡片形态：`probing` = 还没问出服务器状态，先不闪任何表单。 */
type Mode = 'probing' | 'login' | 'setup'

export function LoginScreen() {
  const { t } = useI18n()
  const navigate = useNavigate()
  const [mode, setMode] = useState<Mode>('probing')
  const [base, setBase] = useState(() => window.location.origin)
  const [token, setToken] = useState('')
  const [confirm, setConfirm] = useState('')
  const [showKey, setShowKey] = useState(false)
  const [remember, setRemember] = useState(true)
  const [error, setError] = useState('')
  const [pending, setPending] = useState(false)

  // 只探同源服务器：地址栏里的 base 是给「连别的主机」用的，而首次设置向导
  // 只对「你正打开的这台」有意义。探测失败一律退回登录框。
  useEffect(() => {
    let alive = true
    api
      .setupStatus('')
      .then((s) => {
        if (alive) setMode(s.setupRequired ? 'setup' : 'login')
      })
      .catch(() => {
        if (alive) setMode('login')
      })
    return () => {
      alive = false
    }
  }, [])

  function effectiveBase(): string {
    const trimmed = base.trim()
    return trimmed === window.location.origin ? '' : trimmed
  }

  async function handleLogin(e: FormEvent) {
    e.preventDefault()
    setError('')
    setPending(true)
    try {
      const target = effectiveBase()
      await api.probe(target, token)
      saveCredentials(target, token, remember)
      // 面板停在 /login 时云账号的 SSE 可能一直健康（它只看云账号登录态），期间
      // 收到的下发会被执行端的 !isAuthenticated() 丢弃，也没有信号触发它重连去
      // 补一次全量——本地登录成功是唯一能感知到这个转变的时刻，必须显式补一次。
      resyncRemoteTasks()
      navigate({ to: '/' })
    } catch (err) {
      setError(err instanceof ApiError ? translateBackendMessage(err.message) : t('login.connectFailed'))
    } finally {
      setPending(false)
    }
  }

  async function handleSetup(e: FormEvent) {
    e.preventDefault()
    const issue = validateAccessKey(token)
    if (issue) {
      setError(t(`setup.rule.${issue}`))
      return
    }
    if (token !== confirm) {
      setError(t('setup.mismatch'))
      return
    }
    setError('')
    setPending(true)
    try {
      // 向导只服务同源服务器，密钥落定后立即生效，无需重启即可用它登录。
      await api.completeSetup('', token)
      await api.probe('', token)
      saveCredentials('', token, remember)
      resyncRemoteTasks() // 同上：首次设置向导成功即视为一次本地登录，同样要补单。
      navigate({ to: '/' })
    } catch (err) {
      setError(err instanceof ApiError ? translateBackendMessage(err.message) : t('login.connectFailed'))
      // 409 = 别人抢先设过了；把卡片切回登录框，别让用户对着废表单重试。
      if (err instanceof ApiError && err.status === 409) setMode('login')
    } finally {
      setPending(false)
    }
  }

  return (
    <section className="wscreen active" id="screen-login">
      <div className="login-bg" />
      <div className="login-card">
        <span className="login-logo">
          <svg viewBox="30 30 452 452" role="img" xmlns="http://www.w3.org/2000/svg">
            <rect x="56" y="56" width="400" height="400" rx="88" fill="#3B82F6" />
            <path
              d="M 226 131 Q 226 119 238 119 L 274 119 Q 286 119 286 131 L 286 296 L 331 251 Q 340 242 349 251 L 363 265 Q 372 274 363 283 L 265 381 Q 256 390 247 381 L 149 283 Q 140 274 149 265 L 163 251 Q 172 242 181 251 L 226 296 Z"
              fill="#F2F4F8"
            />
          </svg>
        </span>
        <h2>{mode === 'setup' ? t('setup.title') : t('login.title')}</h2>
        <p className="login-sub">{mode === 'setup' ? t('setup.subtitle') : t('login.subtitle')}</p>

        {mode === 'probing' ? (
          <p className="login-hint">{t('login.connecting')}</p>
        ) : mode === 'setup' ? (
          <>
            <form className="contents" onSubmit={handleSetup}>
              <label className="field-label" htmlFor="setup-key">
                {t('setup.key')}
              </label>
              <div className="key-field">
                <input
                  id="setup-key"
                  className="text-input key-input"
                  type={showKey ? 'text' : 'password'}
                  spellCheck={false}
                  autoComplete="new-password"
                  required
                  value={token}
                  onChange={(e) => setToken(e.target.value)}
                />
                <div className="key-field-actions">
                  <button
                    type="button"
                    title={showKey ? t('set.sec.hideToken') : t('set.sec.showToken')}
                    onClick={() => setShowKey((s) => !s)}
                  >
                    {showKey ? <EyeOff /> : <Eye />}
                  </button>
                  <CopyButton value={token} title={t('set.sec.copyToken')} />
                  <button
                    type="button"
                    title={t('setup.generate')}
                    onClick={() => {
                      const next = randomAccessKey()
                      setToken(next)
                      setConfirm(next)
                      setShowKey(true)
                    }}
                  >
                    <RefreshCw />
                  </button>
                </div>
              </div>
              <label className="field-label" htmlFor="setup-confirm">
                {t('setup.confirm')}
              </label>
              <input
                id="setup-confirm"
                className="text-input key-input"
                type={showKey ? 'text' : 'password'}
                spellCheck={false}
                autoComplete="new-password"
                required
                value={confirm}
                onChange={(e) => setConfirm(e.target.value)}
              />
              <label className="remember">
                <input type="checkbox" checked={remember} onChange={(e) => setRemember(e.target.checked)} />
                <i />
                {t('login.remember')}
              </label>
              {error ? <p className="mt-[-6px] mb-3 text-[12px] text-danger">{error}</p> : null}
              <button className="btn primary block" type="submit" disabled={pending}>
                {pending ? t('setup.saving') : t('setup.submit')}
              </button>
            </form>
            <p className="login-hint">{t('setup.hint')}</p>
          </>
        ) : (
          <>
            <form className="contents" onSubmit={handleLogin}>
              <label className="field-label" htmlFor="login-base">
                {t('login.serverAddress')}
              </label>
              <input
                id="login-base"
                className="text-input"
                type="text"
                spellCheck={false}
                required
                value={base}
                onChange={(e) => setBase(e.target.value)}
              />
              <label className="field-label" htmlFor="login-token">
                {t('login.token')}
              </label>
              <input
                id="login-token"
                className="text-input"
                type="password"
                spellCheck={false}
                required
                value={token}
                onChange={(e) => setToken(e.target.value)}
              />
              <label className="remember">
                <input type="checkbox" checked={remember} onChange={(e) => setRemember(e.target.checked)} />
                <i />
                {t('login.remember')}
              </label>
              {error ? <p className="mt-[-6px] mb-3 text-[12px] text-danger">{error}</p> : null}
              <button className="btn primary block" type="submit" disabled={pending}>
                {pending ? t('login.connecting') : t('login.connect')}
              </button>
            </form>
            <p className="login-hint">{t('login.hint')}</p>
          </>
        )}
      </div>
      <div className="login-feats">
        <span>
          <b>{t('login.featEngine')}</b>{t('login.featEngineDesc')}
        </span>
        <span>
          <b>{t('login.featRealtime')}</b>{t('login.featRealtimeDesc')}
        </span>
        <span>
          <b>{t('login.featPrivacy')}</b>{t('login.featPrivacyDesc')}
        </span>
      </div>
    </section>
  )
}
