# T16 Task 确认 — [#15 步骤4] `pull_request.merged` 走事务内事件写入

> 权威状态在 `tasks/T16.md`；本文件记录约束确认、触碰路径、验证证据、验收命令真实输出与遗留风险。
> payload 等价性论证、`trigger_pr_merged` 删除理由、回滚场景说明见 `docs/plans/T16-plan.md`。

## 1. 约束确认

| 约束 | 结论 |
| --- | --- |
| merge 事务内新增 `record_event_in_txn`，失败用 `?` 回滚整 merge | 是；`pull_request/service.rs` `update_pr_merged` |
| commit 后 `deliver_event`，失败只 warn | 是；`tracing::warn!("Failed to deliver PR merged webhook: {e}")` |
| payload 逐字节等价，外层 `{event, pull_request}` 两键 | 是；内层字面量逐字复制，外层新增同形状；见 plan §3 |
| 不动 `pr_event_ops::record` 那一行 | 是 |
| `trigger_pr_merged` 保留或删除 | 删除（无其它调用点）；原位留注释解释原因，见 plan §4 |
| 正常 merge：日志恰 1 行、receiver 收 1 次、payload 与 merge 结果一致 | 是，见 §2 测试 1 |
| 「不重复写」明确断言 1 行 | 是；并用变异验证（§3） |
| 回滚不写事件：失败点在事务前则如实说明 | 是；见 §2 测试 3 说明与 plan §5 |
| 既有 merge 测试全绿 | 是，见 §4 |
| 不改 `pull_request/service.rs` 里 merge 之外的部分 | 是；只动 `update_pr_merged` 的事件写入 + 该函数 doc |
| 不碰合并算法、冲突处理、保护规则、`head_sha` 逻辑 | 是 |
| 不改 `Cargo.*`、迁移、`rg-db/src/ops/`、`rg-http/src/`、`CLAUDE.md`、`ironforge-docs/`、`.cnb.yml`、`.github/`、`scripts/`、`deploy/`、`rg-mcp/` | 是，全部未触碰 |
| 不顺手做其它 `trigger_*` 迁移、outbox、CI 写路径 | 是 |

## 2. 触碰路径与测试

`crates/rg-core`

- `src/pull_request/service.rs`：`update_pr_merged` —— payload 构造提前到事务内，新增
  `record_event_in_txn(&txn, ..)`；commit 后由 `trigger_pr_merged` 改为 `deliver_event`；函数 doc 同步。
  **未改**：`pull_request_ops::update`、`pr_event_ops::record`、`claim_merge` 流程、merge 算法与
  `head_sha` 相关逻辑。
- `src/webhook/service.rs`：删除 `trigger_pr_merged`（唯一调用点已改写），原位留说明注释。
- `tests/pr_merge_event_txn_tests.rs`（新增）：2 个集成测试。
- `tests/issue_event_txn_tests.rs`：仅去掉「twelve `trigger_*`」这个删除后不再成立的计数（措辞调整，无逻辑变化）。

文档：`docs/plans/T16-plan.md`、`docs/plans/T16-task.md`、`tasks/T16.md`（State → completed）。

### 测试 1 `a_merged_pr_logs_and_delivers_pull_request_merged_exactly_once`

真实 bare git 仓库（`main` + 分叉的 `feature`）+ 真实 `merge_pr(.., MergeStrategy::Merge)`：

- `repo_events` 里 `pull_request.merged` **恰 1 行**（`assert_eq!(events.len(), 1)`，失败消息打印整份内容）；
- `data.event == "pull_request.merged"`；`data.pull_request.{id,repo_id,number,title,merge_commit_sha,strategy}`
  与 DB 的 PR 行、`merge_pr` 返回值一致（`strategy == "merge"`）；
- receiver 收到 1 次，`X-IronForge-Event == pull_request.merged`，投递体 `data == 日志 data`；
- `webhook_deliveries` 恰 1 行；随后 500ms 内无第二次请求；
- `git rev-parse refs/heads/main == merge_commit_sha`（merge 真的发生）。

### 测试 2 `a_refused_merge_writes_no_event`

已 `closed` 的 PR 再 merge → 失败（`state_gate`），`repo_events` 里 `pull_request.merged` 为空。
它覆盖的是「merge 失败就不该有事件」的可达失败路径。

### 测试 3 说明：回滚不写事件（未新增用例，如实说明）

本路径的失败点（state / draft gate、`claim_merge`、`merge_claimed_pr` 里的分支存在性检查与 git merge）
**全部在事务开始之前**；事务内三步（`pull_request_ops::update`、`pr_event_ops::record`、
`record_event_in_txn`）没有可从外部触发的失败注入点，除非改生产代码或伪造场景——按任务要求不这么做。

