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

## 分阶段交付

| 阶段 | 内容 | 风险 |
|---|---|---|
| **A** | `GiteaStep.id` + `GiteaJob.outputs` 解析；`StepConfig` 类型；`to_ci_config` 保留 step 列表（但仍拍平执行） | 低（只是解析，行为不变） |
| **B** | 执行器改为逐 step 执行（`steps` 有值时）；`$GITHUB_OUTPUT` 文件注入与解析；`steps.<id>.outputs.*` 在运行时条件与 `env` 里可引用 | **中高**：执行顺序变化 |
| **C** | `jobs.<job>.outputs` 求值 + `pipeline_jobs.outputs_json` 持久化 + `needs.*.outputs.*` | 中：涉及迁移与跨 job 传递 |
| **D** | `always()/failure()/cancelled()` + step 级 `if` 从转换期挪到运行时 | **高**：语义变更，所有现有 workflow 的 `if` 走新路径 |
| **E** | 真 workflow 回归（用一个含 outputs 与 `always()` 的 `.gitea/workflows/` 跑真实 pipeline） | — |

A 和 B 是 CI-202；C 是 CI-202 的另一半；D 是 CI-203；E 是验收。

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
