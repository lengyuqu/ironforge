# ADR-0002：仓库 Unit 级权限模型

- Status: **Accepted**（2026-11-25 协调者裁决；实现尚未开始）
- Date: 2026-11-25
- Task: T33（`tasks/T33.md`）
- Evidence: `docs/plans/T33-authorization-map.md`（95 行实测）、`docs/plans/T33-plan.md`（执行拆解）
- Supersedes: 无
- Related: ADR-0001（CI 执行边界）、`docs/plans/T20-plan.md` / `T21-plan.md`（`scope_expand`）

## 1. 决策摘要

把仓库权限从「仓库级 `read` / `write` / `admin` 三档」扩展为「**8 个 Unit × 3 档**」，
存储在新表 `repo_unit_permissions`，旧表 `repo_collaborators` 保留并作为唯一事实来源直到 feature flag 切到 `on`。
`read` / `write` / `admin` 三种老角色在迁移时**展开为全部 8 个 unit 的同档授权**，因此旧行为零变化；
新授权才具备 unit 粒度。

不做的代价：当前模型无法表达 6 类真实协作场景（只报 issue 的外部贡献者、只读代码的审计员、只发版的发布经理、
只写 CI 的机器人、只发包的包管理员、按团队分 unit 的组织），涉及 60 个直接判断点中的 49 个。

## 2. 背景与现状语义（复核过的代码）

### 2.1 没有可复用的 action 枚举

`crates/rg-core/src/authorization/action.rs:15-32` 的 `Action` 是**审批工单**的 5 个动作：

```rust
pub enum Action {
    CiEnvironmentRelease,
    ProtectedBranchMerge,
    ProtectedBranchPush,
    ReleasePublish,
    ScopeExpand,
}
```

它回答的是「谁有权批准一次高危操作」，不是「谁能读 issue」。unit 模型需要新的枚举
（本 ADR 第 4 节），**不要**扩展 `Action`。

### 2.2 判定顺序（`repo/service.rs`）

| 原语 | owner | collaborator | org | 团队 | agent 限制 |
|---|---|---|---|---|---|
| `can_read_repo` (`:199`) | 是 | 任意记录（含 `read`） | 任意成员 | 不参与 | owner cap（`:265-267`） |
| `can_write_repo` (`:296`) | 是 | `write` / `admin` | `owner` / `admin`，或写团队映射 | 参与 | owner cap |
| `can_admin_repo` (`:351`) | 是 | `admin` | `owner` / `admin`，或管理团队 | 参与 | **agent 一律 false**（`:359-361`） |

与 Gitea 的差异，逐条：

1. **读没有 unit 概念，且是组织级**。`repo/service.rs:195-198` 明确写「read access is organization-wide」，
   团队 `permission='read'` 无效。Gitea 的 `unit` 表对每个 unit 分别存 `read/write/admin`。
2. **collaborator 是单列 `permission`**。`crates/rg-db/src/entities/repo_collaborator.rs:10-17` 只有
   `repo_id` / `user_id` / `permission` / `created_at`，迁移定义在
   `crates/rg-db/src/migrations/m20260424_000008_create_phase6.rs:230-263`，唯一键
   `idx_repo_collaborators_repo_user` 是 `(repo_id, user_id)`。
3. **团队映射只区分 write/admin 两种团队**，没有 unit。`org_ops::is_member_of_write_team_for_repo` /
   `is_member_of_admin_team`（`crates/rg-db/src/ops/org_ops.rs`）。
4. **Gitea 的 `external_wiki` IronForge 没有**，不能照抄九个 unit。
5. **Gitea 允许 unit 级 `admin` 独立于仓库 admin**（例如 issues admin 删所有 issue），IronForge 今天
   只有仓库 admin。这是本次要补的核心能力。

### 2.3 缓存是 unit 化最大的隐性风险

`can_read_repo` / `can_write_repo` 共用一个 30 秒缓存键空间（`repo/service.rs:302` 注释：
"same cache key space as can_read_repo"），键是 `(repo_id, actor_id, for_write)`。
加入 unit 维度后，键必须变成 `(repo_id, actor_id, unit, for_write)`，否则
`issues:read` 的缓存结果会被 `code:read` 命中。**迁移时忘记改缓存键 = 静默越权。**
`invalidate_perm_cache_user` 目前也是按 `(repo, user)` 清，需扩展为按 unit 清（或全清）。

