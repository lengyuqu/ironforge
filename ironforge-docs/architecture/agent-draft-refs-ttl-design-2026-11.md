# Agent 草稿命名空间（TTL）：设计细化（Proposed，2026-11-11）

> **状态**：设计草案，**未实现** ｜ **可信度**：仓库代码事实为"确认"（2026-11-12 已在 `e225293` 上逐条复核：25 条中 17 成立 / 6 部分成立 / 2 不成立，见 [agent-draft-refs-ttl-verification-2026-11.md](agent-draft-refs-ttl-verification-2026-11.md)）；git 协议行为与性能量级为 **推测**（未跑真实 git 客户端验证）｜ **来源**：本次通读 `crates/rg-git`、`crates/rg-http`、`crates/rg-ci`、`crates/rg-core/src/auth` 的结论
> 议题：Issue #18 ——「给 agent 一个带 TTL 的草稿命名空间（如 `refs/agent/<id>/*`），推上去可跑 CI 但不开 PR、不触发通知，到期自动清理。这样分支白名单令牌能安全试错。」
> 读法：先看 §1 边界与 §9 结论，再看 §3（隔离实现，最容易踩坑的一节）。

---

## 1. 问题与目标

分支白名单令牌（`access_tokens.ref_allowlist`，见 [agent-token-ref-allowlist-verification-2026-11.md](agent-token-ref-allowlist-verification-2026-11.md)）已经能把 agent 的写权限压到 `refs/heads/agent/*`。缺的是三件配套：

1. 分支不会自己消失。每次试错都在仓库里留一条分支，只能靠人删。
2. 现在的推送会顺带产生"社交噪声"：`push` webhook、分支创建/删除 webhook、实时 ws 通知、CI 触发的邮件。
3. 分支名是**全仓库唯一命名空间**。两个 agent（甚至同一个 agent 的两条工作线）在同一仓库里写 `agent/fix-parser` 会互相踩；而令牌白名单只能写"允许哪些名字"，拦不住别人已经占了同名分支。

**目标**：把 `refs/heads/agent/<id>/*` 这种"每 agent 独占的一段分支"变成带过期时间的草稿区——能推、**能跑 CI**、不产生面向人的通知、过期自动删。

**非目标**：不做 PR 沙箱（`refs/pull/*` 由服务端 `publish_head_ref` 写入；带 `ref_allowlist` 的令牌写不进它，无白名单的令牌可以——见 R18-1），不做 CI 脚本的人审门（那是另一件事：见 §8 风险 R3），不改 SSH 通道的令牌语义。

---

## 2. 关键术语与现状前提（代码事实）

