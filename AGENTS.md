# FluxDown — AI 工作契约（核心）

多协议下载管理器（IDM 的免费替代）。官网 <https://fluxdown.zerx.dev>，版本号以 `pubspec.yaml` 为准。
**一套 Rust 下载引擎 `fluxdown_engine` + 多宿主 + 多客户端**：当前默认 PC/移动 App 仍是 Flutter；GPUI 包 `fluxdown_ui_app` 已接入 `fluxdown-desktop → fluxdown-agent → fluxdownd → fluxdown_engine` 三进程本机链路。另有 headless Web 服务器、CLI、WXT 浏览器扩展、Tampermonkey 用户脚本、JS 插件系统、内置 MCP/REST/aria2 API、React Web SPA。FFI 框架 [Rinf 8.10](https://rinf.cunarist.org)（bincode 信号）**仅** Flutter App（`hub` crate）用到；`native/server`/`hub` 在 Flutter 完成独立切换前继续作为 legacy 生产宿主。

---

## 0. 本文件的边界 · 深挖索引

本文件只收**必须每回合在场**的东西：架构缝、硬不变式、命令、红线、坐标。
枚举性、可从源码复原的细节一律下沉到 `.omp/knowledge/*`（随仓分发，见下方「知识文档只能放 `.omp/`」），**需要时再 `read`**：

| 要查什么 | 读哪个 |
|---|---|
| 架构全图、顶层目录树（哪个目录管什么） | `.omp/knowledge/README.md` |
| 状态码 / DB 表与字段语义、6 种协议、引擎子系统（auto_proxy、RSS、segment_coordinator…）、插件系统、受管组件 | `.omp/knowledge/engine.md` |
| HTTP API 路由组与鉴权、hub / cli / nmh / updater、headless server env 与扩展路由 | `.omp/knowledge/hosts-and-api.md` |
| Flutter 与 GPUI 前端（主题 token、云同步、widgets 族、移动端、GPUI 迁移层）、扩展、用户脚本、Web SPA、官网 | `.omp/knowledge/clients.md` |
| 日志系统细节、发布流水线矩阵、设计文档实现状态（已实现 vs 仅设计，含命名歧义澄清） | `.omp/knowledge/ops.md` |
| **「要加 X 改哪里」全表 —— 动手前先查这张** | `.omp/knowledge/extension-points.md` |

维护约定：
- 只有**架构 / 契约 / 不变式**变化才改本文件；改一个普通文件不必回来更新它。
- 任何文档都**不**维护「完整文件清单 / 完整设置项清单」这类每次提交都漂移的枚举——源码是唯一事实源（`read <dir>` 看结构，`grep` 查事实）。
- 事实层（版本号、协议数、路由、env、DB 列、设置键）以代码为准，文档只给坐标。
- **知识文档只能放 `.omp/`**：`docs/` 与 `.agents/` 都在 `.gitignore` 里（本地目录，**零文件入库、不随仓分发**），`.omp/` 才是随仓分发的 AI 工具链目录。所以附录在 `.omp/knowledge/`；`docs/*.md` 只是本机设计草稿，别把契约写进去。
- **同改矩阵**：改本文件时同一回合把配套面过一遍，别只改一个文件。

| 改了什么 | 同回合必须过一遍 |
|---|---|
| 架构缝 / 不变式（§3–§5） | `.omp/knowledge/*` 对应附录；需要执行期拦截的再加 `.omp/rules/*.md` |
| 红线 / 授权 / 禁用命令（§6–§7） | `.omp/RULES.md`（粘性红线）、`.omp/rules/forbidden-commands.md`、`.omp/WATCHDOG.md`（严重级映射） |
| 跨仓契约 / 仓库地图 | 工作区根 `../AGENTS.md`（cwd=根时自动加载的是那份） |
| 长期结论 / 踩过的坑 | memory：`memory://root/MEMORY.md` + `memory_summary.md` |
| 章节标题（别处会引用） | `grep "AGENTS.md"` 扫全工作区，修掉悬空引用（skills / rules / 源码注释都引过） |

**单一事实源坐标**（要改契约先来这里）：

| 契约 | 位置 |
|---|---|
| 设置键 | `lib/src/models/settings_provider.dart` 的 load switch + 引擎 `db.rs` 的 `config` 表（**所有设置键都在这张表**） |
| DB schema | `native/engine/src/db.rs`：`SQLITE_SCHEMA` + `POSTGRES_SCHEMA` + `add_column_if_missing` |
| HTTP 契约 | `native/api/src/types.rs`（wire，camelCase）+ `routes.rs`（路径常量）；规范文件 `website/public/openapi.json` |
| 本机服务协议基线 | `native/protocol`：daemon / agent 角色、版本握手与后续 JSON-RPC wire 的唯一共享层 |
| Rust↔Dart 信号 | `native/hub/src/signals/mod.rs`；Dart 侧 `lib/src/bindings/` 由 `rinf gen` 生成，**勿手改** |
| headless env / 访问密钥策略 | `native/server/src/config.rs`（`validate_access_key`） |
| i18n 基线 | `assets/i18n/{en,zh}.json` + `lib/src/i18n/translations.dart` |

---

## 1. 执行目录：两种打开方式都要可靠

**本文件所有路径以 `FluxDown/` 为根**，命令按 cwd=`FluxDown/` 书写。

- **cwd = `FluxDown/`**（本文件自动加载）：路径与命令照抄即可。
- **cwd = 上级 `FluxDownProject/` 工作区**（那里的 `AGENTS.md` 自动加载，本文件按需 `read`）：所有路径前置 `FluxDown/`；命令必须带目录限定（bash 工具 `cwd="FluxDown"`、`cd FluxDown && …`、`--manifest-path FluxDown/…`），git 一律 `git -C FluxDown …`。**工作区根既不是 git 仓库也不是工程根**，不带限定必然报错；bash 的 cwd 不跨调用保持。

两份文档分工，互不复制：
- 跨仓地图与跨仓契约（FluxCloud `/api/v1`、插件索引仓、主题仓、发布镜像 secret）→ 上级 `../AGENTS.md`。
- FluxDown 内部架构 / 不变式 / 命令 → 本文件；**内部技术细节以本文件为准**（离代码最近）。
- 两边都写的红线（git 授权门槛、分支模型、禁用命令、i18n 基线）语义必须一致；FluxDown 被单独 clone 时本文件自洽，不依赖上级文件存在。

---

## 2. 命令速查

```bash
# ── 代码生成（改 Rust 信号后必须）──
rinf gen                              # 生成 Dart 绑定（lib/src/bindings 自动生成，勿手改）

# ── 构建 / 静态检查 ──
cargo check -p <crate> --lib          # 验证编译按 crate（不要整 workspace）
cargo fmt --check && cargo clippy -- -D warnings   # 提交前必过
flutter analyze                       # Dart 静态分析
# flutter run -d windows              # ⚠️ 禁止运行此命令

# ── 测试（按 crate/过滤，不要 --workspace）──
cargo nextest run -p fluxdown_engine <filter>   # 引擎单测（协议/分段/DB）
cargo test -p fluxdown_api            # HTTP API（axum/aria2/MCP/OpenAPI 漂移守卫）
cargo test -p fluxdown_server         # headless server（WS/actor/扩展路由）
cargo test -p fluxdown_cli            # CLI（退出码/尺寸解析 doctest）
flutter test                          # Dart 测试
PG_TEST_URL=postgres://postgres:pw@localhost/postgres cargo test -p fluxdown_engine -- --ignored pg_smoke
# 插件相关（feature 门控）：
cargo test -p fluxdown_engine --features plugins,components --test plugin_ffmpeg   # 真实执行经 FLUXDOWN_TEST_FFMPEG=<abs> 注入
cargo test -p fluxdown_engine --features plugins,components --test plugin_ytdlp    # FLUXDOWN_TEST_YTDLP=<abs>
cargo check -p fluxdown_engine        # 不带 feature：验证 mobile 关插件时主链路零变化

# ── 各宿主/客户端运行 ──
cargo run -p fluxdown_server          # headless 服务器（env 见 .omp/knowledge/hosts-and-api.md）
cargo run -p fluxdown_cli -- ping     # CLI 探活（子命令同上文件）
cargo run -p fluxdown_cli -- add <url> --local   # B 模式：内嵌引擎独立下载

# ── 前端/官网/扩展 ──
cd web && bun run dev                 # Web SPA localhost:5173（/api 代理到 :17800）；bun run build → web/dist
cd website && npm run dev             # 官网 Astro localhost:4321
cd fluxDown && npm run dev            # 扩展开发（Chrome）；dev:firefox / build / zip

# ── OpenAPI / 图标 / 发布 ──
cargo run -p fluxdown_api --example gen_openapi > website/public/openapi.json   # 改 API 后重生成
bun scripts/gen_icons.ts              # 改 assets/logo/fluxdown_logo.svg 后全平台图标一键生成
git tag -a vX.Y.Z -m "vX.Y.Z" && git push origin vX.Y.Z   # 触发发布流水线（稳定版从 stable，预览 -rc.N 从 main；见 §6）
```

---

## 3. 架构缝：三个引擎自有 trait

**一个引擎，多个宿主，多个客户端。** 所有下载逻辑集中在 `fluxdown_engine`（`native/engine`，零 FFI/零 rinf），经三个 trait 与外界解耦：

| Trait | 定义位置 | 方向 | 职责 |
|---|---|---|---|
| `EventSink` | `engine/src/events.rs` | 引擎→宿主 | 进度/分段拆分/队列变化/组变化等事件推送 |
| `HostSelection` | `engine/src/selection.rs` | 引擎→宿主（请求决策） | HLS 画质 / BT 文件 / 插件 variant 选择（tristate：用户选/超时默认/无 selector 短路） |
| `ApiHost` | `native/api/src/service.rs` | 客户端→引擎（HTTP 契约） | REST/aria2/MCP 的能力面；必需方法 + 可默认降级方法 |

- 当前两个生产宿主仍是 `hub`（App，actor=`download_actor.rs`）与 `server`（headless，actor=`actor.rs`）；`fluxdown_api` 只依赖 `&dyn ApiHost`，同一套 HTTP 面服务任意宿主。CLI 双模式：默认 HTTP 连宿主，`add --local` 内嵌引擎。
- **迁移目标**：`native/daemon` 成为可独立运行的纯下载核心，`native/agent` 常驻承载账户/云同步/设备协同与官方 UI Gateway；两者共享 `native/protocol` 的 JSON-RPC 语义。`native/server` 进入废弃路径，任何新实现不得依赖它。
- **并发模型**：current_thread tokio actor 串行化写；每个下载 spawn 独立 task + CancellationToken；插件 resolve 永不阻塞 actor（off-actor spawn + 通道回流）。
- 客户端捕获三条并行前端进同一本机 RPC（`:17800/download`）：扩展、用户脚本、桌面确认框。

---

## 4. crate 边界与硬不变式

**crate 边界**
- `fluxdown_engine`：零 rinf/Dart/axum 依赖，只经 `EventSink`/`HostSelection` 与宿主解耦。协议/分段/DB/队列/组/插件全在这里。
- `fluxdown_api`：依赖 `fluxdown_protocol` 的规范 DTO 与 `&dyn ApiHost`，只定义 HTTP 路径/服务器/兼容层。零引擎、零 rinf。
- `hub`：**唯一**碰 rinf FFI 的 crate（crate 名不可改，rinf 硬编码）。只做信号收发与类型转换，不含协议逻辑；`signal_bridge.rs` 是 `engine::model` ↔ `hub::signals` 的孤儿规则边界。
- `crates/{i18n,theme,components,shell,downloads,settings,account,rss,extensions,app}`：GPUI PC 迁移层；`app` 是唯一 composition root，所有 capability 只依赖本地端口和 protocol DTO。新增页面与 capability 的 crate 边界、目录归属、依赖方向见 `rule://gpui-crate-architecture`。
- `fluxdown_protocol`：唯一传输无关 wire 层；只能依赖序列化/纯类型能力，不依赖引擎、运行时、数据库、HTTP 或 UI。
- `fluxdown_engine_protocol`：引擎模型与 protocol DTO 的无状态、无损转换边界；宿主使用命名函数，API/agent/UI 不依赖它。
- `fluxdown_daemon`：`fluxdownd` 纯下载核心；独占 engine/下载 DB，拥有任务、队列、组、下载设置、RSS、插件、Webhook 与选择。
- `fluxdown_agent`：`fluxdown-agent` 官方 UI Gateway 与 FluxCloud owner；拥有 Token、设备身份、同步、远程任务、捕获与兼容 API，只经 protocol RPC 调 daemon。
- **feature 门控**：`plugins`、`components`（默认关；desktop/server 开，mobile/CLI 关）。**关插件时下载主链路零行为变化**（注入 no-op `PluginManager`）。

**编译期陷阱**
- `download_actor.rs` 主 `tokio::select!` **已占满 tokio 64 分支硬上限**，再加一条即编译错误。新增任何 Dart 信号 / 定时节拍 / 回流通道**都不许往主循环加分支**——并进既有 `AuxSignal` 合并泵（两个后台 spawn 把消息合流进单条 `aux_tx`，主循环只有一条 `aux_rx.recv()`）。
- rquickjs（`engine/Cargo.toml`）：禁止叠加 `rust-alloc`/`allocator`（会让 `set_memory_limit` 静默失效）；必带 `parallel`（`AsyncRuntime`/`AsyncContext` 的 Send/Sync 依赖它）。
- `profile.release` **不**设 `panic="abort"`——`download_manager` 靠 `catch_unwind` 恢复 task panic。
- **`fluxdown_server` 的 Web UI 是编译期内嵌的**：`native/server/build.rs` 把 `FLUXDOWN_EMBED_WEBROOT`（缺省 `web/dist`）整棵目录递归全量 `include_bytes!` 进二进制（不按扩展名筛选，新增文件/新建子目录下次编译自动进包）。改了前端**必须先 `cd web && bun run build` 再重编服务器**才能看到；产物是单二进制，不再有同级 `web/` 目录，`FLUXDOWN_WEBROOT` 降级为可选的磁盘覆盖。构建时目录缺失只 warning + 运行期 503 提示页，不会让编译失败。

**运行期不变式**
- 两个宿主 actor **都必须** drain `resolve_rx`（off-actor 插件解析回流）与 `plugin_retry_rx`，否则命中 resolver 的下载永久挂起。
- pg 字节列必须 `BIGINT`（`INTEGER` 会在 >2GB 静默截断）；新表要同时进 `SQLITE_SCHEMA` + `POSTGRES_SCHEMA` + 迁移。
- **每个 console 子进程 spawn 都要包 `proc::no_console_window`**（ffmpeg/ffprobe/yt-dlp/tar/探版），否则 Windows 闪黑窗。
- **anyhow 错误一律 `{e:#}`**：`{e}` 只输出最外层 context，会整个吞掉根因（BT 的 `dht.json` 撞 Windows 端口排除区间导致全部 BT 任务 status=4，就是这么被吞掉的；DHT 持久化是纯缓存，`SharedBtSession::new` 三级兜底）。
- **BT 判定只认 `magnet:` 与 `torrent-file://` 哨兵**。HTTP 的 `.torrent` 直链不会走 BT，会被当普通文件下回一个种子文件；要变成真下载必须先抓字节再以 `NewTaskSpec::torrent_file_bytes` 建任务（RSS 就是这么做的）。
- **「复制链接」类 UI 一律读 `origin_url`，空则回退 `url`**（torrent 任务的 `url` 是哨兵）——Dart `DownloadTask.shareUrl` / web `taskShareUrl()`。
- **RSS 是无人值守链路**：任何「需要用户点一下才能继续」的东西都是 bug。建任务即落全选 + `unattended=1`（否则启动时会弹 N 次文件选择框）；`create_task` 内部自发建任务必须补 `load_and_send_all_tasks()`（`TaskProgress` 不带 `queue_id`）；手动「重新下载」对**任何**状态放行。
- 引擎学习/遥测类 config 键（`cdn_node_health`、`auto_route_health`、`cdn_pending_reports`、`domain_conn_caps`）**UI 不读写**。
- **遥测只有两条匿名部署事件**（`app_installed` 一次 + `app_active` 每日，`analytics_enabled` 门控），**绝不**采集下载/任务信息——不要新增遥测点。
- 命名歧义：`tracker_subscription.rs` / `ed2k/server_subscription.rs` 是 BT tracker 列表 / ED2K `server.met` 订阅，与 `rss/` 的 feed 订阅无关；官网 `api/webhooks/github` 是 GitHub 接收器，与 `engine/src/webhook.rs` 的任务事件推送无关。

---

## 5. 镜像契约：改一处必须同步另一处

| 改这里 | 必须同步 |
|---|---|
| `engine/src/rss/filter.rs` | `lib/src/models/rss_filter.dart` + `web/src/lib/rss-filter.ts`（三份逐条对齐；预览与实际下载不一致会直接摧毁功能可信度） |
| `server/src/config.rs::validate_access_key` | `web/src/lib/token-policy.ts` |
| `engine/src/data_dir.rs` | `lib/src/services/platform_utils.dart` 的 `KNOWN_ITEMS` |
| `engine/src/webhook.rs` 的 `WebhookEventKind` | Dart `WebhookEvents.all` + TS `WEBHOOK_EVENTS`，**三处 wire 名逐字一致** |
| `native/nmh/src/main.rs::log_path`（中继自身的诊断日志，在 App 日志目录之外） | `native/hub/src/diagnostics.rs::nmh_log_path`（Doctor 读同一文件的尾部）；改路径必须同步，否则 Doctor 只会报「无日志」 |
| `hub/src/signals/mod.rs` | `rinf gen` → `download_actor` 的 `AuxSignal` 泵 → Dart 侧 `rustSignalStream` 监听 |
| `native/api` 契约 | 重跑 `gen_openapi` 覆盖 `website/public/openapi.json` |
| 任一 UI 文案 | 只补 **en + zh 基线对**：App `assets/i18n/{en,zh}.json` + `translations.dart` getter；`web/src/lib/locales/{en,zh}.json`；`website/src/lib/locales/{en,zh-CN}.json`；`fluxDown/utils/locales/{en,zh-CN}.ts`。社区语言（`ja` 等）由 Weblate 维护，**不碰**（运行时键级回退英文） |
| web 设置项 / 对话框字段归属 | **基准 = 桌面**：同一功能在两端的分类归属与分区排序必须一致（桌面 `settings_page.dart` 分类 ↔ web 分区组件 GeneralSettings/DownloadSettings/ProxySettings…）。双端并行开发时归属分类要写成一份共享契约，禁止两份各自措辞 |
| 「一键分类目录」的目录名推导 | `lib/src/models/custom_category.dart` 的 `sanitizeCategoryDirName` / `categoryDirUnder` ↔ `web/src/lib/categories.ts` 同名函数（含分隔符归一）；**且内置分类显示名两端逐字一致**（App `assets/i18n` 的 `categoryVideo/...` ↔ web `type.video/...`），否则同一台机器上桌面与 Web 会各建一套目录（`Document` vs `Documents`） |

---

## 6. git · 分支 · 发布

- **git 写操作的门槛是「用户授权」**：用户在本会话要求过（含 `/commit`、「提交」「推一下」「发版」）→ 视为已授权，核对前置条件后**直接做完**，不再征询；用户没要求、你自己想顺手做 → 停手先问。授权按动作粒度计（提交 ≠ 推送，打 tag ≠ 发布）。
- **分支模型**：`main` = 开发分支（超集 / 最新），`stable` = 稳定分支（子集）。日常一律在 `main`；`stable` 只经合并/cherry-pick `main` 前进；hotfix 直进 `stable` 必须**同回合**同步回 `main`。一致性判据 `git log stable --not main` **恒为空**。
- **tag**：稳定 `vX.Y.Z` 只从 `stable`，预览 `vX.Y.Z-rc.N` 只从 `main`；CI 有分支守卫，打错分支整条流水线失败。推送 `v*` tag **立即触发全平台发布，不可逆**。
- 流水线是**组件变更检测**式（`changes` job diff `PREV..TAG` 映射路径→`app`/`extension`/`server`/`mobile`/`cli`）；`website/*`、`docs/*`、`*.md` 不触发构建。矩阵细节见 `.omp/knowledge/ops.md`。

---

## 7. 代码风格与强制规则

**Rust**
- Edition 2024；Clippy **deny**：`unwrap_used`/`expect_used`/`wildcard_imports`。非测试代码禁 `.unwrap()`/`.expect()`，用 `?` + `thiserror`；禁 `use foo::*`。禁 `unsafe`（除已批准的 `fallocate`/`statvfs`/`GetDiskFreeSpaceExW`）。
- snake_case 函数/变量，PascalCase 类型，SCREAMING_SNAKE_CASE 常量；公开 API `///` + doctest。
- 异步优先，同步阻塞走 `spawn_blocking`；重试指数退避（MAX=3，base=2s）；task panic 用 `AssertUnwindSafe` + `catch_unwind`。
- 日志宏：`use crate::logger::log_info; log_info!("[mod] ...")`（Rust 2024 无 `#[macro_use]`，每文件显式 use）。Dart 侧 `logInfo(_tag, msg)` 写**同一文件**，格式 `HH:MM:SS.mmm [Tag] message`。

**Dart / Flutter**
- SDK `^3.10.8`，lint `flutter_lints ^6.0.0`。UI **全程 shadcn_ui ^0.52.1**，禁原生 Material/Cupertino：根 `ShadApp`，主题 `ShadTheme.of` / `AppColors.of` / `AppMetrics.of`，对话框 `showShadDialog`，图标 `LucideIcons`，字体 MiSans。
- 状态：ChangeNotifier + ListenableBuilder（无 Provider/Riverpod/Bloc），`_safeNotifyListeners()`；模型不可变 + `copyWith()`；文件名 snake_case.dart。
- **i18n**：面向用户的字符串一律 `S.of(locale).xxx`，禁硬编码文案（快捷键/单位/品牌名/语言自称除外）。

**通用门槛**
- **禁止新增 dependency**，需要时先说明理由等确认；**禁止手编 `Cargo.toml` 版本号**，用 cargo 命令。
- 改动前 `cargo check -p <crate> --lib`；提交前 `cargo fmt --check && cargo clippy -- -D warnings`；测试用 `cargo nextest run -p <crate> <filter>`，**禁 `--workspace`**；**禁 `flutter run -d windows`**。
- 优先复用已有 trait/error 类型，不平行造轮子。单文件 >600 行考虑拆分，单函数 >80 行需说明。
- 查文档优先级：`cargo path <crate>` 本地源码 > docs.rs > web 搜索。
- **命中以下任一项前先读 `rust-router` skill**：新增/改 public API/trait/error 类型、unsafe/FFI/性能关键路径、新增 crate/调 workspace、写 doc comment。仅改名/格式/加日志可跳过。
