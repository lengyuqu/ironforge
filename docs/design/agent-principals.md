# Agent 主体身份

> 状态（0.5.1）：已实现。事实来源：`crates/rg-core/src/agent.rs`、`crates/rg-db/src/entities/user.rs`、`crates/rg-http/src/api/audit.rs`。
> 协作会话是另一层概念（见 [agent-collaboration.md](agent-collaboration.md)）：账号负责授权，会话只标记来源。

## 1. 什么是 Agent

Agent 是 `users` 表中的一行：`kind = "agent"`，`owner_user_id` 指向所属的人，另有 `agent_description`。

- 用户名固定为 `<owner>-agent-<name>`；人名和组织名不能包含 `-agent-`，所以用户名本身说明归属。
- 一个人最多拥有 50 个 Agent。
- **没有人的人口**：没有密码（`password_hash` 不是哈希）、`auth_provider = "agent"`（没有登录路径接受）、邮箱在 `.invalid` 域（任何身份提供方都无法断言）。唯一凭据是 owner 签发的 PAT。
- Agent 不能创建仓库/组织，不能 fork 或转移仓库；这些操作要求 `ensure_human`。

## 2. Owner 上限

`owner_cap` 在每次仓库权限判定时被查询：

| 情况 | 结果 |
|---|---|
| 人类账号 | `None`（无上限） |
| Agent，owner 是活跃人类 | `Owner(owner_id)`：最多等于 owner 当前在该仓库的权限 |
| Agent，owner 不存在/被禁用/本身也是 Agent | `Denied`：什么都不能做 |

Agent **永远不能获得仓库 `admin`**，也不能通过 owner 的权限绕过自己令牌的 scope 限制；两者取交集。

## 3. 令牌

- Agent 只能用 PAT；令牌 scope 只能是仓库家族：`repo`、`repo:write`、`repo:collab`、`repo:read`。`validate_agent_token_scopes` 拒绝 `user` / `admin`，因此账号管理、组织管理和管理员 API 都在射程之外。
- PAT 还有可选的仓库 allowlist 与 ref allowlist；ref 受限令牌只能做不会移动分支的写。
- 令牌可设过期时间；PAT 中间件每次请求从数据库解析，不缓存 scope，所以吊销与扩权在下一次请求生效。

## 4. 被拒绝的入口

- 密码登录、SSO 登录、SSH 密码认证（`auth_password` 直接拒绝 Agent）。
- 成为审批人：受保护环境、分支审批、PR 审批都要求真人（`ensure_person` / `is_human_account`）。
- 用户侧协作门（需要真人身份的操作）拒绝 Agent；Agent 应通过协作会话写入，让记录指向会话而不是账号。
- 管理 Agent 本身（创建、删除、改描述）只有 owner 能做。

## 5. 审计

- Agent 的每个审计行自动带 `on_behalf_of_user_id = owner_user_id`；人类的行该列为 `NULL`。
- 审计查询支持按 `on_behalf_of_user_id` 过滤，能回答「这个人名下的 Agent 做了什么」。
- 该列没有外键：审计行要能活过账号删除。

## 6. 通知

- 「该通知谁」与「权限受谁限制」用同一条规则：人类的 accountable person 是自己；Agent 是它的 owner。
- owner 不存在、被禁用或本身是 Agent 时没有收件人，通知不发出，而不是退回给 Agent。
- Webhook/邮件里同时保留 Agent 用户名与 owner，便于追责。

## 7. 提交身份

- REST 提交可以显式给 `git_author_name` / `git_author_email`；平台动作的主体仍记录为执行者（Agent 或人）。
- Git 作者字段是提交内容的一部分，**不证明**平台侧是谁发起的；追责看审计与 PR/评论记录，不看 author 字符串。
- Agent 的草稿分支用 `agent_draft_namespaces` 预留目录（见协作文档 §9）。

## 8. 未实现 / 边界

- 组织拥有的 Agent（`owner_user_id` 必须是人）。
- Agent 专属的评审规则（例如「Agent 的 PR 必须由人批」目前靠审批人必须是真人来保证，不是独立规则）。
- 按 Agent 的排名或信誉分；健康指标只做平台事实，不做排名。
