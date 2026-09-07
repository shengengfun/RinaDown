//! 浏览器 Native Messaging Host 与 agent 之间的长度帧 IPC 服务。

use std::sync::Arc;

use fluxdown_protocol::{DownloadRequest, TaskDto};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use crate::capture::CaptureService;
use crate::daemon_client::DaemonClient;

const MAX_MESSAGE_SIZE: u32 = 1024 * 1024;
const MAX_BATCH_ITEMS: usize = 1000;
const MAX_COMPLETED_TASKS: usize = 10;

#[derive(Deserialize)]
struct PipeMessage {
    action: String,
    #[serde(default)]
    msg_id: u64,
    #[serde(flatten)]
    payload: Value,
}

#[derive(Deserialize)]
struct BatchDownloadPayload {
    items: Vec<DownloadRequest>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskOperation {
    op: String,
    task_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskIdPayload {
    task_id: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskBrief {
    task_id: String,
    file_name: String,
    status: i32,
    downloaded_bytes: i64,
    total_bytes: i64,
    speed: i64,
    error_message: String,
    created_at: String,
}

#[derive(Serialize)]
struct PipeResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    msg_id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    tasks: Option<Vec<TaskBrief>>,
}

impl PipeResponse {
    fn ok(msg_id: u64, message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: Some(message.into()),
            msg_id,
            tasks: None,
        }
    }

    fn error(msg_id: u64, message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: Some(message.into()),
            msg_id,
            tasks: None,
        }
    }

    fn tasks(msg_id: u64, tasks: Vec<TaskBrief>) -> Self {
        Self {
            success: true,
            message: None,
            msg_id,
            tasks: Some(tasks),
        }
    }
}

#[derive(Clone)]
pub struct NmhService {
    daemon: Arc<DaemonClient>,
    capture: Arc<CaptureService>,
}

impl NmhService {
    #[must_use]
    pub fn new(daemon: Arc<DaemonClient>, capture: Arc<CaptureService>) -> Self {
        Self { daemon, capture }
    }

    pub async fn run(self, cancel: CancellationToken) -> Result<(), std::io::Error> {
        run_server(self, cancel).await
    }

    async fn dispatch(&self, message: PipeMessage) -> PipeResponse {
        match message.action.as_str() {
            "ping" => PipeResponse::ok(message.msg_id, "pong"),
            "download" => match serde_json::from_value::<DownloadRequest>(message.payload) {
                Ok(request) => match self.capture.submit(request, false).await {
                    Ok(_) => PipeResponse::ok(message.msg_id, "download accepted"),
                    Err(error) => PipeResponse::error(message.msg_id, error.to_string()),
                },
                Err(error) => PipeResponse::error(message.msg_id, error.to_string()),
            },
            "batch_download" => self.batch_download(message.msg_id, message.payload).await,
            "tasks" => self.task_list(message.msg_id).await,
            "task_op" => self.task_operation(message.msg_id, message.payload).await,
            "open_file" => {
                self.platform_action(message.msg_id, message.payload, false)
                    .await
            }
            "reveal_file" => {
                self.platform_action(message.msg_id, message.payload, true)
                    .await
            }
            other => PipeResponse::error(message.msg_id, format!("unknown action: {other}")),
        }
    }

    async fn batch_download(&self, msg_id: u64, payload: Value) -> PipeResponse {
        let batch = match serde_json::from_value::<BatchDownloadPayload>(payload) {
            Ok(batch) if !batch.items.is_empty() && batch.items.len() <= MAX_BATCH_ITEMS => batch,
            Ok(batch) => {
                return PipeResponse::error(
                    msg_id,
                    format!("invalid batch size: {}", batch.items.len()),
                );
            }
            Err(error) => return PipeResponse::error(msg_id, error.to_string()),
        };
        let count = batch.items.len();
        for request in batch.items {
            if let Err(error) = self.capture.submit(request, false).await {
                return PipeResponse::error(msg_id, error.to_string());
            }
        }
        PipeResponse::ok(msg_id, format!("batch accepted ({count} items)"))
    }

    async fn task_list(&self, msg_id: u64) -> PipeResponse {
        let tasks = match self
            .daemon
            .call::<Value, Vec<TaskDto>>(fluxdown_protocol::method::DAEMON_TASK_LIST, None)
            .await
        {
            Ok(tasks) => tasks,
            Err(error) => return PipeResponse::error(msg_id, format!("{:?}", error.code)),
        };
        PipeResponse::tasks(msg_id, select_task_briefs(tasks))
    }

