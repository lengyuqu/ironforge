# T23 执行计划 — [#19·R19-2/3/4] 自批守卫下沉 + 邮件文案 + 死代码清理

> 权威约束见 `tasks/T23.md`。基线 `54f9e73`，分支 `agent/t23-review-guards-email`，
> worktree `../ironforge-t23`。实际执行与验收输出见 `docs/plans/T23-task.md`。

## 1. 三件事与两条与任务书的偏差

| 条目 | 任务书要求 | 实际做法 |
| --- | --- | --- |
| R19-2 | `submit_review` 内加自批守卫 | 照做；`ReviewAction` 变体名确认为 `Approve` / `RequestChanges` |
| R19-3.1 | 尾句不能声称存在用户可配置开关 | 照做，但**措辞不采用任务书示例**，见 §3 |
| R19-3.2 | 调用方 `<br/>` 改 `\n` | **调用方已经是 `\n`**（`88d7079`，T19 步骤 5 已改），缺的是模板的 `pre-wrap`，见 §3 |
| R19-4 | 删 `pr_review_ops::count_approvals` | 照做，原位留注释 |

## 2. R19-2：自批守卫下沉 core

### 2.1 位置

`crates/rg-core/src/review/service.rs::submit_review`，在「PR 存在 + `state == "open"`」之后、
构造 `pr_review::ActiveModel` 之前。此时 `pr.author_id` 与 `reviewer_id` 都已在手，任何写库
（`pr_review_ops::create`、`pr_event_ops::record`）都还没发生。

```rust
if matches!(action, ReviewAction::Approve | ReviewAction::RequestChanges)
    && pr.author_id == reviewer_id
{
    return Err(CoreError::invalid_input(
        "a PR author cannot approve or request changes on their own PR",
    ));
}
```

### 2.2 为什么保留 HTTP 那道

`crates/rg-http/src/api/reviews.rs:331` 的守卫在 core 之前触发，返回 `400 AppError::bad_request`，
错误串完全相同。两道的分工：

| 调用面 | 由谁拦 |
| --- | --- |
| `POST /repos/:owner/:repo/pulls/:n/reviews`（显式动作） | HTTP 先拦（400），core 兜底 |
| `reviews.rs:883` 隐式 Comment 评审（行内评论） | 不命中（`action == Comment`） |
| 任何新调用面（MCP 工具 / 内部服务 / 批量导入） | **只有 core 这道** |

删掉 HTTP 那道会把错误码从 400 变成 core 映射的值，属于行为变更，不做。

### 2.3 语义边界

- 只拦 `Approve` / `RequestChanges`——两者都是「决定」，都参与合并门/评审语义。
- `Comment` / `Dismiss` 不拦：作者在自己的 PR 上讨论、撤回自己的评审都是正常操作。
- 只拦「自己评自己」，不拦 agent、不拦权限——那是 `ensure_reviewer_may_approve`
  与 `REVIEW_DECISION_REQUIRES_REVIEWER` 的职责，本任务不碰（R19-5 另做）。

## 3. R19-3：邮件尾句与换行

### 3.1 尾句：任务书的示例在本仓库不成立

任务书示例是「you are involved in this repository」，但 `send_html_notification` 是**三个调用点
共用一个模板**：

| 调用点 | 邮件性质 | 受通知偏好约束？ |
| --- | --- | --- |
| `rg-http/src/lib.rs:3949`（`email.send` 重试 handler） | 任意通知邮件 / CI 邮件 | 入队前已按偏好过滤 |
| `rg-core/src/notification/dispatch.rs:292` | 站内通知的邮件通道 | 是（`prefs.email.enabled`） |
| `rg-core/src/user/service.rs:745` | **密码重置** | **否** |

所以：

- 「notifications enabled」在密码重置邮件上是**纯属虚构**——这封信与通知偏好无关，
  用户即使关掉全部通知也必须收到；
- 「you are involved in this repository」同样不成立——密码重置与任何仓库无关。

采用的措辞：**`You received this email because an action on IronForge involved your account.`**
对三个调用点都成立，且不指向任何设置页、不暗示可配置开关。

