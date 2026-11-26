# 授权、审批与团队同步

> 状态（0.5.1）：仓库级三档权限、PAT / Agent 令牌限制、路由元数据注册表、通用审批原语（库）、CI 环境审批、LDAP 组→团队同步**已实现**；Unit 级权限**未实现**（ADR-0002 已裁决）。
> 事实来源：`crates/rg-core/src/authorization/`、`crates/rg-core/src/auth/`、`crates/rg-db/src/entities/`。

## 1. 两层授权

1. **仓库访问**：`read` / `write` / `admin`，回答「这个主体能不能碰这个仓库」。
2. **动作审批**：对危险动作（合并、直推保护分支、发版、释放受保护环境、扩权）单独收集同意票，回答「这次操作是否被批准」。

会话、展示名、协作答复都不是审批；协作模块只标记来源，不授予 Git、CI、合并或审批权限。

## 2. 仓库访问

- 判定顺序：显式协作者/组织成员 → 团队映射 → 仓库三档；`can_read_repo` / `can_write_repo` / `can_admin_repo` 是统一入口。
- 账号是唯一授权主体；Agent 是账号的受限形态，不能获得 `user` / `admin` 级能力。
- 每次请求重新判定；对象 ID 不授予权限。
- **Unit 级权限尚未落地**，当前仍是仓库级三档（见 §9）。

## 3. PAT 与 Agent 令牌

PAT scope 是**阶梯**，高层包含低层：

| scope | 允许 |
|---|---|
| `repo:read` | 读接口、`git clone` / `fetch` |
| `repo:collab` | 以上 + 写协作区（会话、任务、问题等） |
| `repo:write` | 以上 + 其他仓库写、`git push` |
| `repo` | 历史全仓 scope，包含以上全部 |

`user` 与 `admin` 独立；`admin` 只表示令牌可访问所有 scope，**不绕过账号自身的权限**。令牌永远不能超过持有者本人的权限。

分支受限令牌只能做不会移动分支的写操作；`ref_limited_write_allowed` 在 PAT 中间件里判定。

## 4. 路由元数据注册表

`crates/rg-core/src/auth/route_meta.rs` 是**唯一手写声明**：每条 REST 路由声明一次 PAT 需要什么。四张原本手维护的表由它派生：

| 派生结果 | 声明 |
|---|---|
| 需要的 PAT scope | `PatScope` |
| 是否触达其他仓库（allow-list 拒绝） | `other_repos` |
| 分支受限令牌能否写 | `BranchLimit` |
| 是否接受 `Idempotency-Key` | `Idempotency` |

协作路由统一由 `collaboration::capabilities::REGISTRY` 声明（默认 `repo:collab`、不能移动 ref、无幂等键），触达其他仓库的路由必须显式标注。新增路由必须补一行声明，防漂移测试会比对注册表与实际路由。

## 5. 通用审批原语（`authorization_requests` / `authorization_decisions`）

**一次请求**是一次特权动作的申请，绑定 `(action, subject, head_sha)` 并计算 payload hash（含规则版本）。人投票；票数够后，**执行动作的同一个事务里消费票据**。这是原语的核心：提前消费或在独立提交里消费，等于批准永久有效。

- 动作：`ci_environment_release`、`protected_branch_merge`、`protected_branch_push`、`release_publish`、`scope_expand`。每个动作在 `action.rs` 声明 TTL 与重试类别。
- 状态：`pending` → `consumed`；否则 `expired` / `denied` / `revoked`。行**永不删除**，审计保留原因。
- 决策：`approve` / `deny`，`(request_id, decider_user_id)` 唯一，同一人第二次投票是 no-op。
- 审批人形状：显式列表（受保护环境的 approver、分支的 push 白名单、PR 指定 reviewer），或判定式（`admins` / `writers`，在投票时调用与 handler 相同的函数）；`collaborators` 永不作为审批人——协作成员可能只有读权限。
- 请求主体可以是 `human` 或 `agent_session`；批准人必须是真人。

**当前接线程度（重要）**：

