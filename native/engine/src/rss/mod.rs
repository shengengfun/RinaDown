//! RSS 订阅自动下载（issue #97，设计文档 `docs/rss-subscription-design.md`）。
//!
//! # 形状
//!
//! 与 BT tracker 订阅（[`crate::tracker_subscription`]）、插件惰性解析同款的
//! 「**actor tick → 纯函数判定 → off-actor 抓取 → mpsc 回流 → actor 落库**」
//! 结构：
//!
//! ```text
//! 宿主 actor tick ──▶ RssManager::tick()
//!                        ├─ due_sources()           纯函数，可单测
//!                        └─ spawn 抓取（off-actor）── HTTP + feed 解析
//!                                                      │
//!                        ┌─────────────────────────────┘ mpsc 回流
//!                        ▼
//!  DownloadManager::on_rss_event()
//!        ├─ RssManager::apply_fetch()   去重/过滤/落库/状态机（不建任务）
//!        └─ create_task(NewTaskSpec)    任务创建的唯一收敛点
//! ```
//!
//! **抓取必须 off-actor**：宿主 actor 跑在 `current_thread` runtime 上，网络
//! IO 直接 `await` 在事件循环里会冻住整个 App。
//!
//! # 三层去重
//!
//! 1. `guid`（[`parser`] 的回退链）——同一条目永不重复入库；
//! 2. 单轮上限 `max_per_fetch`——超额条目留在 [`RssItemStatus::New`]，下一轮
//!    按发布时间**从旧到新**继续派发，不丢也不插队；
//! 3. 智能剧集去重（可选）——同源同集只下一个字幕组版本，**识别失败即放行**。

pub mod filter;
pub mod model;
pub mod parser;

use std::collections::HashSet;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;

use crate::db::Db;
use crate::downloader;
use crate::events::{EngineEvent, EventSink};
use crate::logger::{log_error, log_info};
use crate::proxy_config::ProxyConfig;
use crate::rss::filter::{CompiledRule, FilterRule, Verdict};
use crate::rss::model::{RssItemInfo, RssItemStatus, RssSourceInfo};
use crate::rss::parser::{MAX_FEED_BYTES, ParsedFeed, parse_feed};

/// 每源保留的条目上限（超量淘汰最旧的**非已下载**条目）。
pub const MAX_ITEMS_PER_SOURCE: i32 = 500;
/// 失败退避的封顶间隔（6 小时）。
pub const MAX_BACKOFF_SECS: i64 = 6 * 3600;
/// 单次 feed 抓取的超时。
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);
/// 单个 `.torrent` 文件的大小上限（4 MiB）。真实种子几 KB 到几百 KB；
/// 超过这个量级的响应基本可以断定不是种子（登录页 / 误配的直链）。
const MAX_TORRENT_BYTES: usize = 4 * 1024 * 1024;
/// 首轮抓取的历史条目所带的原因码。
pub const REASON_SEED_SKIPPED: &str = "seed_skipped";

/// off-actor 抓取的回流结果。
#[derive(Debug)]
pub struct RssFetchOutcome {
    /// 发起抓取的订阅。
    pub source_id: String,
    /// 解析出的 feed（`error` 非空时为默认值）。
    pub feed: ParsedFeed,
    /// 失败原因（空 = 成功）。
    pub error: String,
}

/// 新建订阅向导第二步的只读验证结果（不落库、不建任务）。
#[derive(Debug)]
pub struct RssValidateOutcome {
    /// 调用方给的请求 ID，用于把结果配回发起的对话框。
    pub request_id: String,
    /// 被验证的 feed 地址。
    pub url: String,
    /// feed 标题（供回填订阅名）。
    pub feed_title: String,
    /// 条目预览（`source_id` 为空的瞬态 [`RssItemInfo`]）。
    pub items: Vec<RssItemInfo>,
    /// 失败原因（空 = 验证通过）。
    pub error: String,
}

/// off-actor worker 回流到 actor 的两类结果。
#[derive(Debug)]
pub enum RssEvent {
    /// 定时/手动抓取完成。
    Fetched(Box<RssFetchOutcome>),
    /// 新建向导的 feed 验证完成。
    Validated(Box<RssValidateOutcome>),
    /// `.torrent` 字节抓取完成——BT 条目建任务的第二段（见
    /// [`RssDownloadPlan::is_torrent_file`]）。
    TorrentReady(Box<RssTorrentOutcome>),
}

/// 为一个 BT 条目抓取 `.torrent` 字节的结果。
#[derive(Debug)]
pub struct RssTorrentOutcome {
    /// 原样带回的建任务指令。
    pub plan: Box<RssDownloadPlan>,
    /// 种子文件内容（`error` 非空时为空）。
    pub bytes: Vec<u8>,
    /// 失败原因（空 = 成功）。
    pub error: String,
}

/// 一条「应当为该条目建任务」的指令。
///
/// [`RssManager`] 只做判定与落库，**不碰任务创建**——建任务必须收敛到
/// [`crate::download_manager::DownloadManager::create_task`] 这唯一入口，
/// 所以判定结果以指令形式交回 `DownloadManager` 执行。
#[derive(Debug, Clone, Default)]
pub struct RssDownloadPlan {
    /// 来源订阅。
    pub source_id: String,
    /// 条目 guid（建完任务后据此回写 `Downloaded` + `task_id`）。
    pub guid: String,
    /// 条目标题（通知文案）。
    pub title: String,
    /// 下载地址（enclosure 优先，回退条目链接）。
    pub url: String,
    /// 订阅配置的保存目录（空 = 由调用方按 队列目录 → 全局目录 兜底）。
    pub save_dir: String,
    /// 目标队列（空 = 主队列）。
    pub queue_id: String,
    /// 是否以 paused 落库。
    pub start_paused: bool,
    /// 请求 Cookie。
    pub cookies: String,
    /// 订阅级 UA（空 = 队列/全局）。
    pub user_agent: String,
    /// 订阅级代理（空 = 全局）。
    pub proxy_url: String,
    /// Referer（`send_referer` 关时为空）。
    pub referrer: String,
    /// enclosure 声明大小（0 = 未知）。**仅供展示，绝不当作 `hint_file_size`**：
    /// BT feed 的 `<enclosure length>` 描述的是种子**内容**总大小（几百 MB 的
    /// 番剧），而 enclosure 本身只是几 KB 的 `.torrent`。拿它当文件大小 hint
    /// 会让引擎按几百 MB 规划多段并发并跳过 probe——首段立刻 truncated、后续
    /// 段全 416，还会把站点误学成「只支持 2 连接」污染域名策略缓存。
    pub size_hint: i64,
    /// 该订阅是否开启「自动下载时通知」。
    pub notify: bool,
}

