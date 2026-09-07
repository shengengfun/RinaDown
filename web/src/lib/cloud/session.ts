// 云账户会话 —— accessToken/refreshToken/用户快照 + 设备身份，localStorage 持久化。
// 与本地下载器登录态（lib/auth.ts）完全独立：云账户是面板的可选增值功能，面板本身
// 作为一台设备接入 FluxCloud（deviceId 常驻本地，devicePlatform 固定 "web"），
// 与宿主 App 的账户状态无关。
//
// 订阅：cloudSessionStore 是轻量外部 store（复用 lib/ws.ts 的 Store），UI 用
// useCloudSession() 订阅登录态变化；网络层（client.ts）在令牌过期/登出时直接
// 调用本文件的 clearCloudSession() 等写操作。
//
// clearCloudSession() vs signOutCloud()：前者只清会话本身，用于「会话失效」的被动
// 路径（401/403 刷新失败）——这类场景不代表用户主动离开，执行端绑定表等派生状态
// 必须原样保留，等网络恢复/重新登录后继续用；后者是「用户显式登出/删除当前设备」
// 的主动路径，先广播 onCloudSignOut 监听者（例如清空执行端绑定表）再清会话。

import { Store, useStore } from '../ws'
import type { AuthResponse, CloudUser } from './types'

const ACCESS_TOKEN_KEY = 'fluxdown.cloud.accessToken'
const REFRESH_TOKEN_KEY = 'fluxdown.cloud.refreshToken'
const USER_KEY = 'fluxdown.cloud.user'
const DEVICE_ID_KEY = 'fluxdown.cloud.deviceId'

export interface CloudSessionState {
  status: 'authenticated' | 'unauthenticated'
  user: CloudUser | null
}

function restore(): CloudSessionState {
  const at = localStorage.getItem(ACCESS_TOKEN_KEY)
  const rt = localStorage.getItem(REFRESH_TOKEN_KEY)
  const userRaw = localStorage.getItem(USER_KEY)
  if (!at || !rt || !userRaw) return { status: 'unauthenticated', user: null }
  try {
    return { status: 'authenticated', user: JSON.parse(userRaw) as CloudUser }
  } catch {
    return { status: 'unauthenticated', user: null }
  }
}

export const cloudSessionStore = new Store<CloudSessionState>(restore())

/** 订阅云账户登录态；已登录时 user 非空。 */
export function useCloudSession(): CloudSessionState {
  return useStore(cloudSessionStore)
}

export function getCloudAccessToken(): string {
  return localStorage.getItem(ACCESS_TOKEN_KEY) ?? ''
}

export function getCloudRefreshToken(): string {
  return localStorage.getItem(REFRESH_TOKEN_KEY) ?? ''
}

export function isCloudLoggedIn(): boolean {
  return getCloudAccessToken() !== ''
}

/** 登录/注册/验证码验证/刷新 成功后落盘会话（令牌 + 用户快照）并通知订阅者。 */
export function applyCloudSession(auth: AuthResponse) {
  localStorage.setItem(ACCESS_TOKEN_KEY, auth.accessToken)
  localStorage.setItem(REFRESH_TOKEN_KEY, auth.refreshToken)
  localStorage.setItem(USER_KEY, JSON.stringify(auth.user))
  cloudSessionStore.set({ status: 'authenticated', user: auth.user })
}

/** 部分字段更新当前用户快照（如 Origin ID 自助修改成功后），不涉及令牌，仅同步本地
 *  存储 + 通知订阅者。未登录态下为空操作。 */
export function updateCloudUser(patch: Partial<CloudUser>) {
  const cur = cloudSessionStore.get()
  if (cur.status !== 'authenticated' || !cur.user) return
  const next: CloudUser = { ...cur.user, ...patch }
  localStorage.setItem(USER_KEY, JSON.stringify(next))
  cloudSessionStore.set({ status: 'authenticated', user: next })
}

/** 清空云账户会话（会话失效 / 令牌刷新失败等被动路径）。绝不能在这里连带清派生状态
 *  ——网络抖动触发的一次 401/403 不该把执行端正在维护的跨设备任务绑定表也清空，
 *  否则设备重连后所有正在执行的下发任务都会被误判为"账号变了"而失联。 */
