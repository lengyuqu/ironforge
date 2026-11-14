# T09 Task 确认 — [#39] 代码智能 · 结构化大纲（抽取层）

> 权威状态在 `tasks/T09.md`；本文件记录约束确认、触碰路径与风险。执行细节与依赖核查见 `docs/plans/T09-plan.md`。

## 1. 约束确认

| 约束 | 结论 |
| --- | --- |
| 只做 #39 第一步的抽取层，不做端点 / token 裁剪 / 路由 / 符号图 / 向量 / MCP | 是，未触碰 `crates/rg-http/`、`rg-mcp`、路由与迁移 |
| 本任务独占 `Cargo.*` | 是，只改 `[workspace.dependencies]` + `crates/rg-core/Cargo.toml` + `Cargo.lock` |
| `tree-sitter` 0.25 系列 | 是，解析到 0.25.10 |
| 五种 grammar：Rust / TypeScript（含 TSX）/ Python / Go / Svelte | Rust/TS/Python/Go 为真 grammar；Svelte 按授权降级（`tree-sitter-svelte` 仅 0.10.2，依赖 `tree-sitter ^0.20.6`，与 0.25 FFI 不兼容） |
| 依赖版本先核实兼容矩阵，不盲写 | 是：`cargo add --dry-run` + crates.io dependencies；全图仅 `tree-sitter-language 0.1.9` |
| 不引入 syn / tantivy / 向量库 | 是 |
| `OutlineItem` / `Outline` / `ItemKind` 按定义 | 是；`Language` 增加 `Other` 变体表示未知扩展名并实现 `Default`（见 plan §3.6） |
| `extract_outline(path, source) -> Outline` 按扩展名判语言 | 是 |
| 真结构化（tree-sitter query，非正则） | 是；层级由 AST 字节区间重建，`impl`/class 含方法、Python 嵌套、Go method、Svelte script 块 |
| fail-soft：语法错误返回 `error` + 空/部分 items，不 panic、不返回 Err | 是；`has_error()` 保留部分结果；未知/二进制/超大均走错误字段 |
| 单文件 > 512 KiB 跳过（`error: Some("file too large")`） | 是，`MAX_FILE_BYTES = 512 * 1024`，理由见 plan §3.5 |
| 单文件 item > 2000 置 `truncated: true` | 是，`MAX_ITEMS = 2000`，理由见 plan §3.5 |
| `signature` 只放首行，不塞函数体 | 是；首行折叠空白 + 200 字符上限，测试断言不含 body |
| 索引器抽象成 trait，FTS 与 Outline 各一个 impl | 是，`search/indexer.rs` 的 `SearchIndexer`（`supports` + `extract`）；`CodeIndexer` 与 `OutlineIndexer` |
| 不破坏现有 FTS 行为与调用点 | 是；`CodeIndexer` 仅两个 helper 提到 `pub(crate)`，async 方法未改 |
| `cargo test -p rg-core` 全绿 | 是，343 lib + 11 webhook + 1 collab + doc，0 失败 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 是，0 warning |
| `scripts/check.sh --quick` | 是，passed |
| 只改 owned paths | 是，清单见 §2 |
| 禁止改 `crates/rg-http/`、migrations、`CLAUDE.md`、`ironforge-docs/`、`.cnb.yml`、`.github/`、`scripts/`、`deploy/` | 是，全部未触碰 |
| 只推分支，不 push main，不 force-push | 是，只推 `agent/t09-outline` |

## 2. 触碰路径

生产代码：

- `Cargo.toml`（仅 `[workspace.dependencies]`，新增 5 条依赖与注释）
- `Cargo.lock`（新增 7 个 package）
- `crates/rg-core/Cargo.toml`（新增 5 条 workspace 依赖）
- `crates/rg-core/src/search/outline/mod.rs`（新建）
- `crates/rg-core/src/search/outline/queries.rs`（新建）
- `crates/rg-core/src/search/outline/tests.rs`（新建）
- `crates/rg-core/src/search/indexer.rs`（新建）
- `crates/rg-core/src/search/mod.rs`（导出两个新模块）
- `crates/rg-core/src/search/code_indexer.rs`（仅 `infer_language` / `should_index` 改 `pub(crate)`）

测试：内嵌于 `outline/tests.rs` 与 `indexer.rs`（未新建独立 integration test）。

文档：

- `docs/plans/T09-plan.md`（新建）
- `docs/plans/T09-task.md`（本文件，新建）
- `tasks/T09.md`（仅 `State` → `completed`）

未触碰：`crates/rg-http/`、`crates/rg-db/src/migrations/`、`crates/rg-mcp/`、`CLAUDE.md`、`ironforge-docs/`、`.cnb.yml`、`.github/`、`scripts/`、`deploy/`。

## 3. 依赖

| 依赖 | 版本 | 用途 |
| --- | --- | --- |
| `tree-sitter` | 0.25.10 | 解析运行时 |
| `tree-sitter-rust` | 0.24.2 | Rust |
| `tree-sitter-typescript` | 0.23.2 | TypeScript / TSX / JS / Svelte script |
| `tree-sitter-python` | 0.25.0 | Python |
| `tree-sitter-go` | 0.25.0 | Go |

未加 `tree-sitter-svelte`（不兼容）。传递新增 `tree-sitter-language 0.1.9`、`streaming-iterator 0.1.9`。`Cargo.lock` 共 +7 package / +72 行。

## 4. 风险确认

| 风险 | 处置 |
| --- | --- |
| Svelte 仅降级支持 | 已在模块文档、plan、本文件标注；script 块是结构化 AST，markup 不产出假数据 |
| grammar query 节点名升级漂移 | query 编译失败走 `error` 字段；测试覆盖五语言，升级 grammar 先跑 outline 测试 |
| 超大/密集文件拖垮索引 | 512 KiB 跳过 + 2000 item 截断 + `truncated` 标志 |
| 语法错误文件让调用方崩 | 不返回 `Result`；`has_error` 时保留部分 items；单测覆盖半截 Rust 与 NUL 二进制 |
| 破坏现有 FTS | 只提升两个 helper 可见性；`cargo test -p rg-core` 343 项全绿，clippy 干净 |
| TSX 与 TS 共享 query 静默失效 | 已实测并修正为分别编译 query，TSX 测试覆盖 |
| 依赖体积 | 只增 7 个 package，其中 4 个 C grammar 首次编译较慢，无运行时服务依赖 |
