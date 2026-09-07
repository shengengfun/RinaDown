//! `ProxyMode::Auto` 路由决策的跨重启先验——host 级采样结论持久化。
//!
//! 完全照搬 `cdn::health` 范式（进程级内存缓存 + config 表 JSON + 24h TTL +
//! 版本标记整体丢弃重学），另加一个**网络指纹 epoch**：路由观察只在同一
//! 网络环境下可信，加载/记录时指纹不符即整表丢弃重学（RFC 8305 §4
//! 「历史数据 MUST NOT 跨接口使用、换网 SHOULD flush」；Chromium 以
//! `last_local_address_when_quic_worked` 做同构判定）。指纹只存哈希，
//! 不落原始 IP/代理地址。
//!
//! # 与内存 [`crate::auto_proxy::DecisionCache`] 的分工（风险不对称，刻意不同）
//!
//! - **Cooldown / NoSwitch 持久化**：过期或误存的代价 = 多等一个冷却窗或
//!   多做一次 256KB 采样，无害。Cooldown 采用指数退避窗口
//!   （300s × 2^n，封顶 24h——Chromium broken-alt-svc 同公式：
//!   `initial_delay * (1 << broken_count)`，上限 2 天；aria2 Adaptive
//!   选择器的 `2^counter` 天重测同理）：反复证明无优势的 host 越来越少
//!   被采样，跨重启依然成立。
//! - **Proxy 胜绩按确认天数分档**：单日一次性胜绩只作加速信号（任务仍
//!   直连起飞保留多 CDN 聚合资格，仅把采样等待期从 `MIN_RUNTIME` 缩短为
//!   `FAST_REEVAL_MIN_RUNTIME`）；在 ≥2 个「不同天」被确认的 host（境外
//!   直连长期受限的典型）直接以代理起飞（AdoptProxy），有效期随确认
//!   天数阶梯延长（24h×2^(n-1) 封顶 7 天）。误判/代理失效由**反向
//!   failover** 自愈：代理起飞的任务连接类失败即作废先验回直连
//!   （[`clear_proxy_prior`]），杜绝锁死。走代理路由的任务完成时确认
//!   计分 + 续期（被动观测，零探测成本，参照 aria2 ServerStat 由真实
//!   传输回写的模式）。
//!
//! 学习数据是可再生的性能缓存——版本不匹配 / 指纹不符 / 过期 / 解析失败
//! 一律丢弃重学，绝不影响下载正确性。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::db::Db;
use crate::logger::log_info;

/// config 表 key。
const ROUTE_CONFIG_KEY: &str = "auto_route_health";

/// 持久化格式版本。语义规则变化时递增——旧版本数据加载时整体丢弃重学。
const ROUTE_FORMAT_VERSION: u32 = 1;

/// 观察 TTL：与 `cdn_node_health`/`domain_conn_caps` 一致的 24h（aria2
/// `--server-stat-timeout` 默认同为 86400s）。网络路由环境天级漂移，
/// 更长的保留期无先例支撑。
const ROUTE_TTL: Duration = Duration::from_secs(24 * 3600);

/// 容量上限（prune-on-save）：超限按最新观察时间淘汰最旧 host。
const MAX_HOSTS: usize = 512;

/// Cooldown 指数退避基数（与内存态 `COOLDOWN_TTL` 同源：300s）。
const COOLDOWN_BASE_SECS: u64 = 300;

/// 退避指数封顶：300s × 2^8 = 21.3h，再翻倍即超 TTL 无意义。
const COOLDOWN_MAX_SHIFT: u32 = 8;

/// 网络指纹缓存时长——指纹计算含注册表/路由表查询，不必每次记录都做。
const FINGERPRINT_CACHE: Duration = Duration::from_secs(60);

/// 代理胜绩确认计分的「不同天」判定间隔：距上次确认超过 12h 才算新一天
/// 的独立证据（防同日批量任务刷分）。
const PROXY_CONFIRM_GAP_SECS: u64 = 12 * 3600;

/// 代理胜绩有效期封顶：7 天。证据换时长的上限——一周不用即重学。
const PROXY_TTL_MAX_SECS: u64 = 7 * 24 * 3600;

/// 代理起飞（AdoptProxy）所需的最少「不同天」确认数。单日一次性的胜绩
/// 只做加速信号，防偶发误判长期锁路由。
const ADOPT_MIN_CONFIRMS: u32 = 2;

/// AdoptProxy 的实证重验期：距上次**采样实证的胜出**（`win_ts`，非完成
/// 续期）超过 72h → 降档 FastReeval，让一个任务直连起飞重验。直连仍烂
/// 则 ~11s 内重新实证并续 72h；直连已恢复则任务直接享受快直连、续期链
/// 自然断裂——防「代理够用就永不回头看直连」的 exploit 锁死（aria2
/// Adaptive `2^counter` 天重测日程的简化形态）。
const WIN_REVALIDATE_SECS: u64 = 72 * 3600;

