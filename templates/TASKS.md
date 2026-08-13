# Multi-Agent Task Claims

> Copy this file to `TASKS.md` in the target repository. This document defines the claim/completion protocol and a coordinator-maintained task index. **Live task state lives in one file per task under `tasks/` (`tasks/Txx.md`) — not in this document.** The index below is a read-only mirror; the authority for each task is its own file. Agents must never edit `TASKS.md`; they edit only their own `tasks/Txx.md` and `docs/plans/` documents.
>
> 来源：吸收自 aifuke 仓库已验证的 worktree 协作方案（v1.0）。

## Registry

| Field | Value |
| --- | --- |
| Project | `{{PROJECT_NAME}}` |
| Planning document | `PLANNING.md` |
| Task state directory | `tasks/` |
| Remote | `{{REMOTE_NAME}}` |
| Base branch | `{{BASE_BRANCH}}` |
| Human coordinator | `{{COORDINATOR}}` |

## Allowed States

- `available`: the task is open for an Agent to claim.
- `in_progress`: an Agent successfully recorded its ownership on the live base branch.
- `completed`: the Agent finished the task constraints and acceptance checks on its task branch.

Do not add other tracking states. `completed` means ready for human review, not merged, released, or formally accepted. Human coordination handles reassignment, integration, and release. If an `in_progress` task shows no activity for more than 48 hours, the coordinator may reset its `State` back to `available` (releasing the claim) — Agents must not reset each other's rows.

## Atomic Claim Protocol

1. Fetch `{{REMOTE_NAME}}/{{BASE_BRANCH}}`, read `PLANNING.md`, this file and `tasks/Txx.md`, then select one `available` task.
2. Confirm its constraints, dependencies, owned paths, proposed branch, and worktree are usable.
3. Create the task's `agent/<task>` branch and worktree from the live remote base.
4. In that worktree, (a) change only `State`, `Agent`, `Branch`, `Worktree` and `Claimed at` in `tasks/Txx.md` to `in_progress` and the claim metadata; (b) create the task's plan and task documents under `docs/plans/`: `Txx-plan.md` (execution plan, step breakdown, acceptance path) and `Txx-task.md` (constraint, owned-paths, dependency and risk confirmation).
5. Commit `tasks/Txx.md` together with the new plan/task documents, message `chore(tasks): claim Txx with plan`.
6. Push the claim commit to the base branch with a normal fast-forward push. The first successful push wins.
7. If the push is rejected, fetch and reread the live task file. If it is already `in_progress`, stop and choose another task. If only unrelated changes landed, rebase, recheck, and retry. Never force-push a claim.
8. After the base push succeeds, publish the recorded task branch and begin implementation in its worktree.

```sh
git fetch {{REMOTE_NAME}}
git worktree add ../{{REPO}}-t01 -b agent/t01-name {{REMOTE_NAME}}/{{BASE_BRANCH}}
cd ../{{REPO}}-t01
# Change only State/Agent/Branch/Worktree/Claimed at in tasks/T01.md, then create docs/plans/T01-plan.md and docs/plans/T01-task.md.
git add tasks/T01.md docs/plans/
git commit -m "chore(tasks): claim T01 with plan"
git push {{REMOTE_NAME}} HEAD:{{BASE_BRANCH}}
git push -u {{REMOTE_NAME}} HEAD
```

## Completion Protocol

1. Before finalizing, fetch `{{REMOTE_NAME}}/{{BASE_BRANCH}}` and rebase the task branch onto the latest base; resolve conflicts only within owned paths.
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
| T01 | `{{TASK_NAME}}` | available | unassigned | `{{TASK_IDS_OR_NONE}}` | `tasks/T01.md` |
| T02 | `{{TASK_NAME}}` | available | unassigned | `{{TASK_IDS_OR_NONE}}` | `tasks/T02.md` |

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
