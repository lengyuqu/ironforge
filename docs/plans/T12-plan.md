# T12 执行计划 — [#19 步骤5] 通知偏好 + 邮件单一出口

> 权威约束见 `tasks/T12.md`。基线 `546fe3e`，分支 `agent/t12-notify-prefs`，worktree `../ironforge-t12`。
> 验收命令的真实输出、触碰路径与遗留风险见 `docs/plans/T12-task.md`。

## 1. 交付边界

做：

1. `notification_preferences` 表 + 迁移 `m20261115_000002`，`Migrator::down` 29 → 30。
2. 单一出口 `notification::dispatch`：`NotifyRequest` → `in_app` 行 + 邮件，按偏好裁剪。
3. 收敛既有通知入口：5 处 `notification::notify` 改为 `dispatch`；`notify_watchers` 内部改走 `dispatch`
   （签名不变，3 个调用方零改动）；CI 邮件改为 `dispatch_email_only`。
4. 两个端点 `GET` / `PUT /user/notification-preferences`，路由三处齐。

不做（明确排除）：webhook / WebSocket 投递（那是 `webhook::service::trigger_event` 与 HTTP 层的事）、
#15 的 outbox、重试队列、按事件类型定制邮件模板、前端页面。
`password_reset` 邮件**故意不收敛**（理由见 §5）。

未改：`Cargo.toml` / `Cargo.lock`（无新依赖，lettre 已在）、`CLAUDE.md`、`ironforge-docs/`、`.cnb.yml`、
`.github/`、`scripts/`、`deploy/`、`crates/rg-mcp/`。

## 2. 表与语义

| 列 | 类型 | 说明 |
| --- | --- | --- |
| `id` | bigint PK auto | |
| `user_id` | bigint NOT NULL, FK → `users` ON DELETE CASCADE | |
| `repo_id` | bigint NULL, FK → `repositories` ON DELETE CASCADE | `NULL` = 全局默认 |
| `event_type` | varchar(64) | 与 `notifications.event_type` 同一值空间 |
| `channel` | varchar(16) | `in_app` / `email` |
| `enabled` | boolean | |
| `updated_at` | timestamptz | |

唯一键 `uq_notification_preferences_scope (user_id, repo_id, event_type, channel)`；另加
`ix_notification_preferences_user (user_id)` 覆盖"我的全部偏好"。

**表里没有记录 = 开启。** 只有显式 `enabled = false` 才是关闭；`enabled = true` 用于在一个全局静音之上
重新打开某个仓库。因此**不为新用户初始化任何行**（`GET` 也不写行，有测试钉住）。

`repo_id` 可空的代价：SQL 里两个 `NULL` 互不相等，唯一键**不能**阻止重复的全局行。三层应对：

1. 写入走 `notification_preference_ops::upsert`，先按精确 scope（含 `repo_id IS NULL`）查再改，不用
   `ON CONFLICT`（冲突目标对 NULL 无效）；
2. 读取对同一 `(channel, scope)` 取 `updated_at DESC, id DESC` 的第一条，重复行不会让答案依赖后端行序；
3. 迁移注释写明"不要用 `repo_id = 0` 哨兵来'修'它"——`NULL` 是"全局"的文档语义。

## 3. 偏好解析：三级

```
repo 覆盖（user, repo_id, event_type, channel）  ──有──▶ 用它
        │没有
        ▼
全局默认（user, NULL, event_type, channel）      ──有──▶ 用它
        │没有
        ▼
开启（true）
```

实现：`notification_preference_ops::effective(db, user_id, event_type, repo_id)`，一次查询取
`repo_id IS NULL OR repo_id = ?` 的所有行，再按 channel 各挑一条，返回
`EffectivePreference { in_app: {enabled, explicit}, email: {enabled, explicit} }`。

`explicit` 是给设置页的：`false` 表示"默认推导出来的开启"，不是用户存过的行。

**读不到偏好怎么办**：`dispatch` 记 warn、计入 `failed`，然后**只写 in_app**、绝不发信——丢一条铃铛
比丢一次静音更可接受，但一次读失败不能变成一封用户没同意的邮件。

## 4. `dispatch` 的行为

