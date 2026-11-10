# Agent 主体身份与 agent 友好化路线图（设计 + 第一期验证记录，2026-11-10）

> **状态**：第一期已实现并在 SQLite 上验证，第二、三期未做 ｜ **可信度**：实现与测试为确认；路线图中"缺失"的判断为 **推测**（grep 级别，未逐一读实现）
> **来源**：仓库代码（`rg-core/src/agent.rs`、`rg-http/src/api/agents.rs`、`rg-http/tests/agent_principal_tests.rs`）+ 2026-11-10 的运行结果
> 写给接手的 agent：先读"第一期现状"和"不变量"，再读"第二期"，要加路线图里的其他项先看"路线图"的前置依赖。

## 1. 背景与目标

IronForge 的目标是 agent 友好、可自部署的 Git 服务端，参考 Gitea、GitHub、CNB。现有 agent 能力（PAT scope 阶梯、仓库/分支白名单、幂等键、事件日志、协作工具、MCP stdio 与 streamable HTTP）已经很多，但**身份模型仍是"人"**：

- `access_tokens`、`ssh_keys`、`agent_sessions`、`audit_log` 全部只挂 `user_id`；
- PAT 换出的是令牌所有者的完整会话 JWT，只多一个 `tid`；
- 评审的自批检查是 `pr.author_id == user_id`（`rg-http/src/api/reviews.rs`），agent 借人的账号开 PR 就会把这个人锁在批准之外。

所以先把"agent 是谁、代表谁、最多能做什么"建成一等概念，后面的审批、配额、审计、评审规则才有地基。

## 2. 第一期现状（已实现）

### 数据模型

迁移 `m20261110_000001_add_user_kind_and_owner`（`up` 幂等，`down` 为空）给 `users` 加：

| 列 | 含义 |
|---|---|
| `kind` | `human`（默认，所有旧行）或 `agent`，`NOT NULL` |
| `owner_user_id` | agent 的所有者（人类账号）；人类为空。**无外键**（SQLite 无法给已有表加），由应用层保证 |
| `agent_description` | 用途说明，仅展示 |

agent **就是 `users` 里的一行**，没有单独的表。原因：一百来个实体的作者、评论者、协作者字段和 `can_read/can_write` 都以用户 id 为键，另建主体表意味着处处多态。代价是必须关死所有"人类入口"（见不变量 1）。

### 核心代码

- [`rg-core/src/agent.rs`](../../crates/rg-core/src/agent.rs)：`create_agent`/`list_agents`/`update_agent`/`find_owned_agent`、`ensure_human`、`owner_cap`/`OwnerCap`、`validate_agent_token_scopes`、保留命名空间 `is_reserved_username`。
- [`rg-http/src/api/agents.rs`](../../crates/rg-http/src/api/agents.rs)：`/agents`、`/agents/{username}`、`/agents/{username}/tokens[/{id}]`。令牌签发复用 `users::issue_token(.., for_agent = true)`。
- [`rg-core/src/repo/service.rs`](../../crates/rg-core/src/repo/service.rs) 的 `apply_owner_cap`：权限上限，被 `can_read_repo`/`can_write_repo` 调用；`can_admin_repo` 对 agent 恒 false。
- 用户名：`<所有者>-agent-<名字>`，名字 1–20 位字母数字/`-`/`_`。人类用户名与组织名**禁止**包含 `-agent-`（`rg-core/src/lib.rs` 与 `user/service.rs` 两个 `validate_username` 都检查）。创建 agent 时也拒绝与已有组织同名（owner 解析是"先用户后组织"，同名会遮住组织）。

### 不变量（改动时必须保持）

