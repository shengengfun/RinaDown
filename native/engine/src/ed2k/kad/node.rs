//! Kad UDP 节点与迭代找源。
//!
//! 单 UDP socket，三阶段：
//! 0. **Bootstrap + Hello**：对初始联系点发 `BootstrapReq`（响应即证存活，
//!    回传当前鲜活联系点）+ `HelloReq`（握手，让对端把我方计入路由表）。
//!    KADEMLIA2 节点通常不响应未握手的发送方，故此阶段是收到回包的前提。
//! 1. **FindNode**：并发（≤[`BRANCH`]）向最近的未查询节点发 `Req{FindNode, target}`，
//!    把应答里的联系点并入候选集（去重、按 XOR 距离排序、上限 [`MAX_CANDIDATES`]），
//!    直到无更近节点或阶段超时。
//! 2. **SearchSources**：对最近的 [`SEARCH_TARGETS`] 个节点发 `SearchSourcesReq`，
//!    收 `SearchRes` 提取源，去重返回。
//!
//! 无节点 / 超时 / cancel → 返回空 vec 或 [`DownloadError`]，绝不 panic。

use std::collections::HashSet;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::time::{Duration, Instant};

use rand::Rng;
use tokio::net::UdpSocket;
use tokio_util::sync::CancellationToken;

use super::proto::{self, Contact, KadId};
use crate::downloader::DownloadError;
use crate::ed2k::server::PeerAddr;
use crate::logger::log_info;

/// 每轮 FindNode 并发查询的节点数。
const BRANCH: usize = 5;
/// 候选集上限（按距离截断）。
const MAX_CANDIDATES: usize = 100;
/// Phase 2 发 SearchSourcesReq 的最近节点数。
const SEARCH_TARGETS: usize = 12;
/// UDP 接收缓冲。
const RECV_BUF: usize = 8192;
/// 单轮收包窗口。
const ROUND_WINDOW: Duration = Duration::from_millis(2000);
/// Phase 1 无改善的最大连续轮数（收敛判据）。
const MAX_STALLS: u32 = 3;
/// Phase 0 bootstrap 并发扇出（对多少个初始联系点发 BootstrapReq）。
const BOOTSTRAP_FANOUT: usize = 40;
/// Phase 0 bootstrap 收包窗口。
const BOOTSTRAP_WINDOW: Duration = Duration::from_millis(3000);

/// 迭代查找中的候选节点。
struct Candidate {
    addr: SocketAddrV4,
    id: KadId,
    queried: bool,
}

impl Candidate {
    fn from_contact(c: &Contact) -> Option<Self> {
        if c.endpoint.udp_port == 0 || c.endpoint.ip.is_unspecified() {
            return None;
        }
        Some(Candidate {
            addr: SocketAddrV4::new(c.endpoint.ip, c.endpoint.udp_port),
            id: c.id,
            queried: false,
        })
    }
}

