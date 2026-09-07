//! [`LinkManager`] —— 设备互联子系统门面。
//!
//! 聚合身份、名册存储、配对协议（响应方 + 发起方）、mDNS 发现、可扩展传输栈，
//! 供宿主（hub 桌面 / headless server）驱动。宿主只跟本门面 + 一个事件通道打交道。
//!
//! # 角色
//! - **响应方**（被添加设备）：生成配对码、处理 `hello`/`confirm`、mDNS 广播。
//! - **发起方**（正在添加设备）：mDNS 浏览、`begin_pairing`（发 hello、算 SAS）、
//!   `confirm_pairing`（发 confirm、落库）。
//! - **数据面**：`dispatch`（把下载下发给已配对设备，先 AEAD 加密再走传输栈）、
//!   `authorize`（校验入站链路请求的 HMAC 鉴权并解密）。

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use tokio::sync::mpsc;

use super::crypto::{
    LINK_AUTH_SKEW_SECS, derive_link_aead_key, link_auth_tag, open_link_body, seal_link_body,
    verify_link_auth_tag,
};
use super::discovery::{self, MdnsAdvertiser, MdnsBrowser};
use super::error::{LinkError, LinkResult};
use super::identity::{IDENTITY_CONFIG_KEY, LinkIdentity};
use super::pairing::{HelloRequest, HelloResponse, PairingInitiator, PairingResponder, SelfInfo};
use super::store::LinkStore;
use super::transport::TransportStack;
use super::types::{DiscoveredPeer, PeerCandidate, PeerRecord, TransportKind};
use crate::db::Db;

/// 引擎侧设备互联事件（宿主消费：hub 转 rinf 信号，server 可广播 WS）。
#[derive(Debug, Clone)]
pub enum LinkEngineEvent {
    /// mDNS/手动发现到一台设备。
    Discovered(DiscoveredPeer),
    /// 一台设备完成配对并入册（含 link_secret，宿主转 UI 前须剥除敏感字段）。
    Paired(PeerRecord),
    /// 一台设备被解除配对（fingerprint）。
    Unpaired(String),
    /// 有设备正在向本机发起配对，等待本机用户核对 SAS 后批准/拒绝。
    IncomingPairing {
        session_id: String,
        sas: String,
        peer_name: String,
        peer_platform: Option<String>,
    },
    /// 子系统错误（供 UI 提示）。
    Error(String),
}

/// 响应方处理一次入站 `confirm` 的终局。四种都是协议的正常结果，**不是错误**——
/// 发起方要据此给出准确提示，不能一律显示「会话过期」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairConfirmOutcome {
    /// 本机用户批准，发起方已入册。
    Paired,
    /// 发起方自己传了 `confirm=false`。
    Declined,
    /// 本机用户核对 SAS 后拒绝。
    Rejected,
    /// 等待本机用户核验超时（60s 决策窗口耗尽）。
    TimedOut,
}

impl PairConfirmOutcome {
    /// 是否真的完成了配对。
    #[must_use]
    pub fn paired(self) -> bool {
        matches!(self, Self::Paired)
    }

