# Harness 原生化设计：让 IronForge 成为 AI Agent 的一等公民

> **状态**：草案（Proposed） ｜ **日期**：2026-11-03 ｜ **可信度**：现状部分 `确认`（附代码证据），设计部分 `推测`（待评审）
> **来源**：仓库代码（`crates/rg-mcp`、`crates/rg-http`、`crates/rg-core`、`crates/rg-git`）+ [client-cli-design-2026-11.md](client-cli-design-2026-11.md) + 本文提出的设计
> **范围**：服务端 API / MCP / 鉴权 / 事件。不含 `ifg` 命令细节（见 CLI 设计文档）。

---

## 1. 背景与目标

「Harness」指驱动 LLM 完成任务的外壳（Claude Code、Cursor、自建 Agent 框架等）。它与人类客户端有三点本质差异，决定了 Git 托管平台的接口该如何设计：

1. **token 有成本**：一次读取返回多少、要往返几次，直接决定成本与成功率。
2. **并发且会重试**：多个 Agent 同时操作同一仓库；网络抖动或模型重试会重复发请求。
3. **靠机器可读反馈闭环**：「改 → 推 → 看 CI → 修」循环里，CI 失败原因、合并冲突、权限拒绝都必须结构化，否则 Agent 只能猜。

**目标**

1. 缩短 Agent 的「改-推-看 CI-修」循环（更少往返、更少 token）。
2. 让 Agent 的写操作**可预览、可限权、可追溯、可安全重试**。
3. 多 Agent 并发写同一仓库时，冲突以结构化形式暴露，不产生静默覆盖。

**非目标**

- 不在服务端内置 LLM 或 Agent 运行时；平台只提供接口与护栏。
- 不防御「Issue/PR/日志内容中的提示词注入」本身（见 §9），只提供标注与最小权限来限制爆炸半径。
- 不改变 Git 协议路径（`rg-git` 热路径）。

---

## 2. 现状事实（`确认`）

| 事实 | 证据 |
|------|------|
| MCP 仅 5 个只读 tool：`list_repos`、`read_file`、`read_dir`、`get_issue`、`get_pr`；3 个 resource（仓库元数据 / 文件内容 / Issue） | `crates/rg-mcp/src/tools/mod.rs`、`resources/mod.rs` |
| MCP 仅 stdio；`--sse` 会直接报错退出 | `crates/rg-mcp/src/main.rs` 顶部文档 |
| `/api/v1/ai/*` 已注册 6 条路由（summary / issues / prs / tree / search/code / index） | `crates/rg-http/src/lib.rs:1422-1435` |
| **`ai/.../tree` 是 501 桩**；**`search/code` 已实现**（FTS5，需先索引） | `crates/rg-http/src/api/ai.rs`（`ai_repo_tree` 返回 `NOT_IMPLEMENTED`） |
| **`.ai/README.md` 把 `search/code` 也标成「待实现」，与代码不符（文档过期）** | `.ai/README.md` AI 端点表 vs `ai.rs::ai_search_code` |
| `search/code` 的 OpenAPI 声明了 `ref` 参数，但处理器只用 `q` 与 `limit`，`offset` 固定为 0（无分页、不按 ref） | `ai.rs::ai_search_code` |
| CI job 响应含 `exit_code`、`log: Option<String>`（整段日志）、`started_at`/`finished_at`；无失败摘要、无增量读取、无等待接口 | `crates/rg-http/src/api/ci.rs`（job 响应结构） |
| 仓库中无 SSE（`text/event-stream` / `axum::response::sse` 零命中）；实时能力仅有 WebSocket（通知按用户、Job 日志按 job，且只认 JWT） | `grep` 结果；`rg-http/src/ws.rs`（引自 CLI 文档 §2） |
| PAT scope 仅 `repo` / `user` / `admin`，`has_scope` 为字符串匹配，`admin` 蕴含全部 | `crates/rg-core/src/auth/pat_scope.rs` |
| 错误体已是 `{ "error": { "code", "message", "request_id" } }`，`code` 为 8 个粗粒度枚举（`NOT_FOUND`/`BAD_REQUEST`/`UNAUTHORIZED`/`FORBIDDEN`/`CONFLICT`/`GONE`/`INTERNAL_ERROR`/`RATE_LIMITED`） | `crates/rg-http/src/error.rs` |
| **CLI 设计文档 §7.4 写「依赖错误响应里的 `message` 字段」，实际在 `error.message` 下，需在 CLI 实现前订正** | 同上 vs `client-cli-design-2026-11.md` §7.4 |
| 审计 `record(user_id, username, action, resource_type, resource_id, resource_name, headers, details)`；无「行为主体类型 / 会话 / 代理对象」字段，只能放进 `details` JSON | `crates/rg-core/src/audit/service.rs` |
| 引用乐观锁在部分路径已存在：`ExistingMustMatch`（`rg-git/src/ops.rs` 两处）；rebase 有 `RebaseOutcome::BaseAdvanced`；但通用 `update_ref` / `delete_ref` 用 `PreviousValue::Any` | `crates/rg-git/src/ops.rs` |
| Merge Queue 路由已存在（列表 / 入队 / 取消） | `crates/rg-http/src/lib.rs:838-842` |
| CI 已有 OIDC：JWKS + audience-bound 短时 token，可作为会话身份机制的参照 | `lib.rs:976-980`、`CLAUDE.md`「CI OIDC」 |
| 服务端不写 `refs/pull/<n>/head` | 引自 CLI 文档 §2 |

