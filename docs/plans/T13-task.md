# T13 Task 确认 — [#15 步骤1] 事件写入可在事务内

> 权威状态在 `tasks/T13.md`；本文件记录约束确认、触碰路径、red→green 证据、验收命令真实输出与遗留风险。
> 签名放宽方案、`trigger_event` 等价性论证、payload/时序变化与「为什么不需要 outbox 表」见
> `docs/plans/T13-plan.md`。

## 1. 约束确认

| 约束 | 结论 |
| --- | --- |
| 只放宽一条链的签名，不做全仓 498 个 | 是；只动 `append` / `record_envelope` / 原 `record_repo_event` |
| `append` → `pub async fn append<C: ConnectionTrait>(conn: &C, ..)` | 是，`crates/rg-db/src/ops/repo_event_ops.rs:12` |
| `latest_id` 保持原样 | 是，未改 |
| `record_repo_event` 一起放宽，其他调用点只保证能编译、不改语义 | 是；它唯一调用点就是 `record_envelope`，已并入 `append_envelope<C>` |
| 新增 `record_event_in_txn<C: ConnectionTrait>(conn, repo_id, event, payload) -> Result<()>` | 是；只写日志，append 失败返回 `Err` |
| 新增 `deliver_event(db, repo_id, event, payload) -> Result<()>`，不写日志 | 是；内部只有 hook 查询 + spawn，结构上没有 `append` 调用 |
| `trigger_event` = 日志 + 投递，签名不变、行为等价 | 是；见 plan §3.3 逐点对照，`trigger_*` 13 个命名糖零改动 |
| `create_issue` 的 `issue.opened` 移到 commit 前，payload 一模一样 | 是；`{event, issue:{id,repo_id,number,title,state,author_id}}` 逐字节等价，只改写入时机 |
| 同一处有多条 trigger 只移 `issue.opened` | 是；`create_issue` 无第二处 trigger，label dual-write 保持提交后 |
| actor 解析用事务连接 | 是；`event_actor_on(&txn, ..)`（plan §4.4 说明为何没直接放宽 `agent::resolve_actors`） |
| 回滚不写事件的测试必须先 red 再 green | 是，见 §3 |
| 提交则写事件 / 不重复写 / 其他 12 条路径兼容 | 是，见 §2 测试 2、3 与 §4 |
| 不加迁移、不加 outbox 表 | 是，`crates/rg-db/src/migrations/` 未触碰 |
| 不改 `Cargo.toml`/`Cargo.lock`、`rg-http/src`、`CLAUDE.md`、`ironforge-docs/`、`.cnb.yml`、`.github/`、`scripts/`、`deploy/`、`rg-mcp/`、`pull_request/service.rs` | 是，全部未触碰 |
| 不顺手做全仓签名收敛、重试策略、SSE、`scope_expand` | 是 |

## 2. 触碰路径

`crates/rg-db`

- `src/ops/repo_event_ops.rs`：`append` 放宽为 `C: ConnectionTrait`（`latest_id` 及其余读函数未动）。

`crates/rg-core`

- `src/webhook/service.rs`：
  - `event_actor` 委托新的 `event_actor_on<C: ConnectionTrait>`，新增 `resolve_actor<C>`（同一规则）；
  - `trigger_event` 改为「`record_envelope` + `deliver_envelope`」，共享一个信封；
  - 新增 `record_event_in_txn`、`deliver_event`；
  - `record_envelope` 放宽为泛型；新增 `append_envelope<C>`（原 `record_repo_event` 的正文，改为返回 `Result`）；
  - `dispatch` 拆为 `record_envelope + deliver_envelope`（`redeliver` 仍用 `dispatch`，语义不变）。
- `src/issue/service.rs`：`create_issue` 中 `issue.opened` 的写入移到 commit 前、投递移到 commit 后；payload 不变。
- `tests/issue_event_txn_tests.rs`（新增）：3 个集成测试。

文档：`docs/plans/T13-plan.md`、`docs/plans/T13-task.md`、`tasks/T13.md`（State → completed）。

