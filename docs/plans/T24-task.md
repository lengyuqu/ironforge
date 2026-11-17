# T24 执行记录 — agent 判定统一到单一来源

> 计划见 `docs/plans/T24-plan.md`，权威约束见 `tasks/T24.md`。
> 基线 `1388f5d`，分支 `agent/t24-agent-guard-unify`，worktree `../ironforge-t24`。

## 1. 开工核对

```
$ git fetch cnb && git rebase cnb/main
Current branch agent/t24-agent-guard-unify is up to date.
$ git log --oneline -3
1388f5d docs(tasks): add T24 to unify the agent-approval guard (R19-5)
6aa1337 docs(tasks): mark T23 completed in the mirror index
c4923c4 Merge task T23: 自批守卫下沉 core + 邮件文案/换行 + 删死代码（#19 R19-2/3/4）
```

三处现状与任务书一致（逐行核对）：

| # | 位置 | 原写法 |
| --- | --- | --- |
| 1 | `rg-http/src/api/auth.rs:165 ensure_reviewer_may_approve` | `find_by_id` → `is_agent`，另有 `ref_allowlist` 分支限制检查 |
| 2 | `rg-core/src/authorization/evaluator.rs:325 ensure_human_decider` | `find_by_id` → `is_agent` → `coded(403, "AGENT_MAY_NOT_APPROVE")` |
| 3 | `rg-http/src/api/ci_environments.rs:288` | `find_by_id(...).is_some_and(\|u\| !is_agent(&u))` |

## 2. 改动文件

| 文件 | 改动 |
| --- | --- |
| `crates/rg-core/src/agent.rs` | +5：`is_human_account`；+62：`human_account_tests`（3 条） |
| `crates/rg-core/src/authorization/evaluator.rs` | `ensure_human_decider` 内部改调 `is_human_account`，签名与错误不变 |
| `crates/rg-http/src/api/auth.rs` | `ensure_reviewer_may_approve` 改调 `is_human_account`，分支 token 检查原位保留 |
| `crates/rg-http/src/api/ci_environments.rs` | 审批闸门改调 `is_human_account` |
| `docs/plans/T24-plan.md`、`docs/plans/T24-task.md` | 新增 |
| `tasks/T24.md` | State → `completed` |

统计：4 个源文件，+106 / −29。

## 3. 实现

```rust
// crates/rg-core/src/agent.rs
pub async fn is_human_account(db: &DatabaseConnection, user_id: i64) -> CoreResult<bool> {
    Ok(user_ops::find_by_id(db, user_id)
        .await?
        .is_some_and(|user| !is_agent(&user) && user.deleted_at.is_none()))
}
```

**与任务书式子的一处偏差（必要）**：任务书写的实现是
`find_by_id` → `is_some_and(|u| !is_agent(&u))`，但同一条验证 2 又要求「软删用户 → false」。
`user_ops::find_by_id`（`rg-db/src/ops/user_ops.rs:28`）不过滤 `deleted_at`，全仓也没有任何
users 的 `DeletedAt` 过滤，按原式写会让**软删的人类账号返回 `true`**。因此加了
`user.deleted_at.is_none()`。语义仍是 fail-closed（读不到 → `false`）。

**不判 `is_active`**：三处原本都不看启用状态，加上即行为变更；停用是各调用方自己的规则。
单测 `a_disabled_person_is_still_a_person` 把这条边界固定下来。

三处接法：

```rust
// #1 auth.rs（match 三分支，分支 token 检查原位不动）
match rg_core::agent::is_human_account(db, user_id).await {
    Ok(true) => {}
    Ok(false) => return Err(/* 原样：coded 403 + with_reason(AGENT_MAY_NOT_APPROVE) */),
    Err(error) => return Err(error.into()),   // 原 `?` 的效果：Internal → 500
}
// …原样的 ref_allowlist 检查…

// #2 evaluator.rs（保持公开 API 与错误）
if crate::agent::is_human_account(db, user_id).await? {
    return Ok(());
}
Err(CoreError::coded(403, "AGENT_MAY_NOT_APPROVE", "an agent may request an authorization but never approve one"))

// #3 ci_environments.rs
let approving_account = match rg_core::agent::is_human_account(&state.db, actor_id).await {
    Ok(human) => human,
    Err(error) => return AppError::internal(error).into_response(),
};
```

## 4. 唯一可观察的行为差异（fail-closed）

「查不到 / 软删账号」原本各自有独立文案，接入后与「是 agent」合流：

| # | 改前（查不到账号） | 改后 |
| --- | --- | --- |
| 1 | 403 `no such account` | 403 `an agent may not approve a pull request` / `AGENT_MAY_NOT_APPROVE` |
| 2 | 403 `only a person may approve`（`CoreError::Forbidden`） | 403 coded `AGENT_MAY_NOT_APPROVE` |
| 3 | 落到 `false` → 403（文案本来就与 agent 同一句） | 同左，无差异 |

