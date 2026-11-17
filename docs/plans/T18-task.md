# T18 Task 确认 — [#15 步骤6] `pr.opened` 加事务 + `pr.closed` 修正事件顺序

> 权威状态在 `tasks/T18.md`；本文件记录约束确认、触碰路径、验证证据、验收命令真实输出与遗留风险。
> 顺序缺陷说明、payload 等价性论证、`trigger_pr_*` 删除理由见 `docs/plans/T18-plan.md`。

## 1. 约束确认

| 约束 | 结论 |
| --- | --- |
| `pull_request_ops::create` 放宽为 `<C: ConnectionTrait>` | 是；`update` 本来就是泛型，未动 |
| `create_pr` 前面的校验（分支解析、编号）留在事务外 | 是；`plan_pr` 调用位置未变 |
| `pr.opened`：事务内 `create` + `pr_event_ops::record` + `record_event_in_txn`，失败 `?` 回滚 | 是 |
| `pr.closed`：**事件必须移到 `update` 之后** | 是；旧调用点在状态变更块内（`update` 之前），已删除 |
| `was_open` 跨到事件块 | 是；提到 `if let Some(s)` 之前，语义等价（见 plan §2） |
| `pr_event_ops::record` 一并改 `&txn` | 是；两条（draft 变化、state 变化）都在事务内 |
| commit 后 `deliver_event`，失败只 warn | 是；`tracing::warn!("Failed to deliver PR closed/opened webhook: {e}")` |
| `state` 不是 `closed`（只改 title）时不写也不投递 | 是；`was_open && updated.state == "closed"` 才构造事件，测试 4 钉住 |
| payload 等价：opened 9 字段 / closed 5 字段，外层 `{event, pull_request}` | 是；同一段字面量，测试用 `len() == 9` / `len() == 5` 钉住 |
| `trigger_pr_*` 无调用点则删除 + 原位注释 | 是，两个都删；测试调用点改为直接构造 payload 走 `trigger_event` |
| `pr.closed` 顺序修正必须有能区分先后的测试 | 是，见 §2 测试 3（SQLite 触发器注入 update 失败）+ §3 变异 |
| 「不重复写」做变异测试 | 是，见 §4 |
| 不改 `milestone.closed` / `issue/service.rs` | 是，未触碰 |
| 不改 `Cargo.*`、迁移、`rg-http/src/`、`CLAUDE.md`、`ironforge-docs/`、`.cnb.yml`、`.github/`、`scripts/`、`deploy/`、`rg-mcp/` | 是，全部未触碰 |
| 不碰 `update_pr` 的 draft 校验 / `auto_merge_enabled` 重置 / `closed_at` 补写 / ActiveModel 显式 `Set` | 是，这几段一行未改 |
| 不顺手做 `issue.closed` / `branch.*` / `tag.*` / outbox | 是 |

## 2. 触碰路径与测试

`crates/rg-db`

- `src/ops/pull_request_ops.rs`：`create` 放宽为 `create<C: ConnectionTrait>`（唯一调用点就是 `create_pr`）。
  其它函数未动。

`crates/rg-core`

- `src/pull_request/service.rs`：
  - `create_pr`：`pull_request_ops::create(&txn, ..)` + `pr_event_ops::record(&txn, ..)` +
    `record_event_in_txn(&txn, ..)` 同一事务；`txn.commit()`；commit 后 `deliver_event`（失败只 warn）。
  - `update_pr`：状态变更块只改内存（`trigger_pr_closed` 调用删除）；`was_open` 提到块外；
    `update(&txn, active)` + 两条 `pr_event_ops::record(&txn, ..)` + 条件性的
    `pull_request.closed` `record_event_in_txn(&txn, ..)` 同一事务；commit 后 `deliver_event`（失败只 warn）。