    async fn task_operation(&self, msg_id: u64, payload: Value) -> PipeResponse {
        let operation = match serde_json::from_value::<TaskOperation>(payload) {
            Ok(operation) => operation,
            Err(error) => return PipeResponse::error(msg_id, error.to_string()),
        };
        let method = match operation.op.as_str() {
            "pause" => fluxdown_protocol::method::DAEMON_TASK_PAUSE,
            "resume" => fluxdown_protocol::method::DAEMON_TASK_RESUME,
            "remove" => fluxdown_protocol::method::DAEMON_TASK_DELETE,
            other => return PipeResponse::error(msg_id, format!("unknown task op: {other}")),
        };
        let params = if operation.op == "remove" {
            serde_json::json!({"taskId": operation.task_id, "deleteFiles": false})
        } else {
            serde_json::json!({"taskId": operation.task_id})
        };
        match self.daemon.call::<Value, Value>(method, Some(params)).await {
            Ok(_) => PipeResponse::ok(msg_id, "ok"),
            Err(error) => PipeResponse::error(msg_id, format!("{:?}", error.code)),
        }
    }

    async fn platform_action(&self, msg_id: u64, payload: Value, reveal: bool) -> PipeResponse {
        let request = match serde_json::from_value::<TaskIdPayload>(payload) {
            Ok(request) => request,
            Err(error) => return PipeResponse::error(msg_id, error.to_string()),
        };
        let task = match self
            .daemon
            .call::<Value, TaskDto>(
                fluxdown_protocol::method::DAEMON_TASK_GET,
                Some(serde_json::json!({"taskId": request.task_id})),
            )
            .await
        {
            Ok(task) => task,
            Err(error) => return PipeResponse::error(msg_id, format!("{:?}", error.code)),
        };
        if task.status != 3 {
            return PipeResponse::error(msg_id, "task is not completed");
        }
        let outcome = if reveal {
            crate::platform::reveal_task(&task)
        } else {
            crate::platform::open_task(&task)
        };
        match outcome {
            Ok(()) => PipeResponse::ok(msg_id, "ok"),
            Err(error) => PipeResponse::error(msg_id, error.to_string()),
        }
    }
}

fn select_task_briefs(tasks: Vec<TaskDto>) -> Vec<TaskBrief> {
    let (mut completed, active): (Vec<_>, Vec<_>) =
        tasks.into_iter().partition(|task| task.status == 3);
    completed
        .sort_by_key(|task| std::cmp::Reverse(task.created_at.parse::<i64>().unwrap_or_default()));
    completed.truncate(MAX_COMPLETED_TASKS);
    active
        .into_iter()
        .chain(completed)
        .map(|task| TaskBrief {
            task_id: task.task_id,
            file_name: task.file_name,
            status: task.status,
            downloaded_bytes: task.downloaded_bytes,
            total_bytes: task.total_bytes,
            speed: 0,
            error_message: task.error_message,
            created_at: task.created_at,
        })
        .collect()
}

async fn handle_stream<S>(mut stream: S, service: NmhService) -> Result<(), std::io::Error>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    loop {
        let mut length = [0_u8; 4];
        if let Err(error) = stream.read_exact(&mut length).await {
            if error.kind() == std::io::ErrorKind::UnexpectedEof {
                return Ok(());
            }
            return Err(error);
        }
        let length = u32::from_le_bytes(length);
        if length == 0 || length > MAX_MESSAGE_SIZE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "native messaging frame is invalid",
            ));
        }
        let mut payload = vec![0_u8; length as usize];
        stream.read_exact(&mut payload).await?;
        let response = match serde_json::from_slice::<PipeMessage>(&payload) {
            Ok(message) => service.dispatch(message).await,
            Err(error) => PipeResponse::error(0, error.to_string()),
        };
        let bytes = serde_json::to_vec(&response).map_err(std::io::Error::other)?;
        let length =
            u32::try_from(bytes.len()).map_err(|error| std::io::Error::other(error.to_string()))?;
        stream.write_all(&length.to_le_bytes()).await?;
        stream.write_all(&bytes).await?;
        stream.flush().await?;
    }
}

#[cfg(unix)]
async fn run_server(service: NmhService, cancel: CancellationToken) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;

    let path = unix_socket_path();
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    if tokio::fs::try_exists(&path).await? {
        match tokio::net::UnixStream::connect(&path).await {
            Ok(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AddrInUse,
                    "native messaging socket is already active",
                ));
            }
            Err(_) => tokio::fs::remove_file(&path).await?,
        }
    }
    let listener = tokio::net::UnixListener::bind(&path)?;
    tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).await?;
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                drop(listener);
                let _ = tokio::fs::remove_file(&path).await;
                return Ok(());
            }
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let service = service.clone();
                tokio::spawn(async move {
                    if let Err(error) = handle_stream(stream, service).await {
                        tracing::debug!(error = %error, "NMH socket closed");
                    }
                });
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn unix_socket_path() -> std::path::PathBuf {
    std::env::var_os("HOME").map_or_else(
        || std::path::PathBuf::from("/tmp/fluxdown.sock"),
        |home| std::path::PathBuf::from(home).join(".local/share/fluxdown/fluxdown.sock"),
    )
}

#[cfg(target_os = "macos")]
fn unix_socket_path() -> std::path::PathBuf {
    std::env::var_os("HOME").map_or_else(
        || std::path::PathBuf::from("/tmp/fluxdown.sock"),
        |home| {
            std::path::PathBuf::from(home)
                .join("Library/Application Support/fluxdown/fluxdown.sock")
        },
    )
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn unix_socket_path() -> std::path::PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map_or_else(
        || std::path::PathBuf::from("/tmp/fluxdown.sock"),
        |dir| std::path::PathBuf::from(dir).join("fluxdown.sock"),
    )
}

