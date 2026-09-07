/**
 * 译文过期检测哈希(设计决策 D5)。
 * 规格与 scripts/docs-hash.mjs 必须保持一致:
 * 去 frontmatter 正文 → CRLF 归一化为 LF → trim → sha256 hex 前 12 位。
 */
import { createHash } from "node:crypto";

export function docsSourceHash(body: string): string {
  return createHash("sha256")
    .update(body.replace(/\r\n/g, "\n").trim())
    .digest("hex")
    .slice(0, 12);
}
