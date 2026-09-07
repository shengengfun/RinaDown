// FluxCloud 云账户 —— Wire 契约 v1（camelCase，见 FluxCloud/server），与本地下载器
// api.ts 的 types.ts 完全独立，互不引用。

/** 用户状态：0=active 1=disabled 2=pending（待邮箱验证）。 */
export type CloudUserStatus = 'active' | 'disabled' | 'pending'

export interface CloudUser {
  id: string
  email: string
  nickname: string
  plan: string
  status: CloudUserStatus
  /** Origin ID(v1.2 新增):类 QQ 号唯一数字身份,从 10001 起严格递增;pending 用户为 null。 */
  originId: number | null
  /** 是否已用掉套餐赠送的那一次 Origin ID 自助修改机会(v1.3 新增,见 originIdEdit
   *  entitlement)。旧会话快照(登录早于此字段上线)反序列化后为 undefined,按“未修改”
   *  兜底,语义上与 false 等价。 */
  originIdChanged?: boolean
  createdAt: string
  lastLoginAt?: string
  /** 当前套餐下的会员编号（v1.4）：仅当套餐 badgeNumbered=true 且已分配时非空。 */
  membershipOrdinal?: number | null
}

/** 套餐能力集：服务端自由演进字段，本文件只按需声明已知字段，未知字段仍可原样读取。 */
export interface Entitlements {
  maxSyncDevices?: number
  /** 套餐是否允许自助修改 Origin ID(v1.3 新增)，配合 CloudUser.originIdChanged
   *  共同决定编辑入口是否展示。 */
  originIdEdit?: boolean
  [key: string]: unknown
}

/** 受信任设备（DeviceDto，v1.1 增补 lastIp/appVersion，均可空；v1.3 增补 isOnline/isCurrent，
 *  多设备协同用，见 mdc §1.2）。 */
export interface CloudDevice {
  id: string
  deviceId: string
  name: string
  platform?: string
  /** 最近登录 IP，服务端按 X-Forwarded-For/X-Real-IP 记录，可能为空。 */
  lastIp?: string
  /** 客户端版本号，登录/信任设备时上报，可能为空（如旧版客户端未上报）。 */
  appVersion?: string
  createdAt: string
  lastSeenAt: string
  /** 该设备当前是否有活跃 SSE 连接（服务端 PresenceRegistry 判定）。 */
  isOnline?: boolean
  /** 是否为发起本次请求的设备（服务端按请求头 deviceId 比对）。 */
  isCurrent?: boolean
}

/** 登录/注册验证/验证码登录 成功后的统一响应。 */
export interface AuthResponse {
  accessToken: string
  refreshToken: string
  expiresIn: number
  user: CloudUser
  entitlements: Entitlements
  /** 当前套餐展示快照；套餐下架后仍返回。 */
  currentPlan?: CatalogPlan | null
  device: CloudDevice
}

/** POST /auth/login 的 tagged 响应：设备已受信任直接下发令牌，新设备则要求邮箱验证码。 */
export type LoginResult =
  | { status: 'ok'; auth: AuthResponse }
  | { status: 'deviceVerificationRequired'; ttlSeconds: number }

/** GET /me 响应：UserDto 字段打平 + entitlements。 */
export interface CloudProfile extends CloudUser {
  entitlements: Entitlements
  /** 当前套餐展示快照；套餐下架后仍返回。 */
  currentPlan?: CatalogPlan | null
}

/** GET /me/origin-id/random 响应：套餐允许时给出的建议 Origin ID(倾向"豹子号"，不锁定)。 */
export interface RandomOriginIdResponse {
  originId: number
}

/** GET /me/origin-id/check 响应：提交前可用性预检。reason 仅在 available=false 时有意义。 */
export interface CheckOriginIdResponse {
  available: boolean
  reason: 'invalid' | 'taken' | null
}

/** POST /auth/register、/auth/code/send 等发码接口的响应。 */
export interface TtlResponse {
  ttlSeconds: number
}
/** GET /plans/catalog 响应元素（公开无鉴权，仅取账户页徽标渲染所需字段；
 *  未声明字段原样保留在对象上，落盘快照时一并存储）。 */
export interface CatalogPlan {
  code: string
  name: string
  badge?: string | null
  /** outline | solid | medal | ribbon（服务端 admin_plans.rs::BADGE_STYLES 白名单）。 */
  badgeStyle: string
  /** #RRGGBB 徽标专用强调色。 */
  badgeColor: string
  badgeNumbered?: boolean
  badgeNumberDigits?: number
}

/** GET /devices 响应。 */
export interface DevicesResponse {
  devices: CloudDevice[]
}

/** 跨设备任务状态机（cross_device_tasks.status，见 mdc §1.1）。 */
export type RemoteTaskStatus = 'pending' | 'accepted' | 'downloading' | 'paused' | 'completed' | 'failed' | 'canceled'

/** 跨设备任务（RemoteTaskDto）：downloadedBytes/speed/progress 来自服务端内存快照（无则 0），
 *  绝不落库、绝不轮询，靠 `GET /tasks/events` SSE 增量回流（见 mdc §1.3/§1.5）。 */
