//! `ProxyMode::Auto`——「直连起飞，后台比较，择快切换」的决策机器。
//!
//! # 设计（成本递增阶梯）
//!
//! Auto 模式下任务一律**直连启动**（零启动阻塞，且直连态保留多 CDN 聚合
//! 资格——这是相对 System 模式的核心收益）。任务越过 coordinator 爬升期
//! 且仍有足够数据时，经全部可用候选代理（手动字段 + 系统代理，端点相同则
//! 去重）并行拉取 256KB 样本；取最快代理与直连单连接均速比较，达到 2×
//! 才热切换（[`NodePool`] 换 SYS client，新分段自然走胜出代理，已下字节
//! 零丢弃）。比较采用相同的单连接量纲，不以直连总吞吐作前置否决——多分段
//! 总速会掩盖每条连接都很慢的 GitHub release 等场景。
//!
//! # 决策缓存（两层，风险不对称）
//!
//! 决策以 host 为 key 缓存在内存（[`DecisionCache`]），租约制且记录胜出
//! 来源——网络环境易变，重启清零回到直连是特性。同 host 批量任务只探测
//! 一次，后续任务按来源采纳。跨重启另有 [`crate::route_health`] 持久化
//! 先验，三态消费（见 `RouteHint`）：Cooldown/NoSwitch 落盘（过期无害，
//! 指数退避抑制重复采样）；Proxy 胜绩单日仅作加速信号（缩短
//! [`MIN_RUNTIME`] 等待期）。持久层未记录代理来源，因此 ≥2 天且 72h 内
//! 有实证胜出的 AdoptProxy 仅在当前只有一个候选时直接代理起飞；手动与
//! 系统代理并存时回到直连并快速并行复评，绝不猜旧胜者。「持久化代理
//! 决策 + 代理失效」的锁死由两道自愈闭环杜绝：传输失败后按未尝试链路
//! 换手动代理/系统代理/直连 + 72h 实证重验。
//!
//! # 完整性防线（前置，不做事后回退）
//!
//! 采样响应的 `ETag`/`Last-Modified` 与任务已锁定的 validator 不一致
//! （代理命中不同 CDN edge）→ **拒绝切换**并写 [`Decision::NoSwitch`]，
//! 宁慢不错。切换只在采样证实内容一致后发生，因此不需要「切换后 ETag
//! 校验失败再回退」的复杂机器——切换后的分段请求沿用既有 If-Range 防线。
//!
//! # 路由可追溯
//!
//! 每个任务的最终链路以 wire 标签写入 `tasks.auto_route` 并广播
//! [`EngineEvent::TaskRouteChanged`]，详情面板据此展示。标签见
//! [`route`] 常量组。

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::{Duration, Instant};

use crate::cdn::NodePool;
use crate::db::Db;
use crate::events::{EngineEvent, EventSink};
use crate::logger::{log_error, log_info};
use crate::proxy_config::{ProxyConfig, ProxyMode, detect_system_proxy};

// ---------------------------------------------------------------------------
// 常量（刻意不做设置项：没有证据表明用户需要调它们）
// ---------------------------------------------------------------------------

/// 任务须运行满该时长才允许采样——跨过 coordinator 3 个完整 ramp tick
/// （2s/tick），避免爬升期的假慢。
const MIN_RUNTIME: Duration = Duration::from_secs(6);

/// 持久化先验记录该 host 有代理胜绩时的采样等待期。已有实证允许提前一个
/// ramp tick 重评估；2× 胜出滞回仍负责挡住临界误切。
const FAST_REEVAL_MIN_RUNTIME: Duration = Duration::from_secs(4);

/// 剩余字节低于此值不采样——小尾巴切换收益覆盖不了探测成本。
const MIN_REMAINING_BYTES: i64 = 4 * 1024 * 1024;

/// 代理须达到直连单连接均速的倍数才切换（滞回，防临界震荡）。
const ADVANTAGE_RATIO: f64 = 2.0;

/// 采样请求拉取的字节数（`Range: bytes=0-262143`）。
const PROBE_RANGE_BYTES: u64 = 256 * 1024;

/// 采样总超时（连接 + 读满 256KB）。超时视为代理无优势。
const PROBE_TIMEOUT: Duration = Duration::from_secs(8);

/// 「走代理」租约时长。到期后新任务回到直连重新评估——决策是租约不是烙印。
const PROXY_LEASE_TTL: Duration = Duration::from_secs(600);

/// 采样无优势后的冷却时长（该 host 内不再重复采样）。
const COOLDOWN_TTL: Duration = Duration::from_secs(300);

/// validator 不一致（代理命中不同 CDN edge）后的禁切换时长。
const NO_SWITCH_TTL: Duration = Duration::from_secs(1800);

// ---------------------------------------------------------------------------
// 路由标签（wire 契约：DB `tasks.auto_route`、TaskRouteChanged 事件、
// api TaskDto.autoRoute、hub 信号与 Web WS 逐字一致）
// ---------------------------------------------------------------------------

/// `tasks.auto_route` 的 wire 标签。空串 = 非 Auto 模式（或任务尚未启动过）。
///
/// 代理类标签带候选来源后缀 `:system`（系统代理检测）/ `:manual`（手动
/// 字段回退），UI 通用解析后缀展示「最终用的是谁」；无后缀的裸标签是
/// 旧库存量值，语义不变。
pub mod route {
    use super::CandidateSource;

