//! 数据面传输抽象 —— **可扩展性核心 seam**。
//!
//! [`Transport`] 定义「如何到达一台已知设备的数据面」。v1 仅实现
//! [`DirectTransport`]（网络可达直连，无 NAT 穿透）。未来的打洞策略
//! （iroh QUIC、云中继）实现同一 trait 并注册进 [`TransportStack`] 的优先级链——
//! 配对协议、数据面调度、UI 全都只依赖本 trait，**新增打洞方式不触碰本 seam 以上
//! 任何代码**。这正是设计文档 §6.5「连接路径 `mDNS→记录地址→打洞→中继` 由引擎
//! 自动升级、用户无感」的落地骨架。

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use super::error::LinkError;
use super::types::{PeerRecord, TransportKind};

/// 拨通一台已配对设备后得到的连接句柄。
///
/// v1 Direct 下 `base_url` 即对端 fluxdown API 的 `http://ip:port`；未来 iroh/relay
/// 变体会把加密隧道封装为一个本地转发的 `base_url`，**数据面调度层只认 base_url**，
/// 因此新增传输方式对上层零改动。
#[derive(Debug, Clone)]
pub struct PeerConn {
    /// 对端 fluxdown API 基址（不含尾斜杠）。
    pub base_url: String,
    /// 本次连接实际走的传输类型（日志/诊断）。
    pub kind: TransportKind,
}

/// 到达已知设备数据面的策略。
#[async_trait]
pub trait Transport: Send + Sync {
    /// 传输类型标签。
    fn kind(&self) -> TransportKind;

    /// 本传输是否具备拨号 `peer` 所需信息（Direct 需至少一个 Direct 候选）。
    fn supports(&self, peer: &PeerRecord) -> bool;

    /// 建立到 `peer` 的已鉴权数据面端点。不可达时返回
    /// [`LinkError::Unreachable`]，让 [`TransportStack`] 落到下一策略。
    async fn connect(&self, peer: &PeerRecord) -> Result<PeerConn, LinkError>;
}

/// 网络可达直连（v1 唯一传输）：拨号首个 Direct 候选 `ip:port` 并 `/ping` 探活。
///
/// **不使用任何 NAT 穿透**——仅适用于双方网络互相可达（同局域网 / 已配端口转发）。
/// 跨网穿透留给未来的 iroh/relay 传输。
pub struct DirectTransport {
    client: reqwest::Client,
    probe_timeout: Duration,
}

impl DirectTransport {
    /// 用给定 HTTP 客户端构造（客户端应配 `.no_proxy()`，回环/局域网直连不走代理）。
    #[must_use]
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            client,
            probe_timeout: Duration::from_secs(3),
        }
    }
}

#[async_trait]
impl Transport for DirectTransport {
    fn kind(&self) -> TransportKind {
        TransportKind::Direct
    }

    fn supports(&self, peer: &PeerRecord) -> bool {
        peer.direct_address().is_some()
    }

    async fn connect(&self, peer: &PeerRecord) -> Result<PeerConn, LinkError> {
        // 依次尝试全部 Direct 候选而非只试第一个：候选列表现在可能同时
        // 保留 mDNS 新发现的地址（未探测、纯启发式排序，见
        // `crate::link::discovery::pick_best_v4` 文档）与配对时验证过的
        // 旧地址——只试首个会让一次误选直接判死本该可达的设备。任一候选
        // 连通即返回；全部候选都失败才 `Unreachable`。
        for candidate in peer
            .candidates
            .iter()
            .filter(|c| c.kind == TransportKind::Direct)
        {
            let addr = candidate.address.as_str();
            let base_url = format!("http://{addr}");
            // 直连以 /ping 探活判定可达；失败即视为该候选不可达，试下一个。
            let ping = format!("{base_url}/ping");
            let Ok(resp) = self
                .client
                .get(&ping)
                .timeout(self.probe_timeout)
                .send()
                .await
            else {
                continue;
            };
            if !resp.status().is_success() {
                continue;
            }
            // TOFU 身份复核（`PeerRecord::identity_pub` 文档承诺的「后续
            // 连接重校验」在此落地）：地址可能因 DHCP 回收复用、或被中间
            // 人篡改指向别的设备，`/ping` 200 响应不代表就是配对时那台
            // ——必须比对指纹。JSON 解析失败视同该候选不可达（换一个候选
            // 或许能解决）；解析成功但指纹不符是明确的身份冒充信号，必须
            // 立即上抛终止整条尝试链，不能继续试其它候选——否则攻击者能
            // 靠混入一个抢答的候选，把本该立即中止的身份冒充错误悄悄
            // 降级成一次普通的「不可达，换个候选」。
            let Ok(body) = resp.json::<serde_json::Value>().await else {
                continue;
            };
            verify_ping_identity(&body, &peer.fingerprint, addr)?;
            return Ok(PeerConn {
                base_url,
                kind: TransportKind::Direct,
            });
        }
        Err(LinkError::Unreachable)
    }
}

/// 从 `/ping` 响应体校验对端身份指纹——[`DirectTransport::connect`] 的 TOFU
/// 复核核心逻辑，抽成纯函数便于单测（无需起真实 HTTP 服务器即可覆盖比对
/// 分支）。指纹一致才算通过；缺字段或不一致一律视为身份不符。
fn verify_ping_identity(
    body: &serde_json::Value,
    expect_fp: &str,
    addr: &str,
) -> Result<(), LinkError> {
    match body.get("linkFingerprint").and_then(|v| v.as_str()) {
        Some(fp) if fp == expect_fp => Ok(()),
        _ => Err(LinkError::IdentityMismatch(addr.to_string())),
    }
}

