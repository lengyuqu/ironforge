# T15 执行计划 — [#15 步骤3] ops 签名放宽：`pipeline_ops`

> 权威约束见 `tasks/T15.md`。基线 `6e477ea`，分支 `agent/t15-pipeline-ops`，worktree `../ironforge-t15`。
> 验收命令真实输出、触碰路径与函数级清单见 `docs/plans/T15-task.md`。

## 1. 交付边界

做：

1. `crates/rg-db/src/ops/pipeline_ops.rs`：37 个 `pub` 函数 + 1 个私有辅助函数（`job_is_schedulable`）共 38 个
   带 `db` 参数的函数，逐个判断后 **31 个**改成 `pub async fn f<C: ConnectionTrait>(db: &C, ..)`。
2. 12 个调用点的最小解引用（见 §4）。

不做（明确排除）：其它 ops 文件、`Cargo.*`、迁移、`CLAUDE.md`、`ironforge-docs/`、`.cnb.yml`、`.github/`、
`scripts/`、`deploy/`、`crates/rg-mcp/`；也不顺手把任何写路径搬进事务。

改动只落在 **3 个文件**（见 §5），外加本任务的 3 个文档。

## 2. 最终数字

| 项 | 数量 |
| --- | --- |
| `pipeline_ops.rs` 带 `db` 参数的函数 | 38 |
| 放宽为 `<C: ConnectionTrait>` | 31 |
| 保留 `&DatabaseConnection` | 7 |
| 改动前 `db: &DatabaseConnection` 出现次数 | 38 |
| 改动后残留 `db: &DatabaseConnection` 出现次数 | 7 |

38 → 31 放宽 / 7 保留，与协调者派单时的函数级分段预统计**完全一致**。判据不是 grep，而是
「函数体里是否直接调 `db.begin()`」：文件里恰好 7 处 `db.begin()`（l.307、398、719、763、1017、1134、1348），
一对一指向前述 7 个函数，其余 31 个函数体只用到 `ConnectionTrait` 提供的能力。

> **方法学备注（应协调者要求记录）**：T14 那次统计失误的根因是「用单行 grep 匹配 `db.begin()`」，
> 而仓库里 `db` 与 `.begin()` 不在同一行。本轮改用 **函数级分段解析**（先按 `fn` 边界切块，再在每个块内
> 找 `begin()` / `clone()` / `spawn`），初判就与编译结果吻合，一次没有返工。结论：判断「是否必须
> `DatabaseConnection`」必须按函数分段，而不是按行 grep；最终仍以 `cargo check` 为准。

## 3. 保留清单（函数级，7 个）

保留原因**全部相同且唯一**：函数体第一行需要 `DatabaseConnection` 专有的 `begin()`（`TransactionTrait`）。

| 函数 | 行 | `db.begin()` 位置 | 业务含义 |
| --- | --- | --- | --- |
| `create_stage` | l.300 | l.307 | 建 stage（Postgres 分支走事务 + `RETURNING`） |
| `create_job` | l.381 | l.398 | 建 job（同上） |
| `try_pause_stage_at_manual` | l.718 | l.719 | manual gate 的暂停判定，先取 SQLite 写锁 |
| `try_update_stage` | l.762 | l.763 | job 结束后级联 stage 状态，先取 SQLite 写锁 |
| `fail_runner_jobs` | l.1013 | l.1017 | runner 掉线后批量失败其 active jobs 并级联 |
| `rerun_failed_job` | l.1132 | l.1134 | rerun 全流程（job/stage/下游/pipeline）单事务 |
| `cancel_pipeline_chain` | l.1347 | l.1348 | 取消 pipeline 及其 stage/job 单事务 |

没有函数因「内部调用了上述 7 个之一」而被迫保留：这些函数体内调用的都是
`*_on` / `*_in_transaction` 内部辅助（本来就是泛型，传 `&txn` 即可），约束不外泄。
`create_pipeline` 虽然也写库，但走 `insert(db)` / `find_by_statement(..).one(db)`，两者都是
`ConnectionTrait` 的方法，不需要事务，因此**放宽**。

