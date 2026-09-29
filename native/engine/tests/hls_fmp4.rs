//! HLS fMP4/CMAF（EXT-X-MAP）与独立音频轨的端到端回归测试。
//!
//! 守护 BUG：YouTube 的 HLS 档是 CMAF 播放列表（`#EXT-X-MAP` 指出的初始化段
//! `ftyp`+`moov` 必须先于媒体分片落盘），且视频轨常常是**纯视频**（音频单列一条
//! 轨）。修复前引擎直接报「EXT-X-MAP (fMP4/CMAF 初始化段) 暂不支持」，YouTube
//! 这类链接一律失败。修复后：
//!   1. 初始化段落在文件最前（字节序 = init + seg0 + seg1）；
//!   2. 产物内容已是 mp4 → 最终落名为 `.mp4`（不再走 ts2mp4），DB 文件名同步；
//!   3. `audio_url`（HLS 或直链）另行下载到 `.audio.m4a` 旁挂文件，再交给 ffmpeg
//!      mux（ffmpeg 不可用时保留旁挂文件，不丢音频）。
//!
//! 用法：`cargo test -p rinadown_engine --test hls_fmp4`

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use rinadown_engine::db::Db;
use rinadown_engine::downloader::{DownloadParams, ProgressUpdate, RequestSpec, build_client};
use rinadown_engine::events::{EngineEvent, EventSink};
use rinadown_engine::proxy_config::ProxyConfig;
use rinadown_engine::speed_limiter::SpeedLimiter;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

struct NoopTestSink;
impl EventSink for NoopTestSink {
    fn emit(&self, _event: EngineEvent) {}
}

fn gen_body(len: usize, seed: u8) -> Vec<u8> {
    (0..len).map(|i| seed.wrapping_add(i as u8)).collect()
}

/// 极简静态 HTTP/1.1 服务器：按请求行取 path，命中 `routes` 则 200 + 全量 body
/// （带 `Content-Length`，供下载器的截断校验），否则 404。每个连接只服务一次
/// （`Connection: close`），避免 keep-alive 复用带来的测试不确定性。
async fn start_server(routes: HashMap<String, Vec<u8>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            let routes = routes.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let Ok(n) = stream.read(&mut buf).await else {
                    return;
                };
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = head
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/")
                    .split('?')
                    .next()
                    .unwrap_or("/")
                    .to_string();
                match routes.get(&path) {
                    Some(body) => {
                        let header = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        let _ = stream.write_all(header.as_bytes()).await;
                        let _ = stream.write_all(body).await;
                    }
                    None => {
                        let _ = stream
                            .write_all(
                                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                            )
                            .await;
                    }
                }
                let _ = stream.flush().await;
            });
        }
    });
    format!("http://{addr}")
}

fn media_playlist(init: Option<&str>, segments: &[&str]) -> Vec<u8> {
    let mut out = String::from("#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:4\n");
    if let Some(uri) = init {
        out.push_str(&format!("#EXT-X-MAP:URI=\"{uri}\"\n"));
    }
    for seg in segments {
        out.push_str(&format!("#EXTINF:4.0,\n{seg}\n"));
    }
    out.push_str("#EXT-X-ENDLIST\n");
    out.into_bytes()
}

struct Harness {
    base: String,
    work_dir: PathBuf,
    db: Db,
    updates: Arc<std::sync::Mutex<Vec<ProgressUpdate>>>,
    collector: tokio::task::JoinHandle<()>,
}

/// 起服务器 + 建引擎 DB + 建一条 HLS 任务（file_name 走 DownloadManager 的
/// 既有约定：HLS 任务先归一化为 `.ts`）。
async fn setup(routes: HashMap<String, Vec<u8>>, task_suffix: &str) -> (Harness, DownloadParams) {
    let base = start_server(routes).await;
    let work_dir = std::env::temp_dir().join(format!("rinadown-hls-fmp4-{task_suffix}"));
    let _ = tokio::fs::remove_dir_all(&work_dir).await;
    tokio::fs::create_dir_all(&work_dir).await.unwrap();
    let db = Db::open(&work_dir).await.expect("db");
    let task_id = format!("hls-{task_suffix}");
    db.insert_task(
        &task_id,
        &format!("{base}/video.m3u8"),
        "clip.ts",
        &work_dir.to_string_lossy(),
        1,
        0,
        "",
        "",
        "",
        0,
    )
    .await
    .expect("insert_task");

    let (tx, mut rx) = mpsc::channel::<ProgressUpdate>(1024);
    let updates = Arc::new(std::sync::Mutex::new(Vec::<ProgressUpdate>::new()));
    let sink_updates = updates.clone();
    let collector = tokio::spawn(async move {
        while let Some(u) = rx.recv().await {
            sink_updates.lock().expect("lock").push(u);
        }
    });

    let params = DownloadParams {
        spawn_gen: 1,
        unattended: true,
        auto_proxy: None,
        task_id,
        url: format!("{base}/video.m3u8"),
        save_dir: work_dir.to_string_lossy().to_string(),
        file_name: "clip.ts".to_string(),
        segment_count: 0,
        is_resume: false,
        range_verified: true,
        db: db.clone(),
        client: build_client(&ProxyConfig::default(), "RinaDownTest/1.0").expect("client"),
        progress_tx: tx,
        cancel_token: CancellationToken::new(),
        sink: Arc::new(NoopTestSink),
        cookies: String::new(),
        referrer: String::new(),
        speed_limiter: SpeedLimiter::new(0),
        hint_file_size: 0,
        proxy_config: ProxyConfig::default(),
        selector: Arc::new(rinadown_engine::NoopSelection),
        checksum: String::new(),
        extra_headers: HashMap::new(),
        spec: RequestSpec::empty_get(),
        audio_url: None,
        auto_max_connections: 0,
        use_server_time: false,
        allow_overwrite: false,
        // 指向不存在的 ffmpeg：mux 必然失败 → 走"保留旁挂音频文件"的降级路径，
        // 使断言与宿主是否装了 ffmpeg 无关。
        ffmpeg_path: Some(PathBuf::from("__no_such_ffmpeg__")),
        cdn: rinadown_engine::cdn::CdnTaskInput::default(),
    };

    (
        Harness {
            base,
            work_dir,
            db,
            updates,
            collector,
        },
        params,
    )
}

