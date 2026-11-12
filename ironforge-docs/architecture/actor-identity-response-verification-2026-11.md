# 响应携带主体类型（T26）现状核实 — issue #37

> 基线 `a41c9b2`（2026-11-12T10:02Z），取证在 worktree `issue-37-manual` 内完成；`scripts/check.sh --quick` 改前 `EXIT=0`（126 s，`Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 2m 05s`）。
> **本文件不改任何代码**：T26 改 OpenAPI 与前端契约（L2），且可能引入响应 DTO。这里只固化「今天的事实」，供拍板。
> 判定口径：**22 条断言 = 成立 11 / 部分成立 5 / 不成立 5 / 不可验 1**。三处字段名写错（PR / 评审 / 评论），一处契约面硬阻塞（capabilities）。

## 1 判定表

| # | 断言（issue #37 或设计文档 §4 T26） | 判定 | 证据 |
|---|---|---|---|
| N1 | 目标：客户端不必再查 `/agents` 就能看出作者是人还是 agent | **成立** | `/agents` 是 owner 自省接口：`api/agents.rs:77` = `rg_core::agent::list_agents(&state.db, owner_id)`，`owner_id` 来自 caller（`:73-76`）⇒ 只能列自己名下的 agent，看别人的 PR/评论时查不到任何东西。这是 T26 唯一的正当性 |
| N2 | 设计文档有 §4 T26 | **成立** | `agent-principals-design-and-roadmap-2026-11.md:386` = `### T26 响应携带主体类型` |
| N3 | 「批次 B2；依赖 T23、T25；排最后」 | **成立** | §4 表 `:231` = `| T26 | 响应携带主体类型 | 8 | L | T23/T25 |`；B2 行 `:605`；「排最后，可延到第三期」`:399` |
| N4 | T23（审计 `on_behalf_of_user_id`）已落地 | **不成立** | `grep -rn on_behalf_of crates/ --include=*.rs` = **1 命中**，且是 `rg-core/src/agent.rs:23` 的 doc 注释；`audit_log` 实体无该列（`entities/audit_log.rs:12-34`），`AuditLogEntry` 也无该字段（`api/audit.rs:29-40`） |
| N5 | T25（提交身份派生）已落地 | **不成立** | `grep -rn commit_identity crates/` = 0；`repo_content.rs` 仍原样传 `&user.username` / `&user.email`。T25 本身是待做项（issue #36 只出了核实文档） |
| N6 | 「文档里的行号与现状判断是 2026-11-10 的快照」 | **部分成立** | 文档只在 `:623` 给 **§5** 加了该免责（「§5 的现状判断…是 2026-11-10 的快照」）；§4 T26 的「现状（核实）」块**没有任何时点声明**，issue 正文把该免责外推到全文 |
| N7 | 动手前读 `.ai/guardrails.md`，新增迁移/改权限属 🟠 CONFIRM | **成立** | `.ai/guardrails.md:28` = `| 新增迁移文件（rg-db/src/migrations/） | 🟠 CONFIRM | …`；`:32` `修改现有迁移文件内容` = 🔴 BLOCK |
| N8 | `REGISTRY` 现在「27 个能力 / 82 条路由」 | **成立** | 复算：`grep -c '^    Capability {'` = **27**；解析 `r(` 调用 = **82** 条、**65** 个不同路径（`SESSIONS` 常量复用 5 次）。`capabilities.rs:133` = `pub const REGISTRY: &[Capability] = &[` |
| N9 | `CONTRACT_MAJOR/MINOR = 1/1`（`:30-31`） | **成立** | `capabilities.rs:30` = `pub const CONTRACT_MAJOR: u32 = 1;`、`:31` = `pub const CONTRACT_MINOR: u32 = 1;` |
| N10 | 防漂移测试在 `collab_contract_tests.rs:116/136/399` | **成立**（`:399` 是「节内行」不是「测试入口」） | `:116` = `fn registry_and_registered_routes_are_the_same_set()`、`:136` = `fn every_registered_operation_is_in_the_openapi_document()`；`:399` 落在 `:354` 起的 `async fn m11_capability_list_is_exactly_what_the_server_serves()` 里（断言 `api_capabilities` 含 `repo.settings.patch.v1`） |
| N11 | 「响应视图在 `collab_capabilities.rs:24-37`（`CapabilityEntry`）+ `:105-119` 映射」 | **部分成立** | `:24` = `pub struct CapabilityEntry {`，结构体到 `:36`（`:37` 是空行）；映射起于 `:105`（`registry::all()`），到 `:118`。**但 `CapabilityEntry` 没有 `routes` 字段**（只有 `name` / `version` / `status` / `requires` / `mcp_tools`，`:25-35`）⇒「routes 指向上述四处」只存在于 Rust 侧，**不上线、客户端看不到** |
| N12 | `ActorRef` 不存在，需新增 | **成立** | `grep -rn ActorRef crates/ web/src` = **0 命中**。但已有两份近邻形状：`TimelineActor { id, username }`（`api/reviews.rs:75-78`）与 `AgentInfo { id, username, kind: &'static str, owner_user_id, description, is_active, created_at }`（`rg-core/src/agent.rs:91-99`）⇒ 新增 `ActorRef` 是**第 3 份** actor 形状，不是第 1 份 |
| N13 | 落地范围之一：「PR 详情/列表的 `author`」 | **不成立** | PR 响应里没有 `author` 对象：`pull_requests` 只有 `author_id: i64`（`entities/pull_request.rs:27`）、`reviewer_id: Option<i64>`（`:29`）；详情直接序列化实体（`api/pulls.rs:229` = `let mut body = serde_json::json!(pr);`），列表直接 `PaginatedResponse<PullRequest>`（`:195`）。全仓 `rg-http` 里唯一的响应字段 `"author"` 是**提交历史**的 `CommitEntry.author`（`api/repo_content.rs:183`，值是 git 作者名，与账号无关）。前端同样只读 `author_id`（`web/src/lib/types/entities.ts:57`/`:82`） |
| N14 | 落地范围之二：「评审列表的 `reviewer`」 | **不成立** | `GET …/pulls/{number}/reviews` 直接返回 `Vec<PrReview>`（`api/reviews.rs:281-283`），实体字段是 `reviewer_id: i64`（`entities/pr_review.rs:16`）。`reviewer` 只出现在两处**事件/时间线 metadata**：`api/reviews.rs:729`（`"reviewer": users.get(&request.reviewer_id)`，值为用户名字符串）与 `:1116`/`:1182`（`"reviewer": reviewer.username`） |
| N15 | 落地范围之三：「评论列表的 `user`」 | **不成立**（字段名是 `author`） | `api/issues.rs:283` = `pub struct CommentResponse {`，`:286` = `pub author: Option<String>,`（`#[serde(flatten)]` 展开 `issue_comment` 实体）。`author_name()`（`:1124-1140`）只查 `users.username` 返回 `Option<String>` —— 无 id、无 kind、无 owner。handler 在 `:700` |
| N16 | 落地范围之四：「`/admin/audit/logs` 的行」 | **成立** | `api/audit.rs:29-40` = `AuditLogEntry { id, user_id: Option<i64>, username: Option<String>, action, resource_type, resource_id, resource_name, ip_address, details, created_at }`，带 `#[derive(ToSchema)]`（`:28`）；构造在 `:145-155`。这是四处里**唯一具名 + ToSchema** 的落点 |
| N17 | 「capabilities 新增 `actors.identity`（routes 指向上述四处）」 | **部分成立（有硬阻塞）** | REGISTRY 只容纳 `/…collaboration…` 路由：`collab_contract_tests.rs:79` = `if !path.contains("collaboration") { continue; }`，而 `registry_operations()`（`:103-109`）取 **全部** REGISTRY 路由；`:116-134` 断言两侧差集为空，任一非空即失败。把 PR/评审/评论/审计路由写进 REGISTRY ⇒ `stale` 非空 ⇒ 测试红。所以这一步实际是「**改防漂移测试的采集口径**」，不是「加一条能力」 |
| N18 | 「`CONTRACT_MINOR` 1 → 2」可宣告新字段 | **部分成立** | 值仍是 1（N9）。但四处里只有审计走 `ToSchema`（OpenAPI 可见字段，`api/audit.rs:28`）；PR 详情/列表在 OpenAPI 里是 `body = serde_json::Value`，评论的 `CommentResponse` **只有 `#[derive(Serialize)]`**（`api/issues.rs:282`）⇒ **升 minor 掩盖不了「OpenAPI 看不到新字段」**，客户端的类型生成器得不到任何东西 |
| N19 | 「字段只增不改名，前端不改也能跑」 | **成立** | 前端 PR 类型用 `author_id`（`web/src/lib/types/entities.ts:57`），没有 `author` 占用（`grep -rn 'author:' web/src/lib --include=*.ts` 命中的是 `comments` 的 `author: string` 与 git 提交的 `author: {name,email,date}`）⇒ 新增 `author` 对象不破现有渲染 |
| N20 | 风险：「多个 handler 直接返回 `serde_json::Value`（`json!` 拼的），类型系统帮不上忙」 | **部分成立（方向反了）** | 四处里只有 **1 处** `json!`：PR 详情（`api/pulls.rs:229`）。评论是具名 `CommentResponse`、审计是具名 `AuditLogEntry`、评审是实体 `Vec<PrReview>` —— 全是编译期强制的。真障碍是反的：PR 详情与列表**直接序列化 DB 实体**，加展示字段要么后置 enrich（详情已有 fork 先例，`pulls.rs:230-237`），要么引入 DTO 并换掉 `PaginatedResponse<PullRequest>` 的元素类型（列表没有 enrich 先例） |
| N21 | 完成口径「新 SQL 至少跨后端跑一次（`IRONFORGE_TEST_DATABASE_URL`）」可行 | **成立** | `rg-db/src/lib.rs:194-200` 按 `DbBackend` 分派 `connect_sqlite` / `connect_postgres` / `connect_mysql`；多个集成测试（`collab_groups_tests.rs:5`、`fork_pr_ref_tests.rs:8` 等）以该 env 切换后端 |
| N22 | 规模 L（1–2 天）、8 个触点文件 | **不可验** | 无实现可比对；但按 N13/N20，真正成本在「PR 列表/详情引入 DTO + 前端类型」而非「给四个字段加 kind」，8 个触点偏少 |

