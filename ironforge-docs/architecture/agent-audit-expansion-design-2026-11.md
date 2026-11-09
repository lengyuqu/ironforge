# 审计扩展设计（E1，2026-11-10，待批注）

> 状态：**只出方案，不写代码**。范围按 2026-11-09 的 CTO 建议收窄：第一阶段只做「权限类写入 + 受保护分支的 push」，其余分批排队。E2（邮件偏好）同日决定搁置，不在本文范围。

## 一、现状（代码事实）

| 项 | 事实 |
|---|---|
| 统一入口 | `rg-http/src/api/audit.rs::record_write(db, user_id, action, resource_type, resource_id, resource_name, headers, details)`：fire-and-forget，失败只记日志；`details` 里由 `with_access_token` 合入 `access_token_id` |
| 已在用 | `repo.file_commit`、`repo.file_delete`、`repo.branch_delete`（REST 与 `git push --delete` 两处）、`pull_request.create`、`pull_request.merge`、`pull_request.review` |
| 旧副本 | 4 个各自实现的 `record_audit`：`api/repos.rs`、`api/orgs.rs`、`api/users.rs`、`api/admin.rs` —— 字段集与 `record_write` 基本一致，差别是 `username` 由调用方传入而不是查库 |
| 已归档 | `rg-core/src/audit/archiver.rs` 后台任务，默认 `archive_after_days = 90`，导出为压缩 NDJSON 后从表里删 |
| 表 | `audit_logs`：`user_id`（可空）、`username`、`action`、`resource_type`、`resource_id`、`resource_name`、`ip_address`、`user_agent`、`details`（JSON 字符串）、`created_at` |

**完全没有审计的写入**（本次盘点，按「谁能做」排序）：issue 与评论、标签与里程碑、wiki、release、分支保护/标签保护、webhook 配置、协作者与团队、仓库设置（默认分支/可见性/改名/转移/归档）、`git push` 本身、组织成员与团队、令牌创建/吊销（部分已在 `users.rs` 的记录里，需核对）。

## 二、第一阶段（建议先做）

只做「改权限 / 改安全边界」的写入 —— 这些是事后追责最需要、量又最小的：

| action | 触发点 | `resource_type` | `details`（白名单） |
|---|---|---|---|
| `repo.protection_create` / `repo.protection_update` / `repo.protection_delete` | `api/branch_protection.rs` | repo | `rule_id`、`branch_name`、变更的字段名（`require_pr` 等，只放布尔/枚举） |
| `repo.tag_protection_*` | 同上（标签保护的三个端点） | repo | 同上 |
| `repo.webhook_create` / `repo.webhook_update` / `repo.webhook_delete` | webhook 配置端点 | repo | `webhook_id`、`events`、`active`（**不放 URL 里的 token 与 secret**） |
| `repo.collaborator_add` / `repo.collaborator_remove` | 协作者端点 | repo | `collaborator_id`、`permission` |
| `repo.settings_update` | 仓库设置（默认分支、可见性、归档） | repo | 变更字段名与新值（枚举/布尔） |
| `repo.transfer` | 转移仓库 | repo | `from_owner_id`、`to_owner_id` |
| `token.create` / `token.revoke` | `api/users.rs` 的令牌端点 | user | `token_id`、`scopes`、`repos`、`refs`（**不放令牌值**） |
| `org.member_add` / `org.member_remove` / `org.team_*` | `api/orgs.rs` | org | `member_id`、`team_id`、`role` |

**push 只记受保护分支**：`git push` 每次一行对 Audit 表是明显的写放大（一个正常仓库一天几百次），而「普通分支被推送」在事件日志（`repo_events`）里已经有了。第一阶段只给「推送到被保护规则覆盖的分支」记一行 `repo.push_protected`，`details` = `ref`、`old_sha`、`new_sha`、`protected_pattern`。

## 三、后两批（先登记，不排期）

| 批次 | 内容 | 为什么往后 |
|---|---|---|
| 第二批 | issue 与评论、标签、里程碑、wiki、release | 量大（评论尤其），价值主要是内容追溯 —— 而这些内容本身已经带作者与时间落库，审计行的边际价值低于权限类 |
| 第三批 | 协作区写入（`collaboration/**`）、CI 触发、commit status | 协作区有自己的事件与消息流；是否再进审计要等 E 系列产品决定 |

## 四、`details` 的三条硬规则

1. **只放 id、枚举、布尔、短字符串**（分支名、规则 id、权限值）。不放令牌、secret、webhook 的 URL 查询串、用户写的正文。
2. **不信任客户端输入**：`details` 只能由服务端从已校验的数据里拼，请求体里的原始 JSON 不直接落库。
3. `access_token_id` 一律由 `with_access_token(headers, …)` 合并，不在各调用点手写。

## 五、把 4 份旧副本并进 `record_write`

`record_write` 现在会自己查用户名；旧副本的调用方已经拿到了用户名，所以合并是「删掉参数、让它查库」：

1. 在 `audit.rs` 再暴露一个 `record_write_named(..., username: Option<&str>)`（或者让 `record_write` 接受 `Option<&str>` 覆盖查库结果），避免给每个调用点平白多一次查询。
2. 逐个替换 `api/{repos,orgs,users,admin}.rs` 的 4 份 `record_audit`，保持它们现有的 `action` 字符串不变（改名要单独做，且有外部消费方风险）。
3. 用一条 grep 守卫（现有 `test_no_raw_git_command_in_crates` 的风格）：仓库里不允许再出现 `fn record_audit`。

## 六、保留与归档

沿用现有 `archiver`：90 天后导出压缩 NDJSON 并从表里删。新增的高频动作（`repo.push_protected`）会让归档体积上升，建议在同一 PR 里给出「每仓库每天行数」的估算，并在压测后决定是否需要给审计表加 `(created_at)` 索引之外的分区策略（SQLite 不支持分区，PG/MySQL 可以 —— 但那是第二个决定）。

## 七、测试与证据计划

| 层级 | 内容 |
|---|---|
| 单元 / 集成 | 每个新 action 一条「做一次 → `audit_logs` 多一行且字段正确」的断言（沿用 `branch_delete_tests` 里查审计表的写法） |
| 负向 | 令牌创建/吊销的审计里**不得出现令牌值**（按字符串断言） |
| 后端 | 新测试进 `scripts/ci-db-tests.sh` 的 agent 列表，PG/MySQL 各跑一遍 |
| 文档 | `CLAUDE.md` 的审计段落 + 一份验证记录，逐条列已覆盖/未覆盖的写入 |

## 八、待你批注的点

1. 第一阶段的范围是否就按上表（**权限类 + 受保护分支 push**）？
2. `repo.push_protected` 只在受保护分支上记，能接受吗？还是要求「所有 push」？
3. 旧 action 名（`repos.rs` 等 4 处的字符串）保持不变、只换实现 —— 同意吗？
4. 审计表是否需要在 UI 上可查（现在只有 `GET /admin/audit`）？这一点会影响第二阶段的优先级。
