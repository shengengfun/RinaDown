---
description: 依赖清单变更需遵守项目依赖政策
condition:
  - '(^|/)Cargo\.toml'
  - '(^|/)pubspec\.yaml'
  - '(^|/)package\.json'
interruptMode: never
scope: tool:edit,tool:write
---

你正在改动依赖清单文件。FluxDown 依赖政策：

- 禁止未经用户确认新增 dependency——先说明理由并等确认。
- 版本管理走工具命令（`cargo add/update`、`flutter pub add`、`bun add`/`npm i`），禁止直接手编版本号。
- Cargo workspace（resolver=3）改结构属于大改动，需先读 rust-router skill。
- 允许的手编场景：feature flag 调整、workspace member 声明、非版本元数据。