该性质由 **T13 的机制测试**覆盖：
`issue_event_txn_tests::an_event_in_a_rolled_back_transaction_is_neither_logged_nor_delivered`
消费的是**同一个** `record_event_in_txn`，并在事务内注入必败写后丢弃事务，断言日志无行、issue 无行、
receiver 无请求。事务回滚语义与调用方无关，因此这条机制对 `pull_request.merged` 同样成立。

## 3. 「不重复写」的活证据（变异检查）

只断言 `len() == 1` 还不足以证明这条断言真的在看着 `deliver_event`。因此做了一次变异：

临时把 `deliver_event` 改成也调用 `append_envelope`（模拟「照抄 trigger_event」的错误迁移），
`cargo test -p rg-core --test pr_merge_event_txn_tests` 立刻变红：

```
test a_merged_pr_logs_and_delivers_pull_request_merged_exactly_once ... FAILED
thread '...' panicked at crates/rg-core/tests/pr_merge_event_txn_tests.rs:387:5:
assertion `left == right` failed: one pull_request.merged row, not two: [
  Object {"actor": Null, "at": "2026-11-17T10:57:23.080879Z", "data": {...}, "id": "91b654e9-...", "kind": "pull_request.merged", ...},
  Object {"actor": Null, "at": "2026-11-17T10:57:23.081166Z", "data": {...}, "id": "3711f0e8-...", "kind": "pull_request.merged", ...}
]
test result: FAILED. 1 passed; 1 failed
```

随即用 `git checkout --` 撤销变异（并重新应用 `trigger_pr_merged` 的删除——`checkout --` 会一并回滚未提交改动，
这一点已复核：`git diff` 中 `webhook/service.rs` 只剩预期的那段删除）。最终代码中
`deliver_event` 不含任何 `append_envelope` 调用。

顺带确认 payload 里的 `actor` 仍是 `Null`：旧 payload 顶层与 `pull_request` 容器内都没有 `author_id`，
新旧 actor 解析规则一致 → `None` 不变（这是「payload 逐字节等价」的一部分）。

## 4. 验收命令真实输出

```
$ cargo test -p rg-core
test result: ok. 377 passed; 0 failed; 0 ignored          (unittests)
test result: ok. 1 passed; 0 failed; 0 ignored            (collab_search_tests)
test result: ok. 3 passed; 0 failed; 0 ignored            (issue_event_txn_tests)
test result: ok. 0 passed; 0 failed; 2 ignored            (multi_backend_smoke)
test result: ok. 2 passed; 0 failed; 0 ignored            (pr_merge_event_txn_tests)   ← 新增
test result: ok. 12 passed; 0 failed; 0 ignored           (webhook_envelope_tests)
test result: ok. 1 passed; 0 failed; 5 ignored            (Doc-tests)
EXIT=0

$ cargo check -p rg-http --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 48.52s
EXIT=0

$ cargo test -p rg-http --test pr_merge_strategy_tests --test merge_pinned_head_tests \
      --test auto_merge_pinned_head_tests --test merge_target_tests
test result: ok. 2 passed; 0 failed    (auto_merge_pinned_head_tests)
test result: ok. 4 passed; 0 failed    (merge_pinned_head_tests)
test result: ok. 2 passed; 0 failed    (merge_target_tests)
test result: ok. 2 passed; 0 failed    (pr_merge_strategy_tests)
EXIT=0

$ cargo test -p rg-http --test merge_readiness_tests --test merge_queue_ci_tests --test collab_webhook_tests
test result: ok. 5 passed; 0 failed    (collab_webhook_tests)
test result: ok. 1 passed; 0 failed    (merge_queue_ci_tests)
test result: ok. 15 passed; 0 failed   (merge_readiness_tests)
EXIT=0

$ cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 45.81s
EXIT=0

$ scripts/check.sh --quick
==> cargo fmt --all --check
    (1s)
==> cargo clippy --workspace --all-targets -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.28s
    (1s)
==> checks passed in 2s (quick)
EXIT=0
```

未跑 `cargo test -p rg-http` 全量（34 分钟），按任务要求改为上表的 PR merge / webhook 子集 +
`cargo check -p rg-http --all-targets`。

## 5. 遗留风险

1. 投递仍是「commit 后立即 spawn」：commit 与 spawn 之间崩溃 = 「有事件、无投递」。至少一次投递留给 outbox。
2. 日志信封与投递信封 `id` 不同（`record_event_in_txn` / `deliver_event` 各自建信封）。业务 payload 一致，
   测试有断言；outbox 从存表信封投递时恢复。
3. `pull_request.merged` 的 actor 仍是 `None`（旧 payload 无 `author_id`；本任务刻意保持 payload 等价）。
   要带 actor 需要改 payload，属于会影响消费者的独立变更。
4. `trigger_issue_opened` 已无调用点但仍是「提交后写事件」的旧形状（T13 遗留）。本任务 owned 范围只允许处理
   `trigger_pr_merged`，故未顺手删；建议单开清理任务一次删掉两个。
5. 事务内多一条 insert + 最多两次 actor 读，未做性能测量（单条 insert 量级）。
