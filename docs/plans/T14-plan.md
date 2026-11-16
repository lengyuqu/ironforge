# T14 执行计划 — [#15 步骤2] ops 签名批量放宽：`org_ops` + `user_ops`

> 权威约束见 `tasks/T14.md`。基线 `bf7d9f0`，分支 `agent/t14-conn-trait-ops`，worktree `../ironforge-t14`。
> 验收命令真实输出、触碰路径与函数级清单见 `docs/plans/T14-task.md`。

## 1. 交付边界

做：

1. `crates/rg-db/src/ops/org_ops.rs`：把可放宽的签名统一改成 `pub async fn f<C: ConnectionTrait>(db: &C, ..)`。
2. `crates/rg-db/src/ops/user_ops.rs`：同上，26 个函数全部放宽。
3. 3 个跨 crate 调用点显式解引用（见 §4）：`rg-ssh` 两处、`rg-http` 一处。

不做（明确排除）：`pipeline_ops.rs`（38 处里 7 处 `db.begin()`，需逐函数判断，留下一轮）、其余 ops 文件、
`Cargo.*`、迁移、`rg-core` 业务源码、`CLAUDE.md`、`ironforge-docs/`、`.cnb.yml`、`.github/`、`scripts/`、
`deploy/`、`crates/rg-mcp/`。

改动只落在 **4 个文件**（见 §5）。

## 2. 最终数字

`grep -c '&DatabaseConnection'`：

| 文件 | 改前 | 改后 | 说明 |
| --- | --- | --- | --- |
| `org_ops.rs` | 25 | 2 | 23 个函数放宽；2 个保留（§3） |
| `user_ops.rs` | 26 | 0 | 26 个函数全部放宽 |
| 合计 | 51 | 2 | 51 − 49 = 2 |

`org_ops.rs` 的 `db: &C` 计数是 24 而不是 23：`add_org_member` 在基线 `bf7d9f0` 就已是
`<C: ConnectionTrait>`（T13 之前就为 `create_org` 的事务路径服务），本次未动它。所以是
「25 处 `&DatabaseConnection` = 23 处放宽 + 2 处保留」，而文件里泛型函数共 24 个。

## 3. 保留清单（函数级，2 个）

两个函数都在 `org_ops.rs`，保留原因相同：**必须 `DatabaseConnection` 专有的 `begin()`**。

| 函数 | 当前签名 | 保留原因 |
| --- | --- | --- |
| `create_org`（l.14） | `pub async fn create_org(db: &DatabaseConnection, name: &str, display_name: Option<&str>, description: Option<&str>, owner_id: i64, visibility: &str) -> Result<organization::Model>` | l.35 `db.begin()`：org 行 + owner 会员行必须同事务 |
| `set_team_repositories`（l.440） | `pub async fn set_team_repositories(db: &DatabaseConnection, team_id: i64, repo_ids: &[i64]) -> Result<()>` | l.447 `db.begin()`：先 `delete_many` 再逐条插入，必须同事务 |

`user_ops.rs` **没有保留项**（0 个函数保留）。该文件 26 个函数全部放宽，因为文件内不存在
`db.begin()` / `db.transaction()` / `db.clone()` / `db.ping()`；`record_failed_login`、`update_agent`
虽然内部调 `find_by_id`，但两者都是「被放宽的一方」（`find_by_id` 也放宽了），方向一致，无需保留。

没有函数因「内部调用了保留函数」而被迫保留：`create_org` 内部只调 `add_org_member`（本来就是泛型，传
`&txn` 即可），`set_team_repositories` 内部只用 `&txn` 与实体 API，二者都不向外扩散约束。

## 4. 连锁处理过程

### 4.1 rg-db 内部：零连锁

一次性对两个文件做机械替换（`db: &DatabaseConnection` → `db: &C`，并在函数名后插入 `<C: ConnectionTrait>`），
随后 `cargo check -p rg-db` **第一轮即通过**：

```
$ cargo check -p rg-db
    Checking rg-db v0.2.0 (/Users/yuqu/vibeCodeing/ironforge-t14/crates/rg-db)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 27.69s
```

原因：`user_ops` 的内部调用方向都指向「被放宽的函数」——`create_user` / `create_ldap_user` / `create_agent`
调 `create`，`sync_ldap_identity` 调 `update`，`record_failed_login` / `update_agent` 调 `find_by_id`，
四者同时放宽后 `&C` 一路传得通。`rg-db` 没有出现「A 放宽后传给仍是 `&DatabaseConnection` 的 B」的情况。

`db.get_database_backend()`（`is_member_of_write_team_for_repo`、`is_member_of_admin_team`）是
`ConnectionTrait` 的方法，不阻碍放宽；`query_one(Statement::from_sql_and_values(..))` 同样来自
`ConnectionTrait`。SQL 文本、绑定参数、`prepare_sql` 调用一字未动。

### 4.2 跨 crate：3 个调用点（已获协调者批准后修改）

`cargo check -p rg-core --all-targets` 干净，但 `cargo check --workspace --all-targets` 红，
9 个 error 全部来自 **3 个调用点**，同一个根因：**泛型 `&C` 不参与 deref coercion**。

