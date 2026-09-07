//! 插件 ffmpeg 桥集成测试：EngineBridge.run_ffmpeg 端到端。
//!
//! 覆盖：
//! - 越牢路径 / 网络协议参数在 spawn 前被拒（确定性，无需 ffmpeg 二进制）。
//! - 真实 ffmpeg 可用时：在牢笼内生成产物、退出码 0、产物落在牢笼内（有 ffmpeg 时）。
//!
//! 仅 `plugins` feature 下编译运行。
#![cfg(feature = "plugins")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use fluxdown_engine::db::Db;
use fluxdown_engine::plugin::PluginBridge;
use fluxdown_engine::plugin::bridge::EngineBridge;
use fluxdown_engine::plugin::runtime::FfmpegSpec;
use fluxdown_engine::proxy_config::ProxyConfig;

fn unique_dir(tag: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let mut d = std::env::temp_dir();
    d.push(format!("fluxdown_ff_{}_{}_{}", tag, std::process::id(), n));
    std::fs::create_dir_all(&d).expect("mkdir temp");
    d
}
async fn make_bridge(data_dir: &Path) -> EngineBridge {
    let db = Db::open(data_dir).await.expect("open db");
    // 测试用真实 ffmpeg：`FLUXDOWN_TEST_FFMPEG=<绝对路径>` 时经 config 手动指定，
    // 使 resolve_ffmpeg 命中（CI/本机无系统 ffmpeg 时的确定性执行入口）。
    if let Ok(p) = std::env::var("FLUXDOWN_TEST_FFMPEG") {
        db.set_config(fluxdown_engine::components::CONFIG_FFMPEG_PATH, &p)
            .await
            .expect("seed ffmpeg path");
    }
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    EngineBridge::new(db, &ProxyConfig::default(), tx, data_dir.to_path_buf()).expect("bridge")
}

fn spec(args: &[&str]) -> FfmpegSpec {
    FfmpegSpec {
        args: args.iter().map(|s| s.to_string()).collect(),
        subdir: None,
        timeout_ms: Some(30_000),
    }
}

/// 越牢/网络参数在 spawn 前被拒——无论 ffmpeg 是否安装（校验先于二进制解析）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejects_escape_and_network_args() {
    let data_dir = unique_dir("data_reject");
    let jail = unique_dir("jail_reject");
    let bridge = make_bridge(&data_dir).await;

    for args in [
        vec!["-i", "/etc/passwd", "-c", "copy", "out.mp4"],
        vec!["-i", "http://evil.example/x.mp4", "out.mp4"],
        vec!["-i", "in.mp4", "-c", "copy", "../escape.mp4"],
        vec!["-i", "concat:a.ts|b.ts", "out.mp4"],
    ] {
        let r = bridge
            .run_ffmpeg("test@ff", jail.clone(), spec(&args))
            .await;
        assert!(r.is_err(), "args {args:?} must be rejected before spawn");
    }

    // 空参数也拒。
    assert!(
        bridge
            .run_ffmpeg("test@ff", jail.clone(), spec(&[]))
            .await
            .is_err()
    );
}

/// 真实 ffmpeg 可用时：lavfi 生成 0.1s 静音 wav 到牢笼，退出码 0、产物存在于牢笼。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runs_real_ffmpeg_into_jail() {
    let data_dir = unique_dir("data_run");
    let jail = unique_dir("jail_run");
    let bridge = make_bridge(&data_dir).await;

    // 探测可用性；无 ffmpeg 环境（多数 CI）直接跳过 —— 执行部分依赖真实二进制。
    let avail = bridge.ffmpeg_available().await;
    let Some(a) = avail else { return };
    if !a.available {
        eprintln!("[skip] ffmpeg 不可用，跳过真实执行断言");
        return;
    }

    let out = bridge
        .run_ffmpeg(
            "test@ff",
            jail.clone(),
            spec(&[
                "-f",
                "lavfi",
                "-i",
                "anullsrc=r=8000:cl=mono",
                "-t",
                "0.1",
                "-y",
                "out.wav",
            ]),
        )
        .await
        .expect("run_ffmpeg");

    assert!(!out.timed_out, "should not time out");
    assert_eq!(out.code, 0, "ffmpeg exit non-zero; stderr: {}", out.stderr);
    let produced = jail.join("out.wav");
    let meta = std::fs::metadata(&produced).expect("output wav should exist in jail");
    assert!(meta.len() > 0, "output wav should be non-empty");
}
