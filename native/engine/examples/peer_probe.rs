//! 实网 peer 下载探针：对一个 HighID 源走完整 eMule 序列，验证能否真正收到
//! 文件字节。Hello → FileRequest → FileStatusRequest → StartUpload →
//! (AcceptUpload/QueueRank) → RequestParts → SendingPart。
//!
//! run: cargo run -p fluxdown_engine --example peer_probe -- <ip> <port>

use std::time::Duration;

use fluxdown_engine::ed2k::proto::{self, PROTO_EDONKEY};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

const OP_HELLO: u8 = 0x01;
const OP_HELLOANSWER: u8 = 0x4C;
const OP_FILEREQUEST: u8 = 0x58;
const OP_FILEANSWER: u8 = 0x59;
const OP_FILEREQANSNOFIL: u8 = 0x48;
const OP_SETREQFILEID: u8 = 0x4F; // = FileStatusRequest
const OP_FILESTATUS: u8 = 0x50; // = FileStatusAnswer
const OP_STARTUPLOADREQ: u8 = 0x54;
const OP_ACCEPTUPLOADREQ: u8 = 0x55;
const OP_REQUESTPARTS: u8 = 0x47;
const OP_SENDINGPART: u8 = 0x46;
const OP_QUEUERANK: u8 = 0x5C;
const OP_OUTOFPARTREQS: u8 = 0x57;

const FILE_HASH: [u8; 16] = [
    0xE8, 0xC6, 0x36, 0xD0, 0xC0, 0x48, 0x63, 0x78, 0xBF, 0x61, 0xE6, 0xA3, 0x00, 0x0D, 0x0F, 0xB7,
];
const FILE_SIZE: u64 = 2907254;
const BLOCK_SIZE: u64 = 180 * 1024; // 184320

fn enc_u32_tag(name: u8, v: u32) -> Vec<u8> {
    let mut o = vec![0x03u8];
    o.extend_from_slice(&1u16.to_le_bytes());
    o.push(name);
    o.extend_from_slice(&v.to_le_bytes());
    o
}
fn enc_str_tag(name: u8, v: &str) -> Vec<u8> {
    let mut o = vec![0x02u8];
    o.extend_from_slice(&1u16.to_le_bytes());
    o.push(name);
    o.extend_from_slice(&(v.len() as u16).to_le_bytes());
    o.extend_from_slice(v.as_bytes());
    o
}

fn build_hello() -> Vec<u8> {
    let mut uh = [0u8; 16];
    uh[5] = 14;
    uh[14] = 111;
    let ev: u32 = (3 << 24) | (1 << 7); // aMule software id 3, minimal
    let tags = [
        enc_str_tag(0x01, "FluxDown"),
        enc_u32_tag(0x11, 0x3C),
        enc_u32_tag(0xFB, ev),
    ];
    let mut p = Vec::new();
    p.push(0x10); // hash length 16
    p.extend_from_slice(&uh);
    p.extend_from_slice(&0u32.to_le_bytes()); // client id
    p.extend_from_slice(&0u16.to_le_bytes()); // port
    p.extend_from_slice(&(tags.len() as u32).to_le_bytes());
    for t in &tags {
        p.extend_from_slice(t);
    }
    p.extend_from_slice(&0u32.to_le_bytes()); // server ip
    p.extend_from_slice(&0u16.to_le_bytes()); // server port
    p
}

fn build_request_parts() -> Vec<u8> {
    // hash(16) + 3×start(u32) + 3×end(u32); request only [0, min(BLOCK_SIZE,size)).
    let end = BLOCK_SIZE.min(FILE_SIZE) as u32;
    let mut p = Vec::new();
    p.extend_from_slice(&FILE_HASH);
    // starts
    p.extend_from_slice(&0u32.to_le_bytes());
    p.extend_from_slice(&0u32.to_le_bytes());
    p.extend_from_slice(&0u32.to_le_bytes());
    // ends
    p.extend_from_slice(&end.to_le_bytes());
    p.extend_from_slice(&0u32.to_le_bytes());
    p.extend_from_slice(&0u32.to_le_bytes());
    p
}

