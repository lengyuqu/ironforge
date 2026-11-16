# T14 Task 确认 — [#15 步骤2] ops 签名批量放宽：`org_ops` + `user_ops`

> 权威状态在 `tasks/T14.md`；本文件记录约束确认、触碰路径、函数级清单、验收命令真实输出与遗留风险。
> 放宽方式、连锁处理过程与两条教训见 `docs/plans/T14-plan.md`。

## 1. 约束确认

| 约束 | 结论 |
| --- | --- |
| 只做 `org_ops.rs` + `user_ops.rs`，不做 `pipeline_ops.rs` | 是；`pipeline_ops.rs` 未触碰 |
| 参数名保留 `db`，只改类型 + 加泛型 | 是；51 处参数签名全部只改了类型，参数名仍为 `db` |
| 泛型名统一 `C`，用 `sea_orm::ConnectionTrait` | 是；两文件均为 `use sea_orm::*;`，未新增 use |
| 编译驱动，不盲改 | 是；`cargo check -p rg-db` 第一轮即绿（内部零连锁），workspace 一轮红→绿（见 §4） |
| 不许 `where Self: Sized` / trait 对象 / 为过编译动逻辑 | 是；无任何此类构造，函数体零改动（§3.3 token 比对） |
| 不改 SQL 条件、排序、错误信息、返回值 | 是；token 流与基线逐字符一致 |
| 调用方原则上不需要改 | **有 3 处例外，已获协调者批准**（`rg-ssh`×2、`rg-http`×1，见 §4），改动均为单个 token |
| 需要 `DatabaseConnection` 专有方法的函数保留原签名并列入清单 | 是；2 个函数，均为 `db.begin()` |
| 不改其它文件（含 `pipeline_ops.rs`、`Cargo.*`、迁移、`rg-core` 源码、`CLAUDE.md`、`ironforge-docs/`、`.cnb.yml`、`.github/`、`scripts/`、`deploy/`、`rg-mcp/`） | 是；`rg-core` 源码未改，`Cargo.*`/迁移/脚本/文档目录均未触碰 |
| 不跑 `cargo test -p rg-http` 全量 | 是（本任务不碰 HTTP 业务逻辑，只改了 `rg-http/src/lib.rs` 一行调用点，用 `cargo check` / `clippy` 覆盖） |

## 2. 触碰路径

`crates/rg-db`

- `src/ops/org_ops.rs`：23 个函数放宽为 `<C: ConnectionTrait>(db: &C, ..)`；`create_org`、
  `set_team_repositories` 保留 `&DatabaseConnection`（`db.begin()`）。
- `src/ops/user_ops.rs`：26 个函数全部放宽；0 个保留。

`crates/rg-ssh`

- `src/lib.rs:363`：`find_by_username(db, ..)` → `find_by_username(db.as_ref(), ..)`。
- `src/lib.rs:646`：`find_by_id(&db, ..)` → `find_by_id(db.as_ref(), ..)`。

`crates/rg-http`

- `src/lib.rs:3678`：`find_by_id(db, ..)` → `find_by_id(*db, ..)`。

文档：`docs/plans/T14-plan.md`、`docs/plans/T14-task.md`、`tasks/T14.md`（State → completed）。

代码改动共 **4 个文件**，`git diff --stat bf7d9f0` 见 §5。## 3. 函数级清单

### 3.1 保留（2 个，全在 `org_ops.rs`）

| 函数 | 当前签名 | 原因 |
| --- | --- | --- |
| `create_org` | `pub async fn create_org(db: &DatabaseConnection, name: &str, display_name: Option<&str>, description: Option<&str>, owner_id: i64, visibility: &str) -> Result<organization::Model>` | `db.begin()`（l.35）：org 行 + owner 会员行同事务 |
| `set_team_repositories` | `pub async fn set_team_repositories(db: &DatabaseConnection, team_id: i64, repo_ids: &[i64]) -> Result<()>` | `db.begin()`（l.447）：`delete_many` + 逐条插入同事务 |

无「因内部调用保留函数而保留」的项。

### 3.2 放宽（49 个）

`org_ops.rs`（23 个新放宽；`add_org_member` 基线即泛型，未动）：

`get_org`、`get_org_by_name`、`list_all_orgs`、`list_user_orgs`、`update_org`、`delete_org`、
`remove_org_member`、`list_org_members`、`is_org_member`、`find_org_member`、`create_team`、`get_team`、
`find_team_by_name`、`list_org_teams`、`delete_team`、`add_team_member`、`remove_team_member`、
`list_team_members`、`is_team_member`、`list_user_teams`、`is_member_of_write_team_for_repo`、
`list_team_repositories`、`is_member_of_admin_team`

`user_ops.rs`（26 个，全部）：

`find_by_username`、`find_by_email`、`find_by_id`、`count_by_ldap_provider`、`list_users`、`create`、
`update`、`update_by_id`、`set_auth_provider`、`promote_if_first_user`、`create_user`、`create_ldap_user`、
`sync_ldap_identity`、`update_totp_secret`、`enable_mfa`、`disable_mfa`、`record_successful_login`、
`reset_login_failures`、`record_failed_login`、`delete_by_id`、`bump_token_version`、`create_agent`、
`list_agents_by_owner`、`count_agents_by_owner`、`update_agent`、`find_by_ids`

