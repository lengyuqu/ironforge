# Multi-Agent Planning — IronForge

> 稳定的协作约束。活的任务状态只写在 `tasks/Txx.md`；`TASKS.md` 只放协议与协调者维护的索引。
> 本文件不记录已完成的批次；历史决策见 git 历史。

## Project Context

| Field | Value |
|---|---|
| Project | IronForge（Rust Git 托管平台，workspace 多 crate） |
| Goal | 按 issue/差距清单并行推进 Agent 友好化与高危缺陷修复 |
| Base branch | `main` |
| 开发主仓 | GitHub `lengyuqu/ironforge`（CI 权威） |
| 镜像 | CNB `lengyuqu/ironforge`（只读镜像，发布时按需同步） |

## Scope and Boundaries

- Required outcome：每个任务的验收命令在本机跑绿，合并后不破坏 workspace 既有测试。
- Explicitly out of scope：产品决策（权限语义、fail-open/fail-closed）、CI 流水线本身、前端 UI。
- Shared configuration that Agents must not change：
  - `Cargo.toml` / `Cargo.lock`（新增依赖先在计划里提出，由协调者裁决）
  - `crates/rg-http/src/lib.rs` 的路由注册段、`crates/rg-http/src/openapi.rs`、`crates/rg-core/src/auth/route_meta.rs`
  - `crates/rg-db/src/migrations/`（迁移序号会撞车）
  - `.github/`、`scripts/`、`deploy/`、`.cnb.yml`
  - `CLAUDE.md`、`docs/`（由协调者在本批合并后统一更新，避免多路改同一行）
- External or manual decisions：DB 迁移、认证/权限语义变更、Git 线协议改动——先记账，不动码。

## Shared Contracts

新批次开始前，把跨任务契约登记在这里；已完成契约不保留。

| Contract | Source path | Owner task | Consumers | Version |
|---|---|---|---|---|
| （示例）合并目标 | `crates/rg-git/src/ops.rs` | Txx | — | main |

## Task Design Rules

1. 每个任务一个 ID、一个明确交付物、明确的 owned paths、一个 Agent owner。
2. 同时可认领的任务不得改同一路径。
3. 契约/ schema 任务先于消费它的任务。
4. 验收命令定义任务边界，但不决定协调者何时合并。
5. `tasks/Txx.md` 是自身状态（`available` / `in_progress` / `completed`）的唯一权威；`TASKS.md` 只是镜像。
6. 没有计划与任务文档的认领无效；认领提交必须带 `tasks/Txx.md` 与 `docs/plans/Txx-*.md`。
7. `tasks/` 或 `TASKS.md` 的冲突只由协调者解决，必须保留远端侧的每一行。

## Claim and Worktree Model

Agent 读取基线分支上的 `tasks/Txx.md`，选一个 `available` 任务，创建记录的分支与 worktree。只改自己任务文件里的 `State`、`Agent`、`Branch`、`Worktree`、`Claimed at`，把 `State` 改为 `in_progress`，连同计划/任务文档一起提交，然后普通快进推送。**第一次成功推送获胜**；被拒绝的推送不是有效认领，禁止强推。

```sh
git fetch github
git worktree add ../ironforge-txx -b agent/txx-<slug> github/main
```

实现完成、验收命令通过后，把 `State` 改为 `completed` 并推送任务分支。`completed` 只表示可审，不等于已合并或已发布。48 小时无活动的认领可由协调者重置为 `available`。

## Work Allocation

| Area | Owned paths | Contract owner | Dependencies | Notes |
|---|---|---|---|---|
| （示例）Git 性能 | `crates/rg-git/src/` | Txx | — | 热区，禁止顺手改合并门禁 |

## Decision Log

只追加本批次仍有效的决策；已完成的批次随任务记录一起清理。

| Date | Decision | Reason | Affected tasks | Decided by |
|---|---|---|---|---|
