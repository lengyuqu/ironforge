# CI-202/203：Step/Job outputs 与运行时条件（设计定稿）

> 状态：设计就绪，待实现。gap list 行 `CI/Actions / Step/Job outputs 与运行时条件`（权重 1.0，MISSING）。
> 结论先行：**这不是"给结构体加两个字段"，是要把「step」这个概念引入内部配置模型。**

## 现状（有代码依据）

| 层 | 现状 | 位置 |
|---|---|---|
| Gitea 解析 | `GiteaStep` 有 `name/uses/run/shell/env/if/with`，**没有 `id`**；`GiteaJob` **没有 `outputs`** | `rg-ci/src/gitea_actions.rs:147` / `:90` |
| 内部模型 | `JobConfig` 只有 `script: Vec<String>` —— **没有 step 列表**；`condition` 是单个字符串 | `rg-ci/src/config.rs:44` |
| 转换 | `to_ci_config` 把 steps **拍平成 script**：每个 step 的 `run` 拼进数组，`if` 在这里**静态求值**决定是否收进来 | `rg-ci/src/lib.rs:707` 附近 |
| 执行 | 没有 `GITHUB_OUTPUT` / `GITHUB_ENV`（全仓 grep 零命中） | — |
| 条件 | `condition.rs` 只认 `github.*` / `env.*` / `matrix.*`，没有函数调用（`always()` 等） | `rg-ci/src/condition.rs:41` |

**这个结构决定了 outputs 无处可放。** `script` 是一个字符串数组，没有"哪个 step 产出了什么"的身份；`if` 在转换期就求值完了，运行时根本没有条件判断的时机。

## 五个决策点

### 1. 引入 `StepConfig`，还是给 `script` 打标记？

**采用**：`JobConfig` 增加 `steps: Option<Vec<StepConfig>>`，与现有 `script` **并存**，二选一（`steps` 有值时忽略 `script`）。

理由：手写的 `.ironforge-ci.yml` 用 `script`（无 step 概念），Gitea workflow 转出来的用 `steps`。强制统一会破坏所有现有手写配置。

```rust
pub struct StepConfig {
    /// `id` from the workflow. Required when a later step or job output
    /// references `steps.<id>.outputs.*`; optional otherwise.
    pub id: Option<String>,
    pub name: Option<String>,
    /// Exactly one of these is set, same rule as the Gitea parser.
    pub run: Option<String>,
    pub uses: Option<String>,
    /// `None` means "run only if the job has not failed yet".
    pub condition: Option<String>,
    pub env: HashMap<String, String>,
    pub with: HashMap<String, String>,
}
```

### 2. `$GITHUB_OUTPUT` 的协议

**采用**：GitHub 的两种格式都支持。

```
name=value          # 单行
name<<EOF           # 多行（heredoc）
value line 1
value line 2
EOF
```

单行格式里 `value` 可以含 `=`（只按**第一个** `=` 切分）。heredoc 的结束标记必须独占一行且无缩进。

**不采用**：只支持单行。多行输出（commit message、JSON）是最常见的用法，只支持单行等于让一半真实 workflow 失效。

### 3. step 之间与 job 之间的作用域

| 引用 | 何时可用 | 实现 |
|---|---|---|
| `steps.<id>.outputs.<name>` | 同一 job 内，**后续** step | 内存 map，job 结束即丢 |
| `needs.<job>.outputs.<name>` | 依赖 job 完成后 | 需要**持久化**：`pipeline_jobs` 加一列 `outputs_json`，job 完成时写入 |
| `jobs.<job>.outputs.<name>` | 声明式 | job 结束时按表达式求值（值通常形如 `steps.<id>.outputs.<name>`），结果写进上面的 `outputs_json` |

**关键约束**：`needs` 只能在**已完成的依赖**上取值。依赖失败而 job 仍要跑（`if: always()`）时，那些 outputs **为空而不是报错** —— GitHub 的行为是空，跟随它，因为报错会让"清理 job"这类模式无法实现。

### 4. 运行时条件：现在做，还是分开？

**分开，先做 outputs（CI-202），条件后做（CI-203）。**

理由：`if` 现在在**转换期**求值（`to_ci_config`），把它挪到运行时意味着 `JobConfig.condition` 的语义变更。同一次改动里既加 outputs 又改条件求值时机，一旦出错无法判断是哪个引起的。

