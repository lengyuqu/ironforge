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


## 六、M2 实施准备清单（2026-09-02 补）

### A. 后端协同确认项（开工前闭环）

| # | 事项 | 阻塞范围 | 无后端配合时的降级方案 |
|---|------|---------|----------------------|
| A1 | Issue 列表 `ListQuery` 增加 `milestone` 过滤参数（issues.rs:69，现仅 state/labels/assignee） | ✅ 已实现（2026-09-03）：`MilestoneFilter` 枚举（Id/Any/None），query 语义 `{id}`/`none`/`*`，非法值 400 | 前端仅对当前页数据过滤，或暂不做筛选 |
| A2 | 里程碑响应 enrich open/closed 计数（`count_open_by_milestone` 已有，closed 计数需补） | ✅ 已实现（2026-09-03）：`counts_by_milestones` 分组计数；`MilestoneResponse`（flatten + open_issues/closed_issues），list/get/create/update 全走 enrich | 列表不显示进度，仅显示状态徽章 + due date |
| A3 | issue 响应回填 milestone 标题（现只有 milestone_id） | ✅ 已实现（2026-09-03）：`titles_by_ids` 批量回填（单页一次查询，无 N+1），`milestone_title` 仅在有值时序列化 | 前端一次性拉 milestones.list 建 id→title 映射（仓库级缓存） |
| A4 | PR 里程碑定位（字段占位无入口） | 不阻塞 | M2 不做，留待定位确认 |
| A5 | 里程碑删除时关联 issue 的 milestone_id 处理策略 | ✅ 已实现（2026-09-03）：`delete_cascade` 事务内 detach issue/PR 再删里程碑（GitHub 语义）；get/update/delete 增加跨仓库 404 守卫 | confirm 文案写"关联 issue 的里程碑关联将被移除"（已确认） |

**关键结论：A1–A5 均不阻塞 M2 第一阶段**——管理页 CRUD 和 issue 挂载/摘除用现有裸 Model API 即可完整交付。

### B. 数据层（就绪状态）

- ✅ `entities.Milestone` / `CreateMilestoneInput` / `UpdateMilestoneInput`（M1，提交 c5a9d98）
- ✅ `milestones.ts` 5 端点全类型化，client 聚合导出
- ✅ A2 已确认并落地：`Milestone` 加可选 `open_issues`/`closed_issues`（后端 MilestoneResponse flatten 直供，无需前端独立 `MilestoneWithCounts` 类型）
- ✅ A3 已确认并落地：`Issue.milestone_title?: string | null` 可选字段，IssueList 徽章直读

### C. UI 范围与组件拆分方案

**参照 labels 域既有模式**（LabelGrid 纯展示 + LabelFormModal/LabelDeleteModal 自包含）：

| 组件 | 模式 | 职责 |
|------|------|------|
| `MilestoneGrid` | 纯展示 | 状态徽章（open/closed）、due date、描述截断；✅ 进度条已启用（M2-2，读 enrich counts） |
| `MilestoneFormModal` | 自包含 | 创建/编辑双模式（参照 MirrorForm 惯例）；due_date 用 datetime-local 输入转 RFC 3339 |
| `MilestoneDeleteModal` | 自包含 | confirm（文案依赖 A5 结论） |
| 管理页 +page.svelte | 编排层 | 目标 ~80 行（labels 页同规模） |

**issue 域联动改动**（改既有组件，不新建）：

- `IssueCreateForm`：里程碑下拉选择器（milestones.list 自加载或 props 注入，映射走 A3 降级方案）
- issue 详情页：里程碑展示与切换面板（与 AssigneesPanel 同级侧栏，PATCH milestone_id 含清除语义）
- `IssueFilterTabs`：✅ 里程碑筛选 chip 已实现（M2-2，A1 语义 `''`/`none`/id，issues 列表页工具栏第二行）

### D. 路由与入口

- 管理页：`web/src/routes/[owner]/[repo]/settings/milestones/+page.svelte`
- 入口：`settings/+layout.svelte` 子导航追加一项（labels 在第 26 行，紧随其后）
- 事件：`milestone.closed` webhook 已有；issue 全关闭自动触发链路后端已实现，验收时覆盖

### E. i18n

- `translations/zh-CN.json` / `en.json` **现无任何 milestone 键**（已核实），需新增 `settings.milestones` 及组件文案键组
- 近期教训：直接补齐键，不走 fallback 降级

### F. 验证与验收场景

- 常规三项：svelte-check 0 errors（warnings ≤37 基线）· vitest 13/13 · vite build
- 功能验收：创建/编辑/关闭/删除里程碑；issue 创建时挂载、详情页切换/摘除；关闭里程碑下全部 issue → 验证 milestone.closed webhook 与通知
- 可选：MilestoneFormModal 日期转换单测

### G. 排期建议

- **M2-1（无后端依赖，可立即开工）**：管理页三组件 + settings 入口 + i18n 键 + IssueCreateForm/详情页里程碑挂载
- **M2-2（依赖 A1/A2/A3）**：里程碑筛选 chip + 进度条 + issue enrich 直读 —— ✅ 已完成（2026-09-03）
