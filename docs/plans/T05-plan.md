# T05 Execution Plan — R10 配额与用量（最小闭环，issue #40）

> 状态：实现完成，四条验收命令已在本机跑绿，新 SQL 已在 PostgreSQL 17 与 MySQL 8.4 上实测。
> 基线 `04e8c87`，分支 `agent/t05-ci-quota`，worktree `../ironforge-t05`。

## 1. 范围

**做（最小闭环）**

1. 新表 `usage_counters`，按 `(subject_kind, subject_id, repo_id, metric, period_start)` 唯一，`value` 是**绝对量**（不是增量）。
2. 首批两个 metric：`ci_minutes`（每日分钟数）、`ci_concurrent_pipelines`（当前占用并发槽的数量）。
3. `trigger_pipeline` 在**创建 pipeline 之前**校验：并发超限 / 当日分钟超限 → 409（`CoreError::Conflict`），message 以稳定码开头。
4. pipeline 进入终态时按 `pipeline_jobs.started_at` / `finished_at` 记账；同一 pipeline 重复记账不叠加。
5. 只对 `users.kind = "agent"` 生效；人类不受限。
6. 承接 T04 遗留（§6.1）：push 侧密钥扫描的 `AuditEvent` 通过 `set_audit_sink` 落 `audit_log`。

**不做（明确留出）**

- 任何 REST 查询端点（避免抢路由注册；后续任务再做）。
- 复用 `collab_task_budget` / `collab_task_usage`（它们是客户端上报的信息性数据，平台不测量）。
- 改 CI `concurrency` 配置语义（`crates/rg-ci/src/config.rs:32` 的并发组是 pipeline 序列化，不是配额）。
- 跨天拆分一个跨越午夜的 job（最小闭环不要求该精度）。

## 2. 上限数值与依据

集中定义在 `crates/rg-ci/src/quota.rs`，禁止散落：

| 常量 | 值 | 依据 |
| --- | --- | --- |
| `MAX_CONCURRENT_PIPELINES_PER_REPO` | **4** | 本部署只有一个自托管 runner（`docker-compose.yml`），4 是一个 agent 在单仓库上占满仍不至于完全饿死其他工作的上限；单 agent 在单仓库上超过 4 条并发几乎都是重试风暴而非有效并行 |
| `DAILY_MINUTES_PER_REPO` | **120**（2 小时） | 典型 agent 任务推几次提交、每次 pipeline 数分钟，2 小时足以覆盖单仓库一天的正常工作并留足余量；每 2 分钟跑一次的失控循环会在上午就撞线 |

口径都是「每 agent × 每仓库」，不跨仓库合并、不跨 agent 合并。

## 3. 数据模型

`crates/rg-db/src/migrations/m20261114_000001_create_usage_counters.rs`（新建，注册于 `migrations/mod.rs` 末尾，`rg-db/src/lib.rs:1028` 回滚步数 `26 → 27`）：

| 列 | 类型 | 说明 |
| --- | --- | --- |
| `id` | bigint PK auto_increment | |
| `subject_kind` | varchar(16) | 目前只有 `user`；留给 `org` / `token` |
| `subject_id` | bigint | `users.id`。**不建外键**：计数要能比账号活得久（同 `audit_log.on_behalf_of_user_id`） |
| `repo_id` | bigint | FK → `repositories(id)` ON DELETE CASCADE |
| `metric` | varchar(48) | `ci_minutes` / `ci_concurrent_pipelines` |
| `period_start` | date | UTC 日 |
| `value` | bigint not null default 0 | **该周期的绝对量** |
| `updated_at` | timestamp with time zone | 仅观察用 |

- 唯一索引 `uq_usage_counters_subject_metric_period`：`(subject_kind, subject_id, repo_id, metric, period_start)`，是并发写的仲裁者。
- 表名显式写成 `usage_counters`（`CLAUDE.md` 第 13 条：`#[derive(Iden)]` 的默认是单数）。有单测在三种方言下断言 DDL 含复数表名与唯一索引名。
- `up` 幂等：`has_table` 分支决定建表还是补列；每条列都有 `has_column` 守卫；索引有 `has_index` 守卫 + `if_not_exists`。`down` 真删除表（`if_exists`）。单测覆盖「up → down → up」后表与索引都回来。

