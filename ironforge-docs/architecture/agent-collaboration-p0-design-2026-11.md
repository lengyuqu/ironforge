# 多 Agent 协作 P0：收件箱、接手、问答与工作包

> 状态：Proposed，待评审；2026-11-04。本文细化[产品需求 R01–R05](agent-collaboration-product-requirements-2026-11.md)，不调整[协作首版 C00–C06](multi-agent-collaboration-design-2026-11.md)的实施范围。
> 依据：首版设计及当前工作区静态检查。当前会话、任务、消息、已读和交接实体/接口存在代码；部分交接和 MCP 修改仍在工作区中。接口可用性、测试和合并状态均未在本次验证；实施前必须重新对照最终契约。

## 1. 目标与约束

两个 Agent 共用账号时，B 可以发现需要处理的事情，找到 A 的工作上下文，在 A 离线或会话丢失后接手；A/B 均可向用户或其他 Agent 提问并获得可追溯答复。工作包应让接手者快速看到目标、代码版本、未解决事项及验证证据。

首版复用仓库权限、会话凭据、任务版本、消息、已读和交接语义；不要求先实现通用事件总线、Agent 调度器或 Git 写工具。账号负责授权，会话只标记来源：使用相同账号的客户端可自报任何会话名称。用户答复是协作决定，不等于独立身份认证或分支保护审批。

默认范围为**单仓库**；跨仓收件箱后置。本文权限暂沿用首版提案：已认证仓库读者可读，仓库写者可创建和答复；人工回复使用用户凭据及既有 CSRF 防护，无需 Agent 会话密钥。实际路由、权限缓存和主体解析以合并后的实现为准。逐请求授权，撤权时不得返回对象内容；跨实例缓存存在时须报告实际一致性，不能宣称即时撤权。

## 2. 共同约定

REST 前缀 `P = /api/v1/repos/{owner}/{repo}/collaboration`；接口为新增提案。Agent 写请求需用户认证、仓库写权限及有效会话凭据；人工端通过现有认证并校验仓库写权限。读取需认证和仓库读权限；敏感字段不进入日志。对象 ID 和游标均不授予权限。无权限遵循现有 403/404 策略；仓库与对象不匹配返回 404。

所有写操作接受 `client_request_id`（UUID），在 `(repo_id, actor_type, actor_id, operation, client_request_id)` 唯一；actor 为会话 ID 或人工用户 ID。服务端存规范化请求哈希及结果引用，事务内与业务写入一并落库：同键同体并发/重试返回同一结果，异体 409 `IDEMPOTENCY_KEY_REUSED`。重试必须重新鉴权，撤权不回放敏感内容；保留期首版至少 30 天，过期后不保证去重，客户端不得无限重试。需要迁移时另加新迁移，不能改首版已有迁移。

更新任务、问题、工作包时必须带 `expected_version`；变更以条件更新和事务完成，版本冲突返回 409 `VERSION_CONFLICT`、当前版本和安全的最新资源定位。公开用户文本返回 `origin: user_content`；平台检查与自报字段分离。`reason` 等结构以实施时 API 的可用支持为准；不得依赖解析自然语言。

## 3. 数据模型

以下名称为候选，类型和约束需适配 SQLite、PostgreSQL、MySQL；`repo_id` 为现有仓库主键，业务对象使用 UUID。每个跨对象引用在事务内校验同仓库及权限。限制按 UTF-8 字节数计算，不把截断后的内容伪称完整。

| 表 / 视图 | 主要字段 | 不变量、索引与生命周期 |
|---|---|---|
| 现有 `collab_tasks` / `collab_handoffs` / `collab_messages` / `collab_reads` | 复用已实现的实际字段 | 不复制原记录；按 task 和 repo 校验；现有会话结束不删除历史。动工时核对实际实体与首版设计差异 |
| `collab_questions` | `id, repo_id, task_id, creator_kind, creator_id, recipient_kind, recipient_id?, title, body, options_json?, context_json, status, version, created_at, updated_at` | 状态 `open/answered/cancelled`；接收者 `session/user/repo_writers`，单 session/user 时 ID 必填；title 1–200 字符、body ≤16 KiB、最多 10 选项；索引 `(repo_id,task_id,status)` |
| `collab_answers` | `id, repo_id, question_id, actor_kind, actor_id, body, option_id?, context_hash, created_at` | 每问题最多一个有效答复；答复只追加，唯一 question_id；body ≤16 KiB；用户/Agent 来源分别记录，不推断真实人为身份 |
| `collab_takeovers` | `id, repo_id, task_id, from_session_id?, to_session_id, previous_task_version, outcome, reason, created_at` | 不可变历史，`outcome=claimed`；与任务 owner/version 更新同事务；不修改旧会话，不自动关闭待处理交接 |
| `collab_work_packages` | `id, repo_id, task_id, revision, author_kind, author_id, body_json, created_at` | `(task_id,revision)` 唯一；不可变版本；事务内锁任务或条件更新分配 revision；建议整体 ≤64 KiB |
| `collab_request_dedup` | `repo_id, actor_kind, actor_id, operation, client_request_id, payload_hash, result_kind, result_id, created_at, expires_at` | 唯一复合键；结果引用而非 secret/响应全文；在同事务创建；到期清理由后台维护 |

