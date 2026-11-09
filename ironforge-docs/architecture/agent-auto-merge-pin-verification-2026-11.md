# 自动合并与合并队列也钉死到评估过的 head（验证记录，2026-11-09）

> 基线 main `04433f8`。接 [agent-merge-pin-verification-2026-11.md](agent-merge-pin-verification-2026-11.md)：手动合并带 `expected_head_sha` 已经会钉死提交，但**自动合并**（`try_auto_merges_for_head_commit`）和**合并队列**（`merge_queue::process_for_head_commit*`）仍然是「按某个 head 满足条件」触发，之后按分支名重新读 head 再合并 —— 同一个「评估时的提交 ≠ 合并时的提交」的窗口。

## 一、改动

| 位置 | 改动 |
|---|---|
| `rg-core/src/pull_request/service.rs` | `try_auto_merge` 现在委托给新的 `try_auto_merge_pinned(…, expected_head)`：调用方没给预期提交时，用**检查保护规则时**读到的 `pr.head_sha`；合并改走 `merge_pr_expecting_head`。`HeadMoved` 不再当错误：释放 merge 认领（`restore_auto_merge`）、记一条 info 日志、返回 `status: "pending"` 与一句「head 已移动，下次推送重新评估」，PR 保持 open、自动合并保持开启 |
| 同上 | `try_auto_merges_for_head_commit` 明确传 `Some(commit_sha)` —— 触发它的那个提交就是它评估的提交 |
| `rg-core/src/pull_request/merge_queue.rs` | 队列合并改用 `merge_pr_expecting_head`，预期提交 = `entry.merge_group_head_sha`（有 CI 合并组时）或 `pr.head_sha`；`HeadMoved` → `release` 把条目放回 `queued`、`clear_merge_group` 清掉为旧 head 建的合并组、`waiting_reason` 说明原因、结束本轮（下次推送重新评估） |
| `rg-db/src/ops/merge_queue_ops.rs` | 新增 `release(db, entry_id)`：`running` → `queued`，清 `started_at`，不写失败原因 |
| `crates/rg-http/tests/auto_merge_pinned_head_tests.rs`（新） | 2 项集成测试 |
| `scripts/ci-db-tests.sh` | agent 列表加 `auto_merge_pinned_head_tests` |

`merge_pr` 的签名与手动合并行为没有变（它仍是 `merge_pr_expecting_head(…, None)` 的包装）。

## 二、证据

### 先红后绿

测试写完先跑一遍**未修的实现**（`git stash` 掉三个源文件）：

```
failures:
    an_auto_merge_whose_head_moved_waits_for_the_next_evaluation
    the_merge_queue_puts_a_moved_head_back_in_the_queue
```

失败信息就是问题本身：队列那一轮 `MergeQueueProcessResult { merged: [1], … }` —— 分支已经前进，队列还是把旧评估的那次 PR 合了。

### 两条测试各验什么

| 测试 | 做法 | 断言 |
|---|---|---|
| `an_auto_merge_whose_head_moved_waits_for_the_next_evaluation` | 开 PR → 开自动合并（squash）→ 分支再推一次 → 把**记录里的 head 改回**评估时的提交（模拟「推送落在评估与合并之间」）→ 调 `try_auto_merge` | `status == "pending"`、没有 merge、`main` 不动、PR 仍 open、`auto_merge_enabled` 仍为 true；随后把记录改成真实 head 再调一次 → `merged`、PR 变 merged、`main` 前进 |
| `the_merge_queue_puts_a_moved_head_back_in_the_queue` | 入队 → 分支前进 → 记录里的 head 改回旧提交 → `process_for_head_commit(旧提交)` | 什么都没合、`main` 不动、条目状态回到 **`queued`**；把记录改成真实 head 后 `process_for_head_commit(新提交)` → 合并、条目 `merged` |

### 全量（SQLite / PostgreSQL 17 / MySQL 8.4）

| 层级 | 结果 |
|---|---|
| 任务验证（SQLite） | `cargo test -p rg-http --test auto_merge_pinned_head_tests` → **2 passed** |
| 后端验证 | `scripts/ci-db-tests.sh postgres agent` 与 `mysql agent` 均 exit 0：各 **9 个目标 / 42 项**，`auto_merge_pinned_head_tests` 两边各 2 项 |
| 既有套件 | `merge_pinned_head_tests`（4 项）、`merge_queue_ci_tests` 等原有用例未改，随 agent 套件一起通过 |
| 静态检查 | `cargo fmt --all --check` 干净；`cargo clippy --workspace --all-targets -- -D warnings` 干净 |

第一次在 PG/MySQL 上跑时 auto-merge 那条是红的，原因不是后端差异：测试进程共用数据库（CLAUDE.md 坑 23），而我的辅助函数只按 PR 编号查行，拿到了别的测试的 PR，于是「把记录里的 head 改回旧提交」改错了行。按仓库 id 限定后通过 —— 与 B1 里踩到的是同一个坑。

## 三、未覆盖

- **真实的竞态窗口没有构造**：仍然是「让记录落后于分支」来复现，而不是在检查与合并之间插入一次真实推送（不可靠）。
- **合并组的 CI 变体没有测 `HeadMoved`**：预期提能在 `entry.merge_group_head_sha` 时取组头，但测试只走了非 CI 路径（`process_for_head_commit` 不带 `ci`）。
- **fork PR 的自动合并没有覆盖**：`merge_pr_expecting_head` 的 fork 分支已经有专门的测试（`merge_pinned_head_tests`），这里是同一段代码，但没有为自动合并再跑一遍 fork 场景。
- **`release` 的并发语义没有单独测**：两个 worker 同时处理同一条目时，`claim` 只有一方成功、`release` 只对 `running` 生效；这一点是读代码确认的，没有专门构造双 worker。
- **没有验证 PR timeline 上是否出现新事件**：`HeadMoved` 只写日志、把条目放回队列，没有写 PR 事件（按任务要求「记录一条日志/PR 事件即可」，这里选了日志）。
- **未在 CI 上跑**：以上都是本机结果。

## 四、重叠

`rg-core/src/pull_request/service.rs` 与 B1（`feat/repair-pull-refs`，未合并）同一个文件，但改的是不同函数（B1 在 `publish_head_ref`/新模块，这里在 `try_auto_merge` 与 `merge_queue`）。文本冲突概率低；若合并时冲突，取舍是两边都保留。
