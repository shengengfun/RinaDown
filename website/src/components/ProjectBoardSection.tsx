import { useState, useEffect, useMemo, useRef } from "react";
import { motion } from "framer-motion";
import {
  CircleDot,
  CircleCheck,
  CircleSlash,
  Clock,
  MessageSquare,
  Kanban,
  Table2,
  ChevronUp,
  ChevronDown,
  ChevronRight,
  ChevronsUpDown,
  Filter,
  Search,
  X,
} from "lucide-react";
import { ThemeProvider, Label, Spinner, Flash } from "@primer/react";
import { useLocale } from "@/lib/i18n";

// ── 类型定义 ──

interface ViewConfig {
  id: string;
  name: string;
  number: number;
  layout: "TABLE_LAYOUT" | "BOARD_LAYOUT";
  filter: string;
  visibleFields: string[];
  groupByField: string | null;
  hasGroupBy: boolean;
}

interface BoardItem {
  id: string;
  issueNumber: number;
  title: string;
  state: "OPEN" | "CLOSED";
  stateReason: string | null;
  labels: Array<{ name: string; color: string }>;
  createdAt: string;
  url: string;
  comments: number;
  statusId: string | null;
  statusName: string | null;
  fieldValues: Record<string, { optionId: string; optionName: string }>;
}

interface BoardColumn {
  id: string;
  name: string;
  color: string;
  items: BoardItem[];
}

interface BoardData {
  views: ViewConfig[];
  columns: BoardColumn[];
  noStatusItems: BoardItem[];
  allItems: BoardItem[];
  totalItems: number;
  projectTitle: string;
  cachedAt: string;
  singleSelectFields: Array<{
    id: string;
    name: string;
    options: Array<{ id: string; name: string; color: string }>;
  }>;
}

interface ProjectBoardSectionProps {
  onIssueClick?: (issueNumber: number) => void;
}

// ── 搜索工具函数 ──

function matchesSearch(item: BoardItem, q: string): boolean {
  if (!q) return true;
  const lower = q.toLowerCase();
  return (
    String(item.issueNumber).includes(lower) ||
    item.title.toLowerCase().includes(lower)
  );
}

// ── GitHub 颜色映射 ──

const GITHUB_COLOR_MAP: Record<string, string> = {
  GRAY: "#6b7280",
  BLUE: "#3b82f6",
  GREEN: "#22c55e",
  YELLOW: "#f59e0b",
  ORANGE: "#f97316",
  RED: "#ef4444",
  PINK: "#ec4899",
  PURPLE: "#a855f7",
};

// ── 工具函数 ──

// ── 主题检测 hook ──

/** 监听 <html> 的 class，返回当前是否处于 light 模式 */
function useIsLight(): boolean {
  const [isLight, setIsLight] = useState(false);

  useEffect(() => {
    const update = () =>
      setIsLight(document.documentElement.classList.contains("light"));
    update();
    const observer = new MutationObserver(update);
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class"],
    });
    return () => observer.disconnect();
  }, []);

  return isLight;
}

// ── 亮度感知 Label 样式工具 ──

/** 计算 HEX 颜色的相对亮度（0=黑，1=白） */
function hexLuminance(hex: string): number {
  const h = hex.startsWith("#") ? hex.slice(1) : hex;
  const r = parseInt(h.slice(0, 2), 16) / 255;
  const g = parseInt(h.slice(2, 4), 16) / 255;
  const b = parseInt(h.slice(4, 6), 16) / 255;
  const toLinear = (c: number) =>
    c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
  return 0.2126 * toLinear(r) + 0.7152 * toLinear(g) + 0.0722 * toLinear(b);
}

/** 将 HEX 颜色按比例加深（factor: 0~1，越小越暗） */
function darkenHex(hex: string, factor: number): string {
  const h = hex.startsWith("#") ? hex.slice(1) : hex;
  const r = Math.round(parseInt(h.slice(0, 2), 16) * factor);
  const g = Math.round(parseInt(h.slice(2, 4), 16) * factor);
  const b = Math.round(parseInt(h.slice(4, 6), 16) * factor);
  return `#${r.toString(16).padStart(2, "0")}${g.toString(16).padStart(2, "0")}${b.toString(16).padStart(2, "0")}`;
}

/**
 * 根据标签颜色亮度 + 当前主题返回合适的内联样式。
 *
 * Light 模式：背景不透明度 40%，边框 85%，浅色标签文字大幅加深（factor 0.38）
 * Dark  模式：背景不透明度 22%，边框 55%，浅色标签文字轻微加深（factor 0.60）
 */
