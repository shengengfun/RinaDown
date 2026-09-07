//! 配对协议：一次性配对码 + X25519 ECDH + SAS 短认证串 + Ed25519 身份绑定。
//!
//! # 安全模型
//! - **配对码**（6 位数字，TTL 120s，单次使用）：一次性引导凭据，授权本次配对并
//!   限速暴力尝试；本身非长期密钥，泄漏一张过期码无害。错误猜码另受节流器限制
//!   （按请求来源分桶：单一来源 120 秒时窗内至多 `MAX_FAILED_HELLOS` 次；另有
//!   远大于此的全局兜底 `MAX_FAILED_HELLOS_GLOBAL`，专防分布式换源 IP 绕过单
//!   来源阈值，见 `PairingResponder::is_throttled`）。超限时 `handle_hello` 在
//!   查码、验签之前直接拒绝；命中节流时不查码、不消费任何码、不做任何签名
//!   验证运算。`Throttled` 与 `InvalidCode` 各自持有独立错误信息——这个区别
//!   是**故意**向对端暴露的：区分信息本身不构成额外的暴力破解助力（真正限速
//!   的是节流阈值本身），暴露它是为了让发起方 UI 能给出准确提示，而非笼统
//!   显示「配对失败」。
//! - **X25519 ECDH**：双方各出一把**临时**密钥做密钥协商得共享密钥 `z`（前向保密）。
//! - **SAS**（6 位，双端肉眼核对）：从 `z` + 双方临时公钥派生；中间人会与两端各自
//!   协商出不同 `z` → 两端 SAS 不一致 → 用户肉眼即可发现（防 MITM）。核对是**双向**
//!   的：响应方用户必须在本机核对 SAS 并显式批准（见 `PairingResponder::set_local_decision`）
//!   后，`handle_confirm` 才会放行——响应方是配对能否成立的最终把关人，而非仅凭
//!   一次性码验证通过就自动登记对端。
//! - **Ed25519 身份绑定**：每端用长期身份私钥对「域分隔串 || 各字段」签名，对端
//!   用其出示的身份公钥验签——除长期身份与本次临时公钥外，发起方一侧的签名
//!   覆盖范围还包括自报的 `name`/`platform`/`app_version`/`initiator_addrs`
//!   （变长字段按长度前缀分帧，杜绝拼接歧义），杜绝明文 HTTP 中间人在转发时
//!   篡改这些字段——尤其 `initiator_addrs`：一旦被篡改会被响应方原样写入
//!   `PeerRecord.candidates` 长期生效，等于让中间人植入回连地址。
//! - **每对设备独立链路密钥**：`derive_link_key(z)`，用于后续数据面 HMAC 鉴权，
//!   绝不上网络明文。
//!
//! 全部密码学步骤在引擎内完成（wire 层只做 base64 编解码），便于集中审计与单测。

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rand::{Rng, RngCore};
use tokio::sync::Notify;
use x25519_dalek::{PublicKey, StaticSecret};

use super::crypto::{derive_link_key, derive_sas, fingerprint};
use super::error::{LinkError, LinkResult};
use super::identity::LinkIdentity;
use super::types::{PeerCandidate, PeerRecord, TransportKind};
/// 配对码有效期。
const CODE_TTL: Duration = Duration::from_secs(120);
/// confirm 会话有效期（hello 之后必须尽快核对 SAS 并确认）。
///
/// 反向约束：必须严格大于 `LOCAL_DECISION_TIMEOUT`——两者共享同一个锚点
/// （hello 抵达时刻，见该常量上方注释）。若 `SESSION_TTL` 反而更短或与之
/// 相等，`prune_sessions` 会抢在决策窗口关闭之前，把一条仍在等待响应方
/// 用户表态的会话当「过期」删掉：用户点了批准，`handle_confirm` 却已经
/// 找不到会话，只能得到文不对题的 `SessionExpired`。
const SESSION_TTL: Duration = Duration::from_secs(180);
/// 无匹配 hello（猜码）在时窗内、**单一来源**的上限——达到即拒绝该来源的
/// 新 hello（见 `PairingResponder::is_throttled`），防止单一来源在线暴力
/// 猜码。与配对码生命周期**解耦**：错误猜测绝不作废有效码（避免猜码 DoS）。
const MAX_FAILED_HELLOS: usize = 10;
/// 无匹配 hello 在时窗内、**全部来源合计**的上限——独立于按来源分桶的
/// `MAX_FAILED_HELLOS`，专防攻击者用大量不同来源 IP 分布式绕过单来源
/// 阈值。取值远大于单来源阈值：6 位配对码空间为 1e6 种，时窗内命中正确
/// 码的概率仅 200/1e6 = 2e-4，安全上完全可接受；而分布式 DoS 的攻击成本
/// 因此被抬高两个数量级——不再是单机脚本打满一个来源就够，而是必须真的
/// 凑齐这么多不同来源 IP。
const MAX_FAILED_HELLOS_GLOBAL: usize = 200;
/// 失败 hello 的统计时窗（对单来源桶与全局合计都适用）。
const FAILED_HELLO_WINDOW: Duration = Duration::from_secs(120);
/// 单个来源桶内失败 hello 时间戳的硬上限，与 `MAX_FAILED_HELLOS` 无关：
/// `record_failed_hello` 必须始终 push（否则最后一条失败记录永远卡在时窗
/// 尾部不再前移，节流会从「限速」退化成「永久封禁」），这里只用来防止单个
/// 桶长期没有新 hello 时向量无界增长。
const MAX_FAILED_HELLO_RECORDS: usize = 64;
/// 失败 hello 分桶表的桶数硬上限：来源 IP 数量不可控（尤其是被攻击时），
/// 桶表本身必须有界，否则「按来源分桶」会退化成又一个无界内存增长的 DoS
/// 面。达到上限后不再为新来源单独开桶，一律并入共享的 `UNKNOWN_SOURCE_KEY`
/// 桶——仍然计入全局阈值 `MAX_FAILED_HELLOS_GLOBAL`，不会因此绕过节流。
const MAX_FAILURE_BUCKETS: usize = 256;
/// 拿不到请求来源地址时的固定分桶键——即便如此也必须正常计入节流，不能
/// 让「拿不到来源地址」变成绕过节流的手段；桶表满员后的溢出来源也并入
/// 这个桶（见 `MAX_FAILURE_BUCKETS`）。
const UNKNOWN_SOURCE_KEY: &str = "unknown";
/// 等待响应方本机用户核验 SAS 并做出批准/拒绝决策的超时时长。
///
/// 不变量：锚点是 hello 抵达时刻，与两端客户端的倒计时同源——响应方/
/// 发起方两个客户端在收到（或广播）`IncomingPairing` 后各自的 60 秒 UI
/// 倒计时都从 hello 时刻起算，`PairingResponder::handle_confirm` 的决策
/// 截止时间必须锚在同一个时刻（`ConfirmEntry::created`），而不是「本次
/// confirm 抵达时刻」——否则发起方核对 SAS 耗时越久，响应方 UI 弹窗与
/// backend 等待窗口错位越大：UI 早已按自己的倒计时自动关闭，backend 却
/// 还在傻等一个不会再来的决策。必须严格小于 `SESSION_TTL`（见该常量上方
/// 反向约束注释），否则 `prune_sessions` 会在决策窗口关闭前就把仍在等待
/// 用户决策的会话剪掉。
const LOCAL_DECISION_TIMEOUT: Duration = Duration::from_secs(60);

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 把一个变长字节串以「4 字节小端长度前缀 + 内容」写入 `t`——长度前缀分帧，
/// 避免多个变长字段直接拼接产生的边界歧义（如 `"ab"+"c"` 与 `"a"+"bc"` 拼接后
/// 字节完全相同）。
fn push_framed(t: &mut Vec<u8>, bytes: &[u8]) {
    t.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    t.extend_from_slice(bytes);
}

