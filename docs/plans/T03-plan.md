# T03 Execution Plan — R3-P1 事件信封与投递头（issue #15）

> 状态：实现完成，验收命令已在本机跑绿。基线 `5509c4c`（基于 `0523bb2`），分支 `agent/t03-event-envelope`。

## 1. 范围

**做（P1）**

1. `EventEnvelope { schema, id, kind, at, repo_id, actor, data }`：一次事件只构造一次，`repo_events` 与全部 webhook 投递共用同一个 `id`。
2. 投递头 `X-IronForge-Event`（kind）、`X-IronForge-Delivery`（delivery id）。
3. payload 外层包信封，原业务字段逐字保留在 `data`。
4. 修「5xx 视为成功」：接收方 429 / 5xx 入队重试（复用 `deliver_retry_job` 的退避），4xx 保持原语义（记为已投递、不重试）。
5. `repo_events` 读端兼容：同时容忍无信封的旧行与带信封的新行。

**不做**

- P2（把 `record_repo_event` 移进业务事务）
- P3（`event_outbox` + 消费者）
- WS 接入 repo 事件（`ws.rs` 未改一行）

## 2. 现状（核实）

| 事实 | 位置（基线） |
| --- | --- |
| `trigger_event` 是唯一出口：`record_repo_event` + 列出匹配 hook + 后台 `deliver` | `rg-core/src/webhook/service.rs:131` |
| 15 个事件由 12 个 helper + 2 个直接调用点各拼 `json!`，无信封/无 schema/无事件头 | `service.rs:409-561`、`rg-http/src/lib.rs:3480`、`collab/webhook_events.rs:95` |
| `deliver` 只设 `X-Hub-Signature-256`；任何 HTTP 状态都返回 `Ok(status)` | `service.rs:257-300` |
| 只有**传输错误**入队重试；5xx 被当成功，delivery 行记 `200/500` 但不再投 | `service.rs:170-197` |
| `repo_events` 单写点 | `service.rs:227` |
| 读端把 `payload` 原样回吐 | `rg-http/src/api/repo_events.rs:172-189` |

## 3. 实现

### 3.1 信封（`rg-core/src/webhook/service.rs`）

```rust
pub const EVENT_SCHEMA_VERSION: u32 = 1;
pub const HEADER_EVENT: &str = "X-IronForge-Event";
pub const HEADER_DELIVERY: &str = "X-IronForge-Delivery";

pub struct EventEnvelope {
    pub schema: u32,          // 常量 1
    pub id: Uuid,             // 事件身份，日志与所有投递共用
    pub kind: String,         // 事件名（与旧 payload.event 同值）
    pub at: DateTime<Utc>,    // RFC 3339
    pub repo_id: i64,
    pub actor: Option<ActorRef>,  // 复用 rg_core::agent::ActorRef（T26）
    pub data: Value,          // 旧 payload，逐字不变
}
```

- `trigger_event` 保持签名不变（`payload: &Value`），内部构造信封后交给 `dispatch`。
- `dispatch` 是新的私有出口：序列化一次 → 写 `repo_events` → 对每个 hook 起后台任务。`redeliver` 也走它。
- `actor` 尽力而为：从 payload 取账号 id（顶层 `actor_id/author_id/pushed_by`，或下一层的 `actor/pull_request/issue/release/milestone/comment`），一次 `agent::resolve_actors` 查询；查不到或查询失败 → `null`，绝不让事件失败。P2 可以改成显式传参。
- `looks_like_envelope` 是读写两端共用的形状判定（`schema >= 1` + `id` + `kind` + 存在 `data`），供读端兼容与 `redeliver` 防二次包裹。

### 3.2 投递头与 5xx

- `deliver(... event, delivery_id)` 增加两个头；delivery id 就是写进 `webhook_deliveries.delivery_id` 的那个，接收方能把请求与投递记录对上。
- `retryable_status(status) = 429 || 500..=599`。
- 初次投递：可重试状态 → 警告日志 + `enqueue_retry`（与传输错误同一条路径）+ delivery 行记真实状态与 `retryable receiver status N`。
- `deliver_retry_job`：同一判定，可重试状态返回 `Err`，队列按既有退避（30s × 2^(n-1)，上限 1h，最多 5 次）重排；4xx 返回 `Ok`，不再重试。
- `redeliver`：存储的 payload 已是信封则原样复用（同一 `id`），旧行（无信封）包一次。

### 3.3 读端（`rg-http/src/api/repo_events.rs`）

`unpack_payload` 把存储行拆成 `(业务 payload, envelope)`：

- 旧行（无信封）→ `payload` 原样，`envelope: null`。
- 新行 → `payload` = `data`（**与旧行同形**），`envelope` = 完整信封。

因此既有读端（含 MCP `wait_repo_events`）读 `payload.<field>` 不需要改；想用 `kind/id/actor` 的读端读新增的 `envelope`。`ws.rs` 未改。

## 4. 兼容性影响清单（webhook payload 是外部契约）

### 4.1 新增的顶层键（全部为增量）

| 键 | 类型 | 说明 |
| --- | --- | --- |
| `schema` | int，恒为 `1` | 信封版本；更高版本表示消费方不要假设只懂 `data` |
| `id` | uuid 字符串 | 事件身份。同一事件在 `repo_events` 与每个 hook 的每次尝试（含重试、重放）都相同 |
| `kind` | 字符串 | 事件名，与 `data.event` 一致（`data.event` 仍保留） |
| `at` | RFC 3339 字符串 | 信封构造时刻（UTC） |
| `repo_id` | int | 仓库 id |
| `actor` | 对象或 `null` | `ActorRef { id, username, kind, owner_id, owner_username }`；payload 未指明账号时为 `null` |
| `data` | 对象 | 旧 payload，逐字保留 |

