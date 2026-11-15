# T11 执行计划 — [#18] Agent 草稿分支命名空间（TTL + 不通知 + 可跑 CI）

> 权威约束见 `tasks/T11.md`。基线 `21d63ab`，分支 `agent/t11-agent-drafts`，worktree `../ironforge-t11`。
> 设计依据：`ironforge-docs/architecture/agent-draft-refs-ttl-design-2026-11.md`（PR #20，未改动）与复核
> `agent-draft-refs-ttl-verification-2026-11.md`。复核判「不成立」的两条（D10 `refs/pull/*` 只读、D15 `ci_secrets`
> 可按 ref 过滤）**没有**被当成前提：本实现不依赖 `refs/pull` 只读，secrets 是**整段不注入**而不是按 ref 过滤。

## 1. 交付边界

做：

1. `agent_draft_namespaces` 表 + 迁移（`m20261115_000001`），`Migrator::down` 28 → 29。
2. 推送守卫：草稿分支**仍跑 CI**、**仍写 `repo_events`**，跳过 push webhook / 分支 webhook / ws / 邮件。
3. 拆 `webhook::service::trigger_event` → `record_event` / `trigger_event_quietly`。
4. 1h 清理任务：到期先 `state=expiring` 关闸 → `grace_secs` 后 `delete_ref_if` CAS 直删（绕开 `on_delete`，审计留痕）。
5. 四个 REST 端点 + 路由三处（`lib.rs` / `openapi.rs` / `route_meta.rs`）。
6. 安全边界：草稿分支不注入 secrets、忽略 `allow_host_execution`（代码分支 + 测试）。

不做（明确排除）：PR 沙箱（`refs/pull/*` 守卫）、CI 的人审门、SSH 通道的令牌语义、前端分支列表过滤、
`pipeline_ids` 记录与 pipeline 取消（T11.md 的表结构里没有这两列，见 §9 遗留风险）。

未改：`Cargo.toml` / `Cargo.lock`（无新依赖）、`CLAUDE.md`、`ironforge-docs/`、`.cnb.yml`、`.github/`、`scripts/`、
`deploy/`、`crates/rg-mcp/`。

## 2. 命名空间：`refs/heads/agent/<id>/...`，普通分支 + 保留前缀

不用 `refs/agent/...`。理由（复核 P1/P2 成立）：分支白名单只拼 `refs/heads/<pattern>`
（`pat_scope.rs` 的 `ref_allowlist_full_patterns`），`receive_pack` 的删除路径也只认 `refs/heads/`
（`receive_pack.rs` 的 `only branches can be deleted`）。独立命名空间要同时改两处，收益只是"分支列表看不到"。

因此草稿区继承：分支保护、分支删除 API、`list_branches`、SSH 路径、PR head 解析，全部零改动。

`prefix` 校验（`rg_core::repo::draft::validate_prefix`）：

| 规则 | 例子 |
| --- | --- |
| 必须以 `/` 结尾 | `agent/42` ✗ |
| 首段必须是 `agent` | `sub/42/` ✗ |
| 第二段 = agent 段，1–40 字符 `[A-Za-z0-9_-]` | `agent/4 2/` ✗ |
| 不允许 `*` | `agent/42/*` ✗ |
| 不允许空段 / `.` / `..` / `.lock` 段 / 非 ASCII | `agent/42//` ✗ |
| 长度 ≤ 200 | — |
| 最后再用 `gix::refs::FullName::try_from("refs/heads/<prefix>probe")` 复核 git 自己是否接受 | 与分支 API 同一权威校验 |

`agent` 段必须是调用者自己（或调用者拥有的 agent）：否则一个 agent 能抢占别人的目录，被抢占者的推送会被
TTL 闸门拒掉（`agent_segment_is_owned_by`）。

## 3. 状态机与时序

```text
                     expires_at 到期                     expires_at + grace_secs 到期
  active ──────────────────────────────▶ expiring ───────────────────────────────▶ deleted
    ▲                                        │
    └──────────── PATCH（extend，宽限期内）────┘
```

