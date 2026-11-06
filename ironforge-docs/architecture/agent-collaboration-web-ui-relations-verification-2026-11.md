# 协作 Web UI：依赖增删、用户重指派、接手记录、预算与用量：实现与验证记录

> 日期：2026-11-06。分支 `claude/web-relations-b15ea5`（基于 `feat/web-conditions` @ `83424de`）。**可信度**：确认（逐层标注，未验证的明说）。

## 范围

任务详情页（`/{owner}/{repo}/collaboration/tasks/{id}`）新增四块，每块一个组件，只调用**不需要会话凭据**的端点（UI 从不发送 `X-Ironforge-Session*`，`collabRelations.test.ts` 断言请求头里没有）。没有改 Rust，没有改 `collaboration.ts`、`CollabNav`、协作首页和 `CLAUDE.md`。

| 功能 | 组件 | 端点（用户门） |
|---|---|---|
| 依赖增删 | `DependenciesEditor.svelte`（替换页面里原只读依赖块的内部，保留原行为与措辞） | `POST …/tasks/{id}/dependencies`、`DELETE …/dependencies/{depends_on_id}` |
| 用户重指派 | `ReassignForm.svelte` | `POST …/tasks/{id}/reassign`；目标会话来自 `GET …/sessions`（需写权限） |
| 接手/重指派记录（只读） | `TakeoversCard.svelte` | `GET …/tasks/{id}/takeovers`（游标分页） |
| 预算与用量 | `BudgetCard.svelte` | 读：`GET …/tasks/{id}/budget`、`…/usage`；写：`PUT …/tasks/{id}/budgets`、`POST …/tasks/{id}/usage` |

新增文件：`web/src/lib/api/collabRelations.ts`（类型与客户端）、`web/src/lib/utils/collabRelations.ts`（纯规则：错误解释、金额/令牌文法、时间转换）、四个组件及其测试、i18n 顶层新键 `collab_relations`（en / zh-CN 各 +146 个，其余键未动）。页面只加了 4 行 import、`refreshTask()`（无加载态的任务重读，使重指派后下方各块保留表单状态）和三个 `SectionCard`（重指派、接手记录、预算），依赖块内部换成组件。

## 契约要点（读后端代码得出）

- **依赖**：写入带 `expected_graph_version`（取自当前列表的 `graph_version`）；服务端 409：`VERSION_CONFLICT`（重读）、`DEPENDENCY_CYCLE`、`GRAPH_LIMIT_EXCEEDED`（失败即关闭）、`DEPENDENCY_EXISTS`、`IDEMPOTENCY_KEY_REUSED`；每任务最多 100 条直接依赖（达到上限不再提供添加）；删除幂等（边已不存在返回 `removed:false`，UI 说明"已经不存在"）。`DELETE` 需要 JSON body（客户端按此发送）。`blocked_by` 仍是由声明派生，措辞沿用「声明未完成」。选择器用 `collaboration.listTasks`（每页 100，可加载更多），排除任务自身与已有依赖，每项显示**声明**状态。
- **重指派**：任何对仓库有写权限的人可做；`reason` 必填、至多 2000 **字节**（UI 按字节数校验）；带 `expected_task_version` 与 `expected_owner_session_id`（无主为 `null`）；目标必须是存活会话（未结束、未过期；在线或离线均可），所以选择器只列 `online/offline` 且不含当前负责会话。**做什么**：改声明的负责会话、写不可变历史、通知原负责会话。**不做什么**：不动待处理交接、不授予 Git/CI/审批权限、不停止或启动工作 —— 页面原文写明。`GET …/sessions` 需写权限，403 时说明原因并不提供目标选择。
- **接手记录**：创建时间升序；`actor_kind` 只是所用入口，不是身份证明；理由是声明。
- **预算**：服务端对 `PUT budgets` 与 `POST usage` **有用户门**（`write_actor` 在无会话头时用登录账号），所以 UI 提供两个小表单。预算写入用 `expected_version` 做乐观并发（无幂等键）：表单按已加载摘要里同单位+来源的预算取 `version`（无则 0）；用量上报带新的 `client_usage_id`，失败后换新键。单位/来源按服务端文法校验（`[A-Za-z0-9._:/@+-]`，1–32/1–64，区分大小写，从不换算）；金额按字符串发送（≤18 位整数、≤6 位小数，预算须 > 0）。显示：用量缺失一律为"未知"（从不显示 0），不同单位/来源分别列出且**不求和**，无对应预算的用量单列为"没有预算的用量"，阈值只显示判断依据与数字，页面明说"不会暂停或停止任何工作"。

## 验证（逐层，前层不替代后层）

### 实现证据
上述文件，见本文件所在提交（`git log -- web/src/lib/components/collaboration/DependenciesEditor.svelte`）。

### 任务验证（分支上的当前命令，2026-11-06，均在 `web/` 内）
| 命令 | 结果 |
|---|---|
| `pnpm run check` | ✅ 0 errors，45 warnings（与基线相同，均为既有文件的 `state_referenced_locally`；新增文件无 warning） |
| `pnpm run test` | ✅ 151 个测试文件 / 802 用例通过（本次新增 6 个文件 / 46 用例） |
| `pnpm run build` | ✅ adapter-static 产出 `web/build` |
| `node ../scripts/i18n-key-check.mjs` | ✅ 1810 静态键、21 动态前缀，en = zh-CN = 1614 |

