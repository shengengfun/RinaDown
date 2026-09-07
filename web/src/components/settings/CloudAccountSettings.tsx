// 云账户：登录/注册/设备验证/设备管理，与本地下载器登录态完全独立（面板本身作为
// 一台 web 设备接入 FluxCloud，见 lib/cloud/*）。未登录时纯介绍 + 登录/注册卡片，不影响
// 面板本地功能；已登录展示资料卡 + 设备列表 + 云服务器地址。

import * as Dialog from '@radix-ui/react-dialog'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { AlertTriangle, ArrowLeft, Check, ChevronRight, Cloud, Copy, Crown, Eye, EyeOff, Monitor, Pencil, Search, Smartphone, Trash2, X } from 'lucide-react'
import { type FormEvent, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { CopyButton } from '../CopyButton'
import { cn } from '../../lib/cn'
import { CLOUD_BASE_URL_EDITABLE, cloudApi, getCloudBaseUrl, isCloudBaseUrlCustom, resetCloudBaseUrl, setCloudBaseUrl } from '../../lib/cloud/client'
import { deviceLabel } from '../../lib/cloud/deviceLabel'
import { suggest } from '../../lib/cloud/nickname'
import { applyCloudSession, cloudDeviceId, getCloudRefreshToken, signOutCloud, updateCloudUser, useCloudSession } from '../../lib/cloud/session'
import { CloudApiError, type CatalogPlan, type CloudDevice, type CloudProfile, type CloudUser, type ReferralCode, type ReferralRecord, type ReferralRecordStatus } from '../../lib/cloud/types'
import { confirmDialog } from '../../lib/confirm'
import { copyText } from '../../lib/copy'
import { fmtIsoTime, fmtRelativeTime } from '../../lib/format'
import type { I18nKey } from '../../lib/i18n'
import { useI18n } from '../../lib/i18n'
import { toast } from '../../lib/toast'
import { MarkdownLite } from '../../lib/markdown'
import { DirectDevicesSection } from './DirectDevicesSection'
import { SetRow, TextInput } from './controls'

const DEVICES_QUERY_KEY = ['cloud', 'devices']
const ME_QUERY_KEY = ['cloud', 'me']
// ---------------------------------------------------------------------------
// 套餐目录快照：上次成功拉取的 catalog 落盘 localStorage。首帧徽标必须是
// "上次最终显示的 UI"——刷新页面或云端暂不可达时，若从空目录起步，徽标会
// 先闪一帧默认纯文本 pill 再跳成正式徽标。与桌面端 cloud_plans_catalog 对称。
// ---------------------------------------------------------------------------

const PLAN_CATALOG_CACHE_KEY = 'fluxdown.cloud.plansCatalog'

function readCatalogCache(): CatalogPlan[] {
  try {
    const raw = localStorage.getItem(PLAN_CATALOG_CACHE_KEY)
    if (!raw) return []
    const parsed = JSON.parse(raw) as unknown
    return Array.isArray(parsed) ? (parsed as CatalogPlan[]) : []
  } catch {
    return []
  }
}

function writeCatalogCache(list: CatalogPlan[]) {
  try {
    localStorage.setItem(PLAN_CATALOG_CACHE_KEY, JSON.stringify(list))
  } catch {
    /* 隐私模式/配额耗尽：下次仍从网络拉取，仅损失首帧缓存 */
  }
}

/** 套餐目录：初始值取本地快照，挂载后台拉取覆盖并回写快照；拉取失败保持
 *  既有快照渲染（正是"保证上次最终显示的 UI"的场景，失败静默）。 */
function useCatalogPlans(): CatalogPlan[] {
  const [catalog, setCatalog] = useState<CatalogPlan[]>(readCatalogCache)
  useEffect(() => {
    let alive = true
    cloudApi
      .plansCatalog()
      .then((list) => {
        if (!alive) return
        setCatalog(list)
        writeCatalogCache(list)
      })
      .catch(() => {})
    return () => {
      alive = false
    }
  }, [])
  return catalog
}

const BADGE_HEX_RE = /^#[0-9a-f]{6}$/i

/** 编号后缀：badgeNumbered 且拿到具体编号才拼（不猜测/不占位），补零位数 1-6。 */
function badgeLabel(plan: CatalogPlan, ordinal: number | null | undefined): string {
  const base = plan.badge ?? ''
  if (!plan.badgeNumbered || ordinal == null) return base
  const digits = Math.min(Math.max(plan.badgeNumberDigits ?? 4, 1), 6)
  return `${base} No.${String(ordinal).padStart(digits, '0')}`
}

/** 套餐徽标 pill：形状/颜色完全由服务端 badgeStyle/badgeColor 数据驱动（对齐
 *  桌面端 _PlanTag 的 outline | solid | medal | ribbon 四态），未知 style 兜底
 *  outline；颜色非法回退主题 accent。纯色/描边渲染，不使用渐变。 */
function PlanBadge({ plan, ordinal }: { plan: CatalogPlan; ordinal: number | null | undefined }) {
  const color = BADGE_HEX_RE.test(plan.badgeColor?.trim() ?? '') ? plan.badgeColor.trim() : 'var(--accent)'
  const label = badgeLabel(plan, ordinal)
  const base = 'inline-flex items-center gap-1 rounded-full px-2 py-px text-[10.5px] font-semibold leading-[17px]'
  switch (plan.badgeStyle) {
    case 'solid':
      return (
        <span className={base} style={{ backgroundColor: color, color: '#fff' }}>
          {label}
        </span>
      )
    case 'medal':
      return (
        <span className="inline-flex items-stretch overflow-hidden rounded-full border text-[10.5px] font-semibold leading-[17px]" style={{ borderColor: color }}>
          <span className="flex items-center px-1.5" style={{ backgroundColor: color, color: '#fff' }}>
            <Crown size={9} />
          </span>
          <span className="px-1.5 py-px" style={{ color }}>
            {label}
          </span>
        </span>
      )
    case 'ribbon':
      // 角标丝带只适合卡片容器，行内 pill 场景近似为实心 + 粗字重 + 加宽字距
      // （同桌面端 _PlanTag 的有意简化）。
      return (
        <span className={base} style={{ backgroundColor: color, color: '#fff', fontWeight: 800, letterSpacing: '0.05em' }}>
          {label}
        </span>
      )
    default:
      return (
        <span className={base} style={{ border: `1.3px solid ${color}`, color, backgroundColor: 'color-mix(in srgb, currentColor 8%, transparent)' }}>
          <Crown size={9} />
          {label}
        </span>
      )
  }
}

// ---------------------------------------------------------------------------
// 错误码 → 本地化文案；未识别的 code 回退服务端原文 message。
// ---------------------------------------------------------------------------

const CLOUD_ERROR_KEYS: Record<string, I18nKey> = {
  invalid_credentials: 'cloud.err.invalidCredentials',
  invalid_code: 'cloud.err.invalidCode',
  rate_limited: 'cloud.err.rateLimited',
  email_taken: 'cloud.err.emailTaken',
  account_disabled: 'cloud.err.accountDisabled',
  registration_closed: 'cloud.err.registrationClosed',
  registration_incomplete: 'cloud.err.registrationIncomplete',
  network_error: 'cloud.err.network',
}

function cloudErrorText(t: (key: I18nKey, params?: Record<string, string | number>) => string, err: unknown): string {
  if (err instanceof CloudApiError) {
    const key = CLOUD_ERROR_KEYS[err.code]
    if (key) return t(key)
    if (err.code === 'validation') return err.message || t('cloud.err.validation')
    return err.message || t('cloud.err.unknown')
  }
  return t('cloud.err.network')
}

// ---------------------------------------------------------------------------
// Origin ID 自助修改：错误码 → 本地化文案，独立于 CLOUD_ERROR_KEYS（登录/注册场景）。
// ---------------------------------------------------------------------------

const ORIGIN_ID_ERROR_KEYS: Record<string, I18nKey> = {
  origin_id_taken: 'cloud.err.originIdTaken',
  origin_id_already_changed: 'cloud.err.originIdAlreadyChanged',
  origin_id_change_not_allowed: 'cloud.err.originIdChangeNotAllowed',
  validation_error: 'cloud.err.originIdInvalid',
}

function originIdErrorText(t: (key: I18nKey, params?: Record<string, string | number>) => string, err: unknown): string {
  if (err instanceof CloudApiError) {
    const key = ORIGIN_ID_ERROR_KEYS[err.code]
    if (key) return t(key)
    return err.message || t('cloud.err.unknown')
  }
  return t('cloud.err.network')
}

/** OID 规则（契约）：≥10000 的整数。 */
function isValidOriginId(n: number): boolean {
  return Number.isInteger(n) && n >= 10000
}

// ---------------------------------------------------------------------------
// 昵称自助修改：错误码 → 本地化文案，独立于 CLOUD_ERROR_KEYS（登录/注册场景）。
// ---------------------------------------------------------------------------

const NICKNAME_ERROR_KEYS: Record<string, I18nKey> = {
  validation_error: 'cloud.err.nicknameInvalid',
}

function nicknameErrorText(t: (key: I18nKey, params?: Record<string, string | number>) => string, err: unknown): string {
  if (err instanceof CloudApiError) {
    const key = NICKNAME_ERROR_KEYS[err.code]
    if (key) return t(key)
    return err.message || t('cloud.err.unknown')
  }
  return t('cloud.err.network')
}

/** 昵称规则（契约）：trim 后 1-32 字符。 */
function isValidNickname(v: string): boolean {
  const trimmed = v.trim()
  return trimmed.length > 0 && trimmed.length <= 32
}

// ---------------------------------------------------------------------------
// 推荐码创建：错误码 → 本地化文案，独立于 CLOUD_ERROR_KEYS。格式/上限校验走
// AppError::Validation，服务端已给出中文提示消息，走 fallback 原样透传。
// ---------------------------------------------------------------------------

const REFERRAL_CODE_ERROR_KEYS: Record<string, I18nKey> = {
  referral_code_taken: 'cloud.referral.err.codeTaken',
}

function referralCodeErrorText(t: (key: I18nKey, params?: Record<string, string | number>) => string, err: unknown): string {
  if (err instanceof CloudApiError) {
    const key = REFERRAL_CODE_ERROR_KEYS[err.code]
    if (key) return t(key)
    return err.message || t('cloud.err.unknown')
  }
  return t('cloud.err.network')
}

// ---------------------------------------------------------------------------
// 平台标签：契约已知取值 windows|macos|linux|android|ios|web，未知值原样展示。
// ---------------------------------------------------------------------------

const PLATFORM_LABEL_KEYS: Record<string, I18nKey> = {
  windows: 'cloud.platform.windows',
  macos: 'cloud.platform.macos',
  linux: 'cloud.platform.linux',
  android: 'cloud.platform.android',
  ios: 'cloud.platform.ios',
  web: 'cloud.platform.web',
}

function platformLabel(t: (key: I18nKey) => string, platform?: string): string {
  if (!platform) return '—'
  const key = PLATFORM_LABEL_KEYS[platform]
  return key ? t(key) : platform
}

export function CloudAccountSettings() {
  const { t } = useI18n()
  const session = useCloudSession()
  return (
    <div className="max-w-[640px]">
      <h2 className="set-title">{t('set.account')}</h2>
      <p className="set-desc">{t('set.account.desc')}</p>
      {session.status === 'authenticated' && session.user ? (
        <LoggedInPanel user={session.user} />
      ) : (
        <AuthPanel />
      )}
      <DirectDevicesSection />
      <CloudServerAddressGroup />
    </div>
  )
}

// ---------------------------------------------------------------------------
// 未登录：介绍卡片 + 登录/注册卡片（卡片内直接呈现表单，无需二次“返回选择”）
// ---------------------------------------------------------------------------

type AuthView = 'intro' | 'login' | 'register'

function AuthPanel() {
  const { t } = useI18n()
  const [view, setView] = useState<AuthView>('intro')
  const [prefillEmail, setPrefillEmail] = useState('')
  const [prefillPassword, setPrefillPassword] = useState('')
  const [incomplete, setIncomplete] = useState(false)

  function goRegister(email: string, password = '', fromIncomplete = false) {
    setPrefillEmail(email)
    setPrefillPassword(password)
    setIncomplete(fromIncomplete)
    setView('register')
  }

  if (view === 'login') {
    return (
      <LoginCard
        onSwitchToRegister={(email) => goRegister(email)}
        onRegistrationIncomplete={(email, password) => goRegister(email, password, true)}
      />
    )
  }
  if (view === 'register') {
    return (
      <RegisterCard
        initialEmail={prefillEmail}
        initialPassword={prefillPassword}
        incomplete={incomplete}
        onSwitchToLogin={() => setView('login')}
      />
    )
  }
  return (
    <div className="cloud-card">
      <div className="cloud-card-head">
        <span className="cloud-card-icon">
          <Cloud size={20} />
        </span>
        <h3>{t('cloud.introTitle')}</h3>
        <p>{t('cloud.introDesc')}</p>
      </div>
      <div className="flex items-center justify-center gap-2">
        <button type="button" className="btn primary" onClick={() => setView('login')}>
          {t('cloud.login')}
        </button>
        <button type="button" className="btn ghost" onClick={() => setView('register')}>
          {t('cloud.register')}
        </button>
      </div>
    </div>
  )
}

// ---------------------------------------------------------------------------
// 验证码输入面板（登录新设备验证 / 验证码登录 / 注册验证码 共用）
// 说明性文字由外层卡片头（cloud-card-head）承载，这里只负责输入 + 状态 + 操作。
// ---------------------------------------------------------------------------

function VerificationCodeStep({
  code,
  onCodeChange,
  ttlSeconds,
  sentAt,
  busy,
  error,
  onResend,
  onSubmit,
  onBack,
}: {
  code: string
  onCodeChange: (v: string) => void
  ttlSeconds: number
  sentAt: number
  busy: boolean
  error: string
  onResend: () => void
  onSubmit: () => void
  onBack: () => void
}) {
  const { t } = useI18n()
  const [ttlLeft, setTtlLeft] = useState(ttlSeconds)
  const [resendLeft, setResendLeft] = useState(60)

  // sentAt 每次发码/重发都会变化（即便 ttlSeconds 数值相同），据此重置倒计时。
  useEffect(() => {
    setTtlLeft(ttlSeconds)
    setResendLeft(60)
  }, [ttlSeconds, sentAt])

  useEffect(() => {
    const timer = window.setInterval(() => {
      setTtlLeft((v) => Math.max(0, v - 1))
      setResendLeft((v) => Math.max(0, v - 1))
    }, 1000)
    return () => window.clearInterval(timer)
  }, [])

  return (
    <div className="flex flex-col">
      {/* 验证码步骤才出现的返回入口：轻量文字链接，非按钮行。 */}
      <button type="button" className="link-btn mb-4 inline-flex w-fit items-center gap-1 text-[12px]" disabled={busy} onClick={onBack}>
        <ArrowLeft size={12} /> {t('common.back')}
      </button>
      <label className="field-label" style={{ marginTop: 0 }}>
        {t('cloud.codePlaceholder')}
      </label>
      <input
        className="text-input"
        inputMode="numeric"
        autoFocus
        placeholder={t('cloud.codePlaceholder')}
        value={code}
        disabled={busy}
        onChange={(e) => onCodeChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') onSubmit()
        }}
      />
      <div className="mt-2 flex items-center justify-between">
        <span className="text-[11px] text-text3">{ttlLeft > 0 ? t('cloud.codeExpireIn', { s: ttlLeft }) : t('cloud.codeExpired')}</span>
        <button type="button" className="link-btn text-[12px]" disabled={resendLeft > 0 || busy} onClick={onResend}>
          {resendLeft > 0 ? t('cloud.resendCodeIn', { s: resendLeft }) : t('cloud.resendCode')}
        </button>
      </div>
      {error ? <p className="mt-2 text-[12px] text-danger">{error}</p> : null}
      <button type="button" className="btn primary block mt-5" disabled={busy || !code.trim()} onClick={onSubmit}>
        {busy ? t('common.loading') : t('cloud.verifySubmit')}
      </button>
    </div>
  )
}

