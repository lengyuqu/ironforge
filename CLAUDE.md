# IronForge — AI 协作上下文

> 本文件是 **Claude Code 的默认入口**，也是所有 AI 编程助手的深度参考文档。
> 提供项目关键背景、约定、踩坑记录、依赖版本速查和常见任务的操作指南。
> **如果你刚打开本项目，建议先快速浏览 `AGENT.md` 获取概览，再深入阅读本文件获取完整细节。**

---

## 文档地图（按 AI 工具）

不同 AI 工具读取文件的习惯不同，以下是指南：

| AI 工具 | 自动读取的文件 | 建议补充读取 |
|---------|-------------|------------|
| **Claude Code** | `CLAUDE.md`（本文件） | `AGENT.md` + 按任务类型从「按任务类型选读」中选 |
| **WorkBuddy** | `.workbuddy/memory/MEMORY.md` + 每日日志 | `AGENT.md` + `CLAUDE.md` + `ARCHITECTURE.md` |
| **Codex** | 通常读取项目根目录的 `README.md` | **`AGENT.md`** ⭐（AI 统一入口） |
| **Trae** | 通常读取 `README.md` | **`AGENT.md`** ⭐（AI 统一入口） |
| **CodeBuddy** | 通常读取 `README.md` | **`AGENT.md`** ⭐（AI 统一入口） |
| **AI Agent（.ai/）** | `.ai/README.md` | 按任务类型选读（AGENT.md / CLAUDE.md / ARCHITECTURE.md） |

**如果你是其他 AI 工具且未自动读取本文件**：请先阅读 `AGENT.md`（更轻量的统一入口），然后通读本文件获取完整细节，再按任务类型从「按任务类型选读」中选择延伸阅读。

---

## 项目简介

**IronForge**（铁匠铺）是一个用 Rust 从零实现的轻量级 Git 托管平台，对标 Gitea/Forgejo。

- **二进制名**: `ironforge`（crate `rg-cli` 的 bin target）
- **目标**: 内存 <50MB、单二进制部署、全功能（仓库/Issue/PR/Wiki/CI）
- **当前阶段**: **Phase 1~20 全部完成**（核心功能 + Protocol V2 + 前端 i18n + P0 Gap + P1 增强 + CI/CD Runner + gix 迁移 + P2 功能 + 工程化）

---

## 仓库结构

```
ironforge/
├── Cargo.toml              # Workspace 根（统一依赖版本）
├── ARCHITECTURE.md         # 完整架构方案（必读！包含数据库模型、技术选型）
├── CLAUDE.md               # 本文件（AI 协作上下文）
├── CONTRIBUTING.md         # 开发规范
├── .ai/                  # AI Agent 接入规范（README + MCP配置 + prompt模板）
├── docs/
│   ├── p0-prd.md           # P0 功能 PRD
│   ├── p0-system-design.md # P0 系统设计 + 任务分解
│   └── git-protocol.md     # Git 协议实现细节与踩坑记录
├── crates/
│   ├── rg-cli/             # 主二进制入口（bin = "ironforge"）
│   ├── rg-core/            # 核心业务逻辑（✅ auth/user/repo/issue/pr/wiki/lfs/webhook/review/branch_protection/collaborator/org/notification/email）
│   ├── rg-git/             # Git 协议层（✅ 完整实现，RefUpdate 返回 push 信息）
│   ├── rg-ssh/             # SSH 服务端 russh（✅ 完整实现）
│   ├── rg-http/            # HTTP 服务端 + REST API（✅ 完整实现 + Git 协议鉴权 + 文件浏览 + 静态资源 + WebSocket + Rate Limit + 分页 + GPG）
│   ├── rg-db/              # 数据库层 SeaORM（✅ 实体+迁移+ops）
│   ├── rg-ci/              # CI/CD 引擎（✅ YAML 解析 + Pipeline 执行器 + Docker Runner）
│   ├── rg-runner/          # Runner Agent 独立二进制（bin = "ironforge-runner"）
│   └── rg-mcp/             # MCP 服务器（bin = "ironforge-mcp"，stdio/SSE，暴露 Tools + Resources 给 AI Agent）
└── web/                    # SvelteKit 前端（✅ 登录/仓库/Issue/PR/Wiki/CI/代码审查/组织/通知/国际化）
```

