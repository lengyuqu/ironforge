# 通用审批原语：现状核实（issue #19）

> 对象：issue #19「通用审批原语」下 NPC 的细化方案（评论 `2120149991086776320`，`2026-11-10T13:42:03Z`）。
> 基线：`94743ca`（`cnb/main`，2026-11-12）。本文件只核**它对现状的断言**，不评价方案取向。
> 判定口径：`成立` = 断言与现树逐字相符；`部分成立` = 主命题成立但名字/范围/机制有偏差；`不成立` = 现树给出相反事实；`不可验` = 设计建议，无现树对应物。
> 汇总：**36 条 → 成立 27 / 部分成立 7 / 不成立 1 / 不可验 1**，另有 4 条它两份材料都没写的新事实（§4）。

## 1. 三套机制的现状对照（行号均按 `94743ca` 现树）

| 维度 | CI 环境审批 | 协作完成条件确认 | 受保护分支合并 |
|---|---|---|---|
| 存储 | `ci_environments` + `ci_environment_approvals`（`entities/ci_environment_approval.rs:6`，表名同文件 `#[sea_orm(table_name = ...)]`） | `collab_completion_confirmations`（`entities/collab_completion_confirmation.rs:16-18`：`condition_set_version` / `task_version` / `head_sha`） | `protected_branches`（`entities/protected_branch.rs:22` `require_approval`、`:24` `required_approvals`） |
| 绑定对象 | `job_id`（`:9`），**无 SHA 列**；sha 在 `pipelines.commit_sha`（`entities/pipeline.rs:12`） | `head_sha` + `task_version` + `condition_set_version` | `pull_requests.head_sha`（`entities/pull_request.rs:35`），按 `commit_id == head_sha` 过滤 |
| 谁能批 | `can_admin_repo` **或** `allowed_approver_ids`（`api/ci_environments.rs:273`/`:278`）；**且必须是 human 账号**（`:285-304`，`AGENT_MAY_NOT_APPROVE`） | 协作组成员（`collaboration/access.rs:18`，先 `invalidate_perm_cache_user`） | ⚠️ **只要 `require_authenticated_read`**（`api/reviews.rs:312`），无写权限门、无审批人名单 |
| 阈值 | `required_approvals`（`api/ci_environments.rs:317`） | 无票数概念（单条确认即可） | `required_approvals.unwrap_or(1)`（`branch_protection/service.rs:451`） |
| 批完做什么 | `release_approved_job`（`ops/ci_environment_ops.rs:149`）+ `stage_has_job_status` 守卫（`api/ci_environments.rs:323`）+ `resume_approval_chain`（`ops/pipeline_ops.rs:497`） | 只让条件显示 `satisfied`（`collaboration/conditions.rs:3`「nothing here blocks, changes or approves anything」、`:24`「not a merge approval」） | 每次合并重算（`branch_protection/service.rs:427` → `:457`），无状态写入 |
| 通知 | 无（`api/ci_environments.rs` 对 `notif|webhook|email|ws::` **0 命中**） | in-app + WS（`collaboration/human_notify.rs`，调用点 `api/collab_questions.rs:162`/`:173`）+ webhook（`collaboration/webhook_events.rs:30`，调用点 `api/collab_questions.rs:182`） | 无 |

## 2. 逐条判定表

