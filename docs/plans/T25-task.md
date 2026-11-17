# T25 执行记录 — 修 `auto_merge_pinned_head_tests` 的 flaky

> 计划见 `docs/plans/T25-plan.md`，权威约束见 `tasks/T25.md`。
> 基线 `e53a98f`，分支 `agent/t25-flaky-auto-merge`，worktree `../ironforge-t25`。

## 1. 开工核对

```
$ git fetch cnb && git rebase cnb/main
Current branch agent/t25-flaky-auto-merge is up to date.
$ git log --oneline -3
e53a98f docs(tasks): add T25 to de-flake the auto-merge pinned-head test
04e4aa5 docs(tasks): mark T24 completed in the mirror index
9fe73fa Merge task T24: agent 判定统一到 is_human_account（#19 R19-5）
```

## 2. 复现（改前，同一台机器）

```
$ BIN=target/debug/deps/auto_merge_pinned_head_tests-fe54bf3e987a2838
# (1) 顺序 20 次
sequential: 20/20 pass

# (2) 6 路并发 × 4 轮 = 24 次
round 1: 6/6  round 2: 5/6  round 3: 6/6  round 4: 6/6

# (3) 8 路并发 × 10 轮 = 80 次
round 1: 6/8   round 2: 8/8   round 3: 6/8   round 4: 4/8   round 5: 6/8
round 6: 7/8   round 7: 7/8   round 8: 8/8   round 9: 8/8   round 10: 5/8
```

合计 **124 次 / 16 次失败**，分布：

```
# 8 路那 80 次的失败用例分布
$ grep -h "FAILED$" $(grep -L 'test result: ok' *.log) | sort | uniq -c
  14 test an_auto_merge_whose_head_moved_waits_for_the_next_evaluation ... FAILED
   1 test the_merge_queue_puts_a_moved_head_back_in_the_queue ... FAILED
# 6 路那 24 次里另有 1 次队列用例失败（同样的 database is locked）
```

目标用例 14 次的失败信息完全一致（第 162 行）：

```
panicked at crates/rg-http/tests/auto_merge_pinned_head_tests.rs:162:5:
assertion `left == right` failed:
  AutoMergeOutcome { status: "merged", reason: None,
                     merge: Some(MergeResult { merge_commit_sha: "68ad29b743f5be722c6bf8a7ea4ef5f7f828013f",
                                               strategy: "squash" }) }
  left: "merged"
 right: "pending"
```

队列用例 2 次是同一后台钩子的另一面：

```
panicked at crates/rg-http/tests/auto_merge_pinned_head_tests.rs:227:10:
queue runs: Internal(db: finish merge-queue entry
Caused by:
    0: Execution Error: error returned from database: (code: 517) database is locked
```

顺序跑 20/20 绿、并发才红 = 与 T24 观察到的「全量红、单独 3/3 绿」一致。

## 3. 根因（A：后台任务，不是 D）

### 3.1 D 先排除

`World::new` 走 `spawn_test_app_on_env_db_with_root` → 未设 `IRONFORGE_TEST_DATABASE_URL`
时是 `spawn_test_app_with_root` → `setup_test_db`（`common/mod.rs:139`）：
**每个 `World` 一个独立临时 SQLite 文件 + 独立 repo 目录**。本次复现全部在默认路径下，
两个测试的库互不可见；失败日志里被合并的也是本测试自己的 PR（`main` 前进了、`state=merged`）。
`list_auto_merge_for_head_commit` 的 `repo_id` 过滤（`head_repository_condition`）经核对是
按 `repo_id` / `head_repo_id` 收紧的，没有漏。

### 3.2 A 的证据

`World::commit` 是 HTTP `POST /contents`；handler 在写完提交后 `announce_branch_move(…)`
（`repo_content.rs:1520`，**没有 `.await`**）→ `spawn_post_push_hooks` →
**`tokio::spawn`**（`lib.rs:3402`）。被 spawn 的钩子对这次 push 做：

1. `sync_pr_head_on_push`（`lib.rs:3551`）：记录的 `head_sha` 从 `reviewed` 改为 `moved`；
2. `try_auto_merges_for_head_commit(…, &new_sha)`（`lib.rs:3579`）：按 `head_sha=moved`
   找到这个开着 auto-merge 的 PR，pin 到 `moved`，**合并**。

