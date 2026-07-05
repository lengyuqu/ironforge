# IronForge 项目架构总览（2026-07）

**生成日期**: 2026-07-05  
**分析基线**: `main` 分支，`9088e2a`；并已回填 2026-07-05 修复波次后的工作区事实  
**事实来源**: 当前代码、配置、迁移、前端路由、测试、部署文件和架构修复回填  
**配套文档**:

- `ironforge-docs/project-architecture-analysis-notes-2026-07.md`
- `ironforge-docs/frontend-backend-structure-2026-07.md`
- `ironforge-docs/architecture-followups-2026-07.md`

---

## 1. 系统定位

IronForge 是一个 Rust + SvelteKit 实现的轻量级 Git 托管平台。当前系统已经不只是仓库托管，还包含 Issue、PR、Wiki、CI/CD、Package Registry、OCI Registry、组织、通知、审计、SSO/MFA、Mirror、Import、Board、Time Tracking、MCP 等平台能力。

当前架构的核心特点：

- 后端是 Rust workspace，主服务二进制为 `ironforge`。
- 前端是 SvelteKit 静态 SPA，构建产物由后端静态托管。
- Git 服务同时支持 HTTP Smart Git 和 SSH。
- 数据库当前是 SQLite + SeaORM，迁移在服务启动时自动执行。
- CI 可以由主服务内置 runner 执行，也可以由外部 runner 轮询执行。
- Package Registry 分为 REST package API 和独立 OCI `/v2/` API。
- MCP server 是独立二进制，通过 IronForge REST API 访问数据。

---

## 2. 顶层架构

```mermaid
flowchart LR
  Browser["Browser / SvelteKit SPA"] --> HTTP["rg-http Axum Server"]
  GitClient["git CLI HTTP"] --> GitHTTP["/git and root Git Smart HTTP"]
  GitHTTP --> HTTP
  SSHClient["git CLI SSH"] --> SSH["rg-ssh russh Server"]
  DockerClient["Docker / OCI Client"] --> OCI["/v2 OCI Registry"]
  OCI --> HTTP
  Runner["ironforge-runner / ironforge runner"] --> RunnerAPI["/api/v1/runners"]
  RunnerAPI --> HTTP
  MCPClient["AI Agent"] --> MCP["ironforge-mcp"]
  MCP --> REST["/api/v1 REST API"]
  REST --> HTTP

  HTTP --> Core["rg-core business services"]
  SSH --> GitProtocol["rg-git protocol layer"]
  HTTP --> GitProtocol
  Core --> DB["rg-db SeaORM + SQLite"]
  HTTP --> DB
  GitProtocol --> RepoRoot["repo_root bare repositories"]
  Core --> RepoRoot
  HTTP --> PackageFiles["package / oci / lfs / release files"]
```

主要边界：

| 层 | 主要路径 | 职责 |
|----|----------|------|
| CLI/进程入口 | `crates/rg-cli/src/main.rs` | 主服务启动、迁移、配置、日志、一次性命令、内置 runner |
| HTTP 服务 | `crates/rg-http/src/lib.rs`、`crates/rg-http/src/api/` | REST、Git HTTP、OCI、WebSocket、OpenAPI、静态前端 |
| SSH 服务 | `crates/rg-ssh/src/lib.rs` | SSH 认证、Git command 分发 |
| 业务服务 | `crates/rg-core/src/` | 用户、仓库、Issue、PR、Wiki、CI、Package、审计、SSO/MFA 等 |
| 数据库 | `crates/rg-db/src/` | entities、ops、migrations |
| Git 协议 | `crates/rg-git/src/` | pkt-line、sideband、upload-pack、receive-pack、Protocol V2、Git CLI gateway |
| CI 引擎 | `crates/rg-ci/src/` | CI 配置读取、pipeline 创建、内置执行器 |
| Runner Agent | `crates/rg-runner/src/main.rs` | 外部 runner 注册、轮询、执行、回传日志 |
| MCP Server | `crates/rg-mcp/src/` | MCP tools/resources，调用 IronForge REST API |
| 前端 | `web/src/` | SvelteKit SPA 页面、API client、状态、i18n、组件 |

---

## 3. 运行入口

### 3.1 二进制入口

| 二进制 | crate | 入口 | 主要用途 |
|--------|-------|------|----------|
| `ironforge` | `rg-cli` | `crates/rg-cli/src/main.rs` | 主服务与管理 CLI |
| `ironforge-runner` | `rg-runner` | `crates/rg-runner/src/main.rs` | 独立 CI Runner Agent |
| `ironforge-mcp` | `rg-mcp` | `crates/rg-mcp/src/main.rs` | MCP stdio server |