### §一 现状表

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| N1 | CI 审批存 `ci_environments` + `ci_environment_approvals` | 成立 | 两个实体文件存在；`ci_environment_approval.rs:6` 为 `pub struct Model` |
| N2 | 绑定 `job_id`（无 SHA） | 成立 | `ci_environment_approval.rs:9` `pub job_id: i64`，实体列只有 job_id/environment_id/approved_by/created_at |
| N3 | 谁能批：管理员或 `allowed_approver_ids`，`required_approvals` 需多方 | 成立 | `api/ci_environments.rs:273`（取允许名单）、`:278`（`can_admin_repo`）、`:317`（阈值）；**补**：还额外拒绝 agent 账号（`:285-304`），它没提 |
| N4 | 批完 `release_approved_job` + `resume_approval_chain` | 成立 | `ops/ci_environment_ops.rs:149`、`ops/pipeline_ops.rs:497`；调用点 `api/ci_environments.rs:319`、`:332`；「边界很脏」也成立——`:323` 的 `stage_has_job_status` 要求本 stage 无其它 `waiting_approval` |
| N5 | CI 侧只有一个 `waiting_approval` 状态、没有推送 | 成立 | `api/ci_environments.rs` 全文无任何通知调用；放行由 `release_approved_job`（`ops/ci_environment_ops.rs:149`）改写状态 |
| N6 | 确认表绑定 `head_sha` + `task_version` + `condition_set_version` | 成立 | `collab_completion_confirmation.rs:16`/`:17`/`:18` |
| N7 | 谁能批：协作组成员（`collab/access.rs`） | 成立 | `collaboration/access.rs:18` `pub async fn is_collab_member` |
| N8 | 批完只显示 `satisfied`，不解除任何阻塞（注释明写） | 成立 | `collaboration/conditions.rs:3` 与 `:24` 两处原文 |
| N9 | 通知：in-app + WS + webhook | 成立 | `human_notify.rs` 建通知、`api/collab_questions.rs:173` 走 WS、`:182` 走 webhook |
| N10 | 受保护分支合并用 `protected_branches` 规则 | 成立 | `entities/protected_branch.rs`；匹配器 `branch_protection/service.rs:438` 复用 push 侧 `rule_matches_ref` |
| N11 | 绑定 `pr.head_sha`（`pr_review_ops.count_current_approvals`） | 部分成立 | 绑定成立；**函数名不存在**：真名 `count_current_human_approvals`（`ops/pr_review_ops.rs:57`），`count_approvals` 只是它的 `None` 包装（`:46`） |
| N12 | 谁能批：审批人 / 写权限 | **不成立** | `submit_review` 只过 `require_authenticated_read`（`api/reviews.rs:312`）⇒ **任何能读仓库的登录账号都能投 approve**；`protected_branches` 只有 `allowed_push_user_ids`（`:30`），管推送不管审批，全表无审批人字段 |
| N13 | 批完 = 放行一次合并 | 成立 | `check_merge_allowed`（`branch_protection/service.rs:427`）每次合并重算，`require_approval` 分支 `:450-461` 不写任何状态 |
| N14 | 通知：无 | 成立 | 合并门内无通知调用 |
| N15 | 「完成条件确认不该被抽象进去——它是记录，不是放行」 | 成立 | 同 N8 两处注释；`conditions.rs:674` 的 `reason` 文案亦写「a collaboration record, not a merge approval」 |

### §2.1 SHA 绑定

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| N16 | `pipeline_jobs` 只有 `environment_id`/`environment_name`，无 commit sha，sha 在 pipeline 上 | 成立 | `entities/pipeline_job.rs:26`/`:27`，实体无 sha 列；`entities/pipeline.rs:12` `pub commit_sha: String` |
| N17 | `count_current_approvals(db, pr_id, head_sha)` 已是「head_sha 一变旧请求失效」的语义 | 部分成立 | 语义对（`:74-79` 按 `commit_id == head_sha` 过滤，且每 reviewer 只取最后一条）；但这是**过滤**不是状态失效——没有 `expired` 记录，也没有任何东西告诉批准人「票废了」 |
| N18 | 判据统一走 `normalize_commit_sha`（`collab/message.rs`） | 成立 | `collaboration/message.rs:72` |

### §2.2 一次性

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| N19 | 「影响行数 0 → 409，语义同现有 `add_approval` 返回 `false`」 | 成立 | `ops/ci_environment_ops.rs:88`（重复插入 → `Ok(false)`，`:104`/`:117`）；handler 侧 `api/ci_environments.rs:309` → `409 "user already approved this job"`；唯一索引 `uq_ci_environment_approval_job_user`（`migrations/m20260712_000011_create_ci_environments.rs:138`，键 `(job_id, approved_by)`）确实存在 |
| N20 | 消费点 = 动作真正执行时、同事务、`WHERE state='pending'` 守卫 | 不可验（设计） | 现树里 CI 侧是「先 `add_approval` 再 `release_approved_job`」两次独立写（`:306`、`:319`），没有事务包裹——这正是它要新增的那层，不能当作现状引用 |

