# T30 执行计划 — 更新 `CLAUDE.md` 到当前实现现状

> 权威约束见 `tasks/T30.md`。基线 `46226ff`，分支 `agent/t30-claude-md-refresh`，worktree `../ironforge-t30`。
> 本文件记录：核实过的文件、每条新陈述对应的代码位置、以及**因为拿不到证据而没有写**的说法。

## 1. 改动范围

| 文件 | 改动 |
| --- | --- |
| `CLAUDE.md` | 标题日期 `2026-07-14` → `2026-11-18`；页首「最后更新」→ `2026-11-18`；迁移条数行补 106 / down 32；「详细实现清单」+4 行；「技术债」补「本轮已关闭」与 4 条新增遗留，更新 CI 隔离段；「重要踩坑」+3 条（24–26） |
| `ARCHITECTURE.md` | 目录树里的 `65+ SeaORM 实体` → `108`、`70+ 数据库迁移` → `106`（唯一两处与实现直接冲突的计数）；日期 → `2026-11-18` |
| `AGENT.md` | 只更新页首日期（正文是索引，没有实现细节需要改） |

未改：`crates/`、迁移、`Cargo.*`、`ironforge-docs/`、`.github/`、`scripts/`、`deploy/`、`rg-mcp/`。

## 2. 迁移数字（验证项 2）

读的是 `crates/rg-db/src/migrations/mod.rs::Migrator::migrations()` 与 `crates/rg-db/src/lib.rs`：

- 注册迁移：**106** 条（`Box::new(...)` 计数，最后一条是 `m20261118_000001_create_symbol_graph`）。
- 回滚窗口：`crates/rg-db/src/lib.rs:1028` `migrations::Migrator::down(&db, Some(32))`，注释写明「32 today: this test starts at 74, the upgrade above applies all 106, the rollback must land back on 74」。
- 另一处 `const TASKS: i64 = 32`（`lib.rs:500`）是并发测试的任务数，与迁移无关，没有引用。

`CLAUDE.md` 原文没有「当前 down 步数」的断言，只有 Agent 主体身份行里 T23 的历史记录「25→26」。那是一条历史事实（当时正确），保留不改，避免篡改历史；当前数字写进「多数据库后端」行。`ARCHITECTURE.md` 的 `70+` 改成 `106`，实体 `65+` 改成 `108`（`crates/rg-db/src/entities/mod.rs` 有 108 个 `pub mod`，108 个文件带 `#[sea_orm(table_name = ...)]`）。

## 3. 四条新行 —— 逐条代码依据

### ① 事件写入的事务性与 outbox（#15）

| 陈述 | 位置 |
| --- | --- |
| `append<C: ConnectionTrait>` 接任意连接 | `crates/rg-db/src/ops/repo_event_ops.rs:17` |
| `record_event_in_txn` 事务内写、失败返回错误 | `crates/rg-core/src/webhook/service.rs:327` |
| `deliver_event` 提交后投递、不再写日志、失败只 `warn!` | `service.rs:351`；调用点各自 `if let Err(e) = ... { warn! }`，如 `pull_request/service.rs:1777` |
| 5 条已迁移路径 | `issue/service.rs:75`（issue.opened）、`pull_request/service.rs:1754`（merged）、`release/service.rs:87`（release.created）、`pull_request/service.rs:224`（opened）、`pull_request/service.rs:528`（closed） |
| `branch.*`/`tag.*` 未迁移 | `docs/plans/T19-task.md` §6 第 3 条 |
| `pull_request.closed` 原为「先发 webhook 再写库」 | `docs/plans/T18-plan.md`；现顺序见 `pull_request/service.rs:528`（事务内写）与 `:547`（commit 后 `deliver_event`） |
| `dispatched_at` 语义 = 已交给投递系统 | 迁移 `m20261117_000001` 文件头注释；实体 `crates/rg-db/src/entities/repo_event.rs:23` |
| 历史行按 `created_at` 回填、重跑不回填 | 同一迁移文件头 §「Existing rows are back-filled with their `created_at`」 |
| 静默路径写入即标记 | `service.rs:440` `record_event` → `record_envelope(..., true)`；`:461` `trigger_event_quietly` |
| sweep 周期 60 s、窗口 `[now-15min, now-2min]`、环境变量 | `crates/rg-core/src/webhook/outbox.rs`：`DEFAULT_INTERVAL_SECS`、`LOOKBACK_SECS`、`SETTLE_SECS`、`SWEEP_INTERVAL_ENV`、`run_sweep_loop`（:196） |
| 坏 payload / 截断 stub 只标记不重放 | `outbox.rs::sweep` 的 `settle` 分支 |
| 遗留：至少一次、payload 认领可能重复、超 15 分钟不补 | `docs/plans/T19-task.md` §6 第 1/2/4 条 |

