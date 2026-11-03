# 同账号多 Agent 协作首版：验证记录

> 日期：2026-11-04。范围：[协作首版设计](multi-agent-collaboration-design-2026-11.md) 的 C01–C06 与 T01–T12。
> 本文按[证据分层](../../CLAUDE.md)逐层陈述，**前一层不能替代后一层**。所有结果来自本次会话中实际执行的命令；没有执行的写在「未验证」。

## 1. 被验证的对象

- 基线：分支 `feat/harness-ci-feedback`，HEAD `7fd3a51`（该提交已在本次实施过程中由他人带入了 C01–C03 的中间版本，见 §6）。C03 之后的全部改动（C04、C05、C06、测试与文档）在**工作区，未提交**。
- 因此“被测代码”= `7fd3a51` + 工作区未提交改动。没有针对某个干净 SHA 的验证；提交后应在 CI 上复跑。
- 环境：macOS 27.2 arm64，rustc 1.99.1，git 2.54.0。
- 数据库：SQLite 3.54（sqlx 内置）；**PostgreSQL 17.11**（`postgres:17` 容器，本机 127.0.0.1:55432）；**MySQL 8.4.13**（`mysql:8.4` 容器，本机 127.0.0.1:53306，`log_bin_trust_function_creators=1`，与 CI 一致）。容器仅用于本次验证，一次性数据。

## 2. 任务（C）状态

| 任务 | 实现 | 任务验证 | 集成验证 | 运行验证 |
|---|---|---|---|---|
| C01 迁移/实体/ops | ✅ | ✅ 三后端 | ✅ 三后端迁移+升级+回滚 | — |
| C02 会话 | ✅ | ✅ | ✅ 三后端 REST | ✅ T12 |
| C03 任务/消息 | ✅ | ✅ | ✅ 三后端 REST | ✅ T12 |
| C04 交接/已读 | ✅ | ✅ | ✅ 三后端 REST | ✅ T12 |
| C05 MCP | ✅ | ✅ 假 API + 真二进制 | — | ✅ T12（真服务器） |
| C06 端到端/文档 | ✅ | — | — | ✅ 仅 SQLite |

“集成验证”指任务分支（工作区）上的命令通过；**不是**合并后的基线；CI 尚未运行。

## 3. 验收（T）矩阵

| T | 场景 | 通过的测试（文件::测试） | 层级 / 后端 |
|---|---|---|---|
| T01 | 同账号两会话，同名 | `collab_session_tests::t01_same_account_two_sessions_with_identical_names_stay_distinct`、`t01_registration_validates_labels`；存储层 `collab_store::same_display_name_gets_distinct_sessions` | REST + 存储 / 三后端 |
| T02 | 错误密钥、他账号、他仓库、结束/过期、撤权 | `t02_wrong_unknown_foreign_and_cross_repo_credentials_are_indistinguishable`、`t02_another_account_cannot_take_over…`、`t02_registration_and_discovery_follow_repository_permissions`、`t02_revoked_write_access_blocks_the_next_request_despite_a_warm_permission_cache`、`t02_ended_and_expired_sessions_cannot_be_revived` | REST / 三后端 |
| T03 | 续接、丢密钥、离线 | `t03_original_credentials_reconnect_until_hard_expiry_and_offline_is_not_ended`、`t03_lost_secret_means_a_new_session…`；MCP：`collab_mcp::t11_restart_*` | REST + MCP / REST 三后端 |
| T04 | 同版本并发 PATCH | `collab_task_message_tests::t04_concurrent_patches_of_one_version_have_a_single_winner`；存储 `collab_store::concurrent_task_updates_with_one_version_have_a_single_winner` | REST + 存储 / 三后端 |
| T05 | 并发/重试幂等 | `t05_concurrent_and_retried_sends_create_one_message`；存储 `concurrent_same_client_message_id_creates_exactly_one_message`；交接 `idempotent_handoff_creation` | REST + 存储 / 三后端 |
| T06 | 提交顺序、分页并发追加、重启续读 | `t06_pollers_see_every_message_once_in_order_across_a_restart`；存储 `concurrent_appends_are_visible_in_sequence_order_without_gaps` | REST + 存储 / 三后端 |
| T07 | accept/cancel 竞争 | `collab_handoff_tests::t07_concurrent_accept_and_cancel_have_exactly_one_winner`；存储 `concurrent_accept_and_cancel_leave_exactly_one_terminal_state` | REST + 存储 / 三后端 |
| T08 | 已读水位 | `t08_watermarks_are_monotonic_bounded_independent_and_durable`；存储 `read_watermark_is_monotonic_bounded_and_per_session` | REST + 存储 / 三后端 |
| T09 | 密钥不泄漏 | `collab_secret_leak_tests::t09_session_secret_never_leaves_the_registration_response`（响应、TRACE 日志、数据库全表）；MCP `t09_t11_secret_is_used_but_never_shown`；T12 中扫描服务端 debug 日志与 MCP 进程日志 | REST + MCP + 真实运行 |
| T10 | 跨仓库/无效/超限 | `t10_invalid_and_cross_repository_input_is_rejected_without_partial_writes`、`t10_invalid_and_cross_repository_handoffs…`、`t10_real_issue_and_real_commit_references…`；存储 `cross_repo_references_are_rejected_by_the_database` | REST + 存储 / 三后端 |
| T11 | MCP 参数/错误/调用链/重启 | `rg-mcp/tests/collab_mcp.rs` 全部 10 个 | 真二进制 + 记录式假 API |
| T12 | 真实双客户端闭环 | `scripts/collab-e2e.py` | 真服务器 + 真 MCP + HTTP 客户端 + 真 Git 仓库；仅 SQLite |