1. **agent 没有人类入口。** `auth_provider = "agent"`、`password_hash = "!"`（不是哈希）、邮箱在 `@agents.invalid`。已显式拒绝：密码登录（`user::service::login`）、密码重置（`forgot_password`/`reset_password`）、SSO 按邮箱匹配（`api/sso.rs`）、SSH 密码（`rg-ssh`）、添加 SSH 公钥、`update_user_admin` 设管理员、创建 PAT 的普通入口（`users::create_token` 先 `ensure_human`）。**新增任何登录方式，必须同步拒绝 agent，并加进测试 `an_agent_has_no_human_entrance`。**
2. **agent 令牌只能带仓库类 scope**（默认 `repo:write`），所以进不了 `/users`、`/agents`、`/admin`（`required_pat_scope` 里 `/agents` 归 `user` scope）。
3. **权限 = agent 自己的授权 ∩ 所有者当前的权限。** 所有者不存在、被停用、或不是人类 → 一律拒绝。agent 永不管理仓库（`can_admin_repo` 恒 false，因此不能增删协作者、改设置、删仓库）。
4. **agent 的判权结果不进 30 秒权限缓存**（`apply_owner_cap` 返回 `cacheable=false`）；所有者一侧走所有者自己的缓存项。原因：现有的缓存失效调用点指向所有者的 `(repo, user)`，不会指向 agent，缓存 agent 的"是"会让撤权延迟 30 秒。代价：agent 每次判权多几次查询。
5. **agent 什么都不拥有**：`ensure_human` 守住创建仓库、fork、转移（两端）、创建组织。名下有 agent 的账号不能被删除（409）。
6. **停用 agent** = `is_active=false` + `token_version += 1`：令牌被拒、已换出的会话立即失效；重新启用后同一令牌恢复。停用状态下不能签发新令牌（409）。
7. 别人的 agent 与不存在的 agent 对调用者一律 404（防探测）；`/agents` 只有人类能调用。

### 验证证据（实现 → 任务验证；**未到集成/运行/发布层**）

| 命令 | 结果 |
|---|---|
| `cargo clippy --workspace --all-targets -- -D warnings` | 无告警 |
| `cargo test -p rg-http --test agent_principal_tests` | 8 项通过（SQLite） |
| `cargo test -p rg-db` | 通过，含迁移回滚后重放 |
| `cargo test -p rg-http -p rg-core -p rg-ssh -p rg-mcp --no-fail-fast`（最终代码，日志在 2026-11-10 的会话草稿目录，未入库） | 128 个测试目标通过；`collab_session_tests::t02_revoked_write_access_blocks_the_next_request_despite_a_warm_permission_cache` **失败过一次**（"control: cache should still be warm"），同一目标随后整体重跑 3 次、单测重跑 3 次均通过 |

关于那次失败，**推测**为既有的跨测试缓存碰撞而非本次改动引入：权限缓存是进程级、按 `(repo_id, user_id)` 记，同一测试二进制里并发的测试各自有数据库，id 会撞，别的测试的 `invalidate_perm_cache_user` 可能恰好清掉本测试刚预热的项。**未在 main 上对照复现**，所以不是已证事实；若它在 CI 上反复出现，先用 `git worktree` 在 main 上重复跑这个目标判断是否既有。`agent_principal_tests` 自己用一把静态锁串行化并在每个测试开头清缓存，就是为了避开同类问题。

**没跑过**：PostgreSQL、MySQL（新迁移与新增查询未在这两个后端验证；按 CLAUDE.md 第 19、23 条，新增并发 SQL 应用 `IRONFORGE_TEST_DATABASE_URL` 跑一遍）；前端；真实 MCP 客户端；真实 git push 的 agent 路径（只测了 REST 文件提交作为写判权的代表）。

### 已知缺口（第一期刻意没做，或排查中发现）

