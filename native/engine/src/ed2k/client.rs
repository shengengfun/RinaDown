//! eD2K 客户端核心 —— 进程级共享会话：持久服务器连接、HighID 监听、
//! LowID callback 中转、源发现聚合。
//!
//! 设计动机：HighID 监听 socket、LowID callback 的入站匹配、以及（后续）
//! Kad UDP 都是**进程级单例**资源，不能每个下载任务各建一份。本模块把这些
//! 资源收敛到一个 [`Ed2kClient`]，由 [`shared_client`] 惰性初始化、全局复用。
//!
//! ## 与旧 `find_sources` 的关系
//!
//! 旧路径 [`crate::ed2k::server::find_sources`] 是**一次性**的（连服务器→查一次
//! →断开），且**丢弃所有 LowID 源**。本模块的 [`Ed2kClient::find_sources`]：
//! - 维持**持久**服务器会话（登录一次，后续复用，掉线重连）；
//! - 用**真实监听端口**登录 → 争取 HighID（可被动接收入站连接）；
//! - **保留 LowID 源** —— 通过 [`Ed2kClient::connect_source`] 的 callback 中转
//!   建立连接，不再丢弃（这是「源稀少」的直接修复）。
//!
//! ## 源的两类
//!
//! [`Source::HighId`]：可直连（`TcpStream::connect`）。
//! [`Source::LowId`]：NAT 后 peer，需经服务器 `OP_CALLBACKREQUEST` 请求其回连；
//! 回连的入站流在监听器里按 `client_id` 匹配后交回等待方。

use std::collections::{HashMap, HashSet};
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex as AsyncMutex, oneshot};
use tokio::task::JoinSet;

use crate::downloader::DownloadError;
use crate::ed2k::proto::{
    self, LOWID_THRESHOLD, MAX_SERVER_FRAME, OP_CALLBACKREQUEST, OP_GETSOURCES, OP_HELLO,
    OP_HELLOANSWER, OP_LOGINREQUEST,
};
use crate::ed2k::server::{
    PeerAddr, build_getsources_payload, build_login_payload, id_to_ipv4, read_until_found_sources,
    read_until_id_change,
};
use crate::logger::{log_error, log_info};

/// 服务器会话保活间隔（无查询时定期发一次 GETSOURCES 心跳靠调用驱动，
/// 这里仅用于重连节流）。
const SERVER_CONNECT_TIMEOUT: Duration = Duration::from_secs(8);

/// 入站 callback 等待超时。
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(30);

/// 单个 GETSOURCES 查询超时。
const QUERY_TIMEOUT: Duration = Duration::from_secs(10);

/// 对 HighID 源直连的超时（住宅源大量下线是常态，必须快速轮转）。
const PEER_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// 并发找源时最多同时查询的服务器数（不同服务器索引不同文件，
/// 单服务器常常没有目标文件 → 必须多服务器并发聚合，见 goed2k
/// "多个 ED2K server 并发找源"）。
const MAX_QUERY_SERVERS: usize = 12;

/// 一个已发现的源。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    /// 可直连的 HighID peer。
    HighId(PeerAddr),
    /// NAT 后 LowID peer：`client_id` 用于向服务器请求 callback 中转。
    LowId(u32),
}

/// 客户端运行期配置（来自 DB config，由 hub 注入）。
#[derive(Debug, Clone, Default)]
pub struct ClientConfig {
    /// TCP 监听端口（0 = 让 OS 选，登录时报实际绑定端口争取 HighID）。
    pub listen_port: u16,
    /// UDP 端口（Kad 用，本模块占位；0 = 让 OS 选）。
    pub udp_port: u16,
    /// 服务器地址列表（`host:port`，手填+订阅合并后）。
    pub servers: Vec<String>,
    /// 是否启用 UPnP 端口映射（争取 NAT 后 HighID）。
    pub enable_upnp: bool,
    /// 是否启用 Kad DHT 找源。
    pub enable_kad: bool,
}

/// 待处理的 LowID callback：`client_id → 送回入站流的 oneshot`。
type PendingCallbacks = Arc<StdMutex<HashMap<u32, oneshot::Sender<TcpStream>>>>;

