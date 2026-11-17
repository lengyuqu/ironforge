# T15 Task 确认 — [#15 步骤3] ops 签名放宽：`pipeline_ops`

> 权威约束见 `tasks/T15.md`。基线 `6e477ea`，分支 `agent/t15-pipeline-ops`，worktree `../ironforge-t15`。
> 计划与决策记录见 `docs/plans/T15-plan.md`。

## 1. 约束确认

| 约束 | 执行情况 |
| --- | --- |
| 只改签名，函数体一字不动 | 已满足：整个文件的 token 流与基线逐字符相同（归一化签名后比较 `token-equal: True 40986 40986`） |
| 参数名保留 `db`、泛型名保留 `C`、用 `sea_orm::ConnectionTrait` | 已满足（文件已有 `use sea_orm::*;`，无需新增 import） |
| SQL 条件 / 排序 / 错误信息 / 返回值 / pipeline 状态机 | 零改动 |
| 不碰其它 ops 文件 | 已满足 |
| 不碰 `Cargo.*` / 迁移 / `CLAUDE.md` / `ironforge-docs/` / `.cnb.yml` / `.github/` / `scripts/` / `deploy/` / `crates/rg-mcp/` | 已满足 |
| 调用点豁免只用单 token 解引用，不 clone / 不 move / 不改签名链 | 已满足（12 处，见 §4） |
| `rg-mcp` 是否调用 `pipeline_ops` | **没有**（`grep -rn pipeline_ops crates/rg-mcp/` 零命中），无需停下 |

## 2. 触碰路径

源码 3 个文件，全部在授权范围内（`crates/rg-db/tests/` 为第 3 次编译后单独向协调者申请并获批）：

| 文件 | 改动 | 授权来源 |
| --- | --- | --- |
| `crates/rg-db/src/ops/pipeline_ops.rs` | 31 个签名泛化 | 派单 owned path |
| `crates/rg-core/src/ci/log_write_queue.rs` | 2 处 `db.as_ref()` | 派单（`rg-core` 调用点） |
| `crates/rg-db/tests/collab_store.rs` | 10 处 `&*f.db` | **追加授权**（派单清单漏列，已获批） |

文档 3 个：`docs/plans/T15-plan.md`、`docs/plans/T15-task.md`、`tasks/T15.md`（State 行）。

## 3. 函数级处置

### 3.1 放宽（31 个，全部改为 `<C: ConnectionTrait>(db: &C, ..)`）

| 行 | 函数 | | 行 | 函数 |
| --- | --- | --- | --- | --- |
| l.102 | `create_pipeline` | | l.975 | `find_offline_runners` |
| l.166 | `list_by_repo_and_commit` | | l.1052 | `assign_job` |
| l.191 | `list_pipelines_by_repo` | | l.1298 | `count_active_pipelines` |
| l.211 | `list_pipelines_by_repo_paginated` | | l.1315 | `find_active_pipelines_by_ref` |
| l.444 | `play_manual_job` | | l.1402 | `get_pipeline` |
| l.467 | `resume_pipeline_chain` | | l.1406 | `update_pipeline_status` |
| l.497 | `resume_approval_chain` | | l.1416 | `list_stages_by_pipeline` |
| l.520 | `get_job` | | l.1423 | `update_stage_status` |
| l.529 | `update_job_log` | | l.1433 | `list_jobs_by_stage` |
| l.569 | `stage_has_job_status` | | l.1440 | `update_job_result` |
| l.668 | `find_latest_by_repo_and_commit` | | l.1452 | `get_stage_by_id` |
| l.870 | `find_pending_job` | | l.1459 | `list_jobs_by_pipeline` |
| l.891 | `find_pending_job_matching_labels` | | l.1466 | `check_stage_jobs` |
| l.932 | `job_is_schedulable`（私有） | | l.1470 | `check_pipeline_stages` |
| l.956 | `find_stuck_jobs` | | l.1477 | `try_update_pipeline` |
| | | | l.1484 | `mark_job_timeout` |