（23 + 26 = 49 个函数放宽；49 + 2 保留 = 51 处 `&DatabaseConnection`。）

### 3.3 语义零变化的证据

把两个文件按同一规则归一化（删 `<C: ConnectionTrait>`、`db: &C` → `db: &DatabaseConnection`、
去尾随逗号、去所有空白）后与基线 `bf7d9f0` 逐字符比较：

```
crates/rg-db/src/ops/org_ops.rs token-equal: True 11667 11667
crates/rg-db/src/ops/user_ops.rs token-equal: True 14419 14419
```

## 4. 编译迭代记录

| 轮次 | 命令 | 结果 |
| --- | --- | --- |
| 1 | `cargo check -p rg-db` | 绿（27.69s）。rg-db 内部零连锁，无需二次放宽 |
| 2 | `cargo check -p rg-core --all-targets` | 绿（32.46s）。rg-core 所有调用点直接推断 `C = DatabaseConnection` |
| 3 | `cargo check --workspace --all-targets` | **红**：9 个 `E0277`，全部来自 3 个调用点（`rg-ssh` 2 处 / `rg-http` 1 处），根因是泛型 `&C` 不参与 deref coercion（`&Arc<DatabaseConnection>`、`&&DatabaseConnection`） |
| 4 | 改 3 处调用点后 `cargo check --workspace --all-targets` | 绿（11.33s） |
| 5 | `cargo fmt --all` 后重跑全部验收 | 全绿（fmt 只重排签名行，见 §3.3） |

**`cargo check` 从红到绿的迭代次数：1 轮**（第 3 轮红 → 第 4 轮绿）；rg-db 内部放宽过程本身零迭代。

## 5. 改动范围

```
$ git diff --stat bf7d9f0 -- crates/
 crates/rg-db/src/ops/org_ops.rs  | 84 +++++++++++++++++++++++-----------------
 crates/rg-db/src/ops/user_ops.rs | 76 +++++++++++++++++++-----------------
 crates/rg-http/src/lib.rs        |  2 +-
 crates/rg-ssh/src/lib.rs         |  5 ++-
 4 files changed, 92 insertions(+), 75 deletions(-)
```

含 3 个文档文件（`docs/plans/T14-plan.md`、`docs/plans/T14-task.md`、`tasks/T14.md`）的完整提交：
7 files changed, 411 insertions(+), 76 deletions(-)。`rg-ssh` 计 5 行是因为 `cargo fmt` 把
`db.as_ref(), user_id,` 折成两行（纯空白）。

## 6. 验收命令真实输出

### 6.1 `grep -c '&DatabaseConnection'`

```
$ grep -c '&DatabaseConnection' crates/rg-db/src/ops/org_ops.rs crates/rg-db/src/ops/user_ops.rs
crates/rg-db/src/ops/org_ops.rs:2
crates/rg-db/src/ops/user_ops.rs:0
```

改前：`org_ops.rs` = 25、`user_ops.rs` = 26（合计 51）。改后：2 / 0（合计 2）。

### 6.2 `cargo test -p rg-db`

```
test result: ok. 59 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 12.43s
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.47s
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.38s
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.53s
test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.73s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s
```

### 6.3 `cargo test -p rg-core`

```
test result: ok. 377 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 13.00s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.71s
test result: ok. 0 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.99s
test result: ok. 1 passed; 0 failed; 5 ignored; 0 measured; 0 filtered out; finished in 0.07s
```

### 6.4 `cargo check -p rg-ssh --all-targets`（额外，因为改了 rg-ssh）

```
    Checking rg-ssh v0.2.0 (/Users/yuqu/vibeCodeing/ironforge-t14/crates/rg-ssh)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.79s
```

### 6.5 `cargo clippy --workspace --all-targets -- -D warnings`

```
    Compiling rg-http v0.2.0 (/Users/yuqu/vibeCodeing/ironforge-t14/crates/rg-http)
    Checking rg-ssh v0.2.0 (/Users/yuqu/vibeCodeing/ironforge-t14/crates/rg-ssh)
    Checking rg-ci v0.2.0 (/Users/yuqu/vibeCodeing/ironforge-t14/crates/rg-ci)
    Checking rg-cli v0.2.0 (/Users/yuqu/vibeCodeing/ironforge-t14/crates/rg-cli)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 34.93s
```

### 6.6 `scripts/check.sh --quick`

```
==> cargo clippy --workspace --all-targets -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 12.38s
    (12s)

==> checks passed in 14s (quick)
```

（`cargo fmt --all --check` 步骤无 diff 输出，即通过。）

## 7. 遗留风险

1. `pipeline_ops.rs` 仍有 38 处 `&DatabaseConnection`（其中 7 处 `db.begin()` 必须保留），留下一轮逐函数判断。
2. 本轮只放宽签名，没有把任何现有写路径搬进事务；「能进事务」是能力，落地要等后续任务真正传 `&txn`。
3. `create_org` / `set_team_repositories` 若要支持外部事务，需要改成
   `C: ConnectionTrait + TransactionTrait`，属于语义扩展，不在本轮范围。
4. 3 个调用点的解引用（`db.as_ref()` / `*db`）依赖实参当前的双层引用形状；若 `PostPushParams.db`
   改成 `&DatabaseConnection`，`*db` 需还原为 `db`（编译器会提示）。
