//! 纯下载 daemon 的传输无关 DTO。

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::Value;
/// 内置主队列的稳定 wire ID。
pub const MAIN_QUEUE_ID: &str = "main";
/// 内置“稍后下载”队列的稳定 wire ID。
pub const LATER_QUEUE_ID: &str = "later";

/// 交互选择的种类。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SelectionKind {
    Hls {
        options: Vec<HlsQualityOptionDto>,
    },
    Bt {
        files: Vec<BtFileDto>,
    },
    Variant {
        options: Vec<ResolveVariantOptionDto>,
    },
}

/// 引擎可接受的类型化选择结果。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SelectionOutcome {
    Hls { index: i32 },
    Bt { indices: Vec<i32> },
    Variant { index: i32 },
    Cancelled,
}

/// daemon 向已订阅连接发布的待选择请求。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct SelectionRequestDto {
    pub request_id: String,
    pub task_id: String,
    pub kind: SelectionKind,
    pub default_choice: SelectionOutcome,
    pub deadline_unix_ms: i64,
}

/// 客户端对待选择请求的终局答复。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct SelectionResolutionDto {
    pub request_id: String,
    pub outcome: SelectionOutcome,
}

/// 原子 daemon 配置投影。
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct DaemonConfigSnapshot {
    pub revision: u64,
    pub values: BTreeMap<String, String>,
}

/// 带乐观并发版本的 daemon 配置补丁。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct DaemonConfigPatch {
    pub expected_revision: u64,
    pub values: BTreeMap<String, String>,
}

/// daemon 运行状态投影。
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct DaemonRuntimeStatsDto {
    pub active_tasks: u32,
    pub pending_tasks: u32,
    pub total_download_bps: i64,
    pub total_upload_bps: i64,
    pub disk_free_bytes: Option<u64>,
    pub save_dir: String,
}

/// daemon 托管组件标识。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum ComponentKind {
    Ffmpeg,
    Ytdlp,
}

/// 查询或卸载一个托管组件。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ComponentParams {
    pub component: ComponentKind,
}

/// 安装或更新一个托管组件。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ComponentInstallParams {
    pub component: ComponentKind,
    #[serde(default)]
    pub version: Option<String>,
}

/// daemon 受管组件的类型化状态。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "component", content = "status", rename_all = "camelCase")]
pub enum ComponentStatusDto {
    Ffmpeg(ComponentFfmpegStatus),
    Ytdlp(ComponentYtdlpStatus),
}

/// `daemon.task.create` 参数。大种子文件通过一次性 blob 引用传递。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct DaemonCreateTaskParams {
    pub request: CreateTaskRequest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub torrent_blob_id: Option<String>,
    #[serde(default)]
    pub unattended: bool,
}

/// CDN 样本的持久化租约。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CdnReportLeaseDto {
    pub batch_id: String,
    pub samples: Vec<Value>,
}

/// 确认一个 CDN 样本租约。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CdnReportAckParams {
    pub batch_id: String,
}

/// 云端 CDN 先验配置经 agent 专用边界下发的可写键。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CdnConfigApplyParams {
    pub values: BTreeMap<String, String>,
}

/// 旧宿主设备链接数据的一次性迁移结果。敏感内容只允许在鉴权 RPC 内传输。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LinkMigrationExport {
    pub revision: u64,
    pub identity: Value,
    pub roster: Vec<Value>,
}

/// 旧宿主 UI Gateway 设置的一次性迁移结果。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GatewayMigrationExport {
    pub revision: u64,
    pub takeover_enabled: bool,
    pub jsonrpc_enabled: bool,
    pub api_enabled: bool,
    pub mcp_enabled: bool,
    pub cors_enabled: bool,
    pub user_token_configured: bool,
    /// 仅在鉴权迁移 RPC 中出现；不进入任何 snapshot/event。
    pub user_token: String,
}

/// 确认已持久化的一次性迁移版本。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct MigrationAckParams {
    pub revision: u64,
}

// ---------------------------------------------------------------------------
// 外部下载请求（浏览器扩展 NMH / 油猴脚本接管 / aria2 兼容层共用）
// ---------------------------------------------------------------------------

/// 浏览器原始请求体（form POST / XHR raw body 等）。
///
/// 当用户在 form-submit 触发的下载中点击下载按钮时，浏览器实际发起的是
/// POST 请求并携带表单数据；扩展通过 `webRequest.onBeforeRequest` 抓到 method
/// 与 body 后透传到此字段。宿主端按 `kind` 重建请求体。
///
/// 协议字段：
/// - `formData`：来自 `requestBody.formData`，宿主用 `reqwest::form()` 编码为
///   `application/x-www-form-urlencoded`
/// - `urlencoded`：扩展端已序列化好的 url-encoded 字符串（直接作为 body 发送）
/// - `raw`：base64 编码的二进制 body（XHR / fetch 直接发送 ArrayBuffer 的场景）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RequestBody {
    FormData {
        fields: HashMap<String, Vec<String>>,
    },
    Urlencoded {
        raw: String,
    },
    Raw {
        #[serde(rename = "bytesB64")]
        bytes_b64: String,
        #[serde(rename = "contentType", default)]
        content_type: Option<String>,
    },
}

/// 外部下载请求载荷（浏览器扩展 / 油猴脚本 / aria2 兼容层）。
///
/// 由宿主的「外部下载」通道消费：缓存请求事务 → 弹出快速下载确认框 →
/// 用户确认后创建任务。与管理 API 的 [`CreateTaskRequest`]（直接建任务、
/// 无确认框）语义不同。
///
/// # Examples
///
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DownloadRequest {
    pub url: String,
    #[serde(default)]
    pub filename: String,
    /// 保存目录（aria2 `dir` 选项 / 接管请求 `saveDir` 字段）。
    /// 空 = 由宿主按分类匹配 / 默认目录决定。
    #[serde(rename = "saveDir")]
    #[serde(default)]
    pub save_dir: String,
    #[serde(default)]
    pub referrer: String,
    #[serde(default)]
    pub cookies: String,
    /// 浏览器请求中捕获的额外 HTTP 头（如 Authorization）。
    /// 由下载引擎在发起请求时附加到请求头中。
    #[serde(default)]
    pub headers: Option<HashMap<String, String>>,
    /// 文件大小提示（字节）。
    ///   - `>0` = 已知大小，跳过 probe
    ///   - `-1` = 大小未知但确认是下载资源（webRequest 嗅探），跳过 probe
    ///   - `0` / `None` = 正常 probe
    #[serde(rename = "fileSize")]
    #[serde(default)]
    pub file_size: Option<i64>,
    #[serde(rename = "mimeType")]
    #[serde(default)]
    pub mime_type: Option<String>,
    /// 浏览器原始请求方法（"GET" / "POST" / ...）。
    /// 缺省 = "GET"。POST/PUT/PATCH 类请求由 `body` 携带请求体。
    #[serde(default)]
    pub method: Option<String>,
    /// 浏览器原始请求体（仅在非 GET 时有意义）。
    #[serde(default)]
    pub body: Option<RequestBody>,
    /// 音频轨 URL（可选，通用「视频轨+音频轨」离散下载对语义，按 MIME
    /// video/* vs audio/* 分轨判定，非站点专用协议字段）。
    /// 非空 = 这是一对轨道，引擎分别下载两路后用 ffmpeg mux 合并；
    /// 空/缺省 = 普通单 URL 下载。
    #[serde(rename = "audioUrl", default)]
    pub audio_url: Option<String>,
}

// ---------------------------------------------------------------------------
// 管理 API（/api/v1）资源类型
// ---------------------------------------------------------------------------

