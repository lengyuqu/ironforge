# IronForge 与主流 Git 平台差距分析（2026-07-10）

> 分析日期：2026-07-10
> 本地代码基线：`ececb16` 及当前工作区
> 对比对象：GitHub、GitLab、Gitea 1.26、Forgejo 15
> 分析方法：当前源码与测试静态核验 + 主流平台官方文档校准
> 注意：本文区分“模块/接口存在”和“功能闭环、权限安全、生产成熟”。旧报告中的完成度百分比不能直接作为平台替代能力结论。

## 实施进度更新（2026-07-11）

- PR/Review 仓库权限、资源归属与越权测试已完成；
- PAT `repo` / `user` / `admin` scope 已在 REST 与 Git HTTP 强制执行；
- SSH Key 用户 CRUD、设置页、唯一性/所有权校验及使用时间记录已完成；
- PostgreSQL/MySQL 运行时 SQLite 硬编码、PostgreSQL 原始 SQL 占位符、MySQL 表重命名，以及 Wiki revision/Runner 字段跨后端迁移已修复；
- 主 CI 已增加 PostgreSQL 17 与 MySQL 8.4 的真实 migration + CRUD + counter + FTS smoke；本地 SQLite 同一 smoke 已通过，服务器数据库结果以首次 CI 运行记录为准。
- 前端 `svelte-check` 基线已恢复为 0 error / 0 warning，并补齐登录页 OAuth2/OIDC SSO provider 入口。
- Draft PR、多人 Reviewer 请求/移除、评论线程 Resolve/Reopen 已完成后端权限、OpenAPI、前端与集成测试闭环。
- CODEOWNERS 已支持标准文件位置、通配符/覆盖规则，以及用户和 `@org/team` 自动 Reviewer 请求；团队仅限仓库所属组织且要求 write/admin 权限，展开后按用户去重。
- PR Diff 已改为逐文件结构化行数据，前端支持新增/删除/上下文行号、行级评论与线程解决/重开。
- 修复分支 ref 指向 commit/tag 时未 peel 到 tree、导致真实仓库 Diff 失败的问题，并增加真实 bare 仓库 + 双分支 + CODEOWNERS 的 API 集成覆盖。
- Auto-merge 已支持 merge/squash/rebase 策略、启停 API、前端等待状态，以及审批、push、内置/外部 Runner CI 成功后的自动重试；并通过数据库认领避免并发重复合并。
- PR 创建和 push 现在持续维护 `head_sha`；审批按 Reviewer 去重并绑定当前提交，新提交会使旧审批失效。
- Merge Queue 已支持仓库级 FIFO、merge/squash/rebase、排队/取消/列表 API、前端队列位置，以及审批/push/CI 事件驱动；队首通过保护规则后原子合并，失败项会记录原因并继续队列。
- 结构化代码建议已支持单行/连续多行 replacement 与范围删除、同仓/Fork 源仓库权限校验、一键应用并生成提交；建议绑定 PR `head_sha`，越界、过期或已应用建议会被拒绝。
- 批量代码建议已支持选择 1—100 条建议在单个提交中原子应用；同文件按原始行号倒序处理，重叠范围、重复 ID、无效路径、symlink 路径及并发分支更新会被拒绝，成功后只触发一次 CI/Auto-merge/Merge Queue 评估。
- PR 会话页已增加统一审查时间线 API/UI，按时间汇总 PR 创建、Review、评论/回复、代码建议及应用、线程解决、Reviewer 请求、Auto-merge、Merge Queue 和最终合并/关闭事件；既有表未保留的 Reviewer 移除、线程重开等历史状态仍需后续不可变事件表补齐。

因此，第 4.1—4.3 节记录的是修复前基线；第 4.4 节的代码与 CI 缺口已进入验证阶段，不再是“完全未闭环”状态。

## 1. 结论

IronForge 已具备小团队自托管 Git 平台的主要骨架，功能广度接近早期 Gitea/Forgejo；但距离当前 Gitea/Forgejo 仍有中等功能差距和较大的生产成熟度差距，距离 GitHub/GitLab 则是平台级差距。

当前最合理的产品定位是：

- Rust 实现的轻量自托管 Git Forge；
- 以单机/小团队部署为主要场景；
- 以 MCP/AI Agent 集成为差异化优势；
- 暂不应宣传为 GitHub/GitLab 的直接替代品；
- PR 授权、PAT scope、SSH Key 管理等本轮 P0 已闭环；生产就绪判断仍需结合真实 CI 数据库矩阵、备份恢复和安全审计结果。

