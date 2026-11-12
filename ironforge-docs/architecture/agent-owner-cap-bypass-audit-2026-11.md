# Agent 所有者上限：绕过 `can_*_repo` 的判权点排查（CNB #42）

> **状态**：已落地（本地分支，未经 CI） ｜ **可信度**：确认（代码 + 本机测试） ｜ **日期**：2026-11-13
> 设计依据：[agent-principals-design-and-roadmap-2026-11.md](agent-principals-design-and-roadmap-2026-11.md) §2.3 不变量 3（权限 = agent 自己的授权 ∩ 所有者当前权限；agent 永不管理仓库）与 §2.5 P2。
> 本文是 §2.5 该行的结论附件；设计文档与 CLAUDE.md 由主会话合并。

## 1. 方法

1. 在当前 main（`d2c6292`）上重跑 issue 的起点 grep（排除 tests）：
   `git grep -n 'repo_collaborator_ops::get_permission\|is_org_member\|find_org_member' -- crates`
2. 对每个命中判定：**授权 agent 的写/管理/读动作吗？** 是 → 必须受上限；否 → 写明证据。
3. grep 之外的变体：`org_ops::`/`repo_collaborator_ops::` 的其它调用、`organization_member`/`repo_collaborator`/`team_member` 实体的直接使用、`owner_id ==` 判断、`list_user_orgs`、`can_admin_org` 的全部调用者、`list_org_members`、`list_team_members`。
4. 上限的定义以 `repo::service::apply_owner_cap` / `agent::owner_cap` 为准：`None`（人）/`Owner(id)`（agent，取 min）/`Denied`（所有者缺失、停用或非人类）。

## 2. 逐点结论（grep 命中）

| 位置（行号为修改前） | 用途 | 结论 | 处置 |
|---|---|---|---|
| `collaboration/access.rs:27`、`:34`（`is_collab_member`） | 协作数据读权限（`require_collab_read`）、升级提醒（`escalation.rs:284`）、诊断（`diagnostics.rs:182`）、协作组读取（`collab_groups.rs:159`） | **绕过**。协作者行或组织成员身份直接返回 true，只有最后的 `can_write_repo` 一支带上限。公开仓库上：所有者无权、agent 是只读协作者 → agent 能读协作任务/问题/决定。 | **已修**：agent 须所有者也是协作组成员（递归一次），`Denied` 直接 false |
| `collaborator/service.rs:151/158`（`get_effective_permission`） | 返回"有效权限" | **当前无调用者**（`git grep` 全仓只剩定义），所以现网不可利用；但它是公开函数，且答案错：agent 会得到自己的 `admin`。 | **已修**：agent 取与所有者的较小值，且不超过 `write`；用单测钉住，防止日后被接上 |
| `org/mod.rs:219`（`can_admin_org`） | org 更新、成员增删、团队创建/删除、团队成员增删、`set_team_repositories`、仓库转移目的地的组织管理判断 | **绕过**。agent 若被 org 管理员加为 `admin`/`owner` 成员，即可改成员与团队，进而经团队→仓库映射授予写权限。与 §2.3「agent 永不管理」矛盾。 | **已修**：agent 恒 false（与 `can_admin_repo` 同规则） |
| `org/mod.rs:244`（`can_read_org`） | 私有 org 及其成员/团队列表的读权限（`require_org_read`，`orgs.rs` 6 处） | **绕过**。agent 是成员即可读，不看所有者。 | **已修**：改走 `is_effective_org_member`（agent 须所有者也是成员） |
| `rg-http/src/api/repos.rs:156`（创建仓库时校验 org 成员） | 在 org 下建仓库 | **不用于授权 agent**：随后 `create_repo_with_opts` 的 `ensure_human(owner_id)`（`repo/service.rs:489`）拒绝 agent；agent 本来也不能建仓库（不变量 5，既有测试覆盖）。 | 不改，写明证据 |
| `rg-http/src/api/repos.rs:312`（`list_repos`，org 成员可见私有仓库） | 列出 org 下仓库，成员能看见私有的 | **绕过**。agent 作为 org 成员能列出所有者看不到的私有仓库（名称、描述）。 | **已修**：改走 `is_effective_org_member` |
| `repo/service.rs:1409`（`transfer_repo` 目的地 org 成员校验） | 把仓库转入 org | **不用于授权 agent**：同函数 `:1392-1393` 先 `ensure_human` 两端，agent 到不了这里。 | 不改，写明证据 |
| `repo/service.rs:216/220`、`:310/316`（`can_read_repo`/`can_write_repo`） | 上限逻辑所在 | 返回前经 `apply_owner_cap` | 无需改 |
| `repo/service.rs:363/373`（`can_admin_repo`） | 管理判权 | 函数开头 agent 恒 false，之后才查表 | 无需改 |
| `org/mod.rs:177/182`（`is_org_member`/`find_org_member` 薄封装） | 原始成员行查询 | 无调用者把结果当授权用（见下） | 文档注释标明"原始行，授权请用 `is_effective_org_member`" |

## 3. grep 没覆盖的变体

