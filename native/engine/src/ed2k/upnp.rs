//! eD2K UPnP-IGD 端口映射 —— 争取 NAT 后 HighID。
//!
//! eD2K 服务器判定 HighID 的前提是「我方声明的监听端口可从公网回连」。NAT 后
//! 的客户端默认不可达（→ LowID）。本模块通过 UPnP-IGD 在网关上建立
//! `外部端口 → 本机 (ip:port)` 映射，使入站连接能穿透 NAT 到达我方监听器，
//! 从而登录时被服务器判为 HighID。
//!
//! 委托 [`igd-next`]（异步 tokio 后端）完成 SSDP 发现 + SOAP `AddPortMapping`。
//! 租约有限期（[`LEASE_SECS`]），[`spawn_upnp`] 起一个后台任务周期续租，
//! 返回的 [`UpnpMapping`] 在 drop 时尽力移除映射。
//!
//! 全程 best-effort：无 IGD 网关 / 映射失败都只记日志、返回 `None`，
//! 下载回退到 LowID + 服务器 callback 路径（仍可用），不阻断主流程。

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use igd_next::PortMappingProtocol;
use igd_next::aio::Gateway;
use igd_next::aio::tokio::{Tokio, search_gateway};

use crate::logger::{log_error, log_info};

/// 端口映射租约时长（秒）。有限租约防网关重启后残留死映射；后台任务在
/// 到期前续租。
const LEASE_SECS: u32 = 3600;

/// 续租间隔（略短于租约，留出重试余量）。
const RENEW_INTERVAL: Duration = Duration::from_secs(3000);

/// 映射描述（网关管理页可见）。
const MAPPING_DESC: &str = "FluxDown eD2K";

/// 一组已建立的 UPnP 映射的句柄。drop 时后台任务被 abort 并尽力移除映射。
pub struct UpnpMapping {
    handle: tokio::task::JoinHandle<()>,
    /// 网关探测到的公网 IP（供上层判定 HighID 候选）。
    external_ip: Option<IpAddr>,
}

impl UpnpMapping {
    /// 网关报告的公网 IP（映射成功时）。
    #[must_use]
    pub fn external_ip(&self) -> Option<IpAddr> {
        self.external_ip
    }
}

impl Drop for UpnpMapping {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

/// 尝试为 eD2K 监听端口建立 UPnP 映射。
///
/// 映射 `tcp_port`（peer 互连/服务器回连必需）；`udp_port > 0` 时一并映射
/// （Kad UDP 用）。成功返回携带公网 IP 的 [`UpnpMapping`]（其后台任务周期
/// 续租）；无网关/失败返回 `None`（回退 LowID，不阻断）。
pub async fn spawn_upnp(tcp_port: u16, udp_port: u16) -> Option<UpnpMapping> {
    let gateway = match search_gateway(igd_next::SearchOptions::default()).await {
        Ok(g) => g,
        Err(e) => {
            log_info!("[ed2k-upnp] no IGD gateway found: {}", e);
            return None;
        }
    };

    let local_ip = match local_ipv4().await {
        Some(ip) => ip,
        None => {
            log_info!("[ed2k-upnp] cannot determine local IPv4, skipping UPnP");
            return None;
        }
    };

    let external_ip = gateway.get_external_ip().await.ok();

    // 建立首次映射。TCP 必须成功；UDP 失败仅告警（Kad 不可用但下载仍行）。
    if let Err(e) = add_mapping(&gateway, PortMappingProtocol::TCP, tcp_port, local_ip).await {
        log_error!("[ed2k-upnp] TCP port {} mapping failed: {}", tcp_port, e);
        return None;
    }
    log_info!(
        "[ed2k-upnp] mapped TCP {} (external ip: {:?})",
        tcp_port,
        external_ip
    );
    if udp_port != 0
        && let Err(e) = add_mapping(&gateway, PortMappingProtocol::UDP, udp_port, local_ip).await
    {
        log_info!(
            "[ed2k-upnp] UDP port {} mapping failed (Kad degraded): {}",
            udp_port,
            e
        );
    }

    // 后台续租任务。
    let handle = tokio::spawn(async move {
        loop {
            tokio::time::sleep(RENEW_INTERVAL).await;
            if let Err(e) =
                add_mapping(&gateway, PortMappingProtocol::TCP, tcp_port, local_ip).await
            {
                log_error!("[ed2k-upnp] TCP renew failed: {}", e);
            }
            if udp_port != 0 {
                let _ = add_mapping(&gateway, PortMappingProtocol::UDP, udp_port, local_ip).await;
            }
        }
    });

    Some(UpnpMapping {
        handle,
        external_ip,
    })
}

/// 在网关上建立/刷新一条 `外部端口 → local_ip:port` 映射（外部端口=内部端口）。
async fn add_mapping(
    gateway: &Gateway<Tokio>,
    protocol: PortMappingProtocol,
    port: u16,
    local_ip: Ipv4Addr,
) -> Result<(), String> {
    let local_addr = SocketAddr::new(IpAddr::V4(local_ip), port);
    gateway
        .add_port(protocol, port, local_addr, LEASE_SECS, MAPPING_DESC)
        .await
        .map_err(|e| e.to_string())
}

/// 探测本机在默认路由上的 IPv4（连一个公网地址取 socket 本地址，不实际发包）。
async fn local_ipv4() -> Option<Ipv4Addr> {
    let sock = tokio::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
        .await
        .ok()?;
    // 连一个公网 IP（8.8.8.8:53）—— UDP connect 只设默认目的、不发包，
    // 由此让 OS 选出出口网卡的本地址。
    sock.connect((Ipv4Addr::new(8, 8, 8, 8), 53)).await.ok()?;
    match sock.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_unspecified() => Some(ip),
        _ => None,
    }
}
