//! 嵌入式 Web SPA 托管 —— 构建期由 `build.rs` 把 `web/dist` 全量 `include_bytes!`
//! 进二进制，运行期无需任何外部目录（单文件分发：下载 → 运行 → 打开浏览器）。
//!
//! 与旧的 `ServeDir` 磁盘托管的差异：
//! - **ETag 取代 Last-Modified**：嵌入资源没有 mtime，改用构建期算好的内容哈希，
//!   `If-None-Match` 命中回 304（语义更强，且不受解包时间戳影响）。
//! - **不支持 Range**：SPA 资源全是小文件整取，不宣告 `Accept-Ranges`。
//! - 未命中的路径一律回 `index.html`（HTTP 200），保持 SPA 前端路由可直接刷新，
//!   与旧的 `ServeDir::fallback(ServeFile)` 行为一致。
//!
//! `FLUXDOWN_WEBROOT` 仍可把托管切回磁盘目录（见 `config`），本模块只负责内嵌路径。

use std::borrow::Cow;

use axum::body::Body;
use axum::http::header::{ALLOW, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE, ETAG, IF_NONE_MATCH};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};

/// 一个嵌入资源。字段全部由 `build.rs` 生成，运行期只读。
pub(crate) struct EmbeddedAsset {
    /// URL 路径（`/` 分隔，无前导斜杠），例如 `assets/index-Cn5SIYx8.js`。
    pub path: &'static str,
    pub bytes: &'static [u8],
    pub content_type: &'static str,
    /// 带引号的强 ETag，直接作为响应头值。
    pub etag: &'static str,
}

// 生成表按 path 升序（`BTreeMap` 遍历序），运行期二分查找。
include!(concat!(env!("OUT_DIR"), "/web_assets.rs"));

/// SPA 入口文件。任何未命中的路径都回退到它。
const INDEX: &str = "index.html";

/// 是否有嵌入的 Web UI（构建时 `web/dist` 缺失则为 false）。
pub(crate) fn is_embedded() -> bool {
    !EMBEDDED_ASSETS.is_empty()
}

/// 嵌入资源数量与总字节数（启动日志用）。
pub(crate) fn stats() -> (usize, usize) {
    (
        EMBEDDED_ASSETS.len(),
        EMBEDDED_ASSETS.iter().map(|a| a.bytes.len()).sum(),
    )
}

/// SPA 兜底 handler：挂在 `Router::fallback`，接管所有未被 API 路由命中的请求。
pub(crate) async fn handler(method: Method, uri: Uri, headers: HeaderMap) -> Response {
    if !matches!(method, Method::GET | Method::HEAD) {
        return (
            StatusCode::METHOD_NOT_ALLOWED,
            [(ALLOW, HeaderValue::from_static("GET, HEAD"))],
        )
            .into_response();
    }
    let Some(asset) = resolve(uri.path()) else {
        return not_embedded();
    };
    respond(asset, &method, &headers)
}

/// URL 路径 → 资源。未命中回退 `index.html`（SPA 前端路由）。
fn resolve(raw_path: &str) -> Option<&'static EmbeddedAsset> {
    let decoded = percent_decode(raw_path.trim_start_matches('/'));
    let hit = decoded.as_deref().and_then(|p| {
        if p.is_empty() {
            return None;
        }
        // 目录形式（`/foo/`）先试其 index.html，再落 SPA 兜底。
        let direct = lookup(p);
        if direct.is_some() {
            return direct;
        }
        p.ends_with('/')
            .then(|| lookup(&format!("{p}{INDEX}")))
            .flatten()
    });
    hit.or_else(|| lookup(INDEX))
}

fn lookup(path: &str) -> Option<&'static EmbeddedAsset> {
    EMBEDDED_ASSETS
        .binary_search_by(|a| a.path.cmp(path))
        .ok()
        .map(|i| &EMBEDDED_ASSETS[i])
}

