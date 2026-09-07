---
title: 安装扩展
description: 为 Chrome 或 Firefox 安装 FluxDown 浏览器扩展,并与桌面客户端建立连接。
section: browser-extension
order: 1
sourceHash: "2487a358d7f8"
---

FluxDown 浏览器扩展接管 Chrome、Firefox 的下载流程,把匹配到的下载转交给 FluxDown 桌面客户端,交由其多线程高速下载。扩展本身不具备下载能力——必须先安装[桌面客户端](/docs/zh/getting-started/installation/)并至少启动过一次。两者的安装顺序不影响使用:无论先装哪一个,后装的一方都会自动找到对方。

扩展直接支持 Chrome(Manifest V3)与 Firefox。桌面客户端同时会为 Microsoft Edge 注册连接,其他基于 Chromium 的浏览器(Brave、Vivaldi、Opera 等)通常也能读到 Chrome 的注册信息,因此下文的手动加载方式在这些浏览器上同样可用。

## 从 Chrome 网上应用店安装

1. 打开 Chrome 网上应用店的 [FluxDown 扩展页面](https://chromewebstore.google.com/detail/fluxdown/meleenglfggcmcajknpeeeiobnpfmahc)。
2. 点击**添加至 Chrome**,再在确认弹窗中点击**添加扩展程序**。
3. 把扩展固定到工具栏(工具栏的拼图图标 → FluxDown 旁边的图钉),方便随时查看连接状态。

<!-- TODO(screenshot): Chrome 网上应用店 FluxDown 详情页,突出显示"添加至 Chrome"按钮 -->

## 从 Firefox 附加组件商店安装

1. 打开 addons.mozilla.org 上的 [FluxDown 附加组件页面](https://addons.mozilla.org/firefox/addon/fluxdown/)。
2. 点击**添加到 Firefox**,再在权限提示中点击**添加**。
3. 需要 Firefox 140 及以上版本;若安装按钮呈灰色不可点,先升级 Firefox。

## 手动安装 GitHub Release 构建版

如果你想抢先用上尚未上架商店的新版本,或所在组织只允许安装经过人工审核的扩展,可以用这种方式。

**Chrome(及其他 Chromium 内核浏览器):**

1. 从[最新 GitHub Release](https://github.com/zerx-lab/fluxdown/releases/latest) 下载 `FluxDown-<版本号>-chrome.zip`,解压到一个会长期保留的目录——Chrome 直接从该目录加载扩展,而不是从 zip 本身加载。
2. 打开 `chrome://extensions`,开启右上角的**开发者模式**。
3. 点击**加载已解压的扩展程序**,选择解压出来的目录。
4. 这种手动加载方式与商店版扩展的身份完全一致,与桌面客户端的连接方式也完全相同,无需额外配置。

**Firefox:**

Release 中的 Firefox 构建是经过签名的 `FluxDown-<版本号>-firefox.xpi`,普通(非开发者版)Firefox 无需任何额外设置即可安装:

1. 从同一个 Release 下载 `FluxDown-<版本号>-firefox.xpi`。
2. 打开 `about:addons`,点击齿轮图标 → **从文件安装附加组件…**,选中下载好的 `.xpi` 文件。

<!-- TODO(screenshot): chrome://extensions 页面,已开启开发者模式,"加载已解压的扩展程序"按钮可见 -->

## 扩展如何与桌面客户端建立连接

扩展与 FluxDown 之间不走网络连接。它使用浏览器提供的 Native Messaging(原生消息)机制:浏览器会拉起桌面客户端在本机注册过的一个小型中继进程,扩展通过 stdin/stdout 与这个进程交换消息;中继进程再通过本机操作系统原生通道,把消息转发给正在运行的桌面客户端——Windows 上是 Named Pipe,Linux 与 macOS 上是 Unix domain socket。这项注册由桌面客户端在首次启动时自行完成,所以**必须先安装并至少启动过一次桌面客户端,扩展才能正常工作**。

## 确认连接状态

点击工具栏上的 FluxDown 图标打开弹窗,右上角的状态徽标实时显示连接情况:

| 徽标 | 含义 |
| --- | --- |
| 检测中...(灰色,闪烁) | 弹窗刚打开,正在探测桌面客户端。 |
| 已连接(绿点) | 桌面客户端已响应,下载会被正常接管。 |
| 未连接(红点) | 桌面客户端未能及时响应,下载会回退为浏览器自带下载,直到恢复连接为止。 |

打开弹窗只是**检测**连接状态,并不会主动拉起客户端——纯粹的连接探测被有意设计为不触发启动。如果显示未连接,要么自己手动启动 FluxDown,要么直接点击任意下载链接:扩展会在真正需要发送文件的那一刻自动拉起客户端。

<!-- TODO(screenshot): 扩展弹窗头部,显示"已连接"状态徽标 -->

## 常见故障排查

| 现象 | 排查方法 |
| --- | --- |
| 状态一直停留在"未连接" | 确认 FluxDown 桌面客户端已安装且至少启动过一次——从未运行过的客户端不会完成注册,扩展也就无法连接到它。 |
| 只有在触发一次下载后状态才变成"已连接" | 属于正常现象。客户端由真实的下载请求自动拉起,而不是打开弹窗就会拉起。稍等几秒(通常远不到 10 秒)后重新打开弹窗查看即可。 |
| 下载反复回退到浏览器,并弹出"未检测到 FluxDown 应用"提示 | 说明尝试发送下载时客户端不可达。确认 FluxDown 正在运行后重试;一旦扩展能再次连接到客户端,会自动停止回退,并且约 30 秒内不会重复弹出同一提示。 |
| 重装或更新桌面客户端后扩展毫无反应 | 重启一次浏览器。客户端在启动时才会写入 Native Messaging 注册信息,而浏览器只在(重新)启动时读取这份注册信息。 |
| 怀疑安全软件拦截了连接 | Native Messaging 走的是本机管道/socket 而非网络,拦截进程间通信的杀毒软件或沙箱工具仍可能干扰它。如果使用此类软件,请将 FluxDown 加入信任名单。 |
| 某个网站的下载始终不被接管 | 检查该域名是否已被加入弹窗中的**排除域名**列表——参见[使用扩展](/docs/zh/browser-extension/usage/)。 |
