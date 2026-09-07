# FluxDown 油猴脚本（下载接管）

通过 **Tampermonkey / Violentmonkey** 用户脚本，把浏览器里的下载与流媒体资源
一键发送到 **FluxDown** 桌面下载器——无需安装浏览器扩展。

> 原理与安全设计详见 [`docs/userscript-takeover-design.md`](../docs/userscript-takeover-design.md)。

## 安装

1. 在浏览器安装 [Tampermonkey](https://www.tampermonkey.net/) 或
   [Violentmonkey](https://violentmonkey.github.io/)。
2. 安装本脚本 `fluxdown.user.js`，任选其一：
   - **从 FluxDown 应用内**：打开 FluxDown → 设置 → RPC → 「复制油猴脚本」，
     在 Tampermonkey「添加新脚本」中粘贴保存；
   - **从文件**：用脚本管理器打开本仓库的 `userscript/fluxdown.user.js`。
3. 确保 FluxDown 桌面端正在运行，且「设置 → RPC」已开启（默认开启，端口 17800）。
4. （Chrome）Tampermonkey 需开启浏览器「开发者模式」，并在脚本权限中允许「所有网站」，
   `GM_xmlhttpRequest` 才能访问本机服务。

## 使用

- **点击下载**：点击下载链接 / 带 `download` 的链接 / 下载型扩展名链接时，自动接管并
  发送到 FluxDown（FluxDown 会弹确认框）。按住 **Alt** 点击则放行给浏览器。
- **媒体嗅探**：播放视频的页面右下角会出现 ⬇ 悬浮按钮，点开「资源面板」可看到嗅探到的
  HLS/DASH/视频/音频等资源，逐个或「全部发送」到 FluxDown。
- **油猴菜单**（点击 Tampermonkey 图标 → 本脚本）：
  - 下载接管 开/关
  - 媒体嗅探 开/关
  - 显示/隐藏 资源面板
  - 下载本页全部链接
  - 设置端口 / 设置 Token / 测试连接

## 配置

| 项 | 说明 |
|---|---|
| 端口 | 须与 FluxDown「设置 → RPC → RPC 监听端口」一致（默认 17800） |
| Token | 仅当你在 FluxDown 设置了 RPC 授权密钥时才需要填写（菜单 → 设置 Token） |

## 能力边界（请知悉）

- **无法**接管「浏览器内核直接发起、非页面 JS 触发」的下载——这类请改用 FluxDown 浏览器扩展。
- 仅能携带非 httpOnly 的 Cookie（`document.cookie`）；需 httpOnly 鉴权的下载建议用扩展。
- HLS/MSE 站点需在播放器初始化时嗅到清单；若播放开始后才装脚本，请**刷新页面**重新嗅探。
- 默认 `@noframes`，不嗅探跨域 iframe 内的媒体。

## 与 aria2 脚本的兼容

FluxDown 的 RPC 服务额外暴露 `POST /jsonrpc` 的 **aria2 JSON-RPC 兼容端点**
（`aria2.addUri` 等）。任何「发送到 aria2」类油猴脚本，只要把 RPC 地址指向
`http://127.0.0.1:17800/jsonrpc` 即可把下载发给 FluxDown。
