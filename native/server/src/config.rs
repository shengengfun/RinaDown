//! 服务器启动配置（环境变量）与首次运行初始化（访问密钥）。
//!
//! | 环境变量 | 含义 | 默认 |
//! |---|---|---|
//! | `FLUXDOWN_DATA_DIR` | 数据目录（DB/日志） | 平台自动探测 |
//! | `FLUXDOWN_DATABASE_URL` | 数据库连接 URL（`sqlite:`/`postgres:`） | 数据目录下 SQLite |
//! | `FLUXDOWN_BIND` | HTTP 监听地址 | `0.0.0.0:17800` |
//! | `FLUXDOWN_WEBROOT` | 覆盖内嵌 Web UI，改从该磁盘目录托管 SPA | 未设置（用二进制内嵌的前端） |
//! | `FLUXDOWN_TOKEN` | 预置管理访问密钥（仅在库中尚未设置时采纳） | 未设置（走 Web 向导） |
//! | `FLUXDOWN_DEMO` | 演示模式：仅允许下载内置本地演示文件 | 未设置（关闭） |
//! | `FLUXDOWN_DEMO_URL` | 演示模式：仅允许下载该 URL（覆盖内置） | 未设置（关闭） |
//! | `FLUXDOWN_LANG` | Web UI 默认语言（`en`/`zh`），设置页保存过语言后以保存值为准 | 未设置（回退浏览器语言） |
//!
//! **访问密钥不再自动生成**：NAS（群晖/QNAP/Unraid）用户看不到容器 stderr，
//! 一次性打印的密钥等于把人锁在门外。库中无密钥时服务器进入「待设置」状态
//! （管理 API 全线 403），由 Web 首次运行向导 `POST /api/v1/setup` 落定；
//! 无人值守部署用 `FLUXDOWN_TOKEN` 预置。

use std::path::PathBuf;

use fluxdown_engine::db::Db;
use fluxdown_engine::log_info;

/// 服务器进程级配置（全部来自环境变量）。
pub struct ServerConfig {
    pub bind: String,
    pub data_dir_override: Option<PathBuf>,
    pub database_url: Option<String>,
    /// SPA 托管目录覆盖。`None` = 用二进制内嵌的 Web UI（常态）；`Some` 仅在
    /// 显式设置 `FLUXDOWN_WEBROOT` 时出现，用于自定义/调试前端。
    /// **不做「二进制同级 ./web」的隐式探测**——旧版本残留的 web/ 目录会让
    /// 升级后的服务器配上过期 SPA，静默出现前后端契约不匹配。
    pub webroot: Option<PathBuf>,
    /// 演示模式：`Some(url)` 时新任务仅允许下载该 URL（见 `host::demo_guard`）。
    pub demo_url: Option<String>,
    /// Web UI 默认语言（`en`/`zh`）。纯回退值，不写库：`/ping` 的 `language`
    /// 实时求值时，设置页保存的 `web_language` 优先、缺省才用本值——
    /// 用户手动更改永远优先且跨重启保留；浏览器端显式选过语言的用户
    /// 则始终以本人选择为准。
    pub language: Option<String>,
}

impl ServerConfig {
    pub fn from_env() -> Self {
        let bind = std::env::var("FLUXDOWN_BIND").unwrap_or_else(|_| "0.0.0.0:17800".to_string());
        let data_dir_override = std::env::var_os("FLUXDOWN_DATA_DIR").map(PathBuf::from);
        let database_url = std::env::var("FLUXDOWN_DATABASE_URL")
            .ok()
            .filter(|s| !s.trim().is_empty());
        let webroot = std::env::var_os("FLUXDOWN_WEBROOT")
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty());
        let demo_url = std::env::var("FLUXDOWN_DEMO_URL")
            .ok()
            .as_deref()
            .and_then(parse_demo_url)
            .or_else(|| demo_flag_enabled().then(|| builtin_demo_url(&bind)));
        let language = match std::env::var("FLUXDOWN_LANG") {
            Ok(raw) => {
                let lang = parse_lang(&raw);
                if lang.is_none() && !raw.trim().is_empty() {
                    eprintln!("FLUXDOWN_LANG 无法识别（支持 en / zh），已忽略：{raw}");
                }
                lang
            }
            Err(_) => None,
        };
        Self {
            bind,
            data_dir_override,
            database_url,
            webroot,
            demo_url,
            language,
        }
    }
}

