# T28 执行记录 — 符号定义表最小闭环（#63 第二步）

> 计划见 `docs/plans/T28-plan.md`，权威约束见 `tasks/T28.md`。
> 基线 `c628195`，分支 `agent/t28-symbol-index`，worktree `../ironforge-t28`。

## 1. 开工核对

```
$ git fetch cnb && git rebase cnb/main
Current branch agent/t28-symbol-index is up to date.
$ git log --oneline -1
c628195 docs(tasks): add T28 for the symbol definition table (#63 step 2)
```

迁移数 106（本任务新增第 106 条，`Migrator::down` 31 → 32 步）。

## 2. 改动

| 文件 | 改动 |
|---|---|
| `crates/rg-db/src/migrations/m20261118_000001_create_symbol_graph.rs` | **新建**：两表 + 3 索引 + 4 条单测（含跨后端 round trip） |
| `crates/rg-db/src/migrations/mod.rs` | +2 行：注册为最后一条 |
| `crates/rg-db/src/entities/symbol_commit.rs`、`symbol_definition.rs` | **新建**实体 |
| `crates/rg-db/src/entities/mod.rs` | +2 行 |
| `crates/rg-db/src/lib.rs` | 回滚测试步数 31 → 32（`lib.rs` 仅步数，任务允许） |
| `crates/rg-core/src/search/symbol_indexer.rs` | **新建**：`SymbolIndexer`（trait 实现）、`index_commit`、`retention_depth`、`IndexReport` + 3 条单测 |
| `crates/rg-core/src/search/mod.rs` | +1 行 |
| `crates/rg-core/tests/symbol_indexer_tests.rs` | **新建**：幂等、修剪、未知名 commit、跨后端写入、本仓实测（`#[ignore]`） |
| `crates/rg-http/src/api/ai.rs` | 改造 `ai_index_repository`（写权限、`commit_sha` 入参、返回 report） |
| `crates/rg-http/tests/ai_index_route_tests.rs` | +3 条测试（403 / 404 / 200） |
| `docs/plans/T28-plan.md`、`docs/plans/T28-task.md` | 本任务文档 |
| `tasks/T28.md` | `State` → `completed` |

**没有**：`Cargo.toml` / `Cargo.lock`、`crates/rg-core/src/search/outline/`、`crates/rg-http/src/lib.rs`
（路由早已存在）、`CLAUDE.md`、`ironforge-docs/`、`scripts/`、`deploy/`、`.github/`、`rg-mcp/`。

## 3. 实测（本仓 `c628195`，release）

```
$ cargo test -p rg-core --release --test symbol_indexer_tests \
    measure_this_repository_symbols -- --ignored --nocapture
commit: c62819589fd8cb2147f5b008d478721493354e3b
definitions_written: 12208
symbol_definitions rows: 12208
impl rows: 678
db growth: 3760128 B (3.59 MiB)
elapsed: 947.36ms
  symbol_definitions: 1552 KiB
  uq_symbol_definitions_anchor: 1072 KiB
  idx_symbol_definitions_path: 704 KiB
  idx_symbol_definitions_lookup: 360 KiB
  uq_symbol_commits_repo_sha: 4 KiB
  symbol_commits: 4 KiB
```

- **行数 12,208 = 设计文档实测值（偏差 0.0%）**；`impl` 行 678 = 设计文档 §5 的 678。
- 索引 1,273 文件耗时 **0.95 s**（串行，未加并行依赖）。
- 体积 **3.59 MiB**，超出 3.5 MiB 门槛 0.09 MiB。原因已定位：任务书要求的第三个
  `UNIQUE` 锚点（1,072 KiB）不在设计文档 2.61 MiB（2 索引）的估算里。详见 §6。

## 4. 验收（7 条）

### #1 本仓行数 12,208 ±5% —— 通过（12,208，100.0%）

见 §3 输出。

### #2 DB 增长 ≤ 3.5 MiB —— **未达标：3.59 MiB**

页级增长 918 页 × 4 KiB = 3,760,128 B。分解见 §3，冲突分析见 §6。不改 schema 的前提下无法
再压缩 112 KiB（唯一锚点是任务强制项）。

### #3 幂等 —— 通过

```
$ cargo test -p rg-core --test symbol_indexer_tests
test indexing_the_same_commit_twice_is_idempotent_and_keeps_impl_rows ... ok
test pruning_keeps_only_the_retention_window ... ok
test unknown_commit_is_not_found ... ok
test write_path_round_trip_on_the_backend_from_the_environment ... ok
test result: ok. 4 passed; 0 failed; 1 ignored
```

同一 commit 索引两次：`symbol_commits` 仍 1 行、`symbol_definitions` 行数不变、
`commit_id` 复用（测试断言）。

### #4 修剪 D+2 → 剩 D —— 通过

`pruning_keeps_only_the_retention_window`：连续索引 8 个 commit（D=6），结果
`symbol_commits` = 6、累计 `pruned_commits` = 2、最旧 commit 的 definitions 行数 = 0、
最新 commit 的 2 行仍在。

### #5 迁移 SQLite / PostgreSQL / MySQL up+down —— 三后端全绿

本机有 docker 镜像 `postgres:16`、`mysql:8.4`，按验收要求实际启动容器跑：