---

## 关键约定

### 命令规范

```bash
# 编译（请始终用 release 构建做集成测试）
cargo build --release

# 启动服务器
./target/release/ironforge serve \
  --repo-root /tmp/ironforge/repos \
  --http-addr 0.0.0.0:8080 \
  --ssh-addr  0.0.0.0:2222 \
  --host-key  /tmp/ironforge_host_key

# 创建测试仓库
./target/release/ironforge create-repo <owner> <repo> --repo-root /tmp/ironforge/repos
# → 创建 /tmp/ironforge/repos/<owner>/<repo>.git
```

### SSH 测试命令模板

```bash
SSH_CMD="ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null"
GIT_SSH_COMMAND="$SSH_CMD" git clone ssh://git@localhost:2222/testuser/testrepo /tmp/if_test
GIT_SSH_COMMAND="$SSH_CMD" git push origin main
```

### HTTP 路由前缀

HTTP Git 端点的路由前缀是 `/git/`（**不是** 直接 `/<owner>/<repo>`）：

```
GET  http://localhost:8080/git/<owner>/<repo>/info/refs?service=git-upload-pack
POST http://localhost:8080/git/<owner>/<repo>/git-upload-pack
POST http://localhost:8080/git/<owner>/<repo>/git-receive-pack
GET  http://localhost:8080/health
```

git clone 示例：

```bash
git clone http://localhost:8080/git/testuser/testrepo /tmp/if_http
```

---

## AI Agent 集成（MCP Server）

IronForge 通过 **MCP (Model Context Protocol)** 暴露仓库数据给 AI Agent（Claude Code / Cursor / Continue.dev 等）。

### 二进制

- **`ironforge-mcp`** — `rg-mcp` crate 的 bin target，位于 `target/debug/ironforge-mcp`

### 使用方式

```bash
# 编译
cargo build -p rg-mcp

# 作为子进程启动（AI Agent 会自动调用）
IRONFORGE_URL=http://localhost:8080 IRONFORGE_PAT=<token> ./target/debug/ironforge-mcp

# 测试（手动发送 JSON-RPC 请求）
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0.1"}}}' | ./target/debug/ironforge-mcp
```

### 暴露的 Tools

| Tool 名称 | 说明 |
|-----------|------|
| `list_repos` | 列出当前用户可访问的仓库 |
| `read_file` | 读取仓库文件内容（UTF-8） |
| `read_dir` | 列出仓库目录内容 |
| `get_issue` | 获取单个 Issue 详情 |
| `get_pr` | 获取单个 PR 详情（含 diff） |

### 暴露的 Resources

| URI 模板 | 说明 |
|-----------|------|
| `repo://{owner}/{name}` | 仓库元数据（JSON） |
| `file://{owner}/{name}/{path}` | 文件内容（text/plain） |
| `issue://{owner}/{name}/{number}` | Issue 详情（JSON） |

### 环境变量

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `IRONFORGE_URL` | `http://localhost:8080` | IronForge API 地址 |
| `IRONFORGE_PAT` | _(空)_ | Bearer Token（API 认证） |

### 支持的 Transport

- **stdio**（默认）— 作为 AI Agent 子进程运行
- **SSE**（`--sse` 标志）— 网页端 Agent（暂未实现）

---

## 实现现状（2026-05-11）

### ✅ 已完成（Phase 1 ~ Phase 20 + P0/P1/P2 Gap Analysis + 工程化）

