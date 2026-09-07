// FluxCloud 跨设备任务 —— Web 面板的执行端。与 useRemoteTasks.ts 的查看端共用同一条
// `/api/v1/tasks/events` SSE，但职责完全分离（一条连接，两个模块）：
//
//   选主：同一浏览器可能开着多个面板标签页，每个都会收到同一条 task.dispatch/
//         task.command。只有 executorLease.ts 选出的唯一 leader 才跑接单与上报，
//         其余标签页退化为纯查看端，避免同一个 cloudTaskId 被建两次本地任务。
//   接单：SSE 收到 task.dispatch（目标为本机）或每次全量 `GET /tasks/remote` 后发现
//         离线期间积压的 pending 记录 → 经宿主引擎 REST 建真实本地任务，回
//         reportTaskStatus(accepted)。宿主 createTask 同步返回新任务 id，故绑定是
//         确定性的，不需要桌面端那套按 url FIFO 回找的近似匹配。
//   上报：1s 定时采样「由下发产生的本地任务」（React Query ['tasks'] 快照叠加 ws
//         live 帧）→ 状态转换即时 reportTaskStatus（去重，只在变化时报）+ 活跃任务
//         批量 reportProgress。节流批量是性能关键：进度只走内存 + SSE，绝不落库。
//         上报是"确认式推进"：只有服务端确认收到之后才落 lastStatus/解绑，失败
//         原样保留旧状态，下一轮 tick 自然重试——绝不能在请求还没成功时就假定
//         发起端已经看到了新状态。
//   命令：SSE 收到 task.command（目标为本机）→ 查绑定表把 pause/resume/cancel 打到
//         对应本地任务上。
//
// 绑定表持久化在 localStorage，按「云账号 + 本地引擎地址」分区（换号/换引擎地址
// 自然隔离，不会拿旧环境的绑定去新环境里误报 failed）：浏览器刷新会清空整个模块
// 内存，桌面端靠「重连全量 + 按 url/fileName/saveDir 回找」重建绑定，面板则直接把
// cloudTaskId → localTaskId 落盘，刷新后进度上报无缝续上。
//
// 数据面永远直连本地引擎执行；云端仅做连接与下发/进度中转，不取回文件。

import type { QueryClient } from '@tanstack/react-query'
import { api } from '../api'
import { getBase, isAuthenticated } from '../auth'
import { liveStore } from '../ws'
import type { TaskDto, TaskStatus } from '../types'
import { cloudApi } from './client'
import { isExecutorLeader, startExecutorLease } from './executorLease'
import { cloudDeviceId, cloudUserId } from './session'
import { CloudApiError } from './types'
import type { ProgressReportItem, RemoteTask, RemoteTaskStatus } from './types'

const REPORT_INTERVAL_MS = 1_000
/** 宿主引擎建任务请求的超时兜底：没有它，一次挂起的 fetch 会让 `accepting` 里的
 *  条目永远卡住，后续同一条 dispatch（重连全量补单/SSE 重放）永远被去重挡在门外。 */
const ACCEPT_TIMEOUT_MS = 20_000
/** `!dto` 宽限轮次：['tasks'] 缓存可能只是暂时没刷到刚创建的本地任务（竞态），
 *  不能第一轮就判"用户删了它"。连续缺失满这个轮次才真正判定为本地任务已消失。 */
const MISS_GRACE_ROUNDS = 3

/** 本地任务状态码（0=pending 1=downloading 2=paused 3=completed 4=error 5=preparing）
 *  → 云端状态机。preparing 对发起端而言就是「已经在下了」，与 downloading 同映射。 */
const WIRE_STATUS: Record<TaskStatus, RemoteTaskStatus> = {
  0: 'accepted',
  1: 'downloading',
  2: 'paused',
  3: 'completed',
  4: 'failed',
  5: 'downloading',
}

/** 本地任务被用户删除后回报给发起端的错误说明。这是回传数据不是 UI 文案，不走 i18n
 *  （发起端按自己的语言环境展示，服务端原样存 error 字段）。 */