其中 12 个是文件末尾的「public wrapper」（`get_pipeline` … `mark_job_timeout`），它们只把参数转发给同名的
`*_on` 辅助函数，而后者本来就是泛型（`get_pipeline_on`、`list_stages_by_pipeline_on`、…），所以放宽后
`&C` 一路传得通，零连锁。`find_pending_job` / `find_pending_job_matching_labels` 内部调
`job_is_schedulable`，后者一并放宽，方向一致。`play_manual_job` / `find_stuck_jobs` / `assign_job` 内的
`job_updated_at_value(db, ..)` 本来就是泛型（`ConnectionTrait` 只用来判 backend），不阻碍放宽。

### 3.2 保留（7 个，签名不变）

| 行 | 函数 | 保留原因（函数体内唯一的 `DatabaseConnection` 专有能力） |
| --- | --- | --- |
| l.300 | `create_stage` | l.307 `db.begin()` |
| l.381 | `create_job` | l.398 `db.begin()` |
| l.718 | `try_pause_stage_at_manual` | l.719 `db.begin()` + `lock_sqlite_ci_writer(&txn)` |
| l.762 | `try_update_stage` | l.763 `db.begin()` + `lock_sqlite_ci_writer(&txn)` |
| l.1013 | `fail_runner_jobs` | l.1017 `db.begin()` + `lock_sqlite_ci_writer(&txn)` |
| l.1132 | `rerun_failed_job` | l.1134 `db.begin()` + `lock_sqlite_ci_writer(&txn)` |
| l.1347 | `cancel_pipeline_chain` | l.1348 `db.begin()` + `lock_sqlite_ci_writer(&txn)` |

文件内 `db.begin()` 恰好 7 处（l.307 / 398 / 719 / 763 / 1017 / 1134 / 1348），与保留集合一一对应。
不存在「因为调用了保留函数而被迫保留」的函数，也不存在 `db.clone()` 进 `spawn`、`db.ping()` 之类的额外阻塞点
（l.235 的 `.clone()` 是 `Select` 查询对象的 clone，与 db 无关）。

### 3.3 语义零变化的证据

```
$ python3 (去空白 → 删 <C:ConnectionTrait> → db:&C 还原 db:&DatabaseConnection)
token-equal: True 40986 40986
```

`crates/rg-db/src/ops/pipeline_ops.rs` 的 token 流与基线 `6e477ea` 完全相同，唯一差异是 31 处签名形态。

## 4. 调用点豁免（12 处，全部单 token）

| # | 文件:行 | 原形态 | 改法 |
| --- | --- | --- | --- |
| 1 | `crates/rg-core/src/ci/log_write_queue.rs:114` | `get_job(&db, ..)`（`db: Arc<DatabaseConnection>`） | `get_job(db.as_ref(), ..)` |
| 2 | `crates/rg-core/src/ci/log_write_queue.rs:138` | `update_job_log(&db, ..)` | `update_job_log(db.as_ref(), ..)`（fmt 折成 3 行） |
| 3 | `crates/rg-db/tests/collab_store.rs:960` | `create_pipeline(&f.db, ..)` | `&*f.db` |
| 4 | `crates/rg-db/tests/collab_store.rs:969` | `get_pipeline(&f.db, ..)` | `&*f.db` |
| 5 | `crates/rg-db/tests/collab_store.rs:978` | `update_pipeline_status(&f.db, ..)` | `&*f.db` |
| 6 | `crates/rg-db/tests/collab_store.rs:987` | `get_pipeline(&f.db, ..)` | `&*f.db` |
| 7 | `crates/rg-db/tests/collab_store.rs:995` | `list_by_repo_and_commit(&f.db, ..)` | `&*f.db` |
| 8 | `crates/rg-db/tests/collab_store.rs:1002` | `find_latest_by_repo_and_commit(&f.db, ..)` | `&*f.db` |
| 9 | `crates/rg-db/tests/collab_store.rs:1010` | `find_latest_by_repo_and_commit(&f.db, ..)` | `&*f.db` |
| 10 | `crates/rg-db/tests/collab_store.rs:1018` | `list_pipelines_by_repo(&f.db, ..)` | `&*f.db` |
| 11 | `crates/rg-db/tests/collab_store.rs:1025` | `list_pipelines_by_repo_paginated(&f.db, ..)` | `&*f.db` |
| 12 | `crates/rg-db/tests/collab_store.rs:1030` | `update_pipeline_status(&f.db, ..)` | `&*f.db` |

