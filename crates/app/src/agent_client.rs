//! GPUI composition root 唯一 agent JSON-RPC 会话。

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use fluxdown_protocol::method;
use fluxdown_protocol::{
    AgentSnapshot, ApplicationErrorCode, EventFrame, RequestId, RpcErrorData, RpcNotification,
    RpcRequest, RpcResponse, ServiceHello, ServiceRole, Snapshot, SnapshotBody,
};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{HeaderValue, header};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::service_bootstrap::ServiceBootstrap;

pub type AgentFuture<T> = Pin<Box<dyn Future<Output = Result<T, RpcErrorData>> + Send + 'static>>;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub enum AgentClientEvent {
    Snapshot(Box<AgentSnapshot>),
    Event(Box<EventFrame>),
    Stale,
    Fatal(RpcErrorData),
}

#[derive(Clone)]
pub struct AgentClientConfig {
    pub rpc_url: String,
    pub bearer_path: PathBuf,
}

struct ClientCommand {
    method: String,
    params: Option<Value>,
    ack: oneshot::Sender<Result<Value, RpcErrorData>>,
}

pub struct AgentClient {
    commands: mpsc::Sender<ClientCommand>,
    _runtime: Arc<tokio::runtime::Runtime>,
}

impl AgentClient {
    pub fn start(
        config: AgentClientConfig,
        bootstrap: Arc<ServiceBootstrap>,
    ) -> Result<(Arc<Self>, mpsc::Receiver<AgentClientEvent>), AgentClientError> {
        validate_url(&config.rpc_url)?;
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_name("fluxdown-agent-client")
                .build()
                .map_err(AgentClientError::Runtime)?,
        );
        let (commands, command_rx) = mpsc::channel(64);
        let (events, event_rx) = mpsc::channel(1024);
        runtime.spawn(run_client(config, bootstrap, command_rx, events));
        Ok((
            Arc::new(Self {
                commands,
                _runtime: runtime,
            }),
            event_rx,
        ))
    }

    pub fn call<P, R>(&self, method_name: &str, params: Option<P>) -> AgentFuture<R>
    where
        P: Serialize + Send + 'static,
        R: DeserializeOwned + Send + 'static,
    {
        let commands = self.commands.clone();
        let method_name = method_name.to_owned();
        Box::pin(async move {
            let params = params
                .map(serde_json::to_value)
                .transpose()
                .map_err(|_| internal_error())?;
            let (ack, response) = oneshot::channel();
            commands
                .send(ClientCommand {
                    method: method_name,
                    params,
                    ack,
                })
                .await
                .map_err(|_| unavailable_error())?;
            let value = response.await.map_err(|_| unavailable_error())??;
            serde_json::from_value(value).map_err(|_| internal_error())
        })
    }
}

async fn run_client(
    config: AgentClientConfig,
    bootstrap: Arc<ServiceBootstrap>,
    mut commands: mpsc::Receiver<ClientCommand>,
    events: mpsc::Sender<AgentClientEvent>,
) {
    let backoff = [1_u64, 2, 5, 15, 30];
    let mut attempt = 0_usize;
    loop {
        match connect(&config).await {
            Ok((socket, snapshot)) => {
                attempt = 0;
                if events
                    .try_send(AgentClientEvent::Snapshot(Box::new(snapshot)))
                    .is_err()
                {
                    return;
                }
                if run_connected(socket, &mut commands, &events).await.is_err() {
                    let _ = events.try_send(AgentClientEvent::Stale);
                }
            }
            Err(ConnectError::Refused) => {
                if bootstrap.ensure_running().await.is_err() {
                    let _ = events.try_send(AgentClientEvent::Stale);
                }
            }
            Err(ConnectError::Fatal(error)) => {
                let _ = events.try_send(AgentClientEvent::Fatal(error));
                return;
            }
            Err(ConnectError::Transient) => {
                let _ = events.try_send(AgentClientEvent::Stale);
            }
        }
        let delay = backoff[attempt.min(backoff.len() - 1)];
        attempt = attempt.saturating_add(1);
        tokio::time::sleep(Duration::from_secs(delay)).await;
    }
}