/// 发起方 hello 的签名覆盖范围：配对码、双方临时/身份公钥，以及发起方自报的
/// `name`/`platform`/`app_version`/`initiator_addrs`。后四者若不入签，明文
/// HTTP 中间人可在转发时任意篡改而不影响 SAS——尤其 `initiator_addrs` 会被
/// 响应方原样写入 `PeerRecord.candidates` 长期生效，等于让中间人植入回连
/// 地址。全部变长字段均按 `push_framed` 长度前缀分帧；`initiator_addrs` 额外
/// 先写元素个数再逐个分帧写入；`platform`/`app_version` 用空串归一 `None`
/// （发起方与响应方必须对同一个 `Option` 值算出完全相同的字节串，否则验签
/// 必然失败——两处调用点见 `PairingInitiator::build_hello` 与
/// `PairingResponder::handle_hello`）。
fn transcript_init(
    code: &str,
    init_eph_pub: &[u8; 32],
    init_id_pub: &[u8; 32],
    name: &str,
    platform: Option<&str>,
    app_version: Option<&str>,
    initiator_addrs: &[String],
) -> Vec<u8> {
    let mut t = Vec::with_capacity(256);
    t.extend_from_slice(b"fluxdown-link-init-v1");
    push_framed(&mut t, code.as_bytes());
    t.extend_from_slice(init_eph_pub);
    t.extend_from_slice(init_id_pub);
    push_framed(&mut t, name.as_bytes());
    push_framed(&mut t, platform.unwrap_or("").as_bytes());
    push_framed(&mut t, app_version.unwrap_or("").as_bytes());
    t.extend_from_slice(&(initiator_addrs.len() as u32).to_le_bytes());
    for addr in initiator_addrs {
        push_framed(&mut t, addr.as_bytes());
    }
    t
}

fn transcript_resp(resp_eph_pub: &[u8; 32], init_eph_pub: &[u8; 32]) -> Vec<u8> {
    let mut t = Vec::with_capacity(21 + 64);
    t.extend_from_slice(b"fluxdown-link-resp-v1");
    t.extend_from_slice(resp_eph_pub);
    t.extend_from_slice(init_eph_pub);
    t
}

/// 本机在配对响应中呈现的自身信息。
#[derive(Debug, Clone)]
pub struct SelfInfo {
    pub name: String,
    pub platform: Option<String>,
    pub app_version: Option<String>,
}

/// 发起方 `hello` 请求（byte-oriented；wire 层负责 base64 编解码）。
#[derive(Debug, Clone)]
pub struct HelloRequest {
    pub code: String,
    pub initiator_eph_pub: [u8; 32],
    pub initiator_id_pub: [u8; 32],
    pub initiator_sig: [u8; 64],
    pub name: String,
    pub platform: Option<String>,
    pub app_version: Option<String>,
    /// 发起方自报的可达候选地址（`ip:port`），供响应方存为回连候选。
    pub initiator_addrs: Vec<String>,
}

/// 响应方 `hello` 回复。
#[derive(Debug, Clone)]
pub struct HelloResponse {
    pub session_id: String,
    pub responder_eph_pub: [u8; 32],
    pub responder_id_pub: [u8; 32],
    pub responder_sig: [u8; 64],
    pub name: String,
    pub platform: Option<String>,
    pub app_version: Option<String>,
    /// 响应方本地显示用的 SAS（应与发起方计算出的一致）。
    pub sas: String,
}

/// 一条待确认会话（hello 已完成、等待 SAS 核对 + confirm）。
struct ConfirmEntry {
    z: [u8; 32],
    initiator_id_pub: [u8; 32],
    name: String,
    platform: Option<String>,
    initiator_addrs: Vec<String>,
    created: Instant,
    /// 本机用户对该入站会话的核验结论：`None` = 尚未表态；`Some(true)` = 批准；
    /// `Some(false)` = 拒绝。由 `PairingResponder::set_local_decision` 写入，
    /// `handle_confirm` 据此放行或拒绝——响应方用户是配对成立与否的最终把关人。
    decision: Option<bool>,
}

/// 一个已生成、尚未消费的配对码。
struct CodeEntry {
    code: String,
    eph_secret: StaticSecret,
    created: Instant,
}

/// 剪枝配对码表：清除超过 `CODE_TTL` 的过期码。抽成独立函数是因为同一逻辑
/// 在生成新码、`handle_hello` 查码、后台 GC（见 `PairingResponder::
/// prune_expired`）三处都要用到，避免各处各写一份容易漂移的 retain。
fn prune_codes(codes: &mut Vec<CodeEntry>) {
    codes.retain(|c| c.created.elapsed() < CODE_TTL);
}

/// 剪枝待确认会话表：清除超过 `SESSION_TTL` 的过期会话。同上，`handle_hello`
/// 建会话、`set_local_decision`、`handle_confirm` 的两处以及后台 GC 都复用它。
fn prune_sessions(sessions: &mut HashMap<String, ConfirmEntry>) {
    sessions.retain(|_, e| e.created.elapsed() < SESSION_TTL);
}

/// 把请求来源标准化为失败 hello 的节流分桶键。`None`（宿主层拿不到来源
/// 地址，如某些部署形态取不到连接对端 IP）统一归入固定的
/// `UNKNOWN_SOURCE_KEY` 桶——不能因为拿不到地址就放弃对这类请求节流。
fn failure_bucket_key(source: Option<IpAddr>) -> String {
    source
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| UNKNOWN_SOURCE_KEY.to_string())
}

