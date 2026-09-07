---
title: 打包与插件市场
description: .fxplug 格式、安装时的安全限制、发布到插件索引。
section: plugins
order: 5
sourceHash: "70f54c9ba733"
---

## `.fxplug` 格式

`.fxplug` 文件就是**插件文件夹打的 zip**，换了个扩展名而已。把 `manifest.json` 和 `.js` 文件打成 zip，重命名为 `my-plugin.fxplug` 就完事。两种结构都行：

```
my-plugin.fxplug          my-plugin.fxplug
├── manifest.json         └── my-plugin/          ← 单层包裹目录
└── resolver.js               ├── manifest.json      会被自动剥掉
                               └── resolver.js
```

`manifest.json` 必须在 zip 根部，或者在唯一的一层包裹目录里；其他情况一律以「未找到根 manifest.json」拒绝。

### 安装时的限制

无论 zip 从哪来，安装器都强制这些检查：

| 检查 | 限制 |
|---|---|
| zip 条目数 | ≤ 200 |
| 解压总大小 | ≤ 50 MB |
| 条目路径 | 必须落在目标目录内——`..` 和绝对路径被拒绝（防 zip-slip） |
| manifest | 完整解析并校验（见 [Manifest 参考](/docs/zh/plugins/manifest/)） |
| 脚本 | 做编译检查，语法错误直接中止安装 |

插件解压到 `<数据目录>/plugins/<identity>/`。相同 identity 再次安装会替换旧版本。

## 插件市场

FluxDown 的市场是**一份数据格式，不是一个服务**——一份应用直接读取的索引，无后端、无账号。组成部分：

- **索引**——一份 Git 版本化的 JSON 文件，列出插件、版本和下载镜像。默认索引在 GitHub 的 `zerx-lab/fluxdown-plugin-index` 仓库；任何人都可以 fork 一份自己维护。用户可以在应用里添加自定义索引源。
- **内容寻址**——每个发布版本都记录 `contentHash = sha256(整个 .fxplug 文件)`。从任何镜像下载后，FluxDown 重新计算哈希，不一致就拒绝。被攻破的镜像换不掉内容。
- **多镜像**——每个版本列出多个下载地址（raw.githubusercontent、jsDelivr、GitHub Releases……），FluxDown 依次尝试。镜像 URL 必须是 `https`，且不能指向私网/环回/元数据地址。
- **防回滚**——每份索引带一个单调递增的 `sequence`；应用记住每个索引见过的最大值，拒绝更旧的。

v1 没有作者级密码学签名——完整性依靠内容寻址 + TLS + Git 历史。索引 schema 预留了签名字段，将来加上不会破坏现有客户端。

### 发布插件

1. 本地写好并测试（开发模式，见[写第一个插件](/docs/zh/plugins/your-first-plugin/)）。
2. 打成 `.fxplug`，托管到一个可以长期通过 `https` 访问的地方——GitHub Release 是常规选择。
3. 计算哈希：`sha256sum my-plugin.fxplug`。
4. 向索引仓库提 PR，添加你的插件条目：identity、版本、`contentHash`、镜像地址，并递增 `sequence`。

合并后，插件在应用内市场的下次索引刷新时出现。

### 撤回（yank）某个版本

索引支持把某个版本标记为已撤回（针对有 bug 或恶意的发布）。「安装最新版」会跳过被撤回的版本；修好后按同样流程发布一个更高版本号即可。

## 安装 API

除了设置页，也可以用接口安装插件：

- `GET /api/v1/market`——拉取合并后的市场索引。
- `POST /api/v1/market/install`，body 为 `{ "pluginId": "name@author" }`——安装最新的未撤回版本。

两者属于管理 API 组，需要管理 token——见 [API 概览](/docs/zh/api/overview/)。