/// 单 host 的路由观察。字段全部 `#[serde(default)]`：局部缺失按「无观察」
/// 处理，绝不因格式演进丢整表。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
struct HostRoute {
    /// 上次代理胜绩（采样切换胜出 / 代理路由任务完成续期）的 Unix 秒，0 = 无。
    #[serde(default)]
    proxy_ts: u64,
    /// 胜出采样的代理单连接吞吐（B/s，仅诊断日志用）。
    #[serde(default)]
    proxy_bps: f64,
    /// 「不同天」确认次数（距上次计分超过 [`PROXY_CONFIRM_GAP_SECS`] 的
    /// 胜出/续期各 +1）。驱动 TTL 阶梯与 AdoptProxy 门槛；反向 failover
    /// 清零。
    #[serde(default)]
    proxy_n: u32,
    /// 上次计分的 Unix 秒（独立于 proxy_ts 的计分时钟——以 proxy_ts 为
    /// 基准的话，高频使用的 host 每次续期都会把窗口往后推，永远攒不满
    /// AdoptProxy 门槛）。
    #[serde(default)]
    confirm_ts: u64,
    /// 采样无优势的累计次数（指数退避的 n；胜出时清零）。
    #[serde(default)]
    cool_n: u32,
    /// 上次采样无优势的 Unix 秒，0 = 无。
    #[serde(default)]
    cool_ts: u64,
    /// 上次 validator 不一致（代理命中不同 CDN edge）的 Unix 秒，0 = 无。
    #[serde(default)]
    nosw_ts: u64,
    /// 上次**采样实证**胜出的 Unix 秒（完成续期不刷新它）。AdoptProxy
    /// 的重验时钟。
    #[serde(default)]
    win_ts: u64,
}

/// 落盘格式：`{"v":1,"net":"<hash16>","hosts":{...}}`。
#[derive(Serialize, Deserialize)]
struct RouteFile {
    v: u32,
    net: String,
    hosts: HashMap<String, HostRoute>,
}

/// 启动期路由提示（`auto_route_decision` 在内存缓存 miss 后消费）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RouteHint {
    /// 该 host 近期已被证明代理无优势 / validator 不一致——本任务跳过
    /// 采样状态机（等效内存态 Cooldown/NoSwitch 的跨重启延续）。
    SuppressProbe,
    /// 该 host 有代理胜绩但确认不足 [`ADOPT_MIN_CONFIRMS`] 天——直连
    /// 起飞不变，仅缩短采样等待期，慢了尽快重评估。
    FastReeval,
    /// 该 host 已在多个「不同天」被证明代理更优（如境外直连长期受限的
    /// 场景）——直接以代理起飞，零慢速窗口。照 aria2 默认 Feedback
    /// 选择器「对已知最快服务器直接贪心采用」的先例。两道自愈：连接类
    /// 失败 → 反向 failover 作废先验；72h 无实证胜出 → 降档重验直连。
    AdoptProxy,
}

static ROUTES: OnceLock<StdMutex<HashMap<String, HostRoute>>> = OnceLock::new();

/// 当前生效的网络指纹（表 epoch）。空 = 尚未初始化 / 网络未就绪。
static NET_EPOCH: OnceLock<StdMutex<String>> = OnceLock::new();

/// 指纹计算结果缓存（60s）。
static FINGERPRINT: OnceLock<StdMutex<Option<(String, Instant)>>> = OnceLock::new();

/// 离线启动时暂存的磁盘先验：load 拿不到在线指纹无从校验，先存着，
/// 待 [`ensure_net_epoch`] 首次拿到在线指纹时比对采纳（不符即弃）。
static PENDING: OnceLock<StdMutex<Option<RouteFile>>> = OnceLock::new();

/// 写盘序列号：晚创建的快照作废早创建的（防「作废写」被乱序盖回）。
static PERSIST_SEQ: AtomicU64 = AtomicU64::new(0);

fn routes() -> &'static StdMutex<HashMap<String, HostRoute>> {
    ROUTES.get_or_init(|| StdMutex::new(HashMap::new()))
}

fn net_epoch() -> &'static StdMutex<String> {
    NET_EPOCH.get_or_init(|| StdMutex::new(String::new()))
}

fn pending() -> &'static StdMutex<Option<RouteFile>> {
    PENDING.get_or_init(|| StdMutex::new(None))
}

/// 当前 Unix 秒（病态时钟回退为 0，仅影响 TTL 判定的保守性）。
fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 时间戳是否仍在 TTL 内。
fn fresh(recorded_secs: u64, now_secs: u64) -> bool {
    fresh_within(recorded_secs, ROUTE_TTL.as_secs(), now_secs)
}

/// Cooldown 指数退避窗口秒数：`300 × 2^min(n,8)`，封顶 TTL。
/// n 是「已累计的无优势次数」，首败（n=1）即 600s——比内存态 300s 略保守，
/// 因为持久化条目要跨重启扛更久的环境漂移。
fn cooldown_window_secs(cool_n: u32) -> u64 {
    COOLDOWN_BASE_SECS
        .saturating_mul(1u64 << cool_n.min(COOLDOWN_MAX_SHIFT))
        .min(ROUTE_TTL.as_secs())
}

