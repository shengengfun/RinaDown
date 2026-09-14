//! 系统「自动代理」（PAC / WPAD）求值 —— 仅 Windows 编译。
//!
//! # 为什么需要它
//!
//! Windows 的「使用设置脚本」/「自动检测设置」（注册表 `AutoConfigURL` /
//! `AutoDetect`）与「使用代理服务器」（`ProxyEnable` + `ProxyServer`）是
//! **两条独立通道**：PAC 模式下 `ProxyEnable` 恒为 0，只读静态项的检测
//! 会判定「系统没配代理」——于是 `ProxyMode::System` 静默直连、
//! `ProxyMode::Auto` 连候选代理都拿不到，用户开着 PAC 模式的 Clash 时
//! 表现为「代理完全不生效」。
//!
//! PAC 是**按目标 URL 求值的脚本**（`FindProxyForURL(url, host)`），不存在
//! 「全局代理地址」，所以本模块的出口是 [`resolve_proxy_for`] 而不是
//! 「检测一次地址」。求值优先交给 Windows 自己的 WinHTTP
//! （[`WinHttpGetProxyForUrl`]）：下载/缓存 PAC、执行脚本、按 `ProxyOverride`
//! 给出绕过列表全由它负责——与浏览器同款实现，也不必给引擎背一个 JS 引擎。
//!
//! # WinHTTP 的两个硬限制（实测）与兜底
//!
//! 1. **不认 `file://` / 裸本地路径**（`ERROR_WINHTTP_UNRECOGNIZED_SCHEME`）。
//!    Shadowsocks-Windows 之类客户端是把 `AutoConfigURL` 指向本地 pac 文件。
//! 2. **要求 PAC 响应带 `Content-Type: application/x-ns-proxy-autoconfig`**，
//!    否则在`text/plain` / `application/octet-stream`（朴素静态服务器常见）
//!    下直接报 `ERROR_WINHTTP_BAD_AUTO_PROXY_SCRIPT`，即使脚本本身完全合法。
//!
//! 两者都会让「VPN 软件开了 PAC 模式，但 APP 不识别」。因此 WinHTTP 失败后走
//! [`fallback_from_script`]：自己取回脚本，再按 [`extract_single_loopback_proxy`]
//! 抽端点——判据刻意收紧（**整份脚本只出现一个代理端点、且必须是回环地址**），
//! 只覆盖「把一切都交给本机 VPN 客户端」这类 PAC（Clash Verge / v2rayN /
//! Shadowsocks 的默认 PAC 都是这个形状）。按规则分流到多个代理的 PAC 不猜：
//! 宁可不走代理，也不把流量塞进错误的出口。
//!
//! # 边界与代价
//!
//! - 求值要下载并执行 PAC（本地 PAC 服务亚毫秒；远端 PAC 是一次 HTTP 往返），
//!   故按 **来源 + 目标 URL** 缓存结果 [`CACHE_TTL`]（URL 参与键是因为 PAC
//!   可以按路径判定），代理设置变更时由 [`clear_cache`] 显式作废。
//! - 会话超时收紧到个位数秒（见 `SESSION_TIMEOUTS`）：PAC 服务不可达时最坏
//!   阻塞在此量级，而不是 WinHTTP 默认的 30s+——调用方可能跑在宿主 actor 上。
//! - 求值失败不缓存成功结论，只按 [`FAILURE_TTL`] 短暂压制重试，避免一个坏
//!   PAC 地址让每次任务启动都付一次超时代价。

use std::collections::HashMap;
use std::sync::Mutex as StdMutex;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{GetLastError, GlobalFree, HGLOBAL};
use windows_sys::Win32::Networking::WinHttp::{
    WINHTTP_ACCESS_TYPE_NO_PROXY, WINHTTP_AUTO_DETECT_TYPE_DHCP, WINHTTP_AUTO_DETECT_TYPE_DNS_A,
    WINHTTP_AUTOPROXY_AUTO_DETECT, WINHTTP_AUTOPROXY_CONFIG_URL, WINHTTP_AUTOPROXY_OPTIONS,
    WINHTTP_PROXY_INFO, WinHttpCloseHandle, WinHttpGetProxyForUrl, WinHttpOpen, WinHttpSetTimeouts,
};

use crate::logger::{log_error, log_info};
use crate::proxy_config::{ProxyConfig, ProxyMode, ProxyType, parse_windows_proxy_server};

/// 求值结果的缓存时长。
const CACHE_TTL: Duration = Duration::from_secs(120);

/// 求值失败后的压制时长（期间不重复尝试同一个坏 PAC）。
const FAILURE_TTL: Duration = Duration::from_secs(60);

