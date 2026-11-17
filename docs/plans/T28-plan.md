# T28 执行计划 — 符号定义表最小闭环（#63 第二步）

> 权威约束见 `tasks/T28.md`，参数来自 `ironforge-docs/architecture/symbol-graph-design-2026-11.md`
> （T27 实测）。基线 `c628195`，分支 `agent/t28-symbol-index`，worktree `../ironforge-t28`。
> 实际执行、验证输出与偏差见 `docs/plans/T28-task.md`。

## 1. 范围

只做**定义表**：迁移（31 → 32）、`SymbolIndexer`、写入 + 修剪、索引端点。

不做（设计文档已用实测数据否掉）：`symbol_references` / postings、影响面、`confidence` 列、
深历史增量（`symbol_file_state`）、按需 PR head、每分支 HEAD、LRU、每小时 sweep。

## 2. 迁移 `m20261118_000001_create_symbol_graph`

### 2.1 形状

| 表 | 列 | 约束 |
|---|---|---|
| `symbol_commits` | `id` INTEGER PK、`repo_id` BIGINT、`commit_sha` VARCHAR(40)、`created_at` timestamp | `UNIQUE (repo_id, commit_sha)`；FK `repo_id → repositories.id ON DELETE CASCADE` |
| `symbol_definitions` | `id` INTEGER PK、`repo_id` BIGINT、`commit_id` INTEGER、`path` VARCHAR(512)、`name` VARCHAR(255)、`kind` VARCHAR(32)、`start_line`/`end_line` INTEGER、`signature` TEXT NULL | FK `commit_id → symbol_commits.id ON DELETE CASCADE`；2 个索引（下） |

索引：

1. `idx_symbol_definitions_lookup (repo_id, commit_id, name)` — 按名查定义的主路径。
2. `uq_symbol_definitions_anchor UNIQUE (commit_id, path, name, kind, start_line)` — 幂等锚点，
   **同时是按文件查询的索引**。

**不建单独的 `(repo_id, commit_id, path)` 索引**（协调者裁决，2026-11-18）。理由：`commit_id`
在 `symbol_commits` 里唯一确定一个仓库，`repo_id` 在 definitions 表里只是反规范化冗余列；锚点的
前两列 `(commit_id, path)` 已经按 path 排序，`WHERE repo_id=? AND commit_id=? AND path=?` 和
「列出该文件的定义」都走它。实测这个索引占 704 KiB/commit（12,208 行），却服务不了锚点之外的
任何查询 —— 删掉后本仓从 3.59 MiB 降到 2.95 MiB。

查询计划实测（SQLite，12,000 行 + `ANALYZE`）：

```
EXPLAIN QUERY PLAN SELECT name, kind, start_line FROM symbol_definitions
  WHERE repo_id=1 AND commit_id=2 AND path='dir7/file7.rs';
-- SEARCH symbol_definitions USING INDEX uq_symbol_definitions_anchor (commit_id=? AND path=?)
```

空表/极小表时规划器会选 `lookup` 的 `(repo_id, commit_id)` 前缀，同样是一次索引查找；
两种计划都不需要额外索引。

`repo_id` 在 definitions 里是冗余列，故意不加 FK：`symbol_commits.repo_id` 已经级联到
`repositories`，两条路径都能在仓库删除时清干净，而查询侧省掉一次 join。

### 2.2 为什么 `kind`/`start_line` 在唯一键里

同一 commit 的同一文件里，`(path, name)` 不唯一（同名方法挂在两个 `impl` 下、同名 `const`），
所以锚点必须带上 `kind` 与 `start_line`。写入侧先在内存按同一四元组去重，再批量插入。

### 2.3 跨后端的三个坑（都实际踩到并在测试里钉住）