| 模块 | 文件 | 说明 |
|------|------|------|
| pkt-line 协议 | `rg-git/src/pkt_line.rs` | 完整编解码 + **V2 Delim/ResponseEnd** |
| sideband-64k | `rg-git/src/sideband.rs` | band 1/2/3 |
| git-upload-pack | `rg-git/src/protocol/upload_pack.rs` | SSH + HTTP 模式 |
| git-receive-pack | `rg-git/src/protocol/receive_pack.rs` | SSH + HTTP 模式，返回 `Vec<RefUpdate>` |
| **Git Protocol V2** | `rg-git/src/protocol/v2.rs` | **ls-refs + fetch 命令 + capability advertisement** |
| **V2 HTTP 集成** | `rg-http/src/git_v2.rs` + `rg-http/src/lib.rs` | **Git-Protocol: version=2 header 检测 + V2 处理** |
| SSH 服务端 | `rg-ssh/src/lib.rs` | russh 0.51，auth_publickey/auth_password 查 DB |
| HTTP 服务端 | `rg-http/src/lib.rs` | Axum 0.8，/git/ 路由 + **Git 协议权限鉴权** + 分支保护审计 + **SvelteKit 静态资源** |
| REST API | `rg-http/src/api/` | Users + Repos + Issues + PRs + Wiki + LFS + Webhooks + CI/CD + **Reviews + Branch Protection + Collaborators + Repo Content** |
| 数据库实体 | `rg-db/src/entities/` | users / repositories / ssh_keys / access_tokens / issues / issue_comments / pull_requests / milestones / wiki_pages / lfs_objects / webhooks / webhook_deliveries / pipelines / pipeline_stages / pipeline_jobs / **pr_reviews / review_comments / protected_branches / repo_collaborators** / **labels / issue_labels / repo_watches / commit_statuses / release_assets** |
| DB 迁移 | `rg-db/src/migrations/` | m20260424_000001~000009 + m20260508_000001~000006 + m20260510_000001~000004 + m20260511_000001~000003，自动 up on start |
| 用户认证 | `rg-core/src/auth/` | argon2 password hash + JWT HS256 |
| 用户服务 | `rg-core/src/user/service.rs` | register / login |
| 仓库服务 | `rg-core/src/repo/service.rs` | create_repo + can_read/can_write（**集成 collaborator 权限**） |
| Issue 服务 | `rg-core/src/issue/service.rs` | CRUD + labels + milestone + comments |
| PR 服务 | `rg-core/src/pull_request/service.rs` | create + diff(git CLI) + merge(3策略) + **分支保护检查** |
| Wiki 服务 | `rg-core/src/wiki/service.rs` | 页面 CRUD（DB 存储） |
| LFS 服务 | `rg-core/src/lfs/service.rs` | batch API + 对象上传/下载（磁盘存储） |
| Webhook 服务 | `rg-core/src/webhook/service.rs` | 注册/触发/投递/HMAC-SHA256 签名 |
| CI/CD 引擎 | `rg-ci/src/` | YAML 解析 + Pipeline 执行器 + 后台运行 |
| Git 鉴权 | `rg-http/src/lib.rs` | HTTP git 协议 Bearer Token 认证 + can_read/can_write |
| **代码审查** | `rg-core/src/review/service.rs` | submit review (comment/approve/request_changes/dismiss) + inline comments |
| **分支保护** | `rg-core/src/branch_protection/service.rs` | protected branches + require PR + require approval + required status checks |
| **协作者** | `rg-core/src/collaborator/service.rs` | repo collaborators + read/write/admin permission |
| **文件浏览** | `rg-http/src/api/repo_content.rs` | tree/blob/log/branches/tags API (git CLI) |
| **Web UI** | `web/src/routes/` | SvelteKit 5 + SPA mode（登录/注册/Dashboard/仓库/Issue/PR/Wiki/CI） |
| **前端组件** | `web/src/lib/components/` | Navbar / Layout / RepoHeader / PipelineBadge |
| **API 客户端** | `web/src/lib/api/client.ts` | REST API 全量 TypeScript 封装 |
| **认证 Store** | `web/src/lib/stores/auth.ts` | JWT 状态管理（Svelte 5 runes） |
| **Docker Runner** | `rg-ci/src/runner.rs` | CI Job Docker 容器化执行（`docker run --rm` + volume mount） |
| **组织系统** | `rg-core/src/org/mod.rs` + `rg-http/src/api/orgs.rs` | CRUD + 成员管理 + 团队 + 权限 |
| **通知系统** | `rg-core/src/notification/mod.rs` + `rg-http/src/api/notifications.rs` | 创建/列表/已读/批量已读/删除 |
| **Rate Limiting** | `rg-http/src/rate_limit.rs` | Token Bucket 中间件（IP 限流 + 可配置窗口） |
| **WebSocket 通知** | `rg-http/src/ws.rs` | 实时通知推送（broadcast channel + JWT 认证） |
| **邮件通知** | `rg-core/src/email/mod.rs` | SMTP 邮件（lettre + HTML 模板） |
| **组织仓库** | `rg-core/src/repo/service.rs` | org_id 关联 + find_repo_by_owner_name |
| **权限鉴权完善** | `rg-core/src/repo/service.rs` | org member + team permission → can_read/can_write |
| **TLS/HTTPS** | `rg-http/src/lib.rs` | axum-server + rustls，CLI --tls-cert/--tls-key |
| **TOML 配置** | `rg-cli/src/main.rs` | 优先级 CLI > config > defaults，ironforge.example.toml |
| **日志轮转** | `rg-cli/src/main.rs` | tracing-appender RollingFileAppender (DAILY + non-blocking) |
| **API 分页** | `rg-http/src/pagination.rs` | PaginationParams + PaginatedResponse\<T\>，5 个 list API |
| **GPG 签名** | `rg-http/src/api/repo_content.rs` | GET /repos/:owner/:name/commits/:sha/signature |
| **Git V2** | `rg-git/src/protocol/v2.rs` | Protocol V2 HTTP 支持（ls-refs/fetch 命令） |
| **前端 i18n** | `web/src/lib/i18n/` | locale store + localStorage + 中/英翻译（199 key） |
| **代码覆盖率** | `cargo-llvm-cov` | LLVM 覆盖率工具，支持 HTML/LCOV/JSON 输出 |
| **P0: Star/Watch** | `rg-core/src/repo/service.rs` + `rg-http/src/api/repos.rs` | Star 计数 + Watch 三态 + Watch 列表查询 |
| **P0: 仓库删除** | `rg-core/src/repo/service.rs` | 软删除（deleted_at）+ Git 数据清理 |
| **P0: Releases/Tags** | `rg-core/src/release/service.rs` + `rg-http/src/api/repos.rs` | 创建/编辑/删除 Release + 关联 Tag + Asset 上传 |
| **P0: Labels CRUD** | `rg-db/src/entities/label.rs` + `rg-db/src/ops/label_ops.rs` | 独立 labels 表 + issue_labels 关联表 + 颜色/描述 |
| **P0: Milestones API** | `rg-http/src/api/issues.rs` | list/create/update/delete REST API |
| **P0: API Tokens/PAT** | `rg-http/src/api/users.rs` | 创建/吊销 PAT + Bearer Token 认证 |
| **P0: Fork 仓库** | `rg-core/src/repo/service.rs` | 复制 Git 数据 + fork_id 双向关联 |
| **P0: 仓库转移** | `rg-core/src/repo/service.rs` | POST /transfer，支持用户→用户/组织 |
| **P0: Commit Status** | `rg-db/src/entities/commit_status.rs` + `rg-core/src/repo/service.rs` | upsert(repo_id,sha,context) + combined status 聚合 |
| **P0: FTS5 搜索** | `rg-core/src/search/service.rs` + `rg-http/src/api/search.rs` | repos_fts/issues_fts/wiki_pages_fts + 触发器自动同步 |
| **P1: Labels-Issue 关联** | `rg-db/src/ops/issue_label_ops.rs` + `rg-http/src/api/issues.rs` | ?labels= 过滤 + GET issue labels |
| **P1: Webhooks 扩展** | `rg-core/src/webhook/service.rs` | 13 个事件（release/branch/tag/issue/PR/milestone）|
| **P1: Watch 通知** | `rg-core/src/notification/mod.rs` | push/PR/milestone 通知（排除 actor）|
| CLI | `rg-cli/src/main.rs` | clap 4，`serve`（含 --db-url, --jwt-secret, --docker, --rate-limit-*, --smtp-*, --tls-*, --config, --log-*, --external-runners）/ `create-repo` / `migrate` / `runner` |

