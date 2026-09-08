# 本地转录（ASR）插件设计 · rinadown@transcribe

> 目标：下载完成（含 yt-dlp 直链 + ffmpeg mux 产物）后，用**本地模型**把视频/音频
> 转录成 SRT/VTT 字幕侧车文件。参照 `D:\Project\Audire`：模型走 **whisper.cpp 的
> ggml 量化模型**（CPU 可跑，另有 GPU 后端可选），模型文件可单独下载。
> 本文只给契约与改动坐标（枚举/事实以源码为准），落地时应作为一个独立变更提交，
> **必须**过 `cargo check/test` 与 Flutter analyze——受管组件不是纯插件脚本。

---

## 1. 为什么不能只写一个“onDone JS”

RinaDown 插件只能在受控面调用宿主已授权的工具：目前仅 `ffmpeg` / `yt-dlp` 两个
权限（`native/engine/src/plugin/manifest.rs::VALID_PERMISSIONS` 闭合枚举）。插件
无权任意 spawn 可执行文件。要做本地 ASR 必须新增一个**受管组件 + 对应插件权限**，
否则 manifest 校验直接拒绝。因此这是一个“组件级”功能，需按下表逐层落地。

## 2. 与 Audire 的对接映射

| Audire | RinaDown 对应方案 |
|---|---|
| `whisper.cpp` 预编译后端 + `models/ggml-*.bin`（见 Audire `docs/models.md`：tiny/small/medium/large-v3/large-v3-turbo 及多语言变体） | 新增受管组件 `whisper`（桌面分发平台官方 whisper.cpp CLI 单文件二进制），模型文件由用户在设置中指定目录/URL（不随仓库分发，版权与体积原因） |
| 前端选择模型 + 本地执行 | RinaDown「组件页」安装 whisper 后端；「插件设置」填模型路径与语言/格式 |
| `Transcript{segments:[{start,stop,text,speaker}]}` 输出 | 插件钩子接收 stdout JSON（whisper `-oj`），在 JS 内组 SRT/VTT 写入 `flux.fs` 工作区，随后用产物注册（对齐 ffmpeg 转码产物注册方式） |
| ffmpeg 抽取/预处理 | 复用 RinaDown 已装 ffmpeg：先抽 16k 单声道 wav 到工作区再喂 whisper |

## 3. 改动坐标（新增受管组件通用面，镜像 `ffmpeg`/`ytdlp` 先例）

1. `native/engine/src/components/whisper.rs`
   - `resolve_whisper(...)`：解析受管 whisper CLI 路径（config `component.whisper.path`
     或受管安装位置；探版本）。
   - `install_whisper / uninstall_whisper / list_whisper_versions`（`components` feature
     门控；release URL 配置镜像 `ytdlp.rs::install`）。
   - 每个 console 子进程 spawn 包 `proc::no_console_window`（Windows）。
2. `native/engine/src/plugin/`：
   - `runtime.rs`：新增 `WhisperSpec{ args, subdir, timeout_ms }` + `run_whisper` Outcome/
     Availability（禁止 rquickjs 类型）；`HostContext` 增加 `whisper_permitted`。
   - `bridge.rs`：实现 `run_whisper`（复用 yt-dlp 的 semaphore + 参数牢笼：禁
     `--exec` 等危险开关、越牢路径、cwd 限定工作区；模型路径经白名单校验）。
   - `runtime.rs::PluginRequest`/`PluginHost` trait 增方法；`quickjs.rs` 注入 `flux.whisper`。
   - `manifest.rs`：`VALID_PERMISSIONS` 追加 `"whisper"` + `PERMISSION_WHISPER`；
     `engine/Cargo.toml` 组件 feature 依赖照旧。
   - `plugin/dependencies.rs`：把 `whisper` 权限映射到组件缺件提示（UI 提醒）。
3. 协议 / 宿主接线：
   - `rinadown_protocol` daemon `ComponentKind` + `ComponentStatusDto`/版本枚举加
     `Whisper`；`native/protocol` 若走 daemon 同样补。
   - `native/daemon/src/service.rs` / `native/server/src/routes_ext.rs` / hub
     `download_actor.rs` 的组件 install/uninstall/status 三臂照 ffmpeg/ytdlp 补齐
     （hub 走 `components_provider` Dart 控制器）。
   - Dart `components_provider.dart`（Ffmpeg/Ytdlp 控制器）+ 组件卡片映射
     `plugin_detail_dialog.dart`/`plugin_list_view.dart`/`settings_page.dart`
     `component-whisper` 加一臂。
4. 示例插件 `examples/plugins/transcribe/{manifest.json,onDone.js}`：
   - manifest：`permissions:["whisper","ffmpeg"]`，`hooks.events:["onDone"]`，
     match.urls 限定本地媒体完成事件（可用 `match` 留空=全部，或限视频站点）。
   - 流程：onDone → `flux.ffmpeg` 抽 wav → `flux.whisper.run({ args:[...] })`
     （model/language/format 来自插件设置）→ stdout JSON → 组装 srt/vtt 写入
     `flux.fs` → `flux.task.recordArtifact(...)` 登记字幕产物。
   - `engine/tests/example_plugins.rs` 自动校验 manifest 合法（真实运行需
     `RINADOWN_TEST_WHISPER=<abs>` + 模型，仿 `plugin_ytdlp` 集成测试）。

## 4. 模型与运行时（对齐 Audire `docs/models.md`）

- 模型：`ggml-*.bin`（huggingface ggerganov/whisper.cpp；多语言用
  NbAiLab/KBLab/ivrit-ai 等变体）。下载走 App 通用下载器（新增任务），存到
  `<data>/whisper-models/`；插件设置项提供“模型文件路径”与“选择下载模型”两个入口。
- 语言：默认 auto；提供插件设置 `language`（如 `zh`/`en`）与
  `outputFormat`（srt/vtt/txt）。
- 性能/超时：hook 预算须抬升（镜像 ffmpeg/ytdlp 的 EXTERNAL_TOOL_HOOK_BUDGET）；
  转录属 CPU 长任务，进度经 `flux.logger`/任务日志上报，不阻塞任务状态机。

## 5. 验收

- `cargo test -p rinadown_engine --features plugins,components --test example_plugins`
  校验新 manifest 可加载、权限合法。
- `cargo check -p rinadown_engine`（关 plugins）验证主链路零变化。
- 真实冒烟（需 whisper 二进制 + 模型）：下载短视频 → 完成后生成同名 `.srt`，
  时间戳与音频对齐；删除任务时字幕产物随 ffmpeg mp4 一并清理（`task_artifacts`）。
- 不引入第三方 crates：whisper 走子进程，运行时只需现有 reqwest + fs 能力。