impl RssDownloadPlan {
    /// 下载地址是否指向一个 `.torrent` 文件。
    ///
    /// 引擎的 BT 判定（`is_bt_url`）只认 `magnet:` 与 `torrent-file://` 哨兵，
    /// **HTTP 的 `.torrent` 直链会被当成普通文件下载**——那样订阅 Mikan 只会
    /// 攒下一堆 `.torrent`，番剧本体一个都不下。命中本判定的条目要先把种子
    /// 字节抓下来，再以 `torrent_file_bytes` 建真正的 BT 任务。
    ///
    /// # Examples
    ///
    /// ```
    /// use fluxdown_engine::rss::RssDownloadPlan;
    ///
    /// let plan = |url: &str| RssDownloadPlan { url: url.to_string(), ..Default::default() };
    ///
    /// assert!(plan("https://mikanani.me/Download/20260727/abc.torrent").is_torrent_file());
    /// // 带 query / 大小写混排照样认
    /// assert!(plan("https://pt.example/dl?id=1&file=x.TORRENT").is_torrent_file());
    /// // magnet 由引擎既有五路分派直接处理，不走种子抓取
    /// assert!(!plan("magnet:?xt=urn:btih:deadbeef").is_torrent_file());
    /// assert!(!plan("https://cdn.example/ep01.mp4").is_torrent_file());
    /// ```
    #[must_use]
    pub fn is_torrent_file(&self) -> bool {
        if crate::bt_downloader::is_magnet_url(&self.url) {
            return false;
        }
        let lowered = self.url.to_ascii_lowercase();
        let path = lowered.split(['?', '#']).next().unwrap_or(&lowered);
        // 少数 PT 站把真实文件名放进 query（`…/dl.php?file=x.torrent`），一并认。
        path.ends_with(".torrent") || lowered.contains(".torrent")
    }
}

/// 订阅调度与条目状态机。
pub struct RssManager {
    db: Db,
    sink: Arc<dyn EventSink>,
    /// 内存镜像（顺序即 `position`）。DB 是事实源，这里避免 tick 每 20 秒扫表。
    sources: Vec<RssSourceInfo>,
    /// 正在抓取中的订阅——防同一源被 tick 与手动刷新重复派发。
    in_flight: HashSet<String>,
    tx: mpsc::UnboundedSender<RssEvent>,
    rx: Option<mpsc::UnboundedReceiver<RssEvent>>,
}