## 3. Unit 集合（8 个，裁剪自 Gitea 的 9 个）

| Unit | IronForge 对应功能 | 纳入 | 理由 |
|---|---|---|---|
| `code` | Git、浏览、分支/标签保护、LFS、archive、mirror | ✅ | 核心 |
| `issues` | issue、label、milestone、time tracking、附件 | ✅ | 有独立数据模型 |
| `pulls` | PR、review、merge queue、suggestion | ✅ | 与 issues 分表 |
| `releases` | release + asset | ✅ | 与 tag 分离发布 |
| `wiki` | wiki 页面与历史 | ✅ | 独立存储 |
| `packages` | 10 种适配器 + OCI | ✅ | 独立发布通道 |
| `actions` | pipeline、job、artifact、secret、environment | ✅ | 独立执行面 |
| `boards` | Board / column / card | ✅ | 有 handler 与表 |
| `external_wiki` | 无 | ❌ | 功能不存在，不建空 unit |

**未纳入但必须裁决的两类**：

- `skills` / `mcp-servers`（12 条路由）：是仓库内的「制品注册表」，既像 packages 又像 code。
  **已裁决：并入 `code`**（读走 code 读、发布走 code 写），不新增 unit。若将来出现「只给 skills
  不给代码」的真实诉求再拆（避免第一版多一个 unit 的迁移与 151 条之外的标注负担）。
- `webhooks` / `mirror` / `push-mirror` / `branch-protection`（13 条路由）：属于**仓库设置**，
  继续要求仓库级 `admin`，不归任何业务 unit。否则「能写 code」就等价于「能配 webhook 外发代码」，
  是明确的提权。

## 4. 每个 Unit 的 read / write / admin 边界（落到具体 handler）

约定：
- `read` = 能列出并读取该 unit 的对象；
- `write` = 能在该 unit 内创建/修改/删除对象（不包含仓库设置）；
- `admin` = 该 unit 的管理动作（删除他人内容、改 unit 级配置、审批该 unit 的高危操作）。
- 隐式规则：`admin ⊃ write ⊃ read`，与现有三档一致。

| Unit | read（落点） | write（落点） | admin（落点） |
|---|---|---|---|
| code | `repo_content::list_tree` (`repo_content.rs:287`)、`get_blob` (`:367`)、`get_log` (`:414`)、`archive::download_archive` (`archive.rs:62`)、SSH `authorize_git_service` (`rg-ssh/src/lib.rs:864`) | `repo_content::create_or_update_file` (`:1443`)、`delete_file` (`:1586`)、`delete_branch` (`:657`)、`lfs::upload_object` (`lfs.rs:203`)、SSH 写 (`rg-ssh/src/lib.rs:867`) | 分支/标签保护规则 CRUD（`branch_protection.rs`、`tag_protection.rs`，今天走 `require_admin`）、`template` 标记 |
| issues | `issues::list_issues` (`issues.rs:307`)、`get_issue` (`:457`)、`list_comments` (`:703`)、`list_milestones` (`:1346`) | `create_issue` (`:490`)、`update_issue` (`:594`)、`add_comment` (`:737`)、`create_label` (`labels.rs:119`)、`create_milestone` (`issues.rs:1392`)、附件上传 (`attachments.rs:377`) | `delete_label` (`labels.rs:229`)、`delete_milestone` (`issues.rs:1612`)、删除他人 issue/评论 |
| pulls | `pulls::list_prs` (`pulls.rs:171`)、`get_pr` (`:221`)、`get_diff` (`:526`)、review 读取 | `create_pr` (`:276`)、`update_pr` (`:466`)、`submit_review` (`reviews.rs:358`)、`set_thread_resolution` (`reviews.rs:1266`)、`enqueue_merge_queue` (`pulls.rs:824`) | `merge_pr` (`pulls.rs:559`)、dismiss review、`update_branch`（`pull_readiness.rs` 的 handler）、合并队列配置、`request_reviewer` 的强制指派（`reviews.rs`） |
| releases | `list_releases` (`releases.rs:71`)、`get_release` (`:206`)、`download_asset` (`:631`) | `create_release` (`:118`)、`update_release` (`:283`)、`upload_asset` (`:446`) | `delete_release` (`:346`)、`delete_asset` (`:720`)、`ReleasePublish` 审批（`authorization/evaluator.rs:196-207`） |
| wiki | `wiki::list_pages` (`wiki.rs:86`)、`get_page` (`:122`)、`list_revisions` (`:341`) | `create_page` (`:162`)、`update_page` (`:241`) | `delete_page` (`:305`)、删除历史版本 |
| packages | `packages::list_packages` (`packages.rs:405`)、`get_package` (`:439`)、`download_file` (`:632`)、OCI pull (`oci.rs:333`) | `packages::publish` (`:215`)、`publish_npm` (`:320`)、OCI push (`oci.rs:346`) | `delete_version` (`:555`)、`yank_version` (`:589`) |
| actions | `ci::list_pipelines` (`ci.rs:114`)、`get_pipeline` (`:175`)、`get_job` (`:297`)、`ws::ws_job_log_handler` (`ws.rs:408`)、`artifacts::download_artifact` (`artifacts.rs:245`) | `ci::trigger_pipeline` (`:465`)、`retry_pipeline` (`:560`)、`rerun_job` (`:778`)、`artifacts::upload_artifact` (`artifacts.rs:57`) | `cancel_pipeline` (`:653`)、`ci_environments::approve` (`ci_environments.rs:278`)、secrets CRUD（`ci_secrets.rs`）、retention 配置（`ci_retention.rs`） |
| boards | `boards::list_boards` (`boards.rs:153`)、`get_board` (`:184`) | `create_board` (`:113`)、`update_board` (`:217`)、`create_card` (`:400`)、`move_card` (`:488`) | `delete_board` (`:252`)、`delete_column` (`:364`)、`delete_card` |

