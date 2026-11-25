# T33 授权判断点全量测绘（Unit 级权限前置勘察）

> 任务卡：`tasks/T33.md`。分支 `agent/t33-unit-permission`，worktree `../ironforge-t33`。
> 本文件只做测绘，不改任何权限语义。所有行号对应的代码都是用 `sed -n 'Np' <file>` 现场打印后粘入的。
> 配套设计见 `docs/plans/T33-adr-unit-permission.md`。

## 0. 取证环境与命令

```sh
$ cd /Users/yuqu/vibeCodeing/ironforge-t33
$ git rev-parse --short HEAD
64861cf9
$ grep -rn "can_write_repo\|can_read_repo\|can_admin_repo\|fn can_" crates/*/src/ | wc -l
      95
```

本文第 2 节的 95 行，与上面这条命令的输出**一一对应、顺序一致**（表内 `#` 列即该命令输出的第 N 行）。
每行的最后一列是 `sed -n 'Np' <file>` 的原样输出；如果代码行内有反引号，已替换成 HTML 实体 `&#96;` 以便在 Markdown 表格中显示，字符本身未改动。

### 0.1 95 个命中不是 95 个判断点

grep 命中包含了**导入、文档注释、函数定义、内部递归委派、日志字符串**。按类型拆分如下（可复核）：

| 类型 | 数量 | 说明 |
|---|---:|---|
| `调用`：业务代码里真正发起的授权判断 | **60** | 本任务的真实工作量 |
| `内部委派`：四个原语互相递归/转发 | 4 | `can_read` → `can_read_repo`、`can_write` → `can_write_repo`、`apply_owner_cap` 递归 |
| `定义`：`pub async fn can_*` | 8 | 4 个仓库原语 + 2 个组织原语 + 2 个按名解析包装 |
| `导入` | 2 | `use` 语句 |
| `注释与文档` | 19 | 说明文字，不是判断 |
| `日志` | 2 | `tracing::error!` 里的字符串 |

**因此：95 是 grep 命中数，60 是业务代码里的直接调用点数，差 35（−36.8%），超过任务卡允许的 10%。**
差异原因就是上表：任务卡的 grep 口径把注释、定义、`use` 一并算了进来。本文件按任务卡要求仍逐条列出 95 行，
并在「类型」列标出每一行到底是什么；做 unit 改造时**只需要处理 60 个调用点 + 4 个原语签名**。

### 0.2 还有 182 个间接调用点不在 95 里

60 个直接调用并不是全部暴露面。`crates/rg-http/src/api/repo_access.rs` 提供了四个通用守卫，handler 通过它们间接调用原语：

```sh
$ for h in require_write require_admin require_read_with_ci_scope require_authenticated_read require_read; do
    echo -n "$h: "; grep -rn "\b$h(" crates/rg-http/src/ | grep -v "pub(crate) async fn" | wc -l
  done
require_write:       43
require_admin:       54
require_read_with_ci_scope:        3
require_authenticated_read:       14
require_read:       69
```

合计 **182 次 handler 侧守卫调用，落在 34 个文件里**（`repo_access.rs` 内部另有 1 次 `require_read` → `require_read_with_ci_scope` 的转发）。改造这四个守卫的签名/语义，
影响面比 60 个直接调用更大。**这是 ADR §9 把守卫改造列为串行第一步的原因。**

## 1. 四个原语当前到底怎么判

| 原语 | 定义位置 | 判定顺序（实际代码） |
|---|---|---|
| `can_read_repo` | `crates/rg-core/src/repo/service.rs:199` | 公开仓库直接 `true` → 30s 缓存 → owner → `repo_collaborators` 有任意记录 → 属于仓库所属组织 → 最后套 `apply_owner_cap`（agent 的读受 owner 读能力封顶） |
| `can_write_repo` | `crates/rg-core/src/repo/service.rs:296` | 30s 缓存 → owner → collaborator 权限为 `write`/`admin` → 组织 `owner`/`admin` → 组织写团队映射（`is_member_of_write_team_for_repo`）→ 套 `apply_owner_cap` |
| `can_admin_repo` | `crates/rg-core/src/repo/service.rs:351` | agent 一律 `false` → owner → collaborator 权限为 `admin` → 组织 `owner`/`admin` → 组织管理团队（`is_member_of_admin_team`） |
| `is_collab_member` | `crates/rg-core/src/collaboration/access.rs:56` | owner / 任意 collaborator / 组织成员，最后再回落到 `can_write_repo` |

关键语义差异（与 Gitea 的 unit 模型对照）：

1. **读是组织级，写才受团队映射限制。** `repo/service.rs:195-198` 注释明说：组织成员读所有私有仓库；
   team `permission='read'` 对读无效，团队→仓库映射只限制写。Gitea 里 `read` 是每个 unit 独立的，
   `code:read` 与 `issues:read` 可以分开；IronForge 当前**做不到**。