/// 剪枝失败 hello 分桶表：每个来源桶按 `FAILED_HELLO_WINDOW` 剪掉过期
/// 记录，剪完为空的桶整个删除——否则见过一次的来源 IP 会在桶表里永久占
/// 一个空位，配合 `MAX_FAILURE_BUCKETS` 就会把桶表挤满，后来的新来源被
/// 迫并入 `UNKNOWN_SOURCE_KEY` 桶，节流粒度退化。`record_failed_hello`、
/// `is_throttled` 与后台 GC 复用它。
fn prune_failures(failures: &mut HashMap<String, Vec<Instant>>) {
    failures.retain(|_, bucket| {
        bucket.retain(|t| t.elapsed() < FAILED_HELLO_WINDOW);
        !bucket.is_empty()
    });
}

/// 常量时间判断字节串是否全为零：按位或全部字节而不提前 short-circuit，
/// 避免比较耗时随「第一个非零字节出现的位置」变化而产生时序侧信道。仅用于
/// 检测退化 ECDH 输出（见 `handle_hello`/`PairingInitiator::on_hello_response`），
/// 不是通用的常量时间比较原语。
fn is_all_zero(bytes: &[u8]) -> bool {
    bytes.iter().fold(0u8, |acc, b| acc | b) == 0
}

/// 配对**响应方**（被添加的设备侧）。持有一次性配对码与待确认会话表。
pub struct PairingResponder {
    identity: LinkIdentity,
    self_info: SelfInfo,
    codes: Mutex<Vec<CodeEntry>>,
    sessions: Mutex<HashMap<String, ConfirmEntry>>,
    /// 失败 hello（猜码）时间戳，按 [`failure_bucket_key`] 分桶——与配对码
    /// 解耦的节流器（防在线暴力猜码 DoS）。见 `MAX_FAILED_HELLOS`（单来源
    /// 阈值）与 `MAX_FAILED_HELLOS_GLOBAL`（全局兜底）。
    failures: Mutex<HashMap<String, Vec<Instant>>>,
    /// 本机用户对某会话作出核验决策时的唤醒信号：`handle_confirm` 等待期间
    /// 挂在这上面，`set_local_decision` 写入决策后 `notify_waiters` 唤醒。
    decision_notify: Arc<Notify>,
}

impl PairingResponder {
    #[must_use]
    pub fn new(identity: LinkIdentity, self_info: SelfInfo) -> Self {
        Self {
            identity,
            self_info,
            codes: Mutex::new(Vec::new()),
            sessions: Mutex::new(HashMap::new()),
            failures: Mutex::new(HashMap::new()),
            decision_notify: Arc::new(Notify::new()),
        }
    }

    /// 记录一次失败 hello（无匹配码），按 [`failure_bucket_key`] 分桶。按
    /// 时窗剪枝后**始终**记录一条——若达到上限就不再记录，最后一条失败记录
    /// 会永远卡在时窗尾部、不再随时间前移，节流就从「限速」退化成「永久
    /// 封禁」。单桶大小用与 `MAX_FAILED_HELLOS` 无关的硬上限
    /// `MAX_FAILED_HELLO_RECORDS` 截断，仅防止向量无界增长；桶表本身也有
    /// 界（`MAX_FAILURE_BUCKETS`）——到达上限后不再为新来源单独开桶，一律
    /// 并入共享的 `UNKNOWN_SOURCE_KEY` 桶，否则攻击者只需不断更换来源
    /// 地址就能让分桶节流形同虚设。与配对码解耦：错误猜测绝不影响任何
    /// 有效码的生命周期。
    fn record_failed_hello(&self, source: Option<IpAddr>) {
        let key = failure_bucket_key(source);
        if let Ok(mut failures) = self.failures.lock() {
            prune_failures(&mut failures);
            let key = if failures.contains_key(&key) || failures.len() < MAX_FAILURE_BUCKETS {
                key
            } else {
                UNKNOWN_SOURCE_KEY.to_string()
            };
            let bucket = failures.entry(key).or_default();
            if bucket.len() >= MAX_FAILED_HELLO_RECORDS {
                bucket.remove(0);
            }
            bucket.push(Instant::now());
        }
    }

    /// 当前来源是否处于猜码节流状态：**该来源**时窗内失败次数达到
    /// `MAX_FAILED_HELLOS`，或**全部来源合计**达到 `MAX_FAILED_HELLOS_GLOBAL`
    /// （分布式猜码兜底）。`handle_hello` 在查码之前调用；命中时不查码、不
    /// 验签、不消费任何码。`Throttled` 与 `InvalidCode` 各自持有独立错误
    /// 信息——这个区别是**故意**向对端暴露的，用于发起方 UI 给出准确提示。
    fn is_throttled(&self, source: Option<IpAddr>) -> bool {
        let key = failure_bucket_key(source);
        let Ok(mut failures) = self.failures.lock() else {
            return false;
        };
        prune_failures(&mut failures);
        let global: usize = failures.values().map(Vec::len).sum();
        if global >= MAX_FAILED_HELLOS_GLOBAL {
            return true;
        }
        failures
            .get(&key)
            .is_some_and(|bucket| bucket.len() >= MAX_FAILED_HELLOS)
    }

    /// 生成一个新配对码（在被添加设备的 UI 上展示，2 分钟内有效、单次使用）。
    pub fn generate_code(&self) -> String {
        let code = format!("{:06}", rand::rng().random_range(0..1_000_000u32));
        let mut seed = [0u8; 32];
        rand::rng().fill_bytes(&mut seed);
        let eph_secret = StaticSecret::from(seed);
        if let Ok(mut codes) = self.codes.lock() {
            // 清空而非仅剪枝过期码：生成新码即令旧码立即失效。同一响应方
            // 同一时刻只应有一个有效码，否则用户点「刷新配对码」后，已被
            // 窥屏泄露的旧码仍会在自身 120s 窗口内保持可用，与用户直觉相悖。
            codes.clear();
            codes.push(CodeEntry {
                code: code.clone(),
                eph_secret,
                created: Instant::now(),
            });
        }
        code
    }