### 3.2 主服务启动链路

`ironforge serve` 的实际启动链路：

```text
Cli::parse
  -> run_serve
  -> load optional TOML config
  -> resolve JWT/config/logging/timeouts
  -> init tracing
  -> create repo_root
  -> init rg_git::cli_gateway
  -> rg_db::connect_with_timeouts
  -> rg_db::run_migrations
  -> build AppState
  -> spawn rg_http::run(...)
  -> spawn rg_ssh::start_ssh_server(...)
  -> await HTTP task
```

重要运行特征：

- HTTP 与 SSH 在同一主服务进程中启动。
- SSH 启动失败不会阻止 HTTP 继续运行。
- HTTP route 同时承载 REST、Git HTTP、OCI、WebSocket、OpenAPI 和 SPA fallback。
- 数据库迁移在 `serve` 启动时自动执行。
- `host_key` 缺失时会自动生成 ed25519 key。

### 3.3 运行时依赖

| 依赖 | 用途 | 当前状态 |
|------|------|----------|
| SQLite | 主数据库 | `sea-orm` + `sqlx-sqlite` |
| Git CLI | pack、diff、archive 等剩余能力 | 通过 `GitCommandGateway` 管控 |
| gix | 部分 Git 读写与 diff 能力 | 已用于多处替代 Git CLI |
| Docker | CI Docker job、外部 runner 可选 | 内置 runner 和外部 runner 对指定 image 的 job 均 fail closed |
| SMTP | 邮件通知、密码重置 | 可选配置 |
| TLS cert/key | HTTPS | 可选 CLI/config |
| repo_root | bare repositories、package/lfs/oci 文件 | 必须可写 |

---

## 4. Rust Workspace 边界

当前 workspace 本地依赖方向：

```text
rg-cli  -> rg-ci, rg-core, rg-db, rg-git, rg-http, rg-ssh
rg-http -> rg-core, rg-db, rg-git
rg-ssh  -> rg-core, rg-db, rg-git
rg-core -> rg-db, rg-git
rg-ci   -> rg-core, rg-db
rg-db   -> no local crate deps
rg-git  -> no local crate deps
rg-runner -> no local crate deps
rg-mcp -> no local crate deps
```

职责判断：

- `rg-http` 是对外协议聚合层，不只是 REST API。
- `rg-core` 是业务和平台服务层，不是纯领域模型层。
- `rg-db` 提供 SeaORM entities、ops、migrations。
- `rg-git` 封装 Git wire protocol 和 Git CLI gateway。
- `rg-ci` 通过 `rg_core::ci::CiTrigger` 被注入 HTTP，保持 HTTP 与 CI 引擎解耦。
- `rg-runner` 和 `rg-mcp` 是独立客户端型二进制，通过 HTTP API 与主服务通信。

---

## 5. 数据模型

数据库实体大致分为以下领域：

| 领域 | 代表实体 |
|------|----------|
| 身份与认证 | `users`、`ssh_keys`、`access_tokens`、`password_reset_tokens`、`oauth_accounts`、`mfa_backup_codes`、`login_logs`、`sso_providers` |
| 仓库 | `repositories`、`repo_collaborators`、`repo_stars`、`repo_watches`、`protected_branches` |
| Issue / PR / Review | `issues`、`issue_comments`、`labels`、`issue_labels`、`milestones`、`pull_requests`、`pr_reviews`、`review_comments` |
| Wiki / Release / LFS | `wiki_pages`、`wiki_revisions`、`releases`、`release_assets`、`lfs_objects` |
| CI/CD | `pipelines`、`pipeline_stages`、`pipeline_jobs`、`runners`、`artifacts`、`commit_statuses` |
| 组织与通知 | `organizations`、`teams`、`team_members`、`organization_members`、`notifications` |
| Package / OCI | `package_registries`、`packages`、`package_versions`、`package_files`、`oci_repositories`、`oci_blobs`、`oci_manifests`、`oci_uploads` |
| 扩展能力 | `webhooks`、`webhook_deliveries`、`mirrors`、`boards`、`board_columns`、`board_cards`、`time_entries`、`import_tasks`、`audit_logs` |
| 搜索 | `repos_fts`、`issues_fts`、`wiki_pages_fts`、`code_fts` |

迁移要点：

