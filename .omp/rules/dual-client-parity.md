---
description: web 与桌面 App 的 UI 信息架构必须对齐（基准 = 桌面）
condition: web/src/components/settings/**
interruptMode: never
---

你正在修改 web 设置页组件。FluxDown 硬约束（AGENTS.md「镜像契约」表最后一行）：**同一功能在 web 与桌面的归属位置必须一致，基准 = 桌面**。

1. 新增/移动设置块前，先确认它在桌面 `lib/src/pages/settings_page.dart` 属于哪个分类，web 必须放进对应分区组件（GeneralSettings↔通用、DownloadSettings↔下载、BitTorrentSettings↔BitTorrent、ProxySettings↔代理、NotifySettings↔通知…）。
2. 排序/分组也尽量跟随桌面同分类内的相对位置。
3. 文案键 en/zh 成对；共享逻辑（如 `lib/site-auth.ts`）复用单一实现，不复制。
4. 交付前自查：桌面里该功能在哪个菜单，web 就在哪个菜单。
