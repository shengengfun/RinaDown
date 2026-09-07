//! `EngineBridge` —— [`PluginBridge`] 实现：网络出口守卫 + storage + log + 重试意图。
//!
//! ## 网络出口守卫（防 SSRF）
//! 保护本机与 headless server 不被恶意/有 bug 的第三方插件访问内部服务。核心是
//! **单一判定函数** [`is_globally_routable_unicast`]，供三处复用（杜绝判定漂移）：
//! 1. 字面量 IP 前置校验（进入 reqwest 之前，挡 hyper-util 对字面量 IP 的短路）；
//! 2. 自定义 `reqwest::dns::Resolve`（解析后过滤，挡 DNS rebinding、消 TOCTOU）；
//! 3. 逐跳重定向 `Policy::custom`（手动重建 30 跳上限 + 每跳字面量 IP 校验）。
//!
//! ## v1 限制（记录在案）
//! - `proxy` 在 bridge 构造时快照（reqwest ClientBuilder 配置构建时定死）；运行期改
//!   代理后插件出口不随动（可接受，非安全问题）。
//! - 单次调用严格 per-call fetch 上限退化为**全局并发 fetch 上限**（对宿主保护更强）。
//! - 配置代理时 DNS 由代理侧解析，[`GuardResolver`] 不参与（hostname 级过滤失效；
//!   字面量 IP 前置校验与逐跳重定向校验仍然生效）。代理由用户显式配置，视为可信出口。

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use tokio::process::Command;
use tokio::sync::{Semaphore, mpsc};

use crate::db::Db;
use crate::logger::{log_error, log_info};
use crate::proxy_config::ProxyConfig;

use super::runtime::{
    BridgeHttpRequest, BridgeHttpResponse, FfmpegAvailability, FfmpegOutcome, FfmpegSpec,
    PluginBridge, PluginError, PluginLogLevel, YtdlpAvailability, YtdlpOutcome, YtdlpSpec,
};

/// 响应体上限（超限截断 + `truncated:true`）。
const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;
/// 单请求超时。
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
/// 全局并发 fetch 上限。
const MAX_CONCURRENT_FETCH: usize = 8;
/// 重定向跳数上限（Policy::custom 丢失 limited 默认保护，手动重建）。
const MAX_REDIRECTS: usize = 30;
/// 单值上限 64KB。
const MAX_STORAGE_VALUE: usize = 64 * 1024;
/// 单插件 storage 键数上限。
const MAX_STORAGE_KEYS: usize = 100;
/// 单条日志截断长度。
const MAX_LOG_BYTES: usize = 4 * 1024;
/// ffmpeg 单次调用默认超时（缺省 `timeoutMs` 时）。
const FFMPEG_DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);
/// ffmpeg 单次调用超时上限（裁剪 `timeoutMs`）。
const FFMPEG_MAX_TIMEOUT: Duration = Duration::from_secs(1800);
/// 全局并发 ffmpeg 进程上限（CPU/IO 密集，保守取 2）。
const MAX_CONCURRENT_FFMPEG: usize = 2;
/// ffmpeg 参数条数上限。
const MAX_FFMPEG_ARGS: usize = 512;
/// ffmpeg 单参数字节上限。
const MAX_FFMPEG_ARG_LEN: usize = 8 * 1024;
/// ffmpeg stdout 回传上限（够 ffprobe 式 JSON；超限截断）。
const FFMPEG_STDOUT_CAP: usize = 256 * 1024;
/// ffmpeg stderr 回传上限（超限截断）。
const FFMPEG_STDERR_CAP: usize = 64 * 1024;
/// yt-dlp 单次调用默认超时（缺省 `timeoutMs` 时；提取通常数秒，下载可长）。
const YTDLP_DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);
/// yt-dlp 单次调用超时上限（裁剪 `timeoutMs`）。
const YTDLP_MAX_TIMEOUT: Duration = Duration::from_secs(3600);
/// 全局并发 yt-dlp 进程上限。
const MAX_CONCURRENT_YTDLP: usize = 2;
/// yt-dlp 参数条数上限。
const MAX_YTDLP_ARGS: usize = 512;
/// yt-dlp 单参数字节上限。
const MAX_YTDLP_ARG_LEN: usize = 8 * 1024;
/// yt-dlp stdout 回传上限（`-J` 播放列表 JSON 可较大；超限截断）。
const YTDLP_STDOUT_CAP: usize = 4 * 1024 * 1024;
/// yt-dlp stderr 回传上限（超限截断）。
const YTDLP_STDERR_CAP: usize = 256 * 1024;

/// flux.fs 单文件字节上限。
const MAX_FS_FILE_BYTES: usize = 8 * 1024 * 1024;
/// flux.fs 单插件工作区顶层文件总量上限。
const MAX_FS_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
/// flux.fs 单插件工作区顶层文件数上限。
const MAX_FS_FILES: usize = 100;
/// flux.fs 文件名长度上限。
const MAX_FS_NAME_LEN: usize = 255;

/// 插件工作区目录：`<data_dir>/plugins-work/<sanitized_id>/`。flux.fs 与
/// flux.ytdlp 共用同一根（cwd 对齐），使插件经 flux.fs 物化的输入文件正好落在
/// 工具的工作目录里。`plugin_id` 经清洗（非 `[a-z0-9_-]` → `_`）成安全目录名。
fn plugin_workspace(data_dir: &Path, plugin_id: &str) -> PathBuf {
    let safe_id: String = plugin_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    data_dir.join("plugins-work").join(safe_id)
}

/// flux.fs 文件名拒绝原因（`None` = 放行）。仅允许单层安全文件名：非空、
/// ≤`MAX_FS_NAME_LEN`、无路径分隔/盘符/NUL、非 `.`/`..`。
fn fs_name_reject_reason(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        return Some("空文件名");
    }
    if name.len() > MAX_FS_NAME_LEN {
        return Some("文件名过长");
    }
    if name == "." || name == ".." {
        return Some(". / .. 保留名");
    }
    if name.contains(['/', '\\', ':']) || name.contains('\0') {
        return Some("含路径分隔/盘符/NUL");
    }
    None
}

