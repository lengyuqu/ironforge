# Multi-Agent Planning — IronForge

> 本文件定义稳定的协作约束。活的任​​务状态只写在 `tasks/Txx.md`；`TASKS.md` 只放协议与协调者维护的镜像索引。
> 来源：`templates/` 下的 aifuke 已验证方案（v1.0），结合本仓 OPT-LOOP 铁律收敛。

## Project Context

| Field | Value |
| --- | --- |
| Project | IronForge（Rust Git 托管平台，workspace 多 crate） |
| Goal | 按 issue 清单并行推进 agent 友好化路线图（T/R 系列）与高危缺陷修复 |
| Human coordinator | 余曲（别问 / cnb.biewen） |
| Remote | `cnb`（`https://cnb.cool/lengyuqu/ironforge.git`）—— **唯一推送目标** |
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
| 2026-11-14 | 第二批开 T04（R7 密钥扫描）+ T05（R10 配额），R9 暂不开 | T04 只需把已在 `Cargo.lock` 的 `regex` 提为直接依赖；T05 独占迁移；R9 的 tree-sitter 授权虽已拿到但需独占 `Cargo.*` 与路由注册，放到下一批 | T04,T05 | 余曲 |
| 2026-11-14 | R9（#39）依赖策略：**允许引入 tree-sitter + 5 个 grammar** | 结构化大纲需要真 AST，正则在 TS/Go/Python 上精度不足；代价是 6 个新 crate 与更长构建，由该任务独占 `Cargo.*` 承担 | T06（待建） | 余曲 |
| 2026-11-14 | R7 扫描超限口径取 **fail-open + 强制审计事件** | 首次导入大仓是真实场景，硬拒会阻断迁移；`secret_scan_skipped` 事件让管理员可见 | T04 | 余曲 |
| 2026-11-14 | R10 配额只对 `kind = agent` 生效 | 人类账号不受限，避免误伤现有用户；最小闭环不新增 REST 端点以避开路由注册争用 | T05 | 余曲 |
| 2026-11-25 | 第四批开 T32–T36，对应差距清单四个 MISSING 硬伤 + CNB 真机验证 | 四块失分合计 3.35（CI Actions 1.50 / HA 1.00 / Git 基线 0.75 / Unit 权限 0.60），是 82.1% → 100% 的主要障碍 | T32,T33,T34,T35,T36 | 余曲 |
| 2026-11-25 | T33（Unit 权限）与 T35（HA）**只出文档不改码** | 前者属 L2 认证语义变更且必改 `route_meta.rs` 与迁移序号；后者依赖 `deploy/`（禁区）与三层架构决策。两者都必须先有可审的设计，再由协调者裁决实现批次 | T33,T35 | 余曲 |
| 2026-11-25 | T34 本轮只做 composite action 展开，node/docker 继续 fail closed | ADR-0001 已冻结执行边界；composite 是纯 YAML 展开、零新运行时，能吃到真实场景又不触碰沙箱。沙箱执行另开任务 | T34 | 余曲 |
| 2026-11-25 | 本批四任务文件重叠为零，可同时认领 | T32 只碰 `rg-git/tests/`，T33/T35 只碰 `docs/plans/`，T34 只碰 `rg-ci/`，T36 只碰 `rg-core/src/import/` 的 CNB 段 | T32–T36 | 余曲 |
