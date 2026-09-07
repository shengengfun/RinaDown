//! axum HTTP 服务器 —— FluxDown 本机 API 服务。
//!
//! 一个端口、一个服务器，三组按配置独立启停的路由：
//!
//! | 路由组 | 端点 | 开关 | 鉴权 |
//! |---|---|---|---|
//! | 探活 | `GET /ping` | 总开关 | 无 |
//! | 脚本接管 | `POST /download`、`/download/batch` | `takeover_enabled` | `X-FluxDown-Client` 头 + 可选 token |
//! | aria2 兼容 | `POST /jsonrpc`、`GET /jsonrpc`（WS 升级） | `jsonrpc_enabled` | 可选 token（头或 `token:xxx`；WS 仅 `token:xxx`） |
//! | 管理 API | `/api/v1/*` | `management_enabled` | **强制** token（Bearer 或头） |
//!
//! 服务器只依赖 [`ApiHost`] trait，宿主形态（桌面 App / headless server）无关。
//! 安全模型详见 [`crate::auth`] 模块文档。

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, Query, State, WebSocketUpgrade};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post, put};
use serde::Deserialize;
use serde_json::json;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::auth::{TokenCell, check_management_auth, check_takeover_auth, header_token_ok};
use crate::jsonrpc::handle_jsonrpc;
use crate::jsonrpc_ws::run_session;
use crate::mcp::handle_mcp;
use crate::routes;
use crate::service::{ApiError, ApiHost, UNKNOWN_ENDPOINT_MESSAGE};
use crate::takeover::parse_batch;
use fluxdown_protocol::daemon::{
    CreateGroupRequest, CreateGroupResponse, CreateTaskRequest, CreatedTask, DownloadRequest,
    LinkAuth, LinkDeviceTaskRequest, LinkDevicesResponse, LinkDiscoveredResponse,
    LinkDiscoveryRequest, LinkOkResponse, LinkPairApproveRequest, LinkPairBeginRequest,
    LinkPairConfirmRequest, LinkPairFinishRequest, LinkPairFinishResponse, LinkPairHelloRequest,
    LinkProbeRequest, ResolvePreviewRequest, RssItemActionRequest, RssSourceDto,
    RssValidateRequest,
};

/// 请求体大小上限：4 MB（足够容纳批量 URL 列表）。
const MAX_BODY_SIZE: usize = 4 * 1024 * 1024;
/// 端口重绑重试次数（热重启时旧 listener 释放存在竞态窗口）。
const BIND_RETRIES: u32 = 20;
/// 每次重绑重试间隔。
const BIND_RETRY_DELAY: Duration = Duration::from_millis(100);
/// `Allow` / `Access-Control-Allow-Methods` 的方法全集。
const ALLOWED_METHODS: &str = "GET, POST, PUT, DELETE, OPTIONS";
/// `Access-Control-Allow-Origin` / `-Allow-Headers` 的通配值（仅在
/// `cors_allow_all` 开启时使用）。
const WILDCARD: HeaderValue = HeaderValue::from_static("*");
/// Chrome 私有网络访问（Private/Local Network Access）预检响应头。
/// `http` crate 未内置该常量，此处自建。
const ALLOW_PRIVATE_NETWORK: HeaderName =
    HeaderName::from_static("access-control-allow-private-network");

/// API 服务器配置，从 DB config 表加载。
///
/// # Examples
///
/// ```
/// use std::collections::HashMap;
/// use fluxdown_api::server::ApiServerConfig;
///
/// let cfg = ApiServerConfig::from_config_map(&HashMap::new(), "1.0.0");
/// assert!(cfg.enabled);            // 默认启用
/// assert_eq!(cfg.port, 17800);
/// assert!(cfg.token.is_empty());
/// assert!(cfg.takeover_enabled);   // 接管默认开
/// assert!(cfg.jsonrpc_enabled);    // aria2 兼容默认开
/// assert!(!cfg.management_enabled); // 管理 API 默认关
/// ```
#[derive(Debug, Clone)]
pub struct ApiServerConfig {
    /// 总开关（`local_server_enabled`，默认 true）。
    pub enabled: bool,
    /// 监听端口（`local_server_port`，默认 17800）。
    pub port: u16,
    /// 鉴权 token（`local_server_token`，空 = 接管/aria2 不鉴权，管理 API 拒绝）。
    /// 见 [`TokenCell`]：headless 首次运行设置 / 重新生成会原地改写它，无需重启。
    pub token: TokenCell,
    /// 脚本接管子开关（`local_server_takeover_enabled`，默认 true）。
    pub takeover_enabled: bool,
    /// aria2 兼容子开关（`local_server_jsonrpc_enabled`，默认 true）。
    pub jsonrpc_enabled: bool,
    /// 管理 API 子开关（`local_server_api_enabled`，默认 false）。
    pub management_enabled: bool,
    /// MCP 端点子开关（`local_server_mcp_enabled`，默认 false）。
    /// 与管理 API 共用 token 鉴权（Bearer / X-FluxDown-Token）。
    pub mcp_enabled: bool,
    /// 允许局域网 / 组网访问（`local_server_lan_enabled`，默认 false）。
    /// 为 true 时绑定 `0.0.0.0` 使同网络 / 用户自建组网内的设备可达（供免账号本地
    /// 配对的响应方场景）；为 false 时仅绑回环 `127.0.0.1`。
    pub lan_enabled: bool,
    /// 允许任意来源的跨域（CORS）请求（`local_server_cors_allow_all`，默认 false）。
    ///
    /// 默认关闭时本服务对**任何**请求都不返回 `Access-Control-*` 头，浏览器页面
    /// 的跨域 `fetch()` 一律在预检阶段被拦下（见 [`crate::auth`] 安全模型第 2 条）。
    /// 开启后预检与真实响应都带 `Access-Control-Allow-Origin: *`
    /// （外加 `Access-Control-Allow-Private-Network: true`，用于 Chrome
    /// 的本地网络访问门禁），等价于 aria2 的 `--rpc-allow-origin-all`——
    /// 把「aria2 RPC 探活」写死成浏览器 `fetch` 的网站需要它才能识别本机服务。
    pub cors_allow_all: bool,
    /// agent 使用的热切换路由开关；普通宿主为 `None`，维持静态注册行为。
    pub runtime_switches: Option<Arc<ApiRuntimeSwitches>>,
    /// 宿主应用版本号（`/ping`、`/api/v1/info` 返回）。
    pub app_version: String,
}

#[derive(Debug)]
pub struct ApiRuntimeSwitches {
    takeover: AtomicBool,
    jsonrpc: AtomicBool,
    management: AtomicBool,
    mcp: AtomicBool,
    cors: AtomicBool,
}

impl ApiRuntimeSwitches {
    #[must_use]
    pub fn new(takeover: bool, jsonrpc: bool, management: bool, mcp: bool, cors: bool) -> Self {
        Self {
            takeover: AtomicBool::new(takeover),
            jsonrpc: AtomicBool::new(jsonrpc),
            management: AtomicBool::new(management),
            mcp: AtomicBool::new(mcp),
            cors: AtomicBool::new(cors),
        }
    }

    pub fn update(&self, takeover: bool, jsonrpc: bool, management: bool, mcp: bool, cors: bool) {
        self.takeover.store(takeover, Ordering::Release);
        self.jsonrpc.store(jsonrpc, Ordering::Release);
        self.management.store(management, Ordering::Release);
        self.mcp.store(mcp, Ordering::Release);
        self.cors.store(cors, Ordering::Release);
    }

    fn enabled(&self, group: RouteGroup) -> bool {
        match group {
            RouteGroup::Takeover => self.takeover.load(Ordering::Acquire),
            RouteGroup::JsonRpc => self.jsonrpc.load(Ordering::Acquire),
            RouteGroup::Management => self.management.load(Ordering::Acquire),
            RouteGroup::Mcp => self.mcp.load(Ordering::Acquire),
        }
    }

    fn cors_enabled(&self) -> bool {
        self.cors.load(Ordering::Acquire)
    }
}

#[derive(Clone, Copy)]
enum RouteGroup {
    Takeover,
    JsonRpc,
    Management,
    Mcp,
}

impl ApiServerConfig {
    /// 从 config 表键值对构建配置。缺失键取默认值。
    #[must_use]
    pub fn from_config_map(map: &HashMap<String, String>, app_version: &str) -> Self {
        let flag = |key: &str, default: bool| -> bool {
            map.get(key).map(|v| v == "true").unwrap_or(default)
        };
        Self {
            enabled: flag("local_server_enabled", true),
            port: map
                .get("local_server_port")
                .and_then(|v| v.parse().ok())
                .unwrap_or(17800),
            token: TokenCell::new(
                map.get("local_server_token")
                    .map(String::as_str)
                    .unwrap_or_default(),
            ),
            takeover_enabled: flag("local_server_takeover_enabled", true),
            jsonrpc_enabled: flag("local_server_jsonrpc_enabled", true),
            management_enabled: flag("local_server_api_enabled", false),
            mcp_enabled: flag("local_server_mcp_enabled", false),
            lan_enabled: flag("local_server_lan_enabled", false),
            cors_allow_all: flag("local_server_cors_allow_all", false),
            runtime_switches: None,
            app_version: app_version.to_string(),
        }
    }