### ✅ Phase 13 已完成（DB 分页 + V2 + Admin，2026-04-27~28）

- PaginatedResponse 统一分页（5 个 list API）
- Git Protocol V2 HTTP 集成完善
- Admin API 增强用户管理

### ✅ Phase 14-15 已完成（P0 Gap 补齐，2026-05-08~09）

- Star/Watch、仓库删除/转移、Releases/Tags
- Labels CRUD + Issue 关联、Milestones、PAT
- Fork 仓库、Commit Status、FTS5 搜索
- Webhooks 13 事件、Watch 通知

### ✅ Phase 16 已完成（P1 增强，2026-05-09）

- Webhooks 扩展（13 个事件）
- Watch 通知集成
- Labels-Issue 关联 API

### ✅ Phase 17 已完成（CI/CD Runner 收尾，2026-05-10）

- Runner Token Bearer 认证中间件（`authenticate_runner`）
- 外部 Runner 模式（`--external-runners` flag）
- Runner Agent 独立二进制（`crates/rg-runner/` → `ironforge-runner`）
- Artifact 管理（DB 迁移 + entity + ops + API 4 端点）
- Job 日志 WebSocket 实时推送（`/ws/job/:job_id`）
- Admin Runner 管理前端

### ✅ Phase 18 已完成（gix 迁移，2026-05-10）