/// 代理胜绩的有效期：证据换时长。`24h × 2^(n-1)`，封顶 7 天——
/// 连用 1 天记 24h，2 天 48h，3 天 96h，≥4 天 7 天（aria2 Adaptive
/// 重测日程 `2^counter × 24h` 同构，方向取正面记忆）。
fn proxy_ttl_secs(proxy_n: u32) -> u64 {
    ROUTE_TTL
        .as_secs()
        .saturating_mul(1u64 << proxy_n.saturating_sub(1).min(3))
        .min(PROXY_TTL_MAX_SECS)
}

/// 时间戳是否仍在给定 TTL 秒数内。未来时间戳（时钟回拨产物）按无效
/// 观察处理——否则恒新鲜永不过期，AdoptProxy 会被变相钉死。
fn fresh_within(recorded_secs: u64, ttl_secs: u64, now_secs: u64) -> bool {
    recorded_secs > 0 && recorded_secs <= now_secs && now_secs - recorded_secs < ttl_secs
}

/// 代理胜绩确认：距上次**计分**（confirm_ts）超过
/// [`PROXY_CONFIRM_GAP_SECS`] 才 +1；proxy_ts 无条件刷新（续期）。
fn bump_proxy_confirm(e: &mut HostRoute, now: u64) {
    if e.confirm_ts == 0 || now.saturating_sub(e.confirm_ts) > PROXY_CONFIRM_GAP_SECS {
        e.proxy_n = e.proxy_n.saturating_add(1);
        e.confirm_ts = now;
    }
    e.proxy_ts = now;
}

/// 纯决策函数：单 host 观察 → 启动期提示。优先级 NoSwitch > Cooldown >
/// Proxy——完整性防线最高，冷却抑制次之；代理胜绩按确认天数分档
/// （≥2 天且 72h 内有实证胜出才代理起飞，否则仅加速）。
fn hint_for(entry: &HostRoute, now: u64) -> Option<RouteHint> {
    if fresh(entry.nosw_ts, now) {
        return Some(RouteHint::SuppressProbe);
    }
    if fresh_within(entry.cool_ts, cooldown_window_secs(entry.cool_n), now) {
        return Some(RouteHint::SuppressProbe);
    }
    if fresh_within(entry.proxy_ts, proxy_ttl_secs(entry.proxy_n), now) {
        let adopt = entry.proxy_n >= ADOPT_MIN_CONFIRMS
            && fresh_within(entry.win_ts, WIN_REVALIDATE_SECS, now);
        return Some(if adopt {
            RouteHint::AdoptProxy
        } else {
            RouteHint::FastReeval
        });
    }
    None
}

/// 纯函数：网络指纹 = sha256(系统代理 host:port + '\0' + 本机 LAN IP) 前
/// 16 hex。两个输入都可为空（离线/无系统代理），退化为常量指纹——表现
/// 同 24h TTL 纯时间失效，不比现状差。
fn fingerprint_of(proxy: &str, lan_ip: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(proxy.as_bytes());
    hasher.update([0u8]);
    hasher.update(lan_ip.as_bytes());
    hex::encode(&hasher.finalize()[..8])
}

/// 本机默认路由的出口 LAN IP。`UdpSocket::connect` **不发任何数据包**，
/// 纯路由表查询；无默认路由（离线/睡眠唤醒窗口）→ None。
fn lan_ip() -> Option<String> {
    std::net::UdpSocket::bind(("0.0.0.0", 0))
        .and_then(|s| {
            s.connect(("8.8.8.8", 53))?;
            s.local_addr()
        })
        .ok()
        .map(|a| a.ip().to_string())
}

/// 本机是否有默认路由——区分「代理死了」与「整机断网」：反向 failover
/// 只应在前者作废先验（断网时所有代理路由任务都会报连接类错误，
/// 与代理无关）。
pub(crate) fn network_reachable() -> bool {
    lan_ip().is_some()
}

/// 计算当前网络指纹（无缓存）。**拿不到出口 IP = 网络未就绪，返回空串
/// 表示 unknown**——不是「另一张网」：调用方见空指纹一律维持现状，
/// 避免睡眠唤醒/Wi-Fi 漫游/VPN 重拨的无路由窗口误清全表。
fn compute_net_fingerprint() -> String {
    let Some(lan) = lan_ip() else {
        return String::new();
    };
    let proxy = crate::proxy_config::detect_system_proxy()
        .ok()
        .flatten()
        .map(|p| format!("{}:{}", p.host, p.port))
        .unwrap_or_default();
    fingerprint_of(&proxy, &lan)
}

/// 带 60s 缓存的当前网络指纹。
fn net_fingerprint() -> String {
    let cache = FINGERPRINT.get_or_init(|| StdMutex::new(None));
    if let Ok(mut slot) = cache.lock() {
        if let Some((fp, at)) = slot.as_ref()
            && at.elapsed() < FINGERPRINT_CACHE
        {
            return fp.clone();
        }
        let fp = compute_net_fingerprint();
        *slot = Some((fp.clone(), Instant::now()));
        return fp;
    }
    compute_net_fingerprint()
}

