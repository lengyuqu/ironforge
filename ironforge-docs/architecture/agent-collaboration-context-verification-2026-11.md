# 协作 U05：任务上下文包（实现与验证记录）

> 日期：2026-11-05。分支 `feat/collab-remaining`。设计依据：[P1 设计 §4](agent-collaboration-p1-design-2026-11.md)。**可信度**：确认（逐层标注）。

## 已实现（实现证据）

`GET /api/v1/repos/{owner}/{name}/collaboration/tasks/{id}/context?max_bytes=&sections=`（仓库读权限）；MCP 工具 `collab_get_context`。

- **section 与优先级**：`task`（始终第一、始终返回）→ `questions`（开放问题）→ `decisions`（`active` 决定）→ `handoffs`（`pending`）→ `work_packages`（最新 3 个）→ `dependencies` → `overlaps` → `messages`（最新 20 条）。`sections` 只能选后 7 个，输出顺序固定，与请求顺序无关；未知名字（包括 `task`）返回 400。
- **字节预算**：`max_bytes` 默认 32768，范围 4096–262144，越界 400。限制的是**序列化后整个响应**的字节数；超限时从最低优先级 section 末尾逐条丢弃（`messages` 丢最旧的，保留最新的连续尾部），每次丢弃记入 `truncation.sections`（`omitted_items`、`reason: byte_budget`），该 section `has_more=true` 并给出 `continue_with`。
- **字段截断**：单个顶层文本字段超过 2 KiB 时在字符边界截断，并列入 `truncation.fields`（`object_id/field/original_bytes/kept_bytes`）。条目被整体丢弃后，其字段截断记录一并移除。
- **预算过小**：连同任务本身也放不下时返回 400 `BUDGET_TOO_SMALL`，`details.needed_bytes` 给出所需字节数。
- **来源标注**：每个条目带 `origin: user_content`；有独立读取端点的对象带 `locator`（如 `/tasks/{id}/questions/{qid}`）；每个 section 带 `source: db_live_query`、`captured_at`、`returned_count`。
- **一致性（如实）**：`consistency = {db: per_section_live_query, git: independent}`。各 section 是独立的实时查询，**没有**跨 section 快照；**没有** `next_cursor`，续读走各 section 的 `continue_with`（已有的分页接口）。分支头 `git.head_sha` 是对声明分支的独立观测，`git.status` 为 `observed` / `unavailable`（声明了分支但读不到）/ `no_branch`；读不到不补猜。

## 取舍 / 偏差

- 设计写“数据库一致性读快照”“cursor 绑定快照”：本实现**不做**，因为协作层没有共享快照机制，伪造一致性比明说更糟；`consistency` 字段和 `notes` 如实写明。N12 中“分页期间任务变化”因此只验证了“每次读取反映最新状态并给出 `task_version`/`as_of`”，**没有**验证跨页拼接一致。
- 不含摘要、不含 LLM；不含 `base_sha`（任务本身没有记录基线；问题的 `context` 里仍带自己的 `base_sha`/`head_sha`）。
- 决定和交接各最多扫描 100 条再过滤（`active` / `pending`），超出时 `has_more=true`。
- 条目内嵌套的长字符串（如数组里的元素）不单独截断，只靠整条丢弃控制总量。
- 多语言文本的 token 数不估算（只保证字节数）。

## 验证

| 层级 | 结论 |
|---|---|
| 单元 | ✅ `rg-core` `collaboration::context`：字符边界截断（0–30 字节逐个）、section 解析与固定顺序、预算范围 |
| 任务验证（SQLite） | ✅ `cargo test -p rg-http --test collab_context_tests`：9 项（默认 section 顺序与来源标注、开放问题/消息/分支头、读不到的分支、8 KiB 预算下整响应 ≤ 8192 字节且 UTF-8 完整并列出丢弃、长字段截断、预算过小、section 校验、任务版本随读取变化、无权限与跨仓库） |
| PostgreSQL 17 / MySQL 8.4（HTTP 层） | ✅ 全新库上同样 9 项通过（本机 Docker） |
| 防漂移 | ✅ `collab_contract_tests` 16 项、`rg-mcp` 测试通过（新增 `context_pack` 能力与 `collab_get_context` 工具已登记） |
| 跨 section 快照 / 分页期间并发变化 | ❌ 未实现，未验证 |
| 真实 Agent 用它恢复工作 | ❌ 未验证（U06 闭环） |
| 合并后集成 / 远端 CI | ❌ 未验证 |
