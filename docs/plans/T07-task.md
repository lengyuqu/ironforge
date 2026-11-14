# T07 Task 确认 — [#19] PR 头部漂移：旧票失效事件化 + 门禁阻断

> 权威状态在 `tasks/T07.md`；本文件记录约束确认、触碰路径与风险。执行细节见
> `docs/plans/T07-plan.md`。基线 `5f32ce4`，分支 `agent/t07-head-drift`。

## 1. 约束确认

| 约束 | 结论 |
| --- | --- |
| 不做门禁票数表切换 | 是。票源仍是 `pr_reviews`，`current_human_approver_ids` 的 latest + `commit_id` 语义未动；理由见 plan §2 |
| head 同步用 CAS，0 行返回 `Raced` 不重试 | 是。`UPDATE … WHERE id AND head_sha IS <读到值>`；0 行不覆盖、不碰票 |
| `new_sha == stored` → `Unchanged`，不写库不产事件 | 是。提前返回，单测以 `updated_at` 未变作证 |
| `head_sha` 为 NULL 的老 PR 允许直接设值 | 是。CAS 守卫改为 `head_sha IS NULL`，返回 `previous_head_sha: None` |
| 多条同 head_branch 的 open PR 全部同步并列出 | 是。返回按 id 排序的 `Vec<SyncedPullRequest>`，空列表表示无 PR |
| head 真的变化时调 `expire_on_head_sha_change`，旧申请置 expired 不删行 | 是。`action = protected_branch_merge`、`subject = pr_id`、比较基准为新 head；未改 `authorization_ops.rs` |
| 票仍在 `pr_reviews`，靠 `commit_id` 不匹配自然不计票 | 是。有端到端用例断言「旧票行仍在、门禁 FAIL」 |
| 门禁在 stored ≠ 实时 tip 时拒绝合并，复用 `HEAD_SHA_STALE` | 是。`merge_gates::head_sha_stale_gate`，readiness 与 merge 同一条规则、同一句 message |
| 行为变更：分支前进后必须先重新批准 | 是，**有意为之**。受影响测试逐条列在 plan §6，全部改测试、未放宽实现 |
| `post_push_hooks` 只加/换一处调用 | 是。把原 `update_open_head_sha` 调用替换为 `sync_pr_head_on_push`，周边未重构 |
| 不碰 `build_routes` / `openapi.rs` / `route_meta.rs` | 是，全部未触碰 |
| 不改 `Cargo.*`、迁移、`CLAUDE.md`、`ironforge-docs/`、`.cnb.yml`、`.github/`、`scripts/`、`deploy/` | 是，全部未触碰 |
| 不改 `authorization_ops.rs` | 是。T06 的签名（新 head 作比较基准）满足需要，未改 |
| 不顺手做 CI 切表 / 通知邮件 / `scope_expand` / MCP | 是 |
| 只推分支，不 push main，不 force-push | 是，只推 `agent/t07-head-drift` |

## 2. 触碰路径

owned paths 内：

- `crates/rg-core/src/pull_request/head_sync.rs`（新建，含内嵌单测）
- `crates/rg-core/src/pull_request/mod.rs`（`pub mod head_sync;`）
- `crates/rg-core/src/branch_protection/merge_gates.rs`
- `crates/rg-core/src/pull_request/readiness.rs`
- `crates/rg-http/src/lib.rs`（仅 `post_push_hooks` 内一段）
- `crates/rg-http/tests/merge_readiness_tests.rs`、`crates/rg-http/tests/pr_permission_tests.rs`

owned paths 之外（必要的签名串联，见 plan §7）：

- `crates/rg-core/src/branch_protection/service.rs`（`check_merge_allowed` 增 `live_head`）
- `crates/rg-core/src/pull_request/service.rs`（`try_auto_merge_pinned` 传入实时 tip）
- `crates/rg-core/src/pull_request/merge_queue.rs`（同上）
- `crates/rg-http/src/api/pulls.rs`（`head_now` 读取提前并传入）

## 3. 依赖

`Depends on: none`。直接用 T06 已在 `main` 的产物：
`rg_db::ops::authorization_ops::expire_on_head_sha_change`、`authorization_requests` /
`authorization_decisions` 两张表、`rg_core::authorization::Action`。无新依赖、无迁移。

## 4. 风险与处置

| 风险 | 处置 |
| --- | --- |
| 行为变更导致既有测试变红 | 逐条列出（plan §6）；点名跑 `merge_readiness_tests` / `pr_review_tests` / `auto_merge_pinned_head_tests`，全部通过 |
| 异步 push 钩子与合并之间的窗口 | 同步是异步的，窗口内合并会被拒（安全的一侧），客户端重试；plan §9 记录 |
| 老数据 `head_sha = NULL` 被卡死 | 门禁 `NotApplicable`、同步补值、失效跳过 NULL 三条分支都有单测 |
| 并发/重复 push 把 head 回滚 | CAS + 0 行即 `Raced` 不重试；单测 `a_concurrent_writer_is_not_overwritten` |
| 与 T08 在 `lib.rs` 冲突 | 只在 `post_push_hooks` 函数体内改，`build_routes` 一行未碰 |
| 本机一次全量 `rg-http` 出现既有竞态用例偶发失败 | 单独与后续全量均通过；plan §9 记录原因（用例依赖异步钩子时机） |
| 未在 CI 验证 | 本仓库当前无可用 CI；报告如实写明「未经 CI」 |
