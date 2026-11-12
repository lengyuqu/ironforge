# 通知改派给 agent 所有者（T24）现状核实 · 2026-11

> 对象：issue #35（`通知改派给 agent 的所有者（T24）`）+ 设计文档 `agent-principals-design-and-roadmap-2026-11.md` §4 T24。
> 基线：`721ab16`（本仓 `cnb/main`，2026-11-12）。
> 形态：issue 正文是**设计摘要**（`comment_count = 0`，无 NPC 分析），所以本轮核的是「它对现状的断言」，不是核 NPC 的指摘。
> 本文只记录核实结论与待拍板项，**不含产品代码改动**（T24 本体是功能新增，属 L2）。

## 1. 结论

- 正文「现状」四段**全部成立**（9 条），且行号与现树一致（`recipients()` `:69-125`、`task_blocked` `:189-247` 与设计文档标注逐字相同）。
- 「方案」段有 **1 条不成立**：给通知收件人加 `kind = human` + `email NOT LIKE '%@agents.invalid'` 的过滤，声称能关闭「agent 的 `.invalid` 邮箱是否会被邮件路径发信」这个未核实项 —— **该路径不发信，过滤零效果**；真实的三处发信口都已被别的机制挡住。
- 方案里另有一条**该写而未写**的前提：`agent::owner_cap`（`crates/rg-core/src/agent.rs:249-262`）已经实现了「agent → 所有者（须存在、启用、非 agent），否则拒绝」的**同一套规则**。新造 `accountable_user` 若不叠在它上面，本仓就出现**第 4 份** agent 判定（前三份：`AGENT_MAY_NOT_APPROVE`、`can_admin_repo` 的 `owner_cap != None`、`ensure_human`）。
- 目标「铃铛响在人那边」在**当前改动范围**下只对「被问」与「卡住」两个场景成立：另有 **4 处**通知生产点仍按 user id 直发（watchers / 评论 / 指派），agent 收得到。

