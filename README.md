# IronForge 🔨

> **铁匠铺** — 一个用 Rust 从零实现的轻量级 Git 托管平台

[![Rust](https://img.shields.io/badge/rust-1.95%2B-orange)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

IronForge 对标 [Gitea](https://gitea.com/) / [Forgejo](https://forgejo.org/)，目标是用纯 Rust 实现一个内存占用极低（<50MB）、单二进制部署的全功能 Git 托管平台，支持仓库管理、Issue、Pull Request、Wiki 和 CI/CD。

---

## 功能状态

| 功能 | 状态 | 说明 |
|------|------|------|
| `git clone` over HTTPS | ✅ 完成 | Git Smart Protocol V1 |
| `git push` over HTTPS  | ✅ 完成 | report-status + sideband-64k |
| `git clone` over SSH   | ✅ 完成 | russh 0.51，公钥/密码认证 |
| `git push` over SSH    | ✅ 完成 | sideband 正确封装 |
| 用户认证（真实）        | ⏳ Phase 2 | 目前全放行 |
| SeaORM 数据库          | ⏳ Phase 2 | 目前 stub |
| Web UI (SvelteKit)     | ⏳ Phase 2+ | 尚未启动 |
| Issue / PR / Wiki      | ⏳ Phase 3-4 | 规划中 |
| CI/CD 引擎             | ⏳ Phase 5 | 规划中 |

---

## 快速开始

### 环境要求

- Rust 1.75+（推荐 stable）
- git（系统命令，用于 pack-objects / index-pack / update-ref）
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

# 启动（HTTP :8080 + SSH :2222）
./target/release/ironforge serve \
  --repo-root /tmp/ironforge/repos \
  --http-addr 0.0.0.0:8080 \
  --ssh-addr  0.0.0.0:2222 \
  --host-key  /tmp/ironforge_host_key
```

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

### 测试 git clone / push（SSH）

```bash
GIT_SSH_COMMAND="ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null" \
  git clone ssh://git@localhost:2222/testuser/testrepo /tmp/myrepo

cd /tmp/myrepo
echo "hello" > test.txt
git add test.txt && git commit -m "test"

GIT_SSH_COMMAND="ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null" \
  git push origin main
```

### 测试 git clone / push（HTTP）

```bash
# HTTP 路由前缀是 /git/
git clone http://localhost:8080/git/testuser/testrepo /tmp/myrepo-http
cd /tmp/myrepo-http
# ... 修改文件后
git push origin main
```

---

## 项目结构

```
ironforge/
├── Cargo.toml              # Workspace 根，统一依赖版本
├── ARCHITECTURE.md         # 完整架构方案文档
├── CLAUDE.md               # AI 协作上下文（Codex / Claude Code / WorkBuddy）
├── CONTRIBUTING.md         # 开发指南
├── docs/
│   └── git-protocol.md     # Git 协议实现细节与踩坑记录
└── crates/
    ├── rg-cli/     # 主二进制入口（bin = "ironforge"）
    ├── rg-core/    # 核心业务逻辑（用户/仓库/Issue/PR/Wiki）
    ├── rg-git/     # Git 协议层（pkt-line、upload-pack、receive-pack、sideband）
    ├── rg-ssh/     # SSH 服务端（russh 0.51）
    ├── rg-http/    # HTTP 服务端（Axum 0.8）
    ├── rg-db/      # 数据库层（SeaORM 1.1 + SQLite）
    └── rg-ci/      # CI/CD 引擎（stub）
```

### 各 crate 职责

| Crate | 职责 | 状态 |
|-------|------|------|
| `rg-cli` | CLI 入口，`serve` / `create-repo` 命令 | ✅ 完成 |
| `rg-git` | Git Smart Protocol V1：pkt-line 编解码、upload-pack（clone/fetch）、receive-pack（push）、sideband-64k | ✅ 完成 |
| `rg-ssh` | russh SSH 服务端，exec_request 路由到 rg-git | ✅ 完成 |
| `rg-http` | Axum HTTP 服务端，Git Smart HTTP 端点 | ✅ 完成 |
| `rg-core` | 业务逻辑（用户、仓库、Issue、PR、Wiki）| ⏳ stub |
| `rg-db` | SeaORM 实体 + 迁移 | ⏳ stub |
| `rg-ci` | CI/CD Pipeline 引擎 | ⏳ stub |

---

## 开发

### 开发构建

```bash
cargo build          # debug 构建
cargo build --release  # release 构建
```

### 端到端测试脚本

```bash
# 停旧进程 + 重建测试环境
pkill -f "target/release/ironforge" 2>/dev/null; sleep 0.5
rm -rf /tmp/ironforge/repos/testuser/testrepo.git /tmp/if_e2e

# 初始化裸仓库（从本地已有的 repo 克隆）
git clone --bare /path/to/your/repo /tmp/ironforge/repos/testuser/testrepo.git

# 启动服务器（后台）
./target/release/ironforge serve \
  --repo-root /tmp/ironforge/repos \
  --host-key /tmp/ironforge_host_key \
  > /tmp/ironforge.log 2>&1 &

sleep 2

# 测试 SSH clone + push
GIT_SSH_COMMAND="ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null" \
  git clone ssh://git@localhost:2222/testuser/testrepo /tmp/if_e2e
cd /tmp/if_e2e
echo "hello" > test.txt
git add test.txt && git commit -m "test commit"
GIT_SSH_COMMAND="ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null" \
  git push origin main
echo "Push exit: $?"
```

### 日志调试

```bash
# 查看服务器日志
tail -f /tmp/ironforge.log

# 开启 git 协议追踪（客户端侧）
GIT_TRACE_PACKET=1 GIT_TRACE=1 git push origin main 2>&1
```

---

## 技术选型

| 层级 | 选型 | 版本 |
|------|------|------|
| 异步运行时 | tokio | 1.x |
| HTTP 框架 | axum | 0.8 |
| SSH 服务端 | russh | 0.51 |
| Git 操作 | git CLI fallback | — |
| ORM | SeaORM | 1.1 |
| 序列化 | serde + serde_json | 1.x |
| 错误处理 | anyhow + thiserror | 1.x / 2.x |
| 日志 | tracing + tracing-subscriber | 0.1 |
| CLI | clap | 4.x |

> **关于 gix**：ARCHITECTURE.md 中规划使用 gix (gitoxide)，但当前 Phase 1 实现为降低复杂度，Git 对象操作全部通过调用系统 `git` 命令实现。Phase 2 起可逐步替换为 gix API。

---

## 架构文档

详细设计请见 [ARCHITECTURE.md](ARCHITECTURE.md)，包含：
- 整体三层架构图
- 技术选型决策分析（ORM / Git 库 / 前端对比）
- 数据库模型设计（ER 图）
- 各子系统设计（Git 协议层、PR 引擎、CI/CD、Wiki）
- Phase 0-5 开发计划

---

## License

MIT