- **绕过 `can_*_repo` 的判权点**：凡是直接查协作者表或组织成员表、不经过 `can_read_repo`/`can_write_repo` 的代码，不受 agent 上限约束。**未逐一排查**。`grep -rn 'repo_collaborator_ops::get_permission\|is_org_member\|find_org_member' crates` 是起点。
- **旧问题，与本次无关，未修**：SSH 密码认证不检查 `is_active`，被停用的人类账号似乎仍能用密码走 SSH；协作者接口里改权限按协作者行 id、删除按用户 id 寻址，删除传错 id 也返回成功。
- agent 创建的数量上限 50（`MAX_AGENTS_PER_OWNER`）是拍脑袋的默认值。
- 邮箱是合成的 `agent-<uuid>@agents.invalid`；是否有邮件发送路径会给 agent 的 `.invalid` 地址发信**未核实**，也没有守卫测试；第二期改通知路由时一并查。

## 3. 第二期（下一步，未做）

| 项 | 要点 | 触点 |
|---|---|---|
| 评审规则 | 所有者可批准自己 agent 的 PR；agent 不能批准其所有者或同所有者其他 agent 的 PR；agent 的批准默认不计入分支保护必需批准数 | `api/reviews.rs`（`pr.author_id == user_id`）、`api/auth.rs::ensure_session_may_approve`、分支保护的批准计数；**已定默认**：所有者批准自己 agent 的 PR **计入**必需批准数（否则单人仓库无法合并 agent 的 PR），要更严再加分支保护开关 |
| 协作"用户门"要求 `kind=human` | 现在靠"请求里没有会话头"区分人与 agent，agent 不带头就能冒充人 | `rg-http/src/api/collab*.rs` 的用户门入口。**推测**如此，动手前先读实现确认 |
| 审计 | `audit_log` 加可索引列 `on_behalf_of_user_id`，不塞进 `details` JSON | `api/audit.rs::record_write`、`with_access_token`；迁移要幂等并带 `has_column` 守卫 |
| 通知 | 发给"负责会话所属账号"的通知，账号是 agent 时改发所有者 | `rg-core/src/collaboration/human_notify.rs` |
| 提交作者 | REST 文件提交用 agent 名 + noreply 作者；git push 的作者字段控制不了，只在事件日志记录推送者 | `create_or_update_file_with` |
| API 响应带主体类型 | 响应里的主体统一带 `kind`/`owner`，并登记进 capabilities 注册表 | `rg-core/src/collaboration/capabilities.rs` |

## 4. 第三期（未做）

Web UI、组织所有的 agent、按 agent 的配额与限流、短时令牌交换（复用 CI OIDC 的 Ed25519 基础设施）。agent 的 SSH 密钥**第一期明确不支持**：SSH 推送不走令牌，会绕过分支白名单。

## 5. 路线图：其他 agent 友好化补充（均为建议，未开工）

下表按优先级排。"现状"一列来自 2026-11-10 对源码的关键词排查，**属推测**，动手前请先读实现确认（例如 `CODEOWNERS` 在 `rg-core/src/review/codeowners.rs` 与 `rg-http/src/api/pulls.rs` 里已有痕迹）。

### 第一梯队：扩展性地基

1. **路由元数据注册表。** 现状：PAT scope 判定（`required_pat_scope`）、仓库白名单拒绝表（`path_in_allowlist`）、分支白名单放行表（`ref_limited_write_allowed`）、幂等支持表（`idempotency_supported`）、协作 capabilities 注册表是各自手工维护的几张表，CLAUDE.md 里有多处"新增路由必须同步加入"的提醒。建议每条路由声明一次元数据（所需 scope、是否跨仓引用、是否自检分支、响应是否含秘密、是否幂等安全），这些表与 OpenAPI 扩展字段由它派生，未声明的路由默认拒绝。收益：新增任何令牌限制维度只加一个字段，不再有"漏登记即越权"。
2. **Agent 主体身份。** 本文第 2、3 节。
3. **统一事件总线。** 现状：webhook 事件、`repo_events` 长轮询、WebSocket 通知三条路，事件日志是尽力而为写入，SSE 未做。建议带 schema 版本的事件信封 + 业务事务内写 outbox 保证至少一次，webhook/长轮询/SSE/MCP 资源订阅都作为消费者；加组织级、实例级事件流。也为多实例的失效广播（CLAUDE.md 的"多实例进程内状态"）留接口。