三者都是 403、都是拒绝，方向不变；且该分支在真实请求里到不了（session 校验先挡掉不存在的账号）。
软删账号原本会被当成「人」放行，现在被拒绝——同样是收紧方向。除此之外无文案变化，
现有测试断言的 `(code, reason)` 全部不变（见 §6.4）。

## 5. 第四处检查：没有第四处

扫了全仓 `is_agent(` 与 `AGENT_MAY_NOT_APPROVE` 的全部调用点：

| 位置 | 它判的是什么 | 结论 |
| --- | --- | --- |
| `rg-http/src/api/ci_environments.rs:80-99`（`validate_approvers`） | 写审批人名单时不允许 agent | **不接**：要区分「不存在」（400 `approver user N does not exist`）与「是 agent」（400 `user N is an agent; an environment approver has to be a person`），布尔判定给不出两种文案；且是配置写入校验，不是决定闸门 |
| `rg-core/src/authorization/scope_expand.rs:365 ensure_decider_is_not_the_agent_owner` | 「agent 的**所有者**不得批准自己 agent 的扩容」 | **不接**：不是「agent 不得批准」。agent 本人那条已由 `mod.rs:228` 的 `ensure_human_decider` 先拦；这条是自批规则的延伸，判的是 `agent.owner_user_id == Some(decider_id)` |
| `rg-core/src/import/service.rs:849 reviewer_is_agent` | 导入 GitHub 历史评审时识别 agent 作者 | **不接**：刻意 fail-open（DB 错误只 warn、返回 `false`，不让导入中断），接进来会把失败语义从「放行」变成「拒绝」，是行为变更 |
| `rg-ci/src/quota.rs:266 is_agent_subject` | agent 的 CI 配额 | 不相关 |
| `collab.rs:113`、`sso.rs:779`、`drafts.rs:737`、`audit.rs:357`、`webhook/service.rs:264`、`rg-ssh/src/lib.rs:365`、`user/service.rs:258/288/655/711/802` | 协作组、SSO 绑定、草稿归属、审计显示、webhook、SSH、账号管理 | 都不是「决定闸门」 |

结论：**本任务覆盖了「agent 不得审批」的全部三处**，无第四处接入。

## 6. 验证映射（4 条）

| # | 任务书要求 | 落点 | 结果 |
| --- | --- | --- | --- |
| 1 | 三处行为不变 | `agent_principal_tests`（`t20_*` 11 条）、`authorization` 单测、`pr_review_tests`、`ci_permission_tests`、`rg-http` 全量 | ok（§6.1） |
| 2 | `is_human_account` 单测四态 | `agent::human_account_tests` 3 条（含「停用仍是人」） | ok（§6.2） |
| 3 | 变异测试：三处同时变红 | `t20_1` / `an_agent_cannot_approve_whatever_its_scope` / `t20_11` | ok（§6.3） |
| 4 | 错误信息逐字不变 | `t20_1`、`t20_11` 抓真实响应体；`an_agent_cannot_approve_whatever_its_scope` 断言 code + 403 | ok（§6.4） |

### 6.1 三处行为不变

```
$ cargo test -p rg-http --test agent_principal_tests t20_
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 12 filtered out

$ cargo test -p rg-core --lib authorization::
（authorization 单测全绿，见 §7.1 的 401 passed）
```

### 6.2 `is_human_account` 单测

```
$ cargo test -p rg-core --lib agent::human_account_tests
running 3 tests
test agent::human_account_tests::a_person_may_decide_and_an_agent_may_not ... ok      # 人类 true / agent false
test agent::human_account_tests::an_account_that_cannot_be_read_is_not_a_person ... ok # 不存在 id false / 软删 false
test agent::human_account_tests::a_disabled_person_is_still_a_person ... ok            # 停用仍是人
test result: ok. 3 passed; 0 failed
```

### 6.3 变异测试（关键证据）

变异体（`crates/rg-core/src/agent.rs`，用 Edit 改、用 Edit 撤销）：

```rust
- .is_some_and(|user| !is_agent(&user) && user.deleted_at.is_none()))
+ .is_some_and(|user| !is_agent(&user) || is_agent(&user) && user.deleted_at.is_none()))
```

即「对 agent 也返回 `true`」。三处各自的一条测试分别变红：

```
=== [1] PR approve ===
test gate::t20_1_an_agent_token_without_a_branch_limit_cannot_approve ... FAILED
test result: FAILED. 0 passed; 1 failed; 0 ignored

=== [2] authorization ticket ===
test authorization::tests::an_agent_cannot_approve_whatever_its_scope ... FAILED
test result: FAILED. 0 passed; 1 failed; 0 ignored

=== [3] environment approval ===
test gate::t20_11_an_agent_named_in_an_old_list_still_cannot_release ... FAILED
test result: FAILED. 0 passed; 1 failed; 0 ignored
```

失败细节（证明红的正是该闸门）：

