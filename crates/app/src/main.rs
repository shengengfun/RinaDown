//! `fluxdown-desktop` 的薄入口；应用装配集中在本 crate。

mod account_port;
mod agent_client;
mod app;
mod assets;
mod capability_ports;
mod downloads_port;
mod launch;
mod service_bootstrap;
mod settings_port;

use std::process::ExitCode;

fn main() -> ExitCode {
    match app::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("failed to start FluxDown desktop client: {error:#}");
            ExitCode::FAILURE
        }
    }
}