    /// 监听地址。默认仅绑本机回环，杜绝外网/局域网暴露；用户显式开启
    /// `local_server_lan_enabled` 后绑 `0.0.0.0`，供免账号本地配对的响应方
    /// 在可信网络 / 自建组网内被对端访问。
    #[must_use]
    pub fn bind_addr(&self) -> SocketAddr {
        let ip = if self.lan_enabled {
            Ipv4Addr::UNSPECIFIED
        } else {
            Ipv4Addr::LOCALHOST
        };
        SocketAddr::from((ip, self.port))
    }

    #[must_use]
    pub fn with_runtime_switches(mut self, switches: Arc<ApiRuntimeSwitches>) -> Self {
        self.runtime_switches = Some(switches);
        self
    }

    fn route_registered(&self, group: RouteGroup) -> bool {
        if self.runtime_switches.is_some() {
            return true;
        }
        match group {
            RouteGroup::Takeover => self.takeover_enabled,
            RouteGroup::JsonRpc => self.jsonrpc_enabled,
            RouteGroup::Management => self.management_enabled,
            RouteGroup::Mcp => self.mcp_enabled,
        }
    }

    fn route_enabled(&self, group: RouteGroup) -> bool {
        self.runtime_switches.as_ref().map_or_else(
            || self.route_registered(group),
            |switches| switches.enabled(group),
        )
    }

    fn cors_enabled(&self) -> bool {
        self.runtime_switches
            .as_ref()
            .map_or(self.cors_allow_all, |switches| switches.cors_enabled())
    }
}

/// 运行中 API 服务器的句柄。[`shutdown`](Self::shutdown) 触发优雅停机，
/// 用于配置变更后的热重启。
#[derive(Debug)]
pub struct ApiServerHandle {
    cancel: CancellationToken,
}

impl ApiServerHandle {
    /// 触发优雅停机（幂等）。
    pub fn shutdown(&self) {
        self.cancel.cancel();
    }
}

/// 启动 API 服务器（后台 tokio 任务），立即返回句柄。
///
/// - `config.enabled == false` → 不监听，返回的句柄无操作。
/// - 绑定失败（端口被占用）→ 重试 [`BIND_RETRIES`] 次后放弃，仅本特性不可用，
///   不影响宿主主功能。
pub fn spawn_api_server(host: Arc<dyn ApiHost>, config: ApiServerConfig) -> ApiServerHandle {
    let cancel = CancellationToken::new();
    let handle = ApiServerHandle {
        cancel: cancel.clone(),
    };
    if !config.enabled {
        tracing::info!("local API server disabled by config");
        return handle;
    }

    let addr = config.bind_addr();
    tokio::spawn(async move {
        // 热重启场景：旧 listener 释放与新绑定之间存在竞态窗口，重试消化。
        let mut listener = None;
        for attempt in 0..BIND_RETRIES {
            match TcpListener::bind(addr).await {
                Ok(l) => {
                    listener = Some(l);
                    break;
                }
                Err(error) if attempt + 1 == BIND_RETRIES => {
                    tracing::error!(error = %error, "local API failed to bind listener");
                }
                Err(_) => tokio::time::sleep(BIND_RETRY_DELAY).await,
            }
        }
        let Some(listener) = listener else {
            return;
        };
        tracing::info!(
            %addr,
            takeover = config.takeover_enabled,
            jsonrpc = config.jsonrpc_enabled,
            management = config.management_enabled,
            "local API server listening"
        );
        serve_on(listener, host, config, cancel).await;
    });
    handle
}

/// 在已绑定的 listener 上跑完整服务（抽出以便集成测试注入 `127.0.0.1:0`
/// 临时端口）。`cancel` 触发优雅停机后返回。
pub(crate) async fn serve_on(
    listener: TcpListener,
    host: Arc<dyn ApiHost>,
    config: ApiServerConfig,
    cancel: CancellationToken,
) {
    let app = build_router(AppState {
        host,
        config: Arc::new(config),
    });
    let served = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(cancel.cancelled_owned())
    .await;
    if let Err(error) = served {
        tracing::error!(error = %error, "local API server failed");
    } else {
        tracing::info!("local API server stopped");
    }
}

// ---------------------------------------------------------------------------
// 路由与状态
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub(crate) struct AppState {
    host: Arc<dyn ApiHost>,
    config: Arc<ApiServerConfig>,
}

/// 核心路由集（按配置开关注册）：探活 / 脚本接管 / aria2 兼容 / 管理 CRUD。
/// **不含** `API_OPENAPI` 路由与 fallback —— 由调用方决定（桌面
/// `build_router` 补齐两者；headless 服务器自带合并版 openapi 与 SPA
/// fallback，见 [`api_router`]）。
fn register_core(state: AppState) -> Router<AppState> {
    let mut router = Router::new().route(routes::PING, get(ping));

    if state.config.route_registered(RouteGroup::Takeover) {
        router = router
            .route(routes::DOWNLOAD, post(takeover_download))
            .route(routes::DOWNLOAD_BATCH, post(takeover_download_batch));
    }
    if state.config.route_registered(RouteGroup::JsonRpc) {
        router = router.route(routes::JSONRPC, post(jsonrpc).get(jsonrpc_ws));
    }
    if state.config.route_registered(RouteGroup::Mcp) {
        router = router.route(routes::MCP, post(mcp));
    }
    // P2P 设备互联端点：**无 token 鉴权**（配对由一次性码 + SAS 守卫；数据面由
    // 每对独立链路 HMAC 守卫），故与 /ping 同级恒注册，不进 management 分组。
    router = router
        .route(routes::API_LINK_PAIR_HELLO, post(api_link_pair_hello))
        .route(routes::API_LINK_PAIR_CONFIRM, post(api_link_pair_confirm))
        .route(routes::API_LINK_TASKS, post(api_link_create_task));
    if state.config.route_registered(RouteGroup::Management) {
        router = router
            .route(routes::API_INFO, get(api_info))
            .route(routes::API_TASKS, get(api_list_tasks).post(api_create_task))
            // 注意：静态段 `/tasks/pause` 与参数段 `/tasks/{id}` 并存，
            // axum(matchit) 静态路由优先，无冲突。
            .route(routes::API_TASKS_PAUSE, put(api_pause_all))
            .route(routes::API_TASKS_CONTINUE, put(api_continue_all))
            .route(routes::API_TASK, get(api_get_task).delete(api_delete_task))
            .route(routes::API_TASK_PAUSE, put(api_pause_task))
            .route(routes::API_TASK_CONTINUE, put(api_continue_task))
            .route(routes::API_TASK_RENAME, post(api_rename_task))
            .route(routes::API_QUEUES, get(api_list_queues))
            .route(routes::API_RESOLVE_PREVIEW, post(api_resolve_preview))
            .route(
                routes::API_GROUPS,
                get(api_list_groups).post(api_create_group),
            )
            .route(routes::API_GROUP, axum::routing::delete(api_delete_group))
            .route(routes::API_GROUP_PAUSE, put(api_group_pause))
            .route(routes::API_GROUP_CONTINUE, put(api_group_continue))
            // 注意：静态段 `/rss/validate` 与参数段 `/rss/{id}` 并存，
            // 与 `/tasks/pause` 同理靠 axum(matchit) 静态优先消歧，无冲突。
            .route(
                routes::API_RSS,
                get(api_list_rss_sources).post(api_create_rss_source),
            )
            .route(
                routes::API_RSS_SOURCE,
                put(api_update_rss_source).delete(api_delete_rss_source),
            )
            .route(routes::API_RSS_REFRESH, post(api_refresh_rss_source))
            .route(routes::API_RSS_ITEMS, get(api_list_rss_items))
            .route(routes::API_RSS_ITEM_ACTION, post(api_rss_item_action))
            .route(routes::API_RSS_VALIDATE, post(api_validate_rss_feed))
            .route(routes::API_PLUGINS, get(api_list_plugins))
            .route(routes::API_PLUGINS_INSTALL, post(api_install_plugin))
            .route(
                routes::API_PLUGINS_INSTALL_DEV,
                post(api_install_plugin_dev),
            )
            .route(routes::API_PLUGIN_ENABLED, put(api_set_plugin_enabled))
            .route(routes::API_PLUGIN_SETTINGS, put(api_update_plugin_settings))
            .route(
                routes::API_PLUGIN,
                axum::routing::delete(api_uninstall_plugin),
            )
            .route(
                routes::API_TASK_IGNORE_PLUGIN_RETRY,
                post(api_ignore_plugin_retry),
            )
            .route(routes::API_MARKET, get(api_market_list))
            .route(routes::API_MARKET_INSTALL, post(api_market_install))
            .route(
                routes::API_LINK_CODE,
                post(api_link_generate_code).delete(api_link_stop_advertising),
            )
            .route(routes::API_LINK_DISCOVERY, post(api_link_discovery))
            .route(routes::API_LINK_DISCOVERED, get(api_link_discovered))
            .route(routes::API_LINK_PROBE, post(api_link_probe))
            .route(routes::API_LINK_PAIR_BEGIN, post(api_link_pair_begin))
            .route(routes::API_LINK_PAIR_FINISH, post(api_link_pair_finish))
            .route(routes::API_LINK_PAIR_APPROVE, post(api_link_pair_approve))
            .route(routes::API_LINK_DEVICES, get(api_link_devices))
            .route(
                routes::API_LINK_DEVICE,
                axum::routing::delete(api_link_remove_device),
            )
            .route(routes::API_LINK_DEVICE_TASKS, post(api_link_device_tasks));
    }

    router
}

