# 提交作者与推送者归因（T25）现状核实 · 2026-11

> 来源：issue #36（`提交作者与推送者归因（T25）`，`2026-11-11T18:05:57Z` 创建）＋ 设计文档 `agent-principals-design-and-roadmap-2026-11.md` §4 T25（`2026-11-10` 快照）。
> 取证时点：`2026-11-12T06:3xZ`；基线 `6d47c28`（本机 `HEAD` 与 `cnb/main` 同一个 SHA，`git worktree` 内检出）。
> 本轮形态：该 issue `comment_count = 0`，先发只读首扫请求（评论 `2120766935095750656`，`06:33:33Z`），`60 s × 5` 到 `06:38:33Z` 平台上仍只有我方一条 ⇒ 按 issue 正文推进。**核实对象是它对现状的断言，不是 NPC 的指摘。**

## 1. 结论摘要

23 条现状断言：**成立 12 / 部分成立 6 / 不成立 5**。

三处真缺陷（正文与设计文档都没写）：

1. **调用点不是两处，是三处。** 除 `repo_content.rs` 的单文件创建/更新（`:1460`）与删除（`:1600`）外，`2026-11-11` 合入的批量提交 `POST /repos/{o}/{r}/commits` 走 `api/repo_commits.rs:174` → `commit_batch(..., &user.username, &user.email, ...)`（`:185-186`）。按 T25 原文只改 `repo_content.rs`，agent 走批量提交仍会写出 `agent-<uuid>@agents.invalid`。
2. **`git push` 路径没有任何审计行。** 「审计行带推送者 id」是「新建能力」而不是「确认已有」：`audit::record_write` 全仓 12 处调用点无一在 push 路径；push 里唯一一条审计是**分支删除**（`lib.rs:3108` 的 `repo.branch_delete`，`details.via = "git_push"`）。
3. **`repo_events` 表没有主体列。** 推送者只能塞进 `payload` JSON（`entities/repo_event.rs:9-17` 只有 `id / repo_id / event_type / payload / created_at`），不可索引、不可按主体筛选——与 T23 已定的「不把 `on_behalf_of` 塞 `details`（JSON 无法索引）」同一原则冲突。

两条可复用判据：

- **判「改提交身份要动几处」**：数**服务端 commit 入口**（`create_or_update_file_with` / `delete_file_with` / `commit_batch`）的调用点，不要数「字段出现在哪些文件」。本轮第三处入口是另一条路由新增的，按文件名找会漏。
- **判「某事件带不带某主体」**：三层依次查——① 表有没有那一列；② payload 构造点有没有那个键；③ 有没有审计行。三层都缺时，「确认已有」是假命题，不能当验收前提。

## 2. 判定表（逐条）

