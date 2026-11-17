# T24 执行计划 — [#19·R19-5] agent 判定统一到单一来源

> 权威约束见 `tasks/T24.md`。基线 `1388f5d`，分支 `agent/t24-agent-guard-unify`，
> worktree `../ironforge-t24`。实际执行与验收输出见 `docs/plans/T24-task.md`。

## 1. 问题与边界

「agent 不得审批」在基线有三份独立实现，各自查库、各自判定 `is_agent`。改一处必忘两处，
且三处对「查不到账号」的处理写法不同（`ok_or_else` / `is_some_and`），看起来像三种规则。

| # | 位置 | 原写法 | 查不到账号 |
| --- | --- | --- | --- |
| 1 | `rg-http/src/api/auth.rs:165 ensure_reviewer_may_approve` | `find_by_id` → `is_agent` | `forbidden("no such account")` |
| 2 | `rg-core/src/authorization/evaluator.rs:325 ensure_human_decider` | 同上 | `forbidden("only a person may approve")` |
| 3 | `rg-http/src/api/ci_environments.rs:288` | `is_some_and(!is_agent)` | 落到 `false` → 拒绝 |

**不改的是错误结构**。三处的 403 文案与 `reason` 各不相同（`AGENT_MAY_NOT_APPROVE` 的
`hint`、`retryable`，授权票的 coded 403，环境审批的另一种措辞），把它统一成一个
`CoreResult<()>` 会强迫统一错误包装、破坏现有客户端可读的提示，也可能打掉契约测试。
本任务只统一**「这个 id 是不是人」这一个事实**。

## 2. 加一个纯判定

`crates/rg-core/src/agent.rs`，紧邻 `is_agent`：

```rust
pub async fn is_human_account(db: &DatabaseConnection, user_id: i64) -> CoreResult<bool>
```

**为什么不复用 `ensure_human`**：它返回 `CoreResult<User>`，查不到时给
`not_found("user not found")`，agent 时给 `forbidden("an agent account may not do this; ask its owner")`。
它的语义是「拿一个人类账号」，调用方要的是「能不能」，硬套会把两处 403 文案改成第三种，
正是任务书禁止的方向。`ensure_human` 不动。

**软删账号**：任务书给的实现式子是 `find_by_id` → `is_some_and(!is_agent)`，但同一条任务的
验证 2 又要求「软删用户 → false」。`user_ops::find_by_id` 不过滤 `deleted_at`
（`rg-db/src/ops/user_ops.rs:28-33`，全仓也没有任何 users 的 `DeletedAt` 过滤），
所以只按式子写会让软删的人类账号返回 `true`。实现取两者交集：

```rust
Ok(user_ops::find_by_id(db, user_id).await?
    .is_some_and(|user| !is_agent(&user) && user.deleted_at.is_none()))
```

即「存在 + 不是 agent + 未软删」。仍然 fail-closed：读不到就是 `false`。

**不判 `is_active`**：三处原本都不看启用状态，加上就是行为变更（例如停用账号在环境审批里
原本能过）。停用是各调用方自己的规则。单测里有一条把这条边界写死。

## 3. 三处怎么接

统一形状：

```rust
match rg_core::agent::is_human_account(db, user_id).await {
    Ok(true) => {}
    Ok(false) => return /* 该处原有的 403，一字不改 */,
    Err(e) => return /* 该处原有的 internal */,
}
```

| # | 接法 | 保留什么 |
| --- | --- | --- |
| 1 | `match` 三分支 | **分支受限 token 检查原位不动**，仍是第二道闸；`Err` 走 `error.into()`（`CoreError::Internal` → 500），与原来 `find_by_id(..)?` 的 `From<DbErr> for AppError` 同结果 |
| 2 | `if is_human_account(..).await? { Ok(()) } else { Err(原 coded 403) }` | 函数签名与 code/文案不动（core 公开 API，`scope_expand` 与 `mod.rs` 在用） |
| 3 | `let approving_account = match ... { Ok(human) => human, Err(e) => internal }` | 下游 `if !approving_account { 原 coded 403 }` 不动 |

一致性说明：三处原本在「查不到账号」时的文案不同，接入后该分支与「是 agent」合流。
两者都是 403、都是拒绝，方向仍是 fail-closed；且该分支在真实请求里到不了
（session 校验先挡掉不存在的账号）。这是本任务唯一可观察的文案变化，逐条记入
`T24-task.md` §4。

## 4. 第四处检查

按任务书要求扫了全仓 `is_agent` / `AGENT_MAY_NOT_APPROVE` 的调用点，结论是**没有第四处
「agent 不得批准」判定**，细节见 `T24-task.md` §5。最像的两处：

- `rg-http/src/api/ci_environments.rs:80-99`：写审批人名单时拒绝 agent。它要区分
  「不存在」（400 `approver user N does not exist`）与「是 agent」（400 `user N is an agent; …`），
  布尔判定给不出两种文案；而且是配置写入校验，不是决定闸门。**不接**。
- `rg-core/src/authorization/scope_expand.rs:365 ensure_decider_is_not_the_agent_owner`：
  规则是「agent 的**所有者**不得批准自己 agent 的扩容」，不是「agent 不得批准」。
  agent 本人那条已由 `mod.rs:228` 的 `ensure_human_decider` 先拦。**不接**。

## 5. 验证设计

1. 三处行为不变：`agent_principal_tests`（`t20_*`，11 条）、`authorization` 单测、
   `pr_review_tests`、`ci_permission_tests`。
2. `is_human_account` 单测四态：人类 / agent / 不存在 / 软删，另加「停用仍是人」。
3. **变异测试**：把判定改成对 agent 也返回 `true`，三处各自的一条测试都要变红。
   选点必须是真正穿过该闸门的那条：
   - #1 `t20_1_an_agent_token_without_a_branch_limit_cannot_approve`（无分支限制的 agent token，
     正是该闸门本身）
   - #2 `an_agent_cannot_approve_whatever_its_scope`（core 直调 `approve`）
   - #3 `t20_11_an_agent_named_in_an_old_list_still_cannot_release`（名单里已有 agent，
     只能靠 handler 内的账号判定拦住）
4. 文案逐字不变：三处 message 与 reason 在 diff 里只是搬位置，没有改写；现有测试断言的
   `(code, reason)` 对不变。

## 6. 不做

- 不改 `is_agent`。
- 不统一三处错误结构。
- 不动 `ensure_human`、`owner_cap`、`accountable_user`、`resolve_actors`。
- 不改 `Cargo.*`、迁移、`review/`、`pull_request/`、`branch_protection/`、
  `authorization/`（除 `ensure_human_decider` 内部）、`scripts/`、`.cnb.yml`、`.github/`、
  `deploy/`、`rg-mcp/`、`CLAUDE.md`、`ironforge-docs/`。