---

## 3. 设计原则

1. **反馈结构化**：凡 Agent 可能需要「据此决策」的信息，都有机器可读字段，不依赖解析自然语言。
2. **读接口有预算**：每个面向 Agent 的读接口都支持 `fields`、`limit`/`budget`，并显式返回 `truncated` 与 `next_cursor`，不静默截断。
3. **写接口三件套**：`dry_run`（预览）、`expected_*`（乐观并发）、`Idempotency-Key`（安全重试）。
4. **最小权限默认**：Agent 凭据短时效、可限仓库、可限分支前缀；Agent 不能自我批准。
5. **一套后端，多个入口**：REST、MCP、`ifg` 共用 `rg-client`，避免三套接口漂移（CLI 文档 P0）。
6. **向后兼容**：现有 `repo`/`user`/`admin` PAT 与 REST 响应字段不变；新能力均为新增字段/新增端点/新增可选参数。

---

## 4. 工作流设计

### 4.1 [A] CI 反馈闭环（最高收益，纯服务端、风险低）

Agent 修 CI 的主要开销是读整段日志、轮询状态。

**新增接口**（均在 `/api/v1/repos/{owner}/{name}/` 下）

| 接口 | 说明 |
|------|------|
| `GET pipelines/{id}/jobs/{job_id}/log?offset=N&limit=B&tail=L&strip_ansi=` | 增量/尾部读取。返回 `{ job_id, status, exit_code, offset, next_offset, total_bytes, eof, reset, text }`，偏移按存储文本的**字节**计，且不会切开 UTF-8 字符。`limit` 默认 64KiB、上限 256KiB；`tail=L` 返回末尾 L 行（上限 2000，仍受 `limit` 字节约束），与 `offset` 互斥（400）。`offset` 超过日志长度（如 rerun 换了更短的日志）时从头返回并置 `reset=true`。`eof` 仅在 job 已进入终态且读完时为真。同时满足 CLI 文档 §7.1-C |
| `GET pipelines/{id}/wait?etag=E&timeout=S&until=terminal` | 长轮询。不带 `etag` 立即返回当前快照；带 `etag` 则在流水线或任一 job 状态变化时返回（`until=terminal` 则只在流水线结束时返回）。响应含 `etag`、`changed`、`terminal`、`timed_out`、`pipeline`、`jobs[]`（失败 job 内嵌 `failure_summary`）。`timeout` 默认 30s、上限 60s；进程内同时最多 64 个等待，超出返回 429 |
| job 响应新增 `failure_summary`（仅失败时） | `{ exit_code, tail: [..], truncated, error_lines: [..] }`。`tail` 为去 ANSI 的末尾 50 行（单行截 300 字符）；`error_lines` 为全文中形似错误的行（最多 10 条，去重），是**启发式**，不保证覆盖。`failed_step` 未实现：现有日志没有结构化的步骤边界 |

**约束**