const LOCAL_TASK_GONE = 'local task no longer exists on the executing device'

/** 执行端绑定表：cloudTaskId → localTaskId，按当前绑定表键分区（见 bindingsKey）。 */
let bindings: Record<string, string> = {}
let bindingsKeyCache = ''
/** 上一次成功上报的 wire 状态，去重用（内存态，刷新后首轮自然补报一次）。只在
 *  上报确认成功后才写入——绝不能提前预置，否则一次失败的上报会被永久当成成功。 */
const lastStatus = new Map<string, RemoteTaskStatus>()
/** 正在建本地任务、尚未写入绑定表的 cloudTaskId，防止 dispatch 事件与全量补单撞车。 */
const accepting = new Set<string>()
/** 本地快照连续缺失 localId 绑定的轮数，见 handleMissingLocalTask。 */
const missCount = new Map<string, number>()
/** 登出/切账号代际计数：clearRemoteTaskBindings 递增，接单异步闭包落盘前核对代际
 *  未变才写入，否则丢弃——防止登出竞态把旧账号建的本地任务绑定写回新账号的表。 */
let epoch = 0

let queryClientRef: QueryClient | null = null
let reportTimer: ReturnType<typeof setInterval> | null = null
let stopLease: (() => void) | null = null
let ticking = false
let visibilityTriggerInstalled = false

// ---------------------------------------------------------------------------
// 绑定表 —— 按「云账号 + 本地引擎地址」分区持久化
// ---------------------------------------------------------------------------

/** FNV-1a 32 位，非密码学用途：只用来把引擎地址折成一段短标识，避免把带端口/路径
 *  的完整 URL 直接拼进 localStorage key。 */
function fingerprint(s: string): string {
  let h = 0x811c9dc5
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i)
    h = Math.imul(h, 0x01000193)
  }
  return (h >>> 0).toString(36)
}

function bindingsKey(): string {
  return `fluxdown.cloud.taskBindings::${cloudUserId() || 'anon'}::${fingerprint(getBase() || 'local')}`
}

function loadBindings(key: string): Record<string, string> {
  try {
    const raw = localStorage.getItem(key)
    if (!raw) return {}
    const parsed = JSON.parse(raw) as unknown
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return {}
    const out: Record<string, string> = {}
    for (const [rid, localId] of Object.entries(parsed as Record<string, unknown>)) {
      if (typeof localId === 'string' && localId) out[rid] = localId
    }
    return out
  } catch {
    return {} // 解析失败当空表：宁可丢绑定，也不让坏数据卡死执行端
  }
}

function persistBindings() {
  try {
    localStorage.setItem(bindingsKeyCache, JSON.stringify(bindings))
  } catch {
    /* 隐私模式/配额耗尽：本次会话内存态仍可用，刷新后退化为丢绑定 */
  }
}

/** 云账号切换（未刷新页面）或本地引擎地址变化时，切换到对应的绑定表——旧键的数据
 *  留在 localStorage 原地不动，供切回去时继续用；内存态的去重/宽限计数清空重开，
 *  它们只对"当前正在用的那张表"有意义。 */
function syncBindingsKey() {
  const key = bindingsKey()
  if (key === bindingsKeyCache) return
  bindingsKeyCache = key
  bindings = loadBindings(key)
  lastStatus.clear()
  missCount.clear()
  accepting.clear()
}

function unbind(rid: string) {
  delete bindings[rid]
  lastStatus.delete(rid)
  missCount.delete(rid)
}

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

function withTimeout<T>(p: Promise<T>, ms: number): Promise<T> {
  return Promise.race([
    p,
    new Promise<never>((_, reject) => setTimeout(() => reject(new Error('accept timeout')), ms)),
  ])
}

// ---------------------------------------------------------------------------
// 上报（确认式：调用方必须看返回值再决定要不要落状态）
// ---------------------------------------------------------------------------

/** 'ok' = 服务端确认收到；'retry' = 网络错误/5xx/限流等临时故障，下一轮 tick 原样
 *  重试；'fatal' = 服务端明确判定不可重试的语义冲突（任务已终态/上报设备不匹配/
 *  任务已不存在）——顶着这类错误硬报只会一直失败，调用方须解绑止损。 */