- `src/webhook/service.rs`：删除 `trigger_pr_opened` / `trigger_pr_closed`，原位留说明注释。
- `tests/pr_open_close_event_txn_tests.rs`（新增）：4 个集成测试。
- `tests/webhook_envelope_tests.rs`：`webhook_pull_request_event_carries_the_envelope_and_the_old_fields_in_data`
  不再调用已删除的助手，改为构造同一份 payload 走 `trigger_event`（断言原样保留）。

文档：`docs/plans/T18-plan.md`、`docs/plans/T18-task.md`、`tasks/T18.md`（State → completed）。

### 测试 1 `a_created_pr_logs_and_delivers_pull_request_opened_exactly_once`

真实 SQLite + 真实 `create_pr`（`feature` → `main`，`body=Some`）：

- `repo_events` 里 `pull_request.opened` **恰 1 行**（失败消息打印整份内容）；
- `data.event == "pull_request.opened"`；
- `data.pull_request.{id,repo_id,number,title,state,head_branch,base_branch,head_repo_id,author_id}`
  与 `create_pr` 返回值一致；键数 `== 9`；
- receiver 收到 1 次，`X-IronForge-Event == pull_request.opened`，投递体 `data == 日志 data`；
- `webhook_deliveries` 恰 1 行；随后 500ms 内无第二次请求；
- `pr_events` 恰 1 行（timeline 与事件同事务）。

### 测试 2 `a_closed_pr_logs_and_delivers_pull_request_closed_exactly_once`

真实 `update_pr(state="closed")`：

- `pull_request.closed` 恰 1 行；`data.pull_request` 5 字段，键数 `== 5`；
- receiver 1 次，投递体 `data == 日志 data`；`webhook_deliveries` 恰 1 行；500ms 无第二次；
- 库里 `state == "closed"` 且 `closed_at.is_some()`——事件描述的是已提交的事实。

### 测试 3 `an_update_that_fails_publishes_no_pull_request_closed`（顺序修正的唯一证据）

失败注入方式：**不改生产代码、不 mock**，测试直接在测试库上装一条 SQLite 触发器：

```sql
CREATE TRIGGER refuse_pr_update BEFORE UPDATE ON pull_requests
BEGIN SELECT RAISE(ABORT, 'injected update failure'); END
```

然后调 `update_pr(state="closed")`：

- 断言返回 `Err`；
- `repo_events` 无 `pull_request.closed` 行；
- receiver 500ms 内无请求；
- `webhook_deliveries` 无行；
- PR 仍是 `open`、`closed_at` 仍为 `NULL`。

改前该测试必红：旧代码在 `update` 之前就 `trigger_pr_closed`（写日志 + 投递）。见 §3 的实测。
注入的触发器在断言前 `DROP TRIGGER`，不影响后续读取。

### 测试 4 `a_title_only_update_publishes_no_pull_request_closed`

只改 title：不写 `pull_request.closed`、receiver 500ms 内无请求、无 delivery 行、PR 仍在。

## 3. 顺序修正的活证据（变异检查 A：把事件挪回 `update` 之前）

临时在 `pr.updated_at = Utc::now();` 之后插回旧顺序的发布块（`trigger_event(db, .."pull_request.closed")` 在
`update` 之前），`cargo test -p rg-core --test pr_open_close_event_txn_tests an_update_that_fails` 立刻变红：

```
running 1 test
test an_update_that_fails_publishes_no_pull_request_closed ... FAILED

---- an_update_that_fails_publishes_no_pull_request_closed stdout ----
thread 'an_update_that_fails_publishes_no_pull_request_closed' panicked at crates/rg-core/tests/pr_open_close_event_txn_tests.rs:535:5:
an update that failed writes no pull_request.closed event

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.22s
```

撤销变异（用 Edit 删除该块，**未用 `git checkout --`**）后 4 条测试全绿。

## 4. 「不重复写」的活证据（变异检查 B：`deliver_event` 也写日志）

临时把 `deliver_event` 改成也调用 `append_envelope`（模拟「照抄 trigger_event」的错误迁移），
`cargo test -p rg-core --test pr_open_close_event_txn_tests` 两条 exactly-once 测试立刻变红：

