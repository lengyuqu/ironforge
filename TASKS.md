# Multi-Agent Task Claims — IronForge

> 认领/完成协议与协调者维护的索引。**活的任务状态在 `tasks/Txx.md`，不在本文件。**
> Agent 不得编辑本文件，只改自己的 `tasks/Txx.md` 与计划文档。

## Registry

| Field | Value |
|---|---|
| Project | IronForge |
| Planning document | `PLANNING.md` |
| Task state directory | `tasks/` |
| 开发主仓 | GitHub `lengyuqu/ironforge` |
| Mirror | CNB `lengyuqu/ironforge` |
| Base branch | `main` |
| Human coordinator | 余曲（别问） |

## Allowed States

- `available`：任务开放认领。
- `in_progress`：Agent 已在基线分支记录所有权。
- `completed`：Agent 在任务分支完成约束与验收。

没有其他状态。`completed` 表示可供人审，不表示已合并、发布或正式接受；集成与发布由人负责。48 小时无活动的 `in_progress` 可由协调者重置；Agent 不得重置他人的行。

## Atomic Claim Protocol

1. 取基线，读 `PLANNING.md`、本文件与目标任务文件，选一个 `available` 任务。
2. 确认约束、owned paths、依赖、分支与 worktree 可用。
3. 从基线创建 `agent/<task>` 分支与 worktree。
4. 只改 `tasks/Txx.md` 的 `State`/`Agent`/`Branch`/`Worktree`/`Claimed at` 为 `in_progress`；在 `docs/plans/` 建 `Txx-plan.md` 与 `Txx-task.md`。
5. 提交 `tasks/Txx.md` + 计划文档，消息 `chore(tasks): claim Txx with plan`。
6. 普通快进推送到基线。第一次成功推送获胜；被拒绝就重新读取任务文件，已被认领则换任务；**禁止强推**。
7. 认领成功后发布任务分支，在记录的 worktree 内工作。
8. 实现与验收通过后，只把自己的 `State` 改为 `completed`，提交并推送任务分支。

## Completion Protocol

- 任务约束已实现于记录的 owned paths。
- 验收命令已实际运行，并如实报告结果。
- `tasks/Txx.md` 从 `in_progress` 改为 `completed`。
- 没有修改其他任务文件或 `TASKS.md`。

## Coordinator Merge Rules

1. 只在任务 `completed` 且验收通过后合并；合并由协调者执行。
2. `tasks/` 或 `TASKS.md` 冲突保留远端每一行，再重放本地任务自己的改动；不得丢弃他人的任务文件或行。
3. 合并后可删除已合并的任务分支并清理 worktree。
4. 任务记录（`tasks/Txx.md`、`docs/plans/Txx-*`）在合并后不进入长期文档集；历史由 git 承载。

## Task Index（镜像；权威是 `tasks/Txx.md`）

当前没有活跃任务。新任务建立后由协调者在此追加，合并完成后移除。

| ID | Task | State | Agent | Depends on | Status file |
|---|---|---|---|---|---|

## Pre-Claim Checks

- [ ] 目标任务 `State` 仍是 `available`。
- [ ] 没有 `in_progress` 任务占用重叠路径。
- [ ] 分支与 worktree 未被占用。
- [ ] 认领提交只含目标任务文件与计划文档；不改 `TASKS.md`。
- [ ] 任务文件没有占位符。
- [ ] 认领用普通推送；禁止强推。

## Completion Checks

- [ ] 约束已实现于 owned paths。
- [ ] 验收命令已运行并报告实际结果。
- [ ] `tasks/Txx.md` 已改为 `completed`。
- [ ] 未修改其他任务文件或 `TASKS.md`。