2. **collaborator 只有一列 `permission`（read/write/admin）**，没有 unit 维度。
3. **没有 action 级枚举可复用。** `crates/rg-core/src/authorization/action.rs` 的 `Action` 是
   「审批工单」的 5 个动作（`ci_environment_release` / `protected_branch_merge` / `protected_branch_push` /
   `release_publish` / `scope_expand`），**不是** 仓库读写的 action 集合。不要把它误当成 unit action 枚举。
4. **agent 有三条额外限制**：`can_admin_repo` 对 agent 直接 `false`；`apply_owner_cap` 把 agent 的能力
   封顶到 owner 当前能力；`scope_expand` 只能扩 `repo` 家族 scope。

## 2. 95 行全量表

| # | 位置 | 类型 | 判断的 action | 语义 Unit | 需 action 级细化 | 说明 | 实证代码（`sed -n 'Np'` 原样） |
|---|---|---|---|---|---|---|---|
| 1 | `crates/rg-core/src/collaboration/human_notify.rs:40` | 导入 | — | — | 否 | 引入四个原语 | `use crate::repo::service::{can_read_repo, can_write_repo, invalidate_perm_cache_user};` |
| 2 | `crates/rg-core/src/collaboration/human_notify.rs:100` | 调用 | read | 跨 unit（按被通知对象） | 是 | 通知收件人过滤：读不到仓库就不发，否则泄露标题 | `if can_read_repo(db, repo, Some(person)).await? {` |
| 3 | `crates/rg-core/src/collaboration/human_notify.rs:135` | 调用 | write | 跨 unit（按被通知对象） | 是 | 写者收件人过滤 | `if can_write_repo(db, repo, Some(person)).await? {` |
| 4 | `crates/rg-core/src/collaboration/human_notify.rs:250` | 调用 | read | collaboration | 是 | 协作任务通知前置校验 | `match can_read_repo(db, repo, Some(user_id)).await {` |
| 5 | `crates/rg-core/src/collaboration/access.rs:17` | 注释与文档 | — | — | 否 | 解释 perm cache 代价 | `/// effect on the very next call — at the price of a global perm-cache write lock per call (&#96;require_collab_read&#96; pays it twice per request: its own clear, then this one; &#96;require_collab_write&#96; pays it once, then calls &#96;can_write_repo&#96; directly).` |
| 6 | `crates/rg-core/src/collaboration/access.rs:26` | 注释与文档 | — | — | 否 | 解释 owner cap 不可绕过 | `// would otherwise bypass the owner cap that &#96;can_read_repo&#96;/&#96;can_write_repo&#96;` |
| 7 | `crates/rg-core/src/collaboration/access.rs:56` | 调用 | write | collaboration | 是 | `is_collab_member` 的写者判定（collab 写复用仓库写） | `crate::repo::service::can_write_repo(db, repo, Some(user_id))` |
| 8 | `crates/rg-core/src/auth/oci_token.rs:74` | 注释与文档 | — | — | 否 | 提及原语名 | `/// &#96;can_read_repo&#96;/&#96;can_write_repo&#96;. Tokens issued before this field` |
| 9 | `crates/rg-core/src/auth/oci_token.rs:444` | 注释与文档 | — | — | 否 | 测试注释 | `/// repository visibility and &#96;can_read_repo&#96; cannot take a username.` |
| 10 | `crates/rg-core/src/org/mod.rs:245` | 注释与文档 | — | — | 否 | 组织级对应物说明 | `/// This is the org-scoped counterpart of &#96;repo::service::can_admin_repo&#96;: the` |
| 11 | `crates/rg-core/src/org/mod.rs:250` | 定义 | admin | 非 unit（组织） | 否 | `can_admin_org` 定义，独立于仓库 unit | `pub async fn can_admin_org(` |
| 12 | `crates/rg-core/src/org/mod.rs:256` | 注释与文档 | — | — | 否 | 与 can_admin_repo 同规则 | `// (the same rule as &#96;can_admin_repo&#96;): membership rows are written by org` |
| 13 | `crates/rg-core/src/org/mod.rs:281` | 定义 | read | 非 unit（组织） | 否 | `can_read_org` 定义 | `pub async fn can_read_org(` |
| 14 | `crates/rg-core/src/search/service.rs:220` | 注释与文档 | — | — | 否 | 搜索可见性镜像 can_read_repo | `/// This mirrors &#96;repo::service::can_read_repo&#96;: public repositories are visible` |
| 15 | `crates/rg-core/src/package_registry/discovery.rs:8` | 注释与文档 | — | — | 否 | 模块文档：发现接口沿用仓库读 | `//! caller may read ([&#96;crate::repo::service::can_read_repo&#96;], the same decision` |
| 16 | `crates/rg-core/src/package_registry/discovery.rs:28` | 注释与文档 | — | — | 否 | 模块文档：30s 缓存 | `//! Read access is evaluated per request, but &#96;can_read_repo&#96; keeps a 30-second` |
| 17 | `crates/rg-core/src/package_registry/discovery.rs:200` | 调用 | read | packages | 是 | 包发现按仓库读过滤 | `if !crate::repo::service::can_read_repo(db, repo, query.actor_id).await? {` |
| 18 | `crates/rg-core/src/review/codeowners.rs:150` | 调用 | read | code | 否 | CODEOWNERS 请求前要求能读仓库（仓库级） | `if !crate::repo::service::can_read_repo(db, repository, Some(user.id))` |
| 19 | `crates/rg-core/src/authorization/evaluator.rs:37` | 导入 | — | — | 否 | 引入两个原语 | `use crate::repo::service::{can_admin_repo, can_write_repo};` |
| 20 | `crates/rg-core/src/authorization/evaluator.rs:69` | 调用 | admin | 跨 unit（审批规则） | 是 | 审批人判定：admins 谓词 | `if self.admins && can_admin_repo(db, repo, Some(user_id)).await? {` |
| 21 | `crates/rg-core/src/authorization/evaluator.rs:72` | 调用 | write | 跨 unit（审批规则） | 是 | 审批人判定：writers 谓词 | `if self.writers && can_write_repo(db, repo, Some(user_id)).await? {` |
| 22 | `crates/rg-core/src/authorization/mod.rs:129` | 调用 | read | 跨 unit（所有 unit） | 是 | 发起授权请求要求能读仓库 | `&& !crate::repo::service::can_read_repo(db, new.repo, Some(new.requested_by_id)).await?` |
| 23 | `crates/rg-core/src/repo/service.rs:197` | 注释与文档 | — | — | 否 | 读模型说明：读是组织级 | `/// [&#96;can_write_repo&#96;]). Narrowing reads to teams would be a separate,` |
| 24 | `crates/rg-core/src/repo/service.rs:199` | 定义 | read | 仓库基元 | 否 | `can_read_repo` 定义（公共读 → owner → collaborator → org member） | `pub async fn can_read_repo(` |
| 25 | `crates/rg-core/src/repo/service.rs:265` | 内部委派 | write | 仓库基元 | 否 | owner cap：agent 的写受 owner 写能力封顶 | `Box::pin(can_write_repo(db, repo, Some(owner_id))).await?` |
| 26 | `crates/rg-core/src/repo/service.rs:267` | 内部委派 | read | 仓库基元 | 否 | owner cap：agent 的读受 owner 读能力封顶 | `Box::pin(can_read_repo(db, repo, Some(owner_id))).await?` |
| 27 | `crates/rg-core/src/repo/service.rs:276` | 定义 | read | 仓库基元 | 否 | 按 owner/name 解析后委派 | `pub async fn can_read(` |
| 28 | `crates/rg-core/src/repo/service.rs:287` | 内部委派 | read | 仓库基元 | 否 | `can_read` 委派到 `can_read_repo` | `can_read_repo(db, &repo, actor_id).await` |
| 29 | `crates/rg-core/src/repo/service.rs:296` | 定义 | write | 仓库基元 | 否 | `can_write_repo` 定义（owner → collaborator write/admin → org owner/admin → write team） | `pub async fn can_write_repo(` |
| 30 | `crates/rg-core/src/repo/service.rs:302` | 注释与文档 | — | — | 否 | 缓存键空间说明 | `// We use the same cache key space as can_read_repo to reuse results.` |
| 31 | `crates/rg-core/src/repo/service.rs:351` | 定义 | admin | 仓库基元 | 否 | `can_admin_repo` 定义（agent 直接 false → owner → collaborator admin → org owner/admin → admin team） | `pub async fn can_admin_repo(` |
| 32 | `crates/rg-core/src/repo/service.rs:388` | 定义 | write | 仓库基元 | 否 | 按 owner/name 解析后委派 | `pub async fn can_write(` |
| 33 | `crates/rg-core/src/repo/service.rs:399` | 内部委派 | write | 仓库基元 | 否 | `can_write` 委派到 `can_write_repo` | `can_write_repo(db, &repo, actor_id).await` |
| 34 | `crates/rg-core/src/repo/service.rs:1287` | 调用 | read | code | 否 | fork 私有源仓库要求可读 | `if source_repo.is_private && !can_read_repo(db, &source_repo, Some(user_id)).await? {` |
| 35 | `crates/rg-db/src/migrations/m20261015_000003_create_team_repositories.rs:5` | 注释与文档 | — | — | 否 | 迁移注释 | `//! scope a team to a subset. &#96;repo::service::can_write_repo&#96; looked only at` |
| 36 | `crates/rg-db/src/ops/org_ops.rs:417` | 注释与文档 | — | — | 否 | 写团队映射注释 | `// &#96;repo::service::can_read_repo&#96; grants read access to *any* member of the` |
| 37 | `crates/rg-db/src/ops/org_ops.rs:421` | 注释与文档 | — | — | 否 | 写团队映射注释 | `// enforced boundary. See &#96;can_read_repo&#96; for the current rule.` |
| 38 | `crates/rg-http/src/oci.rs:82` | 注释与文档 | — | — | 否 | OCI 认证说明 | `/// Normal IronForge JWTs are checked against repo &#96;can_read_repo&#96;/&#96;can_write_repo&#96;.` |
| 39 | `crates/rg-http/src/oci.rs:114` | 调用 | read | packages | 是 | OCI `check_access` 读判定 | `rg_core::repo::service::can_read_repo(&state.db, &repo_model, Some(uid))` |
| 40 | `crates/rg-http/src/oci.rs:118` | 调用 | write | packages | 是 | OCI `check_access` 写判定 | `rg_core::repo::service::can_write_repo(&state.db, &repo_model, Some(uid))` |
| 41 | `crates/rg-http/src/oci.rs:142` | 注释与文档 | — | — | 否 | 旧 token 无 uid 的说明 | `// which &#96;can_read_repo&#96; cannot use. Tokens minted before &#96;uid&#96; existed` |
| 42 | `crates/rg-http/src/oci.rs:146` | 注释与文档 | — | — | 否 | 禁止匿名降级的说明 | `// Do NOT "fix" this by calling &#96;can_read_repo(.., None)&#96; unconditionally:` |
| 43 | `crates/rg-http/src/oci.rs:159` | 调用 | read | packages | 是 | OCI token（带 uid）读判定 | `rg_core::repo::service::can_read_repo(&state.db, &repo_model, claims.uid)` |
| 44 | `crates/rg-http/src/oci.rs:164` | 调用 | write | packages | 是 | OCI token 写判定 | `rg_core::repo::service::can_write_repo(&state.db, &repo_model, Some(uid))` |
| 45 | `crates/rg-http/src/oci.rs:179` | 调用 | read | packages | 是 | 匿名公开仓库读判定 | `let allowed = rg_core::repo::service::can_read_repo(&state.db, &repo_model, None).await?;` |
| 46 | `crates/rg-http/src/oci.rs:333` | 调用 | read | packages | 是 | `get_token` 计算 can_pull | `let can_pull = rg_core::repo::service::can_read_repo(` |
| 47 | `crates/rg-http/src/oci.rs:346` | 调用 | write | packages | 是 | `get_token` 计算 can_push | `let can_push = rg_core::repo::service::can_write_repo(` |
| 48 | `crates/rg-http/src/oci.rs:382` | 注释与文档 | — | — | 否 | 说明 username 不参与 | `// later; &#96;username&#96; alone is not something &#96;can_read_repo&#96; accepts.` |
| 49 | `crates/rg-http/src/api/collab_capabilities.rs:222` | 调用 | write（只读探测） | collaboration | 是 | capabilities 只读回答 caller 是否可写，不产生写 | `let repo_write = rg_core::repo::service::can_write_repo(&state.db, &repo, Some(actor_id))` |
| 50 | `crates/rg-http/src/api/lfs.rs:101` | 调用 | write | code | 是 | LFS batch 上传授权 | `match rg_core::repo::service::can_write_repo(&state.db, &repo_model, Some(user_id)).await {` |
| 51 | `crates/rg-http/src/api/lfs.rs:111` | 调用 | read | code | 是 | LFS batch 下载授权 | `match rg_core::repo::service::can_read_repo(&state.db, &repo_model, Some(user_id)).await {` |
| 52 | `crates/rg-http/src/api/lfs.rs:203` | 调用 | write | code | 是 | LFS 对象上传 | `match rg_core::repo::service::can_write_repo(&state.db, &repo_model, Some(user_id)).await {` |
| 53 | `crates/rg-http/src/api/lfs.rs:293` | 调用 | read | code | 是 | LFS 对象下载 | `match rg_core::repo::service::can_read_repo(&state.db, &repo_model, Some(user_id)).await {` |
| 54 | `crates/rg-http/src/api/repo_content.rs:247` | 调用 | read | code | 否 | 文件/目录读取守卫 | `match rg_core::repo::service::can_read_repo(&state.db, &repo_model, actor_id).await {` |
| 55 | `crates/rg-http/src/api/repo_content.rs:1321` | 调用 | write | code | 否 | 内容写守卫 | `if !rg_core::repo::service::can_write_repo(&state.db, &repo_model, Some(user_id)).await? {` |
| 56 | `crates/rg-http/src/api/collab_templates.rs:155` | 调用 | admin | 仓库设置（模板） | 否 | 模板管理要求仓库管理员 | `match rg_core::repo::service::can_admin_repo(&state.db, &repo, Some(actor_id)).await {` |
| 57 | `crates/rg-http/src/api/push_mirrors.rs:86` | 调用 | write | 仓库设置（push mirror） | 是 | 配置外向推送凭据，实为管理员动作 | `match rg_core::repo::service::can_write_repo(&state.db, &repo, Some(user_id)).await {` |
| 58 | `crates/rg-http/src/api/issues.rs:516` | 调用 | write | issues | 是 | `create_issue` 的写判定 | `rg_core::repo::service::can_write_repo(&state.db, &repo_model, Some(user_id))` |
| 59 | `crates/rg-http/src/api/issues.rs:621` | 调用 | write | issues | 是 | `update_issue` 的写判定 | `match rg_core::repo::service::can_write_repo(&state.db, &repo_model, Some(user_id)).await {` |
| 60 | `crates/rg-http/src/api/issues.rs:624` | 日志 | — | — | 否 | 错误日志字符串 | `tracing::error!(error = %e, "can_write_repo check failed");` |
| 61 | `crates/rg-http/src/api/issues.rs:629` | 调用 | read | issues | 是 | `update_issue` 的读判定（非写者只能改自己的？见正文） | `match rg_core::repo::service::can_read_repo(&state.db, &repo_model, Some(user_id)).await {` |
| 62 | `crates/rg-http/src/api/issues.rs:632` | 日志 | — | — | 否 | 错误日志字符串 | `tracing::error!(error = %e, "can_read_repo check failed");` |
| 63 | `crates/rg-http/src/api/issues.rs:1265` | 调用 | read | issues | 是 | issue 读守卫 | `if !rg_core::repo::service::can_read_repo(&state.db, &repo, Some(user_id))` |
| 64 | `crates/rg-http/src/api/ci_environments.rs:87` | 注释与文档 | — | — | 否 | 管理员捷径说明 | `// though &#96;can_admin_repo&#96; keeps agents away from the admin shortcut.` |
| 65 | `crates/rg-http/src/api/ci_environments.rs:278` | 调用 | admin | actions | 是 | 受保护环境审批要求管理员 | `match rg_core::repo::service::can_admin_repo(&state.db, &repo, Some(actor_id)).await {` |
| 66 | `crates/rg-http/src/api/reviews.rs:121` | 调用 | write | pulls | 是 | PR 管理者守卫 | `let can_write = rg_core::repo::service::can_write_repo(&state.db, &repo_model, Some(actor_id))` |
| 67 | `crates/rg-http/src/api/reviews.rs:161` | 调用 | write | pulls | 是 | suggestion 源仓库写判定（复合：跨仓） | `let can_write = rg_core::repo::service::can_write_repo(&state.db, &source_repo, Some(actor_id))` |
| 68 | `crates/rg-http/src/api/reviews.rs:358` | 调用 | write | pulls | 是 | 提交 review 要求写 | `rg_core::repo::service::can_write_repo(&state.db, &repo_model, Some(user_id))` |
| 69 | `crates/rg-http/src/api/reviews.rs:1118` | 调用 | read | pulls | 是 | 指派 reviewer 要求被指派人可读 | `rg_core::repo::service::can_read_repo(&state.db, &repo_model, Some(reviewer.id))` |
| 70 | `crates/rg-http/src/api/reviews.rs:1266` | 调用 | write | pulls | 是 | 解决 discussion 要求写 | `let can_write = rg_core::repo::service::can_write_repo(&state.db, &repo_model, Some(actor_id))` |
| 71 | `crates/rg-http/src/api/collab_overview.rs:43` | 调用 | write | collaboration | 是 | 总览里区分「我可写」区块 | `let can_write = rg_core::repo::service::can_write_repo(&state.db, &repo, Some(user_id)).await?;` |
| 72 | `crates/rg-http/src/api/repo_access.rs:45` | 调用 | read | 通用守卫（所有 unit） | 是 | `require_read_with_ci_scope` 主体：CI job token 可按 domain scope 放行匿名读 | `match rg_core::repo::service::can_read_repo(&state.db, &repo, actor_id).await {` |
| 73 | `crates/rg-http/src/api/repo_access.rs:78` | 调用 | read | 通用守卫（所有 unit） | 是 | `require_authenticated_read` 主体 | `match rg_core::repo::service::can_read_repo(&state.db, &repo, Some(actor_id)).await {` |
| 74 | `crates/rg-http/src/api/repo_access.rs:96` | 调用 | write | 通用守卫（所有 unit） | 是 | `require_write` 主体，43 个 handler 共用 | `match rg_core::repo::service::can_write_repo(&state.db, &repo, Some(actor_id)).await {` |
| 75 | `crates/rg-http/src/api/repo_access.rs:113` | 调用 | admin | 通用守卫（所有 unit） | 是 | `require_admin` 主体，54 个 handler 共用 | `match rg_core::repo::service::can_admin_repo(&state.db, &repo, Some(actor_id)).await {` |
| 76 | `crates/rg-http/src/api/artifacts.rs:223` | 调用 | write | actions | 是 | CI artifact 写 | `match rg_core::repo::service::can_write_repo(&state.db, &repo, Some(actor_id)).await {` |
| 77 | `crates/rg-http/src/api/artifacts.rs:550` | 调用 | read | actions | 是 | CI artifact 读 | `match rg_core::repo::service::can_read_repo(&state.db, repo, actor_id).await {` |
| 78 | `crates/rg-http/src/api/mirrors.rs:95` | 调用 | write | 仓库设置（mirror） | 是 | 创建镜像需写，实为管理员动作 | `match rg_core::repo::service::can_write_repo(&state.db, &repo, Some(user_id)).await {` |
| 79 | `crates/rg-http/src/api/mirrors.rs:189` | 调用 | write | 仓库设置（mirror） | 是 | 更新镜像 | `match rg_core::repo::service::can_write_repo(&state.db, &repo, Some(user_id)).await {` |
| 80 | `crates/rg-http/src/api/mirrors.rs:249` | 调用 | write | 仓库设置（mirror） | 是 | 删除镜像 | `match rg_core::repo::service::can_write_repo(&state.db, &repo, Some(user_id)).await {` |
| 81 | `crates/rg-http/src/api/mirrors.rs:299` | 调用 | write | 仓库设置（mirror） | 是 | 手动触发同步 | `match rg_core::repo::service::can_write_repo(&state.db, &repo, Some(user_id)).await {` |
| 82 | `crates/rg-http/src/api/notifications.rs:51` | 调用 | read | 跨 unit（通知列表） | 是 | 过滤可读仓库的通知 | `rg_core::repo::service::can_read_repo(&state.db, &repo, Some(user_id)).await,` |
| 83 | `crates/rg-http/src/api/archive.rs:62` | 调用 | read | code | 否 | 下载源码归档 | `match rg_core::repo::service::can_read_repo(&state.db, &repo, actor_id).await {` |
| 84 | `crates/rg-http/src/api/collab_groups.rs:186` | 调用 | write | collaboration | 是 | 加链接要求 anchor+target 双写（复合） | `match rg_core::repo::service::can_write_repo(` |
| 85 | `crates/rg-http/src/api/collab_groups.rs:369` | 调用 | write | collaboration | 是 | 组详情里回答「我可写」 | `let can_write = rg_core::repo::service::can_write_repo(&state.db, &repo, Some(user_id))` |
| 86 | `crates/rg-http/src/api/releases.rs:649` | 调用 | read | releases | 是 | 下载 release asset | `match rg_core::repo::service::can_read_repo(&state.db, &repo, actor_id).await {` |
| 87 | `crates/rg-http/src/api/webhooks_external.rs:93` | 调用 | write | actions / code（提交状态） | 是 | 外部 CI 写 commit status，跨 actions 与 code | `let can_write = rg_core::repo::service::can_write_repo(&state.db, &repo, Some(user_id))` |
| 88 | `crates/rg-http/src/api/attachments.rs:377` | 调用 | write | issues + pulls（复合） | 是 | 上传附件：作者本人或仓库写者（跨 issue/PR） | `let can_write = rg_core::repo::service::can_write_repo(&state.db, &repo, Some(user_id))` |
| 89 | `crates/rg-http/src/api/attachments.rs:544` | 调用 | write | issues + pulls（复合） | 是 | 删除附件 | `let can_write = rg_core::repo::service::can_write_repo(&state.db, &repo, Some(user_id))` |
| 90 | `crates/rg-http/src/api/attachments.rs:577` | 调用 | read | issues + pulls（复合） | 是 | 下载附件 | `if !rg_core::repo::service::can_read_repo(&state.db, &repo, user_id)` |
| 91 | `crates/rg-http/src/api/pulls.rs:481` | 调用 | write | pulls | 是 | `update_pr` 写判定 | `let can_write = rg_core::repo::service::can_write_repo(&state.db, &repo_model, Some(actor_id))` |
| 92 | `crates/rg-http/src/api/collab.rs:248` | 定义 | write | collaboration | 是 | `require_collab_write` 守卫实现 | `match rg_core::repo::service::can_write_repo(&state.db, &repo, Some(actor_id)).await {` |
| 93 | `crates/rg-http/src/ws.rs:408` | 调用 | read | actions | 是 | WebSocket 订阅 job 日志要求可读仓库 | `match rg_core::repo::service::can_read_repo(&state.db, &repository, Some(user_id)).await {` |
| 94 | `crates/rg-ssh/src/lib.rs:864` | 调用 | read | code | 否 | SSH `git-upload-pack` 等读服务授权 | `rg_core::repo::service::can_read_repo(db, &repo, Some(*actor_id)).await?` |
| 95 | `crates/rg-ssh/src/lib.rs:867` | 调用 | write | code | 否 | SSH `git-receive-pack` 写服务授权 | `rg_core::repo::service::can_write_repo(db, &repo, Some(*actor_id)).await?` |

