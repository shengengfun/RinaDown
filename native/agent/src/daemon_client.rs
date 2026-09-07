//! agent 到 daemon 的单连接 JSON-RPC 客户端与重连快照恢复。

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use fluxdown_protocol::method;
use fluxdown_protocol::{
    ApplicationErrorCode, DaemonSnapshot, EventFrame, RequestId, RpcErrorData, RpcNotification,
    RpcRequest, RpcResponse, ServiceHello, ServiceRole, Snapshot, SnapshotBody,
};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::net::TcpStream;
use tokio::sync::{Notify, mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{HeaderValue, header};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::supervisor::DaemonSupervisor;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// daemon 客户端配置。
#[derive(Clone)]
pub struct DaemonClientConfig {
    pub rpc_url: String,
    pub bearer: String,
}

impl DaemonClientConfig {
    /// 拒绝非 loopback daemon URL。
    pub fn validate(&self) -> Result<(), DaemonClientError> {
        let url = reqwest::Url::parse(&self.rpc_url)
            .map_err(|error| DaemonClientError::Configuration(error.to_string()))?;
        let host = url
            .host_str()
            .ok_or_else(|| DaemonClientError::Configuration("daemon URL has no host".to_owned()))?;
        let loopback = host == "localhost"
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback());
        if loopback {
            Ok(())
        } else {
            Err(DaemonClientError::Configuration(
                "daemon URL must be loopback".to_owned(),
            ))
        }
    }
}

/// daemon 连接产生的有序状态流。
pub enum DaemonClientEvent {
    Snapshot(DaemonSnapshot),
    Event(EventFrame),
    Stale,
    Fatal(RpcErrorData),
}

struct ClientCommand {
    method: String,
    params: Option<Value>,
    ack: oneshot::Sender<Result<Value, RpcErrorData>>,
}

/// 可克隆的 daemon 调用入口。
#[derive(Clone)]
pub struct DaemonClient {
    commands: mpsc::Sender<ClientCommand>,
    connected: Arc<AtomicBool>,
    ready: Arc<Notify>,
}

impl DaemonClient {
    /// 启动重连任务与有界事件流。
    pub fn start(
        config: DaemonClientConfig,
        supervisor: Arc<DaemonSupervisor>,
    ) -> Result<(Self, mpsc::Receiver<DaemonClientEvent>), DaemonClientError> {
        config.validate()?;
        let (commands, command_rx) = mpsc::channel(64);
        let (events, event_rx) = mpsc::channel(1024);
        let connected = Arc::new(AtomicBool::new(false));
        let ready = Arc::new(Notify::new());
        tokio::spawn(run_client(
            config,
            supervisor,
            command_rx,
            events,
            connected.clone(),
            ready.clone(),
        ));
        Ok((
            Self {
                commands,
                connected,
                ready,
            },
            event_rx,
        ))
    }

    /// 提交类型化 RPC 调用。
    pub async fn call<P: Serialize, R: DeserializeOwned>(
        &self,
        method: &str,
        params: Option<P>,
    ) -> Result<R, RpcErrorData> {
        if !self.connected.load(Ordering::Acquire) {
            return Err(unavailable_error());
        }
        let params = match params {
            Some(params) => Some(serde_json::to_value(params).map_err(|_| internal_error())?),
            None => None,
        };
        let (ack, response) = oneshot::channel();
        self.commands
            .send(ClientCommand {
                method: method.to_owned(),
                params,
                ack,
            })
            .await
            .map_err(|_| unavailable_error())?;
        let value = response.await.map_err(|_| unavailable_error())??;
        serde_json::from_value(value).map_err(|_| internal_error())
    }

    pub async fn wait_ready(&self, timeout: Duration) -> Result<(), RpcErrorData> {
        if self.connected.load(Ordering::Acquire) {
            return Ok(());
        }
        tokio::time::timeout(timeout, async {
            loop {
                self.ready.notified().await;
                if self.connected.load(Ordering::Acquire) {
                    return;
                }
            }
        })
        .await
        .map_err(|_| unavailable_error())
    }
}

#[cfg(test)]
impl DaemonClient {
    pub(crate) fn disconnected() -> Self {
        let (commands, receiver) = mpsc::channel(1);
        drop(receiver);
        Self {
            commands,
            connected: Arc::new(AtomicBool::new(false)),
            ready: Arc::new(Notify::new()),
        }
    }
}

