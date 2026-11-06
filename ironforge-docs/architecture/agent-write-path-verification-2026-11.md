# Agent 写路径：令牌身份、按分支提交、写语义、MCP 写工具（验证记录，2026-11-06）

> **状态**：已实现，SQLite 全量验证 ｜ **可信度**：确认（附运行证据）｜ **来源**：仓库代码 + 本次运行
> 接 [agent-token-ref-allowlist-verification-2026-11.md](agent-token-ref-allowlist-verification-2026-11.md)（分支白名单）。设计见 [harness 原生化设计](harness-native-design-2026-11.md) §4.4、§4.5、§4.6、§4.7、§4.3（后半）。**创建 PR 的 dry-run、`refs/pull`、PR `export`、streamable-HTTP、SSE 没有做。**新增两个迁移（`m20261106_000003_create_idempotency_keys`、`m20261106_000004_create_repo_events`，均为新表、`up` 幂等，项目负责人已确认）。

## 做了什么

### 1. 令牌身份带进会话 JWT（H3）

- `Claims` 新增可选 `tid`（`#[serde(default, skip_serializing_if = "Option::is_none")]`）：PAT 中间件换出的会话 JWT 带上令牌 id，普通登录没有。旧 JWT 解码为 `None`，不受影响。生成函数 `jwt::generate_token_for(…, Some(token_id))`，`generate_token` 行为不变。
- 会话 JWT 只存在于服务端换出的 `Authorization` 头里，客户端拿不到。
- `api::auth::session_access_token` 按 `tid` 重新取令牌：令牌已被删除、已过期、或不属于该用户 → 403 `access token is no longer valid`。**删除令牌因此也终止它已经换出的会话**（此前换出的 1 天 JWT 在删除后仍可用）。

### 2. 按分支的服务端文件提交（H3）

- 带分支限制的令牌现在可以用 `POST/DELETE /repos/{o}/{n}/contents/**`：中间件放行这条路由（`ref_limited_write_allowed` 新增 `["contents", _, ..]`），两个处理函数调用 `ensure_token_may_write_branch`，目标分支不在令牌名单内 → 403 `this access token may not write to this branch`。无分支限制的令牌、普通登录不受影响。
- **放行条件**：一条路由只有在它的处理函数自己检查分支之后，才能加进 `ref_limited_write_allowed`。合并、应用建议、发布等其余写路由仍对这类令牌默认拒绝。

### 3. 审计里的令牌身份（H3）

- PAT 中间件在每个 `/api/v1` 请求上**先删掉**客户端可能带来的内部头 `x-ironforge-access-token-id`，只在请求确由 PAT 发起时写入真实令牌 id。
- 现有 5 处审计写入（`repos`、`users`、`orgs`、`admin`、`collab_groups`）经 `api::audit::with_access_token` 把 `access_token_id` 合进 `details`（`details` 非对象时放进 `detail` 键，不覆盖原内容）。普通登录的条目不变。
- **只覆盖已有的审计点**：文件提交、PR 创建/合并等本来就没有审计记录的操作，这次**没有**新增审计条目。

### 4. 受限令牌不能批准 PR（H3）

- 带分支限制的令牌视为 Agent 令牌：`POST …/pulls/{n}/reviews` 的 `action: approve` 返回 403。评论、`request_changes`、行内评论仍可用。同一个人用普通登录或无分支限制的令牌可以批准。
- 选择"拒绝"而不是"记录但不计数"，因为后者需要给评审表加列（迁移）；拒绝没有迁移且更严。

### 5. 删除未挂路由的 v2 receive-pack（H3）

- 删除 `git_v2.rs::handle_git_receive_pack_v2`（全仓库只有定义，不带分支保护/令牌检查）。文件里留下一段注释说明为什么不应该再有第二个推送入口。

### 6. 写语义：`expected_sha`、`dry_run`、错误 `reason`（H4）

