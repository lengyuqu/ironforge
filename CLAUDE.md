# IronForge — AI 协作上下文

> **Claude Code 默认入口**，也是所有 AI 编程助手的深度参考文档。
> 提供踩坑记录、依赖版本速查、常见错误排查和实现现状清单。
> 轻量概览见 [AGENT.md](AGENT.md)；完整文档索引见 [ironforge-docs/README.md](ironforge-docs/README.md)。

> **状态**：维护中 ｜ **可信度**：确认 ｜ **来源**：仓库代码 / 文档 / 运行结果 ｜ **最后更新**：2026-08-14

---

## 项目简介

**IronForge**（铁匠铺）是一个用 Rust 从零实现的轻量级 Git 托管平台，对标 Gitea/Forgejo。内存 <50MB、单二进制部署、全功能（仓库/Issue/PR/Wiki/CI/包注册表/企业认证/审计/代码搜索）。Phase 1~21 全部完成。

---

## 关键约定

### Agent 生命周期护栏

> ⚠️ **所有 AI Agent 执行变更操作前必须阅读 [`.ai/guardrails.md`](.ai/guardrails.md)**

护栏等级：🔴 BLOCK（禁止） / 🟠 CONFIRM（需确认） / 🟡 WARN（需警告） / 🟢 ALLOW（允许）。

### 证据分层与可信度标注

> 对齐 mybook `Templates` 通用规范：文档中的关键事实应标注来源与可信度，验证声明逐层区分，不得用前一层替代后一层。

**事实来源优先级**（高 → 低）：仓库代码 > 运行结果（命令输出/测试/实库验证）> 项目文档 > 经验与推测（仅作候选，执行成功前不得标为"当前通过"）。

**可信度标注**：关键事实标注 `确认` / `推测` / `待验证`。本文件中"✅ 已完成"类声明应尽量附上验证证据（测试命令、实库结果或 CI 状态）；仅有文档/静态检查通过的，不得声称业务运行已通过。

**证据分层**（逐层陈述，前层不可替代后层）：

| 层级 | 可证明 | 不能替代 |
|------|--------|----------|
| 实现证据 | 文件存在于明确 diff 或 SHA | 命令通过 |
| 任务验证 | 任务分支上的当前命令通过 | 合并后仍通过 |
| 集成验证 | 合并后的基线通过门禁 | 真实运行或发布 |
| 运行验证 | 浏览器/设备/后端/数据库链路已实际执行 | 生产发布 |
| 远端验证 | live remote ref 等于目标 SHA | CI 或部署成功 |
| 发布验证 | 制品/迁移/部署/观察达到发布条件 | 无 |

### HTTP 路由前缀

Git HTTP 端点路由前缀是 `/git/`（**不是**直接 `/<owner>/<repo>`）：

```
GET  /git/<owner>/<repo>/info/refs?service=git-upload-pack
POST /git/<owner>/<repo>/git-upload-pack
POST /git/<owner>/<repo>/git-receive-pack
GET  /health
```

### 命令与测试

编译、启动、测试命令详见 [README.md](README.md) 的「快速开始」和「开发」章节。
Git 协议测试命令模板见 [README.md](README.md) 的「Git 操作」章节。
完整回归测试入口见 [CONTRIBUTING.md](CONTRIBUTING.md) 的「测试规范」章节。

### AI Agent 集成（MCP Server）

MCP Server（`ironforge-mcp`，stdio-only）暴露仓库数据给 AI Agent。完整配置和使用指南见 [`.ai/README.md`](.ai/README.md)。

---

## 实现现状（2026-07-14）

### 功能概览

Phase 1~21 全部完成。核心能力：

| 领域 | 模块 | 说明 |
|------|------|------|
| Git 协议 | `rg-git` | V1/V2、pkt-line、sideband、upload-pack、receive-pack、GitCommandGateway |
| SSH 服务 | `rg-ssh` | russh 0.51，公钥/密码认证，Deploy Key |
| HTTP 服务 | `rg-http` | Axum 0.8，REST + Git Smart HTTP + OCI + WebSocket + OpenAPI + SPA |
| 业务逻辑 | `rg-core` | auth/user/repo/issue/pr/wiki/lfs/webhook/review/branch_protection/collaborator/org/notification/email/package_registry/mirror/board/time_tracking/import/audit/search/code_indexer |
| 数据库 | `rg-db` | SeaORM 1.1 + SQLite/PostgreSQL/MySQL，自动迁移 |
| CI/CD | `rg-ci` + `rg-runner` | YAML 解析、Pipeline 执行器、Docker Runner、外部 Runner、Secrets/Matrix/Cache/Environment/OIDC/Retention |
| 前端 | `web/` | SvelteKit 5 SPA，中英双语 i18n（199 key） |
| MCP | `rg-mcp` | stdio-only，暴露 Tools + Resources 给 AI Agent |