- rg-ci CI 配置读取迁移（read_ci_config + has_ci_config → gix）
- rg-core checkout 迁移（git checkout ×2 → gix edit_reference）
- rg-core fast-forward 迁移（git merge --ff-only → gix repo.reference）
- 进度 50% → ~60%（18 → 13 处 git CLI 保留）

### ✅ Phase 19 已完成（P2 功能，2026-05-11）

- R-14: Fork PR 跨仓库支持（DB 迁移 + resolve_head_ref + 跨仓库 compute_diff/merge_pr）
- R-15: Release Asset HTTP 端点（upload/download/list/get/delete 5 个端点）
- R-16: Search API 细分（SearchFilters qualifier 解析 + search_issues/search_repos 过滤）

### ✅ Phase 20 已完成（工程化，2026-05-11）

- Step 1: 构建优化（release profile 已有 lto/opt-level/strip）
- Step 2: 统一错误处理（AppError enum + IntoResponse）
- Step 3: SQLite 性能调优（WAL + 7 项 PRAGMA 优化 + 连接池配置）
- Step 4: 配置校验（validate_config 拒绝危险默认值）
- Step 5: 健康检查增强（/health: DB ping + FS check + version/phase）
- Step 6: Request-ID 中间件（UUID v4 + tracing span）
- Step 7: Rate Limiter（Token bucket per-IP）
- Step 8: SQL 注入防护（参数化三元组 filter_clauses）
- Step 9: 集成测试（10 个 API 测试，9 passed / 1 ignored）
- Step 10: OpenAPI 全量覆盖（142 个 utoipa::path 注解 + Swagger UI /api-docs/）

所有 Phase 1~20 全部完成。剩余待完成项见下方。

---

## 重要踩坑（必读！）

在修改 Git 协议相关代码时，请务必了解以下已踩过的坑：

### 1. pkt-line 解析必须用 `read_pkt_line`，不能用 `read_line`

pkt-line 格式是 `<4 hex 字节长度><payload>`。长度包含自身 4 字节。
`read_line()` 会把 `004a...` 这样的长度头当成文本内容读进来，导致 UTF-8 解析失败或逻辑错误。
**正确方式**：始终使用 `rg_git::pkt_line::read_pkt_line(&mut BufReader::new(stream))`。

### 2. receive-pack 的 report-status 必须整体 sideband 封装

当服务端广告了 `side-band-64k` 能力（我们始终广告），客户端期望所有响应都通过 sideband 发送。

**错误做法**：先发 sideband flush `0000`，再发 plain pkt-lines。  
**正确做法**（已验证）：

```
① 把 report-status pkt-lines 写入内存 buf（unpack ok + ok/ng ref... + 0000）
② 整体用 sideband::write_sideband_data(writer, &report_buf) 发出（band 1）
③ 调用 sideband::write_sideband_flush(writer) 发 sideband flush
```

对应代码：`rg-git/src/protocol/receive_pack.rs` 中的 `send_response()` 函数。

### 3. russh ChannelStream 的关闭顺序