## 4. 记账方案（幂等性来源）

### 4.1 为什么不用增量

pipeline 进入终态有多条互不相同的路径：内部 runner（`runner.rs` 三处）、`cancel_pipeline_chain`、runner API 的 `try_update_pipeline`、`trigger_pipeline` 里全 skipped 的直接 success。每条都可能被重试。若写增量就必须再造一张幂等账本。

本实现改成：**每次记账都重算该 `(agent, repo, UTC 日)` 的总量，再写绝对值**。

- 数据来源：`pipelines`（按 `triggered_by` + 终态 + `finished_at` 落在当日）+ `pipeline_jobs`（`started_at`/`finished_at` 都在才计；`skipped` 不计；负跨度按 0）。
- 幂等性：重算同一周期得到同一个数，重复记账天然无害（单测与跨后端测试都显式跑了两遍）。
- 幂等键就是这个唯一索引四元组；`upsert` 用 `UPDATE` → 命中 0 行再 `INSERT` → 唯一冲突视为「别人先插了，改为再 `UPDATE` 一次」（`CLAUDE.md` 第 19 条：MySQL 不支持 `ON CONFLICT`）。
- 写入放在**独立短事务**里，事务第一条就是写语句（`CLAUDE.md` 第 21 条：SQLite 读开头再写会 `SQLITE_BUSY`），不进入 pipeline 主事务。
- 计数永不阻断 pipeline：记账失败只 `warn`。

### 4.2 归日与取整

- 整条 pipeline 归到 `pipelines.finished_at` 的 UTC 日，不拆分跨午夜的 job。
- 当日总秒数向上取整为分钟（`(secs + 59) / 60`），避免一堆亚分钟 pipeline 永远记 0。

### 4.3 记账触发点（按实际覆盖）

| 路径 | 何时记账 |
| --- | --- |
| 内部 runner 跑完（成功/失败） | `spawn_internal_runner` 中 `runner.run()` 返回后立即记（`rg-ci/src/lib.rs`） |
| 全 skipped 直接 success | `trigger_pipeline` 的 success 分支（`rg-ci/src/lib.rs`） |
| 配置了外部 runner / 手工取消 / runner API 改状态 | **不在当期立即记**，由下一次同仓库同 agent 的触发在 `enforce` 里重算补齐 |
| 并发 gauge | 每次触发创建 pipeline 后、每次记账后各刷新一次 |

「延迟补齐」是刻意的：`runner.rs`、`api/ci.rs`、`api/runners.rs` 不在 T05 owned paths 内，且重算保证**在真正做决策的那一刻数字一定是最新的**——这正是配额需要保证的属性。

## 5. 限制点

`trigger_pipeline` 的顺序：读配置 → 语义校验 → **`quota::enforce`** → 现有 `concurrency` 并发组 → 建 pipeline。

- 放在配置校验之后，保证「CI 配置非法」仍先返回 400，不改变既有错误优先级。
- `enforce` 先重算两个 metric 并落库，再用**实时活跃计数**判并发（gauge 只是落库快照），用落库分钟数判当日额度。
- 只对 `users.kind = "agent"` 生效：`triggered_by` 为空（账号已删 / 匿名或内部 push）、查不到用户、或 `kind != "agent"` 时直接放行且不写任何行。人类账号在升级后不会被误伤。

### 5.1 409 + 精确稳定码

`error.code` 精确等于 `CI_QUOTA_CONCURRENCY` / `CI_QUOTA_MINUTES`，不是 message 前缀。

通用变体 `CoreError::Coded { status: u16, code: &'static str, message }` 由 T04（R7 密钥扫描）先一步在主分支引入并在用（`SECRET_DETECTED`）。本任务**复用它，不新增第二个变体**：

- 补 `CoreError::coded(status, code, msg)` 通用构造函数，避免每个调用点手写结构体；
- `rg-http/src/error.rs` 的 `From<CoreError> for AppError` 把它转成 `AppError::Coded`，`engine_error()` 只做 downcast，因此 `error.code` 直接是稳定码；
- 单测：`rg-core` 里 `Coded` 字段经 anyhow 往返不变；`rg-http` 里 `AppError::from(CoreError::coded(409, "CI_QUOTA_CONCURRENCY", ..))` 的 `status()`/`code()` 精确；
- 没有为两个码各加枚举成员，也没有改既有变体的语义；`api/ci.rs` 一行未动。

