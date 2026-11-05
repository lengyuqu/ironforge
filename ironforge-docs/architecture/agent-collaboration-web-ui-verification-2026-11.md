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