| 事实 | 位置 | 影响 |
|---|---|---|
| 分支白名单只对 `refs/heads/<pattern>` 生效；tag 和其他命名空间一律拒绝 | `rg-core/src/auth/pat_scope.rs:268-273`（`ref_allowlist_full_patterns`） | 命名空间可以是"普通分支"或"独立命名空间"，两种都要显式处理（见 §4） |
| 推送拒绝发生在读 pack 之前；一次推送内允许"部分成功" | `rg-git/src/protocol/receive_pack.rs:427-453` | 白名单外被拒后，pack 仍被 `drain_pack` 读掉再返回 → 用 `git push` 试错不会被卡住（好） |
| 一条 ref 的失败**不会中止**后续 ref 的更新 | `receive_pack.rs:412`（命令先全标 `ok`）、`:487`（只跳过非 `ok`） | "不许往 main 推"的真实行为是：**该 ref 被拒，同批其他 ref 照常落地，push 退出码非 0**（见 §8 R1，这算一个现存瑕疵） |
| 分支删除走 `on_delete` 守卫（open PR 占用、分支保护、默认分支都拒） | `rg-http/src/lib.rs:3082-3140` | 清理任务删草稿分支时必须**绕开**这个 REST/推送守卫，否则"分支上还挂着草稿 PR"就永远删不掉（见 §6.3） |
| 推送后钩子：CI → `push` webhook → 分支事件 webhook → ws 通知 | `rg-http/src/lib.rs:3290-3590`（`post_push_hooks`；入口 `spawn_post_push_hooks` 在 `:3202`） | 通知抑制的 3 个注入点在这里，且已有 `if update.status != "ok" { continue; }` 的前置过滤；**第 4 个不在这里**——删分支路径的 `trigger_branch_deleted` 在 `on_delete` 内（`:3125`），本函数过滤不到（R18-5） |
| 每次 `trigger_event` 都会同时写 `repo_events`（长轮询日志） | `rg-core/src/webhook/service.rs:131-141`、`:227-239` | 想"不触发通知但保留事件"，必须把载荷写入与 hook 投递拆开（见 §5.3） |
| CI 读取配置：先 `.gitea/workflows/*.yml`，再 `.ironforge-ci.yml`，**在触发时从该 commit 的树里读** | `rg-ci/src/lib.rs:558-596`（`read_ci_config`） | ✅ **可行性关键**：推送方可以自带 CI 配置，不需要先合并到 main |
| Gitea 格式的 `on: push: branches:` 过滤器已实现（PR 事件过滤的是 base 分支） | `rg-ci/src/gitea_actions.rs:280-312`、`:698-732` | 每个草稿分支会被**所有** `on: push`（不带 branches 过滤）的工作流命中 → 需要 §5.2 的抑制开关 |
| 其余触发入口：手动 `POST .../actions/pipelines`、PR 评审请求 CI | `rg-http/src/api/ci.rs:465`、`api/reviews.rs:204` | 手动触发不受命名空间影响，天然可用 |
| `refs/pull/<n>/head` 由 `publish_head_ref` 写入，是**普通 ref**：`rg-git` 里没有任何命名空间守卫，落地写入是 `PreviousValue::Any` | `rg-core/src/pull_request/service.rs:1389-1430`；写入 `receive_pack.rs:507` → `:631-647` | **"不许开 PR" 只对带 `ref_allowlist` 的令牌天然成立**（前缀恒 `refs/heads/`）；无白名单令牌可改写该 ref，需另加守卫（见 R18-1），**不能写"不需要新代码"** |
| 后台周期任务已有统一模式（`task_supervisor::supervise` + `run_cleanup_loop`，1h 轮询） | `rg-http/src/lib.rs:246-275`；`api/ci_retention.rs:168-188`；`api/repo_events.rs:216-227` | 清理任务照抄这个形状即可，不需要新的调度设施 |
| 白名单模式支持 `*` 且 `*` 可跨 `/`；禁止以 `*` 开头 | `pat_scope.rs:213-257`（`normalize_ref_allowlist`） | `agent/a1/*` 已能表达"只允许 id 为 a1 这一段"；但"`owner/agent` 里 `*` 未展开的文件名"这类歧义要注意（§4.3） |

---

## 3. 命名空间：先定名字，再谈实现

### 3.1 方案 B（推荐）：`refs/heads/agent/<id>/...` —— 普通分支 + 保留前缀

- 与现有分支白名单**零改动**兼容：`refs: ["agent/a1/*"]` 今天就能用。
- 现有能力免费继承：分支保护、分支删除 API、`list_branches` 过滤、前端分支列表、`ssh` 路径、`refs/heads/` 假设（`update_open_head_sha`、合并队列、PR head 解析）全部按原样工作。
- 代价：`git branch -r` / 分支列表里会看到草稿分支。用 §7 的 `visibility` 字段把它们从"普通读者"的列表里过滤掉（仅过滤展示，不隐藏 ref）。

### 3.2 方案 C（可选叠加）：`refs/agent/<id>/...` —— 独立命名空间

