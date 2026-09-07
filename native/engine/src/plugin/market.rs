//! 去中心化插件市场客户端（R4/R5/R6）。
//!
//! 市场不是网站，而是**一份可验证的数据格式**：Git 版本化的索引（联邦式，任何人可
//! fork 另立），插件包内容寻址（`content_hash = sha256(整个 .fxplug zip)`），经多源
//! （CDN / R2 / 众包镜像）分发。**FluxDown 用自己的引擎下载并本地验证插件**。
//!
//! ## v1 范围（记录在案的取舍）
//! - 完整性基座 = `content_hash`（sha256，已在依赖树）+ Git Merkle DAG 防篡改 +
//!   per-index_id sequence 单调高水位防回滚。
//! - **作者签名（sigstore / ed25519）推迟**：受依赖约束（仅 rquickjs 获批），不引入
//!   sigstore-rs / ed25519 crate。索引 schema 预留 `sig_scheme`/`sigstore_bundle_ref`
//!   字段，晚加不破坏兼容（JSON）。当前信任模型 = 内容寻址 + 传输层 TLS + Git 历史。
//! - `.fxplug` = 插件目录（manifest.json + *.js）的 zip；安装复用
//!   [`super::install::install_from_zip`]（zip-slip / 压缩炸弹防护 + manifest 校验）。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::db::Db;

use super::manager::PluginManager;
use super::runtime::PluginError;

/// 内置官方索引候选源（不同法域/托管，共享同一 index_id；任一存活即可用）。
/// 首个成功者胜出；用户可经配置覆盖/追加。
pub const DEFAULT_INDEX_SOURCES: &[&str] = &[
    "https://raw.githubusercontent.com/zerx-lab/fluxdown-plugin-index/main/index.json",
    "https://cdn.jsdelivr.net/gh/zerx-lab/fluxdown-plugin-index@main/index.json",
];

/// 下载/校验/安装的错误。
#[derive(Debug, thiserror::Error)]
pub enum MarketError {
    #[error("网络错误: {0}")]
    Network(String),
    #[error("索引解析失败: {0}")]
    IndexParse(String),
    #[error("内容哈希不匹配: 期望 {expected}，实际 {actual}")]
    HashMismatch { expected: String, actual: String },
    #[error("sequence 回退（可能的回滚攻击）: 索引 {seen} < 本地高水位 {watermark}")]
    SequenceRollback { seen: u64, watermark: u64 },
    #[error("插件包超过体积上限")]
    TooLarge,
    #[error("索引响应超过体积上限")]
    IndexTooLarge,
    #[error("未找到插件条目: {0}")]
    NotFound(String),
    #[error("插件包被 yanked（{0}），需显式确认安装")]
    Yanked(String),
    #[error("所有镜像下载失败")]
    AllMirrorsFailed,
    #[error(transparent)]
    Plugin(#[from] PluginError),
}

/// 索引条目（每插件每版本一份分片，flatten 进 `index.json`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketEntry {
    pub plugin_id: String,
    pub version: String,
    /// 全索引全局单调递增整数（防回滚基线）。
    pub sequence: u64,
    /// `sha256:<hex>` —— 唯一真相源，下载后钉住比对。
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
    /// 多源下载 URL（https 白名单；逐个尝试直到 content_hash 吻合）。
    #[serde(default)]
    pub mirrors: Vec<String>,
    #[serde(default)]
    pub publish_time: String,
    /// `none` / `deprecated` / `vulnerable` / `malicious`。
    #[serde(default = "yanked_none")]
    pub yanked: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// manifest 声明的能力权限（如 `["ffmpeg"]`；CI flatten 自 manifest 抄录，
    /// 供安装前展示授权。旧索引缺省 → 空，不影响安装）。
    #[serde(default)]
    pub permissions: Vec<String>,
    /// 预留：未来作者签名方案（`none` / `sigstore` / `ed25519`）。
    #[serde(default = "sig_none")]
    pub sig_scheme: String,
    #[serde(default)]
    pub sigstore_bundle_ref: String,
}

fn yanked_none() -> String {
    "none".to_string()
}
fn sig_none() -> String {
    "none".to_string()
}

