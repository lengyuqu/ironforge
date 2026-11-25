# T34 实施计划：第三方 Actions 的 composite 展开与能力协商

> 对应任务卡：`tasks/T34.md`。冻结前提：`ironforge-docs/ci/adr-0001-third-party-actions-execution.md`（Accepted）。
> 基线：`main` @ `64861cf9`，分支 `agent/t34-actions-composite`。

## 1. 现状与拒绝路径（基线代码）

### 1.1 `uses:` 的判定与拒绝发生在**创建 Pipeline 时**

`crates/rg-ci/src/lib.rs` 的 `read_ci_config` 先尝试 `.gitea/workflows/*.yml`，其中逐文件调用
`expand_local_reusable_workflows` → `validate_supported_actions` → `to_ci_config`：

```
$ git show 64861cf9:crates/rg-ci/src/lib.rs | sed -n '712,730p'
        let workflow = workflow
            .expand_local_reusable_workflows(&workflow_sources)
            .with_context(|| format!("failed to expand .gitea/workflows/{name}"))?;
        workflow
            .validate_supported_actions()
            .with_context(|| format!("unsupported workflow .gitea/workflows/{name}"))?;
```

判定点本身（基线）：

```
$ git show 64861cf9:crates/rg-ci/src/gitea_actions.rs | sed -n '244,259p'
    pub fn validate_supported_actions(&self) -> Result<()> {
        let mut unsupported = self
            .jobs
            .iter()
            .flat_map(|(job_name, job)| {
                job.steps.iter().filter_map(move |step| {
                    step.uses
                        .as_deref()
                        .filter(|uses| {
                            !uses.starts_with("actions/checkout@")
                                && !uses.starts_with("actions/cache@")
                        })
                        .map(|uses| format!("{job_name}: {uses}"))
                })
            })
```

用户看到的原文：

```
$ git show 64861cf9:crates/rg-ci/src/gitea_actions.rs | sed -n '279,286p'
        if unsupported.is_empty() {
            Ok(())
        } else {
            anyhow::bail!(
                "unsupported action step(s): {}. Convert them to run: commands or use .ironforge-ci.yml",
                unsupported.join(", ")
            )
        }
```

转换层还有一层兜底，避免绕过校验的直接调用者把未知 Action 静默变成空 step：

```
$ git show 64861cf9:crates/rg-ci/src/gitea_actions.rs | sed -n '475,482p'
                    // Direct callers should still fail visibly even if they
                    // skipped `validate_supported_actions`.
                    script.push(format!(
                        "echo \"IronForge does not support action '{}'; use run: or .ironforge-ci.yml\" >&2; exit 78",
                        uses
                    ));
                    continue;
```

### 1.2 `uses` 的形态解析：基线**没有**解析

基线只做两个 `starts_with` 前缀判断，`owner/repo@ref`、`./local/path`、`docker://image`
全部落入同一个 "unsupported" 分支。这正是本轮补的第一件事：先解析成四种形态，再按形态决定行为。

### 1.3 一个既有缺陷（影响验收 3）

```
$ git show 64861cf9:crates/rg-ci/src/lib.rs | sed -n '619,623p'

    // Try Gitea Actions format first
    if let Ok(config) = try_read_gitea_workflows(&repo, commit_sha, ref_name, event) {
        tracing::info!("Using Gitea Actions workflow from .gitea/workflows/");
        return Ok(config);
    }
```

`if let Ok(...)` 把 `unsupported workflow ...` 一并吞掉，随后回退到 `.ironforge-ci.yml`；如果
仓库没有原生配置，用户看到的是 `no CI config found`，而不是真正的拒绝原因。**本轮不在这里
扩大范围**，见 §7 的接线提案。

## 2. 模块划分

