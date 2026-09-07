// 类型化 REST 客户端。401 → 清凭证跳登录。

import { clearCredentials, getBase, getToken } from './auth'
import { t, translateBackendMessage } from './i18n'
import type {
  ApiInfo,
  ComponentFfmpegStatus,
  ComponentVersions,
  ComponentYtdlpStatus,
  ConfigMap,
  CreateGroupRequest,
  CreateGroupResponse,
  CreateQueueRequest,
  CreateTaskRequest,
  CreatedRssSource,
  CreatedTask,
  Ed2kServerSubRefreshResponse,
  FsListResponse,
  GroupDto,
  InstallFfmpegRequest,
  InstalledPlugin,
  LogsResponse,
  MarketEntry,
  PingInfo,
  PluginDto,
  ProxyTestRequest,
  ProxyTestResponse,
  QueueDto,
  ResolvePreviewRequest,
  ResolvePreviewResponse,
  QueueOrderRequest,
  QueueScheduleRequest,
  RssItemActionRequest,
  RssItemDto,
  RssSourceDto,
  RssValidateRequest,
  RssValidateResponse,
  SetupStatus,
  StatsResponse,
  TaskDto,
  TokenResponse,
  TrackerSubRefreshResponse,
  WebhookDeliveriesResponse,
  WebhookEndpoint,
  WebhookSimulateResponse,
  WebhookTestResponse,
} from './types'

export class ApiError extends Error {
  status: number
  constructor(status: number, message: string) {
    super(message)
    this.status = status
  }
}