`rg-ci` / `rg-http` / `rg-runner` / `rg-ssh` 的调用点**一个都没改**：它们传的是 `&DatabaseConnection`
（`&&DatabaseConnection`），sea-orm 对 `&T where T: ConnectionTrait` 有实现，泛型推断一次成功。

`crates/rg-db/tests/collab_store.rs` 的形态说明：该文件原本混用 `&f.db` 与 `&*f.db` 两种写法
（`f.db: Arc<DatabaseConnection>`）；泛型化后前者不再能自动 deref（引用折叠不适用于泛型 `&C`），
必须统一为后者。改动严格限定在报错的 10 处；改后 `grep -c '&f.db'` = 0，文件里没有遗留的待改形态。

## 5. 编译迭代记录

| 轮次 | 命令 | 结果 | 处置 |
| --- | --- | --- | --- |
| 0 | 函数级分段预扫描（按 `fn` 边界切块找 `begin()`） | 38 个带 db 参数函数：31 可放宽 / 7 必保留 | 与协调者预统计一致 |
| 1 | `cargo check -p rg-db --all-targets` | 30 × `E0277`，全在 `collab_store.rs` | 分类：全是 `&Arc<..>` 调用点 |
| 2 | `cargo check -p rg-db --lib` | 绿 | 库内部零连锁 |
| 3 | `cargo check --workspace --all-targets` | 6 × `E0277`，`rg-core/src/ci/log_write_queue.rs`（2 个调用点） | 改 `db.as_ref()` ×2 |
| 4 | `cargo check -p rg-core --all-targets` | 绿 | — |
| 5 | `cargo check --workspace --all-targets --keep-going` | 仅剩 `collab_store.rs` 30 个 | **停下向协调者申请授权**（不在 owned paths） |
| 6 | 授权后改 10 处 → `cargo check --workspace --all-targets --keep-going` | 绿，零 error / 零 warning | 进入验收 |

共 3 次红、1 次「红但可控」，之后干净。`cargo fmt --all --check` 在最后一次改动后直接通过，
没有触发 T14 那种「需 `cargo fmt` 重排」的情况（只有 `log_write_queue.rs:138` 一处因为加长而换行，
换行结果与 rustfmt 输出一致）。

## 6. 改动范围

```
$ git diff --stat 6e477ea
 crates/rg-core/src/ci/log_write_queue.rs |   6 +-
 crates/rg-db/src/ops/pipeline_ops.rs     | 104 ++++++++++++++++----------------
 crates/rg-db/tests/collab_store.rs       |  20 +++---
 3 files changed, 66 insertions(+), 64 deletions(-)
```

`pipeline_ops.rs` 的 `-U0` diff 只有签名行进出：31 × 2 = 62 行 `-`/`+`，加上 fmt 把 2 个单行签名折成多行
（`create_pipeline` 之外的 `list_by_repo_and_commit` 等本来就是多行），净 104 行。没有内容被删除或截断。

## 7. 验收命令真实输出

### 7.1 `cargo test -p rg-db`

```
running 59 tests   (unittests src/lib.rs)
test result: ok. 59 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 12.66s
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.45s   (tests/authorization_store.rs)
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.38s   (tests/ci_integer_compat.rs)
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.51s    (tests/collab_groups_store.rs)
test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s    (tests/collab_migration.rs)
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.73s   (tests/collab_store.rs)
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s    (Doc-tests)
```

