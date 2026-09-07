---
description: FluxDown 的悬浮/弹出层一律无进出场动画——ShadTooltip 必须 effects/const []，showShadDialog 必须 animateIn/animateOut/const []，不得沿用 shadcn_ui 默认的淡入位移
condition:
  - 'ShadTooltip\('
  - 'ShadPopover\('
  - 'showShadDialog\('
interruptMode: never
---

你正在写 shadcn_ui 的弹出层。**FluxDown 的既定观感是：提示与弹窗要么不出现，要么立刻在那儿。**
shadcn_ui 默认的 200ms 淡入+位移在本项目一律关掉——tooltip 已经等了 500ms 悬浮延迟，
再叠一段入场只是把等待继续拖长；弹窗动画则让连续操作（确认→再确认）显得黏滞。

按组件补齐参数，**新写和改到的地方都要带上**：

| 组件 | 必须显式传 |
|---|---|
| `ShadTooltip` | `effects: const []` |
| `ShadPopover` | `effects: const []` |
| `showShadDialog` | `animateIn: const []` + `animateOut: const []` |

既有范例（照抄即可）：
- `lib/src/widgets/task_list.dart` 的「管理」入口 tooltip
- `lib/src/widgets/rss_item_list.dart` 的刷新按钮 tooltip
- `lib/src/widgets/overflow_tooltip_text.dart` 的文件名溢出提示
- 全部 `show*Dialog` 入口（`feedback_dialog.dart` / `queue_manager_dialog.dart` / `manifest_select_dialog.dart` …）

顺带两条同一族的坑，改弹出层时一并检查：

1. **`ShadTooltip` 包裸 `Text` / `Container` / 原生 `MouseRegion` 不会触发。**
   它不自带 `MouseRegion`，而是把 `hoverStrategies` 注入 `ShadTheme`，等 child 内部的
   `ShadGestureDetector` 回调 `onHoverChange`。朴素 widget 没有那一层，`waitDuration` 形同虚设。
   要么 child 换成自带手势层的 `ShadIconButton` / `ShadInput` / 显式 `ShadGestureDetector`，
   要么像 `OverflowTooltipText` 那样自持 `ShadTooltipController` + `MouseRegion` + `Timer`。

2. **给 `ShadGestureDetector` 补手势层时必须传 `onTap`。**
   它会按 theme 默认 `hoverStrategies`（含 `onTapDown`）注册 tap 识别器并赢下手势竞技场，
   不传 `onTap` 就会把点击吞掉，导致外层行/卡片的选中失效。

例外：只有用户明确要求某处带动效时才保留 effects，并在该处写明理由。