## 4. 执行的命令与结果

```bash
# 存储层与迁移（三后端各跑一次）
cargo test -p rg-db                                                # SQLite（含既有 26 个单测）
IRONFORGE_TEST_DATABASE_URL=postgres://…:55432/ironforge cargo test -p rg-db --test collab_store
IRONFORGE_TEST_DATABASE_URL=mysql://…:53306/ironforge     cargo test -p rg-db --test collab_store
# 迁移：全新库 → 回滚最新迁移 → 升级 → 重复升级（各用独立数据库）
cargo test -p rg-db --test collab_migration -- --ignored           # 三后端各一次

# REST（同一套测试，通过 IRONFORGE_TEST_DATABASE_URL 切换后端）
cargo test -p rg-http --test collab_session_tests --test collab_task_message_tests \
  --test collab_handoff_tests --test collab_secret_leak_tests

# MCP
cargo test -p rg-mcp

# 真实运行
cargo build -p rg-cli -p rg-mcp && python3 scripts/collab-e2e.py
```

结果（均为本会话实测）：

| 范围 | SQLite | PostgreSQL 17.11 | MySQL 8.4.13 |
|---|---|---|---|
| `rg-db` 存储层 `collab_store` | 14/14 | 14/14 | 14/14 |
| `rg-db` 迁移升级/回滚/重复应用 `collab_migration`（`--ignored`，独立库） | 1/1 | 1/1 | 1/1 |
| `rg-http` 协作 REST（会话 10、任务消息 8、交接已读 8、泄漏 1） | 27/27，连续 5 次均通过 | 27/27 | 27/27 |
| `rg-mcp`（单测 5 + 真二进制集成 10） | 15/15 | — | — |
| `collab-e2e.py`（T12） | 通过（共运行 3 次） | 未运行 | 未运行 |

- `cargo clippy --workspace --all-targets -- -D warnings`：通过（与 CI 相同的命令）。
- `cargo test --workspace --no-fail-fast`（SQLite）：766 通过、1 失败、11 忽略（忽略项是需要外部数据库的 `--ignored` 测试）。唯一失败是 `rg-http` 的 `openapi_registration_tests::every_annotated_handler_is_registered_in_openapi`：`releases.rs::get_release_by_tag`（随提交 `7fd3a51` 的“release 按 tag 查询”进入，不属于协作改动）有 `#[utoipa::path]` 但未登记到 `openapi.rs`。随后应要求在 `openapi.rs` 补登记这一行，重跑 `openapi_registration_tests` 与 `openapi_docs_auth_tests`（6/6）和 `cargo clippy -p rg-http --all-targets -D warnings` 均通过；**修复后没有重跑整个 workspace**。
- 其间还观察到：变异检查（去掉缓存失效）使撤权测试失败，还原后通过。

## 5. T12 真实闭环（运行验证）

`scripts/collab-e2e.py` 启动真实 `ironforge serve`（SQLite 临时库、`RUST_LOG=debug`），同一账号两个互不相干的客户端：

- **客户端 A**：真实 `ironforge-mcp` 进程（stdio），用 `repo` scope 的 PAT；
- **客户端 B**：纯 HTTP 客户端，用登录 JWT，自己保管会话密钥。

流程与断言：注册会话 → A 发现 B → A 创建并认领任务（关联真实 Issue、声明分支/路径）→ A 带真实提交 SHA 发 note、向 B 发 blocker，重试同一 `client_message_id` 不重复；虚假 SHA 被拒 → B 自行发现任务、读取消息、标记已读、回复、置 `blocked`，过期版本被 409 拒绝并返回当前版本 → **A 的 MCP 进程被 `kill`，服务整体重启** → A 用持久凭据文件续接**同一会话**（不重复注册），只读到漏掉的 `seq 3..4` → A 创建交接，自己不能接受（403），B 接受（v2），之后取消被 409 拒绝；接受交接不改变任务归属/版本 → B 显式认领并声明 `done` → 全程无需人工复制聊天；历史有序无间隙；所有消息 `origin=user_content`；SHA 作为随消息的声明保存。最后扫描服务端 debug 日志（约 0.5 MB）和两份 MCP 进程日志，未发现任何会话密钥。

脚本共运行 3 次，均通过（第二次起开启服务端 debug 日志，使泄漏扫描有意义）。步骤摘要：