## 2. 当前已经具备的能力

当前代码已经覆盖：

- Git Smart HTTP、SSH、Git Protocol V2、LFS；
- 仓库、Fork、Star/Watch、Mirror、Release、归档和导入；
- Issue、PR、Review、Wiki、组织、团队、通知、邮件；
- 原生 CI YAML、Gitea Actions 转换、内置/外部 Runner、Artifact、实时日志、Pipeline 重试和并发控制；
- Package Registry、OCI Registry；
- OAuth2/OIDC、TOTP MFA、审计日志；
- 看板、工时、全文/代码搜索；
- OpenAPI、Prometheus、健康检查、Docker 部署；
- MCP stdio server。

顶层事实见 `ironforge-docs/architecture/project-architecture-2026-07.md`。该文档在数据库口径上已落后于 2026-07-08 的多后端代码变更，数据库现状应结合本文第 4.4 节判断。

## 3. 主要差距矩阵

| 领域 | IronForge 当前情况 | 与主流平台的差距 |
|---|---|---|
| Git/仓库治理 | Clone/Push、Fork、Mirror、LFS、基础分支保护、用户/团队 CODEOWNERS、Auto-merge 和基础 FIFO Merge Queue 已具备 | 缺 Deploy Key 自助管理、Tag 保护、签名提交强制策略、规则集，以及 merge-group 合成提交 CI/批量队列等高级能力；大仓库 pack 性能未充分验证 |
| PR 与代码审查 | 支持三种合并、Review、Draft PR、Reviewer 请求、逐文件行级 Diff/评论、单行/多行及批量代码建议、统一审查时间线、线程 Resolve、提交级审批失效、用户/团队 CODEOWNERS、Auto-merge 和基础 Merge Queue | 时间线尚缺不可变状态变更存储；Merge Queue 尚缺 merge-group CI 等高级能力 |
| CI/CD | 原生 YAML、Docker/外部 Runner、Artifact、日志、重试、并发控制 | Actions 兼容层除 `checkout` 外会跳过其他 `uses:`；缺完整 Actions 运行时、缓存、Matrix、可复用 Workflow、环境审批、OIDC、服务容器和多项目流水线 |
| DevSecOps | 有 GPG 验签和审计日志 | 缺 Dependency Graph、依赖更新、SAST/DAST、Secret Scanning、Push Protection、SBOM、容器漏洞扫描、安全公告和漏洞修复工作流 |
| Package Registry | OCI 加 Cargo/npm/Maven/PyPI/NuGet/RubyGems/Helm/Composer 等原生适配 | 约 10 个原生 adapter，其余声明类型落到 Generic；缺更完整的协议覆盖、清理规则、配额、保留策略和供应链证明 |
| 企业身份与治理 | OAuth2/OIDC、TOTP、LDAP 模块、审计查询 | LDAP 尚未进入主登录调用链；缺 SAML、SCIM、自动开通/回收、团队同步、IP Allowlist、审计流式导出等 |
| 项目与社区协作 | Issue、Milestone、简单看板、工时、Wiki | 缺 Issue Form/模板、Discussions、跨仓库项目、自定义字段、Iteration、Roadmap/Epic、Service Desk、Snippets/Gists |
| 开发者体验 | 单文件 Web 编辑、代码搜索、MCP 只读工具 | 缺完整 Web IDE、Pages、远程开发环境、成熟 CLI 和 IDE 插件 |
| API 与生态 | REST、OpenAPI、Webhook、MCP | 缺 GraphQL、GitHub/GitLab API 兼容层、App 安装模型、插件市场和第三方生态；MCP 目前只有少量只读工具且仅 stdio |
| 部署与扩展 | 单机 Docker、Prometheus、健康检查、SQLite DB 备份 | 缺对象存储、分布式任务队列、水平扩展、仓库存储分片、HA、跨地域复制、完整实例备份/恢复和零停机升级 |
| 测试与发布成熟度 | 现有 Rust 测试、22 个 HTTP 集成测试文件、前端编译检查及 PostgreSQL/MySQL runtime smoke CI | 主 CI 未运行真实浏览器 E2E、大仓库性能或升级/恢复演练；CLI/MCP/Runner/SSH 测试仍偏少 |

## 4. 上线阻断项（P0）

### 4.1 PR/Review 权限边界不完整

静态调用链显示：