临时诊断用例（加在测试文件末尾，跑完即删，未提交）：

```rust
let moved = w.commit("diag_alice", branch, "b.txt").await;
for i in 0..30 {
    let pr = w.pr(number).await;
    let main = w.rev("diag_alice", "refs/heads/main").unwrap();
    println!("t={:>5}ms head_sha={:?} state={} enabled={} main_moved={}",
             i * 100, pr.head_sha, pr.state, pr.auto_merge_enabled, main != main_before);
    tokio::time::sleep(Duration::from_millis(100)).await;
}
```

```
main before push: 7721f00b151930796a3a4a0d4ddfdcdfe1d3c7a7
pushed moved=3cd7d3444bede5df721e180fe2c46e121ced68a3
t=    0ms head_sha=Some("3cd7d344…") state=open   enabled=true  main_moved=false
t=  100ms head_sha=Some("3cd7d344…") state=merged enabled=false main_moved=true
t=  200ms …（此后稳定在 merged）
```

`commit` 返回时 head 已被同步（第 0ms 读到 `moved`），100ms 内 PR 被后台钩子合并。
测试的同步部分（`record_head(reviewed)` + 第一次 `try_auto_merge` + 三条断言）必须全部
跑在这之前，否则它 pin 的 head 已经是 `moved`（`left: "merged"`），或被合并（`state` 不是
`open`）。这就是那个毫秒窗口。

### 3.3 结论：测试问题，不是生产 bug

- 「push 到开着 auto-merge 的 PR 上、规则满足 → 立即合并」是 auto-merge 的正确语义；
- `try_auto_merges_for_head_commit` 的 repo 过滤没有问题；
- 错的是测试：它用 HTTP 提交来「只移动分支」，然后手工把记录写回旧值——两个写者在抢同一行。
  只要后台钩子跑得够快，测试想要的状态就不存在。

## 4. 改动

只改 `crates/rg-http/tests/auto_merge_pinned_head_tests.rs`（+60 / −23）。

| 改动 | 说明 |
| --- | --- |
| `World::commit` → `World::commit_without_push_hooks` | 改调 `rg_core::repo::service::create_or_update_file_with`（HTTP handler 在 `announce_branch_move` 之前调的就是它），分支真的移动，但不 spawn 后置钩子 |
| 两个测试的 4 处 `w.commit(…)` | 换新 helper；目标用例第 2 段加注释说明「这条路径不会有 push 钩子改写记录」 |
| 模块头注释 | 补一段：分支移动也绕开 HTTP contents API 的原因 |

helper 实现：

```rust
async fn commit_without_push_hooks(&self, owner: &str, branch: &str, path: &str) -> String {
    let repo = self.repo().await;
    rg_core::repo::service::create_or_update_file_with(
        &self.db, repo.id, owner, "work", path, "x\n", &format!("add {path}"),
        branch, None, owner, &format!("{owner}@example.com"), &self.root,
        Some(self.alice_id),
        rg_core::repo::service::FileCommitOptions { expected_tip: None, dry_run: false },
    )
    .await
    .unwrap_or_else(|error| panic!("commit {path}: {error:?}"));
    self.rev(owner, &format!("refs/heads/{branch}"))
        .expect("branch tip after commit")
}
```

修后两条用例的时序（无任何后台写者）：

```
分支 = reviewed，记录 = reviewed，auto-merge 开
commit b.txt（无钩子）→ 分支 = moved，记录 = reviewed
record_head(reviewed)  → 记录 = reviewed（显式）
try_auto_merge         → pin reviewed / 实际 moved → HeadMoved → pending  ✔
record_head(moved)     → 记录 = moved
try_auto_merge         → pin moved → merged                              ✔
```

**没有重试、没有 sleep、没有轮询、没有放宽断言**，断言一行未改。

### 为什么不选「显式等待评估完成」

等待的确定结果就是 PR 被合并（§3.2 的诊断输出），会把测试要验证的状态等没。
具体取舍见 `T25-plan.md` §4。

