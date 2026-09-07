# FluxDown 推广渠道提交物料

本目录是各推广/生态渠道的**可提交实物**，已按各平台官方规范核实生成。
`lists/` 下是「被 GitHub 清单收录」的调研结果与逐条可粘贴物料，本文件是总索引与待办。

## 资格校验（已核实的事实）

| 项 | 结论 | 依据 |
|---|---|---|
| 仓库公开 | ✅ `zerx-lab/FluxDown`（public，1002★/51 fork） | GitHub API |
| 开源协议 | ✅ AGPL-3.0（SPDX `AGPL-3.0`） | `LICENSE` |
| 首个 tag | ✅ v0.0.1 @ 2026-02-10 | git tag |
| 首个 GitHub **Release** | ⚠️ 2026-07-03（仅 24 天）——见「阻塞项 B」 | GitHub Releases API |
| headless Web UI | ✅ `fluxdown_server`，端口 17800 | `docker/docker-compose.yml` |
| 公共镜像 | ✅ `ghcr.io/zerx-lab/fluxdown-server`，**多架构 amd64 + arm64** | `release.yml:1678`；GHCR manifest list 实测 |
| aria2 JSON-RPC | ✅ 覆盖官方 36 方法全集，27 个真实实现 | `native/api/src/aria2.rs:633-670`、`jsonrpc.rs:137-180` |
| MCP 端点 | ✅ `POST /mcp`，12 个工具，默认仅 `127.0.0.1` + token | `native/api/src/mcp.rs` |
| crates.io | ❌ 无 `fluxdown` crate（影响 awesome-rust 条目模板） | crates.io API |

> **架构更正**：早期文档写「镜像仅 linux/amd64」是错的。commit `14a9fc6`(2026-07-10) 起
> `platforms: linux/amd64,linux/arm64`，实测 `latest`/`0.1.59`/`0.2.0`/`0.2.2`/`0.2.3`/`0.2.5-rc.2`
> 全部含 arm64。唯一缺失架构是 `linux/arm/v7`(armhf)，本轮调研的清单无一将其列为硬门槛。

---

## 提交记录（2026-07-27）

一轮共 **23 项**：19 个 PR + 4 个 issue。截至 2026-07-28 复查：**9 合并 + 1 issue 已完成 + 14 仍 open**。

