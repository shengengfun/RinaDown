---
description: FluxDown 面向用户的文案必须走 i18n 查表，UI 代码里禁止硬编码中文字面量
condition:
  - '(?m)^(?!\s*(//|///|\*|#|<!--)).*\b(Text|SelectableText)\(\s*(const\s+)?[''"][^''"\n]*[\u4e00-\u9fff]'
  - '(?m)^(?!\s*(//|///|\*|#|<!--)).*TextSpan\(\s*text:\s*(const\s+)?[''"][^''"\n]*[\u4e00-\u9fff]'
  - '(?m)^(?!\s*(//|///|\*|#|<!--)).*Tooltip\(\s*message:\s*(const\s+)?[''"][^''"\n]*[\u4e00-\u9fff]'
  - '(?m)^(?!\s*(//|///|\*|#|<!--)).*\b(label|title|subtitle|placeholder|description|hint|hintText|helperText|errorText|tooltip|semanticLabel|confirmText|cancelText|buttonText|emptyText)\s*:\s*(const\s+)?[''"][^''"\n]*[\u4e00-\u9fff]'
  - '(?m)^(?!\s*(//|///|\*|#|<!--)).*String\s+get\s+\w+\s*=>\s*[''"][^''"\n]*[\u4e00-\u9fff]'
  - '(?m)^(?!\s*(//|///|\*|#|<!--)).*\b(placeholder|title|label|aria-label|alt)\s*=\s*"[^"\n]*[\u4e00-\u9fff]'
  - '(?m)^(?!\s*(//|///|\*|#|<!--)).*>[^<>{}\n]{0,80}[\u4e00-\u9fff][^<>{}\n]{0,80}<'
globs:
  - 'lib/**/*.dart'
  - 'web/src/**/*.tsx'
  - 'website/src/**/*.{tsx,astro}'
  - 'fluxDown/**/*.{ts,tsx,html}'
repeatMode: after-gap
repeatGap: 3
---

你正在 FluxDown 的 UI 代码里写死中文文案。**所有面向用户的字段都必须走 i18n 查表**，否则该字段永远只有一种语言，社区也无从翻译。

按所在面替换：

| 面 | 写法 |
|---|---|
| App（`lib/`） | `final s = S.of(locale);` → `Text(s.xxx)`；参数化 `s.xxx(name: v)` |
| Web SPA（`web/`） | `const { t } = useI18n()` → `t('xxx')` |
| 官网（`website/`） | `useI18n()` / `Messages` 键；Astro 侧取服务端 locale |
| 扩展（`fluxDown/`） | `t('section.key')`，或 HTML 上挂 `data-i18n="section.key"` |

Dart 侧文案承载点不止 `Text()`——本规则覆盖：`Text(` / `SelectableText(` / `TextSpan(text:` / `Tooltip(message:`，以及**字符串直传**的命名参数 `label:`（菜单项）、`tooltip:`（`ShadIconButton`/`ShadTooltip`）、`hintText:`（输入框）、`placeholder:` / `title:` / `description:`（shadcn 多数场景传的是 `Text(...)` widget，内层照样命中）、`semanticLabel:`，以及 `String get xxx => '…'` 形式的展示名 getter。

同一动作里把键补进**基线双语**：`en.json` + `zh.json`（官网 `zh-CN.json`，扩展 `zh-CN.ts` + `en.ts`）；App 端再去 `lib/src/i18n/translations.dart` 加 getter。**ja 等社区语言不要动**（Weblate 维护，运行时按键级回退英文）——细则见规则 `i18n-baseline-zh-en`。

**可以硬编码的例外**（命中本规则时若属于以下情形，说明一句继续即可）：

- 快捷键 / 符号 / 单位 / 版本号（如 `'Ctrl+A'`、`'MB/s'`），以及品牌名、URL、协议名、CLI 子命令等专有名词。
- **语言自称**：语言选择器里的 `简体中文` / `English` / `日本語` 等条目（`NATIVE_NAMES`、`<option value="zh-CN">简体中文</option>`）本就不该被翻译。
- 非 UI 字符串：日志文案、异常 message、注释、测试断言、`debugPrint`。
- 纯中文管理后台（FluxCloud `admin/`）无 i18n 层，不适用本规则。
- Dart 里**不是文案**的同名参数：`TextEditingController(text: …)`（输入框初值）、异常/DTO 的 `message:`（只有 `Tooltip(message:` 才算 UI）、模型字段 `name:`/`description:`（存数据，不渲染）。