## 2 四处落地点的真实形状

| 位置 | 现有字段 | 类型 | 构造点 | 前端消费 |
|---|---|---|---|---|
| PR 详情 `GET /repos/{owner}/{name}/pulls/{number}` | `author_id`、`reviewer_id` | `i64` / `Option<i64>`（DB 实体列） | `pulls.rs:229` `json!(pr)` + fork enrich `:229-236` | `author_id`（`entities.ts:57`） |
| PR 列表 `GET …/pulls` | 同上（每项） | `PaginatedResponse<PullRequest>` | `pulls.rs:193-197`（`PaginatedResponse::new` 在 `:195`） | 同上（`:82`） |
| 评审列表 `GET …/pulls/{number}/reviews` | `reviewer_id` | `i64`（`PrReview`） | `reviews.rs:281-283` | 未在 `web/src` 找到 reviewer 渲染 |
| 评审时间线 metadata | `reviewer`（用户名） | `Option<&String>` | `reviews.rs:729` | — |
| 评论列表 `GET …/issues/{number}/comments` | `author`（用户名） | `Option<String>` | `issues.rs:286`，作者查 `:1124-1140` | `author: string`（`entities.ts:145`/`:151`） |
| `/admin/audit/logs` | `user_id` + `username` | `Option<i64>` + `Option<String>` | `audit.rs:145-153` | 未见前端页面 |