/// 任务信息（`GET /api/v1/tasks`、`GET /api/v1/tasks/{id}` 响应）。
///
/// # Examples
///
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct TaskDto {
    pub task_id: String,
    pub url: String,
    pub file_name: String,
    pub save_dir: String,
    /// 0=pending, 1=downloading, 2=paused, 3=completed, 4=error, 5=preparing
    pub status: i32,
    pub downloaded_bytes: i64,
    pub total_bytes: i64,
    pub error_message: String,
    /// Unix 秒级时间戳（字符串）。
    pub created_at: String,
    /// 单任务代理 URL（空 = 使用全局代理）。
    pub proxy_url: String,
    /// 命名队列 ID（空 = 默认队列）。
    pub queue_id: String,
    /// Checksum spec，格式 `algo=hexhash`（空 = 跳过校验）。
    pub checksum: String,
    /// 是否显式忽略 HTTPS 证书错误。默认 false（严格验证）。
    #[serde(default)]
    pub ignore_tls_errors: bool,
    /// 文件跟踪：completed 任务的目标文件是否已丢失（被删除/移动）。默认 false。
    #[serde(default)]
    pub file_missing: bool,
    /// 任务结束时间，Unix 秒级时间戳（空 = 尚未完成）。
    /// 记录下载真正完成（status→3）的时刻，不含插件 hook 后处理耗时。
    #[serde(default)]
    pub completed_at: String,
    /// Source page URL captured by the browser extension (empty = none).
    #[serde(default)]
    pub referrer: String,
    /// 所属任务组 ID（空 = 不属于任何组）。
    #[serde(default)]
    pub group_id: String,
    /// 由哪条 RSS 订阅自动创建（空 = 非 RSS 来源）。P5 任务溯源。
    #[serde(default)]
    pub rss_source_id: String,
    /// 展示用原始来源链接（空 = 用 `url`）。`.torrent` 任务的 `url` 是
    /// `torrent-file://local` 哨兵，客户端「复制链接」应优先取本字段。
    #[serde(default)]
    pub origin_url: String,
    /// `ProxyMode::Auto` 的任务级最终链路（可追溯性）：`direct` /
    /// `direct:sampled` / `direct:pinned` / `direct:failover` /
    /// `proxy:cached` / `proxy:sampled` / `proxy:failover`（代理类标签带
    /// 候选来源后缀 `:system`/`:manual`）；空 = 非 Auto 模式。
    #[serde(default)]
    pub auto_route: String,
    /// 队列内启动顺序（0 = 未显式排序，按创建时间；>0 = 显式顺序）。
    #[serde(default)]
    pub queue_order: i32,
    /// BT 已上传字节数（做种累计，非 BT 任务恒 0）。
    #[serde(default)]
    pub uploaded_bytes: i64,
    /// 下载完成时刻的已上传字节数（做种后分享率基准，非 BT 任务恒 0）。
    #[serde(default)]
    pub uploaded_at_completion: i64,
    /// BT 做种状态：0=无, 1=做种中, 2=达分享率, 3=达时长, 4=用户停止,
    /// 5=任务删除, 6=会话释放, 7=不活跃停止, 8=排队等待做种槽。
    #[serde(default)]
    pub seeding_status: i32,
    /// 做种状态辅助说明（如停止原因，空 = 无）。
    #[serde(default)]
    pub seeding_message: String,
    /// 累计做种秒数（活跃做种期间累加；排队/暂停不计，非 BT 任务恒 0）。
    #[serde(default)]
    pub seeding_time_secs: i64,
    /// 任务级总分享率上限（千分比，1500 = 1.5）。哨兵：-2 = 跟随全局，
    /// -1 = 不限制，>=0 = 自定义（0 视同不限制）。
    #[serde(default = "default_seed_limit_inherit")]
    pub seed_ratio_limit_milli: i64,
    /// 任务级做种后分享率上限（千分比）。哨兵语义同上。
    #[serde(default = "default_seed_limit_inherit")]
    pub seed_post_ratio_limit_milli: i64,
    /// 任务级做种时长上限（分钟）。哨兵语义同上。
    #[serde(default = "default_seed_limit_inherit")]
    pub seed_time_limit_minutes: i64,
    /// 任务级不活跃做种时长上限（分钟）。哨兵语义同上。
    #[serde(default = "default_seed_limit_inherit")]
    pub seed_inactive_time_limit_minutes: i64,
}

/// 命名队列信息（`GET /api/v1/queues` 响应）。
///
/// # Examples
///
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct QueueDto {
    pub queue_id: String,
    pub name: String,
    /// 队列限速（KB/s），0 = 不限速。
    pub speed_limit_kbps: i64,
    /// 队列上传限速（KB/s），0 = 不限速。
    #[serde(default)]
    pub upload_limit_kbps: i64,
    /// 队列并发上限，0 = 跟随全局。
    pub max_concurrent: i32,
    pub default_save_dir: String,
    pub position: i32,
    pub default_segments: i32,
    pub default_user_agent: String,
    /// 队列运行状态：停止的队列不自动启动其中任务。
    #[serde(default = "default_true")]
    pub is_running: bool,
    /// 每日定时计划是否启用。
    #[serde(default)]
    pub schedule_enabled: bool,
    /// 每日定时启动时间 `HH:MM`（空 = 不定时启动）。
    #[serde(default)]
    pub schedule_start: String,
    /// 每日定时停止时间 `HH:MM`（空 = 不定时停止）。
    #[serde(default)]
    pub schedule_stop: String,
    /// 定时生效星期位掩码：bit0=周一 … bit6=周日；127 = 每天。
    #[serde(default = "default_schedule_days")]
    pub schedule_days: i32,
}

/// 创建任务请求（`POST /api/v1/tasks`）。
///
/// 与外部下载请求 [`DownloadRequest`] 不同：本请求**直接创建任务**，
/// 不经过快速下载确认弹框（管理 API 的调用方是受信任的自动化客户端）。
///
/// # Examples
///
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskRequest {
    pub url: String,
    /// 空 = 从 URL / Content-Disposition 推断。
    #[serde(default)]
    pub file_name: String,
    /// 空 = 使用全局默认保存目录。
    #[serde(default)]
    pub save_dir: String,
    /// 0 = 由 segment_advisor 按文件大小动态决定。
    #[serde(default)]
    pub segments: i32,
    #[serde(default)]
    pub cookies: String,
    #[serde(default)]
    pub referrer: String,
    /// 单任务代理 URL（空 = 使用全局代理）。
    #[serde(default)]
    pub proxy_url: String,
    /// 空 = 使用全局 User-Agent。
    #[serde(default)]
    pub user_agent: String,
    /// 命名队列 ID（空 = 默认队列）。
    #[serde(default)]
    pub queue_id: String,
    /// Checksum spec，格式 `algo=hexhash`（空 = 跳过校验）。
    #[serde(default)]
    pub checksum: String,
    /// 忽略 HTTPS 证书错误。缺省 false（严格验证）。
    #[serde(default)]
    pub ignore_tls_errors: bool,
    /// 附加 HTTP 请求头。
    #[serde(default)]
    pub headers: Option<HashMap<String, String>>,
    /// BT 种子文件字节（base64 编码，aria2 `addTorrent` 兼容入口）。
    /// 非空时按种子任务创建，`url` 允许为空占位。
    #[serde(default)]
    pub torrent_b64: Option<String>,
    /// 浏览器原始 HTTP method（`"GET"`/`"POST"`/…）。缺省 = GET。
    /// form-POST 触发的下载必须携带，否则引擎用 GET 重发会拿到错误内容。
    #[serde(default)]
    pub method: Option<String>,
    /// 浏览器原始请求体（仅非 GET 时有意义）。
    #[serde(default)]
    pub body: Option<RequestBody>,
    /// 音频轨 URL（「视频轨+音频轨」离散下载对语义）。
    /// 非空 = 引擎分别下载两路后 mux 合并；空/缺省 = 普通单 URL 下载。
    #[serde(default)]
    pub audio_url: Option<String>,
    /// 稍后下载：true = 建任务后不启动（paused 落库），待「启动队列」
    /// 按序恢复或用户手动恢复。缺省 false = 立即开始。
    #[serde(default)]
    pub start_paused: bool,
    /// HTTP Basic 认证用户名。非空时引擎生成 `Authorization: Basic` 头
    /// 注入请求（覆盖 `headers` 中的同名头）。空 = 未提供，若该站点有
    /// 已保存凭据则自动套用。
    #[serde(default)]
    pub http_user: String,
    /// HTTP Basic 认证密码（仅 `httpUser` 非空时有意义，允许为空串）。
    #[serde(default)]
    pub http_password: String,
    /// 为此网站保存凭据：true 且 `httpUser` 非空时按站点（host[:port]）
    /// 持久化，供后续同站点任务自动套用。
    #[serde(default)]
    pub save_site_auth: bool,
}

