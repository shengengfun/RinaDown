//! 官方站点 `/api/release` + `/api/changelog` 的版本检查（`agent.update.check`）。
//!
//! 渠道对应 SemVer 预发布后缀：稳定版 `vX.Y.Z`，frontier 为 `vX.Y.Z-rc.N`。
//! 比较遵循 SemVer 2.0 §11：`1.3.0 > 1.3.0-rc.2`，`1.4.0-rc.1 > 1.3.0`，
//! 预发布标识按点分段比较（数字段小于字母段），构建元数据（`+meta`）忽略。

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::time::Duration;

use fluxdown_protocol::{ReleaseNoteDto, UpdateCheckResultDto};
use serde::Deserialize;
use serde_json::Value;

const UPDATE_API_BASE: &str = "https://fluxdown.zerx.dev";
const RELEASE_PAGE_URL: &str = "https://fluxdown.zerx.dev/changelog";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const CHANGELOG_PER_PAGE: u32 = 50;

/// 版本检查错误。
#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("unknown update channel: {0}")]
    InvalidChannel(String),
    #[error("update API request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("update API returned status {0}")]
    Status(u16),
    #[error("update API response is invalid: {0}")]
    Decode(String),
}

#[derive(Deserialize)]
struct ReleaseInfo {
    version: String,
    /// 资产键 → `{ name, size, download_url }`；不存在的资产为 `null`。
    #[serde(default)]
    assets: BTreeMap<String, Value>,
}

#[derive(Deserialize)]
struct ChangelogResponse {
    #[serde(default)]
    releases: Vec<ChangelogRelease>,
}

#[derive(Deserialize)]
struct ChangelogRelease {
    #[serde(default)]
    version: String,
    #[serde(default)]
    published_at: String,
    #[serde(default)]
    body: String,
}

/// 版本检查服务；持有独立的 HTTP 客户端。
pub struct UpdateService {
    current_version: String,
    http: reqwest::Client,
}

impl UpdateService {
    pub fn new(current_version: &str) -> Result<Self, UpdateError> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(REQUEST_TIMEOUT)
            .user_agent(format!("fluxdown-agent/{current_version}"))
            .build()?;
        Ok(Self {
            current_version: current_version.to_owned(),
            http,
        })
    }

    /// 查询渠道最新版本并附带比当前版本新的更新说明。
    pub async fn check(&self, channel: &str) -> Result<UpdateCheckResultDto, UpdateError> {
        let channel = normalize_channel(channel)?;
        let release = self.fetch_release(channel).await?;
        let has_update = is_newer(&release.version, &self.current_version).unwrap_or(false);
        let download_url = select_download_url(&release.assets).unwrap_or_default();
        let notes = if has_update {
            self.fetch_notes(channel).await
        } else {
            Vec::new()
        };
        Ok(UpdateCheckResultDto {
            channel: channel.to_owned(),
            current_version: self.current_version.clone(),
            latest_version: release.version,
            has_update,
            download_url,
            release_page_url: RELEASE_PAGE_URL.to_owned(),
            notes,
        })
    }

    async fn fetch_release(&self, channel: &str) -> Result<ReleaseInfo, UpdateError> {
        let url = format!("{UPDATE_API_BASE}/api/release?channel={channel}");
        let response = self.http.get(&url).send().await?;
        if !response.status().is_success() {
            return Err(UpdateError::Status(response.status().as_u16()));
        }
        response
            .json::<ReleaseInfo>()
            .await
            .map_err(|error| UpdateError::Decode(error.to_string()))
    }

    /// 更新说明是附属信息：拉取失败只降级为空列表，不影响版本判定。
    async fn fetch_notes(&self, channel: &str) -> Vec<ReleaseNoteDto> {
        let url = format!(
            "{UPDATE_API_BASE}/api/changelog?per_page={CHANGELOG_PER_PAGE}&since=v{}&channel={channel}",
            self.current_version
        );
        let response = match self.http.get(&url).send().await {
            Ok(response) if response.status().is_success() => response,
            Ok(response) => {
                tracing::debug!(status = %response.status(), "changelog fetch rejected");
                return Vec::new();
            }
            Err(error) => {
                tracing::debug!(error = %error, "changelog fetch failed");
                return Vec::new();
            }
        };
        match response.json::<ChangelogResponse>().await {
            Ok(changelog) => release_notes(changelog.releases, &self.current_version),
            Err(error) => {
                tracing::debug!(error = %error, "changelog decode failed");
                Vec::new()
            }
        }
    }
}

fn normalize_channel(channel: &str) -> Result<&'static str, UpdateError> {
    match channel.trim() {
        "" | "stable" => Ok("stable"),
        "frontier" => Ok("frontier"),
        other => Err(UpdateError::InvalidChannel(other.to_owned())),
    }
}

