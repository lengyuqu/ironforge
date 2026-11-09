# 合并时钉死 head 提交：`expected_head_sha` 不再有检查与合并之间的空窗（验证记录，2026-11-09）

> 接 [agent-write-path-verification-2026-11.md](agent-write-path-verification-2026-11.md)「未覆盖」里的一条：`expected_head_sha` 的合并前检查非原子。

## 一、问题

`POST …/pulls/{n}/merge` 带 `expected_head_sha` 时，处理函数先读 head 并比较，再调用 `merge_pr`；而 `merge_pr` 合并时**重新**按分支名读 head。两次读取之间有一次推送，服务端就会合并调用者从没看过的提交。

## 二、改动

- `rg_core::pull_request::merge_pr_expecting_head(…, expected_head: Option<&str>) -> Result<MergeResult, MergeError>`；`merge_pr` 是它的 `None` 包装，行为不变（合并队列、自动合并仍走它）。
- 带 `expected_head` 时，合并**以那个提交 id 为输入**（不再按分支名）：同仓 PR 在合并前一刻读 head，不等于预期就返回 `MergeError::HeadMoved{current, expected}`，否则用预期的 sha 调 `merge_from_ref`（三种策略都走这条）；fork PR 取回 fork 的 head 尖端后比较，不符同样 `HeadMoved`，相符则合并的就是这个 ref（即这个提交）。合并认领（`merging` 状态）在失败时照旧释放，PR 保持 `open`。
- REST 处理函数把 `expected_head_sha` 传下去，`HeadMoved` 映射成与原来提前检查相同的 409 `REF_ADVANCED`（`details.current_sha`/`expected_sha`）。提前检查保留，用来在 dry-run 与明显过期时快速回答。

## 三、证据

| 层级 | 结果 |
|---|---|
| 任务验证（SQLite） | `merge_pinned_head_tests` 4 项（真实服务 + 真实 git，直接调用合并服务）：三种策略下，head 在调用者检查后又前进，用旧 sha 合并得到 `HeadMoved`、main 不变、PR 仍 `open`；用现 head 的 sha 合并成功，main 的树里同时有被审提交与其后一个提交；head 分支已不存在 → `HeadMoved{current: None}`；fork PR 同样；REST 层：旧 sha 409 `REF_ADVANCED` 且带当前提交，对的 sha 可以 squash 合并。 |
| 全量（SQLite） | 见 PR 描述（`cargo test -p rg-http -p rg-core -p rg-db -p rg-mcp -p rg-git`、clippy、fmt）。 |
| 后端验证 | `merge_pinned_head_tests` 4 项在 PostgreSQL 17 与 MySQL 8.4（全新数据库）通过。 |

## 四、未覆盖

- **窗口只是变得极小，没有变成零**：同仓路径里「读 head」与 `merge_from_ref` 内部解析 sha 之间已没有按分支名的重新读取（用的是 sha 本身），但 `gix_merge_no_ff` 等仍各自对 `HEAD` 即**基分支**取当时的值；基分支在合并期间前进时的处理沿用原有逻辑，本次既没有改它，也没有重新验证。
- 没有构造出「检查与合并之间恰好有推送」的真实竞态测试（不可靠），而是在 head 已经动过的状态下直接验证服务层的钉死。
- 合并队列与自动合并没有 `expected_head`（它们按 PR 记录的 head 工作），不在本次范围。