## 4. 连锁处理过程

### 4.1 改法

对 31 个函数做纯签名替换（`db: &DatabaseConnection` → `db: &C`，函数名后插 `<C: ConnectionTrait>`），
参数名保留 `db`、泛型名保留 `C`、`ConnectionTrait` 来自文件已有的 `use sea_orm::*;`。**函数体一个字未动**。

证据（把签名形态归一化后比较 token 流）：

```
$ python3 (去空白 → 删 <C:ConnectionTrait> → db:&C 还原为 db:&DatabaseConnection)
token-equal: True 40986 40986
```

即整个文件的 token 流与基线 `6e477ea` 逐字符相同，唯一差异是签名形态。

### 4.2 编译迭代（3 次红 → 1 次绿）

| 轮次 | 命令 | 结果 |
| --- | --- | --- |
| 1 | `cargo check -p rg-db --all-targets` | 红：30 个 `E0277`，**全部**在 `crates/rg-db/tests/collab_store.rs`（`&f.db`，`f.db: Arc<DatabaseConnection>`） |
| 2 | `cargo check -p rg-db --lib` | 绿（`rg-db` 库自身零连锁） |
| 3 | `cargo check --workspace --all-targets` | 红：`rg-core/src/ci/log_write_queue.rs` 6 个 `E0277`（2 个调用点，`db: Arc<DatabaseConnection>`）→ 改 `db.as_ref()` |
| 4 | `cargo check -p rg-core --all-targets` | 绿 |
| 5 | `cargo check --workspace --all-targets --keep-going` | 红：仅剩 `collab_store.rs` 的 30 个 → **停下向协调者申请授权**（该文件不在派单的 owned paths 里） |
| 6 | 授权后改 10 处 `&f.db` → `&*f.db`，再 `cargo check --workspace --all-targets --keep-going` | 绿，零 error / 零 warning |

### 4.3 调用点豁免清单（12 处，逐个列出）

根因同 T14：**泛型 `&C` 不参与 deref coercion**。`&DatabaseConnection` 时 `&Arc<DatabaseConnection>` 与
`&&DatabaseConnection` 都能自动适配；换成 `&C` 后编译器先把 `C` 推成 `Arc<..>` / `&DatabaseConnection`，
再查 `ConnectionTrait` 约束，直接失败。注意 `&C` 形式（`&&DatabaseConnection`）**不需要改**——
sea-orm 对 `&T where T: ConnectionTrait` 有实现，`rg-ci` / `rg-http` 里那些 `&db`（`db: &DatabaseConnection`）
调用点因此全部一次通过，无需触碰。

| # | 文件:行 | 改前 | 改后 | 实参原类型 |
| --- | --- | --- | --- | --- |
| 1 | `crates/rg-core/src/ci/log_write_queue.rs:114` | `get_job(&db, ..)` | `get_job(db.as_ref(), ..)` | `db: Arc<DatabaseConnection>` |
| 2 | `crates/rg-core/src/ci/log_write_queue.rs:138` | `update_job_log(&db, ..)` | `update_job_log(db.as_ref(), ..)` | 同上（fmt 折成 3 行） |
| 3 | `crates/rg-db/tests/collab_store.rs:960` | `create_pipeline(&f.db, ..)` | `&*f.db` | `f.db: Arc<DatabaseConnection>` |
| 4 | `crates/rg-db/tests/collab_store.rs:969` | `get_pipeline(&f.db, ..)` | `&*f.db` | 同上 |
| 5 | `crates/rg-db/tests/collab_store.rs:978` | `update_pipeline_status(&f.db, ..)` | `&*f.db` | 同上 |
| 6 | `crates/rg-db/tests/collab_store.rs:987` | `get_pipeline(&f.db, ..)` | `&*f.db` | 同上 |
| 7 | `crates/rg-db/tests/collab_store.rs:995` | `list_by_repo_and_commit(&f.db, ..)` | `&*f.db` | 同上 |
| 8 | `crates/rg-db/tests/collab_store.rs:1002` | `find_latest_by_repo_and_commit(&f.db, ..)` | `&*f.db` | 同上 |
| 9 | `crates/rg-db/tests/collab_store.rs:1010` | `find_latest_by_repo_and_commit(&f.db, ..)` | `&*f.db` | 同上 |
| 10 | `crates/rg-db/tests/collab_store.rs:1018` | `list_pipelines_by_repo(&f.db, ..)` | `&*f.db` | 同上 |
| 11 | `crates/rg-db/tests/collab_store.rs:1025` | `list_pipelines_by_repo_paginated(&f.db, ..)` | `&*f.db` | 同上 |
| 12 | `crates/rg-db/tests/collab_store.rs:1030` | `update_pipeline_status(&f.db, ..)` | `&*f.db` | 同上 |