// ---------------------------------------------------------------------------
// 登录卡片：验证码 / 密码 两 Tab（等宽分段控件）；密码登录命中新设备转入验证码步骤，
// 命中 registration_incomplete 转去注册（预填邮箱密码）。表单第一步无返回按钮，
// 仅验证码步骤需要返回（见 VerificationCodeStep）。
// ---------------------------------------------------------------------------

type LoginStep = 'form' | 'codeVerify' | 'deviceVerify'
type LoginTab = 'code' | 'password'

/** 纯数字视为 Origin ID；预填注册邮箱框时排除（注册接口仅认邮箱，见契约 v1.2）。 */
function looksLikeOriginId(v: string): boolean {
  return /^\d+$/.test(v.trim())
}

function LoginCard({
  onSwitchToRegister,
  onRegistrationIncomplete,
}: {
  onSwitchToRegister: (email: string) => void
  onRegistrationIncomplete: (email: string, password: string) => void
}) {
  const { t, locale } = useI18n()
  const [tab, setTab] = useState<LoginTab>('code')
  const [step, setStep] = useState<LoginStep>('form')
  const [account, setAccount] = useState('')
  const [password, setPassword] = useState('')
  const [code, setCode] = useState('')
  const [ttl, setTtl] = useState(0)
  const [sentAt, setSentAt] = useState(0)
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)

  async function sendLoginCode() {
    const e = account.trim()
    if (!e) return
    setBusy(true)
    setError('')
    try {
      const res = await cloudApi.codeSend(e)
      setTtl(res.ttlSeconds)
      setSentAt(Date.now())
      setStep('codeVerify')
    } catch (err) {
      setError(cloudErrorText(t, err))
    } finally {
      setBusy(false)
    }
  }

  async function submitCodeLogin() {
    const e = account.trim()
    if (!e || !code.trim()) return
    setBusy(true)
    setError('')
    try {
      // 邮箱不存在时命中自动注册，恒传建议昵称（服务端仅在自动注册新用户时采用，
      // 已存在用户忽略该字段，恒传安全）。
      const auth = await cloudApi.codeVerify(e, code.trim(), suggest(locale))
      applyCloudSession(auth)
    } catch (err) {
      setError(cloudErrorText(t, err))
    } finally {
      setBusy(false)
    }
  }

  async function performLogin() {
    const e = account.trim()
    if (!e || !password) return
    setBusy(true)
    setError('')
    try {
      const result = await cloudApi.login(e, password)
      if (result.status === 'ok') {
        applyCloudSession(result.auth)
        return
      }
      setTtl(result.ttlSeconds)
      setSentAt(Date.now())
      setStep('deviceVerify')
    } catch (err) {
      if (err instanceof CloudApiError && err.code === 'registration_incomplete') {
        onRegistrationIncomplete(looksLikeOriginId(e) ? '' : e, password)
        return
      }
      setError(cloudErrorText(t, err))
    } finally {
      setBusy(false)
    }
  }

  async function submitDeviceVerify() {
    if (!code.trim()) return
    setBusy(true)
    setError('')
    try {
      const auth = await cloudApi.loginVerify(account.trim(), password, code.trim())
      applyCloudSession(auth)
    } catch (err) {
      setError(cloudErrorText(t, err))
    } finally {
      setBusy(false)
    }
  }

  function backToForm() {
    setStep('form')
    setError('')
    setCode('')
  }

  const useCode = tab === 'code'
  const headTitle = step === 'deviceVerify' ? t('cloud.deviceVerifyTitle') : step === 'codeVerify' ? t('cloud.loginTabCode') : t('cloud.loginTitle')
  const headSubtitle =
    step === 'deviceVerify'
      ? looksLikeOriginId(account)
        ? t('cloud.deviceVerifySubtitleAccount')
        : t('cloud.deviceVerifySubtitle', { email: account.trim() })
      : step === 'codeVerify'
        ? t('cloud.codeLoginSubtitle', { email: account.trim() })
        : t('cloud.loginSubtitle')

  return (
    <div className="cloud-card">
      <div className="cloud-card-head">
        <span className="cloud-card-icon">
          <Cloud size={20} />
        </span>
        <h3>{headTitle}</h3>
        <p>{headSubtitle}</p>
      </div>

      {step === 'codeVerify' ? (
        <VerificationCodeStep
          code={code}
          onCodeChange={setCode}
          ttlSeconds={ttl}
          sentAt={sentAt}
          busy={busy}
          error={error}
          onResend={() => void sendLoginCode()}
          onSubmit={() => void submitCodeLogin()}
          onBack={backToForm}
        />
      ) : step === 'deviceVerify' ? (
        <VerificationCodeStep
          code={code}
          onCodeChange={setCode}
          ttlSeconds={ttl}
          sentAt={sentAt}
          busy={busy}
          error={error}
          onResend={() => void performLogin()}
          onSubmit={() => void submitDeviceVerify()}
          onBack={backToForm}
        />
      ) : (
        <>
          <div className="seg-tabs mb-5">
            <button
              type="button"
              className={cn('seg-tab', useCode && 'active')}
              onClick={() => {
                setTab('code')
                setError('')
              }}
            >
              {t('cloud.loginTabCode')}
            </button>
            <button
              type="button"
              className={cn('seg-tab', !useCode && 'active')}
              onClick={() => {
                setTab('password')
                setError('')
              }}
            >
              {t('cloud.loginTabPassword')}
            </button>
          </div>
          <form
            className="flex flex-col"
            onSubmit={(e: FormEvent) => {
              e.preventDefault()
              if (useCode) void sendLoginCode()
              else void performLogin()
            }}
          >
            <label className="field-label" style={{ marginTop: 0 }}>
              {useCode ? t('cloud.emailPlaceholder') : t('cloud.accountPlaceholder')}
            </label>
            <input
              className="text-input"
              type={useCode ? 'email' : 'text'}
              required
              spellCheck={false}
              autoComplete="username"
              placeholder={useCode ? t('cloud.emailPlaceholder') : t('cloud.accountPlaceholder')}
              value={account}
              disabled={busy}
              onChange={(e) => setAccount(e.target.value)}
            />
            {useCode ? null : (
              <>
                <label className="field-label">{t('cloud.passwordPlaceholder')}</label>
                <input
                  className="text-input"
                  type="password"
                  required
                  placeholder={t('cloud.passwordPlaceholder')}
                  value={password}
                  disabled={busy}
                  onChange={(e) => setPassword(e.target.value)}
                />
              </>
            )}
            {error ? <p className="mt-2 text-[12px] text-danger">{error}</p> : null}
            <button type="submit" className="btn primary block mt-5" disabled={busy}>
              {busy ? t('common.loading') : useCode ? t('cloud.sendCode') : t('cloud.login')}
            </button>
            <p className="mt-4 text-center text-[11.5px] text-text3">
              {t('cloud.noAccountYet')}{' '}
              <button type="button" className="link-btn" onClick={() => onSwitchToRegister(looksLikeOriginId(account) ? '' : account.trim())}>
                {t('cloud.register')}
              </button>
            </p>
          </form>
        </>
      )}
    </div>
  )
}

