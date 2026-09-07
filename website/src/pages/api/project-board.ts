/**
 * GET /api/project-board
 *
 * 通过 @octokit/graphql 动态查询 GitHub Projects v2 看板数据，
 * 返回按 Status 字段分组的 issues 以及完整的视图配置列表。
 * 服务端内存缓存 5 分钟。
 *
 * 返回格式:
 * {
 *   views: [{ id, name, number, layout, filter, visibleFields, sortBy, groupByField }],
 *   columns: [{ id, name, color, items: [...] }],
 *   noStatusItems: [...],
 *   allItems: [...],
 *   totalItems: number,
 *   projectTitle: string,
 *   cachedAt: string
 * }
 */

import type { APIRoute } from "astro";
import { graphql } from "@octokit/graphql";
import { GITHUB_TOKEN } from "astro:env/server";

export const prerender = false;

// ── 类型定义 ──

interface StatusOption {
  id: string;
  name: string;
  color: string;
}

interface ViewConfig {
  id: string;
  name: string;
  number: number;
  layout: "TABLE_LAYOUT" | "BOARD_LAYOUT";
  /** 原始 filter 字符串，如 "label:user-feedback"，前端自行解析应用 */
  filter: string;
  /** 可见字段名列表 */
  visibleFields: string[];
  groupByField: string | null;
  /** 视图是否配置了 Group by（totalCount > 0）*/
  hasGroupBy: boolean;
}

interface ProjectMeta {
  projectId: string;
  projectTitle: string;
  statusOptions: StatusOption[];
  views: ViewConfig[];
  allSingleSelectFields: GQLSingleSelectField[];
}

interface ItemLabel {
  name: string;
  color: string;
}

interface BoardItem {
  id: string;
  issueNumber: number;
  title: string;
  state: "OPEN" | "CLOSED";
  stateReason: string | null;
  labels: ItemLabel[];
  createdAt: string;
  url: string;
  comments: number;
  /** 对应 Status 选项 id，无 Status 时为 null */
  statusId: string | null;
  /** 对应 Status 选项名称，无 Status 时为 null */
  statusName: string | null;
  fieldValues: Record<string, { optionId: string; optionName: string }>;
}

interface BoardColumn {
  id: string;
  name: string;
  color: string;
  items: BoardItem[];
}

interface BoardResponse {
  views: ViewConfig[];
  columns: BoardColumn[]; // 按 Status 分组（兼容现有逻辑）
  noStatusItems: BoardItem[];
  allItems: BoardItem[]; // 所有 item 的扁平列表（含 statusId/statusName）
  totalItems: number;
  projectTitle: string;
  cachedAt: string;
  singleSelectFields: Array<{
    id: string;
    name: string;
    options: Array<{ id: string; name: string; color: string }>;
  }>;
}

// ── GraphQL 响应类型 ──

interface GQLSingleSelectField {
  id: string;
  name: string;
  options: StatusOption[];
}

interface GQLViewField {
  id?: string;
  name?: string;
}

interface GQLView {
  id: string;
  name: string;
  number: number;
  layout: string;
  filter: string | null;
  fields: {
    nodes: GQLViewField[];
  };
  groupByFields: {
    totalCount: number;
  };
}

interface GQLProjectMetaAndViewsResponse {
  user: {
    projectV2: {
      id: string;
      title: string;
      fields: {
        nodes: Array<Partial<GQLSingleSelectField>>;
      };
      views: {
        nodes: GQLView[];
      };
    } | null;
  } | null;
}

interface GQLPageInfo {
  hasNextPage: boolean;
  endCursor: string | null;
}

interface GQLFieldValue {
  optionId?: string;
  name?: string;
  field?: { name?: string };
}

interface GQLIssueContent {
  number?: number;
  title?: string;
  state?: string;
  stateReason?: string | null;
  url?: string;
  createdAt?: string;
  comments?: { totalCount: number };
  labels?: { nodes: Array<{ name: string; color: string }> };
}

interface GQLProjectItem {
  id: string;
  fieldValues: { nodes: GQLFieldValue[] };
  content: GQLIssueContent | null;
}

interface GQLItemsResponse {
  node: {
    items: {
      pageInfo: GQLPageInfo;
      nodes: GQLProjectItem[];
    };
  } | null;
}

// ── 服务端内存缓存（5 分钟） ──

const CACHE_TTL_MS = 5 * 60 * 1000;

interface CacheEntry {
  data: BoardResponse;
  timestamp: number;
}

