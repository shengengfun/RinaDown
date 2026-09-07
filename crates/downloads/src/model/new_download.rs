//! 「新建下载」表单的纯模型：链接解析、代理 / UA / 线程预设与请求构建。
//!
//! 规则与 `lib/src/widgets/new_download_dialog.dart`、
//! `lib/src/models/task_proxy_choice.dart`、`lib/src/models/ua_presets.dart`
//! 与 `lib/src/widgets/thread_selector.dart` 逐条对齐，不依赖 GPUI。

use std::collections::{BTreeMap, HashMap, HashSet};

use fluxdown_protocol::CreateTaskRequest;

/// 一条解析出的下载条目（aria2 风格：URL + 可选 `out=` / `checksum=` 选项行）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct UrlEntry {
    pub(crate) url: String,
    pub(crate) file_name: String,
    pub(crate) checksum: String,
}

impl UrlEntry {
    fn with_url(url: &str) -> Self {
        Self {
            url: url.to_owned(),
            ..Self::default()
        }
    }
}

/// 解析多行文本为下载条目。
///
/// - 原始行以空格 / Tab 开头 = 选项行，附着到上一条（`out=` / `checksum=`）；
/// - `#` 开头为注释，空行跳过；
/// - 含 `magnet:?` / `ed2k://` 时从该位置截取到行尾；
/// - 其余行：`loose` 为 true 时取行内首个 `http(s)/ftp` 链接并去掉尾部标点
///   （TXT 导入），否则要求链接位于行首（手动输入）。
pub(crate) fn parse_entries(text: &str, loose: bool) -> Vec<UrlEntry> {
    let mut entries = Vec::new();
    let mut current: Option<UrlEntry> = None;
    for line in text.split('\n') {
        if line.starts_with(' ') || line.starts_with('\t') {
            let Some(entry) = current.as_mut() else {
                continue;
            };
            let trimmed = line.trim();
            if let Some(name) = trimmed.strip_prefix("out=") {
                entry.file_name = name.to_owned();
            } else if let Some(checksum) = trimmed.strip_prefix("checksum=") {
                entry.checksum = checksum.to_owned();
            }
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(entry) = current.take() {
            entries.push(entry);
        }
        let lower = trimmed.to_ascii_lowercase();
        if let Some(index) = lower.find("magnet:?") {
            current = Some(UrlEntry::with_url(&trimmed[index..]));
        } else if let Some(index) = lower.find("ed2k://") {
            current = Some(UrlEntry::with_url(&trimmed[index..]));
        } else if loose {
            if let Some(url) = find_http_url(trimmed) {
                let url = trim_url_tail(url);
                if !url.is_empty() {
                    current = Some(UrlEntry::with_url(url));
                }
            }
        } else if let Some(url) = leading_http_url(trimmed) {
            current = Some(UrlEntry::with_url(url));
        }
    }
    if let Some(entry) = current {
        entries.push(entry);
    }
    entries
}

/// `^(https?|ftp)://\S+`（忽略大小写）。
fn leading_http_url(text: &str) -> Option<&str> {
    let scheme = ["https://", "http://", "ftp://"]
        .into_iter()
        .find(|scheme| {
            text.len() >= scheme.len()
                && text.as_bytes()[..scheme.len()].eq_ignore_ascii_case(scheme.as_bytes())
        })?;
    let rest = &text[scheme.len()..];
    let body = rest.find(char::is_whitespace).unwrap_or(rest.len());
    (body > 0).then(|| &text[..scheme.len() + body])
}

/// 行内首个 `(https?|ftp)://\S+` 匹配（忽略大小写）。
fn find_http_url(line: &str) -> Option<&str> {
    line.char_indices()
        .find_map(|(index, _)| leading_http_url(&line[index..]))
}

/// 去掉 URL 末尾常见标点（TXT 文本中 URL 后可能跟随句号 / 逗号等）。
fn trim_url_tail(url: &str) -> &str {
    url.trim_end_matches(|c: char| ".,;:!?()[]{}".contains(c))
}

/// 条目 → aria2 风格文本（含 `out=` / `checksum=` 选项行）。
pub(crate) fn entry_to_text(entry: &UrlEntry) -> String {
    let mut text = entry.url.clone();
    if !entry.file_name.is_empty() {
        text.push_str("\n  out=");
        text.push_str(&entry.file_name);
    }
    if !entry.checksum.is_empty() {
        text.push_str("\n  checksum=");
        text.push_str(&entry.checksum);
    }
    text
}

/// 把导入条目追加到现有文本：按 URL 去重（保留已有条目），返回新的文本框内容。
pub(crate) fn merge_imported(existing_text: &str, imported: Vec<UrlEntry>) -> String {
    let mut merged = parse_entries(existing_text, false);
    let mut seen = merged
        .iter()
        .map(|entry| entry.url.clone())
        .collect::<HashSet<_>>();
    merged.extend(
        imported
            .into_iter()
            .filter(|entry| seen.insert(entry.url.clone())),
    );
    merged
        .iter()
        .map(entry_to_text)
        .collect::<Vec<_>>()
        .join("\n")
}

/// 强制直连哨兵值。
pub(crate) const PROXY_DIRECT_SENTINEL: &str = "direct://";
/// 跟随系统代理哨兵值。
pub(crate) const PROXY_SYSTEM_SENTINEL: &str = "system://";

/// 任务代理选择项；wire 语义见 `lib/src/models/task_proxy_choice.dart`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProxyChoice {
    FollowGlobal,
    Direct,
    System,
    GlobalManual,
    Custom,
}

