# T31 执行记录 — 让 `partial` 反映真实索引质量

> 计划见 `docs/plans/T31-plan.md`，权威约束见 `tasks/T31.md`。
> 基线 `394c4c8`，分支 `agent/t31-partial-honesty`，worktree `../ironforge-t31`。

## 1. 开工核对

```
$ git fetch cnb && git rebase cnb/main
Successfully rebased and updated refs/heads/agent/t31-partial-honesty.
$ git log --oneline -2
394c4c8 docs(tasks): mark T30 completed and add T31 to the mirror index
bfcb317 docs(tasks): add T31 to make the symbol-index partial flag mean something
```

## 2. 改动

| 文件 | 改动 |
|---|---|
| `crates/rg-db/src/migrations/m20261118_000002_symbol_commit_completeness.rs` | **新建**：加三列 + 历史行回填 `-1`；`down` 删列；3 条单测（含 env 后端往返） |
| `crates/rg-db/src/migrations/mod.rs` | 注册第 107 条迁移 |
| `crates/rg-db/src/lib.rs` | 回滚窗口断言 `Some(32)` → `Some(33)`，注释 106/32 → 107/33 |
| `crates/rg-db/src/entities/symbol_commit.rs` | +3 字段、`UNKNOWN_FILES_TOTAL = -1` |
| `crates/rg-core/src/search/symbol_indexer.rs` | `Completeness`（+`is_partial`）、`Snapshot.stats`、逐文件统计、`upsert_commit` 覆盖计数 |
| `crates/rg-core/src/search/symbol_query.rs` | `STATIC_LIMITATIONS`、`measured_limitations`、`commit_stats_of`、真实 `partial`、`integer()` 改读 `i32`、`substr` 长度改 `i32` 绑定、单测 7 → 9 条 |
| `crates/rg-core/tests/symbol_indexer_tests.rs` | +4 条统计测试、+1 条 env 后端查询往返；`measure_*` 打印三计数 |
| `crates/rg-http/tests/ai_symbol_query_tests.rs` | 原「恒 partial」断言按新语义改为 `false` + 静态限制仍在；新增「有解析失败 → true 且带数字」 |
| `docs/plans/T31-plan.md`、`docs/plans/T31-task.md` | 本任务文档 |
| `tasks/T31.md` | `State` → `completed` |

未改：`Cargo.*`、`crates/rg-core/src/search/outline/`、`crates/rg-core/src/{review,authorization,pull_request,issue,release,webhook}/`、
`crates/rg-http/src/api/ai.rs`、`CLAUDE.md`、`ironforge-docs/`、`.github/`、`scripts/`、`deploy/`、`rg-mcp/`。

## 3. 迁移

- 总条数 **107**（原 106），`down` 窗口 **33**（原 32）。
- 列：`files_total` / `files_failed` / `files_truncated`，均 `INTEGER NOT NULL DEFAULT 0`。
- 历史行：`files_total = -1`（`UNKNOWN_FILES_TOTAL`）表示「未统计」，读侧视为 `partial: true`。
- 三后端 up+down+回填均实测（见 §6）。

## 4. 语义（改动前后）

| 场景 | 改动前 | 改动后 |
| --- | --- | --- |
| 干净 commit | `partial: true` | `partial: false`，静态限制仍列出 |
| 有 3 个文件解析失败 | `partial: true`（无数字） | `partial: true` + `"3 files failed to parse and are only partially indexed"` |
| 2 个文件超 item 上限 | `partial: true`（无数字） | `partial: true` + `"2 files exceeded the item cap and were truncated"` |
| 迁移前的旧行 | `partial: true` | `partial: true` + `"this commit was indexed before completeness stats were recorded; completeness is unknown"` |
| svelte 文件很多但全部解析干净 | `partial: true` | `partial: false`（svelte 那条只在 `limitations`，不进 `partial`） |

静态限制（5 条，永远出现）：svelte `<script>`、Rust 宏/dyn Trait、TS `obj[key]`/`any`、Python `getattr`、Go 接口。
删除：原「parse failures are not recorded per commit …」——现在记录了，这句话不再成立。

## 5. 本仓实测（`measure_this_repository_symbols`）

```
$ cargo test -p rg-core --test symbol_indexer_tests measure_this_repository_symbols -- --ignored --nocapture
commit: 394c4c8f28c8290c9301a33c333e4b564fcaa8fa
definitions_written: 12361
completeness (files_total, files_failed, files_truncated): (1280, 16, 0)
symbol_definitions rows: 12361
impl rows: 685
elapsed: 4.69s
```

**1280 个文件参与索引，16 个解析失败，0 个被截断** → 本仓当前 HEAD 的符号索引是 `partial: true`，
且失败数与 T27 实测的 16 个 `web/**/*.test.svelte.ts` 一致。这也说明：字段不再是恒真，
而是「这个仓库确实有 16 个文件没解析干净」。

## 6. 验收命令（实际输出）