    /// 处理 `hello`：节流检查 → 校验码 → ECDH → 验发起方签名 → 建会话 → 签自身握手。
    /// `source`：请求来源地址，用于按来源分桶节流（见 [`failure_bucket_key`]）；
    /// 宿主层拿不到时传 `None`，仍会计入固定的 `UNKNOWN_SOURCE_KEY` 桶。
    pub fn handle_hello(
        &self,
        req: &HelloRequest,
        source: Option<IpAddr>,
    ) -> LinkResult<HelloResponse> {
        // 节流检查放在最前面、查码与验签之前：命中时立即返回，既不消费任何
        // 配对码也不做任何签名验证运算。`Throttled` 与 `InvalidCode` 各自
        // 持有独立错误信息——这个区别是**故意**向对端暴露的：区分信息本身
        // 不构成额外的暴力破解助力（真正限速的是节流阈值本身），暴露它是
        // 为了让发起方 UI 能给出准确提示，而非笼统地显示「配对失败」。
        if self.is_throttled(source) {
            return Err(LinkError::Throttled);
        }
        // 自配对守卫：发起方与本机持同一长期身份（典型场景：两个进程共享同一
        // 引擎数据库）——它们本就是同一台设备，直接拒绝，不消费配对码。
        if req.initiator_id_pub == self.identity.public_bytes() {
            return Err(LinkError::SelfPairing);
        }
        // 取出并消费匹配的有效码（消费即移除，杜绝重放）。
        //
        // 残余风险（评估为可接受，不做额外处理）：hello 请求本身明文传输，
        // 若攻击者已具备局域网内的流量可见性（中间人位置），可以原样重放
        // 抓到的 hello 抢先消费掉这个一次性码，导致真正的发起方随后收到
        // InvalidCode——效果仅是一次性拒绝服务式骚扰，用户重新生成码即可。
        // 攻击者做不到的：拿不到发起方的临时私钥（eph_secret 从不出网），
        // 因而推不出共享密钥 z / link_secret，不产生任何密钥泄露或身份冒充。
        // 为 hello 引入连接绑定的重放保护（挑战-响应、TLS channel binding
        // 之类）需要改协议、增加往返，相对「重试一次」这个后果不成比例。
        let eph_secret = {
            let mut codes = self.codes.lock().map_err(|_| LinkError::Unavailable)?;
            prune_codes(&mut codes);
            let Some(pos) = codes.iter().position(|c| c.code == req.code) else {
                // 无匹配：仅记一次失败到解耦的全局节流器，**绝不**作废有效码（修复猜码 DoS）。
                drop(codes);
                self.record_failed_hello(source);
                return Err(LinkError::InvalidCode);
            };
            codes.remove(pos).eph_secret
        };

        // 验发起方身份签名：绑定其长期身份、本次临时公钥，以及自报的
        // name/platform/app_version/initiator_addrs（防中间人篡改自报字段）。
        let transcript = transcript_init(
            &req.code,
            &req.initiator_eph_pub,
            &req.initiator_id_pub,
            &req.name,
            req.platform.as_deref(),
            req.app_version.as_deref(),
            &req.initiator_addrs,
        );
        if !LinkIdentity::verify(&req.initiator_id_pub, &transcript, &req.initiator_sig) {
            return Err(LinkError::BadSignature);
        }

        let z = eph_secret
            .diffie_hellman(&PublicKey::from(req.initiator_eph_pub))
            .to_bytes();
        // 纵深防御：x25519_dalek::PublicKey::from([u8; 32]) 不对输入做点
        // 校验，退化/小子群公钥会导致协商出全零共享密钥（与对端私钥无关）。
        // 当前场景临时密钥不复用、可利用性很低，但拒绝退化点成本为零。
        if is_all_zero(&z) {
            return Err(LinkError::BadPayload("degenerate x25519 public key".into()));
        }
        let responder_eph_pub = PublicKey::from(&eph_secret).to_bytes();
        let sas = derive_sas(&z, &req.initiator_eph_pub, &responder_eph_pub);
        let responder_sig = self
            .identity
            .sign(&transcript_resp(&responder_eph_pub, &req.initiator_eph_pub));
        let session_id = uuid::Uuid::new_v4().simple().to_string();

        {
            let mut sessions = self.sessions.lock().map_err(|_| LinkError::Unavailable)?;
            prune_sessions(&mut sessions);
            sessions.insert(
                session_id.clone(),
                ConfirmEntry {
                    z,
                    initiator_id_pub: req.initiator_id_pub,
                    name: req.name.clone(),
                    platform: req.platform.clone(),
                    initiator_addrs: req.initiator_addrs.clone(),
                    created: Instant::now(),
                    decision: None,
                },
            );
        }

        Ok(HelloResponse {
            session_id,
            responder_eph_pub,
            responder_id_pub: self.identity.public_bytes(),
            responder_sig,
            name: self.self_info.name.clone(),
            platform: self.self_info.platform.clone(),
            app_version: self.self_info.app_version.clone(),
            sas,
        })
    }

    /// 记录本机用户对某入站会话的核验结论——响应方 UI 让用户核对 SAS 后调用，
    /// 批准传 `true`、拒绝传 `false`；写入后唤醒正在等待的 `handle_confirm`。
    /// 会话不存在或已过期 → `Err(LinkError::SessionExpired)`。
    pub fn set_local_decision(&self, session_id: &str, accept: bool) -> LinkResult<()> {
        {
            let mut sessions = self.sessions.lock().map_err(|_| LinkError::Unavailable)?;
            prune_sessions(&mut sessions);
            let entry = sessions
                .get_mut(session_id)
                .ok_or(LinkError::SessionExpired)?;
            entry.decision = Some(accept);
        }
        self.decision_notify.notify_waiters();
        Ok(())
    }

