# Task {{TASK_ID}} - {{TASK_NAME}}

> Copy this file to `tasks/{{TASK_ID}}.md` in the target repository for every task. Replace every `{{PLACEHOLDER}}` before opening the task for claim. This file is the only source of truth for this task's state; `TASKS.md` holds only a coordinator-maintained mirror index.

## Task Constraint

{{TESTABLE_DELIVERABLE}}

## State

| Field | Value |
| --- | --- |
| State | `available` |
| Agent | `unassigned` |
| Branch | `agent/{{TASK_BRANCH}}` |
| Worktree | `../{{REPO}}-{{TASK_ID_LOWER}}` |
| Claimed at | unclaimed |
| Depends on | `{{TASK_IDS_OR_NONE}}` |
| Owned paths | `{{PATHS}}` |
| Acceptance commands | `{{COMMANDS}}` |
| Plan document | `docs/plans/{{TASK_ID}}-plan.md` |
| Task document | `docs/plans/{{TASK_ID}}-task.md` |

## Notes

Reserved for the claiming Agent's constraint and risk confirmation (kept in sync with `docs/plans/{{TASK_ID}}-task.md`).
