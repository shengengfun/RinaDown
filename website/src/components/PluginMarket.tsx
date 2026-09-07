import { useEffect, useMemo, useState, useCallback } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { useLocale } from "@/lib/i18n";

// ── 数据类型（对应 fluxdown-plugin-index 仓库 index.json）──

const INDEX_REPO = "zerx-lab/fluxdown-plugin-index";
const REPO_URL = `https://github.com/${INDEX_REPO}`;

/** 索引条目：每插件每版本一条（append-only 分片 flatten 而来）。 */
interface MarketEntry {
  pluginId: string;
  version: string;
  sequence: number;
  contentHash: string;
  minAppVersion?: string;
  name?: string;
  description?: string;
  author?: string;
  homepage?: string;
  mirrors?: string[];
  publishTime?: string;
  yanked?: string;
  tags?: string[];
  permissions?: string[];
}

interface MarketIndex {
  indexId?: string;
  sequence?: number;
  updated?: string;
  entries?: MarketEntry[];
}

/** 按 pluginId 聚合后的插件（含全部历史版本，降序）。 */
interface PluginGroup {
  pluginId: string;
  latest: MarketEntry;
  versions: MarketEntry[];
}

/** 仓库相对路径 → 同源代理 URL（服务端中转 raw.githubusercontent.com，大陆可达 + CDN 缓存）。 */
function proxyUrl(path: string): string {
  return `/api/plugins/${path}`;
}

/** 每插件的提交历史深链——Git 是 Merkle DAG，逐版本变更可独立追踪。 */
function commitsUrl(pluginId: string): string {
  return `${REPO_URL}/commits/main/plugins/${encodeURIComponent(pluginId)}`;
}

/** 优先取 jsDelivr 镜像（大陆可达 + CDN），回退首个镜像。 */
function bestMirror(entry: MarketEntry): string | null {
  const mirrors = entry.mirrors ?? [];
  return mirrors.find((m) => m.includes("jsdelivr")) ?? mirrors[0] ?? null;
}

/** ISO 时间戳 → YYYY-MM-DD（确定性、与语言无关）。 */
function isoDate(iso?: string): string {
  return iso && iso.length >= 10 ? iso.slice(0, 10) : "";
}

/** `sha256:<hex>` → 前 12 位短哈希（展示用）。 */
function shortHash(hash: string): string {
  const hex = hash.startsWith("sha256:") ? hash.slice(7) : hash;
  return hex.slice(0, 12);
}

/** 把索引条目按 pluginId 聚合，版本按 sequence 降序，组按最新版发布时间降序。 */
function groupPlugins(entries: MarketEntry[]): PluginGroup[] {
  const byId = new Map<string, MarketEntry[]>();
  for (const e of entries) {
    const list = byId.get(e.pluginId);
    if (list) list.push(e);
    else byId.set(e.pluginId, [e]);
  }
  const groups: PluginGroup[] = [];
  for (const [pluginId, versions] of byId) {
    versions.sort((a, b) => b.sequence - a.sequence);
    groups.push({ pluginId, latest: versions[0], versions });
  }
  groups.sort(
    (a, b) => (b.latest.publishTime ?? "").localeCompare(a.latest.publishTime ?? ""),
  );
  return groups;
}

// ── 图标 ──

function Icon({ path, size = 14 }: { path: string; size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d={path} />
    </svg>
  );
}