```rust
NotifyRequest { recipients, event_type, repo_id, title, body, action_url }
DispatchOutcome { in_app, email, skipped_by_preference, failed, created }
```

| 场景 | `in_app` | `email` | `skipped` | `failed` |
| --- | --- | --- | --- | --- |
| 未设置（默认开启）+ 有 mailer | 写行 | 发信 | 0 | 0 |
| in_app 被关 | — | 视 email 偏好 | +1 | 0 |
| email 被关 | 写行 | — | +1 | 0 |
| email 开启但 `mailer = None` | 写行 | 静默跳过 | 0 | 0（不是失败） |
| 邮件发送失败 | 写行（已提交） | — | 0 | +1 |
| 偏好读取失败 | 写行（fail-open） | 不发 | 0 | +1 |

- `DispatchOutcome::created: Vec<(user_id, notification_id)>` 是任务书四个计数之外的**第五个字段**。
  理由：`human_notify` 需要刚写入的行 id 去 WebSocket 推送（`notification_id` 是既有载荷），只换入口不能
  丢这个语义。它只是观测数据，不影响四个计数。
- `dispatch` **不返回错误**：业务动作已提交，通知是 best-effort。调用方看 `failed` 记 warn。
- `dispatch_email_only` 是同一实现的 email-only 入口：`in_app` 通道完全不走（不写行、不查 in_app 偏好、
  不计数 skipped）。存在的唯一理由是 CI 邮件那条路径**原本没有铃铛行**，走 `dispatch` 会给每次 pipeline
  推一条通知——那是行为变更。
- **不**去重 recipients、**不**做 webhook / ws、**不**入队重试。

`Mailer` 就是 `email::SmtpConfig` 的类型别名（`pub type Mailer = email::SmtpConfig;`），不新造传输抽象。
没有 mailer 的调用点传 `None`，不为传 mailer 改任何函数签名链。

## 5. 调用点逐条映射

任务书写"13 处 `notification::notify` + 3 处邮件"。按 `546fe3e` 实测：`notification::notify` 外部调用
**5 处**、`notification::notify_watchers` 调用 **3 处**（内部对每个 watcher 扇出，所以作者的 13 可能按
扇出后的条数或旧行号数出来的），邮件 **3 处**，共 11 个入口，另加 1 处测试直接调 `notify`。
全仓没有第 12、13 个 `notify` 调用点（`grep -rn "notification::notify"` 可复核）。

### 5.1 `notification::notify`（5 处，全部改 `dispatch`，mailer = `None`）

| # | 文件:行（改后） | 原调用 | 新调用 | 语义 |
| --- | --- | --- | --- | --- |
| 1 | `rg-core/src/issue/reactions.rs:29` | `notify(db, issue.author_id, "issue_reaction", "{} reacted {} to your issue", Some(issue.title), Some(issue.repo_id))` | `dispatch(db, None, {recipients:[issue.author_id], "issue_reaction", repo_id: Some(issue.repo_id), title, body: Some(issue.title), action_url: None})` | 收件人/事件/标题/正文一致 |
| 2 | `rg-core/src/issue/reactions.rs:104` | `notify(db, comment.author_id, "comment_reaction", "{} reacted {} to your comment", Some(truncate(body,80)), None)` | 同上，`repo_id: None` | 同上（原本就没有 repo_id） |
| 3 | `rg-core/src/issue/assignees.rs:52` | 对每个新指派人 `notify(db, uid, "issue_assigned", "{actor} assigned you to issue #N: {title}", Some(title), Some(repo_id))` | 循环内逐个 `dispatch`，单人 recipients | 逐人一行、逐人 warn |
| 4 | `rg-core/src/collaboration/human_notify.rs:173` | `question_asked`：逐人 `notify(..., EVENT_QUESTION_ASKED, title, Some(body), Some(repo.id))`，拿回 `row.id` 填 `Notified` | 逐人 `dispatch`，用 `outcome.created` 还原 `Notified { user_id, notification_id }` | 被静音的人不再收到铃铛/推送 |
| 5 | `rg-core/src/collaboration/human_notify.rs:270` | `task_blocked`：`notify(..., EVENT_TASK_BLOCKED, ...)` 返回单元素 `Vec<Notified>` | 同上，`outcome.created` 映射 | 同上；`has_unread_with_body` 去重逻辑未动 |

