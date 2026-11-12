# 审计 `on_behalf_of_user_id`（T23）验证记录

> **状态**：已实现，本地分支，未经 CI ｜ **可信度**：确认（本机命令输出）｜ **日期**：2026-11-13 ｜ **对应**：CNB issue #33，设计文档 §4 T23

## 做了什么

- 迁移 `m20261110_000002_add_audit_on_behalf_of`：`audit_log.on_behalf_of_user_id BIGINT NULL`（**无外键**，审计行必须比账号活得久）+ 索引 `idx_audit_log_on_behalf_of (on_behalf_of_user_id, id DESC)`。显式表名；`has_table`/`has_column`/`has_index` 守卫；`down` 为空、`up` 幂等。不回填历史行（NULL = 非 agent 或未知）。
- `api/audit.rs::record_write`：复用取 `username` 的那次 `find_by_id`，agent 取 `owner_user_id`，人类与查不到的账号为 NULL；零额外查询。**不**写进 `details`。
- `GET /admin/audit/logs`：新查询参数 `on_behalf_of_user_id`（`audit_log_ops::list_paginated` 随之加参数）；列表与 `GET /admin/audit/logs/{id}` 的响应都带该字段；`user_id` 保留。OpenAPI `params` 同步。归档 NDJSON 直接序列化 `Model`，自动带上新字段。
- `rg-db/src/lib.rs::upgrade_keeps_child_rows_triggers_and_foreign_keys`：`Migrator::down(&db, Some(25))` → `Some(26)`（迁移总数 99 → 100，注释同步）。

## 覆盖边界（须知）

只有走 `record_write` 的写路径（`agents.rs`、`repo_content.rs`、`pulls.rs`、`repo_commits.rs`、`reviews.rs`）会填这一列。另有 5 处自建 `ActiveModel` 的审计点（`repos.rs`、`admin.rs`、`orgs.rs`、`users.rs`、`collab_groups.rs`）以及 `rg_core::audit::{record, service}`、归档测试，**一律写 NULL**：它们要么 agent 本来进不去（`ensure_human`、`/admin`、`/users`），要么本任务按范围约定未改。`collab_groups.rs` 的审计点 agent 可以走到（仓库类 PAT），其行 `on_behalf_of_user_id` 现为 NULL——需要时另开任务把它改成走 `record_write` 的同一逻辑。`collab_groups.rs`、`repos.rs` 等文件本次只各加了一行 `on_behalf_of_user_id: Set(None)`（结构体字面量不得不补）。

## 测试与结果（均为本机，**未经 CI**）

| 命令 | 结果 |
|---|---|
| `cargo clippy --workspace --all-targets -- -D warnings` | 无告警 |
| `cargo test -p rg-db --lib`（含新迁移幂等测试 `applying_twice_keeps_one_column_and_one_index`，含 `upgrade_keeps_child_rows_triggers_and_foreign_keys` 回滚 26 步） | 28 passed（SQLite） |
| `cargo test -p rg-http --test audit_on_behalf_of_tests`（新） | 通过：SQLite、PostgreSQL 17、MySQL 8.4 各一次（各用全新库） |
| `cargo test -p rg-http --test admin_sso_audit_tests --test repo_audit_tests` | 通过（SQLite） |
| `cargo test -p rg-http --test agent_principal_tests --test pr_export_tests --test openapi_docs_auth_tests --test openapi_registration_tests`、`rg-http --lib`、`rg-core audit` | 通过（SQLite） |
| PostgreSQL 17 / MySQL 8.4：`multi_backend_smoke` 迁满 → 删 `seaql_migrations` 里本迁移一行 → 再跑（迁移重放） | 两个后端都通过；`pg_indexes` 里索引为 `(on_behalf_of_user_id, id DESC)`，MySQL `show index` 第二列 Collation `D`；重放后迁移记录各 1 行 |

新测试 `audit_on_behalf_of_tests` 断言：agent 提交 → `user_id = agent`、`on_behalf_of_user_id = owner`；人类提交 → NULL；`details` 不含 on_behalf；管理 API 按 owner 过滤只返回 agent 行、不存在的 owner 得 0 条、既有 `user_id` 过滤不受影响、单条读取带字段；直接删除 agent 与 owner 两个账号后，审计行仍在且可读，两个 id 原样保留。

## 未覆盖

- `scripts/ci-db-tests.sh` 的目标清单未加入 `audit_on_behalf_of_tests`（脚本不在本任务范围）；PG/MySQL 是手工起容器、指向全新库跑的。
- 没有对真实数据库执行 `ironforge migrate`。
- 未做：把 5 处其他审计点改成填这一列、`rg_core::audit` 的 agent 感知、Web UI 展示。
