# T25 执行计划 — 修 `auto_merge_pinned_head_tests` 的 flaky（确定性等待，不是重试）

> 权威约束见 `tasks/T25.md`。基线 `e53a98f`，分支 `agent/t25-flaky-auto-merge`，
> worktree `../ironforge-t25`。实际执行与验收输出见 `docs/plans/T25-task.md`。

## 1. 现象与目标

`crates/rg-http/tests/auto_merge_pinned_head_tests.rs:138`
`an_auto_merge_whose_head_moved_waits_for_the_next_evaluation` 是 flaky：T24 全量跑红过一次，
单独跑 3/3 绿，第二次全量通过。

目标：让它在**任何负载下都确定**。禁止重试、固定 sleep、放宽断言、`#[ignore]`。

## 2. 复现（改前）

先用已编好的测试二进制跑，两种负载：

| 负载 | 次数 | 失败 | 失败的用例 |
| --- | --- | --- | --- |
| 顺序、`--test-threads=2` | 20 | 0 | — |
| 6 路并发 × 4 轮 | 24 | 1 | 队列用例：`database is locked`（code 517） |
| 8 路并发 × 10 轮 | 80 | 15 | 14 × 目标用例（第 162 行），1 × 队列用例（`database is locked`） |
| 合计 | 124 | 16 | 目标用例 14 次，队列用例 2 次 |

即：**顺序跑复现不了，同机 8 路并发时目标用例失败率约 17.5%**。这正是它只在
`cargo test -p rg-http`（多个测试二进制、机器满载）里偶发、单独跑却总是绿的原因。

目标用例 14 次失败的断言完全相同，都是第 162 行 `assert_eq!(outcome.status, "pending")`：

```
assertion `left == right` failed:
  AutoMergeOutcome { status: "merged", reason: None,
                     merge: Some(MergeResult { merge_commit_sha: "68ad29b…", strategy: "squash" }) }
  left: "merged"
 right: "pending"
```

注意失败形态：不是「合并后的状态不对」，而是**测试自己那次 `try_auto_merge` 就返回了
`merged`**——说明它读到并用于 pin 的 `head_sha` 已经变成了 `b.txt` 的提交。

队列用例那 2 次是同一原因的另一面：后台钩子占着 SQLite 写事务，测试自己的写拿到
`database is locked`（code 517）。

## 3. 根因：A（后台任务），不是 D

先排除 D（跨测试按 `head_sha` 互相干扰）：`World::new` → `spawn_test_app_with_root` →
`setup_test_db`，**每个 `World` 一个独立的临时 SQLite 文件 + 独立 repo 目录**
（`crates/rg-http/tests/common/mod.rs:139-155`），只有设置 `IRONFORGE_TEST_DATABASE_URL`
时才共享库。本次复现全部在默认 SQLite 下，测试之间没有共享任何东西；而且失败日志里被合并的
是**本测试自己的 PR**（`main` 真的前进了，`state` 变成 `merged`），不是别人的。

真正的原因在 `World::commit`：它走 HTTP `POST /repos/{owner}/work/contents/{path}`，而
`create_or_update_file` 在写完提交后调用 `announce_branch_move` →
`spawn_post_push_hooks`，其中是 **`tokio::spawn`**（`crates/rg-http/src/lib.rs:3402`），
**不等待**：

```rust
announce_branch_move(&state, …);          // repo_content.rs:1520，没有 .await
…
tokio::spawn(async move { post_push_hooks(…).await });   // lib.rs:3402
```

被 spawn 的 `post_push_hooks` 对这次 push 依次做：

1. `sync_pr_head_on_push(db, repo_id, branch, new_sha)`（`lib.rs:3553`）——
   把已记录的 `head_sha` 从 `reviewed` 改成 `moved`；
2. `try_auto_merges_for_head_commit(db, repo_root, repo_id, &new_sha)`（`lib.rs:3579`）——
   按 `head_sha = moved` 找到这个开着 auto-merge 的 PR，pin 到 `moved`，**合并成功**。

也就是说：**在生产语义下，「往开着 auto-merge 的 PR 上推一个可合并的提交」本来就会立刻合并。**
测试却在 `w.commit("b.txt")` 之后手工把 `head_sha` 写回 `reviewed`，指望「记录落后于分支」。
这两个动作在抢同一个字段，测试只在跑赢后台钩子时才能观察到它想观察的状态。

直接观测（临时诊断用例，跑完即删；`shell` 里 100ms 轮询 PR 行）：