function getLabelStyles(
  rawColor: string,
  isLight: boolean,
): React.CSSProperties {
  const hex = rawColor.startsWith("#") ? rawColor : `#${rawColor}`;
  const lum = hexLuminance(hex);

  if (isLight) {
    // 浅色主题：阈值更低（0.30），加深更多（0.38），背景/边框更不透明
    const textColor = lum > 0.3 ? darkenHex(hex, 0.38) : hex;
    return {
      backgroundColor: `${hex}40`,
      color: textColor,
      borderColor: `${hex}bb`,
      fontWeight: 500,
    };
  }

  // 深色主题：保持原有轻透明风格
  const textColor = lum > 0.45 ? darkenHex(hex, 0.6) : hex;
  return {
    backgroundColor: `${hex}28`,
    color: textColor,
    borderColor: `${hex}72`,
    fontWeight: 500,
  };
}

function formatTimeAgo(dateStr: string, locale: string): string {
  const now = Date.now();
  const date = new Date(dateStr).getTime();
  const diff = now - date;
  const seconds = Math.floor(diff / 1000);
  const minutes = Math.floor(seconds / 60);
  const hours = Math.floor(minutes / 60);
  const days = Math.floor(hours / 24);
  const months = Math.floor(days / 30);

  const isZh = locale === "zh-CN";

  if (months > 0) return isZh ? `${months} 个月前` : `${months}mo ago`;
  if (days > 0) return isZh ? `${days} 天前` : `${days}d ago`;
  if (hours > 0) return isZh ? `${hours} 小时前` : `${hours}h ago`;
  if (minutes > 0) return isZh ? `${minutes} 分钟前` : `${minutes}m ago`;
  return isZh ? "刚刚" : "just now";
}

// ── Filter 解析（客户端应用） ──

function parseFilter(filterStr: string, item: BoardItem): boolean {
  if (!filterStr || !filterStr.trim()) return true;
  const parts = filterStr.trim().split(/\s+/);
  return parts.every((part) => {
    const colonIdx = part.indexOf(":");
    if (colonIdx === -1) return true;
    const key = part.slice(0, colonIdx).toLowerCase();
    const val = part.slice(colonIdx + 1);
    switch (key) {
      case "label":
        return item.labels.some((l) => l.name === val);
      case "is":
        if (val === "open") return item.state === "OPEN";
        if (val === "closed") return item.state === "CLOSED";
        return true;
      case "no":
        if (val === "status") return item.statusId === null;
        return true;
      case "status":
        return (
          item.statusName !== null &&
          item.statusName.toLowerCase() === val.toLowerCase()
        );
      default:
        return true;
    }
  });
}

// ── ThemeProvider 通用包裹样式 ──

const themeVars = {
  "--bgColor-default": "transparent",
  "--bgColor-muted": "transparent",
} as React.CSSProperties;

// ── 加载状态 ──

function LoadingState() {
  return (
    <ThemeProvider colorMode="night">
      <div style={themeVars}>
        <section className="py-16 bg-dark-bg">
          <div className="flex flex-col items-center justify-center gap-4 min-h-[320px]">
            <div className="w-10 h-10 rounded-full border-2 border-dark-border border-t-brand-sky animate-spin" />
            <p className="text-dark-text-muted text-sm">Loading board...</p>
          </div>
        </section>
      </div>
    </ThemeProvider>
  );
}

// ── 错误状态 ──

function ErrorState({ message }: { message: string }) {
  return (
    <ThemeProvider colorMode="night">
      <div style={themeVars}>
        <section className="py-16 bg-dark-bg">
          <div className="max-w-xl mx-auto px-4">
            <Flash variant="danger">{message}</Flash>
          </div>
        </section>
      </div>
    </ThemeProvider>
  );
}

// ── IssueCard（看板卡片） ──

