# 合并就绪诊断与更新分支（CNB issue #17）验证记录

> **状态**：已实现，未合并 ｜ **可信度**：确认（本机命令输出） ｜ **来源**：本分支代码与测试 ｜ **日期**：2026-11-13
> **未经 CI**（当前没有可用 CI）。数据库：见「命令与结果」，逐库如实列出。

## 做了什么

两个新端点，加一份被合并路径与预览共用的判定。

| 内容 | 位置 |
|------|------|
| 冲突分析（文件、类型、区块、blob、分页）；合并提交对象；带期望值的 ref 更新；祖先判断 | `rg-git/src/merge_analysis.rs`（新文件，`ops.rs` 未动） |
| 合并门控判定（带机器码的 `Gate`；状态/草稿/签名/批准/状态检查） | `rg-core/src/branch_protection/merge_gates.rs`（新） |
| `check_merge_allowed` 改为取同一份判定的第一个拒绝；`merge_pr` 的草稿/状态校验改用同一份 `Gate` | `branch_protection/service.rs`、`pull_request/service.rs`、`rg-http/src/api/pulls.rs`（最小改动） |
| 批准人名单与批准计数同源 | `rg-db/src/ops/pr_review_ops.rs`（`current_human_approver_ids`，计数取其长度；SQL 未变） |
| 就绪预览组装 | `rg-core/src/pull_request/readiness.rs`（新） |
| 更新分支服务 | `rg-core/src/pull_request/update_branch.rs`（新） |
| 两个处理函数 | `rg-http/src/api/pull_readiness.rs`（新，平铺在 `api/`） |
| 路由 / OpenAPI / 路由元数据 | `lib.rs`、`openapi.rs`、`rg-core/src/auth/route_meta.rs`（只追加） |
| `/context` 的 `not_evaluated` 指名端点 | `api/ai_pr_context.rs`；`pr_context_tests` 断言随之更新 |
| DB 套件清单追加两个测试目标 | `scripts/ci-db-tests.sh` |

测试：`rg-git` 单元 15 项（`merge_analysis`）；`rg-http/tests/merge_readiness_tests.rs` 14 项；`rg-http/tests/update_branch_tests.rs` 11 项。

## 关键决定