```
# [3] 变异后审批真的被记录、作业真的被放行（200 + released:true），说明拦它的就是这一层
panicked at crates/rg-http/tests/agent_principal_tests.rs:1368:13:
assertion `left == right` failed: the approval must be refused:
  {"approvals":1,"job_id":1,"released":true,"required_approvals":1}
  left: 200   right: 403

# [2] 变异后不再返回 coded AGENT_MAY_NOT_APPROVE，而是走到后面另一道检查
panicked at crates/rg-core/src/authorization/mod.rs:523:22:
expected a coded error, got Forbidden("this account may not decide this request")
```

撤销变异后复跑：

```
$ cargo test -p rg-core --lib an_agent_cannot_approve_whatever_its_scope
test result: ok. 1 passed; 0 failed
$ cargo test -p rg-http --test agent_principal_tests t20_
test result: ok. 11 passed; 0 failed
```

三处同时变红 ⇒ 三处都走 `is_human_account`，没有一个还在用旧判定。

### 6.4 错误信息逐字不变

源码里三段字面量在 diff 中只是搬位置，未改一个字符（`git diff -U2` 可见）。实际响应体
（临时把断言期望改错以打印 body，随后用 Edit 还原；`git status` 确认 `agent_principal_tests.rs` 无改动）：

```
#1 PR approve（t20_1）
{"error":{"code":"FORBIDDEN","hint":"ask a person to review and approve",
          "message":"an agent may not approve a pull request",
          "reason":"AGENT_MAY_NOT_APPROVE","retryable":false}}

#3 environment approval（t20_11）
{"error":{"code":"FORBIDDEN","hint":"an environment approval has to come from a person",
          "message":"this account may not approve a protected environment",
          "reason":"AGENT_MAY_NOT_APPROVE","retryable":false}}
```

与改前源码字面量逐字一致：

```
auth.rs:              "an agent may not approve a pull request" / AGENT_MAY_NOT_APPROVE / "ask a person to review and approve"
ci_environments.rs:   "this account may not approve a protected environment" / AGENT_MAY_NOT_APPROVE / "an environment approval has to come from a person"
```

`#2` 是 core 错误，不经过 HTTP 序列化；`an_agent_cannot_approve_whatever_its_scope` 断言
`code == "AGENT_MAY_NOT_APPROVE"` 且 `status == 403`，与改前相同；文案
`"an agent may request an authorization but never approve one"` 在 diff 中原样保留。

## 7. 验收（实际运行输出）

### 7.1 `cargo test -p rg-core`

```
lib            401 passed; 0 failed; 0 ignored
collab_search_tests            1 passed
issue_event_txn_tests          3 passed
multi_backend_smoke            0 passed; 2 ignored
outbox_sweep_tests             8 passed
pr_merge_event_txn_tests       2 passed
pr_open_close_event_txn_tests  4 passed
release_event_txn_tests        2 passed
review_self_guard_tests        4 passed
webhook_envelope_tests        12 passed
doc-tests                      1 passed; 5 ignored
合计 passed=438  failed=0  ignored=7
```

### 7.2 `cargo test -p rg-http`（全量）

第一次全量跑在 `auto_merge_pinned_head_tests` 上失败一次，**与本改动无关**，理由：

- 该用例与 agent、授权票、环境审批均无交集；
- 失败是「已移交的 head 被合并了」，根因是 push handler 内联触发的
  `try_auto_merges_for_head_commit`（`rg-http/src/lib.rs:3579`）与测试自己的
  `try_auto_merge` 之间的时序竞争，属 T07 记录过的同类负载敏感用例
  （`docs/plans/T07-plan.md`：「`n_tests::the_merge_queue_puts_a_moved_head_back_in_the_queue`
  在高负载下」会失败）；
- 单独重跑该二进制连续 3 次全绿：

```
$ cargo test -p rg-http --test auto_merge_pinned_head_tests   (×3)
test result: ok. 2 passed; 0 failed   (3/3)
```

第二次全量（完整跑通）：

```
test result: ok  ×134 个测试二进制
passed=867  failed=0  ignored=3
（其中 agent_principal_tests 23 条：11 条 t20_* + 12 条既有 agent 用例）
```

### 7.3 `cargo clippy --workspace --all-targets -- -D warnings`

```
    Checking rg-db v0.2.0 / rg-core v0.2.0 / rg-ssh / rg-ci / rg-cli
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 48.13s
```

无输出 = 无警告。

### 7.4 `scripts/check.sh --quick`

```
==> cargo fmt --all --check
    (1s)
==> cargo clippy --workspace --all-targets -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.27s
    (1s)
==> checks passed in 2s (quick)
```

## 8. 未做 / 遗留

- 未统一三处错误结构（任务书禁止，也是本次刻意的边界）。
- `validate_approvers`（名单写入）与 `reviewer_is_agent`（导入）刻意保留各自的判定与失败语义，
  理由见 §5。
- 「查不到账号」的文案在上述三处合流（§4），无测试依赖旧文案。