> `issues` 的 `admin` 与今天 `can_admin_repo` 不等价：今天只有仓库 admin 能删别人的 issue
> （`issues.rs:621` 的 `update_issue` 对非作者要求 `can_write_repo`）。unit 化后 `issues:admin` 会成为
> 一个**比仓库 admin 窄**的新角色，这正是要补的能力，但也意味着 `is_issue_author || can_write` 这类
> 「作者或写者」判断要改成「作者 || `issues:write`」。

**每个 unit 至少一个 handler 落点，已满足任务卡验证第 2 条。**

## 5. 兼容映射：老角色 → unit 三档

迁移时对 `repo_collaborators` 每一行展开为 8 行。**这张表是行为等价性的唯一依据，必须逐格实现。**

| 老 `permission` | code | issues | pulls | releases | wiki | packages | actions | boards |
|---|---|---|---|---|---|---|---|---|
| `read` | read | read | read | read | read | read | read | read |
| `write` | write | write | write | write | write | write | write | write |
| `admin` | admin | admin | admin | admin | admin | admin | admin | admin |

为什么 `write` 给全部 8 个 unit 的 `write` 而不是「code/issues/pulls + 部分」：因为今天
`can_write_repo` 对所有这些 handler 都返回 true（第 2 节实测：packages publish、actions trigger、
release publish、wiki edit、board edit 全部只查 `can_write_repo`）。任何「更精细」的映射都会在迁移瞬间
缩小现有用户权限，属于行为回归。**先用最宽的等价映射上线，再让管理员主动收窄。**

组织层面的映射：

| 现状 | unit 化后 |
|---|---|
| 仓库 owner | 8 个 unit 的 `admin`（计算得出，不落库） |
| 组织 `owner` / `admin` | 8 个 unit 的 `admin` |
| 组织普通成员 | 8 个 unit 的 `read`（保持「读是组织级」这一既有事实，见 2.2 第 1 条） |
| 写团队映射 | 8 个 unit 的 `write`（阶段 1）；阶段 3 给团队表加 `unit` 列 |
| 管理团队映射 | 8 个 unit 的 `admin`（同上） |
| agent | 仍受 `apply_owner_cap` 封顶；`can_admin_repo` 的「agent 一律 false」改为「agent 一律非 admin，但可持有 read/write」 |

**注意**：第 3 行「组织普通成员 → 8 个 unit read」意味着迁移**不会**修复「组织成员能读所有私有仓库」
的既有行为。是否收紧读模型是**独立决策**（见第 11 节 D1），不要混在 unit 迁移里做，否则回滚面爆炸。

## 6. 数据模型选择

**选择：新建 `repo_unit_permissions`，不改 `repo_collaborators`。**