// ---------------------------------------------------------------------------
// 注册卡片：邮箱+密码(≥8)+昵称(选填) → 验证码
// ---------------------------------------------------------------------------

type RegisterStep = 'form' | 'verify'

function RegisterCard({
  initialEmail,
  initialPassword,
  incomplete,
  onSwitchToLogin,
}: {
  initialEmail: string
  initialPassword: string
  incomplete: boolean
  onSwitchToLogin: () => void
}) {
  const { t, locale } = useI18n()
  const [step, setStep] = useState<RegisterStep>('form')
  const [email, setEmail] = useState(initialEmail)
  const [password, setPassword] = useState(initialPassword)
  // 预填「形容词+动物」建议昵称（情绪触点前置，见 lib/cloud/nickname.ts）；
  // 用户可改可清空，清空后不自动重填，仅提交前静默兜底（见 doRegister）。
  const [nickname, setNickname] = useState(() => suggest(locale))
  const [code, setCode] = useState('')
  const [ttl, setTtl] = useState(0)
  const [sentAt, setSentAt] = useState(0)
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  // 密码明文显示切换（眼睛按钮，同登录页访问密钥的既有模式）。
  const [showPassword, setShowPassword] = useState(false)

  async function doRegister() {
    setBusy(true)
    setError('')
    // 提交前若昵称被清空，静默重新建议一个，保证非空且不打断用户操作。
    const finalNickname = nickname.trim() || suggest(locale)
    if (finalNickname !== nickname) setNickname(finalNickname)
    try {
      const res = await cloudApi.register(email.trim(), password, finalNickname)
      setTtl(res.ttlSeconds)
      setSentAt(Date.now())
      setStep('verify')
    } catch (err) {
      setError(cloudErrorText(t, err))
    } finally {
      setBusy(false)
    }
  }

  function submitForm(e: FormEvent) {
    e.preventDefault()
    if (!email.trim() || password.length < 8) {
      setError(t('cloud.err.validation'))
      return
    }
    void doRegister()
  }

  async function submitVerify() {
    if (!code.trim()) return
    setBusy(true)
    setError('')
    try {
      const auth = await cloudApi.registerVerify(email.trim(), code.trim())
      applyCloudSession(auth)
    } catch (err) {
      setError(cloudErrorText(t, err))
    } finally {
      setBusy(false)
    }
  }

  const headTitle = step === 'verify' ? t('cloud.registerVerifyTitle') : t('cloud.registerTitle')
  const headSubtitle = step === 'verify' ? t('cloud.registerVerifySubtitle', { email: email.trim() }) : t('cloud.registerSubtitle')

  return (
    <div className="cloud-card">
      <div className="cloud-card-head">
        <span className="cloud-card-icon">
          <Cloud size={20} />
        </span>
        <h3>{headTitle}</h3>
        <p>{headSubtitle}</p>
      </div>

      {step === 'verify' ? (
        <VerificationCodeStep
          code={code}
          onCodeChange={setCode}
          ttlSeconds={ttl}
          sentAt={sentAt}
          busy={busy}
          error={error}
          onResend={() => void doRegister()}
          onSubmit={() => void submitVerify()}
          onBack={() => {
            setStep('form')
            setError('')
            setCode('')
          }}
        />
      ) : (
        <form className="flex flex-col" onSubmit={submitForm}>
          {incomplete ? <p className="mb-4 text-[12px] leading-relaxed text-text3">{t('cloud.registerIncompleteNotice')}</p> : null}
          <label className="field-label" style={{ marginTop: 0 }}>
            {t('cloud.emailPlaceholder')}
          </label>
          <input
            className="text-input"
            type="email"
            required
            spellCheck={false}
            placeholder={t('cloud.emailPlaceholder')}
            value={email}
            disabled={busy}
            onChange={(e) => setEmail(e.target.value)}
          />
          <label className="field-label">{t('cloud.passwordPlaceholder')}</label>
          <div className="pw-field">
            <input
              className="text-input"
              type={showPassword ? 'text' : 'password'}
              required
              placeholder={t('cloud.passwordPlaceholder')}
              value={password}
              disabled={busy}
              onChange={(e) => setPassword(e.target.value)}
            />
            <button
              type="button"
              className="pw-toggle"
              title={showPassword ? t('cloud.hidePassword') : t('cloud.showPassword')}
              aria-label={showPassword ? t('cloud.hidePassword') : t('cloud.showPassword')}
              onClick={() => setShowPassword((v) => !v)}
            >
              {showPassword ? <EyeOff /> : <Eye />}
            </button>
          </div>
          <p className="mt-1 text-[11px] text-text3">{t('cloud.passwordHint')}</p>
          <label className="field-label">{t('cloud.nicknamePlaceholder')}</label>
          <div className="flex items-center gap-2">
            <input
              className="text-input flex-1"
              type="text"
              placeholder={t('cloud.nicknamePlaceholder')}
              value={nickname}
              disabled={busy}
              onChange={(e) => setNickname(e.target.value)}
            />
            <button
              type="button"
              className="icon-btn flex-shrink-0"
              disabled={busy}
              title={t('cloud.nicknameReroll')}
              aria-label={t('cloud.nicknameReroll')}
              onClick={() => setNickname(suggest(locale))}
            >
              <span aria-hidden="true" className="text-[15px] leading-none">
                🎲
              </span>
            </button>
          </div>
          {error ? <p className="mt-2 text-[12px] text-danger">{error}</p> : null}
          <button type="submit" className="btn primary block mt-5" disabled={busy}>
            {busy ? t('common.loading') : t('cloud.register')}
          </button>
          <p className="mt-4 text-center text-[11.5px] text-text3">
            {t('cloud.alreadyHaveAccount')}{' '}
            <button type="button" className="link-btn" onClick={onSwitchToLogin}>
              {t('cloud.login')}
            </button>
          </p>
        </form>
      )}
    </div>
  )
}