- **错误体**新增可选字段 `reason`、`retryable`、`hint`（只在有这些信息的错误上出现，旧字段不变）。`AppError::conflict_because(reason, retryable, msg, details, hint)` 产生 `code: CONFLICT` + `reason`。
- **文件提交**（`POST/DELETE contents/**`）：
  - `expected_sha`：分支当前提交必须等于它，否则 409 `reason: REF_ADVANCED`，`details: { current_sha, expected_sha }`，`retryable: true`。分支不存在也算"不在该提交"（`current_sha: null`）。检查之后、写入之前分支又被推进，同样返回 `REF_ADVANCED`。
  - `dry_run`（请求体/查询参数，或头 `X-Dry-Run: 1`）：按真实调用同样的顺序做分支名/路径校验、分支保护、令牌分支限制、"文件已存在/blob sha 不符"、`expected_sha`，然后在**创建分支和写入之前**返回 `{ dry_run: true, would: [ref_update{ref, old, creates_branch}, file{operation, path, blob_before}], not_evaluated: [webhook, ci_trigger, notification] }`。`not_evaluated` 如实列出没有计算的效果。
  - 服务层是 `create_or_update_file_with` / `delete_file_with`（带 `FileCommitOptions`，返回 `FileCommitReport`，错误是 `FileCommitError::{RefAdvanced, Core}`）；原 `create_or_update_file` / `delete_file` 签名和行为不变，作为包装保留。
- **PR 合并**：`expected_head_sha`（head 分支的**实时**提交，fork PR 取 fork 仓库）不符 → 409 `REF_ADVANCED`；`dry_run` 做草稿/状态/分支保护/`expected_head_sha` 检查后报告 `would: [merge{strategy, base, head, head_sha}]`，`not_evaluated` 含 `conflicts`（能否干净合并只有真合并才知道）。**`expected_head_sha` 是合并开始前的检查，不是原子保护**：在检查与合并之间到达的推送不会被拦下。

### 7. MCP 写工具与 CI 等待（H5）

新增 5 个工具（`rg-mcp/src/tools/mod.rs`）：

| 工具 | 路由 | 说明 |
|---|---|---|
| `commit_file` | `POST contents/{path}` | `branch` 必填（Agent 必须说明写哪里）；`sha`、`expected_sha`、`dry_run` 透传 |
| `delete_file` | `DELETE contents/{path}` | `sha` 必填；`expected_sha`、`dry_run` |
| `create_pr` | `POST pulls` | 不合并；无 dry-run（见"未做"） |
| `list_pipelines` | `GET pipelines` | 找流水线 id |
| `wait_pipeline` | `GET pipelines/{id}/wait?until=terminal` | `timeout` 1–55 秒，默认 30；未结束返回 `timed_out: true`，再调一次 |

- 失败以 `isError: true` 返回，文本是 `HTTP <状态>: <API 错误体>`，所以模型能读到 `reason` 和 `details.current_sha`。参数在发请求前校验（owner/repo 名称、路径段、必填字段、布尔值、timeout 范围）。
- **没有 `merge_pr` 工具**：设计里的"入队"步骤不暴露给 Agent；分支受限令牌本来也不能合并。
- `read_file` 增加 `start_line` / `max_lines`，默认且最多返回 2000 行 / 128 KiB，被截断时结果末尾有 `[lines a-b of N; call read_file again with start_line=…]`；单行超长按字符边界截断并在提示中说明。

### 8. 幂等键 `Idempotency-Key`（H4）

- 新表 `idempotency_keys`（迁移 `m20261106_000003`）：`(user_id, token_id, idem_key)` 唯一，`token_id` 对普通登录为 0，所以同一用户的两个令牌互不相干；`fingerprint` = SHA-256(方法 + 路径与查询 + `X-Dry-Run` + 请求体)；`state` 为 `pending`/`done`；成功返回的状态码与响应体存在行里。
- 中间件 `idempotency::idempotency_middleware` 装在 PAT 中间件**里面**（先换出会话、已盖上令牌 id）。行为：
  - 同 key 同请求：重放首个响应（头 `Idempotent-Replay: true`），状态码 < 500 的都重放，包括 4xx；
  - 同 key 不同请求：422，`reason: IDEMPOTENCY_KEY_REUSED`，不可重试；
  - 同 key 首个请求仍在运行：409，`reason: IDEMPOTENCY_IN_PROGRESS`，可重试；`pending` 超过 5 分钟视为服务端中途崩溃，释放后重新认领；
  - 响应 ≥ 500、超过 32000 字节或不是文本：释放 key，重试会重新执行；
  - 24 小时后过期（认领时顺手清理同 key 的过期行，另有每小时一次的后台清理）；
  - key 为 1–255 个 `[A-Za-z0-9._:-]` 字符，否则 400。