| 状态 | 接受推送 | 通知 | 可见性 | 迁移动作 |
| --- | --- | --- | --- | --- |
| `active` | 是 | 只写 `repo_events` | 分支可 fetch | 过期 → `expiring` |
| `expiring` | **否**（`rejected_refs` 拒绝整目录） | 只写 `repo_events` | 分支仍在，CI 可跑完 | 宽限期满 → 删 ref → `deleted` |
| `deleted` | 否 | — | 分支已删 | 终态；行保留，拒绝复活 |

时间语义只有两个锚点：

- `expires_at` = 停止接受推送的时刻，**也是宽限期的起点**。`DELETE` 端点把它设成 `now`（立即关闸），
  于是"软删"和"自然到期"走同一条路径。
- `delete_deadline = expires_at + grace_secs`。

**先关闸再删**的物理保证：`run_cleanup_once` 两阶段，阶段 1 只做 `active → expiring`，阶段 2 只处理
`state = expiring AND delete_due_at <= now`。阶段 2 的查询发生在阶段 1 之后，所以同一 tick 就能删掉
`grace_secs = 0` 的目录；`grace_secs > 0` 时至少跨一个 tick，中间态可观测、可 `PATCH` 救回。

单次 tick 的 SQL：

| 阶段 | 查询 | 动作 |
| --- | --- | --- |
| 关闸 | `state = 'active' AND expires_at <= now` | `UPDATE ... SET state = 'expiring' WHERE id = ? AND state = 'active'`（CAS） |
| 清理 | `state = 'expiring'`（`+ grace` 在 Rust 里比，避免后端区间表达式） | 列 `refs/heads/<prefix>*` → 逐个 `delete_ref_if` → 全成功才 `state = 'deleted'` |

## 4. `trigger_event` 拆分的通道清单

`post_push_hooks` 是通知抑制的注入点，但它只覆盖"推送"这一条路径。拆分前后：

| API | 写 `repo_events` | 投递 webhook | 失败重试入队 | 调用方 |
| --- | --- | --- | --- | --- |
| `trigger_event`（原语义，未变） | 是 | 是 | 是 | 普通推送、分支/标签事件、issue/PR/… |
| `record_event`（新） | 是（同一 envelope 形状） | **否** | **否** | 明确"只进日志"的事件 |
| `trigger_event_quietly`（新） | 是 | **否** | **否** | 草稿分支的 push 事件 |

`trigger_event_quietly` 的实现就是 `record_event` + 一行 `tracing::debug!`。分成两个名字而不是一个
`deliver: bool` 参数，是为了让"以后在 `dispatch` 里新增一条投递通道"这件事**不能**悄悄作用到静默路径：
新通道只会出现在 `dispatch`（`trigger_event` 走的函数）里，静默路径根本不经过它。

各通道的草稿行为（`post_push_hooks` 逐 ref 判断 `is_draft`）：

| 通道 | 非草稿 | 草稿 |
| --- | --- | --- |
| CI 触发 | 跑 | **跑**（`allow_host_execution` 强制 false） |
| ws `ci_triggered` | 发 | 跳过 |
| CI 邮件 | 发 | 跳过 |
| push webhook | `trigger_event` | `trigger_event_quietly`（只进日志） |
| `branch.created` / `branch.deleted` | 发 | **完全跳过**（连 `repo_events` 都不写） |
| ws `push` | 发 | 跳过 |
| `git push --delete` 分支的 `branch.deleted` | `on_delete` 内发 | **跳过**（第四处注入点，`on_delete` 在 `post_push_hooks` 之前执行，R18-5） |

## 5. 清理为什么绕开 `on_delete`，以及审计怎么写

`on_delete`（`git push --delete` / `DELETE /branches/{x}` 的守卫）会拒绝：默认分支、受保护分支、
**被 open PR 占用的分支**。草稿分支正常情况下三条都不命中，但一旦命中（agent 误开 PR，或人给
`agent/42/*` 加了保护规则），清理就会**每小时失败一次并刷告警**，而且目录永远删不掉。

清理是系统行为，不是"用户删分支"，所以走 `rg_git::ops::delete_ref_if(repo_path, ref, sha)`
（`PreviousValue::MustExistAndMatch` 的 CAS）：只删"我看过并确认过"的那个 sha，期间有新推送则 CAS 失败、
下个 tick 重试。

