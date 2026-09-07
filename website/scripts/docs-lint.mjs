#!/usr/bin/env node
/**
 * docs-lint.mjs — 文档安全 lint（零运行时依赖）
 *
 * 扫描 src/content/docs/**\/*.md，拒绝以下两类风险内容（见文档系统设计方案 D2/D9）：
 *
 *   1) Markdown 链接/图片目标使用 javascript: 或 data: scheme —— 潜在 XSS/钓鱼向量。
 *
 *   2) 图片语法 ![alt](url) 引用外链 —— 外链图片可在合入后被域名所有者替换内容，
 *      且加载时会泄露访客 IP，人工 review 挡不住。图片仅允许相对路径或以 /docs/
 *      开头的本地路径；普通文本链接 [text](https://...) 不受此限制。
 *
 * 命中任意一类 → 打印 "文件:行号  说明" 清单并以非零状态退出；干净内容退出 0。
 * 用法: node scripts/docs-lint.mjs
 */
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";

const DOCS_ROOT = path.resolve(process.cwd(), "src/content/docs");

const DANGEROUS_SCHEME_RE = /^(javascript|data)\s*:/i;

// URL 部分允许一层括号嵌套（常见于维基百科风格链接 .../Foo_(bar)）
const URL_PART = String.raw`(?:[^()\s]|\([^()]*\))+`;
const LINK_OR_IMAGE_RE = new RegExp(
  `(!)?\\[[^\\]]*\\]\\(\\s*(${URL_PART})(?:\\s+"[^"]*"|\\s+'[^']*')?\\s*\\)`,
  "g",
);

function walkMarkdown(dir) {
  const out = [];
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return out;
  }
  for (const entry of entries) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      out.push(...walkMarkdown(full));
    } else if (entry.isFile() && entry.name.endsWith(".md")) {
      out.push(full);
    }
  }
  return out;
}

/** 图片目标是否为禁止的外链（仅允许相对路径或 /docs/ 开头的绝对路径） */
function isDisallowedImageTarget(url) {
  if (/^\/docs\//.test(url)) return false; // 本地绝对路径，允许
  if (/^[a-z][a-z0-9+.-]*:/i.test(url)) return true; // 任意 scheme（http/https/ftp/mailto…）
  if (url.startsWith("//")) return true; // 协议相对 URL
  if (url.startsWith("/")) return true; // 其他绝对路径（非 /docs/ 开头）
  return false; // 相对路径，允许
}

/** 扫描单个文件，返回 [{ line, message }]；跳过围栏代码块与行内代码，避免示例代码误报 */
function lintFile(filePath) {
  const issues = [];
  const raw = readFileSync(filePath, "utf8");
  const lines = raw.split(/\r\n|\n/);

  let inFence = false;
  let fenceChar = null;

  lines.forEach((rawLine, idx) => {
    const lineNo = idx + 1;

    const fenceMatch = rawLine.match(/^\s*(`{3,}|~{3,})/);
    if (fenceMatch) {
      const marker = fenceMatch[1][0];
      if (!inFence) {
        inFence = true;
        fenceChar = marker;
      } else if (marker === fenceChar) {
        inFence = false;
        fenceChar = null;
      }
      return;
    }
    if (inFence) return;

    // 去除行内代码 span（保留长度，行号/其余内容不受影响），避免示例代码误报
    const line = rawLine.replace(/`[^`]*`/g, (m) => " ".repeat(m.length));

    LINK_OR_IMAGE_RE.lastIndex = 0;
    let match;
    while ((match = LINK_OR_IMAGE_RE.exec(line)) !== null) {
      const isImage = Boolean(match[1]);
      const url = match[2];

      if (DANGEROUS_SCHEME_RE.test(url)) {
        issues.push({
          line: lineNo,
          message: `禁止的链接目标 scheme：\`${url}\`（javascript:/data: 均不允许）`,
        });
        continue;
      }

      if (isImage && isDisallowedImageTarget(url)) {
        issues.push({
          line: lineNo,
          message: `图片禁止外链：\`${url}\`（仅允许相对路径或 /docs/ 开头的本地路径）`,
        });
      }
    }
  });

  return issues;
}

function main() {
  const files = walkMarkdown(DOCS_ROOT).sort();

  if (files.length === 0) {
    const rel = path.relative(process.cwd(), DOCS_ROOT) || "src/content/docs";
    console.log(`docs-lint: 未在 ${rel} 下找到 Markdown 文件，跳过检查。`);
    process.exit(0);
  }

  let totalIssues = 0;
  for (const file of files) {
    const issues = lintFile(file);
    if (issues.length === 0) continue;
    const rel = path.relative(process.cwd(), file).split(path.sep).join("/");
    for (const issue of issues) {
      console.error(`${rel}:${issue.line}  ${issue.message}`);
      totalIssues += 1;
    }
  }

  if (totalIssues > 0) {
    console.error(`\ndocs-lint: 发现 ${totalIssues} 处问题（共扫描 ${files.length} 个文件）。`);
    process.exit(1);
  }

  console.log(`docs-lint: 通过（共扫描 ${files.length} 个文件，未发现问题）。`);
  process.exit(0);
}

main();