| 坑 | 现象 | 处理 |
|---|---|---|
| InnoDB 3072 字节索引键 | `utf8mb4` 下 `(commit_id, path(512), name(255), kind(32), start_line)` ≈ 3140 B > 3072 | 只在 MySQL 给 `path` 加 400 字符前缀（`index.col((Path, 400))`），SQLite/PG 用完整 `path`。先例：`m20260714_000001_create_attachments` |
| `INTEGER` 列 + 实体 `i64` | PostgreSQL 实体插入报 `INT8 vs INT4` 解码错误 | 实体 `id`/`commit_id` 用 `i32`，与 DDL 的 `INTEGER` 一致（`repo_id` 仍是 `i64`/BIGINT） |
| `DROP INDEX IF EXISTS` | MySQL 不支持，sea-query 直接 **panic** | `down` 不逐个删索引，只按外键顺序删表（SQLite/PG/MySQL 删表都会带走索引）；MySQL 还拒绝删「被外键借用的索引」，逐个删本来就不成立 |

### 2.4 `down` 策略与级联

`down` = 先 `symbol_definitions`、后 `symbol_commits`（外键顺序），`if_exists` 守卫。
`up` 幂等：`has_table` 分支做逐列 `has_column` 修复，四个索引各自 `has_index` 守卫。

**级联**：`symbol_commits` 删除时 definitions 由 FK `ON DELETE CASCADE` 带走；同时写入侧的修剪
**显式两表删**（先 definitions 再 commits），因为这条路径不能依赖后端把外键约束打开了。
迁移测试里另有一条「删 commits 行 → definitions 归零」的守门测试。

## 3. `D`（保留深度）的计算方式

采用设计文档 §3.2 的公式，不固定为 6：

```
bytes_per_commit ≈ 224 B × 本次写入的定义行数        // 224 B/行含 2 索引，§2.1 实测
D = clamp(⌊128 MiB / bytes_per_commit⌋, 1, 6)
```

- 12,208 行 → 2.67 MiB/commit → 47 → **D = 6**（本仓）；
- 95,900 行（10k 文件外推）→ 20.5 MiB → 6；
- 479,000 行（50k 文件外推）→ 102 MiB → **D = 1**。

代价为零：行数在写入前已经算完，纯整数除法，不需要额外测量或配置。空 commit（0 行）取 6。
`retention_depth` 有单元测试钉住这四个点。

保留集 = `git rev-list -n D <默认分支>`（gix `rev_walk`，`Sorting::ByCommitTime(NewestFirst)`，
**不 spawn git**）∪ {本次索引的 commit}。后者保证索引非默认分支的 commit 不会立刻删掉自己。

## 4. 写入 + 修剪（同一事务）

```
txn begin
  upsert symbol_commits (repo_id, commit_sha) -> commit_id     # SELECT 后 INSERT；唯一键兜底
  DELETE FROM symbol_definitions WHERE commit_id = ?           # 重复索引幂等
  INSERT INTO symbol_definitions ... (100 行/批，8 参数/行)
  prune: 该 repo 的 symbol_commits 中不在保留集的行
         DELETE definitions WHERE commit_id IN (...)
         DELETE symbol_commits WHERE id IN (...)
txn commit
```

- 批量大小 100：800 个绑定参数 < SQLite 历史上的 999 上限（`code_indexer` 先例）。
- 参数全部走 `rg_db::prepare_sql`，PostgreSQL 的 `$n` 由它改写。
- git 侧全部在**一个** `spawn_blocking` 里完成（`gix::Repository` 是 `!Send`），返回纯数据：
  定义行 + 保留集 + 解析后的 40 位 SHA。`/map` 与 `code_indexer` 都是这个形状。

## 5. 端点

`POST /ai/repos/{owner}/{name}/index` **已存在**（`crates/rg-http/src/lib.rs` 已注册，前缀确认是
`/ai`，经 `/api/v1` 挂在 `api_v1` 下），本任务改造而不新开路由：