| 文件 | 职责 |
|---|---|
| `crates/rg-ci/src/action_fetch.rs`（新建） | `uses:` 解析、Action 源读取（目录 / git commit）、远程缓存与 clone、`action.yml` 解析、错误分类、能力读侧类型 |
| `crates/rg-ci/src/gitea_actions.rs` | 在既有 workflow 模型上做 composite 展开（纯 YAML→YAML）、输出映射、拒绝文案 |
| `crates/rg-ci/tests/composite_actions.rs`（新建） | 8 项验收 + 输出映射 + commit 树 + 默认 `#[ignore]` 的真实网络获取 |
| `crates/rg-ci/tests/fixtures/actions/`（新建） | 本地假 `action.yml`，不依赖网络 |

没有新依赖：YAML 走 `rg_core::yaml`（`serde-saphyr`），git 读取与 clone 走已有的 `git` CLI
（`rg-git` 的 `cli_gateway` 已采用同一策略）。

## 3. 获取层（`action_fetch.rs`）

### 3.1 四种 `uses` 形态

```rust
pub enum ActionReference {
    Builtin { uses: String },                                  // actions/checkout*, actions/cache@*
    Local { path: String },                                    // ./path
    Remote { owner, repo, subdir, git_ref },                   // owner/repo[/subdir]@ref
    Docker { image: String },                                  // docker://image
}
```

- 前缀判定与转换层完全一致（`starts_with("actions/checkout")` / `starts_with("actions/cache@")`），
  所以既有 workflow 的语义不变。
- 没有 `@ref` 的远程引用是**错误**，不是 "unknown action"；`..`、绝对路径、以 `-` 开头的 ref
  在进入文件系统或命令行之前被拒绝（防路径穿越与选项注入）。
- `docker://` 直接返回 `Unsupported`，文案含 `actions-v1`。

### 3.2 源读取抽象

```rust
pub trait ActionFileSource: Send + Sync {
    fn read(&self, relative: &str) -> Result<Option<Vec<u8>>, ActionError>;
    fn root(&self) -> Box<dyn ActionFileSource>;
    fn location(&self) -> String;
    fn identity(&self) -> String;
}
```

两个实现：

- `DirectorySource`：测试 fixture、远程缓存、checkout 目录。写入不存在时返回 `None`；
  读取后 `canonicalize` 并校验仍在 root 内（防 symlink 越界，对应 ADR §2.3.4）。
- `GitTreeSource`：服务端持有的是裸仓库，没有工作树。用 `git -C <repo> show <commit>:<path>`
  从**同一个不可变 commit** 读取 `action.yml`，与 workflow 内容同源（ADR §2.3.3 "执行前解析为
  不可变 commit SHA"）。

`root()` 的用途：仓库内本地 Action 的嵌套 `./x` 按**仓库根**解析（GitHub 对本地 Action 的语义），
远程 Action 的嵌套 `./x` 按其 checkout 根解析。

### 3.3 缓存与 clone

- 路径：`{cache_root}/{owner}/{repo}/{ref}/`，`ref` 允许 `/`（分支名），`..` 已在解析阶段拒绝。
- 命中判据：目录存在 **且** 有 `action.yml` 或 `action.yaml`（与 GitHub 判定 checkout 后 Action 的
  方式一致）。
- 未命中：`git clone --quiet --depth 1 --single-branch --branch <ref> <base>/<owner>/<repo>.git`
  到 `.tmp/<unique>`，成功后 `rename` 到缓存路径；并发输者直接复用赢家的副本。
- **未配置 remote base 时 fail closed**，不猜 host（ADR §2.3.1 要求实例级 allowlist）。
- 失败分类（可区分，验收要求）：

| stderr 特征 | `ActionErrorKind` | 文案 |
|---|---|---|
| `could not resolve host` / `unable to access` / `connection refused` / `timed out` | `Network` | "could not reach the source ...: network error; retry when the remote is reachable" |
| `remote branch ... not found` / `couldn't find remote ref` | `RefNotFound` | "... does not exist: ref '<ref>' was not found" |
| 其他 | `Fetch` | "failed to fetch action ..." |

错误里只出现**用户写的引用**（`owner/repo@ref`），不出现缓存绝对路径或克隆 URL。

### 3.4 `action.yml` 解析

