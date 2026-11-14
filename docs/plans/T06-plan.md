# T06 执行计划 — [#19] 通用审批原语 · 步骤 1

> 权威约束见 `tasks/T06.md`；本文件记录实现顺序、状态机时序、求值器等价表、越权修复影响面与跨后端结果。
> 基线 `0c870a5`，分支 `agent/t06-authorization`。

## 1. 交付边界

做：

1. `authorization_requests` / `authorization_decisions` 两张表 + `m20261114_000002` 迁移（显式表名、`has_table`/`has_column` 守卫、`down` 真删）。
2. `rg-db::ops::authorization_ops` 状态机原语（申请、投票、消费、作废、按 `head_sha` 失效）。
3. `rg-core::authorization` 求值器 + `payload_hash` + action 平台枚举。
4. 修 `POST .../pulls/{number}/reviews` 的越权（复核 N12）。

不做（既定后续步骤）：CI 环境审批切新表、合并门禁改查 `authorization_decisions`、通知偏好/邮件、`scope_expand`。

## 2. 数据模型

### 2.1 `authorization_requests`

| 列 | 类型 | 说明 |
| --- | --- | --- |
| `id` | bigint PK auto | 自增，刻意不用 UUID（按 id 盲审） |
| `repo_id` | bigint | FK `repositories.id` cascade |
| `action` | varchar(48) | 平台枚举（`rg_core::authorization::Action`），不接受客户端自由字符串 |
| `subject` | varchar(255) | 裁定域：`pr_id` / `job_id` / `ref` / `package_version` / `session_id`（十进制或 ref 字符串） |
| `head_sha` | varchar(64) null | 绑定点，经 `normalize_commit_sha` 归一 |
| `requested_by_kind` | varchar(16) | `human` \| `agent_session` |
| `requested_by_id` | bigint | `users.id` 或 `agent_sessions.id`，刻意**不**建 FK（两种主体共列） |
| `authorization_set_id` | bigint | 授权集不可变版本的摘要 id，见 §4.3 |
| `required_approvals` | integer default 1 | 申请时冻结的阈值，见 §2.3 第 3 条 |
| `state` | varchar(16) default `pending` | `pending`/`consumed`/`expired`/`denied`/`revoked` |
| `state_reason` | varchar(48) null | `head_sha_changed` / `execution_failed_not_retryable` / `expired`（**新增列**，见 §2.3） |
| `expires_at` | timestamptz | 按 action 分级 |
| `consumed_at` | timestamptz null | |
| `consumed_by` | bigint null | 执行人；不建 FK（审计要留存） |
| `consumed_effect_hash` | varchar(128) null | 合并出的 sha / 发布版本 id |
| `client_request_id` | varchar(128) | 幂等键 |
| `payload_hash` | varchar(64) | `(repo_id, action, subject, head_sha)` 的规范化 SHA-256；步骤 1 没有 `parameters` 列，见 §4.4 |
| `created_at` / `updated_at` | timestamptz | |

索引：

- `uq_authorization_requests_client_key (repo_id, requested_by_kind, requested_by_id, client_request_id)` 唯一 —— 幂等三元组的落点。
- `ix_authorization_requests_subject (repo_id, action, subject)` —— `head_sha` 变更批量失效走它。
- `ix_authorization_requests_state (repo_id, state)`。

### 2.2 `authorization_decisions`

| 列 | 说明 |
| --- | --- |
| `id` | bigint PK auto |
| `request_id` | FK `authorization_requests.id` cascade |
| `decider_user_id` | FK `users.id` cascade（与 `ci_environment_approvals.approved_by` 同口径） |
| `decision` | `approve` \| `deny` |
| `payload_hash` | 投票时**重算**的 payload 摘要，与请求行比对防 TOCTOU |
| `created_at` | |

唯一键 `uq_authorization_decisions_request_decider (request_id, decider_user_id)`：重复票语义照 `ci_environment_ops::add_approval` 的 `Ok(false)`。

