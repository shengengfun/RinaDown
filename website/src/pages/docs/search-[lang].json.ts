/**
 * 文档搜索索引(构建期预渲染,en/zh 各一份 JSON)。
 *
 * 客户端 DocsSearch 组件 fetch `/docs/search-{lang}.json` 后在浏览器内做模糊 + 全文匹配,
 * 无需服务端搜索接口(与全站静态预渲染一致)。zh 缺译页沿用 en 正文(与导航/回退策略一致)。
 */
import type { APIRoute, GetStaticPaths } from "astro";
import { getCollection } from "astro:content";
import { buildDocsNav, type DocsLang } from "../../lib/docs-nav";

export const prerender = true;

export const getStaticPaths: GetStaticPaths = () => [
  { params: { lang: "en" } },
  { params: { lang: "zh" } },
];

/** Markdown 正文 → 可搜索纯文本(去代码块/标记/链接语法,压缩空白)。 */
function toPlainText(md: string): string {
  return md
    .replace(/```[\s\S]*?```/g, " ") // 围栏代码块
    .replace(/`[^`]*`/g, " ") // 行内代码
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1") // 图片/链接 → 文字
    .replace(/^#{1,6}\s+/gm, "") // 标题井号
    .replace(/[*_>#~|]/g, " ") // 其余标记符号
    .replace(/\s+/g, " ")
    .trim();
}

export const GET: APIRoute = async ({ params }) => {
  const lang = params.lang as DocsLang;
  const all = await getCollection("docs");

  // 以 en 为 slug 全集,zh 优先取译文正文,缺译回退 en(与 buildDocsNav 一致)
  const en = all.filter((e) => e.id.startsWith("en/"));
  const zhMap = new Map(
    all.filter((e) => e.id.startsWith("zh/")).map((e) => [e.id.slice(3), e]),
  );
  const { flat } = buildDocsNav(all, lang);
  const titleBySlug = new Map(flat.map((i) => [i.slug, i.title]));

  const docs = en.map((e) => {
    const slug = e.id.slice(3);
    const entry = lang === "zh" ? (zhMap.get(slug) ?? e) : e;
    const body = entry.body ?? "";
    const headings = [...body.matchAll(/^#{2,6}\s+(.+?)\s*$/gm)].map((m) =>
      (m[1] ?? "").replace(/[*_`~]/g, "").trim(),
    );
    return {
      slug,
      href: `/docs/${lang}/${slug}/`,
      title: titleBySlug.get(slug) ?? entry.data.title,
      section: e.data.section,
      description: entry.data.description ?? "",
      headings,
      text: toPlainText(body),
    };
  });

  return new Response(JSON.stringify(docs), {
    headers: {
      "Content-Type": "application/json; charset=utf-8",
      "Cache-Control": "public, max-age=3600",
    },
  });
};