/// 为受管外部工具（yt-dlp/ffmpeg/…）子进程补全 `PATH`，使其能发现用户/系统
/// 安装的 JS 运行时（node/deno）等——这些常装在**用户级 PATH**（如 scoop 的
/// `~\scoop\shims`、`~\scoop\apps\nodejs\current`）。
///
/// 根因：GUI 应用（尤其非从终端启动）继承的 `PATH` 常**缺失用户环境变量段**，
/// 子进程 `Command` 默认只继承这份残缺 `PATH` → yt-dlp 找不到 node → n-sig
/// 挑战求解失败 → 只能拿到缩略图。此处从注册表读**权威完整 PATH**
/// （`HKLM\...\Session Manager\Environment\Path` + `HKCU\Environment\Path`），
/// 与进程现有 `PATH` 合并去重后设回子进程——不硬编码任何具体安装目录。
///
/// 仅 Windows 生效；其余平台 `Command` 继承的 `PATH` 已足够，为空操作。
#[cfg(windows)]
fn apply_full_path_env(cmd: &mut Command) {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

    fn read_path(root: winreg::RegKey, sub: &str) -> Option<String> {
        root.open_subkey(sub)
            .ok()
            .and_then(|k| k.get_value::<String, _>("Path").ok())
            .filter(|s| !s.trim().is_empty())
    }

    // 收集：进程现有 PATH（保留其顺序优先）+ 注册表系统/用户 PATH，去重合并。
    let mut seen = std::collections::HashSet::new();
    let mut merged: Vec<String> = Vec::new();
    let mut push_all = |raw: &str| {
        for seg in raw.split(';') {
            let seg = seg.trim();
            if seg.is_empty() {
                continue;
            }
            let key = seg.to_ascii_lowercase();
            if seen.insert(key) {
                merged.push(seg.to_string());
            }
        }
    };

    let inherited = std::env::var("PATH").unwrap_or_default();
    let inherited_segs = if inherited.is_empty() {
        0
    } else {
        inherited
            .split(';')
            .filter(|s| !s.trim().is_empty())
            .count()
    };
    push_all(&inherited);
    if let Some(system) = read_path(
        RegKey::predef(HKEY_LOCAL_MACHINE),
        r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
    ) {
        push_all(&system);
    }
    if let Some(user) = read_path(RegKey::predef(HKEY_CURRENT_USER), "Environment") {
        push_all(&user);
    }

    // 持久化：记录补全前后段数（补入的多为用户级 PATH，如 node/deno 安装目录）。
    log_info!(
        "[ytdlp-env] PATH 补全: 继承 {inherited_segs} 段 → 合并 {} 段（+{} 段来自注册表）",
        merged.len(),
        merged.len().saturating_sub(inherited_segs)
    );

    if !merged.is_empty() {
        cmd.env("PATH", merged.join(";"));
    }
}

/// 非 Windows：`Command` 继承的 `PATH` 已足够，空操作。
#[cfg(not(windows))]
fn apply_full_path_env(_cmd: &mut Command) {}

/// 唯一的「可全局路由单播」判定函数。三处复用，杜绝判定逻辑漂移。
///
/// 拒绝一切环回/私网/链路本地/文档/保留/元数据段等非公网单播地址。
///
/// # Examples
///
/// ```
/// use std::net::IpAddr;
/// use fluxdown_engine::plugin::bridge::is_globally_routable_unicast;
///
/// assert!(is_globally_routable_unicast("8.8.8.8".parse::<IpAddr>().unwrap()));
/// assert!(!is_globally_routable_unicast("127.0.0.1".parse::<IpAddr>().unwrap()));
/// assert!(!is_globally_routable_unicast("169.254.169.254".parse::<IpAddr>().unwrap()));
/// ```
pub fn is_globally_routable_unicast(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_v4_routable(v4),
        IpAddr::V6(v6) => is_v6_routable(v6),
    }
}

fn is_v4_routable(ip: Ipv4Addr) -> bool {
    if ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_unspecified()
        || ip.is_documentation()
    {
        return false;
    }
    let o = ip.octets();
    // 0.0.0.0/8 "this network"
    if o[0] == 0 {
        return false;
    }
    // 100.64.0.0/10 CGNAT
    if o[0] == 100 && (o[1] & 0xc0) == 0x40 {
        return false;
    }
    // 198.18.0.0/15 benchmarking
    if o[0] == 198 && (o[1] & 0xfe) == 18 {
        return false;
    }
    // 240.0.0.0/4 reserved
    if (o[0] & 0xf0) == 240 {
        return false;
    }
    true
}

fn is_v6_routable(ip: Ipv6Addr) -> bool {
    // IPv4-mapped ::ffff:a.b.c.d → 解包递归。
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_v4_routable(v4);
    }
    let seg = ip.segments();
    // 6to4 2002::/16 → 内嵌 IPv4 递归。
    if seg[0] == 0x2002 {
        let v4 = Ipv4Addr::new(
            (seg[1] >> 8) as u8,
            (seg[1] & 0xff) as u8,
            (seg[2] >> 8) as u8,
            (seg[2] & 0xff) as u8,
        );
        return is_v4_routable(v4);
    }
    // NAT64 64:ff9b::/96 → 末 32bit 递归。
    if seg[0] == 0x0064 && seg[1] == 0xff9b {
        let v4 = Ipv4Addr::new(
            (seg[6] >> 8) as u8,
            (seg[6] & 0xff) as u8,
            (seg[7] >> 8) as u8,
            (seg[7] & 0xff) as u8,
        );
        return is_v4_routable(v4);
    }
    if ip.is_loopback()
        || ip.is_multicast()
        || ip.is_unspecified()
        || ip.is_unique_local()
        || ip.is_unicast_link_local()
    {
        return false;
    }
    // Teredo 2001:0000::/32
    if seg[0] == 0x2001 && seg[1] == 0x0000 {
        return false;
    }
    // documentation 2001:0db8::/32
    if seg[0] == 0x2001 && seg[1] == 0x0db8 {
        return false;
    }
    true
}

/// 守卫用自定义 DNS 解析器：解析后仅保留可全局路由的地址。
struct GuardResolver;

impl reqwest::dns::Resolve for GuardResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            let addrs = tokio::net::lookup_host((host.as_str(), 0)).await?;
            let filtered: Vec<SocketAddr> = addrs
                .filter(|sa| is_globally_routable_unicast(sa.ip()))
                .collect();
            let iter: reqwest::dns::Addrs = Box::new(filtered.into_iter());
            Ok(iter)
        })
    }
}