### 2.3 与方案字段表的三处偏离（写清理由）

1. **`state_reason` 是新增列**。任务要求 `head_sha` 变更标 `expired` 且"理由 `head_sha_changed`"，但字段表里没有承载理由的列；不落库就只能写在日志里，审计无法回答"为什么废票"。列可为空，纯附加。
2. **`authorization_set_id` 是摘要而非外键**。任务只允许两张表，"授权集不可变版本"没有可指向的第三张表。取 `(repo_id, action, subject, 规则标志, 显式名单排序后, required_approvals)` 的 SHA-256 前 8 字节转非负 i64：规则一变 id 就变，满足"版本"语义，且不需要新表。步骤 2/3 若需要查询/比对历史版本，可再提升为实体表；本步骤不做。
3. **`required_approvals` 是新增列**。消费点必须自己在**调用方事务内**回答"票够不够"（§3 的第 5 步），而重新求值规则要读仓库成员、组织团队、PR、分支保护——这些 ops 都只接受 `&DatabaseConnection`，在事务里重放会把"同事务消费"变成一次跨连接的判定。规则在申请时就被 `authorization_set_id` 冻结了，把它的数值一半一起冻结是同一件事，不是第二份真相。若以后要改阈值，必须新申请（这正是不可变版本的语义）。

## 3. 状态机与消费时序

状态迁移（`authorization_ops` 里每条迁移都是一次带 `WHERE` 守卫的 `UPDATE`，没有内存标志）：

```
            request()
               │
               v
          ┌─────────┐  deny(最后一票)/显式拒绝   ┌────────┐
          │ pending │ ──────────────────────────> │ denied │
          └────┬────┘                             └────────┘
    head_sha   │   consume_guarded()（同事务执行动作）
    变更       │        ┌──────────────┐
       │       └──────> │   consumed   │
       v                └──────┬───────┘
  ┌─────────┐                  │ 不可幂等执行失败（事务回滚后单独写）
  │ expired │                  v
  └─────────┘             ┌─────────┐
                          │ revoked │
                          └─────────┘
```

消费时序（唯一正确写法，代码里 `authorization::consume` 的文档注释同此）：

```rust
let txn = db.begin().await?;                       // ① 开事务
let outcome = authorization_ops::consume_guarded(  // ② UPDATE ... WHERE state='pending'
    &txn, request_id, actor_id, effect_hash).await?;
if outcome == ConsumeOutcome::AlreadyConsumed {    // ③ 0 行 → 409 AUTHORIZATION_ALREADY_CONSUMED
    return Err(CoreError::coded(409, "AUTHORIZATION_ALREADY_CONSUMED", ...));
}
execute_the_action(&txn).await?;                   // ④ 动作与置 consumed 同事务
txn.commit().await?;                               // ⑤ 成功才一起提交
```

- **执行失败**：`txn` 回滚 → ② 的 `consumed` 一起消失 → 票仍是 `pending`，重试不消耗票（幂等动作走这条）。
- **不可幂等动作失败**：回滚后调用 `authorization::revoke(db, id, "execution_failed_not_retryable")`，票作废、需重新申请。
- **`head_sha` 变更**：`authorization::expire_on_head_sha_change(db, repo_id, action, subject, new_head)` 把该 subject 下所有 `pending` 且 `head_sha` 不等于新值的请求标 `expired`/`head_sha_changed`；**只改状态不删行**（现状 `pr_review_ops.rs:74` 是过滤，这里是事件化）。
- **过期**：`approve`/`consume` 前检查 `expires_at`，超时先标 `expired`/`expired` 再拒绝，避免"票在数据库里躺着却仍可批"。

`consume_guarded` 接受 `&impl ConnectionTrait`，所以既能进调用方事务，也能直接对连接用；后者只在"没有执行面"的场景（单测）出现。

