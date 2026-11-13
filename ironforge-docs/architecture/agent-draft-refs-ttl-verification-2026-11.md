# Agent 草稿命名空间（TTL）：设计前提核实（2026-11-12）

> **状态**：核实记录（**不是**实现）｜ **对象**：issue #18 的 NPC 细化回帖（`2120150626892935168`，2987 字符）+ 其落地文档 `ironforge-docs/architecture/agent-draft-refs-ttl-design-2026-11.md`（PR #20 已合并，258 行 —— `wc -l` 与 `awk END{print NR}` 同为 258；旧写 259 是 `split('\n')` 把末行换行当空串的口径，已订正）｜ **基线**：`e225293`（`cnb/main`，`HEAD...cnb/main` = `0 0`）
> **读法**：先看 §3 两条不成立（它们改变设计结论），再看 §6 待拍板。§2 是全量判定表，§4 只是行号账。

---

## 1. 结论摘要

设计文档自称「标签：可信度 = 代码事实为确认（附文件:行，但 2026-11-11 未在仓库内逐条复核）」。本轮逐条复核 **25 条**现状断言：

| 判定 | 条数 | 含义 |
|---|---|---|
| 成立 | **17** | 结论与语义都对，行号可能需校正（见 §4） |
| 部分成立 | **6** | 语义对、区间端点偏 2–15 行；或结论对但漏了决定性限定 |
| 不成立 | **2** | 结论反了，且都影响设计能不能落地（§3.1 / §3.2） |

两条不成立都不是「文档写错一个数」，而是**设计前提被推翻**：

1. **`refs/pull/*` 不是只读命名空间**。设计三处（§1 非目标、§2 第 10 行、§6.2 步 2）以「内核级只读镜像，任何推送写必失败」为前提推出「不许开 PR 天然成立」。实测**没有任何命名空间守卫**：`receive_pack` 对 ref 名不做白名单，`update_ref` 用 `PreviousValue::Any` —— 无 `ref_allowlist` 的令牌（含 deploy key、bot 令牌、人用 PAT）可以直接改写 `refs/pull/<n>/head`。
2. **CI secrets 不按 ref 过滤**。R3 把它写成「需要确认是否已有能力——**未验证**」。现在验证完了：`list_by_repo` 一把全取、逐条解密、注入每个 job，**没有 branch/ref/tag 条件**。所以 R3 给的缓解措施（「secrets 不注入」）**今天做不到**，草稿区一旦触发 CI 就是「全仓 secrets + 推送方自带脚本」。

---

## 2. 判定表（25 条）

编号 `P*` = NPC 回帖正文，`D*` = 设计文档 §2 事实表及其余章节。