```
$ cargo test -p rg-db
test result: ok. 70 passed; 0 failed; 0 ignored
test result: ok. 5 passed; 0 failed; 0 ignored
test result: ok. 12 passed; 0 failed; 0 ignored
test result: ok. 5 passed; 0 failed; 0 ignored
test result: ok. 0 passed; 0 failed; 1 ignored
test result: ok. 15 passed; 0 failed; 0 ignored
test result: ok. 1 passed; 0 failed; 0 ignored (doc-tests)

$ cargo test -p rg-core
test result: ok. 414 passed; 0 failed; 1 ignored
（其余 11 个 test binary 全绿：1/3/0/8/2/4/2/4/9/12/1，0 failed）

$ cargo test -p rg-http --test ai_symbol_query_tests --test ai_index_route_tests
ai_index_route_tests:      4 passed; 0 failed
ai_symbol_query_tests:     3 passed; 0 failed
  - symbols_answers_name_path_limit_and_honesty_contract ... ok
  - a_commit_with_a_parse_failure_is_partial_and_names_the_count ... ok
  - an_unindexed_commit_is_404_and_never_answered_from_head ... ok

$ cargo clippy --workspace --all-targets -- -D warnings
Finished `dev` profile (0 warnings)

$ scripts/check.sh --quick
==> checks passed in 2s (quick)
```

跨后端（`IRONFORGE_TEST_DATABASE_URL`，docker `postgres:16` / `mysql:8.4`）：

```
$ IRONFORGE_TEST_DATABASE_URL=postgres://ironforge:secret@127.0.0.1:55432/ironforge \
    cargo test -p rg-db --lib symbol_commit_completeness
3 passed（含 round_trip：up → down → 插历史行 → up → files_total = -1）

$ IRONFORGE_TEST_DATABASE_URL=mysql://root:secret@127.0.0.1:53306/ironforge \
    cargo test -p rg-db --lib symbol_commit_completeness
3 passed（同上，含 DROP COLUMN 与回填）

$ IRONFORGE_TEST_DATABASE_URL=<pg> cargo test -p rg-core --test symbol_indexer_tests
9 passed; 1 ignored（write_path + symbol_query 两个 env 往返都过）

$ IRONFORGE_TEST_DATABASE_URL=<mysql> cargo test -p rg-core --test symbol_indexer_tests
9 passed; 1 ignored（同上）
```

PG/MySQL 侧确认：`seaql_migrations` 最后一条是 `m20261118_000002_symbol_commit_completeness`，
`symbol_commits` 三列类型分别为 `integer`/`int`，`NOT NULL DEFAULT 0`。

## 7. 6 条验收映射

| # | 要求 | 实现 | 证据 |
| --- | --- | --- | --- |
| 1 | 干净 commit → `partial == false`，静态限制仍在，无 "N files failed" | `Completeness::is_partial` + `STATIC_LIMITATIONS` | `symbol_query.rs::a_measured_clean_commit_is_not_partial_and_still_states_the_static_limits`；`symbol_indexer_tests::a_clean_commit_is_recorded_as_complete_and_reads_back_not_partial`；`ai_symbol_query_tests` §7 断言 `partial == false` |
| 2 | 有失败 → `true` + 具体数字 | `measured_limitations` | `symbol_indexer_tests::parse_failures_are_counted_and_reindexing_overwrites_the_counters`；`ai_symbol_query_tests::a_commit_with_a_parse_failure_is_partial_and_names_the_count`；本仓实测 16 |
| 3 | 历史行 → `true` + "completeness is unknown" | 迁移回填 `-1` + `measured_limitations` | `symbol_query.rs::a_row_without_stats_is_partial_and_says_completeness_is_unknown`；`symbol_indexer_tests::a_row_without_stats_stays_partial_and_says_why`；迁移单测 `existing_rows_are_marked_unknown_not_complete` |
| 4 | 重复索引覆盖不累加 | `upsert_commit` 的 entity `update` | `parse_failures_are_counted_and_reindexing_overwrites_the_counters` 连跑两次断言 `(2, 1, 0)`；迁移单测断言守卫不改写 `42` |
| 5 | SQLite/PG/MySQL up+down | 迁移 + env 往返测试 | §6 三行输出 |
| 6 | 既有测试按新语义改，不删断言 | `ai_symbol_query_tests` §7 改为 `false` 并补「无失败」断言；`symbol_query.rs` 旧测重写为 4 条 | §6 rg-http 3 passed |

## 8. 遗留

- `crates/rg-http/src/api/ai.rs:1076` 的 OpenAPI 描述仍写「`partial` is true … unrecorded parse failures」，
  已过时；`ai.rs` 属禁改文件，留待后续任务（响应结构未变）。
- 额外修复：`symbol_query.rs` 里两处只有 SQLite 验证过的跨后端问题（`INT4` 解码、`substr` 长度参数类型），
  详见 `T31-plan.md` §5。不修则新列在 PG/MySQL 上无法读出。