失败告警从 `if let Err(e) = notify` 改为 `if outcome.failed > 0`：`dispatch` 不再返回 `Err`，告警文字保留。

### 5.2 `notify_watchers`（3 个调用方，签名与语义不变）

| # | 调用方 | 处理 |
| --- | --- | --- |
| 6 | `rg-core/src/pull_request/service.rs:304` `notify_watchers_pr` | 不动；`notify_watchers` 内部改为一次 `dispatch`（recipients = watchers 去掉作者） |
| 7 | `rg-core/src/issue/service.rs:488` milestone closed | 不动 |
| 8 | `rg-core/src/repo/service.rs:1597` `notify_watchers_push` | 不动 |

为什么不改签名：3 个调用方分布在 PR / issue / repo 三个 service，改签名会把 mailer 或 db 之外的参数
扩散进去，而 watcher 扇出是产品里量最大的通知路径——一接上 mailer（email 默认开启）就是**每次 push 给
所有 watcher 发信**。所以内部固定传 `None`，安静地只写 in_app。函数注释写明了这个理由。

### 5.3 邮件（3 处）

| # | 位置 | 处理 | 理由 |
| --- | --- | --- | --- |
| 9 | `rg-http/src/lib.rs:3693` CI pipeline 触发邮件 | **改** `dispatch_email_only(db, Some(smtp), {recipients:[repo_owner_id], "ci_triggered", repo_id: Some(repo_id), title: 原 subject, body: 原正文, action_url: None})` | 这是"通知类"邮件，最需要偏好；email-only 保持"不新增铃铛行" |
| 10 | `rg-http/src/lib.rs:3936` `email.send` 后台任务 handler | **不改** | 它是 #9 的**传输层重试**：payload 里只有 `to`（没有 user id），失败要 `Err` 让队列再试。走 `dispatch` 会（a）重新查偏好、（b）把失败吞掉让 job 报成功、（c）无 user id 可查。单出口是"投递发起"，不是"重发传输" |
| 11 | `rg-core/src/user/service.rs:745` 忘记密码 | **不改** | 事务型邮件，不能被通知偏好关掉（用户关了 email 就永久无法重置密码）；另外该文件不在本任务 owned paths 内。见 §9 风险 1 |

#9 的两点可见差异，都在代码注释与本文里写明：

- 正文里的内联 `<br/>` 换成换行：共享模板对 body 做 HTML 转义，保留 `<br/>` 只会在邮件里打印字面标签。
  **文字一个没改**，只把标记换成换行；`Commit: ...` / `Pipeline ID: ...` 仍在。
- 失败仍入队 QUEUE-001 重试：`outcome.failed > 0` 时按原 payload（`to`/`subject`/`message`）入队，重试次数与
  今天一致。

## 6. 端点

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| `GET` | `/api/v1/user/notification-preferences` | `{ default_enabled: true, preferences: [{repo_id, event_type, channel, enabled, explicit}] }`：catalogue 里每个事件 × 两个渠道的**有效值**（未设置则推导为开启、`explicit=false`），外加用户存过的所有仓库覆盖行 |
| `PUT` | `/api/v1/user/notification-preferences` | body 是**裸数组** `[{repo_id, event_type, channel, enabled}]`；逐条 upsert，返回与 `GET` 相同的形状 |

校验：`event_type` 去空格后 1–64 字符；`channel` 必须是 `in_app` / `email`；`repo_id` 非空时必须存在
（否则外键会变成 500，这里答 400）；单次最多 200 条。**不**校验调用者能否读该仓库——行是按调用者存的，
不泄露内容。

`/user/` 是这套 API 里**新的 family**（其余都是 `/users/...`）：按任务书保留该路径，同时在
`route_meta::family_scope` 与路由漂移测试的参考表里各加一条 `path.starts_with("/user/")`（带斜杠，
不影响 `/users`）。不这么做，`route_metadata_tests` 会把这条路由判成"未声明的 family = admin"。