问题的 `context_json` 记录 `task_version`、可选 `base_sha`/`head_sha` 和关联链接；写入时做同仓库/格式校验。答复保留回答时的上下文哈希。工作包 `body_json` 包含 `goal`, `scope`, `artifacts[]`, `commit_sha?`, `completed[]`, `open_items[]`, `next_steps[]`, `evidence_refs[]`。每项提供 `source_kind` 和可选 `source_id`，最多 50 项；用户声明原样保存。对于可定位的同仓 pipeline/job，读取时由服务端独立解析为 `verified_reference`（含实际 SHA/状态/权限和查询时间）；缺失、不可访问或不支持时返回 `unavailable`，不能把自报测试当作验证成功。外部链接不自动抓取，避免 SSRF。

无需单独生成通知或收件箱持久事件表：首版从任务 owner、定向消息、pending handoff、open question 和 answers 构建单仓库投影。该投影不替代原始记录，活动与个人已读分别存储；其稳定快照机制见下节。涉及聚合的用户文本仍标记不可信。

## 4. 收件箱

### 接口

`GET P/inbox?session_id={id}&limit=50&cursor=...` → `{items, next_cursor, snapshot_at, has_more}`。需已认证仓库读权限且会话属于当前账号和仓库；读取该会话的个人已读状态需会话凭据。无会话的人工用户使用 `GET P/inbox/user`，仅包含发给该用户或 `repo_writers` 的问题，不继承某 Agent 的已读状态。默认 50，最大 100；不支持任意跨仓聚合。

`items[]` 是可定位的**原始事项**，含 `{item_key, type, task_id, object_id, task_version, occurred_at, state, read_state, origin, target}`。类型为 `assigned_task`、`direct_message`、`pending_handoff`、`open_question`、`question_answer`。相同对象可有多个类型的条目，`item_key` 由类型和对象稳定 ID 组合，不将不同事项错误合并。`state` 来源于当前权威记录；`read_state` 只反映阅读进度，不决定是否待处理。

初始请求先在一致性读事务中计算符合权限的有界快照，记录稳定的 `(type, object_id)` 列表及排序键 `(occurred_at, type, object_id)`，保存在服务端有 TTL 的快照存储（可为数据库表；每快照最多 1000 项，超过返回 `snapshot_truncated=true` 及收窄范围建议）。后续 cursor 绑定账号、仓库、会话、筛选与快照 ID 和偏移，完整性保护，默认 10 分钟有效。翻页时重验权限；对象状态以快照时数据为准并标记 `as_of`，执行前读取对象最新版本。快照到期返回 410 `CURSOR_EXPIRED`，重新开始。此机制保证**该快照**不遗漏或重复；期间的新活动由新快照发现，不假设自增 ID 或时间戳等于提交顺序。多实例下快照必须共享或显式返回可恢复错误，不能只存进程内缓存。

单任务消息已读仍使用首版 `collab_reads`；问题、交接等对象的个人阅读标记可新增 `collab_inbox_reads(repo_id, actor_kind, actor_id, item_key, observed_version, read_at)`，复合唯一；`PUT P/inbox/read` 接受 `{item_key, observed_version}`，只将实际返回的对象版本标记已读，较新版本仍显示未读；超前版本拒绝。该表仅表达阅读，不改变问题/交接状态。首版中的消息 read 不因标记其他事项而推进。

## 5. 离线接手

`POST P/tasks/{task_id}/takeovers`，Agent 请求：`{client_request_id, expected_task_version, expected_owner_session_id, reason}`。在同一事务中重验新会话有效和仓库写权限，以 task version + owner 比较更新 owner 为当前会话、递增版本，并插入 takeover 历史与幂等记录；返回 `{task, takeover}`。两个接手者竞争同一版本只有一个成功，另一方 409 并读取当前任务。

