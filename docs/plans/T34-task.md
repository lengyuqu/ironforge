# T34 实施记录

分支：`agent/t34-actions-composite`（基于 `main` @ `64861cf9`）
计划与设计：`docs/plans/T34-plan.md`

## 提交

| SHA | 内容 |
|---|---|
| `6a07a933` | `action_fetch.rs`、composite 展开、node/docker fail closed、8 项测试 |
| `7962d0bd` | 展开原子化、subdir 缓存、`INPUT_*` 安全值 |
| `ad9ae796` | clone 失败分类（network vs ref-not-found） |
| `7fac5365` | **`lib.rs` 第 2 处**：`NoWorkflows` + 错误冒泡（既有缺陷修复，独立 commit） |
| （本提交） | **`lib.rs` 第 1 处**：接入 `expand_composite_actions` + 端到端测试 + 文档 |

## 交付内容

| 文件 | 变更 |
|---|---|
| `crates/rg-ci/src/action_fetch.rs` | 新建。`uses:` 解析、Action 源读取（目录/commit 树）、远程缓存 + clone、`action.yml` 解析、错误分类、`RunnerCapabilities` 读侧 |
| `crates/rg-ci/src/gitea_actions.rs` | `expand_composite_actions` + 递归展开/深度/循环/inputs/outputs；拒绝文案补 `actions-v1` |
| `crates/rg-ci/src/lib.rs` | **约 55 行（范围外，已授权）**：模块声明、`NoWorkflows`、`read_ci_config` 错误传播、展开接线、缓存落点判断 |
| `crates/rg-ci/tests/composite_actions.rs` | 新建。8 项验收 + 6 项附加（14 个，1 个默认 ignore） |
| `crates/rg-ci/tests/pipeline_action_wiring.rs` | 新建。真走 `trigger_pipeline` 的验收 2/3 |
| `crates/rg-ci/tests/fixtures/actions/` | 新建。15 个假 `action.yml`，全部本地 |
| `docs/plans/T34-plan.md` | 设计、拒绝路径摘录、接口形状、未做清单 |

### `lib.rs`（范围外，协调者已按 A 方案授权）

- 第 1 处（本提交）：`try_read_gitea_workflows` 在 `expand_local_reusable_workflows` 之后、
  `validate_supported_actions` 之前调用 `expand_composite_actions`；fetcher 用
  `repo.path()` + `commit_sha`，缓存落在裸仓库**外面**（`<data>/repos/<owner>/.ironforge/actions`），
  非裸 checkout 则落在 `.git/.ironforge/actions`（`git status` 不可见）。
- 第 2 处（`7fac5365`，独立 commit）：`NoWorkflows` 标记错误 + `downcast` 判别；只有「目录不存在」
  与「没有匹配的 workflow」回退到 `.ironforge-ci.yml`；parse / 展开 / 校验失败一律冒泡。
- 同步修正 `read_ci_config` 的 doc comment（原文自认 "errors in step 1 are swallowed"）。

## 行为变化

1. `uses: ./path`、`uses: owner/repo[/subdir]@ref` 的 **composite** Action 在
   `to_ci_config` 之前被展开成普通 step；`inputs`、`github.action_path`、嵌套、outputs 都按
   `T34-plan.md` §4 处理。
2. `node*` / `docker` / 未知 `runs.using` → 创建 Pipeline 时失败，文案含 `actions-v1`。
3. 未缓存且未配置远程源的 `owner/repo@ref` → fail closed，不猜 host。
4. `actions/checkout*`、`actions/cache@*` 的行为不变（展开层直接放行，转换层原样处理）。
5. 未知 `uses` 仍然显式拒绝；现在这个错误能真正到达用户，而不是退化成 "no CI config found"。
6. `.gitea/workflows` 存在但内容有问题（parse/展开/校验失败）时，不再静默回退到
   `.ironforge-ci.yml`——fail closed 优于静默降级。

## 验收对照

| 任务卡验证项 | 结果 |
|---|---|
| 1. 8 项测试全绿 | ✅ `cargo test -p rg-ci`：79 单测 + 16 集成（15 通过 + 1 默认 ignore） |
| 2. 含 composite 的 workflow → 能展开 → 脚本正确 | ✅ `creating_a_pipeline_expands_local_and_cached_remote_composites`：真走 `trigger_pipeline`，从 DB 读回 job 脚本并打印 |
| 3. 含 `actions/setup-node@v4` → 创建时被拒，文案含 `actions-v1` 且无内部路径 | ✅ `creating_a_pipeline_refuses_a_node_action_with_actions_v1`：文案含 `actions-v1`，且不含 `/Users/` `/private/` `/var/folders/` `ironforge-t34` `.ironforge/actions` |
| 4. `cargo test -p rg-ci` 全绿无回归 | ✅ 既有 73 单测与 `quota_backends` 通过 |
| 5. `cargo clippy --workspace --all-targets -- -D warnings` | ✅ 干净 |

`lib.rs` 改造另有 4 个回归单测（坏 workflow 不再变成 "no CI config found"、纯 parse 失败报错、
坏文件旁边好文件仍运行、原生配置仅在没有可用 workflow 时使用）。

## 复跑方式

```bash
export CARGO_TARGET_DIR=/Users/yuqu/vibeCodeing/ironforge/target
cargo test -p rg-ci
# 看真实展开后的 job 脚本：
cargo test -p rg-ci --test pipeline_action_wiring -- --nocapture
# 真实网络获取（可选）：
IRONFORGE_ACTION_REMOTE_BASE=https://github.com \
  cargo test -p rg-ci --test composite_actions -- --ignored
```

## 已知限制

见 `T34-plan.md` §6。最关键的四条：

1. 远程 composite 目前只在服务端展开 YAML，未把 Action 源码投递到 Runner；引用
   `${{ github.action_path }}` 下文件的远程 Action 还不能真正运行。
2. 远程 Action 未配置 `remote_base`（实例级 ActionResolver 属后续任务），只有命中缓存才会展开，
   否则 fail closed；不会去猜 host。
3. 多行 composite output 受现有扁平 `step-<index>.outputs` 文件协议限制。
4. 远程 `@ref` 暂只支持分支/标签（`git clone --branch`），裸 SHA 属于 ActionResolver 阶段。