```rust
pub enum ActionRuns {
    Composite { steps: Vec<GiteaStep> },  // 可展开
    Node { using: String },               // node16/node20/node24
    Docker { image: String },
    Other { using: String },              // go / 插件 / 未知
}
```

复用 `GiteaStep` 解析 `runs.steps`：字段（`id`/`name`/`uses`/`run`/`shell`/`env`/`if`/`with`）
与 workflow step 完全一致，不需要第二套模型。`inputs.<name>.default` 允许标量（数字/布尔按
字符串处理）。

## 4. 展开规则（`gitea_actions.rs`）

入口：

```rust
impl GiteaWorkflow {
    pub fn expand_composite_actions(&mut self, fetcher: &ActionFetcher) -> Result<()>
}
```

对每个 job 的 step 顺序遍历：

1. 无 `uses` → 原样保留。
2. `actions/checkout*`、`actions/cache@*` → 原样保留（内置适配不变，**每一层**都跳过）。
3. 其他 `uses` → `ActionFetcher::resolve`（嵌套时用 `resolve_within(parent_source, ...)`）。
4. 深度检查：workflow 直接调用算深度 1，`depth > 5` 时报 `DepthLimit`。第 6 层失败（验收 4）。
5. 循环检查：栈里存 `(identity, uses)`；`identity` 是源目录的 canonical 路径 / `commit:path`，
   所以两条不同路径指向同一个目录也算同一个 Action。A→B→A 报 `Cycle`，不是 panic（验收 3）。
6. 按 `runs` 分流：
   - `Composite`：把 `runs.steps` 逐个实例化后递归展开，**原位替换**调用 step，顺序保持；
   - `Node` / `Docker` / `Other`：`Unsupported`，文案里出现 `actions-v1`；
   - 其余不可能（`Composite` 之外的三种已覆盖）。
7. 所有子 step 展开完成后，若调用 step 有 `id` 且 Action 声明了 `outputs`，追加一个**发布 step**
   （保留调用者的 `id`），把输出写回调用者的 `$GITHUB_OUTPUT`。

### 4.1 实例化（inputs / env / 条件）

```rust
struct InstantiatedAction<'a> {
    inputs: HashMap<String, String>,   // manifest 默认值，被调用者 with 覆盖
    action_path: &'a str,              // 替换 ${{ github.action_path }}
    caller: &'a GiteaStep,
    id_prefix: Option<String>,         // "<caller id>." —— 内层 id 命名空间
}
```

- 替换发生在子 step 的 `run`、`env` 值、`with` 值里：
  `${{ inputs.x }}`、`${{inputs.x}}`、`${{ inputs['x'] }}`、`${{ inputs["x"] }}`、
  `${{ github.action_path }}`。未提供的 input 解析为空串（GitHub 语义）。
- 同时注入 `INPUT_<NAME>` 环境变量（GitHub 也这么做），保留 shell 风格访问。
- `id`：调用者有 `id` 时，内层 id 变成 `<caller id>.<inner id>`。Gitea 的 step id 作用域是 job，
  同一个 Action 被调用两次会撞 id；命名空间化让输出映射和后续引用都能唯一解析。这是本实现
  相对 GitHub 的一处**有意偏离**，只影响展开后的内部 id，不影响 workflow 可见的
  `steps.<caller>.outputs.*`。
- `if`：调用者与子 step 的条件用 `({parent}) && ({child})` 合并（现有条件求值器支持 `&&` 和括号）。
- `env`：先继承调用者 step 的 env，再被子 step 的 env 覆盖。

### 4.2 `github.action_path`

- 本地 Action：`/workspace/<相对路径>`。`/workspace` 是容器 job 的工作区
  （`crates/rg-ci/src/runner.rs:669-678`、`crates/rg-runner/src/main.rs:719-720`），所以在容器里
  该路径真实存在。
- 远程 Action：解析时的缓存目录。**注意**：本轮没有把远程 Action 的源码投递到 Runner，
  见 §6/§7。

