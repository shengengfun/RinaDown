# FluxDown internals · 扩展点索引

> 本文件是 `FluxDown/AGENTS.md` 的深挖附录：只放**枚举性 / 可从源码复原**的细节，硬不变式与红线在 AGENTS.md。
> 路径以 `FluxDown/` 为根（cwd=工作区根时前置 `FluxDown/`）。事实层以源码为准，文档给坐标。

---

## 扩展点索引（"要加 X 改哪里"）

| 目标 | 步骤 |
|---|---|
| **新增下载协议** | 加 `is_X_url` 谓词 + `run_X_download`（照 `ed2k` 模板）；在 `download_manager` 的 `do_start_task` **与** `do_resume_task` if/else 各加一臂；新表加进 `SQLITE_SCHEMA`+`POSTGRES_SCHEMA`+迁移 |
| **新增受管组件** | `components/` 下照 `ffmpeg.rs`/`ytdlp.rs` 加模块（resolve 优先级 + Status + install under cfg）；加 config 键；在 `plugin/dependencies.rs` 映射 |
| **新增插件工具面** | `runtime.rs` 加 Spec/Outcome/Availability（禁 rquickjs 类型）；`HostContext` 加门；`bridge.rs` 实现（semaphore + 牢笼）；`manifest` 加 permission |
| **新增 Dart↔Rust 信号** | `hub/src/signals/mod.rs` 定义（`DartSignal`/`RustSignal`/`SignalPiece`）→ `rinf gen` → **并进 `download_actor` 的 `AuxSignal` 合并泵**（主 `select!` 已满 64 分支硬上限，绝不能加新分支，见 AGENTS.md「crate 边界与硬不变式」）→ Dart 端 `XxxSignal.rustSignalStream` 监听 |
| **新增 RSS 过滤规则** | `engine/src/rss/filter.rs` 改判定 + 补单测 → **同步** `lib/src/models/rss_filter.dart` 与 `web/src/lib/rss-filter.ts` 两份镜像（预览与实际下载不一致会直接摧毁功能可信度）→ 三 Tab 对话框加控件 + i18n |
| **新增 HTTP 能力** | 扩 `ApiHost`（带默认 impl 保持现有宿主可编译）+ `api/server.rs` handler + `routes.rs` 常量；两宿主（hub `api_host.rs` / server `host.rs`）按需 override；跑 `gen_openapi` 重生成 |
| **新增本机 RPC 能力** | `native/protocol` 先加唯一 method/DTO/event/error → owner 进程（下载事实进 daemon actor；账户/云/UI Gateway 进 agent）实现 → `crates/app` 单会话 adapter 映射到 capability-local port；二进制 body 留专用鉴权 HTTP 端点 |
| **新增 aria2 方法** | `aria2.rs` `METHOD_NAMES` + `jsonrpc.rs` dispatch | 
| **新增 MCP 工具** | `mcp.rs` tool_definitions + call_tool |
| **新增引擎事件** | `events.rs` `EngineEvent` 变体 + `EventSink`；`rinf_sink`/`ws_hub` 各接线一处 |
| **新增 Doctor 诊断项** | `hub/src/diagnostics.rs` 加探针 fn（返回 `DiagnosticCheck`，wire `id` 稳定、`level` 取 `ok`/`warn`/`error`/`info`、`hint` 取既有 code）→ 接进 `probe_sync()` 或 `run()` 的顺序里 → Dart `translations.dart` 的 `S.doctorCheckLabel` 加一臂 + `assets/i18n/{en,zh}.json` 加 `doctorCheckXxx`（新 hint 再加 `doctorHintXxx`）；要能就地修复则在 `doctor_report_view.dart` 的 `_actionFor` 加一臂 + `doctorActionXxx` 文案。**信号无需改动**：`DiagnosticCheck` 是通用行，加检查项不动 wire schema、不用 `rinf gen`。诊断与修复动作走 `download_actor` 里的独立 Doctor 泵（不碰 Engine、不进主 `select!`） |
| **新增 webhook 事件** | `engine/src/webhook.rs` 的 `WebhookEventKind` 加变体（`wire()`/`title()` 同步）+ 在 `download_manager` 对应生命周期点位 `self.webhook.emit(...)`；UI 侧事件芯片自动跟随 `WebhookEvents.all`（Dart）/ `WEBHOOK_EVENTS`（TS），**三处 wire 名必须逐字一致** |
| **新增 webhook 服务预设** | 只改 `engine/src/webhook.rs`：`Preset` 加变体 + `wire`/`label`/`content_type`/`escape`/`default_template`/`url_placeholder` 六个 match 各补一臂。模板由引擎下发，UI 零改动（只有品牌字标 `WebhookPresetMark`/`PRESET_MARKS` 想美化时才加） |
| **新增引擎设置** | Flutter：`settings_provider.dart` 加字段+setter(`_saveToRust`)+load switch case；要跨设备同步则 `sync_catalog.dart` 加 `SyncEntry`（否则默认设备本地）。**daemon/GPUI**：`native/protocol/src/daemon_config.rs` 的 `DAEMON_CONFIG_FIELDS` 加一行（类型/范围/默认）→ daemon 校验与投影自动跟随 → `native/daemon/src/actor.rs::apply_live_config` 按需 live-apply → GPUI `crates/settings/src/sections/<page>.rs` 用 `ctx.daemon_switch/number/input/dropdown(key)` 加条目（文案键复用 `assets/i18n`）。设备本地偏好（非 daemon）直接 `ctx.pref_*` 写 `agent.preferences`，无需改协议 |
| **新增 Flutter 主题预设/度量** | `flux_theme_tokens.dart` 加 `BuiltinThemeId`+工厂+`builtinThemes` 项 / `flux_metric_tokens.dart` 加 clamped 字段 + `app_metrics.dart` 暴露 |
| **新增 GPUI 主题/基础组件** | `crates/theme` 改完整亮暗 `SemanticThemeTokens`（Base 新 token 必须同步两份）→ `crates/components` 只从 `active_theme().tokens()` 取值；文案仍只改共享 `assets/i18n/{en,zh}.json`，`crates/i18n/build.rs` 自动嵌入 |
| **新增 GPUI capability / 设置分区** | 状态/命令/多页面满足拆 crate 条件时在 `crates/<capability>` 定义本地 port/controller/view；只依赖 `fluxdown_protocol` 和 UI 基础层 → `crates/app` 注入同一个 `Arc<AgentClient>` adapter 与有序 snapshot/event 流；shell 只收内容槽。设置页 = gpui-component `Settings` DSL：`crates/settings/src/sections/<page>.rs::page(&SectionContext, cx) -> SettingPage`，状态经 `Entity<SettingsStore>`（防抖合并写回、乐观覆盖、冲突重试），自定义控件用 `SettingField::render` + `window.use_keyed_state`，对话框走 `window.open_dialog`（shell 已渲染 dialog/sheet/notification 层） |
| **新增 NAS/分发目标** | `packaging/<target>/build_*.sh` 复用（**单个** server 二进制 + `FLUXDOWN_*`；Web UI 已编译期内嵌，别再往包里塞 `web/` 目录）布局 → 接入 release.yml `build-server-nas-packages` |
| **新增发布组件** | `changes` job 加路径→输出映射 + 一对 `build-*`/`release-*` job（各自组件 tag） |
| **新增文档页** | `content/docs/{en,zh}/<section>/<page>.md`（section 加进 `content.config` 枚举，zh 跑 `docs:hash`） |
| **新增自更新平台策略** | `fluxdown_updater` Action + `hub/updater.rs` |
| **新增 UI 文案** | 只补 **en + zh 基线对**：App `assets/i18n/{en,zh}.json` + `lib/src/i18n/translations.dart` 加 getter；web `web/src/lib/locales/{en,zh}.json`；官网 `website/src/lib/locales/{en,zh-CN}.json`；扩展 `fluxDown/utils/locales/{zh-CN,en}.ts`（`MessageKey` 由 zh-CN 推导）。**社区语言（`ja.json` 等）不碰**——Weblate 维护，运行时键级回退英文 |
