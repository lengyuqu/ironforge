# T07 执行计划 — [#19] PR 头部漂移：旧票失效事件化 + 门禁阻断

> 权威约束见 `tasks/T07.md`；本文件记录实现顺序、CAS 时序、stale 阻断的时序理由、
> 受影响测试逐条清单、NULL head 分支处理与验证结果。
> 基线 `5f32ce4`（`cnb/main` 上的 `5f32ce4 docs(tasks): add T07 …`），分支 `agent/t07-head-drift`。

## 1. 交付边界

做：

1. 新增 `rg_core::pull_request::head_sync::sync_pr_head_on_push`：push 后把 open PR 的
   `head_sha` 用 CAS 移到新提交；`Unchanged` 不写库、`Raced` 不覆盖、`NULL` 老数据补值。
2. head 真的从 A 变到 B 时调用 T06 的
   `authorization_ops::expire_on_head_sha_change`，把绑定旧 head 的 `authorization_requests`
   就地置 `expired` + `state_reason = head_sha_changed`（不删行）。
3. `merge_gates` 新增 `head_sha_stale_gate`：stored `pr.head_sha` ≠ head 分支实时 tip 时
   `HEAD_SHA_STALE`（Fail），合并被拒；与 `readiness.rs` 已有的同名 code 是同一条规则。
4. `post_push_hooks` 把原来的 `pull_request_ops::update_open_head_sha` 调用换成
   `head_sync::sync_pr_head_on_push`（同一个位置、同一次循环）。

不做（既定后续步骤，见 `tasks/T07.md`）：门禁票数改查 `authorization_decisions`、
CI 环境审批切新表、通知/邮件、`scope_expand`、MCP 工具、路由/OpenAPI 变更。

## 2. 为什么不做表切换

`tasks/T07.md` 已给出结论，这里只记录复核结果，避免后来者再问一次：

- 票源是 `pr_reviews`。`pr_review_ops::current_human_approver_ids` 的规则是
  「每个 reviewer 取最新一条 → 只留 approve → 按 `commit_id == head_sha` 过滤 → 只留
  `users.kind = human`」。计数与「谁批了」是同一条规则（`crates/rg-db/src/ops/pr_review_ops.rs:74-125`），
  latest / `request_changes` 撤销语义有测试覆盖。
- 切到 `authorization_decisions` 需要在 approve 时双写两张表，并把 latest 语义再实现一遍，
  等于制造两个真相源；而新表的设计用途是 CI 环境审批、发布、scope 扩权。
- 因此本任务只让**新表**在 head 漂移时失效（`authorization_requests` 是步骤 2 的产物），
  PR 评审票仍留在 `pr_reviews`，靠 `commit_id` 不再等于新 `head_sha` 自然不计票。

## 3. head 同步（`crates/rg-core/src/pull_request/head_sync.rs`）

### 3.1 选哪几条 PR

```sql
SELECT * FROM pull_requests
WHERE ((repo_id = :pushed AND head_repo_id IS NULL) OR head_repo_id = :pushed)
  AND head_branch = :branch AND state = 'open'
ORDER BY id
```

条件与 `rg_db::ops::pull_request_ops::head_repository_condition` 一致（同仓库 PR：`head_repo_id`
为空且目标仓就是被推仓；fork PR：`head_repo_id` 是被推仓）。该函数是私有的，`rg-db` 不在本任务
owned paths 内，因此在 `head_sync.rs` 里复制同样三行条件并在注释里指回原处。

同一 `(repo, branch)` 可以有多条 open PR（同仓 + fork 各一条），返回值是按 id 排序的列表，
每条 PR 一个 `HeadSyncOutcome`。空列表 = 这次 push 不涉及任何 open PR。

### 3.2 CAS 与时序

```
读 PR（拿到 previous = pr.head_sha）
  ├─ previous == new_sha            → Unchanged（不写库、不产生事件、不碰票）
  └─ UPDATE pull_requests
       SET head_sha = new_sha, updated_at = now
       WHERE id = :id AND head_sha IS <previous>       -- previous 为 NULL 时用 IS NULL
     ├─ 0 行 → Raced（有人在读与写之间改了这行；不重试、不覆盖、不碰票）
     └─ 1 行 → expire_on_head_sha_change(...) → Updated { previous, expired_requests }
```