新增测试覆盖：恶意标记（`<img onerror>`、`<script>`）在任务标题/会话名/理由/单位/来源里渲染为文本且 `window.__pwned` 未被设置；添加依赖带读到的图版本与新键，被拒后重试换新键（循环、`VERSION_CONFLICT`、403 各一例，冲突触发重读）；选择器排除自身与已有依赖、分页；删除幂等与失败换键；100 条上限无添加入口；重指派的目标过滤、必填理由、2000 字节上限、期望版本/负责会话（含无主 `null`）、失败换键、冲突重读、会话列表 403 的说明；接手记录空态、分页、`refreshKey` 重读；预算的未知≠0、多单位不求和、阈值、记录列表、新建（版本 0）与替换（带版本）、非法金额/令牌拦截、冲突重读、用量上报换键与 ISO 时间；客户端动词、路径编码、`DELETE` 带 body、无会话头、错误码透出；纯规则函数。

### 运行验证（本机真实服务 + 内置浏览器，SQLite；窗格隐藏，故未截图，通过 DOM 文本核对）
方法：用预编译的 `target/debug/ironforge`（未重新 `cargo build`）。`python3 scripts/collab-e2e.py --scenario p1 --keep` 造数，再用 REST 追加：一个名称含 `<img onerror>` 的会话、三个任务（其中一个标题含 `<b>`）、只读协作者 `reader`。`pnpm run build` 后从 worktree 根启动 `serve`（127.0.0.1:38201），用 `http://localhost:38201` 登录 `collab-e2e` 操作。实际看到并核对的：

- 依赖（zh-CN 与 English 均看过）：选择器列出"除自身外的所有任务"并带"（声明）"状态；添加成功后列表与图版本 1→2；**循环被拒**（在被依赖任务上反向添加）显示"这会让任务相互循环等待，因此被拒绝。— …(DEPENDENCY_CYCLE)"；页面打开后用 REST 抢先改图版本，再点移除 → 显示 `VERSION_CONFLICT` 解释 + "已重新加载"，列表刷新为版本 4 且**没有**误删；重读后移除成功。
- 重指派：当前负责会话被排除；含 HTML 的会话名与理由原样显示为文本；提交后负责会话立即更新（无需刷新页面），接手记录出现"由用户重指派 … → …、经 user 入口 id 1、任务版本 1→2、理由"，`<script>` 没有执行（`window.__pwned` 未定义）。
- 接手记录：升序、含理由与版本；分页由单测覆盖，运行中只有 1 条。
- 预算：未上报用量显示"未知…不是零"与"未判断（用量未知）"；用 UI 上报 USD 22.5 后显示"已用 22.5 USD … 达到阈值（阈值 20 USD，来源 billing-api）"；另报 `tokens/llm 1200`（无对应预算）出现在"没有预算的用量"，**未并入** USD；点"编辑"后替换提示"已有预算（版本 1）"，保存后版本 2；未来的 `period_end` 被服务端拒绝（400，消息原样显示）。
- 权限被拒：以只读协作者 `reader` 登录：重指派表单提示"列出会话需要写权限，无法选择目标"且无选择器、提交禁用；添加依赖被拒，显示"你没有权限…写权限 — write access denied (FORBIDDEN)"；预算与接手记录**可读**。REST 复核：`reader` 对 reassign / 添加依赖 / 设预算均 403，读接口 200，`GET sessions` 403。

### 未验证 / 局限
- 窗格隐藏，**没有截图，无法核对视觉布局、间距、移动端宽度**（只核对了 DOM 文本与行为）。
- 仅 SQLite；未在 PostgreSQL/MySQL 上跑。
- 接手记录分页、依赖选择器分页、预算用量记录分页只由单测覆盖，运行中数据不足以出现第二页。
- 冲突场景只在依赖上做了运行验证；重指派与预算的 `VERSION_CONFLICT` 只有单测。
- `GRAPH_LIMIT_EXCEEDED`、`DEPENDENCY_EXISTS`、`BUDGET_LIMIT_REACHED`、`USAGE_LIMIT_REACHED` 的解释文案只由单测/静态键检查覆盖，未在真实服务触发。
- 重指派对**原负责会话的通知**（`TASK_REASSIGNED_AWAY`）只读代码得知，未从会话侧观察。
- 未测试宽屏之外的 `datetime-local` 在各浏览器的输入差异；本地时间按浏览器时区转 UTC ISO。

## 发现与注意事项

1. **`DEPENDENCY_CYCLE` 的 `details.cycle_path` 前端拿不到**：共享的 `ApiError`（`_base.svelte.ts`）只保留 `code`，不带 `error.details`；本分支按约定不改它，所以循环被拒时只显示解释与服务端消息，不画出环路径。若要显示，需要扩展 `ApiError` 保留 `details`。
2. **同一主机名下的 Cookie 在端口之间共享**：多个会话同时在 `127.0.0.1` 的不同端口起服务并用同一个 JWT 密钥（`collab-e2e-jwt-secret-2026`）登录时，浏览器里的登录 Cookie 会被别的会话互相覆盖，用户 id 恰好对得上时表现为"突然变成另一个账号"（我这里变成了只读的 `reader`，id=2）。验证时改用 `http://localhost:38201`（独立 Cookie）规避。这是测试环境问题，不是产品缺陷。
3. 重指派要求目标会话"存活"，离线（漏过心跳）的会话也算存活；这是服务端的有意行为（`require_live_owner` 只看 `ended_at`/`expires_at`），UI 同样列出离线会话并标注状态。
4. 用量的 `period_end` 不能晚于当前时间（服务端 400）；UI 在用量提示里写明，不做客户端时钟校验。
5. 测试里 jsdom 的 `select:checked` 与 Svelte 的 `bind:value`（select）不兼容（读到 `null`）；两个选择器因此用 `value` + `onchange` 的显式写法，行为在真实浏览器里同样正确。
