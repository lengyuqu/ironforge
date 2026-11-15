# T13 执行计划 — [#15 步骤1] 事件写入可在事务内：签名放宽 + `issue.opened` 最小闭环

> 权威约束见 `tasks/T13.md`。基线 `b357bbe`，分支 `agent/t13-event-in-txn`，worktree `../ironforge-t13`。
> 验收命令的真实输出、触碰路径与遗留风险见 `docs/plans/T13-task.md`。

## 1. 交付边界

做：

1. 放宽**一条链**的签名：`repo_event_ops::append`、`webhook::service::{record_envelope, record_repo_event}`
   从 `&DatabaseConnection` 放宽到 `C: ConnectionTrait`。`latest_id` 保持原样。
2. 拆两个新公开函数：`record_event_in_txn`（只写日志，可在事务内调）与 `deliver_event`（只投递，不写日志）。
3. `trigger_event` 内部按「写日志 + 投递」两半重排，对外签名与行为不变（13 个 `trigger_*` 命名糖零改动）。
4. `issue::service::create_issue` 的 `issue.opened` 改为：事务内 `record_event_in_txn(&txn, ..)` → `commit`
   → `deliver_event(&db, ..)`。
5. 新增 `crates/rg-core/tests/issue_event_txn_tests.rs`：回滚不写事件、提交恰好一次、`trigger_event` 等价性。

不做（明确排除）：全仓 498 个签名收敛、outbox 表/迁移、重试策略改造、SSE、`scope_expand`、PR 路径
（`pull_request/service.rs` 留给后续任务）。

未改：`Cargo.toml` / `Cargo.lock`、`crates/rg-db/src/migrations/`、`crates/rg-http/src/`、`CLAUDE.md`、
`ironforge-docs/`、`.cnb.yml`、`.github/`、`scripts/`、`deploy/`、`crates/rg-mcp/`、
`crates/rg-core/src/pull_request/service.rs`。

## 2. 为什么这一步不需要 outbox 表

`#15` 的目标是 outbox，但 outbox 表解决的是**投递可靠性**（业务写与事件写同事务 + 至少一次投递）。
挡住它的第一件事不是表，而是**类型系统**：`repo_event_ops::append` 的入参写死 `&DatabaseConnection`，
而 SeaORM 1.1 的 `DatabaseTransaction` 不 `Deref` 到 `DatabaseConnection`
（`sea-orm-1.1.20/src/database/transaction.rs:17`：`conn: Arc<Mutex<InnerConnection>>`），
所以事务内根本无法写事件。

本任务先打通「事务内写 → 提交后投递」这条链路。它带来的直接收益已经可测：**崩溃窗口从
「业务已提交、事件永久丢失」缩小为「已提交、事件已写、尚未投递」**（投递仍是最佳努力，outbox 才补
至少一次）。等 outbox 落地时，它只是把「提交后按 payload 立即投递」换成「提交后由 worker 从表里取」，
本任务拆出的两个函数与验证模式原样复用，不需要推翻。

## 3. 机制与改动

### 3.1 签名放宽（只这条链）

| 函数 | 之前 | 现在 |
| --- | --- | --- |
| `rg_db::ops::repo_event_ops::append` | `db: &DatabaseConnection` | `conn: &C`，`C: ConnectionTrait` |
| `webhook::service::record_envelope`（私有） | `db: &DatabaseConnection` | `conn: &C`，`C: ConnectionTrait` |
| 原 `record_repo_event`（私有） | `db: &DatabaseConnection`，吞错 | 并入 `append_envelope<C>`，返回 `Result`（是否吞错由调用方决定） |
| `repo_event_ops::latest_id` | `&DatabaseConnection` | **不动**（只被读路径用） |

`DatabaseConnection` / `DatabaseTransaction` / `DatabaseConnectionProxy` 都实现 `ConnectionTrait`，
所以现有调用点（传 `&db`）不改一行就能编译，事务路径传 `&txn` 即可。

### 3.2 新函数与内部拆分