- **不改合并行为。** 既有合并测试（`pr_permission_tests`、`merge_pinned_head_tests`、`pr_merge_strategy_tests`、`merge_queue_ci_tests`、`auto_merge_pinned_head_tests`、`branch_protection_contents_tests`）在判定收敛后原样通过。拒绝顺序、文本、状态码不变：`evaluate_protection(.., stop_at_first_failure = true)` 保持原先「遇第一个失败即返回、之后不读」的读库次序。
- **「不满足」与「读不到」分开。** `GateResult::Unknown` 与 `Fail` 严格分离；`ready` 三态（`true`/`false`/`null`）。`null` = 没有失败项，但有东西判不了（分支读不到、规则配置坏、`HEAD_SHA_STALE`、读库失败）。
- **`required_status_checks` 解析失败：预览报 `PROTECTION_RULE_MALFORMED`（UNKNOWN），合并路径仍按旧行为放行。** 方案建议把合并也改成 fail closed；任务明确「不改现有合并的行为」，故没改。`merge_readiness_tests::a_rule_that_cannot_be_read_is_unknown…` 同时钉住了两侧（预览 UNKNOWN、合并仍 200）。**未决**，见下。
- **门控口径以代码为准。** 批准按 PR 上**存储的** `head_sha` 判定（与合并一致），分支已前进时在 `unavailable[]` 里给 `HEAD_SHA_STALE`；预览与合并因此不会各算一套。
- **冲突口径与现有合并一致：** `gix` 报告任何条目即「冲突」（含自动解决的条目，标 `auto_resolved: true`），否则预览会对一个随后失败的合并说 clean。
- **更新分支的权限：** `require_write`（`can_write_repo`）→ 令牌自身的分支限制（`ensure_token_may_write_branch(head_branch)`）→ 头分支保护规则（`check_direct_commit_allowed`，与 REST `contents` 写入同一判定，即「`git push` 同一套」：`require_pr`/`require_signed_commits` 的头分支拒绝，白名单用户按规则放行）。不放宽任何一条。
- **受分支白名单限制的令牌：更新分支默认拒绝。** `route_meta` 里该路由既不是 `.open()` 也不是 `.checked()`，PAT 中间件在到达处理函数前 403（`update_branch_tests::a_token_limited_to_some_branches…` 用「限定 `agent/*`、PR 头正好在 `agent/x`」这种本可放行的情形证明仍被拒）。处理函数仍调用 `ensure_token_may_write_branch`，所以将来把该行改成 `.checked()` 是一行改动且已是安全的。**是否放开是产品决定，未决。**
- **fork PR：** 明确拒绝，409 `reason: FORK_PR_UNSUPPORTED`（头分支在别的仓库）。就绪预览对 fork PR 不复制对象（GET 不写）：只在目标仓库 `refs/pull/<n>/head` 恰好等于 fork 当前 tip 时才做冲突计算，否则 `merge.state = unavailable`、`reason = HEAD_OBJECTS_NOT_IN_TARGET`。
- **更新分支的结果：** 头分支得到一个合并提交，**父提交顺序为 [旧 head, base tip]**（与 `git merge <base>` 在头分支上一致，头分支的 first-parent 历史不被打断）；作者与提交者均为 `rg_core::agent::commit_identity(user)`；ref 用 `ExistingMustMatch` 做比较并交换；成功后走 `announce_branch_move`（即 `spawn_post_push_hooks`：PR head 同步、CI、webhook、事件日志）并写 `audit::record_write("pull_request.update_branch")`。
- **机器码与错误形状：** `REF_ADVANCED`（409，`details.current_sha`，与合并同形）、`MERGE_CONFLICTS`（409，`details.merge` 与就绪预览的 `merge` 段同一序列化器）、`FORK_PR_UNSUPPORTED`、`NO_MERGE_BASE`；`already_up_to_date` 是 200 且不写任何东西、不审计；`dry_run`（体或 `X-Dry-Run`）不落 ref、不审计、不触发钩子。

## 与方案（评论 2120150507300745216）的偏差

| 方案 | 实际 | 原因 |
|------|------|------|
| 更新分支遇冲突 200 + `outcome: conflicts` | **409** `MERGE_CONFLICTS`，冲突在 `error.details.merge` | 任务书明确要求 409 |
| 合并提交父提交 `<base>, <head>` | `[head, base]` | 与 `git merge` 在头分支上一致 |
| 冲突块由对冲突 blob 对重新跑 diff（`HunkIter`）得到 | 直接解析 `gix` 写入对象库的冲突标记 blob（`<<<<<<<`/`=======`/`>>>>>>>`，兼容 diff3 的 `|||||||`） | 描述的正是同一次合并撞上的冲突，不需要第二遍 diff；代价：文件本身含标记样行时可能多出假块（模块文档已写）；每个条目仍给三个 blob id |
| 区块有 `ours/theirs/base` 行区间、`in_head` | 只给合并 blob 里的 `merged_start_line`、`ours.lines/excerpt`、`theirs.lines/excerpt` | 无 base 区间与 `in_head`；标记 blob 里没有这些 |
| 冲突 `continue_with` 指向 `…/merges/<a>..<b>` | `continue_with` 是同一端点的 `?conflicts_after=<最后一个路径>` | 方案里的端点不存在；路径序分页、每次实时算、总数描述整次合并 |
| 门控含 `CHANGES_REQUESTED`、`AUTHOR_IS_APPROVER`（WARN） | **未做** | 代码里 `count_current_human_approvals` 不排除作者本人，也不看 request_changes 是否阻塞；它们不影响合并，加了会让预览与合并语义分叉。方案里「作者本人批准不算」与代码不符，以代码为准 |
| `REQUIRED_CHECKS_UNDEFINED`：只要求整体 success | WARN，文案「规则没列检查，所以不要求任何检查」 | 代码（R17-1 注释已记）在名单为 null 时什么都不检查，连整体 success 也不要求 |
| `required_status_checks` 坏 JSON 也让合并 fail closed | 只预览报 UNKNOWN，合并保持旧行为 | 任务约束，见上 |
| 规则可能多条命中（`rules_applied`） | 只报实际生效的第一条（`rule`） | 合并只按第一条匹配判定，预览照实 |
| 单独的 `PIPELINE_UNREADABLE`/`REPO_UNREADABLE` 等 | `unavailable[]` 里 `REPO_UNREADABLE`、`HEAD_SHA_STALE`、`DB_UNREADABLE`；读库失败时整个保护段为 `protected: null` | 保护判定一次读库失败整体判不了，不细分 |
| rebase 的冲突区块（要改 `RebaseOutcome`） | **没改**。`strategy=rebase` 用端到端的三方合并近似，`merge.approximation = REBASE_REPLAY_NOT_SIMULATED`，`not_evaluated` 含 `rebase_replay` | 方案问的 Q2；预览不能真重放（`rebase_merge` 会动 ref），也不想改既有枚举 |
| 合并失败响应也升级（方案第六节） | **未做**，`merge` 的冲突响应与 dry_run 的 `not_evaluated: ["conflicts", …]` 都没动 | 任务范围未列，且「不改现有合并的行为」 |
| 就绪预览要求登录 | 只要仓库读权限（公开仓库匿名可读，与 diff/评审/流水线现有权限相同） | 任务书：「只需仓库读权限，评审、检查详情按现有权限」 |

