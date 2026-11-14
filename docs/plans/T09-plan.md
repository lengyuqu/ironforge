# T09 执行计划 — [#39] 代码智能 · 结构化大纲（抽取层）

> 权威约束见 `tasks/T09.md`。基线 `90eafed`，分支 `agent/t09-outline`，worktree `../ironforge-t09`。
> 本文件记录依赖兼容性核查、五语言支持矩阵、上限口径、失败降级与实跑结果。

## 1. 交付边界

做：

1. `tree-sitter` 0.25 + 四个 grammar（Rust / TypeScript / Python / Go）接入 workspace 依赖。
2. `rg-core::search::outline`：`extract_outline(path, source) -> Outline`，按扩展名选语言，tree-sitter query 抽取真实声明层级。
3. `rg-core::search::indexer`：`SearchIndexer` trait + `CodeIndexer`（FTS）与 `OutlineIndexer` 两个 impl。
4. 单测覆盖五语言真实语法特性、语法错误、超大文件、二进制、item 上限、签名单行、未知扩展名。

不做（T10 范围）：`/ai/repos/{o}/{r}/map` 端点、token 预算裁剪、路由三处、符号图（call/reference）、向量后端、MCP 工具。未改 `crates/rg-http/`、迁移、`CLAUDE.md`、`ironforge-docs/`、`.cnb.yml`、`.github/`、`scripts/`、`deploy/`。

## 2. 依赖兼容性核查

### 2.1 核查过程

1. `cargo add -p rg-core --dry-run <crate>` 查可用版本（本机 registry 是 rsproxy 镜像）。

   ```
   tree-sitter            0.27.2（最新）
   tree-sitter-rust       0.24.2
   tree-sitter-typescript 0.23.2
   tree-sitter-python     0.25.0
   tree-sitter-go         0.25.0
   tree-sitter-svelte     0.10.2
   ```

2. 逐个查 crates.io 的 dependencies，重点看 grammar 依赖的 `tree-sitter-language` 与 dev-dependency 的 `tree-sitter`：

   | crate | 版本 | `tree-sitter-language` | dev `tree-sitter` | 结论 |
   | --- | --- | --- | --- | --- |
   | `tree-sitter` | 0.25.10（固定 0.25 系列） | `0.1.9` | — | 运行时 |
   | `tree-sitter-rust` | 0.24.2 | `^0.1` | `^0.25` | 兼容 |
   | `tree-sitter-typescript` | 0.23.2 | `^0.1` | `^0.24`（仅 dev） | 兼容 |
   | `tree-sitter-python` | 0.25.0 | `^0.1` | `^0.25.8` | 兼容 |
   | `tree-sitter-go` | 0.25.0 | `^0.1` | `^0.25.8` | 兼容 |
   | `tree-sitter-svelte` | 0.10.2 | 无 | **`^0.20.6`（normal）** | **不兼容** |

3. 只在 `[workspace.dependencies]` 固定主版本，由 Cargo 解析具体版本。`cargo tree` 证实全图只有一个 `tree-sitter-language v0.1.9`：

   ```
   tree-sitter v0.25.10
   ├── regex v1.12.3
   ├── regex-syntax v0.8.10
   ├── streaming-iterator v0.1.9
   └── tree-sitter-language v0.1.9
   tree-sitter-typescript v0.23.2
   └── tree-sitter-language v0.1.9
   ```

### 2.2 Svelte 结论：降级实现

`tree-sitter-svelte` 唯一可用版本 0.10.2（2022-04）把 `tree-sitter ^0.20.6` 作为 **normal dependency**，其 `language()` 返回 0.20 的 `Language`；与 0.25 的 `Language` 是不同 crate 的类型，同时链接两套 `tree-sitter` 会在编译期冲突（正是 `tasks/T09.md` 警告的 FFI 不匹配）。

按任务授权采用降级方案，**没有退回整语言正则解析**：

- 文本扫描定位 `<script …>…</script>` 区域；
- 区域内容交给 `tree-sitter-typescript` 真 AST 解析（TS grammar 是 JS 超集），层级、签名、行号都是结构化的；
- 行号按 `<script>` 内容起始行偏移回 `.svelte` 文件；
- markup 不解析；Svelte 专有语法（runes、`$:`）对 TS grammar 不可见。

已在模块文档与报告里标注「Svelte 为降级实现，不保证 markup 准确」。未引入 `tree-sitter-svelte-next` 等计划外 crate（依赖范围由协调者裁决，不在本任务擅自扩大）。

### 2.3 依赖增量

`Cargo.lock` 新增 **7 个 package**（+72 行）：

