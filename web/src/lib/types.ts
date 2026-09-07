// Wire 契约 —— 与 native/api `types.rs` + native/server `wire.rs` 一一对应（camelCase）。

/** 任务状态码：0=pending 1=downloading 2=paused 3=completed 4=error 5=preparing */
export type TaskStatus = 0 | 1 | 2 | 3 | 4 | 5

export interface TaskDto {
  taskId: string
  url: string
  fileName: string
  saveDir: string
  status: TaskStatus
  downloadedBytes: number
  totalBytes: number
  errorMessage: string
  /** Unix 秒时间戳（字符串） */
  createdAt: string
  proxyUrl: string
  queueId: string
  checksum: string
  /** 当前任务是否显式接受无效 HTTPS 证书 */
  ignoreTlsErrors: boolean
  /** 文件跟踪：completed 任务的目标文件是否已丢失（被删除/移动）。默认 false */
  fileMissing?: boolean
  /** Unix 秒时间戳（字符串），任务完成时刻；未完成为空串 */
  completedAt?: string
  /** 浏览器扩展捕获的来源页 URL（空 = 无） */
  referrer?: string
  /** 展示用的真实来源链接（空/缺省 = 用 `url`）。RSS 建的 .torrent 任务其 `url` 是
   *  引擎内部哨兵 `torrent-file://local`（种子字节在库里），对用户没有意义；
   *  「复制链接」一类展示点一律走 lib/format.ts 的 taskShareUrl()。 */
  originUrl?: string
  /** 所属任务组 ID（空 = 不属于任何组）；旧服务端可能缺省 */
  groupId?: string
  /** 队列内启动顺序（0 = 未显式排序，按创建时间；>0 = 显式顺序）；旧服务端可能缺省 */
  queueOrder?: number
  /** Auto 代理模式的路由标签（direct / direct:sampled / direct:pinned /
   *  direct:failover / proxy:cached / proxy:sampled / proxy:failover；代理类标签
   *  带 `:system`/`:manual` 来源后缀；空 = 非 Auto 模式）；旧服务端可能缺省 */
  autoRoute?: string
  /** BT 做种累计上传字节数（非 BT 恒 0）；旧服务端可能缺省 */
  uploadedBytes?: number
  /** 下载完成瞬间的累计上传基准（做种后分享率分子基线）；旧服务端可能缺省 */
  uploadedAtCompletion?: number
  /** 0=none, 1=做种中, 2=分享率达标, 3=时长达标, 4=手动停止, 5=已删除,
   *  6=会话释放, 7=不活跃达标, 8=排队做种；旧服务端可能缺省 */
  seedingStatus?: number
  /** 做种停止原因的人类可读描述（空 = 无） */
  seedingMessage?: string
  /** 引擎权威累计做种秒数（排队/暂停不计） */
  seedingTimeSecs?: number
  /** 任务级做种限制哨兵：-2=跟随全局，-1=不限制，>=0=自定义（分享率千分比，0 视同不限制） */
  seedRatioLimitMilli?: number
  seedPostRatioLimitMilli?: number
  /** 同上，单位分钟 */
  seedTimeLimitMinutes?: number
  seedInactiveTimeLimitMinutes?: number
  /** 任务级做种上传限速（B/s，0 = 不限），下次 torrent 挂载时生效 */
  seedUploadLimitBps?: number
}

/** 任务组行（多文件下载的纯逻辑聚合壳）。 */
export interface GroupDto {
  groupId: string
  name: string
  /** 原始分享/清单链接（展示/复制用） */
  sourceUrl: string
  /** 组根目录（子任务落盘 = 本值 + 相对路径） */
  saveDir: string
  /** Unix 秒时间戳（字符串） */
  createdAt: string
}

/** 建组请求的单个成员条目（预览响应勾选后的投影）。 */
export interface GroupItemRequest {
  /** 二段解析标识：`<itemId>` 或 `<itemId>@<variantId>` */
  resolverItem: string
  fileName: string
  /** 相对组根目录的子路径（空 = 组根） */
  relPath?: string
  /** 已知大小（字节，0 = 未知） */
  size?: number
}