```
$ IRONFORGE_TEST_DATABASE_URL="postgres://postgres:test@127.0.0.1:55432/ironforge_t28" \
    cargo test -p rg-db --lib migrations::m20261118
test result: ok. 5 passed; 0 failed

$ IRONFORGE_TEST_DATABASE_URL="mysql://root:test@127.0.0.1:53306/ironforge_t28" \
    cargo test -p rg-db --lib migrations::m20261118
test result: ok. 5 passed; 0 failed
```

PostgreSQL 侧 `\d symbol_definitions` 确认 3 个索引 + 外键 `ON DELETE CASCADE` 都在。
MySQL 侧 `SHOW CREATE TABLE` 确认 `UNIQUE KEY uq_symbol_definitions_anchor
(commit_id, path(400), name, kind, start_line)`（InnoDB 3072 B 限制的前缀）与
`utf8mb4`。

写入路径也在同一批后端上跑过（不只是迁移）：

```
$ IRONFORGE_TEST_DATABASE_URL=… cargo test -p rg-core --test symbol_indexer_tests
test write_path_round_trip_on_the_backend_from_the_environment ... ok   # PG / MySQL 各一次
```

### #6 端点 403 / 200 / 404 —— 通过

```
$ cargo test -p rg-http --test ai_index_route_tests
test ai_index_route_is_mounted_and_indexes_a_repository ... ok
test index_requires_write_access ... ok        # 可读不可写的用户 -> 403
test index_unknown_commit_is_not_found ... ok  # 全 0 SHA -> 404，且两表都不写
test index_returns_the_symbol_report ... ok    # 200 + commit_sha/commit_id/definitions_written/pruned_commits
```

### #7 `kind='impl'` 行确实入表 —— 通过

- 单测 `flattens_children_and_keeps_impl_rows`：`impl` 行在 `extract` 结果里；
- 集成测试：`SELECT COUNT(*) … WHERE kind='impl'` = 2（fixture 两个 impl 块），
  且存在 `name LIKE '% for %'` 的行（名字不是标识符）；
- 端点测试：`= 1`，与 fixture 一致；
- 本仓实测 **678** 行（与设计文档 §5 的 678 一致）。

## 5. 跨后端补充：三个只有真跑才会暴露的坑

1. **MySQL `DROP INDEX IF EXISTS` panic**：`down` 改为按外键顺序删表，不再逐个删索引。
2. **MySQL 拒绝删「被外键借用」的索引**（error 1553）：唯一锚点以 `commit_id` 开头，被 FK
   当成支撑索引；逐个删索引这条路在 MySQL 上根本不成立。
3. **PostgreSQL 实体解码 `INT8 vs INT4`**：实体 `id`/`commit_id` 用 `i32`，与 DDL 的
   `INTEGER` 对齐（`repo_id` 仍为 `i64`/BIGINT）。

## 6. 偏差与未做项

| 项 | 说明 |
|---|---|
| **体积 3.59 MiB > 3.5 MiB** | 任务书要求的 3 个索引（含唯一锚点 1,072 KiB）与设计文档「2 索引 = 2.61 MiB」的预算冲突。选择保 schema：锚点是任务明确要求的幂等保证。若优先过门槛，删 `idx_symbol_definitions_path`（锚点前两列已能服务同一查询）可降到 ≈2.9 MiB；本任务未擅自删。 |
| `index_commit` 多一个 `repo_path` 参数 | 任务书写的是 `index_commit(db, repo_id, commit_sha)`；读提交树必须有仓库路径（`CodeIndexer::index_repository` 同形）。默认分支也从调用方传入。 |
| `IndexReport` 多一个 `commit_sha` | 返回值里带解析后的 40 位 SHA，避免端点再解析一次 ref；`commit_id`/`definitions_written`/`pruned_commits` 与任务书一致。 |
| `down` 不显式删索引 | 任务书写「先删索引再删表」；MySQL 两条都不允许（见 §5）。改为先删子表再删父表，索引随表消失，三后端一致。已说明。 |
| 端点返回多两个字段 | 保留 `indexed_files`（既有回归测试钉死），并把符号报告平铺进同一响应；未新开路由。 |
| 未做（任务明确排除） | 引用表 / postings / 影响面 / `confidence` 列 / 深历史增量 / 按需 PR head / 每小时 sweep / 并行依赖 |

## 7. 验收命令输出

```
$ cargo test -p rg-db
test result: ok. 67 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 13.60s
test result: ok. 5/12/5/15 passed; 0 failed （其余测试二进制）
test result: ok. 1 passed（doc-test）

$ cargo test -p rg-core
test result: ok. 404 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 16.50s
test result: ok. 4 passed; 0 failed; 1 ignored（symbol_indexer_tests）
test result: ok. …（其余集成测试二进制全部 0 failed）

$ cargo test -p rg-http
（100 个测试二进制，全部 `test result: ok … 0 failed`，末尾 doc-tests 0 failed / 2 ignored）

$ cargo clippy --workspace --all-targets -- -D warnings
Finished `dev` profile [unoptimized + debuginfo] target(s) in 19.70s（无 warning）

$ scripts/check.sh --quick
==> cargo fmt --all --check
    (1s)
==> cargo clippy --workspace --all-targets -D warnings
    Finished `dev` profile … in 19.70s
    (20s)
==> checks passed in 21s (quick)
```