```rust
// 只写日志，不投递；可在事务内调用；append 失败会返回 Err
pub async fn record_event_in_txn<C: ConnectionTrait>(
    conn: &C, repo_id: i64, event: &str, payload: &Value,
) -> Result<()>;

// 只投递，不写日志；提交后调用
pub async fn deliver_event(
    db: &DatabaseConnection, repo_id: i64, event: &str, payload: &Value,
) -> Result<()>;
```

内部把原来的 `dispatch` 一分为二：

```
record_envelope(conn, ..)   —— 仅写日志；best-effort（失败只 warn），保持旧行为
append_envelope(conn, ..)   —— 写日志本体（截断 >16KB、event 名截断 64），返回 Err
deliver_envelope(db, ..)    —— 仅投递：查 active hooks + 逐个 spawn（原 dispatch 后半段，原样搬移）
dispatch(db, ..)            —— record_envelope + deliver_envelope（`redeliver` 继续用它，语义不变）
```

`deliver_event` 与 `append_envelope` 的职责互斥，这是「不重复写」的结构保证：投递路径里没有
`repo_event_ops::append` 调用。

### 3.3 `trigger_event` 等价性论证

旧实现：

```
event_actor(db) → EventEnvelope::new → dispatch →
    body = to_string(envelope)
    record_envelope（内部再 to_string 一次）→ append（best effort）
    list_active_by_repo_and_event → for hook { spawn }
```

新实现：

```
event_actor(db) → EventEnvelope::new → to_value
record_envelope(&envelope)：append_envelope（to_string + append，best effort）
deliver_envelope(envelope)：body = to_string → list_active_by_repo_and_event → for hook { spawn }
```

逐点对照：

| 维度 | 旧 | 新 |
| --- | --- | --- |
| 信封 id | 一个信封同时写日志和投递 | 同一个 `envelope` 值，同样一次构建 → **id 相同**（未破坏 `webhook_event_log_holds_the_same_envelope_that_was_delivered`） |
| 日志写失败 | 只 warn，不影响返回值/投递 | `record_envelope` 只 warn，继续投递（不变） |
| hook 查询失败 | `?` 返回 Err | `deliver_envelope` 里 `?` 返回 Err（不变） |
| 日志写与 hook 查询顺序 | 先写日志再查 hook | 不变 |
| 投递内容/头/重试队列 | 见原 `dispatch` 循环 | 同一段代码原样搬进 `deliver_envelope` |

因此 13 个 `trigger_*` 命名糖（含 `trigger_release_created`、`trigger_pr_*` 等）零改动、语义零变化，
`webhook_envelope_tests.rs` 全部通过（其中 `webhook_event_log_holds_the_same_envelope_that_was_delivered`
直接断言日志行与投递体是同一个信封）。

### 3.4 `issue.opened` 走新路径

```
create_issue:
  begin
  insert issue（原逻辑）
  payload = { "event": "issue.opened", "issue": { id, repo_id, number, title, state, author_id } }
  record_event_in_txn(&txn, repo_id, "issue.opened", &payload)   // ← 新增，事务内
  commit
  deliver_event(&db, repo_id, "issue.opened", &payload)          // ← 原 trigger_issue_opened 的位置
  ...（label dual-write 等其余逻辑原样在提交后）
```

payload 构造从「先建内层对象，再交给 `trigger_issue_opened` 包一层 `{event, issue}`」变成「直接写出
最终形状」，**JSON 内容逐字节等价**（同一组键、同一组值；`serde_json` 默认按 BTreeMap 排序，键序也一致）。
`create_issue` 里没有第二处 trigger；label dual-write 不是事件，保持提交后执行。

## 4. 时序变化与已知偏差

1. **写入时机提前**：`issue.opened` 的日志行从「commit 之后」挪到「commit 之前、同一事务内」。这是本任务
   唯一想要的行为变化。
2. **日志写失败从 best-effort 变为致命**（仅 `issue.opened`）：`record_event_in_txn` 返回 `Err`，
   `create_issue` 用 `?` 上抛，事务随之回滚。理由：既然事件已和业务同事务，写不进去就不该提交业务变更
   ——这正是 outbox 的语义；旧的 best-effort 会让「有 issue、没事件」重新出现。其他 12 条 `trigger_*`
   仍是最佳努力，未变。
