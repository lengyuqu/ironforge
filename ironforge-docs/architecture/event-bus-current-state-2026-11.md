# 事件总线：现状核实与未决决策（2026-11）

> 用途：把 issue #15（统一事件总线）的**现状断言**固定下来，供后续实现与评审核对；不改代码、不改 schema。
> 来源：issue #15 的评审分析（comment `2120149023616040960`，`2026-11-10T13:38:12Z`）。本文只保留**经本仓代码核对过**的部分，并订正其中的事实错误（§2 逐条列明，含本文自身的两处）。
> 基线：`8d107d8`，**2026-11-13 重钉到 `6396b65`**（`rg-http/src/lib.rs` 在两者之间净减 53 行，本文该文件的 4 处锚点已随之重取，见 §1.2/§1.5/§3；其余被引文件零漂移）。全部行号在该树上用 `sed -n '<l>p'` 逐个打印核对过；引用即指该树，换基线须重取。
> 与[整合路线](harness-integrated-roadmap-2026-11.md) §4.5 的分工：路线图写**目标语义**（至少一次、可去重、游标不稳则显式失效），本文写**当前实现**与实现之前的决策点。

## 1. 已核实的现状（按代码，不按设计意图）

### 1.1 入口是一个函数，不是三条路

- `trigger_event`（`crates/rg-core/src/webhook/service.rs:131`）一次做三件事：`:140` 写仓库事件日志（`record_repo_event`，定义在 `:227`）→ `:142` 查订阅该事件的 active webhook → 之后 `tokio::spawn` 逐条投递。日志写入失败**只 warn**（`:237`），不影响投递，也不回传调用方。
- 13 个 `trigger_*` 助手（`:409`–`:551`：release×2 / branch×2 / tag×2 / issue×3 / PR×3 / milestone×1）全部在函数体内直接调 `trigger_event(db, repo_id, …)`，`grep -c 'trigger_event(db, repo_id,'` = **13** ⇒ 它们只是命名糖，不构成独立通路。
- `record_repo_event` 全文件只有一个调用点（`:140`）⇒ 「webhook 事件」与「事件日志」今天共用一次写入，**不存在两条独立写入路径**。
- 但两条路的**载荷不同源**：`record_repo_event`（`:227`）把 `event` 截到 64 字符（`:234`），payload 超过 `MAX_EVENT_PAYLOAD_BYTES`（`:225`，16 KiB）时整条替换成 `{"truncated": true, "original_bytes": N}`（`:229-233`）；webhook 侧仍投递原文（`:149` `payload_str` 取自入参）。⇒ 长轮询/事件日志不是无损副本，「按事件日志重放 webhook」会丢长载荷。

### 1.2 四个消费者的形态确实不同

| 消费者 | 代码 | 形态 | 交付语义 |
|---|---|---|---|
| webhook | `webhook/service.rs:308` `deliver_retry_job` + `background_jobs` | POST + `X-Hub-Signature-256` | 至少一次（重试后 dead） |
| 长轮询 | `crates/rg-http/src/api/repo_events.rs:162` | `{id, type, payload, payload_origin, …}`（`:186` 标 `user_content`） | 读已提交行；**游标单调时才「至少一次」，游标不稳即「至多一次」**（现状为何单调见 §1.4） |
| WebSocket | `crates/rg-http/src/ws.rs` `NotificationHub` | `broadcast` 容量 **256**（`:28`） | 至多一次，掉线即丢 |
| MCP | `crates/rg-mcp/src/server.rs:20-21`（只 `resources/list`、`resources/read`） | `initialize` 回 `"resources": {"subscribe": false}`（`:41`） | 不存在推送 |

WebSocket 的发布点只有 4 处：`rg-http/src/lib.rs:3386`、`:3526`、`collab_tasks.rs:284`、`collab_questions.rs:170`（后两处未变；前两处在旧基线 `8d107d8` 上是 `crates/rg-http/src/lib.rs:3440` / `:3579`，该文件此后净减 53 行）。

### 1.3 事件写入在业务事务之外

- issue 创建：`issue/service.rs:56` `begin` → `:59` `commit` → `:70` 才 `trigger_issue_opened`。
- PR 合并：`pull_request/service.rs:1649` `begin` → `:1666` `commit` → `:1668` 注释明写 `outside the transaction on purpose`。
- 仓库事件表 append 走 `repo_event_ops::append`（`crates/rg-db/src/ops/repo_event_ops.rs:8-22`），签名是 `&DatabaseConnection`，`id: NotSet` 自增。