const cache = new Map<string, CacheEntry>();

// ── GraphQL 查询语句 ──

/**
 * 合并查询：同时获取项目元数据（Status 字段选项）和视图配置列表
 */
const QUERY_PROJECT_META_AND_VIEWS = `
  query GetProjectMetaAndViews($owner: String!, $number: Int!) {
    user(login: $owner) {
      projectV2(number: $number) {
        id
        title
        fields(first: 20) {
          nodes {
            ... on ProjectV2SingleSelectField {
              id
              name
              options {
                id
                name
                color
              }
            }
          }
        }
        views(first: 20) {
          nodes {
            id
            name
            number
            layout
            filter
            fields(first: 20) {
              nodes {
                ... on ProjectV2Field {
                  id
                  name
                }
                ... on ProjectV2SingleSelectField {
                  id
                  name
                }
                ... on ProjectV2IterationField {
                  id
                  name
                }
              }
            }
            groupByFields(first: 1) {
              totalCount
            }
          }
        }
      }
    }
  }
`;

const QUERY_PROJECT_ITEMS = `
  query GetProjectItems($projectId: ID!, $cursor: String) {
    node(id: $projectId) {
      ... on ProjectV2 {
        items(first: 100, after: $cursor) {
          pageInfo {
            hasNextPage
            endCursor
          }
          nodes {
            id
            fieldValues(first: 20) {
              nodes {
                ... on ProjectV2ItemFieldSingleSelectValue {
                  optionId
                  name
                  field {
                    ... on ProjectV2SingleSelectField {
                      name
                    }
                  }
                }
              }
            }
            content {
              ... on Issue {
                number
                title
                state
                stateReason
                url
                createdAt
                comments {
                  totalCount
                }
                labels(first: 5) {
                  nodes {
                    name
                    color
                  }
                }
              }
            }
          }
        }
      }
    }
  }
`;

// ── 工具函数 ──

/** 清理 issue 标题：去掉 emoji 前缀和 [Website Feedback] 标记 */
function cleanTitle(title: string): string {
  return title
    .replace(
      /^[\u{1F300}-\u{1FAF6}\u{2600}-\u{27BF}\u{FE00}-\u{FE0F}\u{200D}\u{20E3}\u{E0020}-\u{E007F}]+\s*/u,
      "",
    )
    .replace(/^\[Website Feedback\]\s*/i, "")
    .trim();
}

/** 将 GQLView 原始数据映射为 ViewConfig */
function mapGQLViewToViewConfig(
  view: GQLView,
  allSingleSelectFields: GQLSingleSelectField[],
): ViewConfig {
  const visibleFields = view.fields.nodes
    .map((f) => f.name)
    .filter(
      (name): name is string => typeof name === "string" && name.length > 0,
    );

  // groupBy 无法从 API 直接查询（GitHub 公开 schema 不暴露该字段）。
  // 启发式推断：取 visibleFields 中第一个出现在 allSingleSelectFields 里、
  // 且不是 "Status" 的字段名；若不存在则为 null（前端 fallback 到 Status）。
  const singleSelectNames = new Set(allSingleSelectFields.map((f) => f.name));
  const inferredGroupBy =
    visibleFields.find(
      (name) => singleSelectNames.has(name) && name !== "Status",
    ) ?? null;

  return {
    id: view.id,
    name: view.name,
    number: view.number,
    layout: (view.layout === "BOARD_LAYOUT"
      ? "BOARD_LAYOUT"
      : "TABLE_LAYOUT") as "TABLE_LAYOUT" | "BOARD_LAYOUT",
    filter: view.filter ?? "",
    visibleFields,
    groupByField: inferredGroupBy,
    hasGroupBy: (view.groupByFields?.totalCount ?? 0) > 0,
  };
}

/** 尝试读取 optional 的 Project 相关环境变量 */
async function tryGetProjectEnv(): Promise<{
  projectToken: string | undefined;
  owner: string;
  number: number;
}> {
  try {
    const mod = await import("astro:env/server");
    const m = mod as Record<string, unknown>;
    return {
      projectToken:
        typeof m.GITHUB_PROJECT_TOKEN === "string"
          ? m.GITHUB_PROJECT_TOKEN
          : undefined,
      owner:
        typeof m.GITHUB_PROJECT_OWNER === "string"
          ? m.GITHUB_PROJECT_OWNER
          : "zerx-lab",
      number:
        typeof m.GITHUB_PROJECT_NUMBER === "number"
          ? m.GITHUB_PROJECT_NUMBER
          : 4,
    };
  } catch {
    return { projectToken: undefined, owner: "zerx-lab", number: 4 };
  }
}

