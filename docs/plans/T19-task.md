# T19 Task 确认 — [#15 步骤7] outbox 最小版本：投递状态 + 兜底补投

> 权威状态在 `tasks/T19.md`；本文件记录约束确认、触碰路径、验证证据、验收命令真实输出与遗留风险。
> 方案取舍（为什么 `dispatched_at` 而不是 `deliveries.event_id`、窗口边界、回填口径、id 透传方式）
> 见 `docs/plans/T19-plan.md`。

## 1. 约束确认

| 约束 | 结论 |
| --- | --- |
| 迁移 `repo_events.dispatched_at TIMESTAMP NULL` | 是，`m20261117_000001_add_repo_events_dispatched_at` |
| 迁移步数 `Migrator::down` 30 → 31 | 是，`crates/rg-db/src/lib.rs` 的 window 测试同步改 31，注释里的 104 → 105 |
| **历史行回填为 `created_at`** | 是；且**只在加列那一趟**回填（重跑不覆盖真正未投递的行），有单测 |
| `append` 插入 `dispatched_at` 为 NULL | 是（`append_handled(.., None)`） |
| 投递前（spawn 之前）标记 | 是，`deliver_envelope` 在 `list_active_by_repo_and_event` 之后、循环 spawn 之前 |
| 需要 event id 透传 | 是；`append_envelope` → `record_envelope -> Option<i64>` → `deliver_envelope(.., event_id)` |
| 兜底 `sweep(db)` 扫 `[now-15min, now-2min]` 且 NULL | 是，`webhook/outbox.rs` |
| `SweepReport { scanned, redelivered, failed }` | 是 |
| `failed` 不标记（下轮再试） | 是；只有「payload 不可重放」才标记，减少永久卡住 |
| payload 解析失败 warn + 标记 | 是（坏 JSON 与截断 stub 都标记） |
| 60s 周期，失败只 warn | 是，`run_sweep_loop`；`supervise("webhook.outbox_sweep", ..)` |
| 配置 `webhook_outbox_sweep_secs` 默认 60 / 0 关闭 | 本仓无配置系统，按 `IRONFORGE_GIT_STREAM_TIMEOUT_SECS` 风格用 `IRONFORGE_WEBHOOK_OUTBOX_SWEEP_SECS` |
| 不做状态机 / 幂等键 / 顺序保证 | 是，均未引入 |
| 不把 `trigger_event` 改成「只写不投」 | 是，`trigger_event` 仍是「记录 + 立即投递」 |
| 不碰 `pull_request/` `issue/` `release/` 业务代码 | 是，一行未动（`git diff` 可验） |
| `rg-http/src/lib.rs` 只加 worker 注册 | 是，只加一个 `tokio::spawn(supervise(..))` 块 |
| 不改 `Cargo.*`、`CLAUDE.md`、`ironforge-docs/`、`.cnb.yml`、`.github/`、`scripts/`、`deploy/`、`rg-mcp/` | 是 |

## 2. 触碰路径

`crates/rg-db`

- `src/migrations/m20261117_000001_add_repo_events_dispatched_at.rs`（新建）：加列 + 回填 + 复合索引；
  `down` 删索引再删列；3 条单测（三方言 DDL、回填与重跑、down/up 往返）。
- `src/migrations/mod.rs`：注册迁移（末尾）。
- `src/lib.rs`：仅迁移回滚窗口测试的步数 30 → 31 与注释。
- `src/entities/repo_event.rs`：`dispatched_at: Option<DateTimeUtc>` + 语义文档。
- `src/ops/repo_event_ops.rs`：`append_handled`、`mark_dispatched`、`list_undispatched_between`、
  `list_undispatched_of_type`；`append` 改为委托 `append_handled(.., None)`。

`crates/rg-core`

- `src/webhook/service.rs`：
  - `append_envelope` 返回 `Result<i64>` 并多一个 `dispatched: bool`；
  - `record_envelope` 返回 `Option<i64>`（日志写仍是 best-effort）；
  - `deliver_envelope` 增加 `event_id: Option<i64>`，在 spawn 前 `mark_dispatched`；
  - `trigger_event` / `dispatch`（redeliver）把 id 传下去；
  - `record_event`（静默路径）写入即标记；
  - `deliver_event` 新增 `find_delivery_row`，按 payload 认领事务写入的行。
- `src/webhook/outbox.rs`（新建）：`sweep` / `SweepReport` / `run_sweep_loop` / 窗口与间隔常量。
- `src/webhook/mod.rs`：导出 `outbox`。
- `tests/outbox_sweep_tests.rs`（新建）：8 条集成测试。

`crates/rg-http`

- `src/lib.rs`：仅 `run()` 里新增 `webhook.outbox_sweep` 的 `supervise` 注册块。

文档：`docs/plans/T19-plan.md`、`docs/plans/T19-task.md`、`tasks/T19.md`（State → completed）。

