# 协作 Web UI（首个面向人的界面）：实现与验证记录

> 日期：2026-11-05。分支 `claude/confident-dhawan-d509b7`（基于 `feat/collab-remaining` @ `578098f`，该分支含 U03 重叠、O02 总览、通知/诊断）。**可信度**：确认（逐层标注，未验证的明说）。

## 范围与原则

仓库级页面，入口为仓库导航新增的「协作」标签（`/{owner}/{repo}/collaboration`），任务详情为 `/{owner}/{repo}/collaboration/tasks/{id}`。

- **人用登录身份操作，不用会话密钥**：UI 从不发送 `X-Ironforge-Session*`（`web/src/lib/api/collaboration.test.ts` 断言请求头里没有）。
- **只做服务端已有的「用户侧」端点**，没有新增或绕过任何端点；没有改 Rust。
- **客户端写入的字段一律当文本**：标题、正文、分支、路径、会话名、决定理由、交接内容都用 Svelte 文本插值（无 `{@html}`、无 Markdown），长文本 `white-space: pre-wrap`。
- **声明就标声明**：状态显示「声明，未验证」，`blocked_by` 显示为「声明未完成」，重叠是「提示」，会话状态标「自报」，答复只记录协作决定、不批准合并。

## 已实现的页面（实现证据）

| 区域 | 内容 | 使用的端点 |
|---|---|---|
| 协作首页 | 「等待你处理」（含就地回答/取消）、概览（阻塞/待交接/进行中）、任务列表（状态过滤 + `next_cursor` 加载更多、负责会话与其状态） | `inbox/user`、`overview`、`tasks`、`questions/{id}`、`sessions` |
| 任务详情 | 标题/状态/负责会话与状态/分支/声明路径与 `paths_declared_at`/版本/关联 Issue·PR；完成状态；依赖；路径重叠；问题（可回答/取消）；决定；交接；消息时间线；诊断（按需运行）| `tasks/{id}`、`completion-status`、`dependencies`、`overlaps`、`questions`、`decisions`、`handoffs`、`messages`、`diagnostics?task_id=` |
| 回答/取消问题 | 从单题读取取得最新 `version` 与 `context_hash` 再提交；`client_answer_id` 每个表单只生成一次（重试回放而非重复）；`VERSION_CONFLICT`/`CONTEXT_STALE`/`QUESTION_NOT_OPEN` 后重读并换新 key；取消仅在「本人创建且仍开放」时出现 | `questions/{id}/answer`、`questions/{id}/cancel` |

代码：`web/src/lib/api/collaboration.ts`（客户端）、`web/src/lib/utils/collaboration.ts`（纯规则）、`web/src/lib/components/collaboration/{QuestionCard,StatusBadge,SectionCard}.svelte`、两个路由页、`RepoTabs.svelte`（新标签）、`_base.svelte.ts`（`ApiError` 新增可选 `code`，供 UI 区分 409 原因；向后兼容）、`i18n` en / zh-CN 各 +216 个键（en = zh-CN）。

## 与委托书的偏差与服务端行为发现

1. **基线不符**：委托书称本分支已含 U03/诊断提交，实际 worktree 在 `main`（56893d8）。已把本分支 **fast-forward** 到本地分支 `feat/collab-remaining`（578098f，未推送、未合并 main）后再开发。合并 main 前需注意这三个提交（U03、O02、诊断）也要一起进。
2. **服务端拒绝对 `stale` 问题作答**（`CONTEXT_STALE`，409，且问题上下文不可变）。因此 UI 对 `context_freshness = stale` 的开放问题**不显示回答表单**，只提示「请让 Agent 重新提问」。这是浏览器实测前我写错的地方（原先写成「回答前请确认」），已修正并加单测。
3. **取消只有创建者能做**：人只能取消自己（`creator_kind=user`）发起的问题，**不能**取消 Agent 提的问题；UI 据此显示/隐藏。
4. 确认对话框的危险样式默认按钮写「删除」，对「取消问题」有误导；已显式传 `confirmLabel`/`cancelLabel`。
5. 概览条目的 `state` 随 `object_type` 不同（任务/交接/问题）；UI 按类型选翻译前缀，未知值原样显示。

## 没有用户侧端点、因此**没有做**的动作（按委托书列出，不绕行）

读取这些需要会话凭据（`X-Ironforge-Session` + `-Secret`），人用登录令牌调用会被拒：

- **待处理通知** `GET …/collaboration/notices`、`POST …/notices/{id}/ack`、确认回执/投递状态：`list_notices` 在 `require_collab_read` 之后还要 `acting_session`。通知是发给某个会话的，故 UI 的「通知」卡片只说明这一点，**不显示通知**。委托书第 2 条里的「待处理通知」因此**未满足**；若要给人看，需要新增用户侧只读端点（例如按任务列出未确认通知，只含元数据），这属于后端范围。
- 创建/更新任务（`create_task`、`update_task`）、发消息、`mark_read`、创建/接受/取消交接、认领任务：全部要求 `acting_session`。