// ---------------------------------------------------------------------------
// 已登录：资料卡 + 设备列表
// ---------------------------------------------------------------------------

function LoggedInPanel({ user }: { user: CloudUser }) {
  const { t } = useI18n()
  const qc = useQueryClient()
  const [loggingOut, setLoggingOut] = useState(false)
  const [originIdEditOpen, setOriginIdEditOpen] = useState(false)
  const [nicknameEditOpen, setNicknameEditOpen] = useState(false)
  const [referralOpen, setReferralOpen] = useState(false)
  const [nicknameRevealed, setNicknameRevealed] = useState(false)
  const [nicknameHovering, setNicknameHovering] = useState(false)
  const displayName = user.nickname || user.email.split('@')[0]
  // 徽标渲染用套餐目录（本地快照秒开 + 后台拉取覆盖，见 useCatalogPlans）。
  const catalog = useCatalogPlans()

  // 套餐能力 + 自助修改机会是否已用掉：GET /me 独立拉取（登录/刷新响应的 entitlements
  // 不落会话快照，仅在这一处门控编辑入口用得上，没必要为它扩大 session.ts 的职责）。
  const meQuery = useQuery({
    queryKey: ME_QUERY_KEY,
    queryFn: () => cloudApi.me(),
    staleTime: 10_000,
  })
  // `/me` 的当前套餐快照不受上架状态过滤；公开目录仍只承担可售套餐。
  const currentPlan = meQuery.data?.currentPlan ?? catalog.find((p) => p.code === user.plan)
  const canEditOriginId = !!meQuery.data?.entitlements.originIdEdit && !meQuery.data?.originIdChanged
  // 旧会话快照（登录早于 membershipOrdinal 上线）缺该字段时回退 /me 结果。
  const membershipOrdinal = user.membershipOrdinal ?? meQuery.data?.membershipOrdinal

  async function logout() {
    setLoggingOut(true)
    const rt = getCloudRefreshToken()
    if (rt) {
      try {
        await cloudApi.logout(rt)
      } catch {
        // 尽力通知服务端吊销 refreshToken，失败也不阻塞本地登出。
      }
    }
    signOutCloud()
  }

  // 修改成功：同步会话快照（驱动上方 OriginIdBadge 立即刷新）+ 更新 me 查询缓存，
  // 关闭对话框交由 OriginIdEditDialog 自身在 onChanged 后统一处理。
  function handleOriginIdChanged(profile: CloudProfile) {
    updateCloudUser({ originId: profile.originId, originIdChanged: profile.originIdChanged })
    qc.setQueryData(ME_QUERY_KEY, profile)
    setOriginIdEditOpen(false)
  }

  // 昵称修改成功：同步会话快照（驱动展示名立即刷新）+ 更新 me 查询缓存，关闭对话框
  // 交由 NicknameEditDialog 自身在 onChanged 后统一处理。
  function handleNicknameChanged(profile: CloudProfile) {
    updateCloudUser({ nickname: profile.nickname })
    qc.setQueryData(ME_QUERY_KEY, profile)
    setNicknameEditOpen(false)
  }

  return (
    <>
      <div className="set-group">
        <div className="flex items-center gap-3 p-4">
          <div
            className="flex min-w-0 flex-1 flex-wrap items-center gap-2"
            onMouseEnter={() => setNicknameHovering(true)}
            onMouseLeave={() => setNicknameHovering(false)}
          >
            <b
              className="cursor-pointer text-[14px] font-semibold"
              onClick={() => setNicknameRevealed((v) => !v)}
            >
              {displayName}
            </b>
            {nicknameRevealed || nicknameHovering ? (
              <button
                type="button"
                className="icon-btn sm flex-shrink-0"
                title={t('cloud.nicknameEditBtn')}
                aria-label={t('cloud.nicknameEditBtn')}
                onClick={() => setNicknameEditOpen(true)}
              >
                <Pencil size={14} />
              </button>
            ) : null}
            {currentPlan ? (
              // 硬规则同桌面端：徽标是否渲染唯一由 plan.badge 是否非空决定，
              // 空（服务端未配置，如免费档）时整行不出现任何徽标。
              currentPlan.badge ? (
                <PlanBadge plan={currentPlan} ordinal={membershipOrdinal} />
              ) : null
            ) : user.plan ? (
              // 目录未加载完成/未匹配到该 code：纯数据可用性问题，降级为旧的
              // 纯文本 pill 展示原始 plan code，不让徽标整个消失。
              <span className="rounded-full bg-accent-weak px-2 py-0.5 text-[10.5px] font-semibold text-accent">{user.plan}</span>
            ) : null}
            <OriginIdBadge originId={user.originId} />
            {canEditOriginId ? (
              <button
                type="button"
                className="icon-btn sm flex-shrink-0"
                title={t('cloud.originIdEditBtn')}
                aria-label={t('cloud.originIdEditBtn')}
                onClick={() => setOriginIdEditOpen(true)}
              >
                <Pencil size={14} />
              </button>
            ) : null}
          </div>
          <button type="button" className="btn ghost sm flex-shrink-0" disabled={loggingOut} onClick={() => void logout()}>
            {t('common.logout')}
          </button>
        </div>
      </div>
      <p className="mb-1 mt-6 text-[12.5px] font-semibold text-text2">{t('cloud.securityTitle')}</p>
      <p className="set-desc" style={{ marginBottom: 10 }}>
        {t('cloud.securityDesc')}
      </p>
      <div className="set-group">
        <SetRow title={t('cloud.emailLabel')}>
          <span className="min-w-0 flex-shrink truncate text-[12.5px] text-text2">{user.email}</span>
        </SetRow>
        <SetRow title={t('cloud.referral.rowTitle')} desc={t('cloud.referral.rowDesc')}>
          <button type="button" className="btn ghost sm" onClick={() => setReferralOpen(true)}>
            {t('cloud.referral.openBtn')}
          </button>
        </SetRow>
      </div>
      <DeviceListSection />
      <OriginIdEditDialog open={originIdEditOpen} onClose={() => setOriginIdEditOpen(false)} onChanged={handleOriginIdChanged} />
      <NicknameEditDialog
        open={nicknameEditOpen}
        currentNickname={user.nickname}
        onClose={() => setNicknameEditOpen(false)}
        onChanged={handleNicknameChanged}
      />
      <ReferralDialog open={referralOpen} onClose={() => setReferralOpen(false)} />
    </>
  )
}