/// 缓存条目上限。条目是「来源 + URL」，正常用量远低于此；到顶即整体清空
/// （比 LRU 简单，且清空的代价只是重算）。
const CACHE_MAX_ENTRIES: usize = 256;

/// 会话级超时（毫秒）：解析 / 连接 / 发送 / 接收。PAC 下载走的就是这套。
const SESSION_TIMEOUTS: (i32, i32, i32, i32) = (3000, 3000, 5000, 5000);

/// 兜底取回 PAC 脚本时的连接/读写超时，以及正文上限。
const SCRIPT_TIMEOUT: Duration = Duration::from_secs(3);
const SCRIPT_MAX_BYTES: u64 = 512 * 1024;

/// 目标 URL 缺失或不是 WinHTTP 能解析的绝对 URL 时的探针地址。
///
/// 探针**只用于 PAC 脚本的本地求值**，不会真的向该地址发出请求；选一个
/// 几乎所有分流 PAC 都判定「需要代理」的目的地，检测结果才能反映这台机器
/// 的 PAC 代理落在哪个端点。
pub const PAC_PROBE_URL: &str = "https://www.google.com/";

// ---------------------------------------------------------------------------
// 自动代理来源
// ---------------------------------------------------------------------------

/// 当前用户的自动代理配置来源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AutoProxySource {
    /// `AutoConfigURL`：PAC 脚本的 http(s) 地址或本地文件路径。
    ConfigUrl(String),
    /// `AutoDetect` = 1：WPAD（DHCP / DNS 自动发现），没有固定地址。
    AutoDetect,
}

impl AutoProxySource {
    /// 缓存键与日志用的稳定标识。
    fn key(&self) -> String {
        match self {
            Self::ConfigUrl(url) => format!("pac={url}"),
            Self::AutoDetect => "wpad".to_string(),
        }
    }

    /// 判等用的来源描述（缓存会校验它，来源变了旧结论立即失效）。
    fn signature(&self) -> &str {
        match self {
            Self::ConfigUrl(url) => url.as_str(),
            Self::AutoDetect => "wpad",
        }
    }
}

/// 读取当前用户的自动代理配置来源（注册表 `AutoConfigURL` / `AutoDetect`）。
///
/// `AutoConfigURL` 优先：它与静态 `ProxyEnable`/`ProxyServer` 共存时以 PAC
/// 为准（与浏览器同语义，见 [`crate::proxy_config::detect_system_proxy_for`]）。
pub(crate) fn auto_proxy_source() -> Option<AutoProxySource> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let inet = hkcu
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings")
        .ok()?;
    let url: String = inet
        .get_value("AutoConfigURL")
        .unwrap_or_else(|_| String::new());
    let url = url.trim();
    if !url.is_empty() {
        return Some(AutoProxySource::ConfigUrl(url.to_string()));
    }
    let auto_detect: u32 = inet.get_value("AutoDetect").unwrap_or(0);
    (auto_detect != 0).then_some(AutoProxySource::AutoDetect)
}

// ---------------------------------------------------------------------------
// 结果缓存
// ---------------------------------------------------------------------------

/// 一次求值的结论。
#[derive(Clone)]
enum Outcome {
    /// 脚本判定直连（无代理条目）。
    Direct,
    /// 脚本给出代理端点。
    Proxied(Box<ProxyConfig>),
    /// 求值失败（PAC 不可达 / 脚本报错），附诊断文本。
    Failed(String),
}

impl Outcome {
    fn ttl(&self) -> Duration {
        match self {
            Self::Failed(_) => FAILURE_TTL,
            _ => CACHE_TTL,
        }
    }
}

struct CacheEntry {
    /// 来源标识：来源换掉（用户切了 PAC 地址）时旧结论立即失效。
    signature: String,
    outcome: Outcome,
    at: Instant,
}

/// 键 = `来源标识 + 目标 URL`，同时保存来源签名以识别变更。
static CACHE: OnceLock<StdMutex<HashMap<String, CacheEntry>>> = OnceLock::new();

/// 作废全部求值结论并关掉缓存的 WinHTTP 会话。
///
/// 代理设置变更时调用：既丢掉按 URL 的结论，也让 WinHTTP 会话级的 PAC 缓存
/// 一起失效（PAC 内容可能已随模式切换而变）。
pub(crate) fn clear_cache() {
    if let Some(cache) = CACHE.get() {
        match cache.lock() {
            Ok(mut map) => map.clear(),
            Err(poisoned) => poisoned.into_inner().clear(),
        }
    }
    drop_session();
}

