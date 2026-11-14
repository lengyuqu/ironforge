# T06 Task 确认 — 通用审批原语 · 步骤 1（表 + 状态机 + 求值器）+ 修 PR 评审越权

> 权威状态在 `tasks/T06.md`；本文件记录约束、路径与风险确认。执行细节见 `docs/plans/T06-plan.md`。

## 1. 约束确认

| 约束 | 结论 |
| --- | --- |
| 只做 #19 落地步骤 1 + 越权修复 | 是。未碰 CI 环境审批切表、合并门禁接新表、通知面、`scope_expand` |
| 两张表、照方案字段，不自改模型 | 是。`authorization_requests` / `authorization_decisions`；两处附加（`state_reason` 新列、`authorization_set_id` 摘要而非外键）在 plan §2.3 单独说明理由 |
| 迁移显式表名、`up` 幂等、`down` 真删 | 是。`#[sea_orm(iden = "…")]` 显式复数表名；`has_table`/`has_column`/`has_index` 三重守卫；`down` 按依赖顺序 drop |
| 迁移回滚步数同步 | 是。`rg-db/src/lib.rs` 的 `Migrator::down(&db, Some(27))` → `Some(28)`，注释里的计数同步 |
| 消费点 = 动作执行时，同事务 + `WHERE state='pending'` 守卫 | 是。`consume_guarded` 接受 `&impl ConnectionTrait`，0 行 → 调用方 409；执行失败靠事务回滚退回 `pending`，不可幂等失败调 `revoke` |
| `head_sha` 变更 → `expired`（不删除） | 是。`expire_on_head_sha_change` 批量置状态并写 `state_reason = head_sha_changed`，行保留 |
| 判据统一走 `normalize_commit_sha` | 是。请求入口调用 `rg_core::collaboration::message::normalize_commit_sha`，审批侧不另写短 SHA/大小写处理 |
| 求值器与现状逐条等价 | 是。plan §4.2 表格逐 action 列出「现状代码 → 求值器规则 → 等价性」；隐式集合用谓词在批准时求值，保留 `can_admin_repo` 的 admin 分支与 `is_collab_member` 的绕过缓存语义 |
| 申请人 ≠ 批准人一处判定 | 是。`authorization::ensure_separate_people`，`approve()` 复用；`ci_environments.rs` / `reviews.rs` 现有自批检查本步骤不动（前者不在 owned paths，后者任务明确要求保持） |
| agent 只能申请，approve 一律 403 | 是。`requested_by_kind = agent_session` 的请求在 `approve()` 里先于任何权限判定返回 403，不看 scope、不看是否管理员 |
| 批准时重算 `payload_hash` | 是。`approve()` 用请求行的四个绑定项重算并比对，不一致 409 `AUTHORIZATION_PAYLOAD_CHANGED` |
| `action` 是平台枚举 | 是。`rg_core::authorization::Action::parse` 白名单，未知值 400；`scope_expand` 在枚举内但求值返回 `Unsupported`（步骤 6） |
| 幂等三元组 | 是。唯一键 `(repo_id, requested_by_kind, requested_by_id, client_request_id)`；同 key 同 hash 返回既有行，同 key 不同 hash 409 `IDEMPOTENCY_KEY_REUSED`；键格式复用 `collaboration::validation::client_key` |
| 越权修复是唯一行为变更 | 是。`submit_review` 的 `approve`/`request_changes` 收紧到「写权限或该 PR 当前被请求的评审人」；`comment` 保持只读；`dismiss` 不动（对票数无影响） |
| 只在 owned paths 内改生产代码 | 是，唯一例外见 §2 末尾的测试文件新增说明 |
| 禁止改 `Cargo.*` / 路由注册 / `openapi.rs` / `route_meta.rs` / `CLAUDE.md` / `ironforge-docs/` / `scripts/` / `.cnb.yml` / `.github/` / `deploy/` | 是，全部未触碰 |
| 不 push main，不 force-push | 是。只推 `agent/t06-authorization` |

## 2. 触碰路径

生产代码（owned paths）：