    /// Auto 直连（默认路径，未采样或守卫未命中）。
    pub const DIRECT: &str = "direct";
    /// 采样过，直连胜（代理无优势/采样失败）。
    pub const DIRECT_SAMPLED: &str = "direct:sampled";
    /// 采样发现 validator 不一致，拒绝切换（完整性优先）。
    pub const DIRECT_PINNED: &str = "direct:pinned";
    /// 代理失败后自动回退直连。
    pub const DIRECT_FAILOVER: &str = "direct:failover";
    /// 本任务采样后热切换到代理。
    pub const PROXY_SAMPLED: &str = "proxy:sampled";
    /// 启动时采纳了缓存的域名级代理决策。
    pub const PROXY_CACHED: &str = "proxy:cached";
    /// 直连失败后经代理自动换路重试。
    pub const PROXY_FAILOVER: &str = "proxy:failover";
    /// 带来源后缀的代理标签（base × {system,manual}，全部静态）。
    pub const PROXY_SAMPLED_SYSTEM: &str = "proxy:sampled:system";
    pub const PROXY_SAMPLED_MANUAL: &str = "proxy:sampled:manual";
    pub const PROXY_CACHED_SYSTEM: &str = "proxy:cached:system";
    pub const PROXY_CACHED_MANUAL: &str = "proxy:cached:manual";
    pub const PROXY_FAILOVER_SYSTEM: &str = "proxy:failover:system";
    pub const PROXY_FAILOVER_MANUAL: &str = "proxy:failover:manual";

    /// 代理基础标签 + 候选来源 → 带后缀标签（非代理基础标签原样返回）。
    pub fn with_source(base: &'static str, source: CandidateSource) -> &'static str {
        match (base, source) {
            (PROXY_SAMPLED, CandidateSource::System) => PROXY_SAMPLED_SYSTEM,
            (PROXY_SAMPLED, CandidateSource::ManualFields) => PROXY_SAMPLED_MANUAL,
            (PROXY_CACHED, CandidateSource::System) => PROXY_CACHED_SYSTEM,
            (PROXY_CACHED, CandidateSource::ManualFields) => PROXY_CACHED_MANUAL,
            (PROXY_FAILOVER, CandidateSource::System) => PROXY_FAILOVER_SYSTEM,
            (PROXY_FAILOVER, CandidateSource::ManualFields) => PROXY_FAILOVER_MANUAL,
            _ => base,
        }
    }

    /// 任意代理标签 → failover 变体（保留来源后缀；非代理标签原样返回）。
    pub fn to_failover(label: &'static str) -> &'static str {
        if !label.starts_with("proxy") {
            return label;
        }
        if label.ends_with(":system") {
            PROXY_FAILOVER_SYSTEM
        } else if label.ends_with(":manual") {
            PROXY_FAILOVER_MANUAL
        } else {
            PROXY_FAILOVER
        }
    }
}

/// Auto 候选代理的来源（决定 wire 标签后缀，供 UI 展示「最终用的是谁」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CandidateSource {
    /// 系统代理检测命中（Windows 注册表等）。
    System,
    /// 用户在设置中填写的手动代理地址。
    ManualFields,
}

// ---------------------------------------------------------------------------
// 决策缓存
// ---------------------------------------------------------------------------

/// host 级路由决策。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// 该 host 走指定来源的代理（租约期内新任务启动即采纳）。
    Proxy(CandidateSource),
    /// 采样过全部候选均无优势，冷却期内不再采样。
    Cooldown,
    /// 全部可用代理均出现 validator 不一致，禁止切换（完整性防线）。
    NoSwitch,
}

/// 内存态 host→决策缓存（TTL 过期即失效）。Clone 共享同一份内部表。
#[derive(Clone, Default)]
pub struct DecisionCache {
    inner: Arc<StdMutex<HashMap<String, (Decision, Instant)>>>,
}

impl DecisionCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// 查询未过期的决策；过期条目顺手清除。
    pub fn lookup(&self, host: &str) -> Option<Decision> {
        let mut map = match self.inner.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        match map.get(host) {
            Some(&(decision, until)) if Instant::now() < until => Some(decision),
            Some(_) => {
                map.remove(host);
                None
            }
            None => None,
        }
    }

    pub fn set(&self, host: &str, decision: Decision) {
        let ttl = match decision {
            Decision::Proxy(_) => PROXY_LEASE_TTL,
            Decision::Cooldown => COOLDOWN_TTL,
            Decision::NoSwitch => NO_SWITCH_TTL,
        };
        let mut map = match self.inner.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        map.insert(host.to_string(), (decision, Instant::now() + ttl));
    }

    /// 仅清除一个 host 的内存决策。代理链路失败时使用；不写全局冷却，
    /// 让同 host 的另一个代理候选仍可接管。
    pub fn clear_host(&self, host: &str) {
        let mut map = match self.inner.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        map.remove(host);
    }

    /// 代理设置变更时清空全部决策（旧决策针对旧候选代理，已无意义）。
    pub fn clear(&self) {
        let mut map = match self.inner.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        map.clear();
    }
}

// ---------------------------------------------------------------------------
// 候选代理解析
// ---------------------------------------------------------------------------

/// 已解析为可直接构建 client 的 Auto 代理候选。
#[derive(Debug, Clone)]
pub struct ProxyCandidate {
    pub config: ProxyConfig,
    pub source: CandidateSource,
}

/// 解析 Auto 模式的全部候选代理：手动字段与系统代理同时存在时全部返回；
/// 完全相同的代理 URL 只保留手动候选（显式配置优先，避免重复采样同一端点）。
///
/// 仅对 `mode == Auto` 的配置有意义；其余模式返回空列表。
pub fn resolve_candidates(config: &ProxyConfig) -> Vec<ProxyCandidate> {
    let system = match detect_system_proxy() {
        Ok(system) => system,
        Err(error) => {
            log_info!("[auto-proxy] 系统代理检测失败: {error}");
            None
        }
    };
    resolve_candidates_with_system(config, system)
}

fn resolve_candidates_with_system(
    config: &ProxyConfig,
    system: Option<ProxyConfig>,
) -> Vec<ProxyCandidate> {
    if config.mode != ProxyMode::Auto {
        return Vec::new();
    }

    let mut candidates = Vec::with_capacity(2);
    if !config.host.is_empty() && config.port != 0 {
        let mut manual = config.clone();
        manual.mode = ProxyMode::Manual;
        candidates.push(ProxyCandidate {
            config: manual,
            source: CandidateSource::ManualFields,
        });
    }
    if let Some(system) = system {
        let system_url = system.to_proxy_url();
        let duplicate = candidates
            .iter()
            .any(|candidate| candidate.config.to_proxy_url() == system_url);
        if !duplicate {
            candidates.push(ProxyCandidate {
                config: system,
                source: CandidateSource::System,
            });
        }
    }
    candidates
}