export interface RemoteTask {
  id: string
  fromDevice: string
  toDevice: string
  url: string
  saveDir?: string
  fileName: string
  status: RemoteTaskStatus
  totalBytes?: number
  downloadedBytes: number
  speed: number
  progress: number
  error?: string
  createdAt: string
  updatedAt: string
}

/** GET /tasks/remote 响应。 */
export interface RemoteTasksResponse {
  tasks: RemoteTask[]
}

/** POST /tasks/{id}/status 请求体：执行端上报状态转换（totalBytes/fileName 探测到才带，
 *  error 仅 failed 时有意义）。 */
export interface TaskStatusReport {
  status: RemoteTaskStatus
  totalBytes?: number
  fileName?: string
  error?: string
}

/** POST /tasks/progress 请求体 items[] 单项：执行端批量上报进度（服务端只更内存快照）。 */
export interface ProgressReportItem {
  taskId: string
  downloadedBytes: number
  speed: number
  progress: number
}

/** POST /tasks/{id}/command 的动作集：发起端对执行端的远程控制。 */
export type RemoteTaskAction = 'pause' | 'resume' | 'cancel'
/** GET /cdn/config 响应 resolvers[]（snake_case wire，直接对应引擎 config 表键约定，
 *  与本文件其余 camelCase 模型不同——对齐桌面端 cloud_models.dart CdnConfig）。 */
export interface CdnResolverEntry {
  url: string
  ecs: boolean
}

/** GET /cdn/config 响应 ecs_subnets[]：resolver ECS 查询的地域先验，客户端只消费 subnet。 */
export interface CdnEcsSubnetEntry {
  region: string
  isp: string
  subnet: string
}

/** GET /cdn/config 响应：CDN 多节点聚合下载云端配置快照（P1 §四 + P2 §五契约）。
 *  云端只下发先验，不做套餐门控——聚合开关与节点数上限均为客户端本地设置。 */
export interface CdnConfig {
  revision: number
  resolvers: CdnResolverEntry[]
  ecs_subnets: CdnEcsSubnetEntry[]
}

/** fetchCdnConfig 结果：304 命中时 notModified=true，etag/config 均为 null。 */
export interface CdnConfigResult {
  notModified: boolean
  etag: string | null
  config: CdnConfig | null
}

/** GET /referral/summary 响应 rules[]：仅含 enabled、price_minor>0 且参与推介返利的
 *  套餐(按解析后的生效值，override 已回落 default；被覆盖档排除的套餐不下发)。 */
export interface ReferralRule {
  planCode: string
  planName: string
  priceMinor: number
  discountMinor: number
  rewardPercent: number
}

/** GET /referral/summary 响应：推介有奖总览。enabled=false 时其余字段仍照常返回；
 *  rewardEnabled=false 时该用户的码仍给买家优惠、仍记归因，但永不产生返利台账行——
 *  客户端据此隐藏金额，只展示邀请人数。description 为 admin 后台可编辑的说明文案
 *  （可空串，为空时客户端回退展示 rules 自动列表）；推荐码改由独立的
 *  GET /referral/codes 管理，不再随本接口下发。 */
export interface ReferralSummary {
  enabled: boolean
  description: string
  rewardEnabled: boolean
  contact: string
  invitedCount: number
  pendingRewardMinor: number
  paidRewardMinor: number
  totalRewardMinor: number
  rules: ReferralRule[]
}

/** GET /referral/codes 响应 items[] 单个推荐码；paidOrderCount/rewardMinor 为该码文本
 *  快照命中的本人已付订单数、与对应返利台账合计(pending+paid，排除 revoked)。 */
export interface ReferralCode {
  id: string
  code: string
  paidOrderCount: number
  rewardMinor: number
  createdAt: string
}

/** GET /referral/codes 响应：本人名下全部推荐码，按创建时间升序，分页（每用户码上限
 *  仍为 10，分页主要为未来扩容留口子）。 */
export interface ReferralCodesResponse {
  total: number
  items: ReferralCode[]
}

/** referral_rewards.status：pending 待兑付 / paid 已兑付(人工标记) / revoked 已撤销。 */
export type ReferralRecordStatus = 'pending' | 'paid' | 'revoked'

/** GET /referral/records 响应 items[] 单条返利台账行；buyerLabel 服务端已脱敏
 *  (昵称优先，否则邮箱首字符+***+@域名)；referralCode 为该笔订单使用的推荐码文本快照。 */
export interface ReferralRecord {
  id: string
  buyerLabel: string
  orderAmountMinor: number
  rewardMinor: number
  rewardPercent: number
  status: ReferralRecordStatus
  createdAt: string
  paidAt: string | null
  referralCode: string | null
}

/** GET /referral/records 响应：分页返利台账。 */
export interface ReferralRecordsResponse {
  total: number
  items: ReferralRecord[]
}

/** 服务端错误统一形态 `{code, message}`，附带 HTTP 状态码方便按 code/status 分支处理。 */
export class CloudApiError extends Error {
  code: string
  status: number
  constructor(code: string, message: string, status: number) {
    super(message)
    this.code = code
    this.status = status
  }
}
