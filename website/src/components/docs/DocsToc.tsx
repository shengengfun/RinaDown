/**
 * 文档右侧目录(client:idle):渲染 h2/h3,IntersectionObserver 高亮当前节。
 */
import { useEffect, useState } from "react";

interface TocHeading {
  depth: number;
  slug: string;
  text: string;
}

interface Props {
  headings: TocHeading[];
  lang: "en" | "zh";
}

export default function DocsToc({ headings, lang }: Props) {
  const items = headings.filter((h) => h.depth === 2 || h.depth === 3);
  const [active, setActive] = useState<string | null>(null);

  useEffect(() => {
    if (items.length === 0) return;
    const targets = items
      .map((h) => document.getElementById(h.slug))
      .filter((el): el is HTMLElement => el !== null);
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) {
            setActive(entry.target.id);
            break;
          }
        }
      },
      { rootMargin: "-80px 0px -70% 0px" },
    );
    for (const el of targets) observer.observe(el);
    return () => observer.disconnect();
    // items 由构建期 props 决定,页面生命周期内不变
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  if (items.length === 0) return null;

  return (
    <nav aria-label={lang === "zh" ? "本页目录" : "On this page"} className="text-sm">
      <h3 className="mb-3 text-xs font-semibold uppercase tracking-wider text-dark-text-muted">
        {lang === "zh" ? "本页目录" : "On this page"}
      </h3>
      <ul className="space-y-1.5 border-l border-dark-border">
        {items.map((h) => (
          <li key={h.slug}>
            <a
              href={`#${h.slug}`}
              className={`block border-l-2 py-0.5 transition-colors ${
                h.depth === 3 ? "pl-6" : "pl-3"
              } ${
                active === h.slug
                  ? "-ml-px border-brand-sky text-brand-sky"
                  : "border-transparent text-dark-text-muted hover:text-dark-text-secondary"
              }`}
            >
              {h.text}
            </a>
          </li>
        ))}
      </ul>
    </nav>
  );
}