```sql
CREATE TABLE repo_unit_permissions (
    id           BIGINT PRIMARY KEY,          -- auto increment
    repo_id      BIGINT NOT NULL,
    user_id      BIGINT NOT NULL,
    unit         VARCHAR(16) NOT NULL,        -- code|issues|pulls|releases|wiki|packages|actions|boards
    permission   VARCHAR(8)  NOT NULL,        -- read|write|admin
    created_at   TIMESTAMP   NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at   TIMESTAMP   NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE UNIQUE INDEX uq_repo_unit_perms
    ON repo_unit_permissions (repo_id, user_id, unit);
CREATE INDEX idx_repo_unit_perms_user
    ON repo_unit_permissions (user_id, repo_id);
```

理由：

1. **一个 (repo,user) 要存 8 个 unit 的不同档位**。给 `repo_collaborators` 加列只能加 8×(read/write/admin)
   三列共 24 列，或者加 JSON 列；前者列爆炸且每加一个 unit 要迁移，后者无法建索引、三后端 JSON 查询语义不同。
2. **旧表保持不动**，`down` 时直接丢弃新表即可回到旧语义，回滚风险最低。
3. **唯一键表达力**：`uq_repo_unit_perms(repo_id,user_id,unit)` 天然保证「一个人一个 unit 一个档位」，
   比 24 列的 CHECK 约束清楚。
4. 代价：`can_write_repo` 从 1 次查询变成 1 次查询 + 命中 unit 行；用 30s 缓存（键加 unit）抵消。
   索引 `(repo_id,user_id,unit)` 是等值查询，三后端都是覆盖索引。

不选 JSON 列的理由补充：SQLite 的 `json_extract`、PG 的 `jsonb @>`、MySQL 的 `JSON_CONTAINS` 三套写法，
且无法用普通唯一索引约束，sea-orm 的类型映射也要三套。

**团队 unit 授权**（阶段 3）另建 `team_unit_permissions(team_id, unit, permission)`，
不改本表；本表的 `user_id` 只表示直接协作者。

## 7. 迁移设计（只设计，不写迁移文件）

### 7.1 版本号与顺序

现有最后一个迁移是 `m20261124_000001_add_pipeline_job_steps.rs`。新迁移建议
`m20261125_000001_add_repo_unit_permissions.rs`。
`crates/rg-db/src/migrations/` 是共享禁区（`PLANNING.md:24`），**必须由协调者串行安排**，
否则与其它任务撞号。这是本任务不改码的直接原因之一。

### 7.2 up 的三步

1. `create_table(repo_unit_permissions)` + 两个索引。
2. 回填（幂等）：

```sql
INSERT INTO repo_unit_permissions (repo_id, user_id, unit, permission)
SELECT c.repo_id, c.user_id, u.unit, c.permission
FROM repo_collaborators c
CROSS JOIN (
    SELECT 'code' AS unit UNION ALL SELECT 'issues' UNION ALL SELECT 'pulls'
    UNION ALL SELECT 'releases' UNION ALL SELECT 'wiki' UNION ALL SELECT 'packages'
    UNION ALL SELECT 'actions' UNION ALL SELECT 'boards'
) u
WHERE NOT EXISTS (
    SELECT 1 FROM repo_unit_permissions p
    WHERE p.repo_id = c.repo_id AND p.user_id = c.user_id AND p.unit = u.unit
);
```

三后端差异：

- **SQLite**：不支持 `INSERT ... SELECT` 里的 `LIMIT`，但上面的 `NOT EXISTS` 写法可用；`CROSS JOIN` 子查询
  带 `UNION ALL` 也可用。回填需在单事务内，SQLite 单写者，注意与启动时其它连接竞争（WAL 下写锁）。
- **PostgreSQL**：同样可用；更快的写法是 `CROSS JOIN unnest(ARRAY['code',...]) AS u(unit)`。
  注意 `unit` 不是保留字，但 `permission` 在部分 schema 下是关键字，建议列名保持 `permission` 并加双引号，
  sea-orm 生成的 SQL 会处理。
- **MySQL**：`CROSS JOIN (SELECT ... UNION ALL ...)` 必须给派生表起别名（`) u` 已给）；8.0 之前
  不支持 `VALUES ROW(...)`。`VARCHAR(16)` 足够容纳 `releases`（8 字符）。唯一索引名长度上限 64，
  `uq_repo_unit_perms` 安全。