| # | 断言（出处） | 判定 | 证据 |
|---|---|---|---|
| N1 | `repo_content.rs:1460-1478` 把 `&user.username` / `&user.email` 直接传给 `create_or_update_file_with`（issue 正文·现状） | **成立** | `repo_content.rs:1470` = `&user.username,`、`:1471` = `&user.email,`；调用自 `:1460` 起，参数表到 `:1480` 的 `)` 收口（原文区间写 `-1478`，短了 2 行，语义无误） |
| N2 | 被调方是 `repo/service.rs:1799` | **成立** | `:1799` = `pub async fn create_or_update_file_with(` |
| N3 | 后者到 `rg-git/src/ops.rs:1333` 用同一 signature 作为 author 和 committer | **部分成立** | 语义成立：`ops.rs:1579-1585` 构造唯一 `signature`，`:1587` = `repo.new_commit_as(signature, signature, message, new_tree, parents)`（同一值传两次）。**行号不成立**：`:1333` = `tree: new_tree.to_string(),`（在 `plan_tree_edits` 的返回结构里） |
| N4 | 零 `kind` 分支 | **成立** | `grep -c kind crates/rg-git/src/ops.rs` = 16，全部是 `gix::object::Kind`（`:74`/`:612`/`:683`/`:983`/`:1168`）或 `entry.mode().kind()`（`:1519`），无 `users.kind` / `is_agent` |
| N5 | agent 的 `email` 是 `agent-<uuid>@agents.invalid`，uuid 不可读、不稳定 | **成立** | `rg-core/src/agent.rs:169` = `let email = format!("agent-{}@agents.invalid", uuid::Uuid::new_v4());`；`users.email` 是唯一列（`m20260424_000001_create_users.rs:34-37` `.unique_key()`） |
| N6 | 「分支未合并，无存量需回填」（设计文档 §4 T25 方案 1） | **不成立（同文档自相矛盾）** | 同文件 `:39` 的标题就是「## 2. 第一期现状（分支 `feat/agent-principals`，已合 main）」；迁移 `m20261110_000001`、`rg-core/src/agent.rs`、`rg-http/src/api/agents.rs` 全在当前树里 |
| N7 | 「若已合并则只在提交时派生，不动库」 | **成立（作为落地分支可行）** | `commit_identity` 是纯函数：入参 `&User`，出参 `(String, String)`，不需要写库；`is_agent(&User)`（`agent.rs:48`）读的就是已加载的行 |
| N8 | 方案 2 要新增 `rg_core::agent::commit_identity(user) -> (String, String)` | **成立（现状确实不存在）** | `grep -rn commit_identity crates/` = 0 命中；新增时应复用 `is_agent`（`agent.rs:48`），不要写第 4 份 agent 判定（前三份：`AGENT_MAY_NOT_APPROVE`、`can_admin_repo` 的 `owner_cap != None`、`ensure_human`） |
| N9 | 「`repo_content.rs` 两个调用点（创建/更新 :1460、删除 :1571）」 | **不成立（三处 + 行号错）** | 三处：`repo_content.rs:1460`、`repo_content.rs:1600`（`delete_file_with`）、`api/repo_commits.rs:174`（`commit_batch`）。`:1571` 是 HTTP handler `pub async fn delete_file(` 的声明，不是调用点；`&user.username` / `&user.email` 在 `:1609-1610` |
| N10 | 归因靠审计（T23） | **部分成立（依赖未落地）** | `grep -c on_behalf_of crates/rg-db/src/entities/audit_log.rs` = 0；全仓唯一命中是 `rg-core/src/agent.rs:23` 的 doc 注释（"Not covered here (later phases): … an `on_behalf_of` audit column"）⇒ T23 未落地，T25 的 `on_behalf_of = owner` 断言今天跑不通 |
| N11 | 归因靠「仓库事件」 | **部分成立** | REST 三处写**都有**审计行（`repo_content.rs:1515` `repo.file_commit`、`:1646` `repo.file_delete`、`repo_commits.rs:219` `repo.batch_commit`，均带 `user.id`）；但「仓库事件」表 `repo_events` 无主体列（见 F3） |
| N12 | 方案 4「git push 作者字段由客户端控制，服务端改不了也不该改」 | **成立** | `receive_pack` 只落对象、不改 commit header；服务端唯一自建身份在两处：`ops.rs:309-313`（merge/replay 的 committer 硬编码 `IronForge <noreply@ironforge.local>`）与 `ops.rs:1579-1587`（REST 提交的 author=committer），前者保留原作者（`:378-389` 的 `new_commit_as(committer, author, …)`） |
| N13 | 「`spawn_post_push_hooks`（`lib.rs:3190`）」 | **部分成立（行号不成立）** | 真值 `lib.rs:3202` = `pub(crate) fn spawn_post_push_hooks(`；`:3190` = `StatusCode::GATEWAY_TIMEOUT,` |
| N14 | 它写的 `repo_events` payload 带推送者 id | **不成立** | push payload 构造在 `lib.rs:3505-3513`，键只有 `ref / before / after / repository{owner,name}`；`PostPushParams`（`:3246-3260`）无 `actor` / `user_id` 字段，`spawn_post_push_hooks` 的入参也没有（`:3163` / `repo_content.rs:1382`） |
| N15 | 审计行带推送者 id | **不成立（push 无审计行）** | `grep -rn "audit::record_write(" crates/` = 12 处，无一在 push 路径；`"repo.push"` 全仓 0 命中。push 内唯一审计来自分支删除（`lib.rs:3108`，可用的推送者变量见 `:3109` 的 `if let Some(user_id) = actor`） |
| N16 | 「缺则补 `pushed_by`」 | **成立（可行，但要改签名）** | 推送者 id 在调用点**已经可用**（`:3109` 已在用它记 `repo.branch_delete`）⇒ 只需把 actor 透传进 `spawn_post_push_hooks` → `PostPushParams` → payload。**入参变更属接口面改动（L2）**，且落点需裁决（见 R36-2） |
| N17 | 测试「agent REST 提交 → `git log --format='%an <%ae>'` 为 agent 名与派生邮箱」 | **成立（可写）** | `create_or_update_file_with` 的 author 直接来自 `ops.rs:1579` 的 `SignatureRef`，落库即字节；agent 夹具已有（`rg-http/tests/agent_principal_tests.rs` 的 `World` + `agent()`/`agent_token()`） |
| N18 | 测试「人类提交不变」 | **成立** | 人类分支返回 `(username, email)` 与现状 `:1470-1471` 逐字一致；`commit_identity` 只需在 agent 分支改变输出 |
| N19 | 测试「audit 行 `user_id = agent`、`on_behalf_of = owner`（依赖 T23）」 | **部分成立** | `user_id = agent` 今天就有（`repo_content.rs:1515-1518` 传 `user.id`）；`on_behalf_of` 依赖未落地的 T23（同 N10）⇒ 该断言必须标 blocked-by-T23，否则 T25 的测试永远红 |
| N20 | 测试「push 事件带推送者」 | **不成立** | 同 N14：payload 无该键、表无该列、审计无该行 |
| N21 | 完成口径「`cargo clippy --workspace --all-targets -- -D warnings` 干净」 | **成立（可直接复跑）** | `scripts/check.sh:41` 就是这条命令（`:36-37` 先跑 `cargo fmt --all --check`） |
| N22 | 完成口径「新 SQL 至少跨后端跑一次（`IRONFORGE_TEST_DATABASE_URL`）」 | **部分成立（本方案无对象）** | 基建在：`rg-http/tests/common/mod.rs:251`、`rg-http/tests/collab_common/mod.rs:37`、`rg-core/tests/multi_backend_smoke.rs`；但 T25 明确「只在提交时派生，不动库」⇒ 零新 SQL。仅当 `pushed_by` 落库（新列/新表）才有对象 |
| N23 | 完成口径「不动 §2.3 的不变量」 | **成立** | 不变量 1 要求「邮箱在 `@agents.invalid`」——派生式 `<username>@agents.invalid` 仍满足（域不变）；`users.email` 唯一 + `username` 唯一 ⇒ 派生映射是单射，不会撞既有行 |