// ── 核心数据获取逻辑 ──

/**
 * 合并查询项目元数据（id、标题、Status 字段及其选项）与视图配置列表
 */
async function fetchProjectMeta(
  graphqlWithAuth: ReturnType<typeof graphql.defaults>,
  owner: string,
  number: number,
): Promise<ProjectMeta> {
  const data = await graphqlWithAuth<GQLProjectMetaAndViewsResponse>(
    QUERY_PROJECT_META_AND_VIEWS,
    { owner, number },
  );

  if (!data.user?.projectV2) {
    const err = new Error(
      `未找到项目：${owner} 的第 ${number} 号项目不存在或无权限访问`,
    );
    (err as NodeJS.ErrnoException & { statusCode?: number }).statusCode = 404;
    throw err;
  }

  const project = data.user.projectV2;

  // 筛选出有 options 的字段（即 SingleSelectField）
  const singleSelectFields = project.fields.nodes.filter(
    (f): f is GQLSingleSelectField =>
      Array.isArray((f as GQLSingleSelectField).options) &&
      typeof (f as GQLSingleSelectField).id === "string",
  );

  // 优先取 name === "Status" 的字段，否则取第一个 SingleSelectField
  const statusField =
    singleSelectFields.find((f) => f.name === "Status") ??
    singleSelectFields[0];

  if (!statusField) {
    throw new Error("项目中未找到 SingleSelect 类型的字段（Status）");
  }

  // 将 GQL 原始视图数据映射为 ViewConfig
  const views: ViewConfig[] = project.views.nodes.map((v) =>
    mapGQLViewToViewConfig(v, singleSelectFields),
  );

  return {
    projectId: project.id,
    projectTitle: project.title,
    statusOptions: statusField.options,
    views,
    allSingleSelectFields: singleSelectFields,
  };
}

/** 分页获取项目中所有 items（自动翻页直到 hasNextPage=false） */
async function fetchAllItems(
  graphqlWithAuth: ReturnType<typeof graphql.defaults>,
  projectId: string,
): Promise<GQLProjectItem[]> {
  const allItems: GQLProjectItem[] = [];
  let cursor: string | null = null;

  do {
    const data: GQLItemsResponse = await graphqlWithAuth<GQLItemsResponse>(
      QUERY_PROJECT_ITEMS,
      { projectId, cursor: cursor ?? undefined },
    );

    if (!data.node) break;

    const page: NonNullable<GQLItemsResponse["node"]>["items"] =
      data.node.items;
    allItems.push(...page.nodes);

    cursor = page.pageInfo.hasNextPage
      ? (page.pageInfo.endCursor ?? null)
      : null;
  } while (cursor !== null);

  return allItems;
}