fn default_true() -> bool {
    true
}

fn default_schedule_days() -> i32 {
    127
}

/// 任务级做种限制的反序列化默认值：-2 = 跟随全局（缺字段不得落到 0=自定义）。
fn default_seed_limit_inherit() -> i64 {
    -2
}

/// 创建任务响应（`POST /api/v1/tasks`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CreatedTask {
    pub task_id: String,
}

/// 应用信息（`GET /api/v1/info` 响应）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ApiInfo {
    pub name: String,
    pub version: String,
}

/// 通用结果响应（接管端点应答 / 各端点错误响应统一格式）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ResultMessage {
    pub success: bool,
    pub message: String,
}

// ---------------------------------------------------------------------------
// 插件系统 DTO（camelCase；双向 serde + ToSchema）
// ---------------------------------------------------------------------------

/// select 控件选项。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SettingOptionDto {
    pub value: String,
    pub label: String,
}

/// 声明式设置项（镜像 `engine::plugin::SettingField`，api 本地定义）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct SettingFieldDto {
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// `string` / `number` / `boolean`。
    #[serde(rename = "type")]
    pub setting_type: String,
    /// `text`/`password`/`textarea`/`select`/`toggle`/`number`/`folder`。
    pub widget: String,
    #[serde(default)]
    pub options: Vec<SettingOptionDto>,
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub pattern: Option<String>,
    /// 辅助脚本（非空时 UI 在字段旁渲染复制按钮，仅复制文本、绝不执行）。
    #[serde(default)]
    pub helper_script: Option<String>,
    /// 辅助脚本按钮文案（空则用默认文案）。
    #[serde(default)]
    pub helper_label: Option<String>,
}

/// 已安装插件视图（列表/设置表单）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct PluginDto {
    pub identity: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub homepage: String,
    pub enabled: bool,
    pub dev_mode: bool,
    /// `None` / `Manual` / `CircuitBreaker`。
    pub disabled_reason: String,
    pub settings: Vec<SettingFieldDto>,
    /// 当前设置值（key → value 字符串）。
    pub settings_values: HashMap<String, String>,
    /// manifest 声明的能力权限（如 `["ffmpeg"]`，供 UI 展示授权徽章）。
    #[serde(default)]
    pub permissions: Vec<String>,
}

/// 安装 dev 插件请求体。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct InstallPluginDevRequest {
    pub dir_path: String,
}

/// 重命名任务文件请求体。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct RenameTaskRequest {
    /// 新文件名（不含路径分隔符；引擎侧校验非法字符与状态）。
    pub file_name: String,
}

/// 设置插件启用状态请求体。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SetPluginEnabledRequest {
    pub enabled: bool,
}

/// 安装成功返回体。
///
/// `missing_components` 列出插件声明权限所需、但尚未安装的基础组件
/// （如 `"ffmpeg"`/`"ytdlp"`，依赖表见引擎 `plugin::dependencies`）——
/// 提醒式而非阻断式：安装本身已成功，客户端应提示用户前往组件设置安装依赖。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct InstalledPlugin {
    pub identity: String,
    #[serde(default)]
    pub missing_components: Vec<String>,
}

/// 市场索引条目视图（去中心化插件市场浏览/安装）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct MarketEntryDto {
    pub plugin_id: String,
    pub version: String,
    pub sequence: u64,
    pub content_hash: String,
    #[serde(default)]
    pub min_app_version: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub homepage: String,
    #[serde(default)]
    pub mirrors: Vec<String>,
    #[serde(default)]
    pub publish_time: String,
    #[serde(default)]
    pub yanked: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// manifest 声明的能力权限（如 `["ffmpeg"]`，供安装前展示授权）。
    #[serde(default)]
    pub permissions: Vec<String>,
}

/// 从市场安装请求体。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct MarketInstallRequest {
    pub plugin_id: String,
}

// ---------------------------------------------------------------------------
// 任务组与预解析（多文件任务组，Phase D；`docs/multi-file-task-group-design.md`）
// ---------------------------------------------------------------------------

/// 任务组信息（`GET /api/v1/groups` 响应元素）。
///
/// # Examples
///
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GroupDto {
    pub group_id: String,
    pub name: String,
    /// 原始分享/清单链接（展示/复制用）。
    pub source_url: String,
    /// 组根目录（子任务落盘 = 本值 + 清单条目的相对路径）。
    pub save_dir: String,
    /// Unix 秒级时间戳（字符串）。
    pub created_at: String,
}

/// [`CreateGroupRequest::items`] 的单个组成员条目（客户端在预览响应上
/// 勾选后的清单条目/规格投影）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GroupItemRequest {
    /// 二段解析标识，按 `<itemId>` 或 `<itemId>@<variantId>` 拼接（见
    /// [`PreviewItemDto::id`]/[`PreviewVariantDto::id`]）。
    pub resolver_item: String,
    pub file_name: String,
    /// 相对组根目录的子路径（空 = 组根）。
    #[serde(default)]
    pub rel_path: String,
    /// 已知大小（字节，0 = 未知）。
    #[serde(default)]
    pub size: i64,
}

/// 创建多文件任务组请求（`POST /api/v1/groups`）。`items` 不可为空
/// （空数组 → 400）。
///
/// # Examples
///
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CreateGroupRequest {
    /// 原始分享/清单链接（组行 `source_url`，展示/复制用）。
    #[serde(default)]
    pub source_url: String,
    /// 组名（空 = 组根目录直接用 `save_dir`）。
    #[serde(default)]
    pub group_name: String,
    /// 基础保存目录（组根目录 = `save_dir/sanitize(group_name)`）；
    /// 空 = 使用全局默认保存目录。
    #[serde(default)]
    pub save_dir: String,
    /// 命名队列 ID（空 = 默认队列）。
    #[serde(default)]
    pub queue_id: String,
    /// 0 = 由 segment_advisor 按文件大小动态决定。
    #[serde(default)]
    pub segments: i32,
    #[serde(default)]
    pub cookies: String,
    #[serde(default)]
    pub referrer: String,
    #[serde(default)]
    pub user_agent: String,
    /// 单任务代理 URL（空 = 使用全局代理）。
    #[serde(default)]
    pub proxy_url: String,
    #[serde(default)]
    pub extra_headers: HashMap<String, String>,
    /// 忽略 HTTPS 证书错误。缺省 false（严格验证）。
    #[serde(default)]
    pub ignore_tls_errors: bool,
    /// 稍后下载：true = 建组后不启动，待「启动队列」或用户手动恢复。
    #[serde(default)]
    pub start_paused: bool,
    /// 组成员清单（不可为空，见本类型文档）。
    #[serde(default)]
    pub items: Vec<GroupItemRequest>,
}