- 分支列表天然看不到（大多数实现只列 `refs/heads/` 与 `refs/tags/`），`refs/heads/*` 假设也碰不到它。
- 代价（都要写代码）：
  - `ref_allowlist_full_patterns` 目前固定加 `refs/heads/` 前缀（`pat_scope.rs:268-273`）→ 模式需扩展成"`heads:`/`agent:` 前缀语法"或按模式自身是否以 `refs/` 开头分流。
  - `receive_pack` 的删除分支只接受 `refs/heads/`（`receive_pack.rs:393-401`、`:355-362`）→ 需要同源放开。
  - 清理任务用 `delete_ref`（`rg-git/src/ops.rs:105-124`）直接删即可，**没有** `on_delete` 守卫问题，反而更简单。
- **结论**：先用 B 落地（成本最低、风险最小），把 C 作为 B 的"隐藏度不够时"的后续项。本文后续以 B 为准，C 的差异单独标注。

### 3.3 命名建议

- `refs/heads/agent/<id>/...`，其中 `<id>` = **agent 用户 id 或 agent 用户名**。推荐 `<owner>-agent-<name>` 之外再用一段稳定短 id：`agent/<agent_id>/<free>`。理由：id 不随改名变化，且天然唯一，不需要额外查重。
- 令牌 `refs` 写成 `["agent/<我的 agent id>/*"]`。校验层建议加一条**新规则**：`agent/<数字>` 段只允许令牌所有者自己的 agent 使用（否则一个 agent 可以拿 `agent/*` 覆盖别的 agent 的草稿目录）。若不想引入身份耦合，至少文档要求创建令牌时人工填自己的 id（弱保证，写清楚）。

---

## 4. 数据模型

新增迁移 `mYYYYMMDD_000001_create_agent_draft_namespaces`（`up` 幂等 + `has_table` 守卫，`down` 为空，照 `m20261106_000004_create_repo_events.rs` 的写法）。

```sql
CREATE TABLE agent_draft_namespaces (
    id            INTEGER PRIMARY KEY,
    repo_id       BIGINT  NOT NULL,
    prefix        TEXT    NOT NULL,   -- 唯一键，如 'agent/42/'
    owner_user_id BIGINT  NOT NULL,   -- 谁建的（token 的 user_id）
    token_id      BIGINT  NULL,       -- 由哪个令牌创建，用于"一令牌草稿区"视图
    ttl_secs      BIGINT  NOT NULL,   -- 请求时给的 TTL（秒）
    expires_at    TIMESTAMP NOT NULL, -- 绝对时间，可续期
    grace_secs    BIGINT  NOT NULL,   -- 软删除后的宽限（默认 86400）
    hard_delete_at TIMESTAMP NULL,    -- 到期后计算，NULL = 未进入软删
    pipeline_ids  TEXT    NULL,       -- JSON 数组：本前缀上创建的 pipeline（清理时用）
    created_at    TIMESTAMP NOT NULL,
    updated_at    TIMESTAMP NOT NULL
);
CREATE UNIQUE INDEX idx_agent_draft_ns_prefix ON agent_draft_namespaces(repo_id, prefix);
CREATE INDEX idx_agent_draft_ns_expires ON agent_draft_namespaces(expires_at);
```

**`prefix` 的三个硬约束**（在 core 层校验，不靠数据库）：
1. 必须以字面段开头，禁止 `*`、禁止空段、禁止 `..`、禁止以 `/`、`.`、`.lock` 结尾——复用 `normalize_ref_allowlist` 的同一套校验，**再加一条**：不允许 `*`（前缀是具体路径，不是模式）。
2. 必须匹配 `agent/<seg>/`，`<seg>` 为字母数字/`-`/`_`，长度 1–40。
3. 不能与仓库默认分支、任何分支保护规则、任何已存在的 PR head 分支同名冲突（前缀是目录意图，只要前缀不是某个已存在的 ref 本身即可）。

**要不要新表？** 也可以不建表，把 `expires_at` 塞进 ref 名字（`agent/42/2026-12-01/foo`）或 git notes。否决理由：无法续期、无法记录 token/宽限期、清理要做字符串解析、无法列"我的草稿区"。**一张窄表最省事，且天然可以被审计/前端读取。**