export function clearCloudSession() {
  localStorage.removeItem(ACCESS_TOKEN_KEY)
  localStorage.removeItem(REFRESH_TOKEN_KEY)
  localStorage.removeItem(USER_KEY)
  cloudSessionStore.set({ status: 'unauthenticated', user: null })
}

const signOutListeners = new Set<() => void>()

/** 用户显式登出 / 删除当前设备：先广播给所有监听者，再清会话——广播必须先于清会话，
 *  否则监听者（如执行端）读到的已经是"未登录"快照，没法区分这是主动登出还是普通
 *  会话失效，也就没法只在这里才清绑定表。 */
export function signOutCloud() {
  for (const cb of signOutListeners) cb()
  clearCloudSession()
}

/** 订阅"用户显式登出"事件（不含被动会话失效），返回取消订阅函数。 */
export function onCloudSignOut(cb: () => void): () => void {
  signOutListeners.add(cb)
  return () => signOutListeners.delete(cb)
}

/** 当前云账号 id，未登录返回空串——供执行端绑定表按账号分区存储，换号后自然落到
 *  不同的 localStorage key，不会拿旧账号的绑定去新账号环境里误报 failed。 */
export function cloudUserId(): string {
  return cloudSessionStore.get().user?.id ?? ''
}

// ---------------------------------------------------------------------------
// 设备身份 —— 持久 deviceId + devicePlatform 常量 + UA 探测默认设备名。
// ---------------------------------------------------------------------------

/** 面板本身固定作为一台 web 设备登录 FluxCloud，与宿主 App 账户状态无关。 */
export const CLOUD_DEVICE_PLATFORM = 'web'

/** RFC 4122 UUID v4 via crypto.getRandomValues() —— 不同于 crypto.randomUUID()，
 *  getRandomValues() 无 Secure Context 限制：NAS/Docker 面板经明文 HTTP（非
 *  localhost）访问时 randomUUID 整个缺失，直接调用会抛 TypeError，此处规避。 */
function randomUuidV4(): string {
  const bytes = new Uint8Array(16)
  crypto.getRandomValues(bytes)
  bytes[6] = (bytes[6] & 0x0f) | 0x40 // version 4
  bytes[8] = (bytes[8] & 0x3f) | 0x80 // variant 10
  const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('')
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`
}

/** 客户端持久设备标识（UUID v4），首次调用生成并落盘，此后永久不变 —— 服务端
 *  devices 表识别"同一设备"的唯一依据。 */
export function cloudDeviceId(): string {
  const existing = localStorage.getItem(DEVICE_ID_KEY)
  if (existing) return existing
  const id = randomUuidV4()
  localStorage.setItem(DEVICE_ID_KEY, id)
  return id
}

/** 默认设备名探测：UA 解析浏览器 + 操作系统，如 "Chrome · Windows"；
 *  解析失败返回空串，交由服务端按 devicePlatform 兜底（见契约）。 */
export function cloudDefaultDeviceName(): string {
  const ua = navigator.userAgent
  const browser = detectBrowser(ua)
  const os = detectOs(ua)
  if (browser && os) return `${browser} · ${os}`
  return browser || os || ''
}

function detectBrowser(ua: string): string {
  if (/Edg\//.test(ua)) return 'Edge'
  if (/OPR\//.test(ua) || /Opera/.test(ua)) return 'Opera'
  if (/Firefox\//.test(ua)) return 'Firefox'
  if (/Chrome\//.test(ua) && !/Chromium/.test(ua)) return 'Chrome'
  if (/Safari\//.test(ua) && /Version\//.test(ua)) return 'Safari'
  return ''
}

function detectOs(ua: string): string {
  if (/Windows/.test(ua)) return 'Windows'
  if (/Mac OS X/.test(ua)) return 'macOS'
  if (/Android/.test(ua)) return 'Android'
  if (/iPhone|iPad|iPod/.test(ua)) return 'iOS'
  if (/Linux/.test(ua)) return 'Linux'
  return ''
}