/// 校准表 epoch：网络指纹变化（换 Wi-Fi / 开关 VPN / 改系统代理）时清空
/// 内存表——旧网络学到的路由结论对新网络是噪声（RFC 8305 flush 语义）。
/// 指纹为空（网络未就绪）时维持现状。每次 lookup/record 前调用。
fn ensure_net_epoch() {
    let fp = net_fingerprint();
    if fp.is_empty() {
        return;
    }
    let Ok(mut cur) = net_epoch().lock() else {
        return;
    };
    if *cur == fp {
        return;
    }
    let had = !cur.is_empty();
    *cur = fp.clone();
    drop(cur);
    if had {
        if let Ok(mut map) = routes().lock() {
            if !map.is_empty() {
                log_info!(
                    "[route-health] 网络指纹变化，丢弃 {} 个 host 的路由先验重学",
                    map.len()
                );
            }
            map.clear();
        }
        if let Ok(mut p) = pending().lock() {
            *p = None;
        }
    } else {
        // 离线启动后首次上线：这是磁盘先验唯一的运行时回读路径。
        adopt_pending(&fp);
    }
}

/// 采纳离线启动时暂存的磁盘先验（指纹相符才装表，否则丢弃）。
fn adopt_pending(fp: &str) {
    let stashed = pending().lock().ok().and_then(|mut p| p.take());
    let Some(file) = stashed else { return };
    if file.net != fp {
        log_info!("[route-health] 网络就绪但指纹与上次运行不符，暂存先验丢弃重学");
        return;
    }
    let now = now_unix_secs();
    let mut incoming = file.hosts;
    prune(&mut incoming, now);
    let loaded = incoming.len();
    if let Ok(mut map) = routes().lock() {
        for (host, entry) in incoming {
            map.entry(host).or_insert(entry);
        }
    }
    log_info!(
        "[route-health] 网络就绪，采纳暂存的 {} 个 host 路由先验",
        loaded
    );
}

/// 就地清除过期观察 + 容量裁剪（淘汰最旧）。
fn prune(map: &mut HashMap<String, HostRoute>, now: u64) {
    map.retain(|_, e| {
        if !fresh_within(e.proxy_ts, proxy_ttl_secs(e.proxy_n), now) {
            e.proxy_ts = 0;
            e.proxy_bps = 0.0;
            e.proxy_n = 0;
            e.win_ts = 0;
            e.confirm_ts = 0;
        }
        // cool 状态按 TTL（24h）保活**计数**：当前退避窗过后 host 即恢复
        // 采样资格（hint_for 的显式窗口判定负责抑制），但 cool_n 保留——
        // 再次无优势时退避才能真正升级（300s×2^n）。若在这里用当前窗口
        // 判过期，窗口一过计数即清零，指数退避永远停在第一档。
        if !fresh(e.cool_ts, now) {
            e.cool_ts = 0;
            e.cool_n = 0;
        }
        if !fresh(e.nosw_ts, now) {
            e.nosw_ts = 0;
        }
        e.proxy_ts != 0 || e.cool_ts != 0 || e.nosw_ts != 0
    });
    if map.len() > MAX_HOSTS {
        let newest = |e: &HostRoute| e.proxy_ts.max(e.cool_ts).max(e.nosw_ts);
        let mut ts_sorted: Vec<u64> = map.values().map(newest).collect();
        ts_sorted.sort_unstable();
        let cutoff = ts_sorted[ts_sorted.len() - MAX_HOSTS];
        let mut kept = 0usize;
        map.retain(|_, e| {
            if newest(e) >= cutoff && kept < MAX_HOSTS {
                kept += 1;
                true
            } else {
                false
            }
        });
    }
}

/// Engine 启动时从 config 表读回持久化先验（与 `load_cdn_health` 同一
/// 生命周期点调用）。版本/指纹不匹配、解析失败 → 空表重学。
pub(crate) async fn load(db: &Db) {
    load_with_fp(db, net_fingerprint()).await;
}

/// [`load`] 的指纹注入变体（测试确定性；生产路径恒经 [`load`]）。
async fn load_with_fp(db: &Db, fp: String) {
    if let Ok(mut cur) = net_epoch().lock() {
        *cur = fp.clone();
    }
    let raw = match db.get_config(ROUTE_CONFIG_KEY).await {
        Ok(Some(v)) => v,
        Ok(None) => return,
        Err(e) => {
            log_info!(
                "[route-health] 读取持久化路由先验失败（忽略，重新学习）: {}",
                e
            );
            return;
        }
    };
    let parsed: RouteFile = match serde_json::from_str(&raw) {
        Ok(f) => f,
        Err(e) => {
            log_info!("[route-health] 路由先验解析失败（丢弃重学）: {}", e);
            return;
        }
    };
    if parsed.v != ROUTE_FORMAT_VERSION {
        log_info!(
            "[route-health] 路由先验格式版本不匹配（{} != {}），整体丢弃重学",
            parsed.v,
            ROUTE_FORMAT_VERSION
        );
        return;
    }
    if fp.is_empty() {
        // 网络未就绪：无从校验指纹。暂存待 ensure_net_epoch 首次上线时
        // 比对采纳（开机自启早于网络就绪的常态路径）。
        if let Ok(mut p) = pending().lock() {
            *p = Some(parsed);
        }
        log_info!("[route-health] 网络未就绪，路由先验暂存待采纳");
        return;
    }
    if parsed.net != fp {
        log_info!("[route-health] 网络指纹与上次运行不符，路由先验整体丢弃重学");
        return;
    }
    let now = now_unix_secs();
    let mut incoming = parsed.hosts;
    prune(&mut incoming, now);
    let loaded = incoming.len();
    if let Ok(mut map) = routes().lock() {
        for (host, entry) in incoming {
            map.entry(host).or_insert(entry);
        }
    }
    log_info!("[route-health] 已加载 {} 个 host 的路由先验", loaded);
}