### 4.1 与令牌白名单的关系

`prefix` 是**能力的收窄**，不是替代：

- 令牌 `refs` 仍必须自带 `agent/42/*`（或更窄的 `agent/42/fix-*`）；命名空间登记只是允许服务端在推送时给出**明确的拒绝理由**和**生命周期**。
- 建议默认行为：**未登记前缀 = 普通分支**（现有行为不变，向后兼容）。只有显式 `POST` 登记过（`ttl_secs` ≥ 60）的前缀才有 TTL 与通知抑制。这样老令牌/老分支不受影响。
- 可选的强模式（仓库级开关 `require_draft_namespace_for_agent_branches`）：命中 `agent/*` 但未登记则拒绝推送，理由 `"this branch is in an agent namespace and needs a registered draft (POST /agent-drafts)"`。**默认关**，避免破坏现有使用。

### 4.2 索引与查询

- 推送路径需要"按 ref 名前缀查命名空间"：一条 `find_by_longest_prefix(repo_id, refname)`，即把 refname 逐段截断后 `IN` 查询（最多 3 段，`agent` / `agent/42` / `agent/42/foo`），取最长命中。**每个 ref 一次查询**，一次推送 ≤ 若干 ref，可接受。
- 清理任务：`find_expired(now)` 走 `idx_..._expires`。

---

## 5. 生命周期

### 5.1 创建 / 续期 / 手动删除（REST）

```
POST   /api/v1/repos/{owner}/{name}/agent-drafts
       body: { "prefix": "agent/42/", "ttl_secs": 86400, "grace_secs": 86400 }
       → 201 { id, prefix, expires_at, hard_delete_at: null, pipeline_ids: [] }
       400: prefix 不合法 / ttl_secs 不在 [60, 30*86400]
       403: 调用者不是该仓库的写者，或 prefix 的 agent 段不是调用者自己的 agent
       409: 同 (repo, prefix) 已存在且未过期（返回现有记录，便于幂等）

POST   /api/v1/repos/{owner}/{name}/agent-drafts/{id}/extend   { "ttl_secs": 3600 }
       → 200 { expires_at }   只延长不缩短；累计不超过创建时 ttl 的 4 倍（防"无限续期"）

DELETE /api/v1/repos/{owner}/{name}/agent-drafts/{id}
       → 202  进入软删：hard_delete_at = now + grace_secs，分支保留但拒绝新推送

GET    /api/v1/repos/{owner}/{name}/agent-drafts          列出（owner/admin；agent 令牌看自己）
```

**授权**：`require_write` + `ensure_token_may_write_branch(prefix)`——令牌有分支白名单时，前缀必须在白名单模式内。这样"令牌能推哪个目录"和"能建哪个草稿区"永远是同一个答案。

### 5.2 推送时

在 `handle_receive_pack` 已有 `rejected_refs` 之外，新增一类"命名空间守卫"（同一个注入点，`rg-http/src/lib.rs:2996-3076` 组装 `rejected_refs`/`writable_refs` 的地方，`writable_refs` 在 `:2996`）：

| 情况 | 行为 |
|---|---|
| ref 命中已过期或软删中的命名空间 | 拒绝：`ng <ref> the draft namespace <prefix> expired at <ts>; push again to a new draft` |
| ref 命中前缀但 push 的 ref 恰好等于前缀本身（`agent/42`） | 拒绝：前缀是命名空间不是分支 |
| ref 命中前缀，但推送者是另一个 agent 的令牌 | 由 §3.3 的身份规则拦（可选） |
| 其他 | 不变 |

