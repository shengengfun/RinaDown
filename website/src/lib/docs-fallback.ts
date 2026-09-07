/**
 * 文档回退页判定 —— 单一实现,供两处共享:
 * 1. astro.config.mjs 的 sitemap({ filter })(该上下文无法使用 getCollection,故用纯 fs)
 * 2. src/pages/docs/[lang]/[...slug].astro 的 getStaticPaths
 *
 * 回退页 = en 存在而 zh 缺失的文档:仍生成 /docs/zh/... 路由(渲染英文内容 + 未翻译横幅),
 * 但带 noindex、不参与 hreflang、不进 sitemap(设计决策 D4)。
 */
import { readdirSync } from "node:fs";
import { join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

const DOCS_DIR = fileURLToPath(new URL("../content/docs", import.meta.url));

function listSlugs(lang: "en" | "zh"): string[] {
  const root = join(DOCS_DIR, lang);
  const out: string[] = [];
  const walk = (dir: string) => {
    let entries;
    try {
      entries = readdirSync(dir, { withFileTypes: true });
    } catch {
      return; // 目录不存在(如 zh 尚未建立)时视为空
    }
    for (const e of entries) {
      const p = join(dir, e.name);
      if (e.isDirectory()) walk(p);
      else if (e.name.endsWith(".md")) {
        out.push(relative(root, p).split(sep).join("/").replace(/\.md$/, ""));
      }
    }
  };
  walk(root);
  return out;
}

/** en 有而 zh 缺的 slug 集合(= 回退页) */
export function getFallbackSlugs(): Set<string> {
  const zh = new Set(listSlugs("zh"));
  return new Set(listSlugs("en").filter((s) => !zh.has(s)));
}

/** 回退页路径(不含域名、不含尾斜杠),供 sitemap filter 比对 */
export function getFallbackPathnames(): Set<string> {
  return new Set([...getFallbackSlugs()].map((s) => `/docs/zh/${s}`));
}
