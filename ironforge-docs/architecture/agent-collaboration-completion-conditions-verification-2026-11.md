# 协作 O04：自定义完成条件（展示模式）（实现与验证记录）

> 日期：2026-11-05。分支 `feat/collab-remaining`。设计依据：[运维需求 §5](agent-collaboration-operability-requirements-2026-11.md)。**可信度**：确认（逐层标注）。

## 已实现（实现证据）

**只做“展示”，不做“强制”**：条件不阻止、不修改任务状态，不接分支保护或合并；Agent 的 `done` 仍只是声明。

- 迁移 `m20261105_000003`（`up` 幂等，`down` 删三张表）：
  - `collab_completion_condition_sets`：任务的条件定义，**每次变更插入下一个版本**，永不原地修改。唯一键 `(task_id, version)` 仲裁并发，`(task_id, client_request_id)` 做幂等。
  - `collab_completion_confirmations`：`manual_confirmation` 的显式确认，记录当时的任务版本与分支头。
  - `collab_completion_snapshots`：把一次评估原样存档。
- 接口（读 = 仓库读权限，写 = 仓库写权限）：
  - `GET/PUT …/tasks/{id}/completion-conditions`：读当前定义 / 写新版本（`client_request_id`、`expected_version`、完整 `conditions`，最多 20 条，`[]` 为清空）。成功 201，重放 200；过期的 `expected_version` 返回 409 `VERSION_CONFLICT`（带当前版本）；同键不同体 409 `IDEMPOTENCY_KEY_REUSED`。
  - `GET …/completion-conditions/versions`：历史版本，新到旧，`before_version` 翻页。
  - `POST …/completion-conditions/{condition_id}/confirmations`（用户门，需写权限，**不进 MCP**）。
  - `POST/GET …/completion-status/snapshots`、`GET …/snapshots/{id}`。
- 条件类型与评估（`satisfied` / `unsatisfied` / `stale` / `unknown`）：
  - `question_answered{question_id}`：同任务的问题。开放 → unsatisfied；已答且上下文新鲜 → satisfied；已答但任务版本/提交已变 → stale；无法判断 → unknown。
  - `work_package_present{work_package_id}`：同任务的包。存在且为最新修订 → satisfied（只说明包存在，内容仍是作者声明）；被更新修订取代 → stale。
  - `ci_passed_for_commit{commit_sha}`：复用既有 CI 判定（只认平台记录的 pipeline，只认**那个** commit；分支头已离开该 commit → stale；无记录 → unknown；失败/取消 → unsatisfied）。
  - `manual_confirmation{}`：无确认 → unsatisfied；确认时的分支头与现在一致（或当时就没有分支）→ satisfied；分支已移动 → stale；确认时有头而现在读不到 → unknown。**协作确认，不是合并审批，`actor_kind` 不证明是真人。**
- 条件有稳定 `id`：PUT 里带已有 `id` 保持身份和 `introduced_in_version`，类型不可变；不带 `id` 为新增；未知 id、重复 id 400。参数闭合（多余/缺失参数 400），引用必须是**同任务**的问题/工作包，commit 须是完整十六进制并规范化。
- `completion-status` 新增 `defined`（`condition_set_version`、逐项状态/说明/证据、`overall`）；**有定义时由它决定** `declared_done_without_evidence`，原来由现有记录推导的 `conditions` 与 `overall` 不变（旧客户端不受影响）。快照存的是当时完整的评估，之后不重算。

## 取舍 / 偏差

- **强制模式没做**：文档要求单独评审权限、迁移、旧客户端兼容和故障时 fail-closed 行为；这里不碰。
- **设计里的 `PUT` 同时带“任务期望版本”**：这里只用条件集自己的 `expected_version`。条件与任务版本的关系体现在各条件的评估上（如问题上下文随任务版本变 stale），不把任务版本写进定义。
- **确认不进 MCP**：同账号 Agent 不应能替“授权账号”自我确认；该口只在用户门。
- 每个任务最多保留 200 个快照（`MAX_SNAPSHOTS_PER_TASK`，快照不可变、不删除），满了返回 409 `SNAPSHOT_LIMIT_REACHED`（`details.limit/stored`）。检查是先计数再插入，并发请求可能略微超出，只是存储护栏，不是配额；`POST` 不幂等（重试会多一个快照）。条件集版本和确认记录尚无上限。
- 快照列表无游标（最新在前、最多 100 条）；同一秒内的快照顺序不保证（MySQL 整秒精度，见 CLAUDE.md 第 23 条）。
- 没有“检查名称”（设计提到 `ci_passed_for_commit` 可带检查名）：平台 pipeline 记录没有逐检查名，未虚构该字段。
- 无 Web UI。

## 验证

| 层级 | 结论 |
|---|---|
| 单元 | ✅ `rg-core` `collaboration::conditions`：参数闭合、`overall` 不把 unknown/stale 当作满足 |
| 任务验证（SQLite） | ✅ `cargo test -p rg-http --test collab_condition_tests`：10 项（9 项连跑 3 遍通过，快照上限一项另跑 1 遍）（无定义时只有推导评估；版本/幂等/冲突/历史；非法定义逐项 400；问题与工作包随上下文变 stale；CI 绑定确切 commit 并随分支移动变 stale；确认的显式、幂等与分支绑定；快照原样保留；读写权限；6 路并发写同一 `expected_version` 恰好 1 个成功；快照上限 200 与跨任务隔离） |
| PostgreSQL 17 / MySQL 8.4（HTTP 层） | ✅ 全新库上同样 8 项通过，并发竞争项各连跑 3 遍通过（本机 Docker，迁移含复合外键也在两库上建表成功） |
| 防漂移 | ✅ `collab_contract_tests`、`rg-mcp` 测试（新增 `completion_conditions` 能力与两个 MCP 工具已登记） |
| 迁移回滚窗口 | ✅ `rg-db` `soft_delete_tests` 窗口 17 → 18 |
| 强制模式 / Web UI / 真实 Agent 使用 | ❌ 未做 |
| 合并后集成 / 远端 CI | ❌ 未验证 |