## 3. 实现要点与自查

### 3.1 静默路径必须写非 NULL（本任务额外发现的洞）

`trigger_event_quietly` / `record_event`（T11，agent 草稿区）故意「只写事件、不投递」。若它们写 `NULL`，
sweep 会把草稿 push 投给所有 hook，直接推翻 T11 的静默保证。因此这两条路径**写入时**就写
`dispatched_at = now()`：列语义是「投递决策已终局」，不是「投递成功」。
`a_quiet_event_is_never_swept` 钉住。

### 3.2 事务路径的 id 不可透传（越界处理）

`record_event_in_txn` 丢弃 id，`deliver_event` 只拿得到 `(db, repo_id, event, payload)`，而它的调用点在
`pull_request/` `issue/` `release/` —— 禁止触碰。若不处理，这 5 条路径的每一行都会永远 `NULL`，
2 分钟后被 sweep **全量重投**（比原来更糟）。

处理：`deliver_event` 在提交后 `find_delivery_row` 按内容认领（repo + type + NULL + 最近 15 分钟 +
存储信封的 `data` 与本次 payload 完全相等）。认错的唯一场景是「同一仓库并发两个**完全相同** payload 的
同类事务事件」，此时认领任意一行等价，另一行留给 sweep —— 最坏是一次重复，不会丢事件。
`a_transaction_event_is_marked_after_it_is_delivered` 钉住。

### 3.3 标记时机与失败处理

- 在 `list_active_by_repo_and_event` **之后**：hook 查询失败时不留「已交付」假状态，行保持 `NULL` 交给 sweep。
- 在 spawn **之前**：spawn 无完成回调；之后标记会把「投递成功但标记失败」变成重复投递。
- 标记失败只 `warn`，不中止投递 —— 投递比状态重要，最多让 sweep 再投一次。

### 3.4 不可重放的 payload 也要标记

坏 JSON → warn + 标记；合法 JSON 但不是信封（含 `append_envelope` 的超长截断 stub）→ warn + 标记。
否则它每分钟被重读一次，永远卡在窗口里。`an_unparseable_payload_is_settled_instead_of_looping` 钉住。

### 3.5 `0` 关闭时的生命周期

`run_sweep_loop` 在 `interval == 0` 时用 `std::future::pending()` 挂起而不是 return：该任务被
`supervise` 托管，return 会被当成异常并按秒重启刷错误日志。

## 4. 验证（六条要求 → 测试）

| 要求 | 测试 |
| --- | --- |
| 1. 历史行不被补发 | `a_historical_undispatched_row_is_never_replayed` + 迁移单测 `existing_rows_are_backfilled_with_their_created_at` |
| 2. 漏网被补投 1 次 + 标记 | `a_missed_event_inside_the_window_is_redelivered_once` |
| 3. 正常路径不重复 | `a_normally_triggered_event_is_not_repeated_by_the_sweep`（把行老化到窗口内，仍 0 次） |
| 4. 窗口边界 | `the_window_edges_are_excluded`（`now-10s` / `now-20min` 不扫；中间带能扫到，反证边界有效） |
| 5. 坏 payload 标记 + warn 不卡循环 | `an_unparseable_payload_is_settled_instead_of_looping` |
| 6. worker 幂等 | `a_missed_event_inside_the_window_is_redelivered_once` 的第二轮 |

额外：`a_quiet_event_is_never_swept`（T11 静默）、`a_transaction_event_is_marked_after_it_is_delivered`（5 条已迁移路径）、
`the_sweep_never_duplicates_a_delivery_it_did_not_make`（delivery 行数不变）。

## 5. 验收命令真实输出

最终源码（`cargo fmt` 之后）在本 worktree 实际运行，以下为原样摘要。

### 5.1 `cargo test -p rg-db`

```
     Running unittests src/lib.rs
test result: ok. 62 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 13.99s
     Running tests/authorization_store.rs
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.51s
     Running tests/ci_integer_compat.rs
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.47s
     Running tests/collab_groups_store.rs
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.59s
     Running tests/collab_migration.rs
test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/collab_store.rs
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.72s
   Doc-tests rg_db
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.10s
```

其中 `unittests` 内含迁移回滚窗口测试（`down(&db, Some(31))` 后重新 `up`）与
`every_entity_table_exists_after_fresh_migration`，均通过。

新迁移的 3 条单测：

```
$ cargo test -p rg-db --lib m20261117 -- --nocapture
running 3 tests
test migrations::m20261117_000001_add_repo_events_dispatched_at::tests::the_column_and_index_use_the_plural_table_name ... ok
test migrations::m20261117_000001_add_repo_events_dispatched_at::tests::existing_rows_are_backfilled_with_their_created_at ... ok
test migrations::m20261117_000001_add_repo_events_dispatched_at::tests::down_drops_the_column_and_up_restores_it ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 59 filtered out; finished in 0.36s
```