顺序：
1. 先让 `condition` 在**转换期**仍按现在的方式求值（行为不变），只加 `steps.*.outputs` 的**运行时**求值入口
2. outputs 稳定后，再把 step 级 `if` 从转换期挪到运行时

### 5. `always() / failure() / cancelled()` 的语义

**采用 GitHub 的三值语义**：

| 函数 | 真值条件 |
|---|---|
| `success()`（默认） | 之前所有 step 都成功 |
| `failure()` | 之前至少一个 step 失败 |
| `always()` | 恒真 |
| `cancelled()` | 运行被取消 |

**短路规则**：`always()` 出现在 `if` 里时，无论前面的 step 是否失败都执行 —— 这正是"清理/上传日志"步骤依赖的行为。

**不采用**：自己的简化语义（比如"有 always 就跑"）。真实 workflow 里 `if: failure() && steps.x.outcome == 'success'` 这类组合很常见，简化语义会让它们静默出错。

### 6. outputs 怎么从 runner 回到服务端（**第一版设计漏了这一条**）

补这条之前我假设「执行器改一下就行」。读代码发现不是：

| 事实 | 位置 |
|---|---|
| `script` 被 join 成**一个字符串**下发 | `rg-cli` 内联：`rg-ci/src/lib.rs:250`、`runner.rs:268`；独立进程：`rg-runner/src/main.rs:969` |
| `FinishJobRequest` 只有 `status` + `exit_code` | `rg-http/src/api/runners.rs:955` |
| 服务端与 runner 之间没有任何 step 粒度的往返 | — |

也就是说：**执行可能发生在一个独立进程里**，outputs 要跨进程回到服务端才能给下游 job 用。

**采用**：`FinishJobRequest` 增加可选 `outputs: HashMap<String, String>`，job 结束时与 `status` 一起上报。

理由：
- 一次往返，不新增端点
- **失败时也能带回已产出的 outputs**（独立端点会在崩溃时丢掉它们，而"失败步骤之后的清理 job 需要前序输出"正是 `always()` 场景的核心）
- 向后兼容：字段可选，旧 runner 不上报时按空处理

**不采用**：
- 独立 `POST /jobs/{id}/outputs` 端点 —— 多一次往返，且与 `finish` 的竞态需要额外处理
- 只在本地内联 runner 支持 —— 那等于让 outputs 在自托管 runner 上静默失效，比不支持更糟

服务端把上报的 map 写进 `pipeline_jobs.outputs_json`（与阶段 C 的迁移是同一列），下游 job 通过 `needs.<job>.outputs.*` 读。

### 7. B1/B2 不可再拆（对第 6 条的修正）

写 B1 的时候发现的第二层问题：**`steps` 在持久化时就丢了**。

| 环节 | 现状 |
|---|---|
| `to_ci_config` 产出 | `JobConfig { steps, script }` ✅ |
| 落库 | `pipeline_jobs.script: String`（换行分隔），**没有 steps 列**（`rg-db/src/entities/pipeline_job.rs:15`） |
| 下发 | `PollJobResponse.script: Vec<String>`（从那个字符串切出来） |
| runner | 拿到的只有命令列表 |

所以 outputs 的链路是 **6 个改动点、跨 4 个 crate + 1 个迁移**：

1. 迁移 `pipeline_jobs.steps_json`
2. `api/ci.rs` 落库时写入
3. `PollJobResponse` 下发
4. runner 逐 step 执行 + 写/读 `$GITHUB_OUTPUT`
5. `FinishJobRequest.outputs` + 服务端存 `outputs_json`
6. 引用（同 job B3 / 跨 job C）

**1~4 做完时功能仍然完全不可见**，因为产出值没有任何途径回到服务端。第 6 条把 B 拆成 B1/B2/B3 的前提是"每步可独立验证"，这个前提在这里不成立。

**修正后的划分**：

| 阶段 | 内容 | 可验证性 |
|---|---|---|
| **B** | 上面 1~5 一次做完 | ✅ 能端到端验证：一个 step 写 outputs，另一个 job 通过 `needs` 读到 |
| **B3→C** | 引用语法（同 job 的 `env`/`if`、跨 job 的 `needs`） | ✅ 依赖 B 的存储 |
| **D** | 运行时条件 | ✅ 依赖 B3/C |

也就是说：**B 是一个不可再分的大改动**，不是三个小改。

这不是"想偷懒合并"，而是"拆了之后每一步的验收标准都是空的"。宁可一个 PR 大一点、可验证，也不要三个 PR 都绿但功能不生效。

