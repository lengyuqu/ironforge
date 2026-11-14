# Multi-Agent Planning — IronForge

> 本文件定义稳定的协作约束。活的任​​务状态只写在 `tasks/Txx.md`；`TASKS.md` 只放协议与协调者维护的镜像索引。
> 来源：`templates/` 下的 aifuke 已验证方案（v1.0），结合本仓 OPT-LOOP 铁律收敛。

## Project Context

| Field | Value |
| --- | --- |
| Project | IronForge（Rust Git 托管平台，workspace 多 crate） |
| Goal | 按 issue 清单并行推进 agent 友好化路线图（T/R 系列）与高危缺陷修复 |
| Human coordinator | 余曲（别问 / cnb.biewen） |
| Remote | `cnb`（`https://cnb.cool/zdzso/ironforge.git`）—— **唯一推送目标** |
| Base branch | `main` |
| Baseline SHA | `0523bb2` |

## Scope and Boundaries

- Required outcome：每个任务的验收命令在本机跑绿，代码合并后不破坏 workspace 既有测试。
- Explicitly out of scope：产品决策（是否放开某权限、是否 fail-closed）、CI 流水线本身、前端 UI。
- Shared configuration that Agents must not change：
  - `Cargo.toml` / `Cargo.lock`（新增依赖必须先在计划里提出，由协调者裁决）
  - `crates/rg-http/src/lib.rs` 的路由注册段、`crates/rg-http/src/openapi.rs`、`crates/rg-core/src/auth/route_meta.rs`（新增端点三处手写，天然串行）
  - `crates/rg-db/src/migrations/`（迁移序号会撞车）
  - `.cnb.yml`、`.github/`、`scripts/`、`deploy/`
  - `CLAUDE.md`、`ironforge-docs/`（由协调者在本批合并后统一更新，避免三路同时改同一行）
- External or manual decisions：DB 迁移、认证/权限语义变更、Git 线协议改动 —— 一律先记账、不动码。

## Shared Contracts

| Contract | Source path | Owner task | Consumers | Baseline/version |
| --- | --- | --- | --- | --- |
| 合并目标必须是 `pr.base_branch` | `crates/rg-git/src/ops.rs`（`gix_merge_no_ff` / `gix_squash_merge`） | T01 | T02/T03 不消费，后续 R5 收尾 | `0523bb2` |
| MCP 写工具名清单 | `crates/rg-mcp/src/tools/mod.rs`（`TOOL_NAMES` / 分发 `match` / 参数校验） | T02 | 无 | `0523bb2` |
| 事件信封 `EventEnvelope` | `crates/rg-core/src/webhook/service.rs` | T03 | 后续 R3-P2/P3 | `0523bb2` |

## Task Design Rules

1. Every task has one ID, one clear deliverable, explicit owned paths, and one Agent owner.
2. Tasks that edit the same paths must not be available for claim at the same time.
3. Contract or schema tasks are claimed before tasks that consume those contracts.
4. Acceptance commands describe the task boundary but do not automatically decide when human coordination is complete.
5. Each `tasks/Txx.md` is the only source of truth for its own `available` / `in_progress` / `completed` state; `TASKS.md` holds only a mirror index.
6. A claim is invalid without its plan and task documents in `docs/plans/`; the claim commit must include them.
7. Merge conflicts in `tasks/` or `TASKS.md` are resolved by the coordinator only, preserving every row from the remote side. Never drop another task's file or row during conflict resolution.

## Claim and Worktree Model

An Agent reads the live base-branch `tasks/Txx.md`, selects one `available` task, and creates the recorded task branch/worktree. The Agent changes only `State`, `Agent`, `Branch`, `Worktree` and `Claimed at` in its own `tasks/Txx.md` to `in_progress`, commits the task's plan/task documents under `docs/plans/` together with the claim, then attempts a normal fast-forward push to the live base branch. The first successful push wins; a rejected push is not a valid claim and must never be forced.

```sh
git fetch cnb
git worktree add ../ironforge-t01 -b agent/t01-merge-target cnb/main
git worktree list
```

The claim commit must change only `tasks/Txx.md` plus the new plan/task documents, never `TASKS.md`. Push it with `git push cnb HEAD:main`; after that succeeds, publish the task branch with `git push -u cnb HEAD`. Agents then work only inside the recorded worktree and owned paths.

After implementation and task acceptance checks pass, the Agent changes only its own `tasks/Txx.md` from `in_progress` to `completed` in the final task-branch commit and pushes that branch. `completed` means development is ready for human review; it does not mean merged, released, or formally accepted. Reassignment, integration, and release remain under human coordination and are not additional task states. A claim showing no activity for 48 hours may be reset to `available` by the coordinator.

## Work Allocation

| Area | Owned paths | Contract owner | Dependent task IDs | Coordination notes |
| --- | --- | --- | --- | --- |
| PR 合并目标（`rg-git` 合并原语 + PR 服务） | `crates/rg-git/src/ops.rs`、`crates/rg-core/src/pull_request/service.rs`、`crates/rg-http/tests/` 下本任务新增测试 | T01 | 无 | 合并路径是热区，禁止顺手改 `merge_gates` |
| MCP 写工具集 | `crates/rg-mcp/src/tools/mod.rs`、`crates/rg-mcp/tests/write_tools_mcp.rs` | T02 | 无 | 服务端端点已存在（`f21517b`），只补 MCP 暴露 |
| 仓库事件与 webhook 投递 | `crates/rg-core/src/webhook/service.rs`、`crates/rg-http/src/api/repo_events.rs`、`crates/rg-http/src/ws.rs` | T03 | 无 | P1 只做信封与投递头，不做 outbox |

## Decision Log

| Date | Decision | Reason | Affected tasks | Decided by |
| --- | --- | --- | --- | --- |
| 2026-11-14 | 第一批并行只开 T01/T02/T03 | 三者文件重叠为零，且都不碰路由注册、`Cargo.*`、迁移序号 | T01,T02,T03 | 余曲 |
| 2026-11-14 | R7/R9/R10/R8 不进第一批 | R7 与 R6 同片写路径；R9 独占 `Cargo.*` 与路由注册；R8/R10 需新迁移 | — | 余曲 |
| 2026-11-14 | 禁止任何任务改 `CLAUDE.md` 与 `ironforge-docs/` | 三路同时改同一行必然冲突，文档由协调者合并后统一补 | T01,T02,T03 | 余曲 |
