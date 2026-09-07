---
title: 参与 FluxDown 翻译
description: 通过 GitHub 为应用、Web 界面和官网贡献你的语言翻译。
section: contributing
order: 2
sourceHash: "03eb048d024b"
---

FluxDown 的多语言在公开的 [zerx-lab/FluxDown](https://github.com/zerx-lab/FluxDown) 仓库中维护。社区用户通过 GitHub Pull Request 贡献语言更新。

## 可以翻译什么


| 部件 | 覆盖范围 |
| --- | --- |
| **Desktop & Mobile App** | Windows/macOS/Linux 桌面端与移动端应用内的全部字符串 |
| **Web App** | headless 服务器托管的 Web 管理界面 |
| **Website** | fluxdown.zerx.dev 官网——首页、FAQ、更新日志 |

英文是源语言，简体中文由核心团队维护，其余语言等你来开创。

## 快速开始

1. Fork [zerx-lab/FluxDown](https://github.com/zerx-lab/FluxDown)，从 `main` 创建分支。
2. 新增或更新对应部件的翻译文件：
   - **桌面端与移动端应用**：`assets/i18n/`
   - **Web 应用**：`web/src/lib/locales/`
   - **官网**：`website/src/lib/locales/`
3. 向 `main` 发起 Pull Request，维护者审核后合并。

## 占位符

花括号内容如 `{name}`、`{count}`、`{speed}` 会在运行时被实际值替换。**占位符必须原样保留**——可以按你语言的语序自由调整位置，但不要翻译或删除花括号里的内容。

## 开创新语言

列表里还没有你的语言？在对应部件目录中新增翻译文件，并先填写 `languageNativeName`，让语言选择器能正确显示语言名称。然后向 `main` 发起 Pull Request。

合并之后：

- **应用**：下个版本里你的语言自动出现在「设置 → 语言」。
- **Web 界面与官网**：下次部署后语言切换器中自动出现。

## 小贴士

- **翻译一部分也有价值。**未翻译的字符串逐键回退英文——完成 30% 的语言已经可用。
- **一致性优先于直译。**多看相邻字符串，同一概念用同一译名。
- 发现英文原文有笔误？在 Pull Request 中一并修正，或新建 issue 报告。