### ② 通用审批原语（#19）

| 陈述 | 位置 |
| --- | --- |
| **两张表**（不是任务书说的三张） | 迁移 `m20261114_000002` 只建 `authorization_requests` / `authorization_decisions`；实体目录只有这两个（`crates/rg-db/src/entities/authorization_{request,decision}.rs`） |
| `authorization_set_id` 是规则摘要 | `crates/rg-core/src/authorization/payload.rs:33`；迁移文件头写明「not a foreign key … there is no `authorization_sets` row to point at」 |
| `payload_hash` 规范形、无 `parameters` 列 | `payload.rs:22`；`payload.rs:1-17` 注释 |
| 批准时重算、不一致 409 `PAYLOAD_CHANGED` | `authorization/mod.rs:441` `require_payload_unchanged` |
| `consume` 必须与业务同事务 | `authorization/mod.rs:323` |
| CAS `UPDATE ... WHERE state='pending'`、0 行 → 409 | `crates/rg-db/src/ops/authorization_ops.rs:190` `consume_guarded`；`consume` 的 `ALREADY_CONSUMED` 分支 |
| 过期 / 票数不足 / 自批的错误码 | `authorization/mod.rs` 的 `EXPIRED` / `NOT_APPROVED` / `SELF_APPROVAL` |
| `scope_expand` subject 格式与理由 | `crates/rg-core/src/authorization/scope_expand.rs:1-50` 文件头；`parse_subject` |
| 精确比对 subject（不只靠阶梯） | `scope_expand.rs::apply`（`if request.subject != format!("{token_id}:{target}")`） |
| 原地扩 `access_tokens.scopes`、永久、无收窄 | `scope_expand.rs:1-25`；`expr` 更新 + `SCOPES_CHANGED` |
| `is_human_account` 语义 | `crates/rg-core/src/agent.rs:59` |
| 三处调用点 | `rg-http/src/api/auth.rs:175`（`ensure_reviewer_may_approve`）、`authorization/evaluator.rs:330`（`ensure_human_decider`）、`rg-http/src/api/ci_environments.rs:288`（受保护环境审批） |
| 批准计数另在 SQL 过滤 `kind='human'` | `crates/rg-db/src/ops/pr_review_ops.rs:97`（`Column::Kind.eq(KIND_HUMAN)`） |
| `issue.closed` 没搬进事务（labels/assignees fail-soft） | `issue/service.rs:288`、`:297` 只 `warn!`；关闭事件在 `:325` 的提交后代码块 |
| `milestone.closed` 是纯通知 | `issue/service.rs:481` `notify_milestone_closed`；`docs/plans/T18-plan.md` §5 |

### ③ CI 容器默认用户（#41）

| 陈述 | 位置 |
| --- | --- |
| 默认 `user` = 进程 `uid:gid` | `crates/rg-ci-isolation/src/lib.rs:140` `current_uid_gid`，`libc::getuid/getgid` |
| 不用 `Command::new("id")` 及理由 | `docs/plans/T26-plan.md` §4；`id` 只在测试交叉校验（`lib.rs:304`） |
| 环境变量三态 | `lib.rs:36` `RUNNER_USER_ENV`、`:114` `resolve_runner_user` |
| `root`/`0` 首次 `warn!` 一次 | `lib.rs` `IMAGE_DEFAULT_WARNED: Once` |
| Windows 返回 `None` | `lib.rs:150` 的 `#[cfg(not(unix))]` 分支 |
| 三个使用点都走 `DockerLimits::default()` | `docs/plans/T26-plan.md` §1（`rg-ci/src/runner.rs`、`rg-cli/src/main.rs`、`rg-runner/src/main.rs`） |
| 未做 userns-remap / seccomp / AppArmor / `--network none` | `T26-plan.md` §3；`DockerLimits.network_none` 默认 `false` |

### ④ 符号定义索引与查询（#63）

