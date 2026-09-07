mod actors;
mod api_host;
mod clipboard_file;
mod compat_flags;
mod diagnostics;
mod file_association;
mod logger;
#[cfg(target_os = "macos")]
mod macos_cf;
mod native_messaging;
mod nmh_registry;
mod protocol_registry;
mod reveal_file;
pub mod rinf_selection;
mod rinf_sink;
mod shortcut_icon;
mod signal_bridge;
mod signals;
#[cfg(target_os = "windows")]
mod system_stats;
mod updater;

use actors::create_actors;
use rinf::{dart_shutdown, write_interface};

write_interface!();

// RUNTIME CONSTRAINT: This binary uses a single-threaded (`current_thread`) Tokio runtime.
// All tasks share the same OS thread, so blocking operations (blocking I/O, `std::thread::sleep`,
// `Mutex::lock` held across `.await`, etc.) will stall every other task on the runtime.
//
// Rules for contributors:
//   • Never call blocking APIs directly in `async fn` — wrap them in `tokio::task::spawn_blocking`.
//   • Never use `mpsc::Sender::blocking_send` inside a `tokio::spawn(async { … })` block;
//     use `.send(…).await` instead. `blocking_send` is only safe inside `spawn_blocking` closures.
//   • Never park the thread with `std::thread::sleep` or synchronous `Mutex` contention in async code.
#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = logger::init() {
        // 同进程二次 isolate：logger 已由上次 runtime 装好，继续拉 actor。
        if !error.is_already_initialized() {
            eprintln!(
                "FluxDown logger initialization failed: {}",
                logger::format_error_chain(&error)
            );
            return;
        }
    }
    logger::spawn_logged("hub", "create actors", async {
        create_actors().await?;
        Ok::<(), actors::CreateActorsError>(())
    });
    logger::spawn_logged("hub", "shortcut icon listener", async {
        shortcut_icon::listen().await;
        Ok::<(), std::convert::Infallible>(())
    });
    #[cfg(target_os = "windows")]
    {
        logger::spawn_logged("hub", "system stats sampler", async {
            system_stats::spawn();
            Ok::<(), std::convert::Infallible>(())
        });
    }
    dart_shutdown().await;
}
