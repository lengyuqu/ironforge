# 服务端 cherry-pick / revert 端点验证（2026-11）

> **状态**：已实现，仅本机验证（**未经 CI**）｜ **可信度**：确认（命令与结果见下）｜ **来源**：仓库代码 / 本机运行结果 ｜ **对应**：CNB issue #48 第 3 项

## 做了什么

| 层 | 文件 | 内容 |
|----|------|------|
| git | `crates/rg-git/src/pick.rs`（新，`lib.rs` 加一行 `pub mod pick`） | `pick_commit(repo_path, &PickRequest) -> PickOutcome`：gix 三方树合并 + 新提交 + 带旧值守卫的 ref 更新。**没有改 `ops.rs`**。9 个单测，用 `git cherry-pick` 的结果对拍树 |
| 业务 | `crates/rg-core/src/repo/pick.rs`（新，`repo/mod.rs` 加一行） | `pick_commit(db, …)`：`check_direct_commit_allowed`（与 `contents` 写/批量提交同一个 `require_pr` / `require_signed_commits` 判定）→ `spawn_blocking` 调 rg-git |
| HTTP | `crates/rg-http/src/api/repo_pick.rs`（新） | `POST /repos/{o}/{n}/commits/{sha}/cherry-pick`、`…/revert`；两个处理函数各自调 `ensure_token_may_write_branch` |
| 登记 | `api/mod.rs`、`lib.rs`（路由）、`openapi.rs`（paths）、`rg_core::auth::route_meta::ROUTES` | 都是**追加**；两条路由 `.checked().idem()` |
| 测试 | `crates/rg-http/tests/cherry_pick_revert_tests.rs`（新，11 项） | 见下表 |

没有迁移、没有新增 SQL（只复用已有的审计、幂等键、事件日志表）。

## 决定

- **合并提交**：不带 `mainline` → 400 `MAINLINE_REQUIRED`（`details.parents`）；实现了 `mainline`（1 起算的父提交序号，git 的 `-m`）：非合并提交带 `mainline` 或越界 → 400 `MAINLINE_INVALID`。revert 带 `mainline` 时提交信息为 `This reverts commit <sha>, reversing\nchanges made to <parent>.`（与 git 一致）。
- **根提交**：允许。父树取空树：cherry-pick 根提交 = 把它的文件加到目标分支；revert 根提交 = 删掉它引入且未被改动的文件，有后续修改则正常报冲突。单测覆盖。
- **冲突**：409，`error.code = "CONFLICT"`，稳定机器码在 `error.reason = "MERGE_CONFLICT"`，`details = {operation, source_sha, tip_sha, paths[]}`，`retryable:false`。
- **不留半成品**：先在**内存对象库**（`Repository::with_object_memory`）里合并一次——冲突与 dry_run 因此**不写任何对象**（测试用 `git count-objects -v` 前后相等钉死）；只有干净且非 dry_run 才在真实库里重做合并、写提交、再用 `ExistingMustMatch(tip)` 更新 ref。ref 更新失败（写提交期间分支被别人推进）报 `REF_ADVANCED`，最多留下一个无 ref 指向的提交对象（同批量提交的既有行为）。
- **dry_run 的 `tree`**：在内存里算出，**不对应磁盘上的对象**；dry_run 响应里注明。
- **`expected_sha`**：必填；不匹配或分支不存在 → 409 `REF_ADVANCED`（`details.current_sha`/`expected_sha`，`retryable:true`），不会从 HEAD 兜底、不会建分支。
- **结果与目标树相同**（变更已存在/已不存在，例如重复 cherry-pick、重复 revert）→ 409 `NOTHING_TO_APPLY`，不产生空提交。
- **来源提交**：路径段只接受 4–64 位十六进制，不当作 revspec 解析（`HEAD`、`main~1` 都是 404）；对象必须是提交（blob/tag 对象 404）。允许仓库对象库里任何提交（不要求从某个分支可达）。
- **提交信息**：revert 为 `Revert "<首行>"` + 空行 + `This reverts commit <sha>.`；cherry-pick 为**原消息原样** + 空行 + `(cherry picked from commit <sha>)`（即 `git cherry-pick -x` 的写法，总是加）。可选字段 `message` 替换生成的主题/正文，标准尾行仍追加（任务要求之外的小增补，有测试）。
- **作者/提交者**：都是 `rg_core::agent::commit_identity(user)`（任务要求，与其他 REST 提交一致）。**与 git 不同**：git 的 cherry-pick 保留原作者；这里原作者不保留，来源只能从尾行的 sha 追溯。
- **幂等键**：支持（响应里没有秘密），重放返回首个响应、不重复提交（测试覆盖）。
- **权限**：写权限（`resolve_and_check_write_access`，agent 受所有者上限由 `can_write_repo` 保证）；受保护分支按 `check_direct_commit_allowed`（`require_pr` / 签名提交强制）拒绝，dry_run 同样被拒；分支白名单令牌经 `ensure_token_may_write_branch`，范围外 403。
- **成功后**：`announce_branch_move` → `spawn_post_push_hooks`（PR head 同步、CI、webhook、事件日志）；审计 `repo.cherry_pick` / `repo.revert`（`details`：`branch`、`source_sha`、`commit_sha`、`before`、`mainline`，`access_token_id` 由 `record_write` 合入）。