#[derive(Debug, thiserror::Error)]
enum GuardError {
    #[error("too many redirects")]
    TooManyRedirects,
    #[error("blocked: non-routable redirect target")]
    BlockedRedirect,
}

/// 引擎侧 `PluginBridge` 实现。
pub struct EngineBridge {
    client: reqwest::Client,
    db: Db,
    plugin_retry_tx: mpsc::UnboundedSender<(String, u64)>,
    fetch_sema: Arc<Semaphore>,
    /// 数据目录：供 `flux.ffmpeg` 解析生效 ffmpeg 路径（manual→managed→system）。
    data_dir: PathBuf,
    /// 全局并发 ffmpeg 进程限流。
    ffmpeg_sema: Arc<Semaphore>,
    /// 全局并发 yt-dlp 进程限流。
    ytdlp_sema: Arc<Semaphore>,
}

impl EngineBridge {
    /// 构造带守卫的 bridge。`proxy` 在此快照进 Client（v1 限制，见模块文档）。
    pub fn new(
        db: Db,
        proxy: &ProxyConfig,
        plugin_retry_tx: mpsc::UnboundedSender<(String, u64)>,
        data_dir: PathBuf,
    ) -> Result<Self, PluginError> {
        let mut builder = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .dns_resolver(Arc::new(GuardResolver))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= MAX_REDIRECTS {
                    return attempt.error(GuardError::TooManyRedirects);
                }
                if let Some(host) = attempt.url().host_str() {
                    let trimmed = host.trim_matches(|c| c == '[' || c == ']');
                    if let Ok(ip) = trimmed.parse::<IpAddr>()
                        && !is_globally_routable_unicast(ip)
                    {
                        return attempt.error(GuardError::BlockedRedirect);
                    }
                }
                attempt.follow()
            }));

        if let Some(url) = proxy.resolve().to_proxy_url()
            && let Ok(p) = reqwest::Proxy::all(&url)
        {
            builder = builder.proxy(p);
        }

        let client = builder
            .build()
            .map_err(|e| PluginError::Runtime(format!("构建守卫 Client 失败: {e}")))?;
        Ok(Self {
            client,
            db,
            plugin_retry_tx,
            fetch_sema: Arc::new(Semaphore::new(MAX_CONCURRENT_FETCH)),
            data_dir,
            ffmpeg_sema: Arc::new(Semaphore::new(MAX_CONCURRENT_FFMPEG)),
            ytdlp_sema: Arc::new(Semaphore::new(MAX_CONCURRENT_YTDLP)),
        })
    }
}

#[async_trait::async_trait]
impl PluginBridge for EngineBridge {
    async fn http_request(
        &self,
        _plugin_id: &str,
        req: BridgeHttpRequest,
    ) -> Result<BridgeHttpResponse, PluginError> {
        // scheme 仅 http/https。
        let parsed = url::Url::parse(&req.url)
            .map_err(|e| PluginError::InvalidOutput(format!("URL 非法: {e}")))?;
        if parsed.scheme() != "http" && parsed.scheme() != "https" {
            return Err(PluginError::InvalidOutput(
                "flux.fetch 仅支持 http/https".to_string(),
            ));
        }
        // 字面量 IP 前置校验（挡 hyper-util 对字面量 IP 的短路）。
        if let Some(host) = parsed.host_str() {
            let trimmed = host.trim_matches(|c| c == '[' || c == ']');
            if let Ok(ip) = trimmed.parse::<IpAddr>()
                && !is_globally_routable_unicast(ip)
            {
                return Err(PluginError::InvalidOutput(
                    "blocked: non-routable IP".to_string(),
                ));
            }
        }

        let permit = self
            .fetch_sema
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| PluginError::Runtime("fetch semaphore closed".to_string()))?;
        let _permit = permit;

        let method = reqwest::Method::from_bytes(req.method.as_bytes())
            .map_err(|_| PluginError::InvalidOutput(format!("HTTP method 非法: {}", req.method)))?;
        let mut rb = self.client.request(method, parsed);
        for (k, v) in &req.headers {
            if let (Ok(name), Ok(value)) = (
                reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                reqwest::header::HeaderValue::from_str(v),
            ) {
                rb = rb.header(name, value);
            }
        }
        if let Some(body) = req.body {
            rb = rb.body(body);
        }

        let mut resp = rb
            .send()
            .await
            .map_err(|e| PluginError::Runtime(format!("fetch 失败: {e}")))?;
        let status = resp.status().as_u16();
        let mut headers = std::collections::HashMap::new();
        for (k, v) in resp.headers() {
            if let Ok(s) = v.to_str() {
                headers.insert(k.as_str().to_string(), s.to_string());
            }
        }

        let mut body = Vec::new();
        let mut truncated = false;
        loop {
            match resp.chunk().await {
                Ok(Some(chunk)) => {
                    if body.len() + chunk.len() > MAX_BODY_BYTES {
                        let take = MAX_BODY_BYTES - body.len();
                        body.extend_from_slice(&chunk[..take]);
                        truncated = true;
                        break;
                    }
                    body.extend_from_slice(&chunk);
                }
                Ok(None) => break,
                Err(e) => return Err(PluginError::Runtime(format!("读取响应体失败: {e}"))),
            }
        }

