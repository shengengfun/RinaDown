//! 无人值守创建（`NewTaskSpec::unattended_selection`）的建任务链路集成测试。
//!
//! 该标记由自动化入口（RSS、免打扰接管，config `silent_skip_selection`）置
//! true，承诺「不弹任何二次选择框」：
//!
//! 1. BT 任务 → 「已确认全部文件」**预先**落库（`Some([])`），`do_start_task`
//!    不再经 `HostSelection` 弹文件选择框；
//! 2. 所有任务 → `tasks.unattended` 落 1，start/resume 时 HLS/DASH 画质与
//!    插件 resolve 变体选择静默取默认值（弹窗路径直接短路）；
//! 3. 手动路径（默认 false）→ 两者都不发生，用户仍自己挑文件/画质。
//!
//! 任务全部 `start_paused` 落库，不发起真实网络/BT 会话。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use fluxdown_engine::bt_downloader::BtConfig;
use fluxdown_engine::download_manager::NewTaskSpec;
use fluxdown_engine::proxy_config::ProxyConfig;
use fluxdown_engine::{Engine, EngineConfig, NoopSelection, NoopSink};

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

const MAGNET: &str = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=demo";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unattended_bt_task_preconfirms_all_files_and_persists_flag() {
    let work = std::env::temp_dir().join(format!("fluxdown-unattended-{}", uniq()));
    tokio::fs::create_dir_all(&work).await.expect("mkdir");
    let mut engine = make_engine(&work).await;
    let save_dir = work.to_string_lossy().into_owned();

    let id = engine
        .manager
        .create_task(NewTaskSpec {
            url: MAGNET.to_string(),
            save_dir: save_dir.clone(),
            start_paused: true,
            unattended_selection: true,
            ..Default::default()
        })
        .await
        .expect("create unattended bt task");

    assert_eq!(
        engine
            .db
            .load_bt_selected_files(&id)
            .await
            .expect("query bt selection"),
        Some(Vec::new()),
        "无人值守 BT 任务必须预置「已确认全选」，绝不弹文件选择框"
    );
    assert!(
        engine
            .db
            .is_task_unattended(&id)
            .await
            .expect("query unattended"),
        "无人值守标记必须持久化——惰性 resolve/HLS 选择在重启后的 resume 仍要静默"
    );

    let _ = tokio::fs::remove_dir_all(&work).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unattended_flag_persists_for_non_bt_tasks_too() {
    let work = std::env::temp_dir().join(format!("fluxdown-unattended-{}", uniq()));
    tokio::fs::create_dir_all(&work).await.expect("mkdir");
    let mut engine = make_engine(&work).await;
    let save_dir = work.to_string_lossy().into_owned();

    let id = engine
        .manager
        .create_task(NewTaskSpec {
            url: "https://example.com/live/master.m3u8".to_string(),
            save_dir,
            file_name: "clip.ts".to_string(),
            start_paused: true,
            unattended_selection: true,
            ..Default::default()
        })
        .await
        .expect("create unattended hls task");

    assert!(
        engine
            .db
            .is_task_unattended(&id)
            .await
            .expect("query unattended"),
        "HLS/DASH/插件任务靠 tasks.unattended 在 start 时跳过画质/变体弹窗"
    );

    let _ = tokio::fs::remove_dir_all(&work).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn manual_creation_stays_attended() {
    let work = std::env::temp_dir().join(format!("fluxdown-unattended-{}", uniq()));
    tokio::fs::create_dir_all(&work).await.expect("mkdir");
    let mut engine = make_engine(&work).await;
    let save_dir = work.to_string_lossy().into_owned();

    let id = engine
        .manager
        .create_task(NewTaskSpec {
            url: MAGNET.to_string(),
            save_dir,
            start_paused: true,
            ..Default::default()
        })
        .await
        .expect("create manual bt task");

    assert_eq!(
        engine
            .db
            .load_bt_selected_files(&id)
            .await
            .expect("query bt selection"),
        None,
        "手动建的 BT 任务不许预置全选——用户仍要自己挑文件"
    );
    assert!(
        !engine
            .db
            .is_task_unattended(&id)
            .await
            .expect("query unattended"),
        "手动路径默认在场，选择弹窗必须保留"
    );

    let _ = tokio::fs::remove_dir_all(&work).await;
}