/// 按配置组装桌面 App 完整路由：核心路由 + OpenAPI 规范 + 404 fallback。
/// 关闭的路由组不注册（对应端点回 404，与旧行为一致）。
fn build_router(state: AppState) -> Router {
    let mut router = register_core(state.clone());
    if state.config.route_registered(RouteGroup::Management) {
        // OpenAPI 规范文档（无鉴权——纯接口描述，不含任何用户数据）。
        router = router.route(routes::API_OPENAPI, get(openapi_spec));
    }
    router
        .fallback(unknown_endpoint)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            cors_and_preflight,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            route_group_guard,
        ))
        .layer(DefaultBodyLimit::max(MAX_BODY_SIZE))
        .with_state(state)
}

/// 供其他宿主（headless 服务器）复用的核心路由集。
///
/// 与桌面 [`spawn_api_server`] 的差异：**不含** `/api/v1/openapi.json`
/// 与 404 fallback，调用方 `merge` 自己的扩展路由、提供合并版 OpenAPI
/// 与 SPA fallback，不会与本函数产生路由冲突。已附带 OPTIONS 预检 / CORS
/// 与请求体大小限制两层中间件（与桌面行为一致）。
pub fn api_router(host: Arc<dyn ApiHost>, config: ApiServerConfig) -> Router {
    let state = AppState {
        host,
        config: Arc::new(config),
    };
    register_core(state.clone())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            cors_and_preflight,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            route_group_guard,
        ))
        .layer(DefaultBodyLimit::max(MAX_BODY_SIZE))
        .with_state(state)
}

/// OPTIONS 预检 + CORS 响应头。
///
/// **默认（`cors_allow_all == false`）**：预检统一回 204 且不带任何
/// `Access-Control-*` 头，恶意网页的跨域预检失败 —— 见 [`crate::auth`]
/// 安全模型第 2 条。
///
/// **用户显式开启后**：预检与真实响应都带 `Access-Control-Allow-Origin: *`，
/// 等价于 aria2 的 `--rpc-allow-origin-all`。`Allow-Headers` 原样回显请求的
/// `Access-Control-Request-Headers`（缺省 `*`），使 `X-FluxDown-Client` /
/// `X-FluxDown-Token` / `Authorization` 等自定义头都能过检；额外带
/// `Access-Control-Allow-Private-Network: true` 满足 Chrome 对「公网页面 →
/// 本地回环」的私有网络访问门禁。不发 `Allow-Credentials`：`*` 与凭据互斥，
/// 且本服务鉴权走请求头而非 Cookie，浏览器不会自动附带任何身份。
async fn cors_and_preflight(
    State(state): State<AppState>,
    req: axum::extract::Request,
    next: Next,
) -> Response {
    let allow_all = state.config.cors_enabled();
    if req.method() == Method::OPTIONS {
        let requested_headers = req
            .headers()
            .get(header::ACCESS_CONTROL_REQUEST_HEADERS)
            .cloned();
        let mut resp = (StatusCode::NO_CONTENT, [(header::ALLOW, ALLOWED_METHODS)]).into_response();
        if allow_all {
            let h = resp.headers_mut();
            h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, WILDCARD);
            h.insert(
                header::ACCESS_CONTROL_ALLOW_METHODS,
                HeaderValue::from_static(ALLOWED_METHODS),
            );
            h.insert(
                header::ACCESS_CONTROL_ALLOW_HEADERS,
                requested_headers.unwrap_or(WILDCARD),
            );
            h.insert(
                header::ACCESS_CONTROL_MAX_AGE,
                HeaderValue::from_static("600"),
            );
            h.insert(ALLOW_PRIVATE_NETWORK, HeaderValue::from_static("true"));
        }
        return resp;
    }
    let mut resp = next.run(req).await;
    if allow_all {
        resp.headers_mut()
            .insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, WILDCARD);
    }
    resp
}

async fn route_group_guard(
    State(state): State<AppState>,
    req: axum::extract::Request,
    next: Next,
) -> Response {
    let path = req.uri().path();
    let group = if path == routes::DOWNLOAD || path == routes::DOWNLOAD_BATCH {
        Some(RouteGroup::Takeover)
    } else if path == routes::JSONRPC {
        Some(RouteGroup::JsonRpc)
    } else if path == routes::MCP {
        Some(RouteGroup::Mcp)
    } else if path.starts_with(routes::API_PREFIX)
        && !matches!(
            path,
            routes::API_LINK_PAIR_HELLO | routes::API_LINK_PAIR_CONFIRM | routes::API_LINK_TASKS
        )
    {
        Some(RouteGroup::Management)
    } else {
        None
    };
    if group.is_some_and(|group| !state.config.route_enabled(group)) {
        return unknown_endpoint().await;
    }
    next.run(req).await
}

async fn unknown_endpoint() -> Response {
    result_response(StatusCode::NOT_FOUND, false, UNKNOWN_ENDPOINT_MESSAGE)
}

/// `{"success":bool,"message":...}` 形态响应（接管端点与错误统一格式）。
fn result_response(status: StatusCode, success: bool, message: &str) -> Response {
    (
        status,
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({ "success": success, "message": message })),
    )
        .into_response()
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self {
            ApiError::NotFound => StatusCode::NOT_FOUND,
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::Conflict(_) => StatusCode::CONFLICT,
            ApiError::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
            ApiError::Unauthorized => StatusCode::UNAUTHORIZED,
            ApiError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        match &self {
            ApiError::Internal(_) => {
                tracing::error!(error = %self, "local API request failed");
            }
            ApiError::Unavailable => {
                tracing::warn!(error = %self, "local API host unavailable");
            }
            ApiError::NotFound
            | ApiError::BadRequest(_)
            | ApiError::Conflict(_)
            | ApiError::Unauthorized => {}
        }
        result_response(status, false, &self.to_string())
    }
}

// ---------------------------------------------------------------------------
// 探活
// ---------------------------------------------------------------------------

/// 探活。返回应用名、版本号与 `pong`；宿主配置了 Web UI 语言时附带 `language`
/// （无鉴权——登录前的前端靠它决定界面默认语言；经 [`ApiHost::web_language`]
/// 实时求值，配置变更无需重启）。
#[utoipa::path(get, path = "/ping", tag = "system",
    responses((status = 200, description = "应用存活，返回 app/version/message；配置了 Web 语言时附带 language"))
)]
pub(crate) async fn ping(State(state): State<AppState>) -> Response {
    let mut body = json!({
        "success": true,
        "app": "FluxDown",
        "version": state.config.app_version,
        "message": "pong",
    });
    if let Some(lang) = state.host.web_language().await {
        body["language"] = json!(lang);
    }
    if let Some(info) = state.host.link_ping_info().await {
        body["linkFingerprint"] = json!(info.fingerprint);
        body["linkName"] = json!(info.name);
        if !info.platform.is_empty() {
            body["linkPlatform"] = json!(info.platform);
        }
    }
    ([(header::CACHE_CONTROL, "no-store")], Json(body)).into_response()
}

// ---------------------------------------------------------------------------
// P2P 设备互联端点（无 token 鉴权：配对由一次性码 + SAS 守卫；数据面由链路 HMAC）
// ---------------------------------------------------------------------------