3. 不改 `repo_collaborators`，不删任何列。

### 7.3 down 的可回滚性

```sql
DROP TABLE repo_unit_permissions;   -- 唯一一步
```

旧表从未被修改，因此 `down` 后 100% 回到旧语义。代价是**放弃 unit 粒度**（这是预期的）。
不要在 `down` 里尝试把 unit 行聚合回 `repo_collaborators`：旧表值从未变过，聚合只会引入
「有人在新表里被降权、down 后权限回升」的歧义。

### 7.4 上线开关（三态）

`ironforge.toml` 新增（实现任务负责，本 ADR 只约定语义）：

```toml
[permissions]
unit_model = "off"      # off | shadow | on
```

- `off`：只读旧表，新表被回填但不参与判定。默认值，保证部署即兼容。
- `shadow`：新旧两套都算，结果不一致时**以旧为准**并打 `unit_permission_divergence` 审计事件（含
  repo/user/unit/old/new）。用于灰度验证回填正确性，至少跑一个发布周期。
- `on`：以新表为准，旧表只用于回填与回滚。

三态让「迁移」与「语义切换」解耦：迁移可以跟随任意一次发版，切换则是一次配置变更 + 重启。

## 8. `scope_expand` 与 unit 的交互（最可能出越权的地方）

### 8.1 票据里的 scope 不是 unit

`pat_scope::ALLOWED_SCOPES`（`crates/rg-core/src/auth/pat_scope.rs:22-29`）只有
`repo` / `repo:write` / `repo:collab` / `repo:read` / `user` / `admin`。**token scope 没有 unit 维度**，
也不应该有：scope 限制「这个令牌能调哪一族 API」，unit 限制「这个人能碰仓库里的哪部分」。
两者是 AND 关系：

```
最终允许 = 令牌 scope 满足该路由 && 该用户在目标 repo 的该 unit 上满足该档位
```

这带来一个必须写进实现验收的结论：**unit 模型不会、也不能通过 token scope 收窄**。一个
`repo:write` 的 PAT 持有者，如果只有 `issues:write`，仍然不能 push——因为 `can_write_unit(code)` 为 false。

### 8.2 三个越权点

1. **`apply_owner_cap` 是最大的坑**。`repo/service.rs:252-270` 对 agent 调用
   `can_write_repo` / `can_read_repo`（仓库级）。如果 unit 化后只改 handler 不改这里，
   agent 的「owner 能做」这一半仍然是**整仓**判断：owner 只有 `code:write` 时，
   agent 仍会拿到 issues 写权限。**验收必须有一个用例**：owner 仅 `code:write`，agent 尝试
   `create_issue` 必须 403。修法：`apply_owner_cap` 增加 `unit` 参数，递归调用
   `can_write_unit(db, repo, owner, unit)`。
2. **`evaluator::may_approve` 的 writers/admins 是整仓谓词**（`authorization/evaluator.rs:69-73`）。
   `ProtectedBranchMerge` 今天允许「任何仓库写者」批准合并。unit 化后必须落到 `pulls:write`，
   否则只有 `issues:write` 的人可以批准合并——严重越权。同理：
   - `CiEnvironmentRelease` → `actions:admin`
   - `ReleasePublish` → `releases:admin`
   - `ScopeExpand` → 仍是仓库 `admin`（它扩大的是 token 全域能力，不该由单个 unit 的 admin 批）
3. **`scope_expand` 的目标是 scope 阶梯，不是 unit**。`widening_of`
   （`scope_expand.rs:315-341`）只证明「不缩小、不变窄、不含 user/admin」。这是正确的：
   一个 agent 被扩到 `repo` 后，它在每个 unit 上的实际能力仍被 owner 的 unit 权限封顶（前提是
   第 1 点已修）。**若第 1 点未修，`scope_expand` 就成为跨 unit 提权通道**：
   批准人只需是任一仓库 admin（`:226-237` 的 `admins: true`），批准后 agent 拿到整仓写。

### 8.3 建议的额外闸门

`scope_expand` 的批准人当前是「仓库 admin」。unit 化后如果出现「只有 `issues:admin` 的人」，
他仍应能批准 scope_expand 吗？**建议不能**：scope_expand 影响的是整个仓库的 token 能力，
应要求 `code:admin` 或仓库 owner。这一点需要在实现时显式写进 `evaluator`，并用测试钉住。

