# T05 Task 确认 — R10 配额与用量（最小闭环）

> 权威状态在 `tasks/T05.md`；本文件记录约束、路径与风险确认。执行细节见 `docs/plans/T05-plan.md`。

## 1. 约束确认

| 约束 | 结论 |
| --- | --- |
| 只做「每 agent 每仓库 CI 并发数」+「每日 CI 分钟数」 | 是。两个 metric，落 `usage_counters`，触发时硬限制 |
| 不加任何 REST 查询端点 | 是。未碰 `rg-http/src/lib.rs`、`openapi.rs`、`route_meta.rs` |
| 不复用 `collab_task_budget` / `collab_task_usage` | 是。另起平台侧 `usage_counters`，不复用、不迁移 |
| 表名显式、`up` 幂等、`down` 正确删除 | 是。显式 `usage_counters`；`has_table`/`has_column`/`has_index` 三重守卫；`down` 真 drop |
| 迁移回滚步数同步 | 是。`rg-db/src/lib.rs` 的 `Migrator::down(&db, Some(26))` → `Some(27)`，注释里的 100/26 同步为 101/27 |
| 限制点在创建 pipeline 之前 | 是。`trigger_pipeline` 读配置并校验语义后、现有 concurrency 组之前 |
| 上限集中一处 | 是。全部在 `crates/rg-ci/src/quota.rs` 的两个 `pub const` |
| 记账幂等、独立短事务、不进主事务 | 是。绝对重算 + 唯一索引 upsert，每条记录一个短事务，事务首语句即写 |
| 只对 `users.kind = "agent"` 生效 | 是。人类与无主体（push hook）直接放行且不写行 |
| 不改 CI `concurrency` 配置语义 | 是。`config.rs` 一行未动 |
| 禁止改 `Cargo.*` | 是。未新增依赖（`chrono`、`sea-orm`、`rg-db`、`rg-core` 均为既有依赖） |
| 不 push main | 是。只推 `agent/t05-ci-quota` |

## 2. 触碰路径

生产代码（全部在 T05 owned paths 内）：

- `crates/rg-db/src/migrations/m20261114_000001_create_usage_counters.rs`（新建）
- `crates/rg-db/src/migrations/mod.rs`（注册模块与迁移）
- `crates/rg-db/src/entities/usage_counter.rs`（新建）
- `crates/rg-db/src/entities/mod.rs`（注册实体；`entities/` 下新增文件属 owned path 的自然延伸）
- `crates/rg-db/src/lib.rs`（仅回滚步数与相邻注释）
- `crates/rg-ci/src/quota.rs`（新建）
- `crates/rg-ci/src/lib.rs`（`pub mod quota;`、`enforce`、两处记账接入）

测试：

- `crates/rg-ci/tests/quota_backends.rs`（新建，跨后端；读 `IRONFORGE_TEST_DATABASE_URL`，缺省用临时 SQLite）
- `crates/rg-ci/src/quota.rs` 内的 `#[cfg(test)] mod tests`（5 项）
- `crates/rg-db/src/migrations/m20261114_000001_create_usage_counters.rs` 内的 `#[cfg(test)] mod tests`（2 项）

> 说明：`crates/rg-ci/tests/` 不在 `tasks/T05.md` 的字面 owned paths 列举里，但它是本任务新 SQL 的跨后端落点，不与 T04 或共享配置冲突（`Cargo.toml` 未改，测试只用既有依赖）。

未触碰：`crates/rg-http/src/lib.rs`、`crates/rg-http/src/openapi.rs`、`crates/rg-core/src/auth/route_meta.rs`、`CLAUDE.md`、`ironforge-docs/`、`Cargo.toml`、`Cargo.lock`、`.cnb.yml`、`.github/`、`scripts/`、`deploy/`、`crates/rg-ci/src/runner.rs`、`crates/rg-ci/src/config.rs`。

## 3. 依赖

`Depends on: none`。只用既有 API：`rg_db::ops::pipeline_ops` / `user_ops` / `repo_ops`、`rg_core::agent::is_agent`、`rg_core::error::CoreError`、`rg_db::entities::{pipeline, pipeline_job, usage_counter}`。

## 4. 风险确认

| 风险 | 处置 |
| --- | --- |
| 迁移不可逆 | `down` 真删除；三种方言 DDL 单测 + 「up→down→up」单测 + `Some(27)` 回滚守卫实测通过；`up` 对已有表逐列 `has_column` 修复 |
| push 触发拿不到主体 | 如实降级为「不阻断、不计数」，在 plan §6 与 §10 写明；需要改禁改路径才能开启 |
| 409 稳定码无法进 `error.code` | 状态码 409 + message 稳定前缀；偏差与后续任务写在 plan §5.1 |
| 记账失败影响 CI | 所有记账调用只 `warn`，绝不把已完成的 pipeline 变成错误 |
| 重复记账 | 绝对值重算 + 唯一索引，单测与跨后端测试均重复记账验证 |
| 延迟记账（外部 runner / 取消） | 决策前必重算，配额判定不受影响；plan §4.3 列明覆盖矩阵 |
| 跨后端解码（PG INTEGER 历史列） | 实测暴露并修正：改用 `pipeline_ops` 的 PG 投影查询 |
