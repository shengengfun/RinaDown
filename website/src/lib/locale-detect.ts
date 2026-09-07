/**
 * 服务端 Accept-Language 解析,判定规则与客户端 detectLocale(src/lib/i18n.ts)对齐:
 * 按 q 权重顺序遍历,zh 前缀 → zh,en 前缀 → en,否则回退 en。
 */
export function parseAcceptLanguage(header: string | null): "en" | "zh" {
  if (!header) return "en";
  const tags = header
    .split(",")
    .map((part) => {
      const [tag, ...params] = part.trim().split(";");
      const q = params
        .map((p) => p.trim())
        .find((p) => p.startsWith("q="));
      return { tag: (tag ?? "").trim().toLowerCase(), q: q ? Number.parseFloat(q.slice(2)) : 1 };
    })
    .filter((t) => t.tag.length > 0)
    .sort((a, b) => b.q - a.q);
  for (const { tag } of tags) {
    if (tag.startsWith("zh")) return "zh";
    if (tag.startsWith("en")) return "en";
  }
  return "en";
}
