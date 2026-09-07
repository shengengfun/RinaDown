# FluxDown internals · 日志 · 发布与 CI · 设计文档实现状态

> 本文件是 `FluxDown/AGENTS.md` 的深挖附录：只放**枚举性 / 可从源码复原**的细节，硬不变式与红线在 AGENTS.md。
> 路径以 `FluxDown/` 为根（cwd=工作区根时前置 `FluxDown/`）。事实层以源码为准，文档给坐标。

---

## 日志系统

Dart 与 Rust 两端写**同一目录同一文件**，统一格式 `HH:MM:SS.mmm [Tag] message`。
- 目录：Windows exe 同级 `logs/`；Linux `~/.local/share/fluxdown/logs/`。文件名 `fluxdown_YYYY-MM-DD.log`，单卷 2MB 分卷，总量超 `log_max_size_mb`（默认 10MB）从最旧删，保留 7 天。
- Dart：`import '../services/log_service.dart'; logInfo(_tag, msg); logError(...)`。
- Rust：`use crate::logger::log_info; log_info!("[mod] ...")`（Rust 2024 无 `#[macro_use]`，每文件显式 use）。
- 导出：设置「关于」→ ZIP（纯 Dart 标准库，零依赖）。

---

## 发布与 CI（`.github/workflows/release.yml`）

**组件变更检测**流水线，`v*` tag 触发。`changes` job diff `PREV..TAG` 映射路径→输出（`app`/`extension`/`server`/`mobile`/`cli`），首个 tag 全量构建。**分支守卫**：稳定 `vX.Y.Z` 必须是 `origin/stable` 祖先；预览 `vX.Y.Z-rc.N` 必须在 `origin/main`；否则整条失败。

路径→组件映射（要点）：`fluxDown/*`→extension；`web|native/server|docker|packaging/*`→server；`native/cli/*`→cli；`native/api/*`→server+cli；`native/engine/*`→app+server+mobile+cli；`android|lib/src/mobile/*`→mobile；`lib/*`→app+mobile；`website/*`/`docs/*`/`*.md`→不构建。

构建矩阵：Windows（x64+arm64，Inno 安装器+便携 zip）、扩展（Chrome+Firefox，预发布 tag 不打包扩展）、Linux（AppImage/deb/arch/tar.gz）、macOS（x64+arm64，DMG+便携）、Android（split-per-abi + universal APK，cargokit 编各 ABI cdylib）、Web SPA（一次复用）、server 多平台二进制（musl 静态）、server NAS 包（OpenWrt/QNAP/群晖）、CLI 六平台、server Docker（ghcr.io，QEMU arm64）。每个 release job 各用自己的组件 tag，跑 git-cliff（`--include-path <组件目录>`）后经 Claude Code CLI 翻译为中英双语（`<!-- fluxdown:lang:zh/en -->` 标记，失败回退原始 cliff）。

构建期 dart-define：`APP_VERSION`、`ANALYTICS_APP_KEY`、`FLUXCLOUD_BASE_URL`、`STATS_*`。

---

## 设计文档实现状态（`docs/`）

> ⚠️ `docs/` 在 `.gitignore` 里（零文件入库），下列设计文档**只存在于本机工作副本**；契约与不变式一律写回 `AGENTS.md` / 本目录，别只留在 `docs/`。

避免混淆——**已实现** vs **仅设计**：
- **已实现**：多文件任务组（`multi-file-task-group-design.md`）、插件系统 + 去中心化市场（`fluxdown-plugin-marketplace-plan.md` 等）。
- **部分实现（仅客户端）**：多设备协作 / FluxCloud 配置同步（`multi-device-collab-design.md`）——`lib/src/services/cloud/` + `web/src/lib/cloud/` 已落地，对接**外部 L2 relay**；**本地 headless server 无任何 cloud/sync 路由**；打洞/E2E 仍设计阶段。
- **仅设计（无引擎/服务器代码）**：浏览器扩展嗅探规则市场（`sniff-rule-market-design.md`——云端锚定 FluxCloud，扩展侧 `sniff-engine.ts` + FluxCloud `sniff_packs` 表均未落地；文档含三轮对抗评审记录与逐条打折清单）。
- **已实现（全端）**：RSS 订阅自动下载（`rss-subscription-design.md`，issue #97）——引擎 `native/engine/src/rss/`、REST `/api/v1/rss/*`、WS `rssSourcesChanged`/`rssItemsChanged`、hub 信号、桌面 UI（侧边栏区块 + 条目流 + 三 Tab 对话框 + 两步向导）、web SPA 同构、CLI `fluxdown rss`、MCP `rss_list`/`rss_add`/`rss_remove`。
- **已实现（免费层，全宿主）**：webhook 任务事件通知（`webhook-notification-design.md`）——引擎 `native/engine/src/webhook.rs`（6 事件 × 8 预设 + 占位符模板 + HMAC 签名 + 环形投递日志）、REST `/api/v1/webhooks/{deliveries,test,simulate}`、hub 信号、桌面「通知」设置分类、web SPA 同构。端点表就是 config 键 `webhook.endpoints`，桌面 / headless / CLI `--local` 共享。**付费托管 Relay（设计 §6）未实现**，客户端无任何 relay 代码。
- **命名歧义警告**：引擎里的 `tracker_subscription.rs` / `ed2k/server_subscription.rs` 指 **BT tracker 列表 / ED2K server.met 订阅**，与 `rss/` 的 feed 订阅是两回事；官网 `api/webhooks/github` 是 GitHub 接收器，与 `engine/src/webhook.rs` 的任务事件推送无关。