/// 创建任务组响应（`POST /api/v1/groups`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CreateGroupResponse {
    pub group_id: String,
}

/// 前置预解析请求（`POST /api/v1/resolve/preview`）。只读、不建任务、
/// 不写库；结果见 [`ResolvePreviewResponse`]。
///
/// # Examples
///
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ResolvePreviewRequest {
    pub url: String,
    #[serde(default)]
    pub cookies: String,
    #[serde(default)]
    pub referrer: String,
    #[serde(default)]
    pub user_agent: String,
    #[serde(default)]
    pub extra_headers: HashMap<String, String>,
}

/// [`ResolvePreviewRequest`] 的结果。`items` 为空且 `error` 为空 = 插件未
/// 返回清单（客户端应回退普通单任务创建对话框）；`error` 非空 = 预解析
/// 失败（同样回退，`error` 供 UI 提示）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ResolvePreviewResponse {
    pub name: String,
    pub source_url: String,
    /// 无错误时为空。
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub items: Vec<PreviewItemDto>,
}

/// [`ResolvePreviewResponse::items`] 的单个清单条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct PreviewItemDto {
    /// 插件自定义标识，建组时按 `<id>` 或 `<id>@<variantId>` 拼进
    /// [`GroupItemRequest::resolver_item`]。
    pub id: String,
    pub name: String,
    /// 相对组根目录的子路径（空 = 根）。
    pub path: String,
    /// 已知大小（字节），未知为 0。
    pub size: i64,
    pub variants: Vec<PreviewVariantDto>,
}

/// [`PreviewItemDto::variants`] 的单个规格（画质/格式）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct PreviewVariantDto {
    pub id: String,
    pub label: String,
    /// 已知大小（字节），未知为 0。
    pub size: i64,
}

// ---------------------------------------------------------------------------
// P2P 设备互联（device link）—— 配对握手 + 数据面下发的 wire 契约
// ---------------------------------------------------------------------------

/// `/ping` 透出的本机设备互联身份（无鉴权），供发起方 TOFU 固定 + 展示。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LinkPingInfo {
    /// 本机 Ed25519 身份指纹（设备 ID）。
    pub fingerprint: String,
    /// 展示名。
    pub name: String,
    /// 平台标识。
    #[serde(default)]
    pub platform: String,
}

/// 配对 `hello` 请求（发起方 → 响应方）。全部密钥/签名字段为 base64。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LinkPairHelloRequest {
    /// 一次性配对码（响应方 UI 展示、用户手输）。
    pub code: String,
    /// 发起方临时 X25519 公钥（base64）。
    pub initiator_eph_pub: String,
    /// 发起方 Ed25519 身份公钥（base64）。
    pub initiator_id_pub: String,
    /// 发起方对握手转录的 Ed25519 签名（base64）。
    pub initiator_sig: String,
    /// 发起方展示名。
    pub name: String,
    /// 发起方平台。
    #[serde(default)]
    pub platform: String,
    /// 发起方客户端版本。
    #[serde(default)]
    pub app_version: String,
    /// 发起方自报可达候选地址（`ip:port`），供响应方存为回连候选。
    #[serde(default)]
    pub initiator_addrs: Vec<String>,
}

/// 配对 `hello` 回复（响应方 → 发起方）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LinkPairHelloResponse {
    pub session_id: String,
    pub responder_eph_pub: String,
    pub responder_id_pub: String,
    pub responder_sig: String,
    pub name: String,
    #[serde(default)]
    pub platform: String,
    #[serde(default)]
    pub app_version: String,
    /// 供响应方本地展示的 SAS（应与发起方计算一致）。
    pub sas: String,
}

/// 配对 `confirm` 请求：SAS 核对后确认/拒绝。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LinkPairConfirmRequest {
    pub session_id: String,
    pub confirm: bool,
}

/// 响应方处理一次入站配对 `confirm` 的终局（HTTP 层用于渲染 `paired` + `reason`）。
///
/// 与引擎 `link::manager::PairConfirmOutcome` 一一对应，但独立定义：`fluxdown_api`
/// 不依赖引擎的 link 模块（后者是可选 feature，移动端整块不编译）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkPairConfirmOutcome {
    /// 本机用户批准，发起方已入册。
    Paired,
    /// 发起方自己传了 `confirm=false`。
    Declined,
    /// 本机用户核对 SAS 后拒绝。
    Rejected,
    /// 等待本机用户核验超时（60s 决策窗口耗尽）。
    TimedOut,
}

impl LinkPairConfirmOutcome {
    /// 是否真的完成了配对。
    #[must_use]
    pub fn paired(self) -> bool {
        matches!(self, Self::Paired)
    }

    /// 透出给发起方的稳定判别串；`Paired`/`Declined` 无需额外理由。
    #[must_use]
    pub fn reason(self) -> Option<&'static str> {
        match self {
            Self::Rejected => Some("rejected"),
            Self::TimedOut => Some("timeout"),
            Self::Paired | Self::Declined => None,
        }
    }
}

/// 批准/拒绝一次入站配对核验（管理面版本；`session_id` 对应
/// `LinkEvent{kind:"incomingPairing"}` / WS `linkIncomingPairing` 携带的会话 id）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LinkPairApproveRequest {
    pub session_id: String,
    pub accept: bool,
}

/// 已配对设备下发下载任务的请求体（鉴权走 `X-FluxLink-*` 头，非 body）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LinkTaskRequest {
    pub url: String,
    #[serde(default)]
    pub save_dir: String,
    #[serde(default)]
    pub file_name: String,
}

/// 数据面链路 HMAC 鉴权凭据（从 `X-FluxLink-*` 请求头提取，非 wire body）。
#[derive(Debug, Clone)]
pub struct LinkAuth {
    /// 发起方设备指纹（响应方据此查每对独立链路密钥）。
    pub device: String,
    /// Unix 秒时间戳（防重放）。
    pub ts: i64,
    /// 一次性随机串。
    pub nonce: String,
    /// HMAC-SHA256 标签（hex，覆盖密文摘要，encrypt-then-MAC）。
    pub tag: String,
    /// `X-FluxLink-Enc` 头原始值：数据面 body 加密方案版本号。当前唯一
    /// 合法值是 `"v1"`（ChaCha20-Poly1305）；缺失或其他值一律鉴权失败——
    /// 双端同版本发布，不兼容明文旧客户端，不留降级回退路径。
    pub enc: String,
}

/// 生成配对码的响应（`POST /api/v1/link/code`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LinkCodeResponse {
    /// 6 位一次性配对码。
    pub code: String,
    /// 有效秒数。
    pub ttl_seconds: i64,
}

// ---------------------------------------------------------------------------
// P2P 设备互联管理面（发现/配对/名册；均需 management token；
// docs 见 local://link_mgmt_contract.md）
// ---------------------------------------------------------------------------

/// 一台被发现、尚未配对的设备（`GET /api/v1/link/discovered` 元素 /
/// `POST /api/v1/link/probe` 响应）。对应引擎 `link::DiscoveredPeer`（宿主层
/// 转换，`kind` → `source` 小写字符串 `"mdns"`/`"manual"`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LinkDiscoveredPeer {
    /// 对端指纹（经 `/ping` TOFU 获得；mDNS 未探测到时为 `None`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    pub host: String,
    pub port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_version: Option<String>,
    /// 发现途径：`"mdns"` | `"manual"`。
    pub source: String,
}

/// 本地设备发现开关请求体（`POST /api/v1/link/discovery`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LinkDiscoveryRequest {
    /// `"start"` | `"stop"`。
    pub action: String,
}

/// 手动地址探测请求体（`POST /api/v1/link/probe`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LinkProbeRequest {
    pub host: String,
    pub port: u16,
}