/** 创建多文件任务组请求（POST /api/v1/groups）。items 不可为空。 */
export interface CreateGroupRequest {
  sourceUrl?: string
  groupName?: string
  saveDir?: string
  queueId?: string
  segments?: number
  cookies?: string
  referrer?: string
  userAgent?: string
  proxyUrl?: string
  extraHeaders?: Record<string, string>
  ignoreTlsErrors?: boolean
  /** 稍后下载：true = 建组后不启动 */
  startPaused?: boolean
  items: GroupItemRequest[]
}

export interface CreateGroupResponse {
  groupId: string
}

/** 前置预解析请求（POST /api/v1/resolve/preview，只读不建任务）。 */
export interface ResolvePreviewRequest {
  url: string
  cookies?: string
  referrer?: string
  userAgent?: string
  extraHeaders?: Record<string, string>
}

/**
 * 预解析结果。items 空且 error 空 = 插件未返回清单（回退普通创建）；
 * error 非空 = 预解析失败（同样回退，error 供提示）。
 */
export interface ResolvePreviewResponse {
  name: string
  sourceUrl: string
  error?: string
  items: PreviewItemDto[]
}

/** 清单条目。 */
export interface PreviewItemDto {
  /** 插件自定义标识，建组时拼进 resolverItem */
  id: string
  name: string
  /** 相对组根目录的子路径（空 = 根） */
  path: string
  /** 已知大小（字节），未知为 0 */
  size: number
  variants: PreviewVariantDto[]
}

/** 规格（画质/格式）。 */
export interface PreviewVariantDto {
  id: string
  label: string
  size: number
}

export interface QueueDto {
  queueId: string
  name: string
  speedLimitKbps: number
  /** 队列级 BT 上传限速 KB/s，0 = 不限制；已激活任务下次启动时生效。 */
  uploadLimitKbps: number
  maxConcurrent: number
  defaultSaveDir: string
  position: number
  defaultSegments: number
  defaultUserAgent: string
  /** 队列是否处于运行态；停止时队列内任务全部暂停，不会被调度器恢复。 */
  isRunning: boolean
  /** 是否启用每日定时启停 */
  scheduleEnabled: boolean
  /** 每日启动时间 "HH:MM"，为空表示未设置 */
  scheduleStart: string
  /** 每日停止时间 "HH:MM"，为空表示未设置 */
  scheduleStop: string
  /** 生效星期位掩码：bit0=周一 … bit6=周日，127=每天 */
  scheduleDays: number
}

// ---- RSS 订阅 ----

/**
 * 订阅源（`GET /api/v1/rss`）。字段与 native/api `RssSourceDto` 一一对应；
 * `lastFetchAt` 起的 7 个字段是引擎维护的运行态，提交时会被忽略。
 */
export interface RssSourceDto {
  sourceId: string
  url: string
  /** 空 = 用 feed 标题回填。 */
  name: string
  enabled: boolean
  /** false = 收集模式：只收集条目供手动挑选。 */
  autoDownload: boolean
  /** 自动创建的任务以 paused 落库。 */
  startPaused: boolean
  /** 空 = 内置主队列。 */
  queueId: string
  /** 空 = 队列目录 → 全局目录。 */
  saveDir: string
  /** 抓取间隔（分钟）；0 = 引擎默认 30。 */
  intervalMinutes: number
  /** 包含关键词（`|` = 或，空格 = 且；空 = 不过滤）。 */
  includePattern: string
  excludePattern: string
  useRegex: boolean
  smartEpisode: boolean
  /** 体积下限（字节，0 = 不限）。 */
  sizeMinBytes: number
  /** 体积上限（字节，0 = 不限）。 */
  sizeMaxBytes: number
  sendReferer: boolean
  notifyOnDownload: boolean
  /** 每轮最多新建任务数（1..=100）；0 = 引擎默认 20。 */
  maxPerFetch: number
  cookies: string
  userAgent: string
  proxyUrl: string
  /** 只读：上次发起抓取的 Unix 秒（0 = 从未）。 */
  lastFetchAt: number
  /** 只读：上次成功抓取的 Unix 秒（0 = 从未）。 */
  lastSuccessAt: number
  /** 只读：上次失败原因（空 = 健康）。 */
  lastError: string
  /** 只读：连续失败次数（驱动指数退避）。 */
  failCount: number
  /** 只读：首轮抓取是否已完成。 */
  seeded: boolean
  position: number
  /** 只读：未处理条目数（侧边栏 badge）。 */
  unreadCount: number
}

