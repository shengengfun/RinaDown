// 多标签页选主 —— 同一浏览器开多个面板标签页时，每个标签页都会各自建立 SSE 连接、
// 各自的 remoteTaskExecutor 都会看到同一条 task.dispatch；如果都去接单，同一个
// cloudTaskId 会被创建两份本地任务（对发起端表现为重复下载、状态互相覆盖）。用
// localStorage 租约选出唯一 leader：只有 leader 允许接单与状态/进度上报，其余标签
// 页退化为纯查看端（仍能收 SSE、仍能看任务列表，只是不落地执行）。
//
// 租约判据始终是 localStorage 里的记录本身（跨标签页共享、持久，无 BroadcastChannel
// 时单靠它也能选出主，只是接主有最多一个续租周期的延迟）；BroadcastChannel 只是锦上
// 添花——leader 主动释放时广播一声，让等位标签页不必空等到 TTL 到期才能接主。

const LEASE_KEY = 'fluxdown.cloud.executorLease'
const CHANNEL_NAME = 'fluxdown.cloud.executor'
/** 续租间隔：必须明显小于 TTL，否则一次 setInterval 抖动/标签页节流就可能让租约
 *  在续期前过期，被其它标签页抢走导致来回易主。 */
const RENEW_INTERVAL_MS = 2_000
const LEASE_TTL_MS = 5_000

interface LeaseRecord {
  tabId: string
  expiresAt: number
}

/** 选主用的临时票号，不需要密码学随机性——Math.random 足够，且不受 Secure Context
 *  限制（NAS/Docker 面板经明文 HTTP 访问时 crypto.randomUUID 可能整个缺失）。 */
function randomTabId(): string {
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`
}

const tabId = randomTabId()
let renewTimer: ReturnType<typeof setInterval> | null = null
let leaderFlag = false
let onLeaderChange: ((leader: boolean) => void) | null = null
let channel: BroadcastChannel | null = null
let releaseHandler: (() => void) | null = null

function readLease(): LeaseRecord | null {
  try {
    const raw = localStorage.getItem(LEASE_KEY)
    if (!raw) return null
    const parsed = JSON.parse(raw) as Partial<LeaseRecord>
    if (typeof parsed.tabId !== 'string' || typeof parsed.expiresAt !== 'number') return null
    return parsed as LeaseRecord
  } catch {
    return null
  }
}

function writeLease(record: LeaseRecord) {
  try {
    localStorage.setItem(LEASE_KEY, JSON.stringify(record))
  } catch {
    /* 隐私模式/配额耗尽：本轮抢主失败，下一轮 tick 重试；查看端功能不受影响 */
  }
}

/** 只清自己持有的租约——避免刚失去 leadership（被别的标签页抢到）时手快误删新主
 *  刚写下的记录，导致两个标签页都以为对方是主、都不接单。 */
function clearOwnLease() {
  try {
    if (readLease()?.tabId === tabId) localStorage.removeItem(LEASE_KEY)
  } catch {
    /* ignore */
  }
}

function setLeader(next: boolean) {
  if (leaderFlag === next) return
  leaderFlag = next
  onLeaderChange?.(next)
}

/** 单轮抢主/续租：租约不存在、已过期、或本来就是自己的 → 据为己有/续期；
 *  租约是别人的且未过期 → 让位。localStorage 写入对同源其它标签页立即可见，
 *  不依赖 storage 事件的到达时机，足够避免长期双主。 */
function tick() {
  const now = Date.now()
  const cur = readLease()
  if (!cur || cur.expiresAt <= now || cur.tabId === tabId) {
    writeLease({ tabId, expiresAt: now + LEASE_TTL_MS })
    setLeader(true)
  } else {
    setLeader(false)
  }
}

/** 启动选主：立即抢一轮 + 定时续租，leader 状态变化经 cb 回调通知调用方（只有
 *  leader=true 时才允许 remoteTaskExecutor 接单/上报）。返回停止函数：清理定时器、
 *  主动释放租约（持有时）、拆除监听。 */
export function startExecutorLease(cb: (leader: boolean) => void): () => void {
  onLeaderChange = cb
  tick()
  renewTimer = setInterval(tick, RENEW_INTERVAL_MS)
  if (typeof BroadcastChannel !== 'undefined') {
    channel = new BroadcastChannel(CHANNEL_NAME)
    // 收到"已释放"广播立即重新抢一轮，不必等下一次 RENEW_INTERVAL_MS——多标签页
    // 场景下用户关掉当前主标签页后，剩余标签页能在毫秒级切主，而不是空等到 TTL。
    channel.onmessage = (ev) => {
      if (ev.data === 'released') tick()
    }
  }
  // 标签页关闭/刷新前主动放权：不这样做的话租约要等 TTL（5s）耗尽其它标签页才能
  // 接手，期间没人接单也没人上报，跨设备任务会白白卡 5 秒。
  releaseHandler = () => {
    if (leaderFlag) {
      clearOwnLease()
      channel?.postMessage('released')
    }
  }
  window.addEventListener('beforeunload', releaseHandler)
  window.addEventListener('pagehide', releaseHandler)
  return () => {
    if (renewTimer) {
      clearInterval(renewTimer)
      renewTimer = null
    }
    releaseHandler?.()
    if (releaseHandler) {
      window.removeEventListener('beforeunload', releaseHandler)
      window.removeEventListener('pagehide', releaseHandler)
      releaseHandler = null
    }
    channel?.close()
    channel = null
    setLeader(false)
    onLeaderChange = null
  }
}

/** 当前标签页是否持有执行端主权。 */
export function isExecutorLeader(): boolean {
  return leaderFlag
}
