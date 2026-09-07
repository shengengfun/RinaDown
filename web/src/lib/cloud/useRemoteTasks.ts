// 跨设备任务的实时视图（查看端）—— 登录后常驻 SSE 长连接 `/api/v1/tasks/events`，
// 增量应用 task.dispatch/task.status（RemoteTaskDto 平铺）/task.progress（批量 items[]）/
// presence/session.revoked 事件；断线指数退避重连，重连即 `GET /tasks/remote` 拉一次
// 全量补齐（持久态 + 内存进度快照，见 mdc §1.5）。与 lib/ws.ts 的本地任务 WebSocket
// 完全独立（云端跨设备视图 vs 本地下载引擎）；复用同一套「轻量外部 store」（Store/useStore）。
//
// 生命周期：连接由 [attachRemoteTasks]（应用入口调一次，同 attachCdnServices）按登录态
// 驱动，而非组件引用计数——面板必须在任意路由下都保持在线，否则本机在其他设备的设备
// 列表里会时隐时现，下发到本机的任务也收不到。useRemoteTasks() 因此退化为纯 store 订阅。
//
// 本模块只做「看」：把事件落进快照给 UI。目标为本机的下发/命令另由 remoteTaskExecutor.ts
// 落地成真实本地下载并回报进度——两个模块共用这一条连接。
//
// 心跳：只要 SSE 连着就每 30s 调一次 `POST /tasks/presence`，续服务端的 presence 租约
// ——长连接本身健康但久不产生业务请求（没有跨设备任务在跑）时，没有这条心跳的话服务端
// 后台 sweeper 会把本机误判离线，导致发起端对本机的 pause/resume 落空于 409。

import type { QueryClient } from '@tanstack/react-query'
import { Store, useStore } from '../ws'
import { cloudApi, getCloudBaseUrl } from './client'
import {
  acceptPendingRemoteTasks,
  acceptRemoteTask,
  applyRemoteCommand,
  clearRemoteTaskBindings,
  resetAcceptingState,
  startRemoteTaskExecutor,
  stopRemoteTaskExecutor,
} from './remoteTaskExecutor'
import { cloudDeviceId, cloudSessionStore, getCloudAccessToken, onCloudSignOut, signOutCloud } from './session'
import type { RemoteTask } from './types'

interface RemoteProgressItem {
  taskId: string
  downloadedBytes: number
  speed: number
  progress: number
}

interface RemoteTasksState {
  tasks: Map<string, RemoteTask>
  onlineDeviceIds: Set<string>
}

const remoteTasksStore = new Store<RemoteTasksState>({ tasks: new Map(), onlineDeviceIds: new Set() })

const DEVICES_QUERY_KEY = ['cloud', 'devices']
/** presence 抖动合并窗口，对齐桌面 _kPresenceDebounce。 */
const PRESENCE_DEBOUNCE_MS = 2_000
/** 执行端心跳周期：与 C2 契约的服务端 presence 租约（90s TTL / 30s sweep）对齐，
 *  留出充分冗余——单次心跳丢失不会立即导致误判离线。 */
const PRESENCE_HEARTBEAT_MS = 30_000

let source: EventSource | null = null
let reconnectTimer: ReturnType<typeof setTimeout> | null = null
let presenceTimer: ReturnType<typeof setTimeout> | null = null
let heartbeatTimer: ReturnType<typeof setInterval> | null = null
let attempts = 0
let attached = false
let unsubscribeSession: (() => void) | null = null
let unsubscribeSignOut: (() => void) | null = null
let queryClientRef: QueryClient | null = null

/** task.dispatch / task.status 事件：payload 是平铺的 RemoteTaskDto，直接整条替换。 */
function applyTaskEvent(data: Record<string, unknown>) {
  const task = data as unknown as RemoteTask
  if (!task.id) return
  remoteTasksStore.set((prev) => {
    const tasks = new Map(prev.tasks)
    tasks.set(task.id, task)
    return { ...prev, tasks }
  })
}