/// 进程级共享 eD2K 客户端。
pub struct Ed2kClient {
    config: StdMutex<ClientConfig>,
    /// 服务器分配的本机 client_id（0 = 未登录/未知；`< LOWID_THRESHOLD` = LowID）。
    client_id: AtomicU32,
    /// 本机实际监听端口（HighID 登录与 callback 中转都用它）。
    listen_port: AtomicU32,
    /// 持久服务器连接（写端；读循环独占，故用 async mutex 串行化发送）。
    server_tx: AsyncMutex<Option<TcpStream>>,
    /// 待匹配的入站 callback。
    pending: PendingCallbacks,
    /// 已连通的服务器地址（重连/日志用）。
    connected_server: StdMutex<Option<String>>,
    /// UPnP 映射句柄（drop 时移除映射）；仅 enable_upnp 时存在。
    upnp: StdMutex<Option<crate::ed2k::upnp::UpnpMapping>>,
}

static SHARED: OnceLock<Arc<Ed2kClient>> = OnceLock::new();

/// 获取进程级共享客户端（惰性初始化，首次调用建监听器）。
pub fn shared_client() -> Arc<Ed2kClient> {
    SHARED.get_or_init(|| Arc::new(Ed2kClient::new())).clone()
}

impl Ed2kClient {
    fn new() -> Self {
        Self {
            config: StdMutex::new(ClientConfig::default()),
            client_id: AtomicU32::new(0),
            listen_port: AtomicU32::new(0),
            server_tx: AsyncMutex::new(None),
            pending: Arc::new(StdMutex::new(HashMap::new())),
            connected_server: StdMutex::new(None),
            upnp: StdMutex::new(None),
        }
    }

    /// 注入/更新配置（hub 在启动与 config 变化时调用）。
    pub fn configure(&self, config: ClientConfig) {
        if let Ok(mut g) = self.config.lock() {
            *g = config;
        }
    }

    /// 本机是否已取得 HighID（可被动接收入站连接）。
    #[must_use]
    pub fn is_high_id(&self) -> bool {
        self.client_id.load(Ordering::Relaxed) >= LOWID_THRESHOLD
    }