### 5.2 `cargo test -p rg-core`

```
     Running unittests src/lib.rs
test result: ok. 377 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 13.86s
     Running tests/collab_search_tests.rs
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running tests/issue_event_txn_tests.rs
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.71s
     Running tests/multi_backend_smoke.rs
test result: ok. 0 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/outbox_sweep_tests.rs
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.07s
     Running tests/pr_merge_event_txn_tests.rs
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.90s
     Running tests/pr_open_close_event_txn_tests.rs
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.72s
     Running tests/release_event_txn_tests.rs
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.70s
     Running tests/webhook_envelope_tests.rs
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.92s
   Doc-tests rg_core
test result: ok. 1 passed; 0 failed; 5 ignored; 0 measured; 0 filtered out; finished in 0.09s
```

新增的 8 条 sweep 测试：

```
running 8 tests
test a_historical_undispatched_row_is_never_replayed ... ok
test a_quiet_event_is_never_swept ... ok
test an_unparseable_payload_is_settled_instead_of_looping ... ok
test a_transaction_event_is_marked_after_it_is_delivered ... ok
test a_normally_triggered_event_is_not_repeated_by_the_sweep ... ok
test the_window_edges_are_excluded ... ok
test a_missed_event_inside_the_window_is_redelivered_once ... ok
test the_sweep_never_duplicates_a_delivery_it_did_not_make ... ok

test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.07s
```

### 5.3 `cargo test -p rg-http`（本轮按要求跑全量）

```
$ cargo test -p rg-http
exit=0
suites: 134   passed: 867   failed: 0   ignored: 3
```

（134 个测试目标全部 `test result: ok`，`grep -c FAILED` 为 `0`；耗时约 9 分钟。）

### 5.4 `cargo clippy --workspace --all-targets -- -D warnings`

```
    Checking rg-db v0.2.0
    Checking rg-core v0.2.0
    Checking rg-http v0.2.0
    Checking rg-ssh v0.2.0
    Checking rg-ci v0.2.0
    Checking rg-cli v0.2.0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 9.90s
```

`cargo fmt --all --check` 无输出（干净）。

### 5.5 `scripts/check.sh --quick`

```
==> cargo fmt --all --check
    (1s)

==> cargo clippy --workspace --all-targets -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 14.95s
    (15s)

==> checks passed in 16s (quick)
```

## 5bis. 历史行回填的验证证据（本任务死线）

迁移单测 `existing_rows_are_backfilled_with_their_created_at` 直接复现「部署前的库」：

1. `Migrator::up(&db, Some(104))` —— 停在**本迁移之前**（表在、列不在）；
2. 用裸 SQL 插入一条历史事件（不带 `dispatched_at`），`created_at = '2026-01-01 00:00:00 +00:00'`；
3. `Migrator::up(&db, None)` —— 应用本迁移；
4. 断言 `SELECT COUNT(*) FROM repo_events WHERE dispatched_at IS NULL` **= 0**；
5. 断言 `CAST(created_at AS TEXT) == CAST(dispatched_at AS TEXT)`（回填值就是行自己的 `created_at`，
   不是 `now()`）；
6. 再插一条「列已存在之后」写下的行（`dispatched_at` 为 NULL），直接重跑 `Migration.up`：
   断言 `NULL` 计数 **= 1**（重跑**不**回填，真正漏投的行不会被吞掉）。

行为层的对应证据是 `a_historical_undispatched_row_is_never_replayed`：即使库里有 `NULL` + 一天前的行，
`sweep` 返回 `scanned == 0 / redelivered == 0`，receiver 500ms 内无请求，该行的 `dispatched_at` 仍为 NULL
（sweep 连碰都不碰）。

## 6. 遗留风险

1. 仍然是**至少一次**投递：`mark` 与 spawn 之间的崩溃会被 sweep 补投，天然不保证去重；任务明确不做幂等键。
2. 事务路径按 payload 认领（§3.2），并发**完全相同**的同类事件可能让其中一条被 sweep 再投一次；
   不影响正确性，但会重复。
3. `branch.*` / `tag.*` 四类事件仍然没有事务保护（写入与业务动作之间仍可能丢），本任务只让
   **已落盘**的事件能被补投。
4. 超过 15 分钟仍未投递的事件不再自动补投（任务口径：人工排查更安全）；`scanned` 也不统计它们，
   运维需要另行查询 `dispatched_at IS NULL AND created_at < now()-15min` 才能发现。
5. 超长事件（> 16KB）被存为截断 stub，永远无法重放；sweep 会 warn + 标记，事件内容本身需要从对象侧补齐。
6. `trigger_event` 的日志写失败会让该次投递**无法被标记**（拿不到 id）：投递照常发生，但 sweep 可能在
   2 分钟后重投一次。这是 best-effort 日志的既有语义，未改变。
