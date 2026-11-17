# T16 执行计划 — [#15 步骤4] `pull_request.merged` 走事务内事件写入

> 权威约束见 `tasks/T16.md`。基线 `16b21aa`，分支 `agent/t16-pr-merged-txn`，worktree `../ironforge-t16`。
> 验收命令真实输出、触碰路径与遗留风险见 `docs/plans/T16-task.md`。

## 1. 交付边界

做：

1. `pull_request::service::update_pr_merged` 的 merge 事务内新增 `record_event_in_txn(&txn, repo_id,
   "pull_request.merged", &merge_event_payload)`：`pull_request.merged` 的 `repo_events` 行与 PR 状态、
   `pr_events` 行同一事务提交/回滚。
2. merge 事务 commit 之后改为 `deliver_event(&db, ..)`（只投递、不写日志），失败只 warn。
3. 删除 `webhook::service::trigger_pr_merged`（唯一调用点已改写，无其它调用点），原位留注释说明原因。
4. 新增 `crates/rg-core/tests/pr_merge_event_txn_tests.rs`：真实 merge 恰好 1 行日志 + 1 次投递；
   被拒绝的 merge 不写事件。
5. 修掉 `issue_event_txn_tests.rs` 里「twelve `trigger_*`」的过时数字（删掉一个后计数不再成立，只改措辞）。

不做（明确排除）：其它 `trigger_*` 路径迁移、CI 写路径（13 个助手里没有 pipeline/job 事件）、outbox 表、
合并算法 / 冲突处理 / 保护规则 / `head_sha` 相关逻辑（`update_pr_merged` 只动事件写入那一段）。

未改：`Cargo.*`、迁移、`crates/rg-db/src/ops/`、`rg-http/src/`、`CLAUDE.md`、`ironforge-docs/`、
`.cnb.yml`、`.github/`、`scripts/`、`deploy/`、`crates/rg-mcp/`。

## 2. 改动前后

改前（`16b21aa`）：

```rust
let txn = db.begin().await?;
let merged_pr = pull_request_ops::update(&txn, active).await?;
pr_event_ops::record(&txn, .., "pull_request_merged", ..).await?;   // PR 自己的表
txn.commit().await?;

// Trigger pull_request.merged webhook (outside the transaction on purpose)
let merge_payload = json!({ id, repo_id, number, title, merge_commit_sha, strategy });
trigger_pr_merged(db, repo_id, &merge_payload)   // 失败只 warn
```

改后：

```rust
let txn = db.begin().await?;
let merged_pr = pull_request_ops::update(&txn, active).await?;
pr_event_ops::record(&txn, .., "pull_request_merged", ..).await?;   // 未改

let merge_payload = json!({ id, repo_id, number, title, merge_commit_sha, strategy });   // 同一份构造，提前
let merge_event_payload = json!({ "event": "pull_request.merged", "pull_request": merge_payload });
record_event_in_txn(&txn, merged_pr.repo_id, "pull_request.merged", &merge_event_payload).await?;   // 失败即回滚 merge

txn.commit().await?;

deliver_event(db, merged_pr.repo_id, "pull_request.merged", &merge_event_payload)   // 只投递，失败只 warn
```

`record_event_in_txn` 的失败用 `?` 上抛，与 T13 对 `issue.opened` 的处理一致：宁可 merge 失败（事务回滚、
PR 仍是 `merging`，由 `merge_pr_expecting_head` 的 `restore_merge_claim` 复原为 `open`），也不要
「merge 成功但没有事件」。

## 3. payload 逐字节等价性论证

`trigger_pr_merged` 原本只做一层包装：

```rust
pub async fn trigger_pr_merged(db, repo_id, pr: &Value) -> Result<()> {
    let payload = json!({ "event": "pull_request.merged", "pull_request": pr });
    trigger_event(db, repo_id, "pull_request.merged", &payload).await
}
```

而 `trigger_event` 把 `payload` **原样**放进信封的 `data`（`EventEnvelope::new(.., payload.clone())`），
`record_event_in_txn` / `deliver_event` 用的是同一个 `payload` 入参。所以：

| 维度 | 旧（`trigger_pr_merged`） | 新（直接构造） |
| --- | --- | --- |
| 内层对象 | `json!({ id, repo_id, number, title, merge_commit_sha, strategy })`，取自 `merged_pr` 与函数参数 | **同一段字面量**，同一个来源，逐字复制 |
| 外层 | `{ event: "pull_request.merged", pull_request: <内层> }` | 同一段字面量 |
| 作为 `data` 的形状 | `trigger_event` 直接 `payload.clone()` | `record_event_in_txn` / `deliver_event` 直接 `payload.clone()` |
| 键序 | `serde_json` 未启用 `preserve_order`，`Map` 是 `BTreeMap`，键按字典序序列化 | 同上，且键集合相同 |
| `event` 名 | 字符串字面量 `"pull_request.merged"` | 同上（同一字面量，两处：`record_event_in_txn` 的 `event` 参数与 payload 内 `event` 键） |
| actor | `event_actor(db, payload)`：payload 顶层无 `actor_id`/`author_id`/`pushed_by`，`pull_request` 容器内也没有 `author_id` → `None` | `event_actor_on(&txn, payload)`，规则相同 → `None` |