- 日志读取必须复用现有日志展示路径的 secrets 脱敏逻辑。内置 runner 在落库前已对日志做 `mask_values`（`rg-ci/src/runner.rs`，确认）；**外部 runner（`rg-runner`）上报的日志是否同样脱敏 `待验证`**，在此之前这些接口对外暴露的日志脱敏程度与现有 `GET …/jobs/{job_id}` 的 `log` 字段一致，没有变得更宽松，也没有更严格。
- `wait` 持有连接，受 64 个并发上限与 60s 超时约束；终态集合为 `success`/`failed`/`failure`/`error`/`skipped`/`canceled`，非终态为 `pending`/`running`/`manual`/`waiting_approval`（取自 `pipeline_ops` 的状态判断）。`etag` 是流水线状态与各 job 状态的哈希，跨实例一致。每秒查询一次数据库，不依赖进程内通知，因此多实例部署下同样成立。
- 与 `client-cli-design` 的关系：本项落地后，`ifg ci logs --follow` 可直接用 offset 增量，不必再全量轮询。

### 4.2 [B] 面向 Agent 的读接口

1. **补齐 `ai/.../tree`**（已实现）：`ref`（默认仓库默认分支）、`path`、`depth`（默认 1、最大 8）、`limit`（默认 200、最大 1000）、`cursor`（用上一页的 `next_cursor`）；条目含 `path`/`type`（`blob`/`tree`/`symlink`/`submodule`）/`size`/`sha`，不含内容。空仓库返回空列表。
2. **修正 `search/code`**（已实现，部分）：索引只覆盖默认分支，因此 `ref` 若不是默认分支返回 400，而不是用错误的树回答；新增 `offset`，总数在 `x-total-count` 响应头（响应体仍是数组，保持兼容）。**未做**：`indexed_at` / `indexed_commit`——索引表目前不记录构建时的 commit，需要新列，留到后续迁移。
3. **聚合上下文接口** `GET /api/v1/ai/repos/{o}/{r}/pulls/{n}/context?budget=N`：一次返回
   - PR 元数据（标题/正文/状态/mergeable/分支保护检查结果）
   - 变更文件列表（路径、增删行数、状态；**不含**完整 diff，附 `diff_url`）
   - 评论与 review 线程（按时间，未解决优先）
   - 最新 pipeline 状态 + 失败 job 的 `failure_summary`
   - `truncated: [section...]`：被预算裁掉的段落名
   
   `budget` 以近似 token 计（实现可先用 `chars/4` 估算），按固定优先级分配：失败摘要 > 未解决评论 > 变更文件 > 元数据其余部分 > 已解决评论。
4. **统一参数**：所有 `/ai/*` 列表接口接受 `fields=a,b,c` 与 `limit`，响应带 `{ truncated, next_cursor }`。
5. **内容来源标注**：用户撰写的文本（Issue/PR/评论/提交信息/日志）放在带 `origin: "user_content"` 的字段中，与平台生成字段区分（用于 §9 的注入防护，由 harness 决定如何处理）。

### 4.3 [C] MCP 演进

在 CLI 文档 P0（抽 `rg-client`）的基础上：

| 阶段 | tool |
|------|------|
| 只读扩展 | `get_pr_context`（4.2-3）、`get_ci_status`、`get_ci_failure`、`read_job_log`、`search_code`、`list_tree` |
| 等待 | `wait_for_ci`、`wait_for_events`（4.7，内部走长轮询，单次阻塞 ≤ 55s 以避开 harness 的 tool 超时） |
| 写（须 4.4 先落地） | `create_branch`、`commit_files`（复用服务端文件编辑路径）、`create_pr`、`comment`、`submit_review`、`enqueue_merge`、`rerun_job` |

**规则**

- 每个写 tool 都有 `dry_run` 参数，直接映射 REST 的 `?dry_run=true`（见 4.5）。
- tool 的 `inputSchema` 由手写 DTO 驱动，与 `rg-client` 同源；暂不从 OpenAPI 生成（OpenAPI 未覆盖全部 handler，见 `CLAUDE.md` 踩坑 #11）。
- **传输层**：新增 streamable-HTTP 模式（`ironforge-mcp --http`），**无状态、逐请求透传 `Authorization`**，不在 MCP 进程里保存凭据；原因：远程/云端 harness 无法拉起本地 stdio 子进程。保持独立进程，不并入 `rg-http`，避免扩大服务端攻击面。stdio 行为不变。
- tool 返回统一为 `{ ok, data?, error?: { code, reason, hint, retryable } }` 文本 JSON；`isError` 与 `ok=false` 一致。

