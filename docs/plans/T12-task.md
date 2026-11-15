# T12 Task 确认 — [#19 步骤5] 通知偏好 + 邮件单一出口

> 权威状态在 `tasks/T12.md`；本文件记录约束确认、触碰路径、验收命令真实输出与遗留风险。
> 偏好三级解析、`dispatch` 行为、逐条调用点映射与判断理由见 `docs/plans/T12-plan.md`。

## 1. 约束确认

| 约束 | 结论 |
| --- | --- |
| 表 `notification_preferences`（`user_id` / `repo_id` NULL=全局 / `event_type` / `channel` in_app\|email / `enabled` / `updated_at`） | 是，另加 `id`；无多余列。两个外键都 CASCADE（账号或仓库删除后偏好无意义） |
| 唯一键 `(user_id, repo_id, event_type, channel)` | 是，`uq_notification_preferences_scope`；`repo_id` 可空导致数据库不阻止重复全局行，已在迁移注释与 plan §2 写明，写入走先查后写 |
| 迁移 `m20261115_000002`，`Migrator::down` 29 → 30 | 是，唯一改动 `rg-db/src/lib.rs` 的步数与注释（103→104）；`upgrade_keeps_child_rows_triggers_and_foreign_keys` 通过 |
| 「表里没有记录 = 开启」，不为新用户初始化行 | 是；`GET` 也不写行，`a_fresh_account_sees_every_channel_on_and_no_stored_rows` 断言 `list_for_user` 为空 |
| 只有显式 `enabled = false` 才是关闭 | 是；`enabled = true` 允许在一个全局静音之上重新打开某个仓库 |
| 单一出口 `notification/dispatch.rs`：`NotifyRequest` + `dispatch(db, Option<&Mailer>, req)` | 是；另有 `dispatch_email_only` 与 `effective_matrix`（见 plan §4） |
| 三级查找：repo 覆盖 → 全局默认 → 默认开启 | 是，`notification_preference_ops::effective`；ops 层与 dispatch 层都有测试 |
| 返回 `DispatchOutcome { in_app, email, skipped_by_preference, failed }` | 是，另有 `created: Vec<(user_id, notification_id)>` 保留 WS 推送所需的行 id（plan §4 说明） |
| 邮件失败不能让通知整体失败 | 是；`dispatch` 不返回 `Err`，只 warn + `failed += 1`，`a_failed_email_does_not_lose_the_in_app_row` 钉住 |
| 邮件失败只记 warn 并计入 `failed` | 是 |
| 收敛 `notification::notify` 与邮件调用，只换入口、语义一模一样 | 5 处 `notify` + 3 处 `notify_watchers`（内部）= 8 个通知入口全部改走 `dispatch`；邮件 3 处中 **1 处**改（CI），2 处保留并说明（plan §5.3） |
| 不为传 mailer 改函数签名链，没有就传 `None` | 是；`notify_watchers` 签名不变，所有通知调用点 `mailer = None` |
| 端点数 2（GET 有效偏好含默认推导 / PUT 批量 upsert） | 是，见 §3 |
| 路由三处齐（`lib.rs` / `openapi.rs` / `route_meta.rs`） | 是；`route_metadata_tests` 9 passed |
| 不改 `Cargo.toml` / `Cargo.lock` | 是，无新依赖 |
| 不改 `CLAUDE.md` / `ironforge-docs/` / `.cnb.yml` / `.github/` / `scripts/` / `deploy/` / `crates/rg-mcp/` | 是，全部未触碰 |
| 不顺手做 #15 outbox、#63 符号表、#41 沙箱、CI 环境审批切表 | 是 |
| 不改邮件模板或正文 | 模板未动；CI 邮件正文**文字未改**，内联 `<br/>` 换行（模板会转义 body，见 plan §5.3 / §9 风险 6） |
| 不把 `dispatch` 做成事件总线 | 是：无 webhook、无 ws、无重试队列、无 per-event 策略 |
| 跨后端 PG 17 / MySQL 8.4 各跑一次 | 是，见 §4 |

## 2. 触碰路径

`crates/rg-db`

- `src/migrations/m20261115_000002_create_notification_preferences.rs`（新建）
- `src/migrations/mod.rs`（注册）
- `src/entities/notification_preference.rs`（新建，含 `CHANNEL_*` / `CHANNELS` / `is_known_channel`）
- `src/entities/mod.rs`
- `src/ops/notification_preference_ops.rs`（新建：`effective` / `find` / `upsert` / `list_for_user`）
- `src/ops/mod.rs`
- `src/lib.rs`（仅 `Migrator::down` 步数 29 → 30 与注释）

`crates/rg-core`

- `src/notification/dispatch.rs`（新建：`NotifyRequest` / `DispatchOutcome` / `dispatch` / `dispatch_email_only` / `effective_matrix`）
- `src/notification/mod.rs`（`pub mod dispatch`；`notify` 降级为低层入口并加文档；`notify_watchers` 内部改走 `dispatch`）
- `src/issue/reactions.rs`（2 处调用）
- `src/issue/assignees.rs`（1 处调用）
- `src/collaboration/human_notify.rs`（2 处调用）
- `src/auth/route_meta.rs`（1 行声明 + `/user/` family 规则 1 条）

