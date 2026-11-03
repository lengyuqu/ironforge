# 多 Agent 协作 P1：依赖、上下文、冲突、送达与决定

> 状态：Proposed，待评审；2026-11-04。细化[产品需求 R06–R11](agent-collaboration-product-requirements-2026-11.md)，与[P0 设计](agent-collaboration-p0-design-2026-11.md)及[协作首版](multi-agent-collaboration-design-2026-11.md)配套，不改变正在进行的首版工作。
> 本次仅静态查看代码：已有协作任务实体包含 `branch`、`paths_json`、`version` 和 `updated_at`；工作区中交接及 MCP 等改动仍在进行。设计不代表已实现、测试通过或合并。

## 1. 范围与共同契约

目标是在多 Agent 协作中减少重复问询、盲目等待、并行修改冲突及决定丢失。P1 依赖首版的任务、消息、会话、已读和交接；决定与上下文使用 P0 问题、答复和工作包。以单仓库为边界，跨仓依赖、全局事件和自动调度留待后续。

所有 REST 路径以前缀 `P = /api/v1/repos/{owner}/{repo}/collaboration` 开始；接口名称和字段均是提案。读取需认证和仓库读权限，协作写入需写权限；Agent 写还需有效会话凭据。用户入口依据现有认证并执行 CSRF 防护。每次读取、分页及事件消费重新鉴权；会话与展示名只是自报来源，不构成安全身份或人类审批证明。撤权的实际生效速度受权限缓存影响，部署和测试必须记录，不假设跨实例即时生效。

新增写操作沿用 P0 的 `client_request_id`、主体与动作作用域、请求哈希、事务内去重及 30 天保留建议；同体返回同结果，异体 409，重试重新鉴权。状态变更携带期望版本，冲突返回 409 并给出最新对象定位。文本均为不可信数据；不自动执行消息、合并或审批。错误 reason 均为提案，需与最终实现统一。

## 2. 数据模型

字段为候选，UUID/时间/外键及字符限制按首版实现规范统一，所有引用须事务内校验同仓库。P0 的问题、答复、工作包和幂等表复用，不重复存其正文。

| 对象 | 字段与约束 | 说明 |
|---|---|---|
| `collab_dependency_graphs` | `repo_id` 主键、`version` | 每仓库依赖图并发锁与版本；不能只锁单个任务 |
| `collab_task_dependencies` | `repo_id`, `task_id`, `depends_on_task_id`, `created_by_kind/id`, `created_at`；`(task_id,depends_on_task_id)` 唯一 | 有向边 A→B 表示 A 等待 B；禁止自身依赖、跨仓、重复边；限制每任务最多 100 条直接依赖 |
| `collab_decisions` | `id`, `repo_id`, `task_id`, `question_id?`, `answer_id?`, `title`, `rationale`, `context_json`, `actor_kind/id`, `created_at` | 不可变决定记录；正文和依据视为自报；问题/答复必须匹配任务、仓库且答复属于问题 |
| `collab_decision_changes` | `id`, `repo_id`, `decision_id`, `kind`, `replacement_decision_id?`, `reason`, `actor_kind/id`, `created_at` | `kind=superseded/revoked`；每个决定最多一个终结事件；替代决定同任务，不允许自引用和替代环；保留原文 |
| `collab_delivery_receipts` | `repo_id`, `message_id`, `target_session_id`, `read_at`；`(message_id,target_session_id)` 唯一 | 仅读取目标为该会话的消息时由该会话凭据确认；不等于同意或答复 |
| `collab_notices` | `id`, `repo_id`, `recipient_session_id`, `kind`, `source_type/id`, `generation`, `created_at`, `ack_at?` | 持久化待处理提醒，`(recipient_session_id,kind,source_type,source_id,generation)` 唯一；通知为提示，不作为权威状态 |

依赖、决定和通知的唯一约束必须在数据库中落实；权限过滤不可仅在客户端执行。消息送达首版只覆盖单一明确目标 `target_session_id`，不推断群发消息的收件人集合。交接沿用现有 handoff 原始记录及其版本，不另建平行状态机。

## 3. 任务依赖 R06