    /// 确保监听器已启动（幂等）。返回实际绑定端口。
    ///
    /// # Errors
    ///
    /// 绑定失败返回 [`DownloadError::Io`]。
    pub async fn ensure_listener(self: &Arc<Self>) -> Result<u16, DownloadError> {
        let existing = self.listen_port.load(Ordering::Relaxed);
        if existing != 0 {
            return Ok(existing as u16);
        }
        let want = self.config.lock().ok().map(|c| c.listen_port).unwrap_or(0);
        let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, want))
            .await
            .map_err(DownloadError::Io)?;
        let port = listener.local_addr().map_err(DownloadError::Io)?.port();
        self.listen_port.store(u32::from(port), Ordering::Relaxed);
        let this = Arc::clone(self);
        tokio::spawn(async move {
            this.run_listener(listener).await;
        });
        log_info!("[ed2k-client] listener bound on port {}", port);

        // UPnP：若启用，映射监听端口争取 HighID（best-effort，失败回退 LowID）。
        let (enable_upnp, udp_port) = self
            .config
            .lock()
            .ok()
            .map(|c| (c.enable_upnp, c.udp_port))
            .unwrap_or((false, 0));
        if enable_upnp {
            let this = Arc::clone(self);
            tokio::spawn(async move {
                if let Some(mapping) = crate::ed2k::upnp::spawn_upnp(port, udp_port).await {
                    log_info!(
                        "[ed2k-client] UPnP mapping active (external ip: {:?})",
                        mapping.external_ip()
                    );
                    if let Ok(mut g) = this.upnp.lock() {
                        *g = Some(mapping);
                    }
                }
            });
        }
        Ok(port)
    }

    /// 入站连接接受循环：读对端 HELLO 取其 client_id，匹配待处理 callback。
    async fn run_listener(self: Arc<Self>, listener: TcpListener) {
        loop {
            let (stream, addr) = match listener.accept().await {
                Ok(v) => v,
                Err(e) => {
                    log_error!("[ed2k-client] listener accept error: {}", e);
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    continue;
                }
            };
            let pending = Arc::clone(&self.pending);
            tokio::spawn(async move {
                if let Err(e) = handle_inbound(stream, addr.ip().to_string(), &pending).await {
                    log_info!("[ed2k-client] inbound {} dropped: {}", addr, e);
                }
            });
        }
    }

    /// 向服务器发起 LowID callback 请求，等待该 peer 回连。
    ///
    /// 注册 `client_id → oneshot`，发 `OP_CALLBACKREQUEST`，然后在监听器收到
    /// 匹配 client_id 的入站 HELLO 时通过 oneshot 拿到已连接的流。
    ///
    /// # Errors
    ///
    /// 无服务器会话 / 超时 / socket 失败 → [`DownloadError`]。
    async fn request_callback(&self, low_id: u32) -> Result<TcpStream, DownloadError> {
        let (tx, rx) = oneshot::channel();
        {
            let mut g = self
                .pending
                .lock()
                .map_err(|_| DownloadError::Ed2k("pending lock poisoned".into()))?;
            g.insert(low_id, tx);
        }
        // 发 callback 请求到服务器。
        {
            let mut guard = self.server_tx.lock().await;
            let stream = guard
                .as_mut()
                .ok_or_else(|| DownloadError::Ed2k("no server session for callback".into()))?;
            let payload = low_id.to_le_bytes();
            let frame = proto::frame(OP_CALLBACKREQUEST, &payload);
            stream.write_all(&frame).await.map_err(DownloadError::Io)?;
        }
        match tokio::time::timeout(CALLBACK_TIMEOUT, rx).await {
            Ok(Ok(stream)) => Ok(stream),
            Ok(Err(_)) => Err(DownloadError::Ed2k("callback sender dropped".into())),
            Err(_) => {
                // 清理超时的待处理项。
                if let Ok(mut g) = self.pending.lock() {
                    g.remove(&low_id);
                }
                Err(DownloadError::Ed2k("callback timed out".into()))
            }
        }
    }

    /// 建立到某个源的连接：HighID 直连（受 [`PEER_CONNECT_TIMEOUT`] 约束，
    /// 避免死源吃满 OS 默认 TCP 超时 ~21s 拖慢调度轮转）；LowID 经服务器
    /// callback 中转。
    ///
    /// # Errors
    ///
    /// 连接失败/超时 / callback 超时 → [`DownloadError`]。
    pub async fn connect_source(&self, source: Source) -> Result<TcpStream, DownloadError> {
        match source {
            Source::HighId(peer) => {
                match tokio::time::timeout(
                    PEER_CONNECT_TIMEOUT,
                    TcpStream::connect((peer.ip, peer.port)),
                )
                .await
                {
                    Ok(r) => r.map_err(DownloadError::Io),
                    Err(_) => Err(DownloadError::Ed2k(format!("connect {peer} timed out"))),
                }
            }
            Source::LowId(low_id) => self.request_callback(low_id).await,
        }
    }

    /// 确保持久服务器会话已建立（登录取得 client_id）。幂等。
    ///
    /// 用真实监听端口登录以争取 HighID。首个成功登录的服务器即固定为会话。
    ///
    /// # Errors
    ///
    /// 全部服务器登录失败 → [`DownloadError::Ed2k`]。
    pub async fn ensure_server_session(self: &Arc<Self>) -> Result<(), DownloadError> {
        {
            let guard = self.server_tx.lock().await;
            if guard.is_some() {
                return Ok(());
            }
        }
        let listen_port = self.ensure_listener().await?;
        let servers = self
            .config
            .lock()
            .ok()
            .map(|c| c.servers.clone())
            .unwrap_or_default();
        if servers.is_empty() {
            return Err(DownloadError::Ed2k("no ed2k servers configured".into()));
        }

        for server in &servers {
            let Some((host, port)) = parse_hostport(server) else {
                continue;
            };
            match tokio::time::timeout(
                SERVER_CONNECT_TIMEOUT,
                login_server(&host, port, listen_port),
            )
            .await
            {
                Ok(Ok((stream, client_id))) => {
                    self.client_id.store(client_id, Ordering::Relaxed);
                    if let Ok(mut g) = self.connected_server.lock() {
                        *g = Some(server.clone());
                    }
                    log_info!(
                        "[ed2k-client] server session up: {} (client_id={:#x}, {})",
                        server,
                        client_id,
                        if client_id >= LOWID_THRESHOLD {
                            "HighID"
                        } else {
                            "LowID"
                        }
                    );
                    *self.server_tx.lock().await = Some(stream);
                    self.spawn_server_reader();
                    return Ok(());
                }
                Ok(Err(e)) => log_info!("[ed2k-client] login {} failed: {}", server, e),
                Err(_) => log_info!("[ed2k-client] login {} timed out", server),
            }
        }
        Err(DownloadError::Ed2k("all ed2k server logins failed".into()))
    }

    /// 启动服务器读循环：处理 IDCHANGE / CALLBACKREQUESTED / 推送源。
    ///
    /// 读循环需要独占读半边，但我们的 TcpStream 存在 `server_tx` 里供发送。
    /// 为避免读写争用，读循环通过 `try_clone` 无法用于 tokio TcpStream，故
    /// 这里改为：读循环持有 stream 的引用式访问由后续 Kad/session 重构接管。
    /// 当前实现下 `find_sources` 是「发查询→读应答」的请求-响应式串行，
    /// 读循环仅在空闲期处理服务器主动推送（CALLBACKREQUESTED）。
    fn spawn_server_reader(self: &Arc<Self>) {
        // 请求-响应式会话下，入站 callback 由监听器（run_listener）处理，
        // 不需要独立的服务器读循环；服务器主动推送的 CALLBACKREQUESTED 仅在
        // 我方 GETSOURCES 读应答窗口内被 read_until_* 跳过。占位以便后续
        // 升级为全双工会话时接管。
    }

    /// 通过持久会话查询某文件的源，保留 HighID 与 LowID 两类。
    ///
    /// # Errors
    ///
    /// 无会话 / 查询超时 / socket 失败 → [`DownloadError`]。
    pub async fn find_sources(
        self: &Arc<Self>,
        file_hash: &[u8; 16],
        total_bytes: u64,
        large_file: bool,
    ) -> Result<Vec<Source>, DownloadError> {
        // 保持持久会话（供 LowID callback 中转）+ 监听器就绪，best-effort。
        let listen_port = self.ensure_listener().await.unwrap_or(0);
        let _ = self.ensure_server_session().await;

        let servers = self
            .config
            .lock()
            .ok()
            .map(|c| c.servers.clone())
            .unwrap_or_default();
        if servers.is_empty() {
            return Err(DownloadError::Ed2k("no ed2k servers configured".into()));
        }

        // 并发对多台服务器发 GETSOURCES 并聚合：不同服务器索引不同文件，
        // 单服务器常常没有目标文件（实测 45.82.80.155 无此文件而 77.42.68.79 有）。
        let mut join: JoinSet<Vec<(u32, u16)>> = JoinSet::new();
        for server in servers.into_iter().take(MAX_QUERY_SERVERS) {
            let Some((host, port)) = parse_hostport(&server) else {
                continue;
            };
            let hash = *file_hash;
            join.spawn(async move {
                query_one_server(&host, port, listen_port, &hash, total_bytes, large_file)
                    .await
                    .unwrap_or_default()
            });
        }

        let my_id = self.client_id.load(Ordering::Relaxed);
        let am_high = self.is_high_id();
        let mut seen: HashSet<Source> = HashSet::new();
        let mut out = Vec::new();
        while let Some(res) = join.join_next().await {
            let Ok(raw) = res else { continue };
            for (id, port) in raw {
                if id >= LOWID_THRESHOLD {
                    if port == 0 || id == my_id {
                        continue;
                    }
                    let src = Source::HighId(PeerAddr {
                        ip: id_to_ipv4(id),
                        port,
                    });
                    if seen.insert(src) {
                        out.push(src);
                    }
                } else if am_high {
                    // LowID 源仅在我方 HighID 时可用（经持久会话 callback 中转）。
                    let src = Source::LowId(id);
                    if seen.insert(src) {
                        out.push(src);
                    }
                }
            }
        }
        log_info!(
            "[ed2k-client] find_sources: {} sources aggregated across servers",
            out.len()
        );
        Ok(out)
    }
}

