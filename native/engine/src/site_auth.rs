//! 站点 HTTP Basic 认证凭据。
//!
//! 用户在新建任务时可填写 HTTP 认证用户名/密码，引擎将其转换为
//! `Authorization: Basic <base64>` 头注入任务的 extra_headers——复用既有的
//! 请求上下文持久化链路（`tasks.extra_headers` 列），resume / meta probe /
//! HLS / DASH 全路径自动携带，无需新增表或字段。
//!
//! 勾选「为此网站保存」时，凭据按站点键（`host` 或 `host:port`）存入
//! config 表单键 [`SITE_AUTH_CONFIG_KEY`]（JSON map）。后续对同一站点建任务
//! 且未显式提供凭据、extra_headers 中也没有 Authorization 时，自动套用已保存
//! 凭据。该键**不进云同步目录**（凭据属设备本地敏感数据）。
//!
//! 安全边界：
//! - 凭据以明文存于本地数据库（与代理 URL 内嵌密码、Cookie 同级，现有先例）；
//! - 注入的 Authorization 头由 reqwest 在跨 host 重定向时自动剥除
//!   （`remove_sensitive_headers`），不会泄漏到第三方；
//! - 日志导出侧已有 Authorization 头脱敏规则（Dart `log_service`）。

use std::collections::BTreeMap;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// config 表中保存站点凭据 map 的键。值为 JSON：
/// `{"example.com": {"user":"u","pass":"p"}, "host:8443": {...}}`。
pub const SITE_AUTH_CONFIG_KEY: &str = "site_auth_credentials";

/// 单个站点的 HTTP Basic 凭据。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiteCredential {
    pub user: String,
    pub pass: String,
}

/// 从 URL 提取站点键：仅 http/https 返回 `Some`。
/// 形如 `host`（默认端口）或 `host:port`（非默认端口）；host 已由 Url
/// 规范化为小写。
pub fn site_key(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let host = parsed.host_str()?;
    match parsed.port() {
        Some(port) => Some(format!("{host}:{port}")),
        None => Some(host.to_string()),
    }
}

/// 构造 `Authorization` 头值：`Basic base64(user:pass)`（RFC 7617）。
pub fn basic_auth_value(user: &str, pass: &str) -> String {
    use base64::Engine as _;
    let raw = format!("{user}:{pass}");
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(raw.as_bytes())
    )
}

/// 反序列化站点凭据 map。空串 / 非法 JSON → 空 map（凭据缓存损坏不阻断建任务）。
pub fn parse_store(json: &str) -> BTreeMap<String, SiteCredential> {
    if json.trim().is_empty() {
        return BTreeMap::new();
    }
    serde_json::from_str(json).unwrap_or_default()
}

/// 序列化站点凭据 map（BTreeMap 保证输出稳定，便于 diff / 测试）。
pub fn serialize_store(store: &BTreeMap<String, SiteCredential>) -> String {
    serde_json::to_string(store).unwrap_or_else(|_| "{}".to_string())
}

/// extra_headers 中是否已有 Authorization 头（大小写不敏感）。
/// 已有则不套用已保存的站点凭据——浏览器捕获 / 用户手填的头优先。
pub fn has_authorization(headers: &HashMap<String, String>) -> bool {
    headers
        .keys()
        .any(|k| k.eq_ignore_ascii_case("authorization"))
}

/// 把显式凭据写入 extra_headers（移除既有同名头后以规范名插入，
/// 确保用户在表单里填的用户名/密码覆盖捕获到的旧 Authorization）。
pub fn inject_basic_auth(headers: &mut HashMap<String, String>, user: &str, pass: &str) {
    headers.retain(|k, _| !k.eq_ignore_ascii_case("authorization"));
    headers.insert("Authorization".to_string(), basic_auth_value(user, pass));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn site_key_extracts_host_and_nondefault_port() {
        assert_eq!(
            site_key("https://Example.COM/file.zip"),
            Some("example.com".to_string())
        );
        assert_eq!(
            site_key("http://nas.local:8443/d/f.bin"),
            Some("nas.local:8443".to_string())
        );
        // 默认端口不入键：https://h:443 与 https://h 视为同一站点。
        assert_eq!(
            site_key("https://example.com:443/a"),
            Some("example.com".to_string())
        );
        assert_eq!(site_key("ftp://example.com/f"), None);
        assert_eq!(site_key("magnet:?xt=urn:btih:abc"), None);
        assert_eq!(site_key("not a url"), None);
    }

    #[test]
    fn basic_auth_value_matches_rfc7617_example() {
        // RFC 7617 §2 示例：Aladdin / open sesame
        assert_eq!(
            basic_auth_value("Aladdin", "open sesame"),
            "Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ=="
        );
    }

    #[test]
    fn store_roundtrip_and_corrupt_input() {
        let mut store = BTreeMap::new();
        store.insert(
            "example.com".to_string(),
            SiteCredential {
                user: "u".to_string(),
                pass: "p".to_string(),
            },
        );
        let json = serialize_store(&store);
        assert_eq!(parse_store(&json), store);
        assert!(parse_store("").is_empty());
        assert!(parse_store("not json").is_empty());
    }

    #[test]
    fn inject_overrides_existing_authorization_case_insensitively() {
        let mut headers = HashMap::new();
        headers.insert("AUTHORIZATION".to_string(), "Bearer old".to_string());
        assert!(has_authorization(&headers));
        inject_basic_auth(&mut headers, "u", "p");
        assert_eq!(headers.len(), 1);
        assert_eq!(
            headers.get("Authorization").map(String::as_str),
            Some(basic_auth_value("u", "p").as_str())
        );
    }

    #[test]
    fn has_authorization_is_false_for_unrelated_headers() {
        let mut headers = HashMap::new();
        headers.insert("X-Token".to_string(), "v".to_string());
        assert!(!has_authorization(&headers));
    }
}