async fn read_frame(
    stream: &mut TcpStream,
    deadline: tokio::time::Instant,
) -> Option<(u8, u8, Vec<u8>)> {
    let now = tokio::time::Instant::now();
    if now >= deadline {
        return None;
    }
    match tokio::time::timeout(deadline - now, proto::read_frame(stream, 512 * 1024)).await {
        Ok(Ok(f)) => Some(f),
        _ => None,
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let ip = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "176.84.20.71".to_string());
    let port: u16 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(4662);
    println!("=== peer {ip}:{port} — full eMule download sequence ===");

    let mut stream = match tokio::time::timeout(
        Duration::from_secs(8),
        TcpStream::connect((ip.as_str(), port)),
    )
    .await
    {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            println!("connect failed: {e}");
            return;
        }
        Err(_) => {
            println!("connect timed out");
            return;
        }
    };
    println!("connected");

    // 1. Hello
    if stream
        .write_all(&proto::frame_with_proto(
            PROTO_EDONKEY,
            OP_HELLO,
            &build_hello(),
        ))
        .await
        .is_err()
    {
        println!("hello write failed");
        return;
    }
    println!("-> Hello");

    let mut sent_filereq = false;
    let mut sent_statusreq = false;
    let mut sent_startupload = false;
    let mut sent_reqparts = false;
    let mut bytes_received: u64 = 0;
    let overall = tokio::time::Instant::now() + Duration::from_secs(12);

    loop {
        let Some((proto_b, op, payload)) = read_frame(&mut stream, overall).await else {
            println!("(no more frames / timeout)");
            break;
        };
        match op {
            OP_HELLOANSWER => {
                println!("<- HelloAnswer ({} bytes) — peer alive", payload.len());
                // 2. FileRequest
                if !sent_filereq {
                    let _ = stream
                        .write_all(&proto::frame_with_proto(
                            PROTO_EDONKEY,
                            OP_FILEREQUEST,
                            &FILE_HASH,
                        ))
                        .await;
                    println!("-> FileRequest");
                    sent_filereq = true;
                }
            }
            OP_FILEANSWER => {
                println!("<- FileAnswer (peer HAS the file)");
                if !sent_statusreq {
                    let _ = stream
                        .write_all(&proto::frame_with_proto(
                            PROTO_EDONKEY,
                            OP_SETREQFILEID,
                            &FILE_HASH,
                        ))
                        .await;
                    println!("-> FileStatusRequest");
                    sent_statusreq = true;
                }
            }
            OP_FILEREQANSNOFIL => {
                println!("<- NoFile (peer does NOT have the file) — done");
                break;
            }
            OP_FILESTATUS => {
                println!("<- FileStatusAnswer ({} bytes)", payload.len());
                if !sent_startupload {
                    let _ = stream
                        .write_all(&proto::frame_with_proto(
                            PROTO_EDONKEY,
                            OP_STARTUPLOADREQ,
                            &FILE_HASH,
                        ))
                        .await;
                    println!("-> StartUpload");
                    sent_startupload = true;
                }
            }
            OP_ACCEPTUPLOADREQ => {
                println!("<- AcceptUpload — WE HAVE AN UPLOAD SLOT");
                if !sent_reqparts {
                    let _ = stream
                        .write_all(&proto::frame_with_proto(
                            PROTO_EDONKEY,
                            OP_REQUESTPARTS,
                            &build_request_parts(),
                        ))
                        .await;
                    println!("-> RequestParts [0, {})", BLOCK_SIZE.min(FILE_SIZE));
                    sent_reqparts = true;
                }
            }
            OP_QUEUERANK => {
                let rank = if payload.len() >= 4 {
                    u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]])
                } else {
                    0
                };
                println!("<- QueueRank {rank} (queued, waiting for slot)");
            }
            OP_SENDINGPART => {
                // payload: hash(16) + start(u32) + end(u32) + data
                if payload.len() >= 24 {
                    let start =
                        u32::from_le_bytes([payload[16], payload[17], payload[18], payload[19]]);
                    let end =
                        u32::from_le_bytes([payload[20], payload[21], payload[22], payload[23]]);
                    let data = end.saturating_sub(start) as u64;
                    bytes_received += data;
                    println!(
                        "<- *** SendingPart [{start}, {end}) = {data} bytes DATA! total={bytes_received} ***"
                    );
                    if bytes_received >= 32 * 1024 {
                        println!(
                            "\n=== SUCCESS: received {bytes_received} bytes of real file data ==="
                        );
                        return;
                    }
                }
            }
            OP_OUTOFPARTREQS => {
                println!("<- OutOfPartReqs (peer upload queue full)");
            }
            0x40 => {
                println!(
                    "<- CompressedPart ({} bytes) — real data (zlib)!",
                    payload.len()
                );
                bytes_received += payload.len() as u64;
                if bytes_received >= 16 * 1024 {
                    println!("\n=== SUCCESS: received compressed file data ===");
                    return;
                }
            }
            other => {
                println!(
                    "<- frame proto={proto_b:#x} op={other:#x} len={}",
                    payload.len()
                );
            }
        }
    }
    println!("\n=== result: {bytes_received} bytes received ===");
}
