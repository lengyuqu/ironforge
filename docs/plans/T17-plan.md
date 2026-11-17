# T17 执行计划 — [#15 步骤5] `release.created` 走事务内事件写入（原本无事务的路径）

> 权威约束见 `tasks/T17.md`。基线 `15119a1`，分支 `agent/t17-release-txn`，worktree `../ironforge-t17`。
> 验收命令真实输出、触碰路径与遗留风险见 `docs/plans/T17-task.md`。

## 1. 交付边界

做：

1. `rg-db::ops::release_ops::create` 放宽为 `create<C: ConnectionTrait>(db: &C, model)`，让调用方能传事务。
2. `release::service::create_release` 新增事务：`release_ops::create(&txn, ..)` +
   `record_event_in_txn(&txn, .., "release.created", ..)` 同一事务；commit 后 `deliver_event(&db, ..)`，失败只 warn。
3. 删除 `webhook::service::trigger_release_created`（唯一生产调用点已改写），原位留注释说明原因。
4. 新增 `crates/rg-core/tests/release_event_txn_tests.rs`：真实创建恰 1 行日志 + 1 次投递；
   查重命中不写事件。
5. 测试 `webhook_envelope_tests.rs` 里原本用 `trigger_release_created` 钉信封形状的那条，改为直接构造同一份
   payload 走 `trigger_event`（信封形状的覆盖不因删助手而丢失）。

不做（明确排除）：`release.deleted`、`issue.closed`、其它事件路径、outbox 表、`release_ops` 的其它函数。

未改：`Cargo.*`、迁移、`rg-http/src/`、`CLAUDE.md`、`ironforge-docs/`、`.cnb.yml`、`.github/`、`scripts/`、
`deploy/`、`crates/rg-mcp/`、`crates/rg-core/src/issue/service.rs`。

## 2. 改动前后

改前（`15119a1`）：

```rust
// 校验 + 查重（事务外）
let release = rg_db::ops::release_ops::create(db, model).await?;   // 单条 insert，无事务

let payload = json!({ id, repo_id, tag_name, title, body, is_draft, is_prerelease, author_id });
if let Err(e) = trigger_release_created(db, repo_id, &payload).await {
    tracing::warn!("Failed to trigger release.created webhook: {e}");   // best effort
}
```

改后：

```rust
// 校验 + 查重（事务外，位置未变）
let txn = db.begin().await.context("db: begin release transaction")?;
let release = rg_db::ops::release_ops::create(&txn, model).await?;

let payload = json!({ id, repo_id, tag_name, title, body, is_draft, is_prerelease, author_id });  // 同一段字面量
let event_payload = json!({ "event": "release.created", "release": payload });
record_event_in_txn(&txn, repo_id, "release.created", &event_payload).await?;   // 失败即整条回滚

txn.commit().await.context("db: commit release transaction")?;

deliver_event(db, repo_id, "release.created", &event_payload)   // 只投递，失败只 warn
```

`record_event_in_txn` 用 `?` 上抛，与 T13/T16 一致：宁可创建失败（事务回滚、`releases` 无行），
也不要「release 建好了但事件永久丢失」。

## 3. payload 逐字节等价性论证

`trigger_release_created` 原本只做一层包装：

```rust
pub async fn trigger_release_created(db, repo_id, release: &Value) -> Result<()> {
    let payload = json!({ "event": "release.created", "release": release });
    trigger_event(db, repo_id, "release.created", &payload).await
}
```

`trigger_event` 把 `payload` **原样**放进信封 `data`（`EventEnvelope::new(.., payload.clone())`）；
`record_event_in_txn` / `deliver_event` 用的是同一个 `payload` 入参。

| 维度 | 旧（`create_release` → `trigger_release_created`） | 新（`create_release` 直接构造） |
| --- | --- | --- |
| 内层对象 | `json!({ id, repo_id, tag_name, title, body, is_draft, is_prerelease, author_id })`，取自 `release` 行 | **同一段字面量**，同一个来源（`release` 行），逐字复制 |
| 内层键数 | 8 | 8（测试用 `object.len() == 8` 钉住） |
| 外层 | `{ event: "release.created", release: <内层> }` | 同一段字面量 |
| 作为 `data` 的形状 | `trigger_event` 直接 `payload.clone()` | `record_event_in_txn` / `deliver_event` 直接 `payload.clone()` |
| 键序 | `serde_json` 未启用 `preserve_order`，`Map` 是 `BTreeMap`，键按字典序序列化 | 同上，键集合相同 |
| `event` 名 | 字符串字面量 `"release.created"` | 同一字面量（`record_event_in_txn` 的 `event` 参数与 payload 内 `event` 键） |
| actor | `event_actor(db, payload)`：内层有 `author_id` → 解析为该用户 | `event_actor_on(&txn, payload)`，规则相同 → 同一个用户（测试实际观察到 actor 为 `release-owner`） |