/// 发起配对请求体（`POST /api/v1/link/pair/begin`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LinkPairBeginRequest {
    pub host: String,
    pub port: u16,
    pub code: String,
}

/// 发起配对成功后的待确认结果（`POST /api/v1/link/pair/begin` 响应）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LinkPairBeginResponse {
    pub token: String,
    /// 供双方肉眼核对的短认证串。
    pub sas: String,
    pub peer_name: String,
    pub peer_fingerprint: String,
}

/// SAS 核对后确认/拒绝配对请求体（`POST /api/v1/link/pair/finish`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LinkPairFinishRequest {
    pub token: String,
    pub accept: bool,
}

/// 配对完成结果（`POST /api/v1/link/pair/finish` 响应）。`accept=false` 或
/// 对端拒绝时 `paired=false`，`device` 省略。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LinkPairFinishResponse {
    pub paired: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<LinkDeviceInfo>,
}

/// 一台**已配对**设备的对外视图（`GET /api/v1/link/devices` 元素）。严禁透出
/// `link_secret`/`identity_pub` 等敏感字段（对应引擎 `link::PeerRecord`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LinkDeviceInfo {
    pub fingerprint: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    /// 并发探测得到的在线状态（见 `ApiHost::link_devices` 实现）。
    pub online: bool,
    pub paired_at: i64,
    pub last_seen_at: i64,
}

/// 发现快照响应（`GET /api/v1/link/discovered`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LinkDiscoveredResponse {
    pub peers: Vec<LinkDiscoveredPeer>,
}

/// 已配对设备列表响应（`GET /api/v1/link/devices`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LinkDevicesResponse {
    pub devices: Vec<LinkDeviceInfo>,
}

/// 已配对设备下发下载任务请求体（管理面，token 鉴权；区别于数据面链路 HMAC
/// 鉴权的 [`LinkTaskRequest`]，字段语义一致）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LinkDeviceTaskRequest {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
}

/// 通用 `{"ok":true}` 应答（发现开关 / 解除配对）。与 [`ResultMessage`] 的
/// `{"success","message"}` 形态不同——契约就此路由指定的字面 wire 形状。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LinkOkResponse {
    pub ok: bool,
}

/// 一个 RSS 订阅（`GET/POST /api/v1/rss`、`PUT /api/v1/rss/{id}`）。
///
/// 写请求复用同一结构：全字段 `#[serde(default)]`，客户端只需给关心的字段；
/// 运行态字段（`lastFetchAt`/`lastError`/`failCount`/`seeded`/`unreadCount`）
/// 只读，写入时被引擎忽略。
///
/// # Examples
///
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct RssSourceDto {
    #[serde(default)]
    pub source_id: String,
    pub url: String,
    /// 空 = 用 feed 标题回填。
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// false = 收集模式（只收集条目供手动挑选）。
    #[serde(default = "default_true")]
    pub auto_download: bool,
    /// 自动创建的任务以 paused 落库。
    #[serde(default)]
    pub start_paused: bool,
    /// 空 = 内置主队列。
    #[serde(default)]
    pub queue_id: String,
    /// 空 = 队列目录 → 全局目录。
    #[serde(default)]
    pub save_dir: String,
    /// 抓取间隔（分钟）；0 = 引擎默认 30。
    #[serde(default)]
    pub interval_minutes: i32,
    /// 包含关键词（`|` = 或，空格 = 且；空 = 不过滤）。
    #[serde(default)]
    pub include_pattern: String,
    #[serde(default)]
    pub exclude_pattern: String,
    #[serde(default)]
    pub use_regex: bool,
    #[serde(default)]
    pub smart_episode: bool,
    /// 体积下限（字节，0 = 不限）。
    #[serde(default)]
    pub size_min_bytes: i64,
    /// 体积上限（字节，0 = 不限）。
    #[serde(default)]
    pub size_max_bytes: i64,
    #[serde(default = "default_true")]
    pub send_referer: bool,
    #[serde(default = "default_true")]
    pub notify_on_download: bool,
    /// 每轮最多新建任务数（1..=100）；0 = 引擎默认 20。
    #[serde(default)]
    pub max_per_fetch: i32,
    #[serde(default)]
    pub cookies: String,
    #[serde(default)]
    pub user_agent: String,
    #[serde(default)]
    pub proxy_url: String,
    /// 只读：上次发起抓取的 Unix 秒（0 = 从未）。
    #[serde(default)]
    pub last_fetch_at: i64,
    /// 只读：上次成功抓取的 Unix 秒（0 = 从未）。
    #[serde(default)]
    pub last_success_at: i64,
    /// 只读：上次失败原因（空 = 健康）。
    #[serde(default)]
    pub last_error: String,
    /// 只读：连续失败次数（驱动指数退避）。
    #[serde(default)]
    pub fail_count: i32,
    /// 只读：首轮抓取是否已完成。
    #[serde(default)]
    pub seeded: bool,
    #[serde(default)]
    pub position: i32,
    /// 只读：未处理条目数（侧边栏 badge）。
    #[serde(default)]
    pub unread_count: i32,
}

/// 订阅流中的一个条目（`GET /api/v1/rss/{id}/items`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct RssItemDto {
    pub source_id: String,
    /// 去重主键。
    pub guid: String,
    pub title: String,
    pub link: String,
    /// enclosure 直链（空 = 回退 `link`）。
    pub enclosure_url: String,
    /// enclosure 声明大小（字节，0 = 未知）。
    pub enclosure_length: i64,
    /// 发布时间（Unix 秒，0 = 未知）。
    pub pub_date: i64,
    pub fetched_at: i64,
    /// 0=新 1=已下载 2=已忽略 3=规则未命中 4=重复剧集 5=首轮历史条目。
    pub status: i32,
    /// `status == 1` 时回链的任务 ID。
    pub task_id: String,
    /// 智能剧集归一键（空 = 未识别）。
    pub episode_key: String,
    /// 稳定原因码（`excluded`/`too_large`/`dup_episode`/`seed_skipped`/…；
    /// 空 = 无）。**客户端负责本地化**。
    pub reason: String,
}

/// 对条目执行手动操作（`POST /api/v1/rss/{id}/items/action`）。
///
/// guid 走请求体而不是路径段：真实 feed 的 guid 常常就是一整条 URL，
/// 塞进路径要双重编码，且被反向代理规范化后会静默改写。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct RssItemActionRequest {
    /// `action = "readAll"` 时忽略。
    #[serde(default)]
    pub guid: String,
    /// `download`（绕过规则强制下载）/ `ignore` / `readAll`（全部标记已读）。
    pub action: String,
}

/// 验证一个 feed 地址（`POST /api/v1/rss/validate`，只读、不落库）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct RssValidateRequest {
    pub url: String,
    #[serde(default)]
    pub cookies: String,
    #[serde(default)]
    pub user_agent: String,
    #[serde(default)]
    pub proxy_url: String,
}

/// feed 验证结果。`error` 非空即验证失败（HTTP 状态仍是 200——这是一次
/// **诊断**调用，失败原因本身就是有效载荷）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct RssValidateResponse {
    pub url: String,
    /// feed 标题（供回填订阅名）。
    pub feed_title: String,
    /// 最近条目预览。
    pub items: Vec<RssItemDto>,
    /// 无错误时为空。
    pub error: String,
}

// ---------------------------------------------------------------------------
// WS 服务端 → 客户端
// ---------------------------------------------------------------------------

/// 分段字节范围与进度（`segmentProgress` 载荷）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct SegmentDetailDto {
    pub index: i32,
    pub start_byte: i64,
    pub end_byte: i64,
    pub downloaded_bytes: i64,
}