type ReportOutcome = 'ok' | 'retry' | 'fatal'

async function safeReportStatus(
  rid: string,
  body: { status: RemoteTaskStatus; totalBytes?: number; fileName?: string; error?: string },
): Promise<ReportOutcome> {
  try {
    await cloudApi.reportTaskStatus(rid, body)
    return 'ok'
  } catch (e) {
    if (e instanceof CloudApiError && (e.code === 'task_state_conflict' || e.code === 'task_device_mismatch' || e.status === 404)) {
      console.warn('[remoteTaskExecutor] reportTaskStatus fatal, unbinding', rid, e)
      return 'fatal'
    }
    console.warn('[remoteTaskExecutor] reportTaskStatus failed, will retry', rid, e)
    return 'retry'
  }
}

async function safeReportProgress(items: ProgressReportItem[]) {
  try {
    await cloudApi.reportProgress(items)
  } catch (e) {
    console.warn('[remoteTaskExecutor] reportProgress failed', e)
  }
}

// ---------------------------------------------------------------------------
// 接单
// ---------------------------------------------------------------------------

/** 发起端下发时可能带了它自己的本地保存目录（saveDir）——那是发起端主机上的路径，
 *  执行端很可能是完全不同的操作系统/挂载布局，直接照搬大概率不存在或无权限。带
 *  saveDir 建任务失败 → 去掉 saveDir 用宿主引擎自己的默认目录重试一次，两次都
 *  失败才真正判定为接单失败。 */
async function createTaskWithSaveDirFallback(task: RemoteTask): Promise<{ taskId: string }> {
  const base = {
    url: task.url,
    ...(task.fileName ? { fileName: task.fileName } : {}),
  }
  if (!task.saveDir) return withTimeout(api.createTask(base), ACCEPT_TIMEOUT_MS)
  try {
    return await withTimeout(api.createTask({ ...base, saveDir: task.saveDir }), ACCEPT_TIMEOUT_MS)
  } catch {
    return withTimeout(api.createTask(base), ACCEPT_TIMEOUT_MS)
  }
}

/** 单条下发落地：目标为本机、仍是 pending、且未绑定过才接（幂等，供 SSE 事件与
 *  全量补单共用同一入口）。非 leader 标签页直接放弃——多标签页选主下只有 leader
 *  才建本地任务，否则同一条下发会被建两次本地任务。 */
export function acceptRemoteTask(task: RemoteTask) {
  if (!isExecutorLeader()) return
  // 宿主引擎未登录时任何 api.* 都会触发「清凭证跳登录」，不能碰；这条下发留在
  // pending，等下次 SSE 重连的全量补单再接（服务端另有 7 天兜底）。
  if (!isAuthenticated()) return
  if (!task?.id || task.toDevice !== cloudDeviceId() || task.status !== 'pending') return
  syncBindingsKey()
  if (bindings[task.id] || accepting.has(task.id)) return
  accepting.add(task.id)
  const startEpoch = epoch
  void (async () => {
    try {
      const created = await createTaskWithSaveDirFallback(task)
      // 登出/切账号竞态：绑定表已经换成另一张，这条创建结果对不上号了，直接丢弃
      // ——落盘或上报都会把旧账号的产物写进新账号的状态里。
      if (epoch !== startEpoch) return
      bindings[task.id] = created.taskId
      persistBindings()
      // 只有上报确认成功才记 lastStatus——不然下一轮 tick 会把"已经报过 accepted"
      // 误当成真，实际发起端可能压根没收到，永远卡在服务端默认的 pending 视图上。
      const outcome = await safeReportStatus(task.id, { status: 'accepted' })
      if (outcome === 'ok') lastStatus.set(task.id, 'accepted')
      else if (outcome === 'fatal') {
        unbind(task.id)
        persistBindings()
      }
      // 'retry'：绑定已落盘，lastStatus 缺失会让下一轮 tick 自然把 accepted 补报一次。
    } catch (e) {
      if (epoch === startEpoch) await safeReportStatus(task.id, { status: 'failed', error: errorText(e) })
    } finally {
      accepting.delete(task.id)
    }
  })()
}