| # | 它说的 | 判定 | 证据（本机 `sed -n` 打印核对） |
|---|---|---|---|
| P1 | 分支白名单只拼 `refs/heads/<pattern>`，`refs: ["agent/42/*"]` 今天就能用 | **成立** | `pat_scope.rs:268-273`，前缀在 `:271` 的 `format!("refs/heads/{pattern}")` |
| P2 | `receive_pack` 删分支只认 `refs/heads/` | **成立** | `receive_pack.rs:393`（`new_sha.starts_with("0000000")`）→ `:394` 的 `refname.starts_with("refs/heads/")`，否则 `:398-400` 报 `only branches can be deleted`；简写形式同判在 `:355` |
| P3 | `read_ci_config` 触发时才读、从该 commit 的树里读 | **成立** | `rg-ci/src/lib.rs:131` 调用 → `:558` 定义；`:574` `format!("{}:.ironforge-ci.yml", commit_sha)` |
| P4 | `post_push_hooks` 逐 ref 走 CI → push webhook → 分支事件 webhook → ws，且已有 `status != "ok"` 前置过滤 | **成立**（行号 +12，见 §4） | `lib.rs:3290` 定义、`:3321-3324` 过滤、`:3417-3426` CI、`:3516` push webhook、`:3522-3548` 分支事件、`:3579-3586` ws |
| P5 | `trigger_event` 把「写 `repo_events`」与「投递 webhook」绑死 | **成立** | `webhook/service.rs:131` 定义；`:140` `record_repo_event(...)` 在投递之前、无条件 |
| P6 | 清理不能复用删分支 API：`on_delete` 守卫会拒有 open PR 的分支 | **成立** | `on_delete` `lib.rs:3082-3140`；其内部调 `delete_branch`（`:3102`），三守卫见 `repo/branch.rs:115`（默认分支）/`:118-120`（保护）/`:122-125`（open PR） |
| P7 | 清理用 `delete_ref_if` 做 CAS 删除 | **成立** | `ops.rs:132` 定义，`:143-152` 用 `PreviousValue::MustExistAndMatch` + `RefLog::AndReference` |
| P8 | 一张窄表 + 四个端点 + 后台任务照抄 `task_supervisor` 的 1h 轮询 | **成立** | 三段 `supervise` 在 `lib.rs:246-255`/`:256-265`/`:266-275`；`run_cleanup_loop` 在 `ci_retention.rs:168-188`（`sleep 3600`）与 `repo_events.rs:216-227`（`interval 3600`） |
| D1 | `pat_scope.rs:266-275` 是 `ref_allowlist_full_patterns` | **部分成立** | 实为 `:268-273`（含 `:266-267` 两行文档注释），起点 +2 |
| D2 | 推送拒绝发生在读 pack 之前（`receive_pack.rs:427-446`） | **部分成立** | 拒绝循环实为 `:427-453`（`writable_refs` 判定到 `:452`），pack 读取在 `:475`；语义对，端点短 7 行 |
| D3 | 命令先全标 `ok`（`:412`）；后续只跳过非 `ok`（`:486`） | **成立** | `:412` 是 `updates.push(RefUpdate {`、`:416` 才写 `"ok"`；`continue` 在 `:487` |
| D4 | `on_delete` 守卫在 `lib.rs:3070-3125` | **部分成立** | 实为 `:3082-3140`（`:3077-3081` 是解释注释），整体 +12 |
| D5 | `post_push_hooks` 在 `:3278-3575`，入口 `spawn_post_push_hooks` 在 `:3190` | **部分成立** | 实为 `:3290-3590`，入口 `:3202`（`pub(crate)`，见 R18-6） |
| D6 | `trigger_event` 写事件在 `webhook/service.rs:131-142`、`:227-241` | **成立** | `:131-141`；`record_repo_event` 定义 `:227-239` |
| D7 | `read_ci_config` 在 `rg-ci/src/lib.rs:558-596` | **成立** | 起点终点全中（`:596` 是 `}`） |
| D8 | Gitea `on: push: branches:` 过滤已实现（`gitea_actions.rs:280-312`、`:698-730`） | **成立** | `matches_event` `:280-312`；`ref_matches_filter` `:698-732`（`branches` `:703`、`branches-ignore` `:713`） |
| D9 | 其余触发入口：手动 `POST …/actions/pipelines`（`api/ci.rs:465`）、PR 评审请求 CI（`api/reviews.rs:204`） | **成立** | `api/ci.rs:465` `pub async fn trigger_pipeline`；`api/reviews.rs:204` `.trigger_pipeline(...)`，`trigger_type: "suggestion"` 在 `:210` |
| D10 | `refs/pull/<n>/head` 是内核级只读镜像，任何推送写 → 客户端 push 直接失败，「不需要新代码」 | **不成立** | 见 §3.1 |
| D11 | 后台周期任务模式：`lib.rs:245-274` + `ci_retention.rs:170-200` + `repo_events.rs:214-230` | **部分成立** | 实为 `:246-275` / `:168-188` / `:216-227`；形状（`supervise` + `run_cleanup_loop`）成立 |
| D12 | 白名单支持 `*` 且 `*` 可跨 `/`，禁止以 `*` 开头（`pat_scope.rs:211-262`） | **部分成立** | `normalize_ref_allowlist` 实为 `:213-257`；`*` 字符集在 `:230`、禁止开头在 `:234`；`*` 跨 `/` 由 `ref_matches_pattern` 的 DP（`branch_protection/service.rs:228-249`，未对 `/` 特判）成立 |
| D13 | §5.1 授权用 `ensure_token_may_write_branch(prefix)` | **成立** | `api/auth.rs:133` `pub(crate) async fn ensure_token_may_write_branch` |
| D14 | R5：`ref_limited_write_allowed` 未含 `agent-drafts`，不增补则 agent 令牌恒 403 | **成立** | `pat_scope.rs:301-336` 的 `matches!` 只有 `collaboration`/`issues`/`pulls*`/`contents`/`commits`/`branches`（非 `protection`），无 `agent-drafts` |
| D15 | R3：`ci_secrets` 按 ref 过滤「需要确认是否已有能力——**未验证**」 | **不成立（已可验证）** | 见 §3.2；`ci_secret` 实体只有 `repo_id`（`entities/ci_secret.rs:9`），注入点无 ref 条件 |
| D16 | R7：令牌不用于 SSH，SSH 推送不受 `ref_allowlist` 限制 | **成立** | `grep -rn 'ref_allowlist' crates/rg-ssh/src/` = **0** 命中 |
| D17 | §5.3 顺带记录：`notify_watchers_push` 定义但全仓无调用 | **成立** | `grep -rn 'notify_watchers_push' crates/` = 1 命中，即定义 `repo/service.rs:1590-1606` |