impl RssManager {
    /// 构造（不读库；由 [`RssManager::load`] 装载）。
    pub fn new(db: Db, sink: Arc<dyn EventSink>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            db,
            sink,
            sources: Vec::new(),
            in_flight: HashSet::new(),
            tx,
            rx: Some(rx),
        }
    }

    /// 交出回流接收端给宿主 actor 的事件循环（同 `take_resolve_rx` 惯例，
    /// 只能取一次）。
    pub fn take_event_rx(&mut self) -> Option<mpsc::UnboundedReceiver<RssEvent>> {
        self.rx.take()
    }

    /// 从 DB 装载全部订阅到内存镜像。由 [`crate::Engine::new`] 调用，宿主无需
    /// 记得这一步。
    pub async fn load(&mut self) {
        match self.db.load_all_rss_sources().await {
            Ok(sources) => self.sources = sources,
            Err(e) => log_error!("[rss] failed to load sources: {}", e),
        }
    }

    /// 当前订阅列表（宿主快照请求 / UI 首帧）。
    pub fn sources(&self) -> &[RssSourceInfo] {
        &self.sources
    }

    /// 按 ID 取订阅。
    pub fn source(&self, source_id: &str) -> Option<&RssSourceInfo> {
        self.sources.iter().find(|s| s.source_id == source_id)
    }

    /// 广播订阅列表（含未读计数，重新读库以刷新 badge）。
    pub async fn broadcast_sources(&mut self) {
        self.load().await;
        self.sink
            .emit(EngineEvent::RssSourcesChanged(self.sources.clone()));
    }

    /// 广播某订阅的条目流快照。
    pub async fn broadcast_items(&self, source_id: &str, notify_titles: Vec<String>) {
        let items = self
            .db
            .load_rss_items(source_id, MAX_ITEMS_PER_SOURCE)
            .await
            .unwrap_or_default();
        self.sink.emit(EngineEvent::RssItemsChanged {
            source_id: source_id.to_string(),
            items,
            notify_titles,
        });
    }

    // -----------------------------------------------------------------------
    // CRUD
    // -----------------------------------------------------------------------

    /// 新建订阅。`source.source_id` 为空时自动生成 UUID；返回最终 ID。
    ///
    /// 新订阅的 `last_fetch_at = 0`，因此下一次 tick 立刻抓取——首轮的全部
    /// 历史条目只标记 [`RssItemStatus::SeedSkipped`] 不下载（§2.2）。
    pub async fn create_source(&mut self, mut source: RssSourceInfo) -> Option<String> {
        source.normalize();
        if source.url.is_empty() {
            return None;
        }
        if source.source_id.is_empty() {
            source.source_id = uuid::Uuid::new_v4().to_string();
        }
        source.position = self.db.next_rss_position().await.unwrap_or(0);
        source.last_fetch_at = 0;
        source.seeded = false;
        if let Err(e) = self.db.insert_rss_source(&source).await {
            log_error!("[rss] insert source failed: {}", e);
            return None;
        }
        let id = source.source_id.clone();
        log_info!("[rss] subscribed: {} ({})", source.display_name(), id);
        self.broadcast_sources().await;
        Some(id)
    }

    /// 更新订阅的用户可编辑字段。运行态（退避账本/首轮标记）不受影响。
    pub async fn update_source(&mut self, mut source: RssSourceInfo) -> bool {
        source.normalize();
        if source.source_id.is_empty() || self.source(&source.source_id).is_none() {
            return false;
        }
        if let Err(e) = self.db.update_rss_source(&source).await {
            log_error!("[rss] update source failed: {}", e);
            return false;
        }
        self.broadcast_sources().await;
        true
    }

    /// 删除订阅（级联条目；已创建的下载任务保留）。
    pub async fn delete_source(&mut self, source_id: &str) -> bool {
        if self.source(source_id).is_none() {
            return false;
        }
        if let Err(e) = self.db.delete_rss_source(source_id).await {
            log_error!("[rss] delete source failed: {}", e);
            return false;
        }
        self.in_flight.remove(source_id);
        self.broadcast_sources().await;
        true
    }

    /// 把该源全部「新」条目标记为已读。
    pub async fn mark_all_read(&mut self, source_id: &str) {
        if let Err(e) = self.db.mark_all_rss_items_read(source_id).await {
            log_error!("[rss] mark all read failed: {}", e);
            return;
        }
        self.broadcast_items(source_id, Vec::new()).await;
        self.broadcast_sources().await;
    }

    /// 手动忽略一个条目。
    pub async fn ignore_item(&mut self, source_id: &str, guid: &str) {
        if let Err(e) = self
            .db
            .set_rss_item_status(source_id, guid, RssItemStatus::Ignored, "", "")
            .await
        {
            log_error!("[rss] ignore item failed: {}", e);
            return;
        }
        self.broadcast_items(source_id, Vec::new()).await;
        self.broadcast_sources().await;
    }

    /// 手动下载一个条目（「仍要下载」/「补下」/「重新下载」——绕过规则与
    /// 剧集去重）。
    ///
    /// **任何状态都允许**，包括已下载：任务可能被用户删了、下到一半失败了、
    /// 或者只是想再来一遍。挡住重下没有任何好处，只会逼用户去别处找种子。
    /// 重下会覆盖旧的 `task_id` 回链，指向新任务。
    ///
    /// 条目或订阅不存在时返回 `None`。
    pub async fn manual_download(
        &self,
        source_id: &str,
        guid: &str,
    ) -> Option<Box<RssDownloadPlan>> {
        let source = self.source(source_id)?;
        let item = self.db.rss_item(source_id, guid).await.ok().flatten()?;
        Some(Box::new(plan_for(source, &item)))
    }

    // -----------------------------------------------------------------------
    // 调度
    // -----------------------------------------------------------------------

    /// 定时节拍：派发全部到期订阅的抓取。宿主 actor 每次 tick 调用一次。
    pub fn tick(&mut self, now: i64, proxy: &ProxyConfig, global_ua: &str) {
        let due = due_sources(now, self.sources.iter(), &self.in_flight);
        for id in due {
            self.dispatch_fetch(&id, now, proxy, global_ua);
        }
    }

    /// 立即抓取一个订阅（侧边栏「立即刷新」/ REST `POST /rss/{id}/refresh`）。
    ///
    /// 忽略 due 判定但仍尊重 `in_flight`——连点刷新不该并发打同一个站点。
    pub fn refresh_now(&mut self, source_id: &str, proxy: &ProxyConfig, global_ua: &str) -> bool {
        if self.in_flight.contains(source_id) || self.source(source_id).is_none() {
            return false;
        }
        self.dispatch_fetch(source_id, unix_now(), proxy, global_ua);
        true
    }

    fn dispatch_fetch(&mut self, source_id: &str, now: i64, proxy: &ProxyConfig, global_ua: &str) {
        let Some(source) = self.source(source_id) else {
            return;
        };
        let request = FetchRequest {
            source_id: source.source_id.clone(),
            url: source.url.clone(),
            cookies: source.cookies.clone(),
            user_agent: if source.user_agent.is_empty() {
                global_ua.to_string()
            } else {
                source.user_agent.clone()
            },
            proxy: resolve_proxy(&source.proxy_url, proxy),
        };
        // 乐观置位 last_fetch_at：即便抓取任务本身崩了，due 判定也不会把这个
        // 源变成每 tick 重试的死循环（回流分支会用真实结果覆盖）。
        self.in_flight.insert(request.source_id.clone());
        if let Some(s) = self
            .sources
            .iter_mut()
            .find(|s| s.source_id == request.source_id)
        {
            s.last_fetch_at = now;
        }
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let source_id = request.source_id.clone();
            let outcome = match fetch_feed(&request).await {
                Ok(feed) => RssFetchOutcome {
                    source_id,
                    feed,
                    error: String::new(),
                },
                Err(error) => RssFetchOutcome {
                    source_id,
                    feed: ParsedFeed::default(),
                    error,
                },
            };
            let _ = tx.send(RssEvent::Fetched(Box::new(outcome)));
        });
    }

    /// 构造一个只读验证任务（新建向导 / REST `POST /rss/validate`）。
    ///
    /// 返回 future 而不是自己 spawn：actor 侧的调用方（信号路径）直接
    /// [`tokio::spawn`] 走事件广播；请求-应答的调用方（REST/CLI）在 actor 之外
    /// `await` 它拿返回值。两条路共用同一段抓取+解析逻辑。
    #[allow(clippy::too_many_arguments)]
    pub fn validate_future(
        &self,
        request_id: String,
        url: String,
        cookies: String,
        user_agent: String,
        proxy_url: String,
        proxy: &ProxyConfig,
        global_ua: &str,
    ) -> impl Future<Output = RssValidateOutcome> + Send + use<> {
        let request = FetchRequest {
            source_id: String::new(),
            url: url.clone(),
            cookies,
            user_agent: if user_agent.is_empty() {
                global_ua.to_string()
            } else {
                user_agent
            },
            proxy: resolve_proxy(&proxy_url, proxy),
        };
        async move {
            match fetch_feed(&request).await {
                Ok(feed) => RssValidateOutcome {
                    request_id,
                    url,
                    feed_title: feed.title,
                    items: feed
                        .items
                        .iter()
                        .map(|it| item_from_parsed("", it, 0))
                        .collect(),
                    error: String::new(),
                },
                Err(error) => RssValidateOutcome {
                    request_id,
                    url,
                    feed_title: String::new(),
                    items: Vec::new(),
                    error,
                },
            }
        }
    }

    /// 信号路径的验证：off-actor 跑 [`Self::validate_future`]，结果经回流通道
    /// 广播为 [`EngineEvent::RssFeedValidated`]。
    #[allow(clippy::too_many_arguments)]
    pub fn validate(
        &self,
        request_id: String,
        url: String,
        cookies: String,
        user_agent: String,
        proxy_url: String,
        proxy: &ProxyConfig,
        global_ua: &str,
    ) {
        let fut = self.validate_future(
            request_id, url, cookies, user_agent, proxy_url, proxy, global_ua,
        );
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let _ = tx.send(RssEvent::Validated(Box::new(fut.await)));
        });
    }

    /// 为一个 BT 条目 off-actor 抓取 `.torrent` 字节，结果经回流通道回到
    /// [`crate::download_manager::DownloadManager::on_rss_event`]。
    ///
    /// 这是 BT 条目建任务的**第二段**：第一段（feed 抓取 + 规则判定）产出
    /// plan，这里把种子内容拿到手，最后才在 actor 上以 `torrent_file_bytes`
    /// 调 `create_task`。分两段是因为「哪些条目该下」要查 DB 去重（actor 上
    /// 才能做），而网络 IO 绝不能上 actor。
    pub fn spawn_torrent_fetch(
        &self,
        plan: Box<RssDownloadPlan>,
        proxy: &ProxyConfig,
        global_ua: &str,
    ) {
        let request = FetchRequest {
            source_id: plan.source_id.clone(),
            url: plan.url.clone(),
            cookies: plan.cookies.clone(),
            user_agent: if plan.user_agent.is_empty() {
                global_ua.to_string()
            } else {
                plan.user_agent.clone()
            },
            proxy: resolve_proxy(&plan.proxy_url, proxy),
        };
        let referrer = plan.referrer.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let (bytes, error) = match fetch_torrent(&request, &referrer).await {
                Ok(bytes) => (bytes, String::new()),
                Err(e) => (Vec::new(), e),
            };
            let _ = tx.send(RssEvent::TorrentReady(Box::new(RssTorrentOutcome {
                plan,
                bytes,
                error,
            })));
        });
    }

    /// 广播验证结果。
    pub fn emit_validated(&self, outcome: RssValidateOutcome) {
        self.sink.emit(EngineEvent::RssFeedValidated {
            request_id: outcome.request_id,
            url: outcome.url,
            feed_title: outcome.feed_title,
            items: outcome.items,
            error: outcome.error,
        });
    }

    // -----------------------------------------------------------------------
    // 回流处理
    // -----------------------------------------------------------------------

    /// 消化一次抓取结果：去重 → 过滤判定 → 落库 → 选出应下载的条目。
    ///
    /// 返回的 [`RssDownloadPlan`] 由 `DownloadManager` 逐条走 `create_task`，
    /// 成功后回调 [`RssManager::mark_downloaded`]。
    pub async fn apply_fetch(&mut self, outcome: RssFetchOutcome) -> Vec<RssDownloadPlan> {
        let now = unix_now();
        self.in_flight.remove(&outcome.source_id);
        // 订阅可能在抓取窗口内被删除——静默丢弃，不建表也不建任务。
        let Some(source) = self.source(&outcome.source_id).cloned() else {
            return Vec::new();
        };

        if !outcome.error.is_empty() {
            let fail_count = source.fail_count.saturating_add(1);
            log_error!(
                "[rss] fetch failed ({} consecutive): {}: {}",
                fail_count,
                source.display_name(),
                outcome.error
            );
            self.persist_runtime(
                &source.source_id,
                now,
                source.last_success_at,
                &outcome.error,
                fail_count,
                source.seeded,
                &source.name,
            )
            .await;
            self.broadcast_sources().await;
            return Vec::new();
        }

        let first_round = !source.seeded;
        let known = self
            .db
            .rss_known_guids(&source.source_id)
            .await
            .unwrap_or_default();
        let mut taken = if source.smart_episode {
            self.db
                .rss_taken_episode_keys(&source.source_id)
                .await
                .unwrap_or_default()
        } else {
            HashSet::new()
        };
        let compiled = CompiledRule::new(&rule_of(&source));

        let mut rows: Vec<RssItemInfo> = Vec::new();
        for parsed in &outcome.feed.items {
            if parsed.guid.is_empty() || known.contains(&parsed.guid) {
                continue;
            }
            let mut row = item_from_parsed(&source.source_id, parsed, now);
            if first_round {
                // 首轮：全部标记已读，只展示不下载。仍记录剧集键，让后续新
                // 条目能与历史正确去重。
                row.status = RssItemStatus::SeedSkipped;
                row.reason = REASON_SEED_SKIPPED.to_string();
                if source.smart_episode {
                    row.episode_key = filter::episode_key(&row.title).unwrap_or_default();
                }
            } else {
                match compiled.evaluate(&row.title, row.enclosure_length, &mut taken) {
                    Verdict::Accept { episode_key } => {
                        row.episode_key = episode_key;
                    }
                    Verdict::Reject {
                        reason,
                        episode_key,
                    } => {
                        row.status = reason.item_status();
                        row.reason = reason.code().to_string();
                        row.episode_key = episode_key;
                    }
                }
            }
            rows.push(row);
        }

        // 已知条目不再入库，但解析器可能这一版才补上它们的发布时间（Mikan 的
        // `<torrent><pubDate>`）——单独回填一次，历史行不必等被 prune 掉才有时间。
        let backfill: Vec<(String, i64)> = outcome
            .feed
            .items
            .iter()
            .filter(|p| p.pub_date > 0 && known.contains(&p.guid))
            .map(|p| (p.guid.clone(), p.pub_date))
            .collect();
        let mut backfilled = 0u64;
        if !backfill.is_empty() {
            match self
                .db
                .backfill_rss_pub_dates(&source.source_id, &backfill)
                .await
            {
                Ok(n) => backfilled = n,
                Err(e) => log_error!("[rss] backfill pub_date failed: {}", e),
            }
        }

        let fresh = rows.len();
        if let Err(e) = self.db.insert_rss_items(&rows).await {
            log_error!("[rss] persist items failed: {}", e);
        }
        if let Err(e) = self
            .db
            .prune_rss_items(&source.source_id, MAX_ITEMS_PER_SOURCE)
            .await
        {
            log_error!("[rss] prune items failed: {}", e);
        }

        // 订阅名留空时用 feed 标题回填（只回填一次，之后以用户改名为准）。
        let name = if source.name.is_empty() && !outcome.feed.title.is_empty() {
            outcome.feed.title.clone()
        } else {
            source.name.clone()
        };
        self.persist_runtime(&source.source_id, now, now, "", 0, true, &name)
            .await;

        // 派发：`New` 状态的条目按发布时间从旧到新取，单轮不超过上限。
        // 首轮不派发（本轮全部是 SeedSkipped，但历史遗留的 New 也不该在
        // seeding 这一轮被突然灌下去）。
        let plans = if source.auto_download && !first_round {
            self.db
                .rss_dispatchable_items(&source.source_id, source.max_per_fetch)
                .await
                .unwrap_or_default()
                .iter()
                .map(|item| plan_for(&source, item))
                .collect()
        } else {
            Vec::new()
        };
        log_info!(
            "[rss] {} fetched: {} new item(s), {} to download{}",
            source.display_name(),
            fresh,
            plans.len(),
            if first_round {
                " (first round: history marked read)"
            } else {
                ""
            }
        );
        if fresh > 0 || backfilled > 0 || !plans.is_empty() {
            self.broadcast_items(&source.source_id, Vec::new()).await;
        }
        self.broadcast_sources().await;
        plans
    }

    /// 任务创建成功后回写条目状态与回链。
    pub async fn mark_downloaded(&self, source_id: &str, guid: &str, task_id: &str) {
        if let Err(e) = self
            .db
            .set_rss_item_status(source_id, guid, RssItemStatus::Downloaded, "", task_id)
            .await
        {
            log_error!("[rss] mark downloaded failed: {}", e);
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn persist_runtime(
        &mut self,
        source_id: &str,
        last_fetch_at: i64,
        last_success_at: i64,
        last_error: &str,
        fail_count: i32,
        seeded: bool,
        name: &str,
    ) {
        if let Err(e) = self
            .db
            .set_rss_source_runtime(
                source_id,
                last_fetch_at,
                last_success_at,
                last_error,
                fail_count,
                seeded,
                name,
            )
            .await
        {
            log_error!("[rss] persist runtime failed: {}", e);
        }
        if let Some(s) = self.sources.iter_mut().find(|s| s.source_id == source_id) {
            s.last_fetch_at = last_fetch_at;
            s.last_success_at = last_success_at;
            s.last_error = last_error.to_string();
            s.fail_count = fail_count;
            s.seeded = seeded;
            s.name = name.to_string();
        }
    }
}

/// 到期判定（纯函数）。
///
/// 到期条件：启用 + 不在抓取中 + `now - last_fetch_at >= 生效间隔`。
/// 生效间隔见 [`effective_interval_secs`]。
///
/// 不依赖墙钟对齐——休眠唤醒后按「距上次抓取多久」判定，天然补触发。
pub fn due_sources<'a>(
    now: i64,
    sources: impl Iterator<Item = &'a RssSourceInfo>,
    in_flight: &HashSet<String>,
) -> Vec<String> {
    sources
        .filter(|s| s.enabled && !in_flight.contains(&s.source_id))
        .filter(|s| now.saturating_sub(s.last_fetch_at) >= effective_interval_secs(s))
        .map(|s| s.source_id.clone())
        .collect()
}

