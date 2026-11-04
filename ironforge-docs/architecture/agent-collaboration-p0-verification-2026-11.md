# 多 Agent 协作 P0 子集：实现与验证记录

> 日期：2026-11-04。范围：[运维需求](agent-collaboration-operability-requirements-2026-11.md)与 [P0 设计](agent-collaboration-p0-design-2026-11.md)中按评审结论收敛的最小子集。
> 按[证据分层](../../CLAUDE.md)逐层陈述，**前一层不能替代后一层**。未执行的写在「未验证」。

## 1. 与设计的取舍（及原因）

| 设计 | 实际 | 原因 |
|---|---|---|
| 全局幂等表 `collab_request_dedup` | 每个对象自带 `client_*_id` 唯一键 + `payload_hash` | 沿用首版交接/消息的做法，少一张长期表；同键同体回放 200，异体 409 `IDEMPOTENCY_KEY_REUSED` |
| 收件箱快照存储 + 游标 | **实时查询**，无快照、无游标、无已读标记；`since` 水位由客户端持有 | 避免第三套长期事件/快照存储；响应显式标 `consistency: live_query` 与 `truncated` |
| 可见性“仓库读者可读” | **所有者 / 任意级别协作者 / 组织成员 / 写者可读**；公开仓库的普通登录用户 403 | 问题、工作包比消息更可能含敏感内容；只收紧 `require_collab_read`，**未改** `can_read_repo/can_write_repo` |
| O04 用户自定义条件 + 评估快照 | 只读 `completion-status`，条件由现有记录**推导**（问题已解决 / 有工作包 / 该 commit 的 CI） | 自定义条件需要独立数据模型；推导式无需迁移，且不阻止 `done` |
| CI 证据 | 仅 `pipelines`（CI 引擎写入）；**不采信 commit status** | 任何写者（含被评估的 Agent）都能 POST commit status |
| 工作包按 revision 降序 | 按 `(created_at,id)` 降序 | 复用现有游标；revision 仍唯一且递增 |
| O03a | 删除影响**只读计数**（仓库 / 账号）+ 备份覆盖盘点（§4） | 不执行删除、不执行恢复 |

**未做**：MCP 工具、Web UI、`inbox_reads`、依赖图 / 决定 / 回执 / 通知（P1）、P2、O03b 隔离恢复演练、O02 总览、O04 自定义条件与强制模式、`paths_declared_at`。

## 2. 实现证据（文件 / 接口）

- 迁移 `m20261104_000002_create_collab_p0`（只建新表 `collab_takeovers / collab_questions / collab_answers / collab_work_packages`，`has_table` 守卫，`down` 先删子表再删父表，复合外键保证同仓库）。
- REST（前缀 `/api/v1/repos/{owner}/{name}/collaboration`，均已登记 OpenAPI）：`GET /rules`、`POST /tasks/{id}/takeovers`、`POST /tasks/{id}/reassign`、`GET /tasks/{id}/takeovers`、`POST|GET /tasks/{id}/questions`、`GET /questions/{id}`、`POST /questions/{id}/answer|cancel`、`POST|GET /tasks/{id}/work-packages`、`GET /work-packages/{id}`、`GET /inbox`（需会话凭据）、`GET /inbox/user`、`GET /tasks/{id}/completion-status`、`GET /deletion-impact`（仓库管理员）、`GET /admin/users/{id}/collaboration-impact`（站点管理员）。
- 稳定错误码：`OWNER_MAY_BE_ACTIVE`、`TASK_DONE`、`QUESTION_NOT_OPEN`、`CONTEXT_STALE`（均带 `error.details`），沿用 `VERSION_CONFLICT`、`IDEMPOTENCY_KEY_REUSED`、`SESSION_EXPIRED`。
- 新增依赖：`rg-core` 已有 `toml`；`rg-http` 无新增依赖。

## 3. 验证

