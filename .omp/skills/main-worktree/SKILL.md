---
name: main-worktree
description: >-
  在不切换当前 main 分支的情况下，通过 git worktree 编辑 stable 分支内容（hotfix），
  然后合并回 main 保持 stable⊆main 不变式。用户说「hotfix」「改 stable」「stable worktree」
  「在 stable 上修」「同步回 main」时使用。关键词：worktree, stable, hotfix, 热修复,
  稳定分支, 合并回 main, 不切分支, git worktree
---

# stable 分支 worktree 热修流程

主目录始终停在 `main`；`stable` 挂在仓库内 worktree `.worktrees/stable`（已在 `.gitignore`）。
适用场景：需要直接修改 `stable`（稳定分支）内容的 hotfix，改完必须同回合合并回 `main`。

## 流程

```bash
# 0. worktree 不存在则创建（仅首次；已存在直接跳过）
git worktree list
git worktree add .worktrees/stable stable

# 1. 更新 stable worktree
git -C .worktrees/stable pull --ff-only   # 有远端更新时

# 2. 在 .worktrees/stable 内编辑、验证、提交（Conventional Commits 中文；
#    commit/push 仍需用户明确要求 —— 红线）

# 3. 回主目录合并回 main，恢复不变式
git merge stable

# 4. 校验不变式：输出必须为空，否则违规，先修复
git log stable --not main --oneline
```

## 红线（不可违反）

- `stable` 上只做 hotfix / cherry-pick，禁止直接开发新功能（功能一律在 `main`）。
- hotfix 进 `stable` 后**同一回合**必须合并回 `main`，不许留到以后。
- 未经用户明确要求禁止 commit / push / tag；推送 v* tag 触发不可逆发布流水线。
- 稳定 tag `vX.Y.Z` 只在 stable worktree（`.worktrees/stable`）里打。

## 注意

- 同一分支不能同时被两个 worktree checkout；主目录保持 `main`。
- worktree 有独立的 `target/`、`build/`、`.dart_tool/`，首次构建全量编译属正常。
- 收尾可选：`git worktree remove .worktrees/stable`（长期保留也没问题）。