/// 查询某 host 的启动期路由提示（无观察/已过期/换网 → None）。
pub(crate) fn startup_hint(host: &str) -> Option<RouteHint> {
    ensure_net_epoch();
    let now = now_unix_secs();
    let map = routes().lock().ok()?;
    map.get(host).and_then(|e| hint_for(e, now))
}

/// 该 host 是否有未过期的 validator 不一致记录。forward failover 门禁
/// 消费：已知代理会命中不同 CDN edge 的 host，绝不许被 failover 推上
/// 代理续传（跨重启延续内存态 NoSwitch 的完整性防线）。
pub(crate) fn no_switch_active(host: &str) -> bool {
    ensure_net_epoch();
    let now = now_unix_secs();
    routes()
        .lock()
        .ok()
        .and_then(|map| map.get(host).map(|e| fresh(e.nosw_ts, now)))
        .unwrap_or(false)
}

/// 记录采样胜出：确认计分 + 实证时钟 + 写吞吐，清零冷却退避。低频且
/// 语义重要，立即落盘。
pub(crate) fn record_proxy_win(host: &str, probe_bps: f64, db: &Db) {
    ensure_net_epoch();
    let now = now_unix_secs();
    if let Ok(mut map) = routes().lock() {
        let e = map.entry(host.to_string()).or_default();
        bump_proxy_confirm(e, now);
        e.win_ts = now;
        e.proxy_bps = probe_bps;
        e.cool_n = 0;
        e.cool_ts = 0;
    }
    persist(db);
}

/// 记录采样无优势：退避计数 +1。立即落盘。
pub(crate) fn record_cooldown(host: &str, db: &Db) {
    ensure_net_epoch();
    let now = now_unix_secs();
    if let Ok(mut map) = routes().lock() {
        let e = map.entry(host.to_string()).or_default();
        e.cool_n = e.cool_n.saturating_add(1);
        e.cool_ts = now;
    }
    persist(db);
}

/// 记录 validator 不一致（完整性防线）。立即落盘。
pub(crate) fn record_no_switch(host: &str, db: &Db) {
    ensure_net_epoch();
    let now = now_unix_secs();
    if let Ok(mut map) = routes().lock() {
        map.entry(host.to_string()).or_default().nosw_ts = now;
    }
    persist(db);
}

/// 被动续期：经代理路由完成的任务证明该 host 的代理链路仍然可用——
/// 确认计分 + 刷新胜绩时间戳（「每天用则续期」的零成本实现，间隔
/// 超 12h 的续期同时累积确认天数）。任务完成是低频事件，立即落盘。
pub(crate) fn touch_proxy_route(host: &str, db: &Db) {
    ensure_net_epoch();
    let now = now_unix_secs();
    if let Ok(mut map) = routes().lock() {
        bump_proxy_confirm(map.entry(host.to_string()).or_default(), now);
    }
    persist(db);
}

/// 反向 failover：代理起飞的任务连接类失败 → 作废该 host 的代理先验
/// （胜绩/计分全清，冷却/NoSwitch 保留），重试回直连——杜绝「持久化
/// 代理决策 + 代理失效 → 无法自愈」的锁死。立即落盘。
pub(crate) fn clear_proxy_prior(host: &str, db: &Db) {
    ensure_net_epoch();
    if let Ok(mut map) = routes().lock()
        && let Some(e) = map.get_mut(host)
    {
        e.proxy_ts = 0;
        e.proxy_bps = 0.0;
        e.proxy_n = 0;
        e.win_ts = 0;
        e.confirm_ts = 0;
    }
    persist(db);
}

/// 代理设置变更时全表作废（内存 + 持久化 + 离线暂存）：所有先验都是对
/// 旧候选代理/旧出口的观察（指纹只覆盖系统代理，手动字段变更不换
/// epoch），与 `clear_domain_conn_caps`、`DecisionCache::clear` 同点位
/// 调用。
pub(crate) fn clear_all(db: &Db) {
    if let Ok(mut p) = pending().lock() {
        *p = None;
    }
    if let Ok(mut map) = routes().lock() {
        if !map.is_empty() {
            log_info!(
                "[route-health] 代理设置变更，作废 {} 个 host 的路由先验",
                map.len()
            );
        }
        map.clear();
    }
    // 绕过 persist 的空 epoch 守卫直接写空表：无论网络是否就绪，磁盘上
    // 针对旧候选代理的先验都必须作废（net 为空的空表下次 load 同样会被
    // 丢弃，语义一致）。
    let net = net_epoch().lock().map(|c| c.clone()).unwrap_or_default();
    spawn_write(
        db,
        &RouteFile {
            v: ROUTE_FORMAT_VERSION,
            net,
            hosts: HashMap::new(),
        },
    );
}

