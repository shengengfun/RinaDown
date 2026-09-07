/**
 * 文档搜索(client:idle):侧边栏触发按钮 + 命令面板式弹层。
 *
 * 数据来源:懒加载 `/docs/search-{lang}.json`(构建期预渲染索引),
 * 在浏览器内经 searchDocs 做模糊 + 全文匹配。快捷键 ⌘/Ctrl+K 唤起,↑↓ 选择,Enter 跳转,Esc 关闭。
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { searchDocs, type SearchDoc, type SearchResult } from "../../lib/docs-search";
import { SECTIONS, type DocsLang } from "../../lib/docs-nav";

interface Props {
  lang: DocsLang;
}

const SECTION_LABEL: Record<string, { en: string; zh: string }> = Object.fromEntries(
  SECTIONS.map((s) => [s.id, { en: s.en, zh: s.zh }]),
);

export default function DocsSearch({ lang }: Props) {
  const zh = lang === "zh";
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [docs, setDocs] = useState<SearchDoc[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [active, setActive] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLUListElement>(null);

  const t = useMemo(
    () => ({
      placeholder: zh ? "搜索文档…" : "Search docs…",
      trigger: zh ? "搜索文档" : "Search docs",
      empty: zh ? "未找到相关内容" : "No results found",
      loading: zh ? "加载索引…" : "Loading index…",
      hint: zh ? "输入关键字开始搜索" : "Type to search",
    }),
    [zh],
  );

  // 懒加载搜索索引(首次打开时)
  const loadIndex = useCallback(async () => {
    if (docs || loading) return;
    setLoading(true);
    try {
      const res = await fetch(`/docs/search-${lang}.json`);
      if (res.ok) setDocs((await res.json()) as SearchDoc[]);
    } catch {
      /* 网络失败:保持空索引,展示无结果 */
    } finally {
      setLoading(false);
    }
  }, [docs, loading, lang]);

  // 全局快捷键 ⌘/Ctrl+K 与 /
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setOpen(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // 打开时加载索引 + 聚焦 + 锁滚动
  useEffect(() => {
    if (!open) return;
    void loadIndex();
    const raf = requestAnimationFrame(() => inputRef.current?.focus());
    const prev = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => {
      cancelAnimationFrame(raf);
      document.body.style.overflow = prev;
    };
  }, [open, loadIndex]);

  const results: SearchResult[] = useMemo(
    () => (docs ? searchDocs(docs, query, 20) : []),
    [docs, query],
  );

  useEffect(() => setActive(0), [query]);

  const go = useCallback((r: SearchResult | undefined) => {
    if (r) window.location.href = r.doc.href;
  }, []);

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      setOpen(false);
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((a) => Math.min(a + 1, results.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((a) => Math.max(a - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      go(results[active]);
    }
  };

  // 键盘选中项滚入可视区
  useEffect(() => {
    const el = listRef.current?.querySelector<HTMLElement>(`[data-idx="${active}"]`);
    el?.scrollIntoView({ block: "nearest" });
  }, [active]);

  const label = (section: string) =>
    zh ? (SECTION_LABEL[section]?.zh ?? section) : (SECTION_LABEL[section]?.en ?? section);

  return (
    <>
      {/* 侧边栏触发按钮 */}
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="mb-4 flex w-full items-center gap-2 rounded-lg border border-dark-border bg-dark-surface1/60 px-3 py-2 text-sm text-dark-text-muted transition-colors hover:border-brand-sky/40 hover:text-dark-text-secondary"
        aria-label={t.trigger}
      >
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <circle cx="11" cy="11" r="8" />
          <path d="m21 21-4.3-4.3" />
        </svg>
        <span className="flex-1 text-left">{t.trigger}</span>
        <kbd className="rounded border border-dark-border px-1.5 py-0.5 text-[10px] font-medium text-dark-text-muted">
          {typeof navigator !== "undefined" && /Mac/i.test(navigator.platform) ? "⌘K" : "Ctrl K"}
        </kbd>
      </button>

      {open && (
        <div
          className="fixed inset-0 z-[100] flex items-start justify-center bg-black/60 px-4 pt-[12vh] backdrop-blur-sm"
          onClick={() => setOpen(false)}
          role="presentation"
        >
          <div
            className="w-full max-w-xl overflow-hidden rounded-2xl border border-dark-border bg-dark-surface1 shadow-2xl"
            onClick={(e) => e.stopPropagation()}
            role="dialog"
            aria-modal="true"
            aria-label={t.trigger}
          >
            {/* 输入行 */}
            <div className="flex items-center gap-3 border-b border-dark-border px-4">
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="text-dark-text-muted" aria-hidden="true">
                <circle cx="11" cy="11" r="8" />
                <path d="m21 21-4.3-4.3" />
              </svg>
              <input
                ref={inputRef}
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                onKeyDown={onKeyDown}
                placeholder={t.placeholder}
                className="flex-1 bg-transparent py-4 text-base text-dark-text outline-none placeholder:text-dark-text-muted"
                autoComplete="off"
                spellCheck={false}
              />
              <button
                type="button"
                onClick={() => setOpen(false)}
                className="rounded border border-dark-border px-1.5 py-0.5 text-[10px] text-dark-text-muted hover:text-dark-text-secondary"
              >
                Esc
              </button>
            </div>

            {/* 结果区 */}
            <div className="max-h-[55vh] overflow-y-auto">
              {loading && !docs ? (
                <p className="px-4 py-8 text-center text-sm text-dark-text-muted">{t.loading}</p>
              ) : query.trim().length === 0 ? (
                <p className="px-4 py-8 text-center text-sm text-dark-text-muted">{t.hint}</p>
              ) : results.length === 0 ? (
                <p className="px-4 py-8 text-center text-sm text-dark-text-muted">{t.empty}</p>
              ) : (
                <ul ref={listRef} className="py-2">
                  {results.map((r, i) => (
                    <li key={r.doc.slug} data-idx={i}>
                      <a
                        href={r.doc.href}
                        onMouseEnter={() => setActive(i)}
                        className={`block border-l-2 px-4 py-2.5 transition-colors ${
                          i === active
                            ? "border-brand-sky bg-brand-sky/10"
                            : "border-transparent hover:bg-dark-surface2"
                        }`}
                      >
                        <div className="flex items-center gap-2">
                          <span className="text-sm font-medium text-dark-text">{r.doc.title}</span>
                          <span className="rounded bg-dark-surface2 px-1.5 py-0.5 text-[10px] text-dark-text-muted">
                            {label(r.doc.section)}
                          </span>
                          {r.matchedHeading && (
                            <span className="truncate text-xs text-dark-text-muted">
                              › {r.matchedHeading}
                            </span>
                          )}
                        </div>
                        {r.snippet.length > 0 && (
                          <p className="mt-1 line-clamp-2 text-xs leading-relaxed text-dark-text-secondary">
                            {r.snippet.map((p, k) =>
                              p.hit ? (
                                <mark key={k} className="rounded bg-brand-sky/25 px-0.5 text-brand-sky">
                                  {p.text}
                                </mark>
                              ) : (
                                <span key={k}>{p.text}</span>
                              ),
                            )}
                          </p>
                        )}
                      </a>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </div>
        </div>
      )}
    </>
  );
}