3. **信封 id 偏差（本步的已知限制）**：`record_event_in_txn` 与 `deliver_event` 各自构建信封，因此
   `issue.opened` 的日志行信封 id 与投递体信封 id **不同**。`trigger_*` 路径不受影响（仍共享一个信封）。
   `deliver_event` 的签名只拿到 payload，无法复用事务里那个信封；要恢复「日志与投递同一 id」需要把
   「构建信封」上提给调用方或由 outbox 从表里取信封投递——那是 #15 的下一步。**业务 payload 完全一致**，
   测试断言了 `delivered["data"] == logged["data"]`。
4. **actor 解析**：`record_event_in_txn` 用事务连接解析 actor，避免读到与业务写不一致的快照。
   `crate::agent::resolve_actors` 绑定 `&DatabaseConnection`，放宽它要动 `agent.rs` 与
   `rg-db::ops::user_ops`（不在本任务 owned paths），因此 `webhook/service.rs` 内新增
   `resolve_actor<C: ConnectionTrait>`，规则与 `resolve_actors` 一致（`users` 行 + agent 的 owner 名），
   并让 `event_actor` 委托它，保证本服务只有一份规则。

## 5. 验证策略

新增 `crates/rg-core/tests/issue_event_txn_tests.rs`（独立测试二进制，沿用 loopback receiver 模式）：

1. `an_event_in_a_rolled_back_transaction_is_neither_logged_nor_delivered`
   - 事务内插入 issue → `record_event_in_txn` → 断言事件**在事务视图里已存在**（证明写入真的发生在事务内，
     而不是提交后补写）→ 注入一条必然失败的业务写（`repo_events.repo_id NOT NULL` 被违反）→ 丢弃事务
     （回滚）→ 断言日志里没有 `issue.opened`、issue 行也不存在、receiver 500ms 内收不到请求。
   - **改前是 red**：旧代码没有 `record_event_in_txn`，测试无法编译（证据见 task 文档 §3）。
2. `a_committed_issue_logs_and_delivers_issue_opened_exactly_once`
   - `create_issue` → 日志恰好 1 行、`data.issue.id/title` 正确、`actor.id` 正确（事务内解析）；
     receiver 收到 kind/头正确、`data` 与日志一致；投递记录恰好 1 条；`latest_id` 等于该行 id；
     再建一个 issue → 2 行且 id 单调递增（cursor 跳一格）。
3. `trigger_event_still_writes_one_row_and_delivers_the_same_envelope`
   - `trigger_event` 拆分后的回归护栏：一行日志，且日志信封 == 投递信封。
4. 兼容性：`cargo test -p rg-core` 全绿，其中 `webhook_envelope_tests.rs` 覆盖 12 个 `trigger_*` 路径的
   既有断言（信封、重试、重投、quiet 路径）。

## 6. 遗留风险

1. 投递仍是「提交后立即 spawn」：进程在 commit 与 spawn 之间崩溃，事件在日志里但没人收到（至少一次投递
   要等 outbox）。本任务把窗口从「事件丢失」缩到「事件未投递」，符合 #15 分步计划。
2. `issue.opened` 的日志/投递信封 id 不同（§4.3）。消费者若跨日志与 webhook 去重会看到两个 id；
   下一步 outbox 由 stored envelope 投递时恢复。
3. `resolve_actor` 是 `agent::resolve_actors` 规则的一份实现（§4.4）。已让 `event_actor` 委托它以避免
   「同一事件两种 actor」；若将来 `resolve_actors` 的规则变化，这两处要一起改（建议后续把
   `resolve_actors` 放宽到 `ConnectionTrait` 并删掉本文件里的实现）。
4. `trigger_issue_opened` 保留但已无调用点（`trigger_*` 命名糖「零改动」）。它仍是「提交后写事件」的旧
   形状，后续若有人重新调用会退回旧缺陷；可在 outbox 落地时把它改为 `record_event_in_txn + deliver_event`
   的组合或删除。
5. 事务内写事件会让 SQLite 写事务稍长（多一条 insert + 两读 actor），未做性能测量；影响是单条 insert 级。
