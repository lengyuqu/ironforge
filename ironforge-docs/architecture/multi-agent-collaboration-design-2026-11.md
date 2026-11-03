# 同账号多 Agent 协作：首版设计与实施任务

> 状态：Proposed，待评审；2026-11-04。本文是[整合路线](harness-integrated-roadmap-2026-11.md) S1 的细化，不表示功能已经实现或验证。
> 来源：用户描述的使用场景、整合路线及当前工作区静态检查。当前工作区有未提交改动；实现状态须在动工时重新核对。

## 1. 目标与边界

两个不同 harness 使用同一个账号和仓库时，能够各自注册会话、发现对方、围绕同一任务交流、声明阻塞、交接，并在断线后恢复消息和状态。接口由 REST 提供权威语义，MCP 是适配层。首版支持持久化和轮询，不依赖进程内广播或通用事件系统；不构建 Agent 调度器，不自动执行消息内的命令。

当前代码证据：`crates/rg-mcp/src/tools/mod.rs` 列出五个只读工具；`crates/rg-http/src/api/repo_access.rs` 提供仓库权限入口；`crates/rg-http/src/ws.rs` 的通知广播以用户为单位。上面不证明协作能力已实现，也不将用户级通知当作会话收件箱。

## 2. 身份、权限与威胁边界

- 账号是授权主体，会话是可追踪的协作身份。会话 ID 由服务端生成 UUID v4；名称、客户端类型、状态和交接内容均为自报，不证明 agent 类型或工作完成。
- 创建会话时额外生成 256 位随机 `session_secret`，仅返回一次，服务端只存带域分离的摘要；后续以 `X-Ironforge-Session` 和 `X-Ironforge-Session-Secret` 提交，并同时进行正常账号鉴权和仓库权限检查。密钥不能放 URL、审计详情或日志。
- **同账号不是安全隔离边界**：共用主凭据的客户端可以新建自己的会话并冒用展示名，也可访问该账号获准访问的协作内容。会话密钥只防止凭公开 ID 意外冒用或无密钥续接，无法证明客户端真实身份、抵御掌握相同主凭据的恶意客户端。需要更强归属时使用后续受限的独立凭据。
- 读取协作数据要求认证且具仓库读权限；创建会话、消息、任务、交接和修改状态要求仓库写权限。跨仓库对象返回 404；未认证 401，无权限按现有仓库访问契约返回 403/404。每次请求重查授权；权限撤销立即阻止后续访问，部署的权限缓存一致性限制在实施前验证并记录，不能声称跨实例立即撤权。
- 会话可修改自身心跳、结束状态和已读位置。任务更新允许任何有写权限的会话，但需版本条件；交接仅创建者可撤销、指定接收会话可接受，均需有效会话密钥。管理员代管、人工 Web 操作留后续设计。批准规则不依赖会话自报类型。
- 消息、路径、SHA、链接和交接内容均为不可信数据；返回中标注 `origin: "user_content"`，不触发命令、合并、批准或外发通知。首版无强制锁。

## 3. 领域模型与生命周期

表名、约束与迁移为建议，字段类型须与现有多数据库规范对齐；以下 ID 为服务端 UUID，账号与仓库 ID 复用现有类型。所有表显式指定名称，建立外键或等效一致性检查；涉及多个对象时强制同一 `repo_id`。时间 UTC，客户端不决定顺序。