`crates/rg-http`

- `src/api/notification_prefs.rs`（新建：两个 handler + 校验）
- `src/api/mod.rs`
- `src/lib.rs`（路由段 + CI 邮件调用点）
- `src/openapi.rs`（两个 path）
- `tests/notification_prefs_tests.rs`（新建，9 个用例，含跨后端）
- `tests/route_metadata_tests.rs`（参考表加 `/user/` family 1 行 + 注释）

## 3. 端点

| 方法 | 路径 | 响应 |
| --- | --- | --- |
| GET | `/api/v1/user/notification-preferences` | `200 {default_enabled: true, preferences: [{repo_id, event_type, channel, enabled, explicit}]}`；catalogue 全部事件 × 2 渠道 + 用户存过的仓库覆盖行 |
| PUT | `/api/v1/user/notification-preferences` | body `[{repo_id, event_type, channel, enabled}]`（裸数组）；`200` 同上形状；`400` 未知 channel / 未知 repo / 空 event_type / >200 条 |

鉴权：JWT（cookie 或 Bearer），未登录 `401`。PAT scope：`user`。

## 4. 验收命令真实输出

> 工作目录 `/Users/yuqu/vibeCodeing/ironforge-t12`，提交前（工作树干净以外全部改动已保存）。

### 4.1 `cargo test -p rg-db`

```
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.09s
```
（97 个用例全部通过，含新增 `notification_preference_ops::tests` 6 个、迁移测试 4 个；
`grep '^test result' | awk '{s+=$4}'` 汇总 = 97 passed。）

新增用例逐条：

```
test migrations::m20261115_000002_create_notification_preferences::tests::ddl_uses_the_plural_table_name_and_constraints_on_all_backends ... ok
test migrations::m20261115_000002_create_notification_preferences::tests::down_drops_the_table_and_up_recreates_it ... ok
test migrations::m20261115_000002_create_notification_preferences::tests::up_is_idempotent_and_repairs_a_missing_column ... ok
test migrations::m20261115_000002_create_notification_preferences::tests::down_and_up_round_trip_on_the_backend_from_the_environment ... ok
test ops::notification_preference_ops::tests::nothing_set_means_both_channels_are_on_and_not_explicit ... ok
test ops::notification_preference_ops::tests::a_global_opt_out_is_used_when_no_repository_override_exists ... ok
test ops::notification_preference_ops::tests::a_repository_override_beats_the_global_default ... ok
test ops::notification_preference_ops::tests::preferences_are_per_event_type ... ok
test ops::notification_preference_ops::tests::upsert_updates_instead_of_duplicating ... ok
test ops::notification_preference_ops::tests::a_global_and_a_repository_row_coexist ... ok
```

### 4.2 `cargo test -p rg-core`

```
test result: ok. 377 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 13.74s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 0 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.98s
test result: ok. 1 passed; 0 failed; 5 ignored; 0 measured; 0 filtered out; finished in 0.09s
```

新增 `notification::dispatch` 11 个用例：

```
test notification::dispatch::tests::no_preferences_means_both_channels_are_on_and_the_row_is_written ... ok
test notification::dispatch::tests::a_muted_in_app_channel_writes_no_row ... ok
test notification::dispatch::tests::a_repository_override_beats_the_global_preference ... ok
test notification::dispatch::tests::a_muted_email_channel_still_writes_the_in_app_row ... ok
test notification::dispatch::tests::an_enabled_email_is_attempted ... ok
test notification::dispatch::tests::a_failed_email_does_not_lose_the_in_app_row ... ok
test notification::dispatch::tests::a_missing_mailer_is_silent ... ok
test notification::dispatch::tests::every_recipient_gets_its_own_row_and_id ... ok
test notification::dispatch::tests::email_only_writes_no_row_and_still_obeys_the_email_preference ... ok
test notification::dispatch::tests::email_only_without_a_mailer_is_silent ... ok
test notification::dispatch::tests::the_matrix_expands_the_defaults_and_lists_repository_overrides ... ok
```

### 4.3 `cargo test -p rg-http`

```
test result: ok. 106 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.11s
...  (120 个 target 全部 ok，汇总 867 passed；含 notification_prefs_tests 9 / route_metadata_tests 9)
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.73s
```

（`cargo test -p rg-http 2>&1 | grep '^test result' | awk '{s+=$4}'` = 867 passed；无 `FAILED`。）

新增端点用例：

```
test preferences_require_authentication ... ok
test a_fresh_account_sees_every_channel_on_and_no_stored_rows ... ok
test a_mute_is_stored_and_reflected_in_the_effective_view ... ok
test a_repository_override_is_reported_next_to_the_global_default ... ok
test an_unknown_channel_is_a_bad_request ... ok
test an_unknown_repository_is_a_bad_request_not_a_server_error ... ok
test an_empty_event_type_is_a_bad_request ... ok
test preferences_are_scoped_to_the_caller ... ok
test preferences_round_trip_on_the_backend_from_the_environment ... ok
```