> 另核 8 条不与设计结论耦合的小项（未入表）：`ops.rs:106-124` 的 `delete_ref`（实 `:105-124`）、迁移文件 `m20261106_000004_create_repo_events.rs`（存在于 `crates/rg-db/src/migrations/`，注意目录是复数）、`job_timeout_secs` 在 `AppState`（`lib.rs:148`/`:185`）、`allow_host_execution` 在 `PostPushParams`（`lib.rs:3195`，R1 前是 `:3302`）。全部成立。

---

## 3. 两条不成立

### 3.1 `refs/pull/*` 不是只读命名空间

**它的说法**（三处）：「`refs/pull/*` 已有且是只读镜像」（§1 非目标）、「内核级只读镜像，任何推送写 → 客户端 push 直接失败」（§2 第 10 行）、「草稿区正常不该有 PR（`refs/pull/` 天然只读）」（§6.2 步 2）——并据此得出「不许开 PR 天然成立，不需要新代码」。

**现树事实**：

- `grep -rn 'refs/pull' crates/rg-git/src/` = **0 命中**。`rg-git` 里没有任何针对该命名空间的判定。
- `receive_pack` 的 ref 名不做命名空间白名单：拒绝只有两路 —— `rejected_refs`（`lib.rs:3006-3008`，两半来源：分支保护那一半恒 `refs/heads/`（`branch_protection/service.rs:256`），tag 保护那一半恒 `refs/tags/`（`lib.rs:3223`）；原写「模式恒为 `refs/heads/...`」只对前一半成立，两半都不命中 `refs/pull/`）与 `writable_refs`（`:2928-2930`，`None` = 不限制）。两者都不命中 `refs/pull/1/head`。
- 落地写入是 `receive_pack.rs:507` 的 `update_ref(...)`，其实现 `:631-647` 用 `PreviousValue::Any`（`:641`）——「无条件 set，等同 `git update-ref`」，注释自己写着。

⇒ **无 `ref_allowlist` 的令牌可以直接把 `refs/pull/<n>/head` 指向任意已存在于对象库的 commit**（推一个新 commit 也行，pack 已 index）。`refs/pull/*` 由 `publish_head_ref`（`pull_request/service.rs:1396`）写入，是**普通 ref**，不是伪 ref。

**带白名单的令牌**确实天然被拒 —— 但理由是「前缀恒为 `refs/heads/`」（P1），不是「`refs/pull` 只读」。设计把这两件事混成一条，于是「天然成立、不需要新代码」的推论在**无白名单令牌**这一侧是空的。