## 9. 影响面与实施分解

### 9.1 影响面数字

| 项 | 数量 | 出处 |
|---|---:|---|
| 直接授权调用点 | 60 | 本 ADR 配套 map §0.1 |
| 需要改签名的原语 | 4 | `can_read_repo` / `can_write_repo` / `can_admin_repo` / `is_collab_member` |
| 通用守卫调用点（间接） | 182 次 / 34 文件 | map §0.2 |
| `route_meta.rs` 需加 unit 的仓库路由 | 151（126 单 unit + 13 跨 unit + 12 skills/mcp 待裁决） | map §5 |
| 新增迁移 | 1 | 第 7 节 |
| 前端 | 协作者编辑页出 8×3 勾选矩阵；现有 `web/` 的 collaborators 组件 | 见 9.3 S6 |

### 9.2 子任务拆分（owned paths + 验收命令 + 串行关系）

> 标注 **[串行]** 的必须按序落地；**[并行]** 的彼此 owned paths 不相交，可同时开工。

**S0 [串行，阻塞全部]：Unit 类型与判定骨架（零外部依赖）**

- owned: `crates/rg-core/src/authorization/unit.rs`（新文件）
- 内容：`enum Unit { Code, Issues, Pulls, Releases, Wiki, Packages, Actions, Boards }`、
  `enum UnitPermission { Read, Write, Admin }`、`Unit::ALL`、`as_str`/`parse`、
  `UnitPermission::satisfies`。**不接线任何 handler。**
- 另交付：D1 影响面报告（§11 的 SQL 在三后端各跑一次，给出 `users` / `repos` 数字与是否触发白名单）。
- 验收：`cargo test -p rg-core --lib authorization::unit`；D1 报告随 S0 一起提交到 `docs/plans/T33-d1-impact.md`
- 串行原因：S2/S5/S6 都要引用这些类型；先合入可避免三处各写一套。

**S1 [串行，阻塞 S2]：迁移 + 实体 + ops**

- owned: `crates/rg-db/src/migrations/m20261125_000001_add_repo_unit_permissions.rs`（新）、
  `crates/rg-db/src/migrations/mod.rs`、`crates/rg-db/src/entities/repo_unit_permission.rs`（新）、
  `crates/rg-db/src/entities/mod.rs`、`crates/rg-db/src/ops/repo_unit_permission_ops.rs`（新）、
  `crates/rg-db/src/ops/mod.rs`
- 内容：第 6/7 节的表与回填；`get_permission` / `set_permission` / `list_by_repo` / `expand_legacy`。
- 验收：
  ```sh
  cargo test -p rg-db
  # 三后端各跑一次迁移 + 回填（CI 矩阵）
  cargo test -p rg-db --features db-postgres && cargo test -p rg-db --features db-mysql
  ```
- 串行原因：迁移序号共享；必须等协调者分配号段。

**S2 [串行，S0+S1 之后]：新原语 + feature flag**

- owned: `crates/rg-core/src/repo/service.rs`、`crates/rg-core/src/config/*`（`unit_model` 开关）、
  `crates/rg-core/src/authorization/mod.rs`（`apply_owner_cap` 调用方）
- 内容：
  - `can_read_unit` / `can_write_unit` / `can_admin_unit`，`off` 时分别委派旧原语（行为不变）；
  - `apply_owner_cap` 增加 `unit` 维度（第 8.2 第 1 点）；
  - 缓存键加 `unit`（第 2.3 节）；
  - `shadow` 模式的双算与审计事件。
- 验收：
  ```sh
  cargo test -p rg-core repo::service
  # 新增用例必须包含：owner 仅 code:write 时 agent 不得写 issue
  ```
- 串行原因：`repo/service.rs` 是所有人的依赖；先定型签名，后续只加调用。

**S3 [串行，S2 之后]：四个通用守卫加 Unit 参数**

- owned: `crates/rg-http/src/api/repo_access.rs` + 31 个调用文件（一次性机械替换）
- 内容：`require_read(state, headers, owner, name, unit)` 等；对「仓库设置」类 handler 传
  `UnitGuard::RepoAdmin` 而非业务 unit。