## 测试覆盖（`cherry_pick_revert_tests`，11 项）

| 验收项 | 测试 |
|--------|------|
| cherry-pick 成功（一个提交、父为旧 tip、信息/身份、push 事件、审计） | `a_cherry_pick_lands_as_one_commit_and_is_announced_and_audited` |
| revert 成功（信息、审计、自定义 `message`） | `a_revert_undoes_the_change_in_one_commit_and_is_audited` |
| cherry-pick 冲突 409，tip 不动、对象数不变、dry_run 同样 409、无审计 | `a_cherry_pick_conflict_is_a_409_…` |
| revert 冲突 409，tip 不动 | `a_revert_conflict_is_a_409_…` |
| 权限不足：匿名 401、非协作者 403、`require_pr` 403（含 dry_run），两个端点 | `a_pick_needs_write_access_and_respects_require_pr` |
| `expected_sha` 过期 / 分支不存在 → 409 `REF_ADVANCED` | `a_stale_expected_sha_is_a_409_…` |
| dry_run 不落 ref（`for-each-ref` 前后相同；`X-Dry-Run` 头同义） | `a_dry_run_reports_and_writes_no_ref` |
| 合并提交被拒、`mainline` 校验与成功路径 | `a_merge_commit_needs_a_mainline` |
| 分支受限令牌范围内成功 / 范围外 403、审计带 `access_token_id` | `a_branch_limited_token_picks_only_inside_its_limit` |
| 未知提交 404、无变化 409 | `an_unknown_commit_is_a_404_and_a_no_op_pick_is_a_409` |
| 幂等键重放 | `a_retried_idempotency_key_replays_instead_of_picking_twice` |

rg-git 单测（`pick::tests`，9 项）：对拍 `git cherry-pick` 的树、revert 信息格式、自定义信息、冲突不写对象、dry_run 与过期 tip、无变化、合并提交与 mainline、根提交、非提交来源。

## 验证命令与结果

全部在本机（macOS）、SQLite、**未经 CI**：

| 命令 | 结果 |
|------|------|
| `cargo clippy --workspace --all-targets -- -D warnings` | 干净（退出码 0） |
| `cargo test -p rg-git` | 70 通过，0 失败（含 `pick::tests` 9 项） |
| `cargo test -p rg-core` | 282 通过，0 失败 |
| `cargo test -p rg-http --no-fail-fast` | 782 通过，0 失败（含 `cherry_pick_revert_tests` 11 项，`route_metadata_tests` 9 项，`pat_scope_ladder_tests` 7 项，`openapi_registration_tests` 5 项） |
| `cargo fmt --check -p rg-git -p rg-core -p rg-http` | 干净 |

过程中的一次失败：第一轮 `cargo test -p rg-http` 在 `route_metadata_tests` 停下（3 项失败）。原因是该文件里冻结的「手写旧表」`legacy::branch_limited_write` 不认识两条新路由（注册表说分支受限令牌可写，旧表说不行，测试拒绝比旧表更宽松）。已在该表追加 `["commits", _, "cherry-pick"]` / `["commits", _, "revert"]` 两行（带注释，**这是对 R1 测试夹具的一处改动**，其余测试文件未动）；`cargo test` 在首个失败目标处就停，所以之后改用 `--no-fail-fast` 跑完整个 `rg-http`。

**未覆盖**：PostgreSQL / MySQL（没有新增 SQL，只复用已有表；`cherry_pick_revert_tests` 用了 `spawn_test_app_on_env_db_with_root`，设 `IRONFORGE_TEST_DATABASE_URL` 可跑，但**本次没跑**，也不在 `scripts/ci-db-tests.sh` 清单里）；CI（没有可用环境）；真实反向代理/浏览器；大仓库上的合并耗时。

## 没做（范围外）

- MCP 工具 `cherry_pick` / `revert`（后续：REST 已就绪，MCP 封装需要同步工具表、`dispatch`、`tools/list` 快照测试和 `capabilities` 注册表）。
- unified diff 输入、批量 pick 多个提交、`--no-commit`、保留原作者、重命名检测参数。
- 任何迁移。

## 未决

1. **建议补进 `.ai/guardrails.md`**（本次按要求没改）：把「新增 REST 写路径（会移动分支 tip：cherry-pick、revert、批量提交、`contents` 写）」列为 🟠 CONFIRM，并写明必须同时具备：目标分支的 `check_direct_commit_allowed`、`ensure_token_may_write_branch`、`expected_sha` 守卫、审计、post-push hooks。现在 guardrails 里没有 cherry-pick/revert 条目，「🟠 CONFIRM」只是 issue 正文的判读。
2. **作者语义**：cherry-pick 不保留原作者（遵照任务要求）；若产品希望保留，需要在 `commit_identity` 之外另给 author 并在 committer 里记录操作者，是行为变更，待负责人决定。
3. **并发/资源**：合并在 `spawn_blocking` 里跑，没有套 `run_git_read` 的读并发许可（与批量提交一致）；大树上的 CPU 占用没有压测，也没有上限。
4. **可从任何提交取源**：来源只要求是仓库对象库里的提交，包括不可达提交（如已删除分支的残留）。若要限制为「从某个现存 ref 可达」，需要一次可达性遍历，代价未评估。
5. `PickOutcome::Planned.tree` 在 dry_run 里只是一个算出来的 id；调用方不能拿它去读对象。
