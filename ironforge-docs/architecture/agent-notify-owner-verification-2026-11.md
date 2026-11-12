# 通知改派给 agent 的所有者（T24）验证记录

> **状态**：本机验证，**未经 CI** ｜ **可信度**：确认 ｜ **日期**：2026-11-13 ｜ **对应 CNB issue**：#35

## 做了什么

- `rg_core::agent::accountable_user(db, user_id) -> CoreResult<Option<i64>>`（`agent.rs`，独立区块，追加式）：人类 → 自己（须启用）；agent → 所有者（经 `owner_cap`，所有者须存在、启用、非 agent）；其余 `None`。返回值永远不是 agent。返回 `CoreResult<Option<_>>` 而非裸 `Option`：数据库错误不应被当成"没人可通知"吞掉，调用方（尽力而为的通知）自己记日志。
- `human_notify::recipients()`：每个候选先映射，**映射后**去重，再套 50 人上限；`can_read_repo`/`can_write_repo` 对映射后的那个人判断。`user` 类收件人是 agent 时通知所有者；已停用的人类现在也不再被通知（此前只检查存在）。
- `human_notify::task_blocked()`：`session.user_id` 过 `accountable_user`，`None` 直接返回空，**没有**回退到 `repo_writers`。
- 调用点 `collab_questions.rs`、`collab_tasks.rs` 未改：函数签名与返回的 `Notified` 不变，WebSocket 推送自然按映射后的人推。

## 邮件守卫（核实结论）

通知路径不发信：`notification::notify` 只写库，`human_notify.rs` 与 `notification/` 内无 `lettre`/`send_mail`/`smtp`。按设计文档的核实，真正发信口（CI 通知、重试 worker、密码重置）都已被"仓库所有者恒为人类"与密码重置拒 agent 挡住。因此**没有**给收件人另加 `kind=human`/邮箱后缀过滤（该路径上零效果）；改由 `accountable_user` 保证"收件人永不是 agent"，并以守卫测试 `assert_no_agent_was_notified`（扫描所有 agent 账号名下的 `collab_%` 通知行必须为零）钉死，任何将来复用该收件人列表的邮件路径同样继承这一性质。

## 测试（`rg-http/tests/collab_human_notify_tests.rs`，新增 7 项，原 10 项不变）

| 场景 | 测试 |
|---|---|
| 问题发给 agent → 所有者收到、agent 没有 | `a_question_addressed_to_an_agent_reaches_its_owner_not_the_agent` |
| agent 提问给 `repo_writers`，所有者与 agent 都是写者 → 所有者只收一条 | `a_question_asked_by_an_agent_to_the_writers_tells_people_only_and_once` |
| 仅 agent 是候选（所有者不在协作者表）→ 映射后所有者仍收到且仅一条 | `an_agent_in_the_writers_list_does_not_take_a_slot_from_its_owner_twice` |
| `task_blocked` 于 agent 会话 → 所有者收到，仓库所有者不收 | `a_blocked_task_of_an_agent_session_tells_the_owner` |
| 所有者被停用 → 无人收到且请求不报错（问题发给 agent、发给 writers、`task_blocked` 三条路径） | `nobody_is_told_when_the_owner_of_an_agent_is_disabled_and_nothing_fails` |
| 人类收件人行为不变；停用的人类不被通知 | `a_deactivated_person_is_not_told_but_other_people_are_unaffected` + 原有 10 项 |
| `accountable_user` 本身的四种结局 | `accountable_user_maps_agents_to_active_people_only` |

每个 agent 用例末尾都跑 `assert_no_agent_was_notified`。**变异检查**：把 `human_notify.rs` 暂时还原为 `HEAD` 版本，新增的 6 项失败（`accountable_user` 单测本身不依赖该文件，仍通过），原 10 项通过；恢复后全绿。

## 本机验证命令与结果

| 命令 | 结果 |
|---|---|
| `cargo clippy --workspace --all-targets -- -D warnings` | 干净 |
| `cargo test -p rg-http --test collab_human_notify_tests`（SQLite） | 17 passed |
| 同上，`IRONFORGE_TEST_DATABASE_URL` → PostgreSQL 17（专属容器与库 `ironforge_t24`） | 17 passed |
| 同上 → MySQL 8.4（专属容器与库 `ironforge_t24`，`--log-bin-trust-function-creators=1`） | 17 passed |
| `cargo test -p rg-http --test agent_principal_tests`（SQLite） | 19 passed |
| `notification_tests` / `issue_assignee_tests` / `issue_reaction_tests` / `mcp_remote_tests`（SQLite） | 全部通过 |
| `cargo test -p rg-core agent` | 3 passed |

PostgreSQL/MySQL 用的是单独起的容器（端口 55435/53309），没有用 `scripts/ci-db-tests.sh`（它占固定端口与容器名，会与其他并行会话冲突）；因此只跑了 `collab_human_notify_tests` 这一个目标，**未**跑该脚本的全套。

## 未覆盖 / 已知边界

- 没跑整个 workspace 的测试；只跑了上表所列目标。
- 被停用的 **agent 本身**作为收件人时仍转给所有者（只看所有者状态，与 `owner_cap` 一致），设计文档未要求不同处理。
- 仍无邮件通知、无通知偏好（路线图第三期）。
- 若所有者被停用，agent 的会话/令牌本身也被拒，故 `task_blocked` 只能由别人（如仓库所有者）把 agent 的任务声明为 blocked 才会走到"无人可通知"这条分支。
- 无新迁移。
