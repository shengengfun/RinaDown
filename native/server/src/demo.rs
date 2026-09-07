//! 演示模式内置下载源 —— `GET /demo/file`。
//!
//! 服务器自己充当演示文件的 HTTP 源：内容为确定性生成的字节流（不落盘、
//! 不出外网），支持 HEAD 与单段 `Range`，因此引擎的元数据探测、多线程
//! 分段、断点续传全部走**真实** HTTP 下载路径；按连接限速让进度条以
//! 可观察的速度推进。仅在演示模式（`FLUXDOWN_DEMO` / `FLUXDOWN_DEMO_URL`）
//! 下挂载，见 `main.rs`。

use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::Response;
use axum::routing::get;
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;

/// 内置演示文件路径（`config::builtin_demo_url` 拼默认演示 URL 时共用）。
pub const DEMO_FILE_PATH: &str = "/demo/file";
/// 虚拟文件大小：64 MiB（`segment_advisor` 对 10–100MB 分 8 段并行）。
const DEMO_FILE_SIZE: u64 = 64 * 1024 * 1024;
/// 单连接限速：1 MiB/s。8 段并行 ≈ 8 MiB/s，全程约 8 秒，进度可观察。
const PER_CONN_BYTES_PER_SEC: u64 = 1024 * 1024;
/// 流式写入块大小。
const CHUNK: usize = 256 * 1024;

/// 演示文件路由（无鉴权：内容为生成的字节流，不含任何数据）。
pub fn demo_router() -> Router {
    Router::new().route(DEMO_FILE_PATH, get(demo_file))
}

/// 演示文件 handler：200 全量 / 206 单段 Range，HEAD 只回头不发体。
async fn demo_file(method: Method, headers: HeaderMap) -> Response {
    let range = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| parse_range(v, DEMO_FILE_SIZE));

    let (status, start, end) = match range {
        Some((start, end)) => (StatusCode::PARTIAL_CONTENT, start, end),
        None => (StatusCode::OK, 0, DEMO_FILE_SIZE - 1),
    };
    let len = end - start + 1;

    let mut builder = Response::builder()
        .status(status)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, len)
        .header(
            header::CONTENT_DISPOSITION,
            "attachment; filename=\"FluxDown-Demo.bin\"",
        );
    if status == StatusCode::PARTIAL_CONTENT {
        builder = builder.header(
            header::CONTENT_RANGE,
            format!("bytes {start}-{end}/{DEMO_FILE_SIZE}"),
        );
    }

    let body = if method == Method::HEAD {
        Body::empty()
    } else {
        throttled_body(start, end)
    };
    builder.body(body).unwrap_or_else(|_| {
        // header 全为常量/合法值，此分支不可达；兜底返回 500。
        let mut resp = Response::new(Body::empty());
        *resp.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
        resp
    })
}

/// 生成 `[start, end]` 区间的限速字节流（deterministic pattern）。
fn throttled_body(start: u64, end: u64) -> Body {
    let (mut writer, reader) = tokio::io::duplex(CHUNK);
    tokio::spawn(async move {
        let interval = Duration::from_millis(CHUNK as u64 * 1000 / PER_CONN_BYTES_PER_SEC);
        let mut pos = start;
        while pos <= end {
            #[allow(clippy::cast_possible_truncation)] // min(CHUNK) 保证在 usize 内
            let n = ((end - pos + 1).min(CHUNK as u64)) as usize;
            if writer.write_all(&pattern_chunk(pos, n)).await.is_err() {
                return; // 客户端断开（暂停/取消），停止生成。
            }
            pos += n as u64;
            tokio::time::sleep(interval).await;
        }
    });
    Body::from_stream(ReaderStream::new(reader))
}

/// 按绝对偏移生成确定性内容：`byte[i] = i % 251`。同一偏移永远同一字节，
/// 分段并行/断点续传拼出的文件内容一致。
fn pattern_chunk(offset: u64, len: usize) -> Vec<u8> {
    #[allow(clippy::cast_possible_truncation)] // % 251 后必在 u8 内
    (offset..offset + len as u64)
        .map(|i| (i % 251) as u8)
        .collect()
}

/// 解析单段 `Range: bytes=a-b` / `bytes=a-` / `bytes=-n`（引擎只发单段）。
/// 返回闭区间 `(start, end)`；语法错误或越界返回 `None`（按全量响应处理）。
fn parse_range(value: &str, size: u64) -> Option<(u64, u64)> {
    let spec = value.strip_prefix("bytes=")?;
    let (a, b) = spec.split_once('-')?;
    let (start, end) = match (a.is_empty(), b.is_empty()) {
        (false, false) => (a.parse().ok()?, b.parse::<u64>().ok()?.min(size - 1)),
        (false, true) => (a.parse().ok()?, size - 1),
        (true, false) => {
            let n: u64 = b.parse().ok()?;
            if n == 0 {
                return None;
            }
            (size.saturating_sub(n), size - 1)
        }
        (true, true) => return None,
    };
    (start <= end && start < size).then_some((start, end))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::{parse_range, pattern_chunk};

    #[test]
    fn parse_range_supports_engine_probe_and_segment_forms() {
        // meta_prober 的探测请求：Range: bytes=0-0。
        assert_eq!(parse_range("bytes=0-0", 100), Some((0, 0)));
        // 分段请求：闭区间。
        assert_eq!(parse_range("bytes=10-19", 100), Some((10, 19)));
        // 开区间到末尾 / 超出末尾的 end 被钳到 size-1。
        assert_eq!(parse_range("bytes=90-", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=90-200", 100), Some((90, 99)));
        // 后缀区间。
        assert_eq!(parse_range("bytes=-10", 100), Some((90, 99)));
    }

    #[test]
    fn parse_range_rejects_malformed_or_out_of_bounds() {
        assert_eq!(parse_range("bytes=-", 100), None);
        assert_eq!(parse_range("bytes=abc-1", 100), None);
        assert_eq!(parse_range("bytes=100-", 100), None); // start 越界
        assert_eq!(parse_range("bytes=20-10", 100), None); // 倒置
        assert_eq!(parse_range("items=0-1", 100), None); // 非 bytes 单位
    }

    #[test]
    fn pattern_chunk_is_deterministic_by_absolute_offset() {
        // 同一偏移的字节与拆分方式无关——分段并行拼出的内容必须一致。
        let whole = pattern_chunk(0, 600);
        let mut pieces = pattern_chunk(0, 251);
        pieces.extend(pattern_chunk(251, 349));
        assert_eq!(whole, pieces);
        assert_eq!(whole[0], 0);
        assert_eq!(whole[250], 250);
        assert_eq!(whole[251], 0); // 周期 251
    }
}
