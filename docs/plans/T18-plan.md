# T18 执行计划 — [#15 步骤6] `pr.opened` 加事务 + `pr.closed` 修正事件顺序

> 权威约束见 `tasks/T18.md`。基线 `6628bab`，分支 `agent/t18-pr-open-close-txn`，worktree `../ironforge-t18`。
> 验收命令真实输出、触碰路径与遗留风险见 `docs/plans/T18-task.md`。

## 1. 交付边界

做：

1. `rg-db::ops::pull_request_ops::create` 放宽为 `create<C: ConnectionTrait>(db: &C, model)`，让调用方能传事务。
   `update` 本来就是泛型，未动。
2. `pull_request::service::create_pr`：`pull_request_ops::create` + `pr_event_ops::record` +
   `record_event_in_txn(.., "pull_request.opened", ..)` 同一事务；commit 后 `deliver_event`，失败只 warn。
3. `pull_request::service::update_pr`：**删掉状态变更块里的 `trigger_pr_closed` 调用**（它发生在 `update` 之前），
   `pr_event_ops::record` 与 `pull_request.closed` 的日志行一起放进 `update` 所在事务，commit 后再投递。
4. 删除 `webhook::service::{trigger_pr_opened, trigger_pr_closed}`（唯一生产调用点已改写），原位留注释说明原因。
5. 新增 `crates/rg-core/tests/pr_open_close_event_txn_tests.rs`：4 条测试，其中一条用 SQLite 触发器注入
   「update 失败」，专门证明顺序被修正。
6. 测试 `webhook_envelope_tests.rs` 里原本用 `trigger_pr_opened` 钉信封形状的那条，改为直接构造同一份 payload
   走 `trigger_event`（信封形状覆盖不因删助手而丢失）。

不做（明确排除）：`issue.closed`、`milestone.closed`（纯通知函数、无业务写，见 §5）、`branch.*`/`tag.*`、
outbox 表、`update_pr` 里 draft 校验 / `auto_merge_enabled` 重置 / `closed_at` 补写 / ActiveModel 显式 `Set`。

未改：`Cargo.*`、迁移、`rg-http/src/`、`crates/rg-core/src/issue/service.rs`、`CLAUDE.md`、`ironforge-docs/`、
`.cnb.yml`、`.github/`、`scripts/`、`deploy/`、`crates/rg-mcp/`。

## 2. 顺序缺陷（本任务的核心，不只是「缺事务」）

改前（`6628bab`）：

```
:405  let was_open = pr.state == "open";
:406  pr.state = s.clone();                          // 只改内存
:415  if was_open && s == "closed" {
:416      let close_payload = json!({ id, repo_id, number, title, state });
:424      trigger_pr_closed(db, ..)                  // ← 先写日志、先投递 webhook
:469  let updated = pull_request_ops::update(db, active).await?;   // ← 后写库
```

两条事件路线里，前四步搬的都是「事件在业务写之后」（丢事件）。这一条更重：事件在业务写**之前**，
`update` 失败时消费者已经收到「PR 已关闭」，而库里 PR 仍是 `open`。**发出不存在的事实**。

所以本任务的修法不是「给事件补一个事务」，而是：

1. 把事件构造与写入整体移到 `update` 之后；
2. 把 `update`、`pr_events` 记录、`pull_request.closed` 日志行放进同一事务；
3. 事务提交后才投递。

改后：

```rust
let was_open = pr.state == "open";      // 提到状态变更块外，供后面的事件块使用
if let Some(s) = &state { /* 只改内存里的 pr，不发事件 */ }

// ActiveModel 显式 Set（原样未动）
let txn = db.begin().await.context("db: begin PR update transaction")?;
let updated = pull_request_ops::update(&txn, active).await?;
// pr_event_ops::record（draft 变化、state 变化）改传 &txn
let closed_event = if was_open && updated.state == "closed" {
    let close_payload = json!({ id, repo_id, number, title, state });   // 5 字段
    let event_payload = json!({ "event": "pull_request.closed", "pull_request": close_payload });
    record_event_in_txn(&txn, updated.repo_id, "pull_request.closed", &event_payload).await?;
    Some(event_payload)
} else { None };
txn.commit().await.context("db: commit PR update transaction")?;
if let Some(event_payload) = closed_event {
    if let Err(e) = deliver_event(db, updated.repo_id, "pull_request.closed", &event_payload).await {
        tracing::warn!("Failed to deliver PR closed webhook: {e}");
    }
}
```