// ---------------------------------------------------------------------------
// 任务级上下文（manager 构造，穿透 DownloadParams 进 coordinator）
// ---------------------------------------------------------------------------

/// Auto 直连启动的任务携带的切换上下文。仅当至少一个候选代理存在且任务
/// 以直连起飞时构造；缓存已判代理/无候选的任务为 `None`（无可切换项）。
pub struct AutoProxyCtx {
    /// 已解析为 Manual 模式的全部候选代理（手动字段 + 系统代理）。
    pub candidates: Vec<ProxyCandidate>,
    /// manager 级共享决策缓存。
    pub cache: DecisionCache,
    /// 任务 URL 的 host（含端口），决策缓存 key。
    pub host: String,
    /// 任务解析后的有效 UA（任务 > 队列 > 全局），切换 client 与采样
    /// client 与任务 client 保持一致。
    pub user_agent: String,
    /// 持久化先验有该 host 的代理胜绩——缩短采样等待期。
    pub fast_reeval: bool,
    /// 任务已有局部数据时必须实际采样并核对 validator，不能直接采纳
    /// 兄弟任务写入的代理租约。
    pub require_validation: bool,
}

// ---------------------------------------------------------------------------
// 采样
// ---------------------------------------------------------------------------

/// 一次代理采样的结果。
struct ProbeOutcome {
    /// 实测单连接吞吐（字节/秒）。失败/超时 = 0。
    bps: f64,
    /// 响应 validator 与任务锁定值是否一致（任务无 validator 时恒 true）。
    validators_ok: bool,
    /// 诊断信息（仅日志）。
    detail: String,
}

struct CandidateProbeOutcome {
    candidate: ProxyCandidate,
    outcome: ProbeOutcome,
}

/// 经候选代理拉取 `Range: bytes=0-262143` 并测速。**独立 client、独立
/// 连接**，与进行中的直连下载互不干扰；响应字节直接丢弃（不复用——
/// 复用会把采样与正式下载的 validator 协商纠缠在一起，不值 256KB）。
async fn probe_proxy(
    candidate: &ProxyConfig,
    user_agent: &str,
    url: &str,
    spec: &crate::downloader::RequestSpec,
    task_etag: &str,
    task_last_modified: &str,
) -> ProbeOutcome {
    let fail = |detail: String| ProbeOutcome {
        bps: 0.0,
        validators_ok: true,
        detail,
    };
    let client = match crate::downloader::build_client_with_tls_policy(candidate, user_agent, false)
    {
        Ok(c) => c,
        Err(e) => return fail(format!("build probe client: {e}")),
    };
    let started = Instant::now();
    let run = async {
        let request = crate::downloader::build_request(&client, url, reqwest::Method::GET, spec)
            .header(
                reqwest::header::RANGE,
                format!("bytes=0-{}", PROBE_RANGE_BYTES - 1),
            );
        let resp = request.send().await?;
        let status = resp.status();
        if !(status == reqwest::StatusCode::PARTIAL_CONTENT || status.is_success()) {
            return Err(crate::downloader::DownloadError::Other(format!(
                "probe status {status}"
            )));
        }
        let header_str = |name: reqwest::header::HeaderName| {
            resp.headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string()
        };
        let probe_etag = header_str(reqwest::header::ETAG);
        let probe_lm = header_str(reqwest::header::LAST_MODIFIED);
        // validator 比对：任务锁定了 validator 而代理侧缺失或不同 → 不一致。
        // 任务本身无 validator 时无从校验（直连同样无法防 edge 漂移），放行。
        let validators_ok = (task_etag.is_empty() || task_etag == probe_etag)
            && (task_last_modified.is_empty() || task_last_modified == probe_lm);
        // 读满 256KB 或流结束（服务器无视 Range 回 200 时只取前 256KB 即断开）。
        // 记录首个 body 块的落地时刻与长度：吞吐从它起算（见
        // [`probe_transfer_bps`]）。
        let mut received: u64 = 0;
        let mut first_chunk: Option<(Instant, u64)> = None;
        let mut resp = resp;
        while received < PROBE_RANGE_BYTES {
            match resp.chunk().await? {
                Some(bytes) => {
                    if first_chunk.is_none() {
                        first_chunk = Some((Instant::now(), bytes.len() as u64));
                    }
                    received += bytes.len() as u64;
                }
                None => break,
            }
        }
        Ok::<(u64, Option<(Instant, u64)>, bool), crate::downloader::DownloadError>((
            received,
            first_chunk,
            validators_ok,
        ))
    };
    match tokio::time::timeout(PROBE_TIMEOUT, run).await {
        Ok(Ok((received, first_chunk, validators_ok))) => {
            let total_secs = started.elapsed().as_secs_f64().max(0.001);
            let (body_secs, first_len) = first_chunk
                .map(|(at, len)| (at.elapsed().as_secs_f64(), len))
                .unwrap_or((0.0, 0));
            ProbeOutcome {
                bps: probe_transfer_bps(received, first_len, body_secs, total_secs),
                validators_ok,
                detail: format!("{received}B/{total_secs:.2}s(body {body_secs:.2}s)"),
            }
        }
        Ok(Err(e)) => fail(format!("{e}")),
        Err(_) => fail(format!("timeout {}s", PROBE_TIMEOUT.as_secs())),
    }
}

/// 采样吞吐的纯计算：**从首个 body 块落地起算**——连接/TLS/CONNECT 握手
/// 与 TTFB 是延迟指标，混进吞吐会把高 RTT 代理的真实传输速率低估数倍
/// （256KB 在 150ms RTT 链路上握手就吃掉 ~0.5s；RFC 6349 的 TCP 吞吐
/// 测试同样要求剔除建连阶段）。这也使采样与直连基线（ramp 窗口的纯
/// 传输均速，无握手成分）在同一量纲上比较。首块的字节与耗时一并剔除
/// （其传输时间不可观测）；退化情形（整包单块到齐/时钟异常）回退全程
/// 均速——只会低估不会高估，方向保守（宁不切不误切）。
fn probe_transfer_bps(received: u64, first_chunk_len: u64, body_secs: f64, total_secs: f64) -> f64 {
    let tail_bytes = received.saturating_sub(first_chunk_len);
    if tail_bytes > 0 && body_secs > 0.0 {
        tail_bytes as f64 / body_secs
    } else {
        received as f64 / total_secs.max(0.001)
    }
}