## 4. 求值器

### 4.1 形态

```rust
pub struct AuthorizationSet {
    pub id: i64,                    // 见 §2.3
    pub explicit: Vec<i64>,         // 显式名单（环境审批人 / 推送白名单 / 已请求的评审人）
    pub admins: bool,               // 仓库管理员谓词（can_admin_repo）
    pub writers: bool,              // 写权限谓词（can_write_repo）
    pub collaborators: bool,       // 协作组成员谓词（is_collab_member，刻意绕过 30s 缓存）
    pub required_approvals: u32,
}
pub async fn evaluate(db, repo, action, subject, head_sha) -> CoreResult<AuthorizationSet>;
impl AuthorizationSet { pub async fn may_approve(&self, db, repo, user_id) -> CoreResult<bool>; }
```

`may_approve` 是唯一的准入判定：`explicit` 命中直接真；否则按标志调用**现状同一个函数**（`can_admin_repo` / `can_write_repo` / `is_collab_member`），所以"绕过缓存"这类语义原样保留（`is_collab_member` 内部先 `invalidate_perm_cache_user`）。

隐式集合用谓词而不是枚举名单：仓库管理员/写者/组织成员是可变的、且组织侧要 join `teams`/`team_repositories` 才能枚举；枚举出来的名单会在申请后立刻过期，反而变成"批准时用的是一份过期名单"。谓词在**批准时**求值，等价于现状 handler 的判法。`explicit` 部分是真集合（任务要求的 `approvers`），因为它的来源本来就是一张持久名单。

### 4.2 与现状的逐条等价表

| action | 现状代码 | 求值器规则 | 等价性 |
| --- | --- | --- | --- |
| `ci_environment_release` | `api/ci_environments.rs:273` `can_admin_repo` **或** `:278` `allowed_approver_ids` 含 actor；`:285-304` 硬拒 agent；`required` 取 `ci_environments.required_approvals`；票源 `ci_environment_approvals` 唯一键 `(job_id, approved_by)` | `admins=true` ∪ `explicit=allowed_approver_ids`；`required=required_approvals`；agent 永不是批准人 | 同一组人、同一阈值、同一去重键。**本步骤无调用点**（步骤 2 才切表） |
| `protected_branch_merge` | 准入 `reviews.rs:318` 只要求**读**（N12 漏洞）；票源 `current_human_approver_ids(pr.head_sha)`（human-only、后一次 `request_changes` 覆盖前一次 approve、`commit_id` 精确匹配 head）；阈值 `required_approvals.unwrap_or(1)` | 准入 `writers=true` ∪ `explicit=该 PR 当前被请求的评审人`；票源 = decisions（human-only，唯一键去重）；`head_sha` 变更即 `expired`；阈值同 `required_approvals.unwrap_or(1)` | 票源/阈值/去重等价；**准入是故意的行为变更**，见 §5 |
| `protected_branch_push` | `is_push_allowlisted`（`branch_protection/service.rs:271`）：名单内直接放行，**不**看写权限 | `explicit=allowed_push_user_ids`，`required=1`，**不**置 `writers` | 等价：名单仍是唯一放行者。该字段只管推送，本步骤也不把它当审批人名单 |
| `release_publish` | 现状无审批机制（发布端点只查写权限） | `admins=true`，`required=1` | 纯新增，无调用点，不改变现有行为 |
| `scope_expand` | 无 | `evaluate` 返回 `Unsupported` | 步骤 6 才定义"扩权后拿到什么" |

`collaborators` 标志在步骤 1 **没有任何 action 置真**，这是刻意的：把协作成员并进 PR 合并的批准人集合，等于把 N12 的"能读就能批"换个名字复辟（协作成员含 `read` 级）。它的存在是为了让"绕过 30s 缓存的 `is_collab_member`"能原样进入原语，供步骤 2/3 在**确实以协作成员为规则**的 action 上使用。`may_approve` 的这条分支有独立单测覆盖，不是只声明不用。