- 验收：`cargo test -p rg-http`；`cargo clippy -p rg-http --all-targets`
- 串行原因：43+54+69+14+2 处 handler 侧调用必须同一次编译通过，无法拆给多人并行改同一函数签名。

**S4 [并行 ×8，S3 之后]：按 unit 迁移直接调用点**

| 子任务 | owned paths | 验收 |
|---|---|---|
| S4a code | `api/repo_content.rs`、`api/lfs.rs`、`api/archive.rs`、`rg-ssh/src/lib.rs` | `cargo test -p rg-ssh -p rg-http repo_content lfs archive` |
| S4b issues | `api/issues.rs`、`api/labels.rs`、`api/attachments.rs`（issue 分支） | `cargo test -p rg-http issues` |
| S4c pulls | `api/pulls.rs`、`api/reviews.rs`、`api/attachments.rs`（PR 分支） | `cargo test -p rg-http pulls reviews` |
| S4d releases | `api/releases.rs` | `cargo test -p rg-http releases` |
| S4e wiki | `api/wiki.rs` | `cargo test -p rg-http wiki` |
| S4f packages | `api/packages.rs`、`rg-http/src/oci.rs`、`rg-core/src/package_registry/discovery.rs` | `cargo test -p rg-http packages oci` |
| S4g actions | `api/ci.rs`、`api/artifacts.rs`、`api/ci_environments.rs`、`api/ci_secrets.rs`、`rg-http/src/ws.rs` | `cargo test -p rg-http ci artifacts` |
| S4h boards | `api/boards.rs` | `cargo test -p rg-http boards` |

`attachments.rs` 被 S4b/S4c 同时拥有是**故意标注的冲突点**：它按 `AttachmentTarget` 分流，
建议单独交给一个子任务（S4c）做，S4b 不改该文件。这样 8 个子任务两两无重叠。

**S5 [串行，S0 之后]：`route_meta.rs` 加 unit 字段**

- owned: `crates/rg-core/src/auth/route_meta.rs`、`crates/rg-http/tests/route_metadata_tests.rs`
- 内容：`RouteMeta` 加 `unit: Option<Unit>`，`repo()` / `other_repos()` 增加参数，151 行逐条填。
- 验收：
  ```sh
  cargo test -p rg-http --test route_metadata_tests
  # 新增断言：每条 /repos/{owner}/{name}/issues* 的 unit == Some(Issues)
  ```
- 串行原因：共享禁区，且 drift test 会因任何一条漏填而失败；多人改必然冲突。

**S6 [并行，S3 之后]：前端协作者矩阵**

- owned: `web/`（协作者页与 API client）
- 内容：8 行 × 3 列勾选矩阵；保存走 `PUT /repos/{owner}/{name}/collaborators/{id}` 的新
  `units` 字段（需 S3 提供）。
- 验收：前端构建 + 手工路径（本 ADR 不规定测试框架，由实现任务补）。

**S7 [串行，最后]：切默认值、删旧路径**

- owned: `crates/rg-core/src/repo/service.rs`、`CLAUDE.md`、`ironforge-docs/`（协调者统一改）
- 内容：`unit_model` 默认改 `on`；观察两个发布周期后删除 `off` 路径与旧谓词。
- 验收：`cargo test --workspace`

### 9.3 关键路径

```
S0 ─┬─> S2 ─> S3 ─┬─> S4a..S4h (并行)
    │              └─> S6
    ├─> S5
    └─> S1 ─> S2
S7 依赖 S4 全绿 + 两个发布周期
```

## 10. 不做会怎样（量化）

| 现状代价 | 数字 |
|---|---|
| 无法表达的协作场景 | 6 类（map §6） |
| 受影响判断点 | 49 / 60（82%） |
| 需要「整仓 write」才能完成的最小授权 | 5 类动作：发包、发版、写 CI、写 commit status、管 issue |
| 因整仓 write 顺带获得的越权 | 至少 8 个 unit 的写权限，其中 code 写可推分支（受保护分支除外） |
| 「读是组织级」导致的私有仓库泄露面 | 组织全部成员 × 全部私有仓库，unit 化也不会自动修复（见 D1） |

具体风险例子（可直接写进验收）：

- 一个只该发 npm 包的机器人，拿到 `write` 后可以用同一 token 调
  `PUT /repos/{owner}/{name}/contents/{path}` 改代码（`repo_content.rs:1321` 只查 `can_write_repo`）。