| 陈述 | 位置 |
| --- | --- |
| 两张表、只存 `commit_id`、只建 2 个索引及理由 | 迁移 `m20261118_000001` 文件头；`symbol_definitions` 的索引常量 |
| 复用 `OutlineIndexer::extract` | `crates/rg-core/src/search/symbol_indexer.rs:1-15` |
| `index_commit` 先删后插 | `symbol_indexer.rs:151`（`DELETE FROM symbol_definitions WHERE commit_id = ?` 在 `:177`） |
| 同一事务修剪 + 保留刚索引的提交 | `symbol_indexer.rs` `prune` 调用（`:187`）与文件头 §Retention |
| `D = clamp(128MiB/(224B×定义数), 1, 6)` | `symbol_indexer.rs:128` `retention_depth` |
| 入口只有 POST index（写权限），push 不自动索引 | `rg-http/src/api/ai.rs:923`（`require_write`）；全仓 `index_commit` 调用点只有这一处 |
| `GET .../symbols`（读权限）、limit 50/200、400 `INVALID_LIMIT` | `ai.rs:1082`、`DEFAULT_SYMBOL_LIMIT`/`MAX_SYMBOL_LIMIT` |
| MCP `find_definition` | `rg-mcp/src/tools/mod.rs:58` |
| `partial` 恒 true 及理由 | `symbol_query.rs:123` `const PARTIAL: bool = true` |
| `limitations` 六条 | `symbol_query.rs:107` `LIMITATIONS` |
| 缺索引 404、不回落 HEAD | `symbol_query.rs:28-34`；HTTP 映射 `ai.rs:1181` `symbol_error` |
| 无条件排除 `kind='impl'` | `symbol_query.rs:1-18`；SQL 过滤处 |
| 实测 12,208 / 2.94 MiB / 684 impl | `docs/plans/T28-task.md:45-70`、`T28-plan.md:141-147`、设计文档 §1 |
| 引用部分否决（30 倍、73.9%） | `symbol_indexer.rs:1-10`、迁移文件头、`ironforge-docs/architecture/symbol-graph-design-2026-11.md` §2 |

## 4. 三条新踩坑 —— 对应事件

| 条目 | 证据 |
| --- | --- |
| 24 泛型 `&C` 不参与 deref coercion | `docs/plans/T14-plan.md:74-82`、`T15-plan.md:86-95`；当前代码里仍能看到 `db.as_ref()` / `*db`：`rg-ssh/src/lib.rs:363`/`:646`、`rg-http/src/lib.rs:3695`、`log_write_queue.rs:114`/`:139` |
| 25 收紧 subject 格式后 `parse::<i64>()` 恒失败并放行 | `docs/plans/T21-task.md:28`、`T21-plan.md:86-98`；修复后的 `parse_subject(...)?` 在 `scope_expand.rs:365` |
| 26 contents 写路径 spawn 后置钩子导致毫秒竞争 | `docs/plans/T25-plan.md`、`T25-task.md:79-125`；`repo_content.rs:1520`/`:1654` → `announce_branch_move`（`:1364`）→ `spawn_post_push_hooks`（`:1386`）；测试辅助 `World::commit_without_push_hooks`（`rg-http/tests/auto_merge_pinned_head_tests.rs:89`） |

行号是**当前基线 `46226ff`** 的；T14/T15 计划里记录的旧行号（如 `rg-http/src/lib.rs:3678`）已被后续提交推移，文中改用了现在的行号。

## 5. 因为拿不到证据而没有写（或改写的说法）

1. **「三表 `authorization_requests` / `authorization_decisions` / `authorization_sets`」——不成立。** 全仓只有前两张表；`authorization_sets` 只出现在注释里（`m20261114_000002` 文件头、`T06-plan.md:263`）。文中写「两张表」，并说明 `authorization_set_id` 是规则摘要。
2. **「`is_human_account` 四处消费」——实际是 3 处。** `rg-core` 与 `rg-http` 里的非测试调用点只有 auth.rs、evaluator.rs、ci_environments.rs 三处；批准计数走 SQL 的 `KIND_HUMAN`，不经这个函数。文中写三处并点名。
3. **「`Migrator::down` 25→26 要改成 32」——不改。** 那是 T23 当时的历史事实；当前数字写在「多数据库后端」行，避免把历史记录改成错的。
4. **符号图「3 个索引」——不写。** T28 实测后删掉了 path 索引（`T28-task.md:150`），现在 `symbol_definitions` 上只有 2 个；文中按 2 个写并给出删除理由。`symbol_commits` 自己的唯一键没有算进「2 个索引」，行内限定了范围。
5. **CI「三个使用点」**：没有在代码里逐个 `grep` 出三处 `DockerLimits::default()` 的完整清单（`rg-ci`/`rg-runner`/`rg-cli` 各一处是 T26 计划记录的）。文中沿用该记录，未展开成「全部调用点」的断言。
6. **实测数字**：12,208 定义 / 2.94 MiB / 684 impl / 30 倍 / 73.9% 全部来自 T28/T29 与设计文档的实测记录，本次**没有重跑**（只改文档，不引入新测量）；行内已标成「实测」并给出来源，未声称本次验证。

## 6. 验收

- 标题日期 2026-11-18 ✓
- 迁移 106 / down 32，与 `rg-db/src/lib.rs:1028` 注释和 `migrations/mod.rs` 计数一致 ✓
- 4 行新内容逐条 `sed -n` / `grep -n` 核过（见 §3）✓
- 3 条踩坑能指到 T14/T15、T21、T25 的计划与当前代码 ✓
- `scripts/check.sh --quick`（fmt + clippy `-D warnings`）通过（纯文档改动，见收尾输出）✓