// ---------------------------------------------------------------------------
// 纯决策函数（单测主战场）
// ---------------------------------------------------------------------------

/// 采样守卫：全部满足才允许发起采样。
/// `min_runtime` = 本任务的采样等待期（先验加速时缩短）；`runtime` =
/// 任务本次 spawn 已运行时长；`alive` = 活跃连接数；`remaining` =
/// 剩余字节。吞吐不在守卫里：Auto 比较的是代理与直连的**相对**性能，
/// 直连多连接总吞吐高不代表代理没有显著优势。
fn should_probe(
    min_runtime: Duration,
    runtime: Duration,
    alive: usize,
    remaining_bytes: i64,
    limiter_active: bool,
    conn_sensitive: bool,
) -> bool {
    runtime >= min_runtime
        && alive > 0
        && remaining_bytes >= MIN_REMAINING_BYTES
        && !limiter_active
        && !conn_sensitive
}

/// 切换判定：代理单连接吞吐须 ≥ 2× 直连单连接均速基线。
/// 基线取「采样发起时」与「结果落地时」两个窗口的较大者（保守）。
fn proxy_wins(probe_bps: f64, baseline_per_conn_bps: f64) -> bool {
    probe_bps > 0.0 && probe_bps >= baseline_per_conn_bps * ADVANTAGE_RATIO
}

fn fastest_proxy_winner(
    outcomes: &[CandidateProbeOutcome],
    baseline_per_conn_bps: f64,
) -> Option<&CandidateProbeOutcome> {
    outcomes
        .iter()
        .filter(|result| {
            result.outcome.validators_ok && proxy_wins(result.outcome.bps, baseline_per_conn_bps)
        })
        .max_by(|left, right| left.outcome.bps.total_cmp(&right.outcome.bps))
}

// ---------------------------------------------------------------------------
// coordinator 侧状态机
// ---------------------------------------------------------------------------

/// ramp tick 时喂给状态机的观察值（全部来自 coordinator 现成状态，零新增
/// 统计）。
pub struct TickObs {
    /// 最近 ramp 窗口的总吞吐（字节/秒）。
    pub throughput_bps: f64,
    /// 活跃 worker（连接）数。
    pub alive: usize,
    /// 剩余字节（`effective_total - downloaded`）。
    pub remaining_bytes: i64,
    /// 全局/队列限速是否激活（激活时慢是主动的，不采样）。
    pub limiter_active: bool,
    /// 连接敏感收缩状态（慢不是链路的锅，不采样）。
    pub conn_sensitive: bool,
}

enum Phase {
    /// 等待守卫命中。
    Idle,
    /// 全部代理采样已 off-loop 并行发起，等待结果回流。
    Probing {
        slot: Arc<StdMutex<Option<Vec<CandidateProbeOutcome>>>>,
        baseline_per_conn: f64,
    },
    /// 终局（已切换 / 已放弃），本任务不再动作。
    Done,
}

/// 单任务的 Auto 切换状态机。由 coordinator 在每个完整 ramp tick 驱动。
///
/// 不变式：每任务至多采样一轮、至多切换一次、切换后绝不回切——震荡
/// 由「一次性 + 2× 滞回 + host 冷却」三重压制。
pub struct AutoSwitchState {
    ctx: Arc<AutoProxyCtx>,
    started: Instant,
    phase: Phase,
}

impl AutoSwitchState {
    pub fn new(ctx: Arc<AutoProxyCtx>) -> Self {
        Self {
            ctx,
            started: Instant::now(),
            phase: Phase::Idle,
        }
    }

    /// 测试专用：把状态机的启动时刻回拨，绕过 [`MIN_RUNTIME`] 守卫。
    #[cfg(test)]
    fn backdate(&mut self, by: Duration) {
        self.started = Instant::now() - by;
    }