| 接口 | 契约 |
|---|---|
| `GET P/tasks/{id}/dependencies` | 返回直接依赖及其任务状态、图版本；列出未完成依赖和最近更新时间。状态是声明，不代表验证证据 |
| `POST P/tasks/{id}/dependencies` | `{client_request_id,depends_on_task_id,expected_graph_version}`；返回边与新图版本；版本不符 409 |
| `DELETE P/tasks/{id}/dependencies/{depends_on_id}` | 带 `{client_request_id,expected_graph_version}`；删除边及递增图版本；重复请求按幂等契约处理 |

每次增删在一个事务中以 repo 图版本行做条件更新或等效锁定，再检查目标是否能经依赖边到达源；能到达则拒绝 `DEPENDENCY_CYCLE`。版本锁、环检查与边插入共同提交，保证并发新增 A→B、B→A 不会同时成功。受限遍历设置最大 1000 个节点 / 5000 条边；超出返回 `GRAPH_LIMIT_EXCEEDED`，不可当作无环。SQLite 的写锁及 PostgreSQL/MySQL 的行锁/隔离级别分别验证；禁止仅依赖应用进程内锁。

当 B 从非 done 变为 done 时，对依赖它的 A 的当前 owner 会话创建持久提醒；B 再次打开时将提示标记为过时或以当前状态重新计算。通知写入与状态变更同事务；不存在有效 owner 时仍可在依赖列表发现状态，后续接手者以查询为准。提醒仅建议检查依赖，不自动修改 A 的状态或运行任务。状态变化、接手和提醒并发时，消费者打开任务必须读当前依赖，不能仅凭旧通知行动。

## 4. 上下文包 R07

`GET P/tasks/{id}/context?max_bytes=32768&sections=...` 返回 `{task_id, as_of, task_version, base_sha?, head_sha?, sections, truncation, consistency}`。`max_bytes` 默认 32 KiB、范围 4–256 KiB，限制 UTF-8 JSON 序列化后的完整响应字节数；超限时按确定顺序裁剪条目或文本，保留必要元数据，不能切断 UTF-8。预算过小无法容纳最小 envelope 时返回 400 `BUDGET_TOO_SMALL`。可选 `fields/sections` 控制请求，实际默认优先顺序：任务目标与版本 → 未答问题 → 当前决定 → 待接手交接 → 最新工作包 → 依赖/冲突 → 最近消息。

各 section 返回 `{items, returned_count, has_more, next_cursor?, source, captured_at}`；条目有原对象 ID、版本、时间、来源类型、原文定位与 `origin`。`truncation` 明确列出遗漏/截断节和字段；内容过长返回安全的逐字段截断标记和完整对象链接，绝不把摘要当原文。`max_bytes` 为确定性大小上限，不用 `chars/4` 假装精确 token；客户端可自行计算模型 token。

首期采用数据库一致性读快照获取可读协作数据及版本；Git 分支解析不与 DB 同事务，分别返回各自 `observed_at`、解析 SHA 及 `consistency: {db:"snapshot",git:"independent"}`；ref 不存在或无权限用 `unavailable`，不补猜。分页 cursor 绑定仓库、任务、section 和快照，使用 P0 的共享有界快照与过期规则；未实现稳定快照时拒绝承诺跨页一致，不复用会漂移的 offset。后续可增加生成式摘要，但必须可选、标注模型产物、列出来源且不替代原记录；首版不需要 LLM。

## 5. 冲突预警 R08

`GET P/tasks/{id}/overlaps?limit=&cursor=` 比较当前任务与同仓未完成任务的声明路径，返回 `{items:[{task_id,owner_session_id,branch,overlaps,task_version,declared_at,confidence}],next_cursor,as_of}`。仅比较明确相对路径：相等或以 `/` 边界的目录前缀相交，不使用 glob、符号链接展开或文件系统访问；拒绝绝对路径、`..` 和 NUL，大小写按 Git 路径字节区分。结果按 task ID 稳定排序，分页使用快照；同时返回双方路径原文及重叠原因。

声明与真实变更可能不一致。`confidence` 仅为 `declared_current` 或 `declaration_stale`，基于声明更新时间和服务端配置阈值（建议 24 小时）；任务更新的其他字段不能刷新路径声明时间。需要新增 `paths_declared_at` 时使用新迁移，不能将任务 `updated_at` 冒充路径确认时间。离线会话或过期声明仍可展示但标记不确定；没有声明不等于没有冲突。此接口只是提示，不能阻止写、获取锁或代替 Git ref CAS/合并冲突检查。

