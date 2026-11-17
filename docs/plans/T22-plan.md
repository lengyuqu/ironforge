# T22 执行计划 — [#19·R19-1] 任务作废：越权已由 T06 关闭

> 权威约束见 `tasks/T22.md`。基线 `119023f`，分支 `agent/t22-review-approve-write`，
> worktree `../ironforge-t22`。
> 实际判断过程与核实证据见 `docs/plans/T22-task.md`。

## 1. 结论先行

**T22 不产生任何代码改动。** `submit_review` 的越权（只读者可投 approve）已经在
`cdc2d7c`（T06，2026-11-14）关闭，该提交是基线 `119023f` 的祖先。本任务的产出是：
一份作废记录（本文件）、一份核实记录（`T22-task.md`），以及 `tasks/T22.md` 的状态收尾。

## 2. 为什么任务书的前提不成立

`tasks/T22.md` 写的是「`submit_review` 只过 `require_authenticated_read` ⇒ 能读这个仓库的人
就能投 approve」。这是 `#19` **复核阶段（2026-11-12，finding N12）** 的结论，T22 的任务书
直接沿用了它，但没有对当前代码复核。

实际基线 `crates/rg-http/src/api/reviews.rs:356-376` 在写库之前已有门禁：

```rust
if matches!(action.as_str(), "approve" | "request_changes") {
    let can_write =
        rg_core::repo::service::can_write_repo(&state.db, &repo_model, Some(user_id))
            .await
            .unwrap_or(false);
    if !can_write {
        let requested =
            match rg_db::ops::pr_reviewer_request_ops::list_by_pr(&state.db, pr.id).await { /* … */ };
        if !requested.iter().any(|row| row.reviewer_id == user_id) {
            return AppError::coded(
                axum::http::StatusCode::FORBIDDEN,
                "REVIEW_DECISION_REQUIRES_REVIEWER",
                "approving or requesting changes requires write access or a reviewer seat on this pull request",
            )
            .into_response();
        }
    }
}
```

证据链：

| 证据 | 内容 |
| --- | --- |
| `git merge-base --is-ancestor cdc2d7c 119023f` | 退出码 0（`cdc2d7c` 是基线祖先） |
| `cdc2d7c` | `feat(authz): 通用审批原语（表 + 状态机 + 求值器）并修复 PR 评审越权 (#19)`，2026-11-14 |
| `tasks/T06.md:56` | 「`submit_review` 的 `require_authenticated_read` 收紧为：**approve / request_changes 需要写权限（或明确的审批人身份）**，`comment` 类动作保持只读即可投。」 |
| `reviews.rs:1093` `request_reviewer` | 走 `require_pr_manager` ⇒ 评审人席位不是自助入口 |
| `crates/rg-http/tests/pr_review_tests.rs` | 5 条用例已把契约钉住，基线全绿 |

**更新的决策覆盖更旧的复核结论**：T06 是 11-14 的合并提交、带测试与契约文档；N12 是 11-12 的
复核记录。用旧记录去回退新契约是错误的，因此本任务承认前提错误并作废。

## 3. 与任务书的两个差异，均维持现状

任务书给出「只收紧 approve，`request_changes` 保持只读」的表，与基线有两处出入。
两处都**不做改动**，理由如下。

### 3.1 `approve` 保留「写权限 **或** 评审人席位」

任务书写「approve → 写权限」，若按字面去掉席位分支：

- 会破坏 CODEOWNERS / 显式请求评审人的流程（`pr_review_tests.rs::a_requested_reviewer_may_approve_without_write_access`
  是有意写下的契约，会变红）；
- 席位不是自助入口：`request_reviewer`（`reviews.rs:1093`）走 `require_pr_manager`，只有对该 PR
  有管理权限的人才能写名单，是「PR 管理者显式授予的单 PR 许可」，不是 R19-1 的洞；
- 任务书写「不引入显式审批人名单」时，并不知道这套按 PR 的评审人席位机制已经存在；
  它约束的是「仓库级审批人名单」，与这里不是同一件事。

### 3.2 `request_changes` 不放宽回只读

任务书要求「保持 `require_authenticated_read`」，基线已按 T06 收紧。放宽它是一个**独立的语义
决策**（要不要允许外部贡献者对别人的 PR 表达反对），不属于「修 R19-1」。如果将来要放宽，
应单独立项、单独评估，而不是在这个作废任务里搭车修改。

`request_changes` 不参与合并门计数（`merge_gates.rs:373` 只取
`current_human_approver_ids`，即 `action == "approve"` 的票），所以两边的取舍都不影响合并安全。

## 4. 历史票的清理说明（任务书要求写清的一条）

**本次收紧（以及 T06 的那次收紧）不会让历史票失效。**

`current_human_approver_ids`（`crates/rg-db/src/ops/pr_review_ops.rs:64`）只按
`action == "approve"`、`kind == human`、`commit_id == head_sha` 过滤，**不看批准人当时的权限**。
因此，在 T06 上线之前由「只读用户」投出的 approve，只要 `commit_id` 仍等于当前 head，
**依然计入合并门**。收紧只阻止新的越权票产生，不会自动清理存量；如需清理，必须单独写一次
数据订正（按 `pr_reviews` + 当时的权限快照重算），不在本任务范围内。

## 5. 附带产出：R19-2 ~ R19-5 现状核实

见 `docs/plans/T22-task.md` 第 3 节。结论：四条**都未修**，各自仍有可定位的缺口
（自批守卫仍只在 HTTP 层、邮件尾句与 HTML 转义仍在、`count_approvals` 死函数与合并门口径仍不一致、
「agent 不得批准」仍是三份独立判定）。

## 6. 明确不做

- 不改 `crates/rg-http/src/api/reviews.rs`（含 `submit_review` 的门禁）；
- 不改 `pr_review_tests.rs`；
- 不做 R19-2 / R19-3 / R19-4 / R19-5；
- 不引入仓库级显式审批人名单；
- 不清理历史越权票。

## 7. 验收

零代码改动，因此不跑 `cargo test -p rg-http` 全量（34 分钟，无可验证的 diff）。只跑
`scripts/check.sh --quick`（`cargo fmt --check` + `cargo clippy --workspace --all-targets -D warnings`）
证明仓库干净。基线 `pr_review_tests`（5 条）在本任务开始时已跑过一次，全绿，作为「现状即契约」
的佐证。