⇒ 崩溃窗口里「业务已提交、事件永久丢失、webhook 也不会重试」成立：重试只覆盖**传输失败**，不覆盖「根本没入队」。

### 1.4 游标顺序：现状靠「不在事务里」侥幸成立

长轮询按 `id > since` 读（`repo_event_ops.rs:52` `list_after`、`crates/rg-http/src/api/repo_events.rs:162`），`id` 是自增主键。现在之所以看不到乱序，是因为写入不在任何业务事务里；**一旦把 append 搬进事务**，自增值的分配顺序 ≠ 提交顺序，晚提交的低 id 会被已推进的游标永久跳过。这正是路线图 §4.5「不能直接假设自增 ID 的分配顺序等于事务提交顺序」所指，但路线图没给机制。

### 1.5 双写分叉（三例中成立两例）

- `push`：`rg-http/src/lib.rs:3526` 推 WS、`:3463` 触发 webhook，但**不写 notifications**——`notify_watchers_push` 定义在 `rg-core/src/repo/service.rs:1590`（doc 注释自述「should be called from the push handler」），全仓 `grep` 零调用者。
- 比「消费者归属不同」更硬的一层：`push` 是**同一个名字、两套 payload**——事件日志与 webhook 侧 `{ref, before, after, pushed_by, repository}`（`rg-http/src/lib.rs:3451-3461`），WS 侧 `{repo, ref, commit}`（`:3530-3534`）。§2.2 的「三套命名各自实现」在这条上是反例：命名没分叉，schema 分叉了。
- `issue.opened`：只有 webhook（`issue/service.rs:70`），无 WS（`push_notification` 的 4 个调用点里没有 issue 路径）。
- ~~`collab.question.asked` 走 webhook、`collab_question` 走通知+WS~~ —— **不成立**，见 §2.2。

## 2. 对评审分析的事实订正

### 2.1 ops 层签名不是「一律 `&DatabaseConnection`」

原文称「`rg_db::ops::*` 的签名一律是 `&DatabaseConnection`…得改成 `&impl ConnectionTrait`…工作量约 30–50 个函数签名」。实测（按函数签名切分，非按行）：

| 口径 | 数量 |
|---|---|
| `crates/rg-db/src/ops/*.rs` 的 `pub async fn` 总数 | **615** |
| 已泛型为 `C: ConnectionTrait` | **117** |
| 仍写死 `&DatabaseConnection` | **498** |

而且**事务已经能穿进 ops**：PR 合并路径把 `&txn` 直接传给 `pull_request_ops::update(&txn, …)`（`pull_request/service.rs:1650`）与 `pr_event_ops::record(&txn, …)`（`:1651`）。所以 E0 不是「从零放宽」，而是「把已存在的 117 处写法收敛到剩余 498 处，并把 6 个事件入库点改到事务内」；把规模写成 30–50 会低估一个数量级。

### 2.2 `collab.question.asked` 不是「只走 webhook」

`crates/rg-http/src/api/collab_questions.rs` 同一个 handler 里顺序做三件事：`:162` 写通知（`human_notify::question_asked`）、`:170` 推 WS、`:182` 触发 webhook。`collab.task.blocked` 同形：`collab_tasks.rs:268` webhook、`:276` 通知、`:284` WS。⇒ 这两类事件的「分叉」不在消费者归属，而在**同一件事被三套命名各自实现**（`collab_question` / `EVENT_QUESTION_ASKED` / WS 的 `event_type`）。

### 2.3 三处行号

- `trigger_event`（`crates/rg-core/src/webhook/service.rs`）在 `:131`，不是 `:118`（`:118` 落在 `update_webhook` 的 `ActiveModel` 组装里；`events` 的合并是 `:114-115`）。
- `Column::Events.contains(event)` 在 **`webhook_ops.rs:43`**（`:42` 是 `Active.eq(true)`、`:44` 是 `.all(db)`）。**本行 2026-11-13 自查订正**：本文上一版把 `contains` 记成 `:44`、把 `Active` 记成 `:43`，偏移一行；原始分析里的 `:43` 本来就是对的，是我改错了。
- `webhook/service.rs:29` 才是 `CreateWebhookRequest.events` 那条 `e.g.` 注释（`:28` 是 `active`）。