async fn run_client(
    config: DaemonClientConfig,
    supervisor: Arc<DaemonSupervisor>,
    mut commands: mpsc::Receiver<ClientCommand>,
    events: mpsc::Sender<DaemonClientEvent>,
    connected: Arc<AtomicBool>,
    ready: Arc<Notify>,
) {
    let backoff = [1_u64, 2, 5, 15, 30];
    let mut attempt = 0_usize;
    loop {
        match connect(&config).await {
            Ok((socket, snapshot)) => {
                attempt = 0;
                connected.store(true, Ordering::Release);
                ready.notify_waiters();
                if events
                    .send(DaemonClientEvent::Snapshot(snapshot))
                    .await
                    .is_err()
                {
                    connected.store(false, Ordering::Release);
                    return;
                }
                if run_connected(socket, &mut commands, &events).await.is_err() {
                    let _ = events.send(DaemonClientEvent::Stale).await;
                }
                connected.store(false, Ordering::Release);
                fail_queued_commands(&mut commands);
            }
            Err(ConnectError::Refused) => {
                if let Err(error) = supervisor.ensure_running().await {
                    tracing::warn!(error = %error, "could not supervise fluxdownd");
                }
            }
            Err(ConnectError::Fatal(error)) => {
                let _ = events.send(DaemonClientEvent::Fatal(error)).await;
                connected.store(false, Ordering::Release);
                return;
            }
            Err(ConnectError::Transient(error)) => {
                tracing::warn!(%error, "daemon connection failed");
            }
        }
        connected.store(false, Ordering::Release);
        let delay = backoff[attempt.min(backoff.len() - 1)];
        attempt = attempt.saturating_add(1);
        tokio::time::sleep(Duration::from_secs(delay)).await;
    }
}

fn fail_queued_commands(commands: &mut mpsc::Receiver<ClientCommand>) {
    while let Ok(command) = commands.try_recv() {
        let _ = command.ack.send(Err(unavailable_error()));
    }
}

