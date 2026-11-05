# 协作 U03：声明路径重叠预警（实现与验证记录）

> 日期：2026-11-05。分支 `feat/collab-remaining`。设计依据：[P1 设计 §5](agent-collaboration-p1-design-2026-11.md)。**可信度**：确认（下表逐层标注）。

## 已实现（实现证据）

- 迁移 `m20261105_000001_add_collab_task_paths_declared_at`：`collab_tasks.paths_declared_at`（可空时间戳）。`up` 用 `has_column` 守卫，`down` 为空（SQLite 不能可靠删列；可空列对旧代码无害）。已有行保持 `NULL`，**不**用 `updated_at` 回填。
- 写入点：创建任务/从模板建任务时若 `paths` 非空则写创建时间；`update_cas` 仅当请求携带 `paths` 时写 `now`。改标题、状态、负责人等不会刷新。
- `GET /api/v1/repos/{owner}/{name}/collaboration/tasks/{id}/overlaps?limit=&cursor=`：仓库读权限；候选是同仓、非 `done`、声明过路径的其他任务；按 task id 升序 keyset 分页（HMAC 游标，绑定仓库+任务），单次最多扫描 2000 行，因此页可能偏短但仍带 `next_cursor`。
- 匹配：仓库相对字面量、按字节比较（区分大小写）；相等或在 `/` 边界上互为目录前缀才重叠，忽略末尾 `/`；glob 当字面文本；每个任务最多列 20 对并标 `overlaps_truncated`。
- `confidence` 只有 `declared_current`（声明在 24 小时内）和 `declaration_stale`（更旧**或时间未知**）；另给 `owner_state`（online/offline/expired/ended）。响应带 `authority: hint` 和 `not_covered`。
- `TaskResponse` 新增 `paths_declared_at`；MCP 新增 `collab_get_overlaps`（共 55 个 `collab_*` 工具）；能力注册表新增 `overlaps`。
- 同时修了 `DiagnosticsQuery` 缺 `into_params(parameter_in = Query)`（OpenAPI 漏参数，防漂移测试发现）。

## 取舍 / 偏差

- 24 小时阈值是常量，未做成配置。
- 偏短页：为保持有界扫描，不保证每页满额；调用方必须一直跟随 `next_cursor`。
- 未实现稳定快照：游标只保证单次遍历不重复不漏已存在的行，遍历期间新增/改动的任务可能出现或不出现。
- 不比较分支是否相同；`branch` 只是被引用的声明。

## 验证

| 层级 | 结论 |
|---|---|
| 单元（匹配/置信度/上限） | ✅ `cargo test -p rg-core --lib collaboration::overlap`：4 项 |
| 任务验证（SQLite） | ✅ `cargo test -p rg-http --test collab_overlap_tests`：6 项（路径匹配、排除已完成/自身、声明时间只随 `paths` 变、未知时间不算 current、owner 过期、分页无重复无漏、伪造/跨任务游标 400、无权限读不到） |
| 防漂移 | ✅ `collab_contract_tests` 16 项（路由/OpenAPI/注册表/MCP）、`rg-mcp` 全部通过 |
| 迁移 | ✅ `rg-db` 27+12+5+15+1 项；`soft_delete_tests` 的回滚窗口从 15 调到 16（该测试注释要求每增一个迁移加 1） |
| PostgreSQL 17 / MySQL 8.4（HTTP 层） | ✅ 全新库上 `collab_overlap_tests`、`collab_notice_tests`、`collab_contract_tests` 通过（本机 Docker）。**不含**并发写入 `paths_declared_at` 的竞争场景 |
| 合并后集成 / 远端 CI / 真实多客户端 | ❌ 未验证 |
| 浏览器 / UI | ❌ 无 UI |
