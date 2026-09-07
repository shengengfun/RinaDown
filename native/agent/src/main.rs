//! `fluxdown-agent` 官方客户端常驻后端。

use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let cancel = CancellationToken::new();
    let signal_cancel = cancel.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        signal_cancel.cancel();
    });
    fluxdown_agent::runtime::run(cancel).await
}

#[cfg(unix)]
async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};

    let Ok(mut terminate) = signal(SignalKind::terminate()) else {
        let _ = tokio::signal::ctrl_c().await;
        return;
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }
}

#[cfg(not(unix))]
async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
