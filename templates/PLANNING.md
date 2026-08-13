# Multi-Agent Planning

> Copy this file to `PLANNING.md` in the target repository. Replace every `{{PLACEHOLDER}}` before opening tasks for claim. This document defines stable coordination constraints; live task state belongs only in the per-task files under `tasks/`; `TASKS.md` holds the protocol and a coordinator-maintained mirror index.
>
> 来源：吸收自 aifuke 仓库已验证的 worktree 协作方案（v1.0）。

## Project Context

| Field | Value |
| --- | --- |
| Project | `{{PROJECT_NAME}}` |
| Goal | `{{ONE_SENTENCE_GOAL}}` |
| Human coordinator | `{{COORDINATOR}}` |
| Remote | `{{REMOTE_NAME}}` |
| Base branch | `{{BASE_BRANCH}}` |
| Baseline SHA | `{{BASELINE_SHA}}` |

## Scope and Boundaries

- Required outcome: `{{REQUIRED_OUTCOME}}`
- Explicitly out of scope: `{{NON_GOAL}}`
- Shared configuration that Agents must not change: `{{PROTECTED_PATHS_OR_NONE}}`
- External or manual decisions: `{{HUMAN_DECISION_BOUNDARIES}}`

## Shared Contracts

Define interfaces before opening dependent tasks for claim.

| Contract | Source path | Owner task | Consumers | Baseline/version |
| --- | --- | --- | --- | --- |
| `{{CONTRACT_NAME}}` | `{{PATH}}` | `{{TASK_ID}}` | `{{TASK_IDS}}` | `{{SHA_OR_VERSION}}` |

## Task Design Rules

1. Every task has one ID, one clear deliverable, explicit owned paths, and one Agent owner.
2. Tasks that edit the same paths must not be available for claim at the same time.
3. Contract or schema tasks are claimed before tasks that consume those contracts.
4. Acceptance commands describe the task boundary but do not automatically decide when human coordination is complete.
5. Each `tasks/Txx.md` is the only source of truth for its own `available`, `in_progress`, or `completed` state; `TASKS.md` holds only a mirror index.
6. A claim is invalid without its plan and task documents in `docs/plans/`; the claim commit must include them.
7. Merge conflicts in `tasks/` or `TASKS.md` are resolved by the coordinator only, preserving every row from the remote side. Never drop another task's file or row during conflict resolution.

## Claim and Worktree Model

An Agent reads the live base-branch `tasks/Txx.md`, selects one `available` task, and creates the recorded task branch/worktree. The Agent changes only `State`, `Agent`, `Branch`, `Worktree` and `Claimed at` in its own `tasks/Txx.md` to `in_progress`, and commits the task's plan/task documents under `docs/plans/` together with the claim, then attempts a normal fast-forward push to the live base branch. The first successful push wins; a rejected push is not a valid claim and must never be forced.

Create the dedicated worktree from the live base before making the atomic claim:

```sh
git fetch {{REMOTE_NAME}}
git worktree add ../{{REPO}}-t01 -b agent/t01-name {{REMOTE_NAME}}/{{BASE_BRANCH}}
git worktree list
```

The claim commit must change only `tasks/Txx.md` plus the new plan/task documents, never `TASKS.md`. Push it with `git push {{REMOTE_NAME}} HEAD:{{BASE_BRANCH}}`; after that succeeds, publish the task branch with `git push -u {{REMOTE_NAME}} HEAD`. Agents then work only inside the recorded worktree and owned paths.

After implementation and task acceptance checks pass, the Agent changes only its own `tasks/Txx.md` from `in_progress` to `completed` in the final task-branch commit and pushes that branch. `completed` means development is ready for human review; it does not mean merged, released, or formally accepted. Reassignment, integration, and release remain under human coordination and are not additional task states. A claim showing no activity for 48 hours may be reset to `available` by the coordinator.

## Work Allocation

Record only stable ownership boundaries here. Do not duplicate live task status from `tasks/`.

| Area | Owned paths | Contract owner | Dependent task IDs | Coordination notes |
| --- | --- | --- | --- | --- |
| `{{AREA}}` | `{{PATHS}}` | `{{TASK_ID}}` | `{{TASK_IDS}}` | `{{NOTES}}` |

## Decision Log

| Date | Decision | Reason | Affected tasks | Decided by |
| --- | --- | --- | --- | --- |
| `{{YYYY-MM-DD}}` | `{{DECISION}}` | `{{RATIONALE}}` | `{{TASK_IDS}}` | `{{NAME}}` |