| 表 | 主要字段 | 约束及用途 |
|---|---|---|
| `agent_sessions` | `id`, `repo_id`, `user_id`, `secret_hash`, `display_name`, `client_kind`, `created_at`, `last_seen_at`, `expires_at`, `ended_at` | repo/user 外键；索引 `(repo_id,last_seen_at,id)`；名称最多 80 字符、类型最多 64。过期不删除历史 |
| `collab_tasks` | `id`, `repo_id`, `title`, `description`, `status`, `owner_session_id?`, `branch?`, `paths_json`, `issue_number?`, `pr_number?`, `version`, `next_seq`, `created_at`, `updated_at` | title 1–200 字符，description ≤16 KiB；最多 100 条相对路径、每条 ≤512 字节；`version` 乐观锁；索引 `(repo_id,updated_at,id)` |
| `collab_messages` | `id`, `repo_id`, `task_id`, `seq`, `author_session_id`, `kind`, `body`, `reply_to_id?`, `target_session_id?`, `commit_sha?`, `client_message_id`, `payload_hash`, `created_at` | `(task_id,seq)`、`(author_session_id,client_message_id)` 唯一；body 1–32 KiB；kind = `note`/`help`/`blocker`；按 task+seq 索引；不可编辑或删除 |
| `collab_handoffs` | `id`, `repo_id`, `task_id`, `from_session_id`, `to_session_id`, `summary`, `completed`, `next_steps`, `blockers`, `commit_sha?`, `evidence_json`, `status`, `version`, `created_at`, `accepted_at?` | 状态 `pending`/`accepted`/`cancelled`；每段 ≤16 KiB，证据最多 20 个 URI/ID；声明非证明；CAS 更新 |
| `collab_reads` | `task_id`, `session_id`, `last_read_seq`, `updated_at` | 主键 `(task_id,session_id)`；单调递增，不能超过已提交的最大 seq；每会话独立 |

`branch` 和 paths 只是声明，不操作 Git；关联 issue/PR 创建或更新时校验属于当前仓库；SHA 若提供须是完整、合法的仓库 commit（或明确返回无效），不等于验证通过。首版证据字段是声明链接，读取方自行验证；不能当合并门禁。路径不接受绝对路径、`..` 或 NUL，不执行 glob。

会话创建默认有效 8 小时；每次心跳只更新 `last_seen_at`，不延长硬到期时间。最近 90 秒内有心跳且未结束/未过期显示 `online`，其余显示 `offline`；离线并非结束，结束/过期不可恢复，客户端保留密钥可在硬到期内重新连接同一会话。历史按仓库保留，首版无自动清理；运维删除策略后续单独设计。任务状态为 `open`、`in_progress`、`waiting`、`blocked`、`done`；允许任意状态经 CAS 转移，`done` 可重新打开。状态只代表声明，不能自动推进 Git/CI。

为避免提交顺序导致漏消息：每次发消息在数据库事务内锁定对应任务行，以原子方式递增 `next_seq`、插入消息并提交；同一任务内的序号和提交可见顺序一致。不同任务间不保证全局顺序。使用 backend 支持的行锁或等价条件更新/事务重试；SQLite/PostgreSQL/MySQL 均须验收。消息和交接关联状态的改变在同一 DB 事务提交；首次实现交接不额外自动生成消息，客户端分别显示交接列表。

## 4. REST 契约

所有路径前缀：`/api/v1/repos/{owner}/{repo}/collaboration`。时间使用 RFC 3339 UTC；分页 `limit` 默认 50、最大 100。会话凭据如上两头；错误复用现有 `{error:{code,message,request_id}}`，在新增 reason 可用前禁止客户端依赖解析 message。字段及接口均为待实现提案。

