//! 端到端：yt-dlp 示例插件 `hooks.js` 的 `onDone` 经 `flux.ffmpeg` 把非 mp4 产物
//! 转为 mp4。真实执行依赖 ffmpeg，经 `FLUXDOWN_TEST_FFMPEG=<绝对路径>` 注入；
//! 未设置则跳过（保持 CI 无 ffmpeg 时确定性）。
//!
//! 仅 `plugins` feature 下编译运行。
#![cfg(feature = "plugins")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fluxdown_engine::db::Db;
use fluxdown_engine::plugin::bridge::EngineBridge;
use fluxdown_engine::plugin::quickjs::QuickJsScriptRuntime;
use fluxdown_engine::plugin::runtime::{
    ExecutionBudget, FfmpegSpec, HostContext, PluginBridge, PluginEntryKind, PluginEvent,
    PluginScript, ScriptRuntime,
};
use fluxdown_engine::proxy_config::ProxyConfig;

fn unique_dir(tag: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let mut d = std::env::temp_dir();
    d.push(format!(
        "fluxdown_ythook_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    std::fs::create_dir_all(&d).expect("mkdir temp");
    d
}

async fn make_bridge(data_dir: &Path, ffmpeg: &str) -> Arc<EngineBridge> {
    let db = Db::open(data_dir).await.expect("open db");
    db.set_config(fluxdown_engine::components::CONFIG_FFMPEG_PATH, ffmpeg)
        .await
        .expect("seed ffmpeg path");
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    Arc::new(
        EngineBridge::new(db, &ProxyConfig::default(), tx, data_dir.to_path_buf()).expect("bridge"),
    )
}

fn hooks_source() -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/plugins/ytdlp/hooks.js");
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("读取 {p:?} 失败: {e}"))
}

/// 在 jail 内用 ffmpeg 造一个非 mp4（VP9/opus webm）小样本作为 onDone 的输入。
async fn make_sample_webm(bridge: &EngineBridge, jail: &Path, name: &str) {
    let out = bridge
        .run_ffmpeg(
            "test@ff",
            jail.to_path_buf(),
            FfmpegSpec {
                args: vec![
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=size=32x32:rate=5",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440",
                    "-t",
                    "0.3",
                    "-c:v",
                    "libvpx-vp9",
                    "-deadline",
                    "realtime",
                    "-cpu-used",
                    "8",
                    "-c:a",
                    "libopus",
                    "-y",
                    name,
                ]
                .into_iter()
                .map(String::from)
                .collect(),
                subdir: None,
                timeout_ms: Some(60_000),
            },
        )
        .await
        .expect("gen webm");
    assert_eq!(out.code, 0, "webm 生成失败: {}", out.stderr);
    assert!(jail.join(name).exists(), "webm 样本应存在");
}

async fn run_on_done(
    rt: &QuickJsScriptRuntime,
    bridge: Arc<dyn PluginBridge>,
    jail: &Path,
    file_path: &Path,
    settings_json: &str,
) {
    let script = PluginScript {
        identity: "fluxdown@ytdlp".to_string(),
        source: hooks_source(),
        entry_fn_hint: PluginEntryKind::Hook,
        version: "1.2.0".to_string(),
        app_version: "0.1.60".to_string(),
    };
    let event = PluginEvent::Done {
        task_id: "t1".to_string(),
        url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string(),
        file_path: file_path.to_string_lossy().into_owned(),
        audio_path: None,
        muxed: true,
    };
    let budget = ExecutionBudget {
        timeout: Duration::from_secs(120),
        memory_limit_bytes: 32 * 1024 * 1024,
    };
    let host = HostContext {
        ffmpeg_permitted: true,
        ffmpeg_root: Some(jail.to_path_buf()),
        ..Default::default()
    };
    // invoke_hook 为 fire-and-forget（吞错仅记日志）：await 完成后由产物断言判定成败。
    rt.invoke_hook(
        &script,
        event,
        settings_json.to_string(),
        bridge,
        budget,
        host,
    )
    .await;
}

/// preferMp4=false + 非 mp4 产物 → onDone 应产出同名 .mp4。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn on_done_converts_webm_to_mp4() {
    let Ok(ffmpeg) = std::env::var("FLUXDOWN_TEST_FFMPEG") else {
        eprintln!("[skip] 未设置 FLUXDOWN_TEST_FFMPEG，跳过真实转码");
        return;
    };
    let data_dir = unique_dir("data_conv");
    let jail = unique_dir("jail_conv");
    let bridge = make_bridge(&data_dir, &ffmpeg).await;
    make_sample_webm(&bridge, &jail, "out.webm").await;

    let rt = QuickJsScriptRuntime::new(2).expect("runtime");
    let dyn_bridge: Arc<dyn PluginBridge> = bridge.clone();
    run_on_done(
        &rt,
        dyn_bridge,
        &jail,
        &jail.join("out.webm"),
        r#"{"preferMp4":false,"verbose":true,"quality":"best"}"#,
    )
    .await;

    let mp4 = jail.join("out.mp4");
    let meta = std::fs::metadata(&mp4).expect("onDone 应产出 out.mp4");
    assert!(meta.len() > 0, "产出的 mp4 应非空");
}

/// preferMp4=true → 门控短路，不产出 mp4（源 webm 原样保留）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn on_done_skips_when_prefer_mp4() {
    let Ok(ffmpeg) = std::env::var("FLUXDOWN_TEST_FFMPEG") else {
        eprintln!("[skip] 未设置 FLUXDOWN_TEST_FFMPEG，跳过门控断言");
        return;
    };
    let data_dir = unique_dir("data_skip");
    let jail = unique_dir("jail_skip");
    let bridge = make_bridge(&data_dir, &ffmpeg).await;
    make_sample_webm(&bridge, &jail, "keep.webm").await;

    let rt = QuickJsScriptRuntime::new(2).expect("runtime");
    let dyn_bridge: Arc<dyn PluginBridge> = bridge.clone();
    run_on_done(
        &rt,
        dyn_bridge,
        &jail,
        &jail.join("keep.webm"),
        r#"{"preferMp4":true,"verbose":true,"quality":"best"}"#,
    )
    .await;

    assert!(
        !jail.join("keep.mp4").exists(),
        "preferMp4=true 时不应产出 mp4"
    );
}