### 详细实现清单

| 模块 | 文件 | 说明 |
|------|------|------|
| pkt-line 协议 | `rg-git/src/pkt_line.rs` | 完整编解码 + V2 Delim/ResponseEnd |
| sideband-64k | `rg-git/src/sideband.rs` | band 1/2/3 |
| git-upload-pack | `rg-git/src/protocol/upload_pack.rs` | SSH + HTTP 模式 |
| git-receive-pack | `rg-git/src/protocol/receive_pack.rs` | SSH + HTTP，返回 Vec\<RefUpdate\> |
| Git Protocol V2 | `rg-git/src/protocol/v2.rs` | ls-refs/fetch/object-info；shallow/deepen/partial-clone |
| V2 HTTP 集成 | `rg-http/src/git_v2.rs` | Git-Protocol: version=2 header 检测 |
| GitCommandGateway | `rg-git/src/cli_gateway.rs` | 全部 git 子进程统一入口，防回归守卫 |
| SSH 服务端 | `rg-ssh/src/lib.rs` | russh 0.51，公钥/密码认证查 DB |
| HTTP 服务端 | `rg-http/src/lib.rs` | Axum 0.8，Git 协议鉴权 + 分支保护 + SvelteKit 静态资源 |
| REST API | `rg-http/src/api/` | 30+ API 模块，142 个 OpenAPI 注解 |
| 用户认证 | `rg-core/src/auth/` | argon2 + JWT HS256 + LDAP + SSO(OIDC) + TOTP MFA |
| 仓库服务 | `rg-core/src/repo/service.rs` | create_repo + can_read/can_write（集成 collaborator/org 权限） |
| Issue 服务 | `rg-core/src/issue/service.rs` | CRUD + labels + milestone + comments + Markdown 模板 |
| PR 服务 | `rg-core/src/pull_request/service.rs` | create + diff + merge(3策略) + 分支保护 + Merge Queue |
| 代码审查 | `rg-core/src/review/service.rs` | submit review + inline comments + 不可变事件流 |
| 分支保护 | `rg-core/src/branch_protection/service.rs` | require PR + require approval + required status checks + require signed commits |
| 协作者 | `rg-core/src/collaborator/service.rs` | read/write/admin 权限 |
| Wiki 服务 | `rg-core/src/wiki/service.rs` | 页面 CRUD + 版本历史 + diff |
| LFS 服务 | `rg-core/src/lfs/service.rs` | batch API + 对象上传/下载 + HMAC URL |
| 统一 BlobStorage | `rg-core/src/blob_storage.rs` | 原子本地 backend；LFS/Package/OCI/Artifact/Release 接入 |
| Webhook 服务 | `rg-core/src/webhook/service.rs` | 13 事件 + HMAC-SHA256 签名 + 投递记录 |
| CI/CD 引擎 | `rg-ci/src/` | YAML 解析 + Pipeline 执行器 + Docker Runner + Gitea Actions adapter |
| 组织系统 | `rg-core/src/org/mod.rs` | CRUD + 成员 + 团队 + 权限 |
| 通知系统 | `rg-core/src/notification/mod.rs` | 创建/列表/已读/批量已读/删除 |
| Rate Limiting | `rg-http/src/rate_limit.rs` | Token Bucket 中间件 |
| WebSocket | `rg-http/src/ws.rs` | 通知按用户隔离，Job 日志按 job_id 隔离 |
| 邮件通知 | `rg-core/src/email/mod.rs` | SMTP（lettre）+ HTML 模板 |
| Package Registry | `rg-core/src/package_registry/` | 10 种适配器 + OCI Distribution Spec v1.0 |
| 审计日志 | `rg-core/src/audit/` | audit! 宏 + 90 天归档（NDJSON.zst） |
| Mirror | `rg-core/src/mirror/` | 仓库镜像同步 |
| Board | `rg-core/src/board/` | 看板管理（Board/Column/Card） |
| Time Tracking | `rg-core/src/time_tracking/` | 工时追踪 |
| 数据导入 | `rg-core/src/import/` | GitHub/GitLab 导入 |
| 搜索 | `rg-core/src/search/` | FTS5 全文搜索（repos/issues/wiki/code） |
| 附件 | `rg-core/src/attachment.rs` | 四类归属、统一 BlobStorage、配额、IDOR 防护 |
| 多数据库后端 | `rg-db::connect_with_pool` | SQLite/PostgreSQL/MySQL，backend-aware 迁移 |
| TLS/HTTPS | `rg-http/src/lib.rs` | axum-server + rustls |
| TOML 配置 | `rg-cli/src/main.rs` | CLI > config > defaults |
| API 分页 | `rg-http/src/pagination.rs` | PaginationParams + PaginatedResponse\<T\> |
| GPG 签名 | `rg-http/src/api/repo_content.rs` | GET /repos/:owner/:name/commits/:sha/signature |
| 前端 i18n | `web/src/lib/i18n/` | locale store + localStorage + 中/英翻译 |
| CI Secrets/Matrix | `rg-ci` + `rg-runner` | AES-256-GCM 加密、Matrix 256 上限 |
| CI Environment | `rg-ci` | 受保护环境、审批人/审批数 |
| CI OIDC | `/api/v1/ci/oidc/*` | Ed25519 JWKS、5 分钟 audience-bound token |
| CI Retention | `rg-ci` | 仓库级 Artifact/Cache 保留期 |
| Tag 保护 | `protected_tags` + receive-pack | 通配 Tag 规则 |
| 签名提交强制 | `protected_branches.require_signed_commits` | pack 入库后验证 |
| LDAP/MFA | `rg-core/src/auth/ldap.rs` + `sso.rs` + `totp.rs` | RFC4515 转义、PKCE S256、五次锁定 |