/** 全量快照补单：断线/关机期间积压的、目标为本机的 pending 记录逐条走 acceptRemoteTask。 */
export function acceptPendingRemoteTasks(tasks: RemoteTask[]) {
  for (const task of tasks) acceptRemoteTask(task)
}

/** SSE 重连：断线期间可能有 accept 请求还挂在 accepting 里却永远等不到 finally
 *  （标签页休眠/网络切换导致 fetch 悬挂却不报错）；重连视为新的一轮，清空重试
 *  ——真正卡住的那条会在下一次全量补单/dispatch 重放里重新走一遍接单流程。 */
export function resetAcceptingState() {
  accepting.clear()
}

// ---------------------------------------------------------------------------
// 命令
// ---------------------------------------------------------------------------

/** pause/resume：本地操作失败时不能装死——发起端已经把 UI 切到"已暂停/已恢复"的
 *  乐观态，本机这边其实没变。强制清掉 lastStatus 并立即把当前真实状态重新报一次，
 *  纠正发起端的错误视图；上报也失败只能 warn 兜底（下一轮 1s tick 仍会因为状态
 *  未变化而继续尝试）。 */
async function runControlAction(rid: string, localId: string, run: () => Promise<unknown>) {
  try {
    await run()
  } catch (e) {
    console.warn('[remoteTaskExecutor] control action failed', rid, e)
    lastStatus.delete(rid)
    const dto = queryClientRef?.getQueryData<TaskDto[]>(['tasks'])?.find((t) => t.taskId === localId)
    if (!dto) return
    const wire = WIRE_STATUS[dto.status] ?? 'accepted'
    const outcome = await safeReportStatus(rid, { status: wire })
    if (outcome === 'ok') lastStatus.set(rid, wire)
    else console.warn('[remoteTaskExecutor] correction report failed', rid)
  }
}

/** cancel：先解绑 + 落盘再动本地任务——即便 api.deleteTask 本身挂起，绑定表也已经
 *  不再指向这条任务，不会有迟到的 1s tick 拿着已撤销的本地任务继续上报进度（这是
 *  撤销竞速的另一半防线，前一半是发起端侧的落库+移出 hub）。上报失败重试一次，
 *  仍失败只能 warn：本地任务已经真的删了，没有状态可回滚。 */
async function runCancel(rid: string, localId: string) {
  unbind(rid)
  persistBindings()
  try {
    // 只撤任务不删文件：发起端"取消"的语义是停止传输，落地产物归本机用户处置。
    await api.deleteTask(localId, false)
  } catch (e) {
    console.warn('[remoteTaskExecutor] cancel deleteTask failed', rid, e)
  }
  let outcome = await safeReportStatus(rid, { status: 'canceled' })
  if (outcome === 'retry') outcome = await safeReportStatus(rid, { status: 'canceled' })
  if (outcome !== 'ok') console.warn('[remoteTaskExecutor] cancel report failed after retry', rid)
}

/** task.command 事件：`{taskId, action, toDevice|targetDevice}`。找不到绑定就忽略
 *  ——该命令是发给别的设备的，或本机压根没接过这条任务。非 leader 标签页也忽略：
 *  它没有在跑接单/上报，对命令采取真实动作只会跟 leader 打架。 */
export function applyRemoteCommand(data: Record<string, unknown>) {
  const target = (data.targetDevice ?? data.toDevice) as string | undefined
  if (target !== cloudDeviceId()) return
  if (!isExecutorLeader()) return
  const rid = (data.taskId ?? data.id) as string | undefined
  const action = data.action as string | undefined
  if (!rid || !action) return
  syncBindingsKey()
  const localId = bindings[rid]
  if (!localId || !isAuthenticated()) return
  void (async () => {
    switch (action) {
      case 'pause':
        await runControlAction(rid, localId, () => api.pauseTask(localId))
        break
      case 'resume':
        await runControlAction(rid, localId, () => api.continueTask(localId))
        break
      case 'cancel':
        await runCancel(rid, localId)
        break
      default:
        break
    }
  })()
}