## 6. 消息送达与交接 R09–R10

`GET P/tasks/{id}/messages/{message_id}/delivery` 给消息作者返回 `{saved:true,target_session_id,read_at?,replied_at?,reply_message_id?,status}`；无明确目标返回 `status=not_targeted`。`saved` 仅说明消息持久化，`read_at` 仅由目标会话凭据显式确认；`replied_at` 仅取目标会话对该消息的直接回复并标注来源，不代表已解决或同意。其他仓库读者如可查看消息，也不能借此查看非授权的个人阅读状态。

`POST P/tasks/{id}/messages/{message_id}/read-receipt` 接受 `{client_request_id}`，仅目标会话可调用，幂等写入 receipt；服务端校验消息目标与任务、仓库。首版 `collab_reads.last_read_seq` 保持原有功能，**不自动推导逐条已读回执**：水位可能代表批量跳过而非真实阅读。新的 receipt 不自动移动水位。过期会话不能被新会话冒充，新会话另行沟通；无回执表示未知，不等于未读。

交接继续使用首版 `pending/accepted/cancelled` 及 CAS；`GET P/handoffs/{id}` 返回状态、版本、目标和时间。若目标会话过期，由创建者先按首版规则取消，再创建一个新 handoff 并显式关联旧 ID（可增可选 `supersedes_handoff_id`，新迁移、同任务校验）；两步之间可能中断，列表必须能显示旧交接已取消、新交接尚未创建，不能伪装为原子重指派。未来若提供原子重指派，必须单独定义并发与幂等契约。已接受的交接不能静默变更目标。

通知首期按持久 `collab_notices` 查询：`GET P/notices?session_id=&limit=&cursor=`；目标会话凭据和仓库读权限必需。标记处理 `POST P/notices/{id}/ack` 只影响 notice，不改变任务/消息/交接。通知按单会话稳定快照分页并可重拉；重复投递以 notice ID 去重。推送可选，断连后仍通过持久查询恢复。给无权仓库或已过期会话不得投递内容；会话结束后不向新会话自动转移通知，但新会话可经 P0 收件箱发现任务与公开协作状态。提醒与业务状态须同事务写入；如未来引入异步分发，使用事务 outbox 和重试，不能依赖进程广播保证送达。

## 7. 决定追溯 R11

| 接口 | 契约 |
|---|---|
| `POST P/tasks/{id}/decisions` | `{client_request_id,title,rationale,question_id?,answer_id?,context:{task_version,base_sha?,head_sha?}}`；校验同任务关联和任务版本，创建不可变记录 |
| `GET P/tasks/{id}/decisions?limit=&cursor=` | 返回不可变决定与终结事件；稳定顺序与 cursor；每条含来源、时间、关联答复和上下文新鲜度 |
| `GET P/decisions/{id}` | 返回原决定、引用与当前状态，显示 `fresh/stale/unknown`；状态与 P0 问答的检查规则一致 |
| `POST P/decisions/{id}/changes` | `{client_request_id,kind,replacement_decision_id?,reason}`；写入唯一终结事件；`superseded` 必须关联同任务替代决定、无环；并发只有一个终结事件成功 |

创建者与协作账号来源均可追溯，但不证明谁是人类；决定不能成为分支保护或 Git 写入授权。替代/撤销只添加事件，历史始终可查。若上下文改变，旧决定仍可查但标注 stale；不得自动将其改为 revoked。未来由 LLM 生成决定草稿时须标注并指向证据，实际提交仍需要有权限的主体。

## 8. 实施任务与依赖

