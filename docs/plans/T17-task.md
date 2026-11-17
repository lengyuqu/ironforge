# T17 Task 确认 — [#15 步骤5] `release.created` 走事务内事件写入（原本无事务的路径）

> 权威状态在 `tasks/T17.md`；本文件记录约束确认、触碰路径、验证证据、验收命令真实输出与遗留风险。
> payload 等价性论证、`trigger_release_created` 删除理由、语义变更说明见 `docs/plans/T17-plan.md`。

## 1. 约束确认

| 约束 | 结论 |
| --- | --- |
| `release_ops::create` 放宽为 `<C: ConnectionTrait>` | 是；`find_by_repo_and_tag` 未被事务使用，**保持原样**（编译不要求放宽） |
| 校验 + 查重留在事务外 | 是；位置与顺序未变 |
| 事务内 `release_ops::create(&txn)` + `record_event_in_txn(&txn)`，失败 `?` 回滚 | 是 |
| commit 后 `deliver_event`，失败只 warn | 是；`tracing::warn!("Failed to deliver release.created webhook: {e}")` |
| payload 八字段逐字节等价、顺序不变，外层 `{event, release}` | 是；同一段字面量，测试用 `len() == 8` 钉住 |
| `trigger_release_created` 无调用点则删除 + 原位注释 | 是；测试调用点改为直接构造 payload 走 `trigger_event` |
| 正常创建：日志恰 1 行、receiver 收 1 次、payload 与 DB 行一致 | 是，见 §2 测试 1 |
| 「不重复写」做变异测试 | 是，见 §3 |
| 回滚不写事件：失败点在事务前则如实说明 | 是，见 §2 测试 3 |
| 既有 release 测试全绿 | 是，见 §4（含 `rg-http` release_tests 6 条） |
| 不改 `Cargo.*`、迁移、`rg-http/src/`、`CLAUDE.md`、`ironforge-docs/`、`.cnb.yml`、`.github/`、`scripts/`、`deploy/`、`rg-mcp/`、`issue/service.rs` | 是，全部未触碰 |
| 不顺手做 `release.deleted` / `issue.closed` / outbox | 是 |

## 2. 触碰路径与测试

`crates/rg-db`

- `src/ops/release_ops.rs`：`create` 放宽为 `create<C: ConnectionTrait>`（唯一调用点就是 `create_release`）。
  其它函数未动。

`crates/rg-core`

- `src/release/service.rs`：`create_release` 增加事务；payload 构造与旧代码逐字一致，外层信封在事务内构造并
  `record_event_in_txn`；commit 后 `deliver_event`。
- `src/webhook/service.rs`：删除 `trigger_release_created`，原位留说明注释。
- `tests/release_event_txn_tests.rs`（新增）：2 个集成测试。
- `tests/webhook_envelope_tests.rs`：`webhook_release_event_carries_the_envelope_and_the_old_fields_in_data`
  不再调用已删除的助手，改为构造同一份 payload 走 `trigger_event`（断言原样保留）。

文档：`docs/plans/T17-plan.md`、`docs/plans/T17-task.md`、`tasks/T17.md`（State → completed）。

### 测试 1 `a_created_release_logs_and_delivers_release_created_exactly_once`

真实 SQLite + 真实 `create_release`（`v1.0.0` / `First Release` / `body=Some` / `is_prerelease=true`）：

- `repo_events` 里 `release.created` **恰 1 行**（失败消息打印整份内容）；
- `data.event == "release.created"`；
- `data.release.{id,repo_id,tag_name,title,body,is_draft,is_prerelease,author_id}` 与 `create_release`
  返回值逐字段一致；`data.release` 键数 `== 8`；
- receiver 收到 1 次，`X-IronForge-Event == release.created`，投递体 `data == 日志 data`；
- `webhook_deliveries` 恰 1 行；随后 500ms 内无第二次请求。

### 测试 2 `a_refused_release_writes_no_event`

先建 `v1.0.0`，再建同 tag → `CoreError::Conflict`；`repo_events` 仍是 1 行（没有多出事件）。
覆盖「创建被拒绝就不该有事件」的可达失败路径。

### 测试 3 说明：回滚不写事件（未新增用例，如实说明）

本路径的失败点（空 `tag_name` / 空 `title` 校验、`find_by_repo_and_tag` 查重）**全部在事务开始之前**；
事务内两步（`release_ops::create`、`record_event_in_txn`）没有可从外部触发的失败注入点，除非改生产代码或
伪造场景——按任务要求不这么做。