    /// 供 HTTP 响应体透出的稳定判别串（发起方据此还原语义）；`Paired`/`Declined`
    /// 无需额外理由。
    #[must_use]
    pub fn reason(self) -> Option<&'static str> {
        match self {
            Self::Rejected => Some("rejected"),
            Self::TimedOut => Some("timeout"),
            Self::Paired | Self::Declined => None,
        }
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 发现快照去重键：优先用指纹（mDNS TXT 记录携带，稳定）；探测阶段指纹未知
/// 时退化到 `host:port`。
fn discovered_peer_key(p: &DiscoveredPeer) -> String {
    match &p.fingerprint {
        Some(fp) => fp.clone(),
        None => format!("{}:{}", p.host, p.port),
    }
}

/// 按 [`discovered_peer_key`] 去重更新发现快照：已存在则原地覆盖（刷新地址/
/// 名称等可能变化的字段），否则追加。
fn upsert_discovered(snapshot: &mut Vec<DiscoveredPeer>, peer: DiscoveredPeer) {
    let key = discovered_peer_key(&peer);
    match snapshot.iter_mut().find(|e| discovered_peer_key(e) == key) {
        Some(existing) => *existing = peer,
        None => snapshot.push(peer),
    }
}

/// 发起方待确认会话（[`PendingInit`]）存活上限。**必须**与响应方
/// `pairing.rs` 的 `SESSION_TTL`（180s）保持一致——双端各自独立维护一套
/// 过期判定，此前这个 180 是硬编码在 [`LinkManager::begin_pairing`] 剪枝
/// 调用里的魔法数，[`LinkManager::confirm_pairing`] 需要同一个值做本地
/// 过期检查（此前它完全不做本地过期检查，只依赖对端 `SESSION_TTL` 兜
/// 底），因此提升为具名常量集中定义，未来两端 TTL 一起改，不会各自漂移。
const PENDING_INIT_TTL: std::time::Duration = std::time::Duration::from_secs(180);

/// 已配对设备 Direct 候选端点集合的总数上限。mDNS 重新发现命中已配对
/// 指纹时会把新地址去重后插到候选表首位、旧候选整体保留作回退（见
/// `LinkManager::start_discovery` 的转发任务），若不设上限，设备长期
/// 在多个网络间漂移会让候选表无界增长。取一个小值：日常场景「当前
/// 网段 + 一两个历史网段」远用不到 4 个，仍能覆盖「NAS 同时插着物理
/// 网卡和虚拟机 host-only 网卡」这类多网卡场景的合理候选数。
const MAX_DIRECT_CANDIDATES: usize = 4;

/// 发起方待确认会话（begin_pairing 与 confirm_pairing 之间的状态）。
struct PendingInit {
    initiator: PairingInitiator,
    session_id: String,
    peer_host: String,
    peer_port: u16,
    created: std::time::Instant,
}

/// 设备互联门面。宿主持 `Arc<LinkManager>`。
pub struct LinkManager {
    identity: LinkIdentity,
    self_info: SelfInfo,
    store: LinkStore,
    responder: PairingResponder,
    transport: TransportStack,
    client: reqwest::Client,
    api_port: u16,
    events: mpsc::Sender<LinkEngineEvent>,
    advertiser: Mutex<Option<MdnsAdvertiser>>,
    browser: Mutex<Option<MdnsBrowser>>,
    pending: Mutex<HashMap<String, PendingInit>>,
    /// 数据面防重放：时窗内已见过的 `(device:nonce, ts)`，authorize 剪枝保持有界。
    ///
    /// **纯内存态，无持久化**：进程重启即清空全部历史。残余风险——若攻击者
    /// 截获过一份合法的 `(ts, nonce, tag)`，理论上可在响应方重启后、原始
    /// `ts` 仍落在 `LINK_AUTH_SKEW_SECS`（120s）容忍窗口内时重放一次。攻击
    /// 窗口极窄（需精确命中「重启后 120s 内」这个时机，且重启本身不频繁），
    /// 判定为可接受风险：加持久化的工程成本（落盘 + 跨重启一致性）远大于
    /// 收窄这个窗口带来的收益。这是权衡后的结论，不是遗漏。
    seen_nonces: Mutex<Vec<(String, i64)>>,
    /// 发现快照（发起方侧）：`start_discovery` 转发任务里按 [`upsert_discovered`]
    /// 去重更新；`start_discovery` 调用时清空；`probe` 的结果不入此快照。
    /// `Arc` 包裹以便转发任务（`tokio::spawn` 的 `'static` 闭包）持有写句柄。
    discovered: Arc<Mutex<Vec<DiscoveredPeer>>>,
}

impl LinkManager {
    /// 从引擎数据库加载（或首次生成并持久化）本机身份，构造门面。
    ///
    /// `api_port` = 本机 fluxdown API 端口（mDNS 广播 + 自报候选用）。
    pub async fn load(
        db: Db,
        self_info: SelfInfo,
        api_port: u16,
        events: mpsc::Sender<LinkEngineEvent>,
    ) -> LinkResult<Arc<Self>> {
        let identity = Self::load_or_create_identity(&db).await?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let responder = PairingResponder::new(identity.clone(), self_info.clone());
        let transport = TransportStack::direct_only(client.clone());
        let mgr = Arc::new(Self {
            identity,
            self_info,
            store: LinkStore::new(db),
            responder,
            transport,
            client,
            api_port,
            events,
            advertiser: Mutex::new(None),
            browser: Mutex::new(None),
            pending: Mutex::new(HashMap::new()),
            seen_nonces: Mutex::new(Vec::new()),
            discovered: Arc::new(Mutex::new(Vec::new())),
        });
        Self::spawn_gc(&mgr);
        Ok(mgr)
    }

