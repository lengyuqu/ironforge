# T11 Task 确认 — [#18] Agent 草稿分支命名空间（TTL + 不通知 + 可跑 CI）

> 权威状态在 `tasks/T11.md`；本文件记录约束确认、触碰路径、验收命令真实输出与遗留风险。
> 状态机、通道清单、清理绕过理由与安全边界实现位置见 `docs/plans/T11-plan.md`。

## 1. 约束确认

| 约束 | 结论 |
| --- | --- |
| 表 `agent_draft_namespaces`：`prefix` / `repo_id` / `owner_user_id` / `ttl_secs` / `expires_at` / `grace_secs` / `state` / `extended_count` | 是，另加 `id`、`created_at`，无多余列 |
| 迁移 `m20261115_000001_create_agent_draft_namespaces.rs`，显式表名 + `has_table`/`has_column`/`has_index` 守卫，`up` 幂等 | 是；`up` 在表已存在时逐列补，测试 `up_is_idempotent_and_repairs_a_missing_column` |
| `Migrator::down` 28 → 29 | 是，`crates/rg-db/src/lib.rs` 唯一改动；`upgrade_keeps_child_rows_triggers_and_foreign_keys` 通过 |
| 推送守卫：草稿分支仍跑 CI、仍写 `repo_events`，跳过 push webhook / 分支 webhook / ws / 邮件 | 是；另有第四处 `on_delete` 的 `branch.deleted` 一并抑制（R18-5） |
| 拆 `trigger_event` → `record_event` / `trigger_event_quietly`，不用 bool 关全部 | 是，两个独立函数，静默路径不经过 `dispatch`，见 plan §4 |
| 1h 轮询（照 `task_supervisor`） | 是，`agent_drafts.cleanup` 注册在 `run()` 的三段既有轮询之后 |
| 到期先 `expiring` 关闸，再等 `grace_secs`，才 `delete_ref_if` CAS 直删 | 是；两阶段 tick，`expiry_closes_the_gate...` 验证"关了闸但没删 / 再推被拒 / 宽限后才删" |
| 绕开 `on_delete` 并写审计 `agent_draft.cleaned` 说明原因 | 是，审计含 `bypassed_guard` 与 `bypass_reason`；测试用"open PR 占用"的场景证明 `DELETE /branches` 会拒而清理不拒 |
| ref 已不存在 → 标记 deleted 而非报错 | 是，`delete_ref_if` 的 "moved or no longer exists" 计入 `vanished_refs`；`repo` 已软删则直接标记 cleaned |
| 四个 REST 端点（建 / 续期 / 软删 / 列） | 是，见 plan §7 |
| 续期最多累到初始 TTL 的 4 倍 | 是，上限 `created_at + 4 × ttl_secs`，到顶返回 400；集成测试把 8 次 `+3600s` 打到顶，断言最终 `expires_at == created_at + 4h` |
| 路由三处同时补齐 | 是：`lib.rs` 路由段、`openapi.rs`、`rg-core/src/auth/route_meta.rs`；`route_metadata_tests` / `openapi_registration_tests` 全绿 |
| 草稿分支**完全不注入 secrets**（不是按 ref 过滤） | 是，内置 runner（`runner.rs`）与外部 runner（`api/runners.rs`）两处整段跳过 |
| 草稿分支**忽略 `allow_host_execution`**，只许容器内 | 是，三层：`trigger_pipeline` 强制 false、`post_push_hooks` 传 false、`run_job` 对草稿 + 无 image 直接失败 |
| 两条安全边界都有代码分支 + 测试，并在 plan 写成"安全前提" | 是；plan §6，测试 `a_draft_branch_gets_no_secrets_and_no_host_execution`（rg-ci）、`a_draft_pipeline_is_handed_no_repository_secrets`（rg-http）、`a_draft_push_runs_ci_...`（断言 `allow_host_execution == false`） |
| 用 `refs/heads/agent/<id>/...`，不用 `refs/agent/...` | 是，plan §2 |
| 不改 `Cargo.toml` / `Cargo.lock`（需要新依赖先停下报告） | 是，无新依赖，`git status` 无 `Cargo.*` |
| 不改 `CLAUDE.md` / `ironforge-docs/` / `.cnb.yml` / `.github/` / `scripts/` / `deploy/` / `crates/rg-mcp/` | 是，全部未触碰 |
| 不顺手做 #15 outbox、#63 符号表、#41 沙箱 | 是 |
| 不改设计文档（PR #20 已合并） | 是，`ironforge-docs/` 零改动 |
| 行号用 grep 定位，不照抄 | 是；设计文档行号已漂移，全部按现树定位 |
| 跨后端（PG 17 / MySQL 8.4）各跑一次，跑不了如实说明 | 是，都跑了，见 §4 |

