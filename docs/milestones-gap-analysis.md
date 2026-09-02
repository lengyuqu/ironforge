# Milestones 功能缺口调查报告

日期：2026-09-02
背景：R2 排查时发现 `web/src/lib/api/milestones.ts` 存在 4 处 `any`，且前端零里程碑 UI。本报告核实后端能力与前端缺口，供与后端协同确认。

## 一、结论（先说定性）

**这不是"后端没做"，而是"前端整个域缺失"。** 后端 milestones 能力完整且成熟（含 issue 联动、webhook、通知、导入），前端只有一层 API 封装在"空转"——没有任何页面、组件或表单字段消费它。

更值得注意的是：**导入链路会把里程碑数据真实写入数据库**（GitHub/GitLab 导入器的 `import_milestones` 开关默认开启），即用户从 GitHub 导入仓库后，库里已有里程碑数据，但前端无处可见、不可管理——这是用户可感知的功能断层，优先级应高于普通"补 UI"。

## 二、后端能力盘点（已实现）

### 2.1 REST API（5 端点齐全）

路由注册：`crates/rg-http/src/lib.rs:599-606`，handler 位于 `crates/rg-http/src/api/issues.rs:1166-1460`：

| 端点 | 方法 | 说明 |
|------|------|------|
| `/repos/{owner}/{name}/milestones` | GET | 列表，支持 `?state=` 过滤（open/closed） |
| `/repos/{owner}/{name}/milestones` | POST | 创建（title/description/due_date/state） |
| `/repos/{owner}/{name}/milestones/{id}` | GET | 单个查询 |
| `/repos/{owner}/{name}/milestones/{id}` | PATCH | 部分更新（state 校验 open/closed） |
| `/repos/{owner}/{name}/milestones/{id}` | DELETE | 删除 |

### 2.2 数据层

- 实体 `crates/rg-db/src/entities/milestone.rs`：`id / repo_id / title / description / state("open"|"closed") / due_date / created_at / updated_at`
- ops `milestone_ops.rs`：find_by_id / list_by_repo / create / update / delete_by_id / **count_open_by_milestone**（进度联动专用）

### 2.3 Issue 深度集成（前端完全未接入）

- `CreateIssueRequest.milestone_id`（issues.rs:26）→ 创建 issue 时可指定里程碑
- `UpdateIssueRequest.milestone_id: Option<Option<i64>>`（issues.rs:45）→ 支持设置与**清除**（None 外层=不改，内层 None=置空）
- **自动完成检测**（issue/service.rs:274-284）：关闭 issue 时若该里程碑下 `count_open_by_milestone == 0`，自动触发 `milestone.closed` webhook + watcher 站内通知（`notify_milestone_closed`）

### 2.4 周边联动

- Webhook：`milestone.closed` 事件已注册，前端 WebhookForm 事件选择器里也提供该选项（用户能勾选一个永远不会再收到的事件——因无 UI 关里程碑，但通过 issue 全关闭间接触发是通的）
- 导入：GitHub/GitLab 导入器支持里程碑导入（`import_milestones`，ImportForm 默认勾选）——**数据可入库**
- PR 侧：`pull_request/service.rs:89` 创建 PR 时 `milestone_id: Set(None)`——字段有占位但无任何 API 设置入口

## 三、前端缺口盘点

| 项 | 现状 | 位置 |
|----|------|------|
| API 封装 | 5 端点齐全但返回值全是 `any`（4 处欠账） | `web/src/lib/api/milestones.ts` |
| 消费方 | **零**。grep 全部 routes/components 无命中（仅 ImportForm 导入开关、WebhookForm 事件选项两处边缘提及） | — |
| Issue 创建表单 | 无里程碑选择器（后端支持 milestone_id） | `IssueCreateForm.svelte` |
| Issue 编辑/详情 | 无里程碑展示与切换（后端支持 PATCH milestone_id） | `issues/[number]` |
| Issue 列表过滤 | 无里程碑筛选 | `IssueFilterTabs` |
| 里程碑管理页 | 不存在（列表/创建/编辑/关闭/删除全无） | — |
| entities.Milestone | 已定义但**零使用**，且形状是 GitHub 风格（open_issues/closed_issues/due_on），与后端 `milestone::Model` 不符（无计数、字段名 due_date）——实现时需按后端重写 | `entities.ts:449` |

## 四、后端待协同确认项

1. **Issue 列表不支持里程碑过滤**：`ListQuery`（issues.rs:69）只有 state/labels/assignee/pagination，无 `milestone` 参数（GitHub API 有）。补 UI 前建议后端加上。
2. **里程碑响应缺进度计数**：list/get/create/update 返回 `milestone::Model` 裸 JSON，无 open_issues/closed_issues 计数。前端里程碑列表要展示进度条的话，需要后端 enrich（`count_open_by_milestone` 已有，补个 closed 计数即可），或前端逐个 milestone 拉全量 issue 自行统计（不推荐）。
3. **issue 响应不回填 milestone 标题**：Issue 响应只有 `milestone_id`，前端展示需二次查询或后端 enrich title。
4. **PR 里程碑定位**：PR 有字段占位但无设置入口，需确认是否计划支持（GitHub 支持 PR 归属里程碑）。
5. **里程碑删除策略**：当前 `delete_by_id` 直接删。关联 issue 的 milestone_id 是否置空？建议确认（前端做管理页时需要明确删除后果提示）。

## 五、建议的前端实施路径（供参考，待协同确认后排期）

按依赖顺序，可拆为两个批次：

**M1 类型化 + 数据层就绪（速赢，无 UI，半天级）**
- 按后端 `milestone::Model` 重写 `entities.Milestone`，`milestones.ts` 4 处 any 类型化（与既有 R 批次同套路，无需后端配合）

**M2 里程碑 UI（需后端先确认四、1/2/3 项）**
- 里程碑管理页（列表+进度+创建/编辑/关闭/删除）——建议挂在仓库 tab 或 issues 域下
- IssueCreateForm 加里程碑选择器；issue 详情加里程碑展示与切换
- IssueFilterTabs 加里程碑筛选（依赖后端 ListQuery 扩展）
