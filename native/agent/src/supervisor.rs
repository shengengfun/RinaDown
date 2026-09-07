//! agent 对同级 `fluxdownd` 的单飞启动与异步回收。

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use tokio::sync::Mutex;

/// daemon 启动错误。
#[derive(Debug, thiserror::Error)]
pub enum SupervisorError {
    #[error("could not locate sibling fluxdownd: {0}")]
    Locate(#[from] std::io::Error),
    #[error("failed to spawn fluxdownd: {0}")]
    Spawn(String),
}

#[derive(Default)]
struct SupervisorState {
    generation: u64,
    running: bool,
    reapers: Vec<tokio::task::JoinHandle<()>>,
}

/// 只在连接拒绝路径调用的 daemon 单飞启动器。
pub struct DaemonSupervisor {
    state: Arc<Mutex<SupervisorState>>,
}

impl Default for DaemonSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl DaemonSupervisor {
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(SupervisorState::default())),
        }
    }

    /// 启动同级 daemon；短时间内并发/重复调用只产生一个子进程。
    pub async fn ensure_running(&self) -> Result<(), SupervisorError> {
        let mut state = self.state.lock().await;
        state.reapers.retain(|task| !task.is_finished());
        if state.running {
            return Ok(());
        }
        let executable = daemon_executable()?;
        let mut command = std::process::Command::new(&executable);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        set_no_console_window(&mut command);
        let mut child = tokio::process::Command::from(command)
            .spawn()
            .map_err(|error| SupervisorError::Spawn(format!("{error:#}")))?;
        state.generation = state.generation.saturating_add(1);
        let generation = state.generation;
        state.running = true;
        let supervisor_state = self.state.clone();
        state.reapers.push(tokio::spawn(async move {
            match child.wait().await {
                Ok(status) => tracing::info!(%status, "supervised fluxdownd exited"),
                Err(error) => tracing::warn!(error = %error, "failed to reap fluxdownd"),
            }
            let mut state = supervisor_state.lock().await;
            if state.generation == generation {
                state.running = false;
            }
        }));
        Ok(())
    }
}

fn daemon_executable() -> Result<PathBuf, std::io::Error> {
    if let Some(path) = std::env::var_os("FLUXDOWN_DAEMON_BIN") {
        return Ok(PathBuf::from(path));
    }
    let current = std::env::current_exe()?;
    let name = if cfg!(windows) {
        "fluxdownd.exe"
    } else {
        "fluxdownd"
    };
    Ok(current.with_file_name(name))
}

#[cfg(windows)]
fn set_no_console_window(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x0800_0000);
}

#[cfg(not(windows))]
fn set_no_console_window(_command: &mut std::process::Command) {}