#[cfg(windows)]
async fn run_server(service: NmhService, cancel: CancellationToken) -> Result<(), std::io::Error> {
    use tokio::net::windows::named_pipe::ServerOptions;

    let mut first = true;
    loop {
        let server = ServerOptions::new()
            .first_pipe_instance(first)
            .create(PIPE_NAME)?;
        first = false;
        tokio::select! {
            _ = cancel.cancelled() => return Ok(()),
            connected = server.connect() => connected?,
        }
        let service = service.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_stream(server, service).await {
                tracing::debug!(error = %error, "NMH pipe closed");
            }
        });
    }
}

/// 中继拨号的 IPC 端点（unix socket 路径或命名管道名）。
#[must_use]
pub fn ipc_endpoint() -> String {
    #[cfg(unix)]
    {
        unix_socket_path().display().to_string()
    }
    #[cfg(windows)]
    {
        PIPE_NAME.to_owned()
    }
}

#[cfg(windows)]
const PIPE_NAME: &str = r"\\.\pipe\fluxdown";

/// 以中继的长度帧协议向本进程 IPC 端点发送 `ping`，成功返回 `pong` 载荷。
pub async fn probe_ipc(timeout: std::time::Duration) -> Result<String, std::io::Error> {
    tokio::time::timeout(timeout, async {
        #[cfg(unix)]
        let stream = tokio::net::UnixStream::connect(unix_socket_path()).await?;
        #[cfg(windows)]
        let stream = tokio::net::windows::named_pipe::ClientOptions::new().open(PIPE_NAME)?;
        ping_stream(stream).await
    })
    .await
    .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "IPC ping timed out"))?
}

async fn ping_stream<S>(mut stream: S) -> Result<String, std::io::Error>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let request = serde_json::to_vec(&serde_json::json!({ "action": "ping", "msg_id": 1 }))
        .map_err(std::io::Error::other)?;
    let length =
        u32::try_from(request.len()).map_err(|error| std::io::Error::other(error.to_string()))?;
    stream.write_all(&length.to_le_bytes()).await?;
    stream.write_all(&request).await?;
    stream.flush().await?;
    let mut length = [0_u8; 4];
    stream.read_exact(&mut length).await?;
    let length = u32::from_le_bytes(length);
    if length == 0 || length > MAX_MESSAGE_SIZE {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "IPC ping response frame is invalid",
        ));
    }
    let mut payload = vec![0_u8; length as usize];
    stream.read_exact(&mut payload).await?;
    let response: Value = serde_json::from_slice(&payload)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let success = response.get("success").and_then(Value::as_bool) == Some(true);
    match response.get("message").and_then(Value::as_str) {
        Some("pong") if success => Ok("pong".to_owned()),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("unexpected IPC ping response: {response}"),
        )),
    }
}

/// 浏览器 Native Messaging Host 清单注册：写出指向 `fluxdown_nmh` 中继的清单，
/// 并为 Doctor 提供只读诊断快照。
///
/// 注册目标与 Flutter 时代的 hub 完全一致（Chrome/Edge/Firefox 及各 Chromium 分支），
/// 但中继二进制按 `fluxdown-agent` 的同级目录查找。
pub mod registry {
    use std::io;
    use std::path::{Path, PathBuf};

    use serde::Serialize;

    const NMH_NAME: &str = "com.fluxdown.nmh";
    const NMH_DESCRIPTION: &str = "FluxDown Native Messaging Host";
    #[cfg(windows)]
    const NMH_EXE_NAME: &str = "fluxdown_nmh.exe";
    #[cfg(not(windows))]
    const NMH_EXE_NAME: &str = "fluxdown_nmh";
    /// Chrome 扩展 ID（wxt.config.ts 里通过 `key` 固定）。
    const CHROME_EXTENSION_ID: &str = "chrome-extension://meleenglfggcmcajknpeeeiobnpfmahc/";
    /// Edge 商店扩展 ID：Edge 忽略清单 `key`，必须单独放行，否则 connectNative 报 forbidden。
    const EDGE_EXTENSION_ID: &str = "chrome-extension://nglkkjbogjghekbhhcnccnpfedjbdhhd/";
    const FIREFOX_EXTENSION_ID: &str = "fluxdown@fluxdown.app";

    /// 单个浏览器的 NMH 注册状态。
    #[derive(Debug, Clone)]
    pub struct NmhTarget {
        /// 展示名，如 `Chrome` / `Firefox` / `Brave (Flatpak)`。
        pub label: String,
        /// 注册位置：Windows 为注册表键，类 Unix 为清单文件绝对路径。
        pub location: String,
        /// 浏览器是否安装（配置根目录存在）；false 时 Doctor 只报 info。
        pub installed: bool,
        /// 已注册且指向当前中继。
        pub ok: bool,
        /// `ok == false` 时的具体原因；ok 时为空串。
        pub issue: String,
    }

