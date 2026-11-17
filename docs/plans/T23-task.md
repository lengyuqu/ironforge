# T23 执行记录 — 自批守卫下沉 / 邮件文案与换行 / 死代码删除

> 计划见 `docs/plans/T23-plan.md`，权威约束见 `tasks/T23.md`。
> 基线 `54f9e73`，分支 `agent/t23-review-guards-email`，worktree `../ironforge-t23`。

## 1. 开工核对

```
$ git fetch cnb && git rebase cnb/main
Current branch agent/t23-review-guards-email is up to date.
$ git log --oneline -1
54f9e73 docs(tasks): add T23 for R19-2/3/4 (core guard, email copy, dead code)
```

`ReviewAction` 的实际变体（`crates/rg-core/src/review/service.rs:19-28`）：
`Comment` / `Approve` / `RequestChanges` / `Dismiss`——与任务书一致，`Approve` +
`RequestChanges` 是「决定」，其余两个不是。

`submit_review` 的全部调用点（`grep -rn "submit_review" crates/`）：

| 调用点 | 动作 | 是否命中新守卫 |
| --- | --- | --- |
| `rg-http/src/api/reviews.rs:378`（`POST .../reviews`） | 客户端指定 | 会，但 HTTP 层 `:331` 先返回 400 |
| `rg-http/src/api/reviews.rs:884`（行内评论的隐式评审） | 固定 `Comment` | 不会 |
| `rg-mcp/` | 无 | — |

## 2. 改动文件

| 文件 | 改动 |
| --- | --- |
| `crates/rg-core/src/review/service.rs` | +14：`submit_review` 内自批守卫 |
| `crates/rg-core/src/email/mod.rs` | 模板抽成 `render_html`；尾句改写；`<p>` 加 `white-space: pre-wrap`；+5 条模板单测 |
| `crates/rg-db/src/ops/pr_review_ops.rs` | −4：删除 `count_approvals`，原位留注释 |
| `crates/rg-core/tests/review_self_guard_tests.rs` | 新增：4 条直调 core 的守卫测试 |
| `docs/plans/T23-plan.md`、`docs/plans/T23-task.md` | 新增 |
| `tasks/T23.md` | State → `completed` |

`crates/rg-http/src/lib.rs` **未改动**——理由见 §4.2。

## 3. R19-2 实现

`crates/rg-core/src/review/service.rs:78-90`，位置在「PR 存在 + `state == "open"`」之后、
构造 `ActiveModel` 之前：

```rust
if matches!(action, ReviewAction::Approve | ReviewAction::RequestChanges)
    && pr.author_id == reviewer_id
{
    return Err(CoreError::invalid_input(
        "a PR author cannot approve or request changes on their own PR",
    ));
}
```

错误串与 HTTP 层 `reviews.rs:333` **逐字相同**，所以即便某天 HTTP 那道被绕过，客户端看到的措辞
也不变（状态码会从 400 变成 core → HTTP 的映射值，这正是保留 HTTP 那道的原因）。

### 3.1 变异验证：这条测试确实测到了守卫

把守卫临时改成 `if false && …` 后重跑（随后恢复）：

```
---- the_author_cannot_approve_their_own_pr_through_the_service stdout ----
thread '…' panicked at crates/rg-core/tests/review_self_guard_tests.rs:177:6:
a self-approval must fail in the service, not only over HTTP:
Model { id: 1, pr_id: 1, repo_id: 1, reviewer_id: 1, action: "approve", … }

failures:
    the_author_cannot_approve_their_own_pr_through_the_service
    the_author_cannot_request_changes_on_their_own_pr_through_the_service
test result: FAILED. 2 passed; 2 failed
```

## 4. R19-3 实现

### 4.1 尾句

```
- You received this email because you have notifications enabled on IronForge.
+ You received this email because an action on IronForge involved your account.
```

为什么不用任务书的示例措辞（「you are involved in this repository」）：这个模板有三个调用点，
其中 `rg-core/src/user/service.rs:745` 是**密码重置**——与任何仓库无关，也与通知偏好无关
（用户即使关闭全部通知也必须收到）。仓库相关或偏好相关的措辞在那封信里都是假的。

