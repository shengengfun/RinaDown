<!--
  文档贡献 PR 模板 / Documentation contribution PR template
  在新建 PR 时，于 URL 末尾追加 ?template=docs.md 即可使用本模板，例如：
  https://github.com/zerx-lab/FluxDown/compare/main...<your-branch>?template=docs.md
-->

## 说明 / Description

<!-- 简述本次改动的内容（新增页面 / 修改内容 / 翻译）。 -->
<!-- Briefly describe this change (new page / content edit / translation). -->

## Checklist

- [ ] 本 PR 只涉及**一个页面**（新增或修改），未夹带无关改动
      This PR touches **exactly one page** (new or edited), with no unrelated changes
- [ ] 内容为**纯 Markdown**，不包含内联 HTML 标签
      Content is **plain Markdown** — no inline HTML tags
- [ ] 所有图片均 **<200KB**，且引用**本地路径** `website/public/docs/...`（不外链图床）
      All images are **<200KB** and reference a **local path** under `website/public/docs/...` (no hotlinked images)
- [ ] 更大的资源（视频 / 高清大图）已联系维护者上传至 R2，未直接提交进仓库
      Larger assets (video / high-res images) were coordinated with a maintainer for R2 upload, not committed directly
- [ ] 若为翻译 PR：已在本地运行 `npm run docs:hash <zh 文件相对路径>` 写入 `sourceHash`，或已在下方"翻译声明"中说明留空
      If this is a translation PR: ran `npm run docs:hash <path to the zh file>` locally to write `sourceHash`, or noted below that it is left blank
- [ ] 已勾选右侧 **"Allow edits from maintainers"**（便于维护者直接协助补充，例如 sourceHash）
      Enabled **"Allow edits from maintainers"** on the right sidebar (lets maintainers help fill in details such as sourceHash)

## 翻译声明 / Translation note

<!-- 如不涉及翻译可删除本节。示例："sourceHash 已写入" 或 "sourceHash 留空，请维护者合并前后补上"。 -->
<!-- Remove this section if not a translation. Example: "sourceHash written" or "sourceHash left blank, please fill in around merge time". -->

## 相关链接 / Related links

<!-- 关联的 Issue、讨论等。 / Linked issue, discussion, etc. -->