## 3. 跨 unit 的复合判断（改造时必须拆开）

下表是第 2 节里「一个判断同时覆盖多个 unit」的位置。当前一次 `can_write_repo` 同时授予了这些 unit 的写权限；
unit 化之后必须拆成多次判断，且任一 unit 不通过时**不能**退化成「整仓拒绝」（要按 handler 的实际语义决定）。

| 位置 | 复合内容 | 今天的行为 | unit 化后的正确行为 |
|---|---|---|---|
| `crates/rg-http/src/api/attachments.rs:377` / `:544` / `:577` | 附件同时服务 issue 与 PR | 作者本人 or 仓库写者 | 需先解析 `AttachmentTarget`，按目标是 issue 还是 PR 判 `issues:write` / `pulls:write`；读同理 |
| `crates/rg-http/src/api/reviews.rs:161` | suggestion 源仓库写 | 对源仓库 `can_write_repo` | 这是**跨仓库**判断，unit 化后是 `pulls:write` on source repo；与目标 PR 的 `pulls:write` 是两个独立判断 |
| `crates/rg-http/src/api/collab_groups.rs:186` | 加链接要求 anchor 与 target 双写 | 两次 `can_write_repo` | 两次 `collaboration:write`（或按设计改为 `code:read` 目标可见 + anchor 写） |
| `crates/rg-http/src/api/webhooks_external.rs:93` | 外部 CI 写 commit status | 仓库写 | 语义上属于 `actions:write` + `code:write`（status 挂在 commit 上）；必须二选一裁决，不能两个都要求 |
| `crates/rg-core/src/collaboration/human_notify.rs:100` / `:135` | 通知收件人 | 仓库读 / 仓库写 | 按通知对象所属 unit 判定（issue 通知查 `issues:read`，PR 通知查 `pulls:read`），否则会给只有 issues 权限的人发 code 通知 |
| `crates/rg-core/src/authorization/evaluator.rs:69` / `:72` | 审批人：admins / writers | 仓库管理员 / 仓库写者 | `protected_branch_merge` 应落到 `pulls:write`；`release_publish` 应落到 `releases:admin`；`ci_environment_release` 应落到 `actions:admin`。三种 action 今天共用同一对谓词 |
| `crates/rg-core/src/authorization/mod.rs:129` | 发起授权请求要求仓库读 | 仓库读 | 应按 action 查对应 unit 的读权限 |
| `crates/rg-http/src/api/notifications.rs:51` | 通知列表过滤 | 仓库读 | 需要按通知的 `subject_type` 映射 unit，否则会漏通知或泄露标题 |