/// 处理入站连接：读首帧 HELLO，取对端声明的 client_id，匹配待处理 callback。
async fn handle_inbound(
    mut stream: TcpStream,
    peer_ip: String,
    pending: &PendingCallbacks,
) -> Result<(), DownloadError> {
    // 读一帧，期望 HELLO（含对端 client_id）。
    let (proto_byte, opcode, payload) = tokio::time::timeout(
        Duration::from_secs(10),
        proto::read_frame(&mut stream, MAX_SERVER_FRAME),
    )
    .await
    .map_err(|_| DownloadError::Ed2k("inbound hello timeout".into()))??;

    // 回 HELLOANSWER（对端据此确认我方存活）。
    let answer = build_hello_answer();
    stream
        .write_all(&proto::frame(OP_HELLOANSWER, &answer))
        .await
        .map_err(DownloadError::Io)?;

    let client_id = parse_hello_client_id(proto_byte, opcode, &payload);
    if let Some(id) = client_id {
        let waiter = pending.lock().ok().and_then(|mut g| g.remove(&id));
        if let Some(tx) = waiter {
            let _ = tx.send(stream);
            return Ok(());
        }
    }
    // 无匹配 callback（leech-only：我们不做上传服务）——礼貌关闭。
    log_info!("[ed2k-client] inbound from {} unmatched, closing", peer_ip);
    Ok(())
}

