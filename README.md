# IronForge 🔨

> **铁匠铺** — 一个用 Rust 从零实现的轻量级 Git 托管平台

[![Rust](https://img.shields.io/badge/rust-1.95%2B-orange)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

IronForge 对标 [Gitea](https://gitea.com/) / [Forgejo](https://forgejo.org/)，目标是用纯 Rust 实现一个内存占用极低（<50MB）、单二进制部署的全功能 Git 托管平台，支持仓库管理、Issue、Pull Request、Wiki、CI/CD、包注册表、企业认证、审计和代码搜索。

> Phase 1~21 全部完成。详细实现清单见 [CLAUDE.md](CLAUDE.md)。

---

## 快速开始

### 环境要求

- Rust 1.95+（推荐 stable）
- Node.js（前端构建与回归脚本）
- git（系统命令，用于 pack-objects / index-pack / update-ref / diff）
- macOS 或 Linux

### 编译

```bash
git clone <this-repo>
cd ironforge
cargo build --release
```

二进制产物位于 `target/release/ironforge`。

### 生成 SSH 主机密钥

首次运行需要一个 SSH 主机密钥：

```bash
ssh-keygen -t ed25519 -f /tmp/ironforge_host_key -N ""
```

### 启动服务器

```bash
# 创建仓库根目录
mkdir -p /tmp/ironforge/repos

# 启动（HTTP :8080 + SSH :2222 + SQLite）
./target/release/ironforge serve \
  --repo-root /tmp/ironforge/repos \
  --http-addr 0.0.0.0:8080 \
  --ssh-addr  0.0.0.0:2222 \
  --host-key  /tmp/ironforge_host_key \
  --db-url    sqlite:///tmp/ironforge/ironforge.db?mode=rwc \
  --jwt-secret my-secret-key
```

参数说明：

| 参数 | 说明 | 默认值 |
|------|------|--------|
| `--repo-root` | 裸仓库存储根目录 | 必填 |
| `--http-addr` | HTTP 监听地址 | `0.0.0.0:8080` |
| `--ssh-addr` | SSH 监听地址 | `0.0.0.0:2222` |
| `--host-key` | SSH 主机密钥路径 | 必填（缺失时自动生成） |
| `--db-url` | 数据库 URL | `sqlite:///tmp/ironforge/ironforge.db?mode=rwc` |
| `--jwt-secret` | JWT 签名密钥 | 必填 |
| `--config` | TOML 配置文件路径 | 无 |
| `--tls-cert` | TLS 证书 PEM 路径 | 无（启用 HTTPS） |
| `--tls-key` | TLS 私钥 PEM 路径 | 无（启用 HTTPS） |
| `--log-file` | 日志文件路径 | 无（输出到 stderr） |
| `--log-max-files` | 最大日志文件数 | 10 |
| `--docker` | 启用 Docker CI runner | false |
| `--rate-limit-max` | 限流最大请求数 | 100 |
| `--rate-limit-window` | 限流窗口（秒） | 60 |
| `--smtp-host` | SMTP 服务器地址 | 无（禁用邮件） |
| `--smtp-port` | SMTP 端口 | 587 |
| `--smtp-user` | SMTP 用户名 | 无 |
| `--smtp-pass` | SMTP 密码 | 无 |
| `--smtp-from` | 发件人地址 | 无 |

启动时自动运行数据库迁移，无需手动建表。

日志级别通过环境变量控制：

```bash
RUST_LOG=debug ./target/release/ironforge serve ...
```

### 创建测试仓库

```bash
./target/release/ironforge create-repo testuser testrepo \
  --repo-root /tmp/ironforge/repos
# → 创建 /tmp/ironforge/repos/testuser/testrepo.git
```

---

## REST API

所有 API 在 `/api/v1/` 下，需要认证的接口在 Header 中传 `Authorization: Bearer <token>`。完整 OpenAPI 规范可从运行中的服务获取：`/api-docs/openapi.json`（Swagger UI: `/api-docs/`）。

### 用户

```bash
# 注册
curl -X POST http://localhost:8080/api/v1/users/register \
  -H "Content-Type: application/json" \
  -d '{"username":"testuser","email":"test@example.com","password":"secret123"}'

# 登录（返回 JWT token）
curl -X POST http://localhost:8080/api/v1/users/login \
  -H "Content-Type: application/json" \
  -d '{"username":"testuser","password":"secret123"}'

# 查看当前用户
curl http://localhost:8080/api/v1/users/me \
  -H "Authorization: Bearer <token>"
```

### 仓库

```bash
# 创建仓库
curl -X POST http://localhost:8080/api/v1/repos \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"name":"myrepo","description":"test repo"}'

# 列出用户仓库
curl http://localhost:8080/api/v1/repos/testuser

# 查看仓库详情
curl http://localhost:8080/api/v1/repos/testuser/myrepo
```

### Issue

```bash
# 创建 Issue
curl -X POST http://localhost:8080/api/v1/repos/testuser/myrepo/issues \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"title":"Bug report","body":"Something is wrong","labels":"bug","milestone_id":1}'

# 列出 Issue（?state=open/closed/all）
curl "http://localhost:8080/api/v1/repos/testuser/myrepo/issues?state=open" \
  -H "Authorization: Bearer <token>"

# 查看详情 / 更新 / 添加评论
curl http://localhost:8080/api/v1/repos/testuser/myrepo/issues/1 -H "Authorization: Bearer <token>"
curl -X PATCH http://localhost:8080/api/v1/repos/testuser/myrepo/issues/1 \
  -H "Authorization: Bearer <token>" -H "Content-Type: application/json" \
  -d '{"title":"Updated title","state":"closed"}'
curl -X POST http://localhost:8080/api/v1/repos/testuser/myrepo/issues/1/comments \
  -H "Authorization: Bearer <token>" -H "Content-Type: application/json" \
  -d '{"body":"This is a comment"}'
```

### Pull Request

```bash
# 创建 PR
curl -X POST http://localhost:8080/api/v1/repos/testuser/myrepo/pulls \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"title":"Add feature","body":"Description","head_branch":"feature","base_branch":"main"}'

# 列出 PR（?state=open/closed/merged/all）
curl "http://localhost:8080/api/v1/repos/testuser/myrepo/pulls?state=open" \
  -H "Authorization: Bearer <token>"

# 查看 Diff
curl http://localhost:8080/api/v1/repos/testuser/myrepo/pulls/1/diff \
  -H "Authorization: Bearer <token>"

# 合并 PR（strategy: merge / squash / rebase）
curl -X POST http://localhost:8080/api/v1/repos/testuser/myrepo/pulls/1/merge \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"strategy":"merge"}'
```

### API 文档鉴权

`/api-docs/*` 默认受保护，需要有效 JWT 或 PAT 才能访问。

---

## Git 操作

### SSH

```bash
GIT_SSH_COMMAND="ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null" \
  git clone ssh://git@localhost:2222/testuser/testrepo /tmp/myrepo

cd /tmp/myrepo
echo "hello" > test.txt
git add test.txt && git commit -m "test"

GIT_SSH_COMMAND="ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null" \
  git push origin main
```

### HTTP

```bash
git clone http://localhost:8080/git/testuser/testrepo /tmp/myrepo-http
cd /tmp/myrepo-http
# ... 修改文件后
git push origin main
```

---

## 开发

### 回归测试

```bash
# 全量回归：后端测试、前端静态检查/构建、运行态 smoke
node scripts/full-interface-regression.mjs

# 仅后端 OpenAPI 冒烟
BACKEND_URL=http://127.0.0.1:8080 node scripts/openapi-interface-smoke.mjs

# 仅前端页面 console/network 冒烟
BASE=http://127.0.0.1:5173 node scripts/console-smoke.mjs

# 前端 API client 与 OpenAPI 参数对齐
BACKEND_URL=http://127.0.0.1:8080 node scripts/api-client-contract-check.mjs
```

回归脚本支持丰富的环境变量（超时、重试、步骤级覆盖、报告输出等），详见 `scripts/full-interface-regression.mjs` 源码注释。

### 前后端联调

```bash
# 一键联调
node scripts/frontend-backend-smoke.mjs

# 指定端口
BACKEND_URL=http://127.0.0.1:8080 FRONTEND_URL=http://127.0.0.1:3000 node scripts/frontend-backend-smoke.mjs
```

### 日志调试

```bash
tail -f /tmp/ironforge.log

# 开启 git 协议追踪（客户端侧）
GIT_TRACE_PACKET=1 GIT_TRACE=1 git push origin main 2>&1
```

---

## 项目结构

```
ironforge/
├── Cargo.toml              # Workspace 根，统一依赖版本
├── crates/
│   ├── rg-cli/             # 主二进制入口（bin = "ironforge"）
│   ├── rg-core/            # 核心业务逻辑（auth/repo/issue/pr/wiki/lfs/...）
│   ├── rg-git/             # Git 协议层（pkt-line/V1/V2/cli_gateway）
│   ├── rg-ssh/             # SSH 服务端（russh 0.51）
│   ├── rg-http/            # HTTP 服务端 + REST API（Axum 0.8）
│   ├── rg-db/              # 数据库层（SeaORM 1.1 + SQLite/PG/MySQL）
│   ├── rg-ci/              # CI/CD 引擎（YAML 解析 + Pipeline 执行器）
│   ├── rg-runner/          # Runner Agent（bin = "ironforge-runner"）
│   └── rg-mcp/             # MCP 服务器（bin = "ironforge-mcp"，stdio-only）
├── web/                    # SvelteKit 前端（独立 SPA）
├── docs/                   # 设计文档（PRD/系统设计/Git协议/AI集成）
├── ironforge-docs/         # 分析报告（架构/对比/CI/测试审计）
└── .ai/                    # AI Agent 接入规范（guardrails + MCP 配置）
```

> 各 crate 职责和边界规则详见 [CONTRIBUTING.md](CONTRIBUTING.md)；当前架构事实详见 [ironforge-docs/architecture/](ironforge-docs/architecture/)。

---

## 技术选型

| 层级 | 选型 | 版本 |
|------|------|------|
| 异步运行时 | tokio | 1.x |
| HTTP 框架 | axum + axum-server | 0.8 / 0.7 |
| SSH 服务端 | russh | 0.51 |
| Git 操作 | gix + GitCommandGateway | 0.84 |
| ORM | SeaORM | 1.1 |
| 认证 | argon2 + JWT | 0.5 |
| TLS | rustls + tokio-rustls | 0.23 / 0.26 |
| 序列化 | serde + serde_json + toml | 1.x |
| 错误处理 | anyhow + thiserror | 1.x / 2.x |
| 日志 | tracing + tracing-appender | 0.1 |
| CLI | clap | 4.x |
| 前端 | SvelteKit 5 + adapter-static | SPA mode |
| 前端 i18n | Svelte 5 reactive store + localStorage | 中文 + 英文 |
| 代码覆盖率 | cargo-llvm-cov | HTML/LCOV/JSON 输出 |

> 完整依赖版本速查见 [CLAUDE.md](CLAUDE.md)。gix 当前覆盖率 ~70%，部分能力仍经 GitCommandGateway 调用 git CLI。

---

## 文档导航

| 文件 | 用途 |
|------|------|
| [CLAUDE.md](CLAUDE.md) | AI 深度协作上下文（踩坑记录、依赖版本、错误排查、实现清单） |
| [AGENT.md](AGENT.md) | AI 助手轻量统一入口（概览 + 文件速查） |
| [CONTRIBUTING.md](CONTRIBUTING.md) | 开发规范（crate 边界、编码规范、提交规范、测试规范） |
| [ARCHITECTURE.md](ARCHITECTURE.md) | 架构设计文档（技术选型决策、模块设计、数据模型、核心子系统） |
| [ironforge-docs/README.md](ironforge-docs/README.md) | 分析报告文档索引 |
| [ironforge-docs/contributor-quickstart.md](ironforge-docs/contributor-quickstart.md) | 贡献者 5 分钟快速上手 |

---

## License

MIT