**核对基线后的一处修正**：任务书写「系统没有通知开关（`user` 表无此字段、`notifications` 表无偏好列）」，
但 `notification_preferences` 表已由 `m20261115_000002`（T19 步骤 5）建立，`dispatch.rs`
确实按 `prefs.email.enabled` 决定是否发信。所以本条的真实缺陷不是「开关不存在」，而是
**共用的模板在不受该开关管辖的邮件里替收件人宣称了一个从未在这里给出的同意**。改写后的句子
对三条路径都成立，且不指向任何设置页。

### 4.2 换行：调用方早已是 `\n`，缺的是模板的 `pre-wrap`

```
$ git log --oneline -3 -L 3695,3706:crates/rg-http/src/lib.rs
88d7079 feat(notifications): per-user notification preferences and one delivery exit (#19 step 5)
…  // 该提交把 `<br/><br/>` 换成了 `\n\n`，并留注释「the shared notification template escapes its body」
```

```
$ grep -rn '<br' crates/ --include=*.rs
crates/rg-core/src/package_registry/adapters/pypi.rs:341   # 仓库索引页，不是邮件
（其余命中均为 refs/heads/<branch> 之类的注释）
```

即：任务书所说的「调用方传 `<br/>` 被转义成 `&lt;br/&gt;`」在基线**已不存在**；但 `\n` 仍然不显示，
因为 `<p>` 没有 `white-space`，HTML 把换行折叠成空格。所以本次改的是真正的缺口：

```
- <p style="color: #4b5563; line-height: 1.6;">{message}</p>
+ <p style="color: #4b5563; line-height: 1.6; white-space: pre-wrap;">{message}</p>
```

`html_escape` **保留**——`message` 里有 issue 标题、评论正文等用户文本
（`rg-core/src/issue/reactions.rs:118`、`rg-http/src/api/reviews.rs:707`），去掉即 XSS。

### 4.3 另两个调用点检查结论

| 调用点 | 传的 body | 有无 `<br/>` | 结论 |
| --- | --- | --- | --- |
| `rg-core/src/notification/dispatch.rs:292` | `req.body.clone().unwrap_or_default()` | 无（上游构造的 body 全是纯文本 + `\n`，如 `lib.rs:3703`、`human_notify.rs`） | 不改；`pre-wrap` 对它的 `\n` 是正向修复 |
| `rg-core/src/user/service.rs:745` | 单段 `format!(…)`，无 `<br/>`、无换行 | 无 | 不改；`pre-wrap` 无副作用（无换行可保留） |

两处都**没有**同类问题需要改；只调整了共用模板，调用点逻辑未动。

### 4.4 可测试性

模板原本内联在 `send_html_notification` 里，只能通过真实 SMTP 观察输出。抽出私有
`render_html(title, message, action_url) -> String`，`send_html_notification` 调它，
签名与行为不变，换来 5 条 CI 可跑的模板断言（§6 验证 3/4/5）。

## 5. R19-4 实现

```rust
// `count_approvals(db, pr_id)` lived here until #19 R19-4. Removed: it had no
// caller, and its `None` branch counts only reviews with no `commit_id`, which
// is not the merge gate's yardstick. Call
// `count_current_human_approvals` with the head SHA instead.
```

删除前核对：`grep -rn "pr_review_ops::count_approvals" crates/` 无结果；`ci_environment_ops::count_approvals`
（`rg-http/src/api/ci_environments.rs:312`、`agent_principal_tests.rs`）与
`authorization_ops::count_approvals`（`rg-core/src/authorization/mod.rs:283,347`、
`rg-db/tests/authorization_store.rs:158`）是另外两套，均未改动。

删除后 `grep -rn "count_approvals" crates/rg-db/src/ops/pr_review_ops.rs` 只剩那行注释。

## 6. 验证映射（7 条）

