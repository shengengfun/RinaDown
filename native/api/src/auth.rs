//! 鉴权工具：常量时间比较 + 请求头 token 校验。
//!
//! ## 安全模型（继承自原 http_takeover 模块，逐条保留）
//!
//! 1. **仅监听 `127.0.0.1`**（硬编码，永不监听 `0.0.0.0`），外网不可达。
//! 2. **自定义请求头门禁**：变更类接管端点 `/download`、`/download/batch` 要求
//!    请求带 `X-FluxDown-Client` 头。恶意网页用 `fetch()` 跨域携带自定义头会触发
//!    CORS 预检（OPTIONS），而本服务**不返回** `Access-Control-Allow-Origin`，
//!    预检失败 → 浏览器拦截真实请求。油猴 `GM_xmlhttpRequest` 不受 CORS 约束、
//!    可自由设置该头，故脚本正常工作、恶意网页被挡。**用户可显式开启
//!    `local_server_cors_allow_all` 放弃这道防线**（默认关；见
//!    `ApiServerConfig::cors_allow_all`）——那些把「aria2 探活」写死成浏览器
//!    `fetch` 的网站需要它，代价是任意网页都能探测并提交下载，此时只剩
//!    第 4/5/6 条防护。
//! 3. **JSON-RPC 合法性门禁**：`/jsonrpc` 不校验 `Content-Type`（与真实 aria2
//!    一致），以「请求体能否解析为合法 JSON-RPC」为准入门槛。
//! 4. **可选 token**（`local_server_token` 非空时启用）：请求需带匹配的
//!    `X-FluxDown-Token` 头，常量时间比较，作纵深防御。
//! 5. **管理 API 强制 token**：`/api/v1/*` 在 token 为空时一律拒绝（403），
//!    非空时要求 `Authorization: Bearer <token>` 或 `X-FluxDown-Token` 匹配。
//! 6. **最终安全网**：接管/aria2 入口的下载都会在 FluxDown 中弹出确认框，
//!    杜绝静默下载；管理 API 由强制 token 保护。
//!
//! ## token 的存放形态
//!
//! token 由 [`TokenCell`] 持有而非 `String` 快照：headless 服务器允许「首次
//! 运行设置访问密钥」与「重新生成」在**不重启进程**的前提下改 token，所有读点
//! （核心路由守卫 + 宿主自有扩展路由）必须共享同一份可写状态。桌面宿主不写
//! cell（配置变更走监听器热重启），语义与旧快照一致。

use std::fmt;
use std::sync::{Arc, RwLock};

use axum::http::HeaderMap;

/// 油猴脚本必须携带的来源标识头。
pub const CLIENT_HEADER: &str = "x-fluxdown-client";
/// 可选鉴权 token 头。
pub const TOKEN_HEADER: &str = "x-fluxdown-token";

/// 可热替换的鉴权 token 持有者（克隆共享同一份状态）。
///
/// # Examples
///
/// ```
/// use fluxdown_api::auth::TokenCell;
///
/// let cell = TokenCell::new("");
/// assert!(cell.is_empty());
/// let mirror = cell.clone();
/// cell.set("fxd_abc123");
/// // 克隆体看到同一次写入——路由守卫与扩展路由据此共享 token。
/// assert_eq!(&*mirror.get(), "fxd_abc123");
/// ```
#[derive(Clone)]
pub struct TokenCell(Arc<RwLock<Arc<str>>>);

impl TokenCell {
    /// 以初始 token 构造（空串 = 未配置）。
    #[must_use]
    pub fn new(token: impl Into<Arc<str>>) -> Self {
        Self(Arc::new(RwLock::new(token.into())))
    }

    /// 当前 token 快照。锁中毒时取回内部值——临界区只做克隆/赋值，不会 panic，
    /// 这里恢复而非拒绝，避免把服务器永久锁死在「无 token」状态。
    #[must_use]
    pub fn get(&self) -> Arc<str> {
        match self.0.read() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// 原地替换 token，立即对全部持有者生效。
    pub fn set(&self, token: impl Into<Arc<str>>) {
        let next = token.into();
        match self.0.write() {
            Ok(mut guard) => *guard = next,
            Err(poisoned) => *poisoned.into_inner() = next,
        }
    }

    /// token 是否未配置。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.get().is_empty()
    }
}

impl Default for TokenCell {
    fn default() -> Self {
        Self::new("")
    }
}

/// 只暴露「是否已配置」，绝不把 token 明文打进日志/错误链。
impl fmt::Debug for TokenCell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = if self.is_empty() { "unset" } else { "set" };
        write!(f, "TokenCell({state})")
    }
}

/// 常量时间字符串比较，防 timing attack。
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// `X-FluxDown-Token` 头是否与配置 token 匹配（token 为空 = 不鉴权，恒通过）。
pub fn header_token_ok(headers: &HeaderMap, config_token: &str) -> bool {
    if config_token.is_empty() {
        return true;
    }
    let provided = headers
        .get(TOKEN_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    constant_time_eq(provided, config_token)
}

/// 变更类接管端点门禁：要求 `X-FluxDown-Client` 头存在（挡跨域网页）+ token 校验。
///
/// 返回 `Err((状态码, 消息))`。
pub(crate) fn check_takeover_auth(
    headers: &HeaderMap,
    config_token: &str,
) -> Result<(), (u16, &'static str)> {
    let has_client = headers
        .get(CLIENT_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    if !has_client {
        return Err((403, "missing X-FluxDown-Client header"));
    }
    if !header_token_ok(headers, config_token) {
        return Err((401, "invalid or missing token"));
    }
    Ok(())
}

/// 管理 API 门禁：token 为空 → 403（强制要求配置 token）；
/// 非空 → 接受 `Authorization: Bearer <token>` 或 `X-FluxDown-Token`。
pub fn check_management_auth(
    headers: &HeaderMap,
    config_token: &str,
) -> Result<(), (u16, &'static str)> {
    if config_token.is_empty() {
        return Err((
            403,
            "management API requires a token; set one in Settings > API Service",
        ));
    }
    let bearer = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if constant_time_eq(bearer, config_token) {
        return Ok(());
    }
    let x_token = headers
        .get(TOKEN_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if constant_time_eq(x_token, config_token) {
        return Ok(());
    }
    Err((401, "invalid or missing token"))
}

#[cfg(test)]
mod tests {
    use super::{constant_time_eq, header_token_ok};
    use axum::http::HeaderMap;

    #[test]
    fn constant_time_eq_basic() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "abcd"));
        assert!(constant_time_eq("", ""));
    }

    #[test]
    fn empty_token_always_ok() {
        assert!(header_token_ok(&HeaderMap::new(), ""));
    }
}
