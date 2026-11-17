# T29 执行记录 — 符号查询端点与 MCP 工具（#63 第三步）

> 计划见 `docs/plans/T29-plan.md`，权威约束见 `tasks/T29.md`。
> 基线 `3ba117d`，分支 `agent/t29-symbol-query`，worktree `../ironforge-t29`。

## 1. 开工核对

```
$ git fetch cnb && git rebase cnb/main
Current branch agent/t29-symbol-query is up to date.
$ git log --oneline -1
3ba117d docs(tasks): add T29 for the symbol query endpoint (#63 step 3)
```

本任务**不加迁移、不改 `Cargo.*`**，表结构与 `3ba117d` 完全一致。

## 2. 改动

| 文件 | 改动 |
|---|---|
| `crates/rg-core/src/search/symbol_query.rs` | **新建**：`find_symbols`、`SymbolPage`、`SymbolHit`、`Confidence`、`DEFAULT_LIMIT`/`MAX_LIMIT`、`NOT_INDEXED_CODE`、7 条单测 |
| `crates/rg-core/src/search/mod.rs` | +1 行 `pub mod symbol_query;` |
| `crates/rg-http/src/api/ai.rs` | `GET /ai/repos/{o}/{r}/symbols`：`SymbolQuery`、`AiSymbolItem`、`AiSymbolsResponse`、`resolve_symbol_commit`、`symbol_error` |
| `crates/rg-http/src/lib.rs` | +1 条路由 |
| `crates/rg-http/src/openapi.rs` | +1 行（`openapi_registration_tests` 要求） |
| `crates/rg-core/src/auth/route_meta.rs` | +1 行 `repo("/ai/repos/{owner}/{name}/symbols")`（`route_metadata_tests` 要求） |
| `crates/rg-mcp/src/tools/mod.rs` | `list_tools` +1 条 `find_definition`；`call_tool` +1 分支；`tool_find_definition` / `find_definition_path` + 3 条单测 |
| `crates/rg-http/tests/ai_symbol_query_tests.rs` | **新建**：契约 + 404 不回落的两个测试 |
| `crates/rg-mcp/tests/read_tools_mcp.rs` | +2 条测试（路由/透传、404 是工具错误），坏参数列表 +3 项 |
| `docs/plans/T29-plan.md`、`docs/plans/T29-task.md` | 本任务文档 |
| `tasks/T29.md` | `State` → `completed` |

**没有**：`Cargo.*`、迁移、`crates/rg-core/src/search/outline/`、`symbol_indexer.rs`、
`crates/rg-core/src/{review,authorization,pull_request,issue,release,webhook}/`、`CLAUDE.md`、
`ironforge-docs/`、`.github/`、`scripts/`、`deploy/`。

## 3. 接口形状（实际返回）

```json
{
  "commit_sha": "…40 hex…",
  "partial": true,
  "limitations": [
    "svelte files are parsed only inside <script>: markup, props and template bindings are not in the symbol table",
    "parse failures are not recorded per commit: a file the parser rejected contributes whatever the partial tree yielded, or nothing, and this response cannot say which",
    "Rust macro-generated symbols and dyn Trait dispatch are not resolvable by tree-sitter",
    "TypeScript/JavaScript dynamic property access (obj[key]) and `any` propagation are not resolvable",
    "Python getattr(obj, \"x\") and monkey patching are not resolvable",
    "Go interface method sets are decided at run time: interface calls are not resolvable"
  ],
  "items": [
    {
      "name": "index_commit",
      "kind": "function",
      "path": "src/indexer.rs",
      "start_line": 1,
      "end_line": 1,
      "signature": "pub async fn index_commit()",
      "confidence": "exact"
    }
  ]
}
```

参数：`name` / `path`（至少一个）、`ref` 或 `commit_sha`（缺省默认分支）、`limit`
（默认 50，1..=200）。

## 4. 验证输出

### 4.1 `cargo test -p rg-core`

```
$ cargo test -p rg-core
…（lib 单测 411 passed / 1 ignored + 10 个集成测试二进制）
rg-core passed=452 failed=0
```

新增的 7 条：

