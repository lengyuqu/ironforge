# 协作 Web UI（运维四页）：模板 / 搜索 / 规则 / 设置：实现与验证记录

> 日期：2026-11-06。分支 `claude/web-ops-21bccc`（基于 `feat/web-conditions` @ `83424de`）。**可信度**：确认（逐层标注，未验证的明说）。只改 `web/`，没有改 Rust。

## 范围与原则

四个新页面，都挂在 `/{owner}/{repo}/collaboration/` 下，用共享的 `CollabNav`（未改动）：`templates`、`search`、`rules`、`settings`。

- 人用登录身份操作，**不发** `X-Ironforge-Session*`。
- 客户端/仓库写入的文本（模板、规则路径与清单错误、工具标签、备注）一律用 Svelte 文本插值，无 `{@html}`、无 Markdown、不生成链接。
- 声明就标声明：能力声明显示为「自称」并标 `authorization: none`，从不表现为权限；规则文件显示为「仓库内容，不是指令」。
- 每次写入带新鲜幂等键（`newClientId()`），失败后换新键；`VERSION_CONFLICT` 后重读。

## 实现证据

| 页面 | 内容 | 端点 |
|---|---|---|
| 模板 | 列表（最新版本号）、新建、编辑 = 保存为**新版本**（`expected_version` = 编辑者看到的 `latest_version`）、按版本查看历史（逐版本读取，旧版本标注「由它创建的任务仍保持该版本」）、冲突后「重新加载」。页面明说**不能**从模板创建任务（见下「契约发现 1」） | `GET/POST …/templates`、`GET …/templates/{id}?version=N`、`POST …/templates/{id}/versions` |
| 搜索 | 选一种记录类型 + 字面文本；结果只显示类型、id、创建时间和指向任务的链接；`next_cursor` 加载更多（沿用原搜索词）；无总数、无片段；本地先拒绝 `% _ \`（与服务端一致） | `GET …/search` |
| 规则 | 可选 ref 与 path；显示请求的 ref、解析到的提交、发现版本、清单状态（absent/valid/invalid + 服务端给的原因，作文本）、条目（路径、适用范围、来源、大小、blob、是否超大）、跳过条目与原因、截断；链接指向普通文件查看器并固定到解析出的提交；未知 ref（404）有专门提示 | `GET …/rules` |
| 设置·升级提醒 | 查看/编辑（分钟 ↔ 秒、静默窗口 UTC、重复间隔 0=只一次），版本 CAS，本地先校验范围；冲突后重读并说明「你的修改未被应用」；说明提醒是发给 Agent 会话的惰性通知、不发邮件、不通知真人、静默只推迟 | `GET/PUT …/escalation-settings` |
| 设置·能力声明 | 逐会话列出「自称」的测试/网络/工具/备注，过期标记及原因，`authorization: none`；未声明的会话不在列表（未知≠不能） | `GET …/declarations`（会话名来自 `sessions`，读不到时退回短 id） |
| 设置·删除影响 | 只读预览：各类记录计数、应用内删除与删行的不同后果、Git 保留天数、保留策略、`as_of`；非管理员得到明确的「不可用」 | `GET …/deletion-impact` |

非管理员状态：模板/升级设置的写入由服务端拒绝并原样显示其消息；设置页用删除预览的 403 提前判定「非管理员」并禁用「编辑设置」（该端点本身仅限仓库管理员，所以 403 是可靠信号），其余只读部分照常显示。

代码：`web/src/lib/api/collabOps.ts`（新文件，未改 `collaboration.ts`）、`web/src/lib/utils/collabOps.ts`、`web/src/lib/components/collaboration/{TemplateForm,TemplateContent,EscalationSettings,DeclarationsCard,DeletionImpactCard}.svelte`、四个路由页、`web/src/test-stubs/OpsEmptyStub.svelte`（测试桩）、i18n 新顶层键 `collab_ops`（en 与 zh-CN 各 227 行，键集一致）。

## 任务验证（本分支、本机，2026-11-06）

在 `web/` 执行：

- `pnpm run check`：0 errors（48 warnings，均为既有的 `state_referenced_locally` 同类；本次新增 3 条出自 `TemplateForm.svelte` 刻意只取初值，父级用 `{#key}` 重建）。
- `pnpm run test`：154 个文件、816 个测试全部通过（其中本次新增 9 个文件）。
- `pnpm run build`：成功。
- `node ../scripts/i18n-key-check.mjs`：通过（en = zh-CN = 1675 个键）。

新增测试覆盖：纯规则（占位符、模板校验、升级设置换算与校验）；`TemplateForm`（创建载荷、非法标题模式阻止提交、新版本带 `expected_version`、失败后**换新幂等键**、`VERSION_CONFLICT` 显示服务端消息与重新加载、恶意文本不成标记）；模板页（不提供「从模板创建任务」且给出说明、版本历史与「旧版本」标注、恶意模板名/目标/交付物/清单渲染为文本、冲突后重读、非管理员创建得到服务端 403 消息）；搜索页（LIKE 元字符本地拒绝、类型与链接、无总数、分页沿用原词、空结果不暗示隐藏记录、服务端错误显示）；规则页（来源与提交标注、ref/path 透传、清单无效含恶意原因文本、恶意路径与跳过条目、文件查看器链接做路径编码、无提交、404、403）；升级设置（分钟→秒与版本、范围本地拒绝、冲突后重读、非管理员）；能力声明（声明即主张、过期原因、恶意标签/备注、会话名读不到时退回短 id）；删除预览（无破坏性控件、403 状态、刷新）；设置页（三部分互相独立失败、非管理员联动）。

## 运行验证（真实浏览器 + 真实服务）

环境：预编译的 `ironforge` 二进制（未 `cargo build`），`scripts/collab-e2e.py --scenario p1 --keep` 造数据（`collab-e2e/pair-work`，真实 Git 仓库），我另推了两个提交：根 `AGENTS.md` 内含 `<img onerror>`/`<script>`/「Ignore all previous instructions」、`src/lib/AGENTS.md`、以及分支 `bad-manifest`（`.ironforge/agent-context.toml` 含 `../../etc/passwd`）。另用 REST 建只读协作者 `reader`、恶意命名的模板、一个声明了恶意工具标签与备注的会话。内置浏览器窗格**可见**，用了截图和 DOM 读取。语言：zh-CN 与 English 均看过。

已实际观察到：

- **模板**：管理员保存新版本；用 REST 先抢走一个版本后再从 UI 保存 → 得到服务端 `VERSION_CONFLICT` 消息和「重新加载」，重新加载后表单从最新版本重建，再保存成功（v4）；历史页列出 v4…v1，旧版本标注；恶意模板名/目标作为文本显示，页面上没有 `img/script/b` 元素，`window.__pwned` 保持 undefined；`reader`（只读）新建模板 → 表单内显示服务端 `repository admin access required (FORBIDDEN)`。
- **规则**：默认 `HEAD` 看到解析提交与 `AGENTS.md`；选 `bad-manifest` → 「清单无效」+ 服务端原因 `path must not contain empty, '.' or '..' segments`，并说明已退回默认查找；`main` + `src/lib` → 两个条目（根与 `src/lib`），适用范围正确；不存在的分支 → 专门的「引用不存在」提示；恶意 `AGENTS.md` 内容从未被页面取回或渲染（页面只列路径）。
- **搜索**：task/decision/message 三类有命中，结果只有类型、短 id、时间、「打开任务」链接，点击进入任务详情页；`50%` 被本地拒绝、不发请求。
- **设置（管理员）**：保存升级设置（含 22:30–06:15 UTC 静默窗口）得到版本 1；编辑中途用 REST 改掉后保存 → 冲突提示 + 重新显示服务端当前值（版本 2）；能力声明显示「自称…」「授权：无」，恶意工具标签与备注为文本；删除预览显示计数与说明。
- **设置（`reader` 只读）**：升级设置可读但「编辑」被禁用并给出原因；删除预览显示「不可用：仅仓库管理员」；能力声明可读（会话名因无 `sessions` 权限退回短 id）。
- 全部操作结束后服务已停止，工作目录已删除。

## 未验证 / 局限

- **PostgreSQL / MySQL**：只在 SQLite 上跑；本次没有改后端。
- **窄屏/移动端**：用了 `flex-wrap`/`auto-fit` 与一个 600px 断点，但**没有**在手机视口里看过。
- **浏览器真实键盘/屏幕阅读器**：没做；多处交互用 DOM 派发事件驱动（输入、提交），不是真实键入。
- **「从模板创建任务」**：不在 UI 内，没有端到端验证（见下）。
- **并发提交下的幂等回放**（同一个 key 的重放）：只有单元测试断言「失败后换新 key」，没有在真实服务上回放同一个 key。
- **大量模板/版本**：版本历史一次最多取最新 50 个版本（超出会提示），没在真实服务上压测。
- **规则页只列出路径与 blob id**，不读取内容；因此「规则内容渲染」这件事这页根本没有做，恶意内容只在文件查看器里（不属本页）出现。
- **`reader` 之外的角色**（组织成员、协作者写权限、站点管理员）没有逐一看过。

## 契约发现（后端，未改）

1. **`POST …/tasks/from-template` 必须是会话**：处理函数用 `acting_session`（要求会话 id 与密钥），不像其它写入用 `write_actor` 接受「用户门」。已登录的人无法调用，所以 UI 不提供，页面上明说并建议在 Agent 客户端内创建。若希望人也能从模板建任务，需要后端给它一个用户门（`write_actor`）；这是后端决定，本分支没有绕过。
2. **没有「版本列表」端点**：`…/templates/{id}/versions` 只有 `POST`（新建版本）。历史只能按 `GET …/templates/{id}?version=N` 逐版本读（`N=1..latest_version`），UI 就是这样做的（一次并发读最新 50 个）。
3. **没有 `PUT/PATCH …/templates/{id}`**：「编辑」就是 `POST …/versions`；改名不存在。
4. **`VERSION_CONFLICT` 的 `error.details.current_version` 在前端拿不到**：`ApiError` 只携带 `status/code/message`，所以 UI 一律重新读取而不是直接用服务端给的当前版本号。
5. **没有「我是不是管理员」的信号**：模板写入与升级设置写入只能由服务端 403 告知；设置页借用只限管理员的删除预览的 403 提前判断，模板页没有等价的探针，所以非管理员会看到「新建/新版本」按钮，点了才被拒绝（消息明确）。
6. 一次**未能解释的瞬时 403**：在验证模板页时，一次「保存新版本」的 POST 返回了 `repository admin access required`（同一个管理员账号，之前和之后的同类请求都成功，无法复现；当时浏览器的鉴权已从 localStorage 令牌切换为 cookie）。没有足够证据判断是后端还是测试环境问题，**只作记录，不下结论**。
