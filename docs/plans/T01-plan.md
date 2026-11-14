# T01 执行计划 — 合并目标必须是 `pr.base_branch`

> 任务：`tasks/T01.md`（issue #60）。基线 `5509c4c`（分支 `agent/t01-merge-target`）。

## 1. 缺陷定位

`crates/rg-core/src/pull_request/service.rs`：

- `gix_merge_no_ff`（:1880）与 `gix_squash_merge`（:1911）用 `repo.rev_parse_single("HEAD")` 取 ours；
- 两者都通过 `commit_as_server(..., "HEAD", ...)` 建提交，即把结果写回 **HEAD 指向的分支**；
- 调用方 `do_merge_commit`（:1688）/ `do_squash_merge`（:1693）只传 `pr.head_branch`，没有传 `pr.base_branch`；
- `merge_from_ref`（:1580，fork PR 与 pinned-head 路径）同样把 ours 交给上述两个函数；
- 对照 `do_rebase_merge`（:1701）用 `rg_git::ops::rebase_merge(repo_path, &pr.base_branch, &pr.head_branch)` —— 正确实现，作为参照。

因此：base 不是默认分支的 PR，`merge`/`squash` 会把提交写到裸仓 `HEAD` 指向的默认分支，`pr.base_branch` 原地不动。

## 2. 步骤

1. 新建 `crates/rg-http/tests/merge_target_tests.rs`：
   - 初始化裸仓：`main`（默认分支、HEAD）、`release`（PR 的 base）、`feature`（PR 的 head，从 `main` 分出）；
   - 开 PR `base=release head=feature`；
   - 分别用 `merge` / `squash` 合并；
   - 断言 `refs/heads/release == merge_commit_sha` 且 `refs/heads/main` 不动，并检查合并内容落在 `release` 上。
2. 先运行该测试，确认**修复前失败**（下文 §3 原样记录）。
3. 修实现（只动 `service.rs` 的合并原语与调用点）：
   - `gix_merge_no_ff` / `gix_squash_merge` 增加 `base_branch` 参数，ours 取 `refs/heads/<base_branch>`，`commit_as` 也写回该 ref；
   - `do_merge_commit` / `do_squash_merge` 传入 `pr.base_branch`；
   - `merge_from_ref` 的 `Merge` / `Squash` 分支传入 `pr.base_branch`；
   - base 分支不存在时统一返回 409（`ensure_branch_exists(..., "base")`），而不是让 `rev_parse` 变成 500。
4. 重跑测试至全绿，再跑 `cargo clippy --workspace --all-targets -- -D warnings` 与 `scripts/check.sh --quick`。
5. 写 `docs/plans/T01-task.md`，把 `tasks/T01.md` 的 `State` 改为 `completed`，提交并推送 `agent/t01-merge-target`。

## 3. 修复前测试输出（验收证据，原样粘贴）

命令：`cargo test -p rg-http --test merge_target_tests`

```
running 2 tests
test merge_strategy_advances_the_base_branch_not_head ... FAILED
test squash_strategy_advances_the_base_branch_not_head ... FAILED

failures:

---- merge_strategy_advances_the_base_branch_not_head stdout ----

thread 'merge_strategy_advances_the_base_branch_not_head' (6758230) panicked at crates/rg-http/tests/merge_target_tests.rs:141:5:
assertion `left == right` failed: merge: the base branch must advance to the merge commit
  left: "2ad99b95225b2039a2032fa0a5999fb2a6ea6b4a"
 right: "0db318fed673808cdce5e468bd66d9f2fe830ffa"

---- squash_strategy_advances_the_base_branch_not_head stdout ----

thread 'squash_strategy_advances_the_base_branch_not_head' (6758231) panicked at crates/rg-http/tests/merge_target_tests.rs:141:5:
assertion `left == right` failed: squash: the base branch must advance to the merge commit
  left: "2ad99b95225b2039a2032fa0a5999fb2a6ea6b4a"
 right: "f545e9f30560ad43046f558f8de40cd491b0a83c"
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    merge_strategy_advances_the_base_branch_not_head
    squash_strategy_advances_the_base_branch_not_head

test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.98s

error: test failed, to rerun pass `-p rg-http --test merge_target_tests`
```

`left` 是 `refs/heads/release` 合并后的实际值（未移动，仍是 `main` 分支的初始提交），`right` 是接口返回的 `merge_commit_sha`；两者不相等即复现了“合到错误分支”。

## 4. 修复后测试输出

命令：`cargo test -p rg-http --test merge_target_tests`

```
running 2 tests
test merge_strategy_advances_the_base_branch_not_head ... ok
test squash_strategy_advances_the_base_branch_not_head ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.95s
```

相关回归（`pr_merge_strategy_tests` 2 项、`auto_merge_pinned_head_tests` 2 项、`merge_readiness_tests` 14 项）全部通过。

## 5. 改动面

- `crates/rg-core/src/pull_request/service.rs`：合并原语与调用点（见 §2 第 3 步）。
- `crates/rg-http/tests/merge_target_tests.rs`（新建）：复现 + 回归。
- 未触碰 `merge_gates`、`update_branch`、PR head 同步逻辑、路由注册、`Cargo.*`、迁移、`CLAUDE.md`、`ironforge-docs/`。

## 6. 关于 `HEAD` 依赖的说明

裸仓 `HEAD` 仍被仓库初始化、`create branch via file edit`（新分支从默认 HEAD 分出）与 `get_head_sha` 使用，这些是既有语义，未改动。本次只把 **PR 合并的写入目标**从 `HEAD` 改成 `refs/heads/<pr.base_branch>`：ours 与提交落点都改为 base ref，不再读写 `HEAD`。因此默认分支不是 base 的 PR 也正确，且 base 恰为默认分支时行为与旧版一致（旧版就是写 HEAD→默认分支）。