/** task.progress 事件：`{items:[{taskId,downloadedBytes,speed,progress}]}` 批量，仅更新已知任务。 */
function applyProgressEvent(items: RemoteProgressItem[]) {
  if (!items?.length) return
  remoteTasksStore.set((prev) => {
    const tasks = new Map(prev.tasks)
    for (const item of items) {
      const cur = tasks.get(item.taskId)
      if (!cur) continue
      tasks.set(item.taskId, { ...cur, downloadedBytes: item.downloadedBytes, speed: item.speed, progress: item.progress })
    }
    return { ...prev, tasks }
  })
}

/** presence 事件：`{deviceId,online}`。 */
function applyPresenceEvent(deviceId: string, online: boolean) {
  if (!deviceId) return
  remoteTasksStore.set((prev) => {
    const onlineDeviceIds = new Set(prev.onlineDeviceIds)
    if (online) onlineDeviceIds.add(deviceId)
    else onlineDeviceIds.delete(deviceId)
    return { ...prev, onlineDeviceIds }
  })
}

/** presence 抖动：服务端一次上下线可能连发多帧（多标签页/重连），合并成一次设备列表
 *  失效，避免设置页被高频重拉。 */
function schedulePresenceRefresh() {
  presenceTimer ??= setTimeout(() => {
    presenceTimer = null
    void queryClientRef?.invalidateQueries({ queryKey: DEVICES_QUERY_KEY })
  }, PRESENCE_DEBOUNCE_MS)
}

/** 全量拉取（首次连接 + 每次重连）：与内存快照合并（覆盖同 id 条目，不清空未提及的），
 *  并把离线期间积压、目标为本机的 pending 记录交给执行端补单（幂等）。 */
async function seed() {
  try {
    const { tasks } = await cloudApi.remoteTasks()
    remoteTasksStore.set((prev) => {
      const next = new Map(prev.tasks)
      for (const task of tasks) next.set(task.id, task)
      return { ...prev, tasks: next }
    })
    acceptPendingRemoteTasks(tasks)
  } catch (err) {
    console.warn('[remoteTasks] seed failed', err)
  }
}

/** 本地引擎登录成功后补一次跨设备任务全量：面板停留在 /login 路由时，如果同时已经
 *  登录着云账号，SSE 连接始终健康（它只看云账号登录态，不关心本地引擎登录态）—— 期间
 *  收到的 task.dispatch 会被 acceptRemoteTask 里的 `!isAuthenticated()` 直接丢弃，且
 *  SSE 健康意味着不会重连，没有任何路径重跑 seed()。本地登录成功是唯一能感知到这个
 *  转变的时刻，必须在那里显式补一次。未建连（没有云账号登录）时静默 no-op。 */
export function resyncRemoteTasks() {
  if (!source) return
  void seed()
}

/** 重连前提：已 attach 且仍处于登录态（不再看组件引用计数）。 */
function shouldStayConnected(): boolean {
  return attached && cloudSessionStore.get().status === 'authenticated'
}

function scheduleReconnect() {
  if (reconnectTimer || !shouldStayConnected()) return
  attempts += 1
  const delay = Math.min(30_000, 1_000 * 2 ** Math.min(attempts, 5))
  reconnectTimer = setTimeout(() => {
    reconnectTimer = null
    if (shouldStayConnected()) connect()
  }, delay)
}

function startHeartbeat() {
  heartbeatTimer ??= setInterval(() => {
    void cloudApi.pingPresence().catch((e) => console.warn('[remoteTasks] pingPresence failed', e))
  }, PRESENCE_HEARTBEAT_MS)
}

function stopHeartbeat() {
  if (heartbeatTimer) {
    clearInterval(heartbeatTimer)
    heartbeatTimer = null
  }
}

