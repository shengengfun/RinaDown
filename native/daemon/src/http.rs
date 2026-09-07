//! loopback-only daemon HTTP 与鉴权 WebSocket 传输。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::ws::{CloseFrame, Message, WebSocket};
use axum::extract::{Path as AxumPath, State, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use fluxdown_protocol::{EventFrame, RpcNotification};
use futures_util::StreamExt;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio_util::io::ReaderStream;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::blob_store::BlobKind;
use crate::rpc::RpcSession;
use crate::service::DaemonService;

const REQUEST_BODY_LIMIT: usize = 4 * 1024 * 1024;

#[derive(Clone)]
struct HttpState {
    service: Arc<DaemonService>,
    bearer: Arc<str>,
    cancel: CancellationToken,
}

/// 启动 daemon HTTP 服务直到取消或 listener 失败。
pub async fn serve(
    listener: TcpListener,
    service: Arc<DaemonService>,
    bearer: String,
    cancel: CancellationToken,
) -> Result<(), std::io::Error> {
    let state = HttpState {
        service,
        bearer: Arc::from(bearer),
        cancel: cancel.clone(),
    };
    let app = Router::new()
        .route("/rpc", get(rpc_upgrade))
        .route("/files/tasks/{task_id}", get(download_task_file))
        .route("/blobs/torrents", post(upload_torrent))
        .route("/blobs/plugins", post(upload_plugin))
        .route("/exports/{export_id}", get(download_export))
        .layer(axum::extract::DefaultBodyLimit::max(REQUEST_BODY_LIMIT))
        .with_state(state);
    axum::serve(listener, app)
        .with_graceful_shutdown(cancel.cancelled_owned())
        .await
}

/// 加载或创建 daemon bearer token 文件。
pub async fn load_or_create_bearer(
    data_dir: &Path,
    override_path: Option<&Path>,
) -> Result<String, std::io::Error> {
    let path = override_path
        .map(Path::to_path_buf)
        .unwrap_or_else(|| data_dir.join("daemon.token"));
    if let Ok(existing) = tokio::fs::read_to_string(&path).await {
        let token = existing.trim();
        if !token.is_empty() {
            return Ok(token.to_owned());
        }
    }
    let parent = path.parent().unwrap_or(data_dir);
    tokio::fs::create_dir_all(parent).await?;
    set_private_dir_permissions(parent).await?;
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let temp = temporary_path(&path);
    let mut options = tokio::fs::OpenOptions::new();
    options.create_new(true).write(true);
    let mut file = options.open(&temp).await?;
    use tokio::io::AsyncWriteExt;
    file.write_all(token.as_bytes()).await?;
    file.write_all(b"\n").await?;
    file.sync_all().await?;
    drop(file);
    set_private_file_permissions(&temp).await?;
    tokio::fs::rename(&temp, &path).await?;
    Ok(token)
}

async fn rpc_upgrade(
    State(state): State<HttpState>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    if !authorized(&headers, &state.bearer) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let service = state.service;
    let cancel = state.cancel;
    upgrade
        .on_upgrade(move |socket| run_socket(socket, service, cancel))
        .into_response()
}

async fn upload_torrent(
    State(state): State<HttpState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    upload_blob(&state, &headers, BlobKind::Torrent, &body).await
}

async fn upload_plugin(
    State(state): State<HttpState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    upload_blob(&state, &headers, BlobKind::Plugin, &body).await
}

async fn upload_blob(
    state: &HttpState,
    headers: &HeaderMap,
    kind: BlobKind,
    body: &[u8],
) -> Response {
    if !authorized(headers, &state.bearer) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match state.service.blobs().put(kind, body).await {
        Ok(blob_id) => axum::Json(serde_json::json!({ "blobId": blob_id })).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(serde_json::json!({ "error": format!("{error:#}") })),
        )
            .into_response(),
    }
}

