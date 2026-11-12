# 批量提交（issue #16）落地核实 — 2026-11

> 对象：issue #16「多文件原子提交」。NPC 两轮分析（评论 `2120149224061829120` / `2120152587344187392`）与其实施回报（评论 `2120166446301880320`）。
> 落地提交：`48f03a7` `feat(repo): batch file commit route, phases 1-4`，作者 `cnb.biewen`；在 `cnb/main`（核实基线 `6aaf77f`）中，`git merge-base --is-ancestor 48f03a7 cnb/main` 退出 0。
> 本文只写核实结论与证据；判定过程与反向清单见 `docs/reports/OPT-LOOP-LEDGER.md`「单发轮 · issue #16」。

## 1. 交付面

`git show --stat 48f03a7` = **10 files changed, 1873 insertions(+), 91 deletions(-)**，与它自述的 10 个文件逐一对应：

`git show --numstat 48f03a7`：`pat_scope.rs` 3/0 · `repo/service.rs` 477/0 · `rg-git/src/ops.rs` 330/84 · `api/mod.rs` 1/0 · `api/repo_commits.rs` 327/0（新） · `api/repo_content.rs` 7/4 · `idempotency.rs` 4/3 · `lib.rs` 5/0 · `openapi.rs` 1/0 · `tests/batch_commit_tests.rs` 718/0（新）。合计 = 1873/91，与 `--stat` 一致。

**`rg-db` 零迁移**成立（diff 内无 `crates/rg-db/**`）。

`ops.rs` 的 84 行删除全部落在 `commit_tree_edits` 自身：它被拆成 `validate_tree_edits`（`:1286`）+ `plan_tree_edits`（`:1319`）+ `edit_tree`（`:1449`）+ `commit_tree_edits`（`:1533`）。**没有碰 `rebase_merge`**（`:188`）——但 issue 里被要求「别重构」的 `merge_trees` 在本仓并不存在，`ops.rs:351` 只是对 gix 同名方法的**调用**（`grep -rn 'fn merge_trees' crates/` 零命中）。

## 2. 八条验收条件

| # | 条件（issue 评论 2 原文要求） | 证据 | 判定 |
|---|---|---|---|
| 1 | 原子性有测试钉死：第 3 个文件失败 → 前 2 个也不提交，SHA 不动 | `tests/batch_commit_tests.rs:148` `a_batch_that_fails_on_its_third_edit_leaves_the_branch_untouched`，本机实跑通过 | 成立 |
| 2 | `check_direct_commit_allowed` 必过（`require_pr` 不能绕） | `repo/service.rs:2393`；测试 `a_batch_respects_require_pr` 通过 | 成立 |
| 3 | `idempotency_supported` 显式加 `commits` | `idempotency.rs:82`（`\| "commits"`），函数在 `:64` | 成立 |
| 4 | `commit_tree_edits` 用 `spawn_blocking` 包 | `repo/service.rs:2425`（校验/规划）、`:2573`（真提交） | 成立 |
| 5 | `move` 成环直接拒（409 `PATH_CONFLICT`） | `repo/service.rs:2674` 的错误串；`repo_commits.rs:311` 映射 | 成立 |
| 6 | `branch` 必填 + `expected_sha` 不兜底 HEAD | `repo_commits.rs` 的请求体无默认分支路径；测试 `a_batch_requires_a_branch_and_a_base_commit` | 成立 |
| 7 | `update_files_in_commit` 改造前先搜调用方 | 它**没**改造（自述偏差 1）；真实调用方 1 处：`repo/service.rs:1980` 定义、`review/service.rs:466` 调用 | 成立（以「不动」方式满足） |
| 8 | `dry_run` 走通、`tip()` 不变、形状对齐 | 测试 `a_dry_run_reports_and_writes_nothing` 通过 | 成立 |

路由注册：`rg-http/src/lib.rs:1391`（`/repos/{owner}/{name}/commits`）在 `:1396`（`contents/{*path}`）**之前**，未重排其他路由；OpenAPI `create_commit` 已登记。

## 3. 六处自述偏差

| 它说的 | 核实 | 判定 |
|---|---|---|
| 1 `update_files_in_commit` 有 1 个活调用方 `review/service.rs:466` | 全仓 2 命中：定义 `repo/service.rs:1980` + 调用 `review/service.rs:466` | 成立（且与它分析阶段「无调用方/死代码」自相矛盾，已在其回报中自行订正） |
| 2 动了 `pat_scope::ref_limited_write_allowed`，加 `["commits"]` | `pat_scope.rs:328`；前置条件满足——处理函数 `repo_commits.rs:166` 调 `ensure_token_may_write_branch` | 成立（越界如实申报，见 §5） |
| 3 `lib.rs` 是 +5 行不是 +1 | diff 恰 5 行：1 行注释 + 4 行 `.route(...)` | 成立 |
| 4 冲突判定比「只拒环」更严（写 X 且 move 掉 X 也拒） | `repo/service.rs:2674` 的文案就是这条；测试 `a_self_contradicting_batch_is_refused` | 成立 |
| 5 `dry_run` 会写一个游离 tree 对象 | `plan_tree_edits`（`ops.rs:1319`）→ `edit_tree` 末尾 `editor.write()`（`ops.rs:1488`），dry run 也写对象库；无 ref 指向 | 成立 |
| 6 `rg-db` 零迁移 | 见 §1 | 成立 |