## 3. red → green 证据

### 3.1 red（改动前，基线 `b357bbe` 的工作树上只加了测试文件）

```
$ cargo test -p rg-core --test issue_event_txn_tests
   Compiling rg-core v0.2.0 (/Users/yuqu/vibeCodeing/ironforge-t13/crates/rg-core)
error[E0432]: unresolved import `rg_core::webhook::service::record_event_in_txn`
  --> crates/rg-core/tests/issue_event_txn_tests.rs:14:21
   |
14 |     create_webhook, record_event_in_txn, trigger_event, CreateWebhookRequest,
   |                     ^^^^^^^^^^^^^^^^^^^ no `record_event_in_txn` in `webhook::service`

For more information about this error, try `rustc --explain E0432`.
error: could not compile `rg-core` (test "issue_event_txn_tests") due to 1 previous error
```

red 的实质是「这个能力根本不存在」：旧代码只有接受 `&DatabaseConnection` 的事件写入函数，事务内无法写事件，
所以「回滚不写事件」这条性质既写不出来、也无法被违反。这与任务的前提（SeaORM 的 `DatabaseTransaction`
不 `Deref` 到 `DatabaseConnection`，已在 `sea-orm-1.1.20/src/database/transaction.rs:17` 核实）一致。

### 3.2 green（改动后）

```
$ cargo test -p rg-core --test issue_event_txn_tests
running 3 tests
test trigger_event_still_writes_one_row_and_delivers_the_same_envelope ... ok
test a_committed_issue_logs_and_delivers_issue_opened_exactly_once ... ok
test an_event_in_a_rolled_back_transaction_is_neither_logged_nor_delivered ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.70s
```

红线测试的关键断言：`record_event_in_txn` 之后**在事务连接上**已能读到该事件（证明写进的是事务，不是
提交后补写）；随后注入一条必失败的业务写（`INSERT INTO repo_events (repo_id, ...) VALUES (NULL, ..)`）
并丢弃事务 → `repo_events` 无 `issue.opened`、issue 行也不存在、receiver 500ms 内无请求。

## 4. 验收命令真实输出

```
$ cargo test -p rg-db
EXIT=0  7 个测试二进制全部 ok，passed=97 failed=0（含 doc-test）

$ cargo test -p rg-core
EXIT=0  6 个测试二进制全部 ok，passed=394 failed=0（含新增 3 个）

$ cargo test -p rg-http
EXIT=0  134 个测试二进制全部 ok，passed=867 failed=0

$ cargo clippy --workspace --all-targets -- -D warnings
EXIT=0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 47.46s

$ scripts/check.sh --quick
EXIT=0
==> cargo fmt --all --check
    (1s)
==> cargo clippy --workspace --all-targets -D warnings
    (9s)
==> checks passed in 10s (quick)
```

## 5. 遗留风险

1. 投递仍是「commit 后立即 spawn」：commit 与 spawn 之间崩溃仍是「有事件、无投递」。至少一次投递留给
   outbox（#15 下一步）；本任务只保证「不再出现有业务、无事件」。
2. `issue.opened` 的日志信封 id 与投递信封 id 不同（`deliver_event` 只拿到 payload，无法复用事务里的信封）。
   `trigger_*` 路径不受影响；outbox 从存表信封投递时恢复同一 id。业务 payload 一致，测试有断言。
3. `trigger_issue_opened` 已无调用点但保留（命名糖零改动），它仍是「提交后写事件」的旧形状——后续若被
   重新调用会退回旧缺陷，建议 outbox 步骤一并处理或删除。
4. `webhook/service.rs` 内的 `resolve_actor` 是 `agent::resolve_actors` 规则的一份实现（因为后者绑定
   `&DatabaseConnection`，放宽它不在本任务 owned paths）。`event_actor` 已委托同一实现，当前只有一份规则；
   长期建议放宽 `resolve_actors` + `user_ops::find_by_ids` 并删除本地实现。
5. 事务内多了一条 insert 与最多两次 actor 读，未做性能测量（单条 insert 量级）。
