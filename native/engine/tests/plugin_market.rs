//! 插件市场客户端集成测试（本地 HTTP，无外网依赖）。
//!
//! 覆盖：索引拉取+解析、sequence 防回滚、https-only 镜像白名单（拒 http 降级）。
//! 正向「下载+content_hash 校验+安装」由 `tests/fxplug_install.rs`（安装管线）+
//! market 单测（sha256/解析）+ 线上 E2E（真实索引 hash 吻合）共同覆盖。

#![cfg(feature = "plugins")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write as _;
use std::net::TcpListener;
use std::sync::Arc;

use fluxdown_engine::bt_downloader::BtConfig;
use fluxdown_engine::plugin::{MarketClient, MarketError};
use fluxdown_engine::proxy_config::ProxyConfig;
use fluxdown_engine::{Engine, EngineConfig, NoopSelection, NoopSink};

/// 本地服务器：GET 任意路径返回预置 body（用于服务 index.json）。
fn spawn_index_server(body: String) -> (u16, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let handle = std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buf = [0u8; 4096];
            let _ = std::io::Read::read(&mut stream, &mut buf);
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.flush();
        }
    });
    (port, handle)
}

fn uniq() -> String {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{}-{}", std::process::id(), n)
}

async fn make_engine(work: &std::path::Path) -> Engine {
    let cfg = EngineConfig {
        max_concurrent: 4,
        speed_limit_bps: 0,
        upload_limit_bps: 0,
        default_save_dir: work.to_string_lossy().into_owned(),
        app_data_dir: work.to_string_lossy().into_owned(),
        bt_config: BtConfig::default(),
        proxy_config: ProxyConfig::default(),
        user_agent: String::new(),
        data_dir_override: Some(work.to_path_buf()),
        database_url: None,
    };
    Engine::new(cfg, Arc::new(NoopSink), Arc::new(NoopSelection))
        .await
        .expect("engine")
}

fn index_json(sequence: u64, mirror: &str) -> String {
    format!(
        r#"{{"indexId":"11111111-1111-1111-1111-111111111111","sequence":{sequence},"updated":"now",
        "entries":[{{"pluginId":"test@rewriter","version":"1.0.0","sequence":{sequence},
        "contentHash":"sha256:0000000000000000000000000000000000000000000000000000000000000000",
        "mirrors":["{mirror}"],"yanked":"none"}}]}}"#
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fetch_index_parses_and_enforces_watermark() {
    let work = std::env::temp_dir().join(format!("fluxdown-market-{}", uniq()));
    tokio::fs::create_dir_all(&work).await.expect("mkdir");
    let engine = make_engine(&work).await;
    let pm = engine.manager.plugin_manager().expect("pm");

    // 第一次：sequence=5。
    let (port, _s1) = spawn_index_server(index_json(5, "https://example.com/p.fxplug"));
    let url = format!("http://127.0.0.1:{port}/index.json");
    let mc = MarketClient::new(pm.clone(), engine.db.clone(), vec![url]);
    let idx = mc.fetch_index().await.expect("fetch ok");
    assert_eq!(idx.index_id, "11111111-1111-1111-1111-111111111111");
    assert_eq!(idx.sequence, 5);
    assert_eq!(idx.entries.len(), 1);
    assert_eq!(idx.entries[0].plugin_id, "test@rewriter");

    // 第二次：sequence=3（< 高水位 5）→ 防回滚拒绝。
    let (port2, _s2) = spawn_index_server(index_json(3, "https://example.com/p.fxplug"));
    let url2 = format!("http://127.0.0.1:{port2}/index.json");
    let mc2 = MarketClient::new(pm, engine.db.clone(), vec![url2]);
    let err = mc2.fetch_index().await.expect_err("rollback rejected");
    assert!(matches!(
        err,
        MarketError::SequenceRollback {
            seen: 3,
            watermark: 5
        }
    ));

    let _ = tokio::fs::remove_dir_all(&work).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_mirror_rejected_https_only() {
    let work = std::env::temp_dir().join(format!("fluxdown-market-http-{}", uniq()));
    tokio::fs::create_dir_all(&work).await.expect("mkdir");
    let engine = make_engine(&work).await;
    let pm = engine.manager.plugin_manager().expect("pm");

    // 镜像是 http:// → download_verified 跳过全部 → AllMirrorsFailed。
    let (port, _s) = spawn_index_server(index_json(1, "http://127.0.0.1:9/p.fxplug"));
    let url = format!("http://127.0.0.1:{port}/index.json");
    let mc = MarketClient::new(pm, engine.db.clone(), vec![url]);
    let idx = mc.fetch_index().await.expect("fetch ok");
    let entry = idx.entries[0].clone();
    let err = mc
        .install_entry(&entry, false)
        .await
        .expect_err("http mirror rejected");
    assert!(matches!(err, MarketError::AllMirrorsFailed));

    let _ = tokio::fs::remove_dir_all(&work).await;
}

/// 回归（Bug：索引拉取无体积上限）：被投毒/损坏的源返回超大响应时必须流式
/// 截断报 `IndexTooLarge`，而非 `.text()` 全量缓冲撑爆内存。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn oversized_index_rejected() {
    let work = std::env::temp_dir().join(format!("fluxdown-market-big-{}", uniq()));
    tokio::fs::create_dir_all(&work).await.expect("mkdir");
    let engine = make_engine(&work).await;
    let pm = engine.manager.plugin_manager().expect("pm");

    // 5MB 垃圾响应（> 4MB 上限）。
    let (port, _s) = spawn_index_server("x".repeat(5 * 1024 * 1024));
    let url = format!("http://127.0.0.1:{port}/index.json");
    let mc = MarketClient::new(pm, engine.db.clone(), vec![url]);
    let err = mc
        .fetch_index()
        .await
        .expect_err("oversized index must be rejected");
    assert!(matches!(err, MarketError::IndexTooLarge), "got: {err:?}");

    let _ = tokio::fs::remove_dir_all(&work).await;
}