| 渠道 | 编号 | 状态 |
|---|---|---|
| pcqpcq/open-source-android-apps | [#448](https://github.com/pcqpcq/open-source-android-apps/pull/448) | ✅ 已合并（07-27） |
| rust-unofficial/awesome-rust | [#2672](https://github.com/rust-unofficial/awesome-rust/pull/2672) | ✅ 已合并（07-27） |
| TaKO8Ki/awesome-alternatives-in-rust | [#148](https://github.com/TaKO8Ki/awesome-alternatives-in-rust/pull/148) | open |
| Solido/awesome-flutter | [#1055](https://github.com/Solido/awesome-flutter/pull/1055) | open（只改 source.md） |
| fluttergems/awesome-open-source-flutter-apps | [issue #777](https://github.com/fluttergems/awesome-open-source-flutter-apps/issues/777) | open（Step 1，等分类答复后补 PR） |
| jaywcjlove/awesome-mac | [#2419](https://github.com/jaywcjlove/awesome-mac/pull/2419) | open（EN/zh/ja/ko 四文件） |
| offa/android-foss | [#704](https://github.com/offa/android-foss/pull/704) | ✅ 已合并（07-27） |
| iCHAIT/awesome-macOS | [#950](https://github.com/iCHAIT/awesome-macOS/pull/950) | open |
| thechampagne/awesome-windows | [#38](https://github.com/thechampagne/awesome-windows/pull/38) | ✅ 已合并（07-27） |
| 0PandaDEV/awesome-windows | [#236](https://github.com/0PandaDEV/awesome-windows/pull/236) | open |
| awesome-soft/awesome-windows | [#8](https://github.com/awesome-soft/awesome-windows/pull/8) | open |
| DimitrisPa/Awesome-Linux-Software | [#1](https://github.com/DimitrisPa/Awesome-Linux-Software/pull/1) | open |
| themeselection/best-chrome-extensions | [#57](https://github.com/themeselection/best-chrome-extensions/pull/57) | open |
| AwesomeHomelab/awesome-homelab | [#107](https://github.com/AwesomeHomelab/awesome-homelab/pull/107) | open |
| lissy93/portainer-templates | [#123](https://github.com/lissy93/portainer-templates/pull/123) | ✅ 已合并（07-27） |
| selfhosters/unRAID-CA-templates | [#686](https://github.com/selfhosters/unRAID-CA-templates/pull/686) | ✅ 已合并（07-27） |
| TensorBlock/awesome-mcp-servers | [#1420](https://github.com/TensorBlock/awesome-mcp-servers/pull/1420) | open |
| punkpeye/awesome-mcp-servers | [#9304](https://github.com/punkpeye/awesome-mcp-servers/pull/9304) | ✅ 2026-07-13 已合并 |
| 1c7/chinese-independent-developer | [#1204](https://github.com/1c7/chinese-independent-developer/pull/1204) | ✅ 已合并（07-27） |
| Axorax/awesome-free-apps | [#213](https://github.com/Axorax/awesome-free-apps/pull/213) | ✅ 已合并（07-27） |
| DangJin/awesome-social-media-downloader | [#10](https://github.com/DangJin/awesome-social-media-downloader/pull/10) | open |
| 521xueweihan/HelloGitHub | [issue #3491](https://github.com/521xueweihan/HelloGitHub/issues/3491) | open |
| ruanyf/weekly | [issue #10901](https://github.com/ruanyf/weekly/issues/10901) | open |
| fmhy/edit | [issue #5890](https://github.com/fmhy/edit/issues/5890) | ✅ closed/completed（07-27） |

同步落地的仓库改动：新增 `glama.json`；`native/api/src/mcp.rs` 12 个工具及参数描述改写为英文（`cargo check -p fluxdown_api` 通过）；README/README.zh-CN/`promotion/mcp/server.json` 的「9 个工具」订正为 12 并补全 rss_* 三工具。

### 合并后回挂（2026-07-28）

已在 `README.md` / `README.zh-CN.md` 完成：

1. MCP 徽章换成 Glama 官方 score 徽章（对齐 punkpeye 条目）
2. 渠道 featured 徽章回挂时去掉 `Awesome MCP`（与上方 Glama 重复）；其余已合并渠道保留
3. 在有维护者互动的 PR 下回复致谢：`offa/android-foss#704`、`lissy93/portainer-templates#123`、`1c7/chinese-independent-developer#1204`

其余已合并但无维护者评论的 PR（awesome-rust / awesome-windows / free-apps / unRAID / open-source-android-apps / punkpeye 已回过）不再刷评论。

**仍待项目侧后续**：

- `offa/android-foss#704`：上架 F-Droid / IzzyOnDroid 后补商店徽章 follow-up PR
- Unraid CA 网页提交：`https://ca.unraid.net/submit`（模板仓库已合，但 CA 目录另需填表）

### 未提交及原因

| 渠道 | 原因 |
|---|---|
| awesome-selfhosted-data | 冷却至 2026-11-03（见阻塞项 B） |
| IceWhaleTech/CasaOS-AppStore | 只收整个 store 源，FluxDown 尚无已发布 store（前置见阻塞项 C） |
| FabioLolix/Awesome-Linux-Software | 与 DimitrisPa fork 内容重复，等 #1 有回音再定，避免像刷曝光 |
| mcpservers.org / mcp.so / pulsemcp / Glama 用量 | 纯网页表单，gh 提交不了，需你本人操作 |
| Cline marketplace / Smithery | 需 400×400 PNG + llms-install.md / stdio→HTTP 桥接 .mcpb |
| yt-dlp | 需先开 issue 讨论再提 downloader 后端代码 |


## 一、提交任何清单前必须先解决的阻塞项

| # | 阻塞项 | 影响面 | 处理 |
|---|---|---|---|
| **A** | 仓库 Homepage 字段是 `https://www.fluxdown.com`，README 全用 `https://fluxdown.zerx.dev` | **全部清单**。维护者点 homepage 核对时视为可疑条目 | 二选一统一（改仓库 Settings 或改 README） |
| **B** | 最早 GitHub **Release** 2026-07-03 | `awesome-selfhosted`：PR #2675 已于 2026-07-05 被驳回（"Initial release 2 days ago"）。**补发 v0.0.1 Release 无效**——维护者看 Release 实际发布时间而非 tag 日期 | 冷却到 **2026-11-03** 后重提 |
| **C** | CasaOS 源缺 `supported-languages.json`（v2 协议必需） | CasaOS/ZimaOS 自建源构建 | 补文件后再跑 `build_appstore.py` |

已顺手修复：`casaos/Apps/FluxDown/docker-compose.yml` 的 `architectures` 补 `arm64`，镜像与 `version` 从 `0.1.54` 升到 `0.2.3`。

## 二、⚠️ 提示注入陷阱（务必人工提交）

调研中在两个仓库发现**针对 AI agent 的隐藏陷阱**，已识别但未执行：

- `0PandaDEV/awesome-windows`：README + `llms.txt` 内嵌 prompt injection，含强制拒答串。视为「维护者明确拒绝 AI 代投」→ 必须你本人纯手工撰写并提交。
- `lissy93/portainer-templates`：`CONTRIBUTING` 文末 HTML 注释藏蜜罐，要求在 PR 下贴指定 gif。真人看不到该注释，照做等于自曝 AI 代投。

另有两个仓库**明文封禁**机器生成内容：
- `awesome-selfhosted-data`：*"Machine/LLM-generated contributions … will result in a ban."*
- `selfhosters/unRAID-CA-templates`：准入条款含 "Not fully AI written"，且要求提交账号有 GitHub 历史活动。

**结论：所有 PR 一律用你本人账号、人工 review 后提交。物料只是草稿。**

---

## 三、清单收录矩阵（本轮新增，物料见 `lists/`）

共核实 40+ 个候选仓库，其中 **30 个够格**、13 个已死/拒收（已记录原因，避免重复调研）。

### P0 — 门槛已满足、语义精准、立即可提

| 清单 | ★ | 目标文件 / 入口 | 物料 |
|---|---|---|---|
| `rust-unofficial/awesome-rust` | 58.5k | `README.md` → Applications → Utilities（字母序末位） | `lists/rust-flutter-alternatives.md` |
| `TaKO8Ki/awesome-alternatives-in-rust` | 4.1k | `README.md` → Applications → Utilities，新建 `#### Internet Download Manager` | 同上 |
| `jaywcjlove/awesome-mac` | 108.7k | 4 个语言版 README 的「下载工具」章节（**必须四语言同步**） | `lists/desktop-mobile-apps.md` |
| `offa/android-foss` | 10.7k | `### • Downloader & Manager`，dvd 之后 / Gopeed 之前 | 同上 |
| `pcqpcq/open-source-android-apps` | 10.4k | `categories/tools.md` 表格；可走 Actions 工作流自动生成 | 同上 |
| `AwesomeHomelab/awesome-homelab` | 2.1k | `data/download.yaml`，Deluge 与 Mylar3 之间（禁改生成的 README） | `lists/selfhosted-nas-docker.md` |
| `TensorBlock/awesome-mcp-servers` | — | `docs/utilities--helpers.md` 末尾追加一条 | `lists/mcp-ai.md` |
| Glama 资料完善度 75%→100% | — | 加 `glama.json`、补 12 个工具描述 | 同上 |
| `1c7/chinese-independent-developer` | 59.8k | `README.md` 主版面，按日期倒序新建小节 | `lists/p2p-and-china.md` |
| `521xueweihan/HelloGitHub` | 167k | issue 模板 `submit-cn.yaml`，类别选 Rust | 同上 |
| `ruanyf/weekly` | 98.1k | 空白 issue，标题前缀【开源自荐】 | 同上 |
| `awesome-selfhosted-data` | 308k | `software/fluxdown.yml` | 已备；**卡阻塞项 B** |

### P1 — 够格但有流程摩擦

`fluttergems/awesome-open-source-flutter-apps`（须先开 issue 再 PR）、`Solido/awesome-flutter`（只能改 `source.md`，描述里禁出现 "Flutter"）、`DimitrisPa/Awesome-Linux-Software`、`iCHAIT/awesome-macOS`、`0PandaDEV/awesome-windows`（见陷阱）、`thechampagne/awesome-windows`、`lissy93/portainer-templates`（见陷阱）、CasaOS 官方第三方源推荐页、`bigbeartechworld/big-bear-casaos`、Unraid CA 自有模板仓库、`Axorax/awesome-free-apps`、`fmhy/edit`（走 issue 非 PR）、mcpservers.org / mcp.so / pulsemcp 表单。

### P2 — 边缘或需先做前置工程

`FabioLolix/Awesome-Linux-Software`、`themeselection/best-chrome-extensions`、`awesome-soft/awesome-windows`、`truenas/apps`（需 app.yaml+questions.yaml+Jinja2+CI）、`TWO-ICE/Awesome-NAS-Docker` 与 `coracoo/awesome_docker_cn`（硬要求配套教程文章）、`cline/mcp-marketplace`（需 llms-install.md + 400×400 PNG）、Smithery（需 stdio→HTTP bridge 打成 .mcpb）、`DangJin/awesome-social-media-downloader`。

### 确定拒收 / 已死（勿再尝试）

`veggiemonk/awesome-docker`（结构性排除，其示例直接把此类项目导向 awesome-selfhosted）、`RunaCapital/awesome-oss-alternatives`（只收 SaaS 替代品）、`GorvGoyl/Clone-Wars`（2024 停更 + 强制 Demo 站）、`fluttergems/fluttergems`（只收 pub 包）、`Awesome-Windows/Awesome`（404）、`luong-komorebi/Awesome-Linux-Software`（2026-05 归档）、`modelcontextprotocol/servers` community 章节（官方声明不再收）、`modelcontextprotocol/registry`（拒 127.0.0.1 remote）、`docker/mcp-registry`（明文拒 GPL 系，AGPL 出局）、`wong2`/`appcypher` 的 awesome-mcp-servers（前者"We do not accept PRs"，后者 PR 通道关闭）、`aria2` 官方仓库/Wiki/官网（无 client 列表或历史合并 PR = 0）、`mafintosh/awesome-p2p`（只收论文）、`IceWhaleTech/Awesome-CasaOS`、`hotheadhacker/awesome-selfhost-docker`（均停更 1 年+）、`portainer/templates`（官方自用，master 已 deprecated）、linuxserver.io（收录=交出镜像所有权）。

---

## 四、既有渠道进展

| 渠道 | 物料 | 状态 | 你要做 |
|---|---|---|---|
| CasaOS / ZimaOS | `casaos/` | 物料就绪（已升 0.2.3 + arm64） | 补 `supported-languages.json`；建自有源仓库 / 提官方 PR + 截图 |
| Unraid CA | 独立仓库 `zerx-lab/unraid-templates` | ✅ 已推送并修完两处扫描错误 | 到 https://ca.unraid.net/submit 填仓库地址提交；建议 Icon 换 256×256 PNG |
| MCP `punkpeye/awesome-mcp-servers` | PR #9304 | ✅ **已于 2026-07-13 合并**（README 第 3691 行） | 无 |
| awesome-selfhosted | `awesome-selfhosted/fluxdown.yml` | 物料就绪（desc 222 字符，tag 与上游逐字匹配） | 先解阻塞项 B，再**本人手动**提 PR |
| MCP 官方 Registry | — | ❌ 不适用（拒 localhost remote，实测确认） | 无 |

### CasaOS 提交两种方式
- **自建第三方商店（推荐，自己掌控）**：把 `casaos/` 放到公开仓库/分支，按 `store-config.json` 配好，
  用官方 `build_appstore.py` + Actions 构建到 gh-pages。用户「应用商店 → 添加来源」填 URL 即可一键装。
- **进官方商店**：Fork `IceWhaleTech/CasaOS-AppStore`，拷入 `Apps/FluxDown/`，本地跑
  `python3 scripts/build_appstore.py` 验证后提 PR（附安装成功 + WebUI 可达截图）。

发新版时同步更新 `docker-compose.yml` 的 `image` 版本号与 `x-casaos.version`/`update_at`。

## 五、非清单类引流（另一条线）

- **yt-dlp `--external-downloader`**：复刻 aria2c 的引流路径。但 yt-dlp 明文要求新功能先开 issue 讨论，
  且需提交 downloader 后端代码，成本高——建议先写对接文档而非改 yt-dlp。
- **AriaNg / 「发送到 aria2」油猴脚本**：已可直接对接（27 个真实方法）。
  诚实边界：`getPeers`/`getServers` 返回空数组，`changePosition` 拒绝，
  `tellStatus` 的 `connections`/`numPieces`/`pieceLength`/`uploadLength` 恒为 `"0"`
  → 宣传写「兼容 aria2 JSON-RPC，可直连 AriaNg」，**不要写「完全兼容」**。
- **winget / Scoop / Homebrew Cask**：桌面版包管理器上架，同时是若干清单的隐性加分项。