- 迁移位于 `crates/rg-db/src/migrations/`。
- `serve` 和 `migrate` 都调用 `rg_db::run_migrations`。
- 已存在多次纠偏迁移，用于修复历史表名单复数不一致。
- SQLite FTS5 同时用于仓库、Issue、Wiki 和代码搜索。

---

## 6. HTTP 服务面

### 6.1 路由前缀

| 前缀 | 用途 |
|------|------|
| `/api/v1` | REST API |
| `/git/{owner}/{repo}/...` | Git Smart HTTP |
| `/{owner}/{repo}/info/refs` 等 root Git 路由 | 兼容 Git Smart HTTP |
| `/v2` | OCI Distribution Registry |
| `/api/v1/ws/notifications` | 通知 WebSocket |
| `/api/v1/ws/job/{job_id}` | CI job log WebSocket |
| `/health` | 健康检查 |
| `/metrics` | Prometheus metrics |
| `/api-docs` | Swagger UI / OpenAPI |
| SPA fallback | `web/build/index.html` |

### 6.2 REST API 分组

`crates/rg-http/src/api/` 当前包含 30+ 个 API 模块，覆盖：

- users/auth/mfa/sso/admin/audit；
- repos/repo_content/archive/search/ai；
- issues/labels/pulls/reviews/wiki/releases；
- orgs/collaborators/branch_protection/webhooks；
- ci/runners/artifacts/notifications；
- packages/imports/mirrors/boards/time_tracking/lfs。

REST handler 的边界并不完全统一：多数业务通过 `rg-core` service，部分 handler 直接调用 `rg-db::ops`。最终维护上应继续向“handler -> core service -> db ops”收敛。

### 6.3 中间件

生产 router 包含：

- metrics middleware；
- security headers + CSP nonce；
- request id；
- `TraceLayer`；
- CORS；
- `ConnectInfo`；
- rate limit；
- maintenance mode；
- PAT-to-Bearer middleware；
- docs auth middleware。

---

## 7. Git 与 SSH 协议层

Git 协议由 `rg-git` 承担：

| 模块 | 职责 |
|------|------|
| `pkt_line.rs` | pkt-line 编解码 |
| `sideband.rs` | sideband-64k |
| `protocol/upload_pack.rs` | upload-pack |
| `protocol/receive_pack.rs` | receive-pack |
| `protocol/v2.rs` | Protocol V2 ls-refs/fetch/object-info |
| `cli_gateway.rs` | Git CLI 调用统一入口 |

HTTP Git 与 SSH Git 共同复用 `rg-git` 协议层，但入口不同：

```text
HTTP Git
  -> rg-http handle_info_refs / upload-pack / receive-pack
  -> JWT/PAT auth + repo can_read/can_write
  -> rg-git protocol

SSH Git
  -> rg-ssh exec_request
  -> ssh key/password auth
  -> git-upload-pack / git-receive-pack dispatch
  -> rg-git protocol
```

当前需要注意的边界：

- HTTP Git 已接入仓库读写权限。
- SSH Git 已在 exec path 接入 repo-level `can_read/can_write` 检查。
- receive-pack 已在 HTTP/SSH Git pack/ref 更新前接入 protected branch rejected refs，禁止直推会返回 `ng`。
- V1 upload-pack 的 pack 生成仍偏简单，大仓库性能需要后续优化。

---

## 8. 安全与认证

认证方式矩阵：

| 类型 | 用途 |
|------|------|
| JWT | Web 登录、REST、WebSocket |
| HttpOnly Cookie | 浏览器会话，cookie 名 `ironforge_token` |
| PAT | API client、Git HTTP、docs access |
| SSH Key / Password | SSH Git |
| TOTP MFA | 登录 step-up |
| OAuth2 SSO | 外部身份 provider |
| LDAP | LDAP bind 能力，登录集成需继续核验 |
| Runner Token | 外部 runner API |
| CI Job Token | CI job 最小权限 token，已接入 repo/package 读路径 |
| OCI Token | `/v2` registry bearer token |

安全中间件包括 CSP、安全 headers、CORS、Request-ID、Rate Limit、维护模式和审计日志。

当前 P0/P1 安全和权限缺口已完成首轮修复，需要继续保持的实现边界：

- 浏览器用户 API 优先使用 cookie-aware 的 `AuthUser` / `extract_user_id`，避免新增 Bearer-only handler。
- PAT、Git HTTP、Runner token、CI job token 和 OCI token 保持各自语义，不混用为同一个认证入口。
- 仓库关联资源继续通过 `can_read_repo` / `can_write_repo` 做 repo-scoped 授权。
- LDAP TLS 默认校验证书，只有显式 insecure 配置才跳过校验。
- Rate limit 默认只信任 socket IP，只有配置可信代理后才读取转发头。