function connect() {
  if (source) return
  const token = getCloudAccessToken()
  if (!token) return
  // 断线期间可能有 accept 请求悬挂在执行端的 accepting 集合里却永远等不到 finally
  // （标签页休眠/网络切换导致 fetch 悬挂却不报错）；每次（重）连接都视为新的一轮。
  resetAcceptingState()
  void seed()
  const url = `${getCloudBaseUrl()}/api/v1/tasks/events?access_token=${encodeURIComponent(token)}&deviceId=${encodeURIComponent(cloudDeviceId())}`
  const es = new EventSource(url)
  source = es
  es.onopen = () => {
    attempts = 0
    // 本机的在线状态由这条连接本身决定（服务端 PresenceRegistry 引用计数），
    // 建连瞬间设备列表里的 isOnline 已过期——立刻失效，别等下一次 presence 事件。
    void queryClientRef?.invalidateQueries({ queryKey: DEVICES_QUERY_KEY })
    startHeartbeat()
  }
  es.onmessage = (ev) => {
    let data: Record<string, unknown>
    try {
      data = JSON.parse(ev.data)
    } catch {
      return
    }
    switch (data.type) {
      case 'task.dispatch':
        applyTaskEvent(data)
        // 查看端快照之外，目标为本机的下发要真正落地执行（执行端自行判目标/幂等）。
        acceptRemoteTask(data as unknown as RemoteTask)
        break
      case 'task.status':
        applyTaskEvent(data)
        break
      case 'task.progress':
        applyProgressEvent((data.items as RemoteProgressItem[] | undefined) ?? [])
        break
      case 'task.command':
        applyRemoteCommand(data)
        break
      case 'presence':
        applyPresenceEvent(data.deviceId as string, !!data.online)
        schedulePresenceRefresh()
        break
      case 'session.revoked': {
        // {deviceId: string|null}：null（服务端未知具体设备）或恰好命中本机才算
        // "本机被登出"——多设备场景下别的设备被踢不该连带把本机也登出。
        const deviceId = (data.deviceId ?? null) as string | null
        if (deviceId === null || deviceId === cloudDeviceId()) signOutCloud()
        break
      }
      default:
        break
    }
  }
  es.onerror = () => {
    es.close()
    if (source === es) source = null
    stopHeartbeat()
    scheduleReconnect()
  }
}

function disconnect() {
  if (reconnectTimer) {
    clearTimeout(reconnectTimer)
    reconnectTimer = null
  }
  if (presenceTimer) {
    clearTimeout(presenceTimer)
    presenceTimer = null
  }
  stopHeartbeat()
  source?.close()
  source = null
  attempts = 0
  remoteTasksStore.set({ tasks: new Map(), onlineDeviceIds: new Set() })
}

/** 应用入口调用一次：按登录态起停 SSE 与执行端。未登录时静默待命，登录瞬间自动建连；
 *  显式登出（signOutCloud）才清执行端绑定表——被动的会话失效（401/403 刷新失败等）
 *  不广播这个事件，绑定表原样保留，见 session.ts 顶部注释。 */
export function attachRemoteTasks(queryClient: QueryClient) {
  if (attached) return
  attached = true
  queryClientRef = queryClient
  unsubscribeSession = cloudSessionStore.subscribe(() => {
    if (cloudSessionStore.get().status === 'authenticated') start(queryClient)
    else stop()
  })
  unsubscribeSignOut = onCloudSignOut(() => clearRemoteTaskBindings())
  if (cloudSessionStore.get().status === 'authenticated') start(queryClient)
}

/** 拆除接线（登录态监听 + 登出监听 + 连接 + 执行端）。绑定表保留：detach 不等于登出。 */
export function detachRemoteTasks() {
  if (!attached) return
  attached = false
  unsubscribeSession?.()
  unsubscribeSession = null
  unsubscribeSignOut?.()
  unsubscribeSignOut = null
  stopRemoteTaskExecutor()
  disconnect()
  queryClientRef = null
}

function start(queryClient: QueryClient) {
  startRemoteTaskExecutor(queryClient)
  connect()
}

/** 被动路径（断连/会话失效）只停连接与采样，绝不清绑定表——清表只在 signOutCloud
 *  广播的显式登出监听器（见 attachRemoteTasks）里做，两者语义不能合并。 */
function stop() {
  stopRemoteTaskExecutor()
  disconnect()
}

/** 跨设备任务实时视图的纯订阅：连接由 [attachRemoteTasks] 常驻维护，未登录时为空集合。 */
export function useRemoteTasks(): { remoteTasks: RemoteTask[]; onlineDeviceIds: Set<string> } {
  const state = useStore(remoteTasksStore)
  return { remoteTasks: Array.from(state.tasks.values()), onlineDeviceIds: state.onlineDeviceIds }
}
