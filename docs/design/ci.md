# CI：配置、执行与安全边界

> 状态（0.5.1）：本文描述已实现的行为。事实来源：`crates/rg-ci/`、`crates/rg-runner/`、`crates/rg-ci-isolation/`、`.github/workflows/regression.yml`。
> 第三方 Action 的信任边界由 [ADR-0001](adr-0001-third-party-actions.md) 冻结。

## 1. 配置来源

- `.ironforge-ci.yml`：手写配置，使用 `script`（字符串数组），没有 step 概念。
- `.gitea/workflows/*.yml`：Gitea Actions 兼容层，使用 `steps`。与 `script` 二选一。
- 支持本地 reusable workflow（展开时校验路径与循环）、Matrix、静态与运行时 `if`、step 输出（`$GITHUB_OUTPUT`）与 job 输出（`needs.<job>.outputs.*`）。
- 不支持或无法安全执行的能力在**创建 Pipeline 时**拒绝，不静默忽略。

## 2. 执行模型

Pipeline → Job → Step。每个 Job 可以有独立镜像、环境变量、Secret、缓存、矩阵展开与并发组。

- **内置 Runner**（`rg-ci`）与**外部 Runner**（`rg-runner`）走同一套加固策略：`rg-ci-isolation` 负责镜像名校验（防参数注入）、`no-new-privileges`、丢弃全部 capability、内存/CPU/PID 上限，并默认以调用进程的 `uid:gid` 运行，避免宿主工作区出现 root 文件。
- 宿主机模式只允许在管理员显式标记的受信 Runner 上运行原生 Job，**不会**因此获得 `actions-v1` 能力。
- 并发组是 pipeline 序列化语义，不是配额（配额见 §6）。

## 3. 第三方 Action

信任边界（ADR-0001，Accepted）：

1. 第三方 Action 只能分配给声明 `actions-v1` 的专用外部 Runner，不得在 IronForge 服务进程或服务主机执行。
2. 每个 Job 新建临时容器与临时网络，Job 结束销毁。
3. 禁止宿主机模式；无镜像或无匹配沙箱能力时 **fail closed**。
4. Runner capability 至少包含执行器版本、docker、node、网络 profile 与架构，调度器必须全部满足。

当前实现：

- **composite** Action 在创建 Pipeline 时展开为普通 step：纯 YAML 转换，不引入新运行时；嵌套有深度上限，检测循环；展开是原子的（失败不留下半成品）；远程缓存按子目录区分。
- **node*** 与 **docker** Action 继续 fail closed，错误说明需要 `actions-v1` Runner（尚未实现）。
- 内置适配：`actions/checkout`、`actions/cache`；其他 `uses:` 在创建时拒绝。

## 4. Secret 与令牌

- 仓库/组织 Secret 注入 Job；值不进入日志与 API 响应。
- CI Job 使用短时 job token，scope 由服务端按 job 签发；令牌有过期上限。
- 受保护环境需要真人审批（1–10 人，Agent 不能作为审批人），见 [授权文档](authorization.md) §7。
- OIDC：Job 可换取 audience 绑定的 EdDSA 令牌，claims 绑定 job/pipeline/repo 与实际执行主体。

## 5. 推送时密钥扫描

- 范围：本次推送新增/修改的 blob，不做全历史扫描。
- 硬拒（拒绝该 ref 更新或返回 422 `SECRET_DETECTED`）：私钥头、云访问密钥、平台 PAT（`gh[pousr]_`、`glpat-`、`ifs_`）。
- 记录事件但不拒：JWT 形状、高熵字符串（避免误伤 base64 资源与 minified JS；Git SHA 的熵低于阈值）。
- 限额：单文件 1 MiB、单次 500 个文件；超限按设计记录审计事件，不静默放行。
- 允许列表：路径 glob + 行内标记。REST 提交路径在写对象前扫描；Git push 在改 ref 前扫描，命中硬规则只把该 ref 标为 `ng`，不写 ref。
- 审计事件：`repo.secret_scan_rejected` / `_findings` / `_skipped`。

## 6. 配额与用量

`usage_counters` 按 `(subject_kind, subject_id, repo_id, metric, period_start)` 唯一，`value` 是**绝对量**，重复写入同一次测量不会叠加。

- 只对 `users.kind = agent` 生效，人类不受限。
- 口径：每 Agent × 每仓库。
- `ci_concurrent_pipelines`：最多 4 条并发。
- `ci_minutes`：每天 120 分钟。
- 超限在**创建 Pipeline 之前**返回 409；上限常量集中在 `rg-ci/src/quota.rs`。
- 没有 REST 查询端点；协作任务的 budget/usage 是客户端自报，与平台测量分开。

## 7. 缓存、制品与保留

- 缓存：按 key 命中，作用域跟随仓库与分支策略；远程缓存路径按子目录区分。
- 制品：Job 产出上传后可下载，受保留策略约束。
- 保留：Pipeline、日志、制品按仓库/全局保留策略清理；清理任务幂等。

## 8. 不变量

1. 不受信的第三方代码永远不在服务主机执行；无沙箱能力就拒绝。
2. 扫描只处理本次推送；命中硬规则时 ref 不前进。
3. 配额在创建前判定；用量是平台测量值，不接受客户端上报覆盖。
4. Secret、job token、OIDC 令牌不进日志；错误响应不包含明文。
