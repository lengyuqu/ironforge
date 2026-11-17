# T21 Task 确认 — [#19 步骤6 补丁] 目标 scope 绑进 `scope_expand` 票据

> 权威状态在 `tasks/T21.md`；本文件记录约束确认、触碰路径、测试映射、被改造的既有测试、
> 验收命令真实输出与偏差。方案推导见 `docs/plans/T21-plan.md`。

## 1. 约束确认

| 约束 | 结论 |
| --- | --- |
| subject 格式 `{token_id}:{target_scopes}` | 是；`parse_subject` 单一解析出口 |
| token id 正整数、无前导零 | 是；`u64` 解析 + 往返比对 + `i64::try_from`，拒 `0`/`-3`/`007`/`1.5`/`+10`/空 |
| scopes 过 `validate_agent_token_scopes` | 是；`parse_subject` 末尾调用，`widening_of` 保留原调用 |
| 缺冒号 / 任一段非法 → `invalid_input` | 是；`"10"` / `"10:"` / `":repo:write"` / `"10:repo:write:extra"` 均验证 |
| `consume_scope_expand` 精确匹配 | 是；`request.subject == "{token_id}:{normalize(target_scopes)}"` |
| 「target 覆盖 current」语义校验保留 | 是；`widening_of` 未改，精确匹配之后仍执行 |
| 不改 `payload.rs` / 迁移 / `Cargo.*` | 是；只读 `payload.rs:22` 作为前提 |
| 不改 `rg-http` / `rg-db` / `scripts/` / `.github/` / `.cnb.yml` / `deploy/` / `rg-mcp` / `CLAUDE.md` / `ironforge-docs/` | 是 |
| 不删 T20 事务性测试 | 是；`a_consume_failure_rolls_back_the_scope_change` 保留，仅改 subject 格式 |
| 纯数字 subject 的其它调用点 | 无；全仓 `ScopeExpand` 只出现在 `authorization/{action,evaluator,mod,scope_expand}.rs` |

## 2. 触碰路径

修改（全部在 owned paths 内）：

- `crates/rg-core/src/authorization/scope_expand.rs`
  - 新增 `parse_subject`（`pub(crate)`，格式与 scopes 校验的唯一出口）；
  - `apply`：subject 改为解析 + `token_id` 比对 + **精确匹配**（新增门），`widening_of` 保留；
  - `ensure_decider_is_not_the_agent_owner`：`subject.parse::<i64>()`（新格式下会失败并 fail-open）
    改为 `parse_subject(..)?`，失败即报错；
  - 模块文档新增「What the approval is bound to」；`consume_scope_expand` 文档说明两道门；
  - 末尾新增 `#[cfg(test)] mod tests`：`a_subject_is_a_token_id_then_canonical_scopes`、
    `a_malformed_subject_is_invalid_input`。
- `crates/rg-core/src/authorization/evaluator.rs`：`ScopeExpand` 分支改用 `parse_subject` 取 token id，
  注释说明目标为何在 subject 里。
- `crates/rg-core/src/authorization/action.rs`：`ScopeExpand` 注释补 subject 格式。
- `crates/rg-core/src/authorization/mod.rs`：测试夹具 `file_scope_expand` 增加 `target_scopes` 参数
  （subject = `"{token_id}:{target}"`）；改造受影响的既有测试；新增 3 条测试。

新增文档：`docs/plans/T21-plan.md`、`docs/plans/T21-task.md`。

未改：`payload.rs`、任何迁移、`Cargo.*`、`rg-http/**`、`rg-db/**`、`rg-mcp/**`、`scripts/**`、
`.cnb.yml`、`.github/**`、`deploy/**`、`CLAUDE.md`、`ironforge-docs/**`。

## 3. 测试映射