- `pulls::list_prs`、`get_pr` 和 `get_diff` 没有仓库读权限检查；
- `pulls::update_pr` 没有认证参数；
- `pulls::merge_pr` 只检查是否存在登录用户，没有调用 `can_write_repo`；
- Review 的多个 GET 端点没有仓库读权限检查；
- 按 review id 读取/驳回时，部分路径没有验证 review 是否属于 URL 指定仓库和 PR。

可能影响：私有仓库 PR 信息泄露、越权更新、越权合并、跨仓库 IDOR。

主要代码：

- `crates/rg-http/src/api/pulls.rs`
- `crates/rg-http/src/api/reviews.rs`
- `crates/rg-core/src/pull_request/service.rs`

### 4.2 PAT scope 保存但未执行

`access_tokens.scopes` 保存了 scope；但 `resolve_pat()` 只返回 token 的 `user_id`，随后 middleware 为该用户签发等价 JWT。scope 没有进入 JWT，也没有在 API/Git 路径执行授权判断。

可能影响：用户创建的受限 PAT 实际获得完整用户 API 权限。

主要代码：

- `crates/rg-db/src/entities/access_token.rs`
- `crates/rg-http/src/lib.rs::resolve_pat`
- `crates/rg-http/src/lib.rs::pat_auth_middleware`

### 4.3 SSH 公钥缺少用户自助入口

当前存在 `ssh_keys` entity、DB ops 和 SSH 指纹认证，但没有发现用户可调用的 HTTP API、前端页面或 CLI 命令。公钥认证底层可用，但普通用户无法自行添加、查看、轮换和删除 SSH Key，也没有 Deploy Key 模型。

主要代码：

- `crates/rg-db/src/entities/ssh_key.rs`
- `crates/rg-db/src/ops/ssh_key_ops.rs`
- `crates/rg-ssh/src/lib.rs`

### 4.4 PostgreSQL/MySQL 仍属于实验性支持

2026-07-08 的代码已经增加 PostgreSQL/MySQL 连接、迁移和 FTS 方言，但：

- 设计文档明确说明没有真实 PostgreSQL/MySQL runtime 验证；
- Wiki FTS 写路径仍写死 `DatabaseBackend::Sqlite`；
- repo stars/forks count 更新仍写死 SQLite statement backend；
- 主 CI 只运行 SQLite fresh migration smoke。

因此当前应写成“代码已接入、生产兼容性未闭环”，而不是生产级多数据库支持。

主要代码：

- `ironforge-docs/architecture/db-multi-backend-design-2026-07.md`
- `crates/rg-core/src/wiki/service.rs`
- `crates/rg-db/src/ops/repo_ops.rs`
- `.github/workflows/regression.yml`

## 5. 与 Gitea/Forgejo 的重点差距

### 5.1 CI 生态

Gitea Actions 已用于其自身生产仓库；Forgejo Actions 有独立 Runner、日志/Artifact 保留、缓存和多 Runner 分发。IronForge 当前是 Actions YAML 到内部 `CiConfig` 的有限转换：除 `actions/checkout` 外，其他 `uses:` 会生成“skipping”注释。

### 5.2 Package Registry

Gitea 1.26 官方列出的专用包管理器包括 Alpine、Arch、Cargo、Chef、Composer、Conan、Conda、Container、CRAN、Debian、Generic、Go、Helm、Maven、npm、NuGet、Pub、PyPI、RPM。Forgejo 还覆盖 RubyGems、Swift、Vagrant 等，并提供 package cleanup rules 与 blob deduplication。

IronForge 当前原生 adapter 映射为 Cargo、npm、NuGet、PyPI、RubyGems、Maven、Docker、Generic、Helm、Composer；其他类型使用 Generic fallback。

### 5.3 运维能力

Gitea 已提供 MySQL/PostgreSQL/MSSQL/SQLite、Redis/LevelDB queue，以及 local/MinIO-S3/Azure Blob storage。IronForge 的 Git、LFS、Package、OCI、Artifact 仍主要依赖单机文件系统，没有分布式 queue/storage 抽象。

## 6. 与 GitHub/GitLab 的平台级差距

### 6.1 CI/CD 与部署

GitHub Actions 支持 Matrix、Reusable Workflows、缓存、Environments、部署审批、OIDC、Runner Groups/Scale Sets、Artifact Attestations 和 Marketplace。GitLab CI/CD 支持可复用 Components、Matrix、Parent-child/Multi-project pipelines、环境审批、受保护变量和多种 Runner executor。

