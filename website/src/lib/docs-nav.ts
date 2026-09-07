/**
 * 文档导航树构建:分区元数据 + 从 docs 集合生成分组导航与扁平序列(供上一页/下一页)。
 * zh 导航以 en 集合为全集——缺译页仍出现在导航中(点击进入回退页)。
 */
import type { CollectionEntry } from "astro:content";

export type DocsLang = "en" | "zh";

export const SECTIONS = [
  { id: "getting-started", en: "Getting Started", zh: "快速上手" },
  { id: "protocols", en: "Protocols", zh: "下载协议" },
  { id: "browser-extension", en: "Browser Extension", zh: "浏览器扩展" },
  { id: "headless-server", en: "Headless Server", zh: "Headless 服务器" },
  { id: "api", en: "API", zh: "API" },
  { id: "plugins", en: "Plugin Development", zh: "插件开发" },
  { id: "contributing", en: "Contributing", zh: "参与贡献" },
] as const;

export type SectionId = (typeof SECTIONS)[number]["id"];

export interface DocsNavItem {
  slug: string;
  title: string;
  href: string;
  /** 当前语言缺译(zh 导航中指向回退页) */
  fallback: boolean;
  section: SectionId;
}

export interface DocsNavGroup {
  id: SectionId;
  label: string;
  items: DocsNavItem[];
}

export function buildDocsNav(
  all: CollectionEntry<"docs">[],
  lang: DocsLang,
): { groups: DocsNavGroup[]; flat: DocsNavItem[] } {
  const en = all.filter((e) => e.id.startsWith("en/"));
  const zhMap = new Map(
    all.filter((e) => e.id.startsWith("zh/")).map((e) => [e.id.slice(3), e]),
  );

  const items = en.map((e) => {
    const slug = e.id.slice(3);
    const zh = zhMap.get(slug);
    const use = lang === "zh" && zh ? zh : e;
    return {
      slug,
      title: use.data.title,
      href: `/docs/${lang}/${slug}/`,
      fallback: lang === "zh" && !zh,
      section: e.data.section,
      order: use.data.order,
    };
  });

  const groups: DocsNavGroup[] = [];
  for (const s of SECTIONS) {
    const groupItems = items
      .filter((i) => i.section === s.id)
      .sort((a, b) => a.order - b.order || a.title.localeCompare(b.title));
    if (groupItems.length > 0) {
      groups.push({ id: s.id, label: lang === "zh" ? s.zh : s.en, items: groupItems });
    }
  }
  return { groups, flat: groups.flatMap((g) => g.items) };
}