/// `/api/changelog?since=` 是闭区间，过滤掉当前版本本身。
fn release_notes(releases: Vec<ChangelogRelease>, current_version: &str) -> Vec<ReleaseNoteDto> {
    releases
        .into_iter()
        .filter(|release| release.version != current_version)
        .map(|release| ReleaseNoteDto {
            version: release.version,
            published_at: release.published_at,
            body: release.body,
        })
        .collect()
}

/// 站点资产 URL 可能是相对路径（`/api/download/...`，用于地域路由）。
fn absolute_download_url(url: &str) -> String {
    if url.starts_with('/') {
        format!("{UPDATE_API_BASE}{url}")
    } else {
        url.to_owned()
    }
}

/// 本平台/安装形态的资产键，按优先级排列；取 `/api/release` `assets` 中第一个存在的。
#[cfg(target_os = "windows")]
fn asset_keys() -> &'static [&'static str] {
    let portable = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("portable").exists()))
        .unwrap_or(false);
    let arm64 = std::env::consts::ARCH == "aarch64";
    match (portable, arm64) {
        (true, true) => &["portable_arm64"],
        (true, false) => &["portable"],
        (false, true) => &["setup_arm64"],
        (false, false) => &["setup"],
    }
}

#[cfg(target_os = "linux")]
fn asset_keys() -> &'static [&'static str] {
    if std::env::var_os("APPIMAGE").is_some() {
        return &["linux_appimage"];
    }
    let exe = std::env::current_exe().ok();
    let exe_str = exe
        .as_deref()
        .and_then(std::path::Path::to_str)
        .unwrap_or("");
    if exe_str.starts_with("/opt/fluxdown") {
        if package_owns(&["dpkg", "-S"], exe_str) {
            return &["linux_deb"];
        }
        if package_owns(&["pacman", "-Qo"], exe_str) {
            return &["linux_arch"];
        }
    }
    &["linux_tarball"]
}

#[cfg(target_os = "linux")]
fn package_owns(command: &[&str], exe: &str) -> bool {
    std::process::Command::new(command[0])
        .args(&command[1..])
        .arg(exe)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// 无 Developer ID 签名时无法静默替换 .app，交给 Finder 打开 DMG；缺 DMG 时回退 tarball。
#[cfg(target_os = "macos")]
fn asset_keys() -> &'static [&'static str] {
    if std::env::consts::ARCH == "aarch64" {
        &["macos_dmg_arm64", "macos_tarball_arm64"]
    } else {
        &["macos_dmg_x64", "macos_tarball_x64"]
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn asset_keys() -> &'static [&'static str] {
    &[]
}

fn select_download_url(assets: &BTreeMap<String, Value>) -> Option<String> {
    asset_keys().iter().find_map(|key| {
        assets
            .get(*key)?
            .get("download_url")
            .and_then(Value::as_str)
            .filter(|url| !url.is_empty())
            .map(absolute_download_url)
    })
}

/// 预发布标识：数字段按数值比较且恒小于字母段（SemVer 2.0 §11.4）。
#[derive(Debug, PartialEq, Eq)]
enum PreId {
    Num(u64),
    Text(String),
}

#[derive(Debug, PartialEq, Eq)]
struct SemVer {
    core: (u64, u64, u64),
    pre: Vec<PreId>,
}

fn parse_semver(input: &str) -> Result<SemVer, UpdateError> {
    let input = input.trim();
    let input = input.strip_prefix('v').unwrap_or(input);
    let input = input.split('+').next().unwrap_or(input);
    let (core, pre) = match input.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (input, None),
    };
    let mut parts = core.split('.');
    let mut next_part = |name: &str| -> Result<u64, UpdateError> {
        parts
            .next()
            .ok_or_else(|| UpdateError::Decode(format!("missing {name} in version: {input}")))?
            .parse::<u64>()
            .map_err(|_| UpdateError::Decode(format!("invalid {name} in version: {input}")))
    };
    let major = next_part("major")?;
    let minor = next_part("minor")?;
    let patch = next_part("patch")?;
    if parts.next().is_some() {
        return Err(UpdateError::Decode(format!("invalid version: {input}")));
    }
    let pre = match pre {
        None => Vec::new(),
        Some("") => {
            return Err(UpdateError::Decode(format!("empty prerelease: {input}")));
        }
        Some(pre) => pre
            .split('.')
            .map(|id| match id.parse::<u64>() {
                Ok(number) => PreId::Num(number),
                Err(_) => PreId::Text(id.to_owned()),
            })
            .collect(),
    };
    Ok(SemVer {
        core: (major, minor, patch),
        pre,
    })
}