/// 生效抓取间隔（秒）= `interval_minutes × 2^fail_count`，封顶
/// [`MAX_BACKOFF_SECS`]，但**绝不短于**用户配置的间隔（配了 24h 的源不会因为
/// 封顶被拉快到 6h）。
///
/// 连续失败不自动停用订阅——私有 token feed 的抖动是常态，退避 + 侧边栏警告
/// 点已经够（§2.2）。
///
/// # Examples
///
/// ```
/// use fluxdown_engine::rss::effective_interval_secs;
/// use fluxdown_engine::rss::model::RssSourceInfo;
///
/// let healthy = RssSourceInfo { interval_minutes: 30, ..Default::default() };
/// assert_eq!(effective_interval_secs(&healthy), 1800);
///
/// let failing = RssSourceInfo { interval_minutes: 30, fail_count: 3, ..Default::default() };
/// assert_eq!(effective_interval_secs(&failing), 1800 * 8);
///
/// // 封顶 6h
/// let dead = RssSourceInfo { interval_minutes: 30, fail_count: 20, ..Default::default() };
/// assert_eq!(effective_interval_secs(&dead), 6 * 3600);
/// ```
#[must_use]
pub fn effective_interval_secs(source: &RssSourceInfo) -> i64 {
    let base = i64::from(source.interval_minutes.max(model::MIN_INTERVAL_MINUTES)) * 60;
    let shift = source.fail_count.clamp(0, 32) as u32;
    let scaled = base.checked_shl(shift).unwrap_or(i64::MAX);
    scaled.min(MAX_BACKOFF_SECS).max(base)
}

