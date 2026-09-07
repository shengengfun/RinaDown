---
description: 拦截项目禁止执行的高副作用命令
scope: tool:bash
condition:
  - 'flutter\s+run\s+-d\s+windows'
  - 'cargo\s+test\s+--workspace'
  - 'git\s+push\s+\S*\s*v\d'
  - 'git\s+push\s+.*--tags'
repeatMode: after-gap
repeatGap: 2
---

该命令属于 FluxDown 项目禁令：

- `flutter run -d windows`：AGENTS.md 明确禁止运行。改用 `flutter analyze` / `flutter test` / `cargo check -p <crate>` 验证。
- `cargo test --workspace`：禁止全量跑。用 `cargo nextest run -p <crate> <filter>` 或 `cargo test -p <crate> -- <filter>` 精准跑。
- 推送 `v*` tag 会触发 GitHub Actions 全平台发布流水线，必须由用户明确要求后执行。

请换用合规命令后继续。