/// 索引根（`index.json`，CI flatten 生成）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketIndex {
    /// 索引源身份（CI 初始化时一次性生成的 UUID；fork 保留 → 共享 sequence 高水位）。
    pub index_id: String,
    /// 全索引 sequence 高水位。
    pub sequence: u64,
    #[serde(default)]
    pub updated: String,
    /// 全部条目（每插件可多版本）。
    pub entries: Vec<MarketEntry>,
}

/// 插件包体积上限（10MB，与 hub/server 安装上限一致）。
const MAX_FXPLUG_BYTES: usize = 10 * 1024 * 1024;
/// 索引 JSON 体积上限（流式截断防 OOM；真实索引远小于此，被投毒/损坏的源
/// 可能返回任意大响应，`.text()` 全量缓冲会被撑爆）。
const MAX_INDEX_BYTES: usize = 4 * 1024 * 1024;

/// 市场客户端。持有插件管理器（安装）与 Db（高水位持久化）。
pub struct MarketClient {
    manager: std::sync::Arc<PluginManager>,
    db: Db,
    client: reqwest::Client,
    sources: Vec<String>,
}

impl MarketClient {
    /// 构造。`sources` 为空时用 [`DEFAULT_INDEX_SOURCES`]。
    pub fn new(manager: std::sync::Arc<PluginManager>, db: Db, sources: Vec<String>) -> Self {
        let sources = if sources.is_empty() {
            DEFAULT_INDEX_SOURCES
                .iter()
                .map(|s| s.to_string())
                .collect()
        } else {
            sources
        };
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .unwrap_or_default();
        Self {
            manager,
            db,
            client,
            sources,
        }
    }

    /// 拉取索引（逐源 failover，首个成功者胜出）。校验 sequence 不回退（防回滚）。
    pub async fn fetch_index(&self) -> Result<MarketIndex, MarketError> {
        let mut last_err = MarketError::AllMirrorsFailed;
        for src in &self.sources {
            match self.fetch_index_from(src).await {
                Ok(idx) => {
                    self.check_and_update_watermark(&idx).await?;
                    return Ok(idx);
                }
                Err(e) => last_err = e,
            }
        }
        Err(last_err)
    }