## 2. 触碰路径

`crates/rg-db`

- `src/migrations/m20261115_000001_create_agent_draft_namespaces.rs`（新建）
- `src/migrations/mod.rs`（注册）
- `src/entities/agent_draft_namespace.rs`（新建，含 `STATE_ACTIVE` / `STATE_EXPIRING` / `STATE_DELETED`）
- `src/entities/mod.rs`
- `src/ops/agent_draft_ops.rs`（新建，行级生命周期）
- `src/ops/mod.rs`
- `src/lib.rs`（仅 `Migrator::down` 步数 28 → 29 与注释）

`crates/rg-core`

- `src/repo/draft.rs`（新建：前缀校验、最长前缀匹配、拒绝模式、extend 算术）
- `src/repo/mod.rs`
- `src/webhook/service.rs`（拆 `record_event` / `trigger_event_quietly`，抽出 `record_envelope`）
- `src/auth/route_meta.rs`（两行声明）
- `tests/webhook_envelope_tests.rs`（新增 `a_quiet_event_is_logged_but_never_delivered`）

`crates/rg-ci`

- `src/lib.rs`（`trigger_pipeline` 对草稿强制 `allow_host_execution = false`）
- `src/runner.rs`（`job_environment` 跳过 secrets；`run_job` 拒绝草稿的 host execution；新增测试）

`crates/rg-http`

- `src/api/drafts.rs`（新建：四个 handler + 清理 tick + 审计）
- `src/api/mod.rs`
- `src/lib.rs`（后台任务注册、路由段、`rejected_refs` 增补、`on_delete` 抑制、`post_push_hooks` 的 `is_draft` 分支）
- `src/api/runners.rs`（外部 runner 的 secrets 守卫 —— **owned paths 之外的唯一一处**，见 §5.1）
- `src/openapi.rs`
- `tests/agent_drafts_tests.rs`（新建，6 个集成测试）
- `tests/route_metadata_tests.rs`（legacy oracle 增补 `agent-drafts` 两行，与既有 cherry-pick/revert 的先例一致）

文档：`docs/plans/T11-plan.md`、`docs/plans/T11-task.md`（本文件）、`tasks/T11.md`（`State` → `completed`）。

## 3. 验收命令真实输出

### 3.1 `cargo test -p rg-db`

```
test result: ok. 49 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 10.27s
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.46s
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.35s
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.53s
test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.68s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.11s
```

### 3.2 `cargo test -p rg-core`

```
test result: ok. 366 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 10.77s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 0 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.99s
test result: ok. 1 passed; 0 failed; 5 ignored; 0 measured; 0 filtered out; finished in 0.10s
```

（新增单测 7 个：`repo::draft::tests::*`；新增集成测试 1 个：`a_quiet_event_is_logged_but_never_delivered`。）

### 3.3 `cargo test -p rg-http`

```
EXIT=0
133 个 `test result: ok`，858 个测试通过，0 failed
```

新增集成测试文件 `tests/agent_drafts_tests.rs` 的 6 个全部通过：

```
test a_namespace_is_registered_validated_and_listed ... ok
test a_draft_push_runs_ci_and_is_logged_but_notifies_nobody ... ok
test expiry_closes_the_gate_before_the_cleanup_deletes_anything ... ok
test cleanup_deletes_a_branch_an_open_pull_request_occupies_and_audits_the_bypass ... ok
test extend_never_shortens_and_stops_at_four_times_the_ttl ... ok
test a_draft_pipeline_is_handed_no_repository_secrets ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 7.77s
```

### 3.4 `cargo clippy --workspace --all-targets -- -D warnings`

```
EXIT=0
    Checking rg-cli v0.2.0 (/Users/yuqu/vibeCodeing/ironforge-t11/crates/rg-cli)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 17.44s
```

### 3.5 `scripts/check.sh --quick`

```
EXIT=0
==> checks passed in 2s (quick)
```

（首次运行失败于 `cargo fmt --all --check`；`cargo fmt --all` 后重跑通过。）

## 4. 跨后端真实输出

容器：`postgres:17`（`127.0.0.1:55432`，库 `ironforge`）、`mysql:8.4`（`127.0.0.1:53306`，
`--log-bin-trust-function-creators=1`，库 `ironforge`）。每次跑前 drop/create 数据库。测试串行
（`--test-threads=1`）：跨后端共享一个库，`run_cleanup_once` 是全局的。