- 库已实现并有端到端测试，但**没有公开 REST 端点**。
- 唯一的生产接线是 PR 头部漂移：head 改变时把绑定旧 SHA 的 `protected_branch_merge` 票据原地置为 `expired`（`reason = head_sha_changed`）。
- 合并、直推、发版、受保护环境**仍走各自原有门禁**，尚未改为消费该票据。不要据此认为这些路径已统一审批。

### `scope_expand`

- 语义：**原地放宽**令牌的 `scopes`，不签发新令牌、不重启会话；只限 `repo` 家族。
- 与消费同事务：消费失败则放宽回滚，票据仍 `pending` 可重试。
- 目标写进 subject（`"<token_id>:<target_scopes>"`）并参与 hash，批准绑定的是「这个令牌 + 这个目标」。
- 令牌必须可证明只属于该仓库，否则一个仓库的管理员能放宽别的仓库的令牌。
- 放宽是**永久的**：目前没有「收窄」API，收回只能吊销或更换令牌。
- 批准人要求见 §9 D4（unit 落地后必须是 `code:admin`）。

## 6. PR 评审与协作答复

- PR 评审票只在其 `commit_id` 等于当前 head 时有效；移动 head 会使旧票失效（不需要删除）。
- 合并门禁、分支保护、必需检查是独立链路。
- 协作问题的人工答复是协作决定，**不是**分支保护审批；工作包里的自报证据也不等于平台验证。

## 7. CI 环境审批

`ci_environment_approvals` 是独立于通用原语的表：受保护环境配置 1–10 个审批人，审批人必须是真人（Agent 账号会被拒绝），同一人对同一 Job 只能投一次。它只负责放行等待中的 Job。

## 8. 团队与目录同步

- `teams` / `team_members` / `team_repositories` 属于 org；团队成员通过团队映射获得仓库权限。
- **LDAP group → team 同步已实现**（AUTH-403），三条规则：
  1. 一个 provider 只同步到一个 org（provider 上显式配置目标 org）；
  2. 目录 group 的 `cn` 必须与 team 名**精确相等**，匹配不上只报告、不创建团队（创建团队等于选择权限）；
  3. **只增不删**：目录里没有的成员保留并报告为 `extra_members`，不做自动移除。
- 同步由后台任务与 `ironforge` CLI 触发；管理端可设置 provider 的目标 org。

## 9. Unit 级权限（ADR-0002，未实现）

目标：把仓库三档扩展为 **8 个 unit × 3 档**，存 `repo_unit_permissions`；旧表 `repo_collaborators` 在新开关切到 `on` 前仍是唯一事实来源。老角色迁移时展开为全部 unit 同档，旧行为不变。

Unit：`code`、`issues`、`pulls`、`releases`、`wiki`、`packages`、`actions`、`boards`；`external_wiki` 功能不存在，不建空 unit。

已裁决（2026-11-25，D1–D6）：

| # | 决策 |
|---|---|
| D1 | 收紧「组织成员可读全部私有仓库」，但先产出影响面实测；受影响 >10 人或 >3 个仓库时先给过渡白名单；读模型变更单独开关 |
| D2 | `skills` / `mcp-servers` 不独立，并入 `code` |
| D3 | `webhooks` / `mirror` 保持仓库级 `admin`，不随 unit 放宽 |
| D4 | `scope_expand` 批准人强制 `code:admin`（owner 隐式满足），需负向测试 |
| D5 | 迁移号段预留 `m20261125_0000NN`，实现不得越段 |
| D6 | 附件按挂载实体归属：issue/issue-comment 走 `issues:*`，PR/PR-comment 走 `pulls:*` |

在实现并切换开关前，所有文档与代码都应继续按仓库级三档描述权限。

## 10. 不变量

1. 令牌 scope 只能收紧、不能放大账号权限。
2. 审批票绑定 `(action, subject, head_sha)`；head 变化使旧票失效。
3. 审批只由真人投；Agent 不能成为审批人。
4. 票据在执行动作的同一事务内消费，行永不删除。
5. 自报的主体类型、协作答复、工作包证据都不构成授权。