| crate | 版本 | 直接/传递 |
| --- | --- | --- |
| `tree-sitter` | 0.25.10 | 直接 |
| `tree-sitter-rust` | 0.24.2 | 直接 |
| `tree-sitter-typescript` | 0.23.2 | 直接 |
| `tree-sitter-python` | 0.25.0 | 直接 |
| `tree-sitter-go` | 0.25.0 | 直接 |
| `tree-sitter-language` | 0.1.9 | 传递 |
| `streaming-iterator` | 0.1.9 | 传递 |

`regex`、`regex-syntax`、`cc`、`serde_json` 已在锁定图中，无额外版本。没有 syn / tantivy / 向量库。

## 3. 实现

### 3.1 文件

| 路径 | 内容 |
| --- | --- |
| `crates/rg-core/src/search/outline/mod.rs` | 模型（`Language` / `ItemKind` / `OutlineItem` / `Outline`）、`extract_outline`、上限与截断、按字节区间嵌套（`assemble`）、Rust `impl`/`trait` 子函数改判 `Method` |
| `crates/rg-core/src/search/outline/queries.rs` | 四语言 query、query 缓存、AST 收集、签名/行号/导出判定、Svelte `<script>` 降级 |
| `crates/rg-core/src/search/outline/tests.rs` | 15 个行为测试 |
| `crates/rg-core/src/search/indexer.rs` | `SearchIndexer` trait、`FtsDocument`、`CodeIndexer` impl、`OutlineIndexer` |
| `crates/rg-core/src/search/mod.rs` | 导出 `indexer`、`outline` |
| `crates/rg-core/src/search/code_indexer.rs` | 仅把 `infer_language` / `should_index` 提到 `pub(crate)`，行为不变 |

### 3.2 抽取方式

每种语言一个 tree-sitter query，capture 出 `@name` 与声明节点；收集为 `RawItem` 后按 `(start_byte, end_byte)` 排序，用「字节区间包含」的栈重建层级：

- Rust：`impl` 节点包含 `function_item` → 层级 + 改判 `Method`；`trait` 内 `function_signature_item` 同理。
- TS：`class_declaration` 包含 `method_definition`；`interface_declaration` 包含 `method_signature`。
- Python：`function_definition` 嵌套 `function_definition`（真实嵌套函数）。
- Go：`method_declaration` 独立类型；`type_spec` 的 `struct_type` / `interface_type` 分别映射 `Struct` / `Interface`。
- 同一位置重复 capture 会去重；未知扩展名在解析前返回错误。

Query 按语言分别编译并缓存（`OnceLock`）。TSX 必须使用自己的 query：tree-sitter 的 query 在编译期绑定符号 ID，TypeScript 的 query 不能复用于 TSX 树（实跑时 TSX 返回空 items，已修正）。

### 3.3 导出判定

| 语言 | 规则 |
| --- | --- |
| Rust | 声明首行以 `pub` 开头（含 `pub(crate)`） |
| TypeScript / Tsx / Svelte | 声明所在行的前缀 token 含 `export`（`export` 在 `export_statement` 父节点上，声明自身文本不含它） |
| Python | 名字不以 `_` 开头 |
| Go | 名字首字符大写 |

### 3.4 失败软着陆（fail-soft）

- 函数签名是 `extract_outline(...) -> Outline`，**不返回 `Result`**，调用方不会因坏文件崩。
- 未知扩展名 → `error: Some("unsupported language")`。
- `source.len() > 512 KiB` → `error: Some("file too large")`。
- 含 NUL 字节 → `error: Some("binary content")`。
- tree-sitter 解析出 `ERROR` 节点（`root_node().has_error()`）→ 保留已抽出的部分 items，同时 `error: Some("syntax errors in source")`；不 panic。
- Svelte `<script>` 内语法错误 → `error: Some("svelte: syntax errors in <script> block")`，其余块继续。

### 3.5 上限口径

| 上限 | 值 | 理由 |
| --- | --- | --- |
| 单文件 | 512 KiB | outline 是导航能力；超过 512 KiB 的手写源码极少，生成物/minified 文件的声明树是噪声。直接跳过比截断半个文件更诚实，也避免为无意义输入付解析成本。 |
| item 数 | 2000 / 文件 | 保护后续 T10 的 token 预算；512 KiB 内超过 2000 个声明已属病态。超出后保留前 2000 个并置 `truncated: true`，调用方能区分「完整」与「被截」。 |
| signature | 首行 + 200 字符 | 只放首行是硬约束：函数体绝不进入 signature。200 字符覆盖长泛型签名，同时防止单行 minified 文件把整个文件塞进一个签名。 |

### 3.6 模型偏差说明

`Language` 在任务给定的六个变体外增加 `Other` 并实现 `Default`。任务要求未知语言返回「`Language` 缺省」，但把未知文件标成某个真实语言会误导下游；`Other` + `error` 让调用方必须显式处理未知情况。其余字段与任务定义一致。