const ICON_PUZZLE =
  "M19.439 7.85c-.049.322.059.648.289.878l1.568 1.568c.47.47.706 1.087.706 1.704s-.235 1.233-.706 1.704l-1.611 1.611a.98.98 0 0 1-.837.276c-.47-.07-.802-.48-.968-.925a2.501 2.501 0 1 0-3.214 3.214c.446.166.855.497.925.968a.979.979 0 0 1-.276.837l-1.61 1.61a2.404 2.404 0 0 1-1.705.707 2.402 2.402 0 0 1-1.704-.706l-1.568-1.568a1.026 1.026 0 0 0-.877-.29c-.493.074-.84.504-1.02.968a2.5 2.5 0 1 1-3.237-3.237c.464-.18.894-.527.967-1.02a1.026 1.026 0 0 0-.289-.877l-1.568-1.568A2.402 2.402 0 0 1 1.998 12c0-.617.236-1.234.706-1.704L4.23 8.77c.24-.24.581-.353.917-.303.515.077.877.528 1.073 1.01a2.5 2.5 0 1 0 3.259-3.259c-.482-.196-.933-.558-1.01-1.073-.05-.336.062-.676.303-.917l1.525-1.525A2.402 2.402 0 0 1 12 1.998c.617 0 1.234.236 1.704.706l1.568 1.568c.23.23.556.338.877.29.493-.075.84-.505 1.02-.969a2.5 2.5 0 1 1 3.237 3.237c-.464.18-.894.527-.967 1.02z";
const ICON_CLOCK = "M12 8v4l3 3M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20z";
const ICON_DOWNLOAD = "M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4M7 10l5 5 5-5M12 15V3";
const ICON_EXTERNAL = "M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6M15 3h6v6M10 14 21 3";
const ICON_SEARCH = "M11 11m-8 0a8 8 0 1 0 16 0a8 8 0 1 0-16 0M21 21l-4.3-4.3";
const ICON_SHIELD = "M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z";
const ICON_CLOSE = "M18 6 6 18M6 6l12 12";

// ── 权限徽章 ──

function PermissionBadge({
  perm,
  t,
}: {
  perm: string;
  t: (key: never, params?: Record<string, string>) => string;
}) {
  const known = perm === "ffmpeg" || perm === "ytdlp";
  const title = known ? t(`plugins.permission.${perm}` as never) : "";
  return (
    <span
      title={title}
      className="inline-flex items-center gap-1 rounded-full border border-warning/30 bg-warning/10 px-2 py-0.5 text-[10px] font-medium text-warning"
    >
      <Icon path={ICON_SHIELD} size={10} />
      {perm}
    </span>
  );
}

// ── yanked 徽章 ──

function YankedBadge({
  yanked,
  t,
}: {
  yanked: string;
  t: (key: never, params?: Record<string, string>) => string;
}) {
  if (!yanked || yanked === "none") return null;
  return (
    <span className="inline-flex items-center rounded-full border border-danger/30 bg-danger/10 px-2 py-0.5 text-[10px] font-medium text-danger">
      {t(`plugins.yanked.${yanked}` as never)}
    </span>
  );
}

// ── 插件卡片 ──

function PluginCard({
  group,
  onOpen,
  t,
  index,
}: {
  group: PluginGroup;
  onOpen: () => void;
  t: (key: never, params?: Record<string, string>) => string;
  index: number;
}) {
  const p = group.latest;
  const name = p.name || group.pluginId;

  return (
    <motion.button
      type="button"
      onClick={onOpen}
      initial={index < 3 ? false : { opacity: 0, y: 20 }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once: true, margin: "-50px" }}
      transition={{ duration: 0.4, delay: (index % 6) * 0.05 }}
      className="group flex flex-col text-left rounded-2xl border border-dark-border bg-dark-surface1/50 p-5 backdrop-blur-sm hover:border-brand-sky/40 transition-colors cursor-pointer"
    >
      {/* 名称 + 版本 */}
      <div className="flex items-start justify-between gap-2">
        <div className="flex items-center gap-2 min-w-0">
          <span className="shrink-0 grid place-items-center h-9 w-9 rounded-xl bg-brand-sky/10 text-brand-sky">
            <Icon path={ICON_PUZZLE} size={18} />
          </span>
          <div className="min-w-0">
            <h3 className="text-sm font-semibold text-dark-text truncate">{name}</h3>
            <span className="text-[11px] text-dark-text-muted font-mono">{group.pluginId}</span>
          </div>
        </div>
        <span className="shrink-0 text-[10px] text-dark-text-muted font-mono">v{p.version}</span>
      </div>

      {/* yanked */}
      {p.yanked && p.yanked !== "none" && (
        <div className="mt-2">
          <YankedBadge yanked={p.yanked} t={t} />
        </div>
      )}

      {/* 描述 */}
      {p.description && (
        <p className="mt-3 text-xs text-dark-text-secondary line-clamp-3 leading-relaxed flex-1">
          {p.description}
        </p>
      )}

      {/* tags + permissions */}
      <div className="mt-3 flex flex-wrap items-center gap-1.5">
        {(p.permissions ?? []).map((perm) => (
          <PermissionBadge key={perm} perm={perm} t={t} />
        ))}
        {(p.tags ?? []).slice(0, 3).map((tag) => (
          <span
            key={tag}
            className="rounded-full border border-dark-border px-2 py-0.5 text-[10px] text-dark-text-muted"
          >
            {tag}
          </span>
        ))}
      </div>

      {/* footer：作者 + 版本数 */}
      <div className="mt-4 flex items-center justify-between gap-2 text-[11px] text-dark-text-muted">
        <span className="truncate">@{p.author || "unknown"}</span>
        <span className="inline-flex items-center gap-1">
          <Icon path={ICON_CLOCK} size={11} />
          {t("plugins.card.versions" as never, { count: String(group.versions.length) })}
        </span>
      </div>
    </motion.button>
  );
}