**有用户侧端点、但本阶段未做**（范围外，可作后续）：用户发起问题、新增/移除依赖、创建决定与变更决定（supersede/revoke）、用户重指派任务（`reassign`）、工作包只读列表、接手记录、模板、预算、声明、协作组、健康指标。

## 验证（逐层，前层不替代后层）

### 实现证据
- 文件见上；提交 SHA 见本文件所在提交（`git log -- web/src/routes/[owner]/[repo]/collaboration`）。

### 任务验证（分支上的当前命令，2026-11-05，均在 `web/` 内）
| 命令 | 结果 |
|---|---|
| `pnpm run check` | ✅ 0 errors（45 个 warnings 均为既有文件的 `state_referenced_locally`，协作新增文件无 warning）|
| `pnpm run test` | ✅ 144 个测试文件 / 747 用例通过（新增：`collaboration.test.ts` 12、`utils/collaboration.test.ts` 11、`QuestionCard` 10、`StatusBadge` 3，合计 36）|
| `pnpm run build` | ✅ adapter-static 产出 `web/build` |
| `node ../scripts/i18n-key-check.mjs` | ✅ 1644 静态键、18 动态前缀，en = zh-CN = 1458 |

单测覆盖的关键约束：不发送会话头、路径片段编码、游标/`after_seq` 分页、回答请求回显 `version` 与 `context_hash`、仅创建者可取消、stale 问题无表单、恶意标题/正文（`<img onerror>`、`<script>`）渲染为文本且 `window.__pwned` 未被设置、未知枚举值作为惰性文本显示、`ApiError.code` 透出。

### 运行验证（本机真实服务 + 内置浏览器，SQLite）
方法：用 worktree 的代码 `cargo build -p rg-cli`（debug）构建，`ironforge serve` 监听 127.0.0.1:18080（从 worktree 根目录启动以提供 `web/build`）；脚本经 REST 造数：用户 alice（所有者，登录者）、bob（只读协作者）、carol 未用；两个会话（名称含 HTML）、3+28 个任务（其中一个标题含 `<img onerror>` 与 `**`，描述含 `<script>`）、依赖、路径重叠、问题（含选项/自由文本/用户自己发起/上下文已过期）、已答复问题 + 决定、交接、消息。登录用 `fetch /users/login` 取得 HttpOnly cookie（本地测试账号，种子口令仅在本机一次性库里）。

实际看到并核对的：
- 首页（zh-CN 与 English 均看过）：「等待你处理」3 条（我自己发起的问题按设计不在其中）；概览三栏；任务列表首屏 25 行，点「加载更多」后 31 行（3+28），按钮消失；状态过滤 `blocked` 只剩 1 行。
- 恶意内容：`main` 内 `img`/`script`/`b` 元素数为 0，`window.__xss` 为 `undefined`；标题、描述、会话名 `<b>x</b>`、消息正文都以原文显示。
- 就地回答：对「Default database…」选 PostgreSQL 并发送 → 服务端 `GET questions/{id}` 显示 `status=answered, version=2, answer.option_id=o2, actor_kind=user, actor_id=1`；收件箱从 3 降到 2。
- 取消：对「Alice's own question」取消 → 危险对话框 → 确认 → 卡片显示「已取消」。
- stale：「Should release notes mention MCP?」显示「上下文已过期」与说明，且**没有**回答表单。
- 任务详情：完成状态（`questions_resolved` 未满足「1 of 3 questions are still open」等）、依赖（T2 → T1「声明未完成」）、重叠（`crates/rg-db ⟷ crates/rg-db/src`、置信度「声明较新」、负责会话状态）、问题/决定/交接/消息、诊断（9 条 finding，附「未覆盖」清单）。会话状态随心跳超时由「在线」变「离线」，UI 如实反映。
- 权限：未授权成员 bob（非协作者）→ 「你无权查看此处的协作数据」；把 bob 加为只读协作者后 → 页面可读，「等待你处理」显示「回答问题需要写权限」，会话状态不可用时只显示会话 id 缩写并给出说明。
- 网络：全程请求均为登录 cookie，无会话头；只读成员的 403 只出现在 `sessions`/`inbox/user`（预期）。
- 手机宽度 375px 查看任务详情：`scrollWidth = clientWidth`，无横向溢出。

### 没有验证 / 已知限制（请勿据此声称已覆盖）
- **未验证**：PostgreSQL/MySQL（UI 只调 REST，但本次只用了 SQLite）；Chrome/Safari/Firefox（只用了 Claude 内置浏览器）；键盘可达性与读屏；大数据量（成千上万任务/消息）的渲染与性能；真实 Agent 客户端的端到端；`recipient.kind=user` 的问题在浏览器里回答、以及「非被指定接收者」被服务端拒绝（403）的页面表现；回答的幂等重放与并发冲突只有单测，没有在浏览器里造出 `VERSION_CONFLICT`。
- 浏览器验证是手工的，没有自动化 E2E 脚本入库。
- 完成状态与诊断里的说明句是**服务端英文文本**，不随语言切换翻译（UI 外壳、标签与枚举都已双语）。
- 首页与详情会为取得会话状态额外请求 `sessions`（最多 3 页×100）；只读成员会得到 403 并退化为只显示 id。
- 任务详情的各区块各自请求、各自失败；没有轮询或 WebSocket，数据在加载时刻为准（页面不是实时的，需要刷新）。
- 没有推送、没有合并；运行用的服务与数据库在验证后已停止并留在 scratchpad。