    /// ramp tick 驱动入口。内部自分派 Idle/Probing 两阶段；Done 后零开销。
    #[allow(clippy::too_many_arguments)]
    pub async fn on_ramp_tick(
        &mut self,
        obs: TickObs,
        nodes: &Arc<NodePool>,
        db: &Db,
        sink: &dyn EventSink,
        task_id: &str,
        url: &str,
        spec: &crate::downloader::RequestSpec,
        etag: &str,
        last_modified: &str,
    ) {
        match &self.phase {
            Phase::Done => {}
            Phase::Idle => {
                let min_runtime = if self.ctx.fast_reeval {
                    FAST_REEVAL_MIN_RUNTIME
                } else {
                    MIN_RUNTIME
                };
                if !should_probe(
                    min_runtime,
                    self.started.elapsed(),
                    obs.alive,
                    obs.remaining_bytes,
                    obs.limiter_active,
                    obs.conn_sensitive,
                ) {
                    return;
                }

                // 缓存短路：兄弟任务已给出该 host 的胜出代理来源。局部续传
                // 必须重新采样 validator，不能仅凭 host 级租约直接换 edge。
                match self.ctx.cache.lookup(&self.ctx.host) {
                    Some(Decision::Proxy(source)) if !self.ctx.require_validation => {
                        if let Some(candidate) = self
                            .ctx
                            .candidates
                            .iter()
                            .find(|candidate| candidate.source == source)
                            .cloned()
                        {
                            self.apply_switch(
                                &candidate,
                                nodes,
                                db,
                                sink,
                                task_id,
                                route::PROXY_CACHED,
                            )
                            .await;
                            return;
                        }
                    }
                    Some(Decision::Cooldown) | Some(Decision::NoSwitch) => {
                        self.phase = Phase::Done;
                        return;
                    }
                    Some(Decision::Proxy(_)) | None => {}
                }

                let slot: Arc<StdMutex<Option<Vec<CandidateProbeOutcome>>>> =
                    Arc::new(StdMutex::new(None));
                let baseline = obs.throughput_bps / obs.alive.max(1) as f64;
                log_info!(
                    "[auto-proxy] task {} host {} 开始三路对比（直连 {:.0} B/s × {} conn，{} 个代理候选）",
                    task_id,
                    self.ctx.host,
                    obs.throughput_bps,
                    obs.alive,
                    self.ctx.candidates.len()
                );
                let probe_slot = slot.clone();
                let candidates = self.ctx.candidates.clone();
                let ua = self.ctx.user_agent.clone();
                let url = url.to_string();
                let spec = spec.clone();
                let etag = etag.to_string();
                let lm = last_modified.to_string();
                // off-loop 并行采样全部候选：绝不阻塞 coordinator 事件循环。
                tokio::spawn(async move {
                    let mut probes = tokio::task::JoinSet::new();
                    for candidate in candidates {
                        let ua = ua.clone();
                        let url = url.clone();
                        let spec = spec.clone();
                        let etag = etag.clone();
                        let lm = lm.clone();
                        probes.spawn(async move {
                            let outcome =
                                probe_proxy(&candidate.config, &ua, &url, &spec, &etag, &lm).await;
                            CandidateProbeOutcome { candidate, outcome }
                        });
                    }
                    let mut outcomes = Vec::with_capacity(probes.len());
                    while let Some(result) = probes.join_next().await {
                        if let Ok(outcome) = result {
                            outcomes.push(outcome);
                        }
                    }
                    let mut guard = match probe_slot.lock() {
                        Ok(g) => g,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                    *guard = Some(outcomes);
                });
                self.phase = Phase::Probing {
                    slot,
                    baseline_per_conn: baseline,
                };
            }
            Phase::Probing {
                slot,
                baseline_per_conn,
            } => {
                let outcomes = {
                    let mut guard = match slot.lock() {
                        Ok(g) => g,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                    guard.take()
                };
                let Some(outcomes) = outcomes else {
                    return; // 采样仍在途，下个 tick 再看。
                };
                // 基线保守取两个窗口的较大者。
                let baseline = baseline_per_conn.max(obs.throughput_bps / obs.alive.max(1) as f64);
                for result in &outcomes {
                    log_info!(
                        "[auto-proxy] task {} host {} {:?} 候选采样 {:.0} B/s（validator={}，{}）",
                        task_id,
                        self.ctx.host,
                        result.candidate.source,
                        result.outcome.bps,
                        result.outcome.validators_ok,
                        result.outcome.detail
                    );
                }
                let winner = fastest_proxy_winner(&outcomes, baseline)
                    .map(|result| (result.candidate.clone(), result.outcome.bps));

                if let Some((candidate, winner_bps)) = winner {
                    log_info!(
                        "[auto-proxy] task {} host {} {:?} 代理胜出（{:.0} vs 基线 {:.0} B/s/conn），热切换",
                        task_id,
                        self.ctx.host,
                        candidate.source,
                        winner_bps,
                        baseline
                    );
                    self.ctx
                        .cache
                        .set(&self.ctx.host, Decision::Proxy(candidate.source));
                    // 持久化胜绩只在切换实际生效后记——client 构建失败的
                    // 候选代理不配留下先验（apply_switch 失败臂已降内存冷却）。
                    if self
                        .apply_switch(&candidate, nodes, db, sink, task_id, route::PROXY_SAMPLED)
                        .await
                    {
                        crate::route_health::record_proxy_win(&self.ctx.host, winner_bps, db);
                    }
                } else if !outcomes.is_empty()
                    && outcomes.iter().all(|result| !result.outcome.validators_ok)
                {
                    log_info!(
                        "[auto-proxy] task {} host {} 全部代理候选 validator 不一致，拒绝切换",
                        task_id,
                        self.ctx.host
                    );
                    self.ctx.cache.set(&self.ctx.host, Decision::NoSwitch);
                    crate::route_health::record_no_switch(&self.ctx.host, db);
                    self.publish(db, sink, task_id, route::DIRECT_PINNED).await;
                    self.phase = Phase::Done;
                } else {
                    log_info!(
                        "[auto-proxy] task {} host {} 全部代理候选均无 2× 优势，保持直连",
                        task_id,
                        self.ctx.host
                    );
                    self.ctx.cache.set(&self.ctx.host, Decision::Cooldown);
                    crate::route_health::record_cooldown(&self.ctx.host, db);
                    self.publish(db, sink, task_id, route::DIRECT_SAMPLED).await;
                    self.phase = Phase::Done;
                }
            }
        }
    }

    /// 构建胜出代理 client 并原子替换 NodePool 的服务节点。失败降级为冷却
    /// （保持直连，任务不受影响）。`base` 是不带来源后缀的代理基础标签，
    /// 此处按胜出候选来源补 `:system`/`:manual` 后缀。返回切换是否实际生效。
    async fn apply_switch(
        &mut self,
        candidate: &ProxyCandidate,
        nodes: &Arc<NodePool>,
        db: &Db,
        sink: &dyn EventSink,
        task_id: &str,
        base: &'static str,
    ) -> bool {
        let label = route::with_source(base, candidate.source);
        let switched = match crate::downloader::build_client_with_tls_policy(
            &candidate.config,
            &self.ctx.user_agent,
            false,
        ) {
            Ok(client) => {
                nodes.switch_to_client(client);
                self.publish(db, sink, task_id, label).await;
                true
            }
            Err(e) => {
                log_error!(
                    "[auto-proxy] task {} {:?} 代理 client 构建失败，保持直连: {e}",
                    task_id,
                    candidate.source
                );
                self.ctx.cache.set(&self.ctx.host, Decision::Cooldown);
                false
            }
        };
        self.phase = Phase::Done;
        switched
    }

    /// 路由落库 + 事件广播（详情面板可追溯的唯一事实源）。
    async fn publish(&self, db: &Db, sink: &dyn EventSink, task_id: &str, label: &str) {
        log_info!(
            "[auto-proxy] task {} host {} 链路定论: {}",
            task_id,
            self.ctx.host,
            label
        );
        if let Err(e) = db.set_task_auto_route(task_id, label).await {
            log_error!("[auto-proxy] task {} 路由落库失败: {e:#}", task_id);
        }
        sink.emit(EngineEvent::TaskRouteChanged {
            task_id: task_id.to_string(),
            route: label.to_string(),
        });
    }
}

// ---------------------------------------------------------------------------
// failover 支路（manager 侧调用）
// ---------------------------------------------------------------------------

/// 路由切换可能修复的传输层错误。覆盖建连失败与 body 中途断流；HTTP
/// 状态、校验失败等永久错误不应靠换链路重试。
pub fn is_route_transport_error(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    lower.contains("connection refused")
        || lower.contains("connection reset")
        || lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("network unreachable")
        || lower.contains("network is down")
        || lower.contains("no route to host")
        || lower.contains("dns")
        || lower.contains("stalled")
        || lower.contains("broken pipe")
        || lower.contains("eof")
        || lower.contains("connection closed")
        || lower.contains("connection abort")
        || lower.contains("incomplete download")
        || lower.contains("error decoding response body")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::proxy_config::ProxyType;

    fn auto_config(host: &str, port: u16) -> ProxyConfig {
        ProxyConfig {
            mode: ProxyMode::Auto,
            proxy_type: ProxyType::Http,
            host: host.to_string(),
            port,
            username: String::new(),
            password: String::new(),
            no_proxy_list: String::new(),
        }
    }

    /// 事件收集 sink（组合测试断言 TaskRouteChanged 广播）。
    struct CollectSink(StdMutex<Vec<EngineEvent>>);
    impl EventSink for CollectSink {
        fn emit(&self, event: EngineEvent) {
            if let Ok(mut v) = self.0.lock() {
                v.push(event);
            }
        }
    }

    fn manual_candidate() -> ProxyCandidate {
        let mut config = auto_config("127.0.0.1", 1);
        config.mode = ProxyMode::Manual;
        ProxyCandidate {
            config,
            source: CandidateSource::ManualFields,
        }
    }

    fn switch_state(cache: &DecisionCache) -> AutoSwitchState {
        let mut state = AutoSwitchState::new(Arc::new(AutoProxyCtx {
            candidates: vec![manual_candidate()],
            cache: cache.clone(),
            host: "example.com".to_string(),
            user_agent: String::new(),
            fast_reeval: false,
            require_validation: false,
        }));
        state.backdate(MIN_RUNTIME + Duration::from_secs(5));
        state
    }

    /// 守卫全过的「慢任务」观察值。
    fn slow_obs() -> TickObs {
        TickObs {
            throughput_bps: 100.0 * 1024.0,
            alive: 2,
            remaining_bytes: 64 * 1024 * 1024,
            limiter_active: false,
            conn_sensitive: false,
        }
    }

    /// 组合契约：缓存判代理的慢任务在 ramp tick 上完成整条切换链——
    /// 构建代理 client（Manual 配置构建不发网络）→ NodePool 换 SYS client
    /// → 路由 `proxy:cached` 落库 → 广播 TaskRouteChanged；终局后续 tick
    /// 零动作（单向切换不变式）。
    #[tokio::test]
    async fn cached_proxy_decision_switches_pool_and_publishes_route() {
        let db = Db::connect("sqlite::memory:").await.expect("mem db");
        db.insert_task(
            "t-auto",
            "https://example.com/f.zip",
            "f.zip",
            "/tmp",
            0,
            0,
            "",
            "",
            "",
            0,
        )
        .await
        .expect("insert task");
        let cache = DecisionCache::new();
        cache.set(
            "example.com",
            Decision::Proxy(CandidateSource::ManualFields),
        );
        let mut state = switch_state(&cache);
        let nodes = NodePool::single(reqwest::Client::new());
        let sink = CollectSink(StdMutex::new(Vec::new()));
        let spec = crate::downloader::RequestSpec::empty_get();

        state
            .on_ramp_tick(
                slow_obs(),
                &nodes,
                &db,
                &sink,
                "t-auto",
                "https://example.com/f.zip",
                &spec,
                "",
                "",
            )
            .await;

        let task = db
            .load_task_by_id("t-auto")
            .await
            .expect("load")
            .expect("row");
        assert_eq!(
            task.auto_route,
            route::PROXY_CACHED_MANUAL,
            "路由必须落库（带候选来源后缀）"
        );
        {
            let events = sink.0.lock().unwrap();
            assert!(
                matches!(
                    events.as_slice(),
                    [EngineEvent::TaskRouteChanged { task_id, route }]
                        if task_id == "t-auto" && route == route::PROXY_CACHED_MANUAL
                ),
                "必须恰好广播一条 TaskRouteChanged"
            );
        }
        // 终局：后续 tick 不再产生任何事件/落库（单向切换）。
        state
            .on_ramp_tick(
                slow_obs(),
                &nodes,
                &db,
                &sink,
                "t-auto",
                "https://example.com/f.zip",
                &spec,
                "",
                "",
            )
            .await;
        assert_eq!(sink.0.lock().unwrap().len(), 1, "Done 后必须零动作");
    }

    /// 冷却/禁切换决策下慢任务保持直连：不落库、不广播、直接终局。
    #[tokio::test]
    async fn cooldown_decision_stays_direct_silently() {
        let db = Db::connect("sqlite::memory:").await.expect("mem db");
        db.insert_task(
            "t-cd",
            "https://example.com/f.zip",
            "f.zip",
            "/tmp",
            0,
            0,
            "",
            "",
            "",
            0,
        )
        .await
        .expect("insert task");
        let cache = DecisionCache::new();
        cache.set("example.com", Decision::Cooldown);
        let mut state = switch_state(&cache);
        let nodes = NodePool::single(reqwest::Client::new());
        let sink = CollectSink(StdMutex::new(Vec::new()));
        let spec = crate::downloader::RequestSpec::empty_get();

        state
            .on_ramp_tick(
                slow_obs(),
                &nodes,
                &db,
                &sink,
                "t-cd",
                "https://example.com/f.zip",
                &spec,
                "",
                "",
            )
            .await;

        let task = db
            .load_task_by_id("t-cd")
            .await
            .expect("load")
            .expect("row");
        assert_eq!(task.auto_route, "", "冷却决策不改写路由基线");
        assert!(sink.0.lock().unwrap().is_empty(), "冷却决策零广播");
    }

    /// 局部续传即使看到兄弟任务的代理租约，也必须先实际采样 validator，
    /// 不能直接切换到可能返回另一份内容的 CDN edge。
    #[tokio::test]
    async fn partial_resume_does_not_adopt_cached_proxy_without_probe() {
        let db = Db::connect("sqlite::memory:").await.expect("mem db");
        db.insert_task(
            "t-partial",
            "https://example.com/f.zip",
            "f.zip",
            "/tmp",
            0,
            0,
            "",
            "",
            "",
            0,
        )
        .await
        .expect("insert task");
        let cache = DecisionCache::new();
        cache.set(
            "example.com",
            Decision::Proxy(CandidateSource::ManualFields),
        );
        let mut state = AutoSwitchState::new(Arc::new(AutoProxyCtx {
            candidates: vec![manual_candidate()],
            cache,
            host: "example.com".to_string(),
            user_agent: String::new(),
            fast_reeval: true,
            require_validation: true,
        }));
        state.backdate(MIN_RUNTIME + Duration::from_secs(1));
        let nodes = NodePool::single(reqwest::Client::new());
        let sink = CollectSink(StdMutex::new(Vec::new()));

        state
            .on_ramp_tick(
                slow_obs(),
                &nodes,
                &db,
                &sink,
                "t-partial",
                "https://example.com/f.zip",
                &crate::downloader::RequestSpec::empty_get(),
                "\"locked\"",
                "",
            )
            .await;

        let task = db
            .load_task_by_id("t-partial")
            .await
            .expect("load")
            .expect("row");
        assert_eq!(task.auto_route, "", "采样完成前不得采纳代理租约");
        assert!(sink.0.lock().unwrap().is_empty(), "采样完成前不得广播切换");
    }

    // ---- DecisionCache ----------------------------------------------------

    #[test]
    fn cache_lookup_returns_fresh_decision() {
        let cache = DecisionCache::new();
        cache.set(
            "example.com",
            Decision::Proxy(CandidateSource::ManualFields),
        );
        assert_eq!(
            cache.lookup("example.com"),
            Some(Decision::Proxy(CandidateSource::ManualFields))
        );
        assert_eq!(cache.lookup("other.com"), None);
    }

    #[test]
    fn cache_clear_drops_all_decisions() {
        let cache = DecisionCache::new();
        cache.set("a.com", Decision::Proxy(CandidateSource::System));
        cache.set("b.com", Decision::Cooldown);
        cache.clear();
        assert_eq!(cache.lookup("a.com"), None);
        assert_eq!(cache.lookup("b.com"), None);
    }

    #[test]
    fn cache_clear_host_preserves_other_hosts() {
        let cache = DecisionCache::new();
        cache.set("a.com", Decision::Proxy(CandidateSource::System));
        cache.set("b.com", Decision::Cooldown);

        cache.clear_host("a.com");

        assert_eq!(cache.lookup("a.com"), None);
        assert_eq!(cache.lookup("b.com"), Some(Decision::Cooldown));
    }

    #[test]
    fn cache_clones_share_state() {
        let cache = DecisionCache::new();
        let clone = cache.clone();
        cache.set("example.com", Decision::NoSwitch);
        assert_eq!(clone.lookup("example.com"), Some(Decision::NoSwitch));
    }

    // ---- 守卫 --------------------------------------------------------------

    /// 每条守卫都必须是**独立的否决权**:从一组全部满足的基线出发,单独破坏
    /// 任意一条都要拒绝放行。
    #[test]
    fn should_probe_requires_all_guards() {
        struct Guards {
            runtime: Duration,
            alive: usize,
            remaining_bytes: i64,
            limiter_active: bool,
            conn_sensitive: bool,
        }
        let ok = |break_one: &dyn Fn(&mut Guards)| {
            let mut g = Guards {
                runtime: Duration::from_secs(7),
                alive: 4,
                remaining_bytes: 64 * 1024 * 1024,
                limiter_active: false,
                conn_sensitive: false,
            };
            break_one(&mut g);
            should_probe(
                MIN_RUNTIME,
                g.runtime,
                g.alive,
                g.remaining_bytes,
                g.limiter_active,
                g.conn_sensitive,
            )
        };
        assert!(ok(&|_| {}), "全守卫满足应放行");
        assert!(
            !ok(&|g| g.runtime = Duration::from_secs(5)),
            "运行不足 6s 拒绝"
        );
        assert!(!ok(&|g| g.alive = 0), "无活跃连接拒绝");
        assert!(!ok(&|g| g.remaining_bytes = 1024 * 1024), "剩余过小拒绝");
        assert!(!ok(&|g| g.limiter_active = true), "限速激活拒绝");
        assert!(!ok(&|g| g.conn_sensitive = true), "连接敏感态拒绝");
        assert!(
            should_probe(
                MIN_RUNTIME,
                Duration::from_secs(7),
                16,
                64 * 1024 * 1024,
                false,
                false,
            ),
            "多连接总吞吐不得掩盖代理相对优势"
        );
    }

    #[test]
    fn fast_reeval_shortens_probe_wait() {
        let probe = |min: Duration, runtime_secs: u64| {
            should_probe(
                min,
                Duration::from_secs(runtime_secs),
                4,
                64 * 1024 * 1024,
                false,
                false,
            )
        };
        assert!(!probe(MIN_RUNTIME, 5), "常规等待期 5s 不放行");
        assert!(probe(FAST_REEVAL_MIN_RUNTIME, 5), "先验加速后 5s 放行");
        assert!(
            !probe(FAST_REEVAL_MIN_RUNTIME, 3),
            "加速至少跨 2 个 ramp tick"
        );
    }

    #[test]
    fn probe_bps_excludes_handshake_latency() {
        // 256KB 经高 RTT 代理：全程 1.2s（含 0.5s 握手+TTFB），body 阶段
        // 0.7s。旧算法 218KB/s；剔除握手后按尾部 240KB/0.7s ≈ 351KB/s——
        // 千兆/百兆用户测得一致的「握手税」不再压低传输速率。
        let total = 256.0 * 1024.0;
        let first = 16u64 * 1024;
        let bps = probe_transfer_bps(total as u64, first, 0.7, 1.2);
        let expect = (total - first as f64) / 0.7;
        assert!((bps - expect).abs() < 1.0, "尾部字节 ÷ body 耗时");
        assert!(bps > total / 1.2, "必须高于含握手的全程均速");
        // 退化：整包单块到齐（tail=0）→ 回退全程均速，保守不高估。
        let one_shot = probe_transfer_bps(first, first, 0.0, 1.2);
        assert!((one_shot - first as f64 / 1.2).abs() < 1.0);
        // 退化：body 时钟异常为 0 → 同样回退。
        let degen = probe_transfer_bps(256 * 1024, 16 * 1024, 0.0, 2.0);
        assert!((degen - 256.0 * 1024.0 / 2.0).abs() < 1.0);
        // 空响应不除零。
        assert_eq!(probe_transfer_bps(0, 0, 0.0, 0.0), 0.0);
    }

    #[test]
    fn proxy_wins_requires_double_advantage() {
        assert!(proxy_wins(1_000_000.0, 400_000.0), "2.5x 应切换");
        assert!(proxy_wins(800_000.0, 400_000.0), "恰 2x 应切换");
        assert!(!proxy_wins(700_000.0, 400_000.0), "1.75x 不切换");
        assert!(!proxy_wins(0.0, 0.0), "采样失败（0 bps）不切换");
    }

    #[test]
    fn fastest_proxy_winner_compares_every_valid_candidate() {
        let manual = CandidateProbeOutcome {
            candidate: manual_candidate(),
            outcome: ProbeOutcome {
                bps: 2_000_000.0,
                validators_ok: false,
                detail: "mismatch".to_string(),
            },
        };
        let mut system_candidate = manual_candidate();
        system_candidate.source = CandidateSource::System;
        system_candidate.config.port = 2;
        let system = CandidateProbeOutcome {
            candidate: system_candidate,
            outcome: ProbeOutcome {
                bps: 1_000_000.0,
                validators_ok: true,
                detail: "ok".to_string(),
            },
        };

        let outcomes = [manual, system];
        let winner = fastest_proxy_winner(&outcomes, 400_000.0).expect("system should win");

        assert_eq!(winner.candidate.source, CandidateSource::System);
    }

    // ---- 候选解析 ----------------------------------------------------------

    #[test]
    fn resolve_candidates_ignores_non_auto_modes() {
        let mut cfg = auto_config("127.0.0.1", 7890);
        let mut system = auto_config("127.0.0.1", 7891);
        system.mode = ProxyMode::Manual;
        cfg.mode = ProxyMode::Manual;
        assert!(resolve_candidates_with_system(&cfg, Some(system.clone())).is_empty());
        cfg.mode = ProxyMode::None;
        assert!(resolve_candidates_with_system(&cfg, Some(system)).is_empty());
    }

    #[test]
    fn resolve_candidates_keeps_manual_and_distinct_system_proxy() {
        let cfg = auto_config("127.0.0.1", 7890);
        let mut system = auto_config("127.0.0.1", 7891);
        system.mode = ProxyMode::Manual;

        let candidates = resolve_candidates_with_system(&cfg, Some(system));

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].source, CandidateSource::ManualFields);
        assert_eq!(candidates[0].config.port, 7890);
        assert_eq!(candidates[1].source, CandidateSource::System);
        assert_eq!(candidates[1].config.port, 7891);
    }