**一个值得单独写的坑**：`process_push_with_rejections` 只给命令打标记、**不中止同批其他 ref**（见 §2）。所以"推到 `main` 被拒、推 `agent/42/x` 成功"这种混合推送里，`main` 被拒是对的，但 `git push` 的打包数据已经包含在同一个 pack 里并执行了 `index-pack`（`receive_pack.rs:466-480`）。对象进了对象库但没有 ref 指向它——下次 `gc` 才回收。**这是既有行为，不是本设计引入的**，但草稿区的"低成本试错"会让它更频繁，建议在 §8 R1 里记一笔。

### 5.3 通知与事件抑制（三个注入点）

`post_push_hooks`（`rg-http/src/lib.rs:3290`，逐 ref 循环从 `:3321` 起）里判断 `is_draft = namespace::lookup(ref).is_some_and(active)`，然后：

| 触发点 | 现状 | 草稿区行为 |
|---|---|---|
| CI | `ci_engine.trigger_pipeline(... "push")` → 之后还会 `ws::push_notification(ci_triggered)` + SMTP 邮件（`lib.rs:3417-3495`） | **照常触发 CI**，但跳过 `ws` 通知与邮件 |
| `push` webhook | `webhook::service::trigger_event(db, repo_id, "push", payload)`（`:3516`） | 默认**不投递**，但**仍写 `repo_events`**（见下） |
| `branch.created` / `branch.deleted` webhook | `:3526-3548` | 不投递、不写事件 |
| ws `push` 通知 | `:3579-3586` | 跳过 |

**"仍写事件"需要一次小重构**：`trigger_event` 现在把"写 `repo_events`"和"投递 webhook"绑在一起（`rg-core/src/webhook/service.rs:131-141`）。拆成

```rust
pub async fn record_event(db, repo_id, event, payload) -> Result<()>;       // 仅日志
pub async fn trigger_event(db, repo_id, event, payload) -> Result<()>;      // 日志 + 投递（保持原语义）
pub async fn trigger_event_quietly(db, repo_id, event, payload) -> Result<()>; // 仅日志
```

好处：agent 侧仍然能 `wait_repo_events` 等到自己的推送（`GET /repos/.../events`，`api/repo_events.rs`），只是人不会被 call 起来。前端"活动"页要不要隐藏草稿事件，留给 §7 的 `feed_visibility`。

> ⚠️ 现存不一致（顺带记录）：`notify_watchers_push`（`rg-core/src/repo/service.rs:1590-1606`）定义了但**全仓库无调用**，所以"关注者收推送通知"这条路径今天其实没接线。草稿区设计不受它影响，但如果将来接线，要在那里加同一条过滤。

### 5.4 TTL 语义

- `expires_at = now + ttl_secs`。到期**不立刻删**：先进入"只读关闸"（拒绝新推送），`grace_secs` 之后才删分支。
- 为什么要宽限：CI 可能正好在跑到 90%，或者 agent 想读回结果。
- 宽限期内允许 `extend`（把 `expires_at` 往后推）——这是"救回来"的唯一途径，也需要写权。

---

## 6. 清理

### 6.1 后台任务形状

照抄既有模式（`rg-http/src/lib.rs:246-275`）：

```rust
tokio::spawn(rg_core::task_supervisor::supervise(
    "agent_drafts.cleanup",
    { let state = state.clone(); move || {
        let state = state.clone();
        async move { api::agent_drafts::run_cleanup_loop(state).await }
    }},
));
```

`run_cleanup_loop`：`interval(1h)`，每轮先标记（`expires_at < now AND hard_delete_at IS NULL` → 设 `hard_delete_at`），再清理（`hard_delete_at < now`）。**先标记后清理**，保证"到期了但还没删"这个中间态可观测、可手工续期。

### 6.2 单次清理的步骤（幂等、可重跑）

对每个待清前缀：