    /// 从会话表中移除一条会话——超时/拒绝/confirm 完成后清理，避免残留占用。
    fn remove_session(&self, session_id: &str) {
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.remove(session_id);
        }
    }

    /// 处理 `confirm`：SAS 核对现在是**双向**的。`confirm=false`（发起方一侧
    /// 用户发现 SAS 不符或主动取消）→ 直接丢弃会话，返回 `None`。
    /// `confirm=true` 时不会立即放行：还必须等待响应方本机用户通过
    /// `Self::set_local_decision` 核对 SAS 并明确批准——响应方是这场配对能否
    /// 成立的最终把关人，而非一次性码验证通过就自动登记对端。最多等待
    /// `LOCAL_DECISION_TIMEOUT`（60 秒）：超时 → `Err(PairingTimeout)`；本机
    /// 用户拒绝 → `Err(RejectedByPeer)`；批准 → 登记并返回发起方设备记录。
    pub async fn handle_confirm(
        &self,
        session_id: &str,
        confirm: bool,
    ) -> LinkResult<Option<PeerRecord>> {
        if !confirm {
            let existed = {
                let mut sessions = self.sessions.lock().map_err(|_| LinkError::Unavailable)?;
                prune_sessions(&mut sessions);
                sessions.remove(session_id).is_some()
            };
            return if existed {
                Ok(None)
            } else {
                Err(LinkError::SessionExpired)
            };
        }

        // 决策截止时间锚定在会话创建时刻（hello 抵达时刻），而不是本次
        // confirm 抵达时刻——与两端客户端「收到/广播 IncomingPairing 后从
        // hello 时刻起算 60 秒倒计时」严格同源（不变量见 LOCAL_DECISION_TIMEOUT
        // 上方注释）。发起方核对 SAS 到点 confirm 之间可能耗费任意时长：若
        // 仍从本次 confirm 抵达时刻重新起算，响应方 UI 弹窗早已按自己的
        // 倒计时自动关闭，backend 却还在傻等另一个 60s，用户在场也点不到
        // 已经消失的弹窗。
        let deadline = {
            let mut sessions = self.sessions.lock().map_err(|_| LinkError::Unavailable)?;
            prune_sessions(&mut sessions);
            let entry = sessions.get(session_id).ok_or(LinkError::SessionExpired)?;
            entry.created + LOCAL_DECISION_TIMEOUT
            // MutexGuard 在此作用域结束时 drop。
        };
        // 窗口已过：两端客户端此刻也早已按同源倒计时自动关闭了 UI，不必再
        // 进入下面的等待循环——立即清理会话并返回超时，而不是傻等一个不
        // 可能再来的决策。
        if deadline.checked_duration_since(Instant::now()).is_none() {
            self.remove_session(session_id);
            return Err(LinkError::PairingTimeout);
        }

        let decision = loop {
            // 先构造等待 future 再检查状态：只要 set_local_decision 的
            // notify_waiters() 发生在 `notified()` 构造之后（无论早于还是晚于
            // 随后的 await），该唤醒都不会丢失——这是 tokio::sync::Notify 对
            // notify_waiters 的文档保证，堵上「查到未决 → 决策写入并 notify →
            // 才开始 await」之间的经典竞态窗口。
            let notified = self.decision_notify.notified();
            tokio::pin!(notified);

            let existing = {
                let mut sessions = self.sessions.lock().map_err(|_| LinkError::Unavailable)?;
                prune_sessions(&mut sessions);
                let entry = sessions.get(session_id).ok_or(LinkError::SessionExpired)?;
                entry.decision
                // MutexGuard 在此作用域结束时 drop，绝不带着它跨越下面的 await。
            };
            if let Some(decision) = existing {
                break decision;
            }

            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                self.remove_session(session_id);
                return Err(LinkError::PairingTimeout);
            };
            if tokio::time::timeout(remaining, notified).await.is_err() {
                self.remove_session(session_id);
                return Err(LinkError::PairingTimeout);
            }
            // 被唤醒：可能是本会话的决策落地，也可能是共享 Notify 上其它会话
            // 触发的假唤醒——回到循环顶部重新加锁读取，确认后再决断。
        };

        let entry = {
            let mut sessions = self.sessions.lock().map_err(|_| LinkError::Unavailable)?;
            sessions.remove(session_id)
        };
        let Some(entry) = entry else {
            return Err(LinkError::SessionExpired);
        };
        if !decision {
            return Err(LinkError::RejectedByPeer);
        }

        let now = now_unix();
        let candidates = entry
            .initiator_addrs
            .iter()
            .map(|a| PeerCandidate {
                kind: TransportKind::Direct,
                address: a.clone(),
            })
            .collect();
        Ok(Some(PeerRecord {
            fingerprint: fingerprint(&entry.initiator_id_pub),
            identity_pub: entry.initiator_id_pub.to_vec(),
            name: entry.name,
            platform: entry.platform,
            link_secret: derive_link_key(&entry.z),
            candidates,
            paired_at: now,
            last_seen_at: now,
        }))
    }

    /// 供后台 GC 定时调用：主动剪枝三张表，清理「半途放弃」的配对——例如
    /// hello 已成功但用户从未调用 `set_local_decision`/`handle_confirm`。
    /// 现状这三张表都只在「恰好又发生一次同类调用」时顺带清理，孤儿会话
    /// 会一直驻留到进程重启；提供这个入口让宿主可以定时主动回收。
    pub fn prune_expired(&self) {
        if let Ok(mut codes) = self.codes.lock() {
            prune_codes(&mut codes);
        }
        if let Ok(mut sessions) = self.sessions.lock() {
            prune_sessions(&mut sessions);
        }
        if let Ok(mut failures) = self.failures.lock() {
            prune_failures(&mut failures);
        }
    }
}

/// 配对**发起方**（正在添加设备的一侧）。跨两次 HTTP 往返有状态。
pub struct PairingInitiator {
    identity: LinkIdentity,
    eph_secret: StaticSecret,
    eph_pub: [u8; 32],
    /// hello 之后填充：协商出的 `z` + 响应方信息（confirm 阶段登记用）。
    negotiated: Option<Negotiated>,
}

struct Negotiated {
    z: [u8; 32],
    responder_id_pub: [u8; 32],
    responder_name: String,
    responder_platform: Option<String>,
    responder_addr: String,
}

impl PairingInitiator {
    /// 用本机身份新建一次发起方会话（生成临时 X25519 密钥）。
    #[must_use]
    pub fn new(identity: LinkIdentity) -> Self {
        let mut seed = [0u8; 32];
        rand::rng().fill_bytes(&mut seed);
        let eph_secret = StaticSecret::from(seed);
        let eph_pub = PublicKey::from(&eph_secret).to_bytes();
        Self {
            identity,
            eph_secret,
            eph_pub,
            negotiated: None,
        }
    }

    /// 构造 `hello` 请求（对给定配对码签名；签名覆盖码、双方公钥，以及本次自报的
    /// name/platform/app_version/initiator_addrs，见 `transcript_init`）。
    #[must_use]
    pub fn build_hello(
        &self,
        code: &str,
        self_info: &SelfInfo,
        initiator_addrs: Vec<String>,
    ) -> HelloRequest {
        let id_pub = self.identity.public_bytes();
        let sig = self.identity.sign(&transcript_init(
            code,
            &self.eph_pub,
            &id_pub,
            &self_info.name,
            self_info.platform.as_deref(),
            self_info.app_version.as_deref(),
            &initiator_addrs,
        ));
        HelloRequest {
            code: code.to_string(),
            initiator_eph_pub: self.eph_pub,
            initiator_id_pub: id_pub,
            initiator_sig: sig,
            name: self_info.name.clone(),
            platform: self_info.platform.clone(),
            app_version: self_info.app_version.clone(),
            initiator_addrs,
        }
    }