- **为什么 CAS 而不是盲写**：push 通知可能重复（两个 receive-pack 任务、重试的钩子），
  server 端的 update-branch 也会推进 head。后到的旧通知若盲写，会把 head 回滚到旧提交，
  让针对新提交的票失效。`UPDATE … WHERE head_sha = 读到的值` 让赢家只有一个，输家 0 行。
- **0 行为什么不重试**：重试会重新读到赢家写的新值，然后把「本次 push 的新值」再写回去
  ——那正是要避免的回滚。输家的职责到此为止，下一次 push / 同步会自然收敛。
- **为什么不放在事务里**：SQLite 上「先读后写」的长事务会放大锁竞争（CLAUDE.md 踩坑 21），
  而这里每条 PR 的 CAS 本身已是原子的；票的失效跟在 CAS 成功后执行，失败会把错误返回给
  调用方（push 钩子只记 warn）。即使失效那一步失败，票也已经因为 `commit_id` 不匹配而不计票，
  失效调用只是把「新表里的 pending 申请」也置为 expired，不影响正确性。

### 3.3 NULL head 的老 PR（单独分支）

`head_sha IS NULL` 的 PR 是「记录 head 之前」创建的数据。

- 判定：`NULL` 不可能「stale」——没有可比较的提交，因此
  `head_sha_stale_gate` 对它返回 `NotApplicable`，**不阻断合并**，老 PR 不会被卡死。
- 同步：`previous = None` 时 CAS 用 `head_sha IS NULL` 作守卫，直接补值并返回
  `Updated { previous_head_sha: None, .. }`。补值不是竞争，所以不会误判成 `Raced`。
- 票：`expire_on_head_sha_change` 自带 `head_sha IS NOT NULL` 过滤，绑定 NULL 的申请不会被误伤。

## 4. 旧票失效事件化

head 从 A 真正变到 B（CAS 命中）后：

```rust
authorization_ops::expire_on_head_sha_change(
    db,
    pr.repo_id,                              // 目标仓（授权集按目标仓求值）
    Action::ProtectedBranchMerge.as_str(),   // "protected_branch_merge"
    &pr.id.to_string(),                      // subject = PR id（evaluator 的约定）
    new_sha,                                 // 比较基准：不是新 head 的 pending 申请全部失效
    now,
)
```

- 该函数的语义是 `HeadSha IS NOT NULL AND HeadSha != new_sha` → `expired` + `reason`，
  **不删行**（T06 已实现并有单测）。它接受的是**新** head（`ne(new_sha)`），不是旧 head：
  这样连「更早提交」的 pending 申请也一并失效，不会漏掉中间漂移过的那一代。
- 签名与 `tasks/T07.md` 里写的 `(db, repo_id, pr_id, old_sha, "head_sha_changed")` 不同，
  但满足需要（action + subject 定位，`ne(head_sha)` 比较），未改 `authorization_ops.rs`。
- `pr_reviews` 不迁移：票仍在表里，`current_human_approver_ids(db, pr_id, Some(new_sha))`
  只统计 `commit_id == new_sha` 的票，旧票自然为 0。

## 5. 门禁阻断 stale（行为变更）

### 5.1 规则

`merge_gates::head_sha_stale_gate(pr, live_head)`：

| stored | live | 结果 |
| --- | --- | --- |
| `Some(stored)` | `Some(live)`，且 `stored != live` | **Fail**，code `HEAD_SHA_STALE` |
| `Some(stored)` | `Some(live)`，相等 | Pass |
| `None`（老数据） | 任意 | NotApplicable |
| `Some(_)` | `None`（分支读不到/已删） | NotApplicable（缺失分支另有 `HEAD_BRANCH_DELETED` 判定） |

message 固定说明「批准与 CI 是针对记录的旧提交判定的，请先把 PR 头同步到分支最新提交并重新批准」。

`evaluate_protection` 的第一个门禁就是它（在规则匹配之前），所以没有保护规则的仓库也会被拦：
head 漂移同样让 CI 结论失效，不只是票。`check_merge_allowed` 的 `refusal` 因此也会是它。

### 5.2 live head 从哪来（调用点串联）