## 4. 按 unit 归并的 60 个调用点

| Unit | 调用点数 | 代表位置（改这里就能覆盖该 unit 的主路径） |
|---|---:|---|
| code | 11 | `crates/rg-ssh/src/lib.rs:864/867`（Git 读写）、`crates/rg-http/src/api/repo_content.rs:247/1321`、`lfs.rs:101/111`、`archive.rs:62` |
| packages | 8 | `crates/rg-http/src/oci.rs:114/118/159/164/179/333/346`、`package_registry/discovery.rs:200` |
| pulls | 6 | `crates/rg-http/src/api/pulls.rs:481`、`reviews.rs:121/358/1118/1266`、`reviews.rs:161` |
| collaboration | 6 | `collab.rs:248`（守卫定义）、`access.rs:56`、`collab_groups.rs:186/369`、`collab_overview.rs:43`、`human_notify.rs:100` |
| issues | 4 | `crates/rg-http/src/api/issues.rs:516/621/629/1265` |
| actions | 4 | `ci_environments.rs:278`、`artifacts.rs:223/550`、`ws.rs:408` |
| 通用守卫 | 4 | `repo_access.rs:45/78/96/113`（间接覆盖 182 个 handler 调用） |
| 仓库设置（mirror/push-mirror/模板） | 6 | `mirrors.rs:95/189/249/299`、`push_mirrors.rs:86`、`collab_templates.rs:155` |
| 跨 unit（通知/审批/入口） | 4 | `human_notify.rs:135/250`、`authorization/mod.rs:129`、`notifications.rs:51` |
| issues + pulls 复合 | 3 | `attachments.rs:377/544/577` |
| releases | 1 | `releases.rs:649`（只有下载 asset 走这个判断；写路径走 `require_write`） |
| actions / code 复合 | 1 | `webhooks_external.rs:93` |
| 非 unit（fork 源仓库读） | 1 | `repo/service.rs:1287` |