/// Kad 去中心化找源入口。见模块文档。
///
/// # Examples
///
/// ```
/// # async fn run() {
/// use fluxdown_engine::ed2k::kad::node::find_sources_kad;
/// use std::time::Duration;
/// use tokio_util::sync::CancellationToken;
///
/// let cancel = CancellationToken::new();
/// // 空 nodes.dat → 无 bootstrap，立即返回空。
/// let sources = find_sources_kad(
///     &[0u8; 16], 1024, 0, 4662, &[], Duration::from_secs(1), &cancel,
/// )
/// .await
/// .unwrap_or_default();
/// assert!(sources.is_empty());
/// # }
/// ```
#[allow(clippy::too_many_arguments)]
pub async fn find_sources_kad(
    file_hash: &[u8; 16],
    total_bytes: u64,
    udp_port: u16,
    tcp_port: u16,
    nodes_dat: &[u8],
    timeout: Duration,
    cancel: &CancellationToken,
) -> Result<Vec<PeerAddr>, DownloadError> {
    // 自身 KadID（随机）：Hello 握手上报，供对端把我方计入路由表。
    let mut self_bytes = [0u8; 16];
    rand::rng().fill(&mut self_bytes);
    let self_id = KadId::from_memory(&self_bytes);
    let bootstrap = proto::parse_nodes_dat(nodes_dat);
    if bootstrap.is_empty() {
        return Ok(Vec::new());
    }

    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, udp_port))
        .await
        .map_err(DownloadError::Io)?;

    let target = KadId::from_memory(file_hash);

    // 初始候选集（去重 addr）。
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut seen: HashSet<SocketAddrV4> = HashSet::new();
    for c in &bootstrap {
        if let Some(cand) = Candidate::from_contact(c)
            && seen.insert(cand.addr)
        {
            candidates.push(cand);
        }
    }
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    candidates.sort_by(|a, b| a.id.xor_cmp(&b.id, &target));

    let overall_deadline = Instant::now() + timeout;
    let mut buf = vec![0u8; RECV_BUF];

    // ---------------- Phase 0: Bootstrap + Hello 握手 ----------------
    // KADEMLIA2 节点通常不响应"未曾握手"的发送方 → 直接 FindNode 收 0 包。
    // 先对初始联系点发 BootstrapReq（响应即证明存活，且回传当前鲜活联系点），
    // 同时发 HelloReq 完成握手，把我方计入对端路由表。
    {
        let boot_targets: Vec<SocketAddrV4> = candidates
            .iter()
            .take(BOOTSTRAP_FANOUT)
            .map(|c| c.addr)
            .collect();
        let boot_req = proto::build_bootstrap_req();
        let hello_req = proto::build_hello_req(&self_id, tcp_port);
        let mut send_ok = 0u32;
        let mut send_err = 0u32;
        for addr in &boot_targets {
            match socket.send_to(&boot_req, SocketAddr::V4(*addr)).await {
                Ok(_) => send_ok += 1,
                Err(_) => send_err += 1,
            }
            let _ = socket.send_to(&hello_req, SocketAddr::V4(*addr)).await;
        }
        log_info!(
            "[ed2k-kad] bootstrap sent to {} targets ({} ok, {} send-err)",
            boot_targets.len(),
            send_ok,
            send_err
        );

        let mut boot_responses = 0u32;
        let window_end = (Instant::now() + BOOTSTRAP_WINDOW).min(overall_deadline);
        while let Some(n) = recv_within(&socket, &mut buf, window_end).await {
            let Some((opcode, payload)) = proto::parse_datagram(&buf[..n], RECV_BUF * 8) else {
                continue;
            };
            match opcode {
                proto::OP_BOOTSTRAP_RES => {
                    if let Some(contacts) = proto::decode_bootstrap_res(&payload) {
                        boot_responses += 1;
                        for c in &contacts {
                            if let Some(cand) = Candidate::from_contact(c)
                                && seen.insert(cand.addr)
                            {
                                candidates.push(cand);
                            }
                        }
                    }
                }
                // Hello 应答 / HelloAck：对端已握手，无需额外处理（存活即够）。
                proto::OP_HELLO_RES | proto::OP_HELLO_RES_ACK => boot_responses += 1,
                _ => {}
            }
        }
        candidates.sort_by(|a, b| a.id.xor_cmp(&b.id, &target));
        candidates.truncate(MAX_CANDIDATES);
        log_info!(
            "[ed2k-kad] bootstrap: {} responses, {} candidates after merge",
            boot_responses,
            candidates.len()
        );
    }

    let p1_deadline = Instant::now() + (overall_deadline - Instant::now()) / 2;

    // ---------------- Phase 1: FindNode ----------------
    let mut stalls: u32 = 0;
    let mut fn_responses = 0u32;
    while Instant::now() < p1_deadline {
        if cancel.is_cancelled() {
            return Err(DownloadError::Cancelled);
        }
        // 取最近的未查询节点，最多 BRANCH 个。
        let batch: Vec<(SocketAddrV4, KadId)> = candidates
            .iter_mut()
            .filter(|c| !c.queried)
            .take(BRANCH)
            .map(|c| {
                c.queried = true;
                (c.addr, c.id)
            })
            .collect();
        if batch.is_empty() {
            break;
        }

        for (addr, id) in &batch {
            let pkt = proto::build_find_node_req(&target, id);
            let _ = socket.send_to(&pkt, SocketAddr::V4(*addr)).await;
        }

        let prev_best = candidates.first().map(|c| c.id);
        let window_end = (Instant::now() + ROUND_WINDOW).min(p1_deadline);
        while let Some(n) = recv_within(&socket, &mut buf, window_end).await {
            let Some((opcode, payload)) = proto::parse_datagram(&buf[..n], RECV_BUF * 8) else {
                continue;
            };
            if opcode != proto::OP_RES {
                continue;
            }
            let Some(contacts) = proto::decode_find_node_res(&payload) else {
                continue;
            };
            fn_responses += 1;
            for c in &contacts {
                if let Some(cand) = Candidate::from_contact(c)
                    && seen.insert(cand.addr)
                {
                    candidates.push(cand);
                }
            }
        }

        candidates.sort_by(|a, b| a.id.xor_cmp(&b.id, &target));
        candidates.truncate(MAX_CANDIDATES);

        let improved = match (prev_best, candidates.first()) {
            (Some(prev), Some(now)) => now.id.xor_cmp(&prev, &target) == std::cmp::Ordering::Less,
            _ => false,
        };
        if improved {
            stalls = 0;
        } else {
            stalls += 1;
            if stalls >= MAX_STALLS {
                break;
            }
        }
    }
    log_info!(
        "[ed2k-kad] findnode: {} responses, {} candidates",
        fn_responses,
        candidates.len()
    );

    // ---------------- Phase 2: SearchSources ----------------
    let search_targets: Vec<SocketAddrV4> = candidates
        .iter()
        .take(SEARCH_TARGETS)
        .map(|c| c.addr)
        .collect();
    let req = proto::build_search_sources_req(&target, total_bytes);
    for addr in &search_targets {
        if cancel.is_cancelled() {
            return Err(DownloadError::Cancelled);
        }
        let _ = socket.send_to(&req, SocketAddr::V4(*addr)).await;
    }

    let mut sources: Vec<PeerAddr> = Vec::new();
    let mut source_seen: HashSet<PeerAddr> = HashSet::new();
    let mut idle_gaps = 0u32;
    let mut search_responses = 0u32;
    while Instant::now() < overall_deadline {
        if cancel.is_cancelled() {
            break;
        }
        let window_end = (Instant::now() + ROUND_WINDOW).min(overall_deadline);
        match recv_within(&socket, &mut buf, window_end).await {
            Some(n) => {
                idle_gaps = 0;
                let Some((opcode, payload)) = proto::parse_datagram(&buf[..n], RECV_BUF * 8) else {
                    continue;
                };
                if opcode != proto::OP_SEARCH_RES {
                    continue;
                }
                let Some(entries) = proto::decode_search_res(&payload) else {
                    continue;
                };
                search_responses += 1;
                for e in &entries {
                    if let Some(peer) = e.source_peer()
                        && source_seen.insert(peer)
                    {
                        sources.push(peer);
                    }
                }
            }
            None => {
                idle_gaps += 1;
                if idle_gaps >= MAX_STALLS {
                    break;
                }
            }
        }
    }

    log_info!(
        "[ed2k-kad] done: {} sources ({} search-res) from {} bootstrap contacts, {} search targets",
        sources.len(),
        search_responses,
        bootstrap.len(),
        search_targets.len()
    );
    Ok(sources)
}

