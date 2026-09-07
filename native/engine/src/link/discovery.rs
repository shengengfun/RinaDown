//! 发现层：mDNS 局域网自动发现（广播 + 浏览）+ 手动地址 `/ping` 探测。
//!
//! # 两个职责（设计文档 §6.5）
//! 1. **发现设备以加入名册**（免账号本地配对入口）：浏览 `_fluxdown._tcp.local.`。
//! 2. **为已知设备找最快连接路径**：mDNS 得到的 `ip:port` 即 Direct 候选。
//!
//! **扩展点**：发现方式是可替换策略；未来可加其他发现源（如账户名册回填、二维码
//! 带地址），只要往 [`DiscoveredPeer`] 汇流即可，配对/传输层无感。mDNS 广播/浏览
//! 各自独立运行在 mdns-sd 自建线程上（不阻塞宿主的 async runtime）。
//!
//! # 隐私权衡（可发现性的固有取舍，非编码缺陷）
//! 广播的 TXT 记录明文携带 `fp`（长期身份指纹）/`name`（设备名）/`plat`/
//! `ver`（平台/版本）：局域网内任何主机都能被动监听或主动查询
//! `_fluxdown._tcp.local.` 枚举出这些信息，完全不需要任何配对码。这是
//! 「让本机可被发现」这一功能自身的代价，仅凭这些信息也拼不出配对所需的
//! 临时密钥/共享密钥，无法完成配对。缓解手段：只在确实需要被添加时才开启
//! 广播，配对完成或暂不需要被发现时调用 `LinkManager::stop_advertising`
//! 停止广播，缩短暴露窗口。

use std::net::{IpAddr, Ipv4Addr, UdpSocket};
use std::time::Duration;

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use tokio::sync::mpsc;

use super::error::{LinkError, LinkResult};
use super::types::{DiscoveredPeer, DiscoveryKind};

/// FluxDown 局域网服务类型（DNS-SD）。
pub const SERVICE_TYPE: &str = "_fluxdown._tcp.local.";

/// TXT 记录键。
const TXT_FINGERPRINT: &str = "fp";
const TXT_NAME: &str = "name";
const TXT_PLATFORM: &str = "plat";
const TXT_VERSION: &str = "ver";

fn map_mdns_err(e: mdns_sd::Error) -> LinkError {
    LinkError::Io(e.to_string())
}

/// mDNS 广播器：向局域网通告本设备的 fluxdown 服务（供其他设备发现并配对）。
/// 持有 daemon 句柄，`Drop` 时优雅关闭。
pub struct MdnsAdvertiser {
    daemon: ServiceDaemon,
}

impl MdnsAdvertiser {
    /// 开始广播。`port` 为本机 fluxdown API 端口；TXT 携带身份指纹/名称/平台/版本。
    pub fn start(
        port: u16,
        fingerprint: &str,
        name: &str,
        platform: Option<&str>,
        app_version: Option<&str>,
    ) -> LinkResult<Self> {
        let daemon = ServiceDaemon::new().map_err(map_mdns_err)?;
        // 实例名用短指纹保证唯一（同名设备不冲突）；host_name 走 <fp>.local.。
        let short_fp: String = fingerprint.chars().take(12).collect();
        let host_name = format!("{short_fp}.local.");
        let props = [
            (TXT_FINGERPRINT, fingerprint),
            (TXT_NAME, name),
            (TXT_PLATFORM, platform.unwrap_or("")),
            (TXT_VERSION, app_version.unwrap_or("")),
        ];
        // ip 传空 + enable_addr_auto()：由 mdns-sd 自动探测并跟踪本机接口地址。
        let info = ServiceInfo::new(SERVICE_TYPE, &short_fp, &host_name, "", port, &props[..])
            .map_err(map_mdns_err)?
            .enable_addr_auto();
        daemon.register(info).map_err(map_mdns_err)?;
        Ok(Self { daemon })
    }
}

impl Drop for MdnsAdvertiser {
    fn drop(&mut self) {
        let _ = self.daemon.shutdown();
    }
}

/// mDNS 浏览器：发现局域网内的 fluxdown 设备，解析后经 `sink` 汇出
/// [`DiscoveredPeer`]。持有 daemon 句柄，`Drop` 时优雅关闭。
pub struct MdnsBrowser {
    daemon: ServiceDaemon,
}