该动作是**声明接手，不是强制锁**。默认只允许 owner 为空、已结束或已过期时直接接手；仅心跳离线不能证明原 Agent 已放弃，返回 409 `OWNER_MAY_BE_ACTIVE`，可通过现有协作消息/交接请求协调。人工用户可使用 `POST P/tasks/{task_id}/reassign` 明确指定有效新会话，提交相同的版本和 owner 前提并留下原因；记录操作者为 user，但不声称账户持有人物理上是人类。原待处理 handoff 不自动接受或取消，保留其原状态并在响应列出定位；相关方按首版规则处理。原会话无 secret 丢失恢复时，新会话可浏览历史并在原会话结束/到期后接手。

## 6. 问题与答复

| 接口 | 请求 / 行为 |
|---|---|
| `POST P/tasks/{task_id}/questions` | `{client_request_id, recipient:{kind,id?}, title, body, options?, context:{task_version,base_sha?,head_sha?}}`；有效 Agent 会话或人工用户创建；创建时验证版本和引用，返回 201 question |
| `GET P/tasks/{task_id}/questions?status=&limit=&cursor=` | 仓库读者可列表；固定排序 `(created_at,id)`；按 ID 游标只遍历已有对象，不把变更列表当作实时事件流 |
| `GET P/questions/{id}` | 返回问题、答复、来源和 `context_freshness`；每次对 task 版本及引用分支当前 SHA 做检查。没有分支或无法读取时 `unknown`，不擅自判定 fresh |
| `POST P/questions/{id}/answer` | `{client_request_id, expected_version, body, option_id?, acknowledged_context_hash}`；仅指定会话、指定用户或仓库写者范围可答；在同事务中 CAS `open→answered`、插入唯一答复和幂等记录；并发仅一条成功 |
| `POST P/questions/{id}/cancel` | `{client_request_id,expected_version}`；问题创建主体可取消 open 问题，CAS 更新；与答复并发只能一个成功 |

问题和答复不可静默编辑；若答案需更正，创建关联新问题并在正文标明前一问题 ID（首版不推断前一答案失效）。选项 ID 由服务端生成并与问题绑定；无选项允许自由文本。人工答复可来自现有 Web 认证或 REST，无需创建 Agent 会话；UI 展示背景、选项、影响、上下文版本及关联链接。若客户端提交时上下文已变化，返回 409 `CONTEXT_STALE` 和新版本，须重新查看后提交；答复后上下文变化则显示 `stale`，无法确认时 `unknown`。状态仅 `answered` 不代表决定仍适用，不自动授权后续操作。

## 7. 工作包

| 接口 | 请求 / 响应 |
|---|---|
| `POST P/tasks/{task_id}/work-packages` | `{client_request_id,expected_task_version,goal,scope,artifacts,commit_sha?,completed,open_items,next_steps,evidence_refs}` → 201 `{package}`；在事务内核对任务版本，分配不可变 revision，不自动改变任务状态或 Git |
| `GET P/tasks/{task_id}/work-packages?limit=&cursor=` | 返回版本摘要，按 revision 降序，cursor 绑定 task 和筛选 |
| `GET P/work-packages/{id}` | 返回完整声明、来源标记和可解析证据的当前平台结果；附解析时间与缺失项 |

创建工作包时可引用同任务问题、消息、handoff 和仓库 Git SHA；逐项做归属与权限校验。内容由作者声明，服务端不代写总结；`missing_fields` 显示空目标、未提供 SHA 或证据等，但不阻止保存草稿性质的记录。首版交接可在 `summary` 中链接工作包；后续若新增 `work_package_id` 应单独迁移并验证关联，不修改既有 handoff 语义。列表默认只返回摘要，避免一次加载全部日志。

## 8. MCP 与 UI

MCP 映射 REST：`collab_get_inbox`、`collab_mark_inbox_read`、`collab_takeover_task`、`collab_create_question`、`collab_list_questions`、`collab_get_question`、`collab_answer_question`、`collab_cancel_question`、`collab_create_work_package`、`collab_get_work_package`、`collab_list_work_packages`。人工重新指派及人工答复由 Web/REST 用户凭据完成，不伪装成 Agent。MCP 内部持有 session secret，工具输出不可包含 secret；错误 `isError=true`，结果包含来源及截断信息。具体共享 client 和字段以首版最终实现为准。

Web 最小界面：单仓收件箱、任务中的问题与答复、工作包历史、人工重指派。必须展示当前状态、会话自报身份、版本变化提示及可打开的原始记录。用户答复前展示上下文检查结果；无权限的关联对象不显示敏感内容。可在 API 完成后独立实现，P0 用户决策验收必须实际覆盖 Web 或等价的人工客户端。

## 9. 实施顺序

