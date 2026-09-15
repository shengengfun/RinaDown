//! Global token-bucket speed limiter shared across all download tasks.
//!
//! The limiter uses a token-bucket algorithm with **time-based refill**:
//! - A background task measures *actual* wall-clock elapsed time between ticks
//!   and adds tokens proportionally, eliminating drift from `tokio::time::interval`.
//! - Tokens are capped at 100 ms worth of the configured limit (see
//!   [`CAP_DURATION_MS`]), preventing
//!   excessive bursts after idle periods while tolerating normal tick jitter.
//! - Refill uses a CAS loop (no `fetch_add` + `store` race window).
//! - When `limit == 0`, the limiter is disabled (unlimited speed).
//!
//! The limiter is designed to be cheaply cloneable (`Arc` inside) so every
//! download segment can hold a handle without additional allocation.
//!
//! ## Layering (`new_with_fallback`)
//!
//! 限速优先级是「任务级 > 队列级 > 全局」，但任务级数值**随时会变**（用户在
//! 详情页改单个任务限速），而任务启动时绑定的是某一具体令牌桶。为了让
//! 「改一下立即生效」成立，任务永远拿到一个**分层限速器**：它自己那层承载
//! 任务级限速（初始可能为 0 = 未设置），为 0 时把额度计算整体**委托**给下一层
//! （队列限速器，或全局限速器）。于是：
//! - `set_limit(bps)` 改自己那层 → 正在跑的任务立即换挡，不必重启；
//! - 改成 0 → 自动退回队列/全局额度，而不是变成「不限速」；
//! - 未设置任务级限速时，语义与旧实现逐字节一致（直用下层的桶）。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use tokio::sync::Notify;
use tokio::time::Instant;

/// Shared, cheaply-cloneable speed limiter.
#[derive(Clone)]
pub struct SpeedLimiter {
    inner: Arc<Inner>,
}

struct Inner {
    /// Current speed limit in bytes/sec.  0 = unlimited.
    limit_bps: AtomicU64,
    /// Available tokens (bytes that may be consumed immediately).
    tokens: AtomicU64,
    /// Notify waiters when tokens are replenished.
    notify: Notify,
    /// Notify the refill task to wake from its idle sleep when the limit
    /// changes from unlimited (0) to a positive value.
    refill_wake: Arc<Notify>,
    /// 本层限速为 0 时把额度计算委托给这一层（见模块文档「Layering」）。
    /// `None` = 链的末端（全局 / 队列独占桶）。
    fallback: Option<Box<SpeedLimiter>>,
    /// refill 后台任务是否已经起过（`set_limit` 可能把 0 改成正数，
    /// 这时才需要桶，惰性启动，避免无谓的 20Hz 空转）。
    refill_started: AtomicBool,
}

/// Nominal refill interval — 50 ms gives smooth throughput without too many
/// wake-ups.  The *actual* refill amount is computed from wall-clock elapsed
/// time, so this only controls how often the task wakes.
const REFILL_INTERVAL_MS: u64 = 50;

/// Token bucket capacity expressed as a fraction of the per-second limit.
///
/// 桶容量 = 限速值的 10%（100 ms）。它是「瞬时速度能冲多高」的唯一上限：
/// 满桶被一次抽干时，任意 T 秒窗口内的平均速率最多是 `limit × (1 + cap/T)`。
/// 250 ms 档在 1 s 窗口下允许冲到 1.25×，实测在界面上会被读成「明显超过
/// 设定值」；收到 100 ms 后同样的窗口只剩 1.1×，而 2 个 refill tick 的额度
/// 仍足以吸收 notify 丢失 / 调度抖动，不会掉吞吐（见 `high_speed_accuracy`
/// 与 `very_low_limit_accumulates_tokens_over_time`）。
const CAP_DURATION_MS: u64 = 100;

