# 协作 O02：用户总览（实现与验证记录）

> 日期：2026-11-05。分支 `feat/collab-remaining`。设计依据：[运维需求 §3](agent-collaboration-operability-requirements-2026-11.md)。**可信度**：确认（逐层标注）。

## 已实现（实现证据）

`GET /api/v1/repos/{owner}/{name}/collaboration/overview?sections=&limit=`（仓库读权限；人工入口，**未**暴露为 MCP 工具）。四个 section，各自是独立的实时查询：

| section | 内容 | 备注 |
|---|---|---|
| `awaiting_me` | 发给当前账号或仓库写者的开放问题 | 复用 `inbox::for_user`；回答需要写权限，读者得到 `available:false` 和原因，而不是空列表 |
| `blocked_waiting` | 状态为 `blocked`/`waiting` 的任务 | |
| `pending_handoffs` | 仓库内所有 `pending` 交接 | 带目标会话状态 |
| `in_progress` | 状态为 `in_progress` 的任务 | |

每个条目：`object_type/object_id/task_id/title/state/occurred_at/session_id/session_state/flags/origin`。`flags` 只标出声明看起来不可靠的地方：`unowned`、`session_not_online`（会话 `offline/expired/ended/missing`）。每个 section 带 `as_of`、`truncated`、`continue_with`（同一批记录带游标分页的接口）。响应带 `consistency: per_section_live_query` 和 `notes`（不是原子快照；在线≠正在工作；状态是声明不是验证）。`sections` 不认识的名字返回 400，不静默忽略。

## 取舍 / 偏差

- **没有 `next_cursor`**：设计草案写了每个 section 带游标，这里只给 `truncated` + `continue_with`，翻页走各自已有的分页接口（任务列表、任务交接列表、用户收件箱）。理由：总览不新增一套游标契约和长期状态。
- **没有跨仓聚合**，没有缓存。
- `awaiting_me` 内部取收件箱最多 100 条再过滤开放问题；超过时 `truncated` 为真。
- 已完成（`done`）和 `open` 状态的任务不在任何 section；`open` 未被设计列入默认视图。
- 不含「最近活动」section（设计草案提到）：没有可靠的活动源，未做。

## 验证

| 层级 | 结论 |
|---|---|
| 单元 | ✅ `rg-core` `collaboration::overview`：section 解析默认/去重/未知拒绝 |
| 任务验证（SQLite） | ✅ `cargo test -p rg-http --test collab_overview_tests`：5 项（空仓库、各 section 内容与标记、过期会话不被当成完成、截断与 section 选择、读者看不到待答、无权限不泄漏名称/数量） |
| PostgreSQL 17 / MySQL 8.4（HTTP 层） | ✅ 全新库上同样 5 项通过（本机 Docker） |
| 防漂移 | ✅ `collab_contract_tests` 16 项、`rg-http --lib` 87 项（OpenAPI 守卫） |
| 撤权/慢请求/服务重启的用户体验 | ❌ 未验证（无 UI；接口每次重新判权，沿用协作端点的缓存失效） |
| 合并后集成 / 远端 CI / 真实用户 | ❌ 未验证 |
| 浏览器 / UI | ❌ 无 UI（Web UI 另起会话，见路线图） |