    /// NMH 注册整体诊断快照。
    #[derive(Debug, Clone, Default)]
    pub struct NmhDiagnosis {
        /// 中继可执行文件绝对路径；空表示未找到。
        pub exe_path: String,
        /// 未找到中继时的原因；找到时为空串。
        pub exe_error: String,
        /// Chromium 清单**期望**路径（可能尚未写出）。
        pub chromium_manifest: String,
        /// Firefox 清单期望路径。
        pub firefox_manifest: String,
        /// 每个浏览器一条；未找到中继时为空。
        pub targets: Vec<NmhTarget>,
    }

    /// Chromium 系清单（`allowed_origins`）。
    #[derive(Serialize)]
    struct NmhManifestChromium {
        name: String,
        description: String,
        path: String,
        #[serde(rename = "type")]
        host_type: String,
        allowed_origins: Vec<String>,
    }

    /// Firefox 清单：只能有 `allowed_extensions`，多出 `allowed_origins` 会被 schema 校验拒绝。
    #[derive(Serialize)]
    struct NmhManifestFirefox {
        name: String,
        description: String,
        path: String,
        #[serde(rename = "type")]
        host_type: String,
        allowed_extensions: Vec<String>,
    }

    fn chromium_manifest_json(path: &str) -> Result<String, io::Error> {
        serde_json::to_string_pretty(&NmhManifestChromium {
            name: NMH_NAME.to_owned(),
            description: NMH_DESCRIPTION.to_owned(),
            path: path.to_owned(),
            host_type: "stdio".to_owned(),
            allowed_origins: vec![CHROME_EXTENSION_ID.to_owned(), EDGE_EXTENSION_ID.to_owned()],
        })
        .map_err(io::Error::other)
    }

    fn firefox_manifest_json(path: &str) -> Result<String, io::Error> {
        serde_json::to_string_pretty(&NmhManifestFirefox {
            name: NMH_NAME.to_owned(),
            description: NMH_DESCRIPTION.to_owned(),
            path: path.to_owned(),
            host_type: "stdio".to_owned(),
            allowed_extensions: vec![FIREFOX_EXTENSION_ID.to_owned()],
        })
        .map_err(io::Error::other)
    }

