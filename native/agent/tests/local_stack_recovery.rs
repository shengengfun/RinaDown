#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{HeaderValue, header};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

const TOKEN_WAIT: Duration = Duration::from_secs(30);
const RECOVERY_WAIT: Duration = Duration::from_secs(15);

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires FLUXDOWN_TEST_DAEMON_BIN=<absolute fluxdownd path>"]
async fn client_observes_daemon_stale_recovery_and_agent_restart_state_restoration() {
    let daemon_binary = PathBuf::from(
        std::env::var_os("FLUXDOWN_TEST_DAEMON_BIN").expect("FLUXDOWN_TEST_DAEMON_BIN is required"),
    );
    assert!(
        daemon_binary.is_absolute(),
        "daemon test binary must be absolute"
    );
    assert!(daemon_binary.is_file(), "daemon test binary does not exist");

    let root = std::env::temp_dir().join(format!(
        "fluxdown-local-stack-recovery-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).expect("create local stack data dir");
    let daemon_pid_file = root.join("daemon.pid");
    let wrapper = create_daemon_wrapper(&root, &daemon_binary, &daemon_pid_file);
    let daemon_address = reserve_address();
    let agent_address = reserve_address();
    let agent_token_file = root.join("agent").join("agent.token");
    let mut stack = StackGuard::new(daemon_pid_file.clone());

    stack.agent = Some(spawn_agent(
        &root,
        &wrapper,
        daemon_address,
        agent_address,
        &agent_token_file,
    ));
    let token = wait_for_token(&agent_token_file, TOKEN_WAIT).await;
    let mut client = connect_agent(agent_address, &token).await;
    hello(&mut client, 1).await;
    let initial = wait_snapshot(&mut client, 2, RECOVERY_WAIT, |snapshot| {
        daemon_connected(snapshot)
    })
    .await;
    let initial_daemon_pid = wait_for_daemon_pid(&daemon_pid_file, None, RECOVERY_WAIT).await;
    let initial_revision = daemon_revision(&initial);

    send_signal(initial_daemon_pid, "KILL");
    wait_snapshot(&mut client, 10, RECOVERY_WAIT, |snapshot| {
        !daemon_connected(snapshot)
    })
    .await;
    let replacement_pid =
        wait_for_daemon_pid(&daemon_pid_file, Some(initial_daemon_pid), RECOVERY_WAIT).await;
    assert_ne!(replacement_pid, initial_daemon_pid);
    let recovered = wait_snapshot(&mut client, 20, RECOVERY_WAIT, |snapshot| {
        daemon_connected(snapshot)
    })
    .await;
    assert!(daemon_revision(&recovered) >= initial_revision);

    let revision = daemon_revision(&recovered);
    let patched = rpc_call(
        &mut client,
        30,
        fluxdown_protocol::method::DAEMON_CONFIG_PATCH,
        Some(json!({
            "expectedRevision": revision,
            "values": {"max_concurrent_tasks": "9"}
        })),
    )
    .await;
    assert_eq!(patched["result"]["revision"], json!(revision + 1));
    wait_snapshot(&mut client, 31, RECOVERY_WAIT, |snapshot| {
        daemon_config_value(snapshot, "max_concurrent_tasks") == Some("9")
    })
    .await;

    drop(client);
    terminate_child(
        stack.agent.as_mut().expect("agent child"),
        Duration::from_secs(10),
    )
    .await;
    stack.agent = None;
    assert!(
        process_exists(replacement_pid),
        "daemon must outlive the UI gateway"
    );

    stack.agent = Some(spawn_agent(
        &root,
        &wrapper,
        daemon_address,
        agent_address,
        &agent_token_file,
    ));
    let mut restarted_client = connect_agent(agent_address, &token).await;
    hello(&mut restarted_client, 40).await;
    let restored = wait_snapshot(&mut restarted_client, 41, RECOVERY_WAIT, |snapshot| {
        daemon_connected(snapshot)
            && daemon_config_value(snapshot, "max_concurrent_tasks") == Some("9")
    })
    .await;
    assert!(daemon_revision(&restored) >= revision + 1);

    drop(restarted_client);
    terminate_child(
        stack.agent.as_mut().expect("restarted agent"),
        Duration::from_secs(10),
    )
    .await;
    stack.agent = None;
    send_signal(replacement_pid, "TERM");
    wait_for_process_exit(replacement_pid, Duration::from_secs(10)).await;
    stack.daemon_cleaned = true;
    std::fs::remove_dir_all(&root).expect("remove local stack data dir");
}

struct StackGuard {
    agent: Option<Child>,
    daemon_pid_file: PathBuf,
    daemon_cleaned: bool,
}

impl StackGuard {
    fn new(daemon_pid_file: PathBuf) -> Self {
        Self {
            agent: None,
            daemon_pid_file,
            daemon_cleaned: false,
        }
    }
}

impl Drop for StackGuard {
    fn drop(&mut self) {
        if let Some(agent) = self.agent.as_mut() {
            let _ = agent.kill();
            let _ = agent.wait();
        }
        if !self.daemon_cleaned
            && let Ok(text) = std::fs::read_to_string(&self.daemon_pid_file)
            && let Ok(pid) = text.trim().parse::<u32>()
        {
            let _ = Command::new("kill")
                .args(["-KILL", &pid.to_string()])
                .status();
        }
    }
}

fn create_daemon_wrapper(root: &Path, daemon: &Path, pid_file: &Path) -> PathBuf {
    let wrapper = root.join("fluxdownd-test-wrapper.sh");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$$\" > '{}'\nexec '{}' \"$@\"\n",
        pid_file.display(),
        daemon.display()
    );
    std::fs::write(&wrapper, script).expect("write daemon wrapper");
    let mut permissions = std::fs::metadata(&wrapper)
        .expect("daemon wrapper metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&wrapper, permissions).expect("make daemon wrapper executable");
    wrapper
}

fn spawn_agent(
    root: &Path,
    daemon_wrapper: &Path,
    daemon_address: std::net::SocketAddr,
    agent_address: std::net::SocketAddr,
    agent_token_file: &Path,
) -> Child {
    Command::new(env!("CARGO_BIN_EXE_fluxdown-agent"))
        .env("FLUXDOWN_DATA_DIR", root)
        .env("FLUXDOWN_AGENT_DATA_DIR", root.join("agent"))
        .env("FLUXDOWN_AGENT_TOKEN_FILE", agent_token_file)
        .env("FLUXDOWN_DAEMON_TOKEN_FILE", root.join("daemon.token"))
        .env("FLUXDOWN_DAEMON_BIN", daemon_wrapper)
        .env("FLUXDOWN_DAEMON_BIND", daemon_address.to_string())
        .env("FLUXDOWN_DAEMON_URL", format!("ws://{daemon_address}/rpc"))
        .env("FLUXDOWN_AGENT_BIND", agent_address.to_string())
        .env("FLUXCLOUD_BASE_URL", "http://127.0.0.1:1")
        .env_remove("FLUXDOWN_DATABASE_URL")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn fluxdown-agent")
}

fn reserve_address() -> std::net::SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve stack port");
    let address = listener.local_addr().expect("reserved stack address");
    drop(listener);
    address
}

async fn wait_for_token(path: &Path, timeout: Duration) -> String {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(token) = tokio::fs::read_to_string(path).await
            && !token.trim().is_empty()
        {
            return token.trim().to_owned();
        }
        assert!(Instant::now() < deadline, "agent token was not created");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn wait_for_daemon_pid(path: &Path, previous: Option<u32>, timeout: Duration) -> u32 {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(text) = tokio::fs::read_to_string(path).await
            && let Ok(pid) = text.trim().parse::<u32>()
            && previous != Some(pid)
            && process_exists(pid)
        {
            return pid;
        }
        assert!(
            Instant::now() < deadline,
            "daemon PID did not change in time"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn connect_agent(address: std::net::SocketAddr, token: &str) -> Socket {
    let deadline = Instant::now() + TOKEN_WAIT;
    loop {
        let mut request = format!("ws://{address}/rpc")
            .into_client_request()
            .expect("agent WebSocket request");
        request.headers_mut().insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}")).expect("agent authorization header"),
        );
        match tokio_tungstenite::connect_async(request).await {
            Ok((socket, _)) => return socket,
            Err(_) if Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(error) => panic!("agent gateway did not accept WebSocket: {error:#}"),
        }
    }
}

async fn hello(socket: &mut Socket, id: i64) {
    let response = rpc_call(
        socket,
        id,
        fluxdown_protocol::method::SYSTEM_HELLO,
        Some(json!({
            "clientName": "local-stack-recovery-test",
            "clientVersion": "test",
            "minProtocolVersion": fluxdown_protocol::MIN_PROTOCOL_VERSION,
            "maxProtocolVersion": fluxdown_protocol::PROTOCOL_VERSION,
            "requestedRole": "agent",
            "capabilities": [fluxdown_protocol::method::CAPABILITY_CLIENT_SELECTIONS]
        })),
    )
    .await;
    assert_eq!(response["result"]["role"], json!("agent"));
}

async fn wait_snapshot(
    socket: &mut Socket,
    mut id: i64,
    timeout: Duration,
    predicate: impl Fn(&Value) -> bool,
) -> Value {
    let deadline = Instant::now() + timeout;
    loop {
        let response = rpc_call(socket, id, fluxdown_protocol::method::SYSTEM_SNAPSHOT, None).await;
        let snapshot = &response["result"];
        if predicate(snapshot) {
            return snapshot.clone();
        }
        assert!(
            Instant::now() < deadline,
            "snapshot predicate timed out: {snapshot}"
        );
        id = id.saturating_add(1);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn rpc_call(socket: &mut Socket, id: i64, method: &str, params: Option<Value>) -> Value {
    let request = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params
    });
    socket
        .send(Message::Text(request.to_string().into()))
        .await
        .expect("send agent RPC");
    loop {
        let message = tokio::time::timeout(Duration::from_secs(10), socket.next())
            .await
            .expect("agent RPC response timeout")
            .expect("agent WebSocket closed")
            .expect("read agent WebSocket message");
        match message {
            Message::Text(text) => {
                let value: Value = serde_json::from_str(&text).expect("agent JSON frame");
                if value.get("id").and_then(Value::as_i64) == Some(id) {
                    return value;
                }
            }
            Message::Ping(payload) => socket
                .send(Message::Pong(payload))
                .await
                .expect("send agent pong"),
            Message::Close(frame) => panic!("agent WebSocket closed: {frame:?}"),
            _ => {}
        }
    }
}

fn daemon_connected(snapshot: &Value) -> bool {
    snapshot["body"]["snapshot"]["daemonConnected"]
        .as_bool()
        .unwrap_or(false)
}

fn daemon_revision(snapshot: &Value) -> u64 {
    snapshot["body"]["snapshot"]["daemon"]["config"]["revision"]
        .as_u64()
        .expect("daemon config revision")
}

fn daemon_config_value<'a>(snapshot: &'a Value, key: &str) -> Option<&'a str> {
    snapshot["body"]["snapshot"]["daemon"]["config"]["values"][key].as_str()
}

fn send_signal(pid: u32, signal: &str) {
    let status = Command::new("kill")
        .args([format!("-{signal}"), pid.to_string()])
        .status()
        .expect("send process signal");
    assert!(status.success(), "kill -{signal} {pid} failed: {status}");
}

fn process_exists(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

async fn terminate_child(child: &mut Child, timeout: Duration) {
    send_signal(child.id(), "TERM");
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().expect("wait for agent") {
            assert!(status.success(), "agent exited unsuccessfully: {status}");
            return;
        }
        assert!(Instant::now() < deadline, "agent did not stop in time");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn wait_for_process_exit(pid: u32, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while process_exists(pid) {
        assert!(
            Instant::now() < deadline,
            "process {pid} did not stop in time"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