impl ProxyChoice {
    /// 下拉顺序。
    pub(crate) const ALL: [Self; 5] = [
        Self::FollowGlobal,
        Self::Direct,
        Self::System,
        Self::GlobalManual,
        Self::Custom,
    ];

    /// 生成 `proxy_url` wire 值；`manual_url` 为全局手动代理 URL（空 = 未配置）。
    pub(crate) fn wire(self, manual_url: &str, custom_url: &str) -> String {
        match self {
            Self::FollowGlobal => String::new(),
            Self::Direct => PROXY_DIRECT_SENTINEL.to_owned(),
            Self::System => PROXY_SYSTEM_SENTINEL.to_owned(),
            Self::GlobalManual => manual_url.to_owned(),
            Self::Custom => custom_url.trim().to_owned(),
        }
    }
}

/// 从 daemon 配置拼出全局手动代理 URL：`type://[user[:pass]@]host:port`。
///
/// host 为空或端口解析为 0 / 非法时返回空串（= 未配置），userinfo 百分号编码。
pub(crate) fn manual_proxy_url(config: &BTreeMap<String, String>) -> String {
    let trimmed = |key: &str| config.get(key).map_or("", |value| value.trim());
    let host = trimmed("proxy_host");
    let port = trimmed("proxy_port").parse::<u32>().unwrap_or(0);
    if host.is_empty() || port == 0 {
        return String::new();
    }
    let proxy_type = match trimmed("proxy_type") {
        "" => "http",
        value => value,
    };
    let user = config.get("proxy_username").map_or("", String::as_str);
    let password = config.get("proxy_password").map_or("", String::as_str);
    let mut url = format!("{proxy_type}://");
    if !user.is_empty() {
        encode_component(user, &mut url);
        if !password.is_empty() {
            url.push(':');
            encode_component(password, &mut url);
        }
        url.push('@');
    }
    url.push_str(host);
    url.push(':');
    url.push_str(&port.to_string());
    url
}

/// Dart `Uri.encodeComponent`：除 `A-Za-z0-9-_.!~*'()` 外全部按 UTF-8 百分号编码。
fn encode_component(value: &str, out: &mut String) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
}

/// 预设 UA（key → UA 字符串）。版本基准与 `lib/src/models/ua_presets.dart` 一致。
pub(crate) const UA_PRESETS: &[(&str, &str)] = &[
    (
        "chrome",
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/145.0.0.0 Safari/537.36",
    ),
    (
        "firefox",
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:147.0) Gecko/20100101 Firefox/147.0",
    ),
    (
        "edge",
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/145.0.0.0 Safari/537.36 Edg/145.0.3800.70",
    ),
    (
        "safari",
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.3.1 Safari/605.1.15",
    ),
];