### 3.2 CI secrets 不按 ref 过滤（R3 的「未验证」有答案）

**它的说法**：R3 列缓解措施「草稿分支默认只许容器内执行、忽略 `allow_host_execution`、不注入 secrets」，并把 secrets 那半句标成「需要确认是否已有能力——**未验证**」。

**现树事实**：

- `crates/rg-db/src/entities/ci_secret.rs:9` 只有 `repo_id`，没有 branch/ref/tag 列。
- 注入点 `rg-ci/src/runner.rs:743-762`：`if self.repo_id > 0` → `ci_secret_ops::list_by_repo(&self.db, self.repo_id)` → 逐条 `decrypt` → `env.insert(secret.name, value)`。**没有任何 ref/分支条件**，也没有「仅默认分支」的默认收紧。**这是两处注入之一**：外部 runner 的 `GET /runners/{id}/jobs/poll` 把同一个 `list_by_repo` 的结果直接塞进 job 的 `variables`（`rg-http/src/api/runners.rs:352-356`，取数函数 `:787-793`），而 `external_runners=true` 时内置 runner 压根不启动（`rg-ci/src/lib.rs:308`）⇒ 只堵 `runner.rs` 无效（R18-7）。
- `allow_host_execution` 同样**无触发源判定**：`lib.rs:3376` 原样从 `state.allow_host_execution` 传入（`:3155` 取、`:232` 由**实例配置** `[ci].allow_host_execution` 装载；`repositories` 表没有这一列，见 R18-9）→ `rg-ci/src/lib.rs:402` `runner.set_allow_host_execution(...)`。原写「从仓库配置传入」是错的。`validate_execution_semantics`（`rg-ci/src/lib.rs:416`）只校验 `when`/`timeout`/`environment`/`condition`，不看执行面。

⇒ 「不注入 secrets」不是配置项、是**待实现的代码**。设计若按 S1–S6 顺序推进，S3（推送守卫）先落地而 secrets 过滤没做，草稿区就已经把全仓 secrets 交给推送方自带的脚本。

**同时这条也修正了 R3 的量级判断**：该风险**今天就存在**——任意 `repo:write` 令牌推到 `agent/x` 分支已经会触发 CI（`lib.rs:3364`）、拿全部 secrets、按实例开关决定是否宿主机执行。草稿区**不新增**这条通道，它新增的是「不再有人被通知」。这句话应写进 R3，否则会把存量缺陷记成新设计的副产品，导致「不做草稿区就安全」的错觉。

---

## 4. 行号漂移（设计文档写于 2026-11-11，本轮按 `e225293` 校正）

| 文件 | 文档写的 | 现树 | 差 |
|---|---|---|---|
| `rg-core/src/auth/pat_scope.rs` | `:211-262` / `:266-275` / `:299-330` | `:213-257` / `:268-273` / `:301-336` | +2 |
| `rg-http/src/lib.rs` | `:245-274` / `:2960-3070` / `:2984` / `:3070-3125` / `:3190` / `:3278-3575` / `:3406-3495` / `:3504` / `:3510-3552` / `:3566-3574` | `:246-275` / `:2996-3076` / `:2996` / `:3082-3140` / `:3202` / `:3290-3590` / `:3417-3495` / `:3516` / `:3526-3548` / `:3579-3586` | +1 … +15 |
| `rg-git/src/protocol/receive_pack.rs` | `:393-401` / `:412` / `:427-446` / `:486` / `:466-480` | `:393-401` / `:412` / `:427-453` / `:487` / `:466-480` | 0 … +1 |
| `rg-http/src/api/ci_retention.rs` | `:170-200` | `:168-188` | −2 |
| `rg-http/src/api/repo_events.rs` | `:214-230` | `:216-227` | +2 / −3 |
| `rg-core/src/webhook/service.rs` | `:131-142` / `:227-241` | `:131-141` / `:227-239` | 端点 −1/−2 |
| `rg-ci/src/gitea_actions.rs` | `:280-312` / `:698-730` | `:280-312` / `:698-732` | 0 / +2 |
| `rg-core/src/repo/service.rs` | `:1590-1605` | `:1590-1606` | +1 |
| `rg-git/src/ops.rs` | `:106-124` / `:132-153` | `:105-124` / `:132-153` | −1 / 0 |
| `rg-ci/src/lib.rs` | `:131` / `:558-596` | `:131` / `:558-596` | 0 |