/// 处理配对 `hello`（发起方 → 本机）。
#[utoipa::path(post, path = "/api/v1/link/pair/hello", tag = "link",
    description = "配对握手第一步（发起方 → 响应方）。**无 token 鉴权**：由响应方 UI 展示的一次性配对码守卫，重复/过期码拒绝。",
    request_body = LinkPairHelloRequest,
    responses(
        (status = 200, description = "响应方临时公钥 + SAS 材料", body = fluxdown_protocol::daemon::LinkPairHelloResponse),
        (status = 400, description = "配对码错误/过期/已用，或载荷非法", body = fluxdown_protocol::daemon::ResultMessage),
    )
)]
pub(crate) async fn api_link_pair_hello(
    State(state): State<AppState>,
    parts: axum::http::request::Parts,
    body: Bytes,
) -> Response {
    let req: LinkPairHelloRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return result_response(
                StatusCode::BAD_REQUEST,
                false,
                &format!("invalid link hello payload: {e}"),
            );
        }
    };
    // 从请求扩展里**可选地**取 `ConnectInfo` 而不是把它写成提取器：裸 `ConnectInfo`
    // 在某个 serve 站点没挂 `into_make_service_with_connect_info` 时会直接拒绝请求，
    // 把「忘配置」放大成「配对功能整体不可用」；而 axum 0.8 的 `Option<ConnectInfo<_>>`
    // 并不满足 `OptionalFromRequestParts`，无法直接写成可选提取器。拿不到就传 None，
    // 引擎侧节流器落 "unknown" 分桶，不阻断主流程。
    //
    // 取的是 TCP 层真实对端地址，不解析 `X-Forwarded-For`——本功能面向局域网直连，
    // 反代场景不在设计内，仓库里也没有解析 XFF 的既有惯例，引入反而给伪造来源留后门。
    let source = parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.ip());
    match state.host.link_pair_hello(req, source).await {
        Ok(resp) => Json(resp).into_response(),
        Err(e) => e.into_response(),
    }
}

/// 处理配对 `confirm`（SAS 核对后确认/拒绝）。
#[utoipa::path(post, path = "/api/v1/link/pair/confirm", tag = "link",
    description = "配对第二步：核对 SAS 后确认/拒绝（响应方内部会话，由 pair/hello 建立的 sessionId 守卫）。**无 token 鉴权**。",
    request_body = LinkPairConfirmRequest,
    responses(
        (status = 200, description = "`{success,paired,reason}`"),
        (status = 400, description = "会话不存在/已过期，或载荷非法", body = fluxdown_protocol::daemon::ResultMessage),
    )
)]
pub(crate) async fn api_link_pair_confirm(State(state): State<AppState>, body: Bytes) -> Response {
    let req: LinkPairConfirmRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return result_response(
                StatusCode::BAD_REQUEST,
                false,
                &format!("invalid link confirm payload: {e}"),
            );
        }
    };
    match state.host.link_pair_confirm(req).await {
        Ok(outcome) => Json(json!({
            "success": true,
            "paired": outcome.paired(),
            "reason": outcome.reason(),
        }))
        .into_response(),
        Err(e) => e.into_response(),
    }
}

/// 已配对设备下发下载任务：从 `X-FluxLink-*` 头取鉴权凭据 → 校验 → 建任务。
#[utoipa::path(post, path = "/api/v1/link/tasks", tag = "link",
    description = "数据面：已配对设备下发下载任务。**无 management token**，鉴权靠 `X-FluxLink-Device`/`X-FluxLink-Ts`/`X-FluxLink-Nonce`/`X-FluxLink-Auth` 头做每对设备独立密钥的 HMAC 校验。\n\n\
请求体是对明文 JSON 任务描述做 AEAD 加密后的**二进制密文**（`Content-Type: application/octet-stream`），非普通 JSON；宿主校验鉴权头后解密再反序列化。",
    responses(
        (status = 200, description = "创建成功", body = fluxdown_protocol::daemon::CreatedTask),
        (status = 400, description = "载荷非法（含解密失败）", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "缺少/无效链路鉴权头", body = fluxdown_protocol::daemon::ResultMessage),
    )
)]
pub(crate) async fn api_link_create_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let h = |k: &str| headers.get(k).and_then(|v| v.to_str().ok()).unwrap_or("");
    let auth = LinkAuth {
        device: h("x-fluxlink-device").to_string(),
        ts: h("x-fluxlink-ts").parse::<i64>().unwrap_or(0),
        nonce: h("x-fluxlink-nonce").to_string(),
        tag: h("x-fluxlink-auth").to_string(),
        enc: h("x-fluxlink-enc").to_string(),
    };
    if auth.device.is_empty() || auth.tag.is_empty() {
        return result_response(StatusCode::UNAUTHORIZED, false, "missing link auth headers");
    }
    // 传原始 body 字节给宿主：HMAC 覆盖了 body 摘要，宿主须用**收到的原始字节**
    // 校验后再反序列化，不能先解析（重序列化字节可能与签名不一致）。
    match state.host.link_create_task(auth, body.to_vec()).await {
        Ok(task_id) => Json(CreatedTask { task_id }).into_response(),
        Err(e) => e.into_response(),
    }
}

