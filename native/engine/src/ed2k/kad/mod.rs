//! eMule Kad（Kademlia DHT）去中心化找源。
//!
//! 服务器全挂时的兜底找源路径：解析 `nodes.dat` 引导联系点 → 迭代
//! `FindNode` 逼近 `target = file_hash` 的 KadID → 对最近节点发
//! `SearchSourcesReq` → 提取源。协议编解码见 [`proto`]，UDP 节点与迭代查找
//! 见 [`node`]。
//!
//! 参考实现：monkeyWie/goed2k（`protocol/kad/{types,packets}.go` +
//! `kad_traversal.go`），字节布局逐一核对。

pub mod node;
pub mod proto;

use std::time::Duration;

/// nodes.dat 下载大小上限（正常几十~几百 KB；超此视为异常）。
const MAX_NODES_DAT_BYTES: usize = 4 * 1024 * 1024;
/// nodes.dat 下载超时。
const FETCH_TIMEOUT: Duration = Duration::from_secs(20);

/// 从 `url` 下载 `nodes.dat` 原始字节并校验可解析出 ≥1 个联系点。
///
/// 成功返回原始字节（供缓存 + [`node::find_sources_kad`] 使用）；网络失败、
/// 超限、或解析不出任何联系点均返回 `Err`（调用方容错，不影响服务器找源）。
///
/// # Examples
///
/// ```no_run
/// # async fn run() {
/// use fluxdown_engine::ed2k::kad::fetch_nodes_dat;
/// let bytes = fetch_nodes_dat("https://upd.emule-security.org/nodes.dat")
///     .await
///     .unwrap_or_default();
/// # let _ = bytes;
/// # }
/// ```
pub async fn fetch_nodes_dat(url: &str) -> Result<Vec<u8>, String> {
    let client = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .build()
        .map_err(|e| format!("failed to build http client: {e}"))?;
    let resp = client.get(url).send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("http status {status}"));
    }
    let body = resp.bytes().await.map_err(|e| e.to_string())?;
    if body.len() > MAX_NODES_DAT_BYTES {
        return Err(format!("nodes.dat too large ({} bytes)", body.len()));
    }
    if proto::parse_nodes_dat(&body).is_empty() {
        return Err("nodes.dat parsed zero contacts".to_string());
    }
    Ok(body.to_vec())
}