function IssueCard({
  item,
  locale,
  onIssueClick,
}: {
  item: BoardItem;
  locale: string;
  onIssueClick?: (issueNumber: number) => void;
}) {
  const isLight = useIsLight();
  const isOpen = item.state === "OPEN";
  const isNotPlanned = item.stateReason === "NOT_PLANNED";

  const StateIcon = isOpen
    ? CircleDot
    : isNotPlanned
      ? CircleSlash
      : CircleCheck;
  const stateIconColor = isOpen
    ? "#38bdf8"
    : isNotPlanned
      ? "#6b7280"
      : "#22c55e";

  return (
    <motion.div
      whileHover={{ scale: 1.01 }}
      whileTap={{ scale: 0.99 }}
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.2 }}
      onClick={() => onIssueClick?.(item.issueNumber)}
      className="bg-dark-surface1 border border-dark-border rounded-xl p-4 hover:border-dark-text-secondary hover:bg-dark-surface2 cursor-pointer transition-all"
    >
      {/* 行1：状态图标 + 标题 */}
      <div className="flex items-start gap-2 mb-2">
        <StateIcon
          size={15}
          style={{ color: stateIconColor, flexShrink: 0, marginTop: 2 }}
        />
        <span className="text-sm text-dark-text leading-snug line-clamp-2">
          {item.title}
        </span>
      </div>

      {/* 行2：Labels */}
      {item.labels.length > 0 && (
        <div className="flex flex-wrap gap-1 mb-2">
          {item.labels.map((label) => {
            const hex = label.color.startsWith("#")
              ? label.color
              : `#${label.color}`;
            return (
              <Label
                key={label.name}
                size="small"
                style={getLabelStyles(label.color, isLight)}
              >
                {label.name}
              </Label>
            );
          })}
        </div>
      )}

      {/* 行3：底部元数据 */}
      <div className="flex items-center gap-3 text-xs text-dark-text-muted mt-1">
        <span className="font-mono">#{item.issueNumber}</span>
        <span className="flex items-center gap-1">
          <Clock size={11} />
          {formatTimeAgo(item.createdAt, locale)}
        </span>
        {item.comments > 0 && (
          <span className="flex items-center gap-1">
            <MessageSquare size={11} />
            {item.comments}
          </span>
        )}
      </div>
    </motion.div>
  );
}

// ── 排序图标 ──

type SortDir = "ASC" | "DESC";

function SortIcon({
  field,
  sortField,
  sortDir,
}: {
  field: string;
  sortField: string;
  sortDir: SortDir;
}) {
  if (sortField !== field)
    return (
      <ChevronsUpDown size={13} className="text-dark-text-muted opacity-40" />
    );
  return sortDir === "ASC" ? (
    <ChevronUp size={13} className="text-brand-sky" />
  ) : (
    <ChevronDown size={13} className="text-brand-sky" />
  );
}

// ── Filter 标签栏（只读展示） ──

function FilterBar({ filter, count }: { filter: string; count: number }) {
  if (!filter) {
    return (
      <p className="text-xs text-dark-text-muted mb-4">
        {count} item{count !== 1 ? "s" : ""}
      </p>
    );
  }
  return (
    <div className="flex items-center gap-3 mb-4 flex-wrap">
      <span className="flex items-center gap-1.5 text-xs text-dark-text-muted">
        <Filter size={12} />
        <span className="font-mono bg-dark-surface2 border border-dark-border px-2 py-0.5 rounded text-brand-sky">
          {filter}
        </span>
      </span>
      <span className="text-xs text-dark-text-muted">
        {count} item{count !== 1 ? "s" : ""}
      </span>
    </div>
  );
}

// ── TABLE VIEW ──

const TABLE_COLS: Array<{
  key: string;
  label: string;
  sortable: boolean;
  align?: "right";
}> = [
  { key: "number", label: "#", sortable: true },
  { key: "title", label: "Title", sortable: true },
  { key: "status", label: "Status", sortable: true },
  { key: "labels", label: "Labels", sortable: false },
  { key: "createdAt", label: "Created", sortable: true },
  { key: "comments", label: "Comments", sortable: true, align: "right" },
];

// ── 表格行 ──