- 一个只该看代码的安全审计员，拿到 `read` 后能读私有 issue 与 PR 讨论（`issues.rs:1265`、
  `pulls.rs` 列表均只查 `can_read_repo`）。
- 外部贡献者想报 issue，必须给 `read`，从而能 clone 全部私有代码（`rg-ssh/src/lib.rs:864`）。

## 11. 裁决记录（2026-11-25，余曲）

| ID | 问题 | 裁决 | 落地要求 |
|---|---|---|---|
| D1 | 是否收紧「组织成员可读全部私有仓库」 | **收紧，但先测影响面** | S0 必须先产出一份实测数字：多少仓库、多少用户会失去访问。**受影响用户 >10 或涉及 >3 个仓库时，先提供一次性过渡白名单再收紧**。读模型变更单独开关，不与 unit 迁移同批上线 |
| D2 | `skills` / `mcp-servers` 是否独立 unit | **不独立，并入 `code`** | 12 条路由全部按 `code` 标注；不新增 unit |
| D3 | `webhooks` / `mirror` 归 unit 还是仓库 admin | **保持仓库级 `admin`** | webhook 可外泄私有事件、mirror 可把整仓推到外部，均不可逆；不随 unit 放宽 |
| D4 | `scope_expand` 批准人 | **强制 `code:admin`** | `evaluator` 中 `ScopeExpand` 的 `admins: true` 改为 `code:admin` 谓词；仓库 owner 仍可批（owner 隐式 code:admin），但仅 `issues:admin` 等 unit admin 不可批。必须加负向测试 |
| D5 | 迁移号段 | **批准预留 `m20261125_0000NN` 整段** | 实现时不得使用该段之外的新号；与其它任务串行合入 |
| D6 | `attachments.rs` 归属 | **按附件实际挂载的实体决定，默认 `issues`** | 代码事实：`AttachmentTarget` 区分 issue / PR / comment；S4 按目标分流，issue 与 issue-comment 走 `issues:*`，PR 与 PR-comment 走 `pulls:*`，文件本身由**一个**子任务（S4c）拥有以避免合并冲突 |

### D1 的影响面查询（S0 交付物）

```sql
-- 会失去读权限的用户-仓库对：组织成员，且不是 owner、不是该仓库 collaborator
SELECT COUNT(DISTINCT om.user_id) AS users,
       COUNT(DISTINCT r.id)      AS repos
FROM repositories r
JOIN org_members om ON om.org_id = r.org_id
WHERE r.deleted_at IS NULL
  AND r.is_private = 1
  AND om.user_id <> r.owner_id
  AND NOT EXISTS (
      SELECT 1 FROM repo_collaborators c
      WHERE c.repo_id = r.id AND c.user_id = om.user_id
  );
```

判定：`users > 10` 或 `repos > 3` → 必须附过渡白名单（一张 `repo_read_allowlist(repo_id,user_id,expires_at)` 表 +
一个到期清理任务），否则直接收紧。三后端写法一致（`is_private` 在 MySQL/SQLite 是整数，PG 是 boolean，
实现时按后端绑定）。

## 12. 验收清单（实现任务照此验收）

1. `repo_unit_permissions` 在 SQLite / PG / MySQL 三后端迁移 + 回填成功，行数 = 8 × collaborator 数（每个老角色展开为 8 个 unit）。
2. `unit_model=off` 时全量测试与基线一致（行为零变化）。
3. `unit_model=shadow` 跑一个发布周期，`unit_permission_divergence` 事件为 0。
4. 以下负向用例全部 403：
   - 仅 `issues:write` 的用户 push（SSH 与 HTTP 两条路径）；
   - 仅 `code:write` 的用户 `create_release`；
   - 仅 `code:write` 的 owner 的 agent 调 `create_issue`（第 8.2 第 1 点）；
   - 仅 `issues:admin` 的用户批准 `ProtectedBranchMerge`（第 8.2 第 2 点）。
5. `route_metadata_tests` 对 151 条带 unit 的路由全部断言通过。
6. `down` 后旧表数据完好、旧语义恢复（抽样 10 个仓库对比权限矩阵）。
7. D1 影响面数字已产出；若 `users > 10` 或 `repos > 3`，过渡白名单表与到期清理已实现并测试。
8. 仅 `issues:admin` 的用户批准 `scope_expand` 必须 403（D4）。
