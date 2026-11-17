# T29 执行计划 — 符号查询端点与 MCP 工具（#63 第三步）

> 权威约束见 `tasks/T29.md`，设计依据见
> `ironforge-docs/architecture/symbol-graph-design-2026-11.md`（T27 实测，§5 诚实边界）。
> 基线 `3ba117d`，分支 `agent/t29-symbol-query`，worktree `../ironforge-t29`。
> 实际执行与验证输出见 `docs/plans/T29-task.md`。

## 1. 范围

只做**读**：`GET /ai/repos/{o}/{r}/symbols` + MCP 工具 `find_definition` + core 查询函数。

不做（任务书明确排除）：引用查询、影响面分析、编辑距离等模糊匹配、新表/新迁移、
新造能力注册表。

## 2. 分层与「不重复实现」

```
HTTP  GET /ai/repos/{o}/{r}/symbols        ─┐
                                            ├─→ rg_core::search::symbol_query::find_symbols
MCP   find_definition (tools/call)         ─┘     (匹配 / impl 过滤 / confidence / 404)
        └─ 200 OK: GET 同一端点
```

- `crates/rg-core/src/search/symbol_query.rs`（新建）是**唯一**的查询实现：
  `find_symbols(db, repo_id, commit_sha, name, path, limit) -> Result<SymbolPage>`。
- HTTP handler 只做四件事：权限（`require_read`）、参数校验、ref → 40 位 SHA 解析（gix，
  在 `spawn_blocking` 里）、调用 core 并映射错误。
- MCP 工具是**纯 REST 客户端**（`rg-mcp` 没有数据库依赖，也没有 `rg-core` 依赖，
  `AppState` 只有 `api_base`/`pat`，所有工具都走 REST）。因此 `find_definition` 请求的正是
  HTTP 端点本身，匹配逻辑只有一份；任务书里「调同一个 core 函数」的目标由这条链路满足，
  额外在 `rg-mcp` 里接一个数据库连接会引入第二种取数方式，反而违背本意。
  这一点在 `crates/rg-mcp/src/tools/mod.rs` 的 `tool_find_definition` 注释里写明。

## 3. `confidence` 语义（写进代码注释、OpenAPI 描述与 MCP 工具描述）

| 值 | 含义 |
|---|---|
| `exact` | 这一行**就是**你要找的声明：`name` 与查询词完全相等；或查询是 `path`（那一行本来就是该文件的声明） |
| `name_match` | 只是**前缀**命中（查 `get_` 命中 `get_user`）。名字的前缀相同不等于同一个符号 |

排序保证可解释：按名查询时 `exact` 全部排在 `name_match` 之前
（`ORDER BY CASE WHEN name = ? THEN 0 ELSE 1 END, name, path, start_line`），
所以 `limit` 截断时被截掉的必然是弱匹配。按路径查询按 `path, start_line` 排序。

`confidence` 只回答「这条是不是你要的」，不回答「结果全不全」——后者是 `partial` /
`limitations`。两者不能互相替代（设计文档 §5 的契约措辞）。

## 4. 缺索引 = 404，绝不回落 HEAD（死线）

`symbol_commits` 中没有该 `(repo_id, commit_sha)` → HTTP 404，
`error.code = SYMBOLS_NOT_INDEXED`，`reason = symbols_not_indexed`，
`retryable = true`，`hint` 指向
`POST /api/v1/ai/repos/{o}/{r}/index`（带 `commit_sha`）。

**为什么不做回落**：调用方问的是「这个 commit 上的定义」。回落到默认分支 HEAD 会返回
另一个 commit 的符号，形状完全合法、内容却是错的——读的人无法把它和正确答案区分开。
显式失败至少能被发现、能重试；错答案不能。`find_symbols` 里没有任何 fallback 参数，
`commit_id_of` 的 `None` 分支只有这一个错误。

附带的一条：`ref` / `commit_sha` 先被 gix 解析成 40 位 SHA 再查库，所以
「short sha 恰好等于某个索引里的 sha 前缀」也不会误命中；解析失败是
`404 REF_NOT_FOUND`，与「解析成功但没索引」分开。

参数优先级：`commit_sha` > `ref` > 默认分支（`commit_sha` 更具体）。

## 5. `partial` / `limitations`：给得出什么，给不出什么

**给得出（静态、且已知为真）**：

| `limitations` 条目 | 依据 |
|---|---|
| svelte 只解析 `<script>`，markup / props / 模板绑定不在符号表 | 设计 §5、`tree-sitter-svelte` 与 0.25 runtime 不兼容 |
| 解析失败没有被逐 commit 记录：被拒文件贡献部分树或什么都不贡献，本响应无法说明是哪种 | T28 写入侧只存 `OutlineItem`，不存 `error` |
| Rust 宏生成符号、`dyn Trait` 分派不可解析 | 设计 §5 |
| TS/JS `obj[key]`、`any` 传播不可解析 | 设计 §5 |
| Python `getattr` / 猴子补丁不可解析 | 设计 §5 |
| Go 接口方法集运行期确定，接口调用不可解析 | 设计 §5 |

**`partial` 恒为 `true`**（代码里是 `const PARTIAL: bool = true`，带注释）。理由：
- 五门语言的静态抽取都有已知漏行（宏展开、动态派发、反射）；
- T28 **没存**失败文件数、也没存 commit 的语言分布，因此没有任何 commit 能**证明**「完整」。

`partial: false` 是一个「结果完整」的断言。手上没有支持这个断言的证据，就不写。
任务书要求「已知索引不完整 → true」，这里把「已知不完整」理解为对整张表成立。