门禁模块没有仓库路径，也不该为了一个 ref 去拼路径。实时 tip 由调用方读一次传进来：

| 调用点 | 来源 |
| --- | --- |
| `readiness::merge_readiness` | 已有的 `head_tip`（顺带删掉了原来独立上报的 `unavailable` 版 `HEAD_SHA_STALE`，避免同一个事实报两遍） |
| `POST .../pulls/{n}/merge` | 把原本在保护检查之后才读的 `head_now` 提前读一次，同时用于 `expected_head_sha` 校验 |
| `try_auto_merge_pinned` | `live_head_sha(db, repo_root, owner, repo_name, &pr)` |
| merge queue | `live_head_sha(db, repo_root, &namespace, &repository.name, &pr)` |

只读不到（`None`）时不阻断：那属于「分支没了」，由各自路径按原有方式报告。

### 5.3 为什么这是有意的行为变更

旧行为：分支前进、stored head 仍旧 → 门禁按旧提交算票 → 允许合并（要么合并旧提交，
要么在别的检查里以 `REF_ADVANCED` / `HeadMoved` 之类的理由拒绝，理由与「批准过期」无关）。
新行为：**先把 head 同步到最新提交、再重新批准**，否则拒绝。

同步顺序保证正常情况下不会命中：`post_push_hooks` 在 push 应答后立即同步 head，所以
`stored == live`；阻断是兜底（钩子失败、老数据、手工改库、同步与合并之间的窗口）。
代价是「分支一动就要重新批准」，这正是 #19 要的语义。

## 6. 受影响测试逐条清单

| # | 测试 | 处理 |
| --- | --- | --- |
| 1 | `rg-http/tests/merge_readiness_tests.rs::a_head_that_moved_past_the_stored_commit_is_flagged_not_hidden` | **改名并改断言**：`…_is_refused_not_merged`。旧断言把 drift 归入 `unavailable` 且 `ready = null`；新断言 `HEAD_SHA_STALE` gate = `FAIL`、`ready = false`、`ready_reason` 含该 code、`unavailable` 不再重复、`merge` 返回 403 且 message 等于 gate detail、PR 仍 `open`。 |
| 2 | `rg-http/tests/pr_permission_tests.rs`（`check_merge_allowed` 调用点，约 573 行） | 编译级修复：补 `Some(&new_head)`（该测试刚把分支推到 `new_head`）。语义不变，仍断言「旧批准不计票 → 拒」。 |
| 3 | `rg-http/tests/auto_merge_pinned_head_tests.rs`（2 个用例） | **无需改动**。它们本就把 `stored` 手工留在旧提交、期望「不合并（pending / 回队列）」；新门禁给出同样结论（`Forbidden` → pending / waiting）。第二段把 `stored` 同步到 live 后再评估，仍照常合并。 |
| 4 | `rg-http/tests/pr_review_tests.rs`（5 个用例） | **无需改动**。它们只断言投票权限，不触发 merge gate。 |
| 5 | `rg-core` 既有 319 个单测 | **无需改动**，全部保持绿。`branch_protection::service::shared_rule_tests` 不经过 `evaluate_protection`。 |

新增覆盖：

- `rg-core/src/pull_request/head_sync.rs::tests`（7 个）：同步、`Unchanged` 不写库（用 `updated_at`
  未变作证）、CAS 输家不覆盖、NULL 补值、closed/其它分支不动、fork PR 按源仓匹配、
  旧 head 的 pending 申请就地过期（且别的 PR 的申请不受影响）。
- `rg-http/tests/merge_readiness_tests.rs::a_push_moves_the_recorded_head_and_the_old_approval_stops_counting`：
  端到端覆盖「push 新 commit → `pr.head_sha` 更新（轮询推送钩子）→ 旧票仍在 `pr_reviews` 但不再计数
  → `REQUIRED_APPROVALS_MISSING` FAIL → 合并 403」，并顺带断言同步后 `HEAD_SHA_STALE` 为 PASS。

## 7. 触碰路径（含一处必要的越界说明）

owned paths 内：