function TableRow({
  item,
  rowNum,
  locale,
  isLight,
  onIssueClick,
}: {
  item: BoardItem;
  rowNum: number;
  locale: string;
  isLight: boolean;
  onIssueClick?: (issueNumber: number) => void;
}) {
  const isOpen = item.state === "OPEN";
  const isNotPlanned = item.stateReason === "NOT_PLANNED";
  const StateIcon = isOpen
    ? CircleDot
    : isNotPlanned
      ? CircleSlash
      : CircleCheck;
  const stateIconColor = isOpen
    ? "#38bdf8"
    : isNotPlanned
      ? "#6b7280"
      : "#22c55e";

  return (
    <motion.tr
      initial={{ opacity: 0, y: 4 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.15, delay: Math.min(rowNum * 0.012, 0.3) }}
      onClick={() => onIssueClick?.(item.issueNumber)}
      className="border-b border-dark-border last:border-b-0 hover:bg-dark-surface2 cursor-pointer transition-colors group"
    >
      {/* # */}
      <td className="px-4 py-3 font-mono text-xs text-dark-text-muted whitespace-nowrap w-14">
        {rowNum}
      </td>

      {/* Title */}
      <td className="px-4 py-3 max-w-sm">
        <span className="flex items-start gap-2">
          <StateIcon
            size={14}
            style={{ color: stateIconColor, flexShrink: 0, marginTop: 2 }}
          />
          <span className="text-dark-text leading-snug line-clamp-2">
            {item.title}
          </span>
        </span>
      </td>

      {/* Status */}
      <td className="px-4 py-3 whitespace-nowrap">
        {item.statusName ? (
          <span
            className="inline-flex items-center px-2 py-0.5 rounded-full text-xs font-medium"
            style={{
              backgroundColor: "#3b82f622",
              color: "#3b82f6",
              border: "1px solid #3b82f655",
            }}
          >
            {item.statusName}
          </span>
        ) : (
          <span className="text-dark-text-muted text-xs">—</span>
        )}
      </td>

      {/* Labels */}
      <td className="px-4 py-3">
        {item.labels.length === 0 ? (
          <span className="text-dark-text-muted text-xs">—</span>
        ) : (
          <div className="flex flex-wrap gap-1">
            {item.labels.map((label) => (
              <Label
                key={label.name}
                size="small"
                style={getLabelStyles(label.color, isLight)}
              >
                {label.name}
              </Label>
            ))}
          </div>
        )}
      </td>

      {/* Created */}
      <td className="px-4 py-3 whitespace-nowrap">
        <span className="flex items-center gap-1 text-xs text-dark-text-muted">
          <Clock size={11} />
          {formatTimeAgo(item.createdAt, locale)}
        </span>
      </td>

      {/* Comments */}
      <td className="px-4 py-3 text-right whitespace-nowrap">
        {item.comments > 0 ? (
          <span className="inline-flex items-center justify-end gap-1 text-xs text-dark-text-muted">
            <MessageSquare size={11} />
            {item.comments}
          </span>
        ) : (
          <span className="text-dark-text-muted text-xs">—</span>
        )}
      </td>
    </motion.tr>
  );
}

