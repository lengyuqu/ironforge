# T19 执行计划 — [#15 步骤7] outbox 最小版本：投递状态 + 兜底补投

> 权威约束见 `tasks/T19.md`。基线 `47fe182`，分支 `agent/t19-outbox-min`，worktree `../ironforge-t19`。
> 验收真实输出、触碰路径与遗留风险见 `docs/plans/T19-task.md`。

## 1. 交付边界

做：

1. 迁移 `m20261117_000001_add_repo_events_dispatched_at`：加 `repo_events.dispatched_at`（可空）、
   复合索引 `(dispatched_at, created_at)`，**并把历史行回填为 `created_at`**；`down` 先删索引再删列。
2. `repo_event_ops`：`append_handled`（可显式给 `dispatched_at`）、`mark_dispatched`、
   `list_undispatched_between`（sweep 用）、`list_undispatched_of_type`（事务路径认领行用）。
3. `webhook::service`：`append_envelope` 返回 event id → `record_envelope` 透出 → `deliver_envelope`
   在**发出 spawn 之前**把该 id 标为已交付；静默路径（`record_event` / `trigger_event_quietly`）写入即标记；
   事务路径（`deliver_event`）用 payload 认领行。
4. `webhook::outbox`：`sweep`、`SweepReport { scanned, redelivered, failed }`、`run_sweep_loop`。
5. `rg-http/src/lib.rs`：增加一个 `supervise` 注册块（仅注册，不动其它逻辑）。
6. `crates/rg-core/tests/outbox_sweep_tests.rs`：8 条集成测试（含 6 条要求 + 静默路径 + 事务路径 + 不重复）。

不做（明确排除）：投递状态机（`pending/processing/sent`）、幂等键、投递顺序保证、
把 `trigger_event` 改成「只写不投」、给 `webhook_deliveries` 加 `event_id`、完整 outbox 表。

未改：`Cargo.*`、`crates/rg-core/src/{pull_request,issue,release}/` 的业务代码、`CLAUDE.md`、
`ironforge-docs/`、`.cnb.yml`、`.github/`、`scripts/`、`deploy/`、`crates/rg-mcp/`。

## 2. 为什么是 `repo_events.dispatched_at`，不是 `webhook_deliveries.event_id`

`webhook_deliveries` 已经是「投递结果」表，看起来更自然的位置是给它加 `event_id`，然后用
「是否存在 delivery 行」反推某事件是否投过。否决理由三条：

1. **方向反了。** 要回答的问题是「这条事件有没有交出去过一次投递」，而 deliveries 记录的是**每次尝试的结果**。
   一个事件可以对应 0 行（从未投递）、N 行（多 hook）、甚至 0 行但已投递（hook 查询成功、spawn 前崩溃）。
   用「子表有没有行」表达父表的状态，是拿结果集当状态机，正是任务明令不要做的那类东西。
2. **`spawn` 前没有 delivery 行。** 正常路径是「spawn → 内部建 delivery 行」。崩溃点恰好落在 spawn 之前，
   此时既没有 delivery 行也没有投递 —— 与「还没轮到」无法区分。这正是本任务要补的洞。
3. **成本不对称。** 加 `event_id` 要改 delivery 行的写入路径（每次投递都多一个外键值与一次列写入），
   而且仍需回答「没有任何 delivery 行 = 没投过？」这个上面已经否掉的问题。把状态放在事件行上，
   一次 UPDATE 就够，且语义单一。

`dispatched_at` 的语义是**「已经交给投递系统」**，不是「投递成功」：成功/失败仍在
`webhook_deliveries.response_status`，失败重试仍走 `background_jobs`（QUEUE-001）。本列只回答
「兜底 worker 还欠不欠这条事件一次首发」。

## 3. 历史行回填（本任务最危险的地方）

迁移会把部署前的**全部** `repo_events` 变成 `NULL`（新列默认值），而 worker 的语义是「`NULL` = 漏了」。
如果直接上线：worker 第一次 tick 就会把保留期（7 天）内的历史事件全部重投给每个仓库的每个活跃 hook。
对用户而言就是「升级后突然收到几百条几天前的 webhook」。

两种口径：

| 方案 | 做法 | 评价 |
| --- | --- | --- |
| A. worker 侧限定 `created_at > 部署时刻` | 运行期记住一个时间点 | 依赖进程状态：重启、多实例、时钟漂移都会破坏它，且无法解释「部署后但重启前」的行 |
| B. 迁移回填 `dispatched_at = created_at` | 一次性 SQL，与运行期无关 | 采用 |

`up` 的实现关键：**回填只在「这次真的加了列」的那一趟执行**。