fn respond(asset: &'static EmbeddedAsset, method: &Method, headers: &HeaderMap) -> Response {
    let etag = HeaderValue::from_static(asset.etag);
    let cache = HeaderValue::from_static(cache_control(asset));
    if headers
        .get(IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| etag_matches(v, asset.etag))
    {
        return (
            StatusCode::NOT_MODIFIED,
            [(ETAG, etag), (CACHE_CONTROL, cache)],
        )
            .into_response();
    }
    let content_type = HeaderValue::from_static(asset.content_type);
    if method == Method::HEAD {
        // 显式给出长度并省掉正文，避免为一次 HEAD 写出整份字节。
        return (
            [
                (CONTENT_TYPE, content_type),
                (ETAG, etag),
                (CACHE_CONTROL, cache),
                (CONTENT_LENGTH, HeaderValue::from(asset.bytes.len())),
            ],
            Body::empty(),
        )
            .into_response();
    }
    (
        [
            (CONTENT_TYPE, content_type),
            (ETAG, etag),
            (CACHE_CONTROL, cache),
        ],
        Body::from(asset.bytes),
    )
        .into_response()
}

/// 构建时没有嵌入前端（开发构建未先跑 `bun run build`）时的自解释页面：
/// 503 而非 404，明确区分「服务器活着但没有 UI」与「路径不存在」。
fn not_embedded() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        [(
            CONTENT_TYPE,
            HeaderValue::from_static("text/plain; charset=utf-8"),
        )],
        "FluxDown Server: Web UI is not embedded in this build.\n\
         Build it with `cd web && bun run build` then rebuild the server,\n\
         or point FLUXDOWN_WEBROOT at an existing web build directory.\n\
         \n\
         本构建未嵌入 Web 界面：先在 web/ 执行 `bun run build` 再重新编译服务器，\n\
         或用 FLUXDOWN_WEBROOT 指向已构建好的前端目录。\n\
         \n\
         The REST API / WebSocket endpoints are unaffected.\n",
    )
        .into_response()
}

/// `If-None-Match` 匹配：`*` 或列表中任一 tag 相等（忽略 `W/` 弱前缀）。
fn etag_matches(header: &str, etag: &str) -> bool {
    let header = header.trim();
    if header == "*" {
        return true;
    }
    header
        .split(',')
        .map(|t| t.trim().trim_start_matches("W/"))
        .any(|t| t == etag)
}

/// 缓存分档只依据两条与「文件名清单 / 目录前缀」无关的事实，新增任何资产都
/// 自动落到正确的档位：
/// 1. **HTML 文档一律 revalidate**——它引用的是带哈希的 chunk 名，浏览器若拿
///    到旧 HTML 就会去请求已被新版本删掉的 chunk（白屏）。
/// 2. **文件名自带内容哈希的资产**（Vite/Rollup `[name]-[hash].[ext]`）URL 随
///    内容变，可以 immutable 永久缓存。
///
/// 认不出哈希就退回短缓存 + ETag revalidate：误判成「非哈希」只多一次 304，
/// 误判成「哈希」会把用户钉死在旧版本上一年，所以 [`is_content_hashed`] 从严。
fn cache_control(asset: &EmbeddedAsset) -> &'static str {
    if asset.content_type.starts_with("text/html") {
        "no-cache"
    } else if is_content_hashed(asset.path) {
        "public, max-age=31536000, immutable"
    } else {
        "public, max-age=86400"
    }
}

/// 文件名末段是否是 Rollup/Vite 的内容哈希：去掉全部扩展名后，最后一个 `-`
/// 分段为 8–24 位 `[A-Za-z0-9_]` 且**数字、大写、小写三者齐备**。
///
/// 三者齐备是刻意的收紧条件：真哈希（`Cn5SIYx8`）几乎必然三样都有，而人手写
/// 的后缀（`MiSans-Semibold`、`chunk-vendor`）不会。代价是极少数纯字母哈希被
/// 判为非哈希——那只是少一层缓存，方向安全。
fn is_content_hashed(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let tail = name
        .split('.')
        .next()
        .unwrap_or(name)
        .rsplit('-')
        .next()
        .unwrap_or_default();
    (8..=24).contains(&tail.len())
        && tail.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && tail.bytes().any(|b| b.is_ascii_digit())
        && tail.bytes().any(|b| b.is_ascii_uppercase())
        && tail.bytes().any(|b| b.is_ascii_lowercase())
}