合计 97 passed ＋ 1 ignored，0 failed。注意 `collab_store.rs` 里 15 个用例全绿，包括被改动的
`pipeline_evidence_access_handles_legacy_integer_columns`（覆盖 `create_pipeline` / `get_pipeline` /
`update_pipeline_status` / `list_by_repo_and_commit` / `find_latest_by_repo_and_commit` /
`list_pipelines_by_repo` / `list_pipelines_by_repo_paginated` 六条放宽后的路径）。

### 7.2 `cargo test -p rg-core`

```
running 377 tests
test result: ok. 377 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 13.25s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
running 3 tests
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.70s
running 2 tests
test result: ok. 0 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.00s
running 12 tests
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.90s
running 6 tests
test result: ok. 1 passed; 0 failed; 5 ignored; 0 measured; 0 filtered out; finished in 0.07s   (Doc-tests)
```

### 7.3 `cargo test -p rg-ci`

```
running 46 tests
test result: ok. 46 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 9.53s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.44s
running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

### 7.4 `cargo check -p rg-http --all-targets`（额外，因 `rg-http` 有大量 `pipeline_ops` 调用点但未改动）

```
    Checking rg-http v0.2.0 (/Users/yuqu/vibeCodeing/ironforge-t15/crates/rg-http)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 38.40s
```

### 7.5 `cargo clippy --workspace --all-targets -- -D warnings`

```
    Checking rg-core v0.2.0 (/Users/yuqu/vibeCodeing/ironforge-t15/crates/rg-core)
    Checking rg-ssh v0.2.0 (/Users/yuqu/vibeCodeing/ironforge-t15/crates/rg-ssh)
    Checking rg-ci v0.2.0 (/Users/yuqu/vibeCodeing/ironforge-t15/crates/rg-ci)
    Checking rg-cli v0.2.0 (/Users/yuqu/vibeCodeing/ironforge-t15/crates/rg-cli)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 34.43s
```

### 7.6 `scripts/check.sh --quick`

```
==> cargo fmt --all --check
    (1s)

==> cargo clippy --workspace --all-targets -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.27s
    (1s)

==> checks passed in 2s (quick)
```

## 8. 遗留风险

1. 7 个保留函数仍锁定 `&DatabaseConnection`。要支持「外部事务里建 stage/job、或事务内级联暂停/取消」，
   需把签名扩成 `C: ConnectionTrait + TransactionTrait`（`TransactionTrait::begin` 本身是泛型可用的），
   属于语义扩展，留给后续任务。
2. 本轮**没有**把任何「先查后改」的单行写路径搬进事务（`update_job_log`、`assign_job` 等仍各自独立执行）。
   「能进事务」是能力，落地要等后续任务真正把 `&txn` 传进来。pipeline 状态机转移逻辑零改动。
3. `pipeline_ops` 的直接测试覆盖薄：本 crate 只有 `ci_integer_compat.rs`（12 例）与 `collab_store.rs` 的
   1 个 pipeline 用例直接打它。行为层主要靠 `rg-core`（377 例）与 `rg-http` 集成测试兜底；本轮未跑
   `rg-http` 全量测试（34 分钟），只做了 `--all-targets` 编译 + clippy，`rg-http` 无调用点改动因此风险可控。
4. 12 处解引用是「为了让泛型签名成立」的最小改动。若将来 `LogWriteQueue::handle` 的 `db` 由
   `Arc<DatabaseConnection>` 改回 `&DatabaseConnection`，`db.as_ref()` 可还原成 `db`——类型变化会由编译器提示。
5. `crates/rg-db/tests/collab_store.rs` 不在派单 owned paths 内，本次是追加授权后修改的。若后续任务沿用
   T15 的 `tasks/*.md` 模板，建议把 `crates/<crate>/tests/` 显式写进「允许的调用点解引用」范围——泛型化
   必然波及测试里的 `&Arc<..>` 调用形态，这一条在 T14 没有暴露只是因为没有 ops 测试直接打那两个文件。