```rust
if !manager.has_column(TABLE, COLUMN).await? {
    manager.alter_table(.. add_column(dispatched_at, null) ..).await?;
    manager.get_connection().execute_unprepared(
        "UPDATE repo_events SET dispatched_at = created_at WHERE dispatched_at IS NULL",
    ).await?;   // ← 只在这里
}
```

否则一次「回滚 + 重放」（`rg-db` 的迁移回滚测试每天都会做）会把真正漏投的行一起标记掉。
回填后 `NULL` 只剩两种来源：新代码写下的未投递行、以及回滚/重放期间老二进制写下的行 —— 后者也是
「写入时代码自己会投递」，回填同样是正确口径。

`down` 先 `drop_index` 再 `drop_column`：SQLite 不允许删除被索引引用的列。让 `down` 真的删列，
是为了保住「列存在 ⇔ 回填已跑」这个不变式；若 `down` 做成 no-op，回滚窗口里老二进制写下的行
就会在重放后永远停在 `NULL`。

## 4. event id 怎么进 `deliver_envelope`

`append` 本来就返回 id，被 `record_envelope` 丢掉了。改动后的数据流：

```
trigger_event ──▶ record_envelope(...) ──▶ Option<i64> ──┐
                                                        ├─▶ deliver_envelope(.., event_id)
dispatch(redeliver) ──▶ record_envelope ──▶ Option<i64> ─┘        │
                                                                  ├─ ① list_active_by_repo_and_event
                                                                  ├─ ② mark_dispatched(event_id)   ← 在 spawn 之前
                                                                  └─ ③ for hook { spawn(deliver) }
```

- `record_envelope` 返回 `Option<i64>` 而不是 `Result`：这条路径的日志写一直是 best-effort（失败只 warn），
  改成 `Result` 会改变调用方语义。拿不到 id 时照常投递、只是不标记 —— 宁可让 sweep 多投一次，
  也不让一个日志写失败吞掉 webhook。
- 标记放在 `list_active...` **之后**：hook 查询失败时不该留下「已交付」的假状态，`?` 直接返回，
  行保持 `NULL`，下一轮 sweep 接手。
- 标记放在 spawn **之前**：spawn 是 fire-and-forget，没有完成回调。之后标记会让「deliver 成功但标记失败」
  变成重复投递；之前标记只漏掉「标记后、spawn 前」的微秒级窗口，且这个窗口不产生重复。任务本身
  给了同样的取舍，这里照做。
- 标记失败只 warn，不中止投递：投递比状态更重要；失败留下的 `NULL` 最多让 sweep 再投一次（至少一次语义）。

### 事务路径（`record_event_in_txn` + `deliver_event`）

`record_event_in_txn` 的 id 在事务内被丢弃，`deliver_event` 的签名只拿得到 `(db, repo_id, event, payload)`。
**这条路径的调用点在 `pull_request/` `issue/` `release/` 的业务代码里，属于本任务禁止触碰的路径**，
所以不能把 id 从 `record_event_in_txn` 透到 `deliver_event`。

如果不处理这条路径，问题比缺投递更糟：5 条已迁移路径的每一行都会永远停在 `NULL`，
2 分钟后被 sweep 全部重投一次 —— 升级即给所有用户重复投递。

处理方式：`deliver_event` 在提交后按**内容**认领那一行（`find_delivery_row`）：

- `repo_id` + `event_type` 相同；
- `dispatched_at IS NULL`；
- `created_at` 在最近 15 分钟内（与 sweep 窗口同界，保证「sweep 看得见的」与「这里找得到的」是同一集合）；
- 存储信封的 `data` 与本次 `payload` **完全相等**（两个函数拿到的是同一个 `Value`）。

唯一能认错的场景是「同一仓库同时有两个**完全相同** payload 的同类型事务事件」；此时认领其中任意一行
都是等价投递，另一行保持 `NULL` 由 sweep 补投 —— 结果是最多一次重复，而不是丢事件。取候选上限 20 行，
超过说明已经异常，交给 sweep 更安全。

这条「按内容认领」的代价是事务路径每次多一次 SELECT。相比改三个业务模块（越界）或让 5 条路径全量重投
（事故），这个代价是明确最小的。

## 5. 静默路径：`NULL` 不等于「欠一次投递」

`record_event` / `trigger_event_quietly`（T11，agent 草稿区）故意写事件、不投递。它们如果写 `NULL`，
sweep 会把草稿 push 全部投给 hook —— **直接推翻 T11 的静默保证**。

所以这两条路径写入时就把 `dispatched_at` 置为 `now()`：**「这次投递决策已经终局」**。列语义因此是
「已交给投递系统 *或* 明确决定不投递」，文档写在实体字段上。`append_envelope` 因此多一个
`dispatched: bool` 参数，`append` 保持 NULL、另加 `append_handled` 承载两种取值。