    /// 处理响应方 `hello` 回复：验响应方签名 → 计算 `z` 与 SAS。返回本地应展示的 SAS。
    /// `responder_addr` 是发起方本次拨通对端所用的 `ip:port`（存为回连候选）。
    pub fn on_hello_response(
        &mut self,
        resp: &HelloResponse,
        responder_addr: &str,
    ) -> LinkResult<String> {
        // 自配对守卫先于验签：身份相同即可决断（共享数据库的进程互连场景）。
        if resp.responder_id_pub == self.identity.public_bytes() {
            return Err(LinkError::SelfPairing);
        }
        if !LinkIdentity::verify(
            &resp.responder_id_pub,
            &transcript_resp(&resp.responder_eph_pub, &self.eph_pub),
            &resp.responder_sig,
        ) {
            return Err(LinkError::BadSignature);
        }
        let z = self
            .eph_secret
            .diffie_hellman(&PublicKey::from(resp.responder_eph_pub))
            .to_bytes();
        // 纵深防御：理由同 `PairingResponder::handle_hello` 中的对称检查
        // ——拒绝退化点导致的全零共享密钥，成本为零。
        if is_all_zero(&z) {
            return Err(LinkError::BadPayload("degenerate x25519 public key".into()));
        }
        let sas = derive_sas(&z, &self.eph_pub, &resp.responder_eph_pub);
        self.negotiated = Some(Negotiated {
            z,
            responder_id_pub: resp.responder_id_pub,
            responder_name: resp.name.clone(),
            responder_platform: resp.platform.clone(),
            responder_addr: responder_addr.to_string(),
        });
        Ok(sas)
    }