- **只覆盖响应里没有秘密的路由**（响应会存进数据库）：`POST /repos`，以及 `/repos/{o}/{n}/` 下的 `issues`、`pulls`、`contents`、`releases`、`labels`、`milestones`、`branches`、`wiki`。令牌、登录、webhook、部署密钥、协作会话（响应含会话密钥）都不在内。**对不支持的路由带这个头会被 400 拒绝（`IDEMPOTENCY_NOT_SUPPORTED`），而不是悄悄忽略**，免得客户端以为自己受保护。新增路由要加进 `idempotency_supported` 前先确认响应里没有秘密。
- 读请求带这个头只是读。请求体超过 4 MiB → 413。
- MCP：`commit_file`、`delete_file`、`create_pr` 新增可选 `idempotency_key`，作为头发出（不进请求体）。

### 9. 仓库事件日志与长轮询 `GET /repos/{o}/{n}/events`（H6）

- 新表 `repo_events`（迁移 `m20261106_000004`）：`id` 自增即游标，`repo_id`、`event_type`、`payload`（JSON 文本）、`created_at`。**每个会触发 webhook 的事件也写进这张表**（挂在 `webhook::service::trigger_event` 开头，尽力而为，失败只记日志）；载荷超过 16 KiB 存成 `{truncated: true, original_bytes}` 的占位。保留 7 天，每小时清理。
- `GET /repos/{o}/{n}/events?since=&types=&limit=&timeout=`（仓库读权限）：`since` 之后按 id 升序返回；不带 `since` 只返回 `next_cursor`（日志当前末尾），让客户端从"现在"开始；`types` 逗号分隔（最多 20 个，名称 `[a-z0-9._-]`）；`limit` 默认 100、最多 200；`timeout` 默认 0、最多 60 秒，有新事件立即返回，到点返回 `timed_out: true`；同时挂起的请求最多 64 个，超出 429。响应带 `retention_days` 与 `oldest_retained_at`，客户端据此判断自己的游标是否可能已过期。每个事件带 `payload_origin: "user_content"`（载荷里的标题/评论是用户写的，是数据不是指令）。
- 事件名就是 webhook 的事件名（`issue.opened`、`pull_request.opened`、`push` 等，不是封闭枚举）。设计里提到的 `ci.finished`、`review.submitted` 目前**没有对应的 webhook 事件**，所以日志里没有；等 CI 完成用 `wait_pipeline`。
- MCP `wait_repo_events`：`since`、`types`、`limit`、`timeout`（1–55 秒，默认 30）；不带 `since` 立即返回当前游标。

### 10. 聚合上下文 `GET /ai/repos/{o}/{n}/pulls/{number}/context`（H2）

- 一次返回接手一个 PR 需要的东西，按 `budget`（近似 token，`chars/4`，默认 6000，500–50000，越界 400）裁剪：`pull_request`（编号/状态/草稿/分支/head sha/作者/标题，**从不裁剪**）、`ci`（head 提交最新流水线 + 每个失败 job 的 `failure_summary`，复用 CI 反馈的摘要）、`unresolved_comments`、`changed_files`（路径/状态/增删行数，不含 diff，附 `diff_url`）、`description`、`reviews`（新的在前）、`resolved_comments`，另有 `counts`（永远是完整数字）、`diff_stats`、`budget.estimated_tokens_used`。
- **固定优先级**：CI 失败 > 未解决评论 > 变更文件 > 描述与评审 > 已解决评论。放不下整段的列表保留开头的若干项（`truncated` 里写 `section:partial (k of n)`），一项都放不下的整段省略（`truncated` 里写段名）；描述文本从尾部截断。单条评论/评审正文先截到 2000 字符（`body_truncated`）。
- 评论按线程归并：解决状态在顶层评论上，回复属于它所回复评论的线程。
- 用户写的文字（标题、描述、评论、评审、CI 日志里的行）都带 `origin: "user_content"`（标题是 `title_origin`）——是数据，不是指令。
- **没有计算**：能否干净合并、分支保护检查结果（`not_evaluated: ["mergeable", "branch_protection"]`）。各段是独立的实时读取（`consistency: per_section_live_read`），不是快照。
- MCP `get_pr_context`（`budget` 500–50000，发请求前校验）。

## 它**不**做什么

