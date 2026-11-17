# T20 Task 确认 — [#19 步骤6] `scope_expand` 执行面：repo 系列内提权

> 权威状态在 `tasks/T20.md`；本文件记录约束确认、触碰路径、9 条测试映射、验收命令真实输出与遗留风险。
> 方案取舍（扩权语义、7 天、`has_scope` 现状、「扩大」判定、effect_hash、跨仓库边界）见
> `docs/plans/T20-plan.md`。

## 1. 约束确认

| 约束 | 结论 |
| --- | --- |
| `ScopeExpand` 不再是 501 | 是；`evaluator.rs` 返回 repo admins / 1 票规则 |
| 非数字 subject → `invalid_input` | 是；`parse_id(.., "token id")`，`0`/负数同样拒绝 |
| 目标不能是 `user`/`admin` | 是；`validate_agent_token_scopes(target)` |
| 收缩被拒 | 是；逐 scope `has_scope(target, current)` |
| 正常扩权原地生效 | 是；同事务更新 `access_tokens.scopes`，无重新签发 |
| 未批准 / 已消费 → 409 | 是；复用 `consume` 的 `AUTHORIZATION_NOT_APPROVED` / `ALREADY_CONSUMED` |
| agent 不能批准 | 是；`ensure_human_decider`，403 `AGENT_MAY_NOT_APPROVE`；执行面也拒绝 agent |
| subject 与 token 不匹配 → 拒绝 | 是；`request.subject != token_id.to_string()` → `invalid_input` |
| 更新后 consume 失败必须回滚 | 是；显式 `rollback`，测试 9 断言 token 未变 |
| 不新增迁移 / 不改 `Cargo.*` | 是；`access_tokens.scopes` 已存在；本任务零迁移 |
| 不新开 API 路由 | 是；`rg-http` 一行未改（仓库目前没有任何 authorization 路由） |
| 不碰 `pull_request/issue/release/webhook/collaboration`、`CLAUDE.md`、`ironforge-docs/`、`.cnb.yml`、`.github/`、`scripts/`、`deploy/`、`rg-mcp/` | 是 |

## 2. 触碰路径

新增：

- `crates/rg-core/src/authorization/scope_expand.rs`：`load_expandable_token`、`widening_of`、
  `ensure_decider_is_not_the_agent_owner`、`consume_scope_expand`、`TokenScopeExpandResult`、`SCOPES_CHANGED`。

修改：

- `crates/rg-core/src/authorization/evaluator.rs`：`ScopeExpand` 的真实规则（admins / 1 票）。
- `crates/rg-core/src/authorization/mod.rs`：注册模块与 re-export；`decide` 在 `evaluate` 之后对
  `scope_expand` 加「agent 主人不能批准」；测试模块新增 10 条测试与 `scope_expand_setup` 等夹具。
- `crates/rg-core/src/authorization/action.rs`：把「step 1 未实现」的注释改成现状（TTL 7 天原本就在）。

未改：`crates/rg-http/**`、`crates/rg-db/**`、`Cargo.*`、任何迁移、`scripts/`、`docs`（除本任务文档）、
`ironforge-docs/`、`CLAUDE.md`、`.cnb.yml`、`.github/`、`deploy/`、`crates/rg-mcp/`。

## 3. 9 条测试映射（全部真断言）

| # | 要求 | 测试 |
| --- | --- | --- |
| 1 | evaluator 能求值 | `authorization::tests::scope_expand_is_evaluated_by_repository_administrators`：断言 1 票、admins、非 writers/collaborators、admin 可批 / 陌生人不可批、TTL 7 天；旧的 `scope_expand_is_not_evaluated_yet`（断言 501）被替换为相反断言 |
| 2 | 非数字 subject 被拒 | `a_scope_expand_subject_must_be_a_token_id`：`session` / `token-10` / `""` / `0` / `-3` 都是 `InvalidInput`；`request()` 同样拒绝 |
| 3 | `user`/`admin` 目标被拒 | `user_and_admin_cannot_be_bought_with_an_expansion`：`repo:write,user`、`admin`、`repo:read,admin` 均拒绝，token 未变 |
| 4 | 收缩被拒 | `narrowing_or_moving_a_token_sideways_is_refused`：`repo:write → repo:read`、`repo:collab`、`repo:write`（等高）全部拒绝，票据仍 pending |
| 5 | 正常扩权 | `a_scope_expand_widens_the_same_token_in_place`：`repo:read → repo:write`，`access_tokens.scopes` 真变、`has_scope` 为真、`consumed_at/consumed_by/effect_hash(64 hex)` 写入、二次消费 409；并断言 `expires_at - created_at = 7 天` |
| 6 | 未批准 → 409；已消费 → 409 | `an_unapproved_ticket_cannot_widen_a_token`（`AUTHORIZATION_NOT_APPROVED`, 409，且 token 回滚）；测试 5 与 9（`AUTHORIZATION_ALREADY_CONSUMED`, 409） |
| 7 | agent 不能批准（403） | `an_agent_cannot_approve_or_execute_an_expansion`：`approve` 与 `consume_scope_expand` 都以 agent 身份调用 → 403 `AGENT_MAY_NOT_APPROVE`，token 未变；另有 `the_person_behind_the_agent_cannot_approve_its_expansion` 覆盖「owner 批自己 agent」→ 403 `AUTHORIZATION_SELF_APPROVAL` |
| 8 | subject 与传入 token 不匹配 | `the_token_must_match_the_approved_subject`：请求 subject=10、传 token 11 → `InvalidInput`，两 token 未变、票据仍 pending，随后用正确的 10 成功消费 |
| 9 | **事务性：consume 失败回滚 token** | `a_consume_failure_rolls_back_the_scope_change`：批准后票据先被通用 `consume` 消费，再执行 `consume_scope_expand`：业务写已发生 → `consume` 409 → 整个事务回滚，`scopes` 仍为 `repo:read`，`consumed_effect_hash` 仍是先前那次消费的值 |
| 附加 | 跨仓库边界 | `scope_expand_is_confined_to_the_approving_repository`：多仓库令牌、无 allow-list 令牌、人类令牌、未知令牌均无法求值 |
| 附加 | 失败可重试 | `a_failed_scope_expand_can_be_retried`：未批准失败 → token 回滚、票据 pending → 批准后同一票据成功 |