/** 条目状态码：0=新 1=已下载 2=已忽略 3=规则未命中 4=重复剧集 5=首轮历史条目 */
export type RssItemStatus = 0 | 1 | 2 | 3 | 4 | 5

/** 订阅流中的一个条目（`GET /api/v1/rss/{id}/items`）。 */
export interface RssItemDto {
  sourceId: string
  /** 去重主键。 */
  guid: string
  title: string
  link: string
  /** enclosure 直链（空 = 回退 `link`）。 */
  enclosureUrl: string
  /** enclosure 声明大小（字节，0 = 未知）。 */
  enclosureLength: number
  /** 发布时间（Unix 秒，0 = 未知）。 */
  pubDate: number
  fetchedAt: number
  status: RssItemStatus
  /** `status === 1` 时回链的任务 ID。 */
  taskId: string
  /** 智能剧集归一键（空 = 未识别）。 */
  episodeKey: string
  /** 稳定原因码（`not_included`/`excluded`/`too_small`/`too_large`/`dup_episode`/
   *  `seed_skipped`；空 = 无）。**展示前必须经 i18n 映射**，见 lib/rss-filter.ts。 */
  reason: string
}

/** 条目手动操作（`POST /api/v1/rss/{id}/items/action`）。guid 走请求体而非路径段：
 *  真实 feed 的 guid 常常就是一整条 URL，塞进路径会被反代规范化后静默改写。 */
export interface RssItemActionRequest {
  /** `action === 'readAll'` 时忽略。 */
  guid: string
  action: 'download' | 'ignore' | 'readAll'
}

/** 新建向导的 feed 验证请求（`POST /api/v1/rss/validate`，只读、不落库）。 */
export interface RssValidateRequest {
  url: string
  cookies?: string
  userAgent?: string
  proxyUrl?: string
}

/** feed 验证结果。`error` 非空即验证失败——HTTP 仍是 200，失败原因本身就是有效载荷。 */
export interface RssValidateResponse {
  url: string
  feedTitle: string
  items: RssItemDto[]
  error: string
}

export interface CreatedRssSource {
  sourceId: string
}

export interface CreateTaskRequest {
  url: string
  fileName?: string
  saveDir?: string
  segments?: number
  cookies?: string
  referrer?: string
  proxyUrl?: string
  userAgent?: string
  queueId?: string
  checksum?: string
  /** true = 仅为此任务忽略 HTTPS 证书错误；默认 false */
  ignoreTlsErrors?: boolean
  headers?: Record<string, string>
  /** true = 稍后下载：任务以 paused 状态创建，不自动启动 */
  startPaused?: boolean
  /** HTTP Basic 认证用户名；非空时引擎注入 Authorization 头 */
  httpUser?: string
  /** HTTP Basic 认证密码 */
  httpPassword?: string
  /** true = 按站点保存凭据，后续同站点任务自动套用 */
  saveSiteAuth?: boolean
}

export interface CreatedTask {
  taskId: string
}

export interface ApiInfo {
  name: string
  version: string
}

export interface PingInfo {
  success: boolean
  app: string
  version: string
  message: string
  /** 服务器默认语言（FLUXDOWN_LANG / config `web_language`），未配置时缺省。 */
  language?: string
}

export interface SegmentDetail {
  index: number
  startByte: number
  endByte: number
  downloadedBytes: number
}

export interface HlsQualityOption {
  index: number
  bandwidth: number
  width: number
  height: number
}

export interface BtFileEntry {
  index: number
  path: string
  size: number
}

export interface ResolveVariantOption {
  index: number
  label: string
  container: string
  bandwidth: number
  width: number
  height: number
  totalBytes: number
}

// ---- WS 服务端 → 客户端（tag = type） ----