        Ok(BridgeHttpResponse {
            status,
            headers,
            body: String::from_utf8_lossy(&body).to_string(),
            truncated,
        })
    }

    async fn storage_get(&self, plugin_id: &str, key: &str) -> Option<String> {
        let full = format!("plugin.{plugin_id}.kv.{key}");
        self.db.get_config(&full).await.ok().flatten()
    }

    async fn storage_set(
        &self,
        plugin_id: &str,
        key: &str,
        value: String,
    ) -> Result<(), PluginError> {
        if value.len() > MAX_STORAGE_VALUE {
            return Err(PluginError::InvalidOutput(format!(
                "storage 值超过 {MAX_STORAGE_VALUE} 字节上限"
            )));
        }
        let prefix = format!("plugin.{plugin_id}.kv.");
        let full = format!("{prefix}{key}");
        // 键数上限：仅当是新键时才计数。
        if let Ok(existing) = self.db.list_config_with_prefix(&prefix).await {
            let is_new = !existing.iter().any(|(k, _)| k == &full);
            if is_new && existing.len() >= MAX_STORAGE_KEYS {
                return Err(PluginError::InvalidOutput(format!(
                    "storage 键数超过 {MAX_STORAGE_KEYS} 上限"
                )));
            }
        }
        self.db
            .set_config(&full, &value)
            .await
            .map_err(|e| PluginError::Runtime(format!("storage 写入失败: {e}")))
    }

    async fn fs_write(
        &self,
        plugin_id: &str,
        name: &str,
        content: String,
    ) -> Result<(), PluginError> {
        if let Some(r) = fs_name_reject_reason(name) {
            return Err(PluginError::InvalidOutput(format!(
                "flux.fs 文件名 '{name}' 非法: {r}"
            )));
        }
        if content.len() > MAX_FS_FILE_BYTES {
            return Err(PluginError::InvalidOutput(format!(
                "flux.fs 文件超过 {MAX_FS_FILE_BYTES} 字节上限"
            )));
        }
        let ws = plugin_workspace(&self.data_dir, plugin_id);
        tokio::fs::create_dir_all(&ws)
            .await
            .map_err(|e| PluginError::Runtime(format!("flux.fs 创建工作区失败: {e}")))?;
        // 容量/文件数上限：统计顶层文件（跳过 `.cache` 等子目录）。
        let (mut count, mut total, mut existing, mut found) = (0usize, 0u64, 0u64, false);
        if let Ok(mut rd) = tokio::fs::read_dir(&ws).await {
            while let Ok(Some(e)) = rd.next_entry().await {
                if e.file_type().await.map(|t| t.is_file()).unwrap_or(false) {
                    count += 1;
                    let sz = e.metadata().await.map(|m| m.len()).unwrap_or(0);
                    total += sz;
                    if e.file_name().to_string_lossy() == name {
                        found = true;
                        existing = sz;
                    }
                }
            }
        }
        if !found && count >= MAX_FS_FILES {
            return Err(PluginError::InvalidOutput(format!(
                "flux.fs 文件数超过 {MAX_FS_FILES} 上限"
            )));
        }
        if total - existing + content.len() as u64 > MAX_FS_TOTAL_BYTES {
            return Err(PluginError::InvalidOutput(format!(
                "flux.fs 工作区总量超过 {MAX_FS_TOTAL_BYTES} 字节上限"
            )));
        }
        let path = ws.join(name);
        tokio::fs::write(&path, content.as_bytes())
            .await
            .map_err(|e| PluginError::Runtime(format!("flux.fs 写入失败: {e}")))?;
        // 敏感输入（如 cookie）尽力设 0600。
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).await;
        }
        Ok(())
    }

    async fn fs_read(&self, plugin_id: &str, name: &str) -> Option<String> {
        if fs_name_reject_reason(name).is_some() {
            return None;
        }
        let path = plugin_workspace(&self.data_dir, plugin_id).join(name);
        match tokio::fs::read(&path).await {
            Ok(bytes) => {
                // 读上限保护：超上限截断（避免把超大文件读回 JS）。
                let capped = if bytes.len() > MAX_FS_FILE_BYTES {
                    &bytes[..MAX_FS_FILE_BYTES]
                } else {
                    &bytes[..]
                };
                Some(String::from_utf8_lossy(capped).into_owned())
            }
            Err(_) => None,
        }
    }

    async fn fs_remove(&self, plugin_id: &str, name: &str) -> Result<(), PluginError> {
        if let Some(r) = fs_name_reject_reason(name) {
            return Err(PluginError::InvalidOutput(format!(
                "flux.fs 文件名 '{name}' 非法: {r}"
            )));
        }
        let path = plugin_workspace(&self.data_dir, plugin_id).join(name);
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(PluginError::Runtime(format!("flux.fs 删除失败: {e}"))),
        }
    }

    async fn fs_list(&self, plugin_id: &str) -> Vec<String> {
        let ws = plugin_workspace(&self.data_dir, plugin_id);
        let mut out = Vec::new();
        if let Ok(mut rd) = tokio::fs::read_dir(&ws).await {
            while let Ok(Some(e)) = rd.next_entry().await {
                if e.file_type().await.map(|t| t.is_file()).unwrap_or(false) {
                    out.push(e.file_name().to_string_lossy().into_owned());
                }
            }
        }
        out
    }

    fn log(&self, plugin_id: &str, level: PluginLogLevel, message: &str) {
        let truncated = if message.len() > MAX_LOG_BYTES {
            // 按字符边界安全截断。
            let mut end = MAX_LOG_BYTES;
            while end > 0 && !message.is_char_boundary(end) {
                end -= 1;
            }
            &message[..end]
        } else {
            message
        };
        match level {
            PluginLogLevel::Error => log_error!("[plugin:{}] {}", plugin_id, truncated),
            _ => log_info!("[plugin:{}] {}", plugin_id, truncated),
        }
    }

    fn request_retry(&self, task_id: &str, delay_ms: u64) {
        // fire-and-forget；限流在 actor 侧（max_auto_retries）。
        let _ = self.plugin_retry_tx.send((task_id.to_string(), delay_ms));
    }

    async fn record_artifact(
        &self,
        plugin_id: &str,
        task_id: &str,
        file_name: &str,
    ) -> Result<(), PluginError> {
        // 仅接受单层裸文件名（无路径分隔/盘符/`..`）；删除侧还有
        // `is_safe_file_name` 二次校验，双保险。
        let bad = file_name.is_empty()
            || file_name.len() > 512
            || file_name.contains(['/', '\\', ':'])
            || file_name == "."
            || file_name == "..";
        if bad {
            return Err(PluginError::Runtime(format!(
                "recordArtifact: 非法产物文件名: {file_name:?}"
            )));
        }
        self.db
            .add_task_artifact(task_id, file_name)
            .await
            .map_err(|e| PluginError::Runtime(format!("recordArtifact 落库失败: {e}")))?;
        log_info!(
            "[plugin:{}] recordArtifact: task={} file={}",
            plugin_id,
            task_id,
            file_name
        );
        Ok(())
    }

    async fn ffmpeg_available(&self) -> Option<FfmpegAvailability> {
        let status = crate::components::ffmpeg_status(&self.db, &self.data_dir).await;
        Some(FfmpegAvailability {
            available: !status.path.is_empty(),
            version: status.version,
            source: status.source.as_str().to_string(),
        })
    }

    async fn run_ffmpeg(
        &self,
        _plugin_id: &str,
        jail_root: PathBuf,
        spec: FfmpegSpec,
    ) -> Result<FfmpegOutcome, PluginError> {
        // 生效 ffmpeg（manual→managed→system；不触网）。`-nostdin` 前置。
        let bin = crate::components::resolve_ffmpeg(&self.db, &self.data_dir)
            .await
            .ok_or_else(|| PluginError::Runtime("ffmpeg 未安装或不可用".to_string()))?;
        run_jailed_tool(
            &self.ffmpeg_sema,
            "ffmpeg",
            &bin,
            jail_root,
            spec,
            &["-nostdin"],
        )
        .await
    }

    async fn run_ffprobe(
        &self,
        _plugin_id: &str,
        jail_root: PathBuf,
        spec: FfmpegSpec,
    ) -> Result<FfmpegOutcome, PluginError> {
        // 生效 ffprobe（手动 ffmpeg 同目录 / 托管 / 系统 PATH；随 ffmpeg 组件一并安装）。
        // ffprobe 不识别 `-nostdin`，故无前置（stdin 仍置 null）。与 ffmpeg 同权限门、
        // 同牢笼、同封网/封越牢校验（`validate_ffmpeg_args`）。
        let bin = crate::components::resolve_ffprobe(&self.db, &self.data_dir)
            .await
            .ok_or_else(|| {
                PluginError::Runtime("ffprobe 未安装或不可用（随 ffmpeg 组件一并安装）".to_string())
            })?;
        run_jailed_tool(&self.ffmpeg_sema, "ffprobe", &bin, jail_root, spec, &[]).await
    }

    async fn ytdlp_available(&self) -> Option<YtdlpAvailability> {
        let status = crate::components::ytdlp_status(&self.db, &self.data_dir).await;
        Some(YtdlpAvailability {
            available: !status.path.is_empty(),
            version: status.version,
            source: status.source.as_str().to_string(),
        })
    }

    async fn run_ytdlp(
        &self,
        plugin_id: &str,
        spec: YtdlpSpec,
    ) -> Result<YtdlpOutcome, PluginError> {
        // 1) 参数校验（放行 URL；封越牢路径 + 封危险开关）。先于二进制解析 fail-fast。
        if spec.args.is_empty() {
            return Err(PluginError::InvalidOutput(
                "yt-dlp args 不可为空".to_string(),
            ));
        }
        if spec.args.len() > MAX_YTDLP_ARGS {
            return Err(PluginError::InvalidOutput(format!(
                "yt-dlp 参数过多（>{MAX_YTDLP_ARGS}）"
            )));
        }
        validate_ytdlp_args(&spec.args)?;

        // 2) 生效 yt-dlp 二进制（manual→managed→system；不触网）。
        let bin = crate::components::resolve_ytdlp(&self.db, &self.data_dir)
            .await
            .ok_or_else(|| PluginError::Runtime("yt-dlp 未安装或不可用".to_string()))?;

        // yt-dlp 的合并（bestvideo+bestaudio）/抽音（-x）/remux/recode 等后处理依赖
        // ffmpeg。托管 ffmpeg 落在 <data_dir>/bin，不在 PATH，yt-dlp 默认找不到；这里
        // 解析生效 ffmpeg（manual→managed→system）并经 `--ffmpeg-location` 注入。插件
        // 自带的 `--ffmpeg-location` 仍在黑名单中被拒（防指向任意二进制），宿主注入的
        // 可信路径是唯一来源——两组件由此协同，且不放大攻击面。
        let ffmpeg = crate::components::resolve_ffmpeg(&self.db, &self.data_dir).await;

        // 3) 牢笼根：bridge 自持的每插件工作区（与 flux.fs 同根，懒创建 +
        //    canonicalize）+ 可选安全 subdir，禁逃逸。插件经 flux.fs 物化的输入
        //    文件（cookie/config…）正落在此 cwd，以相对名喂给 yt-dlp。
        let root = plugin_workspace(&self.data_dir, plugin_id);
        tokio::fs::create_dir_all(&root)
            .await
            .map_err(|e| PluginError::Runtime(format!("创建 yt-dlp 牢笼失败: {e}")))?;
        let jail = tokio::fs::canonicalize(&root)
            .await
            .map_err(|e| PluginError::Runtime(format!("yt-dlp 牢笼根无效: {e}")))?;
        let work = match spec.subdir.as_deref() {
            Some(sub) if !sub.is_empty() => {
                if !super::manifest::is_safe_relative_path(sub) {
                    return Err(PluginError::InvalidOutput(format!(
                        "yt-dlp subdir '{sub}' 非法"
                    )));
                }
                let cand = jail.join(sub);
                tokio::fs::create_dir_all(&cand)
                    .await
                    .map_err(|e| PluginError::Runtime(format!("创建 yt-dlp subdir 失败: {e}")))?;
                let real = tokio::fs::canonicalize(&cand)
                    .await
                    .map_err(|e| PluginError::Runtime(format!("yt-dlp subdir 无效: {e}")))?;
                if !real.starts_with(&jail) {
                    return Err(PluginError::InvalidOutput(
                        "yt-dlp subdir 逃逸牢笼".to_string(),
                    ));
                }
                real
            }
            _ => jail.clone(),
        };

        // 4) 超时（裁剪到上限）。
        let timeout = spec
            .timeout_ms
            .map(Duration::from_millis)
            .unwrap_or(YTDLP_DEFAULT_TIMEOUT)
            .min(YTDLP_MAX_TIMEOUT);

        // 5) 并发限流。
        let _permit = self
            .ytdlp_sema
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| PluginError::Runtime("yt-dlp semaphore closed".to_string()))?;

        // 6) 启动。`--ignore-config` 前置注入（挡 ambient 配置里的 --exec 等）；
        //    stdin=null；kill_on_drop 保超时/取消时清进程。
        let mut cmd = Command::new(&bin);
        apply_full_path_env(&mut cmd);
        crate::proc::no_console_window(&mut cmd);
        cmd.current_dir(&work).arg("--ignore-config");
        if let Some(ff) = &ffmpeg {
            cmd.arg("--ffmpeg-location").arg(ff);
        }
        // 缓存收进牢笼（yt-dlp 默认写 ~/.cache/yt-dlp 在牢笼外）。放牢笼根而非
        // subdir，使同插件多次调用共享 nsig/player JS 等缓存；插件自带的
        // `--cache-dir`（只能是牢笼内相对路径）会覆盖此默认，仍在牢笼内。
        cmd.arg("--cache-dir").arg(jail.join(".cache"));
        log_info!(
            "[ytdlp-exec] plugin={} 执行: {} --ignore-config --cache-dir <jail> {}",
            plugin_id,
            bin.display(),
            spec.args.join(" ")
        );
        cmd.args(&spec.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let child = cmd
            .spawn()
            .map_err(|e| PluginError::Runtime(format!("启动 yt-dlp 失败: {e}")))?;
        let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
            Ok(Ok(o)) => o,
            Ok(Err(e)) => return Err(PluginError::Runtime(format!("yt-dlp 执行失败: {e}"))),
            Err(_) => {
                // 超时：future 被 drop → kill_on_drop 杀子进程。
                return Ok(YtdlpOutcome {
                    code: -1,
                    stdout: String::new(),
                    stderr: String::new(),
                    timed_out: true,
                    truncated_stdout: false,
                    truncated_stderr: false,
                });
            }
        };
        let (stdout, truncated_stdout) = truncate_utf8(&output.stdout, YTDLP_STDOUT_CAP);
        let (stderr, truncated_stderr) = truncate_utf8(&output.stderr, YTDLP_STDERR_CAP);
        let code = output.status.code().unwrap_or(-1);
        if code == 0 {
            log_info!("[ytdlp-exec] 结果: code=0, stdout={} 字节", stdout.len());
        } else {
            let stderr_tail: String = {
                let n = stderr.chars().count();
                stderr.chars().skip(n.saturating_sub(600)).collect()
            };
            log_error!(
                "[ytdlp-exec] 结果: code={code}, cwd={}, stdout={} 字节, stderr={stderr_tail}",
                work.display(),
                stdout.len()
            );
        }
        Ok(YtdlpOutcome {
            code: output.status.code().unwrap_or(-1),
            stdout,
            stderr,
            timed_out: false,
            truncated_stdout,
            truncated_stderr,
        })
    }
}