`rg-http/src/lib.rs` 是唯一系统性偏 12 行的文件（`on_delete` 之前插过一段注释与 handler 改动），凡引用它的区间都不能按旧值复用。**结论：文档里的行号已就地订正（1 行 → 1 行，258 行不变；旧写 259 见文件头订正说明）。**

### 4.1 R1 之后（当前 `cnb/main`）的再漂移

`eb1736f` / `4d34e69`（R1 路由元数据注册表）重写了 `lib.rs` 的路由与鉴权装配段，上表的 `lib.rs` 列与设计文档里的 `lib.rs` 行号**同时作废**：`c17735e..45ed54c` 上该文件 `34+/87−`，`on_delete` 之前整体下移 **68** 行、`post_push_hooks` 尾部下移 **53** 行，**不是统一偏移**，不能加减法复用。已逐行打印核实的新锚点（BASE `45ed54c`）：`writable_refs` `:2928`、`rejected_refs` `:3006-3008`、`on_delete` 闭包 `:3014-3072`（内含 `:3057` 的 `trigger_branch_deleted`）、`spawn_post_push_hooks` 定义 `:3141`／唯一调用点 `:3095`、`post_push_hooks` `:3235-3537`、`status != "ok"` `:3268-3270`、`refs/tags/` 模式 `:3223`、`has_ci_config` `:3364`、CI 段 `:3364-3448`、`trigger_event(push)` `:3463`、分支事件 `:3469-3496`、ws 通知 `:3526-3534`、`TriggerPipelineParams` 收 `allow_host_execution` `:3376`。

`pat_scope.rs` 只改了一处但对本文两条结论有关：`ref_limited_write_allowed` 从 32 行 `matches!` 表（`:301-336`）缩成对 `route_meta::branch_limited_write_allowed` 的**一行转发**（`:307-309`）⇒ 「往白名单里加 `agent-drafts`」这个修法不再成立，改成「把路由声明进注册表」（见 R18-4）。其余被引文件（`receive_pack.rs` / `webhook/service.rs` / `ops.rs` / `repo/branch.rs` / `ci_retention.rs` / `repo_events.rs` / `gitea_actions.rs` / `rg-ci` / `rg-cli` / `api/runners.rs` / `entities/*`）自 `e225293` 起零改动，本表行号仍有效。

§2 表里含 `lib.rs` 锚点的行（P4 / P6 / D4 / D5 / D11）保留的是 `e225293` 口径的**历史判定**，现状按上一段换算，其中最容易误用的是三个：P4 的六个锚点现状为 `:3235` 定义 / `:3268-3270` 过滤 / `:3364-3448` CI / `:3463` push webhook / `:3469-3496` 分支事件 / `:3526-3534` ws；P6 与 D4 的 `on_delete` 现状为 `:3014-3072`（旧写的 `:3102` 调 `delete_branch` 现状在 `:3046`）。

---

## 5. 新发现：`git push --delete` 会发两次分支事件（真 `deleted` 或假 `created`）

不在 NPC 的两份材料里，本轮读 `on_delete` 时撞到：

- `lib.rs:3057` —— `on_delete` 守卫内部，`delete_branch` 成功后调 `trigger_branch_deleted`（推送删除走这里）。
- `lib.rs:3488` —— `post_push_hooks` 里 `new_sha` 全 0 的分支再次调 `trigger_branch_deleted`。

`spawn_post_push_hooks(..., ref_updates)`（`lib.rs:3095` 调用、`:3141` 定义）拿到的是**同一份** ref 更新（删除项 `status = "ok"`，`new_sha` = 40 个 0），`post_push_hooks` 的 `status != "ok"` 过滤放行 ⇒ **一次 `git push --delete` 投递两条 `branch.deleted`**。REST `DELETE /branches/{x}` 走的是第三个调用点（`repo_content.rs:723`），不受影响。

