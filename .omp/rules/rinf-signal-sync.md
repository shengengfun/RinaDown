---
description: 改动 Rinf 信号定义后需要重新生成 Dart 绑定
condition: native/hub/src/signals/**
interruptMode: never
---

你正在修改 `native/hub/src/signals/mod.rs`（Dart↔Rust 信号契约）。完成后记得：

1. 运行 `rinf gen` 重新生成 `lib/src/bindings/`。
2. Rust 端在 `download_actor.rs` 的 `tokio::select!` 中添加/调整监听分支。
3. 若涉及 `engine::model` 类型，同步 `signal_bridge.rs` 的 `From` 转换。
4. 已发布信号结构体的字段顺序/类型是二进制契约（bincode），改动需评估兼容性。