`outbox_sweep_tests::a_quiet_event_is_never_swept` 钉住这一点。

## 6. 窗口：为什么是 `[now-15min, now-2min]`

```
   now-15min            now-2min        now
      ├────────────────────┤
      │  sweep 扫描         │  不动
```

- **下界 `now-2min`**：刚写完的事件正在正常投递。扫它会与正常路径抢同一次投递，为「偶发重复」换取
  「极小概率的提前补投」—— 重复投递是用户可见的事故，等两分钟只是延迟。2 分钟也远大于任何一次
  正常投递的耗时（HTTP 超时 30s，spawn 立即返回）。
- **上界 `now-15min`**：`repo_events` 保留 7 天，不设上界就是全表扫；而且超过 15 分钟仍未投递，
  说明不是「进程刚好崩了一下」，而是系统性故障，人工排查比自动重投更安全（任务原文口径）。
- 每 60s 一次 × 15 分钟窗口 ⇒ 每条漏网事件最多被尝试约 13 次，够用且不会无限重试。
- 历史行不靠窗口兜底，靠迁移回填（§3）：窗口只决定「扫多少」，回填决定「哪些根本不该被扫」。

## 7. 索引

`(dispatched_at, created_at)`：sweep 的 WHERE 是 `dispatched_at IS NULL AND created_at BETWEEN ? AND ?`。
`dispatched_at` 绝大部分非空，`IS NULL` 前缀极具选择性，再在剩余行上按 `created_at` 范围扫。
只加 `created_at` 的话，范围扫要回表逐行判断 `dispatched_at`，随事件量线性退化。

已有的 `idx_repo_events_created_at` 保留：保留期清理（`delete_older_than`）只认 `created_at`，
与本索引的列顺序不兼容（前导列不同）。

## 8. worker 注册与配置

`rg-http/src/lib.rs` 只加一个 `supervise` 块，与 T11 的 `agent_drafts.cleanup` 同形：

```rust
tokio::spawn(rg_core::task_supervisor::supervise("webhook.outbox_sweep", { .. }));
```

`run_sweep_loop` 内部：`interval` 的首个 tick 立即触发（重启即补齐上一个进程最后 15 分钟漏掉的），
单次失败只 `warn` 不退出；`redelivered/failed` 都为 0 时不打日志（避免每分钟一条噪声）。

配置：本仓没有通用的间隔配置系统（`agent_drafts.cleanup` 硬编码 3600s，
`repo_events.cleanup` 硬编码 3600s），只有 `IRONFORGE_GIT_STREAM_TIMEOUT_SECS` 这种单点环境变量。
照那个风格加 `IRONFORGE_WEBHOOK_OUTBOX_SWEEP_SECS`（默认 60，`0` 关闭），不新建配置系统。
`0` 时 `run_sweep_loop` 用 `pending()` 挂起而不是 return —— 该任务被 `supervise` 托管，
return 会被当成异常并按秒重启刷错误日志。

## 9. 测试设计

`crates/rg-core/tests/outbox_sweep_tests.rs`（独立测试二进制，理由是它会真的 POST 到 loopback，
需要进程级 `IRONFORGE_ALLOW_LOCAL_OUTBOUND=1`，不能污染 `net_guard` 的「loopback 被拒」单测）：

| # | 测试 | 钉住什么 |
| --- | --- | --- |
| 1 | `a_historical_undispatched_row_is_never_replayed` | **死线**：`NULL` + 一天前的行不被扫、不被标记 |
| 2 | `a_missed_event_inside_the_window_is_redelivered_once` | 漏网被补投 1 次、信封 id 不变、被标记；第二次 sweep 为 0（幂等） |
| 3 | `a_normally_triggered_event_is_not_repeated_by_the_sweep` | `trigger_event` 后 `dispatched_at` 非空；把行人为老化到窗口内仍 0 次 |
| 4 | `a_transaction_event_is_marked_after_it_is_delivered` | 事务路径按 payload 认领成功，老化后不被重投 |
| 5 | `a_quiet_event_is_never_swept` | T11 静默保证不被 sweep 破坏 |
| 6 | `the_window_edges_are_excluded` | `now-10s`、`now-20min` 不扫；中间带（`now-3min`）能扫到（反证边界真的起作用） |
| 7 | `an_unparseable_payload_is_settled_instead_of_looping` | 坏 JSON 与截断 stub 都被标记，第二轮 scanned=0 |
| 8 | `the_sweep_never_duplicates_a_delivery_it_did_not_make` | 正常投递的 delivery 行数不变 |

迁移本身在 `m20261117_...rs` 内有 3 条单测：DDL 三方言表名、**历史行回填 + 重跑不回填**、`down`/`up` 往返。

## 10. 遗留风险（写在 task 文档 §6）