// ---------------------------------------------------------------------------
// 1s 采样上报
// ---------------------------------------------------------------------------

/** 兜底拉取宿主任务全量，避免执行端在非任务页失明。同一时刻只保留一个 in-flight。 */
let snapshotInflight: Promise<unknown> | null = null
function ensureTaskSnapshot(qc: QueryClient) {
  snapshotInflight ??= qc
    .ensureQueryData({ queryKey: ['tasks'], queryFn: api.listTasks })
    .catch(() => undefined)
    .finally(() => {
      snapshotInflight = null
    })
}

/** 本地快照里查不到绑定的 localId：不能立即判定"已删除"——['tasks'] 缓存可能只是
 *  暂未包含刚创建的任务（竞态）或短暂脏读。首次缺失强制刷新一次全量并跳过本轮，
 *  连续 MISS_GRACE_ROUNDS 轮仍缺失才真正判定为用户在本机删除了任务。 */
async function handleMissingLocalTask(rid: string) {
  const count = (missCount.get(rid) ?? 0) + 1
  missCount.set(rid, count)
  if (count < MISS_GRACE_ROUNDS) {
    const qc = queryClientRef
    if (qc) void qc.invalidateQueries({ queryKey: ['tasks'], refetchType: 'all' }).catch(() => undefined)
    return
  }
  const outcome = await safeReportStatus(rid, { status: 'failed', error: LOCAL_TASK_GONE })
  // 只有确认成功/fatal 才解绑；'retry' 时把 missCount 原样留在阈值上，下一轮直接
  // 重试上报，不必再空等 MISS_GRACE_ROUNDS 轮。
  if (outcome !== 'retry') {
    missCount.delete(rid)
    unbind(rid)
    persistBindings()
  }
}

async function runTick() {
  if (!isExecutorLeader()) return // 非 leader 标签页不跑上报，避免跟 leader 抢报同一条任务
  syncBindingsKey()
  const rids = Object.keys(bindings)
  if (rids.length === 0) return
  if (!isAuthenticated()) return
  const qc = queryClientRef
  if (!qc) return
  const tasks = qc.getQueryData<TaskDto[]>(['tasks'])
  if (!tasks) {
    // ['tasks'] 只在任务页有订阅者，离开该路由 5 分钟后会被 GC；此处主动填一次，
    // 填上之后 ws 的 taskProgress 分支就会继续 setQueryData 维护它。本轮跳过，
    // 绝不能把「缓存空」当成「任务被删」。
    ensureTaskSnapshot(qc)
    return
  }

  const byId = new Map(tasks.map((t) => [t.taskId, t]))
  const live = liveStore.get()
  const items: ProgressReportItem[] = []

  // 逐条 await 串行处理状态转换：只有上报确认成功才落 lastStatus/解绑，失败原样
  // 保留绑定与旧去重值，下一轮 tick 自然重试——这是「确认式推进」的核心，绝不能
  // 并发触发多条尚未确认的同一 rid 上报（否则乱序到达可能让旧状态覆盖新状态）。
  for (const rid of rids) {
    const localId = bindings[rid]
    const dto = byId.get(localId)
    if (!dto) {
      await handleMissingLocalTask(rid)
      continue
    }
    missCount.delete(rid) // 命中一次即清零，宽限计数只统计"连续"缺失

    // live 帧比 Query 快照新（高频进度不进 Query 缓存），有则优先。
    const frame = live[localId]
    const status = frame?.status ?? dto.status
    const downloadedBytes = frame?.downloadedBytes ?? dto.downloadedBytes
    const totalBytes = frame?.totalBytes || dto.totalBytes
    const speed = frame?.speed ?? 0
    const fileName = frame?.fileName || dto.fileName
    const errorMessage = frame?.errorMessage || dto.errorMessage

    // 未知状态码（引擎新增）保守视为「已接单」，不误报成功/失败。
    const wire = WIRE_STATUS[status] ?? 'accepted'
    if (lastStatus.get(rid) !== wire) {
      const outcome = await safeReportStatus(rid, {
        status: wire,
        ...(totalBytes > 0 ? { totalBytes } : {}),
        ...(fileName ? { fileName } : {}),
        ...(wire === 'failed' && errorMessage ? { error: errorMessage } : {}),
      })
      if (outcome === 'ok') {
        lastStatus.set(rid, wire)
        if (wire === 'completed' || wire === 'failed' || wire === 'canceled') {
          unbind(rid)
          persistBindings()
        }
      } else if (outcome === 'fatal') {
        // 服务端判了不可重试的语义冲突（任务已终态/上报设备不匹配）：继续报下去
        // 只会一直 409/403，解绑止损，本地任务照常跑，只是不再对外播报。
        unbind(rid)
        persistBindings()
      }
      // 'retry'：什么都不做，绑定与旧 lastStatus 原样保留，下一轮自然重试。
      continue
    }
    if (status === 1) {
      items.push({
        taskId: rid,
        downloadedBytes,
        speed,
        progress: totalBytes > 0 ? downloadedBytes / totalBytes : 0,
      })
    }
  }

  if (items.length > 0) void safeReportProgress(items)
}