6 处无一处虚假陈述。**问题不在偏差，而在漏报**（§4）。

## 4. 未交付项：阶段 4（MCP `commit_files`）

它自己的计划 §6 把落地顺序定为「1 `MoveFile` → 2 `commit_files_with` → 3 `POST /commits` → 4 **MCP `commit_files`**」；issue 评论 2 下令「只做你列的**阶段 1-4**，阶段 5（`apply_patch`）与 6-7（cherry-pick / revert）本次不做」。因此阶段 4 在范围内。

事实：`grep -rc 'commit_files' crates/rg-mcp/` = **0**；`rg-mcp/src/tools/mod.rs` 的工具表 14 项（`commit_file` `:111`、`delete_file` `:131`、`delete_branch` `:150` …）里没有它。而上报正文写「阶段 1-4 做完了」，并在「与方案有出入的 6 处」里**没有**这一条。

后果：issue 的原始诉求（`MCP 对应 commit_files`）仍空着——agent 侧依旧只能循环调 `commit_file`，一次改 N 个文件仍是 N 次提交、N 次 CI，服务端新端点对 MCP 客户端不可见。

## 5. 表所有权：`ref_limited_write_allowed`

`commits` 落进了 T21（路由元数据注册表，issue #14）声明要动的三张表之一。issue 评论 2 明令「别碰 `required_pat_scope` / `path_in_allowlist` / `ref_limited_write_allowed`」，它照做了一部分：前两张一行未动（`pat_scope.rs` diff 只 +3 行）。

裁决建议：**保留**。`CLAUDE.md` 现有的准入条件（「一条 REST 写路由只有在其处理函数自己检查分支后，才能加进 `ref_limited_write_allowed`」）已被满足（`repo_commits.rs:166`），不加这一行则带 `ref_allowlist` 的令牌对主用户恒 403。登记给 T21 的不是「回退」，而是「T21 改这三张表时不得删掉 `["commits"]`」。

## 6. P3 观察（未改码）

1. **幂等匹配臂比意图宽**：`idempotency.rs:76-88` 的 `["repos", _, _, "commits", ..]` 带 `..`，因此 `POST /repos/{o}/{n}/commits/{sha}/statuses`（`lib.rs:1474`）也一并被纳入幂等键支持，此前是 400 拒绝。行为上无害（幂等只是重放响应），但与「新路由进这张表前先确认响应里没有秘密」的口径一样，属于**未申报的放宽**。
2. **测试名与实现相反**：`a_dry_run_reports_and_writes_nothing` 断言的是「不写 commit、不动 ref」，而 `ops.rs:1488` 确实写出了 tree 对象（自述偏差 5 已承认）。名字里的 `writes nothing` 会被后来者读成「零对象写入」。
3. `idempotency.rs` 的注释列表与 `matches!` 的枚举现在是两处（`:59-63` 与 `:76-88`），加新段时容易只改一处——本次两者都改了。

## 7. 验证环境与输出

- host `Darwin arm64`，`cargo 1.99.1`，worktree `issue-16-manual`，`CARGO_TARGET_DIR` 指主工作区 `target`。
- 基线 `6aaf77f`（远端 tip 与本地 `main` 同 SHA，零漂移）：`bash scripts/check.sh --quick` → `Finished dev profile ... in 1m 37s`，`==> checks passed in 99s (quick)`，EXIT=0。
- 原子性证据（本机实跑，非引用它）：`cargo test -p rg-http --test batch_commit_tests` → `finished in 1m 01s` 编译、`running 10 tests ... test result: ok. 10 passed; 0 failed; 0 ignored`（1.08 s）。

## 8. 仍未做

- MCP `commit_files`（阶段 4）——见 §4，需单独落地。
- `apply_patch` 解析（阶段 5）、cherry-pick / revert（阶段 6-7）——issue 明确排除，`grep 'cherry-pick'` 全仓只剩 `ops.rs:1735` 的 rebase 测试造数据。
- CNB 上无绿灯：它自述 lint 流水线因组织 CPU 配额在 Prepare 阶段失败（四个真实步骤全 `skipped`），本次仍无 CI 证据，只有本机 clippy/fmt/测试。