### 4.4 [D] Agent 身份、凭据与策略（写能力的前置条件）

**问题**：现有 PAT 的 `repo` scope 是「该用户在仓库上的全部读写」，把它交给 Agent 等于交出整个账号的写权限。

**4.4.1 凭据**

在 PAT 上增加可选约束（新增列，沿用 `repo`/`user`/`admin` 语义不变）：

| 约束 | 说明 |
|------|------|
| `expires_at` | 到期自动失效；会话令牌建议 ≤ 8h |
| `repos` | 允许访问的仓库白名单（`owner/name`，可空 = 不限） |
| `ref_prefixes` | 允许**写**的 ref 前缀，如 `refs/heads/agent/*`（可空 = 不限） |
| `read_only` | 为真则拒绝所有变更类请求 |
| `kind` | `user`（默认）/ `agent` |
| `agent_label` | 自由文本：harness 名 + 会话 ID，写入审计 |

- 新 scope 采用层级：`repo` ⊇ `repo:write` ⊇ `repo:read`（`has_scope` 需从字符串相等改为层级判断；旧值行为不变，需补回归测试）。
- 签发接口 `POST /api/v1/users/me/agent-tokens`（需 `user` scope 或 JWT），请求体即上表约束，响应一次性返回明文。
- 令牌格式沿用 `ifp_` 还是新前缀 `ifa_` 见 §10 待决问题。

**4.4.2 强制点**

所有会移动 ref 的路径必须经过**同一个**检查函数 `RefPolicy::check(actor, repo, ref, op)`（放 `rg-core`）。已知需接入的入口：

- Git receive-pack（HTTP 与 SSH）——与分支保护相邻
- 文件编辑 API（`edit_tree` + `commit_as`）
- PR 合并（merge / squash / rebase / merge queue）
- 分支创建/删除接口、Tag 接口

> 「有一条路径没接入」等价于策略失效，因此实现时用测试枚举全部 ref 写入口（见 §8），而不是靠人工记忆。

**4.4.3 仓库级 Agent 策略**（`repo_agent_policy`，JSON，管理员可配）

- `allowed_ref_prefixes`：`kind=agent` 的令牌可写的前缀，默认 `refs/heads/agent/*`
- `human_approval_required`：默认 true——**`kind=agent` 令牌产生的 review/approve 不计入分支保护的必需批准数**；Agent 自己创建的 PR 至少需要 1 个人类批准才能合并到受保护分支
- `max_open_prs_per_agent`、`max_push_bytes`
- `ephemeral_branch_ttl_days`：`agent/*` 分支无关联开放 PR 且超期后自动清理（替代原先设想的「沙箱」，成本更低）

**4.4.4 审计**

- 短期（不改表）：`details` JSON 增加 `{ "actor_kind", "agent_label", "token_id", "on_behalf_of" }`；`audit!` 宏调用点由统一的 helper 填充，避免遗漏。
- 中期：在 `audit_logs` 增加 `actor_kind`/`token_id` 列以便按主体过滤。迁移须遵守 `CLAUDE.md` 踩坑 #13/#14（显式表名、幂等守卫）。

### 4.5 [E] 写接口语义：dry-run、乐观并发、幂等

**dry-run**

- 约定：`?dry_run=true`（或头 `X-Dry-Run: 1`）。响应 `200` + `{ "dry_run": true, "would": [ {effect...} ] }`，`effect` 例：`ref_update{ref,old,new}`、`webhook{event,count}`、`ci_trigger{workflow}`、`notification{count}`、`audit{action}`。
- **必须真正无副作用**（含审计、webhook、CI 触发、通知）。实现方式：服务层把「计算效果」与「应用效果」拆成两步（`plan` → `apply`），dry-run 只走 `plan`。不要用「执行后回滚」。
- 首批覆盖：创建 PR、合并（含入队）、文件提交、分支创建/删除、评论（评论的 `would` 可以简化为通知数）。