/// 在 `deadline` 前收一个 datagram，返回字节数；超时/致命错误返回 `None`。
///
/// Windows 专属陷阱：向已死节点发包会触发 ICMP 端口不可达，Windows 把它作为
/// **下一次 `recv_from` 的 WSAECONNRESET(10054)** 上报。若据此返回 `None`，
/// 单个死节点就会终止整个接收窗口，漏掉其后所有存活节点的回包。故遇 10054
/// 忽略并继续等到 deadline。
async fn recv_within(socket: &UdpSocket, buf: &mut [u8], deadline: Instant) -> Option<usize> {
    loop {
        let now = Instant::now();
        if now >= deadline {
            return None;
        }
        match tokio::time::timeout(deadline - now, socket.recv_from(buf)).await {
            Ok(Ok((n, _addr))) => return Some(n),
            Ok(Err(e)) if e.raw_os_error() == Some(10054) => continue,
            _ => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::find_sources_kad;
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn empty_nodes_dat_returns_empty_fast() {
        let cancel = CancellationToken::new();
        let start = std::time::Instant::now();
        let res = find_sources_kad(
            &[0u8; 16],
            1024,
            0,
            4662,
            &[],
            Duration::from_secs(5),
            &cancel,
        )
        .await;
        // 空 bootstrap 立即返回，不触网、不等满 timeout。
        assert!(start.elapsed() < Duration::from_secs(1));
        match res {
            Ok(v) => assert!(v.is_empty()),
            Err(e) => panic!("expected Ok, got {e:?}"),
        }
    }

    #[tokio::test]
    async fn unreachable_bootstrap_returns_within_timeout() {
        // 构造 1 条指向本机高位保留端口的联系点（无人应答）。
        // 短 timeout 内返回空，不 panic、不挂死。
        let mut nodes = Vec::new();
        nodes.extend_from_slice(&1u32.to_le_bytes()); // 旧格式 count=1
        // Contact: KadID(16) + IP(4=127.0.0.1) + UDP(2) + TCP(2) + Version(1)
        nodes.extend_from_slice(&[0u8; 16]);
        nodes.extend_from_slice(&[127, 0, 0, 1]);
        nodes.extend_from_slice(&1u16.to_le_bytes()); // udp_port=1（无监听）
        nodes.extend_from_slice(&4662u16.to_le_bytes());
        nodes.push(0x05);

        let cancel = CancellationToken::new();
        let start = std::time::Instant::now();
        let res = find_sources_kad(
            &[1u8; 16],
            2048,
            0,
            4662,
            &nodes,
            Duration::from_millis(600),
            &cancel,
        )
        .await;
        assert!(start.elapsed() < Duration::from_secs(3));
        match res {
            Ok(v) => assert!(v.is_empty()),
            Err(e) => panic!("expected Ok, got {e:?}"),
        }
    }

    #[tokio::test]
    async fn cancelled_returns_cancelled() {
        let mut nodes = Vec::new();
        nodes.extend_from_slice(&1u32.to_le_bytes());
        nodes.extend_from_slice(&[0u8; 16]);
        nodes.extend_from_slice(&[127, 0, 0, 1]);
        nodes.extend_from_slice(&1u16.to_le_bytes());
        nodes.extend_from_slice(&4662u16.to_le_bytes());
        nodes.push(0x05);

        let cancel = CancellationToken::new();
        cancel.cancel();
        let res = find_sources_kad(
            &[2u8; 16],
            2048,
            0,
            4662,
            &nodes,
            Duration::from_secs(5),
            &cancel,
        )
        .await;
        assert!(matches!(
            res,
            Err(crate::downloader::DownloadError::Cancelled)
        ));
    }
}