> 注：`wiki`、`board` 在 60 个直接调用点里**一个都没有**。它们今天完全走
> `repo_access::require_read` / `require_write` 间接判断（见 `crates/rg-http/src/api/wiki.rs`、
> `boards.rs` 的导入与调用）。这不代表它们不需要 unit，而是说明「直接调用点清单」不足以覆盖全部 surface，
> ADR 的迁移必须同时改守卫。

## 5. `route_meta.rs` 的 unit 标注量（精确统计）

`crates/rg-core/src/auth/route_meta.rs` 的 `ROUTES` 表（`route_meta.rs:183` 起，`518` 行 `];` 结束）：

```sh
$ grep -cE '^    (repo|other_repos)\("' crates/rg-core/src/auth/route_meta.rs   # 202
$ grep -cE '^    admin\("'             crates/rg-core/src/auth/route_meta.rs   #  27
$ grep -cE '^    user\("'              crates/rg-core/src/auth/route_meta.rs   #  38
```

（`ROUTES` 内共 266 条；第 529 行还有一条 `repo(...)` 是 fail-closed 的**非路由模式**，不属于 `ROUTES`。）

按路径前缀把 202 条仓库级路由逐条归类：

| 归属 | 条数 | 处置 |
|---|---:|---|
| `issues` | 21 | 加 `unit = Issues` |
| `pulls` | 25 | 加 `unit = Pulls` |
| `packages` | 21 | 加 `unit = Packages` |
| `actions` | 23 | 加 `unit = Actions` |
| `code` | 18 | 加 `unit = Code` |
| `boards` | 8 | 加 `unit = Boards` |
| `releases` | 6 | 加 `unit = Releases` |
| `wiki` | 4 | 加 `unit = Wiki` |
| **小计：单一 unit** | **126** | `RouteMeta.unit: Option<Unit>` 直接填 |
| 跨 unit 只读（`events` / `statuses` / `ai/*` / `search`） | 13 | `unit = CrossRead(&[Unit])`，或保持仓库读 |
| `skills` / `mcp-servers` | 12 | **需裁决**：新增 `registries` unit，或并入 `code` |
| 仓库设置（`hooks`/`webhooks`、`mirror`、`push-mirror`、`branch/tag protection`） | 13 | 保持仓库级 `admin`（不属于任何业务 unit） |
| 仓库级（`/repos`、`/repos/{owner}/{name}`、collaborators、star/watch/fork/transfer、template、keys） | 22 | 保持仓库级 |
| 组织/实例级（`/orgs/*`、`/imports`、`/skills`、`/mcp-servers`、`/search`） | 16 | 不属 unit |
| **合计** | **202** | |