SSH 会话结束时必须按以下顺序操作，否则会丢失缓冲数据：

```rust
// ① 先发 exit-status（channel 还活着）
handle.exit_status_request(channel_id, exit_code).await?;

// ② 再 shutdown stream（发 SSH EOF，让客户端知道数据发完了）
stream.shutdown().await?;

// ③ stream drop → channel close
```

对应代码：`rg-ssh/src/lib.rs` 中 `exec_request` 的 `tokio::spawn` 块。

### 4. git push 发送的是 thin pack

客户端 `git push --thin` 发送 thin pack，服务端必须用：

```bash
git index-pack --fix-thin --stdin
```

不能用普通的 `git index-pack --stdin`，否则 pack 文件不完整。

### 5. git for-each-ref 不列出 HEAD

`git for-each-ref` 只列出 refs/heads/...、refs/tags/... 等，不包括 HEAD（符号引用）。
需要额外调用 `git rev-parse HEAD` 单独解析，且要校验返回值是 40 位 hex（空 repo 返回字面 "HEAD"）。

### 6. HTTP info/refs 路由的 Content-Type

git HTTP 协议对 Content-Type 极为敏感：

- `GET /info/refs?service=git-upload-pack` → `application/x-git-upload-pack-advertisement`
- `GET /info/refs?service=git-receive-pack` → `application/x-git-receive-pack-advertisement`
- `POST /git-upload-pack` → `application/x-git-upload-pack-result`
- `POST /git-receive-pack` → `application/x-git-receive-pack-result`

### 7. argon2 0.5 的 SaltString 用法

```rust
// 正确：
use password_hash::rand_core::OsRng;
let salt = SaltString::generate(&mut OsRng);

// 错误（rand 0.9 的 rng() 不满足 CryptoRngCore）：
use rand::rng;
let salt = SaltString::generate(&mut rng()); // ❌
```

### 8. axum 0.8 的 Router::nest() 类型约束

`Router::nest()` 要求前后 Router 的 State 类型一致。
推荐做法：把所有 route handler 先组成一个完整 Router，再统一加 `.with_state(state)`。

### 9. axum TLS 必须用 axum-server

- ❌ `tokio-rustls::TlsAcceptor` + `axum::serve(TcpStream)`：`TlsStream` 无法转 `TcpStream`
- ❌ `hyper` 直接处理：`Router` 不实现 `Service<Request<Incoming>>`
- ✅ `axum-server::bind_rustls()` + `RustlsConfig::from_config()`

### 10. serde default 函数类型匹配

`#[serde(default = "fn_name")]` 的函数返回类型必须与字段完全匹配。`Option<String>` 字段不能用返回 `String` 的函数，改用 `#[serde(default)]`（Option 自动 None）。

### 11. utoipa OpenAPI 注解注意事项

- `serde_json::Value` **不能**放在 `schemas()` 列表（不实现 ToSchema）；在 path 注解中用 `request_body(content = serde_json::Value)` 代替
- 通过 `route_layer()` 注册的路由不会被 `.route()` 正则匹配发现，`__path_*` 符号缺失需手动排除
- 添加 `use utoipa::ToSchema;` 时**不能**插入到 `use axum::{` 块内（导致 proc-macro 解析失败）
- handler 名冲突（如 `register` 同时在 users 和 runners 模块）需用 `module::handler` 做 key

### 12. SQLite FTS5 触发器的 'delete' 命令

FTS5 的 `INSERT INTO fts_table(fts_table, rowid, ...) VALUES('delete', ...)` 特殊命令**不接受内容列值**，会导致 `SQL logic error`。
**正确方式**：用标准 SQL `DELETE FROM fts_table WHERE rowid = old.id` 代替。

---

## 开发工作流

### 新功能开发流程

1. 阅读 `ARCHITECTURE.md` 对应章节了解设计意图
2. 确认要修改的 crate 和文件
3. 先写单元测试（或端到端测试脚本）
4. 实现功能
5. `cargo build --release` 验证编译
6. 端到端测试验证（见 README.md 中的测试脚本）
7. 更新本文件中的"实现现状"表格

### 后续开发建议

所有 Phase 1~20 + P0/P1/P2 Gap Analysis 已完成。