/// 归一化 `FLUXDOWN_LANG`：剥首尾空白与包裹引号后取 BCP 47 主语言子标签
/// （`zh-CN`/`zh_TW` → `zh`，`en-US` → `en`，忽略大小写），映射到 Web UI
/// 支持的语言；无法识别视为未设置。
fn parse_lang(raw: &str) -> Option<String> {
    let mut s = raw.trim();
    for quote in ['"', '\''] {
        if s.len() >= 2 && s.starts_with(quote) && s.ends_with(quote) {
            s = s[1..s.len() - 1].trim();
        }
    }
    let primary = s
        .split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(primary.as_str(), "en" | "zh").then_some(primary)
}

/// 归一化 `FLUXDOWN_DEMO_URL`：去掉首尾空白与误带的包裹引号
/// （Windows cmd 的 `set X="v" && …` 会把引号和尾部空格一并写进值），
/// 归一化后为空视为未开启。
fn parse_demo_url(raw: &str) -> Option<String> {
    let mut s = raw.trim();
    for quote in ['"', '\''] {
        if s.len() >= 2 && s.starts_with(quote) && s.ends_with(quote) {
            s = s[1..s.len() - 1].trim();
        }
    }
    (!s.is_empty()).then(|| s.to_string())
}

/// `FLUXDOWN_DEMO` 是否为真值（`1`/`true`/`yes`/`on`，忽略大小写）。
fn demo_flag_enabled() -> bool {
    std::env::var("FLUXDOWN_DEMO")
        .map(|v| flag_truthy(&v))
        .unwrap_or(false)
}

