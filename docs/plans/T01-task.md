# T01 约束与风险确认

## 约束确认

| 约束 | 结论 |
| --- | --- |
| 只改 owned paths | 只改 `crates/rg-core/src/pull_request/service.rs`，新增 `crates/rg-http/tests/merge_target_tests.rs` |
| 先复现再修 | 复现输出（修复前 2 个测试失败）已原样记入 `docs/plans/T01-plan.md` §3，时间早于实现改动 |
| 不动 `merge_gates` / 前置校验 / `update_branch` / PR head 同步 | 未改；`merge_gates`、`update_branch` 相关代码零 diff |
| 不动路由注册、`openapi.rs`、`route_meta.rs` | 未改 |
| 不动 `Cargo.toml` / `Cargo.lock` | 未改（无新依赖） |
| 不动迁移、`.cnb.yml`、`.github/`、`scripts/`、`deploy/` | 未改 |
| 不动 `CLAUDE.md` / `ironforge-docs/` | 未改 |
| 合并目标按 `pr.base_branch` | `merge` / `squash` 的 ours 与提交落点均为 `refs/heads/<pr.base_branch>` |
| fork PR 路径同步 | `merge_from_ref` 的 `Merge` / `Squash` 分支均传 `pr.base_branch` |
| 不 push main | 只推 `agent/t01-merge-target` |

## 风险与说明

1. **`HEAD` 不再参与合并写入**：裸仓 `HEAD` 仍用于初始化、默认分支推导与 `get_head_sha`，语义未变。合并原语现在完全按 `refs/heads/<base_branch>` 读写；base 等于默认分支时行为与旧版等价。
2. **base 分支被删除**：新增 `ensure_branch_exists(base)` 对所有策略生效（此前仅 `rebase`）。缺失 base 现在返回 409 状态冲突，而不是 `rev_parse` 失败导致的 500。这是本次改动的必要配套，未触碰其它校验。
3. **并发写 base**：`commit_as` 以 `PreviousValue::Any` 更新 base ref（与旧版写 `HEAD` 相同的原子性级别），未引入 CAS；并发合并同一 PR 仍由上层 claim/PR 状态机约束，本任务不改变该语义。
4. **fork PR 未新增端到端测试**：HTTP 测试需构造双仓库 fork，成本高且与本次根因无关；fork 路径与同仓路径共用 `merge_from_ref` → 同一组原语，已通过代码路径覆盖。同仓 `merge` / `squash` 有端到端回归。
5. **跨后端**：未触碰 SQL / 迁移，只跑 SQLite 即可（`CLAUDE.md` 第 19/23 条不适用）。

## 验收结果

| 命令 | 结果 |
| --- | --- |
| `cargo test -p rg-http --test merge_target_tests` | 2 passed, 0 failed |
| `cargo clippy --workspace --all-targets -- -D warnings` | 通过（Finished dev profile） |
| `scripts/check.sh --quick` | 通过（fmt + clippy，12s） |