### §2.3 谁有权批

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| N21 | `protected_branches.allowed_push_user_ids` / 审批人 | 部分成立 | `allowed_push_user_ids` 存在（`entities/protected_branch.rs:30`），但**「审批人」没有字段**；把它当审批人名单会静默改变语义 |
| N22 | `ci_environments.allowed_approver_ids` + `can_admin_repo` | 成立 | `api/ci_environments.rs:273`、`:278` |
| N23 | `is_collab_member` 刻意绕过 30s 权限缓存 | 成立 | `collaboration/access.rs:17` 注释原文「not the 30s process-local permission cache」；`:23` 先 `invalidate_perm_cache_user` |
| N24 | PR 评审已拒自批（`reviews.rs`），CI 环境审批没有这条 | 成立 | 自批拒绝在 `api/reviews.rs:325-329`；CI `approve`（`api/ci_environments.rs:230-`）无「批准人 ≠ 触发人」比较 |
| N25 | 「agent 只能申请、永远拿不到 approve」是**新增**硬约束 | 部分成立（大部分已实现） | CI 侧**已经**拒绝 agent（`api/ci_environments.rs:285-304`，`AGENT_MAY_NOT_APPROVE`）；PR 侧靠 `count_current_human_approvals` 过滤非 human（`ops/pr_review_ops.rs:57-77`，agent 的 approve 可写但不计票）。缺的只有「agent 侧申请入口」 |

### §2.4 / §三 / §四

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| N26 | `agent.rs::validate_agent_token_scopes` 已有这套思路 | 成立 | `rg-core/src/agent.rs:225` |
| N27 | `collab_task_budgets` 是 task 级预算（要 `task_id`） | 成立 | `entities/collab_task_budget.rs:14` `pub task_id: String` |
| N28 | 迁移用 `has_column()` 守卫 | 成立 | 例：`migrations/m20260712_000010_add_pipeline_job_when.rs:15`；`SchemaManager::has_column` |
| N29 | capability 按 `collab/capabilities.rs` 追加 + `CONTRACT_MINOR` +1 | 成立 | `capabilities.rs:22` 版本策略注释、`:31` `CONTRACT_MINOR: u32 = 1` |
| N30 | `notifications` 无偏好字段、`user` 无通知/邮件开关 | 成立 | `entities/notification.rs:8-23`（user_id/event_type/title/body/repo_id/is_read/created_at）；`entities/user.rs:15` 只有 `email` 地址列 |
| N31 | 没有订阅、没有退订、无 per-event 静默 | 部分成立 | 用户级确实没有；但**仓级**静默窗口存在：`entities/collab_escalation_setting.rs:17`/`:18` `quiet_start_minute`/`quiet_end_minute` |
| N32 | 邮件两条链路（直发 + 失败落 `email.send`，重试/退避/死信） | 成立 | `email/mod.rs:82`；`crates/rg-http/src/lib.rs:3465` 直发、`:3486` 入队；`background_jobs/mod.rs:33` `TASK_EMAIL_SEND`、`:14` 死信语义 |
| N33 | CI 触发邮件写在 post-push，只有失败才入队 | 成立 | `lib.rs:3465` 直发、`:3486` 仅在 `Err` 分支入队。**补**：邮件本身是**每次触发都发**（`:3452` 的 `if let Some(smtp)` 无失败条件），入队的才是失败补投 |
| N34 | 协作链路完全没有邮件（`human_notify.rs` 原文「There is no email and no webhook yet」） | 部分成立 | 引文逐字对（`human_notify.rs:22`），但 **「no webhook」已过时**：`webhook_events.rs:30` + `api/collab_questions.rs:182` 已在投递 `collab.question.asked`。该注释本轮已订正 |
| N35 | `collab_escalation_settings` 已有 `blocked_after_secs` / `repeat_after_secs` / 静默时段 | 成立 | `collab_escalation_setting.rs:13`、`:16`、`:17-18` |
| N36 | `send_html_notification` 模板尾部那句「已经够用」 | 部分成立 | 句子存在（`email/mod.rs:106`「You received this email because you have notifications enabled on IronForge.」）但**内容是错的**：没有这个开关（见 N30 自证）。做偏好表时必须先改这句，否则邮件继续说一个不存在的设置 |

## 3. 三条要优先看的订正