/** Origin ID 徽标：胶囊 pill（accent 弱底、圆角、tabular-nums），点击复制纯数字
 *  （copyText 内部自带非安全上下文回退，失败静默）。null（pending 用户）兜底
 *  显示 #— 且不可点。颜色走 design.css .origin-badge（unlayered，显式声明背景/颜色
 *  才压得过下方全局 button 重置，Tailwind 工具类在这个元素上不生效，同 .link-btn 注释）。 */
function OriginIdBadge({ originId }: { originId: number | null }) {
  const { t } = useI18n()
  const [copied, setCopied] = useState(false)
  const timer = useRef<number | undefined>(undefined)
  useEffect(() => () => window.clearTimeout(timer.current), [])

  function copy() {
    if (originId == null) return
    copyText(String(originId))
    setCopied(true)
    window.clearTimeout(timer.current)
    timer.current = window.setTimeout(() => setCopied(false), 1500)
  }

  return (
    <button type="button" className="origin-badge flex-shrink-0" disabled={originId == null} onClick={copy} title={t('cloud.originId')}>
      <span>#{originId == null ? '—' : originId}</span>
      {originId != null ? copied ? <Check size={11} /> : <Copy size={11} /> : null}
    </button>
  )
}

/** Origin ID 自助修改对话框：数字输入 + 骰子建议 + 「仅一次」醒目提示；确认前先用
 *  GET /me/origin-id/check 预检可用性，通过才提交 PUT /me/origin-id。成功后把返回的
 *  最新 profile 经 onChanged 回传给调用方（LoggedInPanel 负责同步会话快照 + 缓存 +
 *  关闭对话框），本组件自身不持有“是否已提交成功”的全局状态。 */