详细清单位于 `architecture-followups-2026-07.md`。

---

## 9. CI/CD、Package、MCP 扩展能力

### 9.1 CI/CD

CI 支持：

- `.ironforge-ci.yml` 原生格式；
- `.gitea/workflows/*.yml` Gitea/GitHub Actions 兼容转换；
- pipeline/stage/job DB 记录；
- 内置 runner；
- 外部 runner long-poll；
- job log queue；
- runner labels/tags；
- CI job token 生成。

### 9.2 Package Registry

Package Registry 由通用 DB 模型、文件存储和 adapter 组成。

专用协议端点覆盖：

- Cargo sparse index；
- npm metadata；
- PyPI simple index；
- Maven metadata；
- NuGet service/registration/search；
- RubyGems dependencies/gem info；
- Helm index；
- Composer packages.json；
- Docker/OCI `/v2`。

前端和后端常量列出 17 种 package type，但真正有专用协议/adapter 的类型少于 17 个，其余会落到 generic 存储处理。

### 9.3 MCP

`ironforge-mcp` 默认 stdio，读取：

- `IRONFORGE_URL`
- `IRONFORGE_PAT`

暴露 tools：

- `list_repos`
- `read_file`
- `read_dir`
- `get_issue`
- `get_pr`

暴露 resources：

- `repo://{owner}/{name}`
- `file://{owner}/{name}/{path}`
- `issue://{owner}/{name}/{number}`

`--sse` 目前未实现，不能作为可用 transport 描述。

---

## 10. 构建、部署与运维

### 10.1 构建产物

```text
web npm run build
  -> web/build

cargo build --release --bin ironforge
  -> target/release/ironforge
```

Docker 构建链路：

```text
node frontend build
  -> rust release build --bin ironforge --bin ironforge-runner --bin ironforge-mcp
  -> debian runtime
  -> copy /app/web/build
  -> copy /usr/local/bin/ironforge
  -> copy /usr/local/bin/ironforge-runner
  -> copy /usr/local/bin/ironforge-mcp
```

### 10.2 运行数据

默认 Docker runtime 数据：

```text
/data/repos
/data/ironforge.db
/data/logs/ironforge.log
```

### 10.3 健康检查与观测

`/health` 检查：

- database；
- filesystem；
- metrics；
- git gateway；
- smtp。

`/metrics` 输出 Prometheus text format。`deploy/` 中提供 Prometheus、Alertmanager、Grafana 和 node-exporter 示例配置。

### 10.4 测试体系

当前验证体系包括：

- Rust unit tests；
- `rg-http` integration tests；
- `cargo-llvm-cov`；
- `web npm run check`；
- `web npm run build`；
- `scripts/full-interface-regression.mjs`；
- OpenAPI smoke；
- API client contract check；
- frontend/backend smoke；
- browser console/admin smoke；
- 多个领域 contract check。

---

## 11. 当前架构口径修正

相对旧 `ARCHITECTURE.md` 和历史 Phase 文档，当前应采用以下新口径：

- 系统当前不是“纯核心 Git 平台”，而是带 CI、Package、SSO/MFA、审计、导入、看板、工时、MCP 的平台型服务。
- 数据库仍是 SQLite-only，PostgreSQL 是后续方向，不是当前能力。
- 前端是静态 SPA，不是 SSR 应用。
- `client.svelte.ts` 当前是 38 行纯 re-export 兼容入口；API 真实实现已按领域拆到独立模块。
- Docker runtime 镜像包含 `ironforge`、`ironforge-runner`、`ironforge-mcp` 三个二进制。
- `--sse` MCP transport 未实现。
- 本轮 P0/P1 安全、权限和部署缺口已完成首轮修复；长期生产化方向仍包括 PostgreSQL、MCP SSE、Package 专用协议补全和 gix 后续迁移。

---

## 12. 建议阅读顺序

1. 本文：理解顶层架构与运行模型。
2. `frontend-backend-structure-2026-07.md`：定位前端页面、API client、后端模块和数据库结构。
3. `architecture-followups-2026-07.md`：查看需要修复的架构差异、权限缺口和运维缺口。
4. `project-architecture-analysis-notes-2026-07.md`：追溯每轮源码分析细节。
