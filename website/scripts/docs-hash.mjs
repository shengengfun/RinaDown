#!/usr/bin/env node
/**
 * docs-hash.mjs — 计算并写入文档译文的 sourceHash frontmatter 字段
 *
 * 用法:
 *   node scripts/docs-hash.mjs <zh 文件相对路径>
 *     计算对应 en 原文的内容哈希，写入/更新该 zh 文件 frontmatter 中的 sourceHash 字段
 *     （其余 frontmatter 行保持不动）。找不到对应 en 文件时非零退出并报错。
 *
 *   node scripts/docs-hash.mjs --check-all
 *     遍历 src/content/docs/zh 下所有 Markdown 文件，仅检查 sourceHash 是否缺失/不匹配
 *     （不写入任何文件），报告输出到 stdout，供 CI 写入 $GITHUB_STEP_SUMMARY。
 *     无论检查结果如何，始终以状态码 0 退出（仅供提醒，不阻塞 CI）。
 *
 * sourceHash 规格（须与 CI 报告逻辑保持一致）:
 *   取对应 en 文件"去 frontmatter 后的正文"，统一换行符为 \n 并 trim()，
 *   计算 sha256 十六进制摘要，取前 12 位。
 *
 * 零运行时依赖：仅使用 node:fs / node:crypto / node:path。
 */
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import path from "node:path";

const DOCS_ROOT = path.resolve(process.cwd(), "src/content/docs");

// frontmatter 块：从文件开头的 --- 到下一个 --- 行（含首尾分隔符与结尾换行）
const FRONTMATTER_RE = /^---\r?\n([\s\S]*?)\r?\n---\r?\n/;

/**
 * 拆分文件内容为 frontmatter（不含首尾 --- 的原始文本）与正文。
 * 无 frontmatter 时 frontmatter 为 null，body 为整个原始内容。
 */
function splitFrontmatter(raw) {
  const m = raw.match(FRONTMATTER_RE);
  if (!m) {
    return { frontmatter: null, body: raw };
  }
  return { frontmatter: m[1], body: raw.slice(m[0].length) };
}

/** 按 Contract 规格计算 en 正文的 sourceHash（sha256 hex 前 12 位） */
function computeSourceHash(enRaw) {
  const { body } = splitFrontmatter(enRaw);
  const normalized = body.replace(/\r\n/g, "\n").trim();
  return createHash("sha256").update(normalized, "utf8").digest("hex").slice(0, 12);
}

