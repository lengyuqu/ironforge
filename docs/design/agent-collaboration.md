# 同账号多 Agent 协作

> 状态：已实现（0.5.1）。事实来源：`crates/rg-core/src/collaboration/`、`crates/rg-http/src/api/collab*.rs`、`crates/rg-db/src/entities/collab_*.rs`。
> 本文只写仍然成立的语义；实现细节以代码为准，设计过程见 git 历史。

## 1. 范围与身份

两个不同 harness（Claude Code、Codex、Cursor 等）使用同一个账号和仓库时，可以各自注册会话、发现对方、围绕任务交流、声明阻塞、交接，并在断线后恢复消息和状态。服务端不做 Agent 调度，也不执行消息里的内容。

- **账号是授权主体，会话是协作归属。** 会话 ID 由服务端生成；展示名与客户端类型是自报，不能证明 Agent 类型或工作已完成。
- 会话创建时返回一次 256 位 `session_secret`，服务端只存带域分离的摘要；后续用 `X-Ironforge-Session` 与 `X-Ironforge-Session-Secret` 提交。
- **同账号不是安全隔离边界。** 共用主凭据的客户端可以另开会话、冒用展示名；会话密钥只防公开 ID 的意外冒用，不能抵御掌握同一主凭据的客户端。
- 读协作数据需要仓库读权限；创建会话/任务/消息/交接与改状态需要仓库写权限。Agent 写还要有效会话凭据；人工 Web 写走现有认证与 CSRF。每次请求重新鉴权。
- 跨仓库对象一律 404。用户文本（标题、消息、答复、工作包、链接）是不可信数据，响应标 `origin: user_content`；不触发命令、合并、批准或外发。

## 2. 数据模型

31 张表，按用途分组（实体均在 `crates/rg-db/src/entities/`）：

| 组 | 表 |
|---|---|
| 会话与声明 | `agent_sessions`、`collab_session_declarations`、`agent_draft_namespaces` |
| 任务 | `collab_tasks`、`collab_handoffs`、`collab_messages`、`collab_reads`、`collab_takeovers`、`collab_work_packages` |
| 收件箱与提醒 | `collab_inbox_reads`、`collab_notices`、`collab_delivery_receipts`、`collab_escalation_settings` |
| 问答 | `collab_questions`、`collab_answers` |
| 依赖与决定 | `collab_dependency_graphs`、`collab_task_dependencies`、`collab_decisions`、`collab_decision_changes` |
| 跨仓与指标 | `collab_coordination_groups`、`collab_coordination_links`、`collab_metric_events`、`collab_health_baselines` |
| 完成条件 | `collab_completion_condition_sets`、`collab_completion_confirmations`、`collab_completion_snapshots` |
| 模板与预算 | `collab_task_templates`、`collab_task_template_versions`、`collab_task_template_links`、`collab_task_budgets`、`collab_task_usage` |

所有对象用服务端 UUID（账号与仓库沿用 BIGINT），每张表显式命名并与实体 `table_name` 一致；跨对象引用在数据库层用复合外键保证同一 `repo_id`。时间统一 UTC。

## 3. 任务生命周期

- 状态：`open` / `in_progress` / `waiting` / `blocked` / `done`，任意状态可经 CAS 转移，`done` 可重新打开。
- `collab_tasks.version` 是乐观锁：更新必须带 `expected_version`，冲突返回 409 `VERSION_CONFLICT` 与当前版本。
- `next_seq` 是该任务最后一条已提交消息的序号。发消息在事务内锁任务行、递增 `next_seq`、插入消息并提交，保证同一任务内 `seq` 顺序等于提交顺序；不同任务不保证全局顺序。
- `branch`、`paths_json`、issue/PR 编号都是**声明**：不操作 Git，不做 glob；路径必须是仓库相对路径（拒绝绝对路径、`..`、NUL），最多 100 条。
- 任务标题 1–200 字符，描述 ≤16 KiB；关联 issue/PR 必须属于当前仓库，SHA 若提供必须是该仓库的完整 commit，但只代表格式有效，不代表验证通过。

## 4. 消息、已读、交接、收件箱

- **消息**是不可变追加记录，类型 `note` / `help` / `blocker`，正文 1–32 KiB；同一会话的 `client_message_id` 幂等，同键同体返回原消息，异体 409。
- **已读**是每会话、每任务的单调水位 `last_read_seq`，只能前进，不能超过已提交的最大 `seq`。
- **交接**状态 `pending` / `accepted` / `cancelled`：创建者只能取消，目标会话只能接受，均需会话凭据与 `expected_version`；交接内容是声明，不是验证。
- **收件箱**是从任务归属、定向消息、待接受交接、未答问题等记录算出的投影，不另存权威状态；会话与用户各有已读位置，不提供跨任务全局未读数，也不承诺实时推送。

## 5. 问答、工作包、接手、声明