/// 继承全局 UA 的预设键（输入框留空）。
pub(crate) const UA_PRESET_DEFAULT: &str = "default";
/// 自定义 UA 的预设键（输入框自由填写）。
pub(crate) const UA_PRESET_CUSTOM: &str = "custom";

/// UA 预设下拉顺序：继承全局 → 各浏览器 → 自定义。
pub(crate) fn ua_preset_keys() -> impl Iterator<Item = &'static str> {
    std::iter::once(UA_PRESET_DEFAULT)
        .chain(UA_PRESETS.iter().map(|(key, _)| *key))
        .chain(std::iter::once(UA_PRESET_CUSTOM))
}

/// UA 字符串 → 预设键；空 = `default`，未命中 = `custom`。
pub(crate) fn detect_ua_preset(ua: &str) -> &'static str {
    if ua.is_empty() {
        return UA_PRESET_DEFAULT;
    }
    UA_PRESETS
        .iter()
        .find(|(_, value)| *value == ua)
        .map_or(UA_PRESET_CUSTOM, |(key, _)| key)
}

/// 预设键 → UA 字符串（`default` → 空）。
pub(crate) fn ua_preset_value(key: &str) -> &'static str {
    UA_PRESETS
        .iter()
        .find(|(preset, _)| *preset == key)
        .map_or("", |(_, value)| value)
}

/// 线程数下拉预设（`自动` 与 `自定义` 之间的固定档位）。
pub(crate) const THREAD_PRESETS: &[i32] = &[4, 8, 16, 32, 64];
/// 自定义线程数上限。
pub(crate) const MAX_THREADS: i32 = 256;

/// 线程数选择：自动 / 预设档位 / 自定义输入。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ThreadChoice {
    Auto,
    Preset(i32),
    Custom,
}

impl ThreadChoice {
    /// 由默认分段数推导初始选择：0 = 自动，预设内 = 预设，其余 = 自定义。
    pub(crate) fn from_segments(segments: i32) -> Self {
        if segments <= 0 {
            Self::Auto
        } else if THREAD_PRESETS.contains(&segments) {
            Self::Preset(segments)
        } else {
            Self::Custom
        }
    }
}

/// 自定义输入 → `segments`：非法 / 小于 1 = 0（自动），上限 [`MAX_THREADS`]。
pub(crate) fn custom_segments(text: &str) -> i32 {
    match text.trim().parse::<i32>() {
        Ok(value) if value >= 1 => value.min(MAX_THREADS),
        _ => 0,
    }
}

/// 哈希校验算法（wire 字面量，无本地化），默认 `sha-256`。
pub(crate) const HASH_ALGORITHMS: &[&str] = &["md5", "sha-1", "sha-256", "sha-512"];
pub(crate) const DEFAULT_HASH_ALGORITHM: &str = "sha-256";

/// `algo=hexhash`；哈希值为空时返回空串（跳过校验）。
pub(crate) fn checksum_spec(algorithm: &str, hash: &str) -> String {
    let hash = hash.trim();
    if hash.is_empty() {
        String::new()
    } else {
        format!("{algorithm}={hash}")
    }
}

/// 整批共享的表单选项。`rename` / HTTP 认证 / 高级面板校验值只作用于单条提交。
#[derive(Clone, Debug, Default)]
pub(crate) struct DraftOptions {
    pub(crate) save_dir: String,
    pub(crate) segments: i32,
    pub(crate) cookies: String,
    pub(crate) proxy_url: String,
    pub(crate) user_agent: String,
    pub(crate) queue_id: String,
    pub(crate) checksum: String,
    pub(crate) ignore_tls_errors: bool,
    pub(crate) headers: HashMap<String, String>,
    pub(crate) start_paused: bool,
    pub(crate) rename: String,
    pub(crate) http_user: String,
    pub(crate) http_password: String,
    pub(crate) save_site_auth: bool,
}