**变基说明**：T04 合并（`d62d1c3`）后本分支已 rebase 到最新 `main`。冲突只出现在 `rg-core/src/error.rs`（两边都加了同一个 `Coded` 变体）与 `merge_queue.rs` 的穷尽匹配；解决方式是保留 T04 的变体定义与分支注释，本任务的提交收窄为「`coded()` 构造函数 + 两端测试」，`merge_queue.rs` 最终与主分支完全一致。

## 6. 覆盖面：push 也受限

`triggered_by` 是 CI 引擎唯一能拿到的主体标识。原先唯一真实 push 触发点传 `None`，配额对 agent 的 `git push` 完全无效——这正是 agent 最该被拦的场景，所以已修：

- `crates/rg-http/src/lib.rs` 的 post-push 触发段改为 `triggered_by: *pushed_by`（`PostPushParams.pushed_by` 早已由 git push 的 `actor_id` 和 REST 文件提交的 actor 填好）；
- pusher 解析不出时 `pushed_by` 为 `None`（账号已删、匿名/系统内部 push、无凭据的 ref 更新）→ 不限流、不计数；
- 只改了这一个调用点与其所需参数传递，未碰同文件的路由注册段（`build_routes`），未增删任何路由。

现在配额覆盖：git push、REST 文件提交（同一 post-push hook）、`POST /pipelines`（manual）、retry、review 触发、merge queue（`enqueued_by_id`）。

### 6.1 承接 T04 遗留：push 侧密钥扫描审计落库

T04 的扫描器在 push 侧无法写库（`rg-git` 无 DB），已就位 `AuditEvent` + `set_audit_sink()` + 结构化日志，但没人调用 sink（`docs/plans/T04-plan.md` §9 遗留）。本任务在**同一段** post-push 代码里接上：

- sink 是进程级单例，而审计行需要知道事件属于哪个仓库。实现用 `tokio::task::try_id()` 把事件路由回**正在扫描的那个 receive-pack 任务**：`crates/rg-http/src/lib.rs` 在 push 前 `SecretScanAudit::begin()` 注册带 `mpsc` 的任务槽，push 结束后 `drain()` 取出事件并注销（`Drop` 兜底，取消/ panic 也不会泄漏槽位）；没有注册任务的事件直接丢弃，**不会归错仓库**。
- 事件随 `spawn_post_push_hooks` 进入后台任务，在解析出 `repo_id` 后用 `rg_core::audit::record` 落 `audit_log`：action 为 `repo.secret_scan_rejected` / `repo.secret_scan_findings` / `repo.secret_scan_skipped`，`resource_type = "repo"`，`resource_name = owner/name`，`details` 只含 `via=git_push`、ref、path、rule、files、reason，**绝不含命中的文本**。
- REST 文件提交路径（`api/repo_content.rs`）已有自己的审计，传空事件列表，不重复记录。
- 局限：审计写入在后台任务，push 响应不等待；进程在写入前崩溃会丢该行（事件同时已进 `tracing::warn`）。

## 7. 测试

`crates/rg-ci/src/quota.rs`（单元 + SQLite 集成，6 项）：

- 分钟取整边界（0 / 负数 / 1s / 60s / 61s / 1h）；
- 人类与无主体触发不限流且不写行；
- agent 并发到 4 条 → `CoreError::Coded { status: 409, code: CI_QUOTA_CONCURRENCY }`，gauge 落库为 4；
- 3 × 40 分钟 → 120 分钟撞线 → `Coded { 409, CI_QUOTA_MINUTES }`；重复记账仍是 120；`(subject, repo)` 只有 2 行；
- pending / 他人 pipeline 不计费；
- **`push_triggered_pipelines_are_gated_for_agents_only`**：真实 git 仓库 + 合法 `.ironforge-ci.yml`，以 `trigger_type = "push"` 触发——超配额的 agent 得到 `409 CI_QUOTA_CONCURRENCY` 且不建 pipeline 行；同样情形的人类成功建行（`triggered_by`/`trigger_type` 回读确认）。