## 分阶段交付

| 阶段 | 内容 | 风险 |
|---|---|---|
| **A** ✅ | `GiteaStep.id` + `GiteaJob.outputs` 解析；`StepConfig` 类型；`to_ci_config` 保留 step 列表（但仍拍平执行） | 低（已合并 #33） |
| **B** | 迁移 `steps_json` + 落库 + 下发 + runner 逐 step 执行 + `$GITHUB_OUTPUT` + `FinishJobRequest.outputs` + `outputs_json` 落库 | **高**：一次跨 4 crate，但这是唯一可验证的切法（见第 7 条） |
| **B3→C** | 引用语法：同 job 的 `env`/`if`、跨 job 的 `needs.<job>.outputs.*` | 中 |
| **D** | `always()/failure()/cancelled()` + step 级 `if` 挪到运行时 | **高**：语义变更 |
| **D** | `always()/failure()/cancelled()` + step 级 `if` 从转换期挪到运行时 | **高**：语义变更 |
| **E** | 真 workflow 回归（含 outputs 与 `always()`） | — |

A 已合并（#33），`$GITHUB_OUTPUT` 解析器已合并（#35）。B + B3→C 是 CI-202；D 是 CI-203；E 是验收。

## 回归策略

现有 pipeline **行为必须逐字节不变**：

1. `steps` 为 `None` 时执行路径完全走现在的 `script` 数组（A 阶段之后立即可验证）
2. 每个阶段的 PR 都要跑 `scripts/git-protocol-e2e.sh` 与现有 `rg-ci` 测试
3. D 阶段的验收标准：把现有测试里所有 `if` 表达式抽出来，新求值器对每一个给出与静态求值相同的结果 —— 差异必须是零，除非表达式里含运行时函数

## 明确不做

- **第三方 `uses` 的 outputs**：ADR-0001 已冻结第三方 action 执行边界，`uses` 目前只支持 `actions/checkout` 这类内建项。它们的 outputs 由内建实现提供，不走 `$GITHUB_OUTPUT`
- **`GITHUB_ENV` / `GITHUB_PATH`**：与 outputs 是同一类机制但不同用途，可复用同一套文件注入，独立成一个任务
- **service containers**（CI-204）与 **reusable workflow 完整语义**（CI-205）：同属 CI 行，与本设计无关

## 验收（整行 CI-202/203）

- [ ] 一个 step 写 `$GITHUB_OUTPUT`，后续 step 在 `env` 与 `if` 里能引用到
- [ ] `jobs.a.outputs.x` 引用 `steps.s.outputs.y`，job b 通过 `needs.a.outputs.x` 拿到值
- [ ] `if: always()` 的 step 在之前 step 失败时仍执行
- [ ] `if: failure()` 只在有失败时执行
- [ ] 依赖 job 失败时，`needs.<job>.outputs.*` 求值为空而不是报错
- [ ] 不含 outputs 的现有 workflow 行为逐字节不变

## 验证记录

### 2026-11-24：容器内相对路径 → 宿主可读（实测）

本地 docker 29.9.3 + `alpine:3.21`：

```sh
docker run --rm -v "$WS:/workspace" -w /workspace alpine:3.21 sh -c \
  'export GITHUB_OUTPUT=".ironforge-outputs/step-1.outputs"; mkdir -p .ironforge-outputs; \
   printf "version=1.2.3\nnotes<<EOF\nfirst\nsecond\nEOF\n" > "$GITHUB_OUTPUT"'
```

宿主 `$WS/.ironforge-outputs/step-1.outputs` 内容完整（42 字节，heredoc 未被截断）。

**结论**：workspace 挂载 + 相对路径成立，不需要 `docker cp`。这是 #37 里唯一靠推断而非实测的一环，现在有实测了。

### 单测覆盖

`rg-runner`：

- `step_outputs_are_read_from_the_path_the_server_writes` —— 服务端写的 `step-<workflow-index>` 与 runner 读的索引一致，heredoc 保留换行，缺文件不报错
- `a_job_without_steps_collects_nothing` —— 没有声明 step 的 job 不去扫目录（避免读到上一次运行的残留）
- `a_malformed_output_file_is_ignored_not_fatal` —— 畸形输出文件不影响 job 结果

`rg-ci`：`only_steps_with_ids_get_an_output_file` —— 无 id 的 step 不注入，索引取 workflow 位置。