`merged_pr` 是 `pull_request_ops::update(&txn, active)` 的返回值，**在事务内即可用**；`merge_commit_sha`、
`strategy` 是入参。整份 payload 不依赖 commit 之后的任何值，因此「提到事务内」没有任何信息缺口
（这是任务里「卡住就停」的检查点，结论是不需要停）。

行内证据（新测试的断言）：
`data.event == "pull_request.merged"`、`data.pull_request.id/repo_id/number/title/merge_commit_sha/strategy`
与 `merge_pr` 的返回值、DB 里的 PR 行完全一致，且投递体 `data` 与日志行 `data` 相等。

## 4. `trigger_pr_merged`：删除，而不是保留

选择 **B（删除）**，理由：

1. 它**已无任何调用点**（全仓 grep 只有本任务改写的那一处）。
2. 它的语义就是缺陷本身：`trigger_event` = 「先写日志、再投递」，一旦被重新调用到 merge 路径，就退回
   「merge 已提交、事件在事务外写」的旧形状；而且它比直接写 `trigger_event` 更容易被误认为「merge 专用的
   正确入口」。保留它等于保留一个只等着被误用的 API。
3. 13 个命名糖的「统一形状」在本任务里不是必须的：`trigger_pr_merged` 的兄弟（`trigger_pr_opened` /
   `trigger_pr_closed` / `trigger_issue_*`）服务的是「业务写完、事件随后」的路径，它们的业务写本来就没有
   相应的事务内事件；merge 是唯一被 #15 点名要求改的路径。

原位保留了一段注释（不是死代码）说明「为什么这里没有这个函数」，防止后来者照抄历史代码再补一个。

**T13 的同类隐患**：`trigger_issue_opened` 也已无调用点，但仍在。它同样是「提交后写事件」的旧形状，
应当在 outbox 落地时一并删除或改为 `record_event_in_txn + deliver_event`。本任务 owned paths 里
`webhook/service.rs` 的范围被限定为「`trigger_pr_merged` 的保留/删除」，所以**没有顺手删它**；
建议由协调者单开一条清理任务（一次删掉两个已无调用点的旧形状助手，比分散在两条任务里更容易 review）。

## 5. 验证策略

新增 `crates/rg-core/tests/pr_merge_event_txn_tests.rs`（独立测试二进制，沿用 loopback receiver 模式；
用真实 bare git 仓库 + 真实 `merge_pr`，不 mock merge）：

1. `a_merged_pr_logs_and_delivers_pull_request_merged_exactly_once`
   - 真实 merge（`MergeStrategy::Merge`）→ `repo_events` 里 `pull_request.merged` **恰好 1 行**；
     `data.event` / `data.pull_request.*` 与 merge 结果一致；receiver 收到 1 次、`X-IronForge-Event` 头正确、
     投递体 `data == 日志 data`；`webhook_deliveries` 恰好 1 行；500ms 内无第二次投递；
     最后 `refs/heads/main` 等于返回的 merge commit（证明 merge 真的发生）。
   - 这条断言同时钉住「`deliver_event` 不写日志」：把 `deliver_event` 临时改成也 `append_envelope`，
     测试立刻变红（2 行），见 task 文档 §3。
2. `a_refused_merge_writes_no_event`
   - 已被关闭的 PR 再 merge → 失败（`state_gate`），日志里没有 `pull_request.merged`。
3. **回滚不写事件：无法构造，如实说明。** 本路径的失败点（state/draft gate、`claim_merge`、分支存在性、
   git merge 本身）**全部在事务之前**；事务内的三步（`pull_request_ops::update`、`pr_event_ops::record`、
   `record_event_in_txn`）没有任何可从外部触发的失败注入点，硬造一个只能改生产代码或伪造场景。
   该性质由 T13 的机制测试覆盖：`issue_event_txn_tests::an_event_in_a_rolled_back_transaction_is_neither_logged_nor_delivered`
   消费的是**同一个** `record_event_in_txn`，事务回滚语义与调用方无关。
4. 既有 merge 测试全绿（rg-core 全量 + rg-http 的 PR merge 集成测试子集）。

## 6. 遗留风险

1. 投递仍是「commit 后立即 spawn」：commit 与 spawn 之间崩溃 = 「有事件、无投递」。至少一次投递要等 outbox，
   与 T13 一致。
2. 日志信封与投递信封的 `id` 不同（`record_event_in_txn` 与 `deliver_event` 各自建信封，只拿到 payload）。
   业务 payload 一致（有断言）；outbox 由存表信封投递时恢复同一 id。
3. `pull_request.merged` 的 actor 仍是 `None`：旧 payload 里没有 `author_id`，本任务**刻意不变**（payload
   必须逐字节等价）。若要带上 actor，是独立的 payload 变更，应单独 review（会影响消费者）。
4. 事务内多一条 insert + 最多两次 actor 读，未做性能测量（单条 insert 量级），与 T13 相同。
5. `trigger_issue_opened` 仍是「提交后写事件」的旧形状且无调用点（见 §4），留给后续清理任务。