```
[01] server started — http://127.0.0.1:63951 (sqlite, Darwin arm64)
[02] one account, two credentials — client A: repo-scoped PAT; client B: login JWT
[03] real git repository populated through the server — commit e10fc609d64e
[04] B: registered session over REST — 270fb5b2-ee1e-41fb-9e7e-49d06588a988
[05] A: started session through MCP; secret held by the client, not the model — 84715ddd-9598-4d4d-b739-796502a2a83a
[06] A: discovered B — Harness B (online)
[07] A: created and claimed a task — 32492eee-0b77-4840-8693-1ae9ee1e1cea v1
[08] A: a commit claim is checked for existence only — bogus SHA rejected; real SHA stored as a claim
[09] A: posted a blocker for B; a retry did not duplicate it — seq 2
[10] B: discovered, read, replied, changed status; stale write refused — task v2
[11] A: MCP process killed (crash)
[12] server restarted — same database
[13] A: resumed after crash+restart, read the missed messages — seq 3..4, cursor 4
[14] A handed over, B accepted; the handoff alone changed nothing else — handoff 538f1d14-944f-4222-b8b0-95c3ec2244e9 accepted
[15] B: claimed the task and declared it done — a declaration, not a verification
[16] both sessions ended — history remains readable
[17] no session secret in the server log or MCP process logs — 3 logs scanned (server log=556900B, mcp-2.stderr=17046B, mcp-1.stderr=13258B)
[18] T12 scenario complete
```

“SHA 与声明分开”的含义：服务端只确认该提交对象存在于此仓库；是否已合并、是否通过 CI、是否被评审，协作记录里**没有**也不声称。

## 6. 过程中发现并修复的缺陷（来自实测，非推测）

1. **MySQL 专有**：`OnConflict::do_nothing()` 渲染为 `INSERT IGNORE`，MySQL 报 1064；SQLite/PostgreSQL 正常。已改为“条件 UPDATE + 普通 INSERT + 唯一冲突视为被抢先”。若只在 SQLite 上测试不会发现。
2. `rg-db` 既有测试 `upgrade_keeps_child_rows_triggers_and_foreign_keys` 的回滚窗口计数需随新增迁移 +1（该测试自带注释要求），已由 6 改为 7。
3. MCP：`collab_start_session` 在已有存活会话时先“续接”再校验参数，导致缺参数的调用被接受；已改为先校验。
4. 仓库守卫 `rg-git::cli_gateway::test_no_raw_git_command_in_crates` 禁止在任何 `.rs` 中直接 `Command::new("git")`；测试辅助改用 `global_gateway()`。
5. 变异检查：临时去掉权限缓存失效调用后，撤权测试按预期失败（200≠403），已还原。

此外，第一次 `cargo test --workspace` 因上述守卫失败而在 `rg-git` 处中止（不带 `--no-fail-fast` 时 cargo 遇到失败的测试二进制即停止），后续 crate 未运行；修复后改用 `--no-fail-fast` 重跑。

**需要知情的一件事**：在本次实施期间，另一方（作者 `yuqu`，不是本次会话）创建了提交 `7fd3a51`，标题为“release 按 tag 查询、refs/pull/<n>/head、OpenAPI 版本取包版本”，但其内容同时包含 C01–C03 的当时版本（迁移、实体、ops、协作服务与 REST、部分测试，共 45 个文件）。本次会话没有提交、没有改写历史，也没有触碰他人的未跟踪文档。提交信息与内容不符，建议提交者自行决定是否拆分。

## 7. 未验证与已知限制

- **CI 未运行**：本地的 `cargo clippy --workspace --all-targets -D warnings` 与 `cargo test --workspace` 见 §4，但没有 GitHub Actions 结果；`regression.yml` 的 PostgreSQL/MySQL 作业尚未纳入协作测试（目前协作测试靠 `IRONFORGE_TEST_DATABASE_URL` 手动切换）。
- **真实运行（T12）只覆盖 SQLite**；PostgreSQL/MySQL 上是 REST 集成与存储层测试，没有真实服务器二进制 + MCP 的端到端。
- 并发测试是在单机上通过多请求/多连接制造竞争，不是跨机器多实例；**多实例部署未验证**。权限缓存的跨实例延迟对其余端点依旧存在；协作端点每次从数据库判权，因此不受影响，但这是在单实例内验证的结论。
- 未做性能/容量测试；会话数量不设上限；列表 `limit` 上限 100。
- 没有审计记录、没有保留期与清理、没有推送；未实现后续产品需求（收件箱、问题/答复、依赖等）。
- MySQL 时间戳精度为秒（实测 `datetime_precision=0`），同一秒内的会话/任务/交接列表顺序按 id 决定；PostgreSQL 为微秒。
- 读取可见性（设计 §8.1）仍是产品待决项：当前公开仓库对任何**已登录**读者可见协作内容。
- 会话密钥文件（`IRONFORGE_COLLAB_SESSION_FILE`）以 0600 写入，但如何由各宿主安全分发/加密仍是宿主责任（设计 §8.2）。
- Windows 未验证（凭据文件权限的 0600 仅在 unix 上设置）。
- 没有浏览器/UI 验证：本次没有 Web 界面改动。