impl SpeedLimiter {
    /// Create a new limiter with the given initial limit (bytes/sec).
    /// Pass `0` for unlimited.
    pub fn new(limit_bps: u64) -> Self {
        Self::new_with_fallback(limit_bps, None)
    }

    /// 建一个分层限速器：本层限速为 0 时，把额度计算整体委托给 `fallback`。
    ///
    /// 任务级限速用这个构造（`fallback` = 队列/全局限速器）。本层初始值 > 0
    /// 时立即起 refill 任务；为 0 时不起，等 `set_limit` 改成大于 0 再惰性起。
    pub fn new_with_fallback(limit_bps: u64, fallback: Option<SpeedLimiter>) -> Self {
        let limiter = Self {
            inner: Arc::new(Inner {
                limit_bps: AtomicU64::new(limit_bps),
                tokens: AtomicU64::new(0),
                notify: Notify::new(),
                refill_wake: Arc::new(Notify::new()),
                fallback: fallback.map(Box::new),
                refill_started: AtomicBool::new(false),
            }),
        };
        if limit_bps > 0 {
            limiter.ensure_refill_task();
        }
        limiter
    }

    /// Update the speed limit at runtime.  Takes effect on the next refill tick.
    ///
    /// 对分层限速器而言这是「本层」的值：改成 0 = 退回下一层额度（不是不限速）。
    pub fn set_limit(&self, limit_bps: u64) {
        self.inner.limit_bps.store(limit_bps, Ordering::Relaxed);
        if limit_bps > 0 {
            // 可能之前一直是 0（纯委托状态），此时才第一次需要自己的桶。
            self.ensure_refill_task();
        }
        // Wake any waiters so they re-evaluate immediately.
        self.inner.notify.notify_waiters();
        // Wake the refill task from its idle sleep (when transitioning from
        // unlimited to limited) so tokens start flowing without delay.
        self.inner.refill_wake.notify_one();
    }

    /// 本层配置的限速（0 = 本层不限，委托下一层）。
    #[must_use]
    pub fn own_limit(&self) -> u64 {
        self.inner.limit_bps.load(Ordering::Relaxed)
    }

    /// 实际生效的限速（B/s，0 = 整条链都不限）。
    ///
    /// 分层链上第一个非零值即生效值——供 `limiter_active` 之类的判断使用
    /// （任务级没设限时，真实生效的是队列/全局限速）。
    #[must_use]
    pub fn limit(&self) -> u64 {
        let mut cur = self;
        loop {
            let l = cur.inner.limit_bps.load(Ordering::Relaxed);
            if l > 0 {
                return l;
            }
            match cur.inner.fallback.as_deref() {
                Some(next) => cur = next,
                None => return 0,
            }
        }
    }

