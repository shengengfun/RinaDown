//! aria2 兼容退出码。
//!
//! 对齐 aria2c 的退出码语义（`man aria2c` EXIT STATUS），CLI 各命令
//! 失败时映射到最贴近的码，脚本可据此判断失败类别。
//!
//! # Examples
//!
//! ```
//! use fluxdown_cli::exit::ExitCode;
//!
//! assert_eq!(ExitCode::Success as i32, 0);
//! assert_eq!(ExitCode::Unknown as i32, 1);
//! assert_eq!(ExitCode::Unfinished as i32, 7);
//! ```

/// aria2 风格退出码子集（只保留 CLI 实际会返回的类别）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ExitCode {
    /// 所有下载/操作成功。
    Success = 0,
    /// 未知错误。
    Unknown = 1,
    /// 请求超时。
    Timeout = 2,
    /// 资源未找到（404 / 任务不存在）。
    NotFound = 3,
    /// 网络/连接错误（无法连到服务器）。
    Network = 5,
    /// 鉴权失败（token 缺失或无效）。
    Auth = 24,
    /// 参数非法（400 / 用户输入错误）。
    BadRequest = 32,
    /// 用户中断（Ctrl-C）时仍有未完成下载（对齐 aria2 code 7）。
    Unfinished = 7,
}

impl ExitCode {
    /// 转为进程退出码整数。
    #[must_use]
    pub fn code(self) -> i32 {
        self as i32
    }
}