impl MdnsBrowser {
    /// 开始浏览，把解析出的设备推送到 `sink`（满/关闭即静默丢弃，不阻塞）。
    pub fn start(sink: mpsc::Sender<DiscoveredPeer>) -> LinkResult<Self> {
        let daemon = ServiceDaemon::new().map_err(map_mdns_err)?;
        let receiver = daemon.browse(SERVICE_TYPE).map_err(map_mdns_err)?;
        tokio::spawn(async move {
            while let Ok(event) = receiver.recv_async().await {
                if let ServiceEvent::ServiceResolved(info) = event
                    && let Some(peer) = resolved_to_peer(&info)
                {
                    // 接收端满或已关闭：丢弃本条，继续浏览。
                    let _ = sink.try_send(peer);
                }
            }
        });
        Ok(Self { daemon })
    }
}

impl Drop for MdnsBrowser {
    fn drop(&mut self) {
        let _ = self.daemon.shutdown();
    }
}

/// 把解析出的 mDNS 服务映射为 [`DiscoveredPeer`]（IPv4 地址经 `pick_best_v4`
/// 按可达性优先级挑选，而非不加区分地取第一个）。
fn resolved_to_peer(info: &mdns_sd::ResolvedService) -> Option<DiscoveredPeer> {
    let candidates: Vec<Ipv4Addr> = info.get_addresses_v4().into_iter().collect();
    let addr = pick_best_v4(&candidates)?;
    let fingerprint = info
        .get_property_val_str(TXT_FINGERPRINT)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let name = info
        .get_property_val_str(TXT_NAME)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| addr.to_string());
    let platform = info
        .get_property_val_str(TXT_PLATFORM)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let app_version = info
        .get_property_val_str(TXT_VERSION)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    Some(DiscoveredPeer {
        fingerprint,
        name,
        platform,
        host: addr.to_string(),
        port: info.get_port(),
        app_version,
        kind: DiscoveryKind::Mdns,
    })
}

/// 从候选 IPv4 地址中选出「最可能可达」的一个。
///
/// 广播端用 `enable_addr_auto()` 会把本机所有网卡地址都塞进服务记录；若对端
/// 同时开着 Docker/WSL/VPN 等虚拟网卡，不加区分地取第一个很可能选中一个从
/// 浏览方根本连不通的地址，表现为「发现到了但怎么都连不上」。
///
/// 排除：回环 `127.0.0.0/8`、link-local `169.254.0.0/16`、Docker 默认桥接
/// 网段 `172.17.0.0/16`（几乎总是不可达的容器内部地址）。
/// 优先级（数字越小越优先）：`192.168.0.0/16` 家庭/办公网常见网段 >
/// `10.0.0.0/8` 路由器下发/企业网常见网段 > 其余 `172.16.0.0/12` 私网 >
/// 其他（含公网）地址。都不命中优先私网段时，回退到第一个未被排除的地址
/// （`Iterator::min_by_key` 对并列最小值保留原始顺序中的第一个）；全部被
/// 排除则返回 `None`。
///
/// **启发式，可能选错**：这只是「多个网段里挑一个最可能通」的经验排序，
/// 从未实际探测任何一个候选地址的可达性（不发包、不比对指纹）。调用方
/// ——尤其是刷新**已配对设备**回连候选的路径（见
/// `crate::link::manager::LinkManager::start_discovery`）——必须把这里
/// 选出的地址当作「值得优先一试」而非「确认可达」，保留原有候选作为
/// 回退，不能直接覆盖掉配对时验证过的旧地址。
#[must_use]
fn pick_best_v4(addrs: &[Ipv4Addr]) -> Option<Ipv4Addr> {
    fn is_excluded(ip: &Ipv4Addr) -> bool {
        let o = ip.octets();
        ip.is_loopback() || ip.is_link_local() || (o[0] == 172 && o[1] == 17)
    }
    fn priority(ip: &Ipv4Addr) -> u8 {
        let o = ip.octets();
        if o[0] == 192 && o[1] == 168 {
            0
        } else if o[0] == 10 {
            1
        } else if o[0] == 172 && (16..=31).contains(&o[1]) {
            2
        } else {
            3
        }
    }
    addrs
        .iter()
        .filter(|ip| !is_excluded(ip))
        .min_by_key(|ip| priority(ip))
        .copied()
}