- `crates/rg-core/src/pull_request/head_sync.rs`（新建）
- `crates/rg-core/src/pull_request/mod.rs`（`pub mod head_sync;` 一行）
- `crates/rg-core/src/branch_protection/merge_gates.rs`
- `crates/rg-core/src/pull_request/readiness.rs`
- `crates/rg-http/src/lib.rs`（仅 `post_push_hooks` 内那一段；`build_routes` 未触碰）
- 测试：`crates/rg-http/tests/merge_readiness_tests.rs`、`pr_permission_tests.rs`

owned paths 之外（**必要串联，非扩范围**）：

- `crates/rg-core/src/branch_protection/service.rs`：`check_merge_allowed` 增加 `live_head` 参数。
- `crates/rg-core/src/pull_request/service.rs`：`try_auto_merge_pinned` 读一次 live head 传入。
- `crates/rg-core/src/pull_request/merge_queue.rs`：同上（`namespace` 已在作用域内）。
- `crates/rg-http/src/api/pulls.rs`：把 `head_now` 的读取提前到保护检查之前并传入。

理由：第 3 条要求「门禁按实时 tip 判定」，而门禁模块拿不到仓库路径；把实时 tip 作为参数
从已有读取点传入，比在 `merge_gates` 里再造一套路径解析更小、更可测，也避免 readiness 与 merge
各自读一次 ref。这些文件不属于 T08 的独占段（`build_routes` / `openapi.rs` / `route_meta.rs`），
改动仅为签名串联，未改任何判定逻辑或路由。

未触碰：`Cargo.*`、`crates/rg-db/src/migrations/`、`crates/rg-db/src/ops/authorization_ops.rs`、
`CLAUDE.md`、`ironforge-docs/`、`.cnb.yml`、`.github/`、`scripts/`、`deploy/`。

## 8. 验证（本分支实跑）

```
$ cargo test -p rg-core
test result: ok. 326 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 12.35s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 0 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.57s
test result: ok. 1 passed; 0 failed; 5 ignored; 0 measured; 0 filtered out; finished in 0.09s

$ cargo test -p rg-http --test merge_readiness_tests
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.30s

$ cargo test -p rg-http --test pr_review_tests
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.80s

$ cargo test -p rg-http --test auto_merge_pinned_head_tests
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.54s

$ cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.37s

$ scripts/check.sh --quick
==> cargo fmt --all --check
==> cargo clippy --workspace --all-targets -D warnings
==> checks passed in 2s (quick)
```

全量 `cargo test -p rg-http` 亦跑过（两次通过）：其中一次
`auto_merge_pinned_head_tests::the_merge_queue_puts_a_moved_head_back_in_the_queue` 在高负载下
失败过一次，随后两次全量与多次单独运行均通过。该用例本身用「手工把 stored 留在旧提交」来
制造窗口，而 push 钩子是异步的（提交内容 API 会 `spawn` 后置钩子），存在固有竞态；见 §9。

## 9. 遗留风险

1. **push 钩子与手工改库的窗口**：同步是异步的（`spawn_post_push_hooks`）。在「push 应答已返回、
   钩子尚未写库」的毫秒级窗口里合并，会命中 `HEAD_SHA_STALE` 并被拒。这是安全的一侧
   （拒绝而不是误合并），客户端重试即可。
2. **`updated_at` 的证据强度**：`Unchanged` 用「不写库」测试以 `updated_at` 未变作证；
   MySQL 时间戳精度是秒（CLAUDE.md 踩坑 23），若未来有别的路径在同一秒内改这行则证据变弱。
   生产语义本身是 CAS，不依赖这个断言。
3. **`pull_request_ops::update_open_head_sha` 变成无生产调用者**（只剩 `pr_permission_tests`
   在用）。它不做 CAS，留着有被误用的风险；但它在 `rg-db`（本任务禁止改的路径），
   建议后续任务删除或标注 `#[deprecated]`。
4. **`expire_on_head_sha_change` 失败不回滚 CAS**：head 已移动、票未标记 expired 时，函数返回
   错误，钩子只记 warn。票数不受影响（`pr_reviews` 按 `commit_id` 过滤），但新表里可能残留
   pending 申请，等它们自然过期（TTL）或下一次 head 变更时清掉。
5. **未在 CI 验证**：本仓库当前没有可用 CI 环境（CLAUDE.md），以上均为本机结果。
