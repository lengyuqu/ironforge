# Multi-Agent Task Claims — IronForge

> 本文件定义认领/完成协议与协调者维护的任务索引。**活的任务状态在 `tasks/Txx.md`，不在本文件。** 下面的索引是只读镜像；每个任务的权威是它自己的文件。Agent 不得编辑本文件，只改自己的 `tasks/Txx.md` 与 `docs/plans/` 文档。

## Registry

| Field | Value |
| --- | --- |
| Project | IronForge |
| Planning document | `PLANNING.md` |
| Task state directory | `tasks/` |
| Remote | `cnb` |
| Base branch | `main` |
| Human coordinator | 余曲（别问） |

## Allowed States

- `available`: the task is open for an Agent to claim.
- `in_progress`: an Agent successfully recorded its ownership on the live base branch.
- `completed`: the Agent finished the task constraints and acceptance checks on its task branch.

Do not add other tracking states. `completed` means ready for human review, not merged, released, or formally accepted. Human coordination handles reassignment, integration, and release. If an `in_progress` task shows no activity for more than 48 hours, the coordinator may reset its `State` back to `available` (releasing the claim) — Agents must not reset each other's rows.

## Atomic Claim Protocol

1. Fetch `cnb/main`, read `PLANNING.md`, this file and `tasks/Txx.md`, then select one `available` task.
2. Confirm its constraints, dependencies, owned paths, proposed branch, and worktree are usable.
3. Create the task's `agent/<task>` branch and worktree from the live remote base.
4. In that worktree, (a) change only `State`, `Agent`, `Branch`, `Worktree` and `Claimed at` in `tasks/Txx.md` to `in_progress` and the claim metadata; (b) create the task's plan and task documents under `docs/plans/`: `Txx-plan.md` (execution plan, step breakdown, acceptance path) and `Txx-task.md` (constraint, owned-paths, dependency and risk confirmation).
5. Commit `tasks/Txx.md` together with the new plan/task documents, message `chore(tasks): claim Txx with plan`.
6. Push the claim commit to the base branch with a normal fast-forward push. The first successful push wins.
7. If the push is rejected, fetch and reread the live task file. If it is already `in_progress`, stop and choose another task. If only unrelated changes landed, rebase, recheck, and retry. Never force-push a claim.
8. After the base push succeeds, publish the recorded task branch and begin implementation in its worktree.

```sh
git fetch cnb
git worktree add ../ironforge-t01 -b agent/t01-merge-target cnb/main
cd ../ironforge-t01
# 只改 tasks/T01.md 的 State/Agent/Branch/Worktree/Claimed at，然后建 docs/plans/T01-plan.md 与 docs/plans/T01-task.md
git add tasks/T01.md docs/plans/
git commit -m "chore(tasks): claim T01 with plan"
git push cnb HEAD:main
git push -u cnb HEAD
```

## Completion Protocol

1. Before finalizing, fetch `cnb/main` and rebase the task branch onto the latest base; resolve conflicts only within owned paths.
2. Finish the recorded task constraint and run its acceptance commands.
3. In the same task worktree, change only `State` in `tasks/Txx.md` from `in_progress` to `completed`; preserve the Agent, branch, worktree, and claim time.
4. Include the `tasks/Txx.md` update in the final implementation commit, or add a final document-only commit when implementation was already committed.
5. Push the recorded task branch. Do not push implementation commits directly to the base branch.
6. Report the branch, final commit SHA, changed paths, validation results, and remaining risks to the human coordinator.
7. The base branch receives the `completed` record when the human coordinator merges the task branch.

## Coordinator Merge Rules

1. Merging task branches into the base branch is a coordinator-only operation, executed serially.
2. On each merge, sync the affected row in the Task Index below (State/Agent) to the authoritative `tasks/Txx.md`; never reorder or drop other task rows.
3. On merge conflicts in `tasks/` or `TASKS.md`, preserve every task row from the remote base side and re-apply the local task's own change only. Never discard another task's file or row.
4. After a merge, the coordinator may delete the merged task branch and prune its worktree.

## Task Index (mirror — authority is `tasks/Txx.md`, refreshed by the coordinator on merge)

| ID | Task | State | Agent | Depends on | Status file |
| --- | --- | --- | --- | --- | --- |
| T01 | [#60] PR 合并写到 `base_branch` 而非仓库 `HEAD` | completed | t01-merge-target | none | `tasks/T01.md` |
| T02 | [#48] MCP 写工具集补 cherry-pick / revert / unified diff | completed | t02-mcp-tools | none | `tasks/T02.md` |
| T03 | [#15] R3-P1 事件信封与投递头 | completed | t03-event-envelope | none | `tasks/T03.md` |
| T04 | [#38] R7 推送时密钥扫描 | completed | t04-secret-scan | none | `tasks/T04.md` |
| T05 | [#40] R10 配额与用量（最小闭环） | completed | t05-ci-quota | none | `tasks/T05.md` |
| T06 | [#19] 通用审批原语 · 步骤1（表+状态机+求值器）+ 修 PR 评审越权 | completed | t06-authorization | none | `tasks/T06.md` |
| T08 | [#62] 接受 unified diff 的提交端点（apply-patch） | completed | t08-apply-patch | none | `tasks/T08.md` |
| T07 | [#19·步骤3修正版] PR 头部漂移：旧票失效事件化 + 门禁阻断 | completed | t07-head-drift | none | `tasks/T07.md` |
| T09 | [#39] 代码智能·结构化大纲（抽取层，不含端点） | completed | t09-outline | none | `tasks/T09.md` |
| T10 | [#39] 代码智能·`/ai/.../map` 端点 + token 预算裁剪 | completed | t10-repo-map | T09 | `tasks/T10.md` |
| T11 | [#18] Agent 草稿分支命名空间（TTL + 不通知 + 可跑 CI） | completed | t11-agent-drafts | none | `tasks/T11.md` |
| T12 | [#19·步骤5] 通知偏好 + 邮件单一出口 | in_progress | t12-notify-prefs | none | `tasks/T12.md` |

## Pre-Claim Checks

- [ ] The live `tasks/Txx.md` State is still `available`.
- [ ] No `in_progress` task owns overlapping paths.
- [ ] The proposed branch and worktree are not already in use.
- [ ] Only the selected task file and its plan/task documents will change in the claim commit; `TASKS.md` must not change.
- [ ] No placeholder remains in the selected task file.
- [ ] The claim uses a normal push; force-push is prohibited.

## Completion Checks

- [ ] The task constraint is implemented in the recorded owned paths.
- [ ] Acceptance commands were run and their actual results are reported.
- [ ] `tasks/Txx.md` State changed from `in_progress` to `completed`.
- [ ] No other task file or `TASKS.md` was modified.
- [ ] The final task branch and commit SHA were reported for human coordination.
