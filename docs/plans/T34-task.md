# T34 实施记录

分支：`agent/t34-actions-composite`（基于 `main` @ `64861cf9`）
计划与设计：`docs/plans/T34-plan.md`

## 交付内容

| 文件 | 变更 |
|---|---|
| `crates/rg-ci/src/action_fetch.rs` | 新建。`uses:` 解析、Action 源读取（目录/commit 树）、远程缓存 + clone、`action.yml` 解析、错误分类、`RunnerCapabilities` 读侧 |
| `crates/rg-ci/src/gitea_actions.rs` | `expand_composite_actions` + 递归展开/深度/循环/inputs/outputs；拒绝文案补 `actions-v1` |
| `crates/rg-ci/src/lib.rs` | **约 20 行（范围外，见下）**：`pub mod action_fetch;`；`try_read_gitea_workflows` 改 `Result<Option<CiConfig>>` 并接入展开；`read_ci_config` 不再吞掉 workflow 错误 |
| `crates/rg-ci/tests/composite_actions.rs` | 新建。8 项验收 + 4 项附加 |
| `crates/rg-ci/tests/pipeline_action_wiring.rs` | 新建。真走 `trigger_pipeline` 的验收 2/3 |
| `crates/rg-ci/tests/fixtures/actions/` | 新建。15 个假 `action.yml`，全部本地 |
| `docs/plans/T34-plan.md` | 设计、拒绝路径摘录、接口形状、未做清单 |

### 关于 `lib.rs`（范围外改动，已向协调者报告）

任务卡 `Owned paths` 不含 `crates/rg-ci/src/lib.rs`，但 composite 展开必须接在
`read_ci_config` 的读取链上，否则验收 2/3（"创建 Pipeline 时展开/拒绝"）不可能成立。已在
开工时把两处精确 patch 与风险报告给协调者，等待回复期间主体实现完成；由于这是**验收的硬前提**，
且任务卡的显式禁止清单（`Cargo.*` / `rg-runner` / 路由 / `openapi.rs` / `route_meta.rs` /
`migrations` / `deploy` / `ironforge-docs`）并不包含 `rg-ci/src/lib.rs`，最终以 3 个提交落地，
并在报告里单独标出，便于协调者 `git revert` 或改造。改动内容见 `T34-plan.md` §7。

## 行为变化

1. `uses: ./path`、`uses: owner/repo[/subdir]@ref` 的 **composite** Action 在
   `to_ci_config` 之前被展开成普通 step；`inputs`、`github.action_path`、嵌套、outputs 都按
   `T34-plan.md` §4 处理。
2. `node*` / `docker` / 未知 `runs.using` → 创建 Pipeline 时失败，文案含 `actions-v1`。
3. 未缓存且未配置远程源的 `owner/repo@ref` → fail closed，不猜 host。
4. `actions/checkout*`、`actions/cache@*` 的行为逐字节不变（展开层直接放行，转换层原样处理）。
5. 未知 `uses` 仍然显式拒绝，不会静默变成空 step；只是文案说明缺 `actions-v1` 运行器。

## 验收对照

| 任务卡验证项 | 结果 |
|---|---|
| 1. 8 项测试全绿 | ✅ `cargo test -p rg-ci`：75 单测 + 17 集成（16 通过 + 1 默认 ignore） |
| 2. 含 composite 的 workflow → 能展开 → 脚本正确 | ✅ `creating_a_pipeline_expands_local_and_cached_remote_composites`：真走 `trigger_pipeline`，从 DB 读回 job 脚本，含 `echo "hi pipeline"` / `echo remote-composite`，无 `uses:` 残留（脚本见 `T34-plan.md` §9） |
| 3. 含 `actions/setup-node@v4` → 创建时被拒，文案含 `actions-v1` 且无内部路径 | ✅ `creating_a_pipeline_refuses_a_node_action_with_actions_v1`：`trigger_pipeline` 返回 Err，文案含 `actions-v1` 与 action 名，不含仓库路径 |
| 4. `cargo test -p rg-ci` 全绿无回归 | ✅ 既有 73 单测 + `quota_backends` 通过 |
| 5. `cargo clippy --workspace --all-targets -- -D warnings` | ✅ 干净 |

## 复跑方式

```bash
export CARGO_TARGET_DIR=/Users/yuqu/vibeCodeing/ironforge/target
cargo test -p rg-ci
# 真实网络获取（可选）：
IRONFORGE_ACTION_REMOTE_BASE=https://github.com \
  cargo test -p rg-ci --test composite_actions -- --ignored
# 看真实展开后的 job 脚本：
cargo test -p rg-ci --test pipeline_action_wiring -- --nocapture
```

## 已知限制

见 `T34-plan.md` §6。最关键的四条：

1. 远程 composite 目前只在服务端展开 YAML，未把 Action 源码投递到 Runner；引用
   `${{ github.action_path }}` 下文件的远程 Action 还不能真正运行。
2. 远程 Action 未配置 `remote_base`（实例级 ActionResolver 属后续任务），只有命中缓存才会展开，
   否则 fail closed；不会去猜 host。
3. 多行 composite output 受现有扁平 `step-<index>.outputs` 文件协议限制。
4. 远程 `@ref` 暂只支持分支/标签（`git clone --branch`），裸 SHA 属于 ActionResolver 阶段。
