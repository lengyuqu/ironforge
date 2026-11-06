# 协作 Web UI：决定 / 用户提问 / 工作包（任务详情页）验证记录

> **状态**：首版 ｜ **可信度**：见各层 ｜ **日期**：2026-11-06 ｜ **范围**：`web/` 前端，不含 Rust 改动

任务详情页（`web/src/routes/[owner]/[repo]/collaboration/tasks/[id]/+page.svelte`）新增三块，均只用登录身份、**不发** `X-Ironforge-Session` / `-Secret`：

| 功能 | 组件 | 端点 |
|---|---|---|
| 记录 / 取代 / 撤销决定 | `DecisionPanel.svelte` | `POST …/tasks/{id}/decisions`、`POST …/decisions/{id}/changes` |
| 用户发起提问 | `NewQuestionForm.svelte` | `POST …/tasks/{id}/questions` |
| 工作包（只读） | `WorkPackagesCard.svelte` | `GET …/tasks/{id}/work-packages`、`GET …/work-packages/{id}` |

API 客户端在新文件 `web/src/lib/api/collabDecisions.ts`，纯函数在 `web/src/lib/utils/collabDecisions.ts`，i18n 全部在新顶层键 `collab_decisions`（en / zh-CN 键集一致）。

## 后端契约要点（读 `collab_decisions.rs` / `decision.rs` / `collab_questions.rs` / `question.rs` / `work_package.rs` 所得）

- **用户门存在**：`write_actor` 在没有会话头时把写入记为 `Actor::user`，因此提问不需要绕路；未做任何会话密钥流程。
- 决定不可变；每条决定**只接受一次**终结变更（`superseded` / `revoked`），数据库唯一键仲裁，第二次为 409 `DECISION_ALREADY_CHANGED`。
- `superseded` 必须带 `replacement_decision_id`，且替代决定同任务、`seq` 更大（否则 409 `REPLACEMENT_NOT_NEWER`，成环为 `DECISION_CYCLE`）；`revoked` 禁止带替代项。服务器**不**检查替代决定自身是否仍有效，UI 只提供“更新且有效”的候选（这是 UI 的选择，不是服务器规则）。
- 创建决定/提问的版本 CAS 是 `context.task_version`：不等于任务当前版本 → 409 `VERSION_CONFLICT`，`error.details.current_version` 给出当前版本。
- 决定的 `context_freshness` 为 `stale` 时**不会**自动撤销。
- 工作包正文全是作者声明；只有 `evidence_resolution`（仅 `pipeline` 类引用）是平台查询；commit / url / note 引用不被检查。列表项不带 `evidence_resolution`（仅单条读取有）。

## 层级 1 — 实现证据（确认）

新增/修改文件见提交。任务页只加了三行 import 与三个 `SectionCard`（外加给问题表单传 `sessions`）；`collaboration.ts`、`CLAUDE.md`、`CollabNav.svelte`、协作首页未改。i18n 改动是在 `collab` 之前插入新顶层块，纯新增（0 行删除）。

关键设计：
- 决定面板：只列出**有效**决定供取代/撤销，只读列表仍由页面原有部分负责；取代/撤销前走确认对话框（终局操作）；理由与标题分别按服务器规则（标题 1–200 字符、理由/原因按字节）预校验。
- 每次提交用新的 `newClientId()`；**任何失败之后**换新键；`VERSION_CONFLICT`、`DECISION_ALREADY_CHANGED`、`DECISION_CYCLE`、`REPLACEMENT_NOT_NEWER` 之后重读决定列表，并重读任务版本。`DECISION_ALREADY_CHANGED` 同时收起表单。
- 所有客户端写入的字段（标题、理由、选项、会话名、工作包各字段）一律作为文本节点渲染；声明标注为声明；决定被说明为“记录而非批准”。
- 工作包：`wp-declared`（作者声明，虚线框）与 `wp-verified`（平台核实，实线框）是两个互不嵌套的块；唯一的“合并点”是一个客户端比较提示（流水线实际运行的提交与作者声明的提交不同），它只比较两个已记录的值。
- 注意（环境）：Svelte 5 在本仓库 jsdom 下 `<select bind:value>` 不响应 `fireEvent.change`，因此四个下拉改为 `value` + `onchange` 显式写法；浏览器中行为相同。

## 层级 2 — 任务验证（本分支，确认）

在 `web/` 下：

| 命令 | 结果 |
|---|---|
| `pnpm run check` | 850 FILES 0 ERRORS 45 WARNINGS（本次新增文件无告警；其余为既有） |
| `pnpm run test` | 150 个测试文件、795 个测试全部通过 |
| `pnpm run build` | 成功（adapter-static） |
| `node ../scripts/i18n-key-check.mjs` | ok：1753 静态键，24 动态前缀，en = zh-CN = 1586 |