    /// 启动后台周期清理任务：每 60s 剪枝 `pending`/`seen_nonces`，并转发给
    /// 响应方剪枝其 `codes`/`sessions`（见
    /// [`super::pairing::PairingResponder::prune_expired`]）。此前这几张
    /// 状态表都只在「恰好又发生一次同类调用」时顺带清理，半途放弃的配对/
    /// 过期 nonce 记录会一直驻留到进程重启才被回收。
    ///
    /// 持 `Weak` 而非 `Arc`：循环体自身绝不能成为 `LinkManager` 存活的
    /// 理由——宿主释放最后一个 `Arc` 后，下一次 `upgrade()` 必然失败，循环
    /// 随之退出；否则这个 `'static` 后台任务会永久多攥一份 `Arc`，
    /// `LinkManager`（及其持有的 `advertiser`/`browser` 等资源）就再也
    /// 不会被析构。
    fn spawn_gc(mgr: &Arc<Self>) {
        let weak = Arc::downgrade(mgr);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                tick.tick().await;
                let Some(mgr) = weak.upgrade() else {
                    break;
                };
                mgr.prune_expired();
            }
        });
    }

    async fn load_or_create_identity(db: &Db) -> LinkResult<LinkIdentity> {
        if let Some(b64) = db.get_config(IDENTITY_CONFIG_KEY).await?
            && let Ok(bytes) = B64.decode(b64.trim())
            && let Ok(seed) = <[u8; 32]>::try_from(bytes.as_slice())
        {
            return Ok(LinkIdentity::from_secret_bytes(&seed));
        }
        let identity = LinkIdentity::generate();
        db.set_config(IDENTITY_CONFIG_KEY, &B64.encode(identity.secret_bytes()))
            .await?;
        Ok(identity)
    }

    /// 本机设备指纹（设备 ID）。
    #[must_use]
    pub fn fingerprint(&self) -> &str {
        self.identity.fingerprint()
    }

    /// 本机展示名（供 `/ping` 透出）。
    #[must_use]
    pub fn self_name(&self) -> &str {
        &self.self_info.name
    }

    /// 本机平台（供 `/ping` 透出）。
    #[must_use]
    pub fn self_platform(&self) -> Option<&str> {
        self.self_info.platform.as_deref()
    }

    // ── 响应方（被添加设备侧）─────────────────────────────────────────────

    /// 生成一次性配对码（在被添加设备 UI 展示），并确保 mDNS 广播已开启。
    pub fn generate_code(&self) -> String {
        self.ensure_advertising();
        self.responder.generate_code()
    }

    /// 处理入站 `hello`（HTTP 层解码 base64 后调用）。`source`：请求来源
    /// 地址，直接透传给 [`PairingResponder::handle_hello`] 做按来源分桶
    /// 节流；拿不到时传 `None`。
    pub fn pair_hello(
        &self,
        req: HelloRequest,
        source: Option<IpAddr>,
    ) -> LinkResult<HelloResponse> {
        let resp = self.responder.handle_hello(&req, source)?;
        self.emit_incoming_pairing(resp.session_id.clone(), resp.sas.clone(), &req);
        Ok(resp)
    }

    /// 处理入站 `hello`（wire 形式，base64 编解码全在引擎内完成，宿主纯字段搬运）。
    /// `source`：请求来源地址，同 [`Self::pair_hello`]。
    pub fn pair_hello_wire(
        &self,
        w: WireHello,
        source: Option<IpAddr>,
    ) -> LinkResult<WireHelloResponse> {
        let req = HelloRequest {
            code: w.code,
            initiator_eph_pub: decode_b64_array::<32>(&w.initiator_eph_pub)?,
            initiator_id_pub: decode_b64_array::<32>(&w.initiator_id_pub)?,
            initiator_sig: decode_b64_array::<64>(&w.initiator_sig)?,
            name: w.name,
            platform: w.platform,
            app_version: w.app_version,
            initiator_addrs: w.initiator_addrs,
        };
        let resp = self.responder.handle_hello(&req, source)?;
        self.emit_incoming_pairing(resp.session_id.clone(), resp.sas.clone(), &req);
        Ok(WireHelloResponse {
            session_id: resp.session_id,
            responder_eph_pub: B64.encode(resp.responder_eph_pub),
            responder_id_pub: B64.encode(resp.responder_id_pub),
            responder_sig: B64.encode(resp.responder_sig),
            name: resp.name,
            platform: resp.platform,
            app_version: resp.app_version,
            sas: resp.sas,
        })
    }

    /// 处理入站 `confirm`：本机用户批准则把发起方入册并广播 `Paired` 事件。
    ///
    /// 返回 [`PairConfirmOutcome`] 而非 `bool`：本机用户**拒绝**与**核验超时**都是
    /// 协议的正常终局（不是服务端错误），必须能被发起方区分。此前它们经 `Err` 冒泡成
    /// HTTP 400，发起方的 [`Self::confirm_pairing`] 见非 2xx 一律映射成
    /// `SessionExpired`，用户看到的是「会话过期」而不是「对方拒绝了配对」。
    pub async fn pair_confirm(
        &self,
        session_id: &str,
        confirm: bool,
    ) -> LinkResult<PairConfirmOutcome> {
        match self.responder.handle_confirm(session_id, confirm).await {
            Ok(Some(record)) => {
                self.store.upsert(&record).await?;
                let _ = self.events.send(LinkEngineEvent::Paired(record)).await;
                Ok(PairConfirmOutcome::Paired)
            }
            // 发起方自己传了 confirm=false —— 它当然知道自己拒绝了，无需额外语义。
            Ok(None) => Ok(PairConfirmOutcome::Declined),
            Err(LinkError::RejectedByPeer) => Ok(PairConfirmOutcome::Rejected),
            Err(LinkError::PairingTimeout) => Ok(PairConfirmOutcome::TimedOut),
            Err(e) => Err(e),
        }
    }

    /// 批准/拒绝一次入站配对请求（本机用户核对 SAS 后调用）。转发到响应方
    /// 的本地决策记录，唤醒 [`super::pairing::PairingResponder::handle_confirm`]
    /// 里等待用户核验、且已收到发起方 `confirm=true` 的那次 HTTP 请求。
    pub fn approve_incoming(&self, session_id: &str, accept: bool) -> LinkResult<()> {
        self.responder.set_local_decision(session_id, accept)
    }

    /// 后台广播「有入站配对待核对」事件。`events` 是 async `mpsc::Sender`，
    /// 本方法的调用方 `pair_hello`/`pair_hello_wire` 都是同步 fn，沿用
    /// [`Self::ensure_advertising`] 已有的 `tokio::spawn` 写法把发送挪到
    /// 后台，不阻塞 hello 的返回路径。
    fn emit_incoming_pairing(&self, session_id: String, sas: String, req: &HelloRequest) {
        let tx = self.events.clone();
        let event = LinkEngineEvent::IncomingPairing {
            session_id,
            sas,
            peer_name: req.name.clone(),
            peer_platform: req.platform.clone(),
        };
        tokio::spawn(async move {
            let _ = tx.send(event).await;
        });
    }

    /// 确保 mDNS 广播运行（幂等）。失败仅记 Error 事件，不阻断配对（手动地址可兜底）。
    fn ensure_advertising(&self) {
        let mut guard = match self.advertiser.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if guard.is_some() {
            return;
        }
        match MdnsAdvertiser::start(
            self.api_port,
            self.identity.fingerprint(),
            &self.self_info.name,
            self.self_info.platform.as_deref(),
            self.self_info.app_version.as_deref(),
        ) {
            Ok(a) => *guard = Some(a),
            Err(e) => {
                let tx = self.events.clone();
                let msg = e.to_string();
                tokio::spawn(async move {
                    let _ = tx.send(LinkEngineEvent::Error(msg)).await;
                });
            }
        }
    }

    /// 主动开启 mDNS 广播（宿主在启用本地互联时调用）。
    pub fn start_advertising(&self) {
        self.ensure_advertising();
    }

    /// 停止 mDNS 广播（配对码过期 / 用户关闭配对界面时调用）。与
    /// [`Self::stop_discovery`] 对称：持锁置 `None`，[`MdnsAdvertiser`] 的
    /// `Drop` 负责实际 shutdown；此前只有 `ensure_advertising`/
    /// `start_advertising`，广播一旦开启就持续到进程退出，本机指纹/设备名
    /// 一直暴露在局域网上，即使用户已经关掉了配对界面。
    pub fn stop_advertising(&self) {
        if let Ok(mut guard) = self.advertiser.lock() {
            *guard = None; // Drop → daemon.shutdown()
        }
    }

    // ── 发现（发起方侧）───────────────────────────────────────────────────

    /// 开始 mDNS 浏览：发现的设备经事件通道以 `Discovered` 汇出，并同步去重
    /// 更新发现快照。幂等——重复调用不重启浏览，但仍清空发现快照，与管理面
    /// `POST /link/discovery {"action":"start"}` 的语义对齐。
    /// 命中已配对指纹的广播不入快照/不发事件，而是刷新该设备的 Direct 候选
    /// 地址（见下）。
    pub fn start_discovery(&self) -> LinkResult<()> {
        if let Ok(mut snapshot) = self.discovered.lock() {
            snapshot.clear();
        }
        let mut guard = self.browser.lock().map_err(|_| LinkError::Unavailable)?;
        if guard.is_some() {
            return Ok(());
        }
        let (tx, mut rx) = mpsc::channel::<DiscoveredPeer>(64);
        let out = self.events.clone();
        let self_fp = self.identity.fingerprint().to_string();
        let discovered = Arc::clone(&self.discovered);
        let store = self.store.clone();
        tokio::spawn(async move {
            while let Some(peer) = rx.recv().await {
                // 过滤掉本机自身的广播。
                if peer.fingerprint.as_deref() == Some(self_fp.as_str()) {
                    continue;
                }
                // 命中已配对名册：多半是设备 DHCP 换了 IP 后重新广播——刷新
                // 其 Direct 候选地址，不当作「可添加的新设备」推进 discovered
                // 快照/事件通道（否则已配对设备会污染「发现列表」UI）。
                if let Some(fp) = peer.fingerprint.as_deref()
                    && let Ok(Some(record)) = store.get(fp).await
                {
                    let fresh = PeerCandidate {
                        kind: TransportKind::Direct,
                        address: format!("{}:{}", peer.host, peer.port),
                    };
                    // 保留旧候选作回退，只把新地址去重后放到首位（优先试
                    // 新的，试不通还有旧的）——mDNS 地址来自 `pick_best_v4`
                    // 的启发式排序，从未被真正探测过（既没 /ping 也没比
                    // 指纹，见该函数文档），不能当作权威结果直接覆盖配对
                    // 时验证过的旧候选：一次选错地址就会把唯一可达的候选
                    // 永久顶掉。
                    let old_candidates = record.candidates;
                    let mut candidates = Vec::with_capacity(old_candidates.len() + 1);
                    candidates.push(fresh.clone());
                    candidates.extend(old_candidates.iter().filter(|c| **c != fresh).cloned());
                    candidates.truncate(MAX_DIRECT_CANDIDATES);
                    // 候选集合确实没变时跳过写库——mDNS 广播会频繁重放同一
                    // 地址，避免无谓的写放大。也不因广播就顺带刷新
                    // last_seen_at（`link_update_candidates` 已拆分为只写
                    // candidates 列）：mDNS 广播只证明「对端在广播」，不
                    // 证明「本机刚和它说上话」，与 `touch()`「拨通了才算
                    // 在线」的语义矛盾。
                    if candidates != old_candidates {
                        let _ = store.update_candidates(fp, &candidates).await;
                    }
                    continue;
                }
                if let Ok(mut snapshot) = discovered.lock() {
                    upsert_discovered(&mut snapshot, peer.clone());
                }
                if out.send(LinkEngineEvent::Discovered(peer)).await.is_err() {
                    break;
                }
            }
        });
        *guard = Some(MdnsBrowser::start(tx)?);
        Ok(())
    }

    /// 停止 mDNS 浏览。
    pub fn stop_discovery(&self) {
        if let Ok(mut guard) = self.browser.lock() {
            *guard = None; // Drop → daemon.shutdown()
        }
    }

    /// 当前发现快照（发起方侧 UI 轮询用）；`start_discovery` 时清空，浏览期间
    /// 持续去重更新。
    #[must_use]
    pub fn discovered_peers(&self) -> Vec<DiscoveredPeer> {
        self.discovered
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// 手动地址探测（mDNS 失效兜底）：`/ping` 一台设备，返回其信息（不配对）。
    pub async fn probe(&self, host: &str, port: u16) -> LinkResult<DiscoveredPeer> {
        discovery::probe(&self.client, host, port).await
    }

    // ── 配对（发起方侧）───────────────────────────────────────────────────

    /// 发起配对：向 `host:port` 发送 `hello`（带配对码），返回 `(token, sas, 对端名)`。
    /// UI 展示 SAS 供用户与对端核对，随后调 [`confirm_pairing`]。
    pub async fn begin_pairing(
        &self,
        host: &str,
        port: u16,
        code: &str,
    ) -> LinkResult<BeginPairingResult> {
        let mut initiator = PairingInitiator::new(self.identity.clone());
        let addrs = discovery::local_direct_addrs(host, self.api_port);
        let hello = initiator.build_hello(code, &self.self_info, addrs);

        let body = serde_json::json!({
            "code": hello.code,
            "initiatorEphPub": B64.encode(hello.initiator_eph_pub),
            "initiatorIdPub": B64.encode(hello.initiator_id_pub),
            "initiatorSig": B64.encode(hello.initiator_sig),
            "name": hello.name,
            "platform": hello.platform.clone().unwrap_or_default(),
            "appVersion": hello.app_version.clone().unwrap_or_default(),
            "initiatorAddrs": hello.initiator_addrs,
        });
        let url = format!("http://{host}:{port}/api/v1/link/pair/hello");
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .timeout(std::time::Duration::from_secs(8))
            .send()
            .await
            .map_err(|e| LinkError::Io(e.to_string()))?;
        // 对端的 4xx 携带了它真正的拒绝理由（错码 / 已被节流 / 自配对 / 坏签名）。
        // 此前这里把任何 400 都改写成 InvalidCode，把「猜码过多，已节流」伪装成
        // 「配对码错误」——用户会对着一个其实正确的码一遍遍重试。改为按稳定契约串
        // 还原对端语义，还原不出来才退回 InvalidCode。
        if resp.status() == reqwest::StatusCode::BAD_REQUEST {
            let detail: serde_json::Value = resp.json().await.unwrap_or_default();
            let message = detail.get("message").and_then(|v| v.as_str()).unwrap_or("");
            return Err(LinkError::from_wire_message(message).unwrap_or(LinkError::InvalidCode));
        }
        if !resp.status().is_success() {
            return Err(LinkError::Unreachable);
        }
        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| LinkError::Io(e.to_string()))?;
        let hello_resp = parse_hello_response(&json)?;

        let responder_addr = format!("{host}:{port}");
        let sas = initiator.on_hello_response(&hello_resp, &responder_addr)?;
        let token = uuid::Uuid::new_v4().simple().to_string();
        let peer_name = hello_resp.name.clone();
        if let Ok(mut pending) = self.pending.lock() {
            // 剪枝：丢弃用户已放弃、超过会话时窗的待确认项，避免无界增长 + 及时释放临时密钥。
            pending.retain(|_, p| p.created.elapsed() < PENDING_INIT_TTL);
            // 剪枝后仍达到硬上限（短时间内连续发起多次配对但都未确认）——
            // 拒绝新请求而非无界增长；正常使用不可能同时挂 32 个待确认配对。
            if pending.len() >= 32 {
                return Err(LinkError::Unavailable);
            }
            pending.insert(
                token.clone(),
                PendingInit {
                    initiator,
                    session_id: hello_resp.session_id.clone(),
                    peer_host: host.to_string(),
                    peer_port: port,
                    created: std::time::Instant::now(),
                },
            );
        }
        Ok(BeginPairingResult {
            token,
            sas,
            peer_name,
            peer_fingerprint: super::crypto::fingerprint(&hello_resp.responder_id_pub),
        })
    }

    /// SAS 核对后确认/拒绝配对。`accept=true` 且对端确认成功 → 落库 + 广播 Paired。
    pub async fn confirm_pairing(
        &self,
        token: &str,
        accept: bool,
    ) -> LinkResult<Option<PeerRecord>> {
        let pending = {
            let mut guard = self.pending.lock().map_err(|_| LinkError::Unavailable)?;
            guard.remove(token)
        };
        let Some(pending) = pending else {
            return Err(LinkError::SessionExpired);
        };
        // 本地也校验过期：此前只靠对端 SESSION_TTL 兜底，双端各自维护一套
        // 过期逻辑，未来任一端常量独立调整就会行为不一致。本机用户迟迟不
        // 核验 SAS 时在这里直接短路，不必再发起一次注定失败、还要等待
        // 下方最多 70s 超时的网络往返。
        if pending.created.elapsed() > PENDING_INIT_TTL {
            return Err(LinkError::SessionExpired);
        }
        let body = serde_json::json!({ "sessionId": pending.session_id, "confirm": accept });
        let url = format!(
            "http://{}:{}/api/v1/link/pair/confirm",
            pending.peer_host, pending.peer_port
        );
        let resp = self
            .client
            .post(&url)
            .json(&body)
            // 对端响应方现在要等本机用户在 confirm 阶段核验 SAS 并点击批准/
            // 拒绝（PairingResponder::handle_confirm 等待用户决策，上限
            // 60s），因此这里的超时必须盖过那 60s 决策窗口再留出网络往返
            // 余量，否则发起方会在对端用户点确认前就先行超时掉线。
            .timeout(std::time::Duration::from_secs(70))
            .send()
            .await
            .map_err(|e| LinkError::Io(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(LinkError::SessionExpired);
        }
        if !accept {
            return Ok(None);
        }
        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| LinkError::Io(e.to_string()))?;
        // 对端 confirm 响应体 `{"success":true,"paired":<bool>,"reason":<"rejected"|
        // "timeout"|null>}`。对端用户拒绝与核验超时都是**协议正常终局**，对端以 2xx +
        // `paired=false` 返回（而非 4xx），否则这里的非 2xx 分支会把它们一律压成
        // SessionExpired，用户看到的是「会话过期」而不是「对方拒绝了配对」。
        let paired = json
            .get("paired")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !paired {
            let reason = json.get("reason").and_then(|v| v.as_str()).unwrap_or("");
            return Err(match reason {
                "timeout" => LinkError::PairingTimeout,
                _ => LinkError::RejectedByPeer,
            });
        }
        let record = pending.initiator.finalize()?;
        self.store.upsert(&record).await?;
        let _ = self
            .events
            .send(LinkEngineEvent::Paired(record.clone()))
            .await;
        Ok(Some(record))
    }

    // ── 名册 ──────────────────────────────────────────────────────────────

    /// 全部已配对设备。
    pub async fn list_devices(&self) -> LinkResult<Vec<PeerRecord>> {
        self.store.list().await
    }

    /// 解除配对（删除设备），广播 Unpaired。
    pub async fn remove_device(&self, fingerprint: &str) -> LinkResult<bool> {
        let removed = self.store.remove(fingerprint).await?;
        if removed {
            let _ = self
                .events
                .send(LinkEngineEvent::Unpaired(fingerprint.to_string()))
                .await;
        }
        Ok(removed)
    }

    /// 探测一台已配对设备是否在线（走传输栈拨号），成功则刷新 last_seen。
    pub async fn is_online(&self, fingerprint: &str) -> bool {
        let Ok(Some(record)) = self.store.get(fingerprint).await else {
            return false;
        };
        match self.transport.connect(&record).await {
            Ok(_) => {
                let _ = self.store.touch(fingerprint, now_unix()).await;
                true
            }
            Err(_) => false,
        }
    }

    // ── 数据面 ────────────────────────────────────────────────────────────

    /// 把一个下载任务下发给已配对设备（发起方数据面）。走传输栈解析可达
    /// base_url，请求体先加密（[`derive_link_aead_key`] + [`seal_link_body`]）
    /// 再用每对独立链路密钥对**密文**做 HMAC 鉴权（encrypt-then-MAC），POST
    /// 对端 `/api/v1/link/tasks`。返回新任务 ID。
    pub async fn dispatch(
        &self,
        fingerprint: &str,
        url: &str,
        save_dir: Option<&str>,
        file_name: Option<&str>,
    ) -> LinkResult<String> {
        let record = self
            .store
            .get(fingerprint)
            .await?
            .ok_or(LinkError::Unauthorized)?;
        let conn = self.transport.connect(&record).await?;
        let path = "/api/v1/link/tasks";
        let ts = now_unix();
        let nonce = uuid::Uuid::new_v4().simple().to_string();
        // 明文序列化**一次**，加密**一次**——同一份密文字节既用于 HMAC 也
        // 用于发送，保证签名覆盖的字节与对端收到并校验的字节完全一致
        // （Option 空值序列化为 ""，非 null，否则响应方 `LinkTaskRequest`
        // (非 Option String) 反序列化会 400）。encrypt-then-MAC：先加密、
        // 再对密文算 HMAC；对端必须按同一顺序校验（先验 HMAC 再解密），
        // 否则攻击者能在密文没被认证前就篡改，让对端白白解密一次。
        let body_json = serde_json::json!({
            "url": url,
            "saveDir": save_dir.unwrap_or_default(),
            "fileName": file_name.unwrap_or_default(),
        });
        let plaintext = serde_json::to_vec(&body_json).unwrap_or_default();
        let aead_key = derive_link_aead_key(&record.link_secret);
        let sealed = seal_link_body(&aead_key, &plaintext);
        let tag = link_auth_tag(&record.link_secret, "POST", path, ts, &nonce, &sealed);
        let resp = self
            .client
            .post(format!("{}{}", conn.base_url, path))
            .header("X-FluxLink-Device", self.identity.fingerprint())
            .header("X-FluxLink-Ts", ts.to_string())
            .header("X-FluxLink-Nonce", nonce)
            .header("X-FluxLink-Auth", tag)
            .header("X-FluxLink-Enc", "v1")
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .body(sealed)
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await
            .map_err(|e| LinkError::Io(e.to_string()))?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(LinkError::Unauthorized);
        }
        if !resp.status().is_success() {
            return Err(LinkError::Io(format!("dispatch failed: {}", resp.status())));
        }
        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| LinkError::Io(e.to_string()))?;
        let task_id = json
            .get("taskId")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| LinkError::Io("missing taskId in dispatch response".into()))?
            .to_string();
        let _ = self.store.touch(fingerprint, now_unix()).await;
        Ok(task_id)
    }

    /// 校验入站链路数据面请求：时间戳时窗 → 取设备记录 → 验 HMAC（覆盖
    /// 密文摘要，encrypt-then-MAC）→ nonce 防重放 → 解密。成功返回发起方
    /// 指纹 + **已解密**的明文 body（见 [`LinkRequest`]）。
    ///
    /// `enc` 是 `X-FluxLink-Enc` 请求头的原始值：数据面 body 恒为密文，
    /// 双端同版本发布、不兼容明文旧客户端——缺这个头或头值不是 `"v1"` 一律
    /// 当鉴权失败拒绝，不留明文回退路径（回退路径等于给降级攻击开门）。
    #[allow(clippy::too_many_arguments)]
    pub async fn authorize(
        &self,
        method: &str,
        path: &str,
        device_fp: &str,
        ts: i64,
        nonce: &str,
        body: &[u8],
        tag: &str,
        enc: &str,
    ) -> LinkResult<LinkRequest> {
        if enc != "v1" {
            return Err(LinkError::Unauthorized);
        }
        let now = now_unix();
        // i128 比较，防攻击者构造的极端 ts 触发 i64 溢出（debug 下 panic）。
        if (now as i128 - ts as i128).abs() > LINK_AUTH_SKEW_SECS as i128 {
            return Err(LinkError::Unauthorized);
        }
        let record = self
            .store
            .get(device_fp)
            .await?
            .ok_or(LinkError::Unauthorized)?;
        // encrypt-then-MAC：`body` 此刻仍是密文，先验 HMAC（覆盖密文摘要）
        // ——与发起方 [`Self::dispatch`] 对同一份密文字节算 tag 的顺序一致，
        // 签名覆盖的字节与此处校验的字节完全相同。
        if !verify_link_auth_tag(&record.link_secret, method, path, ts, nonce, body, tag) {
            return Err(LinkError::Unauthorized);
        }
        // 防重放：同 (device, nonce) 在时窗内仅接受一次；顺带按时窗剪枝保持有界。
        {
            let mut seen = self
                .seen_nonces
                .lock()
                .map_err(|_| LinkError::Unavailable)?;
            seen.retain(|(_, seen_ts)| now - *seen_ts <= LINK_AUTH_SKEW_SECS);
            let key = format!("{device_fp}:{nonce}");
            if seen.iter().any(|(k, _)| *k == key) {
                return Err(LinkError::Unauthorized);
            }
            seen.push((key, now));
        }
        // HMAC 通过后才解密：密文完整性此刻已由 HMAC 保证，解密在正常流程
        // 下不会失败（同一 `record.link_secret` 派生的 AEAD 密钥）；仍显式
        // 处理——解密失败统一按鉴权失败处理，不额外泄露「HMAC 过但解密
        // 失败」这种细分信息。
        let aead_key = derive_link_aead_key(&record.link_secret);
        let plaintext = open_link_body(&aead_key, body).ok_or(LinkError::Unauthorized)?;
        Ok(LinkRequest {
            device: device_fp.to_string(),
            body: plaintext,
        })
    }

    /// 剪枝全部「只靠恰好又发生一次同类调用才顺带清理」的过期状态：
    /// `pending`（本机发起、SAS 核对/confirm 未完成的待确认会话）、
    /// `seen_nonces`（数据面防重放时窗），并转发给响应方剪枝其
    /// `codes`/`sessions`。由 [`Self::spawn_gc`] 每 60s 调用一次；半途
    /// 放弃的配对与过期 nonce 记录此前会一直驻留到进程重启才被回收。
    pub fn prune_expired(&self) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.retain(|_, p| p.created.elapsed() < PENDING_INIT_TTL);
        }
        if let Ok(mut seen) = self.seen_nonces.lock() {
            let now = now_unix();
            seen.retain(|(_, seen_ts)| now - *seen_ts <= LINK_AUTH_SKEW_SECS);
        }
        self.responder.prune_expired();
    }
}