| 方法与路径 | 输入 / 响应 | 行为 |
|---|---|---|
| `POST /sessions` | `{display_name,client_kind}` → 201 `{session,session_secret}` | 新注册；密钥只返回一次；重复注册产生新会话 |
| `GET /sessions?limit=&cursor=` | → `{items,next_cursor}` | 仓库写权限用户可发现会话；只返回公开字段与计算状态 |
| `POST /sessions/{id}/heartbeat` | 无 body → `{session}` | 必须本人会话凭据；过期或结束返回 409 `SESSION_EXPIRED` |
| `POST /sessions/{id}/end` | 无 body → 204 | 本人结束，重复调用返回 204；权限撤销时仍不能用旧凭据绕过鉴权 |
| `POST /tasks` | `{title,description?,owner_session_id?,branch?,paths?,issue_number?,pr_number?}` → 201 task | 当前有效会话创建；owner 可为空或有效同仓库会话 |
| `GET /tasks?status=&limit=&cursor=` | → `{items,next_cursor}` | 认证读权限；按 `(updated_at,id)` 排序，游标含筛选条件；变动中的列表为最终一致，完整同步需重新扫描 |
| `GET /tasks/{id}` | → task + `version` | 读最新状态 |
| `PATCH /tasks/{id}` | `{expected_version,...fields}` → task | 条件更新；不匹配 409 `VERSION_CONFLICT`，返回 current_version；部分字段省略表示不变，显式 null 清空可选字段 |
| `POST /tasks/{id}/messages` | `{client_message_id,kind,body,reply_to_id?,target_session_id?,commit_sha?}` → 201 message | 同会话同 client ID 同体重试返回 200 原消息；异体 409 `IDEMPOTENCY_KEY_REUSED`；reply 必须同任务，target 同仓库 |
| `GET /tasks/{id}/messages?after_seq=&limit=` | → `{items,next_after_seq,has_more}` | seq 升序；按返回最后一条推进；空页保留输入 seq；持久轮询，无时间等待 |
| `PUT /tasks/{id}/read` | `{last_read_seq}` → `{last_read_seq}` | 对本人会话执行 max 更新；大于已提交最大 seq 返回 400 |
| `POST /tasks/{id}/handoffs` | `{to_session_id,summary,completed,next_steps,blockers,commit_sha?,evidence?,client_handoff_id}` → 201 handoff | 目标必须同仓库；按 `(from_session_id,client_handoff_id)` 幂等及请求哈希检查，需增加对应唯一约束 |
| `GET /tasks/{id}/handoffs?limit=&cursor=` | → `{items,next_cursor}` | 按创建时间及 ID 稳定排序，返回状态和版本 |
| `POST /handoffs/{id}/accept` | `{expected_version}` → handoff | 目标会话接受 pending；原子 CAS；并发冲突返回 409 |
| `POST /handoffs/{id}/cancel` | `{expected_version}` → handoff | 创建会话取消 pending；与 accept 竞争时仅一个成功 |

示例：A 发 `{ "client_message_id":"a-42", "kind":"blocker", "body":"测试失败，请检查构建日志", "target_session_id":"<B>" }`；B 从 `after_seq=0` 拉取消息，确认后提交已读水位。A 向 B 创建 handoff，B 带自己的会话凭据接受。被指定消息只是可筛选的记录；首版未读数通过 `seq > last_read_seq` 计算，包含该任务所有消息，不提供跨任务全局未读计数或承诺实时推送。交接状态与消息已读彼此独立。

分页游标应为不透明、完整性保护的服务端编码，绑定仓库、筛选、排序与版本；篡改或不匹配返回 400。消息只追加不删除，`after_seq` 可长期续读；任务列表会变化，不承诺固定快照。目标会话可离线或到期，交接保留 pending，创建者可取消并重新指派至新会话。

## 5. MCP 与客户端

首版在现有 stdio server 增加 `collab_start_session`、`collab_list_sessions`、`collab_create_task`、`collab_get_task`、`collab_update_task`、`collab_list_messages`、`collab_send_message`、`collab_mark_read`、`collab_create_handoff`、`collab_list_handoffs`、`collab_accept_handoff`、`collab_cancel_handoff`、`collab_heartbeat`、`collab_end_session`。参数和错误与 REST 一致；输出 JSON 中错误设置 MCP `isError=true`，不把失败包装为成功文本。tool schema 包含必填、长度、枚举与版本条件；回复只返回必要字段并标注不可信来源。

MCP 进程内暂存 session ID 与 secret，不向模型返回 secret；持久续接由宿主提供安全的凭据存储或显式重新注册，不把密钥写入仓库或聊天记录。进程崩溃且未安全保存密钥时不能续接旧会话，重新注册新会话后仍可读任务和历史消息；已读位置按新会话重新建立。多进程同会话续接须由宿主安全分发密钥；首版不提供密钥找回。HTTP 客户端可自己安全保管会话密钥。标准仓库写权限允许协作消息写入，但这不开放 Git 写 MCP 工具；Git 写入门禁仍遵循整合路线 S2–S4。

## 6. 任务拆分与依赖

所有任务初始状态 `TODO`；路径为候选，实施前按现有架构复核。迁移、认证、权限、AppState 等遵守项目护栏；本文是设计，不代表已授权执行受确认约束的变更。