**要点**：T26 不是「给已有 `author`/`reviewer`/`user` 对象补 kind」，而是**在三处新增一个嵌套对象**（PR 的 `author`、评审的 `reviewer`、评论的 `author` 升级），只有审计一处是「给具名结构体加两个字段」。

## 3 capabilities 面的三条路（设计只写了第 1 条）

1. **新建 `Capability`（原案）**：被 N17 的采集过滤器挡住。要落地必须同时改 `collab_contract_tests.rs:79` 的口径 —— 而放宽过滤器会让 `missing` 侧一次性涌入全部非 collaboration 路由（REGISTRY 只有 82 条），等于重写这个防漂移测试。
2. **复用 `api_capabilities` 旗标（原案未提，最小路径）**：`api/collab_capabilities.rs:104` 已是 `vec!["wiki.revision_scope.v1", "wiki.concurrent_update.v1", "ci.default_branch.v1", "repo.settings.patch.v1"]`，语义就是「本服务支持某个加法变更」；`collab_contract_tests.rs:386-395` 逐条断言这些旗标存在。加 `"actors.identity.v1"` = **一行 + 一条断言**，零路由冲突、零 `CONTRACT_MINOR` 争议。
3. **只改 OpenAPI、不动 capability**：若四处响应在 OpenAPI 里本就是 `serde_json::Value`（PR 详情/列表），字段加了也没人知道 ⇒ 等于没有契约。要让它有意义，得先把 PR 响应改成具名 schema（正是 N20 的 DTO 工作）。

## 4 两份材料都没有的新事实