/// 序列化并异步写盘。晚创建的快照作废早创建的：写任务落盘前校验自己
/// 仍是最新序列，过期即弃——防「作废写」（clear_*）被更早排队的常规
/// 快照乱序盖回。
fn spawn_write(db: &Db, file: &RouteFile) {
    let Ok(json) = serde_json::to_string(file) else {
        return;
    };
    let seq = PERSIST_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
    let db = db.clone();
    tokio::spawn(async move {
        if PERSIST_SEQ.load(Ordering::Relaxed) != seq {
            return; // 已有更新的快照排队，本次写作废。
        }
        if let Err(e) = db.set_config(ROUTE_CONFIG_KEY, &json).await {
            log_info!("[route-health] 路由先验持久化失败（忽略）: {}", e);
        }
    });
}

/// 把当前缓存快照异步写回 config 表（fire-and-forget；顺带 prune）。
/// 网络未就绪（epoch 为空）时不写——不许用离线会话的空表覆盖磁盘上
/// 仍然有效的先验。
fn persist(db: &Db) {
    let net = {
        let Ok(cur) = net_epoch().lock() else { return };
        cur.clone()
    };
    if net.is_empty() {
        return;
    }
    let snapshot = {
        let Ok(mut map) = routes().lock() else { return };
        prune(&mut map, now_unix_secs());
        map.clone()
    };
    let file = RouteFile {
        v: ROUTE_FORMAT_VERSION,
        net,
        hosts: snapshot,
    };
    spawn_write(db, &file);
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::{
        ADOPT_MIN_CONFIRMS, COOLDOWN_BASE_SECS, HostRoute, MAX_HOSTS, PROXY_CONFIRM_GAP_SECS,
        PROXY_TTL_MAX_SECS, ROUTE_FORMAT_VERSION, ROUTE_TTL, RouteFile, RouteHint,
        WIN_REVALIDATE_SECS, bump_proxy_confirm, cooldown_window_secs, fingerprint_of, fresh,
        hint_for, proxy_ttl_secs, prune,
    };
    use std::collections::HashMap;

    const NOW: u64 = 2_000_000_000;

    #[test]
    fn cooldown_backoff_doubles_and_caps() {
        assert_eq!(cooldown_window_secs(0), COOLDOWN_BASE_SECS);
        assert_eq!(cooldown_window_secs(1), 600, "首败 600s");
        assert_eq!(cooldown_window_secs(3), 2400);
        assert_eq!(cooldown_window_secs(8), 76800, "2^8 封顶前最大档");
        assert_eq!(cooldown_window_secs(20), 76800, "指数封顶不溢出");
        assert!(cooldown_window_secs(u32::MAX) <= ROUTE_TTL.as_secs());
    }

    #[test]
    fn hint_precedence_nosw_over_cool_over_proxy() {
        let all = HostRoute {
            proxy_ts: NOW,
            proxy_bps: 1e6,
            proxy_n: 1,
            confirm_ts: NOW,
            cool_n: 1,
            cool_ts: NOW,
            nosw_ts: NOW,
            win_ts: NOW,
        };
        assert_eq!(hint_for(&all, NOW), Some(RouteHint::SuppressProbe));
        let cool_and_proxy = HostRoute { nosw_ts: 0, ..all };
        assert_eq!(
            hint_for(&cool_and_proxy, NOW),
            Some(RouteHint::SuppressProbe),
            "冷却抑制优先于加速信号"
        );
        let proxy_only = HostRoute {
            cool_n: 0,
            cool_ts: 0,
            nosw_ts: 0,
            ..all
        };
        assert_eq!(hint_for(&proxy_only, NOW), Some(RouteHint::FastReeval));
        assert_eq!(hint_for(&HostRoute::default(), NOW), None);
    }

    #[test]
    fn cooldown_window_expiry_falls_through_to_proxy_hint() {
        // 冷却窗（n=1 → 600s）过后：冷却失效，代理胜绩（TTL 内）接管。
        let e = HostRoute {
            proxy_ts: NOW - 700,
            proxy_bps: 1e6,
            proxy_n: 1,
            confirm_ts: NOW - 700,
            cool_n: 1,
            cool_ts: NOW - 700,
            nosw_ts: 0,
            win_ts: NOW - 700,
        };
        assert_eq!(hint_for(&e, NOW), Some(RouteHint::FastReeval));
        // 窗内仍抑制。
        assert_eq!(hint_for(&e, NOW - 200), Some(RouteHint::SuppressProbe));
    }

    #[test]
    fn hint_respects_ttl() {
        let stale = HostRoute {
            proxy_ts: NOW - ROUTE_TTL.as_secs() - 1,
            nosw_ts: NOW - ROUTE_TTL.as_secs() - 1,
            ..HostRoute::default()
        };
        assert_eq!(hint_for(&stale, NOW), None, "过期观察不可见");
        assert!(!fresh(0, NOW), "0 = 无记录");
    }

    #[test]
    fn prune_drops_stale_and_caps_hosts() {
        let mut map: HashMap<String, HostRoute> = HashMap::new();
        map.insert(
            "stale.example".into(),
            HostRoute {
                proxy_ts: NOW - ROUTE_TTL.as_secs() - 1,
                ..HostRoute::default()
            },
        );
        for n in 0..(MAX_HOSTS + 8) {
            map.insert(
                format!("h{n}.example"),
                HostRoute {
                    proxy_ts: NOW - n as u64,
                    ..HostRoute::default()
                },
            );
        }
        prune(&mut map, NOW);
        assert!(map.len() <= MAX_HOSTS);
        assert!(!map.contains_key("stale.example"), "过期条目必须被清除");
        assert!(map.contains_key("h0.example"), "最新条目必须保留");
    }

    #[test]
    fn prune_keeps_cooldown_counter_within_ttl() {
        // 当前退避窗（n=2 → 1200s）已过但仍在 24h 内：计数保留——
        // 下次无优势时退避才能升级到 2400s，而不是永远停在第一档。
        let mut map: HashMap<String, HostRoute> = HashMap::new();
        map.insert(
            "cool.example".into(),
            HostRoute {
                cool_n: 2,
                cool_ts: NOW - cooldown_window_secs(2) - 1,
                proxy_ts: NOW, // 让条目整体存活
                ..HostRoute::default()
            },
        );
        prune(&mut map, NOW);
        let e = &map["cool.example"];
        assert_eq!(e.cool_n, 2, "窗口过后计数保留（24h 内）");
        // hint 侧此时已不再抑制（窗口判定在 hint_for）。
        assert_eq!(hint_for(e, NOW), Some(RouteHint::FastReeval));
        // 24h 无新败绩才整段清零。
        if let Some(e) = map.get_mut("cool.example") {
            e.cool_ts = NOW - ROUTE_TTL.as_secs() - 1;
        }
        prune(&mut map, NOW);
        let e = &map["cool.example"];
        assert_eq!(e.cool_n, 0, "超 24h 退避状态整段清零");
        assert_eq!(e.cool_ts, 0);
    }

    #[test]
    fn proxy_ttl_ladder_scales_with_confirms() {
        assert_eq!(proxy_ttl_secs(0), ROUTE_TTL.as_secs(), "无计分退化 24h");
        assert_eq!(proxy_ttl_secs(1), 24 * 3600);
        assert_eq!(proxy_ttl_secs(2), 48 * 3600);
        assert_eq!(proxy_ttl_secs(3), 96 * 3600);
        assert_eq!(proxy_ttl_secs(4), PROXY_TTL_MAX_SECS, "≥4 天封顶 7 天");
        assert_eq!(proxy_ttl_secs(u32::MAX), PROXY_TTL_MAX_SECS);
    }

    #[test]
    fn adopt_requires_multi_day_confirms() {
        let one_day = HostRoute {
            proxy_ts: NOW,
            proxy_n: 1,
            win_ts: NOW,
            ..HostRoute::default()
        };
        assert_eq!(
            hint_for(&one_day, NOW),
            Some(RouteHint::FastReeval),
            "单日胜绩只加速，不代理起飞"
        );
        let confirmed = HostRoute {
            proxy_n: ADOPT_MIN_CONFIRMS,
            ..one_day
        };
        assert_eq!(
            hint_for(&confirmed, NOW),
            Some(RouteHint::AdoptProxy),
            "≥2 天确认直接代理起飞"
        );
        // 阶梯 TTL：n=2 的胜绩 30h 前记录仍然有效（>24h、<48h）。
        let aged = HostRoute {
            proxy_ts: NOW - 30 * 3600,
            ..confirmed
        };
        assert_eq!(hint_for(&aged, NOW), Some(RouteHint::AdoptProxy));
        // 超出自身档位即失效。
        let dead = HostRoute {
            proxy_ts: NOW - 49 * 3600,
            ..confirmed
        };
        assert_eq!(hint_for(&dead, NOW), None);
    }

    #[test]
    fn adopt_demotes_without_recent_verified_win() {
        // 完成续期把 proxy_ts 维持新鲜，但 win_ts（实证胜出）已超 72h：
        // 降档 FastReeval 重验直连——防「代理够用就永不回头」的锁死。
        let renewed_only = HostRoute {
            proxy_ts: NOW,
            proxy_n: 4,
            win_ts: NOW - WIN_REVALIDATE_SECS - 1,
            ..HostRoute::default()
        };
        assert_eq!(hint_for(&renewed_only, NOW), Some(RouteHint::FastReeval));
        // 实证仍新鲜 → 继续代理起飞。
        let verified = HostRoute {
            win_ts: NOW - WIN_REVALIDATE_SECS + 3600,
            ..renewed_only
        };
        assert_eq!(hint_for(&verified, NOW), Some(RouteHint::AdoptProxy));
    }

    #[test]
    fn bump_confirm_uses_credit_clock_not_renewal_gap() {
        // 续期节奏取计分窗的一半:相邻两次续期永远 < 窗口,但第三次距上次
        // 计分已 1.5 倍窗口。基准若错挂在 proxy_ts 上,最常用的 host 反而
        // 永远攒不够确认次数、到不了 AdoptProxy。
        let renew_gap = PROXY_CONFIRM_GAP_SECS / 2;
        let mut e = HostRoute::default();
        bump_proxy_confirm(&mut e, NOW);
        assert_eq!(e.proxy_n, 1, "首次确认 +1");
        bump_proxy_confirm(&mut e, NOW + renew_gap);
        bump_proxy_confirm(&mut e, NOW + 2 * renew_gap);
        assert_eq!(e.proxy_n, 1, "距上次计分未过窗口不加分");
        assert_eq!(e.proxy_ts, NOW + 2 * renew_gap, "续期必须刷新时间戳");
        bump_proxy_confirm(&mut e, NOW + 3 * renew_gap);
        assert_eq!(e.proxy_n, 2, "计分窗到点即 +1,不被高频续期推迟");
        assert_eq!(e.confirm_ts, NOW + 3 * renew_gap, "计分时钟随计分推进");
    }

    #[test]
    fn future_timestamps_are_invalid() {
        // 时钟回拨后 recorded > now：按无效观察处理，绝不恒新鲜。
        assert!(!fresh(NOW + 10, NOW));
        let pinned = HostRoute {
            proxy_ts: NOW + 3600,
            proxy_n: 4,
            win_ts: NOW + 3600,
            ..HostRoute::default()
        };
        assert_eq!(hint_for(&pinned, NOW), None, "未来时间戳不产生任何提示");
    }

    #[test]
    fn file_roundtrip_preserves_entries() {
        let mut hosts = HashMap::new();
        hosts.insert(
            "gh.example".to_string(),
            HostRoute {
                proxy_ts: NOW,
                proxy_bps: 2.5e6,
                proxy_n: 3,
                confirm_ts: NOW - 20,
                cool_n: 1,
                cool_ts: NOW - 10,
                nosw_ts: 0,
                win_ts: NOW - 20,
            },
        );
        let file = RouteFile {
            v: ROUTE_FORMAT_VERSION,
            net: fingerprint_of("proxy:8080", "192.168.1.5"),
            hosts,
        };
        let json = serde_json::to_string(&file).unwrap();
        let back: RouteFile = serde_json::from_str(&json).unwrap();
        assert_eq!(back.v, ROUTE_FORMAT_VERSION);
        assert_eq!(back.net, file.net);
        let e = &back.hosts["gh.example"];
        assert_eq!(e.proxy_ts, NOW);
        assert_eq!(e.cool_n, 1);
    }

    #[test]
    fn fingerprint_is_deterministic_and_input_sensitive() {
        let a = fingerprint_of("p:1", "192.168.1.5");
        assert_eq!(a, fingerprint_of("p:1", "192.168.1.5"));
        assert_ne!(
            a,
            fingerprint_of("p:2", "192.168.1.5"),
            "代理变化必须换 epoch"
        );
        assert_ne!(
            a,
            fingerprint_of("p:1", "10.0.0.3"),
            "LAN IP 变化必须换 epoch"
        );
        assert_eq!(a.len(), 16, "16 hex = 8 字节截断");
        // 边界组合不得同构：("ab","") vs ("a","b")。
        assert_ne!(fingerprint_of("ab", ""), fingerprint_of("a", "b"));
    }

    #[test]
    fn missing_fields_deserialize_as_no_observation() {
        // 旧条目/手改数据缺字段 → serde(default) 兜底，不炸整表。
        let e: HostRoute = serde_json::from_str(r#"{"proxy_ts":123}"#).unwrap();
        assert_eq!(e.proxy_ts, 123);
        assert_eq!(e.cool_n, 0);
        assert_eq!(e.nosw_ts, 0);
    }

    /// 组合契约：net 指纹不符 → 整表丢弃；相符 → 条目可查。经真实
    /// config 表往返（in-memory sqlite）。
    #[tokio::test]
    async fn load_respects_net_epoch() {
        use super::{ROUTE_CONFIG_KEY, load_with_fp, now_unix_secs, routes};
        let db = crate::db::Db::connect("sqlite::memory:")
            .await
            .expect("mem db");
        let mut hosts = HashMap::new();
        hosts.insert(
            "load-epoch.example".to_string(),
            HostRoute {
                proxy_ts: now_unix_secs(),
                ..HostRoute::default()
            },
        );
        let file = RouteFile {
            v: ROUTE_FORMAT_VERSION,
            net: "net-a".to_string(),
            hosts,
        };
        db.set_config(ROUTE_CONFIG_KEY, &serde_json::to_string(&file).unwrap())
            .await
            .expect("seed config");

        load_with_fp(&db, "net-b".to_string()).await;
        assert!(
            !routes().lock().unwrap().contains_key("load-epoch.example"),
            "指纹不符必须整表丢弃"
        );

        load_with_fp(&db, "net-a".to_string()).await;
        assert!(
            routes().lock().unwrap().contains_key("load-epoch.example"),
            "指纹相符必须加载条目"
        );
    }
}
