# T34 实施记录

分支：`agent/t34-actions-composite`（基于 `main` @ `64861cf9`）
计划与设计：`docs/plans/T34-plan.md`

## 交付内容

| 文件 | 变更 |
|---|---|
| `crates/rg-ci/src/action_fetch.rs` | 新建。`uses:` 解析、Action 源读取（目录/commit 树）、远程缓存 + clone、`action.yml` 解析、错误分类、`RunnerCapabilities` 读侧 |
| `crates/rg-ci/src/gitea_actions.rs` | `expand_composite_actions` + 递归展开/深度/循环/inputs/outputs；拒绝文案补 `actions-v1` |
| `crates/rg-ci/src/lib.rs` | **1 行**：`pub mod action_fetch;`（模块声明，见"待裁决"） |
| `crates/rg-ci/tests/composite_actions.rs` | 新建。8 项验收 + 4 项附加 |
| `crates/rg-ci/tests/fixtures/actions/` | 新建。15 个假 `action.yml`，全部本地 |
| `docs/plans/T34-plan.md` | 设计、拒绝路径摘录、接口形状、未做清单 |

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
| 1. 8 项测试全绿 | ✅ `cargo test -p rg-ci`：13 个集成测试（12 通过 + 1 默认 ignore）、73 个单测通过 |
| 2. 含 composite 的 workflow → 能展开 → 脚本正确 | ⚠️ 已在 `expand_composite_actions` + 真实 commit 树这一层跑通并打印脚本（`a_local_composite_is_expanded_from_the_workflow_commit`）；"创建 Pipeline"的入口接线待裁决（见下） |
| 3. 含 `actions/setup-node@v4` → 创建时被拒，文案含 `actions-v1` 且无内部路径 | ⚠️ 拒绝语义与文案已就位（`validate_supported_actions` 与展开层两处），但 `read_ci_config` 目前会吞掉该错误；需要裁决里的第 2 处接线 |
| 4. `cargo test -p rg-ci` 全绿无回归 | ✅ |
| 5. `cargo clippy --workspace --all-targets -- -D warnings` | ✅（`-p rg-ci --all-targets` 干净；workspace 全量见收尾记录） |

## 待裁决：`lib.rs` 的两处接线

任务卡 `Owned paths` 不含 `crates/rg-ci/src/lib.rs`，但 composite 展开必须接在
`read_ci_config` 的读取链上。已向协调者报告（消息含精确 diff 位置），等待选择：

- **A**：授权本任务在 `lib.rs` 做两处小改（约 10 行）——推荐；
- **B**：协调者/其他 agent 打这个 patch；
- **C**：验收只在 `validate_supported_actions` 层面成立，端到端样例以 `expand_composite_actions`
  的 commit 树测试为准。

在裁决前，本分支**只**多了 `pub mod action_fetch;` 一行声明（新建文件必须在某处声明模块；
若不允许改 `lib.rs`，可退化为 `gitea_actions.rs` 内的 `#[path = "action_fetch.rs"] pub mod action_fetch;`）。

## 复跑方式

```bash
export CARGO_TARGET_DIR=/Users/yuqu/vibeCodeing/ironforge/target
cargo test -p rg-ci
# 真实网络获取（可选）：
IRONFORGE_ACTION_REMOTE_BASE=https://github.com \
  cargo test -p rg-ci --test composite_actions -- --ignored
```

## 已知限制

见 `T34-plan.md` §6。最关键的三条：

1. 远程 composite 目前只在服务端展开 YAML，未把 Action 源码投递到 Runner；引用
   `${{ github.action_path }}` 下文件的远程 Action 还不能真正运行。
2. 多行 composite output 受现有扁平 `step-<index>.outputs` 文件协议限制。
3. 远程 `@ref` 暂只支持分支/标签（`git clone --branch`），裸 SHA 属于 ActionResolver 阶段。