`crates/rg-http/tests/secret_scan_push_audit_tests.rs`（1 项）：真实 `git push`（agent 凭证）携带阻塞级 cloud access key → push 被拒，`audit_log` 出现 `repo.secret_scan_rejected`，`resource_id` 是该仓库、`details.via = git_push`、`path/rule/ref` 正确，且 details 不含命中文本。

`crates/rg-http/tests/ci_quota_push_tests.rs`（1 项）：用记录型 `CiTrigger` 替换引擎，agent 通过 contents API 提交文件（与 git push 同一个 post-push hook）→ 引擎收到 `trigger_type = "push"`、`triggered_by = Some(agent_id)`；owner 提交 → `Some(owner_id)`。这条钉住「push 把 pusher 传进引擎」的接线；「超配额被拒 409」由上面 rg-ci 的测试覆盖（`rg-http` 不能依赖 `rg-ci`，`Cargo.*` 冻结）。

`crates/rg-ci/tests/quota_backends.rs`（跨后端，1 项，读 `IRONFORGE_TEST_DATABASE_URL`）：在 SQLite / PostgreSQL / MySQL 跑同一套「重复记账幂等 + 双闸门（`Coded` code 断言）+ 人类不记账 + gauge 为实时值」。

`crates/rg-db/src/migrations/m20261114_000001_create_usage_counters.rs`（2 项）：

- 三种方言 DDL 都含 `usage_counters` 与唯一索引名，且不含单数表名；
- up → down → up：表被真正删除并可重建、唯一索引回来。

`rg-core/src/error.rs`（2 项）：`Coded` 分类与字段经 anyhow 往返不变。
`rg-http/src/error.rs`（1 项新增）：`Coded` → `AppError` 的 `status()/code()` 精确。

既有守卫：`rg_db::soft_delete_tests::upgrade_keeps_child_rows_triggers_and_foreign_keys` 覆盖 `Migrator::down(&db, Some(27))` 的回滚窗口，已通过。

## 8. 验收（真实输出）

```
$ cargo test -p rg-db
running 30 tests   ... test result: ok. 30 passed; 0 failed; ...
running 12 tests   ... test result: ok. 12 passed; 0 failed; ...
running 5 tests    ... test result: ok. 5 passed; 0 failed; ...
running 1 test     ... test result: ok. 0 passed; 0 failed; 1 ignored; ...
running 15 tests   ... test result: ok. 15 passed; 0 failed; ...
running 1 test     ... test result: ok. 1 passed; 0 failed; ...

$ cargo test -p rg-ci
running 45 tests   ... test result: ok. 45 passed; 0 failed; ...
running 1 test     ... test result: ok. 1 passed; 0 failed; ...      (tests/quota_backends.rs, SQLite)

$ cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 12.26s   （零警告）

$ scripts/check.sh --quick
==> cargo fmt --all --check              (1s)
==> cargo clippy --workspace --all-targets -D warnings   (0s)
==> checks passed in 1s (quick)
```

额外跑（改动涉及 rg-http / rg-core，不在验收命令内）：

```
$ cargo test -p rg-http --lib
running 105 tests  ... test result: ok. 105 passed
$ cargo test -p rg-git --lib secret_scan
running 28 tests   ... test result: ok. 27 passed; 1 ignored

$ cargo test -p rg-http --test ci_quota_push_tests --test file_commit_semantics_tests     --test agent_principal_tests --test ci_default_branch_tests --test error_semantics_accounts_tests
23 / 1 / 1 / 14 / 5 项，全部 ok
```

单跑回滚守卫：

```
$ cargo test -p rg-db --lib upgrade_keeps_child_rows_triggers_and_foreign_keys
test soft_delete_tests::upgrade_keeps_child_rows_triggers_and_foreign_keys ... ok
test result: ok. 1 passed
```

## 9. 跨后端实测（PostgreSQL 17 / MySQL 8.4）

临时容器（`postgres:17`、`mysql:8.4`，跑完已删除），同一份 `tests/quota_backends.rs`。错误码改成 `CoreError::Coded` 之后**重新跑过一遍**：

```
$ IRONFORGE_TEST_DATABASE_URL=postgres://ironforge:ironforge@127.0.0.1:55432/ironforge \
    cargo test -p rg-ci --test quota_backends -- --nocapture
quota counters verified on Postgres
test result: ok. 1 passed; 0 failed; finished in 0.52s

$ IRONFORGE_TEST_DATABASE_URL=mysql://root:ironforge@127.0.0.1:53306/ironforge \
    cargo test -p rg-ci --test quota_backends -- --nocapture
quota counters verified on MySql
test result: ok. 1 passed; 0 failed; finished in 2.48s
```