### 顺手修掉队列用例

`the_merge_queue_puts_a_moved_head_back_in_the_queue` 用的是同一个 `World::commit`，
同时也在跟后台钩子的 `merge_queue::process_for_head_commit_with_ci` 抢（复现中它的
`database is locked` 就是后台钩子占着 SQLite 写事务）。换同一个 helper 后一并消失。
这不是扩大范围：同一处 helper、同一处竞争。

## 5. 验证（修后，实际运行）

### 5.1 顺序 30 次

```
$ BIN=target/debug/deps/auto_merge_pinned_head_tests-fe54bf3e987a2838
$ for i in $(seq 1 30); do "$BIN"; done
sequential: 30/30 pass
```

### 5.2 单用例 30 次（`--test-threads=8`）

```
$ "$BIN" an_auto_merge_whose_head_moved_waits_for_the_next_evaluation --test-threads=8   # ×30
single test: 30/30 pass
```

### 5.3 8 路并发 × 10 轮 = 80 次（改前 15/80 失败）

```
$ for round in $(seq 1 10); do
>   for j in $(seq 1 8); do "$BIN" --test-threads=8 & done
>   wait
> done
round 1: 8/8   round 2: 8/8   round 3: 8/8   round 4: 8/8   round 5: 8/8
round 6: 8/8   round 7: 8/8   round 8: 8/8   round 9: 8/8   round 10: 8/8
--- failures:
（空）
```

改前同一负载：15/80 失败（目标用例 14 次）。改后：0/80。

### 5.4 `cargo test -p rg-http`（全量）

```
$ cargo test -p rg-http
…（134 个测试二进制）
test result: ok  ×134
passed=867  failed=0
```

### 5.5 `cargo clippy --workspace --all-targets -- -D warnings`

```
    Checking rg-db v0.2.0 / rg-core v0.2.0 / rg-ssh v0.2.0 / rg-ci v0.2.0 / rg-cli v0.2.0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 47.17s
```

无警告。

### 5.6 `scripts/check.sh --quick`

```
==> cargo fmt --all --check
    (1s)
==> cargo clippy --workspace --all-targets -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.42s
    (1s)
==> checks passed in 2s (quick)
```

（首次跑 fmt 报了一处行尾空行，`cargo fmt --all` 修掉后复跑通过。）

## 6. 同型隐患清单（只列，未改）

| 位置 | 隐患 | 现状 |
| --- | --- | --- |
| `auto_merge_pinned_head_tests.rs` 两条用例 | 都用 HTTP commit 移动分支，再手工改 `head_sha` | **已修**（同一 helper） |
| `merge_pinned_head_tests.rs:72` | 也用 HTTP `/contents` 移动分支，但合并走 API 且显式传 `expected_head_sha`，不读记录的 `head_sha` | 核对过：不受影响（文件内没有任何 recorded-head 断言） |
| `merge_readiness_tests.rs`、`pr_context_tests.rs`、`pr_export_tests.rs`、`file_commit_semantics_tests.rs`、`update_branch_tests.rs`、`agent_principal_tests.rs` | 先 HTTP 提交再读 PR 状态 | 均未开 auto-merge / 未入队，后台钩子对它们只做 head 同步；本次回归与并发压测未观察到失败。**未来若要断言 recorded head，必须注意同一窗口** |
| 通用 | `POST /contents`（`repo_content.rs:1520`）与 `DELETE /contents`（`repo_content.rs:1654`）都走 `announce_branch_move` → `tokio::spawn` | 生产行为正确（响应不阻塞在钩子上），但任何「提交后立即断言 PR 派生状态」的测试都要么绕开 HTTP、要么等钩子。T07 遗留风险 §9.1 记录的正是这一点 |

## 7. 未做 / 遗留

- 没有改生产代码（判定为测试问题；push 后立即 auto-merge 是正确语义）。
- 没有给测试加重试 / sleep / 宽断言 / `#[ignore]`。
- 未顺手修改 §6 里其它文件的写法（超出本任务范围，只记录）。
- 未在 CI 验证（本仓库当前没有可用 CI 环境），以上均为本机结果。