function OriginIdEditDialog({
  open,
  onClose,
  onChanged,
}: {
  open: boolean
  onClose: () => void
  onChanged: (profile: CloudProfile) => void
}) {
  const { t } = useI18n()
  const [value, setValue] = useState('')
  const [error, setError] = useState('')
  const [rolling, setRolling] = useState(false)

  // 每次打开清空上次输入/错误，避免残留上一次会话的值。
  useEffect(() => {
    if (open) {
      setValue('')
      setError('')
    }
  }, [open])

  async function roll() {
    setRolling(true)
    setError('')
    try {
      const res = await cloudApi.randomOriginId()
      setValue(String(res.originId))
    } catch (err) {
      setError(originIdErrorText(t, err))
    } finally {
      setRolling(false)
    }
  }

  const changeMut = useMutation({
    mutationFn: async (n: number) => {
      const check = await cloudApi.checkOriginId(n)
      if (!check.available) {
        throw new CloudApiError(check.reason === 'taken' ? 'origin_id_taken' : 'validation_error', '', 409)
      }
      return cloudApi.changeOriginId(n)
    },
    onSuccess: (profile) => {
      toast(t('cloud.originIdEditSuccess'))
      onChanged(profile)
    },
    onError: (err) => setError(originIdErrorText(t, err)),
  })

  function confirm() {
    const n = Number.parseInt(value, 10)
    if (!isValidOriginId(n)) {
      setError(t('cloud.err.originIdInvalid'))
      return
    }
    setError('')
    changeMut.mutate(n)
  }

  return (
    <Dialog.Root
      open={open}
      onOpenChange={(o) => {
        if (!o) onClose()
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="wbackdrop show" />
        <Dialog.Content className="dialog sm show" aria-describedby={undefined}>
          <header className="dlg-head">
            <Dialog.Title asChild>
              <b>{t('cloud.originIdEditTitle')}</b>
            </Dialog.Title>
            <Dialog.Close asChild>
              <button type="button" className="icon-btn sm" aria-label={t('common.close')}>
                <X size={16} />
              </button>
            </Dialog.Close>
          </header>
          <form
            onSubmit={(e) => {
              e.preventDefault()
              confirm()
            }}
          >
            <div className="dlg-body">
              <label className="field-label" htmlFor="origin-id-edit-value">
                {t('cloud.originIdEditLabel')}
              </label>
              <div className="flex items-center gap-2">
                <input
                  id="origin-id-edit-value"
                  className="text-input flex-1"
                  type="text"
                  inputMode="numeric"
                  autoFocus
                  placeholder={t('cloud.originIdEditPlaceholder')}
                  value={value}
                  disabled={changeMut.isPending}
                  onChange={(e) => setValue(e.target.value.replace(/\D/g, ''))}
                />
                <button
                  type="button"
                  className="icon-btn flex-shrink-0"
                  disabled={rolling || changeMut.isPending}
                  title={t('cloud.originIdEditRoll')}
                  aria-label={t('cloud.originIdEditRoll')}
                  onClick={() => void roll()}
                >
                  <span aria-hidden="true" className="text-[15px] leading-none">
                    🎲
                  </span>
                </button>
              </div>
              <p className="mt-2 flex items-start gap-1.5 text-[11.5px] font-medium text-warning">
                <AlertTriangle size={13} className="mt-[1px] flex-shrink-0" />
                {t('cloud.originIdEditWarning')}
              </p>
              {error ? <p className="mt-2 text-[12px] text-danger">{error}</p> : null}
            </div>
            <footer className="dlg-foot">
              <Dialog.Close asChild>
                <button type="button" className="btn ghost">
                  {t('common.cancel')}
                </button>
              </Dialog.Close>
              <button type="submit" className="btn primary" disabled={changeMut.isPending || !value.trim()}>
                {changeMut.isPending ? t('common.loading') : t('common.confirm')}
              </button>
            </footer>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}

/** 昵称自助修改对话框：单文本输入 + 长度校验（trim 后 1-32 字符，服务端再校验一次）。
 *  成功后把返回的最新 profile 经 onChanged 回传给调用方（LoggedInPanel 负责同步会话
 *  快照 + 缓存 + 关闭对话框），本组件自身不持有“是否已提交成功”的全局状态。 */
function NicknameEditDialog({
  open,
  currentNickname,
  onClose,
  onChanged,
}: {
  open: boolean
  currentNickname: string
  onClose: () => void
  onChanged: (profile: CloudProfile) => void
}) {
  const { t } = useI18n()
  const [value, setValue] = useState('')
  const [error, setError] = useState('')

  // 每次打开回填当前昵称，清空上次的错误。
  useEffect(() => {
    if (open) {
      setValue(currentNickname)
      setError('')
    }
  }, [open, currentNickname])

  const changeMut = useMutation({
    mutationFn: (nickname: string) => cloudApi.changeNickname(nickname),
    onSuccess: (profile) => {
      toast(t('cloud.nicknameEditSuccess'))
      onChanged(profile)
    },
    onError: (err) => setError(nicknameErrorText(t, err)),
  })

  function confirm() {
    const trimmed = value.trim()
    if (!isValidNickname(trimmed)) {
      setError(t('cloud.err.nicknameInvalid'))
      return
    }
    setError('')
    changeMut.mutate(trimmed)
  }

  return (
    <Dialog.Root
      open={open}
      onOpenChange={(o) => {
        if (!o) onClose()
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="wbackdrop show" />
        <Dialog.Content className="dialog sm show" aria-describedby={undefined}>
          <header className="dlg-head">
            <Dialog.Title asChild>
              <b>{t('cloud.nicknameEditTitle')}</b>
            </Dialog.Title>
            <Dialog.Close asChild>
              <button type="button" className="icon-btn sm" aria-label={t('common.close')}>
                <X size={16} />
              </button>
            </Dialog.Close>
          </header>
          <form
            onSubmit={(e) => {
              e.preventDefault()
              confirm()
            }}
          >
            <div className="dlg-body">
              <label className="field-label" htmlFor="nickname-edit-value">
                {t('cloud.nicknameEditLabel')}
              </label>
              <input
                id="nickname-edit-value"
                className="text-input"
                type="text"
                autoFocus
                maxLength={32}
                placeholder={t('cloud.nicknameEditPlaceholder')}
                value={value}
                disabled={changeMut.isPending}
                onChange={(e) => setValue(e.target.value)}
              />
              {error ? <p className="mt-2 text-[12px] text-danger">{error}</p> : null}
            </div>
            <footer className="dlg-foot">
              <Dialog.Close asChild>
                <button type="button" className="btn ghost">
                  {t('common.cancel')}
                </button>
              </Dialog.Close>
              <button type="submit" className="btn primary" disabled={changeMut.isPending || !value.trim()}>
                {changeMut.isPending ? t('common.loading') : t('common.confirm')}
              </button>
            </footer>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}

// ---------------------------------------------------------------------------
// 推介有奖：我的推荐码 + 收益总览 + 规则 + 返利记录（人工兑付，见需求文档）
// ---------------------------------------------------------------------------

/** 单页返利记录条数；「加载更多」在当前列表末尾追加，不做页码导航。 */
const REFERRAL_RECORDS_PAGE_SIZE = 20

const REFERRAL_STATUS_KEYS: Record<ReferralRecordStatus, I18nKey> = {
  pending: 'cloud.referral.status.pending',
  paid: 'cloud.referral.status.paid',
  revoked: 'cloud.referral.status.revoked',
}

/** 分（i64 最小货币单位）→ 元，两位小数，人民币符号前缀——web 侧唯一出现金额的场景。 */
function fmtYuan(minor: number): string {
  return `¥${(minor / 100).toFixed(2)}`
}

function ReferralStatusBadge({ status }: { status: ReferralRecordStatus }) {
  const { t } = useI18n()
  const cls =
    status === 'paid'
      ? 'border-success/40 bg-success/10 text-success'
      : status === 'revoked'
        ? 'border-danger/40 bg-danger/10 text-danger'
        : 'border-accent/40 bg-accent-weak text-accent'
  return <span className={cn('flex-shrink-0 rounded border px-1.5 py-0.5 font-mono text-[10px]', cls)}>{t(REFERRAL_STATUS_KEYS[status])}</span>
}

/** 推介有奖对话框：打开时拉取总览（GET /referral/summary）与推荐码列表（GET
 *  /referral/codes），每次打开都重新拉取。enabled=false 只展示未开放提示；
 *  rewardEnabled=false（内部员工，见需求文档 referral_reward_enabled）时永不产生
 *  返利台账行，这里对应隐藏全部金额与返利记录区块，只展示邀请人数 + 不参与返现
 *  提示——推荐码依旧可正常创建/展示/复制，买家优惠与归因统计不受影响。
 *  description 非空时优先展示 admin 后台配置的说明文案（保留换行），为空回退展示
 *  rules 自动生成的套餐列表。返利记录（GET /referral/records）只在 rewardEnabled
 *  时才有拉取的必要，支持按买家昵称/邮箱子串搜索（回车提交，搜索变更即从第 1 页
 *  重拉），「加载更多」在本地状态里追加而非用查询库分页缓存。 */
function ReferralDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { t } = useI18n()
  const qc = useQueryClient()
  const [records, setRecords] = useState<ReferralRecord[]>([])
  const [recordsTotal, setRecordsTotal] = useState(0)
  const [recordsPage, setRecordsPage] = useState(1)
  const [recordsLoading, setRecordsLoading] = useState(false)
  const [recordsError, setRecordsError] = useState(false)
  const [recordsSearchInput, setRecordsSearchInput] = useState('')
  const [recordsSearch, setRecordsSearch] = useState('')
  const [newCode, setNewCode] = useState('')
  const [createError, setCreateError] = useState('')

  const summaryQuery = useQuery({
    queryKey: ['cloud', 'referralSummary'],
    queryFn: () => cloudApi.referralSummary(),
    enabled: open,
    staleTime: 10_000,
  })
  const summary = summaryQuery.data
  const showRewards = !!summary?.enabled && !!summary?.rewardEnabled

  const codesQuery = useQuery({
    queryKey: ['cloud', 'referralCodes'],
    queryFn: () => cloudApi.referralCodes(),
    enabled: open && !!summary?.enabled,
    staleTime: 10_000,
  })
  const codes = codesQuery.data?.items ?? []

  const createMut = useMutation({
    mutationFn: (code: string) => cloudApi.createReferralCode(code || undefined),
    onSuccess: () => {
      setNewCode('')
      setCreateError('')
      void qc.invalidateQueries({ queryKey: ['cloud', 'referralCodes'] })
    },
    onError: (err) => setCreateError(referralCodeErrorText(t, err)),
  })

  const loadRecords = useCallback(async (page: number, search: string) => {
    setRecordsLoading(true)
    setRecordsError(false)
    try {
      const res = await cloudApi.referralRecords(page, REFERRAL_RECORDS_PAGE_SIZE, search)
      setRecords((prev) => (page === 1 ? res.items : [...prev, ...res.items]))
      setRecordsTotal(res.total)
      setRecordsPage(page)
    } catch {
      setRecordsError(true)
    } finally {
      setRecordsLoading(false)
    }
  }, [])

  // 每次打开重置分页态与搜索态；rewardEnabled=false 时台账永远为空，不值得发请求。
  useEffect(() => {
    if (open && showRewards) {
      setRecords([])
      setRecordsTotal(0)
      setRecordsSearchInput('')
      setRecordsSearch('')
      void loadRecords(1, '')
    }
  }, [open, showRewards, loadRecords])

  // 每次打开重置创建输入态，避免带着上次残留的错误提示或半输入值。
  useEffect(() => {
    if (open) {
      setNewCode('')
      setCreateError('')
    }
  }, [open])

  // 搜索提交：trim 后写入已生效搜索词并从第 1 页重新拉取，与「打开重置」共用同一份
  // 列表清空逻辑，避免翻页态残留串号。
  function submitRecordsSearch() {
    const q = recordsSearchInput.trim()
    setRecordsSearch(q)
    setRecords([])
    setRecordsTotal(0)
    void loadRecords(1, q)
  }

  const hasMoreRecords = records.length < recordsTotal

  return (
    <Dialog.Root
      open={open}
      onOpenChange={(o) => {
        if (!o) onClose()
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="wbackdrop show" />
        <Dialog.Content className="dialog show" style={{ width: 480 }} aria-describedby={undefined}>
          <header className="dlg-head">
            <Dialog.Title asChild>
              <b>{t('cloud.referral.dialogTitle')}</b>
            </Dialog.Title>
            <Dialog.Close asChild>
              <button type="button" className="icon-btn sm" aria-label={t('common.close')}>
                <X size={16} />
              </button>
            </Dialog.Close>
          </header>
          <div className="dlg-body thin-scroll" style={{ maxHeight: 480, overflowY: 'auto' }}>
            {summaryQuery.isLoading ? (
              <p className="p-4 text-[12px] text-text3">{t('common.loading')}</p>
            ) : summaryQuery.isError || !summary ? (
              <div className="flex items-center justify-between p-4">
                <p className="text-[12px] text-danger">{t('cloud.referral.loadFailed')}</p>
                <button type="button" className="btn ghost sm" onClick={() => void summaryQuery.refetch()}>
                  {t('common.retry')}
                </button>
              </div>
            ) : !summary.enabled ? (
              <p className="set-desc">{t('cloud.referral.disabledNotice')}</p>
            ) : (
              <>
                <label className="field-label" style={{ marginTop: 0 }}>
                  {t('cloud.referral.codesTitle')}
                </label>
                <div className="flex items-center gap-2">
                  <input
                    className="text-input flex-1 font-mono uppercase"
                    maxLength={16}
                    placeholder={t('cloud.referral.codeCreatePlaceholder')}
                    value={newCode}
                    disabled={createMut.isPending}
                    onChange={(e) => {
                      setNewCode(e.target.value.toUpperCase().replace(/[^A-Z0-9]/g, ''))
                      setCreateError('')
                    }}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter') createMut.mutate(newCode)
                    }}
                  />
                  <button
                    type="button"
                    className="btn ghost sm flex-shrink-0"
                    disabled={createMut.isPending}
                    onClick={() => createMut.mutate(newCode)}
                  >
                    {createMut.isPending ? t('common.loading') : t('cloud.referral.codeCreateBtn')}
                  </button>
                </div>
                {createError ? <p className="mt-1 text-[11.5px] text-danger">{createError}</p> : null}

                <div className="mt-2 flex flex-col gap-1.5">
                  {codesQuery.isLoading ? (
                    <p className="p-2 text-[12px] text-text3">{t('common.loading')}</p>
                  ) : codesQuery.isError ? (
                    <div className="flex items-center justify-between p-2">
                      <p className="text-[12px] text-danger">{t('cloud.referral.codesLoadFailed')}</p>
                      <button type="button" className="btn ghost sm" onClick={() => void codesQuery.refetch()}>
                        {t('common.retry')}
                      </button>
                    </div>
                  ) : codes.length === 0 ? (
                    <p className="p-2 text-[12px] text-text3">{t('cloud.referral.codesEmpty')}</p>
                  ) : (
                    codes.map((c) => <ReferralCodeRow key={c.id} code={c} />)
                  )}
                </div>

                {showRewards ? (
                  <div className="mt-3 grid grid-cols-3 gap-2">
                    <div className="rounded-lg bg-surface2 px-2 py-2.5 text-center">
                      <b className="block text-[15px] font-semibold text-text">{summary.invitedCount}</b>
                      <span className="text-[10.5px] text-text3">{t('cloud.referral.invitedCount')}</span>
                    </div>
                    <div className="rounded-lg bg-surface2 px-2 py-2.5 text-center">
                      <b className="block text-[15px] font-semibold text-text">{fmtYuan(summary.pendingRewardMinor)}</b>
                      <span className="text-[10.5px] text-text3">{t('cloud.referral.pendingReward')}</span>
                    </div>
                    <div className="rounded-lg bg-surface2 px-2 py-2.5 text-center">
                      <b className="block text-[15px] font-semibold text-text">{fmtYuan(summary.paidRewardMinor)}</b>
                      <span className="text-[10.5px] text-text3">{t('cloud.referral.paidReward')}</span>
                    </div>
                  </div>
                ) : (
                  <div className="mt-3">
                    <p className="text-[12.5px] text-text2">{t('cloud.referral.invitedCountLine', { n: summary.invitedCount })}</p>
                    <p className="mt-1 text-[11.5px] text-text3">{t('cloud.referral.rewardDisabledNotice')}</p>
                  </div>
                )}

                {summary.description.trim() ? (
                  <MarkdownLite text={summary.description} className="mt-4" />
                ) : summary.rules.length > 0 ? (
                  <>
                    <p className="mb-1 mt-4 text-[12px] font-semibold text-text2">{t('cloud.referral.rulesTitle')}</p>
                    <div className="flex flex-col gap-1.5">
                      {summary.rules.map((r) => (
                        <div key={r.planCode} className="flex items-center justify-between rounded-lg border border-line px-3 py-2 text-[12px]">
                          <span className="text-text2">{r.planName}</span>
                          <span className="text-text3">
                            {t('cloud.referral.ruleDiscount', { amount: fmtYuan(r.discountMinor) })}
                            {showRewards ? ` · ${t('cloud.referral.ruleReward', { percent: r.rewardPercent })}` : ''}
                          </span>
                        </div>
                      ))}
                    </div>
                  </>
                ) : null}

                {showRewards ? (
                  <p className="mt-3 text-[11.5px] text-text3">
                    {summary.contact ? t('cloud.referral.redeemNote', { contact: summary.contact }) : t('cloud.referral.redeemNoteNoContact')}
                  </p>
                ) : null}

                {showRewards ? (
                  <>
                    <p className="mb-1 mt-4 text-[12px] font-semibold text-text2">{t('cloud.referral.recordsTitle')}</p>
                    <div className="relative mb-2">
                      <Search size={13} className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-text3" />
                      <input
                        className="text-input"
                        style={{ paddingLeft: 30 }}
                        placeholder={t('cloud.referral.searchPlaceholder')}
                        value={recordsSearchInput}
                        onChange={(e) => setRecordsSearchInput(e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === 'Enter') submitRecordsSearch()
                        }}
                      />
                    </div>
                    {recordsLoading && records.length === 0 ? (
                      <p className="p-3 text-[12px] text-text3">{t('common.loading')}</p>
                    ) : recordsError && records.length === 0 ? (
                      <div className="flex items-center justify-between p-3">
                        <p className="text-[12px] text-danger">{t('cloud.referral.recordsLoadFailed')}</p>
                        <button type="button" className="btn ghost sm" onClick={() => void loadRecords(1, recordsSearch)}>
                          {t('common.retry')}
                        </button>
                      </div>
                    ) : records.length === 0 ? (
                      <p className="p-3 text-[12px] text-text3">{recordsSearch ? t('cloud.referral.searchEmpty') : t('cloud.referral.recordsEmpty')}</p>
                    ) : (
                      <div className="flex flex-col gap-1">
                        {records.map((r) => (
                          <div key={r.id} className="flex items-center gap-2 border-b border-line py-2 text-[12px] last:border-b-0">
                            <div className="min-w-0 flex-1">
                              <p className="truncate text-text2">{r.buyerLabel}</p>
                              <p className="text-[10.5px] text-text3">
                                {r.referralCode ? `${r.referralCode} · ` : ''}
                                {fmtRelativeTime(r.createdAt)}
                              </p>
                            </div>
                            <span className="flex-shrink-0 font-mono text-[12px] text-text2">{fmtYuan(r.rewardMinor)}</span>
                            <ReferralStatusBadge status={r.status} />
                          </div>
                        ))}
                      </div>
                    )}
                    {hasMoreRecords ? (
                      <button
                        type="button"
                        className="btn ghost sm mt-2 w-full"
                        disabled={recordsLoading}
                        onClick={() => void loadRecords(recordsPage + 1, recordsSearch)}
                      >
                        {recordsLoading ? t('common.loading') : t('cloud.referral.loadMore')}
                      </button>
                    ) : null}
                  </>
                ) : null}
              </>
            )}
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}

/** 单条推荐码行：展示码/有效订单数/累计返利/创建时间，支持复制与删除（确认后硬
 *  删除，历史订单归因不受影响）。 */
function ReferralCodeRow({ code }: { code: ReferralCode }) {
  const { t } = useI18n()
  const qc = useQueryClient()

  const deleteMut = useMutation({
    mutationFn: () => cloudApi.deleteReferralCode(code.id),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ['cloud', 'referralCodes'] }),
  })

  async function handleDelete() {
    const ok = await confirmDialog({
      title: t('cloud.referral.codeDeleteTitle'),
      message: t('cloud.referral.codeDeleteDesc', { code: code.code }),
      danger: true,
    })
    if (ok) deleteMut.mutate()
  }

  return (
    <div className="token-box">
      <div className="min-w-0 flex-1">
        <span className="font-mono text-[13px] font-semibold tracking-[1px] text-text">{code.code}</span>
        <p className="mt-0.5 text-[10.5px] text-text3">
          {t('cloud.referral.codePaidOrders', { n: code.paidOrderCount })} · {fmtYuan(code.rewardMinor)} · {fmtRelativeTime(code.createdAt)}
        </p>
      </div>
      <CopyButton value={code.code} />
      <button
        type="button"
        className="text-text3 hover:text-danger"
        title={t('cloud.referral.codeDeleteTitle')}
        aria-label={t('cloud.referral.codeDeleteTitle')}
        disabled={deleteMut.isPending}
        onClick={() => void handleDelete()}
      >
        <Trash2 size={13} />
      </button>
    </div>
  )
}

/** 内联展示的设备行数上限；超出后「查看全部」展开为限高滚动容器 + 过滤输入框，
 *  避免几十台设备把设置页撑爆（见需求：可扩展设备列表）。 */
const DEVICE_INLINE_LIMIT = 5

function DeviceListSection() {
  const { t } = useI18n()
  const currentId = cloudDeviceId()
  const { data, isLoading, isError, refetch } = useQuery({
    queryKey: DEVICES_QUERY_KEY,
    queryFn: () => cloudApi.devices().then((r) => r.devices),
    staleTime: 10_000,
  })
  const [showAll, setShowAll] = useState(false)
  const [filter, setFilter] = useState('')
  const [openId, setOpenId] = useState<string | null>(null)

  // 服务端已按 lastSeenAt 降序返回，这里只把当前设备置顶（稳定排序，不打乱其余顺序）。
  const sorted = useMemo(() => {
    if (!data) return []
    return [...data].sort((a, b) => (a.deviceId === currentId ? -1 : b.deviceId === currentId ? 1 : 0))
  }, [data, currentId])

  const filtered = useMemo(() => {
    const q = filter.trim().toLowerCase()
    if (!q) return sorted
    return sorted.filter((d) => (d.name || '').toLowerCase().includes(q) || (d.platform || '').toLowerCase().includes(q))
  }, [sorted, filter])

  const hasMore = sorted.length > DEVICE_INLINE_LIMIT
  const visible = showAll ? filtered : sorted.slice(0, DEVICE_INLINE_LIMIT)

  function toggleShowAll() {
    setShowAll((v) => !v)
    setFilter('')
  }

  return (
    <>
      <p className="mb-1 mt-6 text-[12.5px] font-semibold text-text2">{t('cloud.devicesTitle')}</p>
      <p className="set-desc" style={{ marginBottom: 10 }}>
        {t('cloud.devicesDesc')}
      </p>
      <div className="set-group">
        {isLoading ? (
          <p className="p-4 text-[12px] text-text3">{t('common.loading')}</p>
        ) : isError ? (
          <div className="flex items-center justify-between p-4">
            <p className="text-[12px] text-danger">{t('cloud.devicesLoadFailed')}</p>
            <button type="button" className="btn ghost sm" onClick={() => void refetch()}>
              {t('common.retry')}
            </button>
          </div>
        ) : !data || data.length === 0 ? (
          <p className="p-4 text-[12px] text-text3">{t('cloud.devicesEmpty')}</p>
        ) : (
          <>
            {showAll ? (
              <div className="device-filter">
                <div className="relative">
                  <Search size={13} className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-text3" />
                  <input
                    className="text-input"
                    style={{ paddingLeft: 30 }}
                    placeholder={t('cloud.deviceFilterPlaceholder')}
                    value={filter}
                    onChange={(e) => setFilter(e.target.value)}
                    autoFocus
                  />
                </div>
              </div>
            ) : null}
            <div className={cn(showAll && 'device-scroll')}>
              {visible.length === 0 ? (
                <p className="device-list-empty">{t('cloud.devicesFilterEmpty')}</p>
              ) : (
                visible.map((d) => (
                  <DeviceItem
                    key={d.id}
                    device={d}
                    label={deviceLabel(d, sorted)}
                    isCurrent={d.deviceId === currentId}
                    open={openId === d.id}
                    onToggle={() => setOpenId((cur) => (cur === d.id ? null : d.id))}
                  />
                ))
              )}
            </div>
            {hasMore ? (
              <button type="button" className="device-list-toggle" onClick={toggleShowAll}>
                {showAll ? t('cloud.devicesCollapse') : t('cloud.devicesShowAll', { n: sorted.length })}
              </button>
            ) : null}
          </>
        )}
      </div>
    </>
  )
}

/** 设备行：主体点击展开/收起详情（最近登录 IP、首次信任时间、最近活跃、平台、
 *  App 版本、设备 ID），对标 Telegram/Google 设备管理的信息量，不做额外采集。 */
function DeviceItem({
  device,
  label,
  isCurrent,
  open,
  onToggle,
}: {
  device: CloudDevice
  /** 展示名：重名设备已带 deviceId 短码后缀，重命名输入仍编辑原始 device.name。 */
  label: string
  isCurrent: boolean
  open: boolean
  onToggle: () => void
}) {
  const { t } = useI18n()
  const qc = useQueryClient()
  const [editing, setEditing] = useState(false)
  const [name, setName] = useState(device.name)
  const [renameError, setRenameError] = useState('')

  const renameMut = useMutation({
    mutationFn: (n: string) => cloudApi.renameDevice(device.id, n),
    onSuccess: () => {
      setEditing(false)
      void qc.invalidateQueries({ queryKey: DEVICES_QUERY_KEY })
    },
    onError: (err) => setRenameError(cloudErrorText(t, err)),
  })

  const deleteMut = useMutation({
    mutationFn: () => cloudApi.deleteDevice(device.id),
    onSuccess: () => {
      if (isCurrent) signOutCloud()
      void qc.invalidateQueries({ queryKey: DEVICES_QUERY_KEY })
    },
  })

  function commitRename() {
    const trimmed = name.trim()
    if (trimmed.length < 1 || trimmed.length > 64) {
      setRenameError(t('cloud.deviceRenameInvalid'))
      return
    }
    if (trimmed === device.name) {
      setEditing(false)
      return
    }
    renameMut.mutate(trimmed)
  }

  function cancelRename() {
    setEditing(false)
    setName(device.name)
    setRenameError('')
  }

  async function handleDelete() {
    const ok = await confirmDialog({
      title: t('cloud.deviceDeleteTitle'),
      message: isCurrent ? `${t('cloud.deviceDeleteDesc')} ${t('cloud.deviceDeleteCurrentWarning')}` : t('cloud.deviceDeleteDesc'),
      danger: true,
    })
    if (ok) deleteMut.mutate()
  }

  const PlatformIcon = device.platform === 'android' || device.platform === 'ios' ? Smartphone : device.platform === 'windows' || device.platform === 'macos' || device.platform === 'linux' ? Monitor : Cloud

  return (
    <div className="device-item">
      <div className="set-row">
        {editing ? (
          <div className="grid h-8 w-8 flex-shrink-0 place-items-center rounded-lg bg-surface2 text-text2">
            <PlatformIcon size={15} />
          </div>
        ) : (
          <button type="button" className="device-row-main" onClick={onToggle}>
            <ChevronRight size={13} className={cn('device-chevron', open && 'open')} />
            <div className="grid h-8 w-8 flex-shrink-0 place-items-center rounded-lg bg-surface2 text-text2">
              <PlatformIcon size={15} />
            </div>
            <div className="min-w-0 flex-1 text-left">
              <div className="flex items-center gap-2">
                <i className={cn('queue-dot', device.isOnline && 'on')} title={device.isOnline ? t('link.online') : t('link.offline')} />
                <b className="truncate text-[13px] font-medium">{label}</b>
                {isCurrent ? (
                  <span className="flex-shrink-0 rounded-full bg-accent-weak px-1.5 py-0.5 text-[9.5px] font-semibold text-accent">
                    {t('cloud.deviceCurrent')}
                  </span>
                ) : null}
              </div>
              <p className="text-[11.5px] text-text3">{fmtRelativeTime(device.lastSeenAt)}</p>
            </div>
          </button>
        )}
        {editing ? (
          <div className="flex flex-1 items-center gap-1.5">
            <input
              className="text-input short"
              autoFocus
              maxLength={64}
              value={name}
              disabled={renameMut.isPending}
              onChange={(e) => {
                setName(e.target.value)
                setRenameError('')
              }}
              onKeyDown={(e) => {
                if (e.key === 'Enter') commitRename()
                if (e.key === 'Escape') cancelRename()
              }}
            />
            <button type="button" className="icon-btn sm accent" disabled={renameMut.isPending} onClick={commitRename}>
              <Check size={14} />
            </button>
            <button type="button" className="icon-btn sm" disabled={renameMut.isPending} onClick={cancelRename}>
              <X size={14} />
            </button>
          </div>
        ) : (
          <>
            <button type="button" className="icon-btn sm" title={t('cloud.deviceRename')} aria-label={t('cloud.deviceRename')} onClick={() => setEditing(true)}>
              <Pencil size={14} />
            </button>
            <button
              type="button"
              className="icon-btn sm text-text3 hover:text-danger"
              title={t('cloud.deviceDeleteTitle')}
              aria-label={t('cloud.deviceDeleteTitle')}
              disabled={deleteMut.isPending}
              onClick={() => void handleDelete()}
            >
              <Trash2 size={14} />
            </button>
          </>
        )}
        {renameError ? <p className="w-full px-0 text-[11.5px] text-danger">{renameError}</p> : null}
      </div>
      {open && !editing ? (
        <div className="device-detail">
          <div className="d-field">
            <span>{t('cloud.deviceDetailPlatform')}</span>
            <p>{platformLabel(t, device.platform)}</p>
          </div>
          <div className="d-field">
            <span>{t('cloud.deviceDetailAppVersion')}</span>
            <p>{device.appVersion || '—'}</p>
          </div>
          <div className="d-field">
            <span>{t('cloud.deviceDetailLastIp')}</span>
            <p>{device.lastIp || '—'}</p>
          </div>
          <div className="d-field">
            <span>{t('cloud.deviceDetailCreatedAt')}</span>
            <p>{fmtIsoTime(device.createdAt)}</p>
          </div>
          <div className="d-field">
            <span>{t('cloud.deviceDetailLastSeenAt')}</span>
            <p>{fmtIsoTime(device.lastSeenAt)}</p>
          </div>
          <div className="d-field mono">
            <span>{t('cloud.deviceDetailId')}</span>
            <p>{device.deviceId}</p>
          </div>
        </div>
      ) : null}
    </div>
  )
}

// ---------------------------------------------------------------------------
// 云服务器地址
// ---------------------------------------------------------------------------

/** 云服务器地址编辑：仅开发构建渲染（与桌面端 kDebugMode 门控对称），
 *  生产构建锁定构建期注入的官方地址，保证请求打到正确的生产环境。 */
function CloudServerAddressGroup() {
  if (!CLOUD_BASE_URL_EDITABLE) return null
  return <CloudServerAddressEditor />
}

function CloudServerAddressEditor() {
  const { t } = useI18n()
  const [value, setValue] = useState(getCloudBaseUrl())
  const [error, setError] = useState('')
  const [custom, setCustom] = useState(isCloudBaseUrlCustom())

  function commit(next: string) {
    const trimmed = next.trim()
    if (trimmed === '') {
      resetCloudBaseUrl()
      setValue(getCloudBaseUrl())
      setCustom(false)
      setError('')
      return
    }
    if (!/^https?:\/\/.+/i.test(trimmed)) {
      setError(t('cloud.serverAddrInvalid'))
      setValue(trimmed)
      return
    }
    setCloudBaseUrl(trimmed)
    setValue(trimmed)
    setCustom(isCloudBaseUrlCustom())
    setError('')
  }

  function reset() {
    resetCloudBaseUrl()
    setValue(getCloudBaseUrl())
    setCustom(false)
    setError('')
  }

  return (
    <>
      <p className="mb-1 mt-6 text-[12.5px] font-semibold text-text2">{t('cloud.serverAddrTitle')}</p>
      <p className="set-desc" style={{ marginBottom: 10 }}>
        {t('cloud.serverAddrDesc')}
      </p>
      <div className="set-group">
        <SetRow title={t('cloud.serverAddr')}>
          <TextInput value={value} onCommit={commit} />
          {custom ? (
            <button type="button" className="btn ghost sm" onClick={reset}>
              {t('cloud.serverAddrReset')}
            </button>
          ) : null}
        </SetRow>
      </div>
      {error ? <p className="mt-2 text-[12px] text-danger">{error}</p> : null}
    </>
  )
}
