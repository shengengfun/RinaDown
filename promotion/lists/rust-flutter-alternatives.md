# 语言生态 + 开源替代品类清单 — 收录调研

调研日期：2026-07-27。所有结论均直接读取目标仓库 `README.md` / `CONTRIBUTING.md` / `contributing.md` / PR 模板原文，并用 `gh api` 查询 star 数与「最近一次合并 PR」时间核实活跃度。

**FluxDown 事实基线（已核实）**

| 项 | 值 | 依据 |
|---|---|---|
| 仓库 | `zerx-lab/FluxDown` | — |
| Star | **1002**（fork 51） | `gh api repos/zerx-lab/FluxDown` |
| License | AGPL-3.0 | 同上 |
| 官网 | https://fluxdown.zerx.dev（README 第 19 行的 Website 链接） | `FluxDown/README.md:19` |
| GitHub 仓库 homepage 字段 | `https://www.fluxdown.com` | `gh api` — 两个域名均实测 HTTP 200 |
| crates.io | **无 crate**（`crates.io/api/v1/crates/fluxdown` → "crate `fluxdown` does not exist"） | 实测 |
| 定位 | Rust + Tokio 引擎 / Flutter UI 的多协议下载管理器，开源 IDM 替代 | `FluxDown/README.md:9`、`:29-35` |
| aria2 JSON-RPC 兼容 | **真实存在**：`native/api/src/aria2.rs:633-670` 的 `METHOD_NAMES` 共 36 项（33 个 `aria2.*` + `system.multicall/listMethods/listNotifications`），`native/api/src/jsonrpc.rs:139-159` 逐条分发，另有 WS 通知帧 | 已读源码核实，可安全宣称「aria2-compatible JSON-RPC」 |

---

## 总览表