/** setInterval 回调改成 async 之后必须防重入：上一轮尚未完成（网络慢/大量任务
 *  串行上报耗时超过 1s）时新的一拍到来会叠层调用，导致同一条 rid 的多次上报乱序
 *  竞速，破坏"确认式推进"的单调性。 */
function reportTick() {
  if (ticking) return
  ticking = true
  void runTick().finally(() => {
    ticking = false
  })
}

/** 页面从后台切回前台：后台标签页里 setInterval 可能被浏览器节流到几十秒一次，
 *  错过的状态转换要立即补一轮，而不是等下一个自然节拍（`lib/ws.ts` 的重扫触发器
 *  同一模式，此处独立安装避免跨模块耦合）。 */
function installVisibilityTrigger() {
  if (visibilityTriggerInstalled) return
  visibilityTriggerInstalled = true
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'visible') reportTick()
  })
}

// ---------------------------------------------------------------------------
// 生命周期（由 useRemoteTasks.ts 的 attach/detach 统一驱动）
// ---------------------------------------------------------------------------

/** 登录态生效时启动采样定时器 + 多标签页选主；qc 用于读取宿主任务快照（同
 *  connectWs 的传参范式）。 */
export function startRemoteTaskExecutor(queryClient: QueryClient) {
  queryClientRef = queryClient
  reportTimer ??= setInterval(reportTick, REPORT_INTERVAL_MS)
  installVisibilityTrigger()
  stopLease ??= startExecutorLease((leader) => {
    // 刚接过主权（原 leader 标签页关闭/失焦太久被换主）：立刻补一轮，不必等下一个
    // 1s 节拍才开始接单/上报，减少跨设备任务因选主延迟多卡的时间。
    if (leader) reportTick()
  })
}

/** 停止采样（断连/登出）。绑定表保留：仅登出才由 clearRemoteTaskBindings 清空。 */
export function stopRemoteTaskExecutor() {
  if (reportTimer) {
    clearInterval(reportTimer)
    reportTimer = null
  }
  accepting.clear()
  stopLease?.()
  stopLease = null
}

/** 登出：绑定表属于「本账号在本设备上的执行端状态」，换号后必须清空，否则新账号
 *  的 tick 会拿旧 cloudTaskId 去上报（必然 404/403）。递增 epoch 让仍在飞行中的
 *  接单异步闭包在落盘前发现代际已变，直接丢弃结果——不然登出竞态会把旧账号刚建
 *  好的本地任务绑定写回新账号的表。 */
export function clearRemoteTaskBindings() {
  epoch += 1
  bindings = {}
  lastStatus.clear()
  missCount.clear()
  accepting.clear()
  try {
    if (bindingsKeyCache) localStorage.removeItem(bindingsKeyCache)
  } catch {
    /* 存储不可用：内存态已清空，够用 */
  }
}
