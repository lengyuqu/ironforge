<p align="center"><img src="assets/brand/ironforge-logo.svg" alt="IronForge" width="288"></p>

# IronForge 🔨

> **面向 AI Agent 的 Git 托管平台** —— 用 Rust 从零实现，单二进制部署。

[English](README.md) | **简体中文**

[![Rust](https://img.shields.io/badge/rust-1.95%2B-orange)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

IronForge 是一个**为 Agent 编程而设计的 Git 托管平台**。仓库、Issue、Pull Request、Wiki、CI/CD、包注册表、SSO/LDAP、审计与代码搜索这些托管基本盘，全部装在一个 Rust 二进制里。它不是再做一个 Gitea；它回答的是另一个问题：

> **当写代码的主力变成并发、会重试、按 token 计费的 Agent 时，代码托管平台应该长什么样？**

每个设计取舍都只过一道判断：能不能让 Agent 更省 token、更可控，并留下人能审计的痕迹。

## 为什么 Agent 需要不同的平台

1. **token 有成本。** 一次读取返回多少、要往返几次，直接决定成本与成功率。
2. **Agent 并发，而且会重试。** 多个 Agent 同时改同一个仓库；网络抖动或模型重试会把同一个请求发两遍。
3. **它们靠机器可读的反馈闭环工作。** CI 为什么挂、合并为什么冲突、权限为什么被拒，必须是结构化字段，而不是让模型去猜 HTML 或日志。
4. **它们不应该被完全信任。** Issue、PR、日志里的内容都可能对 Agent 做提示词注入；平台要限制它**最多能做什么**，而不是指望它自觉。

## IronForge 提供什么

| 方向 | 做了什么 |
|---|---|
| **Agent 是一等主体** | Agent 是独立账号（`kind = agent`），归属某个人类所有者，权限不超过所有者；没有人类登录入口，不能批准 PR、不能建仓库/组织；批准计数只数人类；审计在主写路径记录 Agent 的所有者；提交署名只是内容，不证明平台侧发起者。 |
| **最小权限令牌** | PAT scope 阶梯 `repo:read` < `repo:collab` < `repo:write` < `repo`。令牌可限制到指定仓库（`repos`）与分支模式（`refs`）；带分支限制的令牌不能合并、批准 PR，新增的 REST 写路由默认拒绝。 |
| **可预览、可并发、可重试的写入** | 文件提交、PR 创建与合并支持 `dry_run`、`expected_sha`（分支已前进则 409 `REF_ADVANCED` 并给出当前提交）、`Idempotency-Key`；批量提交让 N 个文件改动成为 **1 个 commit = 1 次 CI**，任一失败整批不落；cherry-pick / revert；错误体带 `reason` / `retryable` / `hint`。 |
| **改 → 推 → 看 CI → 修 的短闭环** | CI 长轮询等待、增量/尾部日志、失败 job 自带 `failure_summary`；持久化仓库事件日志 + 长轮询（不需要公网 webhook）；按 token 预算裁剪的 PR 上下文 `get_pr_context`；合并就绪预览（冲突文件、分支保护逐门控判定）与 `update-branch`；PR 完整导出用于重放评测。 |
| **读代码省 token** | AI 友好端点（仓库摘要、分页文件树、代码搜索）、符号定义索引与 `find_definition`、带 2,000 行 / 128 KiB 窗口的文件读取。 |
| **多 Agent 协作** | 同一仓库的多个 Agent 会话围绕**任务**协作：消息、阻塞声明、交接、结构化提问与答复、不可变决定、任务依赖、路径重叠预警、断线恢复上下文包、完成条件、跨仓协调组与协作健康指标；人类通过 Web UI 与通知介入。 |
| **MCP 原生接入** | `ironforge-mcp` 暴露 **77 个工具**（18 个仓库/PR/CI 工具 + 59 个 `collab_*` 协作工具，以 `tools/list` 为准），支持 stdio 与 streamable HTTP；远程入口不持有凭据，令牌 scope 与白名单照常生效。**没有合并工具**——流程是「提交到 `agent/*` → 开 PR → 等 CI → 由人合并」。 |
| **信任边界写进契约** | 客户端自报字段（会话名、任务状态、交接摘要、评论正文……）一律标 `origin: user_content`，是**数据而不是指令**；声明不等于证据，「声明 done」不算完成；能力端点 `/api/v1/collaboration/capabilities` 只广告构建里真实存在的能力，写请求拒绝未知字段。 |

### 一个典型的 Agent 循环

```text
1. get_pr_context / find_definition / read_file      读上下文（有预算，截断有标注）
2. commit_files  -> agent/fix-login                 N 个文件 = 一次提交 = 一次 CI（expected_sha + 幂等键）
3. create_pr                                        开 PR（可先 dry_run）
4. wait_pipeline                                    长轮询 CI；失败读 failure_summary，回到 2
5. collab_ask_question / collab_create_handoff      需要人决定或交接时写结构化记录，而不是猜
6. 人类审阅并合并                                   Agent 不能合并，也不能批准
```

## 接入 Agent

```bash
# stdio（本机 Agent，如 Claude Code / Cursor / Cline）
IRONFORGE_URL=http://localhost:8080 IRONFORGE_PAT=<token> ironforge-mcp

# streamable HTTP（远程 Agent，每个连接带自己的令牌）
IRONFORGE_URL=http://127.0.0.1:8080 ironforge-mcp --http 127.0.0.1:8765
```

用 `POST /api/v1/agents` 与 `POST /api/v1/agents/{username}/tokens` 创建专用 Agent 账号与令牌（只有人类账号能调用）。发令牌时用最小 scope，并尽量带上 `repos` 与 `refs`。完整的 MCP 配置、工具清单、协作协议与令牌规则见 [.ai/README.md](.ai/README.md)；本仓库内 Agent 的行为边界见 [.ai/guardrails.md](.ai/guardrails.md)。

## ifg：终端客户端

`ifg` 是 IronForge 的命令行客户端：仓库、Issue、PR、CI、Release、代码上下文与多 Agent 协作都可以在终端完成，并为脚本与 Agent 提供稳定的机器可读输出。它是独立的 MIT 项目：[lengyuqu/ifg](https://github.com/lengyuqu/ifg)。

```sh
brew install lengyuqu/ifg/ifg        # macOS / Linux（Homebrew）
ifg auth login --host http://localhost:8080
ifg doctor                            # 检查 host、token、scope、服务端版本与流式能力
```

常用命令：

```sh
ifg repo list
ifg pr list -R owner/name
ifg pr readiness 42
ifg pr checkout 42
ifg ci logs <job-id> --follow
ifg context pr 42
ifg collab task list -R owner/name
```

- `--json` 输出服务端自己的字段名，`jq` 表达式可以直接复用；`-q/--template` 按结果渲染小模板。
- `--no-input`（或 `IFG_NO_INPUT=1`）让所有交互提示立即失败并返回退出码 2，同时提示该传哪个参数。它不是 `--yes`：破坏性命令仍需要 `--yes`。
- `ifg schema` 以 JSON 描述命令树，供 Agent 使用，不需要网络或登录；`ifg api <path>` 覆盖长尾接口（wiki、webhooks、packages、secrets 等）。
- 在 checkout 内 `ifg` 会从 `origin` 推断 `OWNER/NAME`；在仓库外要传 `-R`。
- 其他平台的安装方式见 [ifg 仓库](https://github.com/lengyuqu/ifg)。

## 快速开始

环境要求：

- Rust 1.95+（stable）
- Node.js 22+（前端构建与回归脚本）
- `git` 只用于 mirror 同步与测试造数据；生产 Git 路径是原生 gix
- macOS 或 Linux

```bash
git clone <this-repo>
cd ironforge
cargo build --release
```

二进制在 `target/release/ironforge`。

生成 SSH 主机密钥：

```bash
ssh-keygen -t ed25519 -f /tmp/ironforge_host_key -N ""
```

启动：

```bash
mkdir -p /tmp/ironforge/repos
./target/release/ironforge serve \
  --repo-root /tmp/ironforge/repos \
  --http-addr 0.0.0.0:8080 \
  --ssh-addr  0.0.0.0:2222 \
  --host-key  /tmp/ironforge_host_key \
  --db-url    "sqlite:///tmp/ironforge/ironforge.db?mode=rwc" \
  --jwt-secret my-secret-key
```

HTTP 默认 `0.0.0.0:8080`，SSH 默认 `0.0.0.0:2222`；启动时自动跑迁移。TLS、日志、SMTP、Docker runner 与限流参数见 `ironforge serve --help`。

快速测试可以建一个裸仓库（不写数据库记录）：

```bash
./target/release/ironforge create-repo testuser testrepo \
  --repo-root /tmp/ironforge/repos
```

调试日志用 `RUST_LOG=debug`。

## Docker 部署

仓库自带多阶段 `Dockerfile`（SvelteKit 前端 + Rust release + 精简运行时）和根目录的 `docker-compose.yml`。

```bash
export IRONFORGE_JWT_SECRET=$(openssl rand -hex 32)
docker compose up -d --build

docker compose exec ironforge ironforge create-user admin \
  --email you@example.com --password 'change-me-please' --admin \
  --db-url "sqlite:///data/ironforge.db?mode=rwc"

curl -fsS http://localhost:8080/health
```

- 端口：`8080`（HTTP + Web UI）、`2222`（git over SSH）。
- 数据：`ironforge-data` 卷（repos / SQLite / logs），重建容器不丢。
- PostgreSQL：`docker compose --profile postgres up -d`，并按 compose 注释覆盖 app 的 `command`。
- 镜像自带 `HEALTHCHECK`，`docker compose ps` 会显示 `healthy`。

容器起来后登录 CLI：

```bash
ifg auth login --host http://localhost:8080
ifg doctor
```

## API 一览

REST 在 `/api/v1` 下，接受 Bearer JWT 或 PAT；Git Smart HTTP 在 `/git/<owner>/<repo>/…`。OpenAPI 文档：`/api-docs/openapi.json`；Swagger UI：`/api-docs/`（受保护）。

| 领域 | 端点 |
|---|---|
| 用户与会话 | `/api/v1/users/register`、`/login`、`/me` |
| 仓库 | `/api/v1/repos`、`/repos/{owner}/{name}` |
| Issue | `/repos/{owner}/{name}/issues`（评论、标签、里程碑） |
| Pull Request | `/repos/{owner}/{name}/pulls`（diff、merge、readiness、update-branch） |
| AI 上下文 | `/ai/repos/{owner}/{name}/summary`、`/tree`、`/map`、`/symbols`、`/search/code` |
| PR 上下文 | `/ai/repos/{owner}/{name}/pulls/{number}/context` |
| 协作 | `/repos/{owner}/{name}/collaboration/…` |
| CI | `/repos/{owner}/{name}/…` 下的 pipeline、job、日志、制品与环境 |

长尾接口用 OpenAPI 文档或 `ifg api <path>`。

## Git over SSH / HTTP

```bash
# SSH（2222）
GIT_SSH_COMMAND="ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null" \
  git clone ssh://git@localhost:2222/testuser/testrepo /tmp/myrepo

# HTTP
git clone http://localhost:8080/git/testuser/testrepo /tmp/myrepo-http
```

两条路径都支持协议 V1/V2、shallow 与 partial clone，以及 receive-pack 推送。真实客户端矩阵见 [docs/git-client-matrix.md](docs/git-client-matrix.md)。

## 开发

```bash
scripts/check.sh --quick   # 文档 + fmt + clippy
scripts/check.sh           # 再加 rg-http / rg-core 测试
scripts/check.sh --full    # 全 workspace，与 CI 等价
```

文档门禁是 `python3 scripts/check-docs.py`：版本一致、链接、索引覆盖与已删除路径引用；它已接入 `check.sh` 与 CI 的 `Docs consistency` job。

前端：

```bash
cd web
pnpm install
pnpm dev
```

端到端回归：`node scripts/full-interface-regression.mjs`（后端、前端与运行态 smoke）。crate 边界与测试规则见 [CONTRIBUTING.md](CONTRIBUTING.md)。

## 项目结构

```text
ironforge/
├── crates/
│   ├── rg-cli/             # 主二进制（bin = "ironforge"）
│   ├── rg-core/            # 核心业务（auth/repo/issue/pr/wiki/lfs/...）
│   ├── rg-git/             # Git 协议（pkt-line/V1/V2/gix）
│   ├── rg-ssh/             # SSH 服务端（russh）
│   ├── rg-http/            # HTTP 服务端 + REST API（axum）
│   ├── rg-db/              # 数据库层（SeaORM，SQLite/PostgreSQL/MySQL）
│   ├── rg-ci/              # CI 引擎（YAML + Pipeline 执行器）
│   ├── rg-ci-isolation/    # 两个 runner 共用的容器加固策略
│   ├── rg-runner/          # 外部 CI Runner
│   └── rg-mcp/             # MCP 服务端（stdio + streamable HTTP）
├── web/                    # SvelteKit SPA
├── docs/                   # 索引 + 当前设计 + 协议/性能/运维
└── .ai/                    # Agent 接入指南与护栏
```

## 技术选型

| 层级 | 选型 |
|---|---|
| 异步运行时 | tokio 1 |
| HTTP | axum 0.8 |
| SSH | russh 0.65 |
| Git | gix 0.91（生产路径原生；mirror 同步是唯一 git CLI 例外） |
| 数据库 | SeaORM 1.1，支持 SQLite / PostgreSQL / MySQL |
| 认证 | argon2 + JWT（HS256）+ LDAP/SSO + TOTP |
| 前端 | SvelteKit 5 SPA，中英双语 i18n |
| MCP | Rust，stdio 与 streamable HTTP |

## 状态与边界

当前版本：**0.5.1**。版本历史见 [CHANGELOG.md](CHANGELOG.md)。

- **只支持单实例。** 多实例部署与滚动升级不支持：数据库、仓库存储、启动迁移和周期任务都没有外置。见 [docs/operations.md](docs/operations.md)。
- **仓库权限仍是三档**（read / write / admin）。Unit 级权限已设计但未实现（见 [docs/design/authorization.md](docs/design/authorization.md) §9）。
- **第三方 Action：**只展开 `composite`；`node*` 与 `docker` 在 `actions-v1` Runner 出现前继续 fail closed。
- **内存：**目标是 <50 MB；本机实测空载约 84 MiB，大仓库 clone 峰值 299 MiB。见 [docs/performance.md](docs/performance.md)。
- 功能边界与未覆盖项见 [CLAUDE.md](CLAUDE.md) 的「未实现 / 技术债」。

## 文档

| 文档 | 用途 |
|---|---|
| [AGENT.md](AGENT.md) | AI 工具轻量入口 |
| [CLAUDE.md](CLAUDE.md) | 约定、当前实现、踩坑、错误排查 |
| [ARCHITECTURE.md](ARCHITECTURE.md) | 架构与技术选型 |
| [CONTRIBUTING.md](CONTRIBUTING.md) | crate 边界、编码与测试规范 |
| [.ai/README.md](.ai/README.md) | MCP / REST 接入与工具清单 |
| [docs/README.md](docs/README.md) | 文档索引与维护规则 |
| [docs/releasing.md](docs/releasing.md) | 发布流程与文档门禁 |
| [docs/design/](docs/design/) | 当前设计：协作、授权、Agent、CI、数据、代码智能 |

## License

[MIT](LICENSE) © 2026 Yu Qu (lengyuqu)。允许闭源与商业使用；再分发时保留版权与许可声明。
