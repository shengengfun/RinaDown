//! GPUI 对同级 `fluxdown-agent` 的单飞启动与异步回收。

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use tokio::sync::Mutex;

#[derive(Default)]
struct BootstrapState {
    generation: u64,
    running: bool,
    reapers: Vec<tokio::task::JoinHandle<()>>,
}

pub struct ServiceBootstrap {
    state: Arc<Mutex<BootstrapState>>,
}

impl ServiceBootstrap {
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(BootstrapState::default())),
        }
    }

    /// 仅由 connection-refused/no-listener 路径调用。
    pub async fn ensure_running(&self) -> Result<(), BootstrapError> {
        let mut state = self.state.lock().await;
        state.reapers.retain(|task| !task.is_finished());
        if state.running {
            return Ok(());
        }
        let executable = agent_executable()?;
        let mut command = std::process::Command::new(executable);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        set_no_console_window(&mut command);
        let mut child = tokio::process::Command::from(command)
            .spawn()
            .map_err(|error| BootstrapError::Spawn(format!("{error:#}")))?;
        state.generation = state.generation.saturating_add(1);
        let generation = state.generation;
        state.running = true;
        let bootstrap_state = self.state.clone();
        state.reapers.push(tokio::spawn(async move {
            let _ = child.wait().await;
            let mut state = bootstrap_state.lock().await;
            if state.generation == generation {
                state.running = false;
            }
        }));
        Ok(())
    }
}

fn agent_executable() -> Result<PathBuf, std::io::Error> {
    if let Some(path) = std::env::var_os("FLUXDOWN_AGENT_BIN") {
        return Ok(PathBuf::from(path));
    }
    let current = std::env::current_exe()?;
    Ok(current.with_file_name(if cfg!(windows) {
        "fluxdown-agent.exe"
    } else {
        "fluxdown-agent"
    }))
}

#[cfg(windows)]
fn set_no_console_window(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x0800_0000);
}

#[cfg(not(windows))]
fn set_no_console_window(_command: &mut std::process::Command) {}

#[derive(Debug, thiserror::Error)]
pub enum BootstrapError {
    #[error("could not locate fluxdown-agent: {0}")]
    Locate(#[from] std::io::Error),
    #[error("could not spawn fluxdown-agent: {0}")]
    Spawn(String),
}