- **问题**状态 `open` / `answered` / `cancelled`，接收者可以是会话、用户或仓库写者；每题最多一个有效答复，答复只追加并记录当时的上下文哈希。
- **工作包**是不可变版本（`(task_id, revision)` 唯一），包含目标、范围、制品、commit、已完成项、未决项、下一步与证据引用。同仓的 pipeline/job 引用会被服务端独立解析成 `verified_reference`；解析不了就是 `unavailable`，自报测试不算验证。
- **接手（takeover）** 不可变：记录从哪个会话、到哪个会话、原任务版本与原因，与任务 owner/version 更新同事务；不关闭待处理交接，也不改写旧会话。
- **会话声明**（工具、能否跑测试、网络限制）永远是自报，不参与授权。

## 6. 依赖、决定、提醒

- **依赖图**按仓库维护 `version` 并在图上做并发控制；`A → B` 表示 A 等待 B。禁止自依赖、跨仓与重复边，写入时检查成环；遍历有界。
- **决定**不可变：`collab_decisions` 记录标题、理由、上下文与主体；`collab_decision_changes` 只能记一次终结事件（`superseded` / `revoked`），替代决定必须在同一任务且不能成环。
- **提醒**（`collab_notices`）是持久的待处理提示，按 `(recipient, kind, source, generation)` 去重；它只提示「某对象变了」，消费者必须回读源对象，不能把提醒当权威状态。
- **升级设置**按仓库配置（阻塞/问题/交接的超时、重复间隔、静默时段），无行即关闭。

## 7. 跨仓协调组与健康指标

- **协调组**属于一个锚仓：标题和描述是客户端写入、锚仓读者可见，服务端绝不从其他仓库自动填充名称或内容。组内链接指向目标仓库的任务或 commit，逐仓鉴权；无权仓库不出现在响应、计数或游标里。
- **健康指标**来自 `collab_metric_events`：业务事务同事务写入，只存事件类型、状态迁移与来源 ID，不存正文或密钥。指标区分平台计算与 Agent 自报，不给人或 Agent 排名；未覆盖的数据不能当零。

## 8. 完成条件、模板、预算

- **完成条件**是不可变版本集合，类型有 `question_answered`、`work_package_present`、`ci_passed_for_commit`、`manual_confirmation`。评估结果 `satisfied/unsatisfied/stale/unknown` 可存快照；首期只展示，不阻止任务改成 `done`。CI 结果必须绑定实际 SHA，取消/跳过/无权读取都不算通过。
- **模板**由仓库管理员维护，内容不可变（编辑即新版本）；任务固定模板版本并保留快照。
- **预算/用量**按「一个单位 + 一个来源」记录，金额是规范化十进制文本；不跨单位求和，缺失不当零，超预算不自动暂停。

## 9. 草稿分支命名空间

`agent_draft_namespaces` 为每个 Agent 预留 `refs/heads/<prefix>` 目录（`prefix` 以 `/` 结尾，不含通配符）。状态：
- `active`：接受推送；
- `expiring`：`expires_at` 已过，拒绝新推送但分支仍可读，可续期回 `active`；
- `deleted`：分支已删，行保留作为预留记录，同名分支复活会被拒绝。

清理发生在 `expires_at + grace_secs` 之后，幂等可重跑。

## 10. 接口面

- REST 前缀：`/api/v1/repos/{owner}/{repo}/collaboration`，另有 `/capabilities`、`/rules`、`/overview`、`/search`、`/diagnostics`。
- 路由组：`sessions`（含 heartbeat/end/declaration）、`tasks`（含 messages、read、handoffs、takeovers、reassign、context、overlaps、dependencies、decisions、questions、work-packages、completion-*`）、`inbox`（会话/用户）、`notices`、`groups`、`health`、`templates`、`budgets/usage`、`escalation-settings`、`deletion-impact`。
- MCP 暴露同一套操作（任务、消息、交接、收件箱、问答、决定、小组等），工具清单见 [.ai/README.md](../../.ai/README.md)。

## 11. 不变量

1. 权限每次请求重查；对象 ID 和游标不授予权限；跨仓引用返回 404。
2. 所有写操作可用 `client_request_id` 去重：同键同体返回同一结果，异体 409；重试必须重新鉴权。
3. 状态变更带 `expected_version`；并发只有一个成功。
4. 消息、决定、工作包、接手记录只追加；修正靠新记录，不改历史。
5. 自报字段（会话名、声明、证据链接）永远与平台验证分开呈现。

## 12. 未实现 / 边界

- 跨实例或外部平台（GitHub/Gitea）联邦、跨仓全局调度、服务端 LLM 不在范围内。
- 完成条件未提供「未满足即禁止 done」的强制模式；分支保护与人工审批仍走独立链路。
- 协作健康不做个人排名；多实例撤权延迟按部署的实际缓存语义，不宣称即时。
