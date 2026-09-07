---
description: 禁止用 AnimatedContainer/Color.lerp 从 Colors.transparent 过渡到有色背景——中间帧经过黑基底半透明灰，悬浮时出现深浅两色闪烁
condition:
  - 'AnimatedContainer'
  - 'Color\.lerp'
interruptMode: never
---

你正在写 Flutter 颜色过渡动画。`Colors.transparent` 是 `0x00000000`（黑色基底的全透明），
用 `AnimatedContainer` / `Color.lerp` 从它插值到浅色（如 hoverBg）时，中间帧是偏暗的
半透明灰——视觉上表现为悬浮背景先闪深色再变浅色的「两种颜色闪烁」。

规则：
- 颜色过渡的透明端必须使用目标色的零透明版本：`target.withValues(alpha: 0)`，
  **不得**使用 `Colors.transparent`。
- 悬浮/选中这类即时状态切换，优先与侧栏既有项（`_NavItem` 等）保持一致：
  用普通 `Container` 直接切色，不加动画。
- 仅在确实需要渐变动效、且两端颜色同基底时才用 `AnimatedContainer`。
