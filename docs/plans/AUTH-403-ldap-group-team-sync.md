# AUTH-403：LDAP group → team 同步（设计定稿）

> 状态：设计就绪，待实现。gap list 行 `身份与权限 / 外部身份生命周期` 的剩余面之一。
> 前置：AUTH-401/402 已完成（`#19` + `#20`），目录同步的护栏、幂等、可逆语义已经就位。

## 为什么不能直接开写

`ldap-sync` 现在只处理**账号**（停用/恢复）。group → team 是另一条链路：目录里的 group 决定平台里的 team 成员。它需要三件东西同时存在才有意义：

1. provider 上的 group 配置（迁移 + 设置页表单）
2. 目录侧 group 枚举（`ldap3` 查询，含成员属性）
3. 成员写入（team_member 表）

只有第 2、3 项而没有第 1 项的表单，功能配置不了，等于交付死代码。所以这里先把三个必须拍板的规则写死，实现时不再返工。

## 三个规则（本文件即决策记录）

### 规则 1：一个 provider 只同步到一个 org

`team` 属于 org（`teams.org_id`），而 LDAP group 是全局的。判断「这个 group 对应哪个 org 的 team」需要一个锚点。

**采用**：provider 上新增 `ldap_group_team_org_id`，显式指定目标 org。

**不采用**：从 group DN 或 CN 里解析 org 名（`cn=repo-foo-dev` 这类约定）。理由：那等于把组织架构编码进字符串，一次改名就断，而且解析规则要写死在代码里，比一列更脆。

### 规则 2：group CN 精确等于 team name，匹配不上就跳过并报告

| 目录 | 平台 | 结果 |
|---|---|---|
| `cn=backend` | team `backend`（同 org） | 同步成员 |
| `cn=Backend` | team `backend` | **不匹配**，报告 `unmatched_group` |
| `cn=backend` | team 不存在 | 报告 `unmatched_group`，**不自动建 team** |

**不自动建 team**：team 带 `permission`（read/write/admin），那是授权决策，不能让目录里的人靠建一个同名 group 就拿到。

大小写敏感是有意的：`Backend` 和 `backend` 是两个 group，静默归一化会让人以为同步了实际没有。

### 规则 3：只增不删（第一个版本）

group 里有、team 里没有的成员 → 加入。
team 里有、group 里没有的成员 → **不动**，只报告 `extra_members`。

理由：

- 平台上的 team 成员可能来自 group 之外（手工授权、另一个 provider、历史遗留）。自动移除会误伤。
- 「只增」的第一版可以安全地跑起来观察一段时间，看 `extra_members` 报告里到底是垃圾还是合法例外。
- 移除是**不可逆**的（成员被踢出后要重新加），而增加是可逆的。风险不对称。

第二版再加 `--prune`（默认关闭），届时 `extra_members` 的历史报告就是它的白名单依据。

## 数据模型

迁移 `m20261123_000003_add_ldap_group_sync`，`sso_providers` 加三列，全部可空：

| 列 | 含义 | 空值行为 |
|---|---|---|
| `ldap_group_base_dn` | group 搜索起点 | 不做 group 同步 |
| `ldap_group_filter` | group 过滤，如 `(objectClass=groupOfNames)` | 不做 group 同步 |
| `ldap_group_team_org_id` | 目标 org | 不做 group 同步 |

三列必须同时有值才启用。半配置在设置页保存时就拒绝（与 `IRONFORGE_S3_*` 四项成组的处理一致）。

## 同步语义

```
list_groups(provider) ─┐
                       ├─→ plan_groups() 纯函数 ─→ GroupSyncPlan ─→ apply()
本地 team + 成员 ───────┘
```

`list_groups`：以 `ldap_group_base_dn` + `ldap_group_filter` 搜索，取 `cn` 和成员属性（`member`，回退 `uniqueMember`）。与用户枚举同样的铁律：**非零 LDAP 结果一律报错**，被截断的 group 列表不能当成"成员都退群了"。

`plan_groups(local_teams, directory_groups, user_dn_index) -> GroupSyncPlan`：

```rust
pub struct GroupSyncPlan {
    /// (team_id, user_id) pairs to insert.
    pub add: Vec<(i64, i64)>,
    /// Groups with no team of that name.
    pub unmatched_groups: Vec<String>,
    /// Member DNs with no local account.
    pub unmatched_members: Vec<String>,
    /// Members already in the team that the group does not list (reported only).
    pub extra_members: Vec<(String, String)>, // (team, username)
}
```

成员匹配键：**用户表的 `ldap_dn`**。它是登录时写下的目录身份，与 group 的 `member` 值同源。用户名不可靠（可改），email 可空。

`apply` 调 `org_ops::add_team_member(db, team_id, user_id, "member")`（底层函数，无 `actor_id` —— 目录同步没有行为主体，权限校验在这一层不适用）。

## 交付拆分（4 个 PR）

| # | 内容 | 验收 |
|---|---|---|
| 1 | 迁移 + entity 字段 + 设置页三个输入框（三列成组校验） | 保存半配置被拒；既有 provider 不受影响 |
| 2 | `ldap::list_groups` + 单测（过滤器拼接、成员属性回退、非零 rc 抛错） | 无目录环境时用 mock 的 filter 字符串断言 |
| 3 | `plan_groups` 纯函数 + 7 个测试（匹配、大小写、无 team、无用户、extra 报告、空 group、幂等） | 不需要 LDAP 或 DB |
| 4 | `apply` + CLI `ldap-sync --groups` + 调度接入（复用 6h tick） | 真实 LDAP 目录（`osixia/openldap` 容器）跑一次增员 |

PR 1 和 3 无依赖，可并行；2 依赖 1 的配置字段；4 依赖 1/2/3。

## 明确不做

- **自动建 team**（规则 2）
- **移除成员**（规则 3，第二版加 `--prune`）
- **嵌套 group 展开**（`memberOf` 递归）。真实目录里嵌套很常见，但展开需要环检测和深度上限，是独立一块
- **PAM / SMTP 身份源**：同一 gap-list 行的另一部分，与本设计无关

## 验收（整行 AUTH-403）

- [ ] 一个 provider 配好后，`ldap-sync` 能把 group 成员加进同名 team
- [ ] group 有、team 没有的成员被加；team 有、group 没有的**没被动过**，且出现在报告里
- [ ] group 名匹配不到 team 时只报告，不建 team
- [ ] 目录返回 `sizeLimitExceeded` 时整体报错，不写入任何成员
- [ ] 三列半配置时设置页拒绝保存