```
test search::symbol_query::tests::exact_hits_come_first_and_are_marked_exact ... ok
test search::symbol_query::tests::impl_rows_are_never_returned ... ok
test search::symbol_query::tests::path_queries_return_the_files_declarations_in_order ... ok
test search::symbol_query::tests::limit_is_applied_and_out_of_range_is_refused ... ok
test search::symbol_query::tests::an_unindexed_commit_is_reported_not_answered_from_another_commit ... ok
test search::symbol_query::tests::name_and_path_are_both_required_to_be_absent_by_accident ... ok
test search::symbol_query::tests::limitations_are_always_stated_and_partial_is_honest ... ok
```

### 4.2 `cargo test -p rg-http`（全量）

`134` 个测试二进制 + doc-tests，**872 passed / 0 failed / 3 ignored**，日志
`/tmp/t29-rg-http-full.log`（本次运行，约 7 分钟）：

```
$ cargo test -p rg-http
     Running tests/ai_symbol_query_tests.rs (target/debug/deps/ai_symbol_query_tests-497b657d9f5ad553)

running 2 tests
test symbols_answers_name_path_limit_and_honesty_contract ... ok
test an_unindexed_commit_is_404_and_never_answered_from_head ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.71s
…
test result: ok. 0 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

（末尾那条是 doc-tests。全量里没有 `FAILED` / `panicked`。）

### 4.3 `cargo test -p rg-mcp`

```
$ cargo test -p rg-mcp
rg-mcp passed=72 failed=0
```

覆盖：

```
test tools::find_definition_tests::the_tool_is_visible_to_models ... ok
test tools::find_definition_tests::name_or_path_is_required_and_encoded ... ok
test tools::find_definition_tests::hostile_paths_and_missing_limit_are_refused_or_defaulted ... ok
test find_definition_asks_the_symbols_route_and_passes_the_answer_through ... ok
test an_unindexed_commit_is_a_tool_error_naming_the_reason ... ok
```

### 4.4 `cargo clippy --workspace --all-targets -- -D warnings`

```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 36.03s
```

### 4.5 `scripts/check.sh --quick`

```
==> cargo fmt --all --check
    (1s)
==> cargo clippy --workspace --all-targets -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 12.96s
    (13s)
