# 协作收件箱个人已读标记（实现与验证记录）

> 日期：2026-11-05。分支 `feat/collab-remaining`。设计依据：[P0 设计 §收件箱](agent-collaboration-p0-design-2026-11.md)。**可信度**：确认（逐层标注）。

## 已实现（实现证据）

- 表 `collab_inbox_reads`（迁移 `m20261105_000002`，`up` 幂等，`down` 删表）：主键 `(repo_id, actor_kind, actor_id, item_key)`，存 `observed_version`、`read_at`；仓库外键级联。`actor` 是会话或账号，**不**设外键，故会话/账号被删后留下的行无法被任何查询触达，随仓库一起清理（已知，不另做清理）。
- 接口：`PUT …/collaboration/inbox/read`（会话门：仓库读权限 + 会话凭据）、`PUT …/inbox/user/read`（用户门：写权限，与 `inbox/user` 一致）。请求 `{item_key, observed_version}`，未知字段 400。返回 `{item_key, observed_version, current_version, read_state, read_at}`。MCP：`collab_mark_inbox_read`（仅会话门；用户门是人工入口，不进 MCP）。
- 收件箱每项新增 `object_version`、`read_state`（`read`/`unread`）、`last_read_version`、`last_read_at`。`read_state=read` 当且仅当已存标记 ≥ 当前对象版本，所以对象出现更新版本后重新变为未读，旧标记仍可见。
- 规则：只有**当前在该 actor 收件箱里**的项能标记（任务须归该会话且未完成；交接须发给该会话且待处理；问题须在其收件箱范围内且状态对应），其余——不存在、他人的、状态已变、格式错误、未知类型——统一 404，不泄漏存在性；`observed_version` < 1 或超过当前版本返回 400；较旧版本可标记但不会使当前版本变已读；标记只增不减、幂等；并发写收敛到最大值（条件 `UPDATE` + 唯一键竞争的插入，不用 `ON CONFLICT DO NOTHING`）。
- **并发缺陷与修复（如实）**：第一版 `raise` 在“UPDATE 没命中且此时行已存在”时直接认为已覆盖，但行可能是刚被另一个更低的标记插入的，于是较高的标记被丢掉（完整 workspace 测试中 `racing_marks_converge_on_the_highest_version` 有一次得到 3 而不是 4）。现在只在已存标记 ≥ 目标版本时才结束，否则重试 UPDATE（最多 8 轮）。**同样的缺陷在更早的消息水位 `collab_read_ops::advance` 里也存在**，已一并修复；它没有专门的并发回归测试。修复后该测试单独连跑 6 次通过，这不能证明竞争已被穷尽。
- 只表达阅读：不改问题、交接、任务状态，也不推进消息的 `collab_reads` 水位；已读不代表同意或处理。

## 取舍 / 偏差

- 设计写“`items[]` 含 `task_version`、`read_state`”：这里用 `object_version`（该记录自己的版本：任务/交接/问题），因为 `item_key` 指向的就是这个对象。
- 设计写 `GET P/inbox?session_id=` 带快照与游标：这里的收件箱仍是实时查询（`consistency: live_query`），与已有实现一致；已读标记在每次请求时查询。
- 没有 `unread_only` 过滤：过滤发生在截断之后会让 `truncated` 语义失真，客户端自行按 `read_state` 过滤。
- `direct_message` 类型本来就不在收件箱里，也无法标记；消息已读继续用 `collab_reads` 与 `read-receipt`。

## 验证

| 层级 | 结论 |
|---|---|
| 任务验证（SQLite） | ✅ `cargo test -p rg-http --test collab_inbox_read_tests`：6 项（新项未读并带版本；标记仅对本 actor 生效且不改问题；新版本重新未读；单调与超前拒绝；越界/他人/格式错误统一 404；并发标记收敛） |
| PostgreSQL 17 / MySQL 8.4（HTTP 层） | ✅ 全新库上同样 6 项通过（本机 Docker） |
| 迁移回滚窗口 | ✅ `rg-db` `soft_delete_tests` 窗口 16 → 17 |
| 撤权后的行为 / 账号删除后的孤儿行清理 | ❌ 未验证（孤儿行无害但不会被清理，见上） |
| 合并后集成 / 远端 CI / 真实 Agent | ❌ 未验证 |