### 4.4 `cargo clippy --workspace --all-targets -- -D warnings`

```
    Checking rg-http v0.2.0 (/Users/yuqu/vibeCodeing/ironforge-t12/crates/rg-http)
    Checking rg-cli v0.2.0 (/Users/yuqu/vibeCodeing/ironforge-t12/crates/rg-cli)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.61s
```

### 4.5 `scripts/check.sh --quick`

```
==> cargo fmt --all --check
    (1s)

==> cargo clippy --workspace --all-targets -D warnings
    (...)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 15.34s
    (16s)

==> checks passed in 17s (quick)
```

### 4.6 跨后端（PG 17 / MySQL 8.4）

容器：`postgres:17`（`127.0.0.1:55432`）、`mysql:8.4`（`127.0.0.1:53306`，`--log-bin-trust-function-creators=1`，
既有 FTS 触发器需要）。

| 后端 | 迁移 up→down→up | 端点 + dispatch round trip |
| --- | --- | --- |
| PostgreSQL 17 | `down_and_up_round_trip_on_the_backend_from_the_environment` ok | `preferences_round_trip_on_the_backend_from_the_environment` ok |
| MySQL 8.4 | 同上 ok | 同上 ok |

```
# PG 17
test migrations::m20261115_000002_create_notification_preferences::tests::down_and_up_round_trip_on_the_backend_from_the_environment ... ok
test result: ok. 1 passed; 0 failed; ... finished in 0.49s
# MySQL 8.4
test preferences_round_trip_on_the_backend_from_the_environment ... ok
test result: ok. 1 passed; 0 failed; ... finished in 3.64s
test migrations::m20261115_000002_create_notification_preferences::tests::down_and_up_round_trip_on_the_backend_from_the_environment ... ok
test result: ok. 10 passed; 0 failed; ... finished in 2.12s
```

（MySQL 上迁移 `down` 真的 drop 了表并重建成功；唯一键、可空 `repo_id`、upsert 的 SELECT/UPDATE 都走通。）

## 5. 逐条调用点映射

完整表格与逐条语义说明见 `docs/plans/T12-plan.md` §5。摘要：

| # | 位置 | 原 | 新 |
| --- | --- | --- | --- |
| 1 | `rg-core/src/issue/reactions.rs:29` | `notification::notify(issue.author_id, "issue_reaction", …)` | `dispatch(db, None, …)` |
| 2 | `rg-core/src/issue/reactions.rs:104` | `notify(comment.author_id, "comment_reaction", …, repo None)` | `dispatch(db, None, …)` |
| 3 | `rg-core/src/issue/assignees.rs:52` | 逐人 `notify(uid, "issue_assigned", …)` | 逐人 `dispatch` |
| 4 | `rg-core/src/collaboration/human_notify.rs:173` | `notify(uid, "collab_question", …)` → `row.id` | `dispatch` → `outcome.created` |
| 5 | `rg-core/src/collaboration/human_notify.rs:270` | `notify(uid, "collab_task_blocked", …)` | `dispatch` → `outcome.created` |
| 6 | `rg-core/src/pull_request/service.rs:304` | `notify_watchers(...)` | 调用方不变，内部 `dispatch` |
| 7 | `rg-core/src/issue/service.rs:488` | `notify_watchers(...)` | 同上 |
| 8 | `rg-core/src/repo/service.rs:1597` | `notify_watchers(...)` | 同上 |
| 9 | `rg-http/src/lib.rs:3693` | `email::send_html_notification(smtp, owner.email, subject, body)` | `dispatch_email_only(db, Some(smtp), …)`，失败仍入队重试 |
| 10 | `rg-http/src/lib.rs:3936` | `email::send_html_notification(...)`（`email.send` 重试 handler） | **不改**（传输层重试，吞错会丢重试；无 user id） |
| 11 | `rg-core/src/user/service.rs:745` | `email::send_html_notification(...)`（忘记密码） | **不改**（事务邮件，不能被偏好关掉；且不在 owned paths） |

## 6. 遗留风险

1. **「3 处邮件」只收敛 1 处**：#10 / #11 保留直发，理由见 plan §5.3 与 §9 风险 1。需要负责人拍板；
   按任务书强改会分别造成"重试失效"和"用户关掉 email 后无法重置密码"。
2. **email 默认开启**与 `ironforge-docs` 里「邮件默认关闭」的先前建议相反；按任务书取开启。当前只有 CI 邮件
   一条真实发信路径，其余入口传 `mailer=None`，所以不会突然群发。
3. 全局偏好行的唯一性靠应用层（`NULL <> NULL`），并发 upsert 可能产生重复行；读取取最新，结果确定。
4. `/user/` 是新增 family（任务书指定）；`family_scope` 与漂移测试参考表各加一条带斜杠的前缀，
   对 `/users/*` 无影响。未知 `/user/*` 路径的 PAT scope 从 admin 变为 user（这些路径只回 404）。
5. CI 邮件正文的内联 `<br/>` 换成换行（文字未改），因为共享模板会转义 body。
6. 偏好读取失败时 in_app 走 fail-open、email 一律不发；计入 `failed` 并 warn。