## 3. 新发现（正文与设计文档都没有）

- **F1（最重要）第三处提交入口。** `api/repo_commits.rs:174-187` 调 `commit_batch(..., &user.username, &user.email, ...)`（`:185-186`）。这是「多文件原子提交」（issue #16）在 `2026-11-11` 合入的新路由，设计文档快照早于它。同一文件随后走 `announce_branch_move`（`repo_content.rs:1363`）→ `spawn_post_push_hooks`，所以 F2/F3 的推送者缺口对**REST 批量提交**同样成立。
- **F2 推送没有审计行。** 只有 push 内部的**分支删除**有（`lib.rs:3108`，`details.via = "git_push"`）。所以「谁推的」今天唯一可查的地方是 webhook 消费侧（`repo.push` 事件的 payload），而 payload 里也没有推送者。
- **F3 `repo_events` 无主体列。** `entities/repo_event.rs:9-17`：`id / repo_id / event_type / payload / created_at`。把 `pushed_by` 放进 payload 等于放弃按主体索引——与 T23 已定的「不塞 `details`，因为 JSON 无法索引」（设计文档 §4 T23 的「不做」段）同一条原则相抵。
- **F4 服务端合并提交的身份是常量。** `ops.rs:309-313` 把 committer 硬编码成 `IronForge <noreply@ironforge.local>`，author 保留原提交者。⇒ agent 的 PR 被服务端合并时，git 层看不出「谁点的合并」，只能看分支移动审计（`repo.branch_delete` 那一类）。这不是 T25 的缺陷，但它让「T25 完成后 agent 的提交都可归因」这句话有一个盲区，建议另开题。
- **F5 现有测试不锁 `agent-<uuid>` 前缀。** `agent_principal_tests.rs:290` 只断言 `row.email.ends_with("@agents.invalid")` ⇒ 改成派生式不会破现有测试，只需新增 author 断言。
- **F6 注册侧不拒绝 `.invalid` 域。** 邮箱校验只有 `user/service.rs:602` 的 `email.split_once('@')` + local/domain 非空；`fn validate_email` 全仓 0 命中。⇒ 人类理论上可注册 `alice-agent-bot@agents.invalid` 字面地址先把唯一键占掉，此后建同名 agent 会得到 409（`agent.rs:171-174` 把唯一冲突映射成 `conflict`）。影响可控，但决定「派生式是否需要防撞」时要看它。