- **F1（可复用判据）**：判断「某能力能不能登记进 REGISTRY」，别读 `Capability` 结构，读**采集过滤器**（`collab_contract_tests.rs:79`）。REGISTRY 的语义边界是「`/…collaboration…` 路由的清单」，不是「服务能力的清单」。
- **F2**：`api_capabilities`（`collab_capabilities.rs:104`）是本仓既有的「加法变更宣告」通道，四个旗标都是「新增可选字段/行为」的先例。T26 的正确宣告位置在这里，不在 REGISTRY。
- **F3**：`TimelineActor`（`reviews.rs:75-78`）已经是「actor 对象」，且只用于评审时间线。新增 `ActorRef` 若不复用它，同一个 PR 页面会出现两种 actor 形状（时间线一份、`author` 一份）。
- **F4**：`registry_operations()`（`:103-109`）与 `registered_collab_routes()`（`:48-101`）**都不做路径归一化**（`normalize()` 在 `:30-45` 定义但这两条路径没调用），两侧必须逐字一致。今天 PR 路由 `lib.rs:847` 与 REGISTRY 同用 `{number}` 写法，暂时一致；任一侧改写参数语法即恒红。
- **F5**：审计的 `username` 是**落库时点的快照**（`entities/audit_log.rs` 注释「Denormalised for quick display / filtering without JOIN」），而 `ActorRef.username` 若从 `users` 表现查，会与同一行审计的 `username` 打架（agent 改名后尤其明显）。两处口径必须选一个并在文档里写死。

## 5 L2 待拍板

| 编号 | 级别 | 事项 |
|---|---|---|
| **R37-1** | P1 | **宣告通道裁决**：`actors.identity` 走 `api_capabilities` 旗标（推荐，成本 1 行）还是新建 `Capability`（需重写 `collab_contract_tests.rs:79` 口径）。二者不可同时做 |
| **R37-2** | P1 | **PR 响应 DTO**：列表/详情直接序列化 `PullRequest` 实体。加 `author` 对象必须先决定「后置 enrich（沿用 `pulls.rs:230-237`）还是引入 `PullRequestResponse` DTO」——后者会改 OpenAPI 的 `body = serde_json::Value` 为具名 schema，属契约变更 |
| **R37-3** | P2 | **`ActorRef` 与 `TimelineActor` / `AgentInfo` 合并**：三份形状并存会让前端各写一套。建议 `ActorRef` 作为唯一出参，`TimelineActor` 改为别名 |
| **R37-4** | P2 | **评论 `author` 的兼容路线**：现值是 `Option<String>`（用户名）。改为对象是**破坏性改名**（同名不同类型），与 N19「只增不改名」冲突 ⇒ 需新增 `author_ref` 或升 `CONTRACT_MAJOR` |
| **R37-5** | P3 | **审计 `username` 口径**：F5 的两种取值（快照 vs 现查）必须二选一，写进本文档后实施 |
| **R37-6** | P3 | **`on_behalf_of` 依赖**：N4 未落地，`ActorRef.owner_id` 只能从 `users.owner_user_id` 读，无法表述「代表人 A 而非所有者 B」的委派场景 |

## 6 复核请求（限定文件）

1. `crates/rg-http/tests/collab_contract_tests.rs:79` 与 `:116-134` —— 确认「往 REGISTRY 加非 collaboration 路由必然使 `stale` 非空」，别只看 `missing` 一侧。
2. `crates/rg-http/src/api/pulls.rs:229` 与 `:196-201` —— 确认 PR 详情/列表都没有 `author` 对象，只有 `author_id`。
3. `crates/rg-http/src/api/issues.rs:286` —— 确认评论响应的字段名是 `author` 且类型是 `Option<String>`。
4. `crates/rg-http/src/api/collab_capabilities.rs:24-36` —— 确认 `CapabilityEntry` 不含 `routes`。

## 7 本轮不成立条目（并入反向清单 79–83）

79. 「PR 详情/列表有 `author` 字段」——不成立，只有 `author_id: i64`（`entities/pull_request.rs:27`）。
80. 「评审列表有 `reviewer` 字段」——不成立，实体只有 `reviewer_id`（`entities/pr_review.rs:16`）；`reviewer` 仅存在于事件 payload。
81. 「评论列表有 `user` 字段」——不成立，字段名是 `author` 且是 `Option<String>`（`api/issues.rs:286`）。
82. 「多处 handler 用 `json!` 拼响应、类型系统帮不上忙」——不成立（四处里仅 PR 详情 1 处）；真障碍是 PR 直接序列化 DB 实体。
83. 「T23/T25 已落地，可直接给 `ActorRef` 填 `on_behalf_of`」——不成立（`on_behalf_of` 仅 1 处 doc 注释；`commit_identity` 0 命中）。