/// 按目标 URL 求值 PAC / WPAD。
///
/// - `Ok(Some(config))`：脚本给出代理端点（`Manual` 模式，URL 可直接用）。
/// - `Ok(None)`：脚本判定直连。
/// - `Err(_)`：PAC 不可达 / 判据非法——调用方自行决定回退（静态代理或直连）。
pub(crate) fn resolve_proxy_for(
    source: &AutoProxySource,
    target_url: &str,
) -> Result<Option<ProxyConfig>, String> {
    let target = normalize_target(target_url);
    let key = format!("{}|{target}", source.key());
    let cache = CACHE.get_or_init(|| StdMutex::new(HashMap::new()));

    {
        let map = lock_cache(cache);
        if let Some(entry) = map.get(&key) {
            let fresh =
                entry.at.elapsed() < entry.outcome.ttl() && entry.signature == source.signature();
            if fresh {
                return match &entry.outcome {
                    Outcome::Direct => Ok(None),
                    Outcome::Proxied(config) => Ok(Some((**config).clone())),
                    Outcome::Failed(error) => Err(error.clone()),
                };
            }
        }
    }

    let outcome = match resolve_via_winhttp(source, &target) {
        Ok(Some(config)) => Outcome::Proxied(Box::new(config)),
        Ok(None) => Outcome::Direct,
        Err(error) => match fallback_from_script(source) {
            Some(config) => {
                log_info!(
                    "[pac] WinHTTP 求值失败（{error}），脚本兑底解析到 {}:{}（{}）",
                    config.host,
                    config.port,
                    config.proxy_type.as_str()
                );
                Outcome::Proxied(Box::new(config))
            }
            None => Outcome::Failed(error),
        },
    };

    {
        let mut map = lock_cache(cache);
        if map.len() >= CACHE_MAX_ENTRIES {
            map.clear();
        }
        map.insert(
            key,
            CacheEntry {
                signature: source.signature().to_string(),
                outcome: outcome.clone(),
                at: Instant::now(),
            },
        );
    }

    match outcome {
        Outcome::Direct => Ok(None),
        Outcome::Proxied(config) => Ok(Some(*config)),
        Outcome::Failed(error) => Err(error),
    }
}