/// 手动地址探测：GET `http://host:port/ping`，解析设备身份/名称/平台/版本。
/// 供「本地配对」的「手动输入地址」兜底路径（Docker bridge / AP 隔离等 mDNS 失效场景）。
pub async fn probe(client: &reqwest::Client, host: &str, port: u16) -> LinkResult<DiscoveredPeer> {
    let url = format!("http://{host}:{port}/ping");
    let resp = client
        .get(&url)
        .timeout(Duration::from_secs(3))
        .send()
        .await
        .map_err(|e| LinkError::Io(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(LinkError::Unreachable);
    }
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| LinkError::Io(e.to_string()))?;
    let get = |k: &str| {
        json.get(k)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    Ok(DiscoveredPeer {
        fingerprint: get("linkFingerprint"),
        name: get("linkName").unwrap_or_else(|| host.to_string()),
        platform: get("linkPlatform"),
        host: host.to_string(),
        port,
        app_version: get("version"),
        kind: DiscoveryKind::Manual,
    })
}

/// 计算本机朝向 `peer_host` 的出站本地 IP（UDP connect 技巧，不真正发包），
/// 拼成 Direct 候选 `ip:port`（`api_port` = 本机 fluxdown API 端口）。
///
/// 供配对时向对端自报可达地址（对端存为回连候选）。探测失败返回空列表。
#[must_use]
pub fn local_direct_addrs(peer_host: &str, api_port: u16) -> Vec<String> {
    let Ok(peer_ip) = peer_host.parse::<IpAddr>() else {
        return Vec::new();
    };
    let bind = if peer_ip.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };
    let Ok(sock) = UdpSocket::bind(bind) else {
        return Vec::new();
    };
    // connect 只设置默认目的地，不发包；随后 local_addr 给出朝该目的地的本地 IP。
    if sock.connect((peer_ip, api_port.max(1))).is_err() {
        return Vec::new();
    }
    match sock.local_addr() {
        Ok(local) => vec![format!("{}:{}", local.ip(), api_port)],
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn local_addr_towards_loopback_is_loopback() {
        let addrs = local_direct_addrs("127.0.0.1", 17800);
        assert_eq!(addrs.len(), 1);
        assert!(addrs[0].starts_with("127.0.0.1:") || addrs[0].starts_with("127."));
        assert!(addrs[0].ends_with(":17800"));
    }

    #[test]
    fn local_addr_towards_garbage_is_empty() {
        assert!(local_direct_addrs("not-an-ip", 17800).is_empty());
    }

    #[test]
    fn pick_best_v4_prefers_192_168_then_10_then_other_private() {
        // 全量四个地址：优先命中 192.168/16。
        let all = [
            Ipv4Addr::new(172, 20, 0, 5),
            Ipv4Addr::new(203, 0, 113, 9), // 公网地址，优先级最低
            Ipv4Addr::new(10, 0, 0, 5),
            Ipv4Addr::new(192, 168, 1, 5),
        ];
        assert_eq!(pick_best_v4(&all), Some(Ipv4Addr::new(192, 168, 1, 5)));
        // 去掉 192.168 后应退而求其次选中 10/8。
        assert_eq!(pick_best_v4(&all[..3]), Some(Ipv4Addr::new(10, 0, 0, 5)));
    }

    #[test]
    fn pick_best_v4_excludes_loopback_link_local_and_docker_bridge() {
        // 三个排除项之外仅剩一个公网地址，必须回退到它而不是径直选第一个
        // （第一个是被排除的回环地址）。
        let addrs = [
            Ipv4Addr::new(127, 0, 0, 1),
            Ipv4Addr::new(169, 254, 1, 1),
            Ipv4Addr::new(172, 17, 0, 2),
            Ipv4Addr::new(203, 0, 113, 9),
        ];
        assert_eq!(pick_best_v4(&addrs), Some(Ipv4Addr::new(203, 0, 113, 9)));
    }

    #[test]
    fn pick_best_v4_all_excluded_returns_none() {
        let addrs = [
            Ipv4Addr::new(127, 0, 0, 1),
            Ipv4Addr::new(169, 254, 1, 1),
            Ipv4Addr::new(172, 17, 0, 2),
        ];
        assert!(pick_best_v4(&addrs).is_none());
    }
}