/// 当前 Unix 秒。
#[must_use]
pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn rule_of(source: &RssSourceInfo) -> FilterRule {
    FilterRule {
        include: source.include_pattern.clone(),
        exclude: source.exclude_pattern.clone(),
        use_regex: source.use_regex,
        smart_episode: source.smart_episode,
        size_min_bytes: source.size_min_bytes,
        size_max_bytes: source.size_max_bytes,
    }
}

fn item_from_parsed(source_id: &str, parsed: &parser::ParsedItem, fetched_at: i64) -> RssItemInfo {
    RssItemInfo {
        source_id: source_id.to_string(),
        guid: parsed.guid.clone(),
        title: parsed.title.clone(),
        link: parsed.link.clone(),
        enclosure_url: parsed.enclosure_url.clone(),
        enclosure_length: parsed.enclosure_length,
        pub_date: parsed.pub_date,
        fetched_at,
        status: RssItemStatus::New,
        task_id: String::new(),
        episode_key: String::new(),
        reason: String::new(),
    }
}

fn plan_for(source: &RssSourceInfo, item: &RssItemInfo) -> RssDownloadPlan {
    RssDownloadPlan {
        source_id: source.source_id.clone(),
        guid: item.guid.clone(),
        title: item.title.clone(),
        url: item.download_url().to_string(),
        save_dir: source.save_dir.clone(),
        queue_id: source.queue_id.clone(),
        start_paused: source.start_paused,
        cookies: source.cookies.clone(),
        user_agent: source.user_agent.clone(),
        proxy_url: source.proxy_url.clone(),
        referrer: if source.send_referer {
            feed_origin(&source.url)
        } else {
            String::new()
        },
        size_hint: item.enclosure_length,
        notify: source.notify_on_download,
    }
}

/// 订阅级代理覆盖：空 = 用全局解析结果。
fn resolve_proxy(proxy_url: &str, global: &ProxyConfig) -> ProxyConfig {
    if proxy_url.is_empty() {
        global.resolve()
    } else {
        ProxyConfig::from_proxy_url(proxy_url)
    }
}

