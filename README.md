<p align="center"><img src="assets/brand/ironforge-logo.svg" alt="IronForge" width="288"></p>

# IronForge 🔨

> **铁匠铺** — 面向 AI Agent 编程的 Git 托管平台。用 Rust 从零实现，单二进制部署。

[![Rust](https://img.shields.io/badge/rust-1.95%2B-orange)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

IronForge 是一个**为 Agent 编程而设计的 Git 托管平台**。它自带完整的 Git 托管基本盘——仓库、Issue、Pull Request、Wiki、CI/CD、包注册表、企业认证、审计、代码搜索，目标是内存占用极低（<50MB）、单二进制部署——但项目的重心早已不是「再做一个 [Gitea](https://gitea.com/) / [Forgejo](https://forgejo.org/)」，而是回答另一个问题：

> **当写代码的主力变成多个并发、会重试、按 token 计费的 Agent 时，代码托管平台应该长什么样？**

项目起步时确实以 Gitea/Forgejo 为对标对象（功能差距分析见 [ironforge-docs/comparison/](ironforge-docs/comparison/)），这些基本盘能力仍在；但此后的设计取舍，是看它们能不能让 Agent 更省 token、更可控、更可追溯地完成「改 → 推 → 看 CI → 修」的循环。

> Phase 1~21 全部完成，此后的演进主线是 Agent 友好化。详细实现清单见 [CLAUDE.md](CLAUDE.md)。

---

## 为什么 Agent 需要不同的代码托管平台

面向人的 Git 平台默认它的客户端是一个坐在浏览器前的人。Agent 不是：

1. **token 有成本**——一次读取返回多少、要往返几次，直接决定成本与成功率。
2. **并发，而且会重试**——多个 Agent 同时改同一个仓库；网络抖动或模型重试会把同一个请求发两遍。
3. **靠机器可读的反馈闭环工作**——CI 为什么挂了、合并为什么冲突、权限为什么被拒，必须是结构化字段，而不是让模型去猜一段 HTML 或日志。
4. **不应该被完全信任**——Agent 会被 Issue、PR、日志里的内容「提示词注入」，所以平台要限制它**最多能做什么**，而不是指望它自觉。

IronForge 的 Agent 能力围绕这四点展开：

| 方向 | 做了什么 |
|------|----------|
| **Agent 是一等主体** | Agent 是独立账号（`kind = agent`），归属某个人类所有者，**权限不超过所有者**；没有任何人类登录入口；不能批准 PR、不能建仓库/组织；批准计数只数人类；审计与提交署名能区分「人」和「代表某人的 Agent」。 |
| **最小权限令牌** | PAT scope 阶梯 `repo:read` < `repo:collab` < `repo:write` < `repo`；令牌可限制到指定仓库（`repos`）和分支模式（`refs: ["agent/*"]`）。带分支限制的令牌不能合并、批准 PR，新增的 REST 写路由对它默认被拒。 |
| **可预览、可并发、可重试的写入** | 文件提交、PR 创建与合并支持 `dry_run`、`expected_sha`（分支已前进则 409 `REF_ADVANCED` 并给出当前提交）、`Idempotency-Key`；批量提交让 N 个文件改动成为 **1 个 commit = 1 次 CI**，任一失败整批不落；cherry-pick / revert；错误体带 `reason` / `retryable` / `hint`。 |
| **改 → 推 → 看 CI → 修 的短闭环** | CI 长轮询等待、增量/尾部日志、失败 job 自带 `failure_summary`；仓库事件日志 + 长轮询（不需要公网 webhook 地址）；按 token 预算裁剪的 PR 聚合上下文 `get_pr_context`；合并就绪预览（冲突文件、分支保护逐门控判定）与 `update-branch`；PR 完整导出用于重放评测。 |
| **读代码省 token** | AI 友好端点（仓库摘要、分页文件树、代码搜索）、符号定义索引与 `find_definition`、带 2000 行 / 128 KiB 窗口的文件读取。 |
| **多 Agent 协作** | 同一仓库里多个 Agent 会话围绕**任务**协作：消息、阻塞声明、交接、结构化提问与答复、不可变的决定记录、任务依赖、路径重叠预警、断线恢复用的上下文包、完成条件、跨仓协调组、协作健康指标；人类通过 Web UI 与站内通知介入。 |
| **MCP 原生接入** | `ironforge-mcp` 暴露 **77 个工具**（18 个仓库/PR/CI 工具 + 59 个 `collab_*` 协作工具，以 `tools/list` 为准），支持 stdio 与 streamable HTTP；远程入口不持有凭据，令牌的 scope 与白名单照常生效。**没有合并工具**——Agent 的流程是「提交到 `agent/*` → 开 PR → 等 CI → 由人合并」。 |
| **信任边界写在契约里** | 客户端自报的字段（会话名、任务状态、交接摘要、评论正文……）一律标 `origin: user_content`，是**数据而不是指令**；声明不等于证据，「声明 done」不会被当成完成；能力发现端点 `/api/v1/collaboration/capabilities` 只广告构建里真实存在的能力，写请求拒绝未知字段。 |

### 一个典型的 Agent 循环

```text
1. get_pr_context / find_definition / read_file     读取上下文（有预算、有截断标注）
2. commit_files  → agent/fix-login                   一次提交多个文件 = 一次 CI（带 expected_sha 与幂等键）
3. create_pr                                        开 PR（可先 dry_run）
4. wait_pipeline                                    长轮询等 CI；失败则拿到 failure_summary，回到 2
5. collab_ask_question / collab_create_handoff       需要人决定或要交接时，写结构化记录而不是猜
6. 人类审阅并合并                                    Agent 无法合并，也不能批准
```

### 接入方式

```bash
# stdio（本机 Agent，如 Claude Code / Cursor / Cline）
IRONFORGE_URL=http://localhost:8080 IRONFORGE_PAT=<token> ironforge-mcp

# streamable HTTP（远程 Agent，每个连接带自己的令牌）
IRONFORGE_URL=http://127.0.0.1:8080 ironforge-mcp --http 127.0.0.1:8765
```

给 Agent 创建专用账号与令牌：`POST /api/v1/agents`、`POST /api/v1/agents/{username}/tokens`（只有人类账号能调用）；发令牌时用最小 scope，并尽量带上 `repos` 与 `refs`。完整的 MCP 配置、工具清单、协作协议与令牌用法见 [.ai/README.md](.ai/README.md)，Agent 自身在本仓库内的行为边界见 [.ai/guardrails.md](.ai/guardrails.md)。

### 成熟度说明

Agent 相关能力大多是 2026-11 的快速迭代成果：绝大部分已在本机（SQLite）用真实服务 + 真实 `ironforge-mcp` 的端到端脚本验证，协作与部分写路径另在 PostgreSQL 17 / MySQL 8.4 上跑过，但**尚未经过长期的真实多 Agent 生产使用**，GitHub CI 的覆盖也还没有补全。每项能力的验证范围与未覆盖项见 [CLAUDE.md](CLAUDE.md) 与 [ironforge-docs/architecture/](ironforge-docs/architecture/) 下的 `*-verification-*.md`；设计动机见 [Harness 原生化设计](ironforge-docs/architecture/harness-native-design-2026-11.md) 与 [Agent 主体身份与路线图](ironforge-docs/architecture/agent-principals-design-and-roadmap-2026-11.md)。

---

## 快速开始

### 环境要求

- Rust 1.95+（推荐 stable）
- Node.js（前端构建与回归脚本）
- git（启用仓库镜像同步时必需；测试套件造数据也需要。其余生产路径 gix 原生，不依赖 git CLI）
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

## Docker 部署

仓库自带多阶段 `Dockerfile`（SvelteKit 前端 + Rust release + 精简运行时）和 `docker-compose.yml`。

```bash
export IRONFORGE_JWT_SECRET=$(openssl rand -hex 32)
docker compose up -d --build          # 首次约 11 分钟（前端 + release 编译）

# 首次建号（注册关闭时也能用）
docker compose exec ironforge ironforge create-user admin \
  --email you@example.com --password 'change-me-please' --admin \
  --db-url "sqlite:///data/ironforge.db?mode=rwc"

curl -fsS http://localhost:8080/health
```

- 端口：`8080`（HTTP + Web UI）、`2222`（git over SSH）
- 数据：`ironforge-data` 卷（repos / sqlite / logs），重建容器不丢
- 换 PostgreSQL：`docker compose --profile postgres up -d`，并按 compose 注释覆盖 `command` 的 `--db-url`
- 镜像自带 `HEALTHCHECK`，`docker compose ps` 会显示 `healthy`

命令行客户端验证：

```bash
ifg auth login --host http://localhost:8080   # 粘 Settings → Tokens 生成的 PAT
ifg doctor                                    # 八项自检，全 ok 即可用
```

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

### 提交前自检

```bash
scripts/check.sh --quick   # cargo fmt + clippy（约 40 秒）
scripts/check.sh           # 再加 rg-http / rg-core 测试（约 7 分钟）
scripts/check.sh --full    # 全 workspace，与 CI 等价
```

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
│   ├── rg-ssh/             # SSH 服务端（russh 0.65）
│   ├── rg-http/            # HTTP 服务端 + REST API（Axum 0.8）
│   ├── rg-db/              # 数据库层（SeaORM 1.1 + SQLite/PG/MySQL）
│   ├── rg-ci/              # CI/CD 引擎（YAML 解析 + Pipeline 执行器）
│   ├── rg-ci-isolation/    # CI 容器加固策略（镜像校验、资源/capability 限制）
│   ├── rg-runner/          # Runner Agent（bin = "ironforge-runner"，CI 任务执行器，与 AI Agent 无关）
│   └── rg-mcp/             # MCP 服务器（bin = "ironforge-mcp"，stdio + streamable HTTP）
├── web/                    # SvelteKit 前端（独立 SPA）
├── docs/                   # 设计文档（PRD/系统设计/Git协议/AI集成）
├── ironforge-docs/         # 分析报告（架构/Agent 设计与验证记录/对比/CI/测试审计）
└── .ai/                    # AI Agent 接入规范（接入指南 + guardrails + MCP 配置）
```

> 各 crate 职责和边界规则详见 [CONTRIBUTING.md](CONTRIBUTING.md)；当前架构事实详见 [ironforge-docs/architecture/](ironforge-docs/architecture/)。

---

## 技术选型

| 层级 | 选型 | 版本 |
|------|------|------|
| 异步运行时 | tokio | 1.x |
| HTTP 框架 | axum + axum-server | 0.8 / 0.7 |
| SSH 服务端 | russh | 0.65 |
| Git 操作 | gix（生产路径原生；唯一例外：mirror 同步走 git CLI 网关） | 0.90.0 |
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
| [.ai/README.md](.ai/README.md) | **Agent 接入指南**：MCP 配置、全部工具清单、AI 端点、多 Agent 协作协议、令牌 scope |
| [.ai/guardrails.md](.ai/guardrails.md) | Agent 生命周期护栏（哪些操作禁止 / 需确认） |
| [CLAUDE.md](CLAUDE.md) | AI 深度协作上下文（踩坑记录、依赖版本、错误排查、实现清单） |
| [AGENT.md](AGENT.md) | AI 助手轻量统一入口（概览 + 文件速查） |
| [CONTRIBUTING.md](CONTRIBUTING.md) | 开发规范（crate 边界、编码规范、提交规范、测试规范） |
| [ARCHITECTURE.md](ARCHITECTURE.md) | 架构设计文档（技术选型决策、模块设计、数据模型、核心子系统） |
| [ironforge-docs/README.md](ironforge-docs/README.md) | 分析报告文档索引 |
| [ironforge-docs/contributor-quickstart.md](ironforge-docs/contributor-quickstart.md) | 贡献者 5 分钟快速上手 |

---

## License

IronForge 以 [MIT License](LICENSE) 发布。

> Copyright (c) 2026 Yu Qu (lengyuqu)

MIT 授予任何人免费获取本软件及相关文档副本的权利，可无限制地使用、复制、修改、合并、发布、分发、再许可和/或销售本软件副本。唯一义务是在软件的所有副本或实质部分中保留上述版权声明与本许可声明。本软件按「原样」提供，不附带任何明示或默示担保。

| 项 | 说明 |
|----|------|
| 完整文本 | [LICENSE](LICENSE) |
| SPDX 标识 | `MIT` |
| 闭源 / 商业使用 | 允许，无需公开衍生代码 |
| 再分发义务 | 保留版权声明与许可声明 |
| 第三方依赖 | 各自遵循原有许可（以 MIT / Apache-2.0 为主） |

许可声明在仓库内统一维护：Cargo workspace 通过 `[workspace.package]` 的 `license = "MIT"` 下发给全部 crate，前端包 `web/package.json` 标注 `"license": "MIT"`。
