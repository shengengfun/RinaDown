# MCP / AI 工具收录渠道调研（FluxDown）

> 核实时间：**2026-07-27**。所有结论均来自当日直读目标仓库的 `README.md` / `CONTRIBUTING.md`、
> GitHub API（`gh api`，非缓存）、或目标站点 HTML。未能取证的项一律标注「未核实」。
> 本文件只产出物料与规则，**不代为向任何外部仓库/表单提交**。

## FluxDown 的关键约束（决定所有渠道的资格）

FluxDown 的 MCP 是**内嵌在应用里、由用户各自本地自托管的端点**：

- 传输：Streamable HTTP，`POST http://127.0.0.1:17800/mcp`（默认仅监听回环）
- 鉴权：`Authorization: Bearer <admin token>`，与管理 API 共用
- 工具：**12 个**（`download_add/list/get/pause/resume/pause_all/resume_all/remove`、`queue_list`、
  `rss_list/rss_add/rss_remove`）。2026-07-27 已同步订正 `README.md` / `README.zh-CN.md` /
  `promotion/README.md` / `promotion/mcp/server.json` 中过时的「9 个」。
- 实现：`native/api/src/mcp.rs`
- **不是** stdio 服务器，**不是** 公网可达的 remote endpoint，**没有** npm/PyPI 可安装包

因此渠道筛选的第一道闸门永远是：**它接不接受"本地端点 / 用户自托管"的 MCP server**。
官方 MCP Registry 已实测拒收 `127.0.0.1` remote（见下方「已排除」）。

---

## 汇总表