**推荐下一步优先级：**

#### P0 — 核心缺口（详细方案见 `docs/p0-completion-plan.md`）
1. **`/search/code` 端点** — gix tree 遍历 + 关键词匹配（复杂度高，3-5 天）
2. **Runner 内部端点 OpenAPI** — heartbeat/poll_job/start_job/upload_log/finish_job 5 个端点缺 utoipa 注解（0.5 天）
3. **SSH Protocol V2 完整实现** — `handle_v2_stream_impl` 当前为空壳（仅发送 capabilities），需要添加命令处理循环（2-3 天）

#### P1 — 重要功能（预计 7-10 周）
4. **PR Merge 完整策略** — Squash/Merge/Rebase（依赖 gix 迁移完成）
5. **包注册表 Docker/OCI** — 容器镜像仓库（完全未实现）
6. **OAuth2/OIDC 增强** — 完善第三方登录
7. **Actions Concurrency** — workflow 并发控制
8. **Least-privilege Token** — Actions token 权限控制

#### 技术债
9. **gix 迁移继续** — 剩余 13 处 CLI 调用（GPG×2, Pack×3, Rebase×2, Merge×2, Commit×1, Diff×2, Clone×1）

#### P2 — 增强功能
10. 包注册表扩展（NPM/PyPI/Maven）、Wiki 完善、PR Review 增强、Issue 关联/时间追踪、Webhook 增强、搜索高亮、Subpath 归档下载、CI/CD 增强（Runner 禁用/Re-run/可视化）

---

## 依赖版本速查

```toml
axum            = "0.8"
axum-server     = "0.7"      # features: tls-rustls
tower           = "0.5"
tower-http      = "0.6"      # features: cors, trace, fs
russh           = "0.51"
russh-keys      = "0.45"
sea-orm         = "1.1"      # features: sqlx-sqlite, runtime-tokio-rustls, macros
clap            = "4"        # features: derive
tokio           = "1"        # features: full
serde           = "1"        # features: derive
serde_json      = "1"
toml            = "0.8"
tracing         = "0.1"
tracing-subscriber = "0.3"   # features: env-filter
tracing-appender = "0.2"
rustls-pemfile  = "2"
tokio-rustls    = "0.26"
lettre          = "0.11"     # default-features = false, features: tokio1-rustls-tls, builder, smtp-transport
utoipa          = "4"        # features: axum_extras
utoipa-swagger-ui = "7"      # Swagger UI 嵌入
anyhow          = "1"
thiserror       = "2"
```

---

## 常见错误排查

| 错误信息 | 原因 | 解决方案 |
|----------|------|----------|
| `fatal: the remote end hung up unexpectedly` | SSH 流关闭时机不对 | 确保按 exit_status → shutdown → drop 顺序 |
| `bad band #110` | HTTP receive-pack 响应没有 sideband 编码 | report-status 必须包在 band-1 sideband 中 |
| `bad line length character: unpa` | 发送了 plain pkt-lines 但客户端期望 sideband | 整体用 write_sideband_data 包装 |
| `stream did not contain valid UTF-8` | 用 read_line 读了 pkt-line 二进制头 | 改用 read_pkt_line |
| `nul byte found in provided data` | 向 Command::arg() 传了含 NUL 的字符串 | 先用 split('\0').next() 剥离 capabilities |
| `the feature requires unstable` | 用了需要 nightly 的 gix API | 用系统 git 命令替代 |
| `--repo-root` not found | CLI 用法错误 | 必须加 `serve` 子命令：`ironforge serve --repo-root ...` |
| `HEAD` not found in ref list | `git for-each-ref` 不列出 HEAD | 用 gix API (`repo.references().all()`) 替代，它会正确返回 HEAD |
| `fatal: not a valid ref` (HTTP clone) | Content-Type 不正确 | 确保 `info/refs` 响应使用 `application/x-git-*-advertisement` |
| `pack has delta resolution error` | thin pack 未加 `--fix-thin` | `git index-pack` 必须加 `--fix-thin` 参数 |
| handler 返回类型编译错误 | Axum handler 返回类型不一致 | 同一 handler 不能混用 `(StatusCode, Json)` 和 `Html` |
| JSON 响应 `data` 字段为空 | `PaginatedResponse` 未用 `to_value()` 包装 | 必须用 `serde_json::to_value(resp)` 包装后返回 |
| Axum TLS 报错 | 用了 `axum::serve()` 而不是 `axum_server` | TLS 必须用 `axum_server::bind_rustls()` |
| SeaORM 批量删除不生效 | 用了错误的方法 | 必须用 `Entity::delete_many().filter(...).exec(db)` |
| SeaORM 单行更新失败 | 直接构造 ActiveModel | 必须先 `find_by_id()` 再 `into_active_model()` |
| russh `fingerprint()` 编译错误 | 缺少 `HashAlg` 参数 | 必须传 `HashAlg::Sha256` |
| SSH 认证死循环 | `Auth::Reject` 未设 `partial_success: false` | 必须带 `partial_success: false` |
| FTS5 触发器语法错误 | 用了不正确的 SQL 语法 | 必须用 `DELETE FROM fts WHERE rowid = old.id` |
| 级联编译错误 | `mod.rs` 缺少子模块声明 | 检查 `mod.rs` 是否列出了所有子模块 |