- `crates/rg-db/src/migrations/m20261114_000002_create_authorization_requests.rs`（新建）
- `crates/rg-db/src/migrations/mod.rs`（注册模块与迁移，追加在 `m20261114_000001` 之后）
- `crates/rg-db/src/entities/authorization_request.rs`（新建）
- `crates/rg-db/src/entities/authorization_decision.rs`（新建）
- `crates/rg-db/src/entities/mod.rs`（`pub mod` × 2）
- `crates/rg-db/src/ops/authorization_ops.rs`（新建，含内嵌 `#[cfg(test)] mod tests`）
- `crates/rg-db/src/ops/mod.rs`（`pub mod authorization_ops;`）
- `crates/rg-db/src/lib.rs`（仅回滚步数与相邻注释）
- `crates/rg-core/src/authorization/`（新建：`mod.rs` / `action.rs` / `payload.rs` / `evaluator.rs`）
- `crates/rg-core/src/lib.rs`（仅 `pub mod authorization;`）
- `crates/rg-http/src/api/reviews.rs`（仅 `submit_review` 的权限判定）

测试：

- `crates/rg-http/tests/pr_review_tests.rs`（新建）。验收命令点名了 `--test pr_review_tests`，而基线 `0c870a5` 没有该文件；这是 owned paths 之外唯一新增的文件，理由与覆盖点见 plan §7.3。
- 迁移 / ops / 求值器的单测内嵌在各自新文件里，不新增 crate 级测试文件。

未触碰：`crates/rg-http/src/lib.rs`、`openapi.rs`、`route_meta.rs`、`Cargo.toml`、`Cargo.lock`、`CLAUDE.md`、`ironforge-docs/`、`.cnb.yml`、`.github/`、`scripts/`、`deploy/`、`crates/rg-http/src/api/ci_environments.rs`、`crates/rg-db/src/entities/protected_branch.rs`。

## 3. 依赖

`Depends on: none`。只用既有 API：`rg_db::ops::{repo_collaborator_ops, org_ops, pr_reviewer_request_ops, protected_branch_ops, ci_environment_ops}`、`rg_db::entities::*`、`rg_core::repo::service::{can_admin_repo, can_write_repo, can_read_repo}`、`rg_core::collaboration::{message::normalize_commit_sha, validation::client_key, access::is_collab_member}`、`rg_core::error::CoreError`。无新依赖。

## 4. 风险确认

| 风险 | 处置 |
| --- | --- |
| 迁移不可逆 | `down` 按 `decisions → requests` 顺序真删；三方言 DDL 单测 + 「up→down→up」单测；`rg-db` 的 `upgrade_keeps_child_rows_triggers_and_foreign_keys` 校验回滚步数 28 |
| 「批准后永久开锁」 | 消费必须同事务 `UPDATE … WHERE state='pending'`；单测覆盖「第二次消费 0 行」「执行失败回滚后仍 pending」「不可幂等失败 → revoked」 |
| 求值器把 N12 漏洞固化 | `protected_branch_merge` 的批准人是 `writers` ∪ 显式评审人，**不**并入 collaborators（协作成员含 read 级）；plan §4.2 写明 |
| 越权收紧导致现有测试变红 | 逐条改测试、不放宽实现；受影响清单在 plan §7.2 实跑后登记 |
| 越权收紧误伤合法评审人 | 被显式请求的评审人（含 CODEOWNERS 指派）仍可批；`pr_permission_tests` 的 CODEOWNERS 场景应保持绿色 |
| 跨后端（MySQL/PG/SQLite）差异 | 不用 `ON CONFLICT`；唯一冲突按 `SqlErr::UniqueConstraintViolation` 判定；消费用 `update_many` 的可移植写法；新 SQL 按 `IRONFORGE_TEST_DATABASE_URL` 在 PG 17 与 MySQL 8.4 各跑一次 |
| MySQL 时间戳整秒 | 状态机判定只依赖 `state` 与 `expires_at` 比较，不依赖同秒排序；列表排序不用于门禁 |
| 新表暂无调用点 | 原语 + 求值器有单测覆盖；唯一真实调用点是越权修复，不交付空转但不可测的代码 |
