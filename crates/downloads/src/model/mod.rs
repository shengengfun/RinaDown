pub(crate) mod new_download;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DownloadStatusFilter {
    All,
    Completed,
    Incomplete,
}

impl DownloadStatusFilter {
    pub(crate) fn exclusive_toggle(current: Option<Self>, clicked: Self) -> Option<Self> {
        if current == Some(clicked) {
            None
        } else {
            Some(clicked)
        }
    }

    fn slot(self) -> usize {
        match self {
            Self::All => 0,
            Self::Completed => 1,
            Self::Incomplete => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct StatusFolderMotion {
    from: [f32; 3],
    to: [f32; 3],
}

impl StatusFolderMotion {
    pub(crate) fn settled(open: Option<DownloadStatusFilter>) -> Self {
        let mut amounts = [0.; 3];
        if let Some(status) = open {
            amounts[status.slot()] = 1.;
        }
        Self {
            from: amounts,
            to: amounts,
        }
    }

    pub(crate) fn retarget(self, progress: f32, next: Option<DownloadStatusFilter>) -> Self {
        let progress = progress.clamp(0., 1.);
        let mut from = [0.; 3];
        for (slot, amount) in from.iter_mut().enumerate() {
            *amount = self.from[slot] + (self.to[slot] - self.from[slot]) * progress;
        }
        let mut to = [0.; 3];
        if let Some(status) = next {
            to[status.slot()] = 1.;
        }
        Self { from, to }
    }

    pub(crate) fn amount(self, status: DownloadStatusFilter, progress: f32) -> f32 {
        let progress = progress.clamp(0., 1.);
        let slot = status.slot();
        self.from[slot] + (self.to[slot] - self.from[slot]) * progress
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DownloadCategory {
    All,
    Video,
    Audio,
    Document,
    Image,
    Program,
    Archive,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DownloadFilter {
    pub(crate) status: DownloadStatusFilter,
    pub(crate) category: DownloadCategory,
}

impl DownloadFilter {
    pub(crate) const ALL: Self = Self {
        status: DownloadStatusFilter::All,
        category: DownloadCategory::All,
    };

    pub(crate) const fn new(status: DownloadStatusFilter, category: DownloadCategory) -> Self {
        Self { status, category }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum SidebarSection {
    Queues,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SidebarSelection {
    Download(DownloadFilter),
    MainQueue,
    LaterQueue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum TaskState {
    Pending,
    Downloading,
    Paused,
    Completed,
    Failed,
}

#[derive(Clone, Copy)]
pub(crate) enum TaskKind {
    Application,
    DiskImage,
    Mobile,
    Video,
    Audio,
    Document,
    Image,
    Archive,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum TaskSource {
    Local,
    Remote,
}

#[derive(Clone)]
pub(crate) struct DownloadTaskView {
    pub(crate) id: String,
    pub(crate) source: TaskSource,
    pub(crate) queue_id: String,
    pub(crate) name: String,
    pub(crate) size: String,
    pub(crate) size_bytes: u64,
    pub(crate) speed_bytes_per_second: Option<u64>,
    pub(crate) eta_seconds: Option<u64>,
    pub(crate) created_order: i64,
    pub(crate) kind: TaskKind,
    pub(crate) progress: f32,
    pub(crate) progress_label: String,
    pub(crate) state: TaskState,
    pub(crate) metadata_pending: bool,
}

impl DownloadTaskView {
    pub(crate) fn local_with_speed(task: &fluxdown_protocol::TaskDto, speed: Option<i64>) -> Self {
        Self::new(
            task.task_id.clone(),
            TaskSource::Local,
            task.queue_id.clone(),
            task.file_name.clone(),
            task.total_bytes,
            task.downloaded_bytes,
            speed.map(|value| value.max(0) as u64),
            task.created_at.parse().unwrap_or_default(),
            task.status,
        )
    }

    pub(crate) fn remote(task: &fluxdown_protocol::RemoteTaskDto) -> Self {
        Self::new(
            task.id.clone(),
            TaskSource::Remote,
            String::new(),
            task.file_name.clone(),
            task.total_bytes.unwrap_or_default(),
            task.downloaded_bytes,
            Some(task.speed.max(0) as u64),
            0,
            match task.status {
                fluxdown_protocol::RemoteTaskStatus::Pending
                | fluxdown_protocol::RemoteTaskStatus::Accepted => 0,
                fluxdown_protocol::RemoteTaskStatus::Downloading => 1,
                fluxdown_protocol::RemoteTaskStatus::Paused => 2,
                fluxdown_protocol::RemoteTaskStatus::Completed => 3,
                fluxdown_protocol::RemoteTaskStatus::Failed
                | fluxdown_protocol::RemoteTaskStatus::Canceled => 4,
            },
        )
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "normalizes the same compact projection from local and remote wire tasks"
    )]
    fn new(
        id: String,
        source: TaskSource,
        queue_id: String,
        name: String,
        total_bytes: i64,
        downloaded_bytes: i64,
        speed_bytes_per_second: Option<u64>,
        created_order: i64,
        status: i32,
    ) -> Self {
        let metadata_pending = name.trim().is_empty();
        let kind = task_kind(&name);
        let size_bytes = total_bytes.max(0) as u64;
        let downloaded = downloaded_bytes.max(0) as u64;
        let progress = if size_bytes == 0 {
            0.0
        } else {
            (downloaded as f64 / size_bytes as f64).clamp(0.0, 1.0) as f32
        };
        let eta_seconds = speed_bytes_per_second
            .filter(|speed| *speed > 0 && downloaded < size_bytes)
            .map(|speed| (size_bytes - downloaded) / speed);
        let id = match source {
            TaskSource::Local => format!("local:{id}"),
            TaskSource::Remote => format!("remote:{id}"),
        };
        Self {
            id,
            source,
            queue_id,
            kind,
            name,
            size: format_bytes(size_bytes),
            size_bytes,
            speed_bytes_per_second,
            eta_seconds,
            created_order,
            progress,
            progress_label: format!("{:.1}%", progress * 100.0),
            state: match status {
                0 | 5 => TaskState::Pending,
                1 => TaskState::Downloading,
                2 => TaskState::Paused,
                3 => TaskState::Completed,
                _ => TaskState::Failed,
            },
            metadata_pending,
        }
    }
}

impl DownloadFilter {
    pub(crate) fn matches(self, task: &DownloadTaskView) -> bool {
        let status_matches = match self.status {
            DownloadStatusFilter::All => true,
            DownloadStatusFilter::Completed => task.state == TaskState::Completed,
            DownloadStatusFilter::Incomplete => task.state != TaskState::Completed,
        };
        let category_matches = match self.category {
            DownloadCategory::All => true,
            DownloadCategory::Program => {
                matches!(task.kind, TaskKind::Application | TaskKind::Mobile)
            }
            DownloadCategory::Archive => {
                matches!(task.kind, TaskKind::DiskImage | TaskKind::Archive)
            }
            DownloadCategory::Video => matches!(task.kind, TaskKind::Video),
            DownloadCategory::Audio => matches!(task.kind, TaskKind::Audio),
            DownloadCategory::Document => matches!(task.kind, TaskKind::Document),
            DownloadCategory::Image => matches!(task.kind, TaskKind::Image),
            DownloadCategory::Other => matches!(task.kind, TaskKind::Other),
        };
        status_matches && category_matches
    }
}

fn task_kind(name: &str) -> TaskKind {
    let extension = name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase());
    match extension.as_deref() {
        Some("exe" | "msi" | "appimage") => TaskKind::Application,
        Some("apk" | "ipa") => TaskKind::Mobile,
        Some("iso" | "dmg") => TaskKind::DiskImage,
        Some("zip" | "rar" | "7z" | "tar" | "gz") => TaskKind::Archive,
        Some("mp4" | "mkv" | "webm" | "avi") => TaskKind::Video,
        Some("mp3" | "flac" | "wav" | "m4a") => TaskKind::Audio,
        Some("pdf" | "doc" | "docx" | "txt") => TaskKind::Document,
        Some("png" | "jpg" | "jpeg" | "gif" | "webp") => TaskKind::Image,
        _ => TaskKind::Other,
    }
}

pub(crate) fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{DownloadTaskView, TaskSource, TaskState};

    #[test]
    fn local_and_remote_wire_tasks_project_without_placeholders() {
        let local = serde_json::from_value::<fluxdown_protocol::TaskDto>(json!({
            "taskId":"local-1","url":"https://example.com/a","fileName":"a.bin",
            "saveDir":"/tmp","status":1,"downloadedBytes":50,"totalBytes":100,
            "errorMessage":"","createdAt":"7","proxyUrl":"","queueId":"main","checksum":""
        }))
        .expect("local task");
        let local = DownloadTaskView::local_with_speed(&local, None);
        assert_eq!(local.id, "local:local-1");
        assert_eq!(local.source, TaskSource::Local);
        assert_eq!(local.progress, 0.5);
        assert_eq!(local.state, TaskState::Downloading);
        assert!(!local.metadata_pending);

        let remote = serde_json::from_value::<fluxdown_protocol::RemoteTaskDto>(json!({
            "id":"remote-1","url":"https://example.com/b","fileName":"b.bin",
            "status":"paused","totalBytes":200,"downloadedBytes":20,"speed":0
        }))
        .expect("remote task");
        let remote = DownloadTaskView::remote(&remote);
        assert_eq!(remote.id, "remote:remote-1");
        assert_eq!(remote.source, TaskSource::Remote);
        assert_eq!(remote.state, TaskState::Paused);

        let probing = serde_json::from_value::<fluxdown_protocol::TaskDto>(json!({
            "taskId":"local-2","url":"https://example.com/unknown","fileName":"",
            "saveDir":"/tmp","status":0,"downloadedBytes":0,"totalBytes":0,
            "errorMessage":"","createdAt":"8","proxyUrl":"","queueId":"main","checksum":""
        }))
        .expect("probing task");
        let probing = DownloadTaskView::local_with_speed(&probing, None);
        assert!(probing.metadata_pending);
        assert!(probing.name.is_empty());
    }
}