### 技术债与后续方向

**gix 迁移**：raw git 全消除（经 GitCommandGateway），gix 原生覆盖率 ~70%。16 处 CLI 经网关保留（Diff/Fetch/Rebase/Pack/GPG/Clone）。Phase 3 等待 gix 上游成熟：

| 待办 | 阻塞原因 | 解除条件 |
|---|---|---|
| Rebase 合并 | gix-rebase 无 API | gix 发布稳定 rebase API |
| Pack 生成 | gix 无高层 pack 协商 | gix 提供 server 端 pack 生成 |
| Thin-pack 索引 | gix 缺 thin 补全解析 | gix-pack 支持 --fix-thin |
| GPG 验签 | gix 无验签 | gix 内建或引入 sequoia-openpgp |
| blob-diff patch | 字节一致性待验证 | 对拍测试通过 |

复查节奏：每次 gix 版本升级时过一遍。

详细架构事实见 [ironforge-docs/architecture/project-architecture-2026-07.md](ironforge-docs/architecture/project-architecture-2026-07.md)。

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

### 13. 迁移 `#[derive(Iden)]` 生成的是**单数**表名 ⚠️

迁移里写 `#[derive(Iden)] enum Organization { Table }`，SeaORM 生成的表名是 **单数** `organization`；但实体声明的是 **复数** `#[sea_orm(table_name = "organizations")]`。两者一旦不一致，该实体所有查询运行时报 `no such table: organizations`，后续 ALTER 迁移会让服务启动崩溃。

**正确方式**：
1. 新增表时显式指定表名（`#[sea_orm(iden = "things")]`），确认与实体 `table_name` 完全一致
2. 用全新库验证：`ironforge migrate` + `sqlite3 .tables` 核对
3. 新功能模块务必补集成测试

### 14. 迁移应幂等 + AppState 字段变更要同步测试构造器

- `ALTER TABLE ... ADD COLUMN` 等非幂等语句用 `manager.has_table()/has_column()` 守卫
- 给 `AppState` 新增字段时，**必须**同步更新 `crates/rg-http/tests/common/mod.rs::build_test_app_state`

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
utoipa          = "5"        # ⚠️ 未纳入 workspace，在 rg-http 中硬编码
utoipa-swagger-ui = "8"      # ⚠️ 未纳入 workspace
anyhow          = "1"
thiserror       = "2"
gix             = "0.84"     # features: blocking-http-transport-curl, max-performance, blob-diff, pack-cache-lru-dynamic, merge
chrono          = "0.4"      # features: serde
uuid            = "1"        # features: v4, serde
# Auth / Crypto
argon2          = "0.5"
jsonwebtoken    = "9"
password-hash   = "0.5"
rand_core       = "0.6"
aes-gcm         = "0.10"     # SSO/LDAP 敏感配置加密
hmac            = "0.12"
sha2            = "0.10"
hex             = "0.4"
base64          = "0.22"
# 2FA / TOTP
totp-rs         = "5"        # features: gen_secret, otpauth
qrcode          = "0.14"
rand            = "0.8"
# LDAP
ldap3           = "0.11"     # features: tls
# OAuth2（声明但未直接使用，SSO 通过 reqwest 手动实现）
oauth2          = "5"        # ⚠️ 未直接使用
openidconnect   = "4"        # ⚠️ 未直接使用
# 其他
zstd            = "0.13"     # LFS 压缩（⚠️ 未纳入 workspace，在 rg-core 中硬编码）
reqwest         = "0.12"     # features: json
flate2          = "1"
tar             = "0.4"
home            = "0.5"
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
| `--repo-root` not found | CLI 用法错误 | 必须加 `serve` 子命令 |
| `HEAD` not found in ref list | `git for-each-ref` 不列出 HEAD | 用 gix API (`repo.references().all()`) 替代 |
| `fatal: not a valid ref` (HTTP clone) | Content-Type 不正确 | 确保 `info/refs` 响应使用正确的 advertisement Content-Type |
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
