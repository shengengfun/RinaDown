//! agent 私有状态的独占锁、权限与原子持久化。

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use fluxdown_protocol::{
    AgentPreferencesDto, AgentSessionDto, GatewayStatusDto, RemoteTaskDto, SyncStatusDto,
};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

/// FluxCloud 令牌只存在 agent 私有状态与云传输层。
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudCredentials {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at_unix: i64,
    pub session: Option<AgentSessionDto>,
}

/// 单个配置同步键的私有持久化状态。
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedSyncEntry {
    pub value: serde_json::Value,
    pub version: u64,
    pub dirty: bool,
    pub deleted: bool,
}

/// agent 可恢复状态；不包含 daemon 下载快照或捕获 header/cookie。
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct AgentState {
    pub device_id: String,
    pub device_name: String,
    pub platform: String,
    pub credentials: Option<CloudCredentials>,
    pub sync: SyncStatusDto,
    pub preferences: AgentPreferencesDto,
    pub sync_entries: std::collections::BTreeMap<String, PersistedSyncEntry>,
    pub sync_pulled: bool,
    pub gateway: GatewayStatusDto,
    pub gateway_user_token: String,
    pub link_identity: Option<serde_json::Value>,
    pub linked_devices: Vec<serde_json::Value>,
    pub remote_tasks: Vec<RemoteTaskDto>,
    pub link_migration_revision: Option<u64>,
    pub gateway_migration_revision: Option<u64>,
    pub analytics_install_reported: bool,
    pub analytics_last_active_day: u64,
}

/// 私有状态存储错误。
#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("agent state is already locked")]
    Locked,
    #[error("agent state I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("agent state JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("agent state ACL setup failed: {0}")]
    Acl(String),
}

/// 持有 agent 独占锁的状态存储。
pub struct StateStore {
    data_dir: PathBuf,
    state_path: PathBuf,
    _lock: File,
}

impl StateStore {
    /// 打开状态目录并获取 `<data-dir>/agent.lock`。
    pub async fn open(data_dir: PathBuf) -> Result<Self, StateError> {
        tokio::fs::create_dir_all(&data_dir).await?;
        set_private_dir_permissions(&data_dir).await?;
        let lock_path = data_dir.join("agent.lock");
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)?;
        lock.try_lock_exclusive().map_err(|error| {
            if error.kind() == std::io::ErrorKind::WouldBlock {
                StateError::Locked
            } else {
                StateError::Io(error)
            }
        })?;
        Ok(Self {
            state_path: data_dir.join("agent-state.json"),
            data_dir,
            _lock: lock,
        })
    }

    /// 读取状态；文件不存在时返回默认状态。
    pub async fn load(&self) -> Result<AgentState, StateError> {
        match tokio::fs::read(&self.state_path).await {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(AgentState::default()),
            Err(error) => Err(StateError::Io(error)),
        }
    }

    /// temp-write + fsync + atomic rename 持久化完整状态。
    pub async fn save(&self, state: &AgentState) -> Result<(), StateError> {
        let bytes = serde_json::to_vec(state)?;
        let temp = self
            .data_dir
            .join(format!(".agent-state.{}.tmp", Uuid::new_v4()));
        let mut file = tokio::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .await?;
        set_private_file_permissions(&temp).await?;
        apply_windows_acl(&temp).await?;
        file.write_all(&bytes).await?;
        file.sync_all().await?;
        drop(file);
        tokio::fs::rename(&temp, &self.state_path).await?;
        sync_parent(&self.data_dir).await?;
        Ok(())
    }

    #[must_use]
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }
}

#[cfg(unix)]
pub(crate) async fn set_private_dir_permissions(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).await
}

#[cfg(not(unix))]
pub(crate) async fn set_private_dir_permissions(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}

#[cfg(unix)]
pub(crate) async fn set_private_file_permissions(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).await
}

#[cfg(not(unix))]
pub(crate) async fn set_private_file_permissions(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}

#[cfg(unix)]
async fn sync_parent(path: &Path) -> Result<(), std::io::Error> {
    File::open(path)?.sync_all()
}

#[cfg(not(unix))]
async fn sync_parent(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}

#[cfg(windows)]
pub(crate) async fn apply_windows_acl(path: &Path) -> Result<(), StateError> {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = tokio::task::spawn_blocking(|| {
        std::process::Command::new("whoami")
            .args(["/user", "/fo", "csv", "/nh"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
    })
    .await
    .map_err(|error| StateError::Acl(error.to_string()))??;
    if !output.status.success() {
        return Err(StateError::Acl("whoami /user failed".to_owned()));
    }
    let text =
        String::from_utf8(output.stdout).map_err(|error| StateError::Acl(error.to_string()))?;
    let sid = text
        .split(',')
        .nth(1)
        .map(|value| value.trim().trim_matches('"'))
        .filter(|value| value.starts_with("S-1-"))
        .ok_or_else(|| StateError::Acl("could not parse current SID".to_owned()))?
        .to_owned();
    let path = path.to_owned();
    let status = tokio::task::spawn_blocking(move || {
        std::process::Command::new("icacls")
            .arg(path)
            .args(["/inheritance:r", "/grant:r", &format!("*{sid}:(F)")])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
    })
    .await
    .map_err(|error| StateError::Acl(error.to_string()))??;
    if status.success() {
        Ok(())
    } else {
        Err(StateError::Acl("icacls failed".to_owned()))
    }
}

#[cfg(not(windows))]
pub(crate) async fn apply_windows_acl(_path: &Path) -> Result<(), StateError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{AgentState, StateError, StateStore};

    #[tokio::test]
    async fn state_is_atomic_private_and_exclusively_locked() {
        let dir = std::env::temp_dir().join(format!(
            "fluxdown_agent_state_{}_{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let store = StateStore::open(dir.clone()).await.expect("open store");
        assert!(matches!(
            StateStore::open(dir.clone()).await,
            Err(StateError::Locked)
        ));
        let state = AgentState {
            device_id: "device-1".to_owned(),
            device_name: "Desktop".to_owned(),
            platform: "linux".to_owned(),
            ..AgentState::default()
        };
        store.save(&state).await.expect("save");
        let loaded = store.load().await.expect("load");
        assert_eq!(loaded.device_id, "device-1");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join("agent-state.json"))
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
        drop(store);
        let reopened = StateStore::open(dir.clone()).await.expect("reopen");
        drop(reopened);
        let _ = std::fs::remove_dir_all(dir);
    }
}