fn lock_cache(
    cache: &StdMutex<HashMap<String, CacheEntry>>,
) -> std::sync::MutexGuard<'_, HashMap<String, CacheEntry>> {
    match cache.lock() {
        Ok(guard) => guard,
        // 缓存中毒（求值路径里不该 panic）不应让代理检测整体瘫掉：
        // 取回内部数据继续用。
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// 目标 URL 归一化：WinHTTP 只吃绝对 URL，`magnet:`/`ed2k:` 之类的哨兵
/// 与空串一律换成探针地址（PAC 对这类目标本来也无话可说）。
fn normalize_target(target_url: &str) -> String {
    let trimmed = target_url.trim();
    let has_scheme = ["http://", "https://", "ftp://"].iter().any(|prefix| {
        trimmed.len() > prefix.len()
            && trimmed
                .get(..prefix.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
    });
    if has_scheme {
        trimmed.to_string()
    } else {
        PAC_PROBE_URL.to_string()
    }
}

// ---------------------------------------------------------------------------
// WinHTTP 求值
// ---------------------------------------------------------------------------

/// 进程级共享的 WinHTTP 会话句柄。
///
/// 存成 `usize`（0 = 未建立）而非裸指针：静态变量因此不需要
/// `unsafe impl Send/Sync`，所有访问又都在同一把锁下串行——WinHTTP 句柄本身
/// 允许跨线程使用，串行只是为了让会话内的 PAC 缓存不被并发求值搅在一起。
static SESSION: OnceLock<StdMutex<usize>> = OnceLock::new();

/// 关掉缓存的会话（下次求值重建，从而重新下载 PAC）。
fn drop_session() {
    let Some(slot) = SESSION.get() else { return };
    let handle = {
        let mut guard = match slot.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let handle = *guard;
        *guard = 0;
        handle
    };
    if handle != 0 {
        // SAFETY: 句柄由本模块的 WinHttpOpen 建立，且已从缓存槽位摘除，
        // 不会有第二次释放。
        unsafe { WinHttpCloseHandle(handle as *mut core::ffi::c_void) };
    }
}

/// 在共享会话上执行一次调用。会话惰性建立；建立失败返回 `Err`。
fn with_session<T>(f: impl FnOnce(*mut core::ffi::c_void) -> T) -> Result<T, String> {
    let slot = SESSION.get_or_init(|| StdMutex::new(0));
    let mut guard = match slot.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    if *guard == 0 {
        let agent = to_wide("RinaDown");
        // 用 NO_PROXY 而不是 AUTOMATIC_PROXY：取回 PAC 这一步本身不能反过来
        // 依赖系统代理（否则为解析 PAC 去求值 PAC）。PAC 服务几乎都是本机
        // 回环地址（Clash / Verge / Shadowsocks 都自己跑一个本地 PAC 服务）。
        // SAFETY: 两个参数都是本函数持有的宽字符串，空代理参数按 API 约定允许。
        let handle = unsafe {
            WinHttpOpen(
                agent.as_ptr(),
                WINHTTP_ACCESS_TYPE_NO_PROXY,
                std::ptr::null(),
                std::ptr::null(),
                0,
            )
        };
        if handle.is_null() {
            return Err(format!("WinHttpOpen failed: {}", last_error_text()));
        }
        let (resolve, connect, send, receive) = SESSION_TIMEOUTS;
        // SAFETY: handle 刚由 WinHttpOpen 建立且非空。
        unsafe { WinHttpSetTimeouts(handle, resolve, connect, send, receive) };
        *guard = handle as usize;
        log_info!("[pac] WinHTTP 会话已建立");
    }
    let handle = *guard as *mut core::ffi::c_void;
    Ok(f(handle))
}

/// 一次真正的 PAC/WPAD 求值（无缓存）。
fn resolve_via_winhttp(
    source: &AutoProxySource,
    target_url: &str,
) -> Result<Option<ProxyConfig>, String> {
    let url_wide = to_wide(target_url);
    let config_url_wide = match source {
        AutoProxySource::ConfigUrl(url) => Some(to_wide(url)),
        AutoProxySource::AutoDetect => None,
    };

    // SAFETY: WINHTTP_AUTOPROXY_OPTIONS 是 #[repr(C)] 的 POD 结构，全零
    // 即「无标志、无 URL、不自动登录」，随后逐字段填写。
    let mut options: WINHTTP_AUTOPROXY_OPTIONS = unsafe { std::mem::zeroed() };
    match &config_url_wide {
        Some(url) => {
            options.dwFlags = WINHTTP_AUTOPROXY_CONFIG_URL;
            options.lpszAutoConfigUrl = url.as_ptr();
        }
        None => {
            options.dwFlags = WINHTTP_AUTOPROXY_AUTO_DETECT;
            options.dwAutoDetectFlags =
                WINHTTP_AUTO_DETECT_TYPE_DHCP | WINHTTP_AUTO_DETECT_TYPE_DNS_A;
        }
    }
    // fAutoLogonIfChallenged 保持 FALSE：PAC 服务要凭据时不做认证协商——
    // 宁可快速失败回退静态代理，也不在宿主线程上多等一轮往返。
    //
    // SAFETY: 同 options，POD 全零 = ACCESS_TYPE 未指定 + 两个空指针。
    let mut info: WINHTTP_PROXY_INFO = unsafe { std::mem::zeroed() };

    let (ok, last_error) = with_session(|session| {
        // SAFETY: session 由本模块建立；url_wide/options 活到调用结束；
        // info 是调用方提供的输出参数，文档允许其字段在失败时被写入，
        // 因此无论成败都在下面统一按 GlobalFree 归还。
        let ok =
            unsafe { WinHttpGetProxyForUrl(session, url_wide.as_ptr(), &mut options, &mut info) };
        let last_error = if ok == 0 {
            // SAFETY: 立刻取错误码（中间不存在其它可能覆盖它的调用）。
            unsafe { GetLastError() }
        } else {
            0
        };
        (ok, last_error)
    })?;

    // SAFETY: info 的字符串字段由 WinHTTP 分配（文档要求调用方 GlobalFree），
    // 本函数是唯一持有者，取出后立即复制并释放。
    let proxy_list = unsafe { take_wide(info.lpszProxy) };
    let bypass_list = unsafe { take_wide(info.lpszProxyBypass) };

    if ok == 0 {
        return Err(winhttp_error_text(last_error));
    }

    let Some((proxy_type, host, port)) = parse_proxy_list(&proxy_list) else {
        return Ok(None);
    };
    if port == 0 || host.is_empty() {
        return Ok(None);
    }

    Ok(Some(ProxyConfig {
        mode: ProxyMode::Manual,
        proxy_type,
        host,
        port,
        username: String::new(),
        password: String::new(),
        no_proxy_list: bypass_list
            .replace(';', ",")
            .replace("<local>", "localhost"),
    }))
}

// ---------------------------------------------------------------------------
// 脚本兜底解析（WinHTTP 失败时）
// ---------------------------------------------------------------------------

/// WinHTTP 失败后的兜底：自己取回脚本，只在「唯一回环端点」时采用。
fn fallback_from_script(source: &AutoProxySource) -> Option<ProxyConfig> {
    let script = match load_pac_script(source) {
        Ok(script) => script,
        Err(error) => {
            log_info!("[pac] 兜底取回 PAC 脚本失败: {error}");
            return None;
        }
    };
    let found = extract_single_loopback_proxy(&script);
    if found.is_none() {
        log_info!("[pac] 兜底解析未命中「唯一回环代理端点」判据，不猜代理");
    }
    found
}

/// 取回 PAC 脚本文本：本地文件直读，`http://` 自行取回，`https://` 放弃
/// （自建 HTTP 取回不背 TLS 栈，远端 PAC 只能指望 WinHTTP）。
fn load_pac_script(source: &AutoProxySource) -> Result<String, String> {
    let AutoProxySource::ConfigUrl(raw) = source else {
        return Err("WPAD 没有可直接取回的脚本地址".to_string());
    };
    let lower = raw.to_ascii_lowercase();
    if lower.starts_with("http://") {
        return http_get_text(raw);
    }
    if lower.starts_with("https://") {
        return Err("https PAC 需 TLS，交由 WinHTTP 处理".to_string());
    }
    let path = local_pac_path(raw).ok_or_else(|| format!("无法解析本地 PAC 路径: {raw}"))?;
    read_text_capped(&path)
}

/// `AutoConfigURL` 的本地形态 → 文件路径。既认 `file:///C:/...`，也认
/// 客户端直接写的裸路径 `C:\...\pac.txt`（Shadowsocks-Windows 等）。
fn local_pac_path(raw: &str) -> Option<std::path::PathBuf> {
    if raw.to_ascii_lowercase().starts_with("file://") {
        return url::Url::parse(raw).ok()?.to_file_path().ok();
    }
    let path = std::path::PathBuf::from(raw);
    path.is_absolute().then_some(path)
}

/// 读取 PAC 文件（带体积上限，避免指向超大文件时把内存吃光）。
fn read_text_capped(path: &std::path::Path) -> Result<String, String> {
    let meta = std::fs::metadata(path)
        .map_err(|e| format!("读取 PAC 文件 {} 失败: {e}", path.display()))?;
    if meta.len() > SCRIPT_MAX_BYTES {
        return Err(format!(
            "PAC 文件过大（{} 字节 > {SCRIPT_MAX_BYTES}）",
            meta.len()
        ));
    }
    let bytes =
        std::fs::read(path).map_err(|e| format!("读取 PAC 文件 {} 失败: {e}", path.display()))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// 极简 HTTP/1.1 GET（仅明文 http、直连）：取回 PAC 正文。
///
/// 不用 reqwest 的原因：本模块跑在同步上下文（可能就在宿主 actor 线程上），
/// reqwest 的 blocking 客户端在已有 runtime 里会 panic，async 又进不来。
/// PAC 服务端都是本机或内网的朴素 HTTP 服务，这点代码够用且不引入 TLS 依赖。
fn http_get_text(url_str: &str) -> Result<String, String> {
    use std::io::{Read, Write};
    use std::net::{TcpStream, ToSocketAddrs};

    let parsed = url::Url::parse(url_str).map_err(|e| format!("PAC URL 非法: {e}"))?;
    let host = parsed.host_str().ok_or("PAC URL 缺少主机名")?.to_string();
    let port = parsed.port_or_known_default().ok_or("PAC URL 缺少端口")?;
    let mut path = parsed.path().to_string();
    if path.is_empty() {
        path.push('/');
    }
    if let Some(query) = parsed.query() {
        path.push('?');
        path.push_str(query);
    }

    let addrs = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| format!("PAC 主机解析失败: {e}"))?
        .collect::<Vec<_>>();
    if addrs.is_empty() {
        return Err("PAC 主机无可用地址".to_string());
    }

    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nUser-Agent: RinaDown\r\nAccept: */*\r\nConnection: close\r\n\r\n"
    );
    let mut last_error = String::new();
    for addr in addrs {
        let mut stream = match TcpStream::connect_timeout(&addr, SCRIPT_TIMEOUT) {
            Ok(stream) => stream,
            Err(e) => {
                last_error = format!("连接 {addr} 失败: {e}");
                continue;
            }
        };
        let _ = stream.set_read_timeout(Some(SCRIPT_TIMEOUT));
        let _ = stream.set_write_timeout(Some(SCRIPT_TIMEOUT));
        if let Err(e) = stream.write_all(request.as_bytes()) {
            last_error = format!("发送 PAC 请求失败: {e}");
            continue;
        }
        let mut raw = Vec::new();
        let mut limited = stream.take(SCRIPT_MAX_BYTES + 8192);
        if let Err(e) = limited.read_to_end(&mut raw) {
            last_error = format!("读取 PAC 响应失败: {e}");
            continue;
        }
        return parse_http_body(&raw);
    }
    Err(last_error)
}

/// 从原始响应字节里切出正文（处理 `Content-Length` / `chunked` / 读到断开）。
fn parse_http_body(raw: &[u8]) -> Result<String, String> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("PAC 响应缺少头体分隔")?;
    let head = String::from_utf8_lossy(&raw[..split]);
    let body = &raw[split + 4..];

    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("");
    if status != "200" {
        return Err(format!("PAC 服务返回 HTTP {status}"));
    }

    let chunked = head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked");
    if chunked {
        return dechunk(body);
    }
    Ok(String::from_utf8_lossy(body).into_owned())
}

/// 解开 chunked 传输编码。
fn dechunk(body: &[u8]) -> Result<String, String> {
    let mut out: Vec<u8> = Vec::new();
    let mut rest = body;
    loop {
        let line_end = rest
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or("chunk 长度行缺失")?;
        let size_field = String::from_utf8_lossy(&rest[..line_end]);
        let size_text = size_field.split(';').next().unwrap_or("").trim();
        let size =
            usize::from_str_radix(size_text, 16).map_err(|e| format!("chunk 长度非法: {e}"))?;
        let data_start = line_end + 2;
        if size == 0 {
            break;
        }
        if rest.len() < data_start + size {
            return Err("chunk 数据不完整".to_string());
        }
        out.extend_from_slice(&rest[data_start..data_start + size]);
        // 跳过数据与其后的 CRLF（尾块可能是裸 LF，宽容处理）。
        let mut next = data_start + size;
        if rest.len() >= next + 2 && &rest[next..next + 2] == b"\r\n" {
            next += 2;
        } else if rest.len() > next && rest[next] == b'\n' {
            next += 1;
        }
        rest = &rest[next..];
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// 仅当整份脚本只给出**一个回环代理端点**时返回它。
///
/// 判据刻意收紧（见模块文档）：多个不同端点（按规则分流到不同代理）或端点
/// 不在本机（企业 PAC 的远端代理）时一律不猜——宁可不走代理，也不把流量
/// 塞进错误的出口。命中场景是「把一切交给本机 VPN 客户端」这类 PAC：
/// `PROXY 127.0.0.1:<mixed-port>; SOCKS5 127.0.0.1:<mixed-port>; DIRECT;`。
fn extract_single_loopback_proxy(script: &str) -> Option<ProxyConfig> {
    if !script.contains("FindProxyForURL") {
        return None;
    }
    let mut chosen: Option<(ProxyType, String, u16)> = None;
    for (proxy_type, host, port) in proxy_directives(script) {
        if !is_loopback_host(&host) {
            return None;
        }
        match &chosen {
            // 同一个端点的重复声明（PROXY 与 SOCKS5 指向同一个 mixed-port）：
            // 保留先出现的类型——PAC 的条目顺序就是优先级顺序，第一个通常
            // 是 HTTP `CONNECT`，对下载最稳。
            Some(existing) if existing.1 == host && existing.2 == port => {}
            Some(_) => return None,
            None => chosen = Some((proxy_type, host, port)),
        }
    }
    let (proxy_type, host, port) = chosen?;
    Some(ProxyConfig {
        mode: ProxyMode::Manual,
        proxy_type,
        host,
        port,
        username: String::new(),
        password: String::new(),
        no_proxy_list: String::new(),
    })
}

/// 脚本里出现的 PAC 代理指令（按出现顺序）：`PROXY` / `HTTP` / `HTTPS` /
/// `SOCKS5` / `SOCKS4` / `SOCKS` 后跟 `host:port`。
fn proxy_directives(script: &str) -> Vec<(ProxyType, String, u16)> {
    static RE: OnceLock<Option<regex::Regex>> = OnceLock::new();
    let Some(re) = RE
        .get_or_init(|| {
            regex::Regex::new(r"(?i)\b(PROXY|HTTP|HTTPS|SOCKS5|SOCKS4|SOCKS)\s+([^\s;,'\x22]+)")
                .ok()
        })
        .as_ref()
    else {
        return Vec::new();
    };

    let mut found = Vec::new();
    for caps in re.captures_iter(script) {
        let (Some(kind), Some(addr)) = (caps.get(1), caps.get(2)) else {
            continue;
        };
        let proxy_type = match kind.as_str().to_ascii_uppercase().as_str() {
            "PROXY" | "HTTP" => ProxyType::Http,
            "HTTPS" => ProxyType::Https,
            "SOCKS4" => ProxyType::Socks4,
            // PAC 里的裸 `SOCKS` 历史上指 SOCKS4，但现实中的分流脚本
            // （v2rayN / Shadowsocks / Clash）一律写 `SOCKS5`，此处按 5 处理
            // 更贴合实际；纯 SOCKS4 服务端本来也不吃域名。
            "SOCKS" | "SOCKS5" => ProxyType::Socks5,
            _ => continue,
        };
        let token = addr.as_str().trim_end_matches(':');
        let Some((host, port)) = split_host_port(token) else {
            continue;
        };
        found.push((proxy_type, host, port));
    }
    found
}

/// `host:port` → `(host, port)`；要求显式端口（PAC 里不该省略）。
fn split_host_port(token: &str) -> Option<(String, u16)> {
    let (host, port) = token.rsplit_once(':')?;
    let port = port.parse::<u16>().ok()?;
    if host.is_empty() || port == 0 {
        return None;
    }
    Some((host.to_string(), port))
}

/// 是否回环地址（本机 VPN 客户端的监听端点）。
fn is_loopback_host(host: &str) -> bool {
    let host = host.trim_matches(|c| c == '[' || c == ']');
    host.eq_ignore_ascii_case("localhost")
        || host.starts_with("127.")
        || host == "::1"
        || host == "0:0:0:0:0:0:0:1"
}

/// 解析 WinHTTP 返回的代理列表（`lpszProxy`）。
///
/// WinHTTP 用与注册表 `ProxyServer` 同构的写法回话：单项 `host:port`、
/// 多项 `proto=host:port;proto=host:port`，`PROXY`/`DIRECT` 这些 PAC 语法
/// 残留已被它自己消化。空串与字面量 `DIRECT` 都是直连。
fn parse_proxy_list(value: &str) -> Option<(ProxyType, String, u16)> {
    let value = value.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("direct") {
        return None;
    }
    // 带协议前缀的列表（含 `socks=`）交给注册表那套解析器：它已经处理了
    // 「`https=` 描述目的地、传输仍是 HTTP」这类映射（见其文档）。
    let first = value
        .split(';')
        .map(str::trim)
        .find(|entry| !entry.is_empty())?;
    let parsed = if value.contains('=') {
        parse_windows_proxy_server(value)
    } else {
        let (scheme, rest) = match first.split_once("://") {
            Some((scheme, rest)) => (scheme, rest),
            None => ("", first),
        };
        let (host, port) = crate::proxy_config::parse_host_port(rest);
        let proxy_type = ProxyType::parse_str(scheme).unwrap_or(ProxyType::Http);
        (proxy_type, host, port)
    };
    let (proxy_type, host, port) = parsed;
    if host.is_empty() || port == 0 {
        return None;
    }
    Some((proxy_type, host, port))
}

/// 复制并释放一条 WinHTTP 分配的宽字符串；空指针返回空串。
///
/// # Safety
///
/// `ptr` 必须是 WinHTTP 分配、尚未释放、以 NUL 结尾的宽字符串（或空指针）。
unsafe fn take_wide(ptr: *mut u16) -> String {
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: 由调用方保证指针有效且 NUL 结尾。
    let len = unsafe {
        let mut len = 0usize;
        while *ptr.add(len) != 0 {
            len += 1;
        }
        len
    };
    // SAFETY: 上面已量出长度，切片不越界。
    let value = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(ptr, len) });
    // SAFETY: 同一指针只释放一次（复制已完成，此后不再使用）。
    unsafe { GlobalFree(ptr as HGLOBAL) };
    value
}

/// UTF-16（带 NUL 结尾）编码。
fn to_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 读取线程最近的 Win32 错误码并转成可读文本。
fn last_error_text() -> String {
    // SAFETY: GetLastError 无参数、无副作用。
    winhttp_error_text(unsafe { GetLastError() })
}

/// WinHTTP/Win32 错误码 → 可读文本，并留下一条诊断日志。
fn winhttp_error_text(code: u32) -> String {
    let name = match code {
        0 => return "操作成功".to_string(),
        12002 => "PAC 请求超时",
        12006 => "PAC 服务器已断开连接",
        12007 => "PAC 服务器域名无法解析",
        12029..=12032 => "PAC 服务器无法连接",
        12175..=12179 => "PAC 下载失败（TLS/连接错误）",
        12180 => "自动发现 WPAD 失败（网络里没有 PAC）",
        _ => "未知错误",
    };
    log_error!("[pac] 求值失败：{name}（code={code}）");
    format!("{name} (code={code})")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 取解析结果，直连/无法解析时给一个可断言的中性值。
    fn parsed(value: &str) -> (ProxyType, String, u16) {
        parse_proxy_list(value).unwrap_or((ProxyType::Http, String::new(), 0))
    }

    #[test]
    fn parse_proxy_list_handles_simple_and_prefixed_forms() {
        assert_eq!(parse_proxy_list(""), None);
        assert_eq!(parse_proxy_list("DIRECT"), None);
        assert_eq!(parse_proxy_list("direct"), None);

        let (proxy_type, host, port) = parsed("127.0.0.1:7897");
        assert_eq!(proxy_type, ProxyType::Http);
        assert_eq!(host, "127.0.0.1");
        assert_eq!(port, 7897);

        // 多条目：WinHTTP 已按 PAC 顺序给出，取第一个。
        let (_, host, port) = parsed("10.0.0.1:8080;10.0.0.2:8080");
        assert_eq!(host, "10.0.0.1");
        assert_eq!(port, 8080);

        // 带协议前缀的条目按前缀定类型。
        let (proxy_type, host, port) = parsed("socks=127.0.0.1:1080");
        assert_eq!(proxy_type, ProxyType::Socks5);
        assert_eq!(host, "127.0.0.1");
        assert_eq!(port, 1080);

        let (proxy_type, _, _) = parsed("http://127.0.0.1:7890");
        assert_eq!(proxy_type, ProxyType::Http);
    }

    #[test]
    fn normalize_target_falls_back_to_probe_for_non_http_schemes() {
        assert_eq!(
            normalize_target("https://a.example/f"),
            "https://a.example/f"
        );
        assert_eq!(normalize_target("HTTP://a.example/f"), "HTTP://a.example/f");
        assert_eq!(normalize_target("magnet:?xt=urn:btih:x"), PAC_PROBE_URL);
        assert_eq!(normalize_target(""), PAC_PROBE_URL);
    }

    #[test]
    fn auto_proxy_source_does_not_panic() {
        // 结果取决于运行机器的注册表；只验证读取路径可用。
        let _ = auto_proxy_source();
    }

    /// Clash Verge Rev 的默认 PAC（`pac_file_content`）——两个条目指向同一个
    /// mixed-port，应取先出现的 PROXY/HTTP。
    const VERGE_PAC: &str = "function FindProxyForURL(url, host) {\r\n  \
         return \"PROXY 127.0.0.1:7897; SOCKS5 127.0.0.1:7897; DIRECT;\";\r\n}\r\n";

    #[test]
    fn extract_takes_single_loopback_endpoint() {
        let config = extract_single_loopback_proxy(VERGE_PAC).expect("verge pac");
        assert_eq!(config.proxy_type, ProxyType::Http);
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 7897);
        assert_eq!(config.mode, ProxyMode::Manual);
    }

    #[test]
    fn extract_handles_rule_based_script_with_one_local_port() {
        // Shadowsocks-Windows 式规则脚本：规则再多，代理端点只有一个且在本机。
        let script = r#"
var proxy = "PROXY 127.0.0.1:1080; DIRECT;";
var direct = "DIRECT;";
function FindProxyForURL(url, host) {
  if (shExpMatch(host, "*.cn")) return direct;
  if (isInNet(dnsResolve(host), "192.168.0.0", "255.255.0.0")) return direct;
  return proxy;
}
"#;
        let config = extract_single_loopback_proxy(script).expect("ss pac");
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 1080);
    }

    #[test]
    fn extract_refuses_multiple_or_remote_endpoints() {
        // 两个不同端点：按规则分流到不同代理，不猜。
        let two = "function FindProxyForURL(u,h){ return \"PROXY 127.0.0.1:7897; DIRECT;\"; }\n\
                   function Other(){ return \"PROXY 127.0.0.1:7898;\"; }";
        assert!(extract_single_loopback_proxy(two).is_none());

        // 唯一端点但不在本机：企业 PAC 的远端代理，不猜。
        let remote =
            "function FindProxyForURL(u,h){ return \"PROXY proxy.corp.example:8080; DIRECT;\"; }";
        assert!(extract_single_loopback_proxy(remote).is_none());

        // 不是 PAC。
        assert!(extract_single_loopback_proxy("PROXY 127.0.0.1:7897").is_none());
        // 纯直连脚本：没有可用端点。
        assert!(
            extract_single_loopback_proxy("function FindProxyForURL(u,h){return \"DIRECT\";}")
                .is_none()
        );
    }

    #[test]
    fn local_pac_path_accepts_file_url_and_plain_path() {
        let from_url = local_pac_path("file:///C:/Users/fan/pac.txt").expect("file url");
        assert!(from_url.to_string_lossy().ends_with("pac.txt"));
        let plain = local_pac_path(r"C:\Users\fan\pac.txt").expect("plain path");
        assert!(plain.is_absolute());
        assert!(local_pac_path("https://example.com/pac").is_none());
    }

    #[test]
    fn parse_http_body_handles_content_length_and_chunked() {
        let plain = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello";
        assert_eq!(parse_http_body(plain).unwrap_or_default(), "hello");

        let chunked =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n";
        assert_eq!(parse_http_body(chunked).unwrap_or_default(), "hello");

        let not_found = b"HTTP/1.1 404 Not Found\r\n\r\n";
        assert!(parse_http_body(not_found).is_err());
    }
}