async fn connect(config: &DaemonClientConfig) -> Result<(Socket, DaemonSnapshot), ConnectError> {
    let mut request = config
        .rpc_url
        .clone()
        .into_client_request()
        .map_err(|error| ConnectError::Fatal(invalid_argument(error.to_string())))?;
    let authorization = HeaderValue::from_str(&format!("Bearer {}", config.bearer))
        .map_err(|error| ConnectError::Fatal(invalid_argument(error.to_string())))?;
    request
        .headers_mut()
        .insert(header::AUTHORIZATION, authorization);
    let (mut socket, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(classify_connect_error)?;
    let hello = serde_json::json!({
        "clientName": "fluxdown-agent",
        "clientVersion": env!("CARGO_PKG_VERSION"),
        "minProtocolVersion": fluxdown_protocol::MIN_PROTOCOL_VERSION,
        "maxProtocolVersion": fluxdown_protocol::PROTOCOL_VERSION,
        "requestedRole": "daemon",
        "capabilities": []
    });
    let hello_value = call_on_socket(&mut socket, 1, method::SYSTEM_HELLO, Some(hello)).await?;
    let service = serde_json::from_value::<ServiceHello>(hello_value)
        .map_err(|_| ConnectError::Fatal(protocol_error()))?;
    if service.role != ServiceRole::Daemon
        || service.protocol_version != fluxdown_protocol::PROTOCOL_VERSION
    {
        return Err(ConnectError::Fatal(protocol_error()));
    }
    let snapshot_value = call_on_socket(&mut socket, 2, method::SYSTEM_SNAPSHOT, None).await?;
    let snapshot = serde_json::from_value::<Snapshot>(snapshot_value)
        .map_err(|_| ConnectError::Fatal(protocol_error()))?;
    let SnapshotBody::Daemon(snapshot) = snapshot.body else {
        return Err(ConnectError::Fatal(protocol_error()));
    };
    Ok((socket, *snapshot))
}

async fn run_connected(
    mut socket: Socket,
    commands: &mut mpsc::Receiver<ClientCommand>,
    events: &mpsc::Sender<DaemonClientEvent>,
) -> Result<(), ()> {
    let mut next_id = 10_i64;
    let mut pending = HashMap::<i64, oneshot::Sender<Result<Value, RpcErrorData>>>::new();
    loop {
        tokio::select! {
            command = commands.recv() => {
                let Some(command) = command else { return Ok(()); };
                let id = next_id;
                next_id = next_id.saturating_add(1);
                let request = RpcRequest::new(RequestId::Integer(id), command.method, command.params);
                let text = serde_json::to_string(&request).map_err(|_| ())?;
                pending.insert(id, command.ack);
                if socket.send(Message::Text(text.into())).await.is_err() { break; }
            }
            incoming = socket.next() => {
                let Some(Ok(Message::Text(text))) = incoming else { break; };
                if let Ok(notification) = serde_json::from_str::<RpcNotification>(&text)
                    && notification.method == method::SERVICE_EVENT
                {
                    if let Some(params) = notification.params
                        && let Ok(frame) = serde_json::from_value::<EventFrame>(params)
                        && events.send(DaemonClientEvent::Event(frame)).await.is_err()
                    {
                        return Ok(());
                    }
                    continue;
                }
                let Ok(response) = serde_json::from_str::<RpcResponse>(&text) else { break; };
                match response {
                    RpcResponse::Success(success) => {
                        if let RequestId::Integer(id) = success.id
                            && let Some(ack) = pending.remove(&id)
                        {
                            let _ = ack.send(Ok(success.result));
                        }
                    }
                    RpcResponse::Failure(failure) => {
                        if let Some(RequestId::Integer(id)) = failure.id
                            && let Some(ack) = pending.remove(&id)
                        {
                            let error = failure.error.data.unwrap_or_else(internal_error);
                            let _ = ack.send(Err(error));
                        }
                    }
                }
            }
        }
    }
    for (_, ack) in pending {
        let _ = ack.send(Err(unavailable_error()));
    }
    Err(())
}

async fn call_on_socket(
    socket: &mut Socket,
    id: i64,
    method_name: &str,
    params: Option<Value>,
) -> Result<Value, ConnectError> {
    let request = RpcRequest::new(RequestId::Integer(id), method_name, params);
    let text = serde_json::to_string(&request)
        .map_err(|error| ConnectError::Transient(error.to_string()))?;
    socket
        .send(Message::Text(text.into()))
        .await
        .map_err(|error| ConnectError::Transient(error.to_string()))?;
    while let Some(message) = socket.next().await {
        let message = message.map_err(|error| ConnectError::Transient(error.to_string()))?;
        let Message::Text(text) = message else {
            continue;
        };
        let response = serde_json::from_str::<RpcResponse>(&text)
            .map_err(|error| ConnectError::Transient(error.to_string()))?;
        match response {
            RpcResponse::Success(success) if success.id == RequestId::Integer(id) => {
                return Ok(success.result);
            }
            RpcResponse::Failure(failure) if failure.id == Some(RequestId::Integer(id)) => {
                return Err(ConnectError::Fatal(
                    failure.error.data.unwrap_or_else(internal_error),
                ));
            }
            _ => {}
        }
    }
    Err(ConnectError::Transient("daemon socket closed".to_owned()))
}

fn classify_connect_error(error: tokio_tungstenite::tungstenite::Error) -> ConnectError {
    match &error {
        tokio_tungstenite::tungstenite::Error::Io(io)
            if matches!(
                io.kind(),
                std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
            ) =>
        {
            ConnectError::Refused
        }
        tokio_tungstenite::tungstenite::Error::Http(response) if response.status() == 401 => {
            ConnectError::Fatal(RpcErrorData::new(ApplicationErrorCode::Unauthorized, false))
        }
        _ => ConnectError::Transient(format!("{error:#}")),
    }
}

enum ConnectError {
    Refused,
    Transient(String),
    Fatal(RpcErrorData),
}

/// daemon 客户端启动错误。
#[derive(Debug, thiserror::Error)]
pub enum DaemonClientError {
    #[error("daemon client configuration is invalid: {0}")]
    Configuration(String),
}

fn unavailable_error() -> RpcErrorData {
    RpcErrorData::new(ApplicationErrorCode::Unavailable, true)
}

fn internal_error() -> RpcErrorData {
    RpcErrorData::new(ApplicationErrorCode::Internal, false)
}

fn protocol_error() -> RpcErrorData {
    RpcErrorData::new(ApplicationErrorCode::ProtocolIncompatible, false)
}

fn invalid_argument(_message: String) -> RpcErrorData {
    RpcErrorData::new(ApplicationErrorCode::InvalidArgument, false)
}

#[cfg(test)]
mod tests {
    use fluxdown_protocol::ApplicationErrorCode;

    use super::DaemonClient;

    #[tokio::test]
    async fn disconnected_client_rejects_without_queueing_for_replay() {
        let client = DaemonClient::disconnected();
        let result = client
            .call::<serde_json::Value, serde_json::Value>(
                fluxdown_protocol::method::DAEMON_TASK_CREATE,
                Some(serde_json::json!({})),
            )
            .await;
        let error = result.expect_err("disconnected command must fail");
        assert_eq!(error.code, ApplicationErrorCode::Unavailable);
        assert!(error.retryable);
    }
}