**给不出（T28 没存，不编造）**：

| 想要的动态信息 | 现状 | 处理 |
|---|---|---|
| 该 commit 解析失败的文件数 | 写入侧丢弃 `Outline.error`，无列可查 | 不出现 `"N files failed to parse"` 这种模板，改为静态描述「失败未被记录」 |
| 该 commit 含哪些语言 / 有无 `*.svelte` | 没有按 commit 的语言统计；`SELECT path LIKE '%.svelte'` 只能看到有定义的 svelte 文件 | 不下「本 commit 有 svelte」的结论，只列静态限制 |
| 每个符号是否来自宏展开 | tree-sitter 看不到宏展开后的节点 | 只能整表 `partial: true` |

要做到按 commit 精确，需要在 T28 的表里加列（失败数、语言分布），那需要迁移——本任务禁止。

## 6. SQL 形状

```sql
-- 1) commit 必须已索引（无回落）
SELECT id FROM symbol_commits WHERE repo_id = ? AND commit_sha = ?;

-- 2) 命中行（按名；path 可选叠加）
SELECT name, kind, path, start_line, end_line, signature
FROM symbol_definitions
WHERE repo_id = ? AND commit_id = ? AND kind <> 'impl'
  AND (name = ? OR substr(name, 1, ?) = ?)
  [AND path = ?]
ORDER BY CASE WHEN name = ? THEN 0 ELSE 1 END, name, path, start_line
LIMIT ?;
```

- **`limit` 是 SQL `LIMIT`**，不是查完再截断：调用方能信任「拿到的就是排序后的前 N 条」，
  也不会因为一个大的 `limit` 让服务端把整个 commit 的行都物化出来。
- **`limit` 越界报 400**（`INVALID_LIMIT`），静默 clamp 会让调用方以为拿全了。
  默认 50，上限 200（`rg_core::search::symbol_query::{DEFAULT_LIMIT, MAX_LIMIT}`）。
- **前缀匹配用 `substr(name, 1, n) = ?` 而不是 `LIKE 'prefix%'`**：三种后端对 `LIKE` 的转义
  写法不一致（MySQL 把 `\` 当字符串转义符），而符号名里的 `_`、`%` 是字面量。
  `substr` 是 SQLite/PostgreSQL/MySQL 共有函数，不需要转义。执行计划仍是
  `(repo_id, commit_id)` 索引前缀 + 本 commit 内过滤（12k 行量级，毫秒级）。
- **`kind <> 'impl'` 无条件生效**：`impl` 行的 `name` 是源码首行（`ActiveModelBehavior for
  ActiveModel`），不是标识符；按名查询永远不该命中它们（设计 §1.5 实测 684 行）。
  写进 writer 的对称说明在 `symbol_indexer.rs` 模块注释里，这里只读不改。

## 7. 能力注册核实结果

先按要求核实「是否有防漂移测试要求登记新端点/工具」，结论：**有，且不止一处**，
没有新造注册表，只按既有规则登记：

| 注册点 | 规则 | 本任务动作 |
|---|---|---|
| `crates/rg-http/src/openapi.rs::paths(...)` | `openapi_registration_tests.rs`：每个带 `#[utoipa::path]` 的 handler 必须列入，且列入的必须真的挂载 | +1 行 `crate::api::ai::ai_repo_symbols` |
| `crates/rg-http/src/lib.rs` 路由表 | 同一个测试反向检查「注册进 spec 但没挂载」 | +1 条 `.route("/ai/repos/{owner}/{name}/symbols", get(...))` |
| `crates/rg-core/src/auth/route_meta.rs::ROUTES` | `route_metadata_tests.rs`：`lib.rs` 里每条路由都必须声明（PAT scope / repo allow-list / 幂等） | +1 行 `repo("/ai/repos/{owner}/{name}/symbols")` |
| `crates/rg-mcp/src/tools/mod.rs::list_tools` | 无外部注册表；工具表就是唯一来源 | +1 条 `find_definition` |

- `collab_contract_tests.rs` 只约束 `/…collaboration…` 路由（`COLLAB` 前缀过滤），`/ai/*`
  不在其范围内，**不需要**登记。
- 三个 REST 侧注册点都是「加一行」的机械登记，机制已存在；本任务**没有**创建任何新注册表。

## 8. 验收映射

| # | 验收项 | 测试 |
|---|---|---|
| 1 | 精确：`index_commit` → 1 条、`exact` | `ai_symbol_query_tests::symbols_answers_name_path_limit_and_honesty_contract` |
| 2 | 前缀：`index_` → 多条、`name_match` | 同上 |
| 3 | 按 path 列出该文件、无 `impl` | 同上 |
| 4 | 缺索引 404 + 提示、无回落痕迹 | `ai_symbol_query_tests::an_unindexed_commit_is_404_and_never_answered_from_head` |
| 5 | `limit=1` → 1 条；`limit=500` → 400 | 见 #1 的测试 |
| 6 | `name`/`path` 都不给 → 400 | 见 #1 的测试 |
| 7 | `partial` 存在且为 true、`limitations` 非空含 svelte | 见 #1 的测试 |
| 8 | MCP `find_definition` 在 `list_tools` 可见、`call_tool` 与 HTTP 一致 | `rg-mcp` 单测 `tools::find_definition_tests::*` + `read_tools_mcp::find_definition_asks_the_symbols_route_and_passes_the_answer_through` |
| 9 | 查 `impl` 行里的名字 → 不返回 | `symbol_query::tests::impl_rows_are_never_returned`（core，精确 + 前缀 + 按 path 三种）+ 见 #1 的测试 |