### 4.2 迁移到 `data` 的旧字段（键名、值、嵌套结构全部不变）

`event`、`authority`、`repository`、`ref`、`ref_type`、`before`、`after`、`pushed_by`、`issue`、`pull_request`、`release`、`milestone`、`comment`、`task_id`、`question`、`task`、`url`、`truncated`、`original_bytes`（仅截断存根时）。

**不删除、不改名任何旧字段**；变化只有「整体下移一层」。

### 4.3 破坏性影响（必须让接收方知道）

| 影响 | 受影响方 | 缓解 |
| --- | --- | --- |
| 旧接收方直接读顶层字段（`body.ref`、`body.event`）会读到 `null` | 所有 webhook 接收方 | 迁移到 `body.data.*`；可用 `body.schema >= 1` 判定新格式。未做双写：同一字段出现在两处会让"以哪份为准"变成歧义 |
| HMAC 签名现在覆盖**信封**后的 body | 校验 `X-Hub-Signature-256` 的接收方 | 校验方式不变（对原始 body 求 HMAC），无需改代码 |
| 5xx / 429 会被重试（最多 5 次，退避 30s→1h） | 返回 5xx 的接收方 | 之前"5xx 等于已送达"的假设不再成立；去重请用 `X-IronForge-Delivery`（每次尝试不同）或 `id`（同一事件相同） |
| 4xx 语义不变 | — | 记为已投递、不重试 |

### 4.4 非 HTTP 面

| 面 | 变化 | 兼容性 |
| --- | --- | --- |
| `webhook_deliveries.request_payload` | 存信封后的 body | 管理端只展示原始请求体；`redeliver` 会正确复用信封 |
| `redeliver` | 重放保留原 `id`；旧行包一次新信封 | 接收方能按 `id` 去重 |
| `GET /repos/{o}/{r}/events` | 新增 `envelope` 键；`payload` 仍是业务 payload | 非破坏（仅增键） |
| MCP `wait_repo_events` | 透传上面的响应 | 非破坏 |
| 事件日志 16 KiB 截断 | 预算现在包含信封（约 200 字节开销），超限仍是 `{"truncated":true,...}` 存根 | 行为不变；存根不是信封，读端回 `envelope: null` |

## 5. 测试

新增 `crates/rg-core/tests/webhook_envelope_tests.rs`（11 项）——单独二进制的原因：投递要对 loopback 接收方，需要 `IRONFORGE_ALLOW_LOCAL_OUTBOUND=1`，而该进程级开关会与 `net_guard` 的单元测试（断言 loopback 被拒）互相打架。

- 三类事件各一项钉死信封字段 + 旧字段仍在 `data`：`push`（`pushed_by` 顶层）、`pull_request.opened`（`author_id` 在 `pull_request` 内）、`release.created`。
- `push` 一项同时断言两个投递头。
- 事件日志与投递内容一致；存储行可被判为信封并由读端解包。
- 5xx → 入队重试 + delivery 行记 503 + 重试载荷仍是信封；429 → 重试；4xx → 不重试且记 404。
- `deliver_retry_job`：500 返回 `Err`（队列退避），400 返回 `Ok`。
- `redeliver` 不二次包裹且 `id` 不变。

`rg-core/src/webhook/service.rs` 内保留纯函数单元测试（信封判定、可重试状态、actor 查找）；`rg-http/src/api/repo_events.rs` 内新增 `unpack_payload` 兼容测试。

既有测试调整：`crates/rg-http/tests/collab_webhook_tests.rs` 的 4 处 payload 断言改为读 `data`，并在第一项加断言钉住 `schema/kind/id`（这就是外部契约变更在仓内的落点）。

## 6. 验收（真实输出）

```
$ cargo test -p rg-core webhook
running 10 tests
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 276 filtered out; finished in 0.02s
     Running tests/webhook_envelope_tests.rs
running 11 tests
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.54s

$ cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 45.67s   （零警告）

$ scripts/check.sh --quick
==> cargo fmt --all --check        (1s)
==> cargo clippy --workspace --all-targets -D warnings   (0s)
==> checks passed in 1s (quick)
```

额外跑的（不在验收命令里，用于确认不破坏既有面）：`cargo test -p rg-core -p rg-http` 全绿（含 `repo_events_tests`、`background_job_tests`、`collab_webhook_tests`、`commit_identity_tests`、`pr_export_tests`、`cherry_pick_revert_tests`、`update_branch_tests`）。

## 7. 遗留与下一步

- **P2（事务内写入）**：未做，也不需要——P1 全程没碰事务。`record_repo_event` 仍由 `spawn_post_push_hooks` 在业务提交之后调用，失败只 `warn`。P2 应覆盖 push / PR / issue 三条主路径。
- **P3（outbox + 消费者）**：未做。P1 给它留了两块基础：稳定 `id`（跨通道去重/游标）与 `looks_like_envelope`（同一条 outbox 行既能给新消费者，也能给仍读旧格式的读者）。
- **可改进（不在 P1）**：`actor` 目前是"从 payload 猜"，P2/P3 应在写事件时显式传 `ActorRef`，这样"谁触发的"不再依赖 payload 里恰好有 `author_id`；另外 16 KiB 截断现在把信封一起算进预算，后续可只截 `data`。