export type WsServerMsg =
  | ({ type: 'taskProgress' } & TaskProgressMsg)
  | { type: 'tasksSnapshot'; tasks: TaskDto[] }
  | ({ type: 'segmentProgress' } & SegmentProgressMsg)
  | ({ type: 'segmentSplit' } & SegmentSplitMsg)
  | ({ type: 'taskCdnEvent' } & TaskCdnEventMsg)
  | { type: 'taskMetaProbed'; taskId: string; fileName: string; totalBytes: number }
  | { type: 'queuesChanged'; queues: QueueDto[] }
  | { type: 'groupsChanged'; groups: GroupDto[] }
  | { type: 'taskQueueChanged'; taskId: string; queueId: string }
  | { type: 'taskRouteChanged'; taskId: string; route: string }
  | { type: 'queuePositionsChanged'; positions: { taskId: string; position: number }[] }
  | { type: 'fileMissingChanged'; updates: { taskId: string; missing: boolean }[] }
  | { type: 'priorityTaskChanged'; priorityTaskId: string; autoPausedCount: number }
  | { type: 'hlsSelectionRequest'; taskId: string; options: HlsQualityOption[] }
  | { type: 'btSelectionRequest'; taskId: string; files: BtFileEntry[] }
  | { type: 'resolveVariantRequest'; taskId: string; defaultIndex: number; options: ResolveVariantOption[] }
  | { type: 'pluginsChanged' }
  | { type: 'pluginAutoDisabled'; identity: string; reason: string }
  | { type: 'duplicateTorrent'; taskId: string; existingTaskId: string; existingName: string }
  | { type: 'pluginHookActivity'; taskId: string; pluginId: string; running: boolean }
  | { type: 'componentProgress'; component: string; downloadedBytes: number; totalBytes: number }
  | { type: 'componentResult'; component: string; ok: boolean; message: string }
  | { type: 'linkIncomingPairing'; sessionId: string; sas: string; name: string; platform: string }
  | { type: 'linkDevicesChanged' }
  | { type: 'rssSourcesChanged'; sources: RssSourceDto[] }
  | { type: 'rssItemsChanged'; sourceId: string; items: RssItemDto[]; notifyTitles: string[] }
  | { type: 'webhookDeliveriesChanged'; deliveries: WebhookDelivery[] }
  | { type: 'pong' }

export interface TaskProgressMsg {
  taskId: string
  status: TaskStatus
  downloadedBytes: number
  totalBytes: number
  speed: number
  fileName: string
  saveDir: string
  url: string
  errorMessage: string
  /** BT 做种上传速率 B/s（server >= 本版新增；旧服务端缺省） */
  uploadSpeed?: number
  /** BT 做种累计上传字节数 */
  uploadedBytes?: number
  /** 做种状态码（语义同 TaskDto.seedingStatus） */
  seedingStatus?: number
  seedingMessage?: string
  /** 发帧时刻的累计做种秒数 */
  seedingTimeSecs?: number
}

export interface SegmentProgressMsg {
  taskId: string
  totalBytes: number
  segmentCount: number
  segments: SegmentDetail[]
}

export interface SegmentSplitMsg {
  taskId: string
  parentIndex: number
  parentNewEnd: number
  childIndex: number
  childStart: number
  childEnd: number
  isProactive: boolean
  totalSegments: number
}
/** 多 CDN 单节点描述（taskCdnEvent 载荷，见 server/src/wire.rs CdnNodeDto）。 */
export interface CdnNodeWire {
  /** 节点 IP；SYS 兜底节点（系统 DNS、无钉定）为 "SYS"。 */
  ip: string
  /** 候选来源："sys" / "doh:<端点IP>" / "ecs:<端点IP>"；SYS 为空串。 */
  origin: string
  /** 本任务经该节点下载的字节数（summary 有效，其余 0）。 */
  bytes: number
  /** EWMA 吞吐（B/s）：pool = 健康度先验（0 = 无先验）；summary = 实测。 */
  ewmaBps: number
  /** 当前未归还的段租约数（leases 快照有效，其余 0）。 */
  active: number
}

/** 多 CDN 并发下载的节点级活动事件（任务详情日志；语义见
 *  fluxdown_engine events.rs EngineEvent::TaskCdnEvent）。 */
export interface TaskCdnEventMsg {
  taskId: string
  /** "pool" | "kick" | "breaker" | "fallback" | "leases" | "summary" */
  kind: string
  host: string
  nodes: CdnNodeWire[]
  ip: string
  reason: string
  candidates: number
  alive: number
  cap: number
  autoCap: boolean
}

// ---- WS 客户端 → 服务端 ----