async fn connect(config: &AgentClientConfig) -> Result<(Socket, AgentSnapshot), ConnectError> {
    let bearer = tokio::fs::read_to_string(&config.bearer_path)
        .await
        .map_err(|_| ConnectError::Refused)?;
    let bearer = bearer.trim();
    if bearer.is_empty() {
        return Err(ConnectError::Refused);
    }
    let mut request = config
        .rpc_url
        .clone()
        .into_client_request()
        .map_err(|_| ConnectError::Fatal(protocol_error()))?;
    let authorization = HeaderValue::from_str(&format!("Bearer {bearer}"))
        .map_err(|_| ConnectError::Fatal(protocol_error()))?;
    request
        .headers_mut()
        .insert(header::AUTHORIZATION, authorization);
    let (mut socket, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(classify_connect_error)?;
    let hello = serde_json::json!({
        "clientName": "fluxdown-desktop",
        "clientVersion": env!("CARGO_PKG_VERSION"),
        "minProtocolVersion": fluxdown_protocol::MIN_PROTOCOL_VERSION,
        "maxProtocolVersion": fluxdown_protocol::PROTOCOL_VERSION,
        "requestedRole": "agent",
        "capabilities": [method::CAPABILITY_CLIENT_SELECTIONS]
    });
    let hello_value = call_on_socket(&mut socket, 1, method::SYSTEM_HELLO, Some(hello)).await?;
    let service = serde_json::from_value::<ServiceHello>(hello_value)
        .map_err(|_| ConnectError::Fatal(protocol_error()))?;
    if service.role != ServiceRole::Agent
        || service.protocol_version != fluxdown_protocol::PROTOCOL_VERSION
    {
        return Err(ConnectError::Fatal(protocol_error()));
    }
    let snapshot_value = call_on_socket(&mut socket, 2, method::SYSTEM_SNAPSHOT, None).await?;
    let snapshot = serde_json::from_value::<Snapshot>(snapshot_value)
        .map_err(|_| ConnectError::Fatal(protocol_error()))?;
    let SnapshotBody::Agent(snapshot) = snapshot.body else {
        return Err(ConnectError::Fatal(protocol_error()));
    };
    Ok((socket, *snapshot))
}

async fn run_connected(
    mut socket: Socket,
    commands: &mut mpsc::Receiver<ClientCommand>,
    events: &mpsc::Sender<AgentClientEvent>,
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
                        && events.try_send(AgentClientEvent::Event(Box::new(frame))).is_err()
                    {
                        break;
                    }
                    continue;
                }
                let response = serde_json::from_str::<RpcResponse>(&text).map_err(|_| ())?;
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
                            let _ = ack.send(Err(failure.error.data.unwrap_or_else(internal_error)));
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
    let text = serde_json::to_string(&request).map_err(|_| ConnectError::Transient)?;
    socket
        .send(Message::Text(text.into()))
        .await
        .map_err(|_| ConnectError::Transient)?;
    while let Some(message) = socket.next().await {
        let message = message.map_err(|_| ConnectError::Transient)?;
        let Message::Text(text) = message else {
            continue;
        };
        let response =
            serde_json::from_str::<RpcResponse>(&text).map_err(|_| ConnectError::Transient)?;
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
    Err(ConnectError::Transient)
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
        _ => ConnectError::Transient,
    }
}

enum ConnectError {
    Refused,
    Transient,
    Fatal(RpcErrorData),
}

fn validate_url(url: &str) -> Result<(), AgentClientError> {
    let url = reqwest_url(url)?;
    let host = url
        .host()
        .ok_or_else(|| AgentClientError::Configuration("agent URL has no host".to_owned()))?;
    let loopback = host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    if loopback {
        Ok(())
    } else {
        Err(AgentClientError::Configuration(
            "agent URL must be loopback".to_owned(),
        ))
    }
}

fn reqwest_url(url: &str) -> Result<tokio_tungstenite::tungstenite::http::Uri, AgentClientError> {
    url.parse::<tokio_tungstenite::tungstenite::http::Uri>()
        .map_err(|error| AgentClientError::Configuration(error.to_string()))
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

#[derive(Debug, thiserror::Error)]
pub enum AgentClientError {
    #[error("agent client configuration is invalid: {0}")]
    Configuration(String),
    #[error("could not create agent client runtime: {0}")]
    Runtime(std::io::Error),
}