### 4.3 硬约束

- **申请人 ≠ 批准人**：`authorization::ensure_separate_people(request, decider)` 一处实现，`approve()` 调用它。现有两处 HTTP 自批检查（PR 作者、CI 触发人）本步骤不动（`ci_environments.rs` 不在 owned paths）；步骤 2/3 接线时替换成这一处。
- **agent 只能申请**：`requested_by_kind == agent_session` 的请求，`approve()` 一律 `403`（`CoreError::coded(403, "AGENT_MAY_NOT_APPROVE", ...)` 复用现有常量语义），**不看** scope，也不看它是否是仓库管理员。
- **批准时重算 `payload_hash`**：`approve()` 用请求行里的 `(repo_id, action, subject, head_sha)` 重算，与存下的 `payload_hash` 比对；不一致返回 `409 AUTHORIZATION_PAYLOAD_CHANGED`。防的是"申请时绑定 SHA=A，之后行被改成 B"（TOCTOU）。步骤 1 没有 `parameters` 列，也就没有第五项可以漂移；步骤 2 若要加，必须同时加列 + 迁移，不在本步骤。
- **重复票**：`authorization_decisions` 唯一键 + `Ok(false)`，与 `add_approval` 同语义。

### 4.4 幂等三元组

`client_request_id` + `payload_hash`：

- 同 `(repo, kind, id, client_request_id)` + 同 `payload_hash` → 返回既有请求（Replayed）。
- 同 key + 不同 hash → `409 IDEMPOTENCY_KEY_REUSED`（复用 `CollabError::IdempotencyKeyReused` 的机器码语义）。
- 键格式校验复用 `collaboration::validation::client_key`（`[A-Za-z0-9._:-]{1,128}`），不新造一套。

## 5. 越权修复（唯一的行为变更）

`crates/rg-http/src/api/reviews.rs::submit_review` 现在的准入是 `require_authenticated_read`。改为：

- `comment`（含创建行内评论时隐式建的 comment review）保持**读**权限即可。
- `approve` / `request_changes` 需要 **写权限** 或 **该 PR 当前被请求的评审人**（`pr_reviewer_requests` 里有 `(pr_id, reviewer_id)` 行）。后者是"明确的审批人身份"：请求评审人本身要求 `require_pr_manager`（写权限/管理员），CODEOWNERS 自动指派也走这张表，所以它不是自授权通道。
- `dismiss` 不在此列：`current_human_approver_ids` 只统计 `approve`/`request_changes`，dismiss 行对票数没有影响，收紧它会改变与本任务无关的行为。
- 自批拒绝（`pr.author_id == user_id` 且动作是 approve/request_changes）保持不变。
- `ensure_reviewer_may_approve`（agent / token 分支限制）保持在写权限判定之后，顺序不换。

谁被挡住 / 原来能做什么：

| 主体 | 原来 | 现在 |
| --- | --- | --- |
| 公开仓库的任意登录用户（无写权限、未被请求评审） | 可投 `approve`，且会被 `count_current_human_approvals` 计入合并门禁 | 403 |
| 私有仓库的 `read` 级协作者，未被请求评审 | 可投 `approve` 并计票 | 403 |
| 被显式请求的评审人（含 CODEOWNERS 指派），无写权限 | 可投 `approve` | 不变（仍可投） |
| 仓库 owner / `write` / `admin` 协作者 / 组织写团队 | 可投 `approve` | 不变 |
| 任何能读仓库的人投 `comment` | 可投 | 不变 |
| PR 作者给自己 approve | 400 | 不变 |

受影响测试清单见 §7.2（以实跑为准，逐条列出）。

## 6. 实现顺序