三处形态各自只改一个 token：`&db` → `db.as_ref()`、`&f.db` → `&*f.db`。没有 clone、没有 move、没有改签名链。

**关于 `collab_store.rs` 的说明**：该文件原本混用 `&f.db` 与 `&*f.db` 两种写法（`f.db` 是
`Arc<DatabaseConnection>`）；泛型化后前者不再能自动 deref，因此必须统一为后者。这不是 T14 的遗留问题——
`f710675` 之后该文件未被本轮之前任何签名放宽任务触碰。改法也只针对报错的这 10 处，文件里其余
`&*f.db` 保持原样，`&f.db` 已无残留（grep 0 命中）。

## 5. 改动范围核实

```
$ git diff --stat 6e477ea
 crates/rg-core/src/ci/log_write_queue.rs |   6 +-
 crates/rg-db/src/ops/pipeline_ops.rs     | 104 +++++++++++++++----------------
 crates/rg-db/tests/collab_store.rs       |  20 +++---
 3 files changed, 66 insertions(+), 64 deletions(-)
```

正是派单允许的三个落点，没有第五个源文件被带入。`pipeline_ops.rs` 的 104 行改动全部是签名行进出
（`-U0` diff 已验证），`log_write_queue.rs` 6 行 = 2 处调用点（其中 1 处被 fmt 折行成 3 行），
`collab_store.rs` 20 行 = 10 处 `&f.db` → `&*f.db`（10 换 10）。

## 6. 遗留风险

1. 7 个保留函数仍只能接 `&DatabaseConnection`。若将来需要「在外部事务里建 stage/job、或在事务内级联暂停/取消」，
   要把签名扩到 `C: ConnectionTrait + TransactionTrait`（`TransactionTrait::begin` 是泛型可用的）——那是语义扩展，
   不在本轮「纯机械放宽」范围内。
2. 本轮只放宽签名，**没有**把任何现有单行写路径搬进事务。`pipeline_ops` 里仍有「先查后改」的两次独立语句
   （如 `update_job_log`、`assign_job` 的 `rows_affected == 0` 兜底查询），它们在本轮语义下行为不变；
   真正收进事务属于后续任务。
3. 12 处调用点解引用是「为了让泛型签名成立」的最小改动，语义未变（都是只读查询或既有写调用）。
   若将来把 `LogWriteQueue::handle` 的 `db` 从 `Arc<..>` 改成 `&DatabaseConnection`，`db.as_ref()` 需还原
   成 `db`——类型变化会由编译器提示。
4. `pipeline_ops` 的测试覆盖偏薄是结构性的：本 crate 只有 `ci_integer_compat.rs`（12 例）与
   `collab_store.rs` 里的 1 个 pipeline 用例直接覆盖它，行为层主要在 `rg-core`（377 例）与 `rg-http`
   集成测试。本轮 `rg-http` 无调用点改动，未跑其 34 分钟全量，只由
   `cargo clippy --workspace --all-targets`（即编译全部 rg-http 测试目标）兜底。