/// 每条条目一个 [`CreateTaskRequest`]，共享字段来自 [`DraftOptions`]。
///
/// 与 Dart 一致：单条时重命名优先于 `out=`、高级面板校验值优先于
/// `checksum=`、附带 HTTP 认证；多条时仅使用条目自带的文件名 / 校验值。
pub(crate) fn build_requests(
    entries: &[UrlEntry],
    options: &DraftOptions,
) -> Vec<CreateTaskRequest> {
    let single = entries.len() == 1;
    entries
        .iter()
        .map(|entry| {
            let file_name = if single && !options.rename.is_empty() {
                options.rename.clone()
            } else {
                entry.file_name.clone()
            };
            let checksum = if single && !options.checksum.is_empty() {
                options.checksum.clone()
            } else {
                entry.checksum.clone()
            };
            CreateTaskRequest {
                url: entry.url.clone(),
                file_name,
                save_dir: options.save_dir.clone(),
                segments: options.segments,
                cookies: options.cookies.clone(),
                referrer: String::new(),
                proxy_url: options.proxy_url.clone(),
                user_agent: options.user_agent.clone(),
                queue_id: options.queue_id.clone(),
                checksum,
                ignore_tls_errors: options.ignore_tls_errors,
                headers: (!options.headers.is_empty()).then(|| options.headers.clone()),
                torrent_b64: None,
                method: None,
                body: None,
                audio_url: None,
                start_paused: options.start_paused,
                http_user: if single {
                    options.http_user.clone()
                } else {
                    String::new()
                },
                http_password: if single {
                    options.http_password.clone()
                } else {
                    String::new()
                },
                save_site_auth: single && options.save_site_auth,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};

    use super::{
        DraftOptions, ProxyChoice, ThreadChoice, UrlEntry, build_requests, checksum_spec,
        custom_segments, detect_ua_preset, manual_proxy_url, merge_imported, parse_entries,
    };

    #[test]
    fn strict_parse_requires_url_at_line_start_and_attaches_options() {
        let text = "https://a.example/one.zip\n  out=renamed.zip\n\tchecksum=sha-256=abc\n# comment\nsee https://b.example/skip\nMAGNET link: magnet:?xt=urn:btih:xyz\nfoo ed2k://|file|x|\nFTP://c.example/f.bin trailing";
        let entries = parse_entries(text, false);
        assert_eq!(
            entries,
            vec![
                UrlEntry {
                    url: "https://a.example/one.zip".to_owned(),
                    file_name: "renamed.zip".to_owned(),
                    checksum: "sha-256=abc".to_owned(),
                },
                UrlEntry {
                    url: "magnet:?xt=urn:btih:xyz".to_owned(),
                    ..UrlEntry::default()
                },
                UrlEntry {
                    url: "ed2k://|file|x|".to_owned(),
                    ..UrlEntry::default()
                },
                UrlEntry {
                    url: "FTP://c.example/f.bin".to_owned(),
                    ..UrlEntry::default()
                },
            ]
        );
    }

    #[test]
    fn loose_parse_extracts_inline_urls_and_trims_tail_punctuation() {
        let entries = parse_entries("see https://a.example/x.zip). next\nnothing here\n", true);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].url, "https://a.example/x.zip");
    }

    #[test]
    fn merge_imported_dedupes_by_url_and_keeps_option_lines() {
        let merged = merge_imported(
            "https://a.example/x\n  out=x.bin",
            vec![
                UrlEntry {
                    url: "https://a.example/x".to_owned(),
                    file_name: "dup.bin".to_owned(),
                    checksum: String::new(),
                },
                UrlEntry {
                    url: "https://b.example/y".to_owned(),
                    file_name: String::new(),
                    checksum: "md5=ff".to_owned(),
                },
            ],
        );
        assert_eq!(
            merged,
            "https://a.example/x\n  out=x.bin\nhttps://b.example/y\n  checksum=md5=ff"
        );
    }

    #[test]
    fn proxy_choice_wire_values_match_dart() {
        assert_eq!(ProxyChoice::FollowGlobal.wire("http://m:1", "x"), "");
        assert_eq!(ProxyChoice::Direct.wire("", ""), "direct://");
        assert_eq!(ProxyChoice::System.wire("", ""), "system://");
        assert_eq!(
            ProxyChoice::GlobalManual.wire("socks5://m:1080", ""),
            "socks5://m:1080"
        );
        assert_eq!(
            ProxyChoice::Custom.wire("", "  http://c:8080 "),
            "http://c:8080"
        );
    }

    #[test]
    fn manual_proxy_url_encodes_userinfo_and_requires_host_port() {
        let mut config = BTreeMap::new();
        assert_eq!(manual_proxy_url(&config), "");
        config.insert("proxy_host".to_owned(), " 127.0.0.1 ".to_owned());
        config.insert("proxy_port".to_owned(), "1080".to_owned());
        assert_eq!(manual_proxy_url(&config), "http://127.0.0.1:1080");
        config.insert("proxy_type".to_owned(), "socks5".to_owned());
        config.insert("proxy_username".to_owned(), "us er".to_owned());
        config.insert("proxy_password".to_owned(), "p@ss".to_owned());
        assert_eq!(
            manual_proxy_url(&config),
            "socks5://us%20er:p%40ss@127.0.0.1:1080"
        );
        config.insert("proxy_port".to_owned(), "0".to_owned());
        assert_eq!(manual_proxy_url(&config), "");
    }

    #[test]
    fn ua_thread_and_checksum_helpers() {
        assert_eq!(detect_ua_preset(""), "default");
        assert_eq!(detect_ua_preset(super::UA_PRESETS[1].1), "firefox");
        assert_eq!(detect_ua_preset("FluxDown/1.0"), "custom");
        assert_eq!(ThreadChoice::from_segments(0), ThreadChoice::Auto);
        assert_eq!(ThreadChoice::from_segments(16), ThreadChoice::Preset(16));
        assert_eq!(ThreadChoice::from_segments(10), ThreadChoice::Custom);
        assert_eq!(custom_segments("999"), 256);
        assert_eq!(custom_segments("0"), 0);
        assert_eq!(custom_segments("abc"), 0);
        assert_eq!(checksum_spec("md5", "  "), "");
        assert_eq!(checksum_spec("sha-1", " abc "), "sha-1=abc");
    }

    #[test]
    fn single_request_applies_rename_checksum_and_auth_but_batch_does_not() {
        let options = DraftOptions {
            save_dir: "/downloads".to_owned(),
            segments: 8,
            queue_id: "later".to_owned(),
            checksum: "sha-256=deadbeef".to_owned(),
            headers: HashMap::from([("X-Test".to_owned(), "1".to_owned())]),
            start_paused: true,
            rename: "renamed.bin".to_owned(),
            http_user: "u".to_owned(),
            http_password: "p".to_owned(),
            save_site_auth: true,
            ..DraftOptions::default()
        };
        let entry = UrlEntry {
            url: "https://a.example/x".to_owned(),
            file_name: "out.bin".to_owned(),
            checksum: "md5=ff".to_owned(),
        };
        let single = build_requests(std::slice::from_ref(&entry), &options);
        assert_eq!(single.len(), 1);
        assert_eq!(single[0].file_name, "renamed.bin");
        assert_eq!(single[0].checksum, "sha-256=deadbeef");
        assert_eq!(single[0].http_user, "u");
        assert!(single[0].save_site_auth);
        assert!(single[0].start_paused);
        assert_eq!(single[0].queue_id, "later");
        assert_eq!(
            single[0].headers.as_ref().and_then(|h| h.get("X-Test")),
            Some(&"1".to_owned())
        );

        let batch = build_requests(&[entry.clone(), entry], &options);
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].file_name, "out.bin");
        assert_eq!(batch[0].checksum, "md5=ff");
        assert_eq!(batch[0].http_user, "");
        assert!(!batch[0].save_site_auth);
        assert_eq!(batch[0].segments, 8);
    }

    #[test]
    fn empty_headers_serialize_as_absent() {
        let requests = build_requests(
            &[UrlEntry {
                url: "https://a.example/x".to_owned(),
                ..UrlEntry::default()
            }],
            &DraftOptions::default(),
        );
        assert!(requests[0].headers.is_none());
    }
}