### 3.7 索引器抽象

`search/indexer.rs`：

```rust
pub trait SearchIndexer {
    type Artifact;
    fn name(&self) -> &'static str;
    fn supports(&self, path: &str) -> bool;
    fn extract(&self, path: &str, source: &str) -> Self::Artifact;
}
```

- `CodeIndexer`（FTS）→ `Artifact = FtsDocument`，`supports` 复用现有 `should_index`，`extract` 复用 `infer_language`；`CodeIndexer` 原有 async DB 方法一行未改，现有调用点（`rg-http/src/api/ai.rs`、`rg-cli`）保持可用。
- `OutlineIndexer` → `Artifact = Outline`，`supports` 基于扩展名，`extract` 调 `extract_outline`。
- 注释里写明意图：将来向量/embedding 后端作为第三个 impl 接入，不需要改调用方或 FTS 路径。
- trait 的 `extract` 无副作用（不碰 DB / 文件系统），持久化仍留在各 indexer 自己的 service 方法。

## 4. 五语言支持矩阵

| 语言 | 扩展名 | 语法后端 | 抽取的声明 | 测试钉住的真实特性 |
| --- | --- | --- | --- | --- |
| Rust | `.rs` | `tree-sitter-rust 0.24.2` | `Function`、`Method`、`Struct`、`Enum`、`Trait`、`Impl`（含方法）、`Module`、`Const`/`Static`、`Type`、`Macro` | `pub struct` + `impl` 两个方法 + trait 签名方法 + 嵌套 `mod` |
| TypeScript / JavaScript | `.ts` `.mts` `.cts` `.js` `.jsx` `.mjs` `.cjs` | `tree-sitter-typescript 0.23.2` | `Function`、箭头函数常量、`Class`、`Interface`（含方法签名）、`Enum`、`Type`、`Module`、`Method` | `export interface` 方法签名、`export class` 方法、`export function`、非导出箭头函数 |
| TSX | `.tsx` | 同上（`LANGUAGE_TSX` + 独立 query） | 同 TypeScript | `export function App(): JSX.Element` |
| Python | `.py` `.pyi` | `tree-sitter-python 0.25.0` | `Function`、`Class`（嵌套函数保持层级） | 两层嵌套函数、类方法、`_hidden` / `_private` 非导出 |
| Go | `.go` | `tree-sitter-go 0.25.0` | `Function`、`Method`、`Struct`、`Interface` | `type struct`、`type interface`、`func`、值接收者方法、大小写导出 |
| Svelte | `.svelte` | **降级**：`<script>` 区域 + TypeScript grammar | script 内的函数/类/方法 | `<script lang="ts">` 内 `export function`、`class` getter；行号偏移正确；无 script 时为空且无错误 |

## 5. 验收实跑

```
$ cargo test -p rg-core
running 343 tests
test result: ok. 343 passed; 0 failed; 0 ignored; ... (lib)
test result: ok. 1 passed; 0 failed; ... (collab_search_tests)
test result: ok. 0 passed; 0 failed; 2 ignored; ... (multi_backend_smoke，需要外部数据库)
test result: ok. 11 passed; 0 failed; ... (webhook_envelope_tests)
test result: ok. 1 passed; 0 failed; 5 ignored; ... (doc-tests)

$ cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 20.60s
（0 warning / 0 error）

$ scripts/check.sh --quick
==> cargo fmt --all --check            (2s)
==> cargo clippy --workspace --all-targets -D warnings   (0s)
==> checks passed in 2s (quick)
```

新增 17 个测试（outline 14 + indexer 3），lib 测试总数 326 → 343；原有 FTS / dialect 测试全部仍绿。

## 6. 风险与遗留

| 风险 | 处置 |
| --- | --- |
| Svelte markup 不解析 | 已按任务授权降级并在运行时/文档标注；script 内是结构化结果，markup 不产出假声明。 |
| query 节点名随 grammar 升级漂移 | query 编译失败时 `extract_outline` 返回 `error`，不会 panic；升级 grammar 必须先跑 `cargo test -p rg-core search::outline`。 |
| 2000 item 截断丢尾部 | `truncated: true` 显式暴露；T10 可据此提示用户。 |
| TSX query 与 TS 不通用 | 已各自编译缓存，测试覆盖。 |
| 新增依赖扩大构建时间 | 只增 7 个 package；grammar 是 C 编译，首次构建变慢，增量构建可接受。 |
| `CodeIndexer` 抽象只覆盖 per-file 行为 | 有意为之：`index_repository` 涉及 gix 与 DB，保持原样避免破坏 FTS；trait 目标是让未来后端能注册「能否处理 + 抽取」两件事。 |