## 4. 验收命令真实输出（本机，2026-11-17）

```
$ cargo test -p rg-core
     Running unittests src/lib.rs (target/debug/deps/rg_core-...)
test result: ok. 388 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 16.25s
     Running tests/collab_search_tests.rs ... test result: ok. 1 passed; 0 failed
     Running tests/issue_event_txn_tests.rs ... test result: ok. 3 passed; 0 failed
     Running tests/multi_backend_smoke.rs ... test result: ok. 0 passed; 0 failed; 2 ignored
     Running tests/outbox_sweep_tests.rs ... test result: ok. 8 passed; 0 failed
     Running tests/pr_merge_event_txn_tests.rs ... test result: ok. 2 passed; 0 failed
     Running tests/pr_open_close_event_txn_tests.rs ... test result: ok. 4 passed; 0 failed
     Running tests/release_event_txn_tests.rs ... test result: ok. 2 passed; 0 failed
     Running tests/webhook_envelope_tests.rs ... test result: ok. 12 passed; 0 failed
   Doc-tests rg_core ... test result: ok. 1 passed; 0 failed; 5 ignored
```

```
$ cargo test -p rg-db
     Running unittests src/lib.rs ... test result: ok. 62 passed; 0 failed; 0 ignored
     Running tests/authorization_store.rs ... test result: ok. 5 passed; 0 failed
     Running tests/ci_integer_compat.rs ... test result: ok. 12 passed; 0 failed
     Running tests/collab_groups_store.rs ... test result: ok. 5 passed; 0 failed
     Running tests/collab_migration.rs ... test result: ok. 0 passed; 0 failed; 1 ignored
     Running tests/collab_store.rs ... test result: ok. 15 passed; 0 failed
   Doc-tests rg_db ... test result: ok. 1 passed; 0 failed
```

```
$ cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 54.24s
    (exit 0)
$ cargo clippy -p rg-http --all-targets -- -D warnings
    Checking rg-http v0.2.0 (...)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 43.33s
    (exit 0)
```

```
$ scripts/check.sh --quick
==> cargo fmt --all --check
    (1s)
==> cargo clippy --workspace --all-targets -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.92s
    (8s)
==> checks passed in 9s (quick)
```

## 5. 核实结论（不要凭记录假设的三件事）

1. **`has_scope` 层级**：基线实现已正确。`repo:write` 满足 `repo:collab`（`satisfying("repo:collab")`
   含 `repo:write`），反向不成立；`pat_scope.rs` 的 `repository_scopes_form_a_ladder` 用例钉住了这两个方向。
   本任务直接复用 `has_scope`，未改 `pat_scope.rs`。
2. **scope 缓存**：没有缓存。`rg-http/src/lib.rs::resolve_pat` 每个请求按 hash 查库，scope 判定用刚读出的
   值；换出的 JWT 只带 `tid`，不带 scope。因此更新 `scopes` 后同一个 token 下一个请求立即生效。
3. **`scope_expand_is_not_evaluated_yet`**：已改成相反断言（`scope_expand_is_evaluated_by_repository_administrators`），
   不是删除。

## 6. 遗留风险

- 目标 scope 不在票据里（无 `parameters` 列，本任务禁止迁移），执行面只能保证「不收缩、不拿 `user`/`admin`」，
  无法证明执行目标等于批准人当时看到的目标。补法：新增参数列并纳入 `payload_hash`。
- 扩权永久，无缩权接口；7 天只是票据 TTL。
- 仓库当前没有任何 authorization HTTP 路由，本任务禁止新开路由，所以只有 core API 与测试，没有端到端 HTTP 验证。
- 并发用确定性等价场景验证（票据被先消费），未起两个真实事务压测；CAS + 单事务是结构保证。
- 未跑 `cargo test -p rg-http` 全量（按任务要求，约 34 分钟）；`rg-http` 未改动，clippy 已覆盖编译。