## 2. 判定表（19 条）

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| N1 | `recipients()` 按 `question.recipient.kind` 分支 | **成立** | `human_notify.rs:75` `match question.recipient.kind.as_str()`；三分支 `:76` `RECIPIENT_USER`、`:94` `RECIPIENT_REPO_WRITERS`、`:120-122` `_ => {}` |
| N2 | `user` 分支 = 存在性 + `can_read_repo` | **成立** | `:85` `user_ops::find_by_id`、`:88` `can_read_repo(db, repo, Some(id))`，两守卫内联在 `if` 里 |
| N3 | `repo_writers` = owner + `write`/`admin` 协作者 + 组织成员，`can_write_repo` 过滤，**500 候选 / 50 收件人** | **成立** | 候选装配 `:95-105`；`MAX_CANDIDATES: usize = 500`（`:44`）、`MAX_RECIPIENTS_PER_QUESTION: usize = 50`（`:40`）；循环 `:107-118` |
| N4 | `session` 分支只发 durable notice | **成立** | `:120-122` 注释 + `_ => {}`；notice 实体在 `collaboration/notice.rs`，落库在 `question.rs:301-322`（`notice::prepare` → `create_idempotent_with_notices`） |
| N5 | 全模块**无任何 agent 判断** | **成立** | `grep -nE 'is_agent / users\.kind / kind *= *"agent"' crates/rg-core/src/collaboration/human_notify.rs` = **0**；文件里 `agent` 只出现在注释与 `agent_session_ops` |
| N6 | `task_blocked` 由 `owner_session_id` → `find_in_repo` → **`session.user_id`** 收件 | **成立** | `:195` `task.owner_session_id`、`:198` `agent_session_ops::find_in_repo`、`:206` `let user_id = session.user_id;` |
| N7 | agent 用自己令牌建会话时该 id 就是 agent | **成立** | `collab_sessions.rs:90` `require_collab_write` 取 actor、`:94` 直接把该 id 交给 `svc::create`；`collab.rs:202-217` 的 `require_collab_write` 只查 `can_write_repo`，**无 kind 门** |
| N8 | 人类永远看不到「你的 agent 卡住了」 | **成立** | 收件人恒为 `session.user_id`（`:206`），此后无第二收件人；`owner_user_id` 在通知链路上 **0 命中**（全仓除 `agent.rs`/`entities`/`migrations`/`user_ops.rs` 外无消费点） |
| N9 | 调用点只有两处 | **成立** | `human_notify::` 全仓 4 命中：2 处定义（`human_notify.rs:131`/`:189`）+ `collab_questions.rs:162`、`collab_tasks.rs:276` |
| P1 | 新增 `rg_core::agent::accountable_user(db, user_id) -> Option<i64>` | **部分成立** | 函数不存在（`grep -n accountable_user crates/` = 0），但语义已有 **2/3**：`agent.rs:249-262` 的 `owner_cap` 返回 `None`（human）/ `Owner(owner_id)`（owner 须 `is_active && !is_agent`）/ `Denied`（缺失、停用、owner 也是 agent）。缺的只有「human 分支要判 `is_active`」——而该行 `owner_cap:250` 已经读过、只是丢弃 |
| P2 | `recipients()` 候选逐个映射、**映射后**去重再套 50 上限 | **部分成立** | 现状去重发生在**映射前**、按候选 id：`seen.insert(id)`（`:111`）；cap 检查 `:108` 数的是 `out`（已过权限的收件人）。改为映射后去重不需要动 cap 口径，但**必须**把去重挪到映射之后，否则「owner + 它的 agent 同为候选」会发两条 |
| P3 | `task_blocked` 收件人过 `accountable_user`；`None` 不发；**不要 fallback 到 `repo_writers`** | **部分成立（约束成立，理由不成立）** | `task_blocked` 现状**没有**任何 fallback：`:195-197` 无 session、`:199-201` 会话不存在、`:208-210` 不可读、`:211-214` 权限查询失败、`:220-227` 去重命中 —— 全部 `return Vec::new()`。所以「会广播」这个风险在现树不存在，该句只能当预防性约束读 |
| P4 | 通知路径当前不发邮件（要先 `grep lettre/send_mail/smtp` 确认） | **成立（不发）** | `notification::notify`（`notification/mod.rs:9-20`）只调 `notification_ops::create_notification` 落库；`human_notify.rs` 与 `notification/` 内 `lettre` / `send_mail` / `smtp` **0 命中**；全仓发信函数只有 `rg_core::email::send_html_notification`（`email/mod.rs:82`） |
| P5 | 「给映射后的收件人加 `kind=human && email NOT LIKE '%@agents.invalid'` 过滤」能关闭 `.invalid` 未核实项 | **不成立** | 该路径不发信 ⇒ 过滤零效果。真实发信口三处，且**都已挡住**：① `rg-http/src/lib.rs:3465` CI 通知，收件人取 `repo_owner_id`，而 `repo/service.rs:489` 的 `create_repo` 有 `ensure_human`（fork `:1167`、转移 `:1392-1393` 同样有）⇒ `repository.owner_id` **恒为人类**；② `lib.rs:3678` 是重试 worker，payload 只由 `:3486`（同一 CI 收件人）与 `user/service.rs:763` 入队；③ `user/service.rs:745` 密码重置，`:711` 对 `auth_provider != "local"` 或 `is_agent(&user)` 直接返回（**agent 走不到发信**） |
| T1 | 五条测试可写 | **部分成立** | 落点存在：`crates/rg-http/tests/collab_human_notify_tests.rs`（430 行 / 9 个测试 / `owned_task`、`blocked_for`、`bell` 现成助手），但它的 `collab_common::Env/Sess` **没有**造 agent 的能力（该文件与 `collab_common.rs` 内 `agent` 0 命中）。能造 agent 的夹具在 `agent_principal_tests.rs`（1191 行，`World::agent()`/`agent_token()`/`grant()`，走 HTTP `POST /agents`）。全仓 `tests/` 目录里 `rg_core::agent::create_agent` **0 命中** |
| T2 | 「owner 与 agent 都在 `repo_writers` → 只收一条」是待修行为 | **成立** | 现状按 user id 去重（`:111`），agent 与 owner 是两个 id、且 agent 经 `apply_owner_cap` 继承 owner 的写权限（`repo/service.rs:256-267`）⇒ 现树会发**两条** |
| D1 | 完成口径用 `cargo clippy --workspace --all-targets -- -D warnings` | **成立** | `scripts/check.sh:40-42` 就是这条命令（`--quick` 与全量都跑它） |
| D2 | 新 SQL 至少跨后端跑一次（`IRONFORGE_TEST_DATABASE_URL`，PG/MySQL） | **部分成立** | 能力在：`Cargo.toml:43` sea-orm 带 `sqlx-postgres`/`sqlx-mysql`，`rg-db/src/lib.rs:74-84` 有 `postgres:`/`mysql:` 分支，`rg-core/tests/multi_backend_smoke.rs` 存在；但**本任务不新增 SQL**（纯读 + 走既有 `create_notification`）⇒ 该条对 T24 无对象 |
| D3 | 不破坏 §2.3 的不变量 | **成立，且是前提** | §2.3 条 5「agent 什么都不拥有」由 `ensure_human` 落实（`repo/service.rs:489`）⇒ `repo.owner_id` 恒为人类，映射链不会指向另一个 agent；`owner_cap:260` 的 `!is_agent(&owner)` 是第二层保险 |