// ── 详情弹层（含版本历史时间线）──

function PluginDetail({
  group,
  onClose,
  t,
}: {
  group: PluginGroup;
  onClose: () => void;
  t: (key: never, params?: Record<string, string>) => string;
}) {
  const p = group.latest;
  const name = p.name || group.pluginId;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      onClick={onClose}
      className="fixed inset-0 z-[100] flex items-start justify-center overflow-y-auto bg-black/70 backdrop-blur-sm p-4 sm:p-8"
    >
      <motion.div
        initial={{ opacity: 0, scale: 0.96, y: 20 }}
        animate={{ opacity: 1, scale: 1, y: 0 }}
        exit={{ opacity: 0, scale: 0.96, y: 20 }}
        transition={{ duration: 0.2 }}
        onClick={(e) => e.stopPropagation()}
        className="relative w-full max-w-2xl rounded-2xl border border-dark-border bg-dark-surface1 shadow-2xl my-auto"
      >
        {/* 关闭 */}
        <button
          type="button"
          onClick={onClose}
          aria-label="Close"
          className="absolute right-3 top-3 grid place-items-center h-8 w-8 rounded-lg text-dark-text-muted hover:text-dark-text hover:bg-dark-surface2 transition-colors"
        >
          <Icon path={ICON_CLOSE} size={16} />
        </button>

        <div className="p-6 sm:p-8">
          {/* 头部 */}
          <div className="flex items-start gap-3 pr-8">
            <span className="shrink-0 grid place-items-center h-11 w-11 rounded-xl bg-brand-sky/10 text-brand-sky">
              <Icon path={ICON_PUZZLE} size={22} />
            </span>
            <div className="min-w-0">
              <div className="flex items-center gap-2 flex-wrap">
                <h2 className="text-lg font-bold text-dark-text">{name}</h2>
                <span className="text-[11px] text-dark-text-muted font-mono">v{p.version}</span>
                <YankedBadge yanked={p.yanked ?? "none"} t={t} />
              </div>
              <span className="text-xs text-dark-text-muted font-mono">{group.pluginId}</span>
            </div>
          </div>

          {/* 描述 */}
          {p.description && (
            <p className="mt-4 text-sm text-dark-text-secondary leading-relaxed">{p.description}</p>
          )}

          {/* 元信息 */}
          <div className="mt-4 flex flex-wrap items-center gap-1.5">
            {(p.permissions ?? []).map((perm) => (
              <PermissionBadge key={perm} perm={perm} t={t} />
            ))}
            {(p.tags ?? []).map((tag) => (
              <span
                key={tag}
                className="rounded-full border border-dark-border px-2 py-0.5 text-[10px] text-dark-text-muted"
              >
                {tag}
              </span>
            ))}
          </div>

          {/* 作者 / 主页 / 最低版本 */}
          <dl className="mt-4 grid grid-cols-2 gap-x-4 gap-y-2 text-xs">
            <div>
              <dt className="text-dark-text-muted">{t("plugins.detail.author" as never)}</dt>
              <dd className="text-dark-text-secondary">@{p.author || "unknown"}</dd>
            </div>
            {p.minAppVersion && (
              <div>
                <dt className="text-dark-text-muted">{t("plugins.detail.minApp" as never)}</dt>
                <dd className="text-dark-text-secondary font-mono">v{p.minAppVersion}</dd>
              </div>
            )}
            {p.homepage && (
              <div className="col-span-2">
                <dt className="text-dark-text-muted">{t("plugins.detail.homepage" as never)}</dt>
                <dd>
                  <a
                    href={p.homepage}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="text-brand-sky hover:underline break-all"
                  >
                    {p.homepage}
                  </a>
                </dd>
              </div>
            )}
          </dl>

          {/* 安装提示 */}
          <div className="mt-5 rounded-xl border border-dark-border bg-dark-surface2/40 p-4">
            <p className="text-xs text-dark-text-secondary leading-relaxed">
              {t("plugins.detail.installHint" as never, { id: group.pluginId })}
            </p>
          </div>

          {/* 版本历史时间线（可追踪的核心） */}
          <div className="mt-6">
            <div className="flex items-center justify-between gap-2 mb-3">
              <h3 className="text-sm font-semibold text-dark-text">
                {t("plugins.detail.history" as never)}
              </h3>
              <a
                href={commitsUrl(group.pluginId)}
                target="_blank"
                rel="noopener noreferrer"
                className="inline-flex items-center gap-1 text-[11px] text-brand-sky hover:underline"
              >
                {t("plugins.detail.viewCommits" as never)}
                <Icon path={ICON_EXTERNAL} size={11} />
              </a>
            </div>

            <ol className="relative border-l border-dark-border pl-5 space-y-4">
              {group.versions.map((v, i) => {
                const mirror = bestMirror(v);
                return (
                  <li key={`${v.version}-${v.sequence}`} className="relative">
                    <span
                      className={`absolute -left-[26px] top-1 h-2.5 w-2.5 rounded-full ring-2 ring-dark-surface1 ${
                        i === 0 ? "bg-brand-sky" : "bg-dark-text-muted"
                      }`}
                    />
                    <div className="flex items-center justify-between gap-2 flex-wrap">
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-semibold text-dark-text font-mono">
                          v{v.version}
                        </span>
                        {i === 0 && (
                          <span className="rounded-full bg-brand-sky/15 px-2 py-0.5 text-[10px] font-medium text-brand-sky">
                            {t("plugins.detail.latest" as never)}
                          </span>
                        )}
                        <YankedBadge yanked={v.yanked ?? "none"} t={t} />
                      </div>
                      {isoDate(v.publishTime) && (
                        <span className="text-[11px] text-dark-text-muted font-mono">
                          {isoDate(v.publishTime)}
                        </span>
                      )}
                    </div>
                    <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-dark-text-muted">
                      <span>seq #{v.sequence}</span>
                      {v.minAppVersion && (
                        <span className="font-mono">min v{v.minAppVersion}</span>
                      )}
                      <span
                        className="font-mono truncate"
                        title={v.contentHash}
                      >
                        sha256:{shortHash(v.contentHash)}
                      </span>
                    </div>
                    {mirror && (
                      <a
                        href={mirror}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="mt-1.5 inline-flex items-center gap-1 text-[11px] text-brand-sky hover:underline"
                      >
                        <Icon path={ICON_DOWNLOAD} size={11} />
                        {t("plugins.detail.download" as never)}
                      </a>
                    )}
                  </li>
                );
              })}
            </ol>
          </div>
        </div>
      </motion.div>
    </motion.div>
  );
}