1. 列出该前缀下的所有 ref（`gix::references().all()` + 前缀过滤，**不要用 `refs/heads/` 的"分支列表"接口**，见 C 方案差异）。
2. 跳过仍被**未关闭 PR** 指向的 head 分支？——**不跳过**。草稿区不该开 PR，但 `refs/pull/` **不是**只读的（见 R18-1），别把"天然"写进实现假设；按 §8 R4，若确实存在同名 PR，删分支前先 `publish_head_ref` 固化 `refs/pull/<n>/head`，再从库里清掉记录（"强制关闸"），并写审计 `agent.draft.force_closed`。
3. 每个 ref 用 `rg_git::ops::delete_ref_if(path, ref, sha)`（CAS，`ops.rs:132-153`）删除：**绕开** `receive_pack` 的 `on_delete` 守卫（那个守卫的语义是"用户删分支"，清理不是用户行为）。
4. 把 `pipeline_ids` 里记录的 pipeline 标 `cancel`（若还在 pending/running）——**不删日志/制品**，交给现有 retention 策略（`api/ci_retention.rs`）。
5. 删除命名空间行；写审计 `agent.draft.cleanup`（`{prefix, deleted_refs, cancelled_pipelines}`）。
6. 全程 best-effort：单个 ref 失败记 `tracing::warn` 并保留行，下一轮重试（`failures` 计数，同 `CleanupResponse` 的形状）。

### 6.3 为什么不能用"删分支 API"

`rg-http/src/lib.rs:3082-3140` 的 `on_delete` 会拒绝：默认分支、受保护分支、**有 open PR 占用**。草稿分支在正常情况下三条都不命中，所以"顺手复用"看起来很划算——但一旦命中（agent 误开了 PR、或人给 `agent/42/*` 加了保护规则），清理就会**永久失败并每小时刷一次告警**。清理必须走 `delete_ref_if`，并且**显式记录**它绕开了守卫（这是这个设计里唯一"故意绕过护栏"的地方，审计必须留痕）。

---

## 7. 可见性与前端（最小集）

| 位置 | 变更 |
|---|---|
| `GET /repos/{o}/{n}/branches` | 新增 query `include_drafts=false`（默认 false）：草稿前缀下的分支需要 `include_drafts=true` 或仓库 admin。**这是可见性过滤，不是鉴权**：ref 本身仍可 fetch（agent 要能 `git fetch` 自己的草稿） |
| 仓库首页"分支"下拉 | 默认隐藏草稿；加一个"草稿（N）"折叠区 |
| 仓库设置 → Agent 草稿 | 列表（prefix、剩余时间、pipeline 数、删除按钮）；管理员可看全部 |
| `push` 事件流 / 活动页 | 草稿事件默认折叠（事件本身仍在 `repo_events` 里） |

前端 i18n 需同步 en / zh-CN（`scripts/i18n-key-check.mjs`）。

---

## 8. 风险与开放问题

