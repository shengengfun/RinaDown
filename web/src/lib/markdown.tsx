// 极小 Markdown 子集渲染：# / ## / ### 标题、- 无序列表、1. 有序列表、**加粗**、
// `行内代码`，空行分段。逐行解析，输出 React 节点（不用 dangerouslySetInnerHTML），
// 供推介说明等 admin 可配置的纯文本字段做轻量格式化展示。子集与 App 端
// lib/src/widgets/update_changelog_dialog.dart 的 _MarkdownBody/_InlineMarkdown 对齐。

import type { ReactNode } from 'react'

type Block = { kind: 'h1' | 'h2' | 'h3' | 'p'; text: string } | { kind: 'ul' | 'ol'; items: string[] }

const HEADING_RE = /^(#{1,3})\s+(.*)$/
const UL_RE = /^-\s+(.*)$/
const OL_RE = /^\d+\.\s+(.*)$/

function isBlockStart(line: string): boolean {
  return HEADING_RE.test(line) || UL_RE.test(line) || OL_RE.test(line)
}

function parseBlocks(markdown: string): Block[] {
  const lines = markdown.replace(/\r\n/g, '\n').split('\n')
  const blocks: Block[] = []
  let i = 0
  while (i < lines.length) {
    const line = lines[i].trim()
    if (!line) {
      i++
      continue
    }

    const heading = HEADING_RE.exec(line)
    if (heading) {
      const kind = heading[1].length === 1 ? 'h1' : heading[1].length === 2 ? 'h2' : 'h3'
      blocks.push({ kind, text: heading[2] })
      i++
      continue
    }

    const ul = UL_RE.exec(line)
    if (ul) {
      const items = [ul[1]]
      i++
      while (i < lines.length) {
        const m = UL_RE.exec(lines[i].trim())
        if (!m) break
        items.push(m[1])
        i++
      }
      blocks.push({ kind: 'ul', items })
      continue
    }

    const ol = OL_RE.exec(line)
    if (ol) {
      const items = [ol[1]]
      i++
      while (i < lines.length) {
        const m = OL_RE.exec(lines[i].trim())
        if (!m) break
        items.push(m[1])
        i++
      }
      blocks.push({ kind: 'ol', items })
      continue
    }

    // 段落：连续非空、非其他块起始的行合并为一段
    const paraLines = [line]
    i++
    while (i < lines.length && lines[i].trim() && !isBlockStart(lines[i].trim())) {
      paraLines.push(lines[i].trim())
      i++
    }
    blocks.push({ kind: 'p', text: paraLines.join(' ') })
  }
  return blocks
}

const INLINE_RE = /\*\*(.+?)\*\*|`([^`]+)`/g

function renderInline(text: string): ReactNode[] {
  const nodes: ReactNode[] = []
  let lastEnd = 0
  let key = 0
  INLINE_RE.lastIndex = 0
  let match: RegExpExecArray | null
  while ((match = INLINE_RE.exec(text))) {
    if (match.index > lastEnd) nodes.push(text.slice(lastEnd, match.index))
    if (match[1] !== undefined) {
      nodes.push(
        <strong key={key++} className="font-semibold text-text">
          {match[1]}
        </strong>,
      )
    } else if (match[2] !== undefined) {
      nodes.push(
        <code key={key++} className="rounded bg-surface3 px-1 py-0.5 font-mono text-[11px] text-accent">
          {match[2]}
        </code>,
      )
    }
    lastEnd = match.index + match[0].length
  }
  if (lastEnd < text.length) nodes.push(text.slice(lastEnd))
  return nodes
}

/** 渲染 admin 配置的 Markdown 子集文本；样式对齐对话框正文的 text2 层次。 */
export function MarkdownLite({ text, className }: { text: string; className?: string }) {
  const blocks = parseBlocks(text)
  if (blocks.length === 0) return null
  return (
    <div className={className}>
      {blocks.map((block, idx) => {
        switch (block.kind) {
          case 'h1':
            return (
              <p key={idx} className="mb-1 mt-3 text-[13.5px] font-semibold text-text first:mt-0">
                {renderInline(block.text)}
              </p>
            )
          case 'h2':
            return (
              <p key={idx} className="mb-1 mt-3 text-[13px] font-semibold text-text first:mt-0">
                {renderInline(block.text)}
              </p>
            )
          case 'h3':
            return (
              <p key={idx} className="mb-1 mt-2.5 text-[12.5px] font-semibold text-text2 first:mt-0">
                {renderInline(block.text)}
              </p>
            )
          case 'ul':
            return (
              <ul key={idx} className="my-1.5 list-disc space-y-1 pl-4 marker:text-text3">
                {block.items.map((item, i) => (
                  <li key={i} className="text-[12.5px] leading-relaxed text-text2">
                    {renderInline(item)}
                  </li>
                ))}
              </ul>
            )
          case 'ol':
            return (
              <ol key={idx} className="my-1.5 list-decimal space-y-1 pl-4 marker:text-text3">
                {block.items.map((item, i) => (
                  <li key={i} className="text-[12.5px] leading-relaxed text-text2">
                    {renderInline(item)}
                  </li>
                ))}
              </ol>
            )
          case 'p':
          default:
            return (
              <p key={idx} className="my-1 text-[12.5px] leading-relaxed text-text2 first:mt-0">
                {renderInline(block.text)}
              </p>
            )
        }
      })}
    </div>
  )
}
