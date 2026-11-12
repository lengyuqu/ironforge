# 协作用户门与 agent 门（issue #34）：现状核实

> 对象：issue #34「协作用户门要求 kind = human（T22）」正文 + 设计文档 `agent-principals-design-and-roadmap-2026-11.md` §2.3 条 9 / §4 T22（本 issue 的正文是它 §4 的摘要）。
> 基线：`fa57f3a`（`cnb/main`，2026-11-12）。本文件只核**它对现状的断言**，方案取向记在 §4。
> 判定口径：`成立` = 断言与现树逐字相符；`部分成立` = 主命题成立但名字/范围/机制有偏差；`不成立` = 现树给出相反事实。
> 汇总：**13 条 → 成立 8 / 部分成立 3 / 不成立 2**，另有 6 条两份材料都没写的新事实（§3）。

## 1. 三个"用户门"构造点（行号按 `fa57f3a`）

`Actor { kind, id }`（`rg-core/src/collaboration/mod.rs:75-78`）的 `kind` 只有 `"session"` / `"user"`（`:81-93`），落库后成为 9 个协作实体的 `actor_kind` 列。构造 `Actor::user` 的地方**不是只有 `write_actor` 一处**：

| # | 位置 | 形态 | 覆盖 15 个写？ |
|---|---|---|---|
| 1 | `api/collab.rs:107`（`write_actor` 的 else 分支） | `Actor::user(user_id)` | 是（15/15 调用点都先过 `require_collab_write`） |
| 2 | `api/collab_inbox.rs:166`（`mark_user_read`，`PUT …/inbox/user/read`） | 直接构造 `Actor::user(user_id)` | **否**——绕过 `write_actor`，且它是写（写一条 read-mark 行） |
| 3 | `rg-core/src/collaboration/inbox.rs:199`（`for_user`，收件箱读路径） | `Actor::user(user_id)`，只用于拼查询条件 | 否（只读，无需拦） |

另有一处不经过 `Actor` 的写：`rg-core/src/collaboration/takeover.rs:291-303` 的 `reassign` 把 actor 记成 `user_id.to_string()`（`api/collab_takeovers.rs:116` 调它），同样不经 `write_actor`。

