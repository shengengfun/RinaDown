# FluxDown 收录物料：下载/P2P 垂直清单 + 中文开源社区渠道

> 调研时间：2026-07-27。所有「是否活跃」以 GitHub API 实测的最近一次 **merged PR** 或 issue 时间为准。
> 本文件只产出物料与规则，**不代表已向任何外部仓库提交过 PR / issue**。

## 0. 提交前必须先修的一处硬伤

`zerx-lab/FluxDown` 仓库的 **Homepage 字段填的是 `https://www.fluxdown.com`**，而 README.md:19 / README.zh-CN.md:19 里的官网全是 `https://fluxdown.zerx.dev`。
多数清单维护者会点仓库右上角的 Homepage 做核对，两个域名不一致会被当成可疑条目。**提交任何清单前先把 Homepage 统一成 `https://fluxdown.zerx.dev`**（或反过来，取一个作为唯一官网）。

本文件所有条目文本统一使用 `https://fluxdown.zerx.dev`。

---

## 1. 总览表

| 清单仓库 | star 量级 | 最近活跃（最近一次合并 PR） | FluxDown 是否够格 | 是否禁 AI-PR | 优先级 |
|---|---|---|---|---|---|
| [1c7/chinese-independent-developer](https://github.com/1c7/chinese-independent-developer) | 59.8k | 2026-07-26（#1203） | ✅ 够格 | 未见明文禁止 | **P0** |
| [521xueweihan/HelloGitHub](https://github.com/521xueweihan/HelloGitHub) | 167k | 月刊，2026-07 仍在更新（第 123 期） | ✅ 够格（走 issue 自荐） | 未见明文禁止；但明文「请勿使用复制的内容」 | **P0** |
| [ruanyf/weekly](https://github.com/ruanyf/weekly) | 98.1k | 2026-07-26（issue #10899，投稿走 issue 不走 PR） | ✅ 够格 | 未见明文禁止 | **P0** |
| [Axorax/awesome-free-apps](https://github.com/Axorax/awesome-free-apps) | 7.0k | 2026-07-24（#209） | ✅ 够格（Download Managers 分类） | 未见明文禁止 | **P1** |
| [fmhy/edit](https://github.com/fmhy/edit)（FMHY wiki） | 10.9k | 2026-07-17（#5813） | ✅ 够格（Download Managers 分类） | 未见明文禁止；但明文**要求走 issue 而非 PR** | **P1** |
| [DangJin/awesome-social-media-downloader](https://github.com/DangJin/awesome-social-media-downloader) | 1.8k | 2026-04-30（#3，历史上仅 1 个合并 PR） | ⚠️ 边缘（定位是社媒视频下载） | 无 CONTRIBUTING | **P2** |
| [yt-dlp/yt-dlp](https://github.com/yt-dlp/yt-dlp) `--downloader` | 130k+ | 活跃 | ❌ 不是清单，是代码贡献路径 | 明文要求「先开 issue 讨论，否则 PR 可能直接被拒」 | **P2** |
| [aria2/aria2](https://github.com/aria2/aria2) README.rst | 41.5k | — | ❌ **README 里根本没有 client/frontend 列表** | — | 不做 |
| [aria2/aria2 Wiki](https://github.com/aria2/aria2/wiki) | — | 唯一一页 Home，最后编辑 2020-04-18 | ❌ 已死 | — | 不做 |
| [aria2/aria2.github.io](https://github.com/aria2/aria2.github.io)（官网 UI Frontends / Related Projects） | 59 | **历史 merged PR = 0** | ❌ 事实拒收 | — | 不做 |
| [mayswind/AriaNg-WebSite](https://github.com/mayswind/AriaNg-WebSite)（3rd-extensions 页） | — | **历史 merged PR = 0** | ❌ 双重不够格（收录标准是「基于 AriaNg 的应用」） | — | 不做 |
| [mafintosh/awesome-p2p](https://github.com/mafintosh/awesome-p2p) | 311 | 内容为论文/协议资源，无软件列表 | ❌ 不够格 | — | 不做 |
| Motrix 生态清单 | — | **不存在** | ❌ 目标不存在 | — | 不做 |
| awesome-torrent 类 | — | **不存在可用的** | ❌ 目标不存在 | — | 不做 |
| [wotakumoe/wotaku](https://github.com/wotakumoe/wotaku) | 2.9k | 活跃 | ❌ ACG 垂直索引，通用下载器不对口 | — | 不做 |

> 分工说明：`coracoo/awesome_docker_cn`、`TWO-ICE/Awesome-NAS-Docker`、`awesome-selfhosted` 系列归 SelfhostedNasDocker；`jaywcjlove/awesome-mac` 等桌面 App 清单归 DesktopMobileApps。本文件不重复覆盖。

---

## 2. aria2 JSON-RPC 兼容度（代码证据）

这一节决定「FluxDown 能不能自称 aria2 兼容」。结论：**能，而且是全方法集覆盖，但有 5 个方法明确拒绝、4 个方法降级**。以下全部有代码出处，不夸大。

### 2.1 实现位置

| 文件 | 职责 |
|---|---|
| `FluxDown/native/api/src/jsonrpc.rs` | `POST /jsonrpc` 派发层（`dispatch_method`，L137–180） |
| `FluxDown/native/api/src/aria2.rs` | 纯函数映射层：GID 编解码、status 映射、选项翻译、响应拼装 |
| `FluxDown/native/api/src/jsonrpc_ws.rs` | `GET /jsonrpc` WebSocket 升级 + aria2 风格通知广播 |
| `FluxDown/native/api/src/server.rs` | 路由注册；`jsonrpc_enabled` **默认 true**（server.rs:69, :82–83） |
| `FluxDown/native/api/src/routes.rs` | `pub const JSONRPC: &str = "/jsonrpc"`（routes.rs:23） |
| `FluxDown/native/api/src/tests.rs` | 真实 TCP + `serve_on` 的黑盒集成测试（L660+ 为 aria2 端点专章） |

`aria2.rs:633` 的 `METHOD_NAMES` 常量注释写明：**顺序对齐 aria2 官方 `RpcMethodFactory.cc` 注册表，且必须与 `jsonrpc::dispatch_method` 的分支一一对应**。共 36 个方法。

### 2.2 真实实现（27 个）——经 `&dyn ApiHost` 落到引擎

来源 `jsonrpc.rs:140–165`：

```
aria2.addUri            aria2.addTorrent         aria2.remove          aria2.forceRemove
aria2.pause             aria2.forcePause         aria2.unpause         aria2.pauseAll
aria2.forcePauseAll     aria2.unpauseAll         aria2.tellStatus      aria2.tellActive
aria2.tellWaiting       aria2.tellStopped        aria2.getUris         aria2.getFiles
aria2.getOption         aria2.getGlobalOption    aria2.changeGlobalOption
aria2.getGlobalStat     aria2.purgeDownloadResult                       aria2.removeDownloadResult
aria2.getVersion        aria2.getSessionInfo     system.listMethods    system.listNotifications
system.multicall（在 dispatch_rpc_call 提前拦截，见 jsonrpc.rs:185 system_multicall，禁止嵌套）
```

### 2.3 降级但返回合法结果（4 个）——`jsonrpc.rs:168–169`

| 方法 | 返回 |
|---|---|
| `aria2.getPeers` | `[]` |
| `aria2.getServers` | `[]` |
| `aria2.saveSession` | `"OK"` |
| `aria2.changeOption` | `"OK"` |

### 2.4 明确拒绝（5 个）——`jsonrpc.rs:172–176`，返回 `code: 1` + aria2 风格文案

```
aria2.addMetalink   aria2.changePosition   aria2.changeUri
aria2.shutdown      aria2.forceShutdown
```

### 2.5 WebSocket 通知（6 个）——`aria2.rs:675` `NOTIFICATION_NAMES`

```
aria2.onDownloadStart   aria2.onDownloadPause     aria2.onDownloadStop
aria2.onDownloadComplete aria2.onDownloadError    aria2.onBtDownloadComplete
```

### 2.6 协议细节（可写进 PR 描述，全部有出处）

- `getVersion` 返回 `version: "1.37.0"`（`aria2.rs:624` `ARIA2_VERSION`），`enabledFeatures = ["Async DNS", "BitTorrent", "GZip", "HTTPS", "Message Digest"]`（`aria2.rs:628`）——**如实移除了不支持的 `Metalink`/`XML-RPC`/`Firefox3 Cookie`**。
- GID = `task_id`（UUID v4）去连字符小写后取前 16 位十六进制，无状态可重推导（`aria2.rs:23`）；支持唯一前缀反查（aria2 `GroupId::expandUnique` 语义，`aria2.rs:38`）。
- 鉴权：`X-FluxDown-Token` 头 或 aria2 约定的 `params[0] = "token:xxx"`（`jsonrpc.rs:73`、`strip_token_prefix` L128）。`system.listMethods` / `system.listNotifications` **不鉴权**，对齐 aria2 重写 `execute()` 的行为（`jsonrpc.rs:99`）。`system.multicall` 信封本身不鉴权，token 由每个子调用各自携带（`jsonrpc.rs:106`、L213–216）。
- 不校验 `Content-Type`（与真实 aria2 一致，兼容不带 `application/json` 头的脚本，`jsonrpc.rs:21`；回归测试见 `tests.rs:795`）。
- 错误模型：协议层保留 `-32700`/`-32600`；一旦路由到具体方法，**业务失败统一 `code: 1`**，不用 `-32601`/`-32602`（`jsonrpc.rs:28`）。
- 支持顶层 JSON 数组批量请求（`jsonrpc.rs:18`；测试 `tests.rs:728`）。
- **已知精度缺口（别在 PR 里回避）**：`tellStatus` 返回对象里 `connections` / `numPieces` / `pieceLength` / `uploadLength` **恒为 `"0"`**（`aria2.rs:111–114` 注释：引擎不暴露这些字段，如实反映不可用而非伪造非零值）。

### 2.7 对 aria2 生态收录的现实结论

| 目标 | 需要的兼容度 | 结论 |
|---|---|---|
| AriaNg 能连上并正常用 | addUri / tellStatus / tell{Active,Waiting,Stopped} / getGlobalStat / get·changeGlobalOption / getFiles / getUris / getVersion / system.multicall | **全部已实现** → 可以对接 |
| AriaNg 拖拽调序 | `changePosition` | **被拒绝** → 该功能在 FluxDown 后端上不可用 |
| AriaNg 查看 peers / 分片进度 | `getPeers` + `tellStatus.numPieces/connections` | **降级为空/零** → UI 会显示空 |
| AriaNg 一键关闭后端 | `shutdown` | **被拒绝**（设计如此，本地服务不允许远程关停） |

宣传口径建议：**"aria2 JSON-RPC 兼容层，覆盖官方 36 个方法全集，27 个真实实现，AriaNg / 『发送到 aria2』类脚本可直接对接"**——这句每个字都有代码撑着。不要写"完全兼容 aria2"。

---

## 3. 够格清单的可提交物料

### 3.1 【P0】1c7/chinese-independent-developer（中国独立开发者项目列表，59.8k★）

- **目标文件**：`README.md`（主版面。子版面 `pages/README-Programmer-Edition.md` 是「需要命令行或写代码」的产品；FluxDown 桌面端是打开即用的 GUI App，**走主版面**）
- **插入位置**：`## 3. 项目列表` 之下、现有最新日期小节 **之上**，新建一个当天日期的 `###` 小节。该列表按「添加日期倒序」排，不是字母序。
- **CONTRIBUTING 硬性要求**（来自 `README.md` 顶部 + `CONTRIBUTING.md`）：
  - 入选标准：**必须是网站或 App，不能是开发者工具或论坛型网站** → FluxDown 是桌面/移动 App，符合。
  - 无 star 门槛、无项目年龄门槛、无截图要求。
  - 状态 emoji 必须选一个：`:clock8:` 开发中 / `:white_check_mark:` 已上线 / `:x:` 已关闭。FluxDown 已发布多个 release → `:white_check_mark:`。
  - 介绍语公式：`[产品类型]，[核心价值或独特之处]`，**禁止写成 "AI 视频生成工具" 这种只有品类没有价值的空话**。
  - 城市名 / 博客 / 更多介绍均为可选。
- **提交入口**：https://github.com/1c7/chinese-independent-developer/pulls （PR 或 Issue 均可，实测 PR 从提交到合并 ~20 分钟到 4 小时）
- **可直接复制粘贴的条目文本**（严格照抄相邻条目排版）：

```markdown
### 2026 年 7 月 27 号添加

#### zerx-lab - [Github](https://github.com/zerx-lab)
* :white_check_mark: [FluxDown](https://fluxdown.zerx.dev)：多协议下载管理器，Rust 引擎 + IDM 式动态分段，支持 HTTP/FTP/BT 磁力/eD2K/HLS/DASH，浏览器扩展自动接管下载，Win/macOS/Linux/Android 全平台开源免费
```

> 日期小节按实际提交日改。若当天已存在同日期 `###` 小节，则**不新建小节**，直接在该小节末尾追加 `####` 作者块。
> 提交 PR 的标题照抄社区惯例：`添加 FluxDown` 或 `自荐:FluxDown(开源多协议下载管理器)`。

---

### 3.2 【P0】521xueweihan/HelloGitHub（167k★）

- **提交方式**：**不是 PR，是 issue**。仓库 `blank_issues_enabled: false`，只能走模板。
- **目标模板**：`.github/ISSUE_TEMPLATE/submit-cn.yaml`（title 前缀固定 `[开源推荐] `，自动 assign 给 `521xueweihan`）
- **提交入口**：https://github.com/521xueweihan/HelloGitHub/issues/new?template=submit-cn.yaml
  （网站入口同样可用：https://hellogithub.com/periodical ，见 `config.yml` 的 contact_links）
- **模板硬性要求（逐字段）**：

| 字段 | 必填 | 约束 |
|---|---|---|
| 项目地址 | ✅ | **仅收录 GitHub 项目** |
| 类别 | ✅ | 下拉单选，FluxDown 选 **`Rust`** |
| 项目标题 | ✅ | 约 20 字，**max_length: 50** |
| 项目描述 | ✅ | **min_length: 32，max_length: 256** |
| 亮点 | ✅ | 「类比同类型项目有什么特点」 |
| 示例代码 | ❌ | 可选 |
| 截图或演示视频 | ❌ | 可选，但强烈建议附（下载器是视觉型产品） |

- **额外审核标准**（模板尾部指向的 [issue #271](https://github.com/521xueweihan/HelloGitHub/issues/271)，2018 年开帖，2026-03 仍在被引用）：
  - 文档必须含：项目介绍（必须）、特性（必须）、快速开始（必须）、版权/协议（必须）；ChangeLog 对**自荐项目可选**。
    → 实测 `FluxDown/README.md` 已有 `## Highlights`(L27) / `## Features`(L37) / `## Installation`(L65) / `## License`(L214)，**四项必须全部满足**。
  - 自荐项目 star 数是**可选**参考项（FluxDown 现 1002★，够用）。
  - Insights 活跃度是**必选**参考项（自荐也逃不掉）→ 提交前保证近 30 天有稳定 commit。
  - 「同类型项目数量过多则不予推荐」→ **亮点字段必须打出差异化**，别写成又一个下载器。
  - **请勿使用复制的内容作为项目描述**（照搬 README 会被判定复制）。
  - 提交前先在 https://hellogithub.com 搜 `FluxDown` 确认未被收录过。
- **可直接复制粘贴的字段内容**：

**项目标题**（27 字，未超 50）：
```
Rust 写的开源多协议下载管理器，IDM 的免费替代品
```

**项目描述**（172 字，落在 32–256 区间；重写而非复制 README）：
```
FluxDown 用 Rust + Tokio 写下载引擎、Flutter 做界面，把 HTTP/FTP、BitTorrent 磁力、eD2K/Kad、HLS 与 DASH 收进同一个任务队列。它做 IDM 式动态分段：不是开固定线程数，而是在下载过程中把还没下完的区间继续切开分给空闲连接，慢速节点不会拖住整条任务。装上浏览器扩展后网页里的下载会被自动接管，断点续传落在 SQLite WAL 上，杀进程也不丢进度。
```

**亮点**：
```
1. 分段策略是 IDM 那套「运行时再切分」，不是常见的 aria2 式固定 -x N 连接，慢节点不拖累整体；
2. 引擎是零 FFI 的纯 Rust crate（native/engine），Flutter 通过 Rinf signals 通信，不走 dart:ffi 也不起子进程；
3. 内置 aria2 JSON-RPC 兼容层，覆盖官方 36 个方法全集、27 个真实实现，AriaNg 和各种「发送到 aria2」油猴脚本可以直接对接；
4. 内置 MCP server（Streamable HTTP，12 个工具），Claude Desktop / Cursor 这类 AI 客户端能直接管理下载任务；
5. 同一套代码跑桌面（Win/macOS/Linux）、Android 和 headless 服务端（fluxdown_server + Web UI，支持群晖 spk / QNAP qpkg / OpenWrt ipk / Unraid / CasaOS）。
```

**截图**：直接拖 3 张 —— 主界面（含分段进度条）、亮/暗主题对比、AriaNg 连上 FluxDown 的截图（第 3 张是最有说服力的差异化证据）。

---

### 3.3 【P0】ruanyf/weekly 科技爱好者周刊（98.1k★）

- **提交方式**：**issue，不是 PR**。README 首屏原话：「欢迎投稿文章/软件/资源，请[提交 issue]」。
- **模板**：**没有 issue 模板**（实测 `https://github.com/ruanyf/weekly/tree/master/.github` 返回 404，仓库根本没有 `.github` 目录）→ 空白 issue，格式靠社区惯例。
- **提交入口**：https://github.com/ruanyf/weekly/issues/new
- **硬性要求**：无 star 门槛、无年龄门槛。周刊**每周五发布**，周一到周三投递被当期收录的概率最高。
- **标题格式**（实测 2026-07-25/26 的 12 条投稿，压倒性使用这两种前缀）：
  - 开源项目 → `【开源自荐】<项目名>：<一句话价值>`
  - 闭源/工具 → `【工具自荐】<项目名>：<一句话价值>`
  FluxDown 是 AGPL-3.0 开源 → 用 `【开源自荐】`。
- **可直接复制粘贴的 issue 标题**：

```
【开源自荐】FluxDown：Rust 引擎的多协议下载器，IDM 式动态分段 + aria2 RPC 兼容
```

- **可直接复制粘贴的 issue 正文**：

```markdown
仓库：https://github.com/zerx-lab/FluxDown
官网：https://fluxdown.zerx.dev
协议：AGPL-3.0

FluxDown 是一个多协议下载管理器，Rust + Tokio 写引擎、Flutter 做界面，目标是做 IDM 的开源替代。

**它和常见下载器不一样的地方：**

- **分段是运行时切的**。不是启动时按 `-x 16` 开固定连接，而是在下载过程中把剩余区间继续切开丢给空闲连接 —— 这是 IDM 的做法，效果是单个慢节点不会拖住整条任务的收尾。
- **协议摊得比较开**：HTTP/HTTPS、FTP、BitTorrent（DHT/磁力）、eD2K/Kad、HLS（含 AES 解密）、DASH，都在同一个任务队列里，不用换软件。
- **内置 aria2 JSON-RPC 兼容层**。覆盖 aria2 官方 36 个方法全集，其中 27 个是真实映射到引擎的实现（另有 4 个降级返回、5 个明确拒绝，比如 changePosition 和 shutdown）。所以 AriaNg 和各种「发送到 aria2」的油猴脚本可以直接把 FluxDown 当后端用。
- **内置 MCP server**（Streamable HTTP，`POST /mcp`，12 个工具），Claude Desktop / Cursor 这类 AI 客户端可以直接管理下载任务。默认只监听 127.0.0.1 并要求管理 token。
- **断点续传落在 SQLite WAL 上**，进程被杀也不丢进度。
- 浏览器扩展（Chrome/Edge/Firefox）接管网页下载。

**平台**：Windows (x64/ARM64)、macOS (Intel/Apple Silicon)、Linux (AppImage/deb/Arch/tar.gz)、Android (按 ABI 分包)。
另有 headless 服务端 `fluxdown_server`（Web UI 端口 17800，镜像 `ghcr.io/zerx-lab/fluxdown-server`，amd64 + arm64 多架构），支持群晖 spk / QNAP qpkg / OpenWrt ipk / Unraid / CasaOS。
```

---

### 3.4 【P1】Axorax/awesome-free-apps（7.0k★，最近合并 2026-07-24）

- **目标文件**：`README.md`（桌面端）；Android 版本另投 `MOBILE.md`。**禁止改 `filter/` 目录和目录树 TOC，那些是自动生成的。**
- **目标章节**：`## Download Managers`（README.md:448）
- **插入位置**：**该分类的最底部**（contributing.md 明文：「should be added to the **bottom of the specific category**」，且「Do not change the order of any apps like ordering alphabetically」）。当前该分类最后一条是 `Free Download Manager`（L451），追加在它后面。
- **CONTRIBUTING 硬性要求**：
  - 描述必须简洁、说明功能、**以句号结尾**。
  - 平台 emoji：🪟 Windows / 🍎 macOS / 🐧 Linux；🟢 = 开源（**必须是指向仓库的链接**，写成 `[🟢](repo-url)`）；⭐ = 「Recommended by us」，**自荐时绝对不要自己加**。
  - 无 star 门槛 / 无年龄门槛 / 无截图要求 / 无演示站要求。
  - Commit message 必须是 `Add: FluxDown`（`Add:` / `Update:` / `Remove:` 三选一）。
- **提交入口**：PR → https://github.com/Axorax/awesome-free-apps/pulls ；仅提建议 → https://github.com/Axorax/awesome-free-apps/issues/new?template=Blank+issue
- **README.md 可直接复制粘贴的条目文本**：

```markdown
- [FluxDown](https://fluxdown.zerx.dev) - Multi-protocol download manager with IDM-style dynamic segmentation, supporting HTTP, FTP, BitTorrent, eD2K, HLS and DASH. 🪟 🍎 🐧 [🟢](https://github.com/zerx-lab/FluxDown)
```

- **MOBILE.md 可直接复制粘贴的条目文本**（目标章节 `## Download Managers`，MOBILE.md:258；插入位置为该分类最底部，当前最后一条是 `MyJDownloader`，MOBILE.md:264。MOBILE.md 图标体系已实测：🤖 Android / 🍎 iOS / 🟢 开源 / ⭐ 推荐，且 `[🟢](repo)` 的链接写法在该文件中确有使用，见 MOBILE.md:244–245、:347）：

```markdown
- [FluxDown](https://fluxdown.zerx.dev) - Multi-protocol download manager with IDM-style dynamic segmentation. 🤖 [🟢](https://github.com/zerx-lab/FluxDown)
```

---

### 3.5 【P1】fmhy/edit（FMHY wiki，10.9k★，最近合并 2026-07-17）

- **目标文件**：`docs/file-tools.md`
- **目标章节**：`## ▷ Download Managers`（file-tools.md:20）
- **插入位置**：该分区按「从好到坏」排序，**不是字母序**。自荐条目老实放在无 ⭐ 组的末尾 —— 当前该组最后一条是 `HTTP Downloader`（L41），追加在它之后、分隔线 `***`（L43）之前。
- **CONTRIBUTING 硬性要求**（`.github/CONTRIBUTING.md`）：
  - **明文优先要求走 issue 而不是 PR**：「Note that we have to check sites ourselves, so using an issue, rather than pull request is easier.」→ **应该开 issue，不要直接开 PR**。
  - 提交前必须先在 https://fmhy.net/single-page.md 用 `ctrl+f` 搜 `FluxDown` 确认未收录。
  - 不收：付费 / 仅试用产品（FluxDown 免费开源，✅）、非英文软件（除非口碑极好；FluxDown 有英文 README + 英文 UI，✅）。
  - 所有新增条目要先过 Discord 上的测试流程；**大改动（重构章节等）必须先在 Discord 讨论**——单条新增不算大改动。
  - **⭐ 是他们自己评的星标，自荐时不要加粗、不要加 ⭐。**
  - 无 star 门槛 / 无年龄门槛。
- **提交入口**：Issue → https://github.com/fmhy/edit/issues/new ；（Discord 每周五开放邀请，见 https://github.com/fmhy/FMHY/wiki/FMHY-Discord）
- **可直接复制粘贴的条目文本**（严格照抄同分区非星标条目的 `名称 - 类型 / 平台 / 链接 / GitHub` 排版）：

```markdown
* [FluxDown](https://fluxdown.zerx.dev/) - Download Manager / [Firefox](https://addons.mozilla.org/firefox/addon/fluxdown) / [Chrome](https://chromewebstore.google.com/detail/fluxdown/meleenglfggcmcajknpeeeiobnpfmahc) / [Edge](https://microsoftedge.microsoft.com/addons/detail/fluxdown/nglkkjbogjghekbhhcnccnpfedjbdhhd) / Windows, macOS, Linux, Android / [GitHub](https://github.com/zerx-lab/FluxDown)
```

---

### 3.6 【P2】DangJin/awesome-social-media-downloader（1.8k★）

- **诚实评估**：这个清单的定位是「免费下载油管、B 站、抖音等平台视频的工具」，主体是站点抽取器（lux / BBDown / downkyi / N_m3u8DL-CLI）。FluxDown **没有站点抽取器**，只有 HLS/DASH 直链下载能力。**唯一的先例是 gopeed —— 一个同样没有站点抽取器的通用下载管理器已被收录在「最受欢迎的」表里**，所以并非完全不够格，但被拒的概率不低。
- **活跃度警告**：仓库虽然 2026-07-23 有更新，但**历史上只有 1 个合并的外部 PR**（#3，2026-04-30，从提交到合并花了 2 个月）。维护者主要自己改。**期望值放低**。
- **无 CONTRIBUTING 文件**，格式只能照抄现有条目。
- **目标文件 / 章节**：`README.md` → `## 🌟 最受欢迎的` 的 2 列表格（gopeed 就在这张表里）。
- **插入位置**：表格最后一行 `wechatVideoDownload` 那行的右半格当前是空的（`||` 结尾），填进去即可，不用新增行。
- **提交入口**：https://github.com/DangJin/awesome-social-media-downloader/pulls
- **可直接复制粘贴的表格单元格文本**（socialify 图片 + star badge 的排版与相邻单元格完全一致）：

```markdown
![FluxDown](https://socialify.git.ci/zerx-lab/FluxDown/image?description=1&forks=1&issues=1&language=1&name=1&owner=1&pulls=1&stargazers=1&theme=Light)![img](https://img.shields.io/github/stars/zerx-lab/FluxDown?label=Star)</br>[FluxDown](https://github.com/zerx-lab/FluxDown)
```

即把最后一行改成：

```markdown
|![wechatVideoDownload](https://socialify.git.ci/qiye45/wechatVideoDownload/image?description=1&forks=1&issues=1&language=1&name=1&owner=1&pulls=1&stargazers=1&theme=Light)![img](https://img.shields.io/github/stars/qiye45/wechatVideoDownload?label=Star)</br>[wechatVideoDownload](https://github.com/qiye45/wechatVideoDownload)|![FluxDown](https://socialify.git.ci/zerx-lab/FluxDown/image?description=1&forks=1&issues=1&language=1&name=1&owner=1&pulls=1&stargazers=1&theme=Light)![img](https://img.shields.io/github/stars/zerx-lab/FluxDown?label=Star)</br>[FluxDown](https://github.com/zerx-lab/FluxDown)|
```

---

### 3.7 【P2】yt-dlp `--downloader` —— 这不是清单收录，是代码贡献

- **现状（实测）**：`--downloader` 的可选值硬编码在 `yt_dlp/downloader/external.py` 的 `ExternalFD` 子类里，注册表是 L570 的 `_BY_NAME`（按 `globals()` 里所有以 `FD` 结尾、且不是 `ExternalFD`/`FragmentFD` 的类自动收集）。当前子类只有 6 个：
  `CurlFD`(L192) / `AxelFD`(L258) / `WgetFD`(L273) / `Aria2cFD`(L298) / `HttpieFD`(L349) / `FFmpegFD`(L369)。
  README.md:622 的帮助文本相应写死为 `Currently supports native, aria2c, axel, curl, ffmpeg, httpie, wget`。
- **结论**：**没有任何「外部下载器清单」文件可以提 PR 加一行**。想被 yt-dlp 支持，只能提交一个 `FluxDownFD(ExternalFD)` 类的代码 PR。
- **硬门槛**（`CONTRIBUTING.md` L208–210，原文）：
  > "Before you start writing code for implementing a new feature, open an issue explaining your feature request and at least one use case. ... If you open a pull request for a new feature without discussing with us first, do not be surprised when we ask for large changes to the code, or even reject it outright."
  → **必须先开 feature-request issue 讨论，直接提 PR 有被直接拒的明文风险。**
- **技术前提（本仓库已具备的部分）**：
  - CLI 二进制名是 `fluxdown`（`FluxDown/native/cli/Cargo.toml` 的 `[[bin]] name = "fluxdown"`）；
  - CLI 已对齐 aria2c 的退出码语义（`FluxDown/native/cli/src/exit.rs:1`「对齐 aria2c 的退出码语义（`man aria2c` EXIT STATUS）」），这是 `ExternalFD` 判断成败所依赖的东西；
  - `--pause`、`-i` 批量等参数行为已按 aria2 语义实现（`native/cli/src/main.rs:116`、L488）。
- **建议排期**：低。ROI 远不如 P0/P1 三家，而且需要一个能通过 yt-dlp review 的 Python PR + 长期维护承诺。**先不动，只在这里记录路径，避免以后重复调研。**
- **入口**（真要做时）：https://github.com/yt-dlp/yt-dlp/issues/new/choose → 选 feature request。

---

## 4. 不够格 / 已死 / 拒收清单（记录原因，避免重复调研）

| 目标 | 判定 | 证据 |
|---|---|---|
| **aria2/aria2 README.rst 的「client 列表」** | **不存在** | 完整读过 568 行 README.rst：只有 Disclaimer / Introduction / Features / Versioning / How to get source / Dependency / How to build / Cross-compiling / BitTorrent / Metalink / Metalink-HTTP / netrc / WebSocket / libaria2 / References。**全文没有任何 client、frontend、GUI 列表章节。** |
| **aria2 GitHub Wiki** | **已死** | https://github.com/aria2/aria2/wiki 只有一个 `Home` 页，内容是「Welcome to the aria2 wiki!」，最后编辑 **2020-04-18，共 1 次修订**。 |
| **aria2 官网 UI Frontends / Related Projects** | **事实拒收** | 列表确实存在于 `aria2/aria2.github.io` 的 `index.html:156`（Related Projects：apt-metalink / powerpill / python3-aria2jsonrpc / aria2.js）与 `index.html:165`（UI Frontends：webui-aria2 / uGet）。但该仓库 **历史 merged PR = 0**（GitHub 搜索 `repo:aria2/aria2.github.io is:merged` 返回 0 条），外部贡献进不去。且 FluxDown 不是 aria2 的前端也不是 aria2 的周边库，语义上也不属于这两个分类。 |
| **AriaNg 兼容客户端列表** | **双重不够格** | AriaNg README 指向的是 http://ariang.mayswind.net/3rd-extensions.html，源文件为 `mayswind/AriaNg-WebSite` 的 `3rd-extensions.html`。① 该页收录标准是 **「基于 AriaNg 的第三方应用」**（Server-side app with aria2 / Client-side app with aria2 / Browser add-ons，全部是打包或封装了 AriaNg 的项目），FluxDown 不包含 AriaNg；② `mayswind/AriaNg-WebSite` **历史 merged PR = 0**。 |
| **mafintosh/awesome-p2p** (311★) | **不够格** | 完整读过 README.md：全文只有 4 个分区（Protocols / Data integrity / DHT / Connectivity），内容全是**论文和博客文章链接**（Kademlia 论文、PPSP RFC、Scuttlebutt 论文等），**没有任何软件/客户端条目**。且 README 写明「curated by me, @mafintosh」。 |
| **awesome-torrent 类清单** | **不存在可用的** | GitHub 全站搜 `awesome-torrent` / `awesome-bittorrent` / `torrent awesome list`，能找到的只有 `awesomeguides/awesome-torrent`（**2★，最后更新 2023-12-27**）和 `torrust/awesome-legal-torrent-sources`（**2★**，且收录对象是「分发合法内容的种子站点」不是客户端）。**该垂直方向不存在有影响力的活跃清单。** |
| **Motrix 生态清单** | **不存在** | 搜 `motrix in:name` 的 10 个结果全是应用本体或衍生品（agalwood/Motrix 52.3k、AnInsomniacy/motrix-next 9.0k、gautamkrishnar/motrix-webextension 1.8k、Taoister39/tauri-motrix、ShawnRn/MotrixMac 等），**没有任何 awesome/收录型仓库**。Motrix 本体 README 也没有生态列表章节。 |
| **wotakumoe/wotaku** (2.9k★) | **不对口** | `docs/tools.md` 里确有下载器表格（JDownloader 等），但整站定位是 ACG/动漫资源索引（topics: anime, manga, vtuber, doujin…）。通用下载管理器混进去说服力弱，且容易被打上盗版邻近标签。**放弃。** |
| **fmhy 的 PR 通道** | **降级为 issue** | 不是拒收，但 CONTRIBUTING 明文要求优先开 issue（他们要自己测），直接开 PR 反而拖慢。已在 §3.5 记录。 |

---

## 5. 非 GitHub 中文渠道（只列入口，不产出物料）

这些渠道的收录规则不可控（人工审核 / 社区氛围 / 反广告规则各异），本节只记录入口和硬性红线，不预写文案。

| 渠道 | 入口 | 硬性注意 |
|---|---|---|
| **V2EX · 分享创造** | https://www.v2ex.com/go/create | 需 V2EX 账号且有一定活跃度；**新号发广告帖大概率被降权或删帖**。节点选 `分享创造`，不要发到 `推广`。标题不要带下载站味道。 |
| **少数派 Matrix** | https://sspai.com/matrix | 投稿是**写文章**，不是发链接。软文会被退稿；要写成「我为什么重写了一个下载器 / IDM 式分段是怎么做的」这类技术叙事。审核周期以天计。 |
| **HelloGitHub 官网直投** | https://hellogithub.com/periodical | 与 §3.2 的 GitHub issue 是同一条流水线（`config.yml` 的 contact_links 指向官网），**二选一，不要重复提交**。 |
| **开源中国 OSCHINA 软件收录** | https://www.oschina.net/project/create | 需实名账号；收录后有独立项目页，能吃到一部分中文搜索流量。 |
| **Linux.do** | https://linux.do/ | 需邀请/信任等级；社区对自荐相对宽容，但要求正文有干货。 |
| **吾爱破解 · 原创发布区** | https://www.52pojie.cn/forum-16-1.html | 流量大但**红线也最硬**：FluxDown 是 AGPL-3.0 开源且不含破解内容，符合发布区规则；但绝对不要在帖子里提「下载付费资源」「破解 IDM」这类表述。 |
| **B 站 / 小红书** | — | 演示视频比文字有效得多（动态分段进度条 + AriaNg 对接是天然的视觉素材）。不属于「收录」，属于内容营销，另行规划。 |

---

## 6. 建议执行顺序

1. **先修 §0 的 Homepage 不一致**，否则后面每一处提交都带着一个可疑点。
2. **P0 三家并行**：chinese-independent-developer（PR，当天可合）→ ruanyf/weekly（issue，周一至周三投，周五出刊）→ HelloGitHub（issue，月刊，28 号发布，提前 1–2 周投）。
3. **P1 两家**：Axorax/awesome-free-apps（PR，`Add: FluxDown`）→ fmhy/edit（**issue，不是 PR**）。
4. P2 两项按 ROI 判断，`yt-dlp` 建议直接跳过。

---

## 7. 提交记录（2026-07-27 已执行）

以 `zerx-lab` 账号经 gh CLI 实际提交。外部仓库均走 fork（`$TEMP/fd-promo/<repo>`），FluxDown 主仓库未被改动。

| # | 目标 | 形式 | 链接 | 改动量 | 状态 |
|---|---|---|---|---|---|
| 1 | 1c7/chinese-independent-developer | PR | https://github.com/1c7/chinese-independent-developer/pull/1204 | +3/-0 | OPEN / MERGEABLE |
| 2 | 521xueweihan/HelloGitHub | Issue | https://github.com/521xueweihan/HelloGitHub/issues/3491 | — | OPEN |
| 3 | ruanyf/weekly | Issue | https://github.com/ruanyf/weekly/issues/10901 | — | OPEN |
| 4 | Axorax/awesome-free-apps | PR | https://github.com/Axorax/awesome-free-apps/pull/213 | +2/-0（README.md + MOBILE.md） | OPEN / MERGEABLE |
| 5 | fmhy/edit | Issue | https://github.com/fmhy/edit/issues/5890 | — | OPEN |
| 6 | DangJin/awesome-social-media-downloader | PR | https://github.com/DangJin/awesome-social-media-downloader/pull/10 | +1/-1（填空单元格，不新增行） | OPEN / MERGEABLE |

**未提交**：yt-dlp（需先开 feature-request issue 讨论再写 `FluxDownFD` 代码 PR，成本高，留作后续）。

### 提交时遵守的口径

- 统一写「兼容 aria2 JSON-RPC，可直连 AriaNg」+「36 个方法全集 / 27 个真实实现」，**未出现「完全兼容」**。
- MCP 工具数按 `native/api/src/mcp.rs` 测试 `tools_list_returns_every_tool`（`assert_eq!(tools.len(), 12)`）写作 **12 个**。
- 链接统一 `https://fluxdown.zerx.dev`。
- 中文清单用中文正文，英文清单用英文正文；无 AI 署名、无 Co-authored-by。
- Axorax 的 commit message 严格用 `Add: FluxDown`；两条目均追加在各自分类底部，未重排、未动 TOC 与 `filter/`。
- fmhy 按其 CONTRIBUTING 走 issue（`wiki.yml` 模板，Type = Site suggestion），并在正文声明了开发者身份、请其自行测试。
- DangJin 与 fmhy 的正文均**主动声明 FluxDown 没有站点抽取器**，未把它包装成视频解析工具。

### 提交前已完成的查重

- HelloGitHub：`repo:521xueweihan/HelloGitHub FluxDown` 的 issue 与 code 搜索均 0 命中。
- ruanyf/weekly：`repo:ruanyf/weekly FluxDown` issue 搜索 0 命中。
- fmhy：`https://api.fmhy.net/single-page` 全文搜索 `FluxDown` 0 命中。

### 待办

- **§0 的 Homepage 不一致仍未修**：仓库 Homepage 字段还是 `https://www.fluxdown.com`，而所有提交出去的物料写的都是 `https://fluxdown.zerx.dev`。维护者核对时会看到两个域名，**应尽快统一**。
- 关注 6 条的回复；HelloGitHub 月刊 28 号发布，本次赶在发布前一天投递，可能落到下一期。