该性质由 **T13 的机制测试**覆盖：
`issue_event_txn_tests::an_event_in_a_rolled_back_transaction_is_neither_logged_nor_delivered`
消费的是**同一个** `record_event_in_txn`，并在事务内注入必败写后丢弃事务，断言日志无行、业务行无行、
receiver 无请求。事务回滚语义与调用方无关。

## 3. 「不重复写」的活证据（变异检查）

只断言 `len() == 1` 还不足以证明断言真的在看着 `deliver_event`。因此做了一次变异：临时把 `deliver_event`
改成也调用 `append_envelope`（模拟「照抄 trigger_event」的错误迁移），
`cargo test -p rg-core --test release_event_txn_tests` 立刻变红：

```
---- a_created_release_logs_and_delivers_release_created_exactly_once stdout ----
thread '...' panicked at crates/rg-core/tests/release_event_txn_tests.rs:311:5:
assertion `left == right` failed: one release.created row, not two: [
  Object {"actor": {...}, "at": "2026-11-17T11:22:27.419733Z", "data": {...}, "id": "537735d7-...", "kind": "release.created", ...},
  Object {"actor": {...}, "at": "2026-11-17T11:22:27.420183Z", "data": {...}, "id": "02aef73b-...", "kind": "release.created", ...}
]
  left: 2
 right: 1

test result: FAILED. 0 passed; 2 failed
```

随后手工撤销该变异（不是 `git checkout --`，避免连带回滚其它改动），`git diff` 复核 `deliver_event`
恢复原样（不含任何 `append_envelope`），重跑测试恢复 2 passed。

变异期间测试 2 也红了（它先断言第一次创建后有 1 行，变异下变成 2 行）——这同样证明断言在看真实行数。

## 4. 验收命令真实输出

```
$ cargo test -p rg-core
test result: ok. 377 passed; 0 failed; 0 ignored                     (unittests)
test result: ok. 1 passed; 0 failed; 0 ignored                       (collab_search_tests)
test result: ok. 3 passed; 0 failed; 0 ignored                       (issue_event_txn_tests)
test result: ok. 0 passed; 0 failed; 2 ignored                       (multi_backend_smoke)
test result: ok. 2 passed; 0 failed; 0 ignored                       (pr_merge_event_txn_tests)
test result: ok. 2 passed; 0 failed; 0 ignored                       (release_event_txn_tests)   ← 新增
test result: ok. 12 passed; 0 failed; 0 ignored                      (webhook_envelope_tests)
test result: ok. 1 passed; 0 failed; 5 ignored                       (Doc-tests)
EXIT=0

$ cargo test -p rg-db
test result: ok. 59 passed; 0 failed; 0 ignored
test result: ok. 5 passed; 0 failed; 0 ignored
test result: ok. 12 passed; 0 failed; 0 ignored
test result: ok. 5 passed; 0 failed; 0 ignored
test result: ok. 0 passed; 0 failed; 1 ignored
test result: ok. 15 passed; 0 failed; 0 ignored
test result: ok. 1 passed; 0 failed; 0 ignored
EXIT=0

$ cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 54.28s
EXIT=0

$ scripts/check.sh --quick
==> cargo fmt --all --check
    (2s)
==> cargo clippy --workspace --all-targets -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.49s
    (0s)
==> checks passed in 2s (quick)
EXIT=0

$ cargo check -p rg-http --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 42.25s
EXIT=0

$ cargo test -p rg-http --test release_tests
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
EXIT=0
```

首次 `scripts/check.sh --quick` 因新测试文件未格式化（`cargo fmt --check` diff）而失败，
执行 `cargo fmt --all` 后重跑通过（上表为修复后的输出）。

未跑 `cargo test -p rg-http` 全量（按任务要求）；`create_release` 签名未变，唯一 `rg-http` 调用点
（`src/api/releases.rs`）无需改动。

## 5. 遗留风险

1. 投递仍是「commit 后立即 spawn」：commit 与 spawn 之间崩溃 = 「有事件、无投递」。至少一次投递要等 outbox。
2. 日志信封与投递信封 `id` 不同（各自建信封）。业务 payload 一致（测试有断言）；outbox 从存表信封投递时恢复。
3. **语义变更**：事件写失败现在会回滚整条 release 创建（旧的是 best-effort）。这是本任务有意的取舍，
   影响面已核对：`rg-http` 单条创建返回 500；`import` 两个循环本来就逐条 `warn` 继续。
4. SQLite 单写者：本路径是短事务，实测无锁等待/死锁；高并发创建会被写锁串行化，属既有特性。
5. `trigger_issue_opened` 等仍是「提交后写事件」的旧形状（T13/T16 遗留），留给后续清理任务。
