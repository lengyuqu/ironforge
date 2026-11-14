# T03 Task 确认 — R3-P1 事件信封与投递头

> 权威状态在 `tasks/T03.md`；本文件记录约束、路径与风险确认。执行细节见 `docs/plans/T03-plan.md`。

## 1. 约束确认

| 约束 | 结论 |
| --- | --- |
| 只做 R3-P1（信封 + 投递头 + 5xx 重试） | 是。P2（事务内写入）、P3（outbox）未做 |
| P1 必须能不碰事务完成 | 是。`trigger_event` 内部包一层即可，业务事务一行未改 |
| `ActorRef` 复用 `rg_core::agent::ActorRef` | 是，未新造类型、未改 `agent.rs` |
| 不得新增依赖 | 是。`Uuid`（`uuid`）、`chrono`、`serde_json` 均为既有依赖；`Cargo.toml` / `Cargo.lock` 未改 |
| 旧字段必须留在 `data` | 是，逐字保留，键名/嵌套不变 |

## 2. 触碰路径

生产代码（全部在 T03 owned paths 内）：

- `crates/rg-core/src/webhook/service.rs`
- `crates/rg-http/src/api/repo_events.rs`

测试（新增/调整；均为测试文件，无生产影响）：

- `crates/rg-core/tests/webhook_envelope_tests.rs`（新增，11 项）
- `crates/rg-http/tests/collab_webhook_tests.rs`（4 处断言改读 `data`，并新增信封断言）

未触碰：`crates/rg-http/src/lib.rs`、`openapi.rs`、`route_meta.rs`、`ws.rs`、`Cargo.toml`、`Cargo.lock`、迁移目录、`CLAUDE.md`、`ironforge-docs/`、`.cnb.yml`、`.github/`、`scripts/`、`deploy/`。

> 说明：新增的 rg-core 集成测试文件不在 `tasks/T03.md` 的字面 owned paths 列举里，但它是本任务的测试落点，且不与 T01/T02 或共享配置冲突。选择独立二进制而非 lib 单元测试，是因为投递测试需要进程级 `IRONFORGE_ALLOW_LOCAL_OUTBOUND=1`，放在 lib 测试里会污染 `net_guard` 模块"loopback 必须被拒"的单元测试。

## 3. 依赖

`Depends on: none`。只用到既有 API：`rg_db` 的 webhook / repo_event / background_job ops、T26 落地的 `agent::ActorRef` 与 `resolve_actors`、QUEUE-001 的 `BackgroundJobQueue`。

## 4. 风险确认

| 风险 | 处置 |
| --- | --- |
| webhook payload 是外部契约 | 计划文档 §4 逐条列出新增键、下移字段、破坏面与缓解；未做双写（会产生"以哪份为准"的歧义） |
| 5xx 改为重试会产生重复投递 | 文档说明用 `X-IronForge-Delivery`（每次尝试不同）或信封 `id`（同一事件相同）去重 |
| `deliver_retry_job` 的返回语义变化 | 5xx/429 返回 `Err` 以复用既有退避与死信，4xx 仍 `Ok`；已有 `background_job_tests` 三项仍绿 |
| `redeliver` 二次包裹 | `looks_like_envelope` 判定后原样复用；有专门测试 |
| 事件日志截断吞掉信封 | 与旧行为一致（超限写存根）；读端对该行回 `envelope: null`，已在文档记录 |
| actor 仅"尽力而为" | 找不到账号即 `null`，不伪造；P2/P3 可改为显式传参 |
| 本机验收不等于 CI | 三条验收命令在本机真实跑过；SQLite 之外的库未跑（P1 无迁移、无 SQL 变更，风险低） |