/// [`LinkManager::begin_pairing`] 的结果：待确认令牌 + 供核对的 SAS + 对端信息。
#[derive(Debug, Clone)]
pub struct BeginPairingResult {
    pub token: String,
    pub sas: String,
    pub peer_name: String,
    pub peer_fingerprint: String,
}

/// [`LinkManager::authorize`] 成功后的产出：通过链路鉴权的一次入站数据面
/// 请求——发起方指纹 + **已解密**的明文 body。此前 `authorize` 只返回发起
/// 方指纹（`LinkResult<String>`），宿主拿到指纹后仍用自己手里的原始请求
/// 体字节反序列化；数据面加密后那份原始字节是密文，宿主必须改用这里
/// 返回的解密结果——把「鉴权通过」与「拿到明文」在返回值里绑在一起，
/// 杜绝调用方漏改、继续拿密文当明文解析的错误用法。
#[derive(Debug, Clone)]
pub struct LinkRequest {
    /// 发起方设备指纹。
    pub device: String,
    /// 已解密的明文请求体。
    pub body: Vec<u8>,
}

fn b64_to_array<const N: usize>(json: &serde_json::Value, key: &str) -> LinkResult<[u8; N]> {
    let s = json
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| LinkError::BadPayload(format!("missing {key}")))?;
    let bytes = B64
        .decode(s)
        .map_err(|_| LinkError::BadPayload(format!("bad base64 {key}")))?;
    <[u8; N]>::try_from(bytes.as_slice())
        .map_err(|_| LinkError::BadPayload(format!("bad length {key}")))
}