1. 迁移 + 实体 + `migrations/mod.rs` + `lib.rs` 回滚步数 27 → 28。
2. `rg-db::ops::authorization_ops`（纯 SQL 原语 + 内嵌单测）。
3. `rg-core::authorization`（action 枚举 / payload / 求值器 / 状态机策略 + 内嵌单测）。
4. `reviews.rs` 准入收紧。
5. 新增 `crates/rg-http/tests/pr_review_tests.rs`（验收命令点名的 target 在基线不存在，见 §7.3）。
6. 跑 §7.1 五条命令 + 两后端 SQL 验证。

## 7. 验收

### 7.1 命令

（结果见本文件末尾「执行结果」，逐条粘贴真实输出。）

### 7.2 受影响测试

| 测试 | 原因 | 处理 |
| --- | --- | --- |
| `rg-http/tests/merge_readiness_tests.rs::each_protection_gate_is_reported_and_the_merge_refuses_with_the_same_sentence` | 用"一个普通注册用户"直接 approve，正是被收紧的读权限路径 | 在 approve 之前把该用户加成 `write` 协作者（该用例要验的是门禁计数，不是准入） |
| `rg-http/tests/pat_ref_allowlist_tests.rs::a_branch_limited_token_cannot_approve_a_pull_request` | 同上：reviewer 是只读用户，`request_changes` 也要求写权限 | setup 阶段授予 `write`；用例主体（受限 ref 的 agent token 不得 approve）不变 |
| `rg-db/src/migrations/m20261114_000001_create_usage_counters.rs::down_drops_the_table_and_up_recreates_it` | 与权限无关：它写死 `down(Some(1))`，假设自己是最后一条迁移 | 改为按迁移名从末尾计算步数 |

未受影响（实跑通过）：`pr_permission_tests`（6）、`pr_review_error_semantics_tests`（9）、`agent_principal_tests`（23，含 `AGENT_MAY_NOT_APPROVE` 的顺序）、`actor_ref_tests`（8）。

## 8. 执行结果

### 8.1 五条验收命令（本机 `Darwin arm64`，工作区 `../ironforge-t06`）

```
$ cargo test -p rg-db
test result: ok. 41 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 10.82s   # lib（含 11 项 authorization_ops）
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.48s    # authorization_store（SQLite）
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.53s   # ci_integer_compat
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.54s    # collab_groups_store
test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s    # collab_migration
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.75s   # collab_store
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.10s    # 其它
# `soft_delete_tests::upgrade_keeps_child_rows_triggers_and_foreign_keys`（校验回滚步数 28）在 lib 的 41 项里，ok

$ cargo test -p rg-core
test result: ok. 319 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 10.02s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 2 ignored; 0 measured; finished in 0.00s
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.58s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 5 ignored; 0 measured; finished in 0.11s

$ cargo test -p rg-http --test pr_review_tests
running 5 tests
test the_author_still_cannot_decide_their_own_pr ... ok
test a_refused_approval_does_not_count_towards_the_merge_gate ... ok
test a_reader_may_comment_but_not_approve ... ok
test a_writer_may_approve ... ok
test a_requested_reviewer_may_approve_without_write_access ... ok
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.80s

$ cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 14.78s
（exit 0）

$ scripts/check.sh --quick
==> cargo fmt --all --check
    (1s)
==> cargo clippy --workspace --all-targets -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.34s
    (1s)
==> checks passed in 2s (quick)
```

另跑了一遍 `cargo test -p rg-http`（全量，含 130+ 集成测试二进制）：无 `test result: FAILED`、无 panic。首次全量跑时 `collab_groups_tests::m04_filtering_scans_on_until_the_page_is_full` 在**高负载并行**下失败过一次；单独跑该二进制连续 3 次、整二进制连续 3 次均通过，且该用例走的 `as_of` 分页与本改动无交集，判定为负载相关的既有 flake，不是本次引入。

### 8.2 跨后端（`IRONFORGE_TEST_DATABASE_URL`）

