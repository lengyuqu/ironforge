# T31 执行计划 — 让 `partial` 反映真实索引质量

> 权威约束见 `tasks/T31.md`。基线 `394c4c8`（rebase 后），分支 `agent/t31-partial-honesty`，worktree `../ironforge-t31`。
> 本文件记录三个设计决定：历史行怎么标记「未统计」、为什么把能力边界与数据缺陷拆开、三后端怎么验证。

## 1. 问题

T29 的 `find_symbols` 返回 `partial: true` 恒真，理由是「T28 没记录每个 commit 的解析失败数」。
这个理由本身是诚实的，但代价是**字段零信息量**：干净索引和有 16 个文件解析失败的索引，调用方看到的响应完全一样。

失败信息其实一直存在，只是被丢掉了：`Outline`（`crates/rg-core/src/search/outline/mod.rs:166`）同时带
`error: Option<String>`（解析失败原因，items 可能仍有部分结果）和 `truncated: bool`（命中 2000 item 上限）。
`SymbolIndexer::extract` 把它们压平成 `Vec<SymbolDefinition>` 时正好把这两个信号扔了。

所以本任务不动 `outline` 模块，只在写侧统计、读侧消费。

## 2. 历史行不能假装完整（哨兵 vs `stats_known`）

新列都是 `INTEGER NOT NULL DEFAULT 0`。如果只加列，迁移前写入的行会读成
`files_total = 0, files_failed = 0` —— 等价于断言「这次索引读了 0 个文件、0 个失败」，是一个**旧版本从未做过的完整性承诺**。

两种可选方案：

| 方案 | 判定 | 代价 |
| --- | --- | --- |
| `stats_known BOOLEAN NOT NULL DEFAULT FALSE` | 读侧 `known && failed == 0 && truncated == 0` | 多一列；`known = true` 但三计数为默认 `0` 的组合仍然可能被误读 |
| **`files_total = -1` 哨兵**（采用） | `total < 0` 即「未统计」 | 需要约定负值语义 |

选哨兵，理由：

- 一个列而不是两个，读者只需看一个字段就能区分「有测量」与「没测量」；
- `files_total` 的合法取值域是 `[0, N]`，负值在此域外，不可能与真实计数撞车（`0` 是真实测量：「一个可索引文件都没有」）；
- `has_column` 守卫让回填只在**刚加列的那一次**执行，重跑 `up` 不会把已测好的统计改回未知（`up_is_idempotent_and_down_drops_the_columns` 断言 `42` 不会被改写）。

哨兵常量放在实体旁边（`crates/rg-db/src/entities/symbol_commit.rs::UNKNOWN_FILES_TOTAL = -1`），
因为写侧（迁移）和读侧（`symbol_query`）都要用它，放在任一侧都会让另一侧反向依赖。

`down` 删三列后，行仍在；再次 `up` 时这些行又变回未知 —— 这也是对的：统计值确实已经不存在了。

## 3. 能力边界 vs 数据缺陷

原来的 `LIMITATIONS` 把两件不同的事混在一张表里：

1. **静态语言限制**：svelte 只解析 `<script>`、Rust 宏与 dyn Trait、TS `obj[key]`/`any`、Python `getattr`、Go 接口。
   这些对**每个 commit** 都成立，和这次索引干不干净无关。
2. **这次索引的具体缺陷**：某几个文件解析失败/被截断。

T29 把第 1 条也当成 `partial` 的理由之一，于是「仓库里 svelte 文件多」就足以恒 `partial: true` ——
和原来的恒 `true` 一样没有信息量，只是换了个说法。

本任务的拆分：

- `STATIC_LIMITATIONS`（5 条）**永远**出现在 `limitations` 里。它们是能力边界，不是缺陷；`partial: false` 不等于「tree-sitter 看得见一切」。
- `partial` **只**反映本次索引完整性：`total < 0 || failed > 0 || truncated > 0`。
- 动态句子只在对应计数非零时追加，和 `partial` 永远一致（见 `symbol_query.rs::measured_limitations`）。

为消除误导，原表里那条 **`"parse failures are not recorded per commit: …"` 被删除** ——
它现在已经不成立了。保留一条陈述「我们没有记录」的静态限制，会让调用方以为数据仍然缺失。

## 4. 写侧统计口径

`collect_snapshot` 直接从 `OutlineIndexer.extract` 取 `Outline`（不再经由 `SymbolIndexer::extract` 压平），
所以能在压平前读到 `error` / `truncated`：