fn parse_hello_response(json: &serde_json::Value) -> LinkResult<HelloResponse> {
    let get_str = |k: &str| {
        json.get(k)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    Ok(HelloResponse {
        session_id: get_str("sessionId")
            .ok_or_else(|| LinkError::BadPayload("missing sessionId".into()))?,
        responder_eph_pub: b64_to_array::<32>(json, "responderEphPub")?,
        responder_id_pub: b64_to_array::<32>(json, "responderIdPub")?,
        responder_sig: b64_to_array::<64>(json, "responderSig")?,
        name: get_str("name").unwrap_or_default(),
        platform: get_str("platform"),
        app_version: get_str("appVersion"),
        sas: get_str("sas").unwrap_or_default(),
    })
}

/// 入站 `hello` 的 wire 形式（base64 字符串字段），供 HTTP 宿主纯字段搬运。
#[derive(Debug, Clone)]
pub struct WireHello {
    pub code: String,
    pub initiator_eph_pub: String,
    pub initiator_id_pub: String,
    pub initiator_sig: String,
    pub name: String,
    pub platform: Option<String>,
    pub app_version: Option<String>,
    pub initiator_addrs: Vec<String>,
}

/// 出站 `hello` 回复的 wire 形式（base64 字符串字段）。
#[derive(Debug, Clone)]
pub struct WireHelloResponse {
    pub session_id: String,
    pub responder_eph_pub: String,
    pub responder_id_pub: String,
    pub responder_sig: String,
    pub name: String,
    pub platform: Option<String>,
    pub app_version: Option<String>,
    pub sas: String,
}

fn decode_b64_array<const N: usize>(s: &str) -> LinkResult<[u8; N]> {
    let bytes = B64
        .decode(s)
        .map_err(|_| LinkError::BadPayload("bad base64".into()))?;
    <[u8; N]>::try_from(bytes.as_slice()).map_err(|_| LinkError::BadPayload("bad length".into()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::link::crypto::{derive_link_aead_key, link_auth_tag, seal_link_body};

    async fn mgr_with_device(secret: Vec<u8>) -> (Arc<LinkManager>, String) {
        let url = format!(
            "sqlite:file:linkmgr_{}?mode=memory&cache=shared",
            uuid::Uuid::new_v4().simple()
        );
        let db = Db::connect(&url).await.unwrap();
        let (tx, _rx) = mpsc::channel(8);
        let mgr = LinkManager::load(
            db,
            SelfInfo {
                name: "me".into(),
                platform: None,
                app_version: None,
            },
            17800,
            tx,
        )
        .await
        .unwrap();
        let fp = "peerfp".to_string();
        mgr.store
            .upsert(&PeerRecord {
                fingerprint: fp.clone(),
                identity_pub: vec![1u8; 32],
                name: "peer".into(),
                platform: None,
                link_secret: secret,
                candidates: vec![],
                paired_at: 0,
                last_seen_at: 0,
            })
            .await
            .unwrap();
        (mgr, fp)
    }

    /// 按发起方 [`LinkManager::dispatch`] 同款 encrypt-then-MAC 流程构造
    /// 密文 body + tag：先加密、再对**密文**算 HMAC——测试里的「合法请求」
    /// 必须复刻这个顺序，否则测出来的是另一套协议。
    fn seal_and_tag(
        secret: &[u8],
        path: &str,
        ts: i64,
        nonce: &str,
        plaintext: &[u8],
    ) -> (Vec<u8>, String) {
        let sealed = seal_link_body(&derive_link_aead_key(secret), plaintext);
        let tag = link_auth_tag(secret, "POST", path, ts, nonce, &sealed);
        (sealed, tag)
    }

    #[tokio::test]
    async fn authorize_accepts_valid_then_rejects_replay_tamper_and_skew() {
        let secret = vec![7u8; 32];
        let (mgr, fp) = mgr_with_device(secret.clone()).await;
        let path = "/api/v1/link/tasks";
        let ts = now_unix();
        let plaintext = br#"{"url":"http://x/f"}"#;
        let (sealed, tag) = seal_and_tag(&secret, path, ts, "n1", plaintext);

        // 合法请求通过，且解密还原出原始明文。
        let authorized = mgr
            .authorize("POST", path, &fp, ts, "n1", &sealed, &tag, "v1")
            .await
            .unwrap();
        assert_eq!(authorized.device, fp);
        assert_eq!(authorized.body, plaintext);
        // 同 nonce 重放被拒（防重放）。
        assert!(matches!(
            mgr.authorize("POST", path, &fp, ts, "n1", &sealed, &tag, "v1")
                .await,
            Err(LinkError::Unauthorized)
        ));
        // 篡改密文（哪怕只翻一个字节）→ HMAC 覆盖密文摘要，篡改后 tag 不再
        // 匹配 → 拒（encrypt-then-MAC 的完整性保护在生效）。
        let mut tampered = sealed.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 0xFF;
        assert!(matches!(
            mgr.authorize("POST", path, &fp, ts, "n2", &tampered, &tag, "v1")
                .await,
            Err(LinkError::Unauthorized)
        ));
        // 过期时间戳 → 拒。
        let old_ts = ts - LINK_AUTH_SKEW_SECS - 5;
        let (old_sealed, old_tag) = seal_and_tag(&secret, path, old_ts, "n3", plaintext);
        assert!(matches!(
            mgr.authorize("POST", path, &fp, old_ts, "n3", &old_sealed, &old_tag, "v1")
                .await,
            Err(LinkError::Unauthorized)
        ));
        // 未配对设备 → 拒。
        let (sealed4, tag4) = seal_and_tag(&secret, path, ts, "n4", plaintext);
        assert!(matches!(
            mgr.authorize("POST", path, "unknown", ts, "n4", &sealed4, &tag4, "v1")
                .await,
            Err(LinkError::Unauthorized)
        ));
    }

    #[tokio::test]
    async fn authorize_rejects_missing_or_invalid_enc_header() {
        // 数据面不留明文回退路径：缺 `X-FluxLink-Enc` 头或头值不是 "v1"，
        // 哪怕 HMAC/nonce/时间戳全部合法也一律拒绝——回归防止未来有人为
        // 兼容旧客户端悄悄加回明文分支（降级攻击）。
        let secret = vec![7u8; 32];
        let (mgr, fp) = mgr_with_device(secret.clone()).await;
        let path = "/api/v1/link/tasks";
        let ts = now_unix();
        let (sealed, tag) = seal_and_tag(&secret, path, ts, "n1", br#"{"url":"http://x/f"}"#);

        assert!(matches!(
            mgr.authorize("POST", path, &fp, ts, "n1", &sealed, &tag, "")
                .await,
            Err(LinkError::Unauthorized)
        ));
        assert!(matches!(
            mgr.authorize("POST", path, &fp, ts, "n1", &sealed, &tag, "v0")
                .await,
            Err(LinkError::Unauthorized)
        ));
    }

    #[tokio::test]
    async fn prune_expired_evicts_stale_seen_nonces() {
        // pending 的过期判定已由 confirm_pairing 的本地 PENDING_INIT_TTL
        // 检查覆盖；这里覆盖 prune_expired 对 seen_nonces 的时窗剪枝——
        // 超过 LINK_AUTH_SKEW_SECS 的防重放记录必须被清掉，否则该表在
        // 进程存活期内随数据面请求量无界增长。
        let secret = vec![7u8; 32];
        let (mgr, fp) = mgr_with_device(secret).await;
        let old_ts = now_unix() - LINK_AUTH_SKEW_SECS - 5;
        if let Ok(mut seen) = mgr.seen_nonces.lock() {
            seen.push((format!("{fp}:stale"), old_ts));
        }
        mgr.prune_expired();
        assert!(mgr.seen_nonces.lock().unwrap().is_empty());
    }
}
