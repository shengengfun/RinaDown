/**
 * 一次性迁移脚本：把旧的独立功能投票 issue（标题前缀 "[FeatureVote]"）
 * 合并进反馈体系 —— 重命名为反馈格式标题，补 user-feedback / enhancement
 * 标签，并把 body 重写为 feedback 格式（供 issue 详情弹窗解析元数据）。
 *
 * 投票记录无需迁移：tracking issue 中的记录以 issue number 为键，
 * issue 本身不变，票数自动延续。
 *
 * 用法: node scripts/migrate-feature-votes.mjs [--dry-run]
 * 环境: GITHUB_TOKEN / GITHUB_REPO（缺省时自动读取同目录 ../.env）
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const DRY_RUN = process.argv.includes("--dry-run");
const PREFIX = "[FeatureVote]";
const VOTES_ISSUE_TITLE = "[FluxDown] Feature Vote Records";

// ── env ──
function loadDotEnv() {
  try {
    const envPath = join(dirname(fileURLToPath(import.meta.url)), "..", ".env");
    for (const line of readFileSync(envPath, "utf8").split(/\r?\n/)) {
      const m = line.match(/^([A-Z_][A-Z0-9_]*)=(.*)$/);
      if (m && !process.env[m[1]]) process.env[m[1]] = m[2].trim();
    }
  } catch {
    // no .env — rely on process env
  }
}
loadDotEnv();

const TOKEN = process.env.GITHUB_TOKEN;
const REPO = process.env.GITHUB_REPO;
if (!TOKEN || !REPO) {
  console.error("Missing GITHUB_TOKEN / GITHUB_REPO");
  process.exit(1);
}

const headers = {
  Authorization: `Bearer ${TOKEN}`,
  Accept: "application/vnd.github+json",
  "X-GitHub-Api-Version": "2022-11-28",
  "Content-Type": "application/json",
};

async function listOpenIssues() {
  const all = [];
  for (let page = 1; page <= 5; page++) {
    const res = await fetch(
      `https://api.github.com/repos/${REPO}/issues?state=open&per_page=100&page=${page}`,
      { headers },
    );
    if (!res.ok) throw new Error(`list issues: ${res.status} ${await res.text()}`);
    const batch = await res.json();
    all.push(...batch.filter((i) => !i.pull_request));
    if (batch.length < 100) break;
  }
  return all;
}

/** 从旧 body 提取描述与 meta（<!-- fluxdown:feature-meta {...} -->） */
function parseOldBody(body) {
  const raw = body ?? "";
  let meta = null;
  const m = raw.match(/<!--\s*fluxdown:feature-meta\s*([\s\S]*?)-->/);
  if (m) {
    try {
      meta = JSON.parse(m[1]);
    } catch {
      // malformed meta — ignore
    }
  }
  const description = raw
    .replace(/<!--\s*fluxdown:feature-meta[\s\S]*?-->/g, "")
    .trim();
  return { description, meta };
}

function buildFeedbackBody(description, meta, createdAt) {
  return [
    "## Feature Request",
    "",
    "### 建议内容 / Proposal",
    "",
    description || "_(no description)_",
    "",
    "---",
    "",
    "**Type:** feature",
    "**Source:** Website feature vote",
    `**Submitted:** ${meta?.date ?? createdAt}`,
    meta?.ip ? `**IP:** \`${meta.ip}\`` : null,
  ]
    .filter((l) => l !== null)
    .join("\n");
}

const issues = await listOpenIssues();
const targets = issues.filter(
  (i) => i.title.startsWith(PREFIX) && i.title !== VOTES_ISSUE_TITLE,
);
console.log(`Found ${targets.length} [FeatureVote] issue(s) to migrate.`);

let ok = 0;
for (const issue of targets) {
  const cleanTitle = issue.title.slice(PREFIX.length).trim();
  const { description, meta } = parseOldBody(issue.body);
  const patch = {
    title: `\u2728 [Website Feedback] ${cleanTitle}`,
    body: buildFeedbackBody(description, meta, issue.created_at),
    labels: [
      ...new Set([
        ...issue.labels.map((l) => (typeof l === "string" ? l : l.name)),
        "user-feedback",
        "enhancement",
      ]),
    ],
  };

  if (DRY_RUN) {
    console.log(`[dry-run] #${issue.number}: "${issue.title}" → "${patch.title}"`);
    continue;
  }

  const res = await fetch(
    `https://api.github.com/repos/${REPO}/issues/${issue.number}`,
    { method: "PATCH", headers, body: JSON.stringify(patch) },
  );
  if (!res.ok) {
    console.error(`FAILED #${issue.number}: ${res.status} ${await res.text()}`);
  } else {
    ok++;
    console.log(`Migrated #${issue.number}: ${patch.title}`);
  }
}

console.log(DRY_RUN ? "Dry run complete." : `Done. ${ok}/${targets.length} migrated.`);