    #[test]
    fn resolve_candidates_deduplicates_identical_proxy_endpoint() {
        let cfg = auto_config("127.0.0.1", 7890);
        let mut system = cfg.clone();
        system.mode = ProxyMode::Manual;

        let candidates = resolve_candidates_with_system(&cfg, Some(system));

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].source, CandidateSource::ManualFields);
    }

    #[test]
    fn resolve_candidates_accepts_system_without_manual_fields() {
        let cfg = auto_config("", 0);
        let mut system = auto_config("127.0.0.1", 7891);
        system.mode = ProxyMode::Manual;

        let candidates = resolve_candidates_with_system(&cfg, Some(system));

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].source, CandidateSource::System);
    }

    // ---- 路由标签组合 --------------------------------------------------------

    #[test]
    fn route_source_suffix_composition() {
        use route::*;
        assert_eq!(
            with_source(PROXY_SAMPLED, CandidateSource::System),
            PROXY_SAMPLED_SYSTEM
        );
        assert_eq!(
            with_source(PROXY_CACHED, CandidateSource::ManualFields),
            PROXY_CACHED_MANUAL
        );
        // 非代理基础标签不带后缀。
        assert_eq!(with_source(DIRECT, CandidateSource::System), DIRECT);
        assert_eq!(DIRECT_FAILOVER, "direct:failover");
        // failover 变体保留来源后缀；裸标签（旧库存量）不造假来源。
        assert_eq!(to_failover(PROXY_CACHED_SYSTEM), PROXY_FAILOVER_SYSTEM);
        assert_eq!(to_failover(PROXY_SAMPLED_MANUAL), PROXY_FAILOVER_MANUAL);
        assert_eq!(to_failover(PROXY_CACHED), PROXY_FAILOVER);
        assert_eq!(to_failover(DIRECT), DIRECT);
    }

    // ---- failover 错误分类 ---------------------------------------------------

    #[test]
    fn route_transport_errors_include_connect_and_midstream_failures() {
        assert!(is_route_transport_error(
            "Connection refused (os error 111)"
        ));
        assert!(is_route_transport_error("operation timed out"));
        assert!(is_route_transport_error("dns error: no record"));
        assert!(is_route_transport_error("download stalled for 5s"));
        assert!(is_route_transport_error("unexpected EOF"));
        assert!(is_route_transport_error("error decoding response body"));
        assert!(!is_route_transport_error("HTTP 404 Not Found"));
        assert!(!is_route_transport_error("checksum mismatch"));
    }
}