### 第二梯队：agent 写路径缺口

4. **多文件原子提交**：`contents` 写是单文件的。建议 `POST …/commits`，一次带多个文件动作（增改删移）与 `expected_sha`，支持直接应用 unified diff；MCP `commit_files`；服务端 cherry-pick/revert。
5. **可合并性与冲突详情**：PR 上下文明确写着 `not_evaluated: ["mergeable", "branch_protection"]`（`rg-http/src/api/ai_pr_context.rs`）。建议合并就绪端点，返回冲突文件与区块、每条保护规则的判定与机器码、待完成的必需检查；再加"把 base 合进 head"。`merge_trees` 已有。
6. **临时 ref 与预检 CI**：带 TTL 的草稿命名空间（如 `refs/agent/<id>/*`），可跑 CI 但不开 PR、不通知，到期清理。让分支白名单令牌能安全试错。
7. **推送时密钥扫描**：`receive-pack` 与 REST 提交路径上拦截，用现有 `reason`/`hint` 结构返回。
8. **通用审批原语**：CI 环境审批、协作问题、完成条件确认是三套相似机制。抽象成"一次性授权请求"：agent 申请越权动作，人批准后得到绑定具体 SHA 的单次许可。要求先补通知偏好与邮件（目前没有）。

### 第三梯队：差异化功能

9. **代码智能**：tree-sitter 符号层（定义/引用/文件大纲/按符号读取）+ 带 token 预算的仓库地图；索引器做成 trait，以后可接向量检索。现有只有 FTS5。
10. **配额与用量**：有了 agent 主体后，按主体统计 CI 分钟、存储、请求量并做硬限制（协作预算目前仅信息性）。
11. **Agent 工作区**：对标 CNB 云原生开发环境，在 `rg-runner` + `rg-ci-isolation` 上提供临时沙箱。工作量最大，放最后；先解决 CLAUDE.md 里"CI 容器 root 写宿主文件"的遗留。

### 依赖关系

- 第二期评审规则、审计列、通知改派 → 都依赖第一期（已完成）。
- 8（通用审批）→ 依赖 2 与通知偏好。10（配额）→ 依赖 2。
- 1（路由元数据）独立，且越早做越省事：之后每新增一个 agent 路由都要受益。

## 6. 给接手 agent 的工作约定

- 读 [`.ai/guardrails.md`](../../.ai/guardrails.md)：新增迁移、改认证与权限逻辑都是 🟠 CONFIRM，需要先让项目负责人确认。
- 迁移：显式表名（不要用 `#[derive(Iden)]` 的单数默认，见 CLAUDE.md 第 13 条）、`has_column` 守卫、`down` 为空时 `up` 必须幂等；`rg-db/src/lib.rs` 里 `upgrade_keeps_child_rows_triggers_and_foreign_keys` 的 `Migrator::down(&db, Some(N))` 要随新增迁移加一（本次从 24 改为 25）。
- 新增 `users` 实体字段时，`user_ops.rs` 里所有 `user::ActiveModel { .. }` 字面量要补齐（本次补了 `create_user`、`create_ldap_user`，`create_agent` 新增）；`rg-db/src/lib.rs` 的迁移测试在"只迁移一半"的库上必须用原生 SQL，不能用实体。
- 跨后端：新增 SQL 用 `IRONFORGE_TEST_DATABASE_URL` 在 PostgreSQL 与 MySQL 上各跑一遍（CLAUDE.md 第 19、23 条）。
- 权限缓存是进程级的，写测试时注意跨测试碰撞（参考 `agent_principal_tests.rs` 的 `ONE_AT_A_TIME` 与开头清缓存）。
- 本文"路线图"中的现状判断是 grep 级别的；动手前先读代码，别直接信。