| 渠道 | star / 规模 | 最近活跃（当日实测） | 接受本地端点 MCP | FluxDown 够格 | 禁 AI-PR | 优先级 |
|---|---:|---|---|---|---|---|
| [punkpeye/awesome-mcp-servers](https://github.com/punkpeye/awesome-mcp-servers) | 91,430 ★ | push 2026-07-25 | ✅ 有 🏠 Local Service 图例 | ✅ **已收录** | ❌ 明确欢迎（`🤖🤖🤖` 快速通道） | ✅ 完成 |
| [Glama](https://glama.ai/mcp/servers/zerx-lab/FluxDown) | 目录站 | 持续 | ✅ 页面打 `Local` 标签 | ✅ **已收录**（75% 完成度） | — | **P0（补完资料）** |
| [TensorBlock/awesome-mcp-servers](https://github.com/TensorBlock/awesome-mcp-servers) | 790 ★ / 7,747 条目 | PR #1393 merged 2026-07-25 | ✅ 元数据字段含 `stdio/sse/streamable-http`，公网端点为可选 | ✅ | ❌ 无禁令（自家 bot 也发 PR） | **P0** |
| [mcpservers.org](https://mcpservers.org/submit)（= wong2 仓库唯一入口） | 4,227 ★ | push 2026-07-13 | ✅ 未设公网端点门槛 | ✅ | 纯网页表单，无 PR | **P1** |
| [mcp.so](https://mcp.so/submit) | ~2,031 ★（站点源码 chatmcp/mcpso） | 站点源码 2026-07-26 | ✅ 提交类型区分 "MCP Server" / "Remote Server" | ✅（选 MCP Server） | 纯网页表单，无 PR | **P1** |
| [PulseMCP](https://www.pulsemcp.com/submit) | 22,250+ 条目 | 每日抓取 | ✅ 已收录大量本地自托管栈（如 `arrstack`、`mediabox`） | ✅ | 纯网页表单，无 PR | **P1** |
| [cline/mcp-marketplace](https://github.com/cline/mcp-marketplace) | 785 ★ | issue #2099 closed=completed 2026-07-24 | ✅ 全是本地服务器 | ⚠️ 需先写 `llms-install.md` | Issue 表单提交 | **P2（有前置）** |
| [Smithery](https://smithery.ai/new) | 目录站 | 持续 | ⚠️ 仅两条路：公网 HTTPS URL / 本地 **stdio** MCPB 包 | ⚠️ 需做 stdio 桥接 | 需登录后台 | **P2（有前置）** |
| [appcypher/awesome-mcp-servers](https://github.com/appcypher/awesome-mcp-servers) | 5,718 ★ | push **2026-05-06**；Issues 已关闭；`/pulls` 返回 404 | ✅（分类里有 Local 类服务器） | ✅ 内容够格 | 无禁令 | **P2（提交通道当前关闭）** |
| [modelcontextprotocol/servers](https://github.com/modelcontextprotocol/servers) README | 88,920 ★ | push 2026-07-26 | — | ❌ **章节已废弃** | — | ✖ 排除 |
| [modelcontextprotocol/registry](https://github.com/modelcontextprotocol/registry) | 官方 | 持续 | ❌ 拒收 `127.0.0.1` remote | ❌ | — | ✖ 排除 |
| [docker/mcp-registry](https://github.com/docker/mcp-registry) | 530 ★ | push 2026-07-26 | ✅ 有 Local(容器化) 类型 | ❌ **AGPL 被明文排除** | — | ✖ 排除 |

---

## 0. punkpeye/awesome-mcp-servers —— PR #9304 状态：**已合并**

据实记录（读 `pr://punkpeye/awesome-mcp-servers/9304` + 直读线上 README 双重确认）：

| 项 | 事实 |
|---|---|
| 状态 | **MERGED** |
| 创建 | 2026-07-05T06:55:59Z |
| 合并 | 2026-07-13T10:22:52Z（机器人 welcome-comment 确认 "Your server has been merged"） |
| 标签 | `has-emoji`、`valid-name`、`has-glama` |
| 线上位置 | `README.md` 第 **3691** 行（共 3743 行），在 `ZeparHyfar/mcp-datetime` 与 `zueai/mcp-manager` 之间 |

合并过程中新增的**硬性门槛**（对以后任何人提这个清单都适用）：
机器人 + 维护者 punkpeye 两次要求「必须先在 Glama 上架并在条目里挂 Glama score 徽章」，
补上徽章后当天合并。所以 **Glama 上架是 punkpeye 清单的事实前置条件**，已满足。

线上条目现文（供其他清单复用措辞）：

```markdown
- [zerx-lab/FluxDown](https://github.com/zerx-lab/FluxDown) [![zerx-lab/FluxDown MCP server](https://glama.ai/mcp/servers/zerx-lab/FluxDown/badges/score.svg)](https://glama.ai/mcp/servers/zerx-lab/FluxDown) 🦀 🏠 🪟 - Control the FluxDown multi-protocol download manager (HTTP/FTP/BitTorrent/HLS): add, list, pause, resume, remove downloads and manage queues.
```

**此渠道无需再做任何事。**

---

## 1. Glama —— 已收录，但资料只完成 75%（P0，收益最高）

**接受本地端点：✅ 明确**。FluxDown 页面被打上 `Rust` + **`Local`** 标签，分类 `App Automation`。
这是全网对「本地自托管 MCP」最友好的目录，且是 punkpeye 清单的上游依赖。

- 页面：https://glama.ai/mcp/servers/zerx-lab/FluxDown
- 评分页：https://glama.ai/mcp/servers/zerx-lab/FluxDown/score
- 当前实测状态：**Profile completion 75%**；Server Coherence **A**；Tool Definition Quality **B**
  （9/9 工具平均 3.5/5，最低 2.8/5）；Maintenance **B**；License **A**（AGPL 3.0 被判定为 permissive）；
  Author verified ✅；Has a Glama release ✅（v0.2.2）

**扣分项与对应动作（全部可自查、无需外部审批）：**

| 扣分项 | 原文 | 动作 |
|---|---|---|
| No `glama.json` | "Add a glama.json file to provide metadata about your server." | 在 FluxDown 仓库根目录加下方文件 |
| No recent usage | "No tool usage detected in the last 30 days." | 用服务器页的 **Try in Browser** 跑几次工具，播种使用量 |
| No related servers | "Add related servers to improve discoverability." | 在页面上关联同类服务器（下载/媒体类） |
| Tool Definition Quality B | 平均 3.5/5，最低 2.8/5 | 改 `native/api/src/mcp.rs` 里各工具的 `description` 与参数说明 |
| Naming Consistency 4/5 | "queue_list 没有遵循 `download_` 前缀模式" | 仅记录；改名是破坏性变更，**不建议**为分数改 |

**可直接落地的 `glama.json`**（放 FluxDown 仓库根目录，schema 为当日实测拉取）：

```json
{
  "$schema": "https://glama.ai/mcp/schemas/server.json",
  "maintainers": ["zerx-lab"]
}
```

附带建议（非阻塞）：`FluxDown/README.md` 第 17 行现在挂的是自制 shields 徽章
（`img.shields.io/badge/MCP-Glama-...`），可换成 Glama 官方 score 徽章，与 punkpeye 条目一致：

```markdown
[![MCP Server](https://glama.ai/mcp/servers/zerx-lab/FluxDown/badges/score.svg)](https://glama.ai/mcp/servers/zerx-lab/FluxDown)
```

---

## 2. TensorBlock/awesome-mcp-servers —— 走 GitHub PR（P0）

**接受本地端点：✅**。其元数据规范明确把 `stdio` / `sse` / `streamable-http` 并列为合法 transport，
"public endpoint" 属于 *when available* 的可选字段，没有公网可达要求。

**活跃度实测**：`pushed_at = 2026-07-26T09:46:43Z`；最近合并 PR #1393 / #1391 / #1389 均为
2026-07-25 合并；仓库自带 `tensorblock-mcp-automation[bot]` 持续开清理 PR。**明确活跃。**

**禁 AI-PR：否**。README 全文无任何禁止 AI/机器生成贡献的措辞；相反，其 issue 表单会
「automation drafts a PR」，自家 bot 就是 PR 作者。

### 提交规则（原文摘录）

> To add a new MCP server:
> 1. Pick the best category from Browse by Category.
> 2. Open that category page under `docs/`.
> 3. Add one markdown bullet using this format:
>    `- [Server Name](https://github.com/owner/repo): Brief description of what the MCP server lets an agent do. Install: \`npx your-package\`.`
> 4. Search the repo for your URL or project name to avoid duplicates.
> 5. Open a pull request.

- **目标文件**：`docs/utilities--helpers.md`
- **插入位置**：**文件末尾追加一行**。已实测该文件（361 行）**不是字母序**，条目按提交时间堆叠，
  最后一条是 `jakobautomation/agentsvc-mcp`。不要试图插入字母序位置。
- **排版**：`- [owner/repo](url): 描述。` —— 注意分隔符是**冒号 + 空格**（`): `），
  不是 punkpeye 那种 ` - `。近期条目全部用冒号式。
- **去重自查**：已实测 `utilities--helpers.md`、`operating-system--command-line.md`、
  `multimedia-processing.md`、`filesystems.md`、`developer-productivity--utilities.md`
  中均无 `aria2` / `download manager` / `bittorrent` / `yt-dlp` 条目，无重复风险。
- **无 star 门槛 / 年龄门槛 / 截图要求。**
- 若不想直接开 PR，也可用 [Add MCP server issue 表单](https://github.com/TensorBlock/awesome-mcp-servers/issues/new?template=add-mcp-server.yml)，
  自动化会替你起草 PR。

### 可直接复制粘贴的条目文本

追加到 `docs/utilities--helpers.md` 最后一行之后：

```markdown
- [zerx-lab/FluxDown](https://github.com/zerx-lab/FluxDown): Control the FluxDown multi-protocol download manager from an agent — add tasks from HTTP/HTTPS, FTP, magnet or BitTorrent sources, then list, inspect, pause, resume and remove them and browse named queues. Transport: `streamable-http` at `http://127.0.0.1:17800/mcp`. Auth: bearer token. Install: run the FluxDown desktop app, or the headless server image `ghcr.io/zerx-lab/fluxdown-server`, then enable the MCP endpoint in settings.
```

**分类选择理由**：`Utilities & Helpers`（"simple, general-purpose tools"）是唯一贴合的桶。
已排除：`Multimedia Processing`（限于生成/转换媒体，不含下载）、
`Operating System & Command Line`（限于 shell/系统信息）、
`Filesystems`（限于读写/管理已有文件）、`Infrastructure`（限于 MCP 自身的代理/网关）。
备选桶为 `docs/filesystems.md`，仅在维护者要求改分类时使用。

**提交入口**：https://github.com/TensorBlock/awesome-mcp-servers/compare （fork 后开 PR）
**PR 标题建议**：`Add zerx-lab/FluxDown to Utilities & Helpers`（匹配已合并 PR 的标题习惯，
如 #1391 `Add ThomasCrouzet/icloud-mcp to Project and Task Management`）

---

## 3. mcpservers.org —— wong2 清单的唯一入口（P1，纯表单）

**wong2/awesome-mcp-servers 不接受 PR。** README 顶部原文：

> [!NOTE]
> We do not accept PRs. Please submit your MCP on the website: https://mcpservers.org/submit

所以「wong2 仓库」和「mcpservers.org」是同一个渠道，不要重复投。

**接受本地端点：✅**（表单无公网 URL / 端点可达性字段，只要 GitHub 或文档链接）。

**提交入口**：https://mcpservers.org/submit
**费用**：免费（排队审核、无徽章、随机排序、nofollow）；可选 **$39 一次性** Premium（免排队、官方徽章、dofollow）。

表单字段与建议填写值：

| 字段 | 填写内容 |
|---|---|
| Server Name | `FluxDown` |
| Short Description | `Control the FluxDown multi-protocol download manager (HTTP/FTP/BitTorrent/HLS) from AI agents — add, list, pause, resume and remove downloads and manage queues over a local Streamable HTTP MCP endpoint.` |
| Link (GitHub or docs) | `https://github.com/zerx-lab/FluxDown` |
| Category | 下拉里没有"下载/媒体"，可选项为 Development / Productivity / Database / Search / Web Scraping / File System / Version Control / Communication / Cloud Service / Cloud Storage / Marketing / Finance / Design / Memory / Other → **选 `File System`**（次选 `Productivity`） |
| Contact Email | 你本人的邮箱 |

---

## 4. mcp.so —— 纯表单（P1）

**没有 GitHub PR 通道。** `chatmcp/mcpso` 仓库（2,031 ★）只是站点源码（Next.js + Supabase），
README 全文只讲怎么本地跑站点，不含条目提交流程；条目数据在 Supabase 里，不在仓库里。

**接受本地端点：✅**。表单顶部就把提交类型分成 **MCP Server / Remote Server / MCP Client** 三类
——"Remote Server" 才是公网端点那一类，FluxDown 选 **MCP Server**。

**提交入口**：https://mcp.so/submit?type=server
**费用**：免费（排队审核、无徽章、随机排序、nofollow）；或 **$39 一次性**（免审核立即发布、Verified 徽章、featured 排序、dofollow）。
**必填**：Repository URL → `https://github.com/zerx-lab/FluxDown`（Name 可留空自动抓取）。

---

## 5. PulseMCP —— 纯表单（P1）

**接受本地端点：✅ 已取证**。站内检索 `qbittorrent` 命中 `gh-juancmpdev-mediabox`、
`ct4nk3r-arrstack` 等纯本地自托管栈的 MCP server，说明本地服务不被排斥。目录规模 22,250+。

**提交入口**：https://www.pulsemcp.com/submit → 选 **MCP Server**
**唯一必填**：URL，提示语原文 "Can be a GitHub repository, a subfolder of a repository, or a standalone website."
→ 填 `https://github.com/zerx-lab/FluxDown`
**免费**，无 star/年龄门槛。

注意：PulseMCP 首选数据源是官方 MCP Registry（"We ingest entries from the Official MCP Registry
daily and process them weekly"）。FluxDown 进不了官方 registry，**必须走这个手工表单**。

---

## 6. cline/mcp-marketplace —— Issue 表单提交，有前置（P2）

**接受本地端点：✅**（整个 marketplace 都是本地服务器，Cline 会 clone 并在本机跑起来）。

**活跃度实测**：仓库 `pushed_at` 是 2025-06-24（数据不在仓库里），但 **issue 队列是活的**——
提交 issue 每天都有新增（#2117~#2120 均为 2026-07-26/27），且近期有
`closed / state_reason=completed` 的收录（#2099 于 2026-07-24 完成）。

**问题所在（前置项）**：Cline 的一键安装模型是「把你的 README 丢给 Cline，让它自动 clone + 配置」。
FluxDown 的 MCP 不能单独 clone 运行，必须先装整个 FluxDown 应用或起 headless 容器。
README 原文要求：

> Confirm that you have tested giving Cline just your `README.md` and/or the `llms-install.md`
> and watched him successfully setup the server.

**因此提交前必须在 FluxDown 仓库根目录加一个 `llms-install.md`**，内容至少覆盖：
用 `docker run ghcr.io/zerx-lab/fluxdown-server` 起服务（镜像为 amd64 + arm64 多架构）→
从首次启动日志里取 admin token → 把 `http://127.0.0.1:17800/mcp` + `Authorization: Bearer <token>`
写进客户端 `mcpServers` 配置。并亲自用 Cline 跑通一遍。

**其他硬性要求**：
- **400×400 PNG logo**（现在仓库里 MCP/Glama 用的是 SVG，需另出 PNG）
- 说明理由（Reason for Addition）
- 审核维度：社区采用度、维护者可信度、项目成熟度、安全性。README 明确说 **不要求 star 数**
  （"Can I submit new MCP servers without many stars? **Absolutely!**"）

**提交入口**：https://github.com/cline/mcp-marketplace/issues/new?template=mcp-server-submission.yml

---

## 7. Smithery —— 两条路都需要改造（P2）

**接受本地端点：⚠️ 有条件**。实测 https://smithery.ai/docs/build/publish.md 只有两个发布 Tab：

| 路径 | 要求 | FluxDown 是否满足 |
|---|---|---|
| **URL**（Bring your own hosting） | "Streamable HTTP transport" + **public HTTPS URL**；Smithery 会用 `SmitheryBot/1.0` 从 Cloudflare Workers 主动扫描你的端点 | ❌ `127.0.0.1` 不可达 |
| **Local (MCPB Bundle)** | 明文限定 "**For local stdio servers**"，需上传预构建 `.mcpb` 包 | ⚠️ FluxDown 是 HTTP 不是 stdio |

**结论**：想上 Smithery，唯一现实路径是**做一个 stdio→HTTP 的桥接进程并打成 `.mcpb` 包**：
一个极薄的 stdio MCP 代理，把 JSON-RPC 转发到用户本机 `http://127.0.0.1:17800/mcp`，
token 通过 MCPB 的 config schema 让用户填。这是一次性的工程投入，不是文案工作。

另有 **Uplink**（https://smithery.ai/docs/use/uplink.md）："Expose a local MCP server as a Smithery
connection without deploying it" —— 但那是**终端用户自己打隧道**的功能，不构成目录收录，
不能替代上架。

**提交入口**：https://smithery.ai/new（需 GitHub/Google 登录 WorkOS）
**优先级**：P2。在 stdio 桥接做出来之前不要投。

---

## 8. appcypher/awesome-mcp-servers —— 内容够格，但提交通道当前是关的（P2）

**接受本地端点：✅**（`System Automation`、`File Systems` 等分类下大量本地服务器）。
**禁 AI-PR：否**（`CONTRIBUTING.md` 全文无相关措辞，也无 `LLM-generated` / `AI-generated` / `machine-generated` 字样）。

**但当日实测该仓库无法接收贡献**（多方交叉验证，均在 2026-07-27）：

| 探针 | 结果 |
|---|---|
| `gh api repos/appcypher/awesome-mcp-servers/pulls` | **HTTP 404 Not Found** |
| `gh pr list --repo appcypher/awesome-mcp-servers --state all` | 返回空 |
| `github search_prs`（is:merged / is:open） | 0 结果 |
| 仓库元数据 | `has_issues: false`，`open_issues_count: 531` |
| `pushed_at` | **2026-05-06T08:04:35Z**（近 3 个月无提交） |
| 最近提交 | 2026-05-06 维护者直推；再往前是 2025-09-04 的 PR 合并（#165/#166/#178） |
| 对照组 | 同样命令对 `TensorBlock/awesome-mcp-servers` 正常返回 PR，排除 gh 认证/工具故障 |

即：**Issues 已关，PR 端点 404，仓库自 2026-05 起停更**。`open_issues_count: 531` 是关闭功能前
积压的旧 PR 计数。[推测] 大概率是维护者被 MCP 条目灌水（含 AI 批量 PR）压垮后关闭了贡献入口，
但这一点**未取证**，不要当事实引用。

**处置**：**先不要动**。加个季度回访（探针：`gh api repos/appcypher/awesome-mcp-servers/pulls`
不再 404 即为恢复）。条目文本已备好，恢复当天即可提。

### 备用条目文本（通道恢复后使用）

- **目标文件**：`README.md`
- **章节**：`## 🤖 <a name="system-automation"></a>System Automation`
  （章节定义："Tools for shell access, system control, and task automation. Enables AI models to
  execute commands and interact with the operating system."）
- **插入位置**：该章节**最后一条之后**（现末条为 `Apple Shortcuts`）。
  `CONTRIBUTING.md` 原文 "Link additions should be added to the bottom of the relevant category"，
  虽同时写了 "Make sure your list is ordered alphabetically"，但实测该章节现状**并非字母序**，
  以 bottom-append 为准。
- **排版**：每行 `- <img src="..." height="14"/> [Name](url) - Description`，图标 14px，描述不带句号。

```markdown
- <img src="https://cdn.simpleicons.org/rust/CE422B" height="14"/> [FluxDown](https://github.com/zerx-lab/FluxDown) - Multi-protocol download manager (HTTP/FTP/BitTorrent/HLS) exposing a local Streamable HTTP MCP endpoint to add, list, pause, resume and remove downloads and manage queues
```

其他要求：一个 PR 只提一条、PR 标题要有意义、检查拼写、去尾随空格、先搜重复。无 star/年龄/截图门槛。
**提交入口**（恢复后）：https://github.com/appcypher/awesome-mcp-servers/compare

---

## 已排除的渠道（不要再调研，附拒收依据）

### ✖ modelcontextprotocol/servers README 的 community servers 章节 —— **章节已废弃**

当日直读线上 `README.md`（171 行）：只剩 `## 🌟 Reference Servers`（7 个官方参考实现）
和 `### Archived`，**已无任何 Official Integrations / Community Servers 章节**。
`CONTRIBUTING.md` 原文：

> The README no longer contains a list of third-party MCP servers — that list has been retired in
> favor of the [MCP Server Registry](https://github.com/modelcontextprotocol/registry). To make your
> server discoverable, follow the quickstart guide to publish it there.
>
> We don't accept: **New server implementations** — We encourage you to publish them to the MCP Server Registry instead.

→ 这个渠道已 100% 重定向到官方 registry，而 registry 恰恰拒收 FluxDown。**死路，不要提 PR。**

### ✖ modelcontextprotocol/registry（官方 registry）—— 已实测拒收

`remotes` 类型要求公网可达 URL，`mcp-publisher validate` 实测拒绝 `http://127.0.0.1:17800/mcp`；
`packages` 类型要求发布 npm/PyPI/NuGet 等可安装包，FluxDown 没有。
（此结论沿用 `FluxDown/promotion/README.md` 已记录的实测，本次未重跑。）
`FluxDown/promotion/mcp/server.json` 保留作官方 MCP 描述文档，**不是** registry 提交物。

### ✖ docker/mcp-registry（Docker MCP Catalog / Docker Desktop MCP Toolkit）—— 协议不合格

`CONTRIBUTING.md` 的 PR 流程第一条原文：

> Make sure that the license of your MCP Server allows people to consume it.
> (MIT or Apache 2 are great, **GPL is not**).

FluxDown 是 **AGPL-3.0**，被这一条直接排除。技术上本来是匹配的（它明确支持
"🏠 Local Servers (Containerized)"，FluxDown 也有 `ghcr.io/zerx-lab/fluxdown-server` 多架构镜像），
**唯一的阻断项就是协议**。除非改协议——不建议——否则不要投。

### ✖ wong2/awesome-mcp-servers 直接提 PR —— 明文不接受

见第 3 节，README 顶部 NOTE 明确 "We do not accept PRs"。已折算成 mcpservers.org 表单渠道。

---

## 行动清单（按投入产出比）

| # | 动作 | 谁做 | 阻断项 |
|---|---|---|---|
| 1 | ✅ **已完成** 2026-07-27：`FluxDown/glama.json` 已创建（$schema + maintainers:["zerx-lab"]） | 已代做 | — |
| 2 | Glama 页面用 Try in Browser 播种使用量 + 关联相似服务器 | **你本人**（需登录 Glama） | 需账号 |
| 3 | ✅ **已完成** 2026-07-27：`native/api/src/mcp.rs` 12 个工具的 description 与参数说明全部改写为英文，覆盖「做什么 + 参数语义 + 返回什么」；`cargo check -p fluxdown_api` 通过，`mcp::tests::tools_list_returns_every_tool` 通过 | 已代做 | — |
| 4 | ✅ **已提交** 2026-07-27：PR https://github.com/TensorBlock/awesome-mcp-servers/pull/1420 | 已代做 | 等审核 |
| 5 | mcpservers.org / mcp.so / PulseMCP 三个表单各填一次（均免费） | **你本人**（需邮箱/人机验证） | 无 |
| 6 | 出 400×400 PNG logo + 写 `llms-install.md` + 用 Cline 跑通 → 提 Cline issue | 可代做前两项 | 需你本人验证 |
| 7 | 做 stdio→HTTP 桥接并打 `.mcpb` 包 → Smithery | 工程任务 | 需先立项 |
| 8 | 季度回访 appcypher 是否重开 PR | — | 通道关闭 |

> 说明：第 4、5、6 项均**未发现禁 AI 贡献的规则**，但本次调研按约束**只产出物料、不代为提交**。