| 位置 | 改前实参 | 改后 | 实参原类型 |
| --- | --- | --- | --- |
| `crates/rg-ssh/src/lib.rs:363` | `find_by_username(db, ..)` | `db.as_ref()` | `&Arc<DatabaseConnection>` |
| `crates/rg-ssh/src/lib.rs:646` | `find_by_id(&db, ..)` | `db.as_ref()` | `&Arc<DatabaseConnection>`（`db: Arc<..>`） |
| `crates/rg-http/src/lib.rs:3678` | `find_by_id(db, ..)` | `*db` | `&&DatabaseConnection`（`PostPushParams{db}` 解构自 `&PostPushParams`） |

语义零变化：三处都是只读查询调用点，`as_ref()` / `*db` 只把「泛型推断失败的双层引用」还原成
`&DatabaseConnection`。`rg-core`、`rg-ci`、`rg-db` 的所有调用点都直接传 `&DatabaseConnection`，
`C = DatabaseConnection` 一次推断成功，无需改动。

被否决的替代方案（「不碰调用方」）：把 `find_by_id` / `find_by_username` 退回 `&DatabaseConnection`。
它会连锁——`record_failed_login`、`update_agent` 内部调 `find_by_id`，也只能一起退回——结果是 4 个函数保留，
而 `record_failed_login`（失败登录计数）与 `update_agent`（agent 描述/启用状态）正是最需要「与业务同事务」
的写路径，与 #15 方向相反。因此选择改 3 个 token。

### 4.3 `cargo fmt` 重排

插入 `<C: ConnectionTrait>` 后有几个单行签名超出 100 列，`cargo fmt --all --check`（`scripts/check.sh --quick`
第一步）会报 3 个文件的 diff。执行 `cargo fmt --all` 重排为多行签名，**只影响空白**，已验证：

```
$ python3 (归一化：去掉 <C: ConnectionTrait>、db: &C → db: &DatabaseConnection、去尾随逗号、去所有空白)
crates/rg-db/src/ops/org_ops.rs token-equal: True 11667 11667
crates/rg-db/src/ops/user_ops.rs token-equal: True 14419 14419
```

即两个文件的 token 流与基线**逐字符相同**（唯一差异就是签名形态），函数体零改动。

## 5. 改动范围核实

```
$ git diff --stat bf7d9f0 -- crates/
 crates/rg-db/src/ops/org_ops.rs  | 84 +++++++++++++++++++++++-----------------
 crates/rg-db/src/ops/user_ops.rs | 76 +++++++++++++++++++-----------------
 crates/rg-http/src/lib.rs        |  2 +-
 crates/rg-ssh/src/lib.rs         |  5 ++-
 4 files changed, 92 insertions(+), 75 deletions(-)
```

（含 `docs/plans/T14-plan.md`、`docs/plans/T14-task.md`、`tasks/T14.md` 的完整提交是 7 文件 /
411 insertions / 76 deletions。）

`org_ops` / `user_ops` 的 `-U0` diff 只有签名行进出，`rg-ssh` / `rg-http` 各只有 1 处调用点
（`5 ++-` 是 fmt 把 `db.as_ref(), user_id,` 折成两行）。没有内容被截断或删除——开工时一次脚本事故
（`open(p,"w")` 先于读取求值）曾把两个 owned 文件清空，已 `git checkout --` 恢复并重新核对基线计数
（25 / 26）后重做。

## 6. 两条教训（供后续批量放宽复用）

1. **任务书的统计断言不可信，以编译器为准。** `tasks/T14.md` 与派单都写「两个文件都没有 `db.begin()`」，
   实际 `org_ops.rs` 有 2 处（`create_org`、`set_team_repositories`）。统计脚本只 grep 了
   `db.begin()` 这种单行写法，而仓库里的调用是 `db` 换行 `.begin()`，跨行模式漏掉了。教训：判断
   「是否必须 `DatabaseConnection`」不能靠 grep 单行文本，要靠 `cargo check` 的 `E0277`/`E0599` 反馈，
   并顺带检查跨行链式调用。
2. **泛型 `&C` 不参与 deref coercion，这是签名泛化的固有代价。** 接受 `&DatabaseConnection` 时，
   `&Arc<DatabaseConnection>`（`Deref`）和 `&&DatabaseConnection`（引用折叠）都能自动适配；换成 `&C` 后
   编译器先把 `C` 推断成实参类型（`Arc<..>` / `&DatabaseConnection`），再检查 `ConnectionTrait` 约束，于是
   直接失败。后续每个批量放宽的批次都会遇到这类调用点，处理方式是**在调用点显式解引用**
   （`db.as_ref()` / `*db`），而不是给被调方留特例签名。

## 7. 遗留风险

1. `pipeline_ops.rs` 未处理（38 处 `&DatabaseConnection`、7 处 `db.begin()`），仍需逐函数判断。
2. 本次只放宽签名，**没有**把任何现有写路径改成事务内执行；「能进事务」是能力，落地要等后续任务
   真正把 `&txn` 传进去。
3. `create_org` / `set_team_repositories` 仍只能接 `&DatabaseConnection`：如果将来需要「在外部事务里建
   org」，必须改成接 `C: ConnectionTrait + TransactionTrait`（`TransactionTrait::begin` 是泛型可用的），
   那是语义扩展，不在本轮「纯机械放宽」范围内。
4. 3 个调用点的显式解引用是「为了让泛型签名成立」的最小改动；若将来把 `PostPushParams.db` 改为
   `&DatabaseConnection`（而不是 `&&`），`*db` 需要还原成 `db`——类型变化会由编译器提示。