// ── 页面 ──

export default function PluginMarket() {
  const { t } = useLocale();
  const [groups, setGroups] = useState<PluginGroup[] | null>(null);
  const [error, setError] = useState(false);
  const [loading, setLoading] = useState(true);
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<string>("all");
  const [selected, setSelected] = useState<PluginGroup | null>(null);

  useEffect(() => {
    fetch(proxyUrl("index.json"))
      .then((r) => (r.ok ? r.json() : Promise.reject(new Error(String(r.status)))))
      .then((data: MarketIndex) => setGroups(groupPlugins(data.entries ?? [])))
      .catch(() => setError(true))
      .finally(() => setLoading(false));
  }, []);

  // 可用筛选：all + 数据中出现过的权限 + resolver 标签
  const filters = useMemo(() => {
    const set = new Set<string>();
    for (const g of groups ?? []) {
      for (const perm of g.latest.permissions ?? []) set.add(perm);
      if ((g.latest.tags ?? []).includes("resolver")) set.add("resolver");
    }
    return ["all", ...Array.from(set)];
  }, [groups]);

  const filtered = useMemo(() => {
    let list = groups ?? [];
    if (filter !== "all") {
      list = list.filter((g) =>
        filter === "resolver"
          ? (g.latest.tags ?? []).includes("resolver")
          : (g.latest.permissions ?? []).includes(filter),
      );
    }
    const q = query.trim().toLowerCase();
    if (!q) return list;
    return list.filter((g) => {
      const p = g.latest;
      return (
        g.pluginId.toLowerCase().includes(q) ||
        (p.name ?? "").toLowerCase().includes(q) ||
        (p.description ?? "").toLowerCase().includes(q) ||
        (p.author ?? "").toLowerCase().includes(q) ||
        (p.tags ?? []).some((tag) => tag.toLowerCase().includes(q))
      );
    });
  }, [groups, query, filter]);

  const filterLabel = useCallback(
    (f: string): string => {
      if (f === "all") return t("plugins.filter.all" as never);
      if (f === "resolver") return t("plugins.filter.resolver" as never);
      return f;
    },
    [t],
  );

  return (
    <section className="pt-24 sm:pt-32 pb-16 sm:pb-20">
      <div className="mx-auto max-w-6xl px-4 sm:px-6 lg:px-8">
        {/* 页头 */}
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.5 }}
          className="text-center mb-10 sm:mb-14"
        >
          <span className="inline-flex items-center gap-2 rounded-full border border-dark-border bg-dark-surface1/50 px-4 py-1.5 text-xs font-medium text-dark-text-secondary backdrop-blur-sm mb-6">
            <span className="text-brand-sky">
              <Icon path={ICON_PUZZLE} size={14} />
            </span>
            {t("plugins.badge")}
          </span>

          <h1 className="text-4xl sm:text-5xl font-bold tracking-tight leading-tight">
            {t("plugins.title")}
          </h1>

          <p className="mt-4 text-base sm:text-lg text-dark-text-secondary max-w-2xl mx-auto leading-relaxed">
            {t("plugins.subtitle")}
          </p>

          {/* 搜索 + 提交入口 */}
          <div className="mt-8 flex flex-col sm:flex-row items-center justify-center gap-3">
            <div className="relative w-full sm:w-80">
              <span className="absolute left-3 top-1/2 -translate-y-1/2 text-dark-text-muted">
                <Icon path={ICON_SEARCH} size={15} />
              </span>
              <input
                type="text"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder={t("plugins.searchPlaceholder")}
                className="w-full rounded-full border border-dark-border bg-dark-surface1/50 pl-9 pr-4 py-2 text-sm text-dark-text placeholder:text-dark-text-muted focus:outline-none focus:border-brand-sky/50 transition-colors backdrop-blur-sm"
              />
            </div>
            <a
              href={`${REPO_URL}#publishing-a-plugin`}
              target="_blank"
              rel="noopener noreferrer"
              className="inline-flex items-center gap-1.5 rounded-full border border-dark-border bg-dark-surface1/50 px-4 py-2 text-xs font-medium text-dark-text-secondary hover:text-dark-text hover:border-dark-text-muted/40 transition-colors backdrop-blur-sm"
            >
              <Icon path={ICON_EXTERNAL} size={13} />
              {t("plugins.submitCta")}
            </a>
          </div>

          {/* 筛选 pills */}
          {!loading && !error && filters.length > 1 && (
            <div className="mt-5 flex flex-wrap items-center justify-center gap-2">
              {filters.map((f) => (
                <button
                  key={f}
                  type="button"
                  onClick={() => setFilter(f)}
                  className={`rounded-full px-3 py-1 text-xs font-medium transition-colors ${
                    filter === f
                      ? "bg-brand-sky/15 text-brand-sky border border-brand-sky/40"
                      : "border border-dark-border text-dark-text-muted hover:text-dark-text-secondary"
                  }`}
                >
                  {filterLabel(f)}
                </button>
              ))}
            </div>
          )}
        </motion.div>

        {/* 加载态 */}
        {loading && (
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-5">
            {Array.from({ length: 6 }, (_, i) => (
              <div
                key={i}
                className="rounded-2xl border border-dark-border bg-dark-surface1/30 p-5 animate-pulse"
              >
                <div className="flex items-center gap-2">
                  <div className="h-9 w-9 rounded-xl bg-dark-surface2/60" />
                  <div className="flex-1 space-y-2">
                    <div className="h-4 w-2/3 rounded bg-dark-surface2/60" />
                    <div className="h-3 w-1/3 rounded bg-dark-surface2/40" />
                  </div>
                </div>
                <div className="mt-4 space-y-2">
                  <div className="h-3 w-full rounded bg-dark-surface2/40" />
                  <div className="h-3 w-4/5 rounded bg-dark-surface2/40" />
                </div>
              </div>
            ))}
          </div>
        )}

        {/* 错误态 */}
        {error && !loading && (
          <div className="text-center py-20">
            <p className="text-sm text-dark-text-muted">{t("plugins.loadError")}</p>
            <a
              href={REPO_URL}
              target="_blank"
              rel="noopener noreferrer"
              className="mt-3 inline-block text-xs text-brand-sky hover:underline"
            >
              github.com/{INDEX_REPO} →
            </a>
          </div>
        )}

        {/* 空态 */}
        {!loading && !error && filtered.length === 0 && (
          <div className="text-center py-20">
            <p className="text-sm text-dark-text-muted">{t("plugins.empty")}</p>
          </div>
        )}

        {/* 插件网格 */}
        {!loading && !error && filtered.length > 0 && (
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-5">
            {filtered.map((g, i) => (
              <PluginCard
                key={g.pluginId}
                group={g}
                index={i}
                onOpen={() => setSelected(g)}
                t={t as never}
              />
            ))}
          </div>
        )}

        {/* 底部：如何使用 */}
        {!loading && !error && (
          <motion.div
            initial={{ opacity: 0 }}
            whileInView={{ opacity: 1 }}
            viewport={{ once: true }}
            className="mt-14 rounded-2xl border border-dark-border bg-dark-surface1/40 p-6 backdrop-blur-sm"
          >
            <h2 className="text-sm font-semibold text-dark-text mb-3">{t("plugins.howTo.title")}</h2>
            <ol className="space-y-2 text-xs text-dark-text-secondary leading-relaxed list-decimal list-inside">
              <li>{t("plugins.howTo.step1")}</li>
              <li>{t("plugins.howTo.step2")}</li>
              <li>
                {t("plugins.howTo.step3")}{" "}
                <a
                  href={`${REPO_URL}#publishing-a-plugin`}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-brand-sky hover:underline"
                >
                  {t("plugins.howTo.guideLink")}
                </a>
              </li>
            </ol>
          </motion.div>
        )}
      </div>

      {/* 详情弹层 */}
      <AnimatePresence>
        {selected && (
          <PluginDetail group={selected} onClose={() => setSelected(null)} t={t as never} />
        )}
      </AnimatePresence>
    </section>
  );
}