另：`webhook_ops.rs:43` 的 `contains` 编译成 `events LIKE '%<投递名>%'`，左值是**落库的订阅串**、模式串是**投递名**。所以「`push` 会命中订阅了 `prepush` 的钩子」这一方向是对的（订阅串是左值、被当子串匹配），但当前 **16** 个投递名里没有任何一个是别的订阅名的子串——已知的真实缺陷只有反方向那条：订阅写 `["issues"]`（简称）永远等不到 `issues.opened`。两条都不在本轮改动范围（改匹配语义动 `webhook_ops` 契约 ⇒ L2）。

16 的口径（**2026-11-13 复算，可机械重现**）：`webhook/service.rs` 内 13 个（`release.created/deleted`、`branch.created/deleted`、`tag.created/deleted`、`issue.opened/closed/comment`、`pull_request.opened/closed/merged`、`milestone.closed`）+ `rg-http/src/lib.rs:3463` 的 `"push"` + `webhook_events.rs:30`/`:32` 的 `collab.question.asked`/`collab.task.blocked` = **13 + 1 + 2 = 16**。复算命令：`grep -rhno 'trigger_event(db, repo_id, "[a-z_.]*"' crates/ | sed 's/.*"\(.*\)"/\1/' | sort -u | wc -l` = **14**（含 `push`，不含 collab 那两个变量传参），再 +2。

大小写**不是**缺陷：SQLite 的 `LIKE` 对 ASCII 默认不区分大小写（`SELECT 'Push' LIKE '%push%'` = **1**，本机 `sqlite3 3.53.1` 实测），SeaORM 也不加 `LOWER()` ⇒「订阅串写 `Push`/`Issues` 会静默不命中」不成立，`webhook/service.rs:29` 注释里的「case-insensitive LIKE substring」是对的。真正在**投递名一侧**的通配符风险是 `_`：16 个名字里 3 个 `pull_request.*` 带下划线，`_` 在 LIKE 里匹配任意单字符 ⇒ 订阅串写成 `pullrequest.opened` 也会命中；订阅串里的 `%`/`_` 落在左值、是字面量，只会永不命中，不构成风险。

## 3. 未决决策（L2，本流程不动代码）

| 编号 | 决策点 | 现状锚点 | 影响面 |
|---|---|---|---|
| R15-1 | 事件表与信封：新建 `event_outbox`（含 `scope_level/owner_id/repo_id/actor/occurred_at`）还是扩 `repo_events` | `repo_event_ops.rs:8-22`、`entities/repo_event.rs` | 新迁移 + 双写期 + 前端/长轮询契约 |
| R15-2 | 乱序提交的定序机制：`id_hi` 双列（提交后才填）vs 事务级串行化 | `repo_event_ops.rs:52` `list_after` | 读端查询与 `next_cursor` 语义；三后端可用性 |
| R15-3 | ops 层收敛：498 处 `&DatabaseConnection` → `C: ConnectionTrait` 的批次与顺序（是否先做 `audit/notification/repo_event` 三处） | 615/117/498（§2.1） | 全 workspace 签名、`rg-ssh`/`rg-http` 调用点 |
| R15-4 | 无业务事务的事件（git push、CI 触发）走 `operations` 预插 + 对账，还是先接受「尽力而为」 | `rg-http/src/lib.rs:3463`、`trigger_event:131` | 与路线图 §4.3 operation 的先后顺序 |
| R15-5 | scope 模型：一条事件一行 scope（跨 scope 复制）vs 单 scope 字段 + join | `CLAUDE.md:194`（多实例进程内状态登记） | 权限实时过滤；实例流是否会成为越权读口子 |
| R15-6 | SSE 与 MCP `resources/subscribe` 的落地顺序（依赖 R15-2） | `rg-mcp/src/server.rs:41` `subscribe: false` | 读端正确性依赖定序，先做会返工 |

> 表内只列到 R15-6（与 `ironforge-docs/README.md` 的「六个」对齐）；账本另有 **R15-7**（`webhook_ops.rs:43` 的 LIKE 匹配语义，改逗号包裹/关联表）、**R15-8**（`push` 同名两套 payload，见 §1.5）与 **R15-9**（事件日志 16 KiB 截断，见 §1.1），登记在账本、未进本表，避免单方面改动本表的条数口径。

## 4. 本文不涉及

- 不改任何 schema、不改 ops 签名、不新增端点、不改 `webhook_ops` 匹配语义。
- 路线图 §4.5 的目标语义（至少一次、去重、`CURSOR_EXPIRED`）保持原样；本文只补充「现状靠什么成立」与「实现前必须先定什么」。