**乐观并发**

- 文件编辑/分支更新/合并接口增加可选 `expected_sha`（目标 ref 当前值）与 PR 合并的 `expected_head_sha`。不匹配 → `409`，错误体 `error.details`：`{ current_sha, expected_sha, conflicting_paths? }`。
- 复用 `rg-git` 已有的 `PreviousValue::ExistingMustMatch` 路径；对 `update_ref` 当前的 `PreviousValue::Any` 调用点逐一审查，凡来自 API 写路径的改为带期望值（纯内部维护用途的可保持）。
- rebase 的 `BaseAdvanced` 映射为同一种 `409`，`reason = "REF_ADVANCED"`。

**幂等**

- 所有创建类 `POST`（创建 PR/Issue/评论/Release/分支）接受 `Idempotency-Key` 头。
- 存储 `(token_id, key) → (请求体哈希, 响应状态, 响应体, created_at)`，保留 24h。同 key 同体 → 原样重放响应；同 key 异体 → `422 reason=IDEMPOTENCY_KEY_REUSED`。
- 新表需显式表名并幂等迁移；过期清理走现有后台任务框架。

### 4.6 [F] 错误体细化（向后兼容）

在现有 `{ error: { code, message, request_id } }` 上**新增**可选字段，不改旧字段：

```json
{ "error": {
    "code": "CONFLICT",
    "reason": "REF_ADVANCED",
    "message": "target branch moved while merging",
    "retryable": true,
    "hint": "re-fetch the PR and retry with expected_head_sha=<new>",
    "details": { "current_sha": "…" },
    "request_id": "…" } }
```

- `code` 保持 8 个粗粒度枚举；`reason` 为稳定的细分枚举（如 `PR_NOT_MERGEABLE`、`REF_ADVANCED`、`BRANCH_PROTECTED`、`SCOPE_INSUFFICIENT`、`POLICY_DENIED`、`IDEMPOTENCY_KEY_REUSED`、`INDEX_NOT_READY`）。
- `AppError` 目前每个变体只带 `String`；扩展方式是新增构造器（如 `AppError::conflict_with(reason, details, msg)`），旧构造器与序列化结果不变。
- 先覆盖 Agent 高频路径（合并、推送被拒、权限不足、策略拒绝），其余逐步补，不做一次性全量改造。

### 4.7 [G] 事件订阅

Agent 通常没有公网入口，无法收 webhook。

- `GET /api/v1/repos/{o}/{r}/events?since=<cursor>&types=pr.updated,ci.finished,review.submitted&timeout=S`：长轮询，返回 `{ events: [...], next_cursor }`；`timeout` 上限 60s。
- 事件存入专用表 `repo_events`（单调递增 `id` 作游标，保留 7 天）。**不复用审计表**：审计面向合规、有 90 天归档与不同的写入时机，且无游标语义。
- 与 webhook 复用同一套事件枚举（13 种），避免两套命名。
- 第二阶段再加 SSE（`text/event-stream`），语义同上；SSE 依赖反向代理不缓冲，部署文档需补充。
- 多实例部署下需注意（`CLAUDE.md`「多实例部署的进程内状态」）：事件表在 DB 中，天然跨实例；**不要**用进程内广播通道作为唯一来源。

### 4.8 [H] 可重放与评测支撑

- 服务端在 PR 创建/更新时写 `refs/pull/<n>/head`（CLI 文档 §7.2 已单独立项）：为评测、重放、`pr checkout` 提供不依赖 fork/分支存续的锚点。写入点需经过 `RefPolicy`（系统主体）与 pack 引用保活（gc 不得回收）。
- `GET pulls/{n}/export`：单文件包含 base/head SHA、PR 元数据、审查事件流（本就不可变）、最终 pipeline 摘要。用于从历史 PR 构造评测集（给定 base + Issue，要求产出补丁，用仓库 CI 判分）。
- 属后期能力，**不阻塞** A–G。

---

## 5. 与 CLI 设计的关系