/// 从入站首帧提取对端 client_id（HELLO payload：`hashsize(1)+user_hash(16)+client_id(4)+...`）。
fn parse_hello_client_id(proto_byte: u8, opcode: u8, payload: &[u8]) -> Option<u32> {
    if proto_byte != proto::PROTO_EDONKEY || opcode != OP_HELLO {
        return None;
    }
    // payload[0] = hash size (0x10); user_hash = [1..17); client_id = [17..21).
    let b = payload.get(17..21)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// 构造 HELLOANSWER payload（user_hash + 本机 id/port/tag 计数=0）。
fn build_hello_answer() -> Vec<u8> {
    let mut p = Vec::with_capacity(26);
    p.extend_from_slice(&[0u8; 16]); // user hash
    p.extend_from_slice(&0u32.to_le_bytes()); // client id
    p.extend_from_slice(&0u16.to_le_bytes()); // port
    p.extend_from_slice(&0u32.to_le_bytes()); // tag count
    p
}

/// 连接并登录一个服务器，返回 `(已登录流, 分配的 client_id)`。
async fn login_server(
    host: &str,
    port: u16,
    listen_port: u16,
) -> Result<(TcpStream, u32), DownloadError> {
    let mut stream = TcpStream::connect((host, port))
        .await
        .map_err(DownloadError::Io)?;
    let login = proto::frame(OP_LOGINREQUEST, &build_login_payload(listen_port));
    stream.write_all(&login).await.map_err(DownloadError::Io)?;
    let client_id = read_until_id_change(&mut stream).await?;
    Ok((stream, client_id))
}

/// 连接+登录一台服务器并对 `file_hash` 发一次 GETSOURCES，返回原始
/// `(client_id, port)` 源列表。全程受超时约束；任何失败返回 `Err`
/// （调用方 `unwrap_or_default` 容错，不影响其它服务器）。
async fn query_one_server(
    host: &str,
    port: u16,
    listen_port: u16,
    file_hash: &[u8; 16],
    total_bytes: u64,
    large_file: bool,
) -> Result<Vec<(u32, u16)>, DownloadError> {
    let (mut stream, _client_id) = tokio::time::timeout(
        SERVER_CONNECT_TIMEOUT,
        login_server(host, port, listen_port),
    )
    .await
    .map_err(|_| DownloadError::Ed2k("login timed out".into()))??;
    let gs = proto::frame(
        OP_GETSOURCES,
        &build_getsources_payload(file_hash, total_bytes, large_file),
    );
    stream.write_all(&gs).await.map_err(DownloadError::Io)?;
    match tokio::time::timeout(
        QUERY_TIMEOUT,
        read_until_found_sources(&mut stream, file_hash),
    )
    .await
    {
        Ok(Ok(sources)) => Ok(sources),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(DownloadError::Ed2k("getsources timed out".into())),
    }
}

/// 解析 `host:port`。
fn parse_hostport(s: &str) -> Option<(String, u16)> {
    let (host, port) = s.trim().rsplit_once(':')?;
    if host.is_empty() {
        return None;
    }
    let port: u16 = port.parse().ok()?;
    if port == 0 {
        return None;
    }
    Some((host.to_string(), port))
}
