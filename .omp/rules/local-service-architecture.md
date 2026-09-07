---
description: FluxDown 下一代 daemon/agent/protocol 本机服务边界、状态所有权、JSON-RPC 契约与旧宿主迁移规则
condition: native/**
interruptMode: never
---

你正在修改 FluxDown Rust 本机核心或宿主。`native/{protocol,daemon,agent}` 已建立 crate 边界，但生产运行链路仍在迁移；不得把基础 crate 误报成已可运行服务。

## 固定分层

```text
GPUI / WASM Web / CLI
          |
          v
fluxdown-agent  ---- HTTPS/SSE ----> FluxCloud
          |
          | JSON-RPC
          v
      fluxdownd
          |
          v
 fluxdown_engine
```

- `native/engine`：下载领域实现；不知道 UI、FluxCloud 账户和进程传输。
- `native/daemon`（`fluxdown_daemon`，目标二进制 `fluxdownd`）：aria2c 式纯下载核心；拥有下载任务、下载设置、RSS、插件、Webhook、下载 DB 与下载事件。
- `native/agent`（`fluxdown_agent`，目标二进制 `fluxdown-agent`）：官方客户端常驻后端；拥有 FluxCloud Token、设备身份、配置同步、Entitlements、远程任务和 UI Gateway。
- `native/protocol`（`fluxdown_protocol`）：传输无关 wire；只放 DTO、版本、方法名、事件和稳定错误码，不放网络运行时、业务实现、数据库或 UI。
- `native/server`：旧 headless 生产路径，进入废弃期；新功能不得依赖或落入 server，迁移代码直接归 daemon/agent/protocol。

## 状态单一所有者

- 下载任务、队列、分组、下载配置的事实源只能是 daemon；agent/UI 不直接读写下载 DB。
- FluxCloud Access/Refresh Token、同步 revision、设备信任与远程任务状态机的事实源只能是 agent；Token 不进入 daemon、GPUI、WASM LocalStorage 或 URL。
- `fluxdown_engine` 只执行下载领域行为；agent 不直接依赖或调用 engine，必须经 daemon 协议。
- UI 只管理表单草稿、选择、路由、滚动、窗口等展示状态；关闭全部 UI 后 daemon 与 agent 的后台状态机仍可继续。
- 下载设置云同步由 agent 协调，但最终值必须先由 daemon 校验、持久化和应用；禁止 agent 与 daemon 各存一份可独立修改的下载设置。

## 协议与传输

- 控制面统一 JSON-RPC 2.0 语义；事件用带单调 `sequence` 的 JSON-RPC notification，断线后靠全量 snapshot 恢复。
- Web 使用 HTTP + WebSocket；桌面初期复用同一传输，只有实测瓶颈后才增加 Windows Named Pipe / Unix Domain Socket。传输变化不得分叉方法、DTO、错误码或 UI 业务逻辑。
- 二进制上传/下载、静态资源可走专用 HTTP endpoint；禁止为追求“全 JSON-RPC”把大文件普遍 Base64 化。
- 新增 wire 类型先检查 `native/protocol` 与 `native/api/src/types.rs`，禁止创建同义 DTO；旧 API 迁移时必须一次性迁移调用方并删除旧路径。
- 连接建立先做版本与 capability 握手；协议不兼容必须显式失败，不做静默降级或假回退。

## 依赖方向

```text
engine <- daemon <- agent
protocol <- daemon / agent / clients
```

- 上图的 `daemon <- agent` 表示运行期 RPC，不表示 agent 可依赖 daemon 实现 crate。
- daemon 不依赖 agent、server、hub 或 `crates/*`。
- agent 不依赖 engine、server、hub 或 GPUI capability crate。
- protocol 不依赖 engine、daemon、agent、api、Tokio/Axum、数据库或 GPUI。
- 官方 UI 默认只连 agent；纯下载第三方客户端可以直连 daemon。

## 迁移纪律

- 当前 `hub` / `server` 仍是生产宿主；新 daemon 未覆盖的行为不得删除或宣称已迁移。
- 从 server/hub 迁移实现时复用 `EventSink`、`HostSelection`、`ApiHost` 与既有错误类型；禁止复制出第二套引擎 actor 语义。
- 一个 data dir 同时只允许一个 Engine/daemon 写入；迁移期禁止 daemon 与 hub/server/CLI `--local` 共享 DB 并发运行。
- 云功能从 Flutter `lib/src/services/cloud/` 迁移到 agent 后，删除 Dart 对应状态机；不保留双 Token、双 SSE、双 revision 实现。
- 只为真实迁移切片创建模块；禁止预建空目录、空 trait、占位 handler、假成功响应或无运行行为的二进制。

## 修改前检查

1. UI 全退后仍需继续吗？需要则归 daemon 或 agent，不归 UI。
2. 功能是否需要知道 FluxCloud 账户？需要则归 agent，不归 daemon。
3. 功能是否直接改变下载事实？需要则由 daemon 统一入口调用 engine。
4. 这是 wire 语义还是传输实现？前者归 protocol，后者留在服务端/客户端适配器。
5. 是否依赖了 `native/server` 或建立第二套 DTO/状态机？若是，停止并收敛到新边界。