| ID | 任务与建议位置 | 依赖 | 完成标准 |
|---|---|---|---|
| C00 | 核验认证路径、实际权限缓存、三数据库与并行工作区；确定现有 API 和审计扩展点 | 无 | 形成证据及限制记录，列出明确改动文件；不覆盖在制修改 |
| C01 | `rg-db` 增加 5 表迁移、实体、索引、约束及数据库操作 | C00 | fresh DB、升级和三后端事务/唯一约束测试通过；迁移实体表名一致 |
| C02 | `rg-core` 会话生命周期、权限调用和密钥校验；`rg-http` 会话接口 | C01 | T01–T03、T09 通过 |
| C03 | 任务与消息事务、CAS、幂等与分页；REST 接口 | C01–C02 | T04–T06、T08、T10 通过 |
| C04 | 交接状态机、已读水位和接口 | C03 | T07–T08、T10 通过 |
| C05 | `rg-mcp` 实现协作工具、会话密钥存取和结构化错误 | C02–C04 | schema/错误/密钥泄露测试，T11 通过 |
| C06 | 真实双客户端端到端和文档/OpenAPI；更新 `.ai/README.md` | C05 | T12 真实闭环通过，记录 SHA、命令和运行环境 |

C02–C04 可先为 REST 交付；只读客户端可并行开发。新增迁移前核验三个数据库的约束和迁移约定；不要更改已执行迁移。完成状态应区分实现、任务测试、集成测试及真实运行，不能用文档或 mock 替代真实双客户端验收。

## 7. 验收标准

| ID | 验证场景 | 判定 |
|---|---|---|
| T01 | 同账号创建 A/B 两会话，相同展示名也可用 | ID 不同，消息关联正确；自报标签不改变权限/批准资格 |
| T02 | 错误密钥、不同账号、不同仓库、结束或过期后访问；撤销仓库权限 | 按权限拒绝；不能靠 ID 接管；缓存限制通过实测确认并记录 |
| T03 | 断连后使用原凭据续接、进程丢失 secret 后新注册、离线超时 | 原会话可续接至硬过期；丢 secret 不能冒用；offline 不等于结束或完成 |
| T04 | 两会话同时 PATCH 同版本任务 | 恰一成功，另一得到 409 和当前版本；不会静默覆盖 |
| T05 | 并发同 client_message_id 同体/异体，模拟提交成功后响应丢失 | 同体恰一条，重试返回同 ID；异体冲突；不产生重复消息 |
| T06 | 用 barrier 控制两个并发事务先后提交，分页同时追加、重启服务后续读 | 同任务 seq 可见顺序一致，无永久遗漏或重复；跨任务顺序不作保证；三数据库分别执行 |
| T07 | 两个会话并发 accept/cancel 同一 pending 交接 | 只有合法主体的一个终态成功；另一方 409，版本和状态一致 |
| T08 | 已读顺序乱序、并发推进、超出最大 seq、重启 | 水位单调、不超过已提交消息；两会话水位独立且持久 |
| T09 | 验证响应、服务日志、审计、MCP 输出 | secret 只在 REST 创建响应出现一次；其他响应/日志及模型可见内容均无密钥 |
| T10 | 跨仓库 task/reply/target/issue/PR、无效 SHA/路径、超限内容 | 拒绝且无部分写入；私有仓库信息不泄漏 |
| T11 | MCP 参数、错误、调用链和进程重启 | schema 正确；错误 `isError`；按密钥持久策略恢复或明确重注册 |
| T12 | 真实服务和真实 Git 仓库中，两不同客户端同账号：注册、认领、消息、阻塞、交接、接受、断线续读 | 无人工复制聊天即可继续；确认 SHA 与声明分开，记录请求、环境与结果；不使用 mock 冒充真实运行 |

验证前冻结测试条件和目标 SHA；保留命令输出及数据库后端版本。文档可独立评审，不等于任何 T 项通过。

## 8. 待决事项

1. 同账号协作数据面向所有仓库读者还是仅写者可读：当前建议读权限可读、写权限可写；如消息包含敏感协作信息，应在实施前由产品确定范围。
2. session secret 在桌面、CLI 和不同 MCP 宿主的具体安全存储方式；不支持安全存储的宿主默认重注册。
3. 不同账号协作、管理员代管、长期保留与清理、消息编辑删除、在线推送和全局收件箱均为后续范围；先评估用户需求与资源开销。
4. S1 与受限 agent token 尚未落地时，用户可用现有账号凭据完成协作；不能将其描述为已具备细粒度 Agent 身份隔离。
