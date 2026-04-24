# IronForge — AI 协作上下文

> 本文件供 AI 编程助手（Claude Code、GitHub Copilot Workspace、Codex、Trae、WorkBuddy 等）读取，
> 提供项目关键背景、约定和常见任务的操作指南。
> **每次开始工作前请先通读本文件。**

---

## 项目简介

**IronForge**（铁匠铺）是一个用 Rust 从零实现的轻量级 Git 托管平台，对标 Gitea/Forgejo。

- **二进制名**: `ironforge`（crate `rg-cli` 的 bin target）
- **目标**: 内存 <50MB、单二进制部署、全功能（仓库/Issue/PR/Wiki/CI）
- **当前阶段**: **Phase 10 已完成**（全部 10 个 Phase 完成：Git 协议 + 用户系统 + Issue/PR + Wiki/LFS/Webhook + CI/CD + 代码审查 + Web UI + 组织/通知 + WebSocket/邮件 + TLS/配置/分页/GPG）

---

## 仓库结构

```
ironforge/
├── Cargo.toml              # Workspace 根（统一依赖版本）
├── ARCHITECTURE.md         # 完整架构方案（必读！包含数据库模型、技术选型）
├── CLAUDE.md               # 本文件（AI 协作上下文）
├── CONTRIBUTING.md         # 开发规范
├── docs/
│   └── git-protocol.md     # Git 协议实现细节与踩坑记录
└── crates/
    ├── rg-cli/             # 主二进制入口（bin = "ironforge"）
    ├── rg-core/            # 核心业务逻辑（✅ auth/user/repo/issue/pr/wiki/lfs/webhook/review/branch_protection/collaborator/org/notification/email）
    ├── rg-git/             # Git 协议层（✅ 完整实现，RefUpdate 返回 push 信息）
    ├── rg-ssh/             # SSH 服务端 russh（✅ 完整实现）
    ├── rg-http/            # HTTP 服务端 + REST API（✅ 完整实现 + Git 协议鉴权 + 文件浏览 + 静态资源 + WebSocket + Rate Limit + 分页 + GPG）
    ├── rg-db/              # 数据库层 SeaORM（✅ 实体+迁移+ops）
    ├── rg-ci/              # CI/CD 引擎（✅ YAML 解析 + Pipeline 执行器 + Docker Runner）
    └── web/                # SvelteKit 前端（✅ 登录/仓库/Issue/PR/Wiki/CI/代码审查/组织/通知）
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

## 实现现状（Phase 10 完成，2026-04-24）

### ✅ 已完成（Phase 1 ~ Phase 10）

| 模块 | 文件 | 说明 |
|------|------|------|
| pkt-line 协议 | `rg-git/src/pkt_line.rs` | 完整编解码 |
| sideband-64k | `rg-git/src/sideband.rs` | band 1/2/3 |
| git-upload-pack | `rg-git/src/protocol/upload_pack.rs` | SSH + HTTP 模式 |
| git-receive-pack | `rg-git/src/protocol/receive_pack.rs` | SSH + HTTP 模式，返回 `Vec<RefUpdate>` |
| SSH 服务端 | `rg-ssh/src/lib.rs` | russh 0.51，auth_publickey/auth_password 查 DB |
| HTTP 服务端 | `rg-http/src/lib.rs` | Axum 0.8，/git/ 路由 + **Git 协议权限鉴权** + 分支保护审计 + **SvelteKit 静态资源** |
| REST API | `rg-http/src/api/` | Users + Repos + Issues + PRs + Wiki + LFS + Webhooks + CI/CD + **Reviews + Branch Protection + Collaborators + Repo Content** |
| 数据库实体 | `rg-db/src/entities/` | users / repositories / ssh_keys / access_tokens / issues / issue_comments / pull_requests / milestones / wiki_pages / lfs_objects / webhooks / webhook_deliveries / pipelines / pipeline_stages / pipeline_jobs / **pr_reviews / review_comments / protected_branches / repo_collaborators** |
| DB 迁移 | `rg-db/src/migrations/` | m20260424_000001~000009，自动 up on start |
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
| CLI | `rg-cli/src/main.rs` | clap 4，`serve`（含 --db-url, --jwt-secret, --docker, --rate-limit-*, --smtp-*, --tls-*, --config, --log-*）/ `create-repo` |

### ✅ Phase 10 已完成（TLS + 配置文件 + 日志轮转 + API 分页 + GPG 签名）

所有 10 个 Phase 全部完成。后续可考虑：
- 性能优化（数据库层分页替代应用层分页）
- 更多 Git 协议支持（Smart Protocol V2、protocol.inforefs）
- 国际化（i18n）
- 嵌入式搜索（全文检索代码/Issue/Wiki）
- API 文档（OpenAPI/Swagger）

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

所有 10 个 Phase 已完成，后续优化方向：

1. **数据库层分页**：当前分页是应用层 skip/take，改为 SeaORM Paginator 真正数据库分页
2. **Git Smart Protocol V2**：支持 V2 协议提升性能
3. **全文搜索**：代码/Issue/Wiki 搜索
4. **API 文档**：OpenAPI/Swagger 自动生成
5. **国际化**：前端 i18n

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

---

## 与 WorkBuddy 协作说明

本项目同时维护工作记忆文件：

- `$WORKSPACE/.workbuddy/memory/MEMORY.md`：长期经验积累（踩坑、架构决策）
- `$WORKSPACE/.workbuddy/memory/YYYY-MM-DD.md`：每日工作日志

WorkBuddy 在每次会话开始时会自动读取这些文件，保持跨会话的上下文连续性。
如果你是其他 AI 工具，可以手动读取 `MEMORY.md` 获取项目历史背景。