| 事项 | 处理 |
|------|------|
| `rg-client` 抽取 | 共用 CLI 文档 P0；MCP 与 `ifg` 同源 |
| CLI §7.1（WS 不认 PAT） | 4.1 的增量日志接口 + 4.7 长轮询基本取代它，B 方案（WS 支持 PAT）可降级为可选 |
| CLI §7.2（`refs/pull`） | 即 4.8 |
| CLI §7.4（错误体） | 订正为 `error.message`，并按 4.6 扩展 |
| `ifg` 写命令 | 同样透传 `--dry-run`、`--idempotency-key`、`--expected-sha`，保证 CLI 与 MCP 行为一致 |

---

## 6. 实施分期

> **顺序说明**：写 tool 不先于凭据约束开放。否则 Agent 会拿着用户的完整 `repo` PAT 去写，是倒退的安全姿态。

**实施状态（2026-11-04）**：H0、H1 已在工作区实现（未提交）。运行验证：`ci_feedback` 单元测试 17 项、`ci_feedback_tests` 集成测试 14 项、`ai_tree_search_tests` 集成测试 8 项通过；`cargo test -p rg-http` 全量 72 个测试目标、376 项通过、0 失败；`rustfmt --check` 与 `cargo clippy -p rg-http --all-targets` 对改动文件无告警。仅验证了 `rg-http` crate（任务分支工作区），未跑其他 crate 的测试，也未对真实实例做过手工运行验证。H2 起未开始。

| 阶段 | 内容 | 验收 |
|------|------|------|
| H0 文档与基础整理 | 订正 `.ai/README.md`（`search/code` 已实现）；订正 CLI 文档 §7.4；实现 `ai/.../tree`；修正 `search/code` 的 `ref`/分页 | `ai_repo_tree` 不再返回 501，有集成测试；OpenAPI 与实现一致 |
| H1 CI 反馈（4.1） | 增量日志、`wait`、`failure_summary` | 集成测试：构造失败 job，`failure_summary.tail` 含末尾行；`wait` 在状态变化时提前返回、超时时 `changed=false`；日志脱敏验证 |
| H2 MCP 只读扩展（4.2、4.3 前半） | `get_pr_context` 等只读 tool；streamable-HTTP 传输；依赖 `rg-client` | `cargo tree -p rg-client` 无服务端 crate；tool 清单有契约测试；`budget` 截断有单测 |
| H3 凭据与策略（4.4） | Agent token（过期/仓库/ref 前缀/只读）、层级 scope、`RefPolicy`、人类批准规则、审计字段 | 枚举测试证明**所有** ref 写入口经过 `RefPolicy`；agent 令牌推 `main` 被拒；agent 的 approve 不计数；`has_scope` 旧行为回归通过 |
| H4 写语义（4.5、4.6） | dry-run、`expected_sha`、`Idempotency-Key`、错误 `reason` | dry-run 后断言 DB/ref/审计/webhook 队列均无变化；并发两写者只有一个成功，另一个得到带 `details` 的 409；同 key 重试只创建一个 PR |
| H5 MCP 写 tool | 4.3 后半，所有写 tool 带 `dry_run` | 端到端：Agent 令牌经 MCP 完成「建分支 → 提交 → 开 PR → 等 CI → 入队」，且无法越出 `agent/*` |
| H6 事件与重放（4.7、4.8） | `repo_events` 长轮询、`refs/pull`、`export` | 事件游标不丢不重的并发测试；`refs/pull/<n>/head` 在分支删除后仍可取出 |

H1、H2 互不依赖，可并行。H3 是 H4、H5 的前置。

---

## 7. 测试策略

| 层级 | 做法 |
|------|------|
| 集成 | `crates/rg-http/tests/`，起真实路由。给 `AppState` 加字段必须同步 `tests/common/mod.rs::build_test_app_state`（`CLAUDE.md` 踩坑 #14） |
| ref 写入口枚举 | 一个测试对全部会移动 ref 的 handler 逐个用受限 agent 令牌发请求，断言被 `RefPolicy` 拒绝。新增 ref 写入口而不接入策略时该测试应失败（用于防回归） |
| dry-run 无副作用 | 对每个支持 dry-run 的接口：执行前后快照 DB 行数、ref 列表、webhook 投递表、审计表，断言相等 |
| 并发 | 两个 task 同时合并/提交同一分支，断言恰一个成功；使用 `tokio::join!`，不用 sleep 编排 |
| 幂等 | 同 key 同体重放 N 次，资源数量恒为 1 |
| MCP 契约 | tool 清单与 `inputSchema` 的快照测试；`dry_run` 参数存在于每个写 tool |
| CI 环境 | 遵守「CI 无 git identity」：涉及 commit 的测试显式设置 `GIT_AUTHOR_*`/`GIT_COMMITTER_*`，不依赖全局 git 配置 |
| 迁移 | 全新库跑 `ironforge migrate` 并核对表名；`down` 后再 `up` 必须成功（幂等守卫） |

