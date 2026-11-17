# T22 执行记录 — 任务作废与 R19-2~R19-5 现状核实

> 计划见 `docs/plans/T22-plan.md`，权威约束见 `tasks/T22.md`。
> 基线 `119023f`，分支 `agent/t22-review-approve-write`，worktree `../ironforge-t22`。
> **本任务未产生代码改动。**

## 1. 任务前提复核（开工第一步）

`tasks/T22.md` 的前提是「`submit_review` 只过 `require_authenticated_read`」。开工时先核对基线：

```
$ cd /Users/yuqu/vibeCodeing/ironforge-t22 && git log --oneline -3
119023f docs(tasks): add T22 to require write access for PR approvals (R19-1)
f147926 docs(tasks): mark T21 completed in the mirror index
a83c6c3 Merge task T21: scope_expand 目标 scope 绑定进票据（#19 步骤6 补丁）

$ sed -n 356,376p crates/rg-http/src/api/reviews.rs
    if matches!(action.as_str(), "approve" | "request_changes") {
        let can_write =
            rg_core::repo::service::can_write_repo(&state.db, &repo_model, Some(user_id))
                .await
                .unwrap_or(false);
        if !can_write {
            let requested =
                match rg_db::ops::pr_reviewer_request_ops::list_by_pr(&state.db, pr.id).await { … };
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

追溯来源：

```
$ git log -1 --format='%h %ad %s' --date=iso -S 'REVIEW_DECISION_REQUIRES_REVIEWER' -- crates/rg-http/src/api/reviews.rs
cdc2d7c 2026-11-14 22:59:10 +0800 feat(authz): 通用审批原语（表 + 状态机 + 求值器）并修复 PR 评审越权 (#19)

$ git merge-base --is-ancestor cdc2d7c 119023f && echo "true: cdc2d7c is ancestor of 119023f"
true: cdc2d7c is ancestor of 119023f
```

`tasks/T06.md:56`（T06 的契约原文）：

> `submit_review` 的 `require_authenticated_read` 收紧为：**approve / request_changes 需要写权限
> （或明确的审批人身份）**，`comment` 类动作保持只读即可投。

评审人席位不可自助：`crates/rg-http/src/api/reviews.rs:1093` `request_reviewer` 走
`require_pr_manager`，只有对该 PR 有管理权限的人能写名单。

agent 另有守卫：`crates/rg-http/src/api/auth.rs:165 ensure_reviewer_may_approve`，agent 一律
403 `AGENT_MAY_NOT_APPROVE`，与本次准入面无关。

**结论：R19-1 已由 T06 关闭，`tasks/T22.md` 说的是 T06 之前的状态。**

## 2. 决策过程（A / B / C）

| 方案 | 内容 | 结果 |
| --- | --- | --- |
| A | 保留 approve「写权限或评审人席位」，把 `request_changes` 放宽回只读 | 否 |
| B | 零代码改动，认定 R19-1 已由 T06 关闭，只做文档收尾 | **采纳** |
| C | 按 T22 字面把 approve 收紧为只看 `can_write_repo` | 否 |

选 B 的理由（与协调者确认一致）：

- **更新的决策覆盖更旧的复核结论**。T06（11-14，带测试与契约文档）晚于 N12 复核（11-12）。
- 方案 A 会回退 T06 的 `request_changes` 契约。放宽它是一个独立的语义决策（是否允许外部贡献者
  对别人的 PR 表达反对），不属于「修 R19-1」，应单独立项。
- 方案 C 会去掉评审人席位分支，破坏 CODEOWNERS / 显式请求评审人流程，并让
  `pr_review_tests.rs::a_requested_reviewer_may_approve_without_write_access` 变红。席位是 PR
  管理者授予的单 PR 许可，不是 R19-1 的洞。

另外确认：`request_changes` 不参与合并门计数（`branch_protection/merge_gates.rs:373` 只取
`current_human_approver_ids`，即 `action == "approve"`），所以上述取舍都不影响合并安全。

## 3. R19-2 ~ R19-5 现状核实（基线 `119023f`）

| 编号 | 结论 | 证据 |
| --- | --- | --- |
| R19-2 自批守卫下沉 core | **未修** | 仅 HTTP 层有：`reviews.rs:331` `if matches!(action.as_str(), "approve" \| "request_changes") && pr.author_id == user_id { bad_request }`；`rg-core/src/review/service.rs:57-102 submit_review` 只校验 PR 存在与 `state == "open"`，无任何 author 比较，直接 `pr_review_ops::create` |
| R19-3 邮件文案 | **未修** | `rg-core/src/email/mod.rs:106` 尾句仍在：「You received this email because you have notifications enabled on IronForge.」；同文件 `:110` `message = html_escape(message)`，而 `html_escape`（`:118-122`）把 `<`/`>` 转义，调用方传入的 `<br/>` 会变成字面文本 |
| R19-4 口径分叉 | **未修（并已退化死代码）** | `rg-db/src/ops/pr_review_ops.rs:46 count_approvals(db, pr_id)` 委托 `count_current_human_approvals(db, pr_id, None)`；`head_sha = None` 走的分支是 `review.commit_id.is_none()`（`:79-82`），只数「未绑定 commit」的票，与合并门用的 `Some(head_sha)` 口径不同。全仓库无调用方：`grep -rn "pr_review_ops::count_approvals" crates/` 无结果（同名函数在 `ci_environment_ops` / `authorization_ops` 是另外两套，与本条无关） |
| R19-5 agent 判定复用 | **未修** | 三份独立实现：`rg-http/src/api/auth.rs:176`（PR approve，`ensure_reviewer_may_approve`）、`rg-core/src/authorization/evaluator.rs:325-338`（`ensure_human_decider`，授权票）、`rg-http/src/api/ci_environments.rs:288-303`（环境审批）。三处各自查 `rg_core::agent::is_agent`，无共享 helper |

### 3.1 关于「5 条 L2 一条都没做」

R19-1 不是「没做」，而是已在 T06 做了（且做得比 T22 任务书更严：连 `request_changes` 一起收紧）。
R19-2 ~ R19-5 确实都还没做。所以 `#19` 的剩余面是 **4 条**，不是 5 条。

### 3.2 关于历史票（任务书要求写清的一条）

收紧不会让历史票失效。`current_human_approver_ids`
（`rg-db/src/ops/pr_review_ops.rs:64`）只按 `action == "approve"`、`reviewer_id ∈ human`、
`commit_id == head_sha` 过滤，**不看批准人当时的权限**。T06 上线前由只读用户投出的 approve，
只要 `commit_id` 仍等于当前 head，**依然计入合并门**。清理存量需要单独的数据订正任务。

## 4. 改动文件

| 文件 | 性质 |
| --- | --- |
| `docs/plans/T22-plan.md` | 新增（计划 / 作废说明） |
| `docs/plans/T22-task.md` | 新增（本文件） |
| `tasks/T22.md` | 编辑（State → `completed`，开头加作废提示） |

**代码零改动**：`git diff --stat 119023f` 只包含以上三个文档。

## 5. 验收

```
$ scripts/check.sh --quick
==> cargo fmt --all --check
    (1s)
==> cargo clippy --workspace --all-targets -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 44.12s
    (44s)
==> checks passed in 45s (quick)
```

基线契约测试（开工时跑过一次，用于证明「现状即契约」，不是本次验收的一部分）：

```
$ cargo test -p rg-http --test pr_review_tests
running 5 tests
test the_author_still_cannot_decide_their_own_pr ... ok
test a_requested_reviewer_may_approve_without_write_access ... ok
test a_reader_may_comment_but_not_approve ... ok
test a_refused_approval_does_not_count_towards_the_merge_gate ... ok
test a_writer_may_approve ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

按协调者指示，零代码改动不跑 `cargo test -p rg-http` 全量（无可验证的 diff，且该套件约 34 分钟）。

## 6. 遗留（交给后续任务）

- R19-2：把自批守卫下沉到 `rg-core::review::service::submit_review`（注意 `reviews.rs:883`
  的 `Comment` 隐式评审路径不受影响）。
- R19-3：改成 action 相关的文案，并让消息里的换行真正渲染（当前被 `html_escape` 吃掉）。
- R19-4：删除死函数 `pr_review_ops::count_approvals`，或让它必须传 `head_sha`，收敛为单一口径。
- R19-5：抽出「agent 不得批准」的共用判定，三处复用。
- 历史越权票的清理：需要单独的数据订正，不要指望本次（或 T06 的）收紧会自动生效。