补充事实：`notification_preferences` 表（`m20261115_000002`，T19 步骤 5）**确实存在**，所以
「系统完全没有通知开关」在当前基线上已不准确；本条的真实缺陷是**模板在非通知邮件上冒充通知邮件**。
这一点写进 `T23-task.md`，供后续改文案时参考。

### 3.2 换行：调用方已经是 `\n`，缺的是 `pre-wrap`

复核基线发现 `rg-http/src/lib.rs:3699-3705` 早已把 `<br/>` 换成 `\n`，并留了注释
（`88d7079`，T19 步骤 5）。也就是说任务书描述的「调用方传 `<br/>` 被转义成 `&lt;br/&gt;`」
**在基线已不存在**。

但换行仍然不显示：模板的 `<p>` 没有 `white-space`，HTML 会把 `\n` 折叠成空格。真正缺的是这一条。
因此：

1. 模板 `<p>` 加 `white-space: pre-wrap;`（保留换行、不引入标点重排）；
2. 调用方不需要改（已经是 `\n`）；
3. `html_escape` **保留**——`message` 里确实有用户文本（issue 标题、评论正文，见
   `issue/reactions.rs:118`、`api/reviews.rs:707`），去掉就是 XSS。

### 3.3 可测试性：把渲染抽成 `render_html`

模板原来内联在 `send_html_notification` 里，只能通过真实 SMTP 观察输出，无法在单测里断言。
抽出私有 `render_html(title, message, action_url) -> String`，`send_html_notification` 调它。
这是同文件内的纯提取，不改变任何调用点签名与语义；换来 5 条可在 CI 里跑的模板测试。

### 3.4 另两个调用点的检查结论

| 调用点 | 有无 `<br/>` | 有无同类问题 | 处置 |
| --- | --- | --- | --- |
| `dispatch.rs:292` | 无（`req.body` 原样传） | 无；body 由上游构造，均为纯文本 + `\n` | 不改 |
| `user/service.rs:745` | 无（单段文本） | 无；模板的 `pre-wrap` 对它无害（无换行） | 不改 |

## 4. R19-4：删除 `count_approvals`

`pr_review_ops::count_approvals` 全仓无调用方（`grep -rn "count_approvals" crates/` 只命中
`ci_environment_ops` 与 `authorization_ops` 的同名函数，是另外两套）。且它固定传 `head_sha = None`，
落到 `review.commit_id.is_none()` 分支，与合并门 `Some(head_sha)` 的口径不同。

删除后在原位留注释，指向正确入口：

```rust
// `count_approvals(db, pr_id)` lived here until #19 R19-4. Removed: it had no
// caller, and its `None` branch counts only reviews with no `commit_id`, which
// is not the merge gate's yardstick. Call
// `count_current_human_approvals` with the head SHA instead.
```

保留 `count_current_human_approvals` 与 `current_human_approver_ids` 不动（合并门在用）。

## 5. 不做

- R19-5（三份 agent 判定统一）。
- 不动 HTTP 层自批守卫、不动 `authorization/`、`pull_request/`、`branch_protection/`。
- 不改 `Cargo.*`、迁移、`scripts/`、`rg-mcp/`、`ironforge-docs/`。
- 不为 `action_html` 的 `href` 加转义：`action_url` 由服务端自己拼（`format!("{base}/reset-password?…")`），
  不是用户输入，属既有行为，不在本任务的价值面内。

## 6. 验证映射

| 任务书验证 | 落点 |
| --- | --- |
| 1 core 自批 | `rg-core/tests/review_self_guard_tests.rs`（4 条，直调 service） |
| 2 HTTP 自批 400 | `rg-http/tests/pr_review_tests.rs::the_author_still_cannot_decide_their_own_pr`（未改，回归） |
| 3 尾句 | `email::tests::the_footer_does_not_claim_a_notification_switch` |
| 4 换行 | `email::tests::newlines_survive_without_html_markup` |
| 5 转义 | `email::tests::user_text_is_escaped`、`titles_are_escaped` |
| 6 死代码 | 删除 + `cargo clippy --workspace --all-targets -D warnings` 无 `dead_code` |
| 7 既有 pr_review_tests | `cargo test -p rg-http`（全量） |