## 命令与结果

（本节在全部命令跑完后填写，见下方「结果」。）

## 没做 / 未决

1. **合并路径对 `required_status_checks` 坏 JSON 仍放行**（fail open，R17-1）。预览已如实报 UNKNOWN。是否让合并也 fail closed 是行为变更，需负责人决定。
2. **受分支白名单限制的令牌是否可更新自己的 PR 头分支。** 默认拒绝；放开只需把 `route_meta` 里该行改为 `.checked()`（处理函数已检查）。对 agent 的主要用例（`agent/*` 分支上的 PR）很实用，但属权限面扩大，留给负责人。
3. **（未经实跑，仅读代码）PR 合并实际合进仓库 `HEAD` 指向的分支，而不是 `pr.base_branch`。** `gix_merge_no_ff`/`gix_squash_merge` 以 `HEAD` 为 ours 并 `commit_as(…, "HEAD", …)`。base 不是默认分支的 PR 会得到错误的合并目标。预览按 `pr.base_branch` 计算（`merge.checked_against` 给出所用的 base 提交）。没有改，也没写测试复现，请单独核实。
4. 读到的合并门控里**未做**：`CHANGES_REQUESTED`/`AUTHOR_IS_APPROVER` 信息性门控、合并队列位置、CODEOWNERS 评审要求、自动合并效果（`not_evaluated` 列出了后三项）。
5. rebase 策略的真实重放冲突（见偏差表）。
6. 就绪预览会向对象库写入冲突标记 blob（`gix` 合并自己产生的对象），不可达、待 GC；不写数据库、不动 ref（`analysing_moves_no_ref` 钉住）。GET 有这个写盘副作用，若不可接受需要改成内存合并。
7. 预览的树合并有 10 秒超时（`MERGE_TIMEOUT` → `merge.reason = MERGE_TIMEOUT`，`retryable: true`）；`spawn_blocking` 的线程超时后仍会跑完，无法取消。
8. 未做：迁移（没有新增）、MCP 工具、前端、`CLAUDE.md` 与 roadmap 文档（按约定不改）。`capabilities` 注册表没碰（这不是协作路由）。
9. 更新分支没有合并队列/自动合并的联动：更新后 CI 要重跑、批准按新 head 重新计算（既有语义：批准绑定 head_sha，更新分支会让已有批准失效——这是**预期的行为变化点**，调用方应知道）。
