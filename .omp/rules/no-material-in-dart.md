---
description: Dart UI 强制 shadcn_ui，禁用 Material/Cupertino API
condition:
  - 'showDialog\s*\('
  - 'MaterialApp\s*\('
  - 'Theme\.of\(context\)'
  - 'Scaffold\s*\('
  - 'AppBar\s*\('
  - 'Icons\.'
globs: lib/**/*.dart
repeatMode: after-gap
repeatGap: 3
---

FluxDown 桌面端全程使用 shadcn_ui，禁止原生 Material/Cupertino 组件。替换对照：

- `showDialog()` → `showShadDialog()`
- `MaterialApp` → `ShadApp`
- `Theme.of(context)` → `ShadTheme.of(context)`；主题色板经 `AppColors.of(context)`
- `Icons.xxx` → `LucideIcons.xxx`
- `Scaffold`/`AppBar` → shadcn_ui 布局组件 + 项目自有 `header_bar.dart`/`title_drag_area.dart` 模式

统一导入 `package:shadcn_ui/shadcn_ui.dart`。若正在编辑 `lib/src/mobile/` 且确需平台组件，先向用户确认。
