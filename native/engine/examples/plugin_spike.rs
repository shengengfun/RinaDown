//! rquickjs 0.12.1 能力 spike（feature: full-async + parallel，禁 rust-alloc/allocator）。
//!
//! 运行：`cargo run -p fluxdown_engine --example plugin_spike --features plugins`
//!
//! ## 验证结论（实测记录于下方 main 输出）
//! 1. `set_memory_limit(N)` 超限时 eval 返回 `Err(Error::Allocation)`（非 abort），
//!    且在 full-async+parallel（无 rust-alloc/allocator）组合下**确实生效**（非 no-op）。
//! 2. `set_interrupt_handler`（按字节码指令计数）对 `while(true){}` 死循环的中断延迟——
//!    deadline 闭包返回 true 后引擎抛不可捕获异常，实测数十毫秒内返回。
//! 3. JS 侧 `await` 一个永不 resolve 的 Promise 时：interrupt handler **不触发**
//!    （脚本停在 await，DelSkayn/rquickjs#102），但外层 `tokio::time::timeout` 如期
//!    终止（drop future 回收），进程存活。
//! 4. `AsyncRuntime`+`AsyncContext` 下 JS `await` 一个 Rust async 桥接函数（模拟 fetch）
//!    可跑通：`Async(closure)` 包装、`Function::new` 注入、JS 侧 await 拿到返回值。
//! 5. Windows MSVC 目标编译通过（本文件即证明）。
//!
//! **判停条件**：若 (4) 跑不通则降级为「同步 JS API + Rust 侧阻塞桥接在专用线程执行」
//! （trait 形状不变）；(2)/(3) 即便 interrupt 不生效，外层 timeout + 线程隔离兜底已覆盖。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[cfg(not(feature = "plugins"))]
fn main() {
    eprintln!("需 --features plugins 运行本 spike");
}

#[cfg(feature = "plugins")]
#[tokio::main]
async fn main() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    use rquickjs::{AsyncContext, AsyncRuntime, Function, Promise, prelude::Async};

    println!("=== rquickjs plugin spike (full-async + parallel) ===\n");

    // -------------------------------------------------------------------
    // (1) 内存上限生效验证
    // -------------------------------------------------------------------
    {
        let rt = AsyncRuntime::new().expect("runtime");
        rt.set_memory_limit(2 * 1024 * 1024).await; // 2MB，故意小
        let ctx = AsyncContext::full(&rt).await.expect("context");
        let res: Result<(), rquickjs::Error> = ctx
            .async_with(async |ctx| {
                // 尝试分配远超 2MB 的字符串。
                ctx.eval::<rquickjs::Value, _>("let s='x'; for(let i=0;i<30;i++){ s+=s; } s.length")
                    .map(|_| ())
            })
            .await;
        println!(
            "(1) 内存上限：eval 大分配结果 = {} —— {}",
            if res.is_err() {
                "Err(如期)"
            } else {
                "Ok(未触发!)"
            },
            match &res {
                Err(e) => format!("{e:?}"),
                Ok(()) => "内存限制可能未生效，需复核 feature 组合".to_string(),
            }
        );
    }

    // -------------------------------------------------------------------
    // (2) interrupt handler 中断死循环
    // -------------------------------------------------------------------
    {
        let rt = AsyncRuntime::new().expect("runtime");
        let deadline = Instant::now() + Duration::from_millis(300);
        rt.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)))
            .await;
        let ctx = AsyncContext::full(&rt).await.expect("context");
        let start = Instant::now();
        let res: Result<(), rquickjs::Error> = ctx
            .async_with(async |ctx| ctx.eval::<rquickjs::Value, _>("while(true){}").map(|_| ()))
            .await;
        let elapsed = start.elapsed();
        println!(
            "(2) interrupt 死循环：{:?} 后返回 {} —— {:?}",
            elapsed,
            if res.is_err() {
                "Err(被中断)"
            } else {
                "Ok(!)"
            },
            res.err()
        );
    }

    // -------------------------------------------------------------------
    // (3) await 永不 resolve 的 Promise + 外层 timeout
    // -------------------------------------------------------------------
    {
        let rt = AsyncRuntime::new().expect("runtime");
        let deadline = Instant::now() + Duration::from_millis(200);
        rt.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)))
            .await;
        let ctx = AsyncContext::full(&rt).await.expect("context");
        let start = Instant::now();
        let fut = ctx.async_with(async |ctx| {
            let p: Promise = ctx.eval("new Promise(() => {})").expect("eval promise");
            let _: rquickjs::Value = p.into_future().await.expect("never");
        });
        let outcome = tokio::time::timeout(Duration::from_millis(500), fut).await;
        println!(
            "(3) await 永不 resolve Promise：外层 timeout {:?} 后 = {}（interrupt 对 await 不触发，靠外层 timeout 兜底）",
            start.elapsed(),
            if outcome.is_err() {
                "Elapsed(如期，future 被 drop)"
            } else {
                "提前返回(!)"
            }
        );
    }

    // -------------------------------------------------------------------
    // (4) JS await 一个 Rust async 桥接函数（模拟 fetch）
    // -------------------------------------------------------------------
    {
        let rt = AsyncRuntime::new().expect("runtime");
        let ctx = AsyncContext::full(&rt).await.expect("context");
        let counter = Arc::new(AtomicU64::new(0));
        let counter2 = counter.clone();
        let res: Result<String, rquickjs::Error> = ctx
            .async_with(async move |ctx| {
                // 注入 Rust async 函数：接收 String，await 后返回 String。
                let f = Function::new(
                    ctx.clone(),
                    Async(move |q: String| {
                        let c = counter2.clone();
                        async move {
                            tokio::time::sleep(Duration::from_millis(20)).await;
                            c.fetch_add(1, Ordering::SeqCst);
                            Ok::<String, rquickjs::Error>(format!("fetched:{q}"))
                        }
                    }),
                )?
                .with_name("__bridge")?;
                ctx.globals().set("__bridge", f)?;
                let p: Promise = ctx.eval("__bridge('hello').then(s => s.toUpperCase())")?;
                let s: String = p.into_future().await?;
                Ok(s)
            })
            .await;
        rt.idle().await;
        println!(
            "(4) Rust async 桥接：结果 = {:?}，桥接调用次数 = {}",
            res,
            counter.load(Ordering::SeqCst)
        );
    }

    println!("\n=== spike 完成；进程存活 = true ===");
}