/// 多 CDN 单节点描述（`taskCdnEvent` 载荷）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CdnNodeDto {
    /// 节点 IP；SYS 兜底节点为 "SYS"。
    pub ip: String,
    /// 候选来源："sys" / "doh:<端点IP>" / "ecs:<端点IP>"；SYS 为空串。
    pub origin: String,
    /// 本任务经该节点下载的字节数（summary 有效，其余 0）。
    pub bytes: i64,
    /// EWMA 吞吐（B/s）：pool = 健康度先验（0 = 无先验）；summary = 实测。
    pub ewma_bps: i64,
    /// 当前未归还的段租约数（leases 快照有效，其余 0）。
    pub active: i32,
}

/// 任务在队列中的位置（`queuePositionsChanged` 载荷）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct QueuePositionDto {
    pub task_id: String,
    /// 1-based，0 = 不在队列中。
    pub position: i32,
}

/// 文件跟踪扫描判定的产物文件存在性变化（`fileMissingChanged` 载荷）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct FileMissingUpdateDto {
    pub task_id: String,
    /// true = 已完成任务的目标文件从磁盘上消失；false = 重新探测到存在（自愈）。
    pub missing: bool,
}

/// HLS 可选码率变体（`hlsSelectionRequest` 载荷）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HlsQualityOptionDto {
    pub index: i32,
    pub bandwidth: i64,
    pub width: i64,
    pub height: i64,
}

/// 种子内单个文件条目（`btSelectionRequest` 载荷）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct BtFileDto {
    pub index: i32,
    pub path: String,
    pub size: i64,
}

/// 插件 resolve 返回的可选变体（`resolveVariantRequest` 载荷）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ResolveVariantOptionDto {
    pub index: i32,
    pub label: String,
    pub container: String,
    pub bandwidth: i64,
    pub width: i64,
    pub height: i64,
    pub total_bytes: i64,
}

/// 服务端经 `/api/v1/ws` 推送的实时消息。
///
/// JSON 形态：`{"type":"taskProgress","taskId":"…",…}`（`type` 判别 + 扁平
/// camelCase 字段）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum WsServerMsg {
    /// 任务进度（下载中周期推送；含 live speed —— REST `TaskDto` 无此字段）。
    TaskProgress {
        task_id: String,
        /// 0=pending, 1=downloading, 2=paused, 3=completed, 4=error, 5=preparing
        status: i32,
        downloaded_bytes: i64,
        total_bytes: i64,
        /// 字节/秒。
        speed: i64,
        /// BT 上传速率（字节/秒；非 BT 任务恒 0）。
        upload_speed: i64,
        file_name: String,
        save_dir: String,
        url: String,
        error_message: String,
        /// BT 做种累计上传字节数（非 BT 任务恒 0）。
        uploaded_bytes: i64,
        /// 0=none, 1=做种中, 2-7=停止原因, 8=排队做种（等待槽位）
        seeding_status: i32,
        /// 做种停止原因的人类可读描述（无则为空）。
        seeding_message: String,
        /// 累计做种秒数（发帧时刻；排队/暂停不计，非 BT 任务恒 0）。
        seeding_time_secs: i64,
    },
    /// 全部任务快照（连接建立时 + 引擎主动广播）。
    TasksSnapshot { tasks: Vec<TaskDto> },
    /// 分段级进度（详情面板分段可视化）。
    SegmentProgress {
        task_id: String,
        total_bytes: i64,
        segment_count: i32,
        segments: Vec<SegmentDetailDto>,
    },
    /// 动态分段拆分事件（驱动拆分动画）。
    SegmentSplit {
        task_id: String,
        parent_index: i32,
        parent_new_end: i64,
        child_index: i32,
        child_start: i64,
        child_end: i64,
        is_proactive: bool,
        total_segments: i32,
    },
    /// 多 CDN 并发下载的节点级活动事件（任务详情日志）。语义与字段约定见
    /// `fluxdown_engine::events::EngineEvent::TaskCdnEvent`。
    TaskCdnEvent {
        task_id: String,
        /// "pool" | "kick" | "breaker" | "fallback" | "summary"
        kind: String,
        host: String,
        nodes: Vec<CdnNodeDto>,
        ip: String,
        reason: String,
        candidates: i32,
        alive: i32,
        cap: i32,
        auto_cap: bool,
    },
    /// 任务元数据探测完成（文件名/大小确定）。
    TaskMetaProbed {
        task_id: String,
        file_name: String,
        total_bytes: i64,
    },
    /// 命名队列列表变化。
    QueuesChanged { queues: Vec<QueueDto> },
    /// 单任务队列归属变化（move_task_to_queue 定向广播）。
    TaskQueueChanged { task_id: String, queue_id: String },
    /// `ProxyMode::Auto` 任务的链路决策落定/变更（详情面板「链路」行）。
    /// `route` wire 标签同 `TaskDto.autoRoute`；恒非空。
    TaskRouteChanged { task_id: String, route: String },
    /// 队列内位置批量更新。
    QueuePositionsChanged { positions: Vec<QueuePositionDto> },
    /// 文件跟踪扫描结果增量：已完成任务的产物文件在磁盘上消失/回归。
    /// 只带本轮变化项，客户端按 taskId patch `fileMissing`，不重发全量快照。
    FileMissingChanged { updates: Vec<FileMissingUpdateDto> },
    /// Boost 优先任务变化。
    PriorityTaskChanged {
        priority_task_id: String,
        auto_paused_count: i32,
    },
    /// 请求客户端选择 HLS 画质（超时自动选最高带宽）。
    HlsSelectionRequest {
        task_id: String,
        options: Vec<HlsQualityOptionDto>,
    },
    /// 请求客户端选择 BT 文件（超时默认全部下载）。
    BtSelectionRequest {
        task_id: String,
        files: Vec<BtFileDto>,
    },
    /// 请求客户端选择插件 resolve 变体（画质/格式）；超时用插件提供的默认索引。
    ResolveVariantRequest {
        task_id: String,
        default_index: i32,
        options: Vec<ResolveVariantOptionDto>,
    },
    /// `ping` 应答（RTT 测量）。
    Pong {},
    /// 插件因熔断（连续超时/过载）被自动禁用（`reason` 固定 `"CircuitBreaker"`）。
    PluginAutoDisabled { identity: String, reason: String },
    /// BT 重复添加：新任务的 info-hash 已被 `existingTaskId` 持有，占位任务
    /// （`taskId`）已被引擎删除。客户端据此提示用户；`existingName` 可能为空。
    DuplicateTorrent {
        task_id: String,
        existing_task_id: String,
        existing_name: String,
    },
    /// 插件 onDone 钩子执行中（`running=true` 开始/`false` 结束）；同一任务可
    /// 有多个插件并发钩子，客户端按 `(taskId, pluginId)` 集合跟踪，用于在
    /// 已完成任务旁显示“插件处理中…”指示器。事件可能因 fire-and-forget 丢失
    /// （尤其是 `running=false`），客户端需自带看门狗超时兜底清除。
    PluginHookActivity {
        task_id: String,
        plugin_id: String,
        running: bool,
    },
    /// 插件表发生增删改（安装/卸载/启停/设置变更）；空载荷 ping，客户端收到后
    /// 全量 invalidate 插件列表查询。
    PluginsChanged {},
    /// 任务组列表变化（组建/删除/改名/回收后）；组进度仍由前端按
    /// `groupId` 对 `taskProgress` SUM 聚合，本消息不含进度字段。
    GroupsChanged { groups: Vec<GroupDto> },
    /// RSS 订阅列表变化（增删改 / 抓取状态 / 未读计数）；订阅数量少，直接
    /// 全量推，客户端整表替换。
    RssSourcesChanged { sources: Vec<RssSourceDto> },
    /// 某订阅的条目流快照（新→旧）。只在该源确有变化时下发——定时轮询抓到
    /// 0 条新条目时引擎静默，不打扰 UI。`notifyTitles` 是本轮自动建任务的
    /// 条目标题，客户端据此弹**一条**合批通知（无自动下载时为空数组）。
    RssItemsChanged {
        source_id: String,
        items: Vec<RssItemDto>,
        notify_titles: Vec<String>,
    },
    /// 新建订阅向导的 feed 验证结果。`error` 非空即验证失败——这是诊断
    /// 信息而非传输错误，客户端照常展示。
    RssFeedValidated {
        request_id: String,
        url: String,
        feed_title: String,
        items: Vec<RssItemDto>,
        error: String,
    },
    /// 组件安装/下载进度（`component` 固定 `"ffmpeg"`；`totalBytes=0` 表示未知）。
    ComponentProgress {
        component: String,
        downloaded_bytes: i64,
        total_bytes: i64,
    },
    /// 组件安装/卸载操作结果（成功/失败 + 说明）。
    ComponentResult {
        component: String,
        ok: bool,
        message: String,
    },
    /// 入站配对请求待本机用户核验（本机作为响应方收到远端 hello+配对码后，
    /// 等待管理员核对 SAS 并调用管理面 `POST /api/v1/link/pair/approve`）。
    /// 字段名与桌面端 rinf `LinkEvent{kind:"incomingPairing"}` 信号保持一致。
    LinkIncomingPairing {
        session_id: String,
        sas: String,
        name: String,
        platform: String,
    },
    /// 已配对设备名册发生变化（新增/移除），前端据此 invalidate 名册查询。
    /// 空载荷：名册本身走 REST 拉取，这里只做「该刷新了」的通知——配对落库
    /// （`PairingResponder::handle_confirm` 里的 `store.upsert`）发生在被
    /// 唤醒的后台任务中，早于 `pair/approve` 请求的 HTTP 响应返回；Web 若
    /// 只在 approve 的 onSuccess 里 refetch 名册会读到还没写入新设备的
    /// 陈旧快照，且没有其它机制能纠正它，靠这条消息触发前端重新拉取。
    LinkDevicesChanged {},
    /// 投递日志快照（新→旧，最多 100 条）。任务真完成时的投递、以及
    /// 「模拟一次下载完成」都发生在前端拉过快照之后——没有这条推送，打开着
    /// 的日志面板就停在打开时的样子。引擎侧已按 500ms 节流。
    WebhookDeliveriesChanged { deliveries: Vec<WebhookDeliveryDto> },
}