## 2. 逐条判定表

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| N1 | `api/collab.rs:94-109`：带会话头 → `Actor::session`，否则一律 `Actor::user(user_id)` | 成立 | `:94-96` 文档注释、`:97` 签名、`:103` `if headers.contains_key(HEADER_SESSION) \|\| …_SECRET`、`:105` `Actor::session`、`:107` `Actor::user`；区间终点 `:109` 正是 `}` |
| N2 | 该分派**不判断账号类型** | 成立 | `:97-109` 函数体无任何 `users` 查询（`find_by_id` / `user_ops` 0 命中），只有 `acting_session` 与两个构造器 |
| N3 | 全仓 `collab*.rs` 零处 `users.kind` 判断 | 成立 | 29 个 `api/collab*.rs` 逐个 `grep -c '\.kind\|is_agent\|ActorKind'`：命中项全是无关语义（`collab.rs:87` 的 `gix::object::Kind::Commit`、decisions/messages 的 `kind` 字段、questions 的 `recipient.kind`、search 的 `kind` 参数、work_packages 的 `evidence.kind`）；`is_agent` 在 `collab*.rs` **0 命中** |
| N4 | `write_actor` 有 15 个调用点，分布在 budgets/conditions/decisions/dependencies/groups/questions/work_packages 七个文件 | 成立 | `grep -rn 'write_actor(' crates/` 去定义行 = **15**，逐文件 2/2/2/2/3/3/1，文件名与正文逐一相符；**补**：15/15 都紧跟 `require_collab_write`，所以门放在 `write_actor` 里即可，不必逐 handler 改 |
| N5 | `Actor` 取值只有 `"session"` / `"user"`（设计文档 §4 T22 引 `mod.rs:72-94`） | 成立 | `:75-78` 结构体、`:81-93` 两个构造器，没有第三个 |
| N6 | `collab_conditions.rs::confirm_condition`（`:178`）是"人作出的确认"，现状无人类校验 | 成立 | `:186` 只过 `require_collab_write`；`:184-185` 注释自述 "an explicit confirmation by an authorised account"，无 `ensure_human` |
| N7 | `PUT …/inbox/user/read` 可保留，理由是"无害读标记"（设计文档 §4 T22 要点 3） | **部分成立** | 它是 **PUT 写**：`collab_inbox.rs:156` `mark_user_read` 经 `:162 require_collab_write` 后写一条 read-mark 行；且 `:166` **直接构造 `Actor::user(user_id)`**，不经 `write_actor` ⇒ 只改 `write_actor` 时它仍是一条"agent 走用户门"的写。保留可以，但不能用"只拦写"来论证 |
| N8 | 方案示例 `if crate::api::repo_access::is_agent_account(&state.db, user_id).await?`（设计文档 §4 T22 代码块） | **不成立** | `is_agent_account` 全仓 **0 命中**；`api/repo_access.rs` 全文 0 处 `agent`。现成判定是 `rg_core::agent::is_agent(&User)`（`agent.rs:48`），需要先取 `users` 行（`user_ops::find_by_id`） |
| N9 | "加 `Actor::agent` 会连带改 capabilities 契约、审计口径与前端" | **部分成立** | 前端 ✓（比原文更强）：`actor_kind` 是 **9 个协作实体**的落库列（`collab_answer` / `collab_decision` / `collab_decision_change` / `collab_takeover` / `collab_inbox_read` / `collab_completion_condition_set` / `collab_completion_snapshot` / `collab_coordination_group` / `collab_coordination_link`），`web/src/routes/[owner]/[repo]/collaboration/tasks/[id]/+page.svelte:518` 把 `d.actor_kind` 原样塞进 `collab.decisions.by` 的插值；**capabilities ✗**：`rg-core/src/collaboration/capabilities.rs` 的 `Capability`/`Route` 无 actor 字段；**审计 ✗**：`rg-core/src/audit/` 0 处 `actor` |
| N10 | "每次协作写多一次 `users` 查询，可忽略"（设计文档 §4 T22 要点 4） | 成立（但可省） | 计数对：`write_actor` 一次写调一次。可省：`require_collab_write`（`collab.rs:211-212`）→ `can_write_repo` → `repo/service.rs:339` `apply_owner_cap` → `:256 owner_cap` 已经 `find_by_id(actor)` 一次，且 `OwnerCap::Owner(_)` 本身就是"这是 agent"的判据 ⇒ 再查是同一请求内的第二次 |
| N11 | "要缓存就加短 TTL 的 `kind` 缓存，不要塞进权限缓存（不变量 4）" | 成立 | §2.3 条 4（本文件同目录设计文档 `:70`）原文即"agent 的判权结果不进 30 秒权限缓存"；`apply_owner_cap` 对 agent 返回 `cacheable=false`（`:266`） |
| N12 | "依赖 B1（T23/T24/T25）先落地"（issue 正文首段） | **不成立** | `on_behalf_of` 全仓仅 1 处，是 `rg-core/src/agent.rs:23` 的 doc 注释（"Not covered here (later phases): … an `on_behalf_of` audit column"）；`rg-db/src/entities/audit_log.rs` 无该列（列为 id/user_id/username/action/resource_*/ip_address/user_agent/details/created_at）⇒ B1 三条一条未落 |
| N13 | 测试 5 条可直接写（agent 无会话头写 → 403、带会话头 → 200、人类 → 200、`confirm_condition` → 403、agent 读 → 200） | 部分成立 | 断言可执行，但没有现成夹具：`agent_principal_tests.rs`（900+ 行）`grep -c collaboration` = **0**；14 个 `collab_*_tests.rs` 里 `create_agent` / `kind: Set("agent")` **0 命中** ⇒ "agent × 协作"目前没有任何测试，需自建 agent 账号 + PAT 夹具 |

## 3. 两份材料都没写的新事实