export type WsClientMsg =
  | { type: 'hlsSelection'; taskId: string; selectedIndex: number }
  | { type: 'btSelection'; taskId: string; selectedIndices: number[] }
  | { type: 'selectVariant'; taskId: string; selectedIndex: number }
  /** 任务级做种限制（哨兵 -2/-1/>=0；分享率千分比、时长分钟；uploadLimitBps 0=不限） */
  | {
      type: 'setTaskSeedLimits'
      taskId: string
      ratioLimitMilli: number
      postRatioLimitMilli: number
      seedTimeLimitMinutes: number
      inactiveTimeLimitMinutes: number
      uploadLimitBps: number
    }
  | { type: 'ping' }

// ---- 扩展 REST ----

export interface ProxyTestRequest {
  proxyType: string
  host: string
  port: string
  username?: string
  password?: string
}

export interface ProxyTestResponse {
  latencyMs: number
}

export interface TrackerSubRefreshResponse {
  success: boolean
  trackerCount: number
  okSources: number
  totalSources: number
  updatedAt: number
  error: string
}

/** eD2K 服务器订阅刷新结果（`POST /api/v1/ed2k/server-sub/refresh`）。 */
export interface Ed2kServerSubRefreshResponse {
  success: boolean
  serverCount: number
  okSources: number
  totalSources: number
  updatedAt: number
  error: string
}

export interface CreateQueueRequest {
  name: string
  speedLimitKbps?: number
  uploadLimitKbps?: number
  maxConcurrent?: number
  defaultSaveDir?: string
  defaultSegments?: number
  defaultUserAgent?: string
}

export interface QueueScheduleRequest {
  enabled: boolean
  startTime: string
  stopTime: string
  days: number
}

export interface QueueOrderRequest {
  taskIds: string[]
}

export interface FsEntry {
  name: string
  path: string
}

export interface FsListResponse {
  path: string
  parent: string | null
  dirs: FsEntry[]
}

export interface StatsResponse {
  diskFreeBytes: number | null
  saveDir: string
  serverVersion: string
  wsClients: number
  /** 演示模式开关（服务器以 FLUXDOWN_DEMO_URL 启动时为 true）。 */
  demoMode: boolean
  /** 演示模式下唯一允许下载的 URL；非演示模式为空串。 */
  demoUrl: string
}

export interface TokenResponse {
  token: string
  note: string
}

/** 首次运行状态（`GET /api/v1/setup/status`，无鉴权）。 */
export interface SetupStatus {
  /** true = 服务器尚未设置访问密钥，应展示首次运行向导而非登录框。 */
  setupRequired: boolean
  /** 服务器侧要求的最短长度（与 token-policy.ts 常量同源，用于校对）。 */
  minLength: number
}

export interface LogFileDto {
  name: string
  size: number
}

export interface LogsResponse {
  /** 日志目录绝对路径（服务器文件系统）。 */
  dir: string
  files: LogFileDto[]
  /** Rust 日志 writer 是否已成功初始化。 */
  initialized: boolean
  /** 本次进程生命周期内是否发生过日志基础设施失败。 */
  degraded: boolean
  /** 本次进程生命周期内累计日志基础设施失败次数。 */
  failureCount: number
  /** 最近一次日志基础设施失败；无失败时为 null。 */
  lastError: string | null
}

// ---- 组件（ffmpeg / yt-dlp） ----

/** ffmpeg 路径来源：manual=手动指定 managed=托管安装 system=系统 PATH none=未找到。 */
export type FfmpegSource = 'manual' | 'managed' | 'system' | 'none'

export interface ComponentFfmpegStatus {
  source: FfmpegSource
  /** 当前平台是否提供托管安装（macOS 等为 false）。 */
  managedSupported: boolean
  path: string
  version: string
  managedVersion: string
  systemPath: string
}

export interface ComponentYtdlpStatus {
  source: FfmpegSource
  /** 当前平台是否提供托管安装（yt-dlp 全平台均支持，通常为 true）。 */
  managedSupported: boolean
  path: string
  version: string
  managedVersion: string
  systemPath: string
}

export interface ComponentVersions {
  versions: string[]
  latestStable: string
}

export interface InstallFfmpegRequest {
  version?: string
}

export type ConfigMap = Record<string, string>

// ---- 插件系统 ----

export type SettingValueType = 'string' | 'number' | 'boolean'
export type SettingWidget = 'text' | 'password' | 'textarea' | 'select' | 'toggle' | 'number' | 'folder'
export type PluginDisabledReason = 'None' | 'Manual' | 'CircuitBreaker'

export interface SettingOptionDto {
  value: string
  label: string
}