IronForge 当前 CI 更接近“阶段 + Shell/Docker Job 执行器”，尚不是兼容主流生态的 Workflow 平台。

### 6.2 软件供应链安全

GitHub Code Security/Secret Protection 和 GitLab Application Security 已形成代码、依赖、Secret、容器、IaC、DAST、SBOM、策略与告警治理闭环。IronForge 尚无对应领域模型和 UI。

### 6.3 企业 IAM 与合规

GitHub/GitLab 均提供 SAML SSO、SCIM、企业托管用户、团队/群组同步和企业审计能力。IronForge 当前 OAuth2/OIDC 已有完整流程，但 LDAP 主登录、SAML/SCIM 和生命周期治理未闭环。

### 6.4 规划、社区和开发环境

GitHub 提供 Discussions、Projects 自定义字段与 Iterations；GitLab 提供 Epic/Roadmap、Service Desk、Pages、Web IDE 和 Workspaces。IronForge 当前只有仓库级简单看板、Issue/Milestone、Wiki 和单文件编辑。

### 6.5 API 和扩展生态

GitHub/GitLab 均有 GraphQL API；GitHub Apps 提供细粒度权限、短期安装 token、Webhook 和 Marketplace。IronForge 当前主要是 REST/OpenAPI/Webhook，MCP 是优势但仍为少量只读 stdio 工具。

## 7. 建议执行顺序

### 第一波：权限与身份 P0

1. PR/Review 全链路 `can_read_repo` / `can_write_repo`；
2. 资源 ID 必须校验属于 URL 指定 repo/PR；
3. PAT scope 在 REST 与 Git HTTP 强制执行；
4. SSH Key 用户 CRUD API + 前端设置页 + 唯一性/所有权测试；
5. 增加私有仓库泄露、越权更新、越权合并和跨仓库 IDOR 集成测试。

### 第二波：CI 与代码审查闭环

1. Actions 执行兼容或明确采用独立格式；
2. Secrets/Variables、Cache、Matrix、Reusable Workflow、环境审批和 OIDC；
3. merge-group 合成提交 CI/批量队列；
4. 为 Reviewer 移除、线程重开、Draft/Ready 切换等补充不可变审查事件存储。

### 第三波：生产化

1. PostgreSQL/MySQL 实库 migration + CRUD + FTS smoke；
2. 全量消除 runtime 路径中的 SQLite backend 硬编码；
3. 对象存储、后台任务队列、全实例备份恢复；
4. 多节点/HA/升级与恢复演练；
5. 大仓库 clone/fetch/push 性能和兼容矩阵。

### 第四波：平台能力

1. DevSecOps 与软件供应链；
2. SAML/SCIM 和企业治理；
3. GraphQL/App 扩展模型；
4. Discussions、Roadmap、Pages、Web IDE 等体验功能。

## 8. 竞品官方资料

- GitHub Actions Matrix：<https://docs.github.com/en/actions/how-tos/write-workflows/choose-what-workflows-do/run-job-variations>
- GitHub Reusable Workflows：<https://docs.github.com/en/actions/how-tos/reuse-automations/reuse-workflows>
- GitHub Code Security：<https://docs.github.com/en/code-security/getting-started/quickstart-for-securing-your-repository>
- GitHub GraphQL：<https://docs.github.com/en/graphql>
- GitHub Apps：<https://docs.github.com/en/apps/overview>
- GitHub SAML/SCIM：<https://docs.github.com/en/enterprise-cloud%40latest/organizations/managing-saml-single-sign-on-for-your-organization/connecting-your-identity-provider-to-your-organization>
- GitLab CI/CD：<https://docs.gitlab.com/ci/>
- GitLab Downstream Pipelines：<https://docs.gitlab.com/ci/pipelines/downstream_pipelines/>
- GitLab Application Security：<https://docs.gitlab.com/user/application_security/detect/>
- GitLab Reference Architectures：<https://docs.gitlab.com/administration/reference_architectures/>
- GitLab Geo：<https://docs.gitlab.com/administration/geo/>
- GitLab GraphQL：<https://docs.gitlab.com/api/graphql/>
- Gitea Actions：<https://docs.gitea.com/usage/actions/overview>
- Gitea Packages：<https://docs.gitea.com/usage/packages/overview>
- Gitea Configuration：<https://docs.gitea.com/administration/config-cheat-sheet>
- Forgejo Actions：<https://forgejo.org/docs/latest/user/actions/overview/>
- Forgejo Packages：<https://forgejo.org/docs/latest/user/packages/>