/// 生成一次性配对码（**需 management token**）。供 web/CLI 让 headless 设备出示。
#[utoipa::path(post, path = "/api/v1/link/code", tag = "link",
    description = "生成一次性配对码，供发起方在 pair/hello 出示。**需 management token**。",
    responses(
        (status = 200, description = "配对码 + 有效期", body = fluxdown_protocol::daemon::LinkCodeResponse),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_link_generate_code(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    match state.host.link_generate_code().await {
        Ok(resp) => Json(resp).into_response(),
        Err(e) => e.into_response(),
    }
}

/// 停止 mDNS 广播（撤销「可被发现」状态）；配对码本身若未过期仍可用
/// （手动地址/已知连接仍能核对），只是不再出现在局域网 mDNS 扫描里。
#[utoipa::path(delete, path = "/api/v1/link/code", tag = "link",
    description = "停止 mDNS 广播（撤销「可被发现」状态）；配对码本身未过期仍可用，只是不再出现在局域网扫描里。**需 management token**。",
    responses(
        (status = 200, description = "已停止", body = fluxdown_protocol::daemon::LinkOkResponse),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_link_stop_advertising(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    match state.host.link_stop_advertising().await {
        Ok(()) => Json(LinkOkResponse { ok: true }).into_response(),
        Err(e) => e.into_response(),
    }
}

// ---------------------------------------------------------------------------
// 本地互联管理面（/api/v1/link/*，强制 token；供 web/PC 一致驱动 LinkManager，
// 契约见 docs/link_mgmt_contract.md）
// ---------------------------------------------------------------------------

/// 本地设备发现开关：`start` 幂等且清空发现快照；`stop` 停止 mDNS 浏览。
#[utoipa::path(post, path = "/api/v1/link/discovery", tag = "link",
    description = "本地设备发现开关：`start` 幂等且清空发现快照，`stop` 停止 mDNS 浏览。**需 management token**。",
    request_body = LinkDiscoveryRequest,
    responses(
        (status = 200, description = "已切换", body = fluxdown_protocol::daemon::LinkOkResponse),
        (status = 400, description = "action 非法", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_link_discovery(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    let req: LinkDiscoveryRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return result_response(
                StatusCode::BAD_REQUEST,
                false,
                &format!("invalid discovery payload: {e}"),
            );
        }
    };
    let start = match req.action.as_str() {
        "start" => true,
        "stop" => false,
        other => {
            return result_response(
                StatusCode::BAD_REQUEST,
                false,
                &format!("unknown discovery action: {other}"),
            );
        }
    };
    match state.host.link_discovery(start).await {
        Ok(()) => Json(LinkOkResponse { ok: true }).into_response(),
        Err(e) => e.into_response(),
    }
}

/// 当前发现快照（发起方侧 UI 轮询）。
#[utoipa::path(get, path = "/api/v1/link/discovered", tag = "link",
    description = "当前发现快照（发起方侧 UI 轮询）。**需 management token**。",
    responses(
        (status = 200, description = "发现到的对端列表", body = fluxdown_protocol::daemon::LinkDiscoveredResponse),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_link_discovered(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    match state.host.link_discovered().await {
        Ok(peers) => Json(LinkDiscoveredResponse { peers }).into_response(),
        Err(e) => e.into_response(),
    }
}

/// 手动地址探测（mDNS 失效兜底）；结果不入发现快照。
#[utoipa::path(post, path = "/api/v1/link/probe", tag = "link",
    description = "手动地址探测（mDNS 失效兜底）；结果不入发现快照，直接返回给调用方。**需 management token**。",
    request_body = LinkProbeRequest,
    responses(
        (status = 200, description = "探测到的对端信息", body = fluxdown_protocol::daemon::LinkDiscoveredPeer),
        (status = 400, description = "载荷非法", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 503, description = "对端不可达", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_link_probe(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    let req: LinkProbeRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return result_response(
                StatusCode::BAD_REQUEST,
                false,
                &format!("invalid probe payload: {e}"),
            );
        }
    };
    match state.host.link_probe(&req.host, req.port).await {
        Ok(peer) => Json(peer).into_response(),
        Err(e) => e.into_response(),
    }
}

/// 发起配对：向 `host:port` 发送 `hello`（带配对码），返回待确认令牌 + SAS +
/// 对端信息，供 UI 展示 SAS 核对后调 [`api_link_pair_finish`]。
#[utoipa::path(post, path = "/api/v1/link/pair/begin", tag = "link",
    description = "发起配对：向 `host:port` 发送 hello（带配对码），返回待确认令牌 + SAS，供 UI 展示核对后调用 pair/finish。**需 management token**。",
    request_body = LinkPairBeginRequest,
    responses(
        (status = 200, description = "待确认令牌 + SAS + 对端信息", body = fluxdown_protocol::daemon::LinkPairBeginResponse),
        (status = 400, description = "配对码错误，或载荷非法", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 503, description = "对端不可达，或本机待确认配对已达上限", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_link_pair_begin(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    let req: LinkPairBeginRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return result_response(
                StatusCode::BAD_REQUEST,
                false,
                &format!("invalid pair begin payload: {e}"),
            );
        }
    };
    match state
        .host
        .link_pair_begin(&req.host, req.port, &req.code)
        .await
    {
        Ok(resp) => Json(resp).into_response(),
        Err(e) => e.into_response(),
    }
}

/// SAS 核对后确认/拒绝配对（管理面版本，区别于响应方内部 `pair/confirm`）。
#[utoipa::path(post, path = "/api/v1/link/pair/finish", tag = "link",
    description = "SAS 核对后确认/拒绝配对（管理面视角，区别于响应方内部 `pair/confirm`）。**需 management token**。",
    request_body = LinkPairFinishRequest,
    responses(
        (status = 200, description = "`paired=false` 表示对端拒绝，此时 device 省略", body = fluxdown_protocol::daemon::LinkPairFinishResponse),
        (status = 400, description = "令牌不存在/已过期，或载荷非法", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_link_pair_finish(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    let req: LinkPairFinishRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return result_response(
                StatusCode::BAD_REQUEST,
                false,
                &format!("invalid pair finish payload: {e}"),
            );
        }
    };
    match state.host.link_pair_finish(&req.token, req.accept).await {
        Ok(device) => Json(LinkPairFinishResponse {
            paired: device.is_some(),
            device,
        })
        .into_response(),
        Err(e) => e.into_response(),
    }
}

/// 批准/拒绝一次入站配对核验（响应本机收到的 `IncomingPairing` 通知；区别于
/// 发起方视角、核对 SAS 后调用的 [`api_link_pair_finish`]）。
#[utoipa::path(post, path = "/api/v1/link/pair/approve", tag = "link",
    description = "批准/拒绝一次入站配对核验（响应本机收到的 `IncomingPairing` 通知；区别于发起方视角、核对 SAS 后调用的 pair/finish）。**需 management token**。",
    request_body = LinkPairApproveRequest,
    responses(
        (status = 200, description = "已处理", body = fluxdown_protocol::daemon::LinkOkResponse),
        (status = 400, description = "会话不存在/已过期，或载荷非法", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_link_pair_approve(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    let req: LinkPairApproveRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return result_response(
                StatusCode::BAD_REQUEST,
                false,
                &format!("invalid pair approve payload: {e}"),
            );
        }
    };
    match state
        .host
        .link_approve_incoming(&req.session_id, req.accept)
        .await
    {
        Ok(()) => Json(LinkOkResponse { ok: true }).into_response(),
        Err(e) => e.into_response(),
    }
}

/// 已配对设备列表（含并发在线探测）。
#[utoipa::path(get, path = "/api/v1/link/devices", tag = "link",
    description = "已配对设备列表（含并发在线探测）。**需 management token**。",
    responses(
        (status = 200, description = "设备列表", body = fluxdown_protocol::daemon::LinkDevicesResponse),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_link_devices(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    match state.host.link_devices().await {
        Ok(devices) => Json(LinkDevicesResponse { devices }).into_response(),
        Err(e) => e.into_response(),
    }
}

/// 解除配对（删除设备）。
#[utoipa::path(delete, path = "/api/v1/link/devices/{fingerprint}", tag = "link",
    description = "解除配对（删除设备记录及双方链路密钥）。**需 management token**。",
    params(("fingerprint" = String, Path, description = "设备指纹")),
    responses(
        (status = 200, description = "已解除", body = fluxdown_protocol::daemon::LinkOkResponse),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 404, description = "设备不存在", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_link_remove_device(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(fingerprint): Path<String>,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    match state.host.link_remove_device(&fingerprint).await {
        Ok(true) => Json(LinkOkResponse { ok: true }).into_response(),
        Ok(false) => ApiError::NotFound.into_response(),
        Err(e) => e.into_response(),
    }
}

/// 下发下载任务给已配对设备（管理面，token 鉴权；区别于数据面链路 HMAC 鉴权的
/// `POST /api/v1/link/tasks`）。
#[utoipa::path(post, path = "/api/v1/link/devices/{fingerprint}/tasks", tag = "link",
    description = "下发下载任务给已配对设备（管理面，token 鉴权；区别于数据面链路 HMAC 鉴权的 `POST /api/v1/link/tasks`）。**需 management token**。",
    params(("fingerprint" = String, Path, description = "目标设备指纹")),
    request_body = LinkDeviceTaskRequest,
    responses(
        (status = 200, description = "创建成功", body = fluxdown_protocol::daemon::CreatedTask),
        (status = 400, description = "载荷非法", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效，或目标设备不存在/未配对", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_link_device_tasks(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(fingerprint): Path<String>,
    body: Bytes,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    let req: LinkDeviceTaskRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return result_response(
                StatusCode::BAD_REQUEST,
                false,
                &format!("invalid link task payload: {e}"),
            );
        }
    };
    match state
        .host
        .link_dispatch(
            &fingerprint,
            &req.url,
            req.save_dir.as_deref(),
            req.file_name.as_deref(),
        )
        .await
    {
        Ok(task_id) => Json(CreatedTask { task_id }).into_response(),
        Err(e) => e.into_response(),
    }
}

// ---------------------------------------------------------------------------
// 脚本接管端点
// ---------------------------------------------------------------------------

/// 提交单个外部下载请求。用 `Bytes` 而非 `Json` 提取：油猴脚本可能不带
/// `application/json` Content-Type（保留旧实现的宽容行为）。
#[utoipa::path(post, path = "/download", tag = "takeover",
    request_body = DownloadRequest,
    responses(
        (status = 200, description = "已受理，进入快速下载确认流程", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 400, description = "载荷非法或缺少 url", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 403, description = "缺少 X-FluxDown-Client 头", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("tokenHeader" = []))
)]
pub(crate) async fn takeover_download(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err((code, msg)) = check_takeover_auth(&headers, &state.config.token.get()) {
        return result_response(status_from(code), false, msg);
    }
    let dl: DownloadRequest = match serde_json::from_slice(&body) {
        Ok(dl) => dl,
        Err(e) => {
            return result_response(
                StatusCode::BAD_REQUEST,
                false,
                &format!("invalid download payload: {e}"),
            );
        }
    };
    submit_external(&state, dl).await
}

/// 批量提交外部下载请求。支持 `{"urls":[...]}` 与 `{"items":[{...}]}` 两种形态，
/// 合并为单次确认。鉴权与 `/download` 相同。
#[utoipa::path(post, path = "/download/batch", tag = "takeover",
    responses(
        (status = 200, description = "已受理", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 400, description = "载荷非法", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("tokenHeader" = []))
)]
pub(crate) async fn takeover_download_batch(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err((code, msg)) = check_takeover_auth(&headers, &state.config.token.get()) {
        return result_response(status_from(code), false, msg);
    }
    match parse_batch(&body) {
        Ok(dl) => {
            let count = dl.url.split('\n').filter(|s| !s.trim().is_empty()).count();
            tracing::info!(url_count = count, "received external download batch");
            submit_external(&state, dl).await
        }
        Err(e) => result_response(StatusCode::BAD_REQUEST, false, &e),
    }
}

async fn submit_external(state: &AppState, dl: DownloadRequest) -> Response {
    if dl.url.trim().is_empty() {
        return result_response(StatusCode::BAD_REQUEST, false, "url is required");
    }
    tracing::info!(url = %dl.url, "received external download");
    match state.host.submit_external(dl).await {
        Ok(()) => result_response(StatusCode::OK, true, "download accepted"),
        Err(e) => e.into_response(),
    }
}

fn status_from(code: u16) -> StatusCode {
    StatusCode::from_u16(code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

// ---------------------------------------------------------------------------
// aria2 JSON-RPC 兼容端点
// ---------------------------------------------------------------------------

/// aria2 客户端约定：HTTP 层始终 200，错误在 JSON-RPC envelope 内表达。
#[utoipa::path(post, path = "/jsonrpc", tag = "aria2",
    responses((status = 200, description = "JSON-RPC 响应（错误在 envelope 内表达）。支持方法：aria2.addUri / aria2.getVersion / aria2.getGlobalStat / system.multicall / system.listMethods；token 可经 X-FluxDown-Token 头或 params[0]=\"token:xxx\" 传递")),
    security(("tokenHeader" = []))
)]
pub(crate) async fn jsonrpc(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let token = state.config.token.get();
    let token_ok = header_token_ok(&headers, &token);
    let resp = handle_jsonrpc(state.host.as_ref(), &token, token_ok, &body).await;
    ([(header::CACHE_CONTROL, "no-store")], Json(resp)).into_response()
}

/// aria2 WS 通知 + 双向 JSON-RPC。GET 带 WebSocket upgrade 头时握手升级，进入
/// [`run_session`] 会话循环；非 upgrade 的普通 GET 由 [`WebSocketUpgrade`]
/// 提取器自身拒绝（400 Bad Request，见 axum `WebSocketUpgradeRejection`），
/// 本函数体不会被调用。与 `POST /jsonrpc` 共用 `jsonrpc_enabled` 开关
/// （[`register_core`] 里同一个 `if` 分支内注册）。
pub(crate) async fn jsonrpc_ws(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| async move {
        let events = state.host.subscribe_task_events();
        let token = state.config.token.get();
        run_session(socket, state.host.as_ref(), &token, events).await;
    })
}

// ---------------------------------------------------------------------------
// MCP 兼容端点
// ---------------------------------------------------------------------------

/// MCP（Model Context Protocol）端点。强制 token 鉴权（Bearer /
/// X-FluxDown-Token，复用管理 API 门禁）。请求返回 `200 application/json`
/// JSON-RPC 响应；通知（无 `id`）返回 `202 Accepted` 空体。
#[utoipa::path(post, path = "/mcp", tag = "mcp",
    responses(
        (status = 200, description = "JSON-RPC 响应（initialize / tools/list / tools/call / ping）"),
        (status = 202, description = "通知已接受（无响应体）"),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 403, description = "服务端未配置 token", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn mcp(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err((code, msg)) = check_management_auth(&headers, &state.config.token.get()) {
        return result_response(status_from(code), false, msg);
    }
    match handle_mcp(state.host.as_ref(), &state.config.app_version, &body).await {
        Some(resp) => ([(header::CACHE_CONTROL, "no-store")], Json(resp)).into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

// ---------------------------------------------------------------------------
// 管理 API（/api/v1）
// ---------------------------------------------------------------------------

/// 管理 API 统一鉴权入口。`Err` 装箱：`Response` 体积大，避免撑大每个
/// handler 的返回路径（clippy::result_large_err）。
fn guard(state: &AppState, headers: &HeaderMap) -> Result<(), Box<Response>> {
    check_management_auth(headers, &state.config.token.get())
        .map_err(|(code, msg)| Box::new(result_response(status_from(code), false, msg)))
}

/// 应用信息（名称与版本号）。
#[utoipa::path(get, path = "/api/v1/info", tag = "management",
    responses(
        (status = 200, description = "应用信息", body = fluxdown_protocol::daemon::ApiInfo),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 403, description = "服务端未配置 token", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_info(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    Json(fluxdown_protocol::daemon::ApiInfo {
        name: "FluxDown".to_string(),
        version: state.config.app_version.clone(),
    })
    .into_response()
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub(crate) struct TaskListQuery {
    /// 按状态过滤：0=pending, 1=downloading, 2=paused, 3=completed, 4=error, 5=preparing
    status: Option<i32>,
}

/// 列出全部任务，可按状态过滤。
#[utoipa::path(get, path = "/api/v1/tasks", tag = "management",
    params(TaskListQuery),
    responses(
        (status = 200, description = "任务列表", body = Vec<fluxdown_protocol::daemon::TaskDto>),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_list_tasks(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<TaskListQuery>,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    match state.host.list_tasks().await {
        Ok(mut tasks) => {
            if let Some(status) = q.status {
                tasks.retain(|t| t.status == status);
            }
            Json(tasks).into_response()
        }
        Err(e) => e.into_response(),
    }
}

/// 直接创建下载任务（不弹确认框），返回新任务 ID。
#[utoipa::path(post, path = "/api/v1/tasks", tag = "management",
    request_body = CreateTaskRequest,
    responses(
        (status = 200, description = "创建成功", body = fluxdown_protocol::daemon::CreatedTask),
        (status = 400, description = "载荷非法或缺少 url", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 503, description = "应用关闭中", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_create_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    let req: CreateTaskRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return result_response(
                StatusCode::BAD_REQUEST,
                false,
                &format!("invalid create payload: {e}"),
            );
        }
    };
    if req.url.trim().is_empty() {
        return result_response(StatusCode::BAD_REQUEST, false, "url is required");
    }
    match state.host.create_task(req).await {
        Ok(task_id) => Json(CreatedTask { task_id }).into_response(),
        Err(e) => e.into_response(),
    }
}

/// 按 ID 查询单个任务。
#[utoipa::path(get, path = "/api/v1/tasks/{id}", tag = "management",
    params(("id" = String, Path, description = "任务 ID（UUID）")),
    responses(
        (status = 200, description = "任务信息", body = fluxdown_protocol::daemon::TaskDto),
        (status = 404, description = "任务不存在", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_get_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    match state.host.get_task(&id).await {
        Ok(Some(task)) => Json(task).into_response(),
        Ok(None) => ApiError::NotFound.into_response(),
        Err(e) => e.into_response(),
    }
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeleteTaskQuery {
    /// true = 同时删除磁盘文件。默认 false（仅删记录）。
    #[serde(default)]
    delete_files: bool,
}

/// 删除任务，可选同时删除磁盘文件。
#[utoipa::path(delete, path = "/api/v1/tasks/{id}", tag = "management",
    params(("id" = String, Path, description = "任务 ID（UUID）"), DeleteTaskQuery),
    responses(
        (status = 200, description = "已删除", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 404, description = "任务不存在", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_delete_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<DeleteTaskQuery>,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    ack(state.host.delete_task(&id, q.delete_files).await)
}

/// 暂停单个任务。
#[utoipa::path(put, path = "/api/v1/tasks/{id}/pause", tag = "management",
    params(("id" = String, Path, description = "任务 ID（UUID）")),
    responses(
        (status = 200, description = "已暂停", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 404, description = "任务不存在", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_pause_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    ack(state.host.pause_task(&id).await)
}

/// 恢复单个任务。
#[utoipa::path(put, path = "/api/v1/tasks/{id}/continue", tag = "management",
    params(("id" = String, Path, description = "任务 ID（UUID）")),
    responses(
        (status = 200, description = "已恢复", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 404, description = "任务不存在", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_continue_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    ack(state.host.continue_task(&id).await)
}

/// 重命名任务文件。
#[utoipa::path(post, path = "/api/v1/tasks/{id}/rename", tag = "management",
    params(("id" = String, Path, description = "任务 ID（UUID）")),
    request_body = fluxdown_protocol::daemon::RenameTaskRequest,
    responses(
        (status = 200, description = "已重命名", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 400, description = "文件名非法（message 为错误码 `invalid-name`）", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 404, description = "任务不存在", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 409, description = "业务拒绝（message 为错误码 `task-active` / `bt-unsupported` / `target-exists`）", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_rename_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    let req: fluxdown_protocol::daemon::RenameTaskRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return result_response(
                StatusCode::BAD_REQUEST,
                false,
                &format!("invalid payload: {e}"),
            );
        }
    };
    ack(state.host.rename_task(&id, &req.file_name).await)
}

/// 暂停全部活跃任务（pending / downloading / preparing）。
#[utoipa::path(put, path = "/api/v1/tasks/pause", tag = "management",
    responses(
        (status = 200, description = "已全部暂停", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_pause_all(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    ack(state.host.pause_all().await)
}

/// 恢复全部已暂停任务。
#[utoipa::path(put, path = "/api/v1/tasks/continue", tag = "management",
    responses(
        (status = 200, description = "已全部恢复", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_continue_all(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    ack(state.host.continue_all().await)
}

/// 列出全部命名队列。
#[utoipa::path(get, path = "/api/v1/queues", tag = "management",
    responses(
        (status = 200, description = "队列列表", body = Vec<fluxdown_protocol::daemon::QueueDto>),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_list_queues(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    match state.host.list_queues().await {
        Ok(queues) => Json(queues).into_response(),
        Err(e) => e.into_response(),
    }
}

// ---------------------------------------------------------------------------
// 任务组与前置预解析（/api/v1/groups*、/api/v1/resolve/preview，全强制 token；
// docs/multi-file-task-group-design.md Phase D）
// ---------------------------------------------------------------------------

/// 前置预解析清单（多文件清单，只读、不建任务、不写库）。强制鉴权——
/// 会触发插件网络调用（网盘 API），与管理 API 其余端点同一门禁。
#[utoipa::path(post, path = "/api/v1/resolve/preview", tag = "groups",
    request_body = ResolvePreviewRequest,
    responses(
        (status = 200, description = "预解析结果（items 为空且 error 为空 = 插件未返回清单）", body = fluxdown_protocol::daemon::ResolvePreviewResponse),
        (status = 400, description = "载荷非法或缺少 url", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_resolve_preview(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    let req: ResolvePreviewRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return result_response(
                StatusCode::BAD_REQUEST,
                false,
                &format!("invalid resolve preview payload: {e}"),
            );
        }
    };
    if req.url.trim().is_empty() {
        return result_response(StatusCode::BAD_REQUEST, false, "url is required");
    }
    match state.host.resolve_preview(req).await {
        Ok(resp) => Json(resp).into_response(),
        Err(e) => e.into_response(),
    }
}

/// 列出全部任务组。
#[utoipa::path(get, path = "/api/v1/groups", tag = "groups",
    responses(
        (status = 200, description = "任务组列表", body = Vec<fluxdown_protocol::daemon::GroupDto>),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_list_groups(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    match state.host.list_groups().await {
        Ok(groups) => Json(groups).into_response(),
        Err(e) => e.into_response(),
    }
}

/// 创建多文件任务组（建组 + N 子任务），返回新组 ID。`items` 为空 → 400。
#[utoipa::path(post, path = "/api/v1/groups", tag = "groups",
    request_body = CreateGroupRequest,
    responses(
        (status = 200, description = "创建成功", body = CreateGroupResponse),
        (status = 400, description = "载荷非法或 items 为空", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 503, description = "应用关闭中", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_create_group(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    let req: CreateGroupRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return result_response(
                StatusCode::BAD_REQUEST,
                false,
                &format!("invalid create group payload: {e}"),
            );
        }
    };
    if req.items.is_empty() {
        return result_response(StatusCode::BAD_REQUEST, false, "items is required");
    }
    match state.host.create_task_group(req).await {
        Ok(group_id) => Json(CreateGroupResponse { group_id }).into_response(),
        Err(e) => e.into_response(),
    }
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeleteGroupQuery {
    /// true = 同时删除磁盘文件。默认 false（仅删记录）。
    #[serde(default)]
    delete_files: bool,
}

/// 删除任务组（批量删成员），可选同时删除磁盘文件。
#[utoipa::path(delete, path = "/api/v1/groups/{id}", tag = "groups",
    params(("id" = String, Path, description = "任务组 ID（UUID）"), DeleteGroupQuery),
    responses(
        (status = 200, description = "已删除", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 404, description = "任务组不存在", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_delete_group(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<DeleteGroupQuery>,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    ack(state.host.group_delete(&id, q.delete_files).await)
}

/// 暂停组内全部成员。
#[utoipa::path(put, path = "/api/v1/groups/{id}/pause", tag = "groups",
    params(("id" = String, Path, description = "任务组 ID（UUID）")),
    responses(
        (status = 200, description = "已暂停", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 404, description = "任务组不存在", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_group_pause(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    ack(state.host.group_pause(&id).await)
}

/// 恢复组内全部成员。
#[utoipa::path(put, path = "/api/v1/groups/{id}/continue", tag = "groups",
    params(("id" = String, Path, description = "任务组 ID（UUID）")),
    responses(
        (status = 200, description = "已恢复", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 404, description = "任务组不存在", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_group_continue(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    ack(state.host.group_continue(&id).await)
}

// ---------------------------------------------------------------------------
// RSS 订阅（/api/v1/rss*，全强制 token）
// ---------------------------------------------------------------------------

/// 列出全部 RSS 订阅。
#[utoipa::path(get, path = "/api/v1/rss", tag = "rss",
    responses(
        (status = 200, description = "订阅列表", body = Vec<fluxdown_protocol::daemon::RssSourceDto>),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_list_rss_sources(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    match state.host.list_rss_sources().await {
        Ok(sources) => Json(sources).into_response(),
        Err(e) => e.into_response(),
    }
}

/// 新建 RSS 订阅，返回新订阅 ID。`url` 为空 → 400。
#[utoipa::path(post, path = "/api/v1/rss", tag = "rss",
    request_body = RssSourceDto,
    responses(
        (status = 200, description = "创建成功，`{sourceId}`"),
        (status = 400, description = "载荷非法或缺少 url", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_create_rss_source(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    let req: RssSourceDto = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return result_response(
                StatusCode::BAD_REQUEST,
                false,
                &format!("invalid rss source payload: {e}"),
            );
        }
    };
    if req.url.trim().is_empty() {
        return result_response(StatusCode::BAD_REQUEST, false, "url is required");
    }
    match state.host.create_rss_source(req).await {
        Ok(source_id) => Json(json!({ "sourceId": source_id })).into_response(),
        Err(e) => e.into_response(),
    }
}

/// 更新订阅配置。运行态字段（`lastFetchAt`/`failCount` 等）写入被忽略。
#[utoipa::path(put, path = "/api/v1/rss/{id}", tag = "rss",
    params(("id" = String, Path, description = "订阅 ID（UUID）")),
    request_body = RssSourceDto,
    responses(
        (status = 200, description = "已更新", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 400, description = "载荷非法", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 404, description = "订阅不存在", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_update_rss_source(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    let req: RssSourceDto = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return result_response(
                StatusCode::BAD_REQUEST,
                false,
                &format!("invalid rss source payload: {e}"),
            );
        }
    };
    ack(state.host.update_rss_source(&id, req).await)
}

/// 删除订阅（级联删条目）。已建下载任务保留——用户要的是不再抓新条目，
/// 不是撤销已下载的东西。
#[utoipa::path(delete, path = "/api/v1/rss/{id}", tag = "rss",
    params(("id" = String, Path, description = "订阅 ID（UUID）")),
    responses(
        (status = 200, description = "已删除", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 404, description = "订阅不存在", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_delete_rss_source(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    ack(state.host.delete_rss_source(&id).await)
}

/// 立即抓取一个订阅。抓取异步派发，本端点只表示「已受理」，结果走
/// `rssSourcesChanged` / `rssItemsChanged` 事件。
#[utoipa::path(post, path = "/api/v1/rss/{id}/refresh", tag = "rss",
    params(("id" = String, Path, description = "订阅 ID（UUID）")),
    responses(
        (status = 200, description = "已派发抓取", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 404, description = "订阅不存在", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_refresh_rss_source(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    ack(state.host.refresh_rss_source(&id).await)
}

/// 一个订阅的条目流（新→旧）。
#[utoipa::path(get, path = "/api/v1/rss/{id}/items", tag = "rss",
    params(("id" = String, Path, description = "订阅 ID（UUID）")),
    responses(
        (status = 200, description = "条目列表", body = Vec<fluxdown_protocol::daemon::RssItemDto>),
        (status = 404, description = "订阅不存在", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_list_rss_items(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    match state.host.list_rss_items(&id).await {
        Ok(items) => Json(items).into_response(),
        Err(e) => e.into_response(),
    }
}

/// 对条目执行手动操作（下载 / 忽略 / 全部标记已读）。guid 在请求体里，
/// 不进路径段——真实 feed 的 guid 常常是一整条 URL。
#[utoipa::path(post, path = "/api/v1/rss/{id}/items/action", tag = "rss",
    params(("id" = String, Path, description = "订阅 ID（UUID）")),
    request_body = RssItemActionRequest,
    responses(
        (status = 200, description = "已执行", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 400, description = "载荷非法或 action 未知", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 404, description = "订阅或条目不存在", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_rss_item_action(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    let req: RssItemActionRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return result_response(
                StatusCode::BAD_REQUEST,
                false,
                &format!("invalid rss item action payload: {e}"),
            );
        }
    };
    ack(state.host.rss_item_action(&id, req).await)
}

/// 只读验证一个 feed 地址（新建订阅向导）。抓取失败**不是** HTTP 错误：
/// 这是一次诊断调用，失败原因本身就是有效载荷，仍回 200 + `error` 非空。
#[utoipa::path(post, path = "/api/v1/rss/validate", tag = "rss",
    request_body = RssValidateRequest,
    responses(
        (status = 200, description = "验证结果（`error` 非空 = 抓取失败）", body = fluxdown_protocol::daemon::RssValidateResponse),
        (status = 400, description = "载荷非法或缺少 url", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_validate_rss_feed(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    let req: RssValidateRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return result_response(
                StatusCode::BAD_REQUEST,
                false,
                &format!("invalid rss validate payload: {e}"),
            );
        }
    };
    if req.url.trim().is_empty() {
        return result_response(StatusCode::BAD_REQUEST, false, "url is required");
    }
    match state.host.validate_rss_feed(req).await {
        Ok(resp) => Json(resp).into_response(),
        Err(e) => e.into_response(),
    }
}

// ---------------------------------------------------------------------------
// 插件系统管理端点（/api/v1/plugins*，全强制 token）
// ---------------------------------------------------------------------------

/// 插件 zip 上传上限（10MB）。
const MAX_PLUGIN_ZIP: usize = 10 * 1024 * 1024;

/// 列出全部已安装插件。
#[utoipa::path(get, path = "/api/v1/plugins", tag = "plugins",
    responses(
        (status = 200, description = "插件列表", body = Vec<fluxdown_protocol::daemon::PluginDto>),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_list_plugins(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    match state.host.list_plugins().await {
        Ok(plugins) => Json(plugins).into_response(),
        Err(e) => e.into_response(),
    }
}

/// 从 zip 安装插件（≤10MB）。
#[utoipa::path(post, path = "/api/v1/plugins/install", tag = "plugins",
    responses(
        (status = 200, description = "安装成功", body = fluxdown_protocol::daemon::InstalledPlugin),
        (status = 400, description = "zip 非法或超限", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_install_plugin(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    if body.len() > MAX_PLUGIN_ZIP {
        return result_response(
            StatusCode::BAD_REQUEST,
            false,
            "plugin zip too large (>10MB)",
        );
    }
    match state.host.install_plugin_zip(body.to_vec()).await {
        Ok(identity) => installed_response(&state, identity).await,
        Err(e) => e.into_response(),
    }
}

/// dev 安装插件（引用目录，不拷贝）。
#[utoipa::path(post, path = "/api/v1/plugins/install-dev", tag = "plugins",
    request_body = fluxdown_protocol::daemon::InstallPluginDevRequest,
    responses(
        (status = 200, description = "安装成功", body = fluxdown_protocol::daemon::InstalledPlugin),
        (status = 400, description = "路径非法", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_install_plugin_dev(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    let req: fluxdown_protocol::daemon::InstallPluginDevRequest =
        match serde_json::from_slice(&body) {
            Ok(r) => r,
            Err(e) => {
                return result_response(
                    StatusCode::BAD_REQUEST,
                    false,
                    &format!("invalid payload: {e}"),
                );
            }
        };
    match state.host.install_plugin_dev(req.dir_path).await {
        Ok(identity) => installed_response(&state, identity).await,
        Err(e) => e.into_response(),
    }
}

/// 启用/禁用插件。
#[utoipa::path(put, path = "/api/v1/plugins/{identity}/enabled", tag = "plugins",
    params(("identity" = String, Path, description = "插件 identity")),
    request_body = fluxdown_protocol::daemon::SetPluginEnabledRequest,
    responses(
        (status = 200, description = "已更新", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_set_plugin_enabled(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(identity): Path<String>,
    body: Bytes,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    let req: fluxdown_protocol::daemon::SetPluginEnabledRequest =
        match serde_json::from_slice(&body) {
            Ok(r) => r,
            Err(e) => {
                return result_response(
                    StatusCode::BAD_REQUEST,
                    false,
                    &format!("invalid payload: {e}"),
                );
            }
        };
    ack(state.host.set_plugin_enabled(&identity, req.enabled).await)
}

/// 批量更新插件设置（all-or-nothing）。请求体为 `{key: value}` 字符串映射。
#[utoipa::path(put, path = "/api/v1/plugins/{identity}/settings", tag = "plugins",
    params(("identity" = String, Path, description = "插件 identity")),
    responses(
        (status = 200, description = "已保存", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 400, description = "设置校验失败", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_update_plugin_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(identity): Path<String>,
    body: Bytes,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    let entries: std::collections::HashMap<String, String> = match serde_json::from_slice(&body) {
        Ok(m) => m,
        Err(e) => {
            return result_response(
                StatusCode::BAD_REQUEST,
                false,
                &format!("invalid payload: {e}"),
            );
        }
    };
    ack(state.host.update_plugin_settings(&identity, entries).await)
}

/// 卸载插件。
#[utoipa::path(delete, path = "/api/v1/plugins/{identity}", tag = "plugins",
    params(("identity" = String, Path, description = "插件 identity")),
    responses(
        (status = 200, description = "已卸载", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_uninstall_plugin(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(identity): Path<String>,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    ack(state.host.uninstall_plugin(&identity).await)
}

/// 任务级逃生舱：忽略插件重试，按原始链接重跑。
#[utoipa::path(post, path = "/api/v1/tasks/{id}/ignore-plugin-retry", tag = "plugins",
    params(("id" = String, Path, description = "任务 ID")),
    responses(
        (status = 200, description = "已按原始链接重跑", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_ignore_plugin_retry(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    ack(state.host.ignore_plugin_retry(&id).await)
}

/// 拉取去中心化插件市场索引。
#[utoipa::path(get, path = "/api/v1/market", tag = "plugins",
    responses(
        (status = 200, description = "市场索引条目", body = Vec<fluxdown_protocol::daemon::MarketEntryDto>),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_market_list(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    match state.host.market_list().await {
        Ok(entries) => Json(entries).into_response(),
        Err(e) => e.into_response(),
    }
}

/// 从市场安装某插件最新版。
#[utoipa::path(post, path = "/api/v1/market/install", tag = "plugins",
    request_body = fluxdown_protocol::daemon::MarketInstallRequest,
    responses(
        (status = 200, description = "安装成功", body = fluxdown_protocol::daemon::InstalledPlugin),
        (status = 400, description = "下载/校验/安装失败", body = fluxdown_protocol::daemon::ResultMessage),
        (status = 401, description = "token 无效", body = fluxdown_protocol::daemon::ResultMessage),
    ),
    security(("bearerAuth" = []), ("tokenHeader" = []))
)]
pub(crate) async fn api_market_install(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(resp) = guard(&state, &headers) {
        return *resp;
    }
    let req: fluxdown_protocol::daemon::MarketInstallRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return result_response(
                StatusCode::BAD_REQUEST,
                false,
                &format!("invalid payload: {e}"),
            );
        }
    };
    match state.host.market_install(&req.plugin_id).await {
        Ok(identity) => installed_response(&state, identity).await,
        Err(e) => e.into_response(),
    }
}

/// 安装成功统一返回体：回填缺失基础组件列表（提醒式依赖检查，见
/// [`fluxdown_protocol::daemon::InstalledPlugin`]）。
async fn installed_response(state: &AppState, identity: String) -> Response {
    let missing_components = state.host.plugin_missing_components(&identity).await;
    Json(fluxdown_protocol::daemon::InstalledPlugin {
        identity,
        missing_components,
    })
    .into_response()
}

/// 无返回值操作的统一应答。
fn ack(result: Result<(), ApiError>) -> Response {
    match result {
        Ok(()) => result_response(StatusCode::OK, true, "ok"),
        Err(e) => e.into_response(),
    }
}

/// OpenAPI 3.1 规范（JSON）。无鉴权：纯接口描述，不含任何用户数据。
async fn openapi_spec() -> Response {
    (
        [(header::CONTENT_TYPE, "application/json")],
        crate::openapi::openapi_json(),
    )
        .into_response()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn from_config_map_reads_all_keys_including_new_subswitches() {
        let mut map = HashMap::new();
        map.insert("local_server_enabled".to_string(), "false".to_string());
        map.insert("local_server_port".to_string(), "9999".to_string());
        map.insert("local_server_token".to_string(), "secret".to_string());
        map.insert(
            "local_server_takeover_enabled".to_string(),
            "false".to_string(),
        );
        map.insert(
            "local_server_jsonrpc_enabled".to_string(),
            "false".to_string(),
        );
        map.insert("local_server_api_enabled".to_string(), "true".to_string());
        map.insert(
            "local_server_cors_allow_all".to_string(),
            "true".to_string(),
        );

        let cfg = ApiServerConfig::from_config_map(&map, "2.3.4");

        assert!(!cfg.enabled);
        assert_eq!(cfg.port, 9999);
        assert_eq!(&*cfg.token.get(), "secret");
        assert!(!cfg.takeover_enabled);
        assert!(!cfg.jsonrpc_enabled);
        assert!(cfg.management_enabled);
        assert!(cfg.cors_allow_all);
        assert_eq!(cfg.app_version, "2.3.4");
    }

    /// CORS 放行是安全模型的显式豁免，缺省必须为关——默认值回归会静默让
    /// 任意网页可访问本机服务。
    #[test]
    fn cors_allow_all_defaults_to_false() {
        let cfg = ApiServerConfig::from_config_map(&HashMap::new(), "1.0.0");
        assert!(!cfg.cors_allow_all);
    }

    #[test]
    fn bind_addr_defaults_to_loopback() {
        let mut map = HashMap::new();
        map.insert("local_server_port".to_string(), "12345".to_string());
        let cfg = ApiServerConfig::from_config_map(&map, "1.0.0");
        let addr = cfg.bind_addr();
        assert_eq!(addr.ip().to_string(), "127.0.0.1");
        assert_eq!(addr.port(), 12345);
    }

    #[test]
    fn bind_addr_binds_all_interfaces_when_lan_enabled() {
        let mut map = HashMap::new();
        map.insert("local_server_port".to_string(), "12345".to_string());
        map.insert("local_server_lan_enabled".to_string(), "true".to_string());
        let cfg = ApiServerConfig::from_config_map(&map, "1.0.0");
        assert!(cfg.lan_enabled);
        let addr = cfg.bind_addr();
        assert_eq!(addr.ip().to_string(), "0.0.0.0");
        assert_eq!(addr.port(), 12345);
    }
}