---

## 与其他 AI 工具协作说明

本项目同时维护多份 AI 上下文文件，不同工具读取不同文件：

### 文档定位

| 文件 | 定位 | 适用场景 |
|------|------|---------|
| `AGENT.md` | **所有 AI 工具的轻量统一入口** | 快速了解项目概览、技术栈、关键文件速查 |
| `CLAUDE.md` | **Claude Code 默认入口 + 所有 AI 的深度参考** | 完整的踩坑记录、依赖版本、常见错误排查、实现现状清单 |
| `ARCHITECTURE.md` | 架构设计文档 | 设计新功能时了解技术选型和模块设计 |
| `CONTRIBUTING.md` | 开发规范 | 写新代码时遵循编码规范 |

### Claude Code
- **自动读取**: `CLAUDE.md`（本文件）+ `AGENT.md`
- **特点**: Claude Code 会优先读取 `CLAUDE.md`，同时会读取 `AGENT.md` 作为补充
- **建议**: 以本文件为主，AGENT.md 为辅

### Codex / Trae / CodeBuddy / 其他 AI 工具
- **自动读取**: `AGENT.md`（优先）+ `CLAUDE.md`（同时）
- **特点**: 这些工具通常优先读取 `AGENT.md` 作为统一入口，同时会读取 `CLAUDE.md` 获取深度上下文
- **建议**: 以 AGENT.md 为概览，本文件为深度参考

### WorkBuddy（本项目的主要 AI 协作工具）
- **自动读取**: `.workbuddy/memory/MEMORY.md`（长期经验） + `.workbuddy/memory/YYYY-MM-DD.md`（每日日志）
- **建议补充**: `AGENT.md` + `CLAUDE.md`（本文件）+ `ARCHITECTURE.md`（架构设计）
- WorkBuddy 在每次会话开始时会自动读取记忆文件，保持跨会话的上下文连续性

### 记忆文件位置
```
$WORKSPACE/.workbuddy/memory/
├── MEMORY.md                           # 长期经验积累（踩坑、架构决策、文档阅读指南）
├── doc-code-inconsistencies.md         # 文档与代码不一致问题追踪
└── YYYY-MM-DD.md                       # 每日工作日志
```

### 分析报告位置
```
$WORKSPACE/ironforge-docs/
├── README.md                           # 报告索引 + 项目状态总览
├── gitea-feature-gap-analysis.md       # vs Gitea 1.26 功能差距（⚠️ Phase 17 之前）
├── gix-migration-feasibility-analysis.md # gix 迁移可行性评估
├── gix-migration-status-report.md      # gix 迁移进度（⚠️ Phase 18 之前）
└── ci-runner-architecture.md           # CI Runner 架构设计
```

### 项目文档位置
```
$WORKSPACE/ironforge/docs/
├── git-protocol.md                     # Git 协议实现细节与踩坑记录
├── p0-prd.md                           # P0 功能 PRD（产品需求）
└── p0-system-design.md                 # P0 系统设计 + 任务分解
```
