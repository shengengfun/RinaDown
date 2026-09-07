---
description: 禁止手动编辑 rinf 生成的 Dart 绑定
condition: lib/src/bindings/**
interruptMode: always
---

`lib/src/bindings/` 是 `rinf gen` 的自动生成产物，手动编辑会在下次生成时被覆盖。

正确流程：
1. 修改 `native/hub/src/signals/mod.rs` 中的信号结构体（标注 DartSignal/RustSignal/SignalPiece）。
2. 运行 `rinf gen` 重新生成 Dart 绑定。
3. Dart 端通过 `XxxSignal.rustSignalStream` / `.sendSignalToRust()` 使用。