/// 百分号解码。非法转义或解出非 UTF-8 → `None`（当作未命中走 SPA 兜底）。
fn percent_decode(raw: &str) -> Option<Cow<'_, str>> {
    if !raw.contains('%') {
        return Some(Cow::Borrowed(raw));
    }
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hi = bytes.get(i + 1).and_then(|b| (*b as char).to_digit(16))?;
            let lo = bytes.get(i + 2).and_then(|b| (*b as char).to_digit(16))?;
            out.push((hi * 16 + lo) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok().map(Cow::Owned)
}

#[cfg(test)]
mod tests {
    use super::{EmbeddedAsset, cache_control, etag_matches, is_content_hashed, percent_decode};

    fn asset(path: &'static str, content_type: &'static str) -> EmbeddedAsset {
        EmbeddedAsset {
            path,
            bytes: b"",
            content_type,
            etag: "\"0-0\"",
        }
    }

    #[test]
    fn percent_decode_handles_escapes_and_rejects_malformed() {
        assert_eq!(
            percent_decode("assets/a.js").as_deref(),
            Some("assets/a.js")
        );
        assert_eq!(
            percent_decode("fonts/Mi%20Sans.ttf").as_deref(),
            Some("fonts/Mi Sans.ttf")
        );
        // 截断的转义序列不能被当成字面量放过——否则 `%2e%2e` 类构造会绕过键匹配。
        assert_eq!(percent_decode("a%2").as_deref(), None);
        assert_eq!(percent_decode("a%zz").as_deref(), None);
        // 解出非 UTF-8 字节序列同样拒绝。
        assert_eq!(percent_decode("%ff%fe").as_deref(), None);
    }

    #[test]
    fn etag_matches_star_list_and_weak_prefix() {
        let tag = "\"deadbeef-10\"";
        assert!(etag_matches("*", tag));
        assert!(etag_matches(tag, tag));
        assert!(etag_matches("\"other\", \"deadbeef-10\"", tag));
        assert!(etag_matches("W/\"deadbeef-10\"", tag));
        assert!(!etag_matches("\"deadbeef-11\"", tag));
        assert!(!etag_matches("", tag));
    }

    #[test]
    fn cache_control_never_freezes_html_and_only_freezes_hashed_names() {
        const HTML: &str = "text/html; charset=utf-8";
        const JS: &str = "text/javascript; charset=utf-8";
        // HTML 判定看 Content-Type 而非文件名：多入口构建的 about.html 同样
        // 必须 revalidate，否则它引用的旧 chunk 在新版本里已经不存在。
        assert_eq!(cache_control(&asset("index.html", HTML)), "no-cache");
        assert_eq!(cache_control(&asset("pages/about.html", HTML)), "no-cache");
        // 带内容哈希的 chunk 可以永久缓存，且与所在目录无关（assetsDir 可配）。
        assert!(cache_control(&asset("assets/index-Cn5SIYx8.js", JS)).contains("immutable"));
        assert!(cache_control(&asset("static/app-CFGyPd1g.js", JS)).contains("immutable"));
        // 固定名资产（字体）只能短缓存 + ETag，否则改版后换不掉。
        assert!(
            !cache_control(&asset("fonts/MiSans-Regular.ttf", "font/ttf")).contains("immutable")
        );
    }

    #[test]
    fn content_hash_detection_errs_toward_short_cache() {
        assert!(is_content_hashed("assets/index-Cn5SIYx8.js"));
        assert!(is_content_hashed("index-CFGyPd1g.css"));
        // 人写的后缀缺数字/大小写混排，绝不能被当成哈希冻一年。
        assert!(!is_content_hashed("fonts/MiSans-Semibold.ttf"));
        assert!(!is_content_hashed("chunk-vendor.js"));
        assert!(!is_content_hashed("favicon.svg"));
        assert!(!is_content_hashed("index.html"));
        // 多重扩展名（sourcemap）按第一个点之前判定，仍能认出哈希。
        assert!(is_content_hashed("assets/index-Cn5SIYx8.js.map"));
    }
}