### 4.3 `outputs` 映射

`to_ci_config` 已经按 **step 在列表中的下标** 给带 `id` 的 step 分配
`.ironforge-outputs/step-<index>.outputs`。展开发生在 `to_ci_config` 之前，且不改变顺序，因此
展开结束时每个 step 的最终下标是确定的。

发布 step 的脚本（由 `apply_output_mappings` 生成，`index` 是子 step 的最终下标）：

```bash
printf '%s\n' 'version<<IRONFORGE_COMPOSITE_OUTPUT' >> "$GITHUB_OUTPUT"
sed -n 's/^version=//p' ".ironforge-outputs/step-0.outputs" >> "$GITHUB_OUTPUT"
printf '%s\n' 'IRONFORGE_COMPOSITE_OUTPUT' >> "$GITHUB_OUTPUT"
```

- 只支持 `value: ${{ steps.<id>.outputs.<name> }}`；其他表达式 fail closed（`ManifestInvalid`）。
- 输出名/输入名限定为 `[A-Za-z0-9_.-]`，避免生成脚本被注入。
- 用 heredoc 形式写回，值里的换行不会被截断；但**子 step 的输出本身若是 heredoc 形式**
  （多行值），`sed 's/^name=//p'` 取不到——这是当前扁平执行模型的已知限制，见 §6。
- 调用 step 没有 `id` 时不生成发布 step（没有任何东西能引用它的输出）。

## 5. 能力协商：接口形状与当前落点

### 5.1 现状（复核结论）

`runners` 表只有 `name/token/status/labels/last_seen_at/version/os/arch`
（`crates/rg-db/src/entities/runner.rs:8-21`），注册请求体是 `labels/os/arch`
（`crates/rg-http/src/api/runners.rs:36-39`，`crates/rg-runner/src/main.rs:154-169`）。
**不存在结构化的 capability 集合**，也没有 `actions-v1` 这个值。

按任务卡 §Notes 的裁决："只做到 rg-ci 侧校验 + 读侧解析，runner 侧上报留给后续任务"。因此本轮：

- 读侧接口（`action_fetch.rs`）：

```rust
pub const ACTIONS_V1_CAPABILITY: &str = "actions-v1";

pub struct RunnerCapabilities { pub actions_v1: bool }
impl RunnerCapabilities {
    /// 从现有 `labels` 数组解释能力：不需要迁移、不需要新字段。
    pub fn from_labels<I, S>(labels: I) -> Self;

    /// node/docker/未知 runtime → true；composite → false。
    pub fn required_capabilities(&self) -> RunnerCapabilities;  // ActionManifest 上
}
```

- 调度侧（本轮**不**改）：因为没有任何 Runner 能声明 `actions-v1`，含 node/docker Action 的
  workflow 在**创建 Pipeline 时**就被拒绝（fail closed），不会出现"调度到不满足能力的 Runner"。
  一旦 `actions-v1` Runner 出现，调度器可以用 `RunnerCapabilities::from_labels(runner.labels)`
  与 `manifest.required_capabilities()` 做交集判断。
- composite 展开后的 step 是普通 `run` step，**不需要** `actions-v1`——这是本轮的核心收益。
- 仍然保留"未知 `uses` 显式拒绝"的语义，只是文案改为说明缺少 `actions-v1`。

## 6. 明确未做（及原因）

| 项 | 原因 |
|---|---|
| 执行 node / docker Action | ADR-0001 §2.4 / §2.5：需要 Job 容器、临时网络、资源上限、`cap-drop` 等基线；本轮只做转换层，继续 fail closed |
| Runner 侧能力上报 / 调度器匹配 | `runners` 没有 capability 列，改动会碰 `rg-db` 迁移与 `rg-runner`；任务卡明确留给后续任务 |
| 远程 Action 源码投递到 Runner | 远程 composite 目前只在服务端展开 YAML；若 Action 引用 `${{ github.action_path }}` 下的脚本，runner 侧没有该目录。需要 actions-v1 的 materialize 阶段 |
| Secret 按引用收敛、`CI_JOB_TOKEN` 权限收敛 | ADR §2.7，另开任务 |
| 多行 composite output | 现有扁平执行器只提供 `step-<index>.outputs` 文件；heredoc 值需要结构化 step 执行（CI-202 stage B） |
| 远程 ref 为裸 commit SHA | `git clone --branch` 只接受分支/标签。SHA 固定属于 ActionResolver 阶段（ADR §2.3.3） |
| 递归拉取 submodule | ADR §2.3.5：默认不递归，需显式策略 |