```
test a_closed_pr_logs_and_delivers_pull_request_closed_exactly_once ... FAILED
test a_created_pr_logs_and_delivers_pull_request_opened_exactly_once ... FAILED

thread 'a_closed_pr_...' panicked at .../pr_open_close_event_txn_tests.rs:436:5:
assertion `left == right` failed: one pull_request.closed row, not two: [ {... "id": "a0ac49f8-..."}, {... "id": "74c02bcb-..."} ]

thread 'a_created_pr_...' panicked at .../pr_open_close_event_txn_tests.rs:353:5:
assertion `left == right` failed: one pull_request.opened row, not two: [ {... "id": "27f5ced2-..."}, {... "id": "f455ab2e-..."} ]

test result: FAILED. 2 passed; 2 failed; ...
```

撤销（Edit）后恢复全绿；`git diff ... | grep -c MUTATION` 为 `0`。

## 5. 验收命令真实输出

```
$ cargo test -p rg-core
     Running unittests src/lib.rs (target/debug/deps/rg_core-7643ea5ceb901dca)
test result: ok. 377 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 13.86s
     Running tests/collab_search_tests.rs
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running tests/issue_event_txn_tests.rs
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.71s
     Running tests/multi_backend_smoke.rs
test result: ok. 0 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/pr_merge_event_txn_tests.rs
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.91s
     Running tests/pr_open_close_event_txn_tests.rs
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.73s
     Running tests/release_event_txn_tests.rs
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.71s
     Running tests/webhook_envelope_tests.rs
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.89s
   Doc-tests rg_core
test result: ok. 1 passed; 0 failed; 5 ignored; 0 measured; 0 filtered out; finished in 0.08s

$ cargo test -p rg-http --test pr_merge_strategy_tests --test merge_readiness_tests
     Running tests/merge_readiness_tests.rs
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.36s
     Running tests/pr_merge_strategy_tests.rs
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.92s

$ cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.37s

$ scripts/check.sh --quick
==> cargo fmt --all --check
    (2s)
==> cargo clippy --workspace --all-targets -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.29s
    (1s)
==> checks passed in 2s (quick)
```

额外跑过的 PR 相关集成测试（`create_pr` / `update_pr` 的真实调用方）：

```
$ cargo test -p rg-http --test pr_head_ref_tests --test pr_milestone_tests --test fork_pr_ref_tests --test pr_context_tests
fork_pr_ref_tests      : ok. 2 passed
pr_context_tests       : ok. 4 passed
pr_head_ref_tests      : ok. 2 passed
pr_milestone_tests     : ok. 2 passed
```

未跑 `cargo test -p rg-http` 全量（按任务要求，约 34 分钟）。

## 6. 遗留风险

1. 投递仍是「commit 后立即 spawn」：commit 与 spawn 之间崩溃 = 「有事件、无投递」。至少一次投递要等 outbox
   （与 T13/T16/T17 一致的已知缺口）。
2. 日志信封与投递信封的 `id` 不同（两次各自建信封）；业务 payload 一致（有断言）。
3. `update_pr` 事务略变长（`update` + ≤2 条 `pr_events` + ≤1 条 `repo_events`）。SQLite 单写者，单请求短事务，
   实测无锁等待；高并发下写锁串行化是既有特性。
4. `create_pr` 现在在事件写失败时整体回滚：PR 不创建、调用方收到错误（T13/T16/T17 同取舍）。
   影响面：`rg-http` 的 create PR 接口会在极端磁盘/DB 故障下返回 5xx 而不是「PR 建好但无事件」。
5. `trigger_issue_opened` / `trigger_issue_closed` / `trigger_issue_comment` / `trigger_milestone_closed` /
   `trigger_branch_*` / `trigger_tag_*` / `trigger_release_deleted` 仍是旧形状，留给后续任务。
6. `pr.closed` 只在 `was_open && updated.state == "closed"` 时发出；`merged` 走 T16 的 `pull_request.merged`，
   本任务未改。