==> checks passed in 14s (quick)
```

### 4.6 九条验收映射

| # | 验收项 | 断言位置 | 结果 |
|---|---|---|---|
| 1 | `name=index_commit` → 1 条、`exact`、path 正确 | `crates/rg-http/tests/ai_symbol_query_tests.rs:126`（`symbols_answers_name_path_limit_and_honesty_contract`） | 通过 |
| 2 | `name=index_` → 多条、全部 `name_match` | 同上 | 通过 |
| 3 | `path=src/lib.rs` → 该文件定义、无 `kind=impl` | 同上 | 通过 |
| 4 | 未索引 commit → 404 `SYMBOLS_NOT_INDEXED` + 提示，响应无回落痕迹 | `an_unindexed_commit_is_404_and_never_answered_from_head`：断言 message 含**被请求的** SHA、hint 含 `/index`、响应体不含结果名、不含另一个已索引 commit 的 SHA | 通过 |
| 5 | `limit=5` → ≤5 条；`limit=1` → 1 条；`limit=500` → 400 `INVALID_LIMIT` | 同上两个测试 | 通过 |
| 6 | `name`/`path` 都不给 → 400 | 同上 | 通过 |
| 7 | `partial` 存在且为 true、`limitations` 非空且含 svelte / Rust macro | 同上 | 通过 |
| 8 | `find_definition` 在 `list_tools` 可见；`call_tool` 结果与 HTTP 一致 | `tools::find_definition_tests::the_tool_is_visible_to_models`（工具表原文）、`read_tools_mcp::find_definition_asks_the_symbols_route_and_passes_the_answer_through`（断言请求路径与返回文档逐字段相同） | 通过 |
| 9 | 查 `impl` 行里的名字 → 不返回 | `symbol_query::tests::impl_rows_are_never_returned`（精确 / 前缀 / 按 path 三种）＋ HTTP 测试查 `std::fmt::Display for Widget` 与前缀 `std::fmt::` 均为 0 条 | 通过 |

### 4.7 `find_definition` 在 `list_tools` 里的原文

```json
{
  "name": "find_definition",
  "description": "Find where a symbol is declared in a repository, from the stored symbol index (not a live parse). Give 'name', 'path', or both: with 'name', rows whose name equals yours come first and rows that only start with it follow, marked confidence 'exact' or 'name_match' respectively; with 'path', every declaration of that file (confidence 'exact'). Only the commit that was indexed can be answered: if the commit you ask about was never indexed the call fails with SYMBOLS_NOT_INDEXED and a hint to index it, rather than silently answering from another commit. A missing row does not prove the symbol does not exist: 'partial' is true and 'limitations' names what a static parse cannot see (svelte markup, Rust macros and dyn Trait, TypeScript obj[key]/any, Python getattr, Go interface calls). Rust 'impl' rows are never returned because their name is a line of source, not an identifier.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "owner": { "type": "string", "description": "Repository owner" },
      "repo":  { "type": "string", "description": "Repository name" },
      "name":  { "type": "string", "description": "Symbol name; exact matches first, then prefix matches" },
      "path":  { "type": "string", "description": "File path, e.g. 'src/main.rs'; lists that file's declarations" },
      "ref":   { "type": "string", "description": "Git ref (branch/tag/commit). Defaults to the default branch." },
      "limit": { "type": "integer", "minimum": 1, "maximum": 200, "description": "Most rows to return (default 50, max 200; more than 200 is refused, not clamped)" }
    },
    "required": ["owner", "repo"]
  }
}
```

## 5. 偏差与决策记录

1. **MCP 走 REST，而不是直连 core**。`rg-mcp` 没有 `rg-core`/数据库依赖，`AppState` 只有
   `api_base`/`pat`，全部工具都是 REST 客户端。硬接一个数据库才算「调 core 函数」会引入第二种
   取数路径。因此 `find_definition` 请求的就是这个端点，逻辑仍只有 `find_symbols` 一份；
   `tool_find_definition` 的注释写明了这一点，MCP 测试断言「请求路径 + 返回文档与端点一致」。
2. **`partial` 恒为 true**，理由见 plan §5：没有「完整性证据」可比 `partial: false` 更弱；
   T28 未存失败文件数与语言分布，任何 commit 都无法证明完整。任务书要求「已知不完整 → true」，
   静态抽取对五门语言都不完整，所以对整表成立。
3. **前缀匹配用 `substr` 而非 `LIKE`**：三后端转义不一致，`_`/`%` 在符号名里是字面量。
   代价是不走 `name` 索引的 range seek，但 `(repo_id, commit_id)` 已把扫描限制在一个
   commit（本仓 12k 行）内，实测全部测试 <1 s。
4. **`limit` 越界 400 而非 clamp**（`INVALID_LIMIT`），并且 core 层也校验一次，
   非 HTTP 调用方同样拿不到静默截断。
5. **`commit_sha` 优先于 `ref`**：两者都给时取更具体的那个；都在调用前解析成 40 位 SHA。

## 6. 禁止项自查

- 未改 `Cargo.toml` / `Cargo.lock` / 任何迁移（`git status` 里没有这些文件）。
- 未改 `crates/rg-core/src/search/outline/`、`symbol_indexer.rs`（只调用 `SymbolIndexer`
  写入的数据，未改其代码）。
- 未改 `rg-core/src/{review,authorization,pull_request,issue,release,webhook}/`、
  `CLAUDE.md`、`ironforge-docs/`、`.github/`、`scripts/`、`deploy/`。
- 未做引用查询、影响面、编辑距离匹配；未新建能力注册表（只在既有的三处防漂移注册点各加一行）。
- owned paths 之外只改了四个**必须**的登记点：`crates/rg-http/src/lib.rs`（挂路由）、
  `crates/rg-http/src/openapi.rs`、`crates/rg-core/src/auth/route_meta.rs`（前两个由
  `openapi_registration_tests`、第三个由 `route_metadata_tests` 强制要求，
  对应任务书「capabilities 注册处（若存在防漂移要求）」）。
