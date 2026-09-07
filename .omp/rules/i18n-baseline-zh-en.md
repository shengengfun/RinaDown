---
description: FluxDown 多语言只以英文 + 简体中文为开发基线，其余语言由社区经 Weblate 完善，开发中不检查也不手改
condition:
  - 'assets/i18n/*.json'
  - 'lib/src/i18n/*.dart'
  - 'web/src/lib/locales/*'
  - 'website/src/lib/locales/*'
  - 'fluxDown/utils/locales/*'
interruptMode: never
---

你正在改 FluxDown 的翻译文件。**语言完成度只按 en + zh 两条基线判定**，其余语言（ja 及未来任何新增语言）一律**不检查、不阻塞、不由 AI 补**。

## 四个 i18n 面各自的基线对（缺一不可，多的不管）

| 面 | 基线文件 | 备注 |
|---|---|---|
| App（Flutter，桌面+移动+popup） | `assets/i18n/en.json` + `assets/i18n/zh.json` | 键契约同时体现在 `lib/src/i18n/translations.dart` 的 `S` getter |
| Web SPA | `web/src/lib/locales/en.json` + `zh.json` | `Messages` 类型由 `en.json` 推导 |
| 官网 | `website/src/lib/locales/en.json` + `zh-CN.json` | 中文文件名是 `zh-CN.json`（代码里映射为 `zh`）；`ja.json` 是社区语言 |
| 浏览器扩展 | `fluxDown/utils/locales/zh-CN.ts` + `en.ts` | 反过来：`MessageKey` 类型由 **`zh-CN.ts`** 推导，`en.ts` 是 `Record<MessageKey, string>`，少一个键直接 tsc 报错 |

官网文档另算：`website/src/content/docs/{en,zh}/`，改了 en 正文要在 `website/` 下跑 `npm run docs:hash` 刷新 zh 的 `sourceHash`；en-only 页会自动回退，不必为其造 zh 占位。

## 加/改一个 UI 字符串的完整动作

1. `en.json` 与 `zh.json`（官网为 `zh-CN.json`；扩展为 `zh-CN.ts` + `en.ts`）**同名键、同时补上、都非空**。
2. App 端再去 `translations.dart` 加对应 getter/方法（`_r('key')`，参数化用 `{name}` 占位）——该文件的成员签名是全部调用点的契约。
3. 删键同理两边一起删；改键名等于删旧加新。
4. **不要碰社区语言文件**（`website/src/lib/locales/ja.json` 等）：这些由 Weblate 同步，手写/机翻会与 Weblate 回写冲突。缺键在运行时按键级回退英文，不是 bug，也不是"未完成"。
5. 新增一门语言不需要改代码：三个面都用 glob/AssetManifest 自动发现语言文件，落一个 `<lang>.json` 就会出现在语言选择器。

## 自检（在 FluxDown 仓根执行）

```bash
node -e "const a=require('./assets/i18n/en.json'),b=require('./assets/i18n/zh.json');const A=Object.keys(a),B=Object.keys(b);console.log('zh 缺/空:',A.filter(k=>!(k in b)||!b[k]));console.log('zh 多余:',B.filter(k=>!(k in a)))"
```

把路径换成 `web/src/lib/locales/{en,zh}.json`、`website/src/lib/locales/{en,zh-CN}.json` 即可复用；扩展侧由 `tsc` 把关，跑 `cd fluxDown && npx tsc --noEmit` 或直接看 IDE 报错。**只对基线对跑这个检查，别拿社区语言文件跑。**