| 层级 | 结论 | 证据 |
|---|---|---|
| 任务验证（SQLite） | ✅ | `collab_p0_tests` 19 项、`collab_rules_tests` 7 项、`collab_visibility_tests` 1 项；`rg-core` 单测（rules 4、inbox 2、completion 3） |
| 既有协作测试回归 | ✅ | `collab_session / task_message / handoff / secret_leak` 全部通过（可见性收紧后无需改动） |
| 门禁（本地） | ✅ | `cargo clippy --workspace --all-targets -- -D warnings` 无告警；`cargo check --workspace --all-targets` 通过；`rg-http --lib`（含 OpenAPI 守卫）通过 |
| 新库迁移 | ✅ | 空 SQLite 上 `ironforge migrate`：新增 4 张表，表名与实体一致（踩坑 #13）；重复执行 “No pending migrations” |
| 并发 | ✅ SQLite | 5 路并发接手只有 1 个成功且历史一致；回答/取消竞争恰有一个终态；6 路并发工作包得到 revision 1..6；重复 6 轮稳定 |
| 全量 workspace 测试（SQLite） | ✅ | `cargo test --workspace --no-fail-fast`：92 个测试目标，803 通过 / 0 失败（§5） |
| 合并后集成 / CI | ❌ 未验证 | 尚未推送，GitHub Actions 未运行 |
| PostgreSQL / MySQL | ❌ 未验证 | 本机测试容器未运行（127.0.0.1:55432 / 53306 不可达）；迁移与并发约束（唯一键、条件 UPDATE）未在这两个后端实测。`IRONFORGE_TEST_DATABASE_URL` 可复用同一套测试 |
| 真实运行（真服务器 + MCP + 两客户端） | ❌ 未验证 | `scripts/collab-e2e.py` 尚未扩展到新接口；MCP 没有对应工具 |
| 浏览器 / UI | ❌ 未验证 | 无 UI |

### 验证中发现并修复的缺陷

1. **并发 500**：接手事务先 `SELECT` 后 `UPDATE`，SQLite 读快照无法升级写锁，竞争时 `SQLITE_BUSY`。改为回放检查放事务外、写语句作为事务第一条（见 CLAUDE.md 踩坑 #21）。
2. **重试被误拒**：成功的接手重试先撞上 “session already owns this task”。现在幂等回放先于前置条件检查。
3. **设计前提错误**：见 §4。

## 4. O03a：删除与备份审计（事实）

- **仓库删除是软删除**：行保留，协作记录**不会**被删除；Git 目录移到 tombstone，`IRONFORGE_DELETED_REPO_RETENTION_DAYS`（默认 30）后清理 Git 数据，行永不清。因此协作记录里的 commit id 在保留期后不可解析。只有直接移除仓库**行**才外键级联删除全部协作表（测试 `o03a_repository_delete_is_soft_*` 两种路径都验证）。
- **账号删除是硬删除**：级联到其会话 → 其拥有的任务 → 任务里所有参与者的消息，以及其在他人任务里写的消息。预览与实际删除的差值一致（`o03a_account_deletion_preview_*`：任务 −1、消息 −3、会话 −1）。
- **协作记录没有保留期**，随所挂行存亡。
- **备份**：`ironforge backup-db` / `backup` 仅支持 SQLite（代码明确拒绝其他后端），整库快照包含协作表；全实例备份要求停服务。PostgreSQL / MySQL 没有内置备份入口，需外部手段。**备份能否恢复协作数据：未做恢复演练，未验证。**
- 两个预览接口只计数，不授权、不执行；计数是时间点读取，执行前须重读。

## 5. 全量 workspace 测试

`cargo test --workspace --no-fail-fast` 在含本次全部改动的工作区上：803 通过、0 失败（92 个有结果的测试目标）。

过程中暴露一次真实失败：`rg-db` 的 `soft_delete_tests::upgrade_keeps_child_rows_triggers_and_foreign_keys` 逐步回滚迁移并做 `foreign_key_check`，而新迁移最初的 `down` 为空，回滚首版迁移删掉 `collab_tasks` 后，新表的外键悬空（`no such table: main.collab_tasks`）。修复：新迁移的 `down` 先删子表；该测试注释要求的回滚窗口 `7 → 8` 同步调整。

**被验证的对象**：分支 `feat/harness-ci-feedback`，HEAD `53a6a0e`（含 main 合并）+ **未提交的工作区改动**；没有针对干净 SHA 的验证。环境：macOS arm64，SQLite；未提交、未推送。