## 3. 新发现（两份材料都没有）

- **F1 通知面不止 `human_notify`。** 另有 4 处按 user id 直发：`notification/mod.rs:122`（`notify_watchers`，watch 仓库的人）、`issue/reactions.rs:29` / `:97`（评论与回复）、`issue/assignees.rs:52`（被指派）。⇒ T24 只改正文那两处时，「铃铛响在人那边」只在**被问**与**卡住**两个场景成立；agent 被指派 issue、被回复、watch 仓库时仍会自己响铃。
- **F2 「改派」是全新能力，不是接线。** `owner_user_id` 在 `crates/` 内除 `agent.rs`、`entities/`、`migrations/`、`user_ops.rs` 外**零消费点**——没有任何通知/邮件/webhook 路径读过它。
- **F3 `accountable_user` 应叠在 `owner_cap` 上。** 现成的 `owner_cap:249-262` 已经把「agent → 所有者（存在、启用、非 agent），否则拒绝」写完，`accountable_user` 只需补 human 分支（`user.is_active`）。独立重写会产生第 4 份 agent 判定，与本流程 R19-5 的既有结论（复用 `AGENT_MAY_NOT_APPROVE` + human-only 计数，不新造判定）冲突。
- **F4 `.invalid` 未核实项今天可以结案为「否」。** 依据是三道闸（`ensure_human` 守 `repository.owner_id`、密码重置 `:711` 拒 agent、重试队列只有两个入队点），**不是**收件人过滤。
- **F5 映射顺序影响查询成本。** 现状每个候选都 `invalidate_perm_cache_user` + `can_write_repo`（`:114-115`），而 agent 的 `can_write_repo` 会递归到所有者（`apply_owner_cap` → `can_write_repo(owner)`）。映射是纯读、权限查是重活 ⇒ 应「先映射去重、再查权限」，否则 500 个候选里若混入 N 个 agent，会多付 N 次 owner 权限链。

## 4. 待拍板（L2，本流程不动 main）

- **R35-1（P2）** T24 落地是实现新能力（改通知语义），不在自动路径内；且 `accountable_user` 必须薄封装 `owner_cap`，否则第 4 份 agent 判定（F3）。
- **R35-2（P2）** 范围裁决：只改 `human_notify` 两处（覆盖「被问 / 卡住」），还是连 `notify_watchers` / 评论 / 指派一起改（覆盖全部铃铛）。两种选择的用户可见行为不同：后者会让「agent 的 watch」变成「所有者的 watch」，可能造成噪音（F1）。
- **R35-3（P3）** `.invalid` 未核实项（设计文档 §2.5 的 P2 行，第 99 行）建议直接结案为「否」，依据写三处发信口与两道 agent 拒绝（F4）；**不要**按原方案给收件人加过滤（零效果）。
- **R35-4（P3）** 测试落点：给 `collab_common` 加 agent 助手（`POST /agents` + 令牌），或把 T24 测试写进 `agent_principal_tests.rs` 的 `World`；两者都不要新造第三套夹具（T1）。
- **R35-5（P3）** `recipients()` 的映射/去重/权限查顺序（先映射去重、再查权限），以及 `MAX_CANDIDATES=500` 在映射后是否还有意义（F5、P2）。

## 5. 复核方法（可重跑）

```bash
# 现状断言
sed -n '69,125p' crates/rg-core/src/collaboration/human_notify.rs
sed -n '189,247p' crates/rg-core/src/collaboration/human_notify.rs
grep -rn 'human_notify::' crates/ --include=*.rs
# 已有判定（owner_cap）
sed -n '249,262p' crates/rg-core/src/agent.rs
# 「agent 不拥有仓库」这条前提
grep -n 'ensure_human' crates/rg-core/src/repo/service.rs
# 发信口（三处）与密码重置的 agent 拒绝
grep -rn 'send_html_notification' crates/ --include=*.rs
sed -n '709,715p' crates/rg-core/src/user/service.rs
# 通知面（4 处直发）
grep -rn 'notification::notify(\|notification_ops::create_notification' crates/ --include=*.rs | grep -v /tests/
```