async fn finish(h: Harness, task_id: &str) -> (String, Vec<(i32, i64, i64)>) {
    let _ = h.collector.await;
    let summary = h
        .updates
        .lock()
        .expect("lock")
        .iter()
        .map(|u| (u.status, u.downloaded_bytes, u.total_bytes))
        .collect::<Vec<_>>();
    let file_name =
        h.db.load_task_by_id(task_id)
            .await
            .expect("load task")
            .expect("task exists")
            .file_name;
    (file_name, summary)
}

/// fMP4 播放列表：初始化段在最前，产物落名为 `.mp4`。
#[tokio::test(flavor = "current_thread")]
async fn fmp4_playlist_prepends_init_segment_and_lands_as_mp4() {
    let init = gen_body(16, 0xA0);
    let seg0 = gen_body(100, 0x10);
    let seg1 = gen_body(50, 0x20);

    let mut routes = HashMap::new();
    routes.insert(
        "/video.m3u8".to_string(),
        media_playlist(Some("init.mp4"), &["seg0", "seg1"]),
    );
    routes.insert("/init.mp4".to_string(), init.clone());
    routes.insert("/seg0".to_string(), seg0.clone());
    routes.insert("/seg1".to_string(), seg1.clone());

    let (h, params) = setup(routes, "fmp4").await;
    let work_dir = h.work_dir.clone();
    let task_id = params.task_id.clone();
    rinadown_engine::hls_downloader::run_hls_download(params).await;
    let (file_name, summary) = finish(h, &task_id).await;

    assert_eq!(
        file_name, "clip.mp4",
        "fMP4 产物内容已是 mp4，最终落名必须是 .mp4（而不是被 remux 成 .ts）"
    );
    let expected: Vec<u8> = [init, seg0, seg1].concat();
    let got = tokio::fs::read(work_dir.join("clip.mp4"))
        .await
        .expect("output file");
    assert_eq!(
        got, expected,
        "字节序必须是 初始化段 + seg0 + seg1（初始化段只写一次且落在文件最前）"
    );
    assert!(
        !tokio::fs::try_exists(work_dir.join("clip.ts"))
            .await
            .unwrap_or(false),
        "mp4 落名后不应残留 .ts"
    );
    assert_eq!(
        summary.last().map(|s| s.0),
        Some(3),
        "任务必须以 status=3 结束: {summary:?}"
    );
}

/// 独立音频轨：HLS 音频播放列表（含自身初始化段）下载到旁挂文件；ffmpeg 不可用时
/// 保留旁挂文件而不是丢弃音频。
#[tokio::test(flavor = "current_thread")]
async fn separate_hls_audio_track_is_downloaded_next_to_video() {
    let vinit = gen_body(12, 0xB0);
    let vseg = gen_body(64, 0x30);
    let ainit = gen_body(10, 0xC0);
    let aseg = gen_body(40, 0x40);

    let mut routes = HashMap::new();
    routes.insert(
        "/video.m3u8".to_string(),
        media_playlist(Some("vinit.mp4"), &["vseg0"]),
    );
    routes.insert("/vinit.mp4".to_string(), vinit.clone());
    routes.insert("/vseg0".to_string(), vseg.clone());
    routes.insert(
        "/audio.m3u8".to_string(),
        media_playlist(Some("ainit.mp4"), &["aseg0"]),
    );
    routes.insert("/ainit.mp4".to_string(), ainit.clone());
    routes.insert("/aseg0".to_string(), aseg.clone());

    let (h, params) = setup(routes, "audio").await;
    let work_dir = h.work_dir.clone();
    let task_id = params.task_id.clone();
    let audio_url = format!("{}/audio.m3u8", h.base);
    let mut params = params;
    params.audio_url = Some(audio_url.clone());

    rinadown_engine::hls_downloader::run_hls_download(params).await;
    let (file_name, summary) = finish(h, &task_id).await;

    assert_eq!(file_name, "clip.mp4", "视频轨为 fMP4 → 落名 .mp4");
    let video = tokio::fs::read(work_dir.join("clip.mp4"))
        .await
        .expect("video output");
    assert_eq!(video, [vinit, vseg].concat(), "视频轨字节序");

    let audio = tokio::fs::read(work_dir.join("clip.audio.m4a"))
        .await
        .expect("音频轨旁挂文件（ffmpeg 不可用时必须保留）");
    assert_eq!(
        audio,
        [ainit, aseg].concat(),
        "音频轨同样要预置自己的初始化段（fMP4 音频）"
    );
    assert_eq!(
        summary.last().map(|s| s.0),
        Some(3),
        "mux 失败不是致命错误，任务仍须完成: {summary:?}"
    );
}