/** 将原始 items 组装为看板数据（含视图配置、allItems 扁平列表） */
async function buildBoardData(
  graphqlWithAuth: ReturnType<typeof graphql.defaults>,
  owner: string,
  number: number,
): Promise<BoardResponse> {
  // 1. 合并查询项目元数据与视图配置
  const meta = await fetchProjectMeta(graphqlWithAuth, owner, number);

  // 2. 分页获取所有 items
  const rawItems = await fetchAllItems(graphqlWithAuth, meta.projectId);

  // 3. 构建 optionId -> StatusOption 的快速查找 Map
  const statusOptionById = new Map<string, StatusOption>(
    meta.statusOptions.map((opt) => [opt.id, opt]),
  );

  // 4. 初始化按 optionId 分组的 Map
  const columnMap = new Map<string, BoardItem[]>(
    meta.statusOptions.map((opt) => [opt.id, []]),
  );
  const noStatusItems: BoardItem[] = [];
  const allItems: BoardItem[] = [];

  for (const raw of rawItems) {
    // 过滤非 Issue 内容（PR 等 content 无 number 字段）
    if (!raw.content || raw.content.number === undefined) continue;

    const issue = raw.content;

    // 从 fieldValues 中找 Status 字段对应的 optionId
    const statusValue = raw.fieldValues.nodes.find(
      (fv) =>
        fv.field?.name === "Status" &&
        typeof fv.optionId === "string" &&
        fv.optionId.length > 0,
    );

    const optionId = statusValue?.optionId ?? null;
    const matchedOption = optionId
      ? (statusOptionById.get(optionId) ?? null)
      : null;

    const fieldValues: Record<
      string,
      { optionId: string; optionName: string }
    > = {};
    for (const fv of raw.fieldValues.nodes) {
      if (
        typeof fv.optionId === "string" &&
        fv.optionId.length > 0 &&
        typeof fv.field?.name === "string"
      ) {
        const ssField = meta.allSingleSelectFields.find(
          (f) => f.name === fv.field!.name!,
        );
        const opt = ssField?.options.find((o) => o.id === fv.optionId);
        if (opt)
          fieldValues[fv.field.name] = {
            optionId: fv.optionId,
            optionName: opt.name,
          };
      }
    }

    const boardItem: BoardItem = {
      id: raw.id,
      issueNumber: issue.number!,
      title: cleanTitle(issue.title ?? ""),
      state: (issue.state === "CLOSED" ? "CLOSED" : "OPEN") as
        | "OPEN"
        | "CLOSED",
      stateReason: issue.stateReason ?? null,
      labels: issue.labels?.nodes ?? [],
      createdAt: issue.createdAt ?? "",
      url: issue.url ?? "",
      comments: issue.comments?.totalCount ?? 0,
      statusId: matchedOption?.id ?? null,
      statusName: matchedOption?.name ?? null,
      fieldValues,
    };

    // 加入扁平全量列表
    allItems.push(boardItem);

    // 按 Status 分组
    if (optionId && columnMap.has(optionId)) {
      columnMap.get(optionId)!.push(boardItem);
    } else {
      noStatusItems.push(boardItem);
    }
  }

  // 5. 按 Status 选项原始顺序构建列数组
  const columns: BoardColumn[] = meta.statusOptions.map((opt) => ({
    id: opt.id,
    name: opt.name,
    color: opt.color,
    items: columnMap.get(opt.id) ?? [],
  }));

  const totalItems =
    columns.reduce((sum, col) => sum + col.items.length, 0) +
    noStatusItems.length;

  return {
    views: meta.views,
    columns,
    noStatusItems,
    allItems,
    totalItems,
    projectTitle: meta.projectTitle,
    cachedAt: new Date().toISOString(),
    singleSelectFields: meta.allSingleSelectFields.map((f) => ({
      id: f.id,
      name: f.name,
      options: f.options,
    })),
  };
}

// ── API 路由 ──

export const GET: APIRoute = async () => {
  // 读取可选的 Project 专用环境变量
  const { projectToken, owner, number } = await tryGetProjectEnv();

  // 优先使用 GITHUB_PROJECT_TOKEN，fallback 到 GITHUB_TOKEN
  const token = projectToken || GITHUB_TOKEN || null;

  if (!token) {
    return new Response(
      JSON.stringify({
        error:
          "服务端未配置 GitHub Token（需要 GITHUB_PROJECT_TOKEN 或 GITHUB_TOKEN）",
      }),
      {
        status: 503,
        headers: { "Content-Type": "application/json" },
      },
    );
  }

  const cacheKey = `${owner}:${number}`;

  // 命中缓存则直接返回
  const cached = cache.get(cacheKey);
  if (cached && Date.now() - cached.timestamp < CACHE_TTL_MS) {
    return new Response(JSON.stringify(cached.data), {
      status: 200,
      headers: {
        "Content-Type": "application/json",
        "Cache-Control": "public, max-age=300",
        "X-Cache": "HIT",
      },
    });
  }

  // 初始化带鉴权的 graphql 客户端
  const graphqlWithAuth = graphql.defaults({
    headers: { authorization: `token ${token}` },
  });

  try {
    const data = await buildBoardData(graphqlWithAuth, owner, number);

    // 写入服务端缓存
    cache.set(cacheKey, { data, timestamp: Date.now() });

    return new Response(JSON.stringify(data), {
      status: 200,
      headers: {
        "Content-Type": "application/json",
        "Cache-Control": "public, max-age=300",
        "X-Cache": "MISS",
      },
    });
  } catch (err: unknown) {
    console.error("[project-board] 获取看板数据失败:", err);

    const maybeStatus = (err as { statusCode?: number }).statusCode;
    const httpStatus = maybeStatus === 404 ? 404 : 500;
    const message =
      err instanceof Error ? err.message : "获取 GitHub Projects 数据失败";

    return new Response(
      JSON.stringify({ error: message, detail: String(err) }),
      {
        status: httpStatus,
        headers: { "Content-Type": "application/json" },
      },
    );
  }
};