## 7. 接线（需要协调者裁决）

`Owned paths` 不含 `crates/rg-ci/src/lib.rs`，但展开必须接在读取链上。本分支只改了 1 行
声明（`pub mod action_fetch;`）。要满足验收 2/3 还需要两处：

```rust
// 1) try_read_gitea_workflows（lib.rs:712 附近）
let mut workflow = workflow.expand_local_reusable_workflows(&workflow_sources)?;
let fetcher = gitea_actions::ActionFetcher::for_git_commit(
    repo.path(),
    commit_sha,
    repo.path().join(".ironforge/actions"),
);
workflow.expand_composite_actions(&fetcher)?;
workflow.validate_supported_actions()?;

// 2) read_ci_config（lib.rs:620）
match try_read_gitea_workflows(&repo, commit_sha, ref_name, event) {
    Ok(config) => return Ok(config),
    // 没有 workflows 目录 / 没有匹配的 workflow 才回退
    Err(error) if is_missing_workflows(&error) => {}
    Err(error) => return Err(error),
}
```

在裁决前，`expand_composite_actions` 已经在集成测试里用真实 commit 树跑通（§8 第 10 项），
接线只是把它放进 `read_ci_config`。

## 8. 测试映射（`cargo test -p rg-ci`）

| # | 任务卡要求 | 测试 |
|---|---|---|
| 1 | 单层 composite，2 step→数量/顺序 | `single_layer_composite_expands_to_its_steps_in_order` |
| 2 | 嵌套 2 层 | `nested_composites_expand_depth_first` |
| 3 | 循环引用 → 报错非 panic | `a_composite_cycle_fails_closed` |
| 4 | 深度 6 → 报错 | `a_sixth_composite_level_fails_closed` |
| 5 | `node20` → 错误含 `actions-v1` | `a_node_action_is_refused_and_names_actions_v1` |
| 6 | `./local/path` composite | `a_local_path_composite_resolves_from_the_workflow_source` |
| 7 | inputs 默认值 / 调用方覆盖 | `inputs_use_defaults_unless_the_caller_overrides_them` |
| 8 | checkout/cache 回归 | `checkout_and_cache_behaviour_is_unchanged` |
| 附加 | outputs 映射 | `composite_outputs_are_published_on_the_calling_step` |
| 附加 | docker fail closed | `a_docker_action_is_refused_and_names_actions_v1` |
| 附加 | 未缓存远程 + 无 base → fail closed | `an_uncached_remote_action_fails_closed_without_a_remote_source` |
| 附加 | 真实 commit 树（生产读取路径） | `a_local_composite_is_expanded_from_the_workflow_commit` |
| 附加 | 真实网络获取（默认 ignore） | `fetches_a_real_remote_action_from_the_configured_base` |

单测另覆盖：引用解析的正/负例、manifest 分类、标量默认值、label→能力。

## 9. 实际展开样例

`a_local_composite_is_expanded_from_the_workflow_commit` 把一个 workflow 与
`./.gitea/actions/greet` 一起提交到临时仓库，用 `ActionFetcher::for_git_commit` 读取 commit 树，
输出：

```
converted script:
# [IronForge] Repository is already checked out at /workspace
set -e
export INPUT_WHO=pipeline
echo "hi pipeline"
```

即：`uses: ./.gitea/actions/greet` 消失，Action 的 `run` 变成了普通 step，input 以
`INPUT_WHO` 环境变量 + 字面替换两种方式同时可用。