/// feed 站点根地址，用作 `.torrent` 下载的 Referer（部分 PT 站校验来源）。
/// 解析失败时回退整条 feed 地址。
fn feed_origin(feed_url: &str) -> String {
    url::Url::parse(feed_url)
        .ok()
        .and_then(|u| u.host_str().map(|h| format!("{}://{h}/", u.scheme())))
        .unwrap_or_else(|| feed_url.to_string())
}

struct FetchRequest {
    source_id: String,
    url: String,
    cookies: String,
    user_agent: String,
    proxy: ProxyConfig,
}

/// 抓取并解析一个 feed。**只在 off-actor 任务里调用。**
async fn fetch_feed(req: &FetchRequest) -> Result<ParsedFeed, String> {
    let client = downloader::build_client(&req.proxy, &req.user_agent)
        .map_err(|e| format!("failed to build http client: {e}"))?;
    let mut request = client.get(&req.url).timeout(FETCH_TIMEOUT);
    if !req.cookies.is_empty() {
        request = request.header(reqwest::header::COOKIE, &req.cookies);
    }
    let resp = request.send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    // 先看 Content-Length 早退，再按实际字节兜底——`parse_feed` 内部同样有
    // 上限检查，此处是为了不把超大响应先读满内存。
    if resp
        .content_length()
        .is_some_and(|n| n > MAX_FEED_BYTES as u64)
    {
        return Err("feed too large".to_string());
    }
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    parse_feed(&bytes)
}