/// 在牢笼内执行受管外部工具（ffmpeg / ffprobe）的共用管线：参数校验（封网 +
/// 封越牢）→ 牢笼 canonicalize + 可选 subdir 禁逃逸 → 超时裁剪 → 全局并发限流
/// （`ffmpeg_sema`）→ off-actor 子进程（`prefix` 前置注入、stdin=null、
/// kill_on_drop）→ 输出按上限截断。`tool` 仅用于错误文案。
#[allow(clippy::too_many_arguments)]
async fn run_jailed_tool(
    sema: &Arc<Semaphore>,
    tool: &str,
    bin: &Path,
    jail_root: PathBuf,
    spec: FfmpegSpec,
    prefix: &[&str],
) -> Result<FfmpegOutcome, PluginError> {
    // 1) 参数校验（封网 + 封越牢路径；近乎全量 CLI）。
    if spec.args.is_empty() {
        return Err(PluginError::InvalidOutput(format!("{tool} args 不可为空")));
    }
    if spec.args.len() > MAX_FFMPEG_ARGS {
        return Err(PluginError::InvalidOutput(format!(
            "{tool} 参数过多（>{MAX_FFMPEG_ARGS}）"
        )));
    }
    validate_ffmpeg_args(&spec.args)?;

    // 2) 工作目录：牢笼根（canonicalize）+ 可选安全 subdir，禁逃逸。
    let jail = tokio::fs::canonicalize(&jail_root)
        .await
        .map_err(|e| PluginError::Runtime(format!("{tool} 牢笼根无效: {e}")))?;
    let work = match spec.subdir.as_deref() {
        Some(sub) if !sub.is_empty() => {
            if !super::manifest::is_safe_relative_path(sub) {
                return Err(PluginError::InvalidOutput(format!(
                    "{tool} subdir '{sub}' 非法"
                )));
            }
            let cand = jail.join(sub);
            tokio::fs::create_dir_all(&cand)
                .await
                .map_err(|e| PluginError::Runtime(format!("创建 {tool} subdir 失败: {e}")))?;
            let real = tokio::fs::canonicalize(&cand)
                .await
                .map_err(|e| PluginError::Runtime(format!("{tool} subdir 无效: {e}")))?;
            if !real.starts_with(&jail) {
                return Err(PluginError::InvalidOutput(format!(
                    "{tool} subdir 逃逸牢笼"
                )));
            }
            real
        }
        _ => jail.clone(),
    };

    // 3) 超时（裁剪到上限）。
    let timeout = spec
        .timeout_ms
        .map(Duration::from_millis)
        .unwrap_or(FFMPEG_DEFAULT_TIMEOUT)
        .min(FFMPEG_MAX_TIMEOUT);

    // 4) 并发限流。
    let _permit = sema
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| PluginError::Runtime(format!("{tool} semaphore closed")))?;

    // 5) 启动。`prefix` 前置注入；stdin=null；kill_on_drop 保超时/取消时清进程。
    let mut cmd = Command::new(bin);
    apply_full_path_env(&mut cmd);
    crate::proc::no_console_window(&mut cmd);
    cmd.current_dir(&work)
        .args(prefix)
        .args(&spec.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = cmd
        .spawn()
        .map_err(|e| PluginError::Runtime(format!("启动 {tool} 失败: {e}")))?;
    let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => return Err(PluginError::Runtime(format!("{tool} 执行失败: {e}"))),
        Err(_) => {
            // 超时：future 被 drop → kill_on_drop 杀子进程。
            return Ok(FfmpegOutcome {
                code: -1,
                stdout: String::new(),
                stderr: String::new(),
                timed_out: true,
                truncated_stdout: false,
                truncated_stderr: false,
            });
        }
    };
    let (stdout, truncated_stdout) = truncate_utf8(&output.stdout, FFMPEG_STDOUT_CAP);
    let (stderr, truncated_stderr) = truncate_utf8(&output.stderr, FFMPEG_STDERR_CAP);
    Ok(FfmpegOutcome {
        code: output.status.code().unwrap_or(-1),
        stdout,
        stderr,
        timed_out: false,
        truncated_stdout,
        truncated_stderr,
    })
}