**这条要按两种删除形态分开写（R18-8，2026-11-13 补）**：`post_push_hooks` 的分支事件判断**先看 `old_sha` 再看 `new_sha`**（`:3469-3496`：`old_sha` 空/全 0 → `branch.created`，`else if new_sha` 空/全 0 → `branch.deleted`）。客户端报得出旧值时（`<old> 0000… <ref>`）才是上面的双 `deleted`；客户端不知道旧值时有两条路径都落进「旧值为空/全 0」——`~/.git` 无该 remote-tracking ref 的老 git 发 `<0…0> <0…0> <ref>`，以及 `:<ref>` 简写（`receive_pack.rs:349-366` 显式写 `old_sha: String::new()`）——此时 `on_delete` 仍会删成功并投**一条真的 `branch.deleted`**（`:3057`），而 `post_push_hooks` 再投**一条假的 `branch.created`**（`:3475`）。所以两种形态都是两条事件，差别只在第二条是真 `deleted` 还是假 `created`；原 §5 写的「一次删除投两条 `deleted`」只覆盖第一种。

这与草稿设计直接相关：§5.3 声称「通知抑制的三个注入点全在 `post_push_hooks` 一处」。**实际是四处**——删分支路径的 `trigger_branch_deleted` 在 `on_delete` 里，`post_push_hooks` 的过滤加不到它。按现文档实现，草稿分支被 agent 用 `git push --delete` 删掉时仍会投递一条 `branch.deleted` webhook。

---

## 6. 新立待拍板（L2，本流程不动代码）