/// 抓取一个 `.torrent` 文件的原始字节。**只在 off-actor 任务里调用。**
///
/// 带上订阅级 Cookie 与 Referer——部分 PT 站的种子下载正是靠这两样鉴权
/// （`send_referer` 开关的实际用武之地）。
async fn fetch_torrent(req: &FetchRequest, referrer: &str) -> Result<Vec<u8>, String> {
    let client = downloader::build_client(&req.proxy, &req.user_agent)
        .map_err(|e| format!("failed to build http client: {e}"))?;
    let mut request = client.get(&req.url).timeout(FETCH_TIMEOUT);
    if !req.cookies.is_empty() {
        request = request.header(reqwest::header::COOKIE, &req.cookies);
    }
    if !referrer.is_empty() {
        request = request.header(reqwest::header::REFERER, referrer);
    }
    let resp = request.send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    if resp
        .content_length()
        .is_some_and(|n| n > MAX_TORRENT_BYTES as u64)
    {
        return Err("torrent file too large".to_string());
    }
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    if bytes.len() > MAX_TORRENT_BYTES {
        return Err(format!("torrent file too large ({} bytes)", bytes.len()));
    }
    // 站点没登录时常常返回一个 200 的 HTML 登录页——不校验就会把网页存成
    // 种子再让 librqbit 报一句看不懂的解析错。bencode 字典必以 `d` 起头。
    if bytes.first() != Some(&b'd') {
        return Err("response is not a torrent file (login required?)".to_string());
    }
    Ok(bytes.to_vec())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::collections::HashSet;

    use super::{
        MAX_BACKOFF_SECS, due_sources, effective_interval_secs, feed_origin, plan_for, rule_of,
    };
    use crate::rss::model::{RssItemInfo, RssItemStatus, RssSourceInfo};

    fn source(id: &str, interval: i32, last_fetch: i64) -> RssSourceInfo {
        RssSourceInfo {
            source_id: id.to_string(),
            url: format!("https://feed.test/{id}"),
            interval_minutes: interval,
            last_fetch_at: last_fetch,
            ..Default::default()
        }
    }

    #[test]
    fn brand_new_source_is_due_immediately() {
        let sources = [source("s1", 30, 0)];
        let due = due_sources(1_000_000, sources.iter(), &HashSet::new());
        assert_eq!(due, vec!["s1".to_string()]);
    }

    #[test]
    fn source_is_not_due_before_its_interval_elapses() {
        let now = 1_000_000;
        let sources = [source("s1", 30, now - 1799)];
        assert!(due_sources(now, sources.iter(), &HashSet::new()).is_empty());
        let sources = [source("s1", 30, now - 1800)];
        assert_eq!(due_sources(now, sources.iter(), &HashSet::new()).len(), 1);
    }

    #[test]
    fn disabled_and_in_flight_sources_are_skipped() {
        let now = 1_000_000;
        let mut disabled = source("s1", 30, 0);
        disabled.enabled = false;
        assert!(due_sources(now, [disabled].iter(), &HashSet::new()).is_empty());

        let busy = source("s2", 30, 0);
        let in_flight: HashSet<String> = ["s2".to_string()].into_iter().collect();
        assert!(
            due_sources(now, [busy].iter(), &in_flight).is_empty(),
            "a source already being fetched must not be dispatched again"
        );
    }

    #[test]
    fn backoff_doubles_per_failure_and_caps_at_six_hours() {
        let mut s = source("s1", 30, 0);
        assert_eq!(effective_interval_secs(&s), 1800);
        s.fail_count = 1;
        assert_eq!(effective_interval_secs(&s), 3600);
        s.fail_count = 3;
        assert_eq!(effective_interval_secs(&s), 1800 * 8);
        // 第 4 次失败起 30min×16 = 8h 已越过 6h 封顶
        s.fail_count = 4;
        assert_eq!(effective_interval_secs(&s), MAX_BACKOFF_SECS);
        s.fail_count = 10;
        assert_eq!(effective_interval_secs(&s), MAX_BACKOFF_SECS);
        // 极端 fail_count 不能溢出成负数/零间隔（那会变成每 tick 猛打站点）
        s.fail_count = i32::MAX;
        assert_eq!(effective_interval_secs(&s), MAX_BACKOFF_SECS);
    }

    #[test]
    fn backoff_never_shortens_a_long_user_interval() {
        let mut s = source("s1", 24 * 60, 0);
        s.fail_count = 8;
        assert_eq!(
            effective_interval_secs(&s),
            24 * 3600,
            "capping at 6h must not speed up a 24h subscription"
        );
    }

    #[test]
    fn zero_interval_is_clamped_instead_of_spinning() {
        let s = source("s1", 0, 0);
        assert_eq!(effective_interval_secs(&s), 60);
    }

    #[test]
    fn feed_origin_reduces_to_site_root() {
        assert_eq!(
            feed_origin("https://mikanani.me/RSS/MyBangumi?token=abc"),
            "https://mikanani.me/"
        );
        assert_eq!(feed_origin("not a url"), "not a url");
    }

    #[test]
    fn plan_prefers_enclosure_and_honours_referer_switch() {
        let mut s = source("s1", 30, 0);
        s.url = "https://mikanani.me/RSS/MyBangumi?token=abc".to_string();
        s.queue_id = "anime".to_string();
        s.save_dir = "D:/Anime".to_string();
        let item = RssItemInfo {
            guid: "g1".to_string(),
            title: "[ANi] Show - 02".to_string(),
            link: "https://mikanani.me/Home/Episode/g1".to_string(),
            enclosure_url: "https://mikanani.me/Download/g1.torrent".to_string(),
            enclosure_length: 418 * 1024 * 1024,
            status: RssItemStatus::New,
            ..Default::default()
        };
        let plan = plan_for(&s, &item);
        assert_eq!(plan.url, "https://mikanani.me/Download/g1.torrent");
        assert_eq!(plan.referrer, "https://mikanani.me/");
        assert_eq!(plan.queue_id, "anime");
        assert_eq!(plan.size_hint, 418 * 1024 * 1024);

        s.send_referer = false;
        assert!(plan_for(&s, &item).referrer.is_empty());

        // 无 enclosure 时回退条目链接
        let no_enclosure = RssItemInfo {
            enclosure_url: String::new(),
            ..item
        };
        assert_eq!(
            plan_for(&s, &no_enclosure).url,
            "https://mikanani.me/Home/Episode/g1"
        );
    }

    #[test]
    fn rule_projection_carries_every_filter_field() {
        let s = RssSourceInfo {
            include_pattern: "1080P".to_string(),
            exclude_pattern: "720P".to_string(),
            use_regex: true,
            smart_episode: true,
            size_min_bytes: 100,
            size_max_bytes: 200,
            ..Default::default()
        };
        let rule = rule_of(&s);
        assert_eq!(rule.include, "1080P");
        assert_eq!(rule.exclude, "720P");
        assert!(rule.use_regex);
        assert!(rule.smart_episode);
        assert_eq!(rule.size_min_bytes, 100);
        assert_eq!(rule.size_max_bytes, 200);
    }

    // -----------------------------------------------------------------------
    // 状态机（§2.2 行为语义表）——用内存 SQLite 跑真实落库路径
    // -----------------------------------------------------------------------

    use std::sync::Arc;

    use super::{RssFetchOutcome, RssManager};
    use crate::db::Db;
    use crate::rss::parser::{ParsedFeed, ParsedItem};

    async fn manager() -> RssManager {
        let db = Db::connect("sqlite::memory:").await.expect("open mem db");
        RssManager::new(db, Arc::new(crate::NoopSink))
    }

    fn parsed(guid: &str, title: &str, size: i64, pub_date: i64) -> ParsedItem {
        ParsedItem {
            guid: guid.to_string(),
            title: title.to_string(),
            link: format!("https://feed.test/item/{guid}"),
            enclosure_url: format!("https://feed.test/dl/{guid}.torrent"),
            enclosure_length: size,
            pub_date,
        }
    }

    fn feed(items: Vec<ParsedItem>) -> ParsedFeed {
        ParsedFeed {
            title: "Test Feed".to_string(),
            link: "https://feed.test/".to_string(),
            items,
        }
    }

    async fn fetched(m: &mut RssManager, id: &str, items: Vec<ParsedItem>) -> Vec<String> {
        let plans = m
            .apply_fetch(RssFetchOutcome {
                source_id: id.to_string(),
                feed: feed(items),
                error: String::new(),
            })
            .await;
        // 模拟 DownloadManager 建任务成功后的回写，否则下一轮会重复派发。
        for p in &plans {
            m.mark_downloaded(&p.source_id, &p.guid, &format!("task-{}", p.guid))
                .await;
        }
        plans.into_iter().map(|p| p.guid).collect()
    }

    async fn subscribe(m: &mut RssManager, source: RssSourceInfo) -> String {
        m.create_source(source).await.expect("create source")
    }

    #[tokio::test]
    async fn first_round_marks_history_read_and_downloads_nothing() {
        let mut m = manager().await;
        let id = subscribe(
            &mut m,
            RssSourceInfo {
                url: "https://feed.test/rss".to_string(),
                ..Default::default()
            },
        )
        .await;

        let downloaded = fetched(
            &mut m,
            &id,
            vec![
                parsed("a", "[X] Show - 01", 100, 10),
                parsed("b", "[X] Show - 02", 100, 20),
                parsed("c", "[X] Show - 03", 100, 30),
            ],
        )
        .await;
        assert!(
            downloaded.is_empty(),
            "first round must never auto-download history"
        );

        let items = m.db.load_rss_items(&id, 100).await.expect("items");
        assert_eq!(items.len(), 3);
        assert!(
            items.iter().all(|i| i.status == RssItemStatus::SeedSkipped),
            "every history item is marked read, not downloaded"
        );
        assert!(m.source(&id).expect("source").seeded);
    }

    #[tokio::test]
    async fn second_round_downloads_only_genuinely_new_items() {
        let mut m = manager().await;
        let id = subscribe(
            &mut m,
            RssSourceInfo {
                url: "https://feed.test/rss".to_string(),
                ..Default::default()
            },
        )
        .await;
        fetched(&mut m, &id, vec![parsed("a", "[X] Show - 01", 100, 10)]).await;

        // 第二轮：feed 重发 a（同 guid）+ 新增 b
        let downloaded = fetched(
            &mut m,
            &id,
            vec![
                parsed("a", "[X] Show - 01 (edited title)", 100, 10),
                parsed("b", "[X] Show - 02", 100, 20),
            ],
        )
        .await;
        assert_eq!(downloaded, vec!["b".to_string()]);

        // 第三轮：完全没有新条目 → 不再派发任何东西
        assert!(
            fetched(&mut m, &id, vec![parsed("b", "[X] Show - 02", 100, 20)])
                .await
                .is_empty()
        );
    }

    #[tokio::test]
    async fn per_round_cap_defers_the_remainder_oldest_first() {
        let mut m = manager().await;
        let id = subscribe(
            &mut m,
            RssSourceInfo {
                url: "https://feed.test/rss".to_string(),
                max_per_fetch: 2,
                ..Default::default()
            },
        )
        .await;
        fetched(&mut m, &id, Vec::new()).await; // 首轮播种（空 feed）

        let batch: Vec<ParsedItem> = (1..=5)
            .map(|n| parsed(&format!("g{n}"), &format!("[X] Show - {n:02}"), 100, n))
            .collect();
        let round1 = fetched(&mut m, &id, batch.clone()).await;
        assert_eq!(
            round1,
            vec!["g1".to_string(), "g2".to_string()],
            "cap honoured, oldest first"
        );

        // 下一轮即便 feed 没变，积压的 g3/g4 继续派发（不丢、不插队）
        let round2 = fetched(&mut m, &id, batch.clone()).await;
        assert_eq!(round2, vec!["g3".to_string(), "g4".to_string()]);
        let round3 = fetched(&mut m, &id, batch).await;
        assert_eq!(round3, vec!["g5".to_string()]);
    }

    #[tokio::test]
    async fn collect_mode_records_items_but_never_creates_tasks() {
        let mut m = manager().await;
        let id = subscribe(
            &mut m,
            RssSourceInfo {
                url: "https://feed.test/rss".to_string(),
                auto_download: false,
                ..Default::default()
            },
        )
        .await;
        fetched(&mut m, &id, Vec::new()).await;
        let downloaded = fetched(&mut m, &id, vec![parsed("a", "[X] Show - 01", 100, 10)]).await;
        assert!(downloaded.is_empty());

        let items = m.db.load_rss_items(&id, 100).await.expect("items");
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].status,
            RssItemStatus::New,
            "collect mode leaves items pickable by hand"
        );
    }

    #[tokio::test]
    async fn rules_and_smart_dedup_are_persisted_with_their_reason_codes() {
        let mut m = manager().await;
        let id = subscribe(
            &mut m,
            RssSourceInfo {
                url: "https://feed.test/rss".to_string(),
                exclude_pattern: "720P".to_string(),
                smart_episode: true,
                ..Default::default()
            },
        )
        .await;
        fetched(&mut m, &id, Vec::new()).await;

        let downloaded = fetched(
            &mut m,
            &id,
            vec![
                parsed("ani", "[ANi] Show - 02 [1080P]", 100, 30),
                parsed("sakura", "[Sakurato] Show - 02 [1080P]", 100, 20),
                parsed("low", "[Other] Show - 03 [720P]", 100, 10),
            ],
        )
        .await;
        assert_eq!(downloaded, vec!["ani".to_string()]);

        let items = m.db.load_rss_items(&id, 100).await.expect("items");
        let by_guid = |g: &str| {
            items
                .iter()
                .find(|i| i.guid == g)
                .cloned()
                .expect("item present")
        };
        assert_eq!(by_guid("ani").status, RssItemStatus::Downloaded);
        assert_eq!(by_guid("ani").task_id, "task-ani");
        assert_eq!(by_guid("sakura").status, RssItemStatus::DuplicateEpisode);
        assert_eq!(by_guid("sakura").reason, "dup_episode");
        assert_eq!(by_guid("low").status, RssItemStatus::Filtered);
        assert_eq!(by_guid("low").reason, "excluded");
    }

    #[tokio::test]
    async fn manual_download_works_for_every_item_state_including_downloaded() {
        let mut m = manager().await;
        let id = subscribe(
            &mut m,
            RssSourceInfo {
                url: "https://feed.test/rss".to_string(),
                exclude_pattern: "720P".to_string(),
                ..Default::default()
            },
        )
        .await;
        fetched(&mut m, &id, Vec::new()).await;
        fetched(
            &mut m,
            &id,
            vec![parsed("low", "[X] Show - 01 [720P]", 100, 10)],
        )
        .await;

        // 被规则过滤掉的条目仍可手动「仍要下载」。
        let plan = m
            .manual_download(&id, "low")
            .await
            .expect("filtered items stay manually downloadable");
        assert_eq!(plan.url, "https://feed.test/dl/low.torrent");

        // 已下载的条目**也**要能重下：任务可能被删了、下崩了，或者只是想
        // 再来一遍。挡住重下没有任何好处，只会逼用户去别处找种子。
        m.mark_downloaded(&id, "low", "task-low").await;
        assert!(
            m.manual_download(&id, "low").await.is_some(),
            "an already-downloaded item must remain re-downloadable"
        );

        // 不存在的条目仍然返回 None。
        assert!(m.manual_download(&id, "nope").await.is_none());
    }

    #[tokio::test]
    async fn fetch_failure_records_the_error_and_backs_off() {
        let mut m = manager().await;
        let id = subscribe(
            &mut m,
            RssSourceInfo {
                url: "https://feed.test/rss".to_string(),
                ..Default::default()
            },
        )
        .await;
        for _ in 0..2 {
            let plans = m
                .apply_fetch(RssFetchOutcome {
                    source_id: id.clone(),
                    feed: ParsedFeed::default(),
                    error: "HTTP 403 Forbidden".to_string(),
                })
                .await;
            assert!(plans.is_empty());
        }
        let s = m.source(&id).expect("source").clone();
        assert_eq!(s.fail_count, 2);
        assert_eq!(s.last_error, "HTTP 403 Forbidden");
        assert!(
            s.enabled,
            "repeated failures must not auto-disable the feed"
        );
        assert!(!s.seeded, "a failed first round must stay unseeded");
        assert_eq!(effective_interval_secs(&s), 30 * 60 * 4);
    }

    #[tokio::test]
    async fn deleting_a_source_cascades_items_but_keeps_task_links_intact() {
        let mut m = manager().await;
        let id = subscribe(
            &mut m,
            RssSourceInfo {
                url: "https://feed.test/rss".to_string(),
                ..Default::default()
            },
        )
        .await;
        fetched(&mut m, &id, vec![parsed("a", "[X] Show - 01", 100, 10)]).await;
        assert!(m.delete_source(&id).await);
        assert!(m.source(&id).is_none());
        assert!(
            m.db.load_rss_items(&id, 100)
                .await
                .expect("items")
                .is_empty()
        );
        assert!(!m.delete_source(&id).await, "deleting twice is a no-op");
    }

    #[tokio::test]
    async fn results_for_a_source_deleted_mid_fetch_are_dropped() {
        let mut m = manager().await;
        let plans = m
            .apply_fetch(RssFetchOutcome {
                source_id: "ghost".to_string(),
                feed: feed(vec![parsed("a", "[X] Show - 01", 100, 10)]),
                error: String::new(),
            })
            .await;
        assert!(plans.is_empty(), "no source, no tasks, no orphan rows");
    }
}