/// 校验 ffmpeg 参数：仅封堵网络协议与越牢路径引用，其余（滤镜/编码器/复用器
/// /元数据…）近乎全量放行。文件引用一律相对 cwd（牢笼根/subdir）。
fn validate_ffmpeg_args(args: &[String]) -> Result<(), PluginError> {
    for a in args {
        if a.len() > MAX_FFMPEG_ARG_LEN {
            return Err(PluginError::InvalidOutput("ffmpeg 参数过长".to_string()));
        }
        if a.contains('\0') {
            return Err(PluginError::InvalidOutput("ffmpeg 参数含 NUL".to_string()));
        }
        if let Some(reason) = arg_reject_reason(a) {
            return Err(PluginError::InvalidOutput(format!(
                "ffmpeg 参数 '{a}' 被拒: {reason}"
            )));
        }
    }
    Ok(())
}

/// 单参数拒绝原因（`None` = 放行）。判定：绝对路径 / 盘符 / `..` / URL scheme /
/// 协议前缀 / 内嵌绝对路径。除法（`30000/1001`）、流选择器（`0:a`/`-c:v`）、
/// 滤镜分隔（`scale=1280:720`）等合法语法均放行。
fn arg_reject_reason(a: &str) -> Option<&'static str> {
    // 绝对路径 / 分隔符开头。
    if a.starts_with('/') || a.starts_with('\\') {
        return Some("绝对路径");
    }
    // Windows 盘符（X: 开头，含 UNC 前缀由上面的 `\` 覆盖）。
    let b = a.as_bytes();
    if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
        return Some("盘符路径");
    }
    // `..` 路径段（含内嵌 `foo/../bar`）。
    if a.split(['/', '\\']).any(|seg| seg == "..") {
        return Some(".. 越级");
    }
    // 显式 URL（http:// / file:// / rtmp:// …）。
    if a.contains("://") {
        return Some("URL scheme");
    }
    // 无 `//` 的协议前缀（file:/concat:/crypto:/data:/pipe:/subfile: …）：首个 `:`
    // 前缀是 ≥2 位、字母起头的合法 scheme 字符集时判为协议。`-c:v`（`-` 起头）、
    // `0:a`（数字/单字符）、`scale=…:…`（含 `=`）均不满足，放行。
    if let Some(idx) = a.find(':') {
        let scheme = &a[..idx];
        if scheme.len() >= 2
            && scheme.as_bytes()[0].is_ascii_alphabetic()
            && scheme
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'+' || c == b'.' || c == b'-')
        {
            return Some("协议前缀");
        }
    }
    // 选项值内嵌的绝对路径（如 `subtitles=/etc/passwd`、`movie=C\:/x`）。
    if a.contains("=/") || a.contains("=\\") || a.contains(":/") || a.contains(":\\") {
        return Some("内嵌绝对路径");
    }
    None
}