MySQL 8.4 上唯一索引与排序规则（`SHOW CREATE TABLE` / `information_schema` 实测）：

```
CREATE TABLE `usage_counters` (
  `id` bigint NOT NULL AUTO_INCREMENT,
  `subject_kind` varchar(16) NOT NULL,
  `subject_id` bigint NOT NULL,
  `repo_id` bigint NOT NULL,
  `metric` varchar(48) NOT NULL,
  `period_start` date NOT NULL,
  `value` bigint NOT NULL DEFAULT '0',
  `updated_at` timestamp NOT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `uq_usage_counters_subject_metric_period`
    (`subject_kind`,`subject_id`,`repo_id`,`metric`,`period_start`),
  KEY `repo_id` (`repo_id`),
  CONSTRAINT `usage_counters_ibfk_1` FOREIGN KEY (`repo_id`)
    REFERENCES `repositories` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci
```

- **长度**：五列全部整列入索引（`SUB_PART` 为 NULL），前缀总长 `16×4 + 8 + 8 + 48×4 + 3 = 275` 字节，远低于 InnoDB DYNAMIC 行格式的 3072 字节上限，不需要前缀索引。
- **排序规则**：`subject_kind` / `metric` 是 `utf8mb4_0900_ai_ci`（大小写不敏感）。两者取值都是平台固定的全小写字符串，不存在「大小写不同被误判为同一 metric」的风险；将来若允许客户端自定义 metric，需要改成二进制排序规则。
- `updated_at` 落成 MySQL `TIMESTAMP`（秒精度），与本仓其它 `timestamp_with_time_zone` 列的既有约定一致，且只用于观察。

PostgreSQL 17 侧结构：`bigint` / `date` / `timestamp with time zone`，唯一约束与 `ON DELETE CASCADE` 外键均按预期创建。

**过程中修掉的一个真实跨后端缺陷**：最初用 `pipeline::Entity::find()` 读取当日 pipeline，在 PostgreSQL 上报 `decoding column "id": Option<i64> is not compatible with INT4`——历史迁移把 `pipelines.id` 等列建成 `INTEGER`。改为走 `rg_db::ops::pipeline_ops::list_pipelines_by_repo`（内部对 PG 做了 `::bigint` 投影）后通过。这正是只在 SQLite 上测试会漏掉的问题。

## 10. 遗留风险

| 风险 | 影响 | 处置 |
| --- | --- | --- |
| 无主体的 push（`pushed_by = None`：账号已删、匿名/内部 push） | 这类触发不受配额约束，也不计数 | 与任务书「难以区分则降级」一致；有主体的 push 全部受限，plan §6 列明分支 |
| post-push hook 在后台任务里跑 | 409 不会回给 push 的 HTTP 响应；表现是「pipeline 没被创建」 | 引擎层拒绝并留 warn 日志；不改变 push 本身成功的事实（ref 已更新） |
| 外部 runner / 取消路径延迟记账 | 计数在下一次触发时才准确 | 决策点必定先重算，配额判定不受影响 |
| `seconds_for_day` 会遍历该 agent 在该仓库的全部历史 pipeline | 极端长寿命仓库的触发成本随历史线性增长 | 最小闭环可接受；后续可加 `finished_at` 索引/日表，或按 `created_at >= day_start - 1d` 预筛 |
| MySQL `TIMESTAMP` 2038 上限 | `updated_at` 仅观察用 | 与既有表一致，不阻塞 |
| 迁移不可逆风险 | `down` 必须正确 | 三个方言 DDL 单测 + 「up→down→up」单测 + `Some(27)` 回滚守卫实测通过 |
| 扫描审计在后台任务里写 | 进程在写入前崩溃会丢该行 | 事件同时由 `rg-git` 以 `tracing::warn` 记录；DB 行是审计增强不是唯一凭据 |
| sink 是进程级单例 | 并发 push 若不做路由会归错仓库 | 用 `tokio::task::try_id()` 路由回扫描任务；未注册任务的事件丢弃而不误归属，plan §6.1 写明 |