| 清单仓库 | star 量级 | 最近活跃（最近一次合并 PR） | FluxDown 是否够格 | 是否禁 AI-PR | 优先级 |
|---|---|---|---|---|---|
| [rust-unofficial/awesome-rust](https://github.com/rust-unofficial/awesome-rust) | 58.5k | **2026-07-25**（PR #2668 `feat: add gaze`） | ✅ 够格（1002 star > 50 门槛） | 全文未见任何 AI/机器生成 PR 的禁令 | **P0** |
| [TaKO8Ki/awesome-alternatives-in-rust](https://github.com/TaKO8Ki/awesome-alternatives-in-rust) | 4.1k | **2026-07-13**（PR #146 `Add pgrust`） | ✅ 够格（无 star 门槛，定位就是「Rust 写的现有软件替代品」） | 未见禁令 | **P0** |
| [fluttergems/awesome-open-source-flutter-apps](https://github.com/fluttergems/awesome-open-source-flutter-apps) | 3.0k | 2026-05-01（PR #757） | ✅ 够格（唯一硬门槛「3 年内有更新」满足） | 未见禁令 | **P1** |
| [Solido/awesome-flutter](https://github.com/Solido/awesome-flutter) | 60.7k | 2026-04-21（PR #999/#1001/#1010，批量合并） | ✅ 够格（35 star 门槛，FluxDown 1002） | 未见禁令 | **P1** |
| [RunaCapital/awesome-oss-alternatives](https://github.com/RunaCapital/awesome-oss-alternatives) | 19.4k | 2025-09-03（PR #243 `Add OpenStatus`），134 open issue 积压 | ❌ **不够格**（收录标准第 3 条要求「private for-profit company」，且对标必须是 SaaS） | 未见禁令 | 不提交 |
| [GorvGoyl/Clone-Wars](https://github.com/GorvGoyl/Clone-Wars) | 36.3k | **2024-08-06**（PR #255），此后近 2 年零合并 | ⛔ **事实停更**（且 FluxDown 不是「某站点的 clone」） | 未见禁令 | 不提交 |
| [fluttergems/fluttergems](https://github.com/fluttergems/fluttergems) | 242 | 2026-07-20 | ❌ **品类不符**：只收 pub.dev **package**；app 提交已官方迁至上面的兄弟仓 | 未见禁令 | 不提交（改投兄弟仓） |

> **AI-PR 禁令核查方式**：对上述全部 CONTRIBUTING / PR 模板 / README 贡献章节做了 `AI|LLM|ChatGPT|Copilot|AI-generated|machine.generated|bot` 正则全文检索，**零命中**（命中的都是收录条目自身描述里的 "AI"，例如 awesome-rust 的 TabbyML 条目）。即：**目前无一家明文禁止 AI 生成 PR**；但也没有一家明文允许，仍应以人工署名、单条目单 PR 的方式提交。

> **「收录库」vs「收录应用」的判别结论**：FluxDown 是**应用（application）**，不是 crate / package。
> - awesome-rust 同时收录二者，但分处 `## Applications` 与 `## Libraries` 两个顶层区块 → 必须进 `## Applications`。
> - awesome-alternatives-in-rust 同样分 `## Applications` / `## Libraries` → 进 `## Applications`。
> - Solido/awesome-flutter 主体是 package，但设有 `## Open Source Apps` 专区 → 只能进该专区。
> - fluttergems/fluttergems 是**纯 package 目录**，app 已剥离到 awesome-open-source-flutter-apps → 前者直接排除。

---

## P0-1 · rust-unofficial/awesome-rust

**目标文件**：`README.md`（该仓库无 source 文件，直接编辑 README）
**提交入口**：https://github.com/rust-unofficial/awesome-rust/edit/main/README.md
**分支**：`main`

### CONTRIBUTING 硬性要求（原文引用）

来源 https://github.com/rust-unofficial/awesome-rust/blob/main/CONTRIBUTING.md：

> ## TL;DR
> - Accepted: `(stars > 50 | downloads > 2000)`
> - Template: `[ACCOUNT/REPO](https://github.com/ACCOUNT/REPO) [[CRATE](https://crates.io/crates/CRATE)] - DESCRIPTION`
> - Sort: alphabetical

> In order to make this objective, the entry needs to either have at least 50 stars on GitHub, 2000 downloads on crates.io, or an equivalent level of other popularity metrics (which should be specified in the PR).

> - if you've not published your crate to `crates.io` remove the `[[CRATE](...)]` part.
> - if you have a CI build, please add the build badge. Put the image after the description, separated by a space. Please make sure to add the branch information to the image
> - please pay attention to the alphabetical ordering

PR 模板（`.github/pull_request_template.md`）全文只有一行勾选项：

> - [ ] - I have read the [CONTRIBUTING.md](CONTRIBUTING.md) and my pull request fulfills the criteria therein

**达标核对**：1002 star ≫ 50 ✅；无 crates.io crate → **必须去掉 `[[CRATE]]` 段** ✅。

### 插入位置

`## Applications` → `### Utilities`（当前 README 第 762 行起）。
选此分类的依据：该分类已收录同品类条目 —
- `* [YueMiyuki/Risuko](https://github.com/YueMiyuki/Risuko) - A full-featured download manager. …`（README:811，**下载管理器**）
- `* [suckit](https://github.com/Skallwar/suckit) - Recursively visit and download a website's content to your disk. …`（README:794）

字母序（不区分大小写）：`… wthrr → YAKC → YueMiyuki/Risuko → **zerx-lab/FluxDown**`。
→ **`zerx-lab/FluxDown` 排在 `### Utilities` 最末**，紧跟 `YueMiyuki/Risuko` 那一行之后、`### Video` 标题之前。

### 可直接复制粘贴的条目文本

```markdown
* [zerx-lab/FluxDown](https://github.com/zerx-lab/FluxDown) - A multi-protocol download manager with a Rust/Tokio engine, supporting HTTP/FTP, BitTorrent, eD2K, HLS and DASH, with IDM-style dynamic segmentation, browser extensions and an aria2-compatible JSON-RPC endpoint.
```

**关于 CI badge（建议省略）**：CONTRIBUTING 只说「if you have a CI build」，是可选项，`### Utilities` 中绝大多数条目也没有 badge。FluxDown 的 workflow 只有 `release.yml` / `website-ci.yml` / `signing-test.yml` 等，`release.yml` 最近一次运行在 tag 分支 `v0.2.5-rc.2`（success），**默认分支上可能显示 "no status"**，加了反而难看。若仍要加，用与 Risuko 同款写法并显式钉 branch：

```markdown
 [![Release](https://github.com/zerx-lab/FluxDown/actions/workflows/release.yml/badge.svg?branch=main)](https://github.com/zerx-lab/FluxDown/actions/workflows/release.yml)
```
（前置一个空格接在描述之后 —— CONTRIBUTING 原文：「Put the image after the description, separated by a space」）

---

## P0-2 · TaKO8Ki/awesome-alternatives-in-rust

**目标文件**：`README.md`
**提交入口**：https://github.com/TaKO8Ki/awesome-alternatives-in-rust/edit/main/README.md
**分支**：`main`

### CONTRIBUTING 硬性要求（原文引用）

来源 https://github.com/TaKO8Ki/awesome-alternatives-in-rust/blob/main/CONTRIBUTING.md：

> - if you want to add something, please use the template `[REPO](https://github.com/ACCOUNT/REPO) — DESCRIPTION`
> - if you want to add categories, please refer to categories in [awesome-rust](https://github.com/rust-unofficial/awesome-rust).
> - please pay attention to the alphabetical ordering.

**无 star / 年龄门槛。**

⚠️ **模板与实际排版不一致**：CONTRIBUTING 写的是全角破折号 `—`，但 README 里现存的 **每一条** 都用半角 ` - `（例：`* [youki](https://github.com/youki-dev/youki) - An experimental container runtime written in Rust`）。**以邻近条目为准，用 ` - `。**

### 结构与插入位置

该清单是三层结构：`## Applications` → `### 分类` → `#### 被替代的原软件` → `* 条目`。
`### Utilities` 现有子标题依次为：`#### [codemod]` → `#### [jq]` → `#### [lazygit]` → `#### [Toggl Track]`。
字母序（不区分大小写）：`codemod < Internet Download Manager < jq`
→ **新建 `#### Internet Download Manager` 子块，插在 `#### [codemod]` 块（含其 `fastmod` 条目）之后、`#### [jq]` 之前。**

`####` 标题不带链接是有先例的（IDM 是闭源商业软件，无 GitHub 仓库可链）：README 中 `#### autojump / z`、`#### awk`、`#### bash/PowerShell/fish`、`#### bc`、`#### cat`、`#### Vim`、`#### grep`、`#### make`、`#### Reddit` 均为纯文本标题。

**为什么不新开 `### Download managers` / `### Networking` 分类**：CONTRIBUTING 要求新分类「refer to categories in awesome-rust」，而 awesome-rust 的 `## Applications` 下**没有** Download/Networking 分类，下载管理器就放在 `### Utilities`；且该仓库现存 PR #136 `Add MoonProxy Desktop (Networking)` 至今未合并 —— **新增分类的 PR 明显更难被接受**。用现有 `### Utilities` 风险最低。

### 可直接复制粘贴的条目文本

```markdown
#### Internet Download Manager

* [FluxDown](https://github.com/zerx-lab/FluxDown) - A multi-protocol download manager with IDM-style dynamic segmentation, written in Rust
```

同时需要在文件顶部 `## Table of contents` 中同步（现有 ToC 只到 `### 分类` 层级，`####` 层级不入 ToC，因此**无需改 ToC**——`- [Utilities](#utilities)` 已存在于第 22 行）。

---

## P1-1 · fluttergems/awesome-open-source-flutter-apps

**目标文件**：`README.md`
**提交入口**：**先开 issue**（见下方流程），再 https://github.com/fluttergems/awesome-open-source-flutter-apps/edit/main/README.md
**分支**：`main`

### CONTRIBUTING 硬性要求（原文引用）

来源 https://github.com/fluttergems/awesome-open-source-flutter-apps/blob/main/CONTRIBUTING.md：

> **NOTE**: Make sure the open source flutter app repository you are adding has been updated in the last 3 years. A PR with any project that has not been updated in the last 3 years will be automatically rejected.

> **Step 1**: Raise a **new issue** that you want to add a project. We will assign the issue to you and add relevant labels.
> **Step 2**: Star and fork THIS repository.
> **Step 3**: Now in your fork, edit the README.md file. Locate the category to which the open source Flutter app/project belongs. Add the open source app, link and a short description.
> **Step 4**: Raise a PR.
> **Step 5**: Wait for review and PR merge.

> ### Do not raise issues or send PRs for changing issue template, adding header-footer, badges or any buttons.

**无 star 门槛**；唯一硬门槛「3 年内有更新」FluxDown 显然满足。**必须先开 issue 再发 PR**，这是与其它清单最大的流程差异。

### 插入位置（两选一，均为表格行）

排版：`| 项目名 | [Link](repo url) | 描述 |`，三列表头 `| Project | Repo | Description |`。

**首选：`### Tools & Utilities`** —— 最贴近的同类条目 Gopeed 就在这里：
`| Gopeed | [Link](https://github.com/GopeedLab/gopeed) | A modern download manager that supports all platforms |`
字母序：`Floating Volume → **FluxDown** → Frigoligo`（插在 Floating Volume 行之后、Frigoligo 行之前）。

```markdown
| FluxDown | [Link](https://github.com/zerx-lab/FluxDown) | Multi-protocol download manager powered by a Rust engine, supporting HTTP/FTP, BitTorrent, eD2K, HLS and DASH with IDM-style dynamic segmentation and browser integration |
```

**备选：`### Network, Bluetooth & Sharing`** —— 另一款下载管理器 Brisk 在这里：
`| Brisk | [Link](https://github.com/BrisklyDev/brisk) | Fast, multithreaded, cross-platform download manager |`
字母序：`Destiny → **FluxDown** → foldie`（插在 Destiny 行之后、foldie 行之前）。行文本同上。

> 两个分类各有先例，**不要同时提交两处**（CONTRIBUTING 要求一条一 PR 的流程语义）。建议在 Step 1 的 issue 里直接问维护者放哪一栏，把选择权交给他们。

---

## P1-2 · Solido/awesome-flutter

**目标文件**：**`source.md`（小写，仓库根目录），绝对不要改 `README.md`**
**提交入口**：https://github.com/Solido/awesome-flutter/edit/master/source.md
**分支**：`master`（注意不是 main）

### CONTRIBUTING 硬性要求（原文引用）

来源 https://github.com/Solido/awesome-flutter/blob/master/contributing.md：

> - 35 stars minimum are required to apply, it mean your project hold interest
> - Use a meaningful name to your commit or I'll close it instantly, Update source.md is NOT a name
> - Does your app bring something really interesting ?
> - Do not commit on README, use SOURCE.md !
> - Is there a direct link so I we can check the details of your repo ?
> - Flutter is all about UI, use screenshots and animated media for your widget
> - Remember the team is giving its own time to help the community. This is not a paid job yet it take a lot of time to curate and review.

> - Make an individual pull request for each suggestion.
> - Use the following format: `[resource](link) - Description by [Author](link to author)`
> - Use [title-casing](http://titlecapitalization.com) (AP style).
> - **Additions should be added to the bottom of the relevant category.**
> - Keep descriptions short and simple, but descriptive.
> - **Don't mention `Flutter` in the description as it's implied.**
> - Start the description with a capital.

PR 模板（`.github/pull_request_template.md`）勾选项：

> - [ ] I read [How to contribute](https://github.com/Solido/awesome-flutter/blob/master/contributing.md)
> - [ ] I edited the SOURCE.md file only
> - [ ] Added a link to the repo in the PR

**达标核对**：1002 star ≫ 35 ✅。**注意三条容易踩的坑**：① commit message 不能叫 "Update source.md"；② 描述里**不许出现 "Flutter"**；③ 按分类**末尾追加**，不是字母序。

### 插入位置

`## Open Source Apps` → `### Top`，追加到该分类**最后一行之后**（当前最后一条是 `- [Table Habit](https://github.com/FriesI23/mhabit) …`，`source.md:672`，其后即空行 + `## Utilities`）。

不放 `### Premium`：该子栏目前只有 5 条、全是 6k–99k star 的头部项目（AppFlowy 65k / RustDesk 99k / Spotube 41k），1002 star 放进去会被直接退回。`### Top` 里 200–2000 star 的条目比比皆是（Mooltik 221⭐、Instory 206⭐、TailorMade 334⭐）。

### 可直接复制粘贴的条目文本

`source.md` 的排版包含一个 HTML 注释占位符 `<!--stargazers:owner/repo-->`，star 数由构建脚本注入 README，**手写时必须带上这个注释**：

```markdown
- [FluxDown](https://github.com/zerx-lab/FluxDown) <!--stargazers:zerx-lab/FluxDown--> - Multi-protocol download manager and free IDM alternative, powered by a Rust engine by [zerx-lab](https://github.com/zerx-lab)
```

PR 标题（title-case，AP style）建议：`Add FluxDown to "Open Source Apps/Top" Section`（对齐已合并的 PR #999 `Add Table Habit to "Open Source Apps/Top" section`）。
PR 正文里附截图/动图（contributing 明确要求 "use screenshots and animated media"）。

---

## 不够格 / 已死 / 拒收的清单（勿再重复调研）

### ❌ RunaCapital/awesome-oss-alternatives — 收录标准不符 + 半停更

README `## Criteria` 原文（https://github.com/RunaCapital/awesome-oss-alternatives#criteria）：

> Open-source company is added to the list if:
> 1. Its product is strongly based on an open-source repo
> 2. It has a well-known closed-source competitor, solving a similar business problem
> 3. **It is a private for-profit company, founded in the last 10 years**
> 4. Its repo has 100+ stars on GitHub

**否决理由**：
1. 第 3 条要求条目主体是**私营营利性公司**，FluxDown 是 AGPL-3.0 开源项目，不是公司产品；表格里 "Company" 一列填的都是公司名 + 商业官网。
2. 全表 `Alternative to` 一列指向的全是 **SaaS**（Postman / Auth0 / Firebase / DataDog…）。IDM 是买断制桌面软件，不是 SaaS —— 整个清单标题就是 "Awesome open-source alternatives to **SaaS**"。
3. 活跃度：最近一次合并 PR 是 2025-09-03（PR #243），近 11 个月零合并，134 个 open issue、多条 `Add xxx` PR 长期挂着未处理。

投递期望值极低且不符标准，**放弃**。

### ⛔ GorvGoyl/Clone-Wars — 事实停更近两年 + 品类不符

- 最近一次合并 PR：**2024-08-06**（PR #255 `Add mastodon clone: echoloop`）；仓库 `pushed_at` 同为 2024-08-06。此后所有 `Add …` PR（#268/#271/#272/#295）全部挂起未合并。
- 品类：该表两个表格分别是 "Clones with Tutorials" 与 "Clones and Alternatives"，Contribution Guide 原文：

> - It should be a clone/alternative of some popular software or app.
> - Project must have at least minimal functionality, please do not submit any 'UI only' clone.

  FluxDown 严格说算 IDM 的 alternative，勉强能套；但表格列结构强制要求 **Demo 列**（一个可点开的在线 demo 站点），桌面下载管理器无法提供网页 demo（Repo 列另有，Demo 列全表都填的是可访问站点）。

**结论：不投**。若未来该仓库恢复维护再重新评估。

### ❌ fluttergems/fluttergems — 只收 package，不收 app

README 原文（https://github.com/fluttergems/fluttergems）：

> ## How to add a new open source Flutter App/Project to Flutter Gems?
> We have migrated open source Flutter app submissions to [fluttergems/awesome-open-source-flutter-apps](https://github.com/fluttergems/awesome-open-source-flutter-apps)

其 `CONTRIB.md` 流程通篇针对「pub.dev 上的 package」。FluxDown 不发 pub package → **直接改投上面的 P1-1 兄弟仓**，本仓不提交。

### （附）已排查、无需另投的同名/近名仓库

- `theonlypwner/…` / `kirillzubovsky/…` 下**不存在** awesome-alternatives-in-rust。真实仓库是 **`TaKO8Ki/awesome-alternatives-in-rust`**（作者本人 README 说明 "I renamed the repository to 'Awesome Alternatives in Rust'"），已作为 P0-2 处理。
- "flutter-awesome" 类目录站（flutterawesome.com 等）为投稿制内容站而非 GitHub 清单仓库，不属于本 slice 的「GitHub 收录仓库」范围，另见其它 slice。

---

## 提交前的收尾检查（跨清单通用）

1. **去重**：对 4 个目标仓库的 README/source.md 做过 `fluxdown|zerx` 全文检索，**零命中** —— 目前均未收录，不会撞重复条目。
2. **homepage 字段口径不一致**：GitHub 仓库 homepage 字段是 `https://www.fluxdown.com`，而 README 正文链接的是 `https://fluxdown.zerx.dev`（两者实测均 HTTP 200）。审阅者通常点仓库右侧的 homepage，建议提交前统一成一个域名，避免「链接对不上」的观感问题。
3. **一条目一 PR**：awesome-rust、awesome-flutter、awesome-oss-alternatives 均明文要求 individual PR，不要合并提交。
4. **不要自动化批量投**：4 个仓库全部由个人维护者人工 review，同一账号同日群发多仓 PR 极易被判定为 spam。建议按 P0 → P1 分批、间隔提交。

---

## 提交记录（2026-07-27）

全部由 `zerx-lab` 账号提交，外部仓库均走 fork → 分支 → 单条目 minimal diff → PR 流程，FluxDown 主仓未做任何改动。

| # | 目标仓库 | 类型 | 链接 | 状态 |
|---|---|---|---|---|
| P0-1 | rust-unofficial/awesome-rust | PR | https://github.com/rust-unofficial/awesome-rust/pull/2672 | open |
| P0-2 | TaKO8Ki/awesome-alternatives-in-rust | PR | https://github.com/TaKO8Ki/awesome-alternatives-in-rust/pull/148 | open |
| P1-1 | fluttergems/awesome-open-source-flutter-apps | Issue（Step 1） | https://github.com/fluttergems/awesome-open-source-flutter-apps/issues/777 | open |
| P1-2 | Solido/awesome-flutter | PR | https://github.com/Solido/awesome-flutter/pull/1055 | open |

### P0-2 · awesome-alternatives-in-rust PR #148

- 分支 `zerx-lab:add-fluxdown` → `main`；commit `Add FluxDown as an Internet Download Manager alternative`。
- 唯一改动 `README.md`，+4 行：在 `#### [codemod]` 块之后、`#### [jq]` 之前新建**未加链接**的 `#### Internet Download Manager` 小节。
- 按调研结论用了 README 实际排版的 ` - ` 分隔符（非 CONTRIBUTING 写的 `—`），并在 PR 正文里说明了这一取舍；同时说明了 IDM 无仓库可链、标题走纯文本的先例。
- **未新建** Networking / Download managers 分类。ToC 未动（`####` 层级本就不入 ToC）。
- ⚠️ 该仓 CI 的 `cargo run`（死链检查器）**在未改动的 upstream README 上同样崩溃**，报 `serializing nested enums in YAML is not supported yet`：`src/main.rs:354` 用 `serde_yaml::to_string` 序列化 `Working::No(CheckerError::…)` 这一嵌套 enum，只要全表出现任意一条死链就会 panic。已实测对比 upstream/main 原始 README 复现同样报错 → **与本次改动无关的既有缺陷**，不必修。新增的 `https://github.com/zerx-lab/FluxDown` 实测 HTTP 200。

### P1-1 · awesome-open-source-flutter-apps Issue #777

- 按 CONTRIBUTING 的 Step 1 **只开 issue，未提 PR**（该仓强制先 issue 后 PR）。
- 标题 `Add FluxDown - Multi-protocol download manager (open-source IDM alternative)`，沿用该仓现有 issue 的 `Add <Name> - <tagline>` 命名习惯。
- 正文列出项目事实 + 3 年活跃度达标说明，并**把分类选择权交给维护者**：`Tools & Utilities`（Gopeed 先例，插 Floating Volume 与 Frigoligo 之间）vs `Network, Bluetooth & Sharing`（Brisk 先例，插 Destiny 与 foldie 之间），两处的字母序位置都已给出。
- **待办**：维护者回复分类后，再按 Step 2–4 fork 改 `README.md` 表格并提 PR。

### P1-2 · awesome-flutter PR #1055

- 分支 `zerx-lab:add-fluxdown` → `master`；commit `Add FluxDown to Open Source Apps/Top`（**刻意避开 contributing 明令禁止的 "Update source.md"**）。
- 唯一改动 `source.md`，+1/-0；`README.md` **零改动**（已用 `gh pr view --json files` 核实 `changedFiles: 1`）。
- 位置：`## Open Source Apps` → `### Top` 的**最末行**（`Table Habit` 之后、空行 + `## Utilities` 之前），按要求追加到分类底部而非字母序。
- 落地条目：

  ```markdown
  - [FluxDown](https://github.com/zerx-lab/FluxDown) <!--stargazers:zerx-lab/FluxDown--> - Multi-protocol download manager and free IDM alternative, powered by a Rust engine by [zerx-lab](https://github.com/zerx-lab)
  ```

- 自查（对 `git diff upstream/master...add-fluxdown` 的 added 行做正则统计）：`/flutter/gi` 命中数 **0**，`<!--stargazers:zerx-lab/FluxDown-->` 占位符存在，touched files 仅 `source.md`。
- PR 模板三个复选框（含 `I edited the SOURCE.md file only`）全部勾选；正文附了官网 UI 展示链接与 og 图。

### 遗留事项

- awesome-open-source-flutter-apps 需等 issue #777 的分类答复后补提 PR。
- 收尾检查第 2 条（GitHub homepage 字段 `www.fluxdown.com` 与 README 的 `fluxdown.zerx.dev` 不一致）**仍未处理**；本次全部对外提交统一使用 `https://fluxdown.zerx.dev`。