`release` 是 `release_ops::create(&txn, model)` 的返回值，**在事务内即可用**；payload 不依赖 commit 之后的任何值，
因此「提到事务内」没有信息缺口（任务「卡住就停」的检查点：没有出现锁等待/死锁，不需要停）。

行内证据（新测试的断言）：`data.release` 的八个字段与 `create_release` 返回的 `release` 行、以及数据库
`releases` 行逐字段一致；投递体 `data` 与日志行 `data` 相等。

## 4. `trigger_release_created`：删除，而不是保留

选择**删除**，理由同 T16 对 `trigger_pr_merged` 的处理：

1. 全仓 grep 确认，除本任务改写的那一处外，只有测试 `webhook_envelope_tests.rs` 用它钉信封形状——那是测试，
   已改为直接构造同一份 payload 走 `trigger_event`，覆盖不降级。
2. 它的语义就是缺陷本身：`trigger_event` = 「先写日志、再投递」，一旦被重新调用到创建路径，就退回
   「release 已提交、事件在事务外写」的旧形状；保留它等于保留一个只等着被误用的 API。
3. 原位留注释说明「为什么这里没有这个函数」，防止后来者照抄历史代码补回来。

`trigger_release_deleted` 未动（`release.deleted` 不在本任务范围）。

## 5. 有意的语义变更

| 场景 | 旧行为 | 新行为 |
| --- | --- | --- |
| 事件日志写入失败（`append_envelope` 出错） | release 已提交，`trigger_event` 内部的日志写是 best-effort，仅 warn；事件**永久丢失** | `record_event_in_txn` 返回错误 → `?` 上抛 → 整个事务回滚；**release 不创建**，调用方收到错误 |
| 投递失败（receiver 5xx / 网络） | 只 warn | 只 warn（不变） |
| 查重命中 | 返回 `Conflict`，无写入 | 不变，且在事务前，不会白开事务 |

对 `rg-core::import::service` 的影响：两个导入循环本来就是「单条 release 失败只 warn 并继续下一条」
（`Err(e) => tracing::warn!("Failed to import release ...")`）。新语义下，事件写失败会让**该条** release
不落库并计入失败，循环继续——不会让整次导入失败，行为可接受。

## 6. 验证策略

新增 `crates/rg-core/tests/release_event_txn_tests.rs`（独立测试二进制，沿用 loopback receiver 模式；
真实 SQLite + 真实 `create_release`，不 mock）：

1. `a_created_release_logs_and_delivers_release_created_exactly_once`
   - `repo_events` 里 `release.created` **恰好 1 行**；`data.event == "release.created"`；
     `data.release` 八字段与返回的 release 行一致，且键数恰为 8；
   - receiver 收到 1 次，`X-IronForge-Event == release.created`，投递体 `data == 日志 data`；
   - `webhook_deliveries` 恰 1 行；随后 500ms 内无第二次投递。
   - 这条断言同时钉住「`deliver_event` 不写日志」：变异测试见 task 文档 §3。
2. `a_refused_release_writes_no_event`
   - 同 tag 建第二次 → `Conflict`，`repo_events` 仍只有第一次的 1 行。
3. **回滚不写事件：无法从外部构造事务内失败，如实说明。** 本路径的失败点（空 tag/title 校验、查重）
   全部在事务之前；事务内两步（`release_ops::create`、`record_event_in_txn`）没有可从外部触发的失败注入点，
   硬造只能改生产代码或伪造场景。该性质由 T13 的机制测试覆盖：
   `issue_event_txn_tests::an_event_in_a_rolled_back_transaction_is_neither_logged_nor_delivered`
   消费的是**同一个** `record_event_in_txn`，事务回滚语义与调用方无关。
4. 既有测试全绿：`cargo test -p rg-core`、`cargo test -p rg-db`、`rg-http` 的 `release_tests`。

## 7. 遗留风险

1. 投递仍是「commit 后立即 spawn」：commit 与 spawn 之间崩溃 = 「有事件、无投递」。至少一次投递要等 outbox，
   与 T13/T16 一致。
2. 日志信封与投递信封的 `id` 不同（`record_event_in_txn` 与 `deliver_event` 各自建信封，只拿到 payload）。
   业务 payload 一致（有断言）；outbox 由存表信封投递时恢复同一 id。
3. 事务内多一条 insert + 最多两次 actor 读，未做性能测量（单条 insert 量级），与 T13/T16 相同。
4. SQLite 单写者：本路径只在 HTTP 请求内短事务，实测无锁等待；但高并发同时创建 release 时写锁会串行化，
   这是原有单写者特性，不是本改动引入。
5. `trigger_issue_opened` 等仍是「提交后写事件」的旧形状（T13/T16 遗留），留给后续清理任务。