    /// 查找中继二进制：先看 agent 同级目录（发布形态），再看 cargo workspace `target/`（开发）。
    fn find_nmh_exe() -> Result<PathBuf, io::Error> {
        if let Ok(exe) = std::env::current_exe() {
            let canonical = std::fs::canonicalize(&exe).unwrap_or(exe);
            if let Some(dir) = canonical.parent() {
                let candidate = dir.join(NMH_EXE_NAME);
                if candidate.exists() {
                    return Ok(candidate);
                }
            }
        }
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent);
        if let Some(workspace) = workspace_root {
            for profile in ["debug", "release"] {
                let candidate = workspace.join("target").join(profile).join(NMH_EXE_NAME);
                if candidate.exists() {
                    return Ok(candidate);
                }
            }
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{NMH_EXE_NAME} not found. Build it with: cargo build -p fluxdown_nmh"),
        ))
    }

    /// 已安装浏览器中存在缺失或过期注册时为 true；判定口径与 [`diagnose`] 完全一致。
    #[must_use]
    pub fn needs_update() -> bool {
        let diagnosis = diagnose();
        diagnosis.exe_path.is_empty()
            || diagnosis
                .targets
                .iter()
                .any(|target| target.installed && !target.ok)
    }

    #[cfg(unix)]
    pub use unix::{diagnose, register};
    #[cfg(windows)]
    pub use windows::{diagnose, register};

    /// 类 Unix：清单指向 shell 包装脚本，脚本再 `exec` 真实中继。
    ///
    /// macOS 上 Hardened Runtime 的浏览器只允许拉起系统签名的 `/bin/sh`；
    /// Linux 上 AppImage 挂载点每次启动都变，包装脚本给清单一个稳定路径。
    #[cfg(unix)]
    mod unix {
        use std::io;
        use std::path::{Path, PathBuf};

        use super::{EDGE_EXTENSION_ID, NmhDiagnosis, NmhTarget};

        const MANIFEST_FILENAME: &str = "com.fluxdown.nmh.json";
        const NMH_WRAPPER_NAME: &str = "fluxdown_nmh.sh";

        fn home_dir() -> Option<PathBuf> {
            directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf())
        }

        /// 浏览器已安装的代理判定：清单目录的父目录（profile 根）存在。
        fn browser_installed(nmh_dir: &Path) -> bool {
            nmh_dir.parent().is_some_and(Path::is_dir)
        }

        #[cfg(target_os = "macos")]
        fn wrapper_path() -> Option<PathBuf> {
            home_dir().map(|home| {
                home.join("Library")
                    .join("Application Support")
                    .join("fluxdown")
                    .join(NMH_WRAPPER_NAME)
            })
        }

        #[cfg(not(target_os = "macos"))]
        fn wrapper_path() -> Option<PathBuf> {
            home_dir().map(|home| {
                home.join(".local")
                    .join("share")
                    .join("fluxdown")
                    .join(NMH_WRAPPER_NAME)
            })
        }

        #[cfg(target_os = "macos")]
        fn chromium_nmh_dirs() -> Vec<PathBuf> {
            let Some(home) = home_dir() else {
                return Vec::new();
            };
            let lib = home.join("Library").join("Application Support");
            vec![
                lib.join("Google")
                    .join("Chrome")
                    .join("NativeMessagingHosts"),
                lib.join("Google")
                    .join("Chrome Beta")
                    .join("NativeMessagingHosts"),
                lib.join("Google")
                    .join("Chrome Canary")
                    .join("NativeMessagingHosts"),
                lib.join("Chromium").join("NativeMessagingHosts"),
                lib.join("Microsoft Edge").join("NativeMessagingHosts"),
                lib.join("Microsoft Edge Beta").join("NativeMessagingHosts"),
                lib.join("Arc")
                    .join("User Data")
                    .join("NativeMessagingHosts"),
                lib.join("BraveSoftware")
                    .join("Brave-Browser")
                    .join("NativeMessagingHosts"),
                lib.join("Vivaldi").join("NativeMessagingHosts"),
            ]
        }

        /// Firefox 清单目录及其“已安装”判定。macOS 上 Firefox 的 profile 根是
        /// `Firefox/` 而清单目录在 `Mozilla/`，两者任一存在都算已安装。
        #[cfg(target_os = "macos")]
        fn firefox_targets() -> Vec<(PathBuf, bool)> {
            let Some(home) = home_dir() else {
                return Vec::new();
            };
            let lib = home.join("Library").join("Application Support");
            let installed = lib.join("Firefox").is_dir() || lib.join("Mozilla").is_dir();
            vec![(lib.join("Mozilla").join("NativeMessagingHosts"), installed)]
        }

        #[cfg(target_os = "macos")]
        fn label_for_dir(dir: &Path) -> String {
            fn dir_name(path: Option<&Path>) -> &str {
                path.and_then(Path::file_name)
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
            }
            let parent = dir.parent();
            let mut root = dir_name(parent);
            if root == "User Data" {
                root = dir_name(parent.and_then(Path::parent));
            }
            match root {
                "Microsoft Edge" => "Edge",
                "Microsoft Edge Beta" => "Edge Beta",
                "Brave-Browser" => "Brave",
                "Mozilla" => "Firefox",
                "" => "Unknown browser",
                other => other,
            }
            .to_owned()
        }

        #[cfg(not(target_os = "macos"))]
        fn chromium_nmh_dirs() -> Vec<PathBuf> {
            let Some(home) = home_dir() else {
                return Vec::new();
            };
            let config = home.join(".config");
            let var_app = home.join(".var").join("app");
            let snap = home.join("snap");
            vec![
                config.join("google-chrome").join("NativeMessagingHosts"),
                config.join("chromium").join("NativeMessagingHosts"),
                config.join("microsoft-edge").join("NativeMessagingHosts"),
                config
                    .join("BraveSoftware")
                    .join("Brave-Browser")
                    .join("NativeMessagingHosts"),
                config.join("vivaldi").join("NativeMessagingHosts"),
                var_app
                    .join("com.google.Chrome")
                    .join("config")
                    .join("google-chrome")
                    .join("NativeMessagingHosts"),
                var_app
                    .join("org.chromium.Chromium")
                    .join("config")
                    .join("chromium")
                    .join("NativeMessagingHosts"),
                var_app
                    .join("com.microsoft.Edge")
                    .join("config")
                    .join("microsoft-edge")
                    .join("NativeMessagingHosts"),
                var_app
                    .join("com.brave.Browser")
                    .join("config")
                    .join("BraveSoftware")
                    .join("Brave-Browser")
                    .join("NativeMessagingHosts"),
                snap.join("chromium")
                    .join("common")
                    .join(".config")
                    .join("chromium")
                    .join("NativeMessagingHosts"),
            ]
        }

        #[cfg(not(target_os = "macos"))]
        fn firefox_targets() -> Vec<(PathBuf, bool)> {
            let Some(home) = home_dir() else {
                return Vec::new();
            };
            let var_app = home.join(".var").join("app");
            [
                home.join(".mozilla").join("native-messaging-hosts"),
                var_app
                    .join("org.mozilla.firefox")
                    .join(".mozilla")
                    .join("native-messaging-hosts"),
                home.join(".librewolf").join("native-messaging-hosts"),
                home.join(".zen").join("native-messaging-hosts"),
                var_app
                    .join("io.gitlab.librewolf-community")
                    .join(".librewolf")
                    .join("native-messaging-hosts"),
            ]
            .into_iter()
            .map(|dir| {
                let installed = browser_installed(&dir);
                (dir, installed)
            })
            .collect()
        }

        #[cfg(not(target_os = "macos"))]
        fn label_for_dir(dir: &Path) -> String {
            let flatpak = dir
                .components()
                .any(|component| component.as_os_str().to_str() == Some(".var"));
            let snap = !flatpak
                && dir
                    .components()
                    .any(|component| component.as_os_str().to_str() == Some("snap"));
            let root = dir
                .parent()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            let base = match root {
                "google-chrome" => "Chrome",
                "chromium" => "Chromium",
                "microsoft-edge" => "Edge",
                "Brave-Browser" => "Brave",
                "vivaldi" => "Vivaldi",
                ".mozilla" => "Firefox",
                ".librewolf" => "LibreWolf",
                ".zen" => "Zen Browser",
                "" => "Unknown browser",
                other => other,
            };
            if flatpak {
                format!("{base} (Flatpak)")
            } else if snap {
                format!("{base} (Snap)")
            } else {
                base.to_owned()
            }
        }

        fn write_wrapper_script(nmh_exe: &Path) -> Result<PathBuf, io::Error> {
            use std::os::unix::fs::PermissionsExt;

            let Some(wrapper) = wrapper_path() else {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "cannot determine home directory for wrapper script",
                ));
            };
            if let Some(parent) = wrapper.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let script = format!("#!/bin/sh\nexec '{}' \"$@\"\n", nmh_exe.display());
            std::fs::write(&wrapper, script)?;
            std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755))?;
            Ok(wrapper)
        }

        fn write_manifest(dir: &Path, json: &str) -> Result<PathBuf, io::Error> {
            std::fs::create_dir_all(dir)?;
            let path = dir.join(MANIFEST_FILENAME);
            std::fs::write(&path, json)?;
            Ok(path)
        }

        /// 只读检查单个浏览器清单目录；清单指向包装脚本，脚本本身的问题在清单无误后再浮出。
        fn diagnose_dir(
            dir: &Path,
            installed: bool,
            wrapper_str: &str,
            require_edge_origin: bool,
            wrapper_issue: Option<&str>,
        ) -> NmhTarget {
            let manifest = dir.join(MANIFEST_FILENAME);
            let location = manifest.display().to_string();
            let issue = match std::fs::read_to_string(&manifest) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    format!("manifest file missing: {location}")
                }
                Err(error) => format!("manifest unreadable: {location}: {error}"),
                Ok(content) => {
                    if !content.contains(wrapper_str) {
                        format!("manifest does not point to current relay: {location}")
                    } else if require_edge_origin && !content.contains(EDGE_EXTENSION_ID) {
                        format!("missing Edge origin in manifest: {location}")
                    } else {
                        wrapper_issue.unwrap_or_default().to_owned()
                    }
                }
            };
            NmhTarget {
                label: label_for_dir(dir),
                location,
                installed,
                ok: issue.is_empty(),
                issue,
            }
        }

        /// 只读注册快照；从不写清单、包装脚本或目录。
        #[must_use]
        pub fn diagnose() -> NmhDiagnosis {
            let mut diagnosis = NmhDiagnosis::default();
            let chromium_dirs = chromium_nmh_dirs();
            let firefox_dirs = firefox_targets();
            if let Some(first) = chromium_dirs.first() {
                diagnosis.chromium_manifest = first.join(MANIFEST_FILENAME).display().to_string();
            }
            if let Some((first, _)) = firefox_dirs.first() {
                diagnosis.firefox_manifest = first.join(MANIFEST_FILENAME).display().to_string();
            }
            let nmh_exe = match super::find_nmh_exe() {
                Ok(path) => path,
                Err(error) => {
                    diagnosis.exe_error = error.to_string();
                    return diagnosis;
                }
            };
            diagnosis.exe_path = nmh_exe.display().to_string();
            let Some(wrapper) = wrapper_path() else {
                return diagnosis;
            };
            let wrapper_str = wrapper.display().to_string();
            let wrapper_issue = if !wrapper.exists() {
                Some(format!("wrapper script missing: {wrapper_str}"))
            } else if std::fs::read_to_string(&wrapper)
                .is_ok_and(|content| content.contains(&diagnosis.exe_path))
            {
                None
            } else {
                Some(format!(
                    "wrapper script does not point to current relay: {wrapper_str}"
                ))
            };
            for dir in &chromium_dirs {
                diagnosis.targets.push(diagnose_dir(
                    dir,
                    browser_installed(dir),
                    &wrapper_str,
                    true,
                    wrapper_issue.as_deref(),
                ));
            }
            for (dir, installed) in &firefox_dirs {
                diagnosis.targets.push(diagnose_dir(
                    dir,
                    *installed,
                    &wrapper_str,
                    false,
                    wrapper_issue.as_deref(),
                ));
            }
            diagnosis
        }

        /// 为所有已安装浏览器写出清单；未安装的浏览器不凭空创建 profile 目录。
        pub fn register() -> Result<(), io::Error> {
            let nmh_exe = super::find_nmh_exe()?;
            let wrapper = write_wrapper_script(&nmh_exe)?;
            let wrapper_str = wrapper.display().to_string();
            let chromium = super::chromium_manifest_json(&wrapper_str)?;
            let firefox = super::firefox_manifest_json(&wrapper_str)?;
            for dir in chromium_nmh_dirs() {
                if !browser_installed(&dir) {
                    continue;
                }
                match write_manifest(&dir, &chromium) {
                    Ok(path) => {
                        tracing::info!(path = %path.display(), "NMH Chromium manifest written")
                    }
                    Err(error) => {
                        tracing::warn!(dir = %dir.display(), error = %error, "NMH Chromium manifest write failed")
                    }
                }
            }
            for (dir, installed) in firefox_targets() {
                if !installed {
                    continue;
                }
                match write_manifest(&dir, &firefox) {
                    Ok(path) => {
                        tracing::info!(path = %path.display(), "NMH Firefox manifest written")
                    }
                    Err(error) => {
                        tracing::warn!(dir = %dir.display(), error = %error, "NMH Firefox manifest write failed")
                    }
                }
            }
            tracing::info!(exe = %nmh_exe.display(), wrapper = %wrapper.display(), "NMH registered");
            Ok(())
        }
    }

    /// Windows：HKCU 注册表键指向写在中继旁边的两份清单（Chromium / Firefox 各一份）。
    #[cfg(windows)]
    mod windows {
        use std::io;
        use std::path::{Path, PathBuf};

        use winreg::RegKey;
        use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};

        use super::{EDGE_EXTENSION_ID, NMH_NAME, NmhDiagnosis, NmhTarget};

        const MANIFEST_FILENAME_CHROMIUM: &str = "com.fluxdown.nmh.json";
        const MANIFEST_FILENAME_FIREFOX: &str = "com.fluxdown.nmh.firefox.json";
        /// Brave/Vivaldi/Opera 等分支在自身键缺失时回退读 Chrome 键，只需 Chrome 与 Edge。
        const CHROMIUM_REG_PATHS: [(&str, &str); 2] = [
            (r"Software\Google\Chrome\NativeMessagingHosts", "Chrome"),
            (r"Software\Microsoft\Edge\NativeMessagingHosts", "Edge"),
        ];
        const FIREFOX_REG_PATH: &str = r"Software\Mozilla\NativeMessagingHosts";

        fn strip_unc_prefix(path: &str) -> String {
            path.strip_prefix(r"\\?\").unwrap_or(path).to_owned()
        }

        fn env_dir_exists(var: &str, rest: &[&str]) -> bool {
            let Ok(base) = std::env::var(var) else {
                return false;
            };
            let mut path = PathBuf::from(base);
            for segment in rest {
                path.push(segment);
            }
            path.is_dir()
        }

        /// 只读检查单个注册表键，返回失败原因；全部匹配时为空串。
        fn diagnose_registry(
            hkcu: &RegKey,
            reg_path: &str,
            manifest_filename: &str,
            expected_exe_json: &str,
            require_edge_origin: bool,
        ) -> String {
            let full_path = format!("{reg_path}\\{NMH_NAME}");
            let Ok(key) = hkcu.open_subkey_with_flags(&full_path, KEY_READ) else {
                return format!("registry key missing: HKCU\\{full_path}");
            };
            let Ok(manifest_str) = key.get_value::<String, _>("") else {
                return format!("registry default value unreadable: HKCU\\{full_path}");
            };
            if !manifest_str.ends_with(manifest_filename) {
                return format!("registry points to unexpected manifest: {manifest_str}");
            }
            if !Path::new(&manifest_str).exists() {
                return format!("manifest file missing: {manifest_str}");
            }
            let content = match std::fs::read_to_string(&manifest_str) {
                Ok(content) => content,
                Err(error) => return format!("manifest unreadable: {manifest_str}: {error}"),
            };
            if !content.contains(expected_exe_json) {
                return format!("manifest does not point to current relay: {manifest_str}");
            }
            if require_edge_origin && !content.contains(EDGE_EXTENSION_ID) {
                return format!("missing Edge origin in manifest: {manifest_str}");
            }
            String::new()
        }

        /// 只读注册快照；从不写注册表、清单或目录。
        #[must_use]
        pub fn diagnose() -> NmhDiagnosis {
            let mut diagnosis = NmhDiagnosis::default();
            let nmh_exe = match super::find_nmh_exe() {
                Ok(path) => path,
                Err(error) => {
                    diagnosis.exe_error = error.to_string();
                    return diagnosis;
                }
            };
            diagnosis.exe_path = strip_unc_prefix(&nmh_exe.to_string_lossy());
            if let Some(dir) = nmh_exe.parent() {
                diagnosis.chromium_manifest =
                    strip_unc_prefix(&dir.join(MANIFEST_FILENAME_CHROMIUM).to_string_lossy());
                diagnosis.firefox_manifest =
                    strip_unc_prefix(&dir.join(MANIFEST_FILENAME_FIREFOX).to_string_lossy());
            }
            // 清单由 serde_json 写出，路径中的 `\` 被转义为 `\\`，内容匹配必须用转义形式。
            let expected_exe_json = diagnosis.exe_path.replace('\\', "\\\\");
            let hkcu = RegKey::predef(HKEY_CURRENT_USER);
            let chromium_installed = [
                env_dir_exists("LOCALAPPDATA", &["Google", "Chrome", "User Data"]),
                env_dir_exists("LOCALAPPDATA", &["Microsoft", "Edge", "User Data"]),
            ];
            for ((reg_path, label), installed) in CHROMIUM_REG_PATHS.iter().zip(chromium_installed)
            {
                let issue = diagnose_registry(
                    &hkcu,
                    reg_path,
                    MANIFEST_FILENAME_CHROMIUM,
                    &expected_exe_json,
                    true,
                );
                diagnosis.targets.push(NmhTarget {
                    label: (*label).to_owned(),
                    location: format!("HKCU\\{reg_path}\\{NMH_NAME}"),
                    installed,
                    ok: issue.is_empty(),
                    issue,
                });
            }
            let issue = diagnose_registry(
                &hkcu,
                FIREFOX_REG_PATH,
                MANIFEST_FILENAME_FIREFOX,
                &expected_exe_json,
                false,
            );
            diagnosis.targets.push(NmhTarget {
                label: "Firefox".to_owned(),
                location: format!("HKCU\\{FIREFOX_REG_PATH}\\{NMH_NAME}"),
                installed: env_dir_exists("APPDATA", &["Mozilla", "Firefox"]),
                ok: issue.is_empty(),
                issue,
            });
            diagnosis
        }

        /// 写出两份清单并注册 Chrome / Edge / Firefox 键；幂等。
        pub fn register() -> Result<(), io::Error> {
            let nmh_exe = super::find_nmh_exe()?;
            let nmh_path = strip_unc_prefix(&nmh_exe.to_string_lossy());
            let dir = nmh_exe
                .parent()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no parent dir"))?;
            let chromium_path = dir.join(MANIFEST_FILENAME_CHROMIUM);
            std::fs::write(&chromium_path, super::chromium_manifest_json(&nmh_path)?)?;
            let firefox_path = dir.join(MANIFEST_FILENAME_FIREFOX);
            std::fs::write(&firefox_path, super::firefox_manifest_json(&nmh_path)?)?;
            let chromium_str = strip_unc_prefix(&chromium_path.to_string_lossy());
            let firefox_str = strip_unc_prefix(&firefox_path.to_string_lossy());
            let hkcu = RegKey::predef(HKEY_CURRENT_USER);
            for (reg_path, _) in CHROMIUM_REG_PATHS {
                let (key, _) =
                    hkcu.create_subkey_with_flags(format!("{reg_path}\\{NMH_NAME}"), KEY_WRITE)?;
                key.set_value("", &chromium_str)?;
            }
            let (key, _) = hkcu
                .create_subkey_with_flags(format!("{FIREFOX_REG_PATH}\\{NMH_NAME}"), KEY_WRITE)?;
            key.set_value("", &firefox_str)?;
            tracing::info!(exe = %nmh_path, chromium = %chromium_str, firefox = %firefox_str, "NMH registered");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;

    use super::{
        MAX_BATCH_ITEMS, NmhService, PipeMessage, handle_stream, ping_stream, select_task_briefs,
    };

    #[tokio::test]
    async fn ipc_ping_round_trips_through_frame_protocol() {
        let daemon = Arc::new(crate::daemon_client::DaemonClient::disconnected());
        let events =
            crate::event_hub::AgentEventHub::new(fluxdown_protocol::AgentSnapshot::default());
        let capture = Arc::new(crate::capture::CaptureService::new(daemon.clone(), events));
        let service = NmhService::new(daemon, capture);
        let (client, server) = tokio::io::duplex(4096);
        let server_task = tokio::spawn(handle_stream(server, service));
        let reply = ping_stream(client).await.expect("pong");
        assert_eq!(reply, "pong");
        let _ = server_task.await;
    }

    #[test]
    fn task_panel_keeps_active_and_ten_recent_completions() {
        let tasks = (0..15)
            .map(|index| {
                serde_json::from_value(json!({
                    "taskId": format!("task-{index}"),
                    "url": "https://example.com/a",
                    "fileName": format!("{index}.bin"),
                    "saveDir": "/tmp",
                    "status": if index == 0 { 1 } else { 3 },
                    "downloadedBytes": 1,
                    "totalBytes": 1,
                    "errorMessage": "",
                    "createdAt": index.to_string(),
                    "proxyUrl": "",
                    "queueId": "main",
                    "checksum": ""
                }))
                .expect("task")
            })
            .collect();
        let selected = select_task_briefs(tasks);
        assert_eq!(selected.len(), 11);
        assert_eq!(selected[0].task_id, "task-0");
        assert_eq!(selected[1].task_id, "task-14");
    }

    #[test]
    fn flattened_native_message_preserves_download_payload() {
        let message = serde_json::from_value::<PipeMessage>(json!({
            "action": "download",
            "msg_id": 7,
            "url": "https://example.com/a"
        }))
        .expect("native message");
        assert_eq!(message.msg_id, 7);
        assert_eq!(message.payload["url"], "https://example.com/a");
        assert_eq!(MAX_BATCH_ITEMS, 1000);
    }
}
