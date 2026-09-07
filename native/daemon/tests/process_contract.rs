#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

const TOKEN: &str = "daemon-process-contract-token";
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

#[test]
fn daemon_process_enforces_wire_auth_conflict_body_limit_and_shutdown() {
    let address = reserve_loopback_address();
    let data_dir = unique_temp_dir("fluxdown-daemon-process-contract");
    std::fs::create_dir_all(&data_dir).expect("create daemon test data dir");
    std::fs::write(data_dir.join("daemon.token"), format!("{TOKEN}\n"))
        .expect("write daemon test token");

    let mut daemon = ProcessGuard::spawn(address, &data_dir);
    wait_until_listening(address, Duration::from_secs(30));

    let unauthorized = http_request(
        address,
        "GET /rpc HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: Zmx1eGRvd24tdW5hdXRob3JpemVk\r\n\r\n",
        None,
    );
    assert!(unauthorized.starts_with("HTTP/1.1 401"), "{unauthorized}");

    let mut wrong_role = open_websocket(address, TOKEN);
    let wrong_role_response = rpc_call(
        &mut wrong_role,
        1,
        fluxdown_protocol::method::SYSTEM_HELLO,
        Some(json!({
            "clientName": "process-contract-test",
            "clientVersion": "test",
            "minProtocolVersion": fluxdown_protocol::MIN_PROTOCOL_VERSION,
            "maxProtocolVersion": fluxdown_protocol::PROTOCOL_VERSION,
            "requestedRole": "agent",
            "capabilities": []
        })),
    );
    assert_eq!(
        wrong_role_response["error"]["data"]["code"],
        json!("protocolIncompatible")
    );

    let mut socket = open_websocket(address, TOKEN);
    let hello = rpc_call(
        &mut socket,
        2,
        fluxdown_protocol::method::SYSTEM_HELLO,
        Some(json!({
            "clientName": "fluxdown-agent",
            "clientVersion": "test",
            "minProtocolVersion": fluxdown_protocol::MIN_PROTOCOL_VERSION,
            "maxProtocolVersion": fluxdown_protocol::PROTOCOL_VERSION,
            "requestedRole": "daemon",
            "capabilities": []
        })),
    );
    assert_eq!(hello["result"]["role"], json!("daemon"));
    assert_eq!(
        hello["result"]["protocolVersion"],
        json!(fluxdown_protocol::PROTOCOL_VERSION)
    );

    let config = rpc_call(
        &mut socket,
        3,
        fluxdown_protocol::method::DAEMON_CONFIG_GET,
        None,
    );
    let revision = config["result"]["revision"]
        .as_u64()
        .expect("config revision");
    let first_patch = rpc_call(
        &mut socket,
        4,
        fluxdown_protocol::method::DAEMON_CONFIG_PATCH,
        Some(json!({
            "expectedRevision": revision,
            "values": {"max_concurrent_tasks": "7"}
        })),
    );
    assert_eq!(first_patch["result"]["revision"], json!(revision + 1));

    let stale_patch = rpc_call(
        &mut socket,
        5,
        fluxdown_protocol::method::DAEMON_CONFIG_PATCH,
        Some(json!({
            "expectedRevision": revision,
            "values": {"max_concurrent_tasks": "8"}
        })),
    );
    assert_eq!(stale_patch["error"]["data"]["code"], json!("conflict"));
    assert_eq!(
        stale_patch["error"]["data"]["revision"],
        json!(revision + 1)
    );

    let oversized_body = vec![0_u8; MAX_BODY_BYTES + 1];
    let oversized_headers = format!(
        "POST /blobs/torrents HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {TOKEN}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        oversized_body.len()
    );
    let oversized = http_request(address, &oversized_headers, Some(&oversized_body));
    assert!(oversized.starts_with("HTTP/1.1 413"), "{oversized}");

    daemon.terminate(Duration::from_secs(10));
    std::fs::remove_dir_all(&data_dir).expect("remove daemon test data dir");
}

struct ProcessGuard {
    child: Option<Child>,
}

impl ProcessGuard {
    fn spawn(address: SocketAddr, data_dir: &Path) -> Self {
        let child = Command::new(env!("CARGO_BIN_EXE_fluxdownd"))
            .env("FLUXDOWN_DAEMON_BIND", address.to_string())
            .env("FLUXDOWN_DATA_DIR", data_dir)
            .env_remove("FLUXDOWN_DATABASE_URL")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn fluxdownd");
        Self { child: Some(child) }
    }