/// UTF-8 有损转换 + 按字符边界截断到 `cap` 字节。返回 `(文本, 是否截断)`。
fn truncate_utf8(bytes: &[u8], cap: usize) -> (String, bool) {
    let s = String::from_utf8_lossy(bytes);
    if s.len() <= cap {
        return (s.into_owned(), false);
    }
    let mut end = cap;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    (s[..end].to_string(), true)
}

/// 会执行外部程序 / 加载任意配置或插件 / 读浏览器凭据的 yt-dlp 开关黑名单
/// （突破沙箱边界，一律拒绝）。
const YTDLP_BLOCKED_FLAGS: [&str; 13] = [
    "--exec",
    "--exec-before-download",
    "--downloader",
    "--external-downloader",
    "--config-location",
    "--config-locations",
    "--plugin-dirs",
    "--ffmpeg-location",
    "--batch-file",
    "-a",
    "--load-info-json",
    "--load-info",
    "--cookies-from-browser",
];

/// 校验 yt-dlp 参数：放行 URL（yt-dlp 本职），封越牢文件路径 + 封会执行外部
/// 程序 / 加载任意配置或插件的开关。
fn validate_ytdlp_args(args: &[String]) -> Result<(), PluginError> {
    for a in args {
        if a.len() > MAX_YTDLP_ARG_LEN {
            return Err(PluginError::InvalidOutput("yt-dlp 参数过长".to_string()));
        }
        if a.contains('\0') {
            return Err(PluginError::InvalidOutput("yt-dlp 参数含 NUL".to_string()));
        }
        if let Some(reason) = ytdlp_arg_reject_reason(a) {
            return Err(PluginError::InvalidOutput(format!(
                "yt-dlp 参数 '{a}' 被拒: {reason}"
            )));
        }
    }
    Ok(())
}

