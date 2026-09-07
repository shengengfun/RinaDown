//! 进程生成的跨平台辅助。
//!
//! 引擎会拉起若干**控制台子进程**（ffmpeg / ffprobe / yt-dlp / tar，以及组件
//! 版本探测 `-version` / `--version`）。在 Windows 上，若不显式设置
//! `CREATE_NO_WINDOW`，每次拉起都会闪现一个黑色控制台窗口——打开设置「组件」
//! 页做版本探测时尤其高频、肉眼可见。
//!
//! 因此引擎内所有经 [`tokio::process::Command`] 拉起控制台程序的调用点都
//! **必须**先经 [`no_console_window`] 处理，切勿直接 `.spawn()` / `.output()`。

/// 为控制台子进程设置 Windows `CREATE_NO_WINDOW`，避免闪现黑色控制台窗口。
///
/// GUI 子系统的可执行程序不受此标志影响；非 Windows 平台为空操作。
/// 引擎内所有拉起外部控制台程序（ffmpeg/ffprobe/yt-dlp/tar/版本探测）的
/// [`tokio::process::Command`] **必须**先经此函数处理，否则会在 Windows 上闪窗。
pub(crate) fn no_console_window(cmd: &mut tokio::process::Command) {
    #[cfg(target_os = "windows")]
    {
        /// `CREATE_NO_WINDOW`：不为控制台子进程分配/闪现窗口。
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = cmd;
    }
}