| 项 | 改造前 | 改造后 |
|---|---|---|
| 权限 | `require_read` | **`require_write`**（403 不再是 200） |
| 入参 | `Json<Value>` 忽略 | `{ "commit_sha": "..." }`，缺省 = 默认分支；`commit_sha` 接受任意可解析 ref |
| 行为 | 只写 `code_fts` | 先符号索引（校验 commit、拿 `IndexReport`），再按同一 SHA 刷 `code_fts` |
| 返回 | `{ indexed_files }` | `{ indexed_files, commit_sha, commit_id, definitions_written, pruned_commits }` |
| 错误 | 404 仅仓库 | 未知 commit → 404（`CoreError::NotFound` → `AppError::NotFound`），无写权限 → 403 |

`indexed_files` 保留：既有回归测试（`ai_index_route_tests.rs` 的
`ai_index_route_is_mounted_and_indexes_a_repository`）钉死了这个字段。
符号索引先跑：commit 解析失败时 404 且两表都不写。

## 6. 依赖评估（结论：**不加任何依赖**）

| 需要 | 现成能力 | 结论 |
|---|---|---|
| 读提交树 / 遍历 blob | `gix`（`rg-core` 已有依赖） | 够用 |
| `rev-list -n D` | `gix::Repository::rev_walk` + `Sorting::ByCommitTime` | 够用，不 spawn git |
| tree-sitter 抽取 | `OutlineIndexer`（`crates/rg-core/src/search/`） | 直接复用，未改 `outline/` 一个字节 |
| 事务 / 批量插 | `sea-orm` + `rg_db::prepare_sql` | 够用 |
| 并行 | 不需要 | 实测 12,208 行 / 1,273 文件 **0.95 s**（release），串行足够 |

`Cargo.toml` / `Cargo.lock` 无改动。

## 7. 验收映射

| # | 验收项 | 手段 | 结果 |
|---|---|---|---|
| 1 | 本仓行数 12,208 ±5% | `#[ignore] measure_this_repository_symbols` | **12,208（100.0%，`c628195`）/ 12,301（+0.76%，`c1db0bf`，含本任务新增的源文件）** |
| 2 | DB 增长 ≤ 3.5 MiB | 同上，`page_count × page_size` 差值 | **2.95 MiB**（删冗余 path 索引后） |
| 3 | 幂等 | `indexing_the_same_commit_twice_…` + 跨后端 round trip | 通过 |
| 4 | 修剪 D+2 → 剩 D | `pruning_keeps_only_the_retention_window` | 通过（8 → 6，pruned=2） |
| 5 | 迁移 SQLite/PG/MySQL up+down | 迁移单测 + `IRONFORGE_TEST_DATABASE_URL`（本机 docker：postgres:16、mysql:8.4） | 三后端全绿 |
| 6 | 端点 403 / 200 / 404 | `ai_index_route_tests.rs` 三个新测试 | 通过 |
| 7 | `kind='impl'` 入表 | 单测 + 端点测试 + 本仓实测 678 行（`c628195`）/ 684 行（`c1db0bf`） | 通过 |

### 验收 #2 的决策记录

初版按任务书的三个索引实现，实测 **3.59 MiB**，超 3.5 MiB 上限 0.09 MiB。原因是 3.5 MiB 的预算
来自设计文档只含 **2 个**索引的 2.61 MiB 实测，而任务书的 schema 多了一个 UNIQUE 锚点（1,072 KiB）。

协调者裁决：**删 `idx_symbol_definitions_path`，保留 UNIQUE 锚点**。锚点不是索引优化，是数据库层的
幂等保证（应用层的「先删后插」只有一处代码保证）；path 索引则是重复覆盖。

删掉后的实测（`c1db0bf`，12,301 行）：

| 对象 | 体积 |
|---|---|
| `symbol_definitions` 表 | 1,568 KiB |
| `uq_symbol_definitions_anchor`（幂等锚点，兼按文件查询） | 1,080 KiB |
| `idx_symbol_definitions_lookup` | 380 KiB |
| `symbol_commits` + 唯一索引 | 8 KiB |
| **合计** | **3,036 KiB = 2.96 MiB**（页级增长 3,088,384 B = **2.95 MiB**） |

按 50k 文件档外推：2.95 MiB / 12,301 行 × 479,000 行 ≈ **115 MiB**（设计值 103 MiB），
而不是三索引的 141 MiB。