- **F1 `actor_kind` 的语义面比文档写的大**：它是 9 个协作实体的落库列（清单见 N9），且前端把一个未翻译的 `kind` 直接插进文案。新增第三种值不是"改一个枚举"，是改一个已落库的字符串域 + 一处 UI 文案。
- **F2 用户门有三个构造点**（§1 表），只改 `write_actor` 覆盖 15 个写，漏 `mark_user_read`；`reassign` 连 `Actor` 都不用。
- **F3 门的位置可以更省**：`can_write_repo` 已经知道 `OwnerCap`（N10），把 cap 从 `apply_owner_cap` 带出来即可，不必在 `write_actor` 再查一次 users。
- **F4 不变量 9 是 §2.3 里唯一未落地的一条**：条 1（无人类入口）、2（令牌只有仓库 scope）、3（owner cap）、4（不进缓存）、5（`ensure_human`）、6（停用即失效）、7（他人 agent 一律 404）、8（agent 不得批准）都有对应实现或测试，只有条 9 没有任何拦截代码。它却以"改动时必须保持"的形态列出，读文档的人会以为已落地。
- **F5 拒绝口径已分叉**：`AGENT_MAY_NOT_APPROVE` 走 `AppError::coded(...).with_reason(...)`（`api/auth.rs:165-186`），`ensure_human` 只有自由文本 `CoreError::forbidden("an agent account may not do this; ask its owner")`（`agent.rs:126-136`）。方案让 `confirm_condition` 用 `ensure_human`，同一类拒绝在协作区就拿不到机器可读 reason。
- **F6 模板写不是漏网**：`create_template` / `new_template_version`（`collab_templates.rs:176` / `:224`）走 `require_template_admin`（`:145-157` → `can_admin_repo`），而 `can_admin_repo` 在 `repo/service.rs:356-359` 已经"agent 永不管理仓库"⇒ agent 被挡在门外，与不变量 9 一致。

## 4. 待拍板（本流程不动代码）

| 编号 | 级别 | 事项 |
|---|---|---|
| R34-1 | P1 | T22 落地本身：写侧权限收紧，agent 无 collab session 时协作写 403。改的是认证/权限语义 ⇒ L2 |
| R34-2 | P2 | `mark_user_read`（`collab_inbox.rs:166`）是否纳入"用户门只服务人类"。纳入 = 改行为；不纳入 = 在 §2.3 条 9 写明例外（"read-mark 允许 agent 写"），否则该条不可执行 |
| R34-3 | P2 | `reassign`（`takeover.rs:291-303`）记 actor 用 `user_id` 字符串而非 `Actor`，落地条 9 时需一并裁决（是否也要求 session） |
| R34-4 | P2 | 稳定 reason：新增 `AGENT_MUST_USE_SESSION` 走 `with_reason`；`ensure_human` 是否补一个通用 reason（否则"agent 被拒"在协作区不可机读） |
| R34-5 | P3 | 设计文档 §4 T22 引用不存在的 `is_agent_account`（本轮已按现树 API 订正 1 行） |
| R34-6 | P3 | 声明依赖 B1（T23/T24/T25）未落地（N12）⇒ T22 排期应后置，或先做 T23 的审计列 |

## 5. 本轮 L1 改动

1. `crates/rg-http/src/api/collab.rs:94-96`：`write_actor` 的文档注释补一句"这里不读 `users.kind`，agent 尚未被拒绝使用用户门"（3 行 → 3 行，零漂移）。
2. `ironforge-docs/architecture/agent-principals-design-and-roadmap-2026-11.md`：§2.3 条 9 标注**当前未强制**并指向 §4 T22（1 行 → 1 行）；§4 T22 的"方案"句补机器可读 reason 的写法（1 行 → 1 行）；代码块里不存在的 `is_agent_account` 换成 `rg_core::agent::is_agent` + `user_ops::find_by_id`（1 行 → 1 行）。
3. 本文件 + `ironforge-docs/README.md` 索引一行。

三处 1→1 行替换是刻意的：该设计文档被多处按行号引用（§2.3 的不变量 4 在 `:70`、§4 T22 在 `:241`、§5 R5 段在 `:508-512`），插行会让既有引用当场失准。