- **本文写成时没有做、之后已补的**：PR `export`、创建 PR 的 dry-run、`refs/pull` 的评估（结论：同仓 PR 早已写 `refs/pull/<n>/head`，设计文档里"服务端不写"是过期的）、REST 文件提交不触发推送钩子的缺口、审计覆盖，见 [agent-gaps-verification-2026-11.md](agent-gaps-verification-2026-11.md)。
- 评论/分支创建/删除没有 dry-run；dry-run 覆盖文件提交、PR 合并与（后补的）创建 PR。
- `expected_head_sha` 非原子（见上）；`dry_run` 不预测冲突、webhook、CI 触发、通知。
- 审计只覆盖已有的审计点；审计里的令牌 id 靠内部请求头，只对挂在 PAT 中间件之后的路由可信。
- 分支受限令牌的 REST 写仍只有白名单里的路由；`apply suggestion`、`merge` 等不对它们开放。
- 受限令牌的评审行内评论仍可用。
- 幂等重放只对**上面那些路由**有效，且重放的是首个响应的文本：响应里的 `request_id` 之类逐请求的字段按本次请求重新生成。
- 事件日志不是仓库快照；离开超过 7 天要重新读仓库本身。没有 SSE（设计里的第二阶段）。

## 证据

| 层级 | 结果 |
|---|---|
| 任务验证（SQLite） | `rg-core` 单测：JWT `tid` 往返与缺省、受限令牌 REST 写白名单（含 `contents/**` 放行、`contents` 本身拒绝）。`rg-http` 单测：审计 `with_access_token`（无头、有头、非对象 details、非数字值）。`pat_ref_allowlist_tests` 共 8 项（真实 HTTP、真实 `git`）：受限令牌编辑 `agent/one` 成功、`main`/`agentx/one` 被拒、删除在名单内越过限制（随后因 blob sha 错误失败而非 403）；删除令牌后它换出的会话被拒；审计条目带真实令牌 id，普通登录带伪造头时不带 id，PAT 带伪造头时用真实 id；受限令牌批准 PR 403，评论/`request_changes` 201，同一人的普通登录和无限制令牌批准 201。`file_commit_semantics_tests` 4 项：dry-run（体与头）不写任何东西且检查是真的（已存在/blob 不符 409、非法分支 400、不存在的分支报告 `creates_branch`）；`expected_sha` 过期 409 `REF_ADVANCED` 带 `current_sha`、不存在的分支 `current_sha: null`、dry-run 同样受约束；删除同理；合并 dry-run、head 移动后固定 head 的合并 409、按新 head 合并成功、已合并再合并 409。`rg-mcp`：`read_file` 窗口单测 6 项 + 集成 1 项；`write_tools_mcp` 5 项（工具定义、请求体与路由、409 错误原样回传、参数先校验后请求且零请求、删除/PR/流水线路由）。 |
| 运行验证（真实服务 + 真实 `ironforge-mcp`，SQLite） | 令牌 `repo:write` + 仓库白名单 + `refs: [agent/*]`：`commit_file` dry-run 报告 `creates_branch: true`；真提交到 `agent/fix`（分支被创建，服务器上只有 `main`、`agent/fix`）；提交到 `main` 被 403；`read_file` 读回；过期 `expected_sha` 得到 409 并带 `current_sha`；`create_pr` 成功；`list_pipelines` 空列表；`wait_pipeline` 对不存在的 id 报 404；同一令牌 REST 合并 403；仓库所有者用普通登录 dry-run 合并 200。**运行时发现并排除了一个"陈旧二进制"问题**：`target/debug/ironforge` 是旧版本时，同一流程在 `contents` 上得到 403；重新 `cargo build -p rg-cli -p rg-mcp` 后正常——跑 `scripts/collab-e2e.py` 系列前要先重编。 |
| 任务验证补充（幂等与事件） | `idempotency_tests` 5 项：重试同 key 同请求得到同一响应且只建一个 issue（响应头 `Idempotent-Replay`），同 key 不同请求 422 `IDEMPOTENCY_KEY_REUSED`，新 key 与无 key 保持原行为；两个令牌与一次普通登录用同一个 key 互不影响，各自重试各自重放；4xx 被原样重放（`request_id` 重新生成），非法 key（空格、`/`、256 字符）400，对 `POST /users/tokens` 带 key 400 `IDEMPOTENCY_NOT_SUPPORTED` 且没有创建令牌，读请求带 key 只是读；两个并发的同 key 请求只有一个执行，另一个拿到重放或 `IDEMPOTENCY_IN_PROGRESS`；25 小时前的 key 可以再用。`repo_events_tests` 5 项：按游标顺序读取、不重复不遗漏、`limit` 分页；`types` 过滤与非法参数 400；长轮询在事件到达时立即返回、安静时在超时返回；陌生人读不到私有仓库的事件、匿名被拒；超过保留期的事件被清理，超过 16 KiB 的载荷存成占位。`write_tools_mcp` 现 7 项（含 `wait_repo_events`、`idempotency_key` 作为头发出且先校验）。`rg-db` 的迁移回滚窗口测试 `Some(21)` → `Some(23)`。 |
| 运行验证补充 | 真实服务 + 真实 `ironforge-mcp`：`wait_repo_events` 无游标返回 `next_cursor: 0`；`commit_file` 建 `agent/x`；`create_pr` 带 `idempotency_key: k1` 两次得到同一个 PR 号，服务器上只有 1 个 PR；同 key 换标题得到 HTTP 422；`wait_repo_events since=0` 取到 `pull_request.opened`，安静时 2 秒后 `timed_out: true`；`get_pr_context budget=800` 返回 `ci`（该提交没有流水线时给出说明）、`changed_files` 与预算用量，`budget=5` 在发请求前被拒。 |
| 后端验证（PostgreSQL 17 / MySQL 8.4，真实容器，每个测试目标用全新数据库） | 新测试目标（`pat_ref_allowlist_tests` 7 项、`file_commit_semantics_tests` 4 项、`idempotency_tests` 5 项、`repo_events_tests` 5 项）通过 `IRONFORGE_TEST_DATABASE_URL` 在两个后端都全部通过；迁移 `m20261106_000001`–`000004` 在两个后端建出的结构已核对（`access_tokens.repo_allowlist`/`ref_allowlist` 为可空 `text`，`idempotency_keys` 的 `(user_id, token_id, idem_key)` 唯一索引与 `created_at` 索引存在，`repo_events` 有数据）。这些测试用新增的 `common::spawn_test_app_on_env_db`（设了环境变量就连该数据库，否则回到 SQLite）；唯一键冲突按 `SqlErr::UniqueConstraintViolation` 判定，没有用 `ON CONFLICT DO NOTHING`（CLAUDE.md 踩坑 #19）。 |
| 任务验证补充（PR 上下文） | `pr_context_tests` 4 项（SQLite、PostgreSQL、MySQL）：带失败流水线、两个评论线程（一个已解决）、两个变更文件的 PR——`ci` 里有失败 job 的 `error_lines`、未解决与已解决分开、`changed_files` 与 `counts`、用户文字都带 `origin`、`truncated` 为空；`budget=500` 时核心仍在、已解决评论最先被裁、估计用量不超过预算、`counts` 仍是完整数字，`budget=20000` 时什么都不裁；`budget` 越界 400、不存在的 PR 404、私有仓库的陌生人读不到。`ai_pr_context` 的分配函数单测 5 项（按优先级、保留开头的项、文本截断、空列表不算被裁）。`write_tools_mcp` 现 8 项（含 `get_pr_context`）。 |
| 全量（SQLite） | `cargo test --no-fail-fast -p rg-http -p rg-core -p rg-mcp -p rg-db -p rg-git`：122 个测试目标、1090 项通过、0 失败（该次运行在提交 `2007cfa` 的代码上；之后只增加了 `get_pr_context` 与测试夹具改动，这些由各自的测试目标、`openapi_registration_tests`/`pat_scope_ladder_tests`/`pat_repo_allowlist_tests`/`collab_contract_tests` 与 `rg-mcp` 全部测试单独复跑通过）；`cargo clippy --workspace --all-targets -- -D warnings` 与 `cargo fmt --check` 干净；前端 173 个文件、962 项通过，i18n 键一致（各 2071）。 |

## 未覆盖

- PostgreSQL/MySQL 只跑了下面"后端验证"里列的四个测试目标；`rg-http` 的其余套件仍只在 SQLite 上跑。
- `wait_pipeline` 的成功路径本文写成时只在 stub 里验证；之后用真实服务 + 主机执行的 CI 验证过成功与失败两条路径，见 [agent-gaps-verification-2026-11.md](agent-gaps-verification-2026-11.md)。
- CI 未看。
- 并发：`expected_sha` 的"检查后被推进"分支靠 `commit_tree_edits` 的 `TipAdvanced` 返回，没有为它单独写竞态测试。