### 4.1 PostgreSQL 17

```
# IRONFORGE_TEST_DATABASE_URL=postgres://ironforge:ironforge@127.0.0.1:55432/ironforge
# cargo test -p rg-db --lib m20261115
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 45 filtered out; finished in 0.41s

# cargo test -p rg-http --test agent_drafts_tests -- --test-threads=1
test a_draft_pipeline_is_handed_no_repository_secrets ... ok
test a_draft_push_runs_ci_and_is_logged_but_notifies_nobody ... ok
test a_namespace_is_registered_validated_and_listed ... ok
test cleanup_deletes_a_branch_an_open_pull_request_occupies_and_audits_the_bypass ... ok
test expiry_closes_the_gate_before_the_cleanup_deletes_anything ... ok
test extend_never_shortens_and_stops_at_four_times_the_ttl ... ok
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 11.51s
```

### 4.2 MySQL 8.4

```
# IRONFORGE_TEST_DATABASE_URL=mysql://ironforge:ironforge@127.0.0.1:53306/ironforge
# cargo test -p rg-db --lib m20261115
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 45 filtered out; finished in 3.19s

# cargo test -p rg-http --test agent_drafts_tests -- --test-threads=1
test a_draft_pipeline_is_handed_no_repository_secrets ... ok
test a_draft_push_runs_ci_and_is_logged_but_notifies_nobody ... ok
test a_namespace_is_registered_validated_and_listed ... ok
test cleanup_deletes_a_branch_an_open_pull_request_occupies_and_audits_the_bypass ... ok
test expiry_closes_the_gate_before_the_cleanup_deletes_anything ... ok
test extend_never_shortens_and_stops_at_four_times_the_ttl ... ok
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 11.27s
```

MySQL 首次失败于 `1419 (HY000): You do not have the SUPER privilege and binary logging is enabled`：
那是**既有**迁移里的触发器/函数（与本任务的建表无关），容器加 `--log-bin-trust-function-creators=1` 后
全部通过。这一点如实记录：默认参数的 MySQL 8.4 跑不了本仓库的完整迁移，需要放开该开关或使用有 SUPER
的账号。

### 4.3 SQLite（默认）

见 §3；`down_and_up_round_trip_on_the_backend_from_the_environment` 在未设环境变量时直接 return。

## 5. 需要上报的两处偏离

### 5.1 `crates/rg-http/src/api/runners.rs`（owned paths 之外）

复核 R18-7：secrets 有**两个**注入点，且 `external_runners = true` 时内置 runner（`rg-ci/src/runner.rs`）
根本不启动。只在 `rg-ci` 里堵，等于开着外部 runner 的实例没有任何保护，而且 secrets 会离开本进程。
改动是 `poll_job` 里一个 `is_draft_ref` 判断包住既有的 `decrypted_repo_secrets` 调用（净增约 20 行），
没有改装配路径、没有动 secrets 的存储或可见范围、没有全局禁用。测试
`a_draft_pipeline_is_handed_no_repository_secrets` 通过真实 `GET /runners/{id}/jobs/poll` 验证：草稿 ref
的 job 里没有 secret，普通 ref 的 job 里仍有。认为这是完成任务所必需的最小改动，而不是"大改 rg-ci 装配路径"。

### 5.2 `crates/rg-http/tests/route_metadata_tests.rs` 的 legacy oracle

`route_metadata_tests` 的 oracle 是"注册表替换前的旧表"快照，新增路由必须同步（既有先例：cherry-pick /
revert / apply-patch 三条路由都在 oracle 里加过分支）。加了 `["agent-drafts"]` 与 `["agent-drafts", _]` 两行，
其余断言未动。

## 6. 遗留风险

见 `docs/plans/T11-plan.md` §9。摘要：

1. R18-5 的"假 `branch.created`"在**非草稿**路径仍在（草稿路径已抑制两种形态）。
2. 草稿分支没有独立的 CI 超时上限（`job_timeout_secs` 是实例级参数，收紧会改动所有 pipeline）。
3. 清理不取消 pipeline、不记录 `pipeline_ids`（T11.md 的表结构无此列）。
4. 每次推送多一次 `list_by_repo`（按 ref 计）；未加缓存。
5. 外部 runner 的日志遮蔽仍会解密全仓 secrets（仅用于 `mask_values`，不下发给 job）。
6. `refs/pull/*` 可被无白名单令牌改写（R18-1，与本任务无关）：清理已能处理被 open PR 占用的分支，
   但不解决"PR 能被伪造"本身。
