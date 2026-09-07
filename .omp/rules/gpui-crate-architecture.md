---
description: GPUI 客户端 crate 边界、目录归属、依赖方向、本机参考源码与新增 capability 的判定规则
condition: crates/**
interruptMode: never
---

你正在修改 FluxDown GPUI 客户端。目录结构必须表达职责边界，避免把迁移后的客户端重新堆成单一 UI crate。

## 本机参考源码

- `~/Desktop/code/github/gpui-component`：组件 API、主题 token、交互状态和组合示例的首选参考；写 GPUI 组件前先查这里，禁止凭印象猜接口。
- `~/Desktop/code/github/zed`：GPUI 原语、窗口行为和大型应用架构的补充参考；仅在 gpui-component 没有对应模式时查阅。
- 两个参考仓只读。FluxDown 的 `Cargo.toml`/`Cargo.lock` 锁定版本与实际编译结果优先；禁止照搬版本不匹配的 API。

## 固定分层

```text
i18n/theme -> components -> shell
                     \-> capability crates
shell + capability crates + host adapters -> app
```

- `crates/i18n`：语言目录、locale 回退、翻译查询；不承载具体页面。
- `crates/theme`：颜色、排版、间距、圆角等设计 token 及主题注册；不包含业务状态。
- `crates/components`：无业务含义的通用 UI 积木。业务组件只有被第二个 capability 实际复用后才上移。
- `crates/shell`：窗口 chrome、标题栏、活动栏、侧栏、内容槽位、路由和窗口级状态；不得实现下载、设置、RSS 等业务页面，不得依赖具体 capability crate。
- capability crate（如 `downloads`、`settings`）：拥有该能力的页面、领域组件、视图状态、动作和宿主接口；一个能力可包含多个页面，不得一页一 crate。
- `crates/app`：唯一 composition root；初始化 GPUI、资产、主题、i18n、宿主连接，创建各 feature Entity 并注入 shell。只有 app 可以知道全部 capability。
- 官方 GPUI/WASM UI 默认只连接 `fluxdown-agent`；页面只依赖能力端口与 `fluxdown_protocol` wire，不直接依赖 `fluxdown_engine`、`fluxdown_daemon`、FluxCloud SDK 或具体 HTTP/WS/IPC 实现。完整服务边界见 `rule://local-service-architecture`。

## capability 内部结构

按真实复杂度渐进拆分，禁止预建空目录：

```text
src/
  lib.rs          # 最小公开面与 feature 注册入口
  strings.rs      # 本能力的类型化文案
  model/          # UI 视图模型、筛选与选择状态
  controller/     # 用户动作、状态转换、宿主命令
  pages/          # 页面级组合
  components/     # 仅本能力复用的领域组件
```

设置类能力按 `sections/` 拆分区；下载类能力按 `pages/components/model/controller` 拆。文件和模块按职责创建，不为结构对称制造空壳。

## 新 crate 判定

满足以下至少两项才新增 capability crate：独立业务状态；独立命令/事件；多个相关页面或对话框；重依赖可隔离；可独立测试/裁剪；与其他能力仅需窄接口通信。单个卡片、任务行、About 页、空状态不是 crate。

## 依赖与装配红线

- capability 之间默认不得直接依赖；跨能力协调由 app 完成，或通过最小宿主接口/事件契约。
- 设置 UI 不得依赖下载 UI；它通过设置端口读写配置。
- capability 的命令由 app 注入统一 agent client；禁止 downloads/settings 页面各自创建连接、持有 Token、重连或维护 daemon/cloud 状态机。
- 禁止新建 `core`、`common`、`shared`、`utils`、`services` 等无明确所有权的垃圾场 crate。
- 不为假想插件化提前引入复杂 trait、宏或服务定位器。先用明确的 `RouteId`、导航元数据和 Entity 内容槽位；出现多个真实注册方后再稳定抽象。
- 资源、文案、状态和组件归属其最窄业务边界；窗口级资源归 app/shell，业务资源归对应 capability。
- 新增 dependency 仍需用户确认；新增 crate 后只运行该 crate 的聚焦 `cargo check -p <crate> --lib`，禁止 workspace 全量验证。

## 修改前检查

1. 这是窗口外壳、通用组件还是业务能力？放到最窄正确边界。
2. 是否已有同职责模块或 crate？复用现有约定，禁止建立第二套目录体系。
3. 是否让 shell 知道了具体业务，或让业务 crate 互相依赖？若是，把装配移到 app。
4. 是否只是未来可能复用？若是，先留在 capability 内。