| # | 要求 | 测试 |
| --- | --- | --- |
| 1 | **批准 `10:repo:write`、执行 `repo` → 拒绝**（核心） | `authorization::tests::the_target_scope_cannot_be_widened_beyond_what_was_approved`：`InvalidInput`，token 仍 `repo:read`，票据仍 `pending`，随后 `repo:write` 仍可成功 |
| 2 | 变窄也不行 | `the_target_scope_cannot_be_narrowed_either`：`repo:read` / `repo:collab` → `InvalidInput`，token 与票据状态不变 |
| 3 | 完全一致则成功 | `a_scope_expand_widens_the_same_token_in_place`（`10:repo:write` → `repo:write`）+ `a_failed_scope_expand_can_be_retried` |
| 4 | hash 敏感性 | `the_payload_hash_covers_the_target_scope`：`payload_hash(.., "10:repo:write", ..) != payload_hash(.., "10:repo", ..)`，且同输入相等 |
| 5 | 格式校验 | `a_scope_expand_subject_binds_the_token_id_and_the_target_scope`（`"10"` / `"10:"` / `":repo:write"` / `"10:repo:write:extra"` / `"0:..."` / `"-3:..."` / `"007:..."` / `"1.5:..."` / `"+10:..."` / `"10:not-a-scope"` / `"10:user"` / `"10:repo:write,admin"` / 非 canonical 顺序）+ `scope_expand::tests::a_malformed_subject_is_invalid_input` |
| 附加 | subject 匹配但目标不更宽 → 语义门仍生效 | `narrowing_or_moving_a_token_sideways_is_refused` 的 `11:repo:read` 用例 |
| 附加 | agent owner 守卫在新格式下仍生效 | `the_person_behind_the_agent_cannot_approve_its_expansion`（403 `AUTHORIZATION_SELF_APPROVAL`） |
| 附加 | 事务回滚（T20 保留） | `a_consume_failure_rolls_back_the_scope_change`：`ALREADY_CONSUMED` 409，token 回滚为 `repo:read` |

## 4. 被改造的既有测试清单

| 测试 | 改动 |
| --- | --- |
| `scope_expand_is_evaluated_by_repository_administrators` | evaluate 的 subject `"10"` → `"10:repo:write"` |
| `a_scope_expand_subject_must_be_a_token_id` | **重命名并重写**为 `a_scope_expand_subject_binds_the_token_id_and_the_target_scope`：断言从「必须是数字」扩展为「两段格式 + 两段内容」；`token-10` 由 `"10"` 等新用例覆盖 |
| `scope_expand_is_confined_to_the_approving_repository` | 四个 subject 补 `:repo:write` |
| `user_and_admin_cannot_be_bought_with_an_expansion` | 申请改为 `10:repo:write`（目标仍由 `target_scopes` 参数尝试越界） |
| `narrowing_or_moving_a_token_sideways_is_refused` | 申请改为 `12:repo`；新增 `11:repo:read` 的「匹配但不更宽」用例 |
| `a_scope_expand_widens_the_same_token_in_place` | 申请改为 `10:repo:write`；**移除**末尾「已消费后再传 `repo` → `ALREADY_CONSUMED`」的三行断言，原因见第 7 节偏差 1 |
| `the_token_must_match_the_approved_subject` | 申请改为 `10:repo:write` |
| `an_unapproved_ticket_cannot_widen_a_token` / `an_agent_cannot_approve_or_execute_an_expansion` / `the_person_behind_the_agent_cannot_approve_its_expansion` / `a_consume_failure_rolls_back_the_scope_change` / `a_failed_scope_expand_can_be_retried` | 仅适配 `file_scope_expand` 的新签名，断言未改 |
| `file_scope_expand`（夹具） | 新签名 `(db, token_id, target_scopes, key)`，subject 由两段拼出 |

## 5. 验收命令真实输出（本机，2026-11-17）