路由三处：`rg-http/src/lib.rs`（注册）、`rg-http/src/openapi.rs`（文档）、
`rg-core/src/auth/route_meta.rs`（PAT scope，`user`）。

## 7. 测试

| 层 | 测试 | 钉住什么 |
| --- | --- | --- |
| rg-db 迁移 | `ddl_uses_the_plural_table_name_and_constraints_on_all_backends` | 显式复数表名、两个外键、四列唯一键在三种 SQL 方言下都在 |
| | `down_drops_the_table_and_up_recreates_it` / `up_is_idempotent_and_repairs_a_missing_column` | 回滚真的删表、重放真的补列 |
| | `down_and_up_round_trip_on_the_backend_from_the_environment` | PG / MySQL 上的 up→down→up |
| rg-db ops | 6 个 `notification_preference_ops::tests` | 未设置=开启、全局静音、**repo 覆盖优先于全局**、按事件隔离、upsert 不重复、全局行与仓库行共存 |
| rg-core dispatch | 11 个 `dispatch::tests` | 默认开启、in_app 静音不写行、repo 覆盖优先、email 静音仍写 in_app、email 开启会**尝试**发送（不可达 SMTP → `failed=1`）、**邮件失败不影响 in_app**、`mailer=None` 静默、逐收件人 id、email-only 不写行、matrix 展开 |
| rg-http | 9 个 `notification_prefs_tests` | 401、全新账号全开且**没有写行**、PUT 后 GET 反映、repo 覆盖与全局并存且重复 PUT 只一行、非法 channel / 仓库 / event_type → 400、跨用户隔离、跨后端 round trip |

`dispatch` 的邮件断言用 `127.0.0.1:1`（连接立即被拒）当"可行但必然失败"的 mailer：于是"没有尝试"（`failed=0`）
与"尝试后失败"（`failed=1`）可区分，测试不依赖真实 SMTP、也不等超时。

## 8. 跨后端

新 SQL（建表、两个索引、upsert 的 SELECT/UPDATE/INSERT、层级查询）在三个后端各跑一次，命令与输出见
`T12-task.md` §4。MySQL 8.4 需要 `--log-bin-trust-function-creators=1`（既有 FTS 触发器的限制，与本任务无关）。

## 9. 遗留风险

| # | 风险 | 说明 |
| --- | --- | --- |
| 1 | 「3 处邮件」只收敛了 1 处（CI）；#10 重试 handler 与 #11 忘记密码保持直发 | #10 是传输层重试（吞错就无法重试）、#11 是事务邮件（关掉 email 会锁死账号）且 `user/service.rs` 不在 owned paths。**需要负责人确认**是否要按任务书强行改；我判断改了是缺陷 |
| 2 | email 渠道**默认开启** | 与 `ironforge-docs/architecture/agent-collaboration-human-notify-verification-2026-11.md`「邮件默认关闭」的先前建议相反，按任务书取开启。当前只有 CI 邮件一条真实邮箱路径（其余调用点传 `mailer=None`），所以现状不会突然群发；将来谁把 mailer 接到 watcher 扇出上就是在给所有 watcher 发信，代码注释已写警告 |
| 3 | 全局行的唯一性靠应用层 | `NULL <> NULL`，唯一键管不住重复的全局行；upsert 先查后写，读取取最新一条。并发 upsert 同一 scope 仍可能产生两行（同一用户的两次设置请求），结果确定（取最新），不会错投 |
| 4 | `repo_id` 不校验可读性 | 只有存在性校验。用户可以为自己看不到的仓库 id 存偏好（要猜到 id），不泄露内容，也不影响他人 |
| 5 | 迁移只回滚到 30 步 | `crates/rg-db/src/lib.rs` 里 `down(Some(29))` → `30`、注释 103→104，是既有测试的窗口计数，按规矩只改数字 |
| 6 | CI 邮件正文的 `<br/>` 变换行 | 共享模板转义 body；文字未改，仅标记→换行。要完全一致只能改模板或绕过模板，两者都被"不改邮件模板/正文"禁止 |
| 7 | 偏好读取失败时 fail-open 写 in_app | 宁可多一条铃铛也不丢通知；`failed` 会计数并 warn。若将来要求严格静音，需改成 fail-closed |