async fn download_task_file(
    State(state): State<HttpState>,
    headers: HeaderMap,
    AxumPath(task_id): AxumPath<String>,
) -> Response {
    if !authorized(&headers, &state.bearer) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Some(task) = state.service.task(&task_id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if task.status != 3 {
        return StatusCode::CONFLICT.into_response();
    }
    let path = PathBuf::from(&task.save_dir).join(&task.file_name);
    let file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return StatusCode::NOT_FOUND.into_response();
        }
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let length = match file.metadata().await {
        Ok(metadata) => metadata.len(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let safe_name = task.file_name.replace(['\r', '\n', '"'], "_");
    match Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, length)
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{safe_name}\""),
        )
        .body(Body::from_stream(ReaderStream::new(file)))
    {
        Ok(response) => response,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn download_export(
    State(state): State<HttpState>,
    headers: HeaderMap,
    AxumPath(export_id): AxumPath<String>,
) -> Response {
    if !authorized(&headers, &state.bearer) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let bytes = match state.service.blobs().read(&export_id, BlobKind::Logs).await {
        Ok(bytes) => bytes,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };
    if state
        .service
        .blobs()
        .consume(&export_id, BlobKind::Logs)
        .await
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    (StatusCode::OK, bytes).into_response()
}

async fn run_socket(mut socket: WebSocket, service: Arc<DaemonService>, cancel: CancellationToken) {
    let mut session = RpcSession::new(service.clone());
    let mut events = None;
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                let _ = socket.send(Message::Close(Some(CloseFrame {
                    code: 1001,
                    reason: "daemon-shutdown".into(),
                }))).await;
                break;
            }
            incoming = socket.next() => {
                let Some(Ok(message)) = incoming else { break; };
                match message {
                    Message::Text(text) => {
                        let reply = session.handle_text(&text).await;
                        if reply.became_ready {
                            let (receiver, _) = service.events().subscribe_and_snapshot();
                            events = Some(receiver);
                        }
                        let Ok(json) = serde_json::to_string(&reply.response) else { break; };
                        if socket.send(Message::Text(json.into())).await.is_err() { break; }
                    }
                    Message::Close(_) => break,
                    Message::Ping(payload) => {
                        if socket.send(Message::Pong(payload)).await.is_err() { break; }
                    }
                    Message::Pong(_) => {}
                    Message::Binary(_) => {
                        let _ = socket.send(Message::Close(Some(CloseFrame {
                            code: 1003,
                            reason: "text frames required".into(),
                        }))).await;
                        break;
                    }
                }
            }
            event = receive_event(&mut events), if events.is_some() => {
                match event {
                    Ok(frame) => {
                        if send_event(&mut socket, frame).await.is_err() { break; }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        let _ = socket.send(Message::Close(Some(CloseFrame {
                            code: 4009,
                            reason: "event-gap".into(),
                        }))).await;
                        break;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
    session.disconnect();
}

async fn receive_event(
    receiver: &mut Option<broadcast::Receiver<EventFrame>>,
) -> Result<EventFrame, broadcast::error::RecvError> {
    match receiver {
        Some(receiver) => receiver.recv().await,
        None => std::future::pending().await,
    }
}

async fn send_event(socket: &mut WebSocket, frame: EventFrame) -> Result<(), ()> {
    let params = serde_json::to_value(frame).map_err(|_| ())?;
    let notification = RpcNotification::new(fluxdown_protocol::method::SERVICE_EVENT, Some(params));
    let json = serde_json::to_string(&notification).map_err(|_| ())?;
    socket
        .send(Message::Text(json.into()))
        .await
        .map_err(|_| ())
}

fn authorized(headers: &HeaderMap, expected: &str) -> bool {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return false;
    };
    let Ok(value) = value.to_str() else {
        return false;
    };
    value
        .strip_prefix("Bearer ")
        .is_some_and(|token| constant_time_eq(token.as_bytes(), expected.as_bytes()))
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |diff, (a, b)| diff | (a ^ b))
        == 0
}

fn temporary_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("daemon.token");
    path.with_file_name(format!(".{name}.{}.tmp", Uuid::new_v4()))
}

#[cfg(unix)]
async fn set_private_dir_permissions(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).await
}

#[cfg(not(unix))]
async fn set_private_dir_permissions(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}

#[cfg(unix)]
async fn set_private_file_permissions(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).await
}

#[cfg(not(unix))]
async fn set_private_file_permissions(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue, header};

    use super::authorized;

    #[test]
    fn bearer_auth_requires_exact_header_token() {
        let mut headers = HeaderMap::new();
        assert!(!authorized(&headers, "secret"));
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer wrong"),
        );
        assert!(!authorized(&headers, "secret"));
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer secret"),
        );
        assert!(authorized(&headers, "secret"));
    }
}