/// 传输策略优先级链：按序尝试各传输，首个成功即返回；全部不可达 → `Unreachable`。
///
/// v1 只含 [`DirectTransport`]；未来 `TransportStack::new(vec![direct, iroh, relay])`
/// 即实现「直连优先、打洞次之、中继兜底」的自动升级，上层调用 [`connect`] 不变。
#[derive(Clone)]
pub struct TransportStack {
    transports: Vec<Arc<dyn Transport>>,
}

impl TransportStack {
    /// 用一组传输（按优先级排列）构造。
    #[must_use]
    pub fn new(transports: Vec<Arc<dyn Transport>>) -> Self {
        Self { transports }
    }

    /// v1 便捷构造：仅网络可达直连。
    #[must_use]
    pub fn direct_only(client: reqwest::Client) -> Self {
        Self::new(vec![Arc::new(DirectTransport::new(client))])
    }

    /// 已注册的传输类型（诊断/日志用）。
    #[must_use]
    pub fn kinds(&self) -> Vec<TransportKind> {
        self.transports.iter().map(|t| t.kind()).collect()
    }

    /// 按优先级拨号 `peer`，返回首个成功的连接。
    ///
    /// 单个传输返回 [`LinkError::Unreachable`] 时继续尝试下一个；返回其他错误
    /// （如鉴权失败）则立即上抛（那不是「换条路能解决」的问题）。
    pub async fn connect(&self, peer: &PeerRecord) -> Result<PeerConn, LinkError> {
        let mut last = LinkError::Unreachable;
        for t in &self.transports {
            if !t.supports(peer) {
                continue;
            }
            match t.connect(peer).await {
                Ok(conn) => return Ok(conn),
                Err(LinkError::Unreachable) => {
                    last = LinkError::Unreachable;
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
        Err(last)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::link::types::{PeerCandidate, PeerRecord};

    fn peer_with(candidates: Vec<PeerCandidate>) -> PeerRecord {
        PeerRecord {
            fingerprint: "fp".to_string(),
            identity_pub: vec![0u8; 32],
            name: "n".to_string(),
            platform: None,
            link_secret: vec![0u8; 32],
            candidates,
            paired_at: 0,
            last_seen_at: 0,
        }
    }

    #[test]
    fn direct_supports_requires_direct_candidate() {
        let client = reqwest::Client::new();
        let t = DirectTransport::new(client);
        assert!(!t.supports(&peer_with(vec![])));
        assert!(t.supports(&peer_with(vec![PeerCandidate {
            kind: TransportKind::Direct,
            address: "127.0.0.1:1".to_string(),
        }])));
        // 仅有未来传输的候选（无 Direct）→ Direct 不支持。
        assert!(!t.supports(&peer_with(vec![PeerCandidate {
            kind: TransportKind::Iroh,
            address: "node-id".to_string(),
        }])));
    }

    #[tokio::test]
    async fn stack_returns_unreachable_when_no_supporting_transport() {
        let stack = TransportStack::direct_only(reqwest::Client::new());
        // 无 Direct 候选 → 没有传输支持 → Unreachable。
        let err = stack.connect(&peer_with(vec![])).await.unwrap_err();
        assert!(matches!(err, LinkError::Unreachable));
        assert_eq!(stack.kinds(), vec![TransportKind::Direct]);
    }

    #[tokio::test]
    async fn direct_connect_unreachable_on_dead_address() {
        let t = DirectTransport::new(reqwest::Client::new());
        // 一个不监听的端口（TEST-NET-1，不可路由）→ Unreachable，不 panic。
        let peer = peer_with(vec![PeerCandidate {
            kind: TransportKind::Direct,
            address: "192.0.2.1:9".to_string(),
        }]);
        let err = t.connect(&peer).await.unwrap_err();
        assert!(matches!(err, LinkError::Unreachable));
    }

    #[test]
    fn verify_ping_identity_accepts_matching_fingerprint() {
        let body = serde_json::json!({ "linkFingerprint": "abc123" });
        assert!(verify_ping_identity(&body, "abc123", "10.0.0.1:1").is_ok());
    }

    #[test]
    fn verify_ping_identity_rejects_mismatched_fingerprint() {
        // 地址被 DHCP 回收复用/篡改指向了另一台设备：指纹对不上，必须拒绝
        // 而不是静默连接——否则任务会被下发给冒充者。
        let body = serde_json::json!({ "linkFingerprint": "someone-else" });
        let err = verify_ping_identity(&body, "abc123", "10.0.0.1:1").unwrap_err();
        assert!(matches!(err, LinkError::IdentityMismatch(addr) if addr == "10.0.0.1:1"));
    }

    #[test]
    fn verify_ping_identity_rejects_missing_fingerprint_field() {
        // 对端没有开启 link（`/ping` 不带 linkFingerprint）：同样不能当作
        // 身份匹配处理。
        let body = serde_json::json!({ "success": true });
        let err = verify_ping_identity(&body, "abc123", "10.0.0.1:1").unwrap_err();
        assert!(matches!(err, LinkError::IdentityMismatch(_)));
    }
}