**结论：需要新增 unit 字段的仓库路由 = 126（单 unit）+ 13（跨 unit 读）+ 12（skills/mcp，待裁决）= 151 条；
其余 51 条保持仓库级/组织级。** `route_meta.rs` 的 `RouteMeta` 结构体加一个字段，
`repo()`/`other_repos()` 构造器加一个 `unit` 参数，151 行调用点必须手写——
这就是任务卡所说的「实现必然改共享禁区、必须串行」。

## 6. 当前三档模型无法表达的场景（量化）

| # | 场景 | 今天的结果 | 期望 | 影响 |
|---|---|---|---|---|
| 1 | 外部贡献者只能报 issue、不能读代码 | 给 `read` 就能读全部私有代码；不给就 issue 也读不到 | `issues:write` + 无 `code:read` | 无法安全地开放 issue 协作 |
| 2 | 安全审计员只读代码、不能看 issue 讨论 | 只能给 `read`，连带看到全部 issue/PR | `code:read` only | 私有 issue 内容泄露 |
| 3 | 发布经理只能发 release、不能推代码 | 必须给 `write`，可推任意分支（受保护分支除外） | `releases:write` + `code:read` | 越权推代码 |
| 4 | CI 机器人只能写 commit status / 读 actions | 给 `write` 就能改代码、改 issue | `actions:write` | 供应链风险 |
| 5 | 包管理员只能发包、不能改仓库 | 给 `write` 全开 | `packages:write` | 同上 |
| 6 | 组织内 A 团队只做前端 code、B 团队只做 issues | 团队映射只影响写，且不分 unit | 每 unit 团队授权 | 组织协作无法落地 |

这 6 类共涉及上表 60 个调用点中的 **49 个**：code 11 + packages 8 + pulls 6 + issues 4 + actions 4 +
collaboration 6 + releases 1 + 仓库设置 6 + 复合 3。剩下 11 个是横切逻辑（通用守卫 4、审批入口 2、
通知入口 4、fork 1），unit 化后仍需仓库级兜底。

## 7. 复核方法

```sh
# 1) 命中数与本文一致
grep -rn "can_write_repo\|can_read_repo\|can_admin_repo\|fn can_" crates/*/src/ | wc -l   # 95

# 2) 任取一行复核实证列
sed -n '516p' crates/rg-http/src/api/issues.rs
#     rg_core::repo::service::can_write_repo(&state.db, &repo_model, Some(user_id))

# 3) 路由统计
grep -cE '^    (repo|other_repos)\("' crates/rg-core/src/auth/route_meta.rs   # 202（含 1 条 .idem() 变体）

# 4) 间接调用面
grep -rn "\brequire_write(" crates/rg-http/src/ | grep -v "pub(crate) async fn" | wc -l   # 43
```