```
main before push: 7721f00b…
pushed moved=3cd7d344…
t=    0ms head_sha=Some("3cd7d344…") state=open   enabled=true  main_moved=false
t=  100ms head_sha=Some("3cd7d344…") state=merged enabled=false main_moved=true
```

`commit` 返回时钩子可能已经同步完 head，随后的 100ms 内 PR 被后台钩子合并。测试同步做的
`record_head(reviewed)`、`try_auto_merge` 和三条断言必须全部在这之前完成——**这就是那个
毫秒窗口**，负载越高越容易输。

结论：**这是测试问题，不是生产 bug**：
- 生产侧「push 后 auto-merge 立即评估并合并」是正确行为（auto-merge 的全部意义），
  不该为了让测试好写而改；
- `try_auto_merges_for_head_commit` 的 `repo_id` 过滤也没有问题
  （`list_auto_merge_for_head_commit` 用 `head_repository_condition(source_repo_id)`，
  同仓 PR 按 `repo_id`、fork PR 按 `head_repo_id` 过滤）；
- 测试的构造方式（用 HTTP 提交来「只移动分支」）本身就不成立。

## 4. 修法：让分支移动不经过 push 钩子

`World::commit` → `World::commit_without_push_hooks`，直接调**同一个** service：

```rust
rg_core::repo::service::create_or_update_file_with(
    &self.db, repo.id, owner, "work", path, "x\n", &format!("add {path}"),
    branch, None, owner, &format!("{owner}@example.com"), &self.root,
    Some(self.alice_id),
    rg_core::repo::service::FileCommitOptions { expected_tip: None, dry_run: false },
)
```

这与 HTTP handler 在调用 `announce_branch_move` 之前做的完全一样——同样的文件校验、
段保护、提交构造——只是**不 spawn 后置钩子**。分支是真的移动了，记录的 head 留旧值，
之后没有任何后台任务会再写这一行。测试的两个阶段因此变成纯同步：

| 阶段 | 状态 |
| --- | --- |
| `commit a.txt` → `open_pr` → `enable_auto_merge` | 记录 = `reviewed` |
| `commit b.txt`（无钩子） | 分支 = `moved`，记录仍是 `reviewed`（`record_head` 再显式写一次） |
| `try_auto_merge` | pin 到 `reviewed`，实际分支在 `moved` → `HeadMoved` → `pending` |
| `record_head(moved)` → `try_auto_merge` | pin 到 `moved` → `merged` |

同文件的 `the_merge_queue_puts_a_moved_head_back_in_the_queue` 用的是同一个 helper
（`w.commit`），同样在跟后台钩子的 `merge_queue::process_for_head_commit_with_ci` 抢，
所以一起修掉——这不是「顺手改」，是同一处 helper 的同一处竞争。

### 为什么不是「显式等待评估完成」

任务书给的 A/B 修法是「等这个 PR 的 auto-merge 评估完成」，在**这里不成立**：
等它完成的确定结果就是**PR 被合并**（见 §3 的诊断输出），测试的前提（记录落后、PR 仍 open）
被这个等待本身消灭。要等，就只能等一个把测试目标毁掉的结果。

### 为什么不选重试 / sleep / 放宽断言

- 重试和 sleep 都只是把窗口挪到「通常够长」，负载、机器、SQLite 的 fsync 抖动换了就复发。
  本任务的全部意义在于这条用例只在满载时红。
- 放宽断言（例如允许 `merged`）等于删掉这条用例要验证的东西（moved head 不合并）。
- `#[ignore]` 直接把回归保护的缺口留在那里。

新写法里**没有任何时间假设**：不轮询、不等待、不重试，也没有后台任务需要等。

## 5. 验证设计

1. 改前/改后用**同样的负载**对比：8 路并发 × 10 轮（改前 15/80 失败，其中目标用例 14 次）。
2. 修后：顺序 30 次、单用例 30 次、8 路并发（`--test-threads=8`）× 10 轮 = 80 次，全绿。
3. `cargo test -p rg-http` 全量、`cargo clippy --workspace --all-targets -- -D warnings`、
   `scripts/check.sh --quick`。
4. 顺带核查同文件/同型用例的隐患（只列，不改）。

## 6. 不做

- 不改生产代码（本任务判定为测试问题，见 §3 结论）。
- 不给测试加重试 / sleep / 宽断言 / `#[ignore]`。
- 不改 `Cargo.*`、迁移、`crates/rg-core/src/{review,authorization}/`、`branch_protection/`、
  `CLAUDE.md`、`ironforge-docs/`、`.cnb.yml`、`.github/`、`scripts/`、`deploy/`、`rg-mcp/`。
- 不顺手改其它文件里同型（但当前未观察到失败）的写法，只记入报告。