| 批次 | 任务 | 前置 / 交付门 |
|---|---|---|
| P0-0 首版复核 | 对照已合并的实体、路由、鉴权、错误、MCP、迁移与真实双客户端验收；冻结契约和基线 | 首版 C01–C06 及相关验收达到适用门禁；未完成明确记录，不修改并行 Agent 正在写的文件 |
| P0-1 基础与接手 | 幂等存储、接手历史、任务 CAS 与人工重指派 | P0-0；新增迁移与数据约束经项目护栏处理；测试 P01–P04 |
| P0-2 问答 | 问题/答复模型、上下文版本与人工入口 API | P0-1 幂等基础；测试 P05–P08 |
| P0-3 工作包 | 不可变 revision、来源、证据引用解析 | P0-1；与 P0-2 可并行；测试 P09–P10 |
| P0-4 收件箱 | 聚合任务、消息、交接、问题和答复；快照与个人已读 | P0-1–P0-3；测试 P11–P13 |
| P0-5 客户端与闭环 | MCP、人工 UI/等价客户端、OpenAPI、文档、真实两客户端验收 | P0-1–P0-4；P14–P15；不依赖通用事件总线 |

产品路线先按“收件箱和离线接手”交付价值：P0-1 后可发布**仅含已有任务/消息/交接**的收件箱子集；P0-4 再补问题、答复和完整快照验收。若先发布子集，响应须显式标出支持的类型，不能宣称完整 P0 收件箱。

## 10. 验收矩阵

| ID | 场景 | 通过标准 |
|---|---|---|
| P01 | 原会话正常、离线但未过期、结束、过期各自接手 | 前两种不能自动抢占；后两种与无人负责可 CAS 接手；人工重指派有原因和来源 |
| P02 | 两个接手者 barrier 竞争同版本；中途任务更新 | 仅一个成功，其余 409；历史与 owner 原子一致；旧会话保持原状 |
| P03 | 原会话丢密钥、新会话接手、旧 handoff 待处理 | 新会话可读历史；不恢复旧会话；handoff 不被默默接受或删除 |
| P04 | 同幂等键并发、响应丢失后重试、不同请求体、撤权后重试 | 单次业务效果；同体同结果；异体冲突；撤权不泄露原结果 |
| P05 | Agent 提问，人工用户答复，关联上下文 | 答复主体、时间、选项、版本可追溯；无需 Agent 会话；不能充当合并审批 |
| P06 | 两个接收者同时答复，或答复与取消竞争 | 只产生一个终态，唯一答案及版本一致；败者得到冲突 |
| P07 | task 或 ref 在答复前后变化；分支不可解析 | 提交前 stale 被拒；答复后显示 stale；不可判断显示 unknown，不误报 fresh |
| P08 | 无权限、跨仓对象、非接收者答复 | 请求被拒、无部分写入，无私有数据泄露 |
| P09 | 并发创建工作包、重试同键 | revision 唯一、历史不可变、重复请求无额外版本；task 版本不符拒绝 |
| P10 | 自报测试、合法 CI 引用、不可访问或不存在引用 | 分别标识声明、平台结果、unavailable；不将自报当 CI 通过或抓取外部 URL |
| P11 | 多类型收件箱跨页时追加、修改和撤权 | 固定快照无漏项/重复；新活动下一快照可见；后续页重新鉴权；过期明确 410 |
| P12 | 已读后产生新版本；消息已读与交接待处理 | 新版本仍未读；read 不改变 handoff/question 状态；其他会话水位不变 |
| P13 | 多实例读取快照、服务重启、超过 1000 项 | 同一共享快照可续读或返回明确可恢复错误；截断显式报告；不能静默遗漏 |
| P14 | MCP 调用所有新增工具、错误和日志检查；Web 人工答复 | `isError` 正确，secret 不暴露；人工可查看问题和上下文并答复 |
| P15 | 真实 Git 仓库、真实服务、两个不同客户端同账号；A 断线，B 接手并向用户提问，用户答复，B 创建工作包 | 无复制聊天即可继续；SHA 和证据可核对；记录服务版本、数据库、命令、输出；不以 mock 替代 |

P02/P04/P06/P09/P11 用 barrier 和故障注入覆盖竞态；数据约束和事务在 SQLite、PostgreSQL、MySQL 分别验证。真实数据库或客户端不可用时对应验收标记未通过/待验证；记录实现、任务、集成、运行各层证据。

## 11. 待确认与开放选择

1. 同账号情境下人工用户与自动客户端可共用主凭据，因此 `actor_kind=user` 表示所走入口，不构成可靠“真人”身份。真正的人类审批依赖独立可信认证机制。
2. 收件箱快照持久化成本、TTL、1000 项限制及清理作业需要容量评估；若采用其他实现，必须证明相同权限、分页和并发语义。
3. 人工用户重指派的权限粒度（普通写者或管理员）为产品待定；本文暂建议写权限并保留审计。
4. 首版对仓库读者开放协作数据的可见性仍待确认；修改权限边界须另行评审。