| ID | 交付 | 依赖 / 完成门 |
|---|---|---|
| U00 | 核对首版和 P0 合并结果，冻结认证、事务、分页与现有 handoff 契约，记录真实工作区 SHA | 首版完成相应验收；P0 模型设计定稿后映射，不把 Proposed 当已存在 |
| U01 | 图版本、依赖事务、完成提醒 | U00；N01–N03；新迁移按项目护栏 |
| U02 | 决定及终结事件、关联和新鲜度 | U00 + P0 问答；N04–N05 |
| U03 | 声明路径时间和冲突预警 | U00；N06–N07；可与 U01/U02 并行 |
| U04 | 消息 receipt、通知、handoff 关联历史 | U00；通知与业务同事务；N08–N10 |
| U05 | 上下文包与预算、稳定快照和出处 | P0 快照/工作包 + U01–U04 中实际纳入的字段；N11–N12 |
| U06 | MCP、客户端和 Web/等价人工界面；真实双客户端闭环 | U01–U05；N13–N14；各项已验证后才声称 P1 完成 |

优先交付依赖和手动可读状态，再补通知；决定与冲突预警可独立实施。上下文接口可分 section 发布，但必须声明实际包含的 section、版本和截断情况。各数据库测试和仓库护栏不因并行实施而省略。

## 9. 验收标准

| ID | 情景 | 通过条件 |
|---|---|---|
| N01 | A 等待 B、B done 后 reopened | 显示依赖状态及持久提示；reopened 不被旧通知误导；A 不自动执行或改变状态 |
| N02 | barrier 并发添加 A→B 与 B→A；并发修改长环 | 不能形成环；至少一方成功或结构化冲突；SQLite/PostgreSQL/MySQL 分别验证 |
| N03 | 图达到节点或边上限、跨仓依赖、撤权 | fail closed，无部分写入和跨仓信息泄漏 |
| N04 | 创建决定、关联不匹配答复、版本过期 | 仅合法关联成功；错误不产生记录；来源与 SHA 可追溯 |
| N05 | 两方并发撤销与替代；替代环；上下文变化 | 每个决定最多一个终结事件；环被拒；历史保留；新鲜度正确区分 stale/unknown |
| N06 | 声明相等、目录前缀、相似但非边界、大小写、非法路径 | 仅正确交集有提示；非法输入拒绝；不能操作文件系统 |
| N07 | 仅更新任务状态、路径声明过期或 owner 离线 | 路径声明时间不被其他更新刷新；不确定状态可见；不阻止 Git 写入 |
| N08 | 发送消息、推进任务已读水位、显式 receipt、回复 | 分别展示 saved、目标确认已读和目标回复；水位不伪造回执；非目标无法确认 |
| N09 | 交接 accept 与 cancel 并发、目标过期后取消再新建、两步间中断 | 不出现两个终态；新旧交接可追溯；中断状态可解释，不假装原子 |
| N10 | 断线、重复拉取、多实例、通知 ack、撤权 | 持久 notice 不遗漏，重复可去重；ack 不改变业务；无权无法读；提醒与业务事务一致 |
| N11 | 超长中文、特殊字符、多 section 和最小预算 | 完整 JSON 字节数不超过上限，UTF-8 完整，来源/截断与可续读入口明确 |
| N12 | 分页期间任务/分支变化和 cursor 到期 | 协作快照一致、Git 独立观测与 SHA 清晰；游标到期明确恢复，不拼混版本 |
| N13 | MCP 工具和用户界面验证权限、错误和来源 | 每种错误可机器识别；密钥不进入输出；模型文本标注不可信 |
| N14 | 两个真实不同客户端共用账号，在真实仓库建依赖、发消息、交接、记录决定、恢复上下文 | 状态和证据可回溯，无需复制聊天；保存环境、SHA、命令与结果；不以 mock 冒充运行验证 |

并发使用 barrier/可控事务与故障点，不依赖 sleep 证明；各测试记录 SQLite、PostgreSQL、MySQL 实例和结果。未接入真实数据库或双客户端时，对应项标记待验证。

## 10. 待决事项

- 声明过期阈值、图规模上限、上下文预算和通知保留期需依据真实负载调整；上文数字是初始建议。
- 目标会话永久过期后是否允许用户把未处理事项定向新会话，需要先明确授权和历史语义；本方案保留原记录，不自动转移。
- P0 收件箱快照与 notices 的去重和保留策略应统一评审，避免建两套长期事件系统；任何实现都不得假设 ID 分配序与事务提交序相同。
- 提案涉及新增表和可能新增字段，实施依项目迁移与权限护栏；本文不表示授权执行迁移，也未宣称已有功能通过验收。