`was_open` 的作用域问题：它本来在 `if let Some(s)` 的 `match` 分支里。因为 `pr.state` 在 `if let Some(s)` 之前
不会被任何分支改写（title/body/milestone/draft 都不碰 `state`），把它提到 `if let Some(s)` 之前求值与原语义
逐字节等价，不需要重构函数。任务「卡住就停」的检查点没有触发：中间没有别的写，事务边界就是 `update` 一节。

## 3. payload 逐字节等价性论证

`trigger_pr_opened` / `trigger_pr_closed` 原本只做一层包装：

```rust
pub async fn trigger_pr_opened(db, repo_id, pr: &Value) -> Result<()> {
    let payload = json!({ "event": "pull_request.opened", "pull_request": pr });
    trigger_event(db, repo_id, "pull_request.opened", &payload).await
}
```

`trigger_event` 把 `payload` 原样放进信封 `data`；`record_event_in_txn` / `deliver_event` 用同一个入参。

| 维度 | 旧 | 新 |
| --- | --- | --- |
| `pr.opened` 内层 | `json!({ id, repo_id, number, title, state, head_branch, base_branch, head_repo_id, author_id })`，取自 `pr` 行 | **同一段字面量**，同一个来源；测试用 `len() == 9` 钉住 |
| `pr.closed` 内层 | `json!({ id, repo_id, number, title, state })`（`state` 用请求里的 `s`） | 同一段字面量，`state` 用 `updated.state`；测试用 `len() == 5` 钉住 |
| 外层 | `{ event, pull_request }` 两键 | 同一段字面量 |
| `event` 名 | `"pull_request.opened"` / `"pull_request.closed"` | 同一字面量（`record_event_in_txn` 的 `event` 参数与 payload 内 `event` 键） |
| actor | `event_actor(db, payload)`：内层有 `author_id` → 该用户 | `event_actor_on(&txn, payload)`，规则相同 → 同一用户（测试观察到 actor 为 `pr-event-owner`） |
| 时机 | 写库之前（closed）/ 写库之后但事务外（opened） | 同一事务内、写库之后 |

`pr.closed` 的 `state`：旧代码取请求参数 `s`，新代码取 `updated.state`，两者在 `was_open && updated.state == "closed"`
条件下必然同为 `"closed"`。`title` 旧代码取已经改过的内存 `pr.title`，新代码取 `updated.title`——`update` 刚把它写下去，
是同一个值；用 `updated` 反而更诚实（事件只描述已提交的事实）。

## 4. `trigger_pr_*`：删除，而不是保留

同 T16（`trigger_pr_merged`）/ T17（`trigger_release_created`）的处理：

1. 全仓 grep：改写后 `trigger_pr_closed` 无任何调用点；`trigger_pr_opened` 只剩 `webhook_envelope_tests.rs`
   一个测试调用，已改为直接构造同一份 payload 走 `trigger_event`，覆盖不降级。
2. 这两个助手的语义就是缺陷本身：`trigger_event` = 「先写日志、再投递」，与业务写没有事务关系；
   保留它们等于留一个随时被误用回旧形状的 API。`trigger_pr_closed` 更糟——旧调用点在写之前。
3. 原位留注释说明「为什么这里没有这两个函数」，防止后来者照抄历史代码补回来。

`trigger_release_deleted` / `trigger_branch_*` / `trigger_tag_*` / `trigger_issue_*` 未动（不在本任务范围）。