1. **合并审批的准入面比它写的宽得多**（N12）。它把「审批人 / 写权限」写进现状表，据此说「抽象只是把现有判定收进求值器」。真值：`submit_review` 只有 `require_authenticated_read`，**读权限即可批准**；仓库里也没有任何「审批人名单」字段。⇒ 抽象若照抄现状，会把「任何能读仓库的人都能批」固化进新原语；若改成写权限，那是**权限收紧**（会影响现有仓库的合并流程），必须先拍板。
2. **「agent 不得批准」已经实现了一半**（N25）。CI 侧有 `AGENT_MAY_NOT_APPROVE` 硬拒（`api/ci_environments.rs:285-304`），PR 侧靠 human-only 计数（`ops/pr_review_ops.rs:57-77`）。它把这写成「抽象时一处写死」的**新增**约束，落地时容易另写一套判定，产生第三份口径。
3. **`human_notify.rs` 的注释是过时的**（N34）。它把「no webhook yet」当作「协作链路没有外发通道」的证据，而 `webhook_events` 早已在投。注释本轮已订正为「无邮件，外发走 `webhook_events`」——否则下一份设计还会照抄这句。

## 4. 它两份材料都没写的新事实

| # | 事实 | 证据 | 影响 |
|---|---|---|---|
| F1 | **自批守卫只在 HTTP 层** | `api/reviews.rs:325` 有比较；core `review/service.rs:57-101` 的 `submit_review` 既不查权限也不查自批，只校验 PR `state == "open"` | 任何绕过 HTTP handler 的调用面（MCP 工具、内部服务）都能自批。抽象原语时若把守卫留在 handler，同一个洞会跟着搬到新入口 |
| F2 | **CI 通知邮件的 `<br/>` 会被转义成字面文本** | `crates/rg-http/src/lib.rs:3462` 的 body 含 `<br/><br/>`；`email/mod.rs:110` 对 `message` 调 `html_escape` | 用户收到的邮件里显示字面 `<br/><br/>`。属用户可见渲染 |
| F3 | **邮件尾句为不存在的设置背书** | `email/mod.rs:106` | 每封通知邮件都在陈述一个不存在的开关（与 N30 直接冲突） |
| F4 | **`count_approvals` 与合并门不是同一口径** | `ops/pr_review_ops.rs:46` 传 `None` ⇒ 只数「`commit_id` 为 NULL 的审批」；合并门传 `pr.head_sha`（`branch_protection/service.rs:457`） | 两个名字近似的入口给出不同票数；新原语若引用 `count_approvals` 会数错 |

## 5. 待拍板（L2，本流程不改代码）

| # | 级别 | 事项 | 为什么不能自动落 |
|---|---|---|---|
| R19-1 | **P1** | 合并审批准入面：现状是「能读仓库即可批」（`api/reviews.rs:312`）。要收紧到写权限或审批人名单，属权限语义变更 | 改的是谁能合并代码，会让现有仓库的合并突然 403 |
| R19-2 | P2 | 自批守卫下沉到 core（F1）：`review/service.rs:57` 补守卫，或明确「只允许 HTTP 入口」 | 改 core 服务签名/语义，影响 MCP 与内部调用面 |
| R19-3 | P2 | `email/mod.rs:106` 尾句与不存在的 `notifications enabled` 开关（F3）；同 `:110` 的 `html_escape(message)` 吃掉 `<br/>`（F2） | 用户可见文案与渲染，改了两个入口的行为 |
| R19-4 | P3 | `count_approvals` 与 `count_current_human_approvals` 口径分叉（F4）：建议前者删除或标注「仅历史」 | 删除 `pub` 函数属 API 面变更 |
| R19-5 | P3 | 「agent 不得批准」应复用现有实现（`AGENT_MAY_NOT_APPROVE` + human-only 计数）而不是新造第三份判定（N25） | 需要新原语落地时一并裁决，单改现状无收益 |

## 6. 复核请求范围

1. `crates/rg-http/src/api/reviews.rs:305-330`：`submit_review` 的准入是否只有 `require_authenticated_read`（N12），自批比较是否只此一处（F1）。
2. `crates/rg-core/src/email/mod.rs:100-112`：`html_escape(message)` 与 `:106` 尾句是否构成 F2/F3。
3. `crates/rg-db/src/ops/ci_environment_ops.rs:88-146` 与 `api/ci_environments.rs:254-345`：N3/N4/N19 的现状链是否还漏了第四处守卫。