// ---------------------------------------------------------------------------
// WS 客户端 → 服务端
// ---------------------------------------------------------------------------

/// 客户端经 `/api/v1/ws` 发来的入站消息。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum WsClientMsg {
    /// 应答 `hlsSelectionRequest`。
    HlsSelection {
        task_id: String,
        selected_index: i32,
    },
    /// 应答 `btSelectionRequest`（空数组 = 全部文件）。
    BtSelection {
        task_id: String,
        selected_indices: Vec<i32>,
    },
    /// 应答 `resolveVariantRequest`。
    SelectVariant {
        task_id: String,
        selected_index: i32,
    },
    /// 设置单任务做种限制覆盖（qBittorrent 三态语义：-2 = 跟随全局，
    /// -1 = 不限制，>=0 = 自定义，0 视同不限制；分享率为千分比）。
    /// `upload_limit_bps` 为任务级做种上传限速（B/s，0 = 不限），在
    /// 下一次 torrent add 时烘焙生效。
    SetTaskSeedLimits {
        task_id: String,
        ratio_limit_milli: i64,
        post_ratio_limit_milli: i64,
        seed_time_limit_minutes: i64,
        inactive_time_limit_minutes: i64,
        #[serde(default)]
        upload_limit_bps: i64,
    },
    /// RTT 测量，服务端回 `pong`。
    Ping {},
}

// ---------------------------------------------------------------------------
// 扩展 REST 端点请求/响应体
// ---------------------------------------------------------------------------

/// 代理连通性测试请求（`POST /api/v1/proxy/test`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ProxyTestRequest {
    /// `http` / `https` / `socks4` / `socks5`。
    pub proxy_type: String,
    pub host: String,
    pub port: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
}

/// 代理连通性测试响应。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ProxyTestResponse {
    pub latency_ms: i64,
}

/// 已保存的站点 HTTP Basic 凭据（`daemon.siteAuth.list`）；不携带密码。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct SiteAuthEntryDto {
    /// `host` 或 `host:port`。
    pub site: String,
    pub user: String,
}

/// `daemon.siteAuth.delete` 参数。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct SiteAuthDeleteParams {
    pub site: String,
}

/// 引擎学习到的按域连接上限摘要（`daemon.config.connPolicy`）。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ConnPolicySummaryDto {
    pub domain_count: u64,
}

/// Tracker 订阅刷新结果（`POST /api/v1/bt/tracker-sub/refresh`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct TrackerSubRefreshResponse {
    /// 至少一个订阅源拉取成功。
    pub success: bool,
    /// 去重合并后的唯一 Tracker 数。
    pub tracker_count: i64,
    /// 成功拉取的源数。
    pub ok_sources: i64,
    /// 尝试的订阅源总数。
    pub total_sources: i64,
    /// 缓存更新时间（Unix 秒；本次未成功时沿用旧值）。
    pub updated_at: i64,
    /// 全部源失败时的错误摘要（成功时为空）。
    pub error: String,
}

/// ED2K 服务器订阅刷新结果（`POST /api/v1/ed2k/server-sub/refresh`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct Ed2kServerSubRefreshResponse {
    /// 至少一个订阅源拉取成功。
    pub success: bool,
    /// 去重合并后的唯一服务器（`ip:port`）数。
    pub server_count: i64,
    /// 成功拉取的源数。
    pub ok_sources: i64,
    /// 尝试的订阅源总数。
    pub total_sources: i64,
    /// 缓存更新时间（Unix 秒；本次未成功时沿用旧值）。
    pub updated_at: i64,
    /// 全部源失败时的错误摘要（成功时为空）。
    pub error: String,
}

/// 创建命名队列请求（`POST /api/v1/queues`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CreateQueueRequest {
    pub name: String,
    #[serde(default)]
    pub speed_limit_kbps: i64,
    #[serde(default)]
    pub upload_limit_kbps: i64,
    #[serde(default)]
    pub max_concurrent: i32,
    #[serde(default)]
    pub default_save_dir: String,
    #[serde(default)]
    pub default_segments: i32,
    #[serde(default)]
    pub default_user_agent: String,
}

/// 更新命名队列请求（`PUT /api/v1/queues/{id}`），字段同创建。
pub type UpdateQueueRequest = CreateQueueRequest;

/// 移动任务到队列请求（`PUT /api/v1/tasks/{id}/queue`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct MoveQueueRequest {
    /// 空 = 默认队列。
    #[serde(default)]
    pub queue_id: String,
}

/// 队列每日定时计划请求（`PUT /api/v1/queues/{id}/schedule`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct QueueScheduleRequest {
    /// 定时计划是否启用。
    pub enabled: bool,
    /// 每日定时启动时间 `HH:MM`（空 = 不定时启动）。
    #[serde(default)]
    pub start_time: String,
    /// 每日定时停止时间 `HH:MM`（空 = 不定时停止）。
    #[serde(default)]
    pub stop_time: String,
    /// 生效星期位掩码：bit0=周一 … bit6=周日；0/缺省 = 每天。
    #[serde(default)]
    pub days: i32,
}