function TableView({
  view,
  data,
  locale,
  searchQuery,
  onIssueClick,
}: {
  view: ViewConfig;
  data: BoardData;
  locale: string;
  searchQuery: string;
  onIssueClick?: (issueNumber: number) => void;
}) {
  const isLight = useIsLight();
  const { t } = useLocale();

  const STORAGE_KEY = `fluxdown-board-sort-${view.id}`;

  const defaultSort = view.hasGroupBy ? "status" : "number";
  const defaultDir: SortDir = view.hasGroupBy ? "ASC" : "DESC";

  const [sortField, setSortField] = useState<string>(() => {
    if (typeof window === "undefined") return defaultSort;
    try {
      const saved = localStorage.getItem(STORAGE_KEY);
      if (saved)
        return (JSON.parse(saved) as { field: string; dir: SortDir }).field;
    } catch {}
    return defaultSort;
  });
  const [sortDir, setSortDir] = useState<SortDir>(() => {
    if (typeof window === "undefined") return defaultDir;
    try {
      const saved = localStorage.getItem(STORAGE_KEY);
      if (saved)
        return (JSON.parse(saved) as { field: string; dir: SortDir }).dir;
    } catch {}
    return defaultDir;
  });
  // 记录每个分组的折叠状态，key = statusId（或 "__none__"）
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});

  // 过滤（视图过滤 + 搜索关键词）
  const filteredItems = useMemo(
    () =>
      data.allItems.filter(
        (item) =>
          parseFilter(view.filter, item) && matchesSearch(item, searchQuery),
      ),
    [data.allItems, view.filter, searchQuery],
  );

  // Status 选项顺序索引（由 data.columns 定义，与 GitHub Projects 完全一致）
  const statusOrder = useMemo(() => {
    const map = new Map<string, number>();
    data.columns.forEach((col, idx) => map.set(col.id, idx));
    return map;
  }, [data.columns]);

  // 排序（组内排序）
  const sortedItems = useMemo(() => {
    const copy = [...filteredItems];
    copy.sort((a, b) => {
      let cmp = 0;
      switch (sortField) {
        case "title":
          cmp = a.title.localeCompare(b.title);
          break;
        case "status":
          // 按 GitHub Status 选项定义顺序排，而非字母序
          cmp =
            (statusOrder.get(a.statusId ?? "") ?? 9999) -
            (statusOrder.get(b.statusId ?? "") ?? 9999);
          break;
        case "createdAt":
          cmp =
            new Date(a.createdAt).getTime() - new Date(b.createdAt).getTime();
          break;
        case "comments":
          cmp = a.comments - b.comments;
          break;
        case "number":
          cmp = a.issueNumber - b.issueNumber;
          break;
        default:
          cmp = 0;
      }
      return sortDir === "ASC" ? cmp : -cmp;
    });
    return copy;
  }, [filteredItems, sortField, sortDir, statusOrder]);

  // 是否启用分组：视图配置了 Group by 且用户当前按 status 排序
  const groupingEnabled = view.hasGroupBy && sortField === "status";

  // 按 Status 分组，顺序与 data.columns 保持一致；
  // 当排序字段为 "status" 时，分组顺序随 sortDir 同步反转
  const groups = useMemo(() => {
    if (!groupingEnabled) return [];
    // 建立 statusId → items 映射
    const itemsByStatus = new Map<string, BoardItem[]>();
    const noStatusItems: BoardItem[] = [];

    for (const item of sortedItems) {
      if (item.statusId) {
        const bucket = itemsByStatus.get(item.statusId) ?? [];
        bucket.push(item);
        itemsByStatus.set(item.statusId, bucket);
      } else {
        noStatusItems.push(item);
      }
    }

    // 按 columns 顺序构建分组列表
    const result: Array<{
      id: string;
      name: string;
      color: string;
      items: BoardItem[];
    }> = [];

    // 当 sortField === "status" 且 sortDir === "DESC" 时反转列顺序
    const orderedColumns =
      sortField === "status" && sortDir === "DESC"
        ? [...data.columns].reverse()
        : data.columns;

    for (const col of orderedColumns) {
      const items = itemsByStatus.get(col.id);
      if (items && items.length > 0) {
        result.push({ id: col.id, name: col.name, color: col.color, items });
      }
    }

    // 无 Status 分组：DESC 时放最前，ASC 时放最后
    if (noStatusItems.length > 0) {
      const noStatusGroup = {
        id: "__none__",
        name: t("board.noStatus"),
        color: "GRAY",
        items: noStatusItems,
      };
      if (sortField === "status" && sortDir === "DESC") {
        result.unshift(noStatusGroup);
      } else {
        result.push(noStatusGroup);
      }
    }

    return result;
  }, [sortedItems, data.columns, sortField, sortDir, t]);

  function handleSort(field: string) {
    const nextField = field;
    const nextDir: SortDir =
      sortField === field ? (sortDir === "ASC" ? "DESC" : "ASC") : "ASC";
    setSortField(nextField);
    setSortDir(nextDir);
    try {
      localStorage.setItem(
        STORAGE_KEY,
        JSON.stringify({ field: nextField, dir: nextDir }),
      );
    } catch {}
  }

  function toggleGroup(id: string) {
    setCollapsed((prev) => ({ ...prev, [id]: !prev[id] }));
  }

  // 连续行序号（跨分组累计）
  let rowCounter = 0;

  return (
    <div className="max-w-screen-2xl mx-auto px-4 sm:px-8">
      <FilterBar filter={view.filter} count={sortedItems.length} />

      <div className="overflow-x-auto rounded-xl border border-dark-border">
        <table className="w-full text-sm text-dark-text border-collapse">
          <thead>
            <tr className="bg-dark-surface2 border-b border-dark-border">
              {TABLE_COLS.map((col) => (
                <th
                  key={col.key}
                  onClick={col.sortable ? () => handleSort(col.key) : undefined}
                  className={[
                    "px-4 py-3 text-xs font-semibold text-dark-text-muted whitespace-nowrap select-none",
                    col.align === "right" ? "text-right" : "text-left",
                    col.key === "#" ? "w-14" : "",
                    col.sortable
                      ? "cursor-pointer hover:text-dark-text transition-colors"
                      : "",
                  ]
                    .filter(Boolean)
                    .join(" ")}
                >
                  <span
                    className={`inline-flex items-center gap-1 ${col.align === "right" ? "justify-end w-full" : ""}`}
                  >
                    {col.label}
                    {col.sortable && (
                      <SortIcon
                        field={col.key}
                        sortField={sortField}
                        sortDir={sortDir}
                      />
                    )}
                  </span>
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {sortedItems.length === 0 ? (
              <tr>
                <td
                  colSpan={TABLE_COLS.length}
                  className="px-4 py-12 text-center text-dark-text-muted text-sm"
                >
                  {t("board.empty")}
                </td>
              </tr>
            ) : groupingEnabled ? (
              groups.map((group) => {
                const isCollapsed = !!collapsed[group.id];
                const colColor =
                  GITHUB_COLOR_MAP[group.color] ?? GITHUB_COLOR_MAP.GRAY;

                return (
                  <>
                    {/* 分组头行 */}
                    <tr
                      key={`group-${group.id}`}
                      className="bg-dark-surface2 border-b border-dark-border"
                    >
                      <td colSpan={TABLE_COLS.length} className="px-3 py-2">
                        <button
                          onClick={() => toggleGroup(group.id)}
                          className="flex items-center gap-2 w-full text-left cursor-pointer select-none group/hdr"
                        >
                          <ChevronRight
                            size={14}
                            className={`text-dark-text-muted transition-transform duration-200 ${isCollapsed ? "" : "rotate-90"}`}
                          />
                          <span
                            className="w-3 h-3 rounded-full flex-shrink-0"
                            style={{ backgroundColor: colColor }}
                          />
                          <span className="text-sm font-semibold text-dark-text">
                            {group.name}
                          </span>
                          <span className="text-xs text-dark-text-muted font-mono bg-dark-surface3 rounded-full px-2 py-0.5">
                            {group.items.length}
                          </span>
                        </button>
                      </td>
                    </tr>

                    {/* 分组内条目 */}
                    {!isCollapsed &&
                      group.items.map((item) => {
                        rowCounter += 1;
                        return (
                          <TableRow
                            key={item.id}
                            item={item}
                            rowNum={rowCounter}
                            locale={locale}
                            isLight={isLight}
                            onIssueClick={onIssueClick}
                          />
                        );
                      })}
                  </>
                );
              })
            ) : (
              sortedItems.map((item, idx) => (
                <TableRow
                  key={item.id}
                  item={item}
                  rowNum={idx + 1}
                  locale={locale}
                  isLight={isLight}
                  onIssueClick={onIssueClick}
                />
              ))
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}

// ── BOARD VIEW ──

function BoardView({
  view,
  data,
  locale,
  searchQuery,
  onIssueClick,
}: {
  view: ViewConfig;
  data: BoardData;
  locale: string;
  searchQuery: string;
  onIssueClick?: (issueNumber: number) => void;
}) {
  const { t } = useLocale();

  // 确定分组字段：优先用 view.groupByField，fallback 到 "Status"
  const groupByFieldName = view.groupByField ?? "Status";

  // 从 data.singleSelectFields 找对应字段的 options
  const groupField = data.singleSelectFields?.find(
    (f) => f.name === groupByFieldName,
  );

  const groupColumns = useMemo(() => {
    if (!groupField) {
      // fallback: 用 data.columns（Status 字段）
      const allCols: BoardColumn[] = [
        ...data.columns,
        ...(data.noStatusItems.length > 0
          ? [
              {
                id: "none",
                name: t("board.noStatus"),
                color: "GRAY",
                items: data.noStatusItems,
              },
            ]
          : []),
      ];
      return allCols.map((col) => ({
        id: col.id,
        name: col.name,
        color: col.color,
        items: col.items.filter(
          (item) =>
            parseFilter(view.filter, item) && matchesSearch(item, searchQuery),
        ),
      }));
    }

    // 过滤 items（视图过滤 + 搜索关键词）
    const filtered = data.allItems.filter(
      (item) =>
        parseFilter(view.filter, item) && matchesSearch(item, searchQuery),
    );

    // 按 groupField 的 options 顺序分组
    const result: BoardColumn[] = groupField.options.map((opt) => ({
      id: opt.id,
      name: opt.name,
      color: opt.color,
      items: filtered.filter(
        (item) =>
          (item.fieldValues ?? {})[groupByFieldName]?.optionId === opt.id,
      ),
    }));

    // 没有该字段值的 items 放"无分组"列
    const assignedIds = new Set(
      filtered.flatMap((item) =>
        (item.fieldValues ?? {})[groupByFieldName] ? [item.id] : [],
      ),
    );
    const unassigned = filtered.filter((item) => !assignedIds.has(item.id));
    if (unassigned.length > 0) {
      result.unshift({
        id: "__none__",
        name: t("board.noStatus"),
        color: "GRAY",
        items: unassigned,
      });
    }

    return result;
  }, [data, groupField, groupByFieldName, view.filter, searchQuery, t]);

  const totalFiltered = groupColumns.reduce(
    (acc, c) => acc + c.items.length,
    0,
  );

  return (
    <div>
      {/* 过滤标签 */}
      <div className="max-w-screen-2xl mx-auto px-4 sm:px-8 mb-5">
        <FilterBar filter={view.filter} count={totalFiltered} />
      </div>

      {/* 横向滚动看板 */}
      <div className="overflow-x-auto scrollbar-none pb-4">
        <div className="flex gap-4 px-4 sm:px-8 w-max mx-auto">
          {groupColumns.map((column, colIndex) => {
            const colColor =
              GITHUB_COLOR_MAP[column.color] ?? GITHUB_COLOR_MAP.GRAY;
            return (
              <motion.div
                key={column.id}
                className="w-72 flex-shrink-0"
                initial={{ opacity: 0, x: 20 }}
                animate={{ opacity: 1, x: 0 }}
                transition={{ duration: 0.3, delay: colIndex * 0.06 }}
              >
                {/* 列头 */}
                <div className="flex items-center gap-2 mb-3 px-1">
                  <span
                    className="w-2.5 h-2.5 rounded-full flex-shrink-0"
                    style={{ backgroundColor: colColor }}
                  />
                  <span className="text-sm font-semibold text-dark-text truncate flex-1">
                    {column.name}
                  </span>
                  <span className="bg-dark-surface3 text-dark-text-muted rounded-full px-2 py-0.5 text-xs font-mono">
                    {column.items.length}
                  </span>
                </div>

                {/* 列内卡片 */}
                <div className="space-y-2">
                  {column.items.length === 0 ? (
                    <div className="bg-dark-surface1 border border-dashed border-dark-border rounded-xl p-4 text-center">
                      <p className="text-xs text-dark-text-muted">
                        {t("board.empty")}
                      </p>
                    </div>
                  ) : (
                    column.items.map((item) => (
                      <IssueCard
                        key={item.id}
                        item={item}
                        locale={locale}
                        onIssueClick={onIssueClick}
                      />
                    ))
                  )}
                </div>
              </motion.div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

// ── Fallback 看板（无 views 数据） ──

function FallbackBoardView({
  data,
  locale,
  searchQuery,
  onIssueClick,
}: {
  data: BoardData;
  locale: string;
  searchQuery: string;
  onIssueClick?: (issueNumber: number) => void;
}) {
  const { t } = useLocale();

  const allColumns: BoardColumn[] = [
    ...data.columns,
    ...(data.noStatusItems.length > 0
      ? [
          {
            id: "none",
            name: t("board.noStatus"),
            color: "GRAY",
            items: data.noStatusItems,
          },
        ]
      : []),
  ].map((col) => ({
    ...col,
    items: col.items.filter((item) => matchesSearch(item, searchQuery)),
  }));

  return (
    <div className="overflow-x-auto scrollbar-none pb-4">
      <div className="flex gap-4 px-4 sm:px-8 w-max mx-auto">
        {allColumns.map((column, colIndex) => {
          const colColor =
            GITHUB_COLOR_MAP[column.color] ?? GITHUB_COLOR_MAP.GRAY;
          return (
            <motion.div
              key={column.id}
              className="w-72 flex-shrink-0"
              initial={{ opacity: 0, x: 20 }}
              animate={{ opacity: 1, x: 0 }}
              transition={{ duration: 0.3, delay: colIndex * 0.06 }}
            >
              <div className="flex items-center gap-2 mb-3 px-1">
                <span
                  className="w-2.5 h-2.5 rounded-full flex-shrink-0"
                  style={{ backgroundColor: colColor }}
                />
                <span className="text-sm font-semibold text-dark-text truncate flex-1">
                  {column.name}
                </span>
                <span className="bg-dark-surface3 text-dark-text-muted rounded-full px-2 py-0.5 text-xs font-mono">
                  {column.items.length}
                </span>
              </div>
              <div className="space-y-2">
                {column.items.length === 0 ? (
                  <div className="bg-dark-surface1 border border-dashed border-dark-border rounded-xl p-4 text-center">
                    <p className="text-xs text-dark-text-muted">
                      {t("board.empty")}
                    </p>
                  </div>
                ) : (
                  column.items.map((item) => (
                    <IssueCard
                      key={item.id}
                      item={item}
                      locale={locale}
                      onIssueClick={onIssueClick}
                    />
                  ))
                )}
              </div>
            </motion.div>
          );
        })}
      </div>
    </div>
  );
}

// ── 主组件 ──

export default function ProjectBoardSection({
  onIssueClick,
}: ProjectBoardSectionProps) {
  const [data, setData] = useState<BoardData | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [activeViewIdx, setActiveViewIdx] = useState(0);
  const [searchInput, setSearchInput] = useState("");
  const [searchQuery, setSearchQuery] = useState("");
  const debounceTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const { t, locale } = useLocale();

  const handleSearchChange = (value: string) => {
    setSearchInput(value);
    if (debounceTimer.current) clearTimeout(debounceTimer.current);
    debounceTimer.current = setTimeout(() => {
      setSearchQuery(value.trim());
    }, 400);
  };

  const handleSearchClear = () => {
    setSearchInput("");
    setSearchQuery("");
    if (debounceTimer.current) clearTimeout(debounceTimer.current);
  };

  useEffect(() => {
    fetch("/api/project-board")
      .then((r) => (r.ok ? r.json() : Promise.reject(r.status)))
      .then(setData)
      .catch(() => setError(t("board.error")))
      .finally(() => setLoading(false));
  }, []);

  if (loading) return <LoadingState />;
  if (error) return <ErrorState message={error} />;
  if (!data) return null;

  const hasViews = Array.isArray(data.views) && data.views.length > 0;

  // ── Fallback：无 views 数据时 ──
  if (!hasViews) {
    return (
      <ThemeProvider colorMode="night">
        <div style={themeVars}>
          <section className="py-16 bg-dark-bg">
            <div className="max-w-screen-2xl mx-auto px-4 sm:px-8 mb-4">
              <div className="relative">
                <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-dark-text-muted pointer-events-none" />
                <input
                  type="text"
                  value={searchInput}
                  onChange={(e) => handleSearchChange(e.target.value)}
                  placeholder={t("fbList.searchPlaceholder")}
                  className="w-full pl-9 pr-9 py-2 rounded-lg bg-dark-surface1 border border-dark-border text-sm text-dark-text placeholder-dark-text-muted focus:outline-none focus:border-brand-sky/50 focus:ring-1 focus:ring-brand-sky/20 transition-all"
                />
                {searchInput && (
                  <button
                    onClick={handleSearchClear}
                    className="absolute right-2.5 top-1/2 -translate-y-1/2 p-0.5 rounded text-dark-text-muted hover:text-dark-text transition-colors cursor-pointer"
                    title={t("fbList.searchClear")}
                  >
                    <X className="w-3.5 h-3.5" />
                  </button>
                )}
              </div>
            </div>
            <FallbackBoardView
              data={data}
              locale={locale}
              searchQuery={searchQuery}
              onIssueClick={onIssueClick}
            />
          </section>
        </div>
      </ThemeProvider>
    );
  }

  // ── 多视图模式 ──
  const safeIdx = Math.min(activeViewIdx, data.views.length - 1);
  const activeView = data.views[safeIdx];

  return (
    <ThemeProvider colorMode="night">
      <div style={themeVars}>
        <section className="py-16 bg-dark-bg">
          {/* 视图标签页 */}
          <div className="max-w-screen-2xl mx-auto px-4 sm:px-8 mb-6">
            <div className="flex items-end gap-0 border-b border-dark-border overflow-x-auto scrollbar-none">
              {data.views.map((view, idx) => {
                const isActive = idx === safeIdx;
                const LayoutIcon =
                  view.layout === "TABLE_LAYOUT" ? Table2 : Kanban;
                return (
                  <button
                    key={view.id}
                    onClick={() => setActiveViewIdx(idx)}
                    className={[
                      "flex items-center gap-1.5 px-4 py-2.5 text-sm font-medium border-b-2 -mb-px transition-colors whitespace-nowrap flex-shrink-0",
                      isActive
                        ? "border-brand-sky text-brand-sky"
                        : "border-transparent text-dark-text-muted hover:text-dark-text hover:border-dark-text-muted",
                    ].join(" ")}
                  >
                    <LayoutIcon size={13} />
                    {view.name}
                  </button>
                );
              })}
            </div>
          </div>

          {/* 搜索框 */}
          <div className="max-w-screen-2xl mx-auto px-4 sm:px-8 mb-4">
            <div className="relative">
              <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-dark-text-muted pointer-events-none" />
              <input
                type="text"
                value={searchInput}
                onChange={(e) => handleSearchChange(e.target.value)}
                placeholder={t("fbList.searchPlaceholder")}
                className="w-full pl-9 pr-9 py-2 rounded-lg bg-dark-surface1 border border-dark-border text-sm text-dark-text placeholder-dark-text-muted focus:outline-none focus:border-brand-sky/50 focus:ring-1 focus:ring-brand-sky/20 transition-all"
              />
              {searchInput && (
                <button
                  onClick={handleSearchClear}
                  className="absolute right-2.5 top-1/2 -translate-y-1/2 p-0.5 rounded text-dark-text-muted hover:text-dark-text transition-colors cursor-pointer"
                  title={t("fbList.searchClear")}
                >
                  <X className="w-3.5 h-3.5" />
                </button>
              )}
            </div>
          </div>

          {/* 视图内容 */}
          <motion.div
            key={activeView.id}
            initial={{ opacity: 0, y: 6 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.2 }}
          >
            {activeView.layout === "TABLE_LAYOUT" ? (
              <TableView
                view={activeView}
                data={data}
                locale={locale}
                searchQuery={searchQuery}
                onIssueClick={onIssueClick}
              />
            ) : (
              <BoardView
                view={activeView}
                data={data}
                locale={locale}
                searchQuery={searchQuery}
                onIssueClick={onIssueClick}
              />
            )}
          </motion.div>
        </section>
      </div>
    </ThemeProvider>
  );
}