    /// SAS 核对通过后，产出要登记的响应方设备记录（confirm 成功分支调用）。
    pub fn finalize(&self) -> LinkResult<PeerRecord> {
        let n = self.negotiated.as_ref().ok_or(LinkError::SessionExpired)?;
        let now = now_unix();
        Ok(PeerRecord {
            fingerprint: fingerprint(&n.responder_id_pub),
            identity_pub: n.responder_id_pub.to_vec(),
            name: n.responder_name.clone(),
            platform: n.responder_platform.clone(),
            link_secret: derive_link_key(&n.z),
            candidates: vec![PeerCandidate {
                kind: TransportKind::Direct,
                address: n.responder_addr.clone(),
            }],
            paired_at: now,
            last_seen_at: now,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn self_info(name: &str) -> SelfInfo {
        SelfInfo {
            name: name.to_string(),
            platform: Some("linux".to_string()),
            app_version: Some("0.1.0".to_string()),
        }
    }

    #[test]
    fn self_pairing_rejected_without_consuming_code() {
        // 双端同一身份（共享引擎数据库的两个进程）→ hello 直接拒绝，且配对码不被消费。
        let id = LinkIdentity::generate();
        let responder = PairingResponder::new(id.clone(), self_info("NAS"));
        let mut initiator = PairingInitiator::new(id.clone());

        let code = responder.generate_code();
        let hello = initiator.build_hello(&code, &self_info("NAS"), vec![]);
        assert!(matches!(
            responder.handle_hello(&hello, None),
            Err(LinkError::SelfPairing)
        ));

        // 码未被消费：换一个正常身份仍可用同一码完成 hello。
        let other = PairingInitiator::new(LinkIdentity::generate());
        let hello2 = other.build_hello(&code, &self_info("Laptop"), vec![]);
        let resp = responder.handle_hello(&hello2, None).unwrap();
        // 发起方若发现响应方就是自己，同样拒绝。
        assert!(matches!(
            initiator.on_hello_response(&resp, "127.0.0.1:1"),
            Err(LinkError::SelfPairing)
        ));
    }

    #[tokio::test]
    async fn full_handshake_agrees_on_sas_and_link_key() {
        let resp_id = LinkIdentity::generate();
        let responder = PairingResponder::new(resp_id.clone(), self_info("NAS"));
        let init_id = LinkIdentity::generate();
        let mut initiator = PairingInitiator::new(init_id.clone());

        let code = responder.generate_code();
        let hello =
            initiator.build_hello(&code, &self_info("Laptop"), vec!["10.0.0.2:17800".into()]);
        let hello_resp = responder.handle_hello(&hello, None).unwrap();

        let init_sas = initiator
            .on_hello_response(&hello_resp, "10.0.0.1:17800")
            .unwrap();
        // 双端 SAS 一致（无中间人）。
        assert_eq!(init_sas, hello_resp.sas);

        // 响应方本机用户核对 SAS 后批准，双端确认 → 各自登记对方。
        responder
            .set_local_decision(&hello_resp.session_id, true)
            .unwrap();
        let resp_side = responder
            .handle_confirm(&hello_resp.session_id, true)
            .await
            .unwrap()
            .unwrap();
        let init_side = initiator.finalize().unwrap();

        // 响应方登记的是发起方身份；发起方登记的是响应方身份。
        assert_eq!(resp_side.fingerprint, init_id.fingerprint());
        assert_eq!(init_side.fingerprint, resp_id.fingerprint());
        // 关键：双方派生出**相同**链路密钥（ECDH 对称）。
        assert_eq!(resp_side.link_secret, init_side.link_secret);
        assert_eq!(resp_side.link_secret.len(), 32);
        // 候选端点各自记录了对端地址。
        assert_eq!(resp_side.direct_address(), Some("10.0.0.2:17800"));
        assert_eq!(init_side.direct_address(), Some("10.0.0.1:17800"));
    }

    #[test]
    fn wrong_code_is_rejected() {
        let responder = PairingResponder::new(LinkIdentity::generate(), self_info("NAS"));
        let initiator = PairingInitiator::new(LinkIdentity::generate());
        responder.generate_code();
        let hello = initiator.build_hello("000000", &self_info("L"), vec![]);
        // 除非恰好猜中，几乎必然 InvalidCode（用固定错码 000000 对真码概率 1e-6）。
        let real = responder.generate_code();
        if real != "000000" {
            assert!(matches!(
                responder.handle_hello(&hello, None),
                Err(LinkError::InvalidCode)
            ));
        }
    }

    #[test]
    fn code_is_single_use() {
        let responder = PairingResponder::new(LinkIdentity::generate(), self_info("NAS"));
        let initiator = PairingInitiator::new(LinkIdentity::generate());
        let code = responder.generate_code();
        let hello = initiator.build_hello(&code, &self_info("L"), vec![]);
        assert!(responder.handle_hello(&hello, None).is_ok());
        // 同码第二次 → 已消费 → InvalidCode。
        assert!(matches!(
            responder.handle_hello(&hello, None),
            Err(LinkError::InvalidCode)
        ));
    }

    #[test]
    fn tampered_initiator_signature_rejected() {
        let responder = PairingResponder::new(LinkIdentity::generate(), self_info("NAS"));
        let initiator = PairingInitiator::new(LinkIdentity::generate());
        let code = responder.generate_code();
        let mut hello = initiator.build_hello(&code, &self_info("L"), vec![]);
        hello.initiator_sig[0] ^= 0xff; // 篡改签名
        assert!(matches!(
            responder.handle_hello(&hello, None),
            Err(LinkError::BadSignature)
        ));
    }

    #[test]
    fn tampered_initiator_addrs_rejected() {
        // 签名扩围的核心回归测试：initiator_addrs 原本不在签名覆盖范围内，
        // 明文中间人可任意篡改回连地址而不影响验签——扩围后必须能检测到。
        let responder = PairingResponder::new(LinkIdentity::generate(), self_info("NAS"));
        let initiator = PairingInitiator::new(LinkIdentity::generate());
        let code = responder.generate_code();
        let mut hello = initiator.build_hello(&code, &self_info("L"), vec!["10.0.0.2:1".into()]);
        hello.initiator_addrs = vec!["10.0.0.66:9999".into()]; // 中间人篡改回连地址
        assert!(matches!(
            responder.handle_hello(&hello, None),
            Err(LinkError::BadSignature)
        ));
    }

    #[test]
    fn tampered_initiator_name_rejected() {
        // 同上，验证 name 字段也已纳入签名覆盖范围。
        let responder = PairingResponder::new(LinkIdentity::generate(), self_info("NAS"));
        let initiator = PairingInitiator::new(LinkIdentity::generate());
        let code = responder.generate_code();
        let mut hello = initiator.build_hello(&code, &self_info("L"), vec![]);
        hello.name = "attacker-renamed".to_string();
        assert!(matches!(
            responder.handle_hello(&hello, None),
            Err(LinkError::BadSignature)
        ));
    }

    #[test]
    fn mitm_yields_diverging_sas() {
        // 模拟中间人：响应方回复里的临时公钥被替换（攻击者夹在中间）。
        // 发起方据被替换的公钥算出的 z' ≠ 响应方真实 z → SAS 不一致 → 用户可发现。
        let responder = PairingResponder::new(LinkIdentity::generate(), self_info("NAS"));
        let mut initiator = PairingInitiator::new(LinkIdentity::generate());
        let code = responder.generate_code();
        let hello = initiator.build_hello(&code, &self_info("L"), vec![]);
        let mut hello_resp = responder.handle_hello(&hello, None).unwrap();
        let attacker = LinkIdentity::generate();
        // 攻击者替换响应方临时公钥（并被迫重签，否则验签直接失败）。
        let mut seed = [1u8; 32];
        seed[0] = 7;
        let atk_secret = StaticSecret::from(seed);
        hello_resp.responder_eph_pub = PublicKey::from(&atk_secret).to_bytes();
        hello_resp.responder_id_pub = attacker.public_bytes();
        hello_resp.responder_sig = attacker.sign(&transcript_resp(
            &hello_resp.responder_eph_pub,
            &hello.initiator_eph_pub,
        ));
        let init_sas = initiator.on_hello_response(&hello_resp, "x").unwrap();
        // 发起方 SAS ≠ 响应方真实 SAS → 肉眼核对失败。
        assert_ne!(init_sas, hello_resp.sas);
    }

    #[tokio::test]
    async fn confirm_false_registers_nothing() {
        let responder = PairingResponder::new(LinkIdentity::generate(), self_info("NAS"));
        let initiator = PairingInitiator::new(LinkIdentity::generate());
        let code = responder.generate_code();
        let hello = initiator.build_hello(&code, &self_info("L"), vec![]);
        let hr = responder.handle_hello(&hello, None).unwrap();
        assert!(
            responder
                .handle_confirm(&hr.session_id, false)
                .await
                .unwrap()
                .is_none()
        );
        // 会话已消费 → 再次 confirm → SessionExpired。
        assert!(matches!(
            responder.handle_confirm(&hr.session_id, true).await,
            Err(LinkError::SessionExpired)
        ));
    }

    #[tokio::test]
    async fn local_rejection_yields_rejected_by_peer() {
        // 发起方一侧 confirm(true)（它自己已核对 SAS 通过），但响应方本机
        // 用户核对后拒绝——配对必须失败，且错误要能区分「对端拒绝」。
        let responder = PairingResponder::new(LinkIdentity::generate(), self_info("NAS"));
        let initiator = PairingInitiator::new(LinkIdentity::generate());
        let code = responder.generate_code();
        let hello = initiator.build_hello(&code, &self_info("L"), vec![]);
        let hr = responder.handle_hello(&hello, None).unwrap();

        responder.set_local_decision(&hr.session_id, false).unwrap();
        assert!(matches!(
            responder.handle_confirm(&hr.session_id, true).await,
            Err(LinkError::RejectedByPeer)
        ));
    }

    #[tokio::test]
    async fn local_approval_completes_pairing() {
        let init_id = LinkIdentity::generate();
        let responder = PairingResponder::new(LinkIdentity::generate(), self_info("NAS"));
        let initiator = PairingInitiator::new(init_id.clone());
        let code = responder.generate_code();
        let hello = initiator.build_hello(&code, &self_info("L"), vec![]);
        let hr = responder.handle_hello(&hello, None).unwrap();

        responder.set_local_decision(&hr.session_id, true).unwrap();
        let record = responder
            .handle_confirm(&hr.session_id, true)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.fingerprint, init_id.fingerprint());
    }

    #[tokio::test]
    async fn confirm_after_decision_window_times_out_immediately() {
        // 回归测试：决策截止时间锚定在会话创建时刻（hello 到达时）。confirm
        // 抵达时若这个窗口已经过去，必须立即返回 PairingTimeout、不进入
        // 等待循环——而不是从「本次 confirm 抵达」重新起算一整个
        // LOCAL_DECISION_TIMEOUT，那样响应方 UI 弹窗早已按同源倒计时自动
        // 关闭，backend 却还在傻等一个不会再来的决策。
        //
        // 直接回拨会话的 `created` 模拟「发起方核对 SAS 耗时超过决策窗口」，
        // 避免测试真的等待 LOCAL_DECISION_TIMEOUT（60 秒）。
        let responder = PairingResponder::new(LinkIdentity::generate(), self_info("NAS"));
        let initiator = PairingInitiator::new(LinkIdentity::generate());
        let code = responder.generate_code();
        let hello = initiator.build_hello(&code, &self_info("L"), vec![]);
        let hr = responder.handle_hello(&hello, None).unwrap();

        {
            let mut sessions = responder.sessions.lock().unwrap();
            let entry = sessions.get_mut(&hr.session_id).unwrap();
            entry.created = Instant::now() - LOCAL_DECISION_TIMEOUT - Duration::from_secs(1);
        }

        let start = Instant::now();
        let result = responder.handle_confirm(&hr.session_id, true).await;
        assert!(matches!(result, Err(LinkError::PairingTimeout)));
        // 不阻塞：这是窗口检查的快速路径，不是凑巧在等待循环里等到超时——
        // 5 秒的余量远小于 LOCAL_DECISION_TIMEOUT（60 秒），调度抖动绰绰
        // 有余，但足以证明没有真的进入等待。
        assert!(start.elapsed() < Duration::from_secs(5));

        // 会话已被立即清理：随后即便本机用户才做出决策，也找不到会话了。
        assert!(matches!(
            responder.set_local_decision(&hr.session_id, true),
            Err(LinkError::SessionExpired)
        ));
    }

    #[test]
    fn set_local_decision_unknown_session_is_expired() {
        let responder = PairingResponder::new(LinkIdentity::generate(), self_info("NAS"));
        assert!(matches!(
            responder.set_local_decision("no-such-session", true),
            Err(LinkError::SessionExpired)
        ));
    }

    #[test]
    fn throttle_blocks_after_limit() {
        let responder = PairingResponder::new(LinkIdentity::generate(), self_info("NAS"));
        let initiator = PairingInitiator::new(LinkIdentity::generate());
        let attacker: Option<IpAddr> = Some("203.0.113.9".parse().unwrap());
        let real_code = responder.generate_code();
        // 固定错码：与随机生成的真码撞车概率仅 1e-6，这里直接避开而非依赖概率。
        let bad_code = if real_code == "999999" {
            "888888"
        } else {
            "999999"
        };

        // 同一来源连续 MAX_FAILED_HELLOS 次错码，喂满该来源的节流桶。
        for _ in 0..MAX_FAILED_HELLOS {
            let bad = initiator.build_hello(bad_code, &self_info("L"), vec![]);
            assert!(matches!(
                responder.handle_hello(&bad, attacker),
                Err(LinkError::InvalidCode)
            ));
        }

        // 第 11 次即便用正确、未过期的码也必须被节流拒绝——返回值必须是
        // Throttled 而不是 Ok(_)/InvalidCode，证明这次请求在查码、消费码、
        // 验签之前就被拒绝：正确码根本没被触碰。
        let good = initiator.build_hello(&real_code, &self_info("L"), vec![]);
        assert!(matches!(
            responder.handle_hello(&good, attacker),
            Err(LinkError::Throttled)
        ));

        // 节流是「读时窗」而非一次性消费的资源：仍在窗口内时，重复用正确码
        // 请求依旧稳定返回 Throttled，不会退化成第二次就侥幸放行。
        assert!(matches!(
            responder.handle_hello(&good, attacker),
            Err(LinkError::Throttled)
        ));
    }

    #[test]
    fn throttle_is_per_source() {
        // 回归测试：节流必须按来源分桶，而不是退化回全局单一计数器——否则
        // 任何能触达 hello 端点的主机只需刷满一个来源的失败次数，就能让
        // 全部来源（包括携带正确码的合法发起方）永久无法完成配对；`POST
        // /api/v1/link/pair/hello` 按设计又是免鉴权端点，这是可从公网触发
        // 的 DoS。
        let responder = PairingResponder::new(LinkIdentity::generate(), self_info("NAS"));
        let attacker_init = PairingInitiator::new(LinkIdentity::generate());
        let victim_init = PairingInitiator::new(LinkIdentity::generate());
        let attacker: Option<IpAddr> = Some("203.0.113.9".parse().unwrap());
        let victim: Option<IpAddr> = Some("198.51.100.7".parse().unwrap());
        let real_code = responder.generate_code();
        let bad_code = if real_code == "999999" {
            "888888"
        } else {
            "999999"
        };

        // 来源 A（攻击者）刷满自己的节流桶。
        for _ in 0..MAX_FAILED_HELLOS {
            let bad = attacker_init.build_hello(bad_code, &self_info("L"), vec![]);
            assert!(matches!(
                responder.handle_hello(&bad, attacker),
                Err(LinkError::InvalidCode)
            ));
        }
        // 来源 A 自己确实被节流了。
        let attacker_retry = attacker_init.build_hello(&real_code, &self_info("L"), vec![]);
        assert!(matches!(
            responder.handle_hello(&attacker_retry, attacker),
            Err(LinkError::Throttled)
        ));

        // 来源 B（合法发起方）用正确码依旧能成功配对——不受来源 A 的节流
        // 状态牵连。
        let hello = victim_init.build_hello(&real_code, &self_info("L"), vec![]);
        assert!(responder.handle_hello(&hello, victim).is_ok());
    }

    #[test]
    fn regenerating_code_invalidates_previous() {
        // 生成新码必须让旧码立即失效——同一响应方同一时刻只应有一个有效码，
        // 否则「刷新配对码」挤不掉已被窥屏泄露的旧码。
        let responder = PairingResponder::new(LinkIdentity::generate(), self_info("NAS"));
        let initiator = PairingInitiator::new(LinkIdentity::generate());
        let old_code = responder.generate_code();
        let new_code = responder.generate_code();
        assert_ne!(old_code, new_code, "极小概率随机撞码，重跑即可");
        let hello = initiator.build_hello(&old_code, &self_info("L"), vec![]);
        assert!(matches!(
            responder.handle_hello(&hello, None),
            Err(LinkError::InvalidCode)
        ));
    }

    #[test]
    fn degenerate_eph_pub_rejected() {
        // 全零 32 字节是 X25519 的退化点：无论对端私钥是什么，协商出的共享
        // 密钥恒为全零。x25519_dalek::PublicKey::from([u8; 32]) 不做点校验，
        // handle_hello 必须在 ECDH 之后显式拒绝。这里手工构造 hello（而非走
        // `PairingInitiator::build_hello`，因为它内部随机生成临时公钥，拿不到
        // 全零值），让签名先通过，才能验证到达 ECDH 之后的拒绝分支。
        let responder = PairingResponder::new(LinkIdentity::generate(), self_info("NAS"));
        let initiator_id = LinkIdentity::generate();
        let code = responder.generate_code();
        let degenerate_eph_pub = [0u8; 32];
        let addrs: Vec<String> = Vec::new();
        let transcript = transcript_init(
            &code,
            &degenerate_eph_pub,
            &initiator_id.public_bytes(),
            "L",
            None,
            None,
            &addrs,
        );
        let hello = HelloRequest {
            code,
            initiator_eph_pub: degenerate_eph_pub,
            initiator_id_pub: initiator_id.public_bytes(),
            initiator_sig: initiator_id.sign(&transcript),
            name: "L".to_string(),
            platform: None,
            app_version: None,
            initiator_addrs: vec![],
        };
        assert!(matches!(
            responder.handle_hello(&hello, None),
            Err(LinkError::BadPayload(_))
        ));
    }
}
