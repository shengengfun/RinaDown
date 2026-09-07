---
description: Windows 拉起控制台子进程必须设 CREATE_NO_WINDOW，避免闪现黑窗口
condition:
  - 'process::Command::new'
  - 'Command::new\('
interruptMode: never
globs:
  - native/**/*.rs
---

你正在用 `Command::new(...)` 拉起子进程。在 Windows 上，拉起**控制台程序**（ffmpeg / ffprobe / yt-dlp / tar、以及 `-version` / `--version` 版本探测等）若不设 `CREATE_NO_WINDOW`，每次都会闪现一个黑色控制台窗口——组件版本检查、插件调用外部工具时高频可见，是反复出现的体验缺陷。

规则：

- **engine crate（`native/engine`）**：任何 `tokio::process::Command` 构造后、`.spawn()` / `.output()` 之前，必须先经 `crate::proc::no_console_window(&mut cmd)`（见 `native/engine/src/proc.rs`）。因此需要 `let mut cmd = Command::new(...)` 绑定，不能直接链式 `Command::new(...).args(...).output()`。
- **hub / nmh / updater 等 App-shell crate**：沿用已有写法——`use std::os::windows::process::CommandExt;` + `.creation_flags(0x0800_0000)`（`CREATE_NO_WINDOW`），参考 `native/hub/src/reveal_file.rs` / `updater.rs`。
- 该标志用 `#[cfg(target_os = "windows")]` 包裹；GUI 子系统程序不受影响，非 Windows 平台为空操作。

例外（无需设标志）：
- 拉起的是 **GUI 程序**（如重启 App 自身、`explorer.exe` 打开目录）——GUI 子系统不弹控制台窗；但 `cmd.exe /c ...` 仍属控制台程序，必须设。
- 代码已经设置了该标志或已调用 `no_console_window`——保持即可。
- Linux/macOS 专属分支（`#[cfg(unix)]` 下的 `xdg-open` / `open` / `dbus-send` 等）。

修正方式：改为 `let mut cmd = Command::new(...)`，紧接一行 `crate::proc::no_console_window(&mut cmd);`（engine）或 `.creation_flags(CREATE_NO_WINDOW)`（App-shell），再继续配置与拉起。