## 5. `milestone.closed` 为什么不改

`notify_milestone_closed`（`issue/service.rs:485`）是纯通知函数：只写事件、没有业务写。它被 `update_issue` 的
fail-soft 块调用（`:339` `if let Err(e) = ... { warn }`）。给它「加事务」只会把一次事件写包进一个单语句事务，
不提供任何原子性收益，却改变了它「通知失败不失败业务」的既有语义。本任务**不碰** `issue/service.rs`。

## 6. 验证策略

新增 `crates/rg-core/tests/pr_open_close_event_txn_tests.rs`（独立测试二进制，沿用 loopback receiver 模式；
真实 SQLite + 真实 `create_pr` / `update_pr`，不 mock）：

1. `a_created_pr_logs_and_delivers_pull_request_opened_exactly_once`
   - `repo_events` 里 `pull_request.opened` **恰 1 行**；`data.event == "pull_request.opened"`；
     `data.pull_request` 9 字段与 DB 行一致，键数 `== 9`；
   - receiver 收到 1 次，`X-IronForge-Event == pull_request.opened`，投递体 `data == 日志 data`；
   - `webhook_deliveries` 恰 1 行；随后 500ms 内无第二次投递；
   - `pr_events` 恰 1 行（timeline 与事件同事务）。
2. `a_closed_pr_logs_and_delivers_pull_request_closed_exactly_once`
   - 同上，5 字段、键数 `== 5`；并断言库里 `state == "closed"`、`closed_at.is_some()`（事件描述的是已提交事实）。
3. **`an_update_that_fails_publishes_no_pull_request_closed` —— 唯一能区分先后的测试。**
   - 用一条 SQLite 触发器 `BEFORE UPDATE ON pull_requests ... RAISE(ABORT, ...)` 从**外部**注入 update 失败
     （不改生产代码、不 mock）；
   - 断言 `update_pr` 返回 `Err`；`repo_events` 无 `pull_request.closed` 行；receiver 500ms 内无请求；
     `webhook_deliveries` 无行；PR 仍是 `open`、`closed_at` 仍为 `NULL`。
   - 变异验证：把事件发布临时挪回 `update` 之前（复刻旧顺序），该测试变红（见 task 文档 §3）。
4. `a_title_only_update_publishes_no_pull_request_closed`
   - 只改 title：不写 `pull_request.closed`、不投递、无 delivery 行。
5. 「不重复写」变异测试：临时让 `deliver_event` 也 `append_envelope`，两条 exactly-once 测试立刻变红
   （见 task 文档 §4，撤销用 Edit）。
6. 既有测试全绿：`cargo test -p rg-core`（377 + 各集成二进制）、`rg-http` 的 PR 相关集成测试。

## 7. 遗留风险

1. 投递仍是「commit 后立即 spawn」：commit 与 spawn 之间崩溃 = 「有事件、无投递」。至少一次投递要等 outbox，
   与 T13/T16/T17 一致。
2. 日志信封与投递信封的 `id` 不同（`record_event_in_txn` 与 `deliver_event` 各自建信封，只拿到 payload）。
   业务 payload 一致（有断言）；outbox 由存表信封投递时恢复同一 id。
3. `update_pr` 的事务比原来覆盖更多：`update` + 最多 2 条 `pr_events` + 1 条 `repo_events`。SQLite 单写者下
   事务稍长，但仍是单请求内的短事务，实测无锁等待。
4. `create_pr` 现在在事件写失败时整体回滚：PR 不创建、调用方收到错误。这与 T13/T16/T17 的取舍一致
   ——宁可创建失败，也不要「PR 存在但 opened 事件永久缺失」。
5. `pr.closed` 仍然只在 `was_open && updated.state == "closed"` 时发出：`merged` 路径有自己独立的
   `pull_request.merged` 事件（T16），本任务不改。
6. `trigger_issue_opened` / `trigger_issue_closed` 等仍是「提交后写事件」的旧形状（T13/T16 遗留），留给后续清理。