/// 单参数拒绝原因（`None` = 放行）。放行网络 URL；拒绝 `file:` 本地方案、危险
/// 开关、绝对路径 / 盘符 / `..` / 内嵌绝对路径（`type:/abs` 形式的 `--paths`）。
fn ytdlp_arg_reject_reason(a: &str) -> Option<&'static str> {
    // 危险开关（含 `--flag=value` 形式，取 `=` 前的 flag 部分比较）。
    let flag = a.split('=').next().unwrap_or(a);
    if YTDLP_BLOCKED_FLAGS.contains(&flag) {
        return Some("危险开关");
    }
    // file: 本地方案拒绝；其余 URL（http/https/ftp/rtmp/…）放行——yt-dlp 本职。
    if a.to_ascii_lowercase().starts_with("file:") {
        return Some("file: 本地方案");
    }
    if a.contains("://") {
        return None;
    }
    // 非 URL：按路径校验。绝对路径 / 分隔符开头。
    if a.starts_with('/') || a.starts_with('\\') {
        return Some("绝对路径");
    }
    // Windows 盘符：`X:` 结尾或 `X:/`、`X:\`（不误伤含单字符前缀的普通值如 `A:b`）。
    let b = a.as_bytes();
    if b.len() >= 2
        && b[0].is_ascii_alphabetic()
        && b[1] == b':'
        && (b.len() == 2 || b[2] == b'/' || b[2] == b'\\')
    {
        return Some("盘符路径");
    }
    // `..` 路径段（含内嵌 `foo/../bar`）。
    if a.split(['/', '\\']).any(|seg| seg == "..") {
        return Some(".. 越级");
    }
    // 选项值内嵌的绝对路径（如 `--paths home:/abs`、`temp:C\:\x`）。
    if a.contains(":/") || a.contains(":\\") {
        return Some("内嵌绝对路径");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{
        arg_reject_reason, is_globally_routable_unicast, truncate_utf8, validate_ffmpeg_args,
        validate_ytdlp_args, ytdlp_arg_reject_reason,
    };
    use std::net::IpAddr;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap_or_else(|_| panic!("bad ip {s}"))
    }

    #[test]
    fn ffmpeg_args_accept_legit_syntax() {
        // 常见合法参数：滤镜/编码器/流选择器/除法/时间戳/相对文件名，均须放行。
        for a in [
            "-i",
            "video.ts",
            "-c",
            "copy",
            "out.mp4",
            "-vf",
            "scale=1280:720",
            "-r",
            "30000/1001",
            "-map",
            "0:a",
            "-c:v",
            "libx264",
            "-b:v",
            "2M",
            "-ss",
            "00:01:02",
            "-metadata:s:a:0",
            "title=x",
            "-vf",
            "setpts=0.5*PTS",
            "-filter_complex",
            "overlay=W-w:H-h",
            "sub.srt",
            "clip.audio.m4a",
            "-y",
        ] {
            assert!(
                arg_reject_reason(a).is_none(),
                "'{a}' should be accepted, got {:?}",
                arg_reject_reason(a)
            );
        }
    }

    #[test]
    fn ffmpeg_args_reject_network_and_escape() {
        // 越牢路径 + 网络协议，须逐一拒绝。
        for a in [
            "/etc/passwd",
            "\\\\server\\share",
            "C:\\Windows\\system32",
            "../secret",
            "a/../../b",
            "http://evil.example/x",
            "https://evil/x",
            "file:/etc/passwd",
            "concat:a.ts|b.ts",
            "crypto:key",
            "subfile:,start,0,end,10,:in",
            "subtitles=/etc/passwd",
            "movie=C\\:/secret",
        ] {
            assert!(arg_reject_reason(a).is_some(), "'{a}' should be rejected");
        }
    }

    #[test]
    fn ffmpeg_validate_rejects_nul_and_reports() {
        assert!(validate_ffmpeg_args(&["ok.mp4".into()]).is_ok());
        assert!(validate_ffmpeg_args(&["bad\0name".into()]).is_err());
        assert!(validate_ffmpeg_args(&["/abs".into()]).is_err());
    }

    #[test]
    fn ytdlp_args_accept_urls_and_relative() {
        // URL（本职）、相对输出模板、格式选择器、含冒号的头部/路径类型前缀，均放行。
        for a in [
            "-J",
            "--no-warnings",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "http://example.com/a?x=1&y=2",
            "-f",
            "bestvideo+bestaudio/best",
            "-o",
            "%(title)s.%(ext)s",
            "--paths",
            "temp:sub",
            "--add-header",
            "Referer:https://site.example/",
            "--add-header",
            "A:b",
            "--download-sections",
            "*00:01:00-00:02:00",
            "--merge-output-format",
            "mp4",
        ] {
            assert!(
                ytdlp_arg_reject_reason(a).is_none(),
                "'{a}' should be accepted, got {:?}",
                ytdlp_arg_reject_reason(a)
            );
        }
    }

    #[test]
    fn ytdlp_args_reject_dangerous_flags_and_escape() {
        // 危险开关（执行外部程序/加载配置或插件/读浏览器凭据）+ 越牢路径，逐一拒绝。
        for a in [
            "--exec",
            "--exec=rm -rf x",
            "--exec-before-download",
            "-a",
            "--batch-file",
            "--downloader",
            "--external-downloader",
            "--config-location",
            "--config-locations",
            "--plugin-dirs",
            "--ffmpeg-location",
            "--load-info-json",
            "--load-info",
            "--cookies-from-browser",
            "/etc/passwd",
            "\\\\server\\share",
            "C:\\Windows\\system32",
            "../secret",
            "a/../../b",
            "file:///etc/passwd",
            "home:/abs/dir",
        ] {
            assert!(
                ytdlp_arg_reject_reason(a).is_some(),
                "'{a}' should be rejected"
            );
        }
    }

    #[test]
    fn ytdlp_validate_rejects_nul_and_reports() {
        assert!(validate_ytdlp_args(&["-J".into(), "https://x/y".into()]).is_ok());
        assert!(validate_ytdlp_args(&["bad\0name".into()]).is_err());
        assert!(validate_ytdlp_args(&["--exec".into()]).is_err());
    }

    #[test]
    fn truncate_utf8_respects_char_boundary() {
        let (s, t) = truncate_utf8("hello".as_bytes(), 100);
        assert_eq!(s, "hello");
        assert!(!t);
        // 3 字节字符边界：cap=4 落在第二个 '啊'(3 字节) 中间 → 回退到 3。
        let (s, t) = truncate_utf8("啊啊".as_bytes(), 4);
        assert_eq!(s, "啊");
        assert!(t);
    }

    #[test]
    fn v4_rejects_private_and_special() {
        for s in [
            "127.0.0.1",       // loopback
            "10.0.0.1",        // private
            "172.16.0.1",      // private
            "192.168.1.1",     // private
            "169.254.169.254", // link-local (cloud metadata)
            "100.64.0.1",      // CGNAT
            "198.18.0.1",      // benchmarking
            "240.0.0.1",       // reserved
            "0.0.0.0",         // this-network
            "255.255.255.255", // broadcast
            "224.0.0.1",       // multicast
            "192.0.2.1",       // documentation
        ] {
            assert!(
                !is_globally_routable_unicast(ip(s)),
                "{s} should be rejected"
            );
        }
    }

    #[test]
    fn v4_allows_public() {
        for s in ["8.8.8.8", "1.1.1.1", "93.184.216.34"] {
            assert!(is_globally_routable_unicast(ip(s)), "{s} should be allowed");
        }
    }

    #[test]
    fn v6_rejects_special() {
        for s in [
            "::1",                // loopback
            "fe80::1",            // link-local
            "fc00::1",            // ULA
            "fd00::1",            // ULA
            "2001:db8::1",        // documentation
            "2001:0::1",          // Teredo
            "ff02::1",            // multicast
            "::",                 // unspecified
            "::ffff:127.0.0.1",   // v4-mapped loopback
            "2002:0a00:0001::",   // 6to4 embedding 10.0.0.1
            "64:ff9b::0a00:0001", // NAT64 embedding 10.0.0.1
        ] {
            assert!(
                !is_globally_routable_unicast(ip(s)),
                "{s} should be rejected"
            );
        }
    }

    #[test]
    fn v6_allows_public() {
        for s in ["2606:4700:4700::1111", "2001:4860:4860::8888"] {
            assert!(is_globally_routable_unicast(ip(s)), "{s} should be allowed");
        }
    }
}