| # | 风险 | 说明与建议 |
|---|---|---|
| R1 | 混合推送不会整体失败 | 一条 ref 失败不影响其他 ref 落地（§5.2）。想让"推到 main 失败"更醒目的代价是改造 `receive_pack` 的原子性，**不在本设计范围**，但要在文档里写清，别让用户以为"被拒了就啥都没推上去" |
| R2 | 对象孤儿 | 被拒 ref 的 pack 仍会 `index-pack`（§5.2），草稿区高频试错会加快对象库增长。建议清理任务顺带记录 `objects/pack` 增长率；真正回收靠既有 `gc` |
| R3 | **CI 脚本是人审盲区（最大风险）** | 草稿分支上的 CI 跑的是**推送方自带的** `.gitea/workflows/*.yml` / `.ironforge-ci.yml`（`rg-ci/src/lib.rs:131`）。凡是 agent 有 `repo:write` 且仓库开了 Docker runner 或（更糟）`allow_host_execution`，草稿区就等于"任意代码执行"通道，且**排除了人工 review 这一环**。需求方要的"能跑 CI"和这条是同一枚硬币。**注意量级：该通道今天就存在**（任意可写分支的推送都已触发 CI，`lib.rs:3418`），草稿区新增的是"不再有人被通知"⇒ 抑制通知前必须先关掉这个洞。必须明写：草稿分支默认只允许 **容器内** 执行（忽略 `allow_host_execution`，现由仓库开关直传 runner，无触发源判定），job 超时用仓库级 `job_timeout_secs`（`AppState` 已有），secrets 不注入（**已验证做不到**：`ci_secret` 只有 `repo_id`，注入点 `rg-ci/src/runner.rs:743-762` 一把全取无 ref 条件，见 R18-2）。若仓库把 `allow_host_execution` 打开，草稿区应默认**拒绝触发 CI**并给出明确理由 |
| R4 | 已开 PR 的草稿分支 | 见 §6.2 第 2 步：固化 `refs/pull/<n>/head` 后强制关闸 |
| R5 | 令牌与命名空间的授权重叠 | §5.1 用 `ensure_token_may_write_branch(prefix)` 保证两者一致；但 REST 的 `ref_limited_write_allowed`（`pat_scope.rs:301-336`）需要新增 `agent-drafts/**` 到白名单，否则 agent 令牌调不了这个 API（今天所有非白名单写路由默认 403） |
| R6 | `extend` 的滥用 | 累计不超过创建时 ttl 的 4 倍 |
| R7 | SSH 通道 | 令牌不用于 SSH，SSH 推送不受 `ref_allowlist` 与命名空间限制（现有事实）。如果 SSH 的 agent 也要受约束，得另做（用户级 SSH key → 命名空间），**列为后续** |
| R8 | 多后端的迁移 | PostgreSQL/MySQL 需各跑一次迁移与清理 SQL（`hard_delete_at` 的时间比较按后端写法），照 CLAUDE.md 的数据库约定走 |

---

## 9. 实施拆分与验收

| 步骤 | 内容 | 验收（任务验证层） |
|---|---|---|
| S1 | 迁移 + `rg-core/src/agent_draft.rs`（前缀校验、最长前缀查询、TTL/宽限计算，纯函数优先） | 单测：前缀校验表（空、`*`、`..`、控制字符、超长、非 `agent/` 段）、最长前缀命中、TTL 计算与 `extend` 上限 |
| S2 | REST `agent-drafts` 四个端点 + `ref_limited_write_allowed` 增补 + 前端最小入口 | `rg-http` 集成测试（真实 git + SQLite）：建草稿区 → 推送 → 过期为只读 → 宽限后清理；令牌白名单外前缀 403 |
| S3 | 推送守卫（`rejected_refs` 增补） | 真实 `git push`：过期前缀被拒且消息含前缀与到期时间；未登记 `agent/*` 仍可推（默认关强模式） |
| S4 | `webhook::service` 拆分 `record_event` / `trigger_event_quietly` | `repo_events` 有事件、webhook 投递表无记录、ws 无消息 |
| S5 | 清理任务 + 审计 + pipeline 取消 | 单测 + 集成：删 ref 走 CAS；有人工 PR 占用时走强制关闸分支；重跑幂等 |
| S6 | 文档：本篇转"验证记录"，补 `ironforge-docs/README.md` 索引与 `CLAUDE.md` 现状行 | CI 的 `changes` 分类不乱（只改文档 → 不跑测试） |

**整体验收口径**（对齐仓库证据分层）：实现证据（diff/SHA）→ 任务验证（上述命令通过）→ 集成验证（合并后门禁）→ 运行验证（真实服务 + 真实 `git` + 真实 CI runner 至少跑通一次"草稿推送 → CI 绿 → 到期清理"）→ 远端验证。**本设计不宣称任何一层已达成。**

---

## 10. 一句话结论

用**普通分支 + 保留前缀 `refs/heads/agent/<id>/`** 落地，成本最低且和现有白名单/分支保护/删除 API 天然兼容；把"TTL + 通知抑制 + 自动清理"做成一张窄表加一个后台任务，通知抑制的三个注入点全在 `post_push_hooks` 一处；**唯一必须盯住的安全面是草稿分支上的 CI 执行边界（R3）**，它不是本设计的副产品，而是设计的前提条件。