- `files_total`：进入 `blobs` 列表的每个文件（已被 `Language::from_path(...).is_supported()`
  与 `MAX_FILE_BYTES` 过滤）都计 1，作为两个失败计数的分母；
- `files_failed`：`outline.error.is_some()`；此外 **读不出来的文件也算失败**
  （`find_object` / `try_into_blob` / `from_utf8` 失败）。它们不是「部分索引」，而是完全没被索引，
  是更严重的缺口；把它们排除在 `files_total` 外会让分母缩小、指标变好看，这正是本任务要消灭的自我安慰。
  动态句子沿用任务书规定的措辞（`N files failed to parse …`），因为对调用方而言「没进索引」与「解析失败」是同一类损失。
- `files_truncated`：`outline.truncated`（item 数 > `MAX_ITEMS = 2000`）。
- 不支持的语言（如 `README.md`）在 `collect_blobs` 阶段就被排除，既不计入总数也不计失败 —— 它们从来不在索引范围内。

重复索引时 `upsert_commit` 覆盖三个计数（entity `update` 只写 `Set` 的字段），
与「先删该 commit 的 definitions 再写」保持同一语义：统计描述的是**当前实际存着的那份索引**。

`IndexReport` 没有新增字段：任务书明确要求不把 `files_total` 暴露成新响应字段，`partial` + `limitations` 已足够。

## 5. 读侧顺带修掉的两个跨后端缺陷

为了让「新列在 PG/MySQL 上也能读」这件事可验证，新增了
`symbol_query_round_trip_on_the_backend_from_the_environment`
（`crates/rg-core/tests/symbol_indexer_tests.rs`）。它第一次把 `find_symbols` 放到 PG/MySQL 上跑，暴露出 T29 遗留的两处只有 SQLite 测过的问题：

1. `integer()` 用 `try_get_by_index::<i64>` 解码 `INTEGER` 列。PostgreSQL 把 `INTEGER` 映射为 `INT4`，
   sqlx 报 `ColumnDecode mismatched types INT8 vs INT4`。改为按列真实宽度读 `i32`（三后端都接受）。
2. `substr(name, 1, ?)` 的长度参数按 `i64` 绑定，PG 解析不到 `substr(character varying, integer, bigint)`。
   改为 `i32` 绑定（`symbol_query.rs` 内的注释写明了原因）。

这两处都在本任务 owned path（`symbol_query.rs` / 测试文件）内，且不修就无法完成「新列在 PG/MySQL 可读」的验证，故一并修复。

## 6. 迁移与三后端

`m20261118_000002_symbol_commit_completeness`（第 **107** 条，`down` 窗口 **33**）：

- `up`：`has_column("files_total")` 为假 → 加列（`INTEGER NOT NULL DEFAULT 0`）→
  立即 `UPDATE symbol_commits SET files_total = -1`（此刻表里每一行都是旧版本写的，`0` 只是列默认值，不是测量）。
  `files_failed` / `files_truncated` 各自 `has_column` 守卫。只加列、不回填，默认 `0` 是正确的中性值。
- `down`：按逆序 `drop_column`，每列 `has_column` 守卫。SQLite 3.35+ 支持 `DROP COLUMN`，且这三列不被索引/约束引用。
- `rg-db/src/lib.rs` 里「锚点之后所有迁移都回滚」的断言 `Some(32)` → `Some(33)`（注释同步 107/33）。
  该断言是**故意写死**的，防止窗口悄悄缩小。

验证矩阵：

| 后端 | 版本 | up | down | 回填 `-1` | 走 `find_symbols` 读计数 |
| --- | --- | --- | --- | --- | --- |
| SQLite | 3.4x（内存/文件） | ✅ | ✅ | ✅ | ✅ |
| PostgreSQL | 16（docker） | ✅ | ✅ | ✅ | ✅ |
| MySQL | 8.4（docker） | ✅ | ✅ | ✅ | ✅ |

## 7. 未做 / 遗留

- `crates/rg-http/src/api/ai.rs:1076` 的 OpenAPI 描述仍写着
  「`partial` is true and `limitations` names … unrecorded parse failures」。这句话现在**过时**了，
  但 `ai.rs` 是本任务禁改文件，留给后续文档任务处理（响应结构未变，只是值变准）。
- 不做按语言统计、不暴露 `files_total`、不改 `Outline`。