fn flag_truthy(v: &str) -> bool {
    matches!(
        v.trim()
            .trim_matches(['"', '\''])
            .to_ascii_lowercase()
            .as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// 内置演示 URL：指向本进程自己挂载的 [`crate::demo::DEMO_FILE_PATH`]
/// （下载器与服务器同机，走 127.0.0.1 回环，不出外网）。
fn builtin_demo_url(bind: &str) -> String {
    let port = bind.rsplit(':').next().unwrap_or("17800");
    format!("http://127.0.0.1:{port}{}", crate::demo::DEMO_FILE_PATH)
}

/// 平台默认下载目录（与 App 侧 `download_actor::default_save_dir` 同源：
/// 走系统 API 解析，不做 `$HOME/Downloads` 拼接）。
pub fn default_save_dir() -> String {
    fluxdown_engine::user_dirs::download_dir_or_cwd()
}

/// 访问密钥最短长度。
pub const ACCESS_KEY_MIN_LEN: usize = 8;
/// 访问密钥最长长度：既防误粘整段文本，也保证能原样塞进 HTTP 头。
pub const ACCESS_KEY_MAX_LEN: usize = 128;

/// 校验用户设定的访问密钥（管理 token）。
///
/// 规则与 Web 端 `web/src/lib/token-policy.ts` **逐条对齐**——两侧不一致会让
/// 前端放行、后端拒收，首次运行向导直接卡死。返回的 `Err` 是稳定英文 wire
/// 契约，Web 端经 `translateBackendMessage` 本地化。
///
/// 单测见本文件 `access_key_tests`（bin crate 不跑 doctest，故不写 Examples）。
pub fn validate_access_key(key: &str) -> Result<(), &'static str> {
    if !key.chars().all(|c| c.is_ascii_graphic()) {
        return Err("access key must not contain spaces or non-ASCII characters");
    }
    if key.len() < ACCESS_KEY_MIN_LEN {
        return Err("access key must be at least 8 characters");
    }
    if key.len() > ACCESS_KEY_MAX_LEN {
        return Err("access key must be at most 128 characters");
    }
    if !key.bytes().any(|b| b.is_ascii_alphabetic()) || !key.bytes().any(|b| b.is_ascii_digit()) {
        return Err("access key must contain both letters and digits");
    }
    Ok(())
}

/// 生成一个满足 [`validate_access_key`] 的随机访问密钥（`fxd_` + 32 位十六进制）。
///
/// 循环重试的唯一目的是兜住「32 位十六进制恰好全是数字」这种小概率取值——
/// 概率约 1e-7，但一旦命中就会生成一个自己都校验不过的密钥。
#[must_use]
pub fn generate_access_key() -> String {
    loop {
        let key = format!("fxd_{}", uuid::Uuid::new_v4().simple());
        if validate_access_key(&key).is_ok() {
            return key;
        }
    }
}

/// 首次运行初始化：强制开启管理 API；返回生效的管理 token（**可能为空**）。
///
/// 空 token = 尚未完成首次设置。此时管理 API 全线 403（见
/// [`fluxdown_api::auth::check_management_auth`]），Web 界面会进入
/// 「设置访问密钥」向导（`POST /api/v1/setup`）。
///
/// 不再自动生成 token 并打印到 stderr：NAS（群晖/QNAP/Unraid）用户拿不到
/// 容器/套件的 stderr，一次性打印的密钥等于永久锁在门外。无人值守部署
/// （docker-compose / k8s / CI）可用 `FLUXDOWN_TOKEN` 预置密钥跳过向导。
pub async fn ensure_server_config(db: &Db) -> Result<String, fluxdown_engine::db::DbError> {
    // headless 服务器的存在意义就是远程管理——管理 API 恒开。
    db.set_config("local_server_api_enabled", "true").await?;

    // MCP 默认开（headless 场景面向自动化/AI 客户端），但尊重用户后续关闭：仅在缺省时播种。
    if db.get_config("local_server_mcp_enabled").await?.is_none() {
        db.set_config("local_server_mcp_enabled", "true").await?;
    }

    let token = db
        .get_config("local_server_token")
        .await?
        .unwrap_or_default();
    if !token.is_empty() {
        return Ok(token);
    }

    let Some(preset) = std::env::var("FLUXDOWN_TOKEN")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
    else {
        return Ok(String::new());
    };
    if let Err(why) = validate_access_key(&preset) {
        log_info!("[server] FLUXDOWN_TOKEN rejected: {}", why);
        eprintln!("FLUXDOWN_TOKEN 不符合密钥要求（{why}），已忽略；请在 Web 界面完成首次设置。");
        return Ok(String::new());
    }
    db.set_config("local_server_token", &preset).await?;
    log_info!("[server] management token adopted from FLUXDOWN_TOKEN");
    Ok(preset)
}

/// 首次运行横幅：引导用户去 Web 界面设定访问密钥。
///
/// `bind` 形如 `0.0.0.0:17800`；通配地址对用户无意义，替换成 `<服务器 IP>`。
pub fn print_setup_banner(bind: &str) {
    let port = bind.rsplit(':').next().unwrap_or("17800");
    eprintln!("==============================================================");
    eprintln!("  FluxDown Server: first run — no access key is set yet.");
    eprintln!("  Open the Web UI and create one:");
    eprintln!("    http://<server-ip>:{port}/");
    eprintln!("  Requirements: 8+ characters, letters and digits.");
    eprintln!("  Unattended deploys can preset it via FLUXDOWN_TOKEN.");
    eprintln!("  ---");
    eprintln!("  首次运行：尚未设置访问密钥。请打开上面的 Web 界面自行设置");
    eprintln!("  （至少 8 位，必须同时包含字母和数字）。");
    eprintln!("==============================================================");
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::parse_demo_url;

    #[test]
    fn parse_demo_url_strips_whitespace_and_wrapping_quotes() {
        let want = Some("https://example.com/demo.bin".to_string());
        assert_eq!(parse_demo_url("https://example.com/demo.bin"), want);
        // cmd.exe 的 `set X="v" && …`：引号 + 尾部空格一并进值。
        assert_eq!(parse_demo_url("\"https://example.com/demo.bin\" "), want);
        assert_eq!(parse_demo_url("'https://example.com/demo.bin'"), want);
    }

    #[test]
    fn parse_demo_url_empty_or_quotes_only_means_disabled() {
        assert_eq!(parse_demo_url(""), None);
        assert_eq!(parse_demo_url("   "), None);
        assert_eq!(parse_demo_url("\"\""), None);
    }

    #[test]
    fn parse_demo_url_keeps_interior_quotes_intact() {
        // 只剥一层「包裹」引号，不动 URL 内部字符。
        assert_eq!(
            parse_demo_url("\"https://e.com/a?q='x'\""),
            Some("https://e.com/a?q='x'".to_string())
        );
    }
}

#[cfg(test)]
mod lang_tests {
    use super::parse_lang;

    #[test]
    fn parse_lang_normalizes_region_case_and_quotes() {
        for v in ["zh", "zh-CN", "zh_TW", "ZH", " \"zh\" ", "'zh-Hans'"] {
            assert_eq!(parse_lang(v).as_deref(), Some("zh"), "{v:?}");
        }
        for v in ["en", "en-US", "EN_gb"] {
            assert_eq!(parse_lang(v).as_deref(), Some("en"), "{v:?}");
        }
    }

    #[test]
    fn parse_lang_rejects_unsupported_or_empty() {
        for v in ["fr", "ja-JP", "", "   ", "\"\"", "-CN"] {
            assert_eq!(parse_lang(v), None, "{v:?}");
        }
    }
}

#[cfg(test)]
mod builtin_tests {
    use super::{builtin_demo_url, flag_truthy};

    #[test]
    fn builtin_demo_url_uses_bind_port_over_loopback() {
        assert_eq!(
            builtin_demo_url("0.0.0.0:17800"),
            "http://127.0.0.1:17800/demo/file"
        );
        assert_eq!(
            builtin_demo_url("[::]:9000"),
            "http://127.0.0.1:9000/demo/file"
        );
    }

    #[test]
    fn flag_truthy_accepts_common_forms_and_quotes() {
        for v in ["1", "true", "YES", "On", "\"1\"", " true "] {
            assert!(flag_truthy(v), "{v:?} should be truthy");
        }
        for v in ["0", "false", "off", "", "  "] {
            assert!(!flag_truthy(v), "{v:?} should be falsy");
        }
    }
}

#[cfg(test)]
mod access_key_tests {
    use super::{ACCESS_KEY_MAX_LEN, generate_access_key, validate_access_key};

    #[test]
    fn accepts_mixed_alphanumeric_of_min_length() {
        for v in ["flux2026", "fxd_1a2b3c4d", "Aa1!@#$%^&*()"] {
            assert!(validate_access_key(v).is_ok(), "{v:?} should pass");
        }
    }

    #[test]
    fn rejects_too_short_or_too_long() {
        assert_eq!(
            validate_access_key("abc123"),
            Err("access key must be at least 8 characters")
        );
        assert_eq!(
            validate_access_key(""),
            Err("access key must be at least 8 characters")
        );
        let long = format!("a1{}", "x".repeat(ACCESS_KEY_MAX_LEN));
        assert_eq!(
            validate_access_key(&long),
            Err("access key must be at most 128 characters")
        );
    }

    #[test]
    fn rejects_letters_only_or_digits_only() {
        let want = Err("access key must contain both letters and digits");
        assert_eq!(validate_access_key("allletters"), want);
        assert_eq!(validate_access_key("1234567890"), want);
        // 纯符号同样缺字母和数字。
        assert_eq!(validate_access_key("!@#$%^&*"), want);
    }

    #[test]
    fn rejects_whitespace_and_non_ascii() {
        let want = Err("access key must not contain spaces or non-ASCII characters");
        // 空白会在 HTTP 头/命令行里被静默吞掉，落库前就挡住。
        assert_eq!(validate_access_key("flux 2026"), want);
        assert_eq!(validate_access_key(" flux2026"), want);
        assert_eq!(validate_access_key("flux2026\n"), want);
        assert_eq!(validate_access_key("密钥12345678"), want);
    }

    #[test]
    fn generated_key_satisfies_policy() {
        let key = generate_access_key();
        assert!(key.starts_with("fxd_"));
        assert_eq!(validate_access_key(&key), Ok(()));
    }
}