export interface SettingFieldDto {
  key: string
  title: string
  description: string
  type: SettingValueType
  widget: SettingWidget
  options: SettingOptionDto[]
  default: string | null
  required: boolean
  min: number | null
  max: number | null
  pattern: string | null
  /** 辅助脚本（非空时字段旁渲染复制按钮，仅复制文本、绝不执行）。旧服务端可能缺省。 */
  helperScript?: string | null
  /** 辅助脚本按钮文案（空则用默认文案）。 */
  helperLabel?: string | null
}

export interface PluginDto {
  identity: string
  name: string
  version: string
  description: string
  homepage: string
  enabled: boolean
  devMode: boolean
  disabledReason: PluginDisabledReason
  settings: SettingFieldDto[]
  settingsValues: Record<string, string>
  /** manifest 声明的能力权限（如 ["ffmpeg"]），旧服务端可能缺省。 */
  permissions?: string[]
}

export interface InstalledPlugin {
  identity: string
  /** 插件声明权限所需但尚未安装的基础组件（"ffmpeg"/"ytdlp"），提醒式。 */
  missingComponents?: string[]
}

/** 插件市场索引条目（浏览/安装用）。yanked 值域：none/deprecated/vulnerable/malicious。 */
export interface MarketEntry {
  pluginId: string
  version: string
  sequence: number
  contentHash: string
  minAppVersion: string
  name: string
  description: string
  author: string
  homepage: string
  mirrors: string[]
  publishTime: string
  yanked: string
  tags: string[]
  /** manifest 声明的能力权限（如 ["ffmpeg"]），旧索引可能缺省。 */
  permissions?: string[]
}

// ── Webhook 任务事件推送（免费自托管 BYOE）──────────────────────────

/** 端点配置。**schema 与 Rust `webhook::EndpointSpec`、Dart `WebhookEndpoint`
 *  三方一致**——整表以 JSON 数组存进 config 键 `webhook.endpoints`。 */
export interface WebhookEndpoint {
  id: string
  name: string
  /** ntfy/gotify/bark/serverchan/telegram/discord/slack/custom；未知值按 custom 处理。 */
  preset: string
  url: string
  enabled: boolean
  /** 订阅的事件 wire 名；空数组 = 不推送任何事件。 */
  events: string[]
  /** 队列过滤：空 = 全部队列。 */
  queueId: string
  headers: Record<string, string>
  /** 空 = 用预设默认模板。 */
  bodyTemplate: string
  /** 非空 = 开启 HMAC-SHA256 签名。 */
  signSecret: string
  allowHttp: boolean
  useProxy: boolean
}

/** 一条投递记录（内存环形缓冲 100 条，不落盘）。 */
export interface WebhookDelivery {
  deliveryId: string
  timestampMs: number
  event: string
  endpointId: string
  endpointName: string
  url: string
  /** 每行 `K: V`；鉴权类值已掩码。 */
  requestHeaders: string
  requestBody: string
  /** 0 = 未拿到响应（网络错误/超时）。 */
  statusCode: number
  responseBody: string
  latencyMs: number
  attempts: number
  success: boolean
  error: string
}

/** 服务预设元数据（引擎是模板的单一事实源）。 */
export interface WebhookPreset {
  id: string
  label: string
  urlPlaceholder: string
  /** 空 = custom，走 schemaVersion 信封。 */
  defaultTemplate: string
  contentType: string
}

export interface WebhookDeliveriesResponse {
  deliveries: WebhookDelivery[]
  presets: WebhookPreset[]
  /** 可用占位符清单（`{task.fileName}` 等）。 */
  variables: string[]
}

export interface WebhookTestResponse {
  success: boolean
  statusCode: number
  latencyMs: number
  error: string
}

export interface WebhookSimulateResponse {
  /** 按订阅规则投出去的端点数。0 = 没有目标订阅 `task.completed`。 */
  dispatched: number
}

/** v1 事件集（wire 名即契约）。 */
export const WEBHOOK_EVENTS = [
  'task.created',
  'task.started',
  'task.completed',
  'task.failed',
  'task.paused',
  'queue.drained',
] as const

/** 新端点默认订阅：完成 + 失败覆盖 80% 场景。 */
export const WEBHOOK_DEFAULT_EVENTS = ['task.completed', 'task.failed']