    fn terminate(&mut self, timeout: Duration) {
        let child = self.child.as_mut().expect("daemon child");
        #[cfg(unix)]
        {
            let status = Command::new("kill")
                .args(["-TERM", &child.id().to_string()])
                .status()
                .expect("send SIGTERM to fluxdownd");
            assert!(status.success(), "kill -TERM failed: {status}");
        }
        #[cfg(not(unix))]
        child.kill().expect("terminate fluxdownd");

        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = child.try_wait().expect("wait for fluxdownd") {
                assert!(
                    status.success(),
                    "fluxdownd exited unsuccessfully: {status}"
                );
                self.child = None;
                return;
            }
            assert!(Instant::now() < deadline, "fluxdownd did not stop in time");
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for ProcessGuard {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn reserve_loopback_address() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("reserve daemon port");
    let address = listener.local_addr().expect("reserved address");
    drop(listener);
    address
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{}-{nonce}", std::process::id()))
}

fn wait_until_listening(address: SocketAddr, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_ok() {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("fluxdownd did not listen on {address}");
}

fn http_request(address: SocketAddr, headers: &str, body: Option<&[u8]>) -> String {
    let mut stream = connect(address);
    stream
        .write_all(headers.as_bytes())
        .expect("write HTTP headers");
    if let Some(body) = body {
        stream.write_all(body).expect("write HTTP body");
    }
    let response_headers = read_http_headers(&mut stream);
    let content_length = response_headers
        .lines()
        .find_map(|line| {
            line.split_once(':').and_then(|(name, value)| {
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
        })
        .unwrap_or(0);
    let mut body = vec![0_u8; content_length];
    stream
        .read_exact(&mut body)
        .expect("read HTTP response body");
    format!("{response_headers}{}", String::from_utf8_lossy(&body))
}

fn open_websocket(address: SocketAddr, token: &str) -> TcpStream {
    let mut stream = connect(address);
    let request = format!(
        "GET /rpc HTTP/1.1\r\nHost: {address}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: Zmx1eGRvd24tcHJvY2Vzcy10ZXN0\r\nAuthorization: Bearer {token}\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .expect("write WebSocket upgrade");
    let headers = read_http_headers(&mut stream);
    assert!(headers.starts_with("HTTP/1.1 101"), "{headers}");
    stream
}

fn connect(address: SocketAddr) -> TcpStream {
    let stream =
        TcpStream::connect_timeout(&address, Duration::from_secs(2)).expect("connect to fluxdownd");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("set read timeout");
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .expect("set write timeout");
    stream
}

fn read_http_headers(stream: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut byte = [0_u8; 1];
    while !bytes.ends_with(b"\r\n\r\n") {
        stream
            .read_exact(&mut byte)
            .expect("read HTTP upgrade response");
        bytes.push(byte[0]);
        assert!(bytes.len() < 16 * 1024, "HTTP headers too large");
    }
    String::from_utf8(bytes).expect("UTF-8 HTTP headers")
}

fn rpc_call(stream: &mut TcpStream, id: i64, method: &str, params: Option<Value>) -> Value {
    let request = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params
    });
    write_text_frame(stream, request.to_string().as_bytes());
    loop {
        let response = read_text_frame(stream);
        let value: Value = serde_json::from_slice(&response).expect("JSON WebSocket frame");
        if value.get("id").and_then(Value::as_i64) == Some(id) {
            return value;
        }
    }
}

fn write_text_frame(stream: &mut TcpStream, payload: &[u8]) {
    let mask = [0x46_u8, 0x4c, 0x55, 0x58];
    let mut frame = Vec::with_capacity(payload.len() + 14);
    frame.push(0x81);
    match payload.len() {
        0..=125 => frame.push(0x80 | payload.len() as u8),
        126..=65_535 => {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        }
        _ => {
            frame.push(0x80 | 127);
            frame.extend_from_slice(&(payload.len() as u64).to_be_bytes());
        }
    }
    frame.extend_from_slice(&mask);
    frame.extend(
        payload
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ mask[index % mask.len()]),
    );
    stream.write_all(&frame).expect("write WebSocket frame");
}

fn read_text_frame(stream: &mut TcpStream) -> Vec<u8> {
    loop {
        let mut header = [0_u8; 2];
        stream
            .read_exact(&mut header)
            .expect("read WebSocket header");
        let opcode = header[0] & 0x0f;
        let masked = header[1] & 0x80 != 0;
        let mut length = u64::from(header[1] & 0x7f);
        if length == 126 {
            let mut extended = [0_u8; 2];
            stream
                .read_exact(&mut extended)
                .expect("read WebSocket length");
            length = u64::from(u16::from_be_bytes(extended));
        } else if length == 127 {
            let mut extended = [0_u8; 8];
            stream
                .read_exact(&mut extended)
                .expect("read WebSocket length");
            length = u64::from_be_bytes(extended);
        }
        let mut mask = [0_u8; 4];
        if masked {
            stream.read_exact(&mut mask).expect("read WebSocket mask");
        }
        let mut payload = vec![0_u8; usize::try_from(length).expect("frame length")];
        stream
            .read_exact(&mut payload)
            .expect("read WebSocket payload");
        if masked {
            for (index, byte) in payload.iter_mut().enumerate() {
                *byte ^= mask[index % mask.len()];
            }
        }
        match opcode {
            0x1 => return payload,
            0x8 => panic!("WebSocket closed before RPC response"),
            0x9 => write_control_frame(stream, 0xA, &payload),
            _ => {}
        }
    }
}

fn write_control_frame(stream: &mut TcpStream, opcode: u8, payload: &[u8]) {
    assert!(payload.len() <= 125, "control frame payload too large");
    let mask = [0x44_u8, 0x41, 0x45, 0x4d];
    let mut frame = Vec::with_capacity(payload.len() + 6);
    frame.push(0x80 | opcode);
    frame.push(0x80 | payload.len() as u8);
    frame.extend_from_slice(&mask);
    frame.extend(
        payload
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ mask[index % mask.len()]),
    );
    stream
        .write_all(&frame)
        .expect("write WebSocket control frame");
}