| 变体 | 结论 |
|---|---|
| `org::list_user_orgs`（`GET /orgs`） | **绕过**（信息泄露）：agent 看到自己所属、但所有者不在的 org（含私有 org 的名称与元数据）。**已修**：agent 只列所有者也属于的 org，`Denied` 为空 |
| `can_admin_org` 的全部调用者（`org/mod.rs` 内 8 处 + `repo/service.rs:1377`） | 都经同一函数，一处修复全部覆盖；`delete_org` 用 `org.owner_id == requesting_user_id`，agent 不能拥有 org（`create_org` 有 `ensure_human`），不受影响 |
| `*_unchecked` 的 org/team 变更函数 | `pub(crate)`，调用者是 org 创建与测试夹具，不接受 HTTP actor；不改 |
| `human_notify.rs`（`repo_collaborator_ops::list_by_repo`、`org_ops::list_org_members`） | 只是**候选名单**，随后逐人 `can_read_repo`/`can_write_repo`（带上限）才入选；不改 |
| `review/codeowners.rs`（`list_team_members`） | 只产生评审**请求**候选，不授权；agent 本来也不能批准（不变量 8）；不改 |
| `repo.owner_id == user_id` 直接比较 | 命中点逐个看过：`collaborator/service.rs`（已覆盖）、`access.rs`（已覆盖）、`repos.rs:534`（比较两个仓库的 owner，无关主体）、`repo/service.rs:1384`（转移，前有 `ensure_human`）。agent 永不是 `owner_id`（不变量 5） |
| `/admin/orgs` 等管理员接口 | 需要 `admin` scope，agent 令牌拿不到（不变量 2） |
| 30 秒权限缓存 | 未改。`is_collab_member` 与 `can_admin_org` 不读缓存；`is_effective_org_member` 每次查库 |

**未覆盖/待确认**：
- 其它直接读 `team_member`/`organization_member` 表的 SQL 在 `rg-db/src/ops/org_ops.rs`（37 处）里，本次只审了上层调用者；`is_member_of_write_team_for_repo`、`is_member_of_admin_team` 仅被已带上限的 `can_write_repo`/`can_admin_repo`/`can_admin_org`（agent 已提前返回）调用。
- Web 前端与 MCP 工具表未逐一核对是否另有路径直接展示 org 成员信息。
- `add_org_member` 仍允许把 agent 加成 org 成员（含 `admin`/`owner` 角色）；现在它拿不到任何超出所有者的东西，是否应在加入时直接拒绝 `owner`/`admin` 角色属产品决定，未做。

## 4. 测试

`crates/rg-http/tests/agent_principal_tests.rs` 新增 4 项（每项都含"人类/所有者在位时通过"的对照）：

| 测试 | 钉住的行为 |
|---|---|
| `an_agent_in_an_org_its_owner_is_not_in_sees_nothing_of_it` | org 页面/成员/团队/`GET /orgs`/`GET /repos/{org}` 对 agent 不可见，所有者入组后可见 |
| `an_agent_with_an_org_admin_role_still_does_not_administer_the_org` | 有 `admin` 角色的 agent 加成员、建团队均 403，`can_admin_org` 为 false；同角色的所有者成功 |
| `an_agent_is_in_a_collaboration_group_only_if_its_owner_is` | 公开仓库上，agent 是协作者而所有者不是 → 协作任务 403、`is_collab_member` false；所有者加入后放行 |
| `the_effective_permission_of_an_agent_is_capped_by_its_owner` | `get_effective_permission`：None / read / write（admin 被压到 write），人类不受限 |

验证结果见 §5。

## 5. 运行记录（本机，**未经 CI**，仅 SQLite）

| 命令 | 结果 |
|---|---|
| 去掉源码修复（保留新测试）后 `cargo test -p rg-http --test agent_principal_tests -- <4 项新测试>` | 4 项全部失败（红）——测试确实咬住了缺口 |
| 恢复修复后 `cargo test -p rg-http --test agent_principal_tests` | 23 项通过（原 19 + 新 4） |
| `cargo clippy --workspace --all-targets -- -D warnings` | 无告警 |
| `cargo fmt --all -- --check` | 干净（已 `cargo fmt --all`） |
| `cargo test -p rg-http` 的相邻套件：`org_tests`、`admin_org_tests`、`collaborator_tests`、`collab_visibility_tests`、`collab_escalation_tests`、`collab_diagnostics_tests`、`collab_groups_tests`、`collab_human_notify_tests`、`team_read_write_asymmetry_tests`、`team_repo_mapping_tests`、`org_repo_name_scope_tests` | 全部通过 |
| `cargo test -p rg-core --lib` | 278 项通过 |

**没跑**：PostgreSQL / MySQL（本次没有新增 SQL，全部复用既有 `org_ops`/`repo_collaborator_ops`/`user_ops` 查询，故未跑，但这不是跨库证据）；完整 `cargo test --workspace`；`collab_*` 之外的其它 rg-http 套件；前端；真实 MCP 客户端。

## 6. 给设计文档 §2.5 的建议改写

> **P2（已排查）**：绕过 `can_*_repo` 的判权点已逐一排查，见 [agent-owner-cap-bypass-audit-2026-11.md](agent-owner-cap-bypass-audit-2026-11.md)。修了 5 处：`is_collab_member`（协作读）、`can_admin_org`（agent 永不管理 org）、`can_read_org`、`list_user_orgs`、`repos.rs::list_repos` 的 org 成员判断；`get_effective_permission` 无调用者但已加上限。确认不授权 agent 的 2 处：`repos.rs` 建仓库（`ensure_human`）、`transfer_repo`（`ensure_human`）。新增 org 或协作者相关判权点时，**用 `is_effective_org_member` 或 `can_*_repo`，不要直接读成员行**。