export async function apiFetch<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${getBase()}${path}`, {
    ...init,
    headers: {
      Authorization: `Bearer ${getToken()}`,
      ...(init?.body ? { 'Content-Type': 'application/json' } : {}),
      ...init?.headers,
    },
  })
  if (res.status === 401) {
    clearCredentials()
    if (!location.pathname.startsWith('/login')) location.href = '/login'
    throw new ApiError(401, 'unauthorized')
  }
  if (!res.ok) {
    let message = res.statusText
    try {
      const body = (await res.json()) as { message?: string }
      if (body.message) message = body.message
    } catch {
      /* 非 JSON 错误体，用 statusText */
    }
    throw new ApiError(res.status, translateBackendMessage(message))
  }
  return (await res.json()) as T
}

export const api = {
  // 探活（无鉴权）：版本 + 服务器默认语言，登录前也可用
  ping: async (): Promise<PingInfo> => {
    const res = await fetch(`${getBase()}/ping`)
    if (!res.ok) throw new ApiError(res.status, res.statusText)
    return (await res.json()) as PingInfo
  },

  // 登录校验（带指定凭证探测，不写存储）
  probe: async (base: string, token: string): Promise<ApiInfo> => {
    const res = await fetch(`${base}/api/v1/info`, {
      headers: { Authorization: `Bearer ${token}` },
    })
    if (!res.ok) throw new ApiError(res.status, res.status === 401 ? t('login.invalidToken') : res.statusText)
    return (await res.json()) as ApiInfo
  },

  // 首次运行状态（无鉴权）：服务器是否还没设置访问密钥。
  setupStatus: async (base?: string): Promise<SetupStatus> => {
    const res = await fetch(`${base ?? getBase()}/api/v1/setup/status`)
    if (!res.ok) throw new ApiError(res.status, res.statusText)
    return (await res.json()) as SetupStatus
  },

  // 首次运行：落定访问密钥（无鉴权，仅在未设置时可用；已设置返回 409）。
  completeSetup: async (base: string, token: string): Promise<void> => {
    const res = await fetch(`${base}/api/v1/setup`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ token }),
    })
    if (!res.ok) {
      let message = res.statusText
      try {
        const body = (await res.json()) as { message?: string }
        if (body.message) message = body.message
      } catch {
        /* 非 JSON 错误体，用 statusText */
      }
      throw new ApiError(res.status, translateBackendMessage(message))
    }
  },

  info: () => apiFetch<ApiInfo>('/api/v1/info'),
  listTasks: () => apiFetch<TaskDto[]>('/api/v1/tasks'),
  getTask: (id: string) => apiFetch<TaskDto>(`/api/v1/tasks/${id}`),
  createTask: (req: CreateTaskRequest) =>
    apiFetch<CreatedTask>('/api/v1/tasks', { method: 'POST', body: JSON.stringify(req) }),
  deleteTask: (id: string, deleteFiles: boolean) =>
    apiFetch<unknown>(`/api/v1/tasks/${id}?deleteFiles=${deleteFiles}`, { method: 'DELETE' }),
  pauseTask: (id: string) => apiFetch<unknown>(`/api/v1/tasks/${id}/pause`, { method: 'PUT' }),
  continueTask: (id: string) =>
    apiFetch<unknown>(`/api/v1/tasks/${id}/continue`, { method: 'PUT' }),
  pauseAll: () => apiFetch<unknown>('/api/v1/tasks/pause', { method: 'PUT' }),
  continueAll: () => apiFetch<unknown>('/api/v1/tasks/continue', { method: 'PUT' }),
  boostTask: (id: string) => apiFetch<unknown>(`/api/v1/tasks/${id}/boost`, { method: 'PUT' }),
  /** 立即重扫已完成任务的产物是否还在磁盘上；结果经 WS `fileMissingChanged` 回来。 */
  rescanFiles: () => apiFetch<unknown>('/api/v1/tasks/rescan', { method: 'POST' }),
  moveTaskToQueue: (id: string, queueId: string) =>
    apiFetch<unknown>(`/api/v1/tasks/${id}/queue`, {
      method: 'PUT',
      body: JSON.stringify({ queueId }),
    }),
  // 重命名任务文件（错误码经 body.message 原样透传：invalid-name/task-active/
  // bt-unsupported/target-exists/not-found 或引擎原文，由弹窗侧映射文案）
  renameTask: (id: string, fileName: string) =>
    apiFetch<unknown>(`/api/v1/tasks/${id}/rename`, {
      method: 'POST',
      body: JSON.stringify({ fileName }),
    }),

  // 任务组与前置预解析（多文件下载）
  resolvePreview: (req: ResolvePreviewRequest) =>
    apiFetch<ResolvePreviewResponse>('/api/v1/resolve/preview', {
      method: 'POST',
      body: JSON.stringify(req),
    }),
  listGroups: () => apiFetch<GroupDto[]>('/api/v1/groups'),
  createGroup: (req: CreateGroupRequest) =>
    apiFetch<CreateGroupResponse>('/api/v1/groups', { method: 'POST', body: JSON.stringify(req) }),
  deleteGroup: (id: string, deleteFiles: boolean) =>
    apiFetch<unknown>(`/api/v1/groups/${id}?deleteFiles=${deleteFiles}`, { method: 'DELETE' }),
  pauseGroup: (id: string) => apiFetch<unknown>(`/api/v1/groups/${id}/pause`, { method: 'PUT' }),
  continueGroup: (id: string) =>
    apiFetch<unknown>(`/api/v1/groups/${id}/continue`, { method: 'PUT' }),

  listQueues: () => apiFetch<QueueDto[]>('/api/v1/queues'),
  createQueue: (req: CreateQueueRequest) =>
    apiFetch<unknown>('/api/v1/queues', { method: 'POST', body: JSON.stringify(req) }),
  updateQueue: (id: string, req: CreateQueueRequest) =>
    apiFetch<unknown>(`/api/v1/queues/${id}`, { method: 'PUT', body: JSON.stringify(req) }),
  deleteQueue: (id: string) => apiFetch<unknown>(`/api/v1/queues/${id}`, { method: 'DELETE' }),
  startQueue: (id: string) => apiFetch<unknown>(`/api/v1/queues/${id}/start`, { method: 'POST' }),
  stopQueue: (id: string) => apiFetch<unknown>(`/api/v1/queues/${id}/stop`, { method: 'POST' }),
  setQueueSchedule: (id: string, req: QueueScheduleRequest) =>
    apiFetch<unknown>(`/api/v1/queues/${id}/schedule`, { method: 'PUT', body: JSON.stringify(req) }),
  reorderQueue: (id: string, taskIds: string[]) =>
    apiFetch<unknown>(`/api/v1/queues/${id}/order`, {
      method: 'PUT',
      body: JSON.stringify({ taskIds } satisfies QueueOrderRequest),
    }),

  // RSS 订阅：条目流单独走 ['rss-items', sourceId] 缓存（WS rssItemsChanged 直接整表替换）。
  listRssSources: () => apiFetch<RssSourceDto[]>('/api/v1/rss'),
  createRssSource: (req: RssSourceDto) =>
    apiFetch<CreatedRssSource>('/api/v1/rss', { method: 'POST', body: JSON.stringify(req) }),
  updateRssSource: (id: string, req: RssSourceDto) =>
    apiFetch<unknown>(`/api/v1/rss/${id}`, { method: 'PUT', body: JSON.stringify(req) }),
  deleteRssSource: (id: string) => apiFetch<unknown>(`/api/v1/rss/${id}`, { method: 'DELETE' }),
  refreshRssSource: (id: string) => apiFetch<unknown>(`/api/v1/rss/${id}/refresh`, { method: 'POST' }),
  listRssItems: (id: string) => apiFetch<RssItemDto[]>(`/api/v1/rss/${id}/items`),
  rssItemAction: (id: string, req: RssItemActionRequest) =>
    apiFetch<unknown>(`/api/v1/rss/${id}/items/action`, { method: 'POST', body: JSON.stringify(req) }),
  // 抓取失败不是 HTTP 错误：仍返回 200，失败原因在 error 字段（这是一次诊断调用）。
  validateRssFeed: (req: RssValidateRequest) =>
    apiFetch<RssValidateResponse>('/api/v1/rss/validate', { method: 'POST', body: JSON.stringify(req) }),

  getConfig: () => apiFetch<ConfigMap>('/api/v1/config'),
  putConfig: (entries: ConfigMap) =>
    apiFetch<unknown>('/api/v1/config', { method: 'PUT', body: JSON.stringify(entries) }),

  refreshTrackerSub: () =>
    apiFetch<TrackerSubRefreshResponse>('/api/v1/bt/tracker-sub/refresh', { method: 'POST' }),
  refreshEd2kServerSub: () =>
    apiFetch<Ed2kServerSubRefreshResponse>('/api/v1/ed2k/server-sub/refresh', { method: 'POST' }),
  fsList: (path: string) =>
    apiFetch<FsListResponse>(`/api/v1/fs/list?path=${encodeURIComponent(path)}`),
  proxyTest: (req: ProxyTestRequest) =>
    apiFetch<ProxyTestResponse>('/api/v1/proxy/test', { method: 'POST', body: JSON.stringify(req) }),
  regenerateToken: () =>
    apiFetch<TokenResponse>('/api/v1/token/regenerate', { method: 'POST' }),
  stats: () => apiFetch<StatsResponse>('/api/v1/stats'),
  logs: () => apiFetch<LogsResponse>('/api/v1/logs'),

  // Webhook 任务事件推送（端点表本身是 config 键 `webhook.endpoints`，
  // 这里只有引擎内存里查不到的东西：投递日志 / 预设目录 / 测试投递）。
  webhookDeliveries: () => apiFetch<WebhookDeliveriesResponse>('/api/v1/webhooks/deliveries'),
  clearWebhookDeliveries: () =>
    apiFetch<unknown>('/api/v1/webhooks/deliveries', { method: 'DELETE' }),
  simulateWebhook: () =>
    apiFetch<WebhookSimulateResponse>('/api/v1/webhooks/simulate', { method: 'POST' }),
  testWebhook: (endpoint: WebhookEndpoint) =>
    apiFetch<WebhookTestResponse>('/api/v1/webhooks/test', {
      method: 'POST',
      body: JSON.stringify(endpoint),
    }),

  listPlugins: () => apiFetch<PluginDto[]>('/api/v1/plugins'),
  installPlugin: (zip: File | Blob | ArrayBuffer) =>
    apiFetch<InstalledPlugin>('/api/v1/plugins/install', {
      method: 'POST',
      body: zip,
      headers: { 'Content-Type': 'application/octet-stream' },
    }),
  installPluginDev: (dirPath: string) =>
    apiFetch<InstalledPlugin>('/api/v1/plugins/install-dev', {
      method: 'POST',
      body: JSON.stringify({ dirPath }),
    }),
  setPluginEnabled: (identity: string, enabled: boolean) =>
    apiFetch<unknown>(`/api/v1/plugins/${identity}/enabled`, {
      method: 'PUT',
      body: JSON.stringify({ enabled }),
    }),
  updatePluginSettings: (identity: string, entries: Record<string, string>) =>
    apiFetch<unknown>(`/api/v1/plugins/${identity}/settings`, {
      method: 'PUT',
      body: JSON.stringify(entries),
    }),
  uninstallPlugin: (identity: string) =>
    apiFetch<unknown>(`/api/v1/plugins/${identity}`, { method: 'DELETE' }),
  ignorePluginRetry: (taskId: string) =>
    apiFetch<unknown>(`/api/v1/tasks/${taskId}/ignore-plugin-retry`, { method: 'POST' }),

  listMarket: () => apiFetch<MarketEntry[]>('/api/v1/market'),
  installFromMarket: (pluginId: string) =>
    apiFetch<InstalledPlugin>('/api/v1/market/install', {
      method: 'POST',
      body: JSON.stringify({ pluginId }),
    }),

  getFfmpegStatus: () => apiFetch<ComponentFfmpegStatus>('/api/v1/components/ffmpeg'),
  getFfmpegVersions: () => apiFetch<ComponentVersions>('/api/v1/components/ffmpeg/versions'),
  installFfmpeg: (version?: string) =>
    apiFetch<unknown>('/api/v1/components/ffmpeg/install', {
      method: 'POST',
      body: JSON.stringify({ version } satisfies InstallFfmpegRequest),
    }),
  uninstallFfmpeg: () =>
    apiFetch<unknown>('/api/v1/components/ffmpeg/uninstall', { method: 'POST' }),

  getYtdlpStatus: () => apiFetch<ComponentYtdlpStatus>('/api/v1/components/ytdlp'),
  getYtdlpVersions: () => apiFetch<ComponentVersions>('/api/v1/components/ytdlp/versions'),
  installYtdlp: (version?: string) =>
    apiFetch<unknown>('/api/v1/components/ytdlp/install', {
      method: 'POST',
      body: JSON.stringify({ version } satisfies InstallFfmpegRequest),
    }),
  uninstallYtdlp: () =>
    apiFetch<unknown>('/api/v1/components/ytdlp/uninstall', { method: 'POST' }),
}

/** 「保存到本地」下载地址（浏览器导航下载，token 走查询参数）。 */
export function taskFileUrl(taskId: string): string {
  return `${getBase()}/api/v1/tasks/${taskId}/file?token=${encodeURIComponent(getToken())}`
}

/** 「导出日志」下载地址（浏览器导航下载 zip，token 走查询参数）。 */
export function logsExportUrl(): string {
  return `${getBase()}/api/v1/logs/export?token=${encodeURIComponent(getToken())}`
}