按「证据分层」陈述：每个阶段的完成声明需区分实现证据、任务分支验证、合并后基线验证，不以文档或静态检查代替运行验证。

---

## 8. 迁移与兼容性

- 全部为**新增**：新端点、新可选请求参数、新响应字段、新表/新列。旧客户端无感。
- PAT 新增列均可空，旧 PAT 视为 `kind=user`、无约束，行为不变。
- 新表（`repo_events`、`idempotency_keys`、`repo_agent_policy`）需显式表名并与实体 `table_name` 一致；迁移用 `has_table`/`has_column` 守卫。
- 多实例：权限缓存 TTL 30s（`IRONFORGE_DISABLE_PERM_CACHE`）意味着收紧 Agent 权限后最长 30s 才在其他实例生效；令牌 `expires_at`/`read_only` 的判断**不得**进入该缓存，需每次请求校验。

---

## 9. 风险与安全考量

| 风险 | 说明 / 缓解 |
|------|-------------|
| 提示词注入 | Issue/PR/评论/日志是不可信输入，可能诱导 Agent 执行危险操作。平台无法根除；缓解：4.2-5 的来源标注 + 最小权限令牌（`ref_prefixes`、`read_only`、人类批准）限制爆炸半径。文档需明确这一边界 |
| 日志泄密 | CI 日志可能含 secrets。增量/尾部接口必须与现有展示共用脱敏；上线前先验证（`待验证`） |
| Agent 自批 | 通过「agent 令牌的批准不计数」+ 受保护分支必须有人类批准堵住 |
| 策略绕过 | 任一 ref 写入口漏接 `RefPolicy` 即绕过 → §7 的枚举测试 |
| 长轮询耗尽连接 | `wait`/`events` 受 60s 上限、并发上限与现有限流约束；多实例下限流为各实例独立计数（已登记的技术债） |
| 审计缺失 | 所有 Agent 写操作必须带 `agent_label`；helper 统一填充，禁止调用点各自拼装 |
| 令牌泄露 | 明文仅签发时返回一次；落库只存哈希（沿用 PAT 现有做法，实现前确认）；会话令牌短时效 |
| 范围蔓延 | 上述每项独立可交付；H3 之前不对外宣称「Agent 可写」 |

---

## 10. 待决问题

1. **令牌前缀**：Agent 令牌沿用 `ifp_` 复用现有鉴权路径，还是新前缀 `ifa_` 便于泄露扫描（GitHub secret scanning 式）和区分日志？倾向 `ifa_`，但需确认鉴权中间件对前缀的假设。
2. **主体模型**：`kind=agent` 只是令牌属性（代表某用户行事），还是独立的机器人账号？前者实现简单、归属清晰；后者便于在仓库成员列表里授权。本文按前者设计。
3. **批准规则的粒度**：「agent 批准不计数」是全局固定，还是仓库可配？本文按固定处理。
4. **事件保留与量级**：`repo_events` 7 天是否足够；高活跃仓库的写入放大需要压测。
5. **`budget` 的 token 估算**：先用 `chars/4`，是否引入分词器依赖？倾向不引入，并在响应里返回实际字符数。
6. **`refs/pull` 的 gc 与镜像**：写入该命名空间对 mirror 同步（`+refs/*:refs/*` 的现状限制，见 `CLAUDE.md` 技术债）与对象保活的影响，需在 H6 前单独评估。
7. **是否需要显式沙箱**：本文以「`agent/*` 分支 + TTL 清理」代替服务端沙箱工作区。若出现「无需推送即可在服务端试跑 CI」的需求，再单独立项。