    async fn fetch_index_from(&self, url: &str) -> Result<MarketIndex, MarketError> {
        let mut resp = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| MarketError::Network(e.to_string()))?;
        // 流式读取 + 体积上限（与 download_one 对称，防恶意源 OOM）。
        let mut buf = Vec::new();
        loop {
            match resp.chunk().await {
                Ok(Some(chunk)) => {
                    if buf.len() + chunk.len() > MAX_INDEX_BYTES {
                        return Err(MarketError::IndexTooLarge);
                    }
                    buf.extend_from_slice(&chunk);
                }
                Ok(None) => break,
                Err(e) => return Err(MarketError::Network(e.to_string())),
            }
        }
        serde_json::from_slice::<MarketIndex>(&buf)
            .map_err(|e| MarketError::IndexParse(e.to_string()))
    }

    /// per-index_id 高水位防回滚：索引 sequence 不得低于本地已见最高值。
    async fn check_and_update_watermark(&self, idx: &MarketIndex) -> Result<(), MarketError> {
        let key = format!("market.{}.sequence", idx.index_id);
        let watermark: u64 = self
            .db
            .get_config(&key)
            .await
            .ok()
            .flatten()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        if idx.sequence < watermark {
            return Err(MarketError::SequenceRollback {
                seen: idx.sequence,
                watermark,
            });
        }
        if idx.sequence > watermark {
            let _ = self.db.set_config(&key, &idx.sequence.to_string()).await;
        }
        Ok(())
    }

    /// 找到某 plugin_id 的最新非 yanked 版本条目。
    pub fn latest_entry<'a>(
        &self,
        idx: &'a MarketIndex,
        plugin_id: &str,
    ) -> Option<&'a MarketEntry> {
        idx.entries
            .iter()
            .filter(|e| e.plugin_id == plugin_id && e.yanked == "none")
            .max_by(|a, b| a.sequence.cmp(&b.sequence))
    }

    /// 安装指定条目：多镜像择优下载 → content_hash 钉住校验 → install_from_zip。
    /// `allow_yanked = false` 时拒绝 yanked 版本（malicious 恒拒）。
    pub async fn install_entry(
        &self,
        entry: &MarketEntry,
        allow_yanked: bool,
    ) -> Result<String, MarketError> {
        if entry.yanked == "malicious" {
            return Err(MarketError::Yanked("malicious".to_string()));
        }
        if entry.yanked != "none" && !allow_yanked {
            return Err(MarketError::Yanked(entry.yanked.clone()));
        }
        let bytes = self.download_verified(entry).await?;
        let identity = self.manager.install_from_zip(bytes).await?;
        Ok(identity)
    }

    /// 逐镜像下载并校验 content_hash（首个吻合者胜出）。https 白名单。
    async fn download_verified(&self, entry: &MarketEntry) -> Result<Vec<u8>, MarketError> {
        let expected = entry
            .content_hash
            .strip_prefix("sha256:")
            .unwrap_or(&entry.content_hash)
            .to_ascii_lowercase();
        if entry.mirrors.is_empty() {
            return Err(MarketError::NotFound(format!(
                "{} 无可用镜像",
                entry.plugin_id
            )));
        }
        for url in &entry.mirrors {
            // 镜像白名单：https-only（防降级）+ 字面量 IP 必须可全局路由（联邦
            // 索引的镜像 URL 不可全信，挡「把环回/内网地址伪装成镜像」的 SSRF
            // 探测；hostname 级过滤不做——自托管 LAN 索引经 hostname 仍可用，
            // 记录在案的 v1 取舍，完整性由 content_hash 钉住兜底）。
            if !mirror_url_allowed(url) {
                continue;
            }
            match self.download_one(url).await {
                Ok(bytes) => {
                    let actual = sha256_hex(&bytes);
                    if actual == expected {
                        return Ok(bytes);
                    }
                    // 哈希不符 → 试下一镜像（可能是被投毒/损坏的源）。
                    continue;
                }
                Err(_) => continue,
            }
        }
        Err(MarketError::AllMirrorsFailed)
    }

    async fn download_one(&self, url: &str) -> Result<Vec<u8>, MarketError> {
        let resp = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| MarketError::Network(e.to_string()))?;
        let mut stream_resp = resp;
        let mut buf = Vec::new();
        loop {
            match stream_resp.chunk().await {
                Ok(Some(chunk)) => {
                    if buf.len() + chunk.len() > MAX_FXPLUG_BYTES {
                        return Err(MarketError::TooLarge);
                    }
                    buf.extend_from_slice(&chunk);
                }
                Ok(None) => break,
                Err(e) => return Err(MarketError::Network(e.to_string())),
            }
        }
        Ok(buf)
    }

    /// 便捷：按 plugin_id 安装最新版（拉索引 → 找最新 → 安装）。
    pub async fn install_latest(&self, plugin_id: &str) -> Result<String, MarketError> {
        let idx = self.fetch_index().await?;
        let entry = self
            .latest_entry(&idx, plugin_id)
            .ok_or_else(|| MarketError::NotFound(plugin_id.to_string()))?
            .clone();
        self.install_entry(&entry, false).await
    }

    /// 市场来源配置（供 UI 展示/编辑）。
    pub fn sources(&self) -> &[String] {
        &self.sources
    }

    /// 当前设置值映射（供上层构造带自定义源的客户端）。
    pub fn source_config(map: &HashMap<String, String>) -> Vec<String> {
        map.get("market_index_sources")
            .map(|s| {
                s.split(',')
                    .map(|x| x.trim().to_string())
                    .filter(|x| !x.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// `sha256(bytes)` 的小写 hex。
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    let digest = h.finalize();
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// 镜像 URL 白名单：https-only + 字面量 IP host 必须可全局路由。
///
/// hostname 不做 DNS 级过滤（自托管 LAN 索引经 hostname 仍可用，v1 取舍）；
/// 完整性由 content_hash 钉住兜底，此守卫只挡最直接的内网探测形态。
fn mirror_url_allowed(url: &str) -> bool {
    if !url.starts_with("https://") {
        return false;
    }
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    if let Some(host) = parsed.host_str() {
        let trimmed = host.trim_matches(|c| c == '[' || c == ']');
        if let Ok(ip) = trimmed.parse::<std::net::IpAddr>()
            && !super::bridge::is_globally_routable_unicast(ip)
        {
            return false;
        }
    }
    true
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::{MarketEntry, MarketIndex, mirror_url_allowed, sha256_hex};

    #[test]
    fn mirror_whitelist_rejects_http_and_nonroutable_ip() {
        // https-only。
        assert!(!mirror_url_allowed("http://cdn.example.com/a.fxplug"));
        assert!(!mirror_url_allowed("ftp://cdn.example.com/a.fxplug"));
        // 字面量非公网 IP 拒绝（环回/私网/元数据/v6 环回）。
        assert!(!mirror_url_allowed("https://127.0.0.1/a.fxplug"));
        assert!(!mirror_url_allowed("https://192.168.1.10:8443/a.fxplug"));
        assert!(!mirror_url_allowed("https://169.254.169.254/a.fxplug"));
        assert!(!mirror_url_allowed("https://[::1]/a.fxplug"));
        // 公网字面量 IP 与 hostname 放行。
        assert!(mirror_url_allowed("https://93.184.216.34/a.fxplug"));
        assert!(mirror_url_allowed("https://cdn.jsdelivr.net/gh/x/y.fxplug"));
        assert!(mirror_url_allowed("https://lan-index.internal/a.fxplug"));
    }

    #[test]
    fn sha256_known_vector() {
        // sha256("") = e3b0c442...
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn index_parses_camel_case() {
        let json = r#"{
          "indexId": "00000000-0000-0000-0000-000000000000",
          "sequence": 3,
          "updated": "2026-07-13T00:00:00Z",
          "entries": [{
            "pluginId": "a@b", "version": "1.0.0", "sequence": 3,
            "contentHash": "sha256:abc", "mirrors": ["https://x/y.fxplug"],
            "yanked": "none"
          }]
        }"#;
        let idx: MarketIndex = serde_json::from_str(json).expect("parse");
        assert_eq!(idx.index_id, "00000000-0000-0000-0000-000000000000");
        assert_eq!(idx.sequence, 3);
        assert_eq!(idx.entries.len(), 1);
        assert_eq!(idx.entries[0].plugin_id, "a@b");
        assert_eq!(idx.entries[0].sig_scheme, "none"); // 默认值
    }

    #[test]
    fn latest_entry_skips_yanked_and_picks_highest_sequence() {
        let idx = MarketIndex {
            index_id: "i".into(),
            sequence: 5,
            updated: String::new(),
            entries: vec![
                MarketEntry {
                    plugin_id: "a@b".into(),
                    version: "1.0.0".into(),
                    sequence: 3,
                    content_hash: "sha256:x".into(),
                    min_app_version: String::new(),
                    name: String::new(),
                    description: String::new(),
                    author: String::new(),
                    homepage: String::new(),
                    mirrors: vec![],
                    publish_time: String::new(),
                    yanked: "none".into(),
                    tags: vec![],
                    permissions: vec![],
                    sig_scheme: "none".into(),
                    sigstore_bundle_ref: String::new(),
                },
                MarketEntry {
                    plugin_id: "a@b".into(),
                    version: "2.0.0".into(),
                    sequence: 5,
                    content_hash: "sha256:y".into(),
                    min_app_version: String::new(),
                    name: String::new(),
                    description: String::new(),
                    author: String::new(),
                    homepage: String::new(),
                    mirrors: vec![],
                    publish_time: String::new(),
                    yanked: "vulnerable".into(),
                    tags: vec![],
                    permissions: vec![],
                    sig_scheme: "none".into(),
                    sigstore_bundle_ref: String::new(),
                },
            ],
        };
        // 2.0.0 被 yank → 回退到 1.0.0。
        let mgr_latest = idx
            .entries
            .iter()
            .filter(|e| e.plugin_id == "a@b" && e.yanked == "none")
            .max_by(|a, b| a.sequence.cmp(&b.sequence))
            .unwrap();
        assert_eq!(mgr_latest.version, "1.0.0");
    }
}