/** 从 frontmatter 原始文本中读取某字段的值（去除首尾引号），不存在则返回 null */
function extractField(frontmatter, key) {
  const re = new RegExp(`^${key}\\s*:\\s*(.*)$`, "m");
  const m = frontmatter.match(re);
  if (!m) return null;
  return m[1].trim().replace(/^["']|["']$/g, "");
}

/**
 * 在 zh 文件原始内容中写入/更新 sourceHash 字段，其余 frontmatter 行原样保留。
 * zh 文件理论上总有 frontmatter（title 必填），无 frontmatter 时兜底新建一个最小块。
 */
function upsertSourceHash(raw, hash) {
  const m = raw.match(FRONTMATTER_RE);
  if (!m) {
    return `---\nsourceHash: "${hash}"\n---\n${raw}`;
  }
  const lines = m[1].split(/\r\n|\n/);
  const fieldRe = /^sourceHash\s*:/;
  let found = false;
  const newLines = lines.map((line) => {
    if (fieldRe.test(line)) {
      found = true;
      return `sourceHash: "${hash}"`;
    }
    return line;
  });
  if (!found) newLines.push(`sourceHash: "${hash}"`);
  const newFrontmatterBlock = `---\n${newLines.join("\n")}\n---\n`;
  return newFrontmatterBlock + raw.slice(m[0].length);
}

/**
 * 根据 zh 文件路径推导对应 en 文件路径（替换路径中的 zh 目录段为 en）。
 * 优先匹配 .../docs/zh/...；找不到独立的 "zh" 路径段时返回 null。
 */
function resolveEnPath(zhPath) {
  const segments = zhPath.split(path.sep);

  const docsIdx = segments.lastIndexOf("docs");
  if (docsIdx !== -1 && segments[docsIdx + 1] === "zh") {
    const out = [...segments];
    out[docsIdx + 1] = "en";
    return out.join(path.sep);
  }

  const zhIdx = segments.lastIndexOf("zh");
  if (zhIdx !== -1) {
    const out = [...segments];
    out[zhIdx] = "en";
    return out.join(path.sep);
  }

  return null;
}

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

function toDisplayPath(p) {
  return path.relative(process.cwd(), p).split(path.sep).join("/");
}

function writeOne(zhArg) {
  const zhPath = path.resolve(process.cwd(), zhArg);

  if (!existsSync(zhPath)) {
    console.error(`错误: zh 文件不存在: ${zhArg}`);
    process.exit(1);
  }

  const enPath = resolveEnPath(zhPath);
  if (!enPath) {
    console.error(
      `错误: 无法从路径推导出对应的 en 文件（路径中未找到独立的 "zh" 目录段）: ${zhArg}`,
    );
    process.exit(1);
  }
  if (!existsSync(enPath)) {
    console.error(`错误: 找不到对应的 en 文件: ${toDisplayPath(enPath)}`);
    process.exit(1);
  }

  const enRaw = readFileSync(enPath, "utf8");
  const hash = computeSourceHash(enRaw);

  const zhRaw = readFileSync(zhPath, "utf8");
  const updated = upsertSourceHash(zhRaw, hash);
  writeFileSync(zhPath, updated, "utf8");

  console.log(`已写入 sourceHash="${hash}" → ${toDisplayPath(zhPath)}`);
}

function checkAll() {
  if (!existsSync(DOCS_ROOT)) {
    console.log(
      `docs-hash --check-all: 未找到 ${toDisplayPath(DOCS_ROOT) || "src/content/docs"} 目录，跳过检查。`,
    );
    process.exit(0);
  }

  const zhRoot = path.join(DOCS_ROOT, "zh");
  const zhFiles = walkMarkdown(zhRoot).sort();

  if (zhFiles.length === 0) {
    console.log("## 文档翻译 sourceHash 检查报告\n\nsrc/content/docs/zh 下暂无 Markdown 文件，跳过检查。");
    process.exit(0);
  }

  const ok = [];
  const missing = [];
  const mismatched = [];
  const noEnSource = [];

  for (const zhPath of zhFiles) {
    const rel = toDisplayPath(zhPath);
    const enPath = resolveEnPath(zhPath);

    if (!enPath || !existsSync(enPath)) {
      noEnSource.push(rel);
      continue;
    }

    const zhRaw = readFileSync(zhPath, "utf8");
    const { frontmatter } = splitFrontmatter(zhRaw);
    const currentHash = frontmatter ? extractField(frontmatter, "sourceHash") : null;
    const expectedHash = computeSourceHash(readFileSync(enPath, "utf8"));

    if (!currentHash) {
      missing.push(rel);
    } else if (currentHash !== expectedHash) {
      mismatched.push({ rel, currentHash, expectedHash });
    } else {
      ok.push(rel);
    }
  }

  const lines = [];
  lines.push("## 文档翻译 sourceHash 检查报告");
  lines.push("");
  lines.push(`- ✅ 已同步: ${ok.length}`);
  lines.push(`- ⚠️ 缺失 sourceHash: ${missing.length}`);
  lines.push(`- ⚠️ sourceHash 不匹配（译文可能已过期）: ${mismatched.length}`);
  lines.push(`- ℹ️ 无对应 en 源文件（无法校验）: ${noEnSource.length}`);

  if (missing.length) {
    lines.push("");
    lines.push("### 缺失 sourceHash");
    for (const f of missing) lines.push(`- \`${f}\``);
  }
  if (mismatched.length) {
    lines.push("");
    lines.push("### sourceHash 不匹配");
    for (const { rel, currentHash, expectedHash } of mismatched) {
      lines.push(`- \`${rel}\`：当前 \`${currentHash}\` ≠ 期望 \`${expectedHash}\``);
    }
  }
  if (noEnSource.length) {
    lines.push("");
    lines.push("### 无对应 en 源文件");
    for (const f of noEnSource) lines.push(`- \`${f}\``);
  }

  lines.push("");
  lines.push(
    "提示：运行 `npm run docs:hash <zh 文件相对路径>` 可自动写入/更新 sourceHash（详见 CONTRIBUTING.md）。",
  );

  console.log(lines.join("\n"));
  process.exit(0); // 仅供提醒，永远不 fail
}

function main() {
  const args = process.argv.slice(2);

  if (args.includes("--check-all")) {
    checkAll();
    return;
  }

  const target = args[0];
  if (!target) {
    console.error("用法: node scripts/docs-hash.mjs <zh 文件相对路径>");
    console.error("      node scripts/docs-hash.mjs --check-all");
    process.exit(1);
  }

  writeOne(target);
}

main();