### 集成 / 远端 / 发布验证
未做：未合并到 main，未推送，未跑 CI。

## 增补（2026-11-06）：完成条件卡片（O04 用户门）

任务详情页新增「已定义的完成条件」区块（`web/src/lib/components/collaboration/ConditionsCard.svelte`）。此前「手工确认」只有 REST 入口，而设计上它**只给用户**（Agent 不能替授权账号自我确认），所以没有这块界面功能对人不可用。

**做了什么**
- 读取 `completion-status.defined`：定义版本、每条条件的标签、类型、判定状态（满足/未满足/过期/未知）、说明与 CI 证据；`manual_confirmation` 显示确认人（账号 id）、时间、绑定的分支头和备注。
- 人能做的两件事：对 `manual_confirmation` 条件**确认**（带可选备注；状态已满足时不再给按钮，过期或未确认时给「确认/再次确认」），以及把当前评估**记录为快照**并查看已存快照（展示的是当时存下的原样内容，不重算）。
- 提示语写明：仅作展示，不阻止/批准/合并；确认绑定当时的分支头，分支移动后变过期；确认是账号记录，不证明是人本人操作。
- 写入带幂等键：确认与快照各自每次表单生成一个 `client_request_id`，失败后换新键；快照 `POST` 走 `?client_request_id=`。
- 条件标签、备注等客户端字段一律按文本渲染。

**不做**：没有给人编辑条件定义的界面（定义由客户端写入；`PUT` 的用户入口存在，但本次不做）；没有强制模式（已决定保持仅展示）。

**验证（逐层）**
| 层 | 结果 |
|---|---|
| 任务验证（`web/`） | `pnpm run test`：145 个文件、756 项通过（新增 `ConditionsCard.test.svelte.ts` 9 项：无定义、恶意标签不渲染成标记、确认带稳定键与去空白备注并刷新、仅 `manual_confirmation` 有按钮、过期显示确认人/分支头并给"再次确认"、已满足无按钮、失败保留表单且下次换新键、快照列表与展开读取已存内容、记录快照带键并刷新）；`pnpm run check` 0 错误；`node scripts/i18n-key-check.mjs` en/zh-CN 各 1480 键一致；`pnpm run build` 通过 |
| 运行验证 | 真实服务（`scripts/collab-e2e.py --scenario p1 --keep` 留下的数据，SQLite）+ Claude 内置浏览器（面板隐藏，用 DOM 文本读取与 `find`/点击驱动，无截图）：中文界面下看到过期确认显示"再次确认"、新增的未确认条件显示"确认"；含 `<b>` 的标签按纯文本显示；提交确认→201，状态变"已满足"、绑定当前分支头、备注显示、整体变"证据不完整"、按钮消失；记录快照→201、列表变 2 条，展开显示当时存下的状态；网络请求里唯一的 401 是登录前的 `users/me` |
| 未验证 | PostgreSQL/MySQL；英文界面的肉眼检查（只验证了键一致）；其他浏览器；键盘可达性与读屏；写权限不足时的 403 呈现（只有单元测试覆盖）；没有截图（面板隐藏） |

## 附录：合并后整体走查（2026-11-06，PR 合并四组之后）

**确认（运行结果）**：合并后的 `web/build` 由真实 `ironforge serve`（SQLite，全新库）提供，用内置浏览器以登录身份走查。

- 仓库「协作」下七个页面（任务/组/健康/模板/搜索/规则/设置）均渲染，共享导航在每页可用。
- 任务页包含四组新增的全部区块；在其上实际操作：添加依赖（201，图版本 0→1）、对反向依赖得到成环拒绝（409 `DEPENDENCY_CYCLE`，页面显示可读信息，未写入）、记录决定（201，列表刷新）、搜索（命中该任务）。
- 客户端写入的标题（含 `<img onerror>`）在列表、下拉框中均为纯文本。
- 手机宽度（375px）：任务页、任务列表、组、健康、模板、搜索、规则无横向溢出；**设置页曾溢出（440px）**，原因是 `grid-template-columns: max-content 1fr` 让长标签无法折行，已改为 `fit-content(55%) minmax(0, 1fr)`（同模式的五个组件一并修改），复测 375px。
- zh-CN：界面文案均已翻译；用户写入的文本与服务端返回的完成条件说明句（如 "1 of 1 questions are still open"）是英文，后者由服务端生成。

**未覆盖**：只有 SQLite；仅看了部分页面的截图，没有逐页逐状态的视觉审查；创建组、创建模板、预算、重指派、完成条件确认在合并后未再手工操作（各自在原分支验证过）；未测深色以外的主题。