## 4. 待拍板（L2，本轮不动代码）

| ID | 级别 | 事项 |
|---|---|---|
| R36-1 | P2 | T25 落地范围：**三处**入口（`repo_content.rs:1460` / `:1600` / `repo_commits.rs:174`）+ `commit_identity` 复用 `is_agent`（`agent.rs:48`），不要新写第 4 份 agent 判定 |
| R36-2 | P2 | `pushed_by` 落点裁决：① 只进 `repo_events.payload`（零 SQL，但不可索引）；② 新增审计 action `repo.push`（能按 `user_id` 过滤，但 push 是高频写，审计量级要评估）；③ `repo_events` 加 `actor_id` 列（改 schema） |
| R36-3 | **P1** | 「谁推的可查」= **新能力**而不是补字段：今天 push 既无审计行，payload 也无推送者（F2/F3）。排期时不要按「S（2–3 小时）」估 |
| R36-4 | P3 | 存量 agent 回填裁决。本机 `ironforge.db` 未迁移到 `m20261110_000001`（无 `kind` 列，`select … where kind='agent'` 报 `no such column`）⇒ **无法用本机库证明有无存量**，需生产库确认；若有存量，`<username>@agents.invalid` 派生只影响新提交、不回填历史 commit（历史 commit 的 author 是 git 对象，改不动） |
| R36-5 | P3 | 注册侧不拒 `.invalid`（F6）：若采纳派生式，需决定是否在 `validate_username`/注册校验里顺手拒绝该域 |
| R36-6 | P3 | 服务端 merge/rebase 的 committer 恒为 `IronForge`（F4）：若要「合并者归因」，另开题（不在 T25） |

## 5. 复核命令（可复跑）

```bash
cd <repo>
# 三处提交入口
grep -rn "create_or_update_file_with(\|delete_file_with(\|commit_batch(" crates/rg-http/src/api/
# author 与 committer 同值
sed -n '1579,1590p' crates/rg-git/src/ops.rs
# agent 邮箱现状
sed -n '169p' crates/rg-core/src/agent.rs
# push 无审计行、payload 无推送者
grep -rn '"repo.push"' crates/ ; sed -n '3505,3514p' crates/rg-http/src/lib.rs
grep -rn "audit::record_write(" crates/ | wc -l
# repo_events 无主体列
sed -n '9,17p' crates/rg-db/src/entities/repo_event.rs
# T23 未落地
grep -c on_behalf_of crates/rg-db/src/entities/audit_log.rs
```