fn cmp_pre(a: &[PreId], b: &[PreId]) -> Ordering {
    for (x, y) in a.iter().zip(b.iter()) {
        let ordering = match (x, y) {
            (PreId::Num(m), PreId::Num(n)) => m.cmp(n),
            (PreId::Text(m), PreId::Text(n)) => m.cmp(n),
            (PreId::Num(_), PreId::Text(_)) => Ordering::Less,
            (PreId::Text(_), PreId::Num(_)) => Ordering::Greater,
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    a.len().cmp(&b.len())
}

fn cmp_semver(a: &SemVer, b: &SemVer) -> Ordering {
    match a.core.cmp(&b.core) {
        Ordering::Equal => {}
        ordering => return ordering,
    }
    match (a.pre.is_empty(), b.pre.is_empty()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => cmp_pre(&a.pre, &b.pre),
    }
}

/// `latest` 是否严格高于 `current`；任一方不可解析（如开发版 `dev`）返回错误。
pub fn is_newer(latest: &str, current: &str) -> Result<bool, UpdateError> {
    let latest = parse_semver(latest)?;
    let current = parse_semver(current)?;
    Ok(cmp_semver(&latest, &current) == Ordering::Greater)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::{Value, json};

    use super::{
        ChangelogRelease, absolute_download_url, asset_keys, is_newer, normalize_channel,
        release_notes, select_download_url,
    };

    #[test]
    fn download_url_follows_platform_asset_preference() {
        let keys = asset_keys();
        assert!(
            !keys.is_empty(),
            "desktop platforms always have an asset key"
        );
        let mut assets: BTreeMap<String, Value> = keys
            .iter()
            .map(|key| ((*key).to_owned(), Value::Null))
            .collect();
        assert_eq!(select_download_url(&assets), None);
        let last = keys[keys.len() - 1];
        assets.insert(
            last.to_owned(),
            json!({ "name": "x", "size": 1, "download_url": "/api/download/x" }),
        );
        assert_eq!(
            select_download_url(&assets).as_deref(),
            Some("https://fluxdown.zerx.dev/api/download/x")
        );
        assets.insert(
            keys[0].to_owned(),
            json!({ "download_url": "https://cdn.example/first" }),
        );
        assert_eq!(
            select_download_url(&assets).as_deref(),
            Some("https://cdn.example/first")
        );
    }

    #[test]
    fn stable_versions_compare_numerically() {
        assert!(is_newer("1.3.0", "1.2.5").unwrap());
        assert!(is_newer("1.2.6", "1.2.5").unwrap());
        assert!(is_newer("1.10.0", "1.9.9").unwrap());
        assert!(!is_newer("1.2.5", "1.2.5").unwrap());
        assert!(!is_newer("1.2.4", "1.2.5").unwrap());
        assert!(is_newer("v2.0.0", "1.99.99").unwrap());
    }

    #[test]
    fn prerelease_precedence_follows_semver() {
        assert!(is_newer("1.3.0", "1.3.0-rc.1").unwrap());
        assert!(!is_newer("1.3.0-rc.2", "1.3.0").unwrap());
        assert!(is_newer("1.3.0-rc.2", "1.3.0-rc.1").unwrap());
        assert!(!is_newer("1.3.0-rc.1", "1.3.0-rc.2").unwrap());
        assert!(is_newer("1.4.0-rc.1", "1.3.0").unwrap());
        assert!(is_newer("1.3.0-rc.1", "1.3.0-alpha").unwrap());
        assert!(is_newer("1.3.0-rc.1.1", "1.3.0-rc.1").unwrap());
        assert!(!is_newer("1.3.0-rc.1", "1.3.0-rc.1").unwrap());
        assert!(is_newer("1.3.0+build.7", "1.2.0+build.9").unwrap());
        assert!(!is_newer("1.3.0+build.7", "1.3.0").unwrap());
    }

    #[test]
    fn invalid_versions_are_rejected() {
        assert!(is_newer("dev", "1.0.0").is_err());
        assert!(is_newer("1.0.0", "dev").is_err());
        assert!(is_newer("1.0", "1.0.0").is_err());
        assert!(is_newer("1.0.0.1", "1.0.0").is_err());
        assert!(is_newer("1.0.0-", "1.0.0").is_err());
    }

    #[test]
    fn channels_normalize_and_reject_unknown() {
        assert_eq!(normalize_channel("").unwrap(), "stable");
        assert_eq!(normalize_channel("stable").unwrap(), "stable");
        assert_eq!(normalize_channel(" frontier ").unwrap(), "frontier");
        assert!(normalize_channel("nightly").is_err());
    }

    #[test]
    fn notes_exclude_current_version_and_resolve_relative_urls() {
        let releases = vec![
            ChangelogRelease {
                version: "1.3.0".to_owned(),
                published_at: "2026-01-01T00:00:00Z".to_owned(),
                body: "new".to_owned(),
            },
            ChangelogRelease {
                version: "1.2.0".to_owned(),
                published_at: String::new(),
                body: "current".to_owned(),
            },
        ];
        let notes = release_notes(releases, "1.2.0");
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].version, "1.3.0");
        assert_eq!(notes[0].body, "new");
        assert_eq!(
            absolute_download_url("/api/download/FluxDown.dmg"),
            "https://fluxdown.zerx.dev/api/download/FluxDown.dmg"
        );
        assert_eq!(
            absolute_download_url("https://cdn.example/a.dmg"),
            "https://cdn.example/a.dmg"
        );
    }
}