| # | 任务书要求 | 落点 | 结果 |
| --- | --- | --- | --- |
| 1 | 直调 core：author 自批 approve → Err，author 自评 comment → Ok | `rg-core/tests/review_self_guard_tests.rs` 4 条 | ok（含变异验证，见 §3.1） |
| 2 | HTTP 自批仍 400、错误信息不变 | `rg-http/tests/pr_review_tests.rs::the_author_still_cannot_decide_their_own_pr`（未改） | ok |
| 3 | 生成 HTML 不含 "notifications enabled" | `email::tests::the_footer_does_not_claim_a_notification_switch` | ok |
| 4 | 含 `\n` 的 body：`&lt;br` 不出现、换行保留 | `email::tests::newlines_survive_without_html_markup` | ok |
| 5 | `<script>alert(1)</script>` 必须被转义 | `email::tests::user_text_is_escaped`、`titles_are_escaped` | ok |
| 6 | `count_approvals` 不存在，clippy 无 dead_code | 见 §5；`cargo clippy --workspace --all-targets -D warnings` | ok，无警告 |
| 7 | 既有 `pr_review_tests` 全绿 | `cargo test -p rg-http` 全量 | ok，5/5 |

第 1 条的变异验证（§3.1）说明：只测 HTTP 层时这条守卫是可以被静默移除的；加了 core 测试后移除
即变红。这就是本次下沉的可验证价值。

## 7. 验收（实际运行输出）

### 7.1 `cargo test -p rg-core`

```
running 398 tests            (src/lib.rs)
test result: ok. 398 passed; 0 failed; 0 ignored
     Running tests/collab_search_tests.rs          1 passed
     Running tests/issue_event_txn_tests.rs        3 passed
     Running tests/multi_backend_smoke.rs          0 passed, 2 ignored
     Running tests/outbox_sweep_tests.rs           8 passed
     Running tests/pr_merge_event_txn_tests.rs     2 passed
     Running tests/pr_open_close_event_txn_tests.rs 4 passed
     Running tests/release_event_txn_tests.rs      2 passed
     Running tests/review_self_guard_tests.rs      4 passed
     Running tests/webhook_envelope_tests.rs      12 passed
（另有 doc-tests：1 passed, 5 ignored）
全部 0 failed
```

### 7.2 `cargo test -p rg-db`

```
62 passed / 5 passed / 12 passed / 5 passed / 0 passed(1 ignored) / 15 passed / 1 passed
全部 0 failed
```

### 7.3 `cargo test -p rg-http`（全量）

```
binaries=134 passed=867 failed=0 ignored=3
real 9m51s
```

其中与本任务直接相关的两个套件：

```
$ cargo test -p rg-http --test pr_review_tests
running 5 tests
test the_author_still_cannot_decide_their_own_pr ... ok
test a_writer_may_approve ... ok
test a_refused_approval_does_not_count_towards_the_merge_gate ... ok
test a_requested_reviewer_may_approve_without_write_access ... ok
test a_reader_may_comment_but_not_approve ... ok
test result: ok. 5 passed; 0 failed; 0 ignored

$ cargo test -p rg-http --test pr_review_error_semantics_tests
running 9 tests
test review_endpoints_classify_errors ... ok
（其余 8 条同样 ok）
test result: ok. 9 passed; 0 failed; 0 ignored
```

### 7.4 `cargo clippy --workspace --all-targets -- -D warnings`

```
    Checking rg-db v0.2.0 …
    Checking rg-core v0.2.0 …
    Checking rg-http v0.2.0 …
    Checking rg-ssh / rg-ci / rg-cli
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 54.27s
```

无输出 = 无警告（`-D warnings` 下任何 dead_code 都会中断）。

### 7.5 `scripts/check.sh --quick`

```
==> cargo fmt --all --check
    (1s)
==> cargo clippy --workspace --all-targets -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 9.93s
    (10s)
==> checks passed in 11s (quick)
```

## 8. 未做 / 遗留

- **R19-5**（三处 agent 判定统一）未做，按任务书由单独任务承担。
- `send_html_notification` 的 `action_url` 仍直接插进 `href`（未转义）。它由服务端自己拼
  （`format!("{base}/reset-password?token=…")`），不是用户输入，属既有行为，未在本次改动。
- 历史邮件已发出，尾句修正只对今后发出的邮件生效。