```
$ cargo test -p rg-core
     Running unittests src/lib.rs (target/debug/deps/rg_core-...)
test result: ok. 393 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 16.42s
     Running tests/collab_search_tests.rs ... test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running tests/issue_event_txn_tests.rs ... test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.70s
     Running tests/multi_backend_smoke.rs ... test result: ok. 0 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/outbox_sweep_tests.rs ... test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.06s
     Running tests/pr_merge_event_txn_tests.rs ... test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.89s
     Running tests/pr_open_close_event_txn_tests.rs ... test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.72s
     Running tests/release_event_txn_tests.rs ... test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.70s
     Running tests/webhook_envelope_tests.rs ... test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.99s
   Doc-tests rg_core ... test result: ok. 1 passed; 0 failed; 5 ignored; 0 measured; 0 filtered out; finished in 0.08s
```

```
$ cargo test -p rg-db
     Running unittests src/lib.rs ... test result: ok. 62 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 14.28s
     Running tests/authorization_store.rs ... test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.48s
     Running tests/ci_integer_compat.rs ... test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.45s
     Running tests/collab_groups_store.rs ... test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.54s
     Running tests/collab_migration.rs ... test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/collab_store.rs ... test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.72s
   Doc-tests rg_db ... test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.07s
```

```
$ cargo clippy --workspace --all-targets -- -D warnings
    Checking rg-core v0.2.0 (/Users/yuqu/vibeCodeing/ironforge-t21/crates/rg-core)
    Checking rg-ssh v0.2.0 (...)
    Checking rg-ci v0.2.0 (...)
    Checking rg-cli v0.2.0 (...)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 58.43s
    (exit 0)
```

```
$ scripts/check.sh --quick
==> cargo fmt --all --check
    (2s)
==> cargo clippy --workspace --all-targets -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.29s
    (0s)
==> checks passed in 2s (quick)
```

## 6. 关键实现点（便于 review）

- 精确匹配用 `pat_scope::normalize_scopes(target_scopes)` 后再拼字符串，而不是直接拼原始入参：
  执行参数里的空格/重复 scope 不会造成「语义相同、字符串不等」的误拒；同时因为
  `parse_subject` 要求票据侧的 scopes 是 canonical 形式，两边比较的是同一套拼写。
- 精确匹配放在 `load_expandable_token` 之前（不合法目标不查库），`widening_of` 在拿到 token 之后
  （需要当前 scopes），顺序与 T20 的事务结构一致：`UPDATE` 仍先于 `consume`，失败回滚不变。
- `effect_hash` 未改（`scope_expand:{request_id}:{token_id}:{target}`），审计语义不变。

## 7. 偏差与说明

1. **`a_scope_expand_widens_the_same_token_in_place` 删掉了末尾的 `ALREADY_CONSUMED` 三行断言。**
   该断言原来是「成功扩权后再传更宽的 `repo` → 409 已消费」。现在两道门都在 `consume` 之前：
   成功扩权后 token 已是 `repo:write`，重复传 `repo` 会先被精确匹配拒绝（`InvalidInput`）；
   若传批准目标 `repo:write`，会先被 `widening_of` 的「没有更宽」拒绝。也就是说，
   「已消费」只有在目标**仍然构成一次合法扩权**时才会被触及（例如 token scopes 被回退过）。
   该场景已由保留的 `a_consume_failure_rolls_back_the_scope_change` 覆盖（先被通用 `consume` 烧票、
   再执行 `consume_scope_expand` → `ALREADY_CONSUMED` 409 且 token 回滚），因此本节测试不再重复。
   没有放宽任何断言：票据 `state == "consumed"`、`consumed_by`、`consumed_at`、`effect_hash` 仍在断言中。
2. **`a_scope_expand_subject_must_be_a_token_id` 被重命名**而非保留原名改内容：断言范围从「是数字」
   变成「两段格式」，旧名字描述的约束已不存在。`token-10` 用例由新用例中的 `"10"` / `"-3:..."` 覆盖。
3. **`ensure_decider_is_not_the_agent_owner` 从 fail-open 改为 fail-closed**（解析失败返回错误）。
   这是新格式下的必要修正：原实现 `parse::<i64>()` 对新 subject 恒失败，会让 owner 自批守卫静默失效。