新增测试：`DecisionPanel.test.svelte.ts`（12）、`NewQuestionForm.test.svelte.ts`（7）、`WorkPackagesCard.test.svelte.ts`（9）、`collabDecisions.test.ts`（API，5）、`utils/collabDecisions.test.ts`（6）。每个组件含：恶意标记按文本渲染、失败后重试换新键、服务器错误原文展示；决定面板另含 `VERSION_CONFLICT` 重读版本、`DECISION_ALREADY_CHANGED` 关表单、仅提供更新且有效的替代项、确认被拒不发请求；提问含数值用户 ID / 会话必选、权限错误保留草稿；工作包含声明与核实分块不混合、提交不一致提示、`unavailable` 不当作失败。

## 层级 3 — 运行验证（SQLite、内置浏览器，窗格隐藏 → 以 DOM 文本为证，无截图）

环境：预编译的 `target/debug/ironforge`，`scripts/collab-e2e.py --scenario p1 --keep` 造数据，`pnpm run build` 后由 `ironforge serve` 在 `127.0.0.1:38101` 提供 `web/build`；账号 `collab-e2e`（所有者）与 `reader`（只读协作者，经 REST 加入）。页面和 API 的结果均经 `read_page` / `javascript_tool` 读取。

已实际执行并观察到：
- **记录决定**：选择已答复的问题并勾选“引用其答复”，保存后 API 返回 `actor_kind=user`、`question_id` 与 `answer_id` 均已写入、`context.task_version=3`；页面列表刷新。标题 `Use <i>Postgres</i> …` 在页面上按字面文本显示（无 `<i>` 元素）。
- **冲突**：在 UI 取代表单已打开并确认前，用 REST 先撤销同一条决定；UI 确认后显示服务器消息“decision was already revoked (DECISION_ALREADY_CHANGED)”，列表重读后该决定显示“已撤销”，表单关闭。
- **取代**：用 #3 取代 #2（候选下拉只含 #3），确认对话框后成功，列表显示“已被取代”及原因。
- **撤销**：撤销 #3，提交按钮在原因为空时禁用，确认对话框后成功，toast “决定已撤销”，面板显示“没有可更改的有效决定”。
- **过期决定**：已有决定（task_version 1，任务现为 3）显示“上下文已过期”及“在有人取代或撤销之前它仍然有效”，仍可操作。
- **用户提问**：带两个选项、收件人“任何有写权限的人”提交后，API 中 `creator_kind=user`、`recipient.kind=repo_writers`、选项 `API first / UI first`、`context.task_version=3`、状态 open；问题列表刷新。标题中的 `<b>` 按文本显示。
- **工作包**：展开后“作者声明”块列出目标、已完成（含恶意 `<img …onerror>` 字面文本，页面无 `img`/`b` 元素）、证据引用“流水线 999 / 备注 trust me”；“平台核实”块独立显示“流水线 999 · 未找到”及说明，且声明块里的流水线引用旁没有任何状态。
- **权限拒绝**：以只读协作者登录后，提问与决定提交均显示“write access denied (FORBIDDEN)”，草稿保留；只读用户仍能读到工作包。
- **文案**：zh-CN 与 en 的三块文案均实际读到（en 由 `localStorage.locale='en'` + 刷新得到）。

测试结束后：停止服务器、删除本次工作目录；浏览器登录身份恢复为 `collab-e2e` 并清除 `locale` 项。

## 未验证（不得当作已通过）

- **服务器端 `VERSION_CONFLICT` 在 UI 里的路径**：REST 直接验证了 409 与 `details.current_version`，组件测试验证了重读版本与换新键；但真实浏览器中没有构造出“任务版本在表单打开期间变化”的场景（改任务需要会话凭据，不属于用户门）。
- **`REPLACEMENT_NOT_NEWER` / `DECISION_CYCLE` / `IDEMPOTENCY_KEY_REUSED`**：仅有组件测试与后端文档，UI 里没有走过。
- **工作包的“平台已核实”分支（`verified_reference`）**：本环境没有 CI 流水线行（流水线只由 CI 引擎写入），所以只在组件测试里用模拟数据验证；真实环境中只看到 `unavailable`。
- **向指定用户 / 指定会话提问**：只有组件测试；浏览器里只实际提交了 `repo_writers`。服务器对 `user` 收件人只检查是正整数、不检查账号是否存在，对此 UI 只做同样的格式校验。
- 窗格隐藏，无截图 → **视觉样式（间距、颜色、窄屏布局）未人工检查**。
- 仅 SQLite；未跑 PostgreSQL / MySQL。
- 对话框焦点与键盘可达性、屏幕阅读器未验证。
- CI 未跑（负责人说明暂不关注）。

## 给后端/产品的观察（不是缺陷结论）

- 没有“用户侧”的任务改版本端点，因此 UI 无法触发 `VERSION_CONFLICT`，也无法在页面里让用户“刷新到最新版本”以外的动作；这是预期的用户门边界。
- 页面上既有的只读决定列表对终结变更只显示 `kind: reason`，不显示替代决定是哪一条；`DecisionView` 返回的 `change.replacement_decision_id` 目前未被只读列表使用（不在本切片范围）。
- 验证英文时，用脚本点“切换语言”后 `<html lang>` 变为 `en` 但页面文本仍是中文；改用 `localStorage.locale='en'` + 刷新才得到英文。原因未调查（可能是脚本点击方式所致），不在本切片范围。