**这是本功能里唯一一处故意绕过护栏的地方**，所以审计必须写明"绕了什么、为什么"：

```jsonc
// audit_log: action = "agent_draft.cleaned", user_id = null（系统动作）
{
  "prefix": "agent/42/",
  "deleted_refs": 2, "vanished_refs": 0, "refs": ["refs/heads/agent/42/a", "..."],
  "bypassed_guard": "on_delete",
  "bypass_reason": "the draft cleanup is not a user deleting a branch: on_delete refuses the
                    default branch, protected branches and any branch an open pull request
                    occupies, which would make a draft undeletable and re-alert every hour. …"
}
```

测试 `cleanup_deletes_a_branch_an_open_pull_request_occupies_and_audits_the_bypass` 直接构造这个场景：
从草稿分支开一个 open PR → `DELETE /branches/{x}` 被拒 → 清理仍然删掉分支 → 审计里有
`bypassed_guard = "on_delete"` 与含 `open pull request` 的理由。

## 6. 安全边界（本功能的安全前提，不是可选优化）

草稿分支跑的是**推送方自带的** CI 配置（`.gitea/workflows/*.yml` / `.ironforge-ci.yml`，触发时从该
commit 的树里读），而且**排除了人工 review**。若允许 host execution 或注入 secrets，草稿区就是一条
绕过 review 的任意代码执行通道。复核 D15 已确认 `ci_secrets` 没有按 ref 过滤的能力，所以两条都不是
"加个配置"，是代码分支。

### 6.1 不注入 repository secrets —— 两处注入点都要堵

| 注入点 | 位置 | 实现 |
| --- | --- | --- |
| 内置 runner | `crates/rg-ci/src/runner.rs` `PipelineRunner::job_environment` | `is_draft_ref(db, repo_id, pipeline.ref_name)` 为真时**整段跳过** `list_by_repo` + 解密，`secret_values` 也保持空（否则日志遮蔽会误以为注入过） |
| 外部 runner | `crates/rg-http/src/api/runners.rs` `poll_job` | 同一判断；为真时**不调用** `decrypted_repo_secrets`，job 的 `variables` 里没有任何 secret |

只堵内置 runner 是无效的：`external_runners = true` 时内置 runner 根本不启动（复核 R18-7）。

### 6.2 忽略 `allow_host_execution` —— 三处，纵深防御

| 层 | 位置 | 行为 |
| --- | --- | --- |
| 触发入口 | `crates/rg-ci/src/lib.rs` `trigger_pipeline` | `allow_host_execution = requested && !is_draft_ref(...)`，让 `spawn_internal_runner` 与 merge-queue 评估器拿到一致的值 |
| 推送钩子 | `crates/rg-http/src/lib.rs` `post_push_hooks` | 传给 `TriggerPipelineParams` 的 `allow_host_execution: *allow_host_execution && !is_draft` |
| job 执行 | `crates/rg-ci/src/runner.rs` `run_job` | `image.is_none() && draft_ref` → 直接失败，错误消息说明"draft CI runs in a container only"；即使有人 `set_allow_host_execution(true)` 也拦得住 |

`allow_host_execution` / `job_timeout_secs` 是**实例级**参数（`[ci].allow_host_execution`、`[timeouts].job_secs`
→ `AppState`），仓库配置里不存在（复核 R18-9），所以"草稿区强制 false"只能落在 per-pipeline 参数与
runner 运行时判断上，本实现两处都做了。

## 7. REST 契约

```
POST   /api/v1/repos/{owner}/{name}/agent-drafts        { prefix, ttl_secs, grace_secs? }  → 201
GET    /api/v1/repos/{owner}/{name}/agent-drafts                                        → 200 [ ... ]
PATCH  /api/v1/repos/{owner}/{name}/agent-drafts/{id}   { ttl_secs }                    → 200
DELETE /api/v1/repos/{owner}/{name}/agent-drafts/{id}                                   → 202（关闸，宽限后删）
```

响应：`{ id, repo_id, prefix, owner_user_id, ttl_secs, expires_at, delete_due_at, grace_secs, state, extended_count, created_at }`。