| 后端 | 命令 | 结果 |
| --- | --- | --- |
| SQLite（默认） | `cargo test -p rg-db --test authorization_store` | 5 passed |
| PostgreSQL **17**（`postgres:17` 容器，`postgres://…@127.0.0.1:55432/t06pg`） | 同上 | 5 passed |
| MySQL **8.4**（`mysql:8.4` 容器，`mysql://…@127.0.0.1:53306/t06my`，需 `--log-bin-trust-function-creators=1`，否则迁移里的 FTS 触发器创建被 binlog 拒绝，报 1419） | 同上 | 5 passed |

覆盖点：`client_request_id` 复合唯一键（含"另一主体可用同 key"）、重复票返回 `false` 而不是插入第二行、`consume_guarded` 的 `WHERE state='pending'` 守卫与事务回滚、`head_sha` 扫描只动其它 head 的行、过期只发生一次。

### 8.3 遗留风险

1. **迁移里没有 `authorization_sets` 实体**：`authorization_set_id` 是规则摘要。步骤 2/3 若要"按授权集查询/比对历史版本"，需要提升为表 —— 那时旧行的 `authorization_set_id` 需要回填或保留摘要语义。
2. **`required_approvals` 是冻结值**：申请后改阈值不影响已存在的请求（要新申请）。若产品要求"改阈值立即生效"，需在 `approve`/`consume` 里对比 `authorization_set_id` 并作废旧请求 —— 本步骤不做，因为会与"阈值可预测"冲突。
3. **`deny` 一票即终止**：没有"多数否决"或"否决可撤销"。三套现状里也没有，保持最小。
4. **`release_publish` 的批准人规则是新的**（仓库管理员），现有发布流程没有审批面；本步骤无调用点，接入时必须复核。
5. **`collaborators` 谓词在步骤 1 不被任何 action 置真**，只有单测覆盖；若步骤 2/3 要把它用于 PR 审批，等于重开 N12，plan §4.2 已写死不要这样做。
6. **agent 申请入口仍不存在**：本步骤只交付原语，`requested_by_kind = agent_session` 的分支由单测覆盖（agent 不能 approve），真正的 agent 申请端点在后续步骤。
7. **权限缓存**：`approve` 的成员判定走 `can_admin_repo`/`can_write_repo`（30s 进程缓存），与现有 handler 一致；撤权最多 30 秒后才生效。`collaborators` 分支若启用则绕过缓存。

### 7.3 owned paths 之外的文件（三处，全部是测试）

1. **`crates/rg-http/tests/pr_review_tests.rs`（新建）**：验收命令点名的 target 在基线 `0c870a5` 不存在（`ls` 只有 `pr_review_error_semantics_tests.rs` / `pr_permission_tests.rs`）。覆盖：读权限用户 approve/request_changes 403（机器码 `REVIEW_DECISION_REQUIRES_REVIEWER`）、comment 仍 201、被请求的评审人可批、写权限可批、作者自批仍 400、被拒的票不落库。
2. **`crates/rg-db/tests/authorization_store.rs`（新建）**：新 SQL 的跨后端落点（照 `collab_store.rs` 的 `IRONFORGE_TEST_DATABASE_URL` 模式）。覆盖唯一键、重复票、带守卫的消费、回滚、`head_sha` 扫描与过期。
3. **`crates/rg-db/src/migrations/m20261114_000001_create_usage_counters.rs`（改测试）**：T05 的 `down_drops_the_table_and_up_recreates_it` 写死 `Migrator::down(&db, Some(1))`，假设自己是最后一条迁移；追加 `m20261114_000002` 后回滚 1 步会删掉新表，断言失败。改为按迁移名从末尾计算步数，之后再有迁移追加也不会再断。

另有两处既有 HTTP 测试编码了"读权限即可 approve"，按任务要求改测试、不放宽实现（见 §7.2）。

## 8. 执行结果

（见文末，实现完成后回填。）