    /// Consume up to `requested` bytes worth of tokens.
    ///
    /// - If the limiter is disabled (limit == 0), returns `requested` immediately.
    /// - Otherwise waits until at least 1 token is available, then returns
    ///   `min(requested, available)`.  The caller should only process that many
    ///   bytes, then call `consume` again for the remainder.
    ///
    /// This design avoids holding an async lock and naturally distributes
    /// bandwidth among all concurrent callers via contention on the atomic.
    pub async fn consume(&self, requested: u64) -> u64 {
        if requested == 0 {
            return 0;
        }

        'resolved: loop {
            // 沿分层链定位「当前真正生效」的一层：本层限速 > 0 就用它，
            // 为 0 则委托下一层；整条链都没有限速 = 直通。
            let mut cur = self;
            loop {
                if cur.inner.limit_bps.load(Ordering::Relaxed) > 0 {
                    break;
                }
                match cur.inner.fallback.as_deref() {
                    Some(next) => cur = next,
                    None => return requested,
                }
            }
            let inner = &cur.inner;

            // Try to take some tokens.
            let available = inner.tokens.load(Ordering::Acquire);
            if available > 0 {
                let take = requested.min(available);
                // CAS loop to atomically subtract tokens.
                match inner.tokens.compare_exchange_weak(
                    available,
                    available - take,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => return take,
                    Err(_) => continue,
                }
            }

            // No tokens available — wait for the refill task to notify us.
            // Bounded wait guards against the rare TOCTOU race where
            // notify_waiters() fires between our tokens.load() returning 0
            // and the notified().await registration: since notify_waiters()
            // only wakes *currently-registered* listeners, that notification
            // would be silently lost.  The timeout (REFILL_INTERVAL_MS + 10 ms)
            // ensures we retry within at most one extra refill cycle.
            tokio::select! {
                biased;
                () = inner.notify.notified() => {}
                () = tokio::time::sleep(std::time::Duration::from_millis(
                    REFILL_INTERVAL_MS + 10,
                )) => {}
            }
            // 等待期间本层限速可能被热改（含改成 0 → 需重新走链退回下层），
            // 故重新解析一次生效层，而不是固守刚才那一层。CAS 失败走这里的
            // 代价只是多一次链遍历，远小于出错的风险。
            continue 'resolved;
        }
    }

    /// Spawn the background refill task.  Idempotent（重复调用不会再起一个，
    /// 否则同一个桶会被多个任务按倍速率灌令牌）。The task runs until the
    /// `SpeedLimiter` (and all its clones) are dropped.
    pub fn spawn_refill_task(&self) {
        if self.inner.refill_started.swap(true, Ordering::AcqRel) {
            return;
        }
        self.spawn_refill_loop();
    }

    /// 惰性起 refill 任务（`set_limit` 把本层从 0 改成正数时用）。
    fn ensure_refill_task(&self) {
        self.spawn_refill_task();
    }

    fn spawn_refill_loop(&self) {
        let weak = Arc::downgrade(&self.inner);
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(std::time::Duration::from_millis(REFILL_INTERVAL_MS));
            // The first tick completes immediately — skip it and record baseline.
            interval.tick().await;
            let mut last_refill = Instant::now();

            loop {
                interval.tick().await;
                let Some(inner) = weak.upgrade() else {
                    // All SpeedLimiter handles dropped — exit.
                    break;
                };

                let limit = inner.limit_bps.load(Ordering::Relaxed);
                if limit == 0 {
                    // Unlimited — clear any accumulated tokens and wake waiters
                    // (they will see limit==0 and pass through).
                    inner.tokens.store(0, Ordering::Relaxed);
                    inner.notify.notify_waiters();
                    // Clone the Arc<Notify> so we can await it after dropping
                    // the strong reference to `inner`.
                    let refill_wake = Arc::clone(&inner.refill_wake);
                    drop(inner);
                    let wake = refill_wake.notified();
                    // Sleep longer when unlimited — no need for frequent ticks.
                    // `refill_wake` cuts this short if the limit changes, so
                    // tokens start flowing without delay.
                    tokio::select! {
                        () = tokio::time::sleep(std::time::Duration::from_secs(2)) => {}
                        () = wake => {}
                    }
                    // Reset baseline so the first limited tick doesn't get a
                    // huge elapsed value.
                    last_refill = Instant::now();
                    continue;
                }

                // ── Time-based refill ───────────────────────────────────
                // Measure actual wall-clock time since last refill to
                // compensate for interval jitter / missed-tick bursts.
                let now = Instant::now();
                let elapsed_us = (now - last_refill).as_micros() as u64;

                // refill = limit * elapsed_us / 1_000_000
                // Use u128 intermediate to avoid overflow for very large limits.
                let refill = ((limit as u128) * (elapsed_us as u128) / 1_000_000u128) as u64;

                if refill == 0 {
                    // Extremely low limit + short interval — skip this tick.
                    // Crucially, do NOT advance `last_refill` here: the elapsed
                    // microseconds that were too small to form a whole token must
                    // accumulate into the next tick's `elapsed_us`, otherwise this
                    // fractional time is permanently discarded and the limiter
                    // would systematically deliver below the configured rate at
                    // very low limits (e.g. < ~20 B/s).
                    continue;
                }

                // A whole token's worth of time has elapsed — advance the
                // baseline only now that the elapsed time has been accounted for.
                last_refill = now;

                // Cap: 100 ms worth of the current limit.  For very low limits
                // ensure at least 2× nominal-tick amount so tokens aren't
                // perpetually capped to zero.
                let nominal_tick = limit * REFILL_INTERVAL_MS / 1000;
                let cap = (limit * CAP_DURATION_MS / 1000)
                    .max(nominal_tick * 2)
                    .max(1);

                // ── Atomic CAS refill ───────────────────────────────────
                // Add `refill` tokens and clamp to `cap` in a single atomic
                // step, eliminating the fetch_add + store race window.
                loop {
                    let current = inner.tokens.load(Ordering::Acquire);
                    let new_val = current.saturating_add(refill).min(cap);
                    if new_val == current {
                        // Nothing to change (already at or above cap).
                        break;
                    }
                    match inner.tokens.compare_exchange_weak(
                        current,
                        new_val,
                        Ordering::AcqRel,
                        Ordering::Relaxed,
                    ) {
                        Ok(_) => break,
                        Err(_) => continue, // contention — retry
                    }
                }

                inner.notify.notify_waiters();
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::SpeedLimiter;
    use std::time::{Duration, Instant};

    #[tokio::test]
    async fn unlimited_returns_full_request() {
        let limiter = SpeedLimiter::new(0); // unlimited
        limiter.spawn_refill_task();

        let got = limiter.consume(1_000_000).await;
        assert_eq!(
            got, 1_000_000,
            "unlimited limiter should return full requested amount"
        );
    }

    #[tokio::test]
    async fn zero_request_returns_zero() {
        let limiter = SpeedLimiter::new(1024);
        limiter.spawn_refill_task();

        let got = limiter.consume(0).await;
        assert_eq!(got, 0);
    }

    #[tokio::test]
    async fn limited_consumes_in_chunks() {
        let limiter = SpeedLimiter::new(10_000); // 10 KB/s
        limiter.spawn_refill_task();

        // Wait for one refill tick (~50ms) to have tokens available
        tokio::time::sleep(Duration::from_millis(100)).await;

        let got = limiter.consume(100_000).await; // request 100 KB
        // Should get at most ~1 KB (10KB/s × 0.1s), capped to the bucket size.
        assert!(got > 0, "should get some tokens");
        assert!(got < 100_000, "should not get full request under limit");
    }

    #[tokio::test]
    async fn set_limit_changes_behavior() {
        let limiter = SpeedLimiter::new(0); // start unlimited
        limiter.spawn_refill_task();

        let got = limiter.consume(1_000_000).await;
        assert_eq!(got, 1_000_000, "should be unlimited initially");

        // Switch to limited
        limiter.set_limit(1024);
        assert_eq!(limiter.limit(), 1024);

        // Switch back to unlimited
        limiter.set_limit(0);
        tokio::time::sleep(Duration::from_millis(60)).await; // wait for refill tick
        let got = limiter.consume(500_000).await;
        assert_eq!(got, 500_000, "should be unlimited again");
    }

    #[tokio::test]
    async fn limited_speed_is_approximately_correct() {
        let limit_bps: u64 = 50_000; // 50 KB/s
        let limiter = SpeedLimiter::new(limit_bps);
        limiter.spawn_refill_task();

        let start = Instant::now();
        let mut total = 0u64;
        let target = 25_000u64; // 25 KB — should take ~0.5s at 50 KB/s

        while total < target {
            let got = limiter.consume(target - total).await;
            total += got;
        }

        let elapsed = start.elapsed();
        // Should take roughly 0.3–1.5s (allowing wide margin for CI variance)
        assert!(
            elapsed > Duration::from_millis(200),
            "consumed {target} bytes in {elapsed:?} — too fast for {limit_bps} bps limit"
        );
        assert!(
            elapsed < Duration::from_secs(3),
            "consumed {target} bytes in {elapsed:?} — too slow, possible deadlock"
        );
    }

    /// Validates that throughput is within ±15 % of the configured limit at
    /// higher speeds (the scenario reported as inaccurate).
    #[tokio::test]
    async fn high_speed_accuracy() {
        let limit_bps: u64 = 5_000_000; // 5 MB/s
        let limiter = SpeedLimiter::new(limit_bps);
        limiter.spawn_refill_task();

        let duration = Duration::from_secs(2);
        let start = Instant::now();
        let mut total = 0u64;

        while start.elapsed() < duration {
            let got = limiter.consume(65_536).await;
            total += got;
        }

        let elapsed = start.elapsed().as_secs_f64();
        let actual_bps = total as f64 / elapsed;
        let ratio = actual_bps / limit_bps as f64;

        assert!(
            ratio > 0.85,
            "throughput {actual_bps:.0} B/s is too low vs limit {limit_bps} B/s (ratio {ratio:.3})"
        );
        assert!(
            ratio < 1.15,
            "throughput {actual_bps:.0} B/s exceeds limit {limit_bps} B/s (ratio {ratio:.3})"
        );
    }

    /// Regression for F008: at extremely low limits the per-tick refill
    /// truncates to 0; the fractional elapsed time must accumulate across ticks
    /// instead of being discarded, otherwise throughput stalls below the limit.
    #[tokio::test]
    async fn very_low_limit_accumulates_tokens_over_time() {
        // 10 B/s with a 50 ms tick yields ~0.5 B/refill, so the refill==0 branch
        // is exercised on most ticks. Tokens must still accumulate to ~1 B every
        // ~100 ms rather than perpetually rounding to zero.
        let limit_bps: u64 = 10;
        let limiter = SpeedLimiter::new(limit_bps);
        limiter.spawn_refill_task();

        let start = Instant::now();
        let mut total = 0u64;
        // Aim for ~5 bytes, which at 10 B/s should take roughly 0.5 s; bail out
        // after a generous deadline so a regression manifests as too-few bytes.
        let deadline = start + Duration::from_secs(3);
        while total < 5 && Instant::now() < deadline {
            let got = limiter.consume(5 - total).await;
            total += got;
        }

        assert!(
            total >= 5,
            "very low limit stalled: got only {total} bytes in {:?} — refill==0 elapsed time was discarded",
            start.elapsed()
        );
    }

    /// 分层链：本层为 0 时额度整体来自 fallback（任务级没设限 → 跟随队列/全局）。
    #[tokio::test]
    async fn layered_delegates_to_fallback_when_own_limit_is_zero() {
        let global = SpeedLimiter::new(10_000); // 10 KB/s
        global.spawn_refill_task();
        let task = SpeedLimiter::new_with_fallback(0, Some(global));

        assert_eq!(task.own_limit(), 0, "本层：任务级没设限");
        assert_eq!(task.limit(), 10_000, "生效值应取链上第一个非零值");

        let start = Instant::now();
        let mut total = 0u64;
        while start.elapsed() < Duration::from_millis(400) {
            total += task.consume(4_096).await;
        }
        assert!(
            total > 1_000,
            "委托下层不能饿死：400ms 内只拿到 {total} 字节"
        );
        assert!(
            total < 20_000,
            "委托下层应按 10KB/s 收口，400ms 内实得 {total} 字节"
        );
    }

    /// 分层链：本层 > 0 时压过 fallback（任务级限速优先于队列/全局）。
    #[tokio::test]
    async fn layered_own_limit_takes_precedence_over_fallback() {
        let global = SpeedLimiter::new(1_000_000); // 1 MB/s
        global.spawn_refill_task();
        let task = SpeedLimiter::new_with_fallback(10_000, Some(global));

        assert_eq!(task.own_limit(), 10_000);
        assert_eq!(task.limit(), 10_000, "本层有值时不再看下层");

        let start = Instant::now();
        let mut total = 0u64;
        while start.elapsed() < Duration::from_millis(400) {
            total += task.consume(4_096).await;
        }
        assert!(
            total < 20_000,
            "任务级 10KB/s 应压过全局 1MB/s，400ms 内实得 {total} 字节"
        );
    }

    /// 运行中热改任务级限速：无需重建限速器（即「改完立即生效」）；
    /// 改回 0 表示退回下层额度，而不是变成「不限速」。
    #[tokio::test]
    async fn layered_hot_limit_applies_without_restart() {
        let global = SpeedLimiter::new(0); // 下层不限速
        global.spawn_refill_task();
        let task = SpeedLimiter::new_with_fallback(0, Some(global.clone()));

        assert_eq!(task.limit(), 0, "整条链都不限速时生效值为 0");
        assert_eq!(
            task.consume(1_000_000).await,
            1_000_000,
            "本层 0 且下层不限 → 直通"
        );

        // 热改本层 → 立即受本层约束。
        task.set_limit(20_000);
        assert_eq!(task.limit(), 20_000);
        let start = Instant::now();
        let mut total = 0u64;
        while total < 20_000 {
            total += task.consume(20_000 - total).await;
        }
        let elapsed = start.elapsed();
        assert!(
            elapsed > Duration::from_millis(300),
            "热改后应立刻限速：20KB 只用了 {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_secs(6),
            "热改后不应死锁：20KB 用了 {elapsed:?}"
        );

        // 改回 0 → 退回下层（不限速），而不是永久卡住。
        task.set_limit(0);
        assert_eq!(task.limit(), 0);
        assert_eq!(
            task.consume(5_000_000).await,
            5_000_000,
            "本层改回 0 应重新委托下层"
        );
    }

    /// 未设置任务级限速时，分层包装的行为必须与直接用下层桶一致
    /// （否则会给「没设限速」的下载引入额外约束）。
    #[tokio::test]
    async fn layered_with_unlimited_chain_passes_through() {
        let global = SpeedLimiter::new(0);
        global.spawn_refill_task();
        let task = SpeedLimiter::new_with_fallback(0, Some(global));
        assert_eq!(task.limit(), 0);
        assert_eq!(task.consume(8_000_000).await, 8_000_000);
    }

    /// Tests that the speed limiter doesn't starve when multiple consumers
    /// compete for tokens. This is relevant to Bug #3 (multi-segment FTP).
    #[tokio::test]
    async fn multiple_concurrent_consumers_all_make_progress() {
        let limiter = SpeedLimiter::new(100_000); // 100 KB/s
        limiter.spawn_refill_task();

        let mut handles = Vec::new();
        for _ in 0..8 {
            let l = limiter.clone();
            handles.push(tokio::spawn(async move {
                let mut total = 0u64;
                let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
                while tokio::time::Instant::now() < deadline {
                    let got = l.consume(4096).await;
                    total += got;
                }
                total
            }));
        }

        let mut totals = Vec::new();
        for h in handles {
            if let Ok(t) = h.await {
                totals.push(t);
            }
        }

        // All consumers should have made some progress
        for (i, t) in totals.iter().enumerate() {
            assert!(*t > 0, "consumer {i} got 0 bytes — starvation detected");
        }

        // Total across all consumers should be approximately 100 KB (1 s at 100 KB/s)
        let grand_total: u64 = totals.iter().sum();
        assert!(
            grand_total > 50_000,
            "total {grand_total} too low for 100KB/s limit over 1s"
        );
        assert!(
            grand_total < 200_000,
            "total {grand_total} exceeds limit — limiter broken"
        );
    }
}