- **R18-1（P1）`receive_pack` 无命名空间守卫**。`refs/pull/*`、`refs/notes/*`、任意 `refs/*` 都能被无 `ref_allowlist` 的令牌改写（`receive_pack.rs:507` → `:631-647` 的 `PreviousValue::Any`）。修法二选一：①在 `receive_pack` 只接受 `refs/heads/` + `refs/tags/`，其余 `ng`；②显式对 `refs/pull/` 返回「只读镜像」。这条同时是草稿设计 §6.2 的前提，**裁决前不要以「天然只读」写实现**。
- **R18-2（P1）CI secrets 无 ref 过滤**（两处注入点，见 R18-7）。草稿设计以「不注入 secrets」为安全前提，现状做不到。建议按「默认分支 / 受保护分支 / 其它」三档做 `ci_secret` 的可见范围列，或至少在非默认分支上默认不注入并在文档写明。**在它之前，草稿区不该开 CI**。
- **R18-3（P2）`allow_host_execution` 无触发源判定**（`lib.rs:3376` 传入 → `rg-ci/src/lib.rs:402`；值来自 `state.allow_host_execution`，`:3155` 取、`:232` 从实例配置装载 —— **不是仓库配置**，`repositories` 表无此列）。裁决点：草稿分支是否强制 `allow_host_execution=false`，以及是否顺手对「非默认分支 + 非保护分支」一并收紧。落点只能是 per-pipeline 参数（`TriggerPipelineParams`）。
- **R18-4（P2）`ref_limited_write_allowed` 缺 `agent-drafts`**（`pat_scope.rs:307-309`）。**R1 后机制变了**：该函数已缩成对 `route_meta::branch_limited_write_allowed` 的一行转发，白名单不再是一张 `matches!` 表 ⇒ 修法从「往表里加一段」变成「把 `agent-drafts` 的路由声明进注册表（未声明的写路由默认拒）」。与 T21 的三张表耦合，增补时须协调（参考 `["commits"]` 的处置：准入条件由 handler 自查分支，不因 T21 移除）。
- **R18-5（P2）`branch.deleted` 双发**（§5）。修法：`post_push_hooks` 跳过删除项的分支事件，或 `on_delete` 不再发、统一由 `post_push_hooks` 发（后者要求删除项也进 `ref_updates`，现状已进）。**修的时候要连 R18-8 的「假 `branch.created`」一起修**：只去掉 `on_delete` 那一发，旧值缺失的删除仍会投一条 `created`。属 webhook 语义变更，需拍板。
- **R18-6（P3）`spawn_post_push_hooks` 是 `pub(crate)`**（`lib.rs:3141`）。若清理任务放在 `rg-core`（设计 §6.1 的 `rg_core::task_supervisor::supervise` + `api::agent_drafts::run_cleanup_loop` 属 `rg-http`，这条不触发；但 S5 若下沉到 core 就会撞同一堵墙，同 R17-7）。
- **R18-7（P1，补 R18-2）secrets 有两个注入点，且 `external_runners=true` 时内置 runner 那条根本不跑**。①`rg-ci/src/runner.rs:747` 的 `ci_secret_ops::list_by_repo(&self.db, self.repo_id)`（内置 runner）；②`rg-http/src/api/runners.rs:352-356` 在 `poll_job`（`:275`）里把 `decrypted_repo_secrets`（`:787-793`，同一个 `list_by_repo`）直接塞进该 job 的 `variables`；③日志遮蔽在 `:537`（`mask_values`）。而 `rg-ci/src/lib.rs:308` 的 `if !external_runners { spawn_internal_runner(...) }` ⇒ 开了外部 runner 时只改 `runner.rs` 等于没堵，且 secrets 被送到**本进程之外**的机器上。裁决口与 R18-2 同一个（secrets 可见范围），合并登记。
- **R18-8（P2，修正 §5）删除事件的第二种形态是「假 `branch.created`」**（详见 §5 末段）。两种形态都是两条事件，但第二条的身份不同：客户端报得出旧值 → 两条 `branch.deleted`；旧值为空/全 0（`:<ref>` 简写或老 git 不知道旧值）→ 真 `deleted` + 假 `created`。裁决口与 R18-5 同一个。
- **R18-9（P3，措辞级但会误导裁决）`allow_host_execution` 与 `job_timeout_secs` 都不是「仓库级」**。前者 `rg-cli/src/main.rs:2280-2284` 从 `[ci].allow_host_execution` 解析、经 `AppState`（`lib.rs:137`/`:232`）到 `:3155`；后者 `main.rs:2349` 从 `[timeouts].job_secs` 解析、`AppState` `lib.rs:148`/`:240`。`grep -rn 'allow_host_execution\|job_timeout_secs' crates/rg-db/src/` = **0** 命中、`entities/repository.rs` 的 `host|docker` = 0 ⇒ 仓库配置这一层不存在。设计 `:232` 与本文件 `:85` 的「仓库开关 / 仓库级 / 仓库配置」措辞已按此订正，裁决点因此从「仓库开关」变为「per-pipeline 参数」。
- **R18-10（P3，文档账）设计文档的非 `lib.rs` 锚点已随本轮重钉，但 `§2 :36` 之外的历史锚点说明与「基线 `e225293`」这层关系只在本文件 §4.1 写清**：设计文档头部仍只写「2026-11-12 已在 `e225293` 上逐条复核」，读者拿旧 SHA 对不上当前树时容易误判为错号。下次动设计文档时把它改成「行号已按 `45ed54c` 重钉，历史复核仍在 `e225293`」。

---

## 7. 验证口径

- 基线 `e225293`（`git rev-parse --short HEAD`；`HEAD...cnb/main` = `0 0`，零漂移）。
- `scripts/check.sh --quick` 改前 `EXIT=0` / `106 s`（`==> checks passed in 106s (quick)`），宿主 `Darwin arm64`、`/Users/yuqu/.cargo/bin/cargo`。
- 本轮**零代码行为改动**：本文 + 设计文档行号/结论订正 + `ironforge-docs/README.md` 索引 + 台账。`crates/` 下无一字节变化。
- 每条 `path:line` 均以 `sed -n '<n>p'` 打印后人工确认「那一行能读出你要它证明的东西」，不只确认行存在。