| 场景 | 状态码 |
| --- | --- |
| `prefix` 不合法 / `ttl_secs` 不在 `[60, 30d]` / `grace_secs` 不在 `[0, 30d]` | 400 |
| 调用者不能写该仓库 | 403 |
| `agent` 段不是调用者自己或自己拥有的 agent | 403 |
| 令牌有分支白名单且 `prefix` 不在其中（`ensure_token_may_write_branch`） | 403 |
| `(repo, prefix)` 已存在（含 `expiring` / `deleted`，消息区分） | 409 |
| `extend` 已到 `created_at + 4 × ttl_secs` | 400 |
| `extend` / `delete` 一个 `deleted` 的命名空间 | 409 |
| id 属于别的仓库 | 404 |

`extend` 语义：基准是 `max(now, 当前 expires_at)`（只延长不缩短），结果 `min(..., created_at + 4 × ttl_secs)`；
在 `expiring` 上 extend 会把状态改回 `active`（宽限期是"救回来"的窗口，与设计 §5.4 一致）。

授权：读用 `require_read`；写用 `require_write` + `ensure_token_may_write_branch(prefix)`，保证"令牌能推哪个
目录"和"能登记哪个目录"是同一个答案。`route_meta.rs` 里 `/agent-drafts` 声明 `.open()`（登记不移动 ref，
handler 自查前缀）、`/agent-drafts/{id}` 声明 `.checked()`（extend/delete 的 handler 都调用了
`ensure_token_may_write_branch`，漂移测试会验证这一点）。

## 8. 跨后端结果

新 SQL（建表、唯一索引、`(state, expires_at)` 索引、CAS 状态迁移、`expires_at <= now` 过滤、审计写入）
在三个后端各跑一次：

| 后端 | 迁移 up → down → up | 集成测试（6 个） |
| --- | --- | --- |
| SQLite（默认） | `down_drops_the_table_and_up_recreates_it` ok | 6 passed |
| PostgreSQL 17 | `down_and_up_round_trip_on_the_backend_from_the_environment` ok | 6 passed |
| MySQL 8.4 | 同上 ok | 6 passed |

命令与真实输出见 `docs/plans/T11-task.md` §4。MySQL 需要 `--log-bin-trust-function-creators=1`
（既有迁移里就有触发器/函数，与本任务无关），已在报告里注明。

## 9. 遗留风险

| # | 风险 | 说明 |
| --- | --- | --- |
| 1 | R18-5 的"假 `branch.created`"仍存在（非草稿路径） | 草稿路径两种形态都已抑制；普通分支"删除时多投一条 `branch.created`"是既有语义缺陷，属 T21/webhook 语义变更，本任务不动 |
| 2 | 草稿分支的 CI **没有单独的超时上限** | 设计 R3 建议"job 超时收紧到草稿专用值"。`job_timeout_secs` 是实例级参数，收紧会改变所有 pipeline，超出本任务边界；如实记录 |
| 3 | 清理**不取消**该目录上已触发的 pipeline，也不记录 `pipeline_ids` | T11.md 的表结构没有 `pipeline_ids` 列；清理只删 ref。已在 pipeline 侧保留日志/制品，交给既有 retention |
| 4 | 每次推送多一次 `list_by_repo`（每 ref 一次） | 一个仓库的草稿命名空间是个位数；没有为它加缓存 |
| 5 | 外部 runner 的日志遮蔽仍会解密全仓 secrets（只用于 `mask_values`，不下发给 job） | 为兼容既有 mask 行为保留；不构成泄漏（值只进入遮蔽器） |
| 6 | `refs/pull/*` 可被无白名单令牌改写（复核 R18-1） | 与本任务无关，但意味着"草稿区不该有 PR"不是内核保证；清理已用"绕过 on_delete + CAS"处理被 PR 占用的分支 |

## 10. 一句话结论

草稿区 = 一张窄表 + `post_push_hooks` 里的一个 `is_draft` 判断 + 一个两阶段清理 tick；通知抑制集中在
`post_push_hooks`（外加 `on_delete` 的第四处），真正需要盯住的是 **CI 执行边界**——secrets 整段不注入、
host execution 三层拒绝——这两条不是优化项，是这个功能能不能开的前提。