/// 队列内任务排序请求（`PUT /api/v1/queues/{id}/order`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ReorderQueueRequest {
    /// 队列内任务的完整新顺序（依次写入 1..N 的 queueOrder）。
    pub task_ids: Vec<String>,
}

/// 目录项（`FsListResponse.dirs` 元素）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct FsEntry {
    pub name: String,
    pub path: String,
}

/// 目录列举响应（`GET /api/v1/fs/list`，服务器端保存目录选择器用）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct FsListResponse {
    /// 实际列举的目录（绝对路径）。
    pub path: String,
    /// 上级目录（根目录时为 None）。
    pub parent: Option<String>,
    /// 子目录列表（不含文件）。
    pub dirs: Vec<FsEntry>,
}

/// 服务器运行状态（`GET /api/v1/stats`，前端状态栏用）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct StatsResponse {
    /// 默认保存目录所在磁盘的剩余字节；探测失败为 None。
    pub disk_free_bytes: Option<u64>,
    pub save_dir: String,
    pub server_version: String,
    /// 当前 WS 连接数。
    pub ws_clients: usize,
    /// 演示模式开关（服务器以 `FLUXDOWN_DEMO_URL` 启动时为 true）。
    pub demo_mode: bool,
    /// 演示模式下唯一允许下载的 URL；非演示模式为空串。
    pub demo_url: String,
}

/// 单个日志文件（`GET /api/v1/logs` 列表项）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LogFileDto {
    pub name: String,
    pub size: i64,
}

/// 日志目录与文件清单（`GET /api/v1/logs`，前端「关于」页展示 + 导出入口）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LogsResponse {
    /// 日志目录绝对路径（NAS 用户据此在服务器文件系统定位日志）。
    pub dir: String,
    /// 全部日志文件（按日期 + 分卷序升序）。
    pub files: Vec<LogFileDto>,
    /// 日志 writer 是否已成功初始化。
    pub initialized: bool,
    /// 本次进程生命周期内是否发生过日志写入/轮转/清理失败。
    pub degraded: bool,
    /// 本次进程生命周期内累计日志基础设施失败次数。
    pub failure_count: u64,
    /// 最近一次日志基础设施失败；无失败时为 `null`。
    pub last_error: Option<String>,
}

/// token 重新生成响应（`POST /api/v1/token/regenerate`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct TokenResponse {
    pub token: String,
    /// 生效说明（新 token 立即生效，旧 token 同时失效）。
    pub note: String,
}

/// 首次运行状态（`GET /api/v1/setup/status`，**无鉴权**）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct SetupStatusResponse {
    /// `true` = 尚未设置访问密钥，Web 端应展示首次运行向导。
    pub setup_required: bool,
    /// 访问密钥最短长度（前端提示与校验用，避免两端各写一份常量）。
    pub min_length: i64,
}

/// 首次运行设置访问密钥请求（`POST /api/v1/setup`，**无鉴权**）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct SetupRequest {
    /// 用户设定的访问密钥；规则见 `config::validate_access_key`。
    pub token: String,
}

// ---------------------------------------------------------------------------
// 组件（v1 仅 ffmpeg）
// ---------------------------------------------------------------------------

/// ffmpeg 组件状态（`GET /api/v1/components/ffmpeg`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ComponentFfmpegStatus {
    /// 生效路径来源：`manual` / `managed` / `system` / `none`。
    pub source: String,
    /// 生效的可执行文件路径（`source == "none"` 时为空）。
    pub path: String,
    /// `ffmpeg -version` 探测到的版本串（探测失败/未找到时为空）。
    pub version: String,
    /// 托管安装记录的版本号（空 = 未托管安装）。
    pub managed_version: String,
    /// 系统 PATH 中探测到的 ffmpeg 路径（无论是否生效；空 = 无）。
    pub system_path: String,
    /// 当前平台是否提供托管安装（BtbN 构建）。`false` = macOS 等——Web UI 隐藏
    /// 托管安装区块，只引导系统 PATH / 手动指定，避免反复弹「不支持安装」。
    pub managed_supported: bool,
}

/// ffmpeg 可安装版本列表（`GET /api/v1/components/ffmpeg/versions`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ComponentVersions {
    /// 降序排列的稳定版本号。
    pub versions: Vec<String>,
    /// 最新稳定版（= `versions` 首个；空 = 解析失败）。
    pub latest_stable: String,
}

/// yt-dlp 组件状态（`GET /api/v1/components/ytdlp`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ComponentYtdlpStatus {
    /// 生效路径来源：`manual` / `managed` / `system` / `none`。
    pub source: String,
    /// 生效的可执行文件路径（`source == "none"` 时为空）。
    pub path: String,
    /// `yt-dlp --version` 探测到的版本串（探测失败/未找到时为空）。
    pub version: String,
    /// 托管安装记录的版本号（空 = 未托管安装）。
    pub managed_version: String,
    /// 系统 PATH 中探测到的 yt-dlp 路径（无论是否生效；空 = 无）。
    pub system_path: String,
    /// 当前平台是否提供托管安装（GitHub Release 构建）。
    pub managed_supported: bool,
}

/// 安装/更新 ffmpeg 请求（`POST /api/v1/components/ffmpeg/install`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct InstallFfmpegRequest {
    /// 钉住的版本号；`None` = 安装/更新到最新稳定版。
    #[serde(default)]
    pub version: Option<String>,
}

// ---------------------------------------------------------------------------
// Webhook 任务事件推送（免费自托管）
// ---------------------------------------------------------------------------

/// 一条投递记录（`GET /api/v1/webhooks/deliveries` 列表项）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct WebhookDeliveryDto {
    pub delivery_id: String,
    /// Unix 毫秒。
    pub timestamp_ms: i64,
    /// 事件 wire 名（`task.completed` 等）。
    pub event: String,
    pub endpoint_id: String,
    pub endpoint_name: String,
    pub url: String,
    /// 请求头摘录，每行 `K: V`；鉴权类值已掩码。
    pub request_headers: String,
    pub request_body: String,
    /// HTTP 状态码；0 = 未拿到响应。
    pub status_code: i32,
    pub response_body: String,
    pub latency_ms: i64,
    pub attempts: i32,
    pub success: bool,
    pub error: String,
}

/// 服务预设元数据（`GET /api/v1/webhooks/deliveries` 一并返回）——Web 端
/// 「实时载荷预览」的模板来源，客户端只做占位符替换，不复制模板内容。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct WebhookPresetDto {
    pub id: String,
    pub label: String,
    pub url_placeholder: String,
    pub default_template: String,
    pub content_type: String,
}

/// 投递日志 + 预设目录（`GET /api/v1/webhooks/deliveries`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct WebhookDeliveriesResponse {
    /// 新的在前，最多 100 条（内存环形缓冲，不落盘）。
    pub deliveries: Vec<WebhookDeliveryDto>,
    pub presets: Vec<WebhookPresetDto>,
    /// 可用占位符清单（`{task.fileName}` 等）。
    pub variables: Vec<String>,
}

/// 测试投递请求（`POST /api/v1/webhooks/test`）。
///
/// 直接内嵌端点草稿（**无需先保存**），schema 同 `webhook.endpoints` 数组元素。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct WebhookTestRequest {
    #[serde(flatten)]
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub endpoint: serde_json::Value,
}

/// 测试投递结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct WebhookTestResponse {
    pub success: bool,
    /// HTTP 状态码；0 = 未拿到响应。
    pub status_code: i32,
    pub latency_ms: i64,
    /// 成功时为空。
    pub error: String,
}

/// `POST /api/v1/webhooks/simulate` 的回执。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct WebhookSimulateResponse {
    /// 按订阅规则投出去的端点数。0 = 没有端点订阅 `task.completed`，
    /// 前端该直说而不是干等投递记录。
    pub dispatched: i32,
}
