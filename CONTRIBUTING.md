# Contributing to IronForge

感谢你对 IronForge 的关注！本文档说明开发规范、crate 职责划分和常见工作流程。

---

## 目录

- [开发环境](#开发环境)
- [项目结构与 crate 职责](#项目结构与-crate-职责)
- [编码规范](#编码规范)
- [提交规范](#提交规范)
- [测试规范](#测试规范)
- [分支管理](#分支管理)
- [Phase 开发计划](#phase-开发计划)

---

## 开发环境

### 必要工具

```bash
# Rust stable (1.75+)
rustup update stable

# 代码格式化
rustfmt --edition 2021

# Lint
cargo clippy

# 系统依赖（macOS）
# git（用于 pack-objects / index-pack / update-ref）
which git   # 必须存在
```

### 推荐工具

```bash
# 快速重建（监听文件变化）
cargo install cargo-watch
cargo watch -x "build --release"

# 查看依赖树
cargo tree

# 审计安全漏洞
cargo audit
```

### 初次设置

```bash
git clone <repo>
cd ironforge
cargo build      # 验证依赖下载和编译通过

# 生成测试用 SSH 主机密钥（一次性）
ssh-keygen -t ed25519 -f /tmp/ironforge_host_key -N ""
```

---

## 项目结构与 crate 职责

### 依赖关系图

```
rg-cli
  ├── rg-core
  │     └── rg-db
  ├── rg-git
  ├── rg-ssh
  │     └── rg-git
  ├── rg-http
  │     ├── rg-git
  │     └── rg-core
  └── rg-db

rg-ci   (独立，未来集成到 rg-core)
```

### 各 crate 边界规则

#### `rg-git` — Git 协议层（纯协议，无业务逻辑）

**允许**：
- pkt-line / sideband 编解码
- upload-pack / receive-pack 协议处理
- 调用系统 `git` 命令（pack-objects、index-pack、update-ref、for-each-ref）
- 文件路径操作

**禁止**：
- 不能依赖 `rg-core`、`rg-db`、`rg-http`、`rg-ssh`
- 不能包含用户认证逻辑
- 不能直接访问数据库

#### `rg-ssh` — SSH 传输层

**允许**：
- russh 服务端实现
- exec_request 路由到 `rg-git`
- SSH 认证（Phase 1 全放行，Phase 2 对接 `rg-core::auth`）

**禁止**：
- 不能包含 Git 协议解析逻辑（委托给 `rg-git`）
- 不能直接操作数据库

#### `rg-http` — HTTP 传输层

**允许**：
- Axum 路由定义
- Git Smart HTTP 端点实现
- REST API 端点（Phase 2+）
- 中间件（认证、CORS、限流）

**禁止**：
- 不能包含 Git 协议解析逻辑（委托给 `rg-git`）
- 业务逻辑应委托给 `rg-core`

#### `rg-core` — 核心业务逻辑

**允许**：
- 用户/仓库/Issue/PR/Wiki/Hook 业务逻辑
- 认证授权（argon2 密码哈希、JWT）
- 权限校验

**禁止**：
- 不能包含 HTTP/SSH 协议细节
- 不能包含 Git wire 协议实现

#### `rg-db` — 数据库层

**允许**：
- SeaORM 实体定义
- 数据库迁移文件
- CRUD 操作函数

**禁止**：
- 不能包含业务逻辑
- 不能包含 HTTP/SSH 层代码

#### `rg-cli` — 入口

**允许**：
- CLI 参数解析（clap）
- 各服务的启动和协调

**禁止**：
- 不能包含业务逻辑（全部委托给其他 crate）

---

## 编码规范

### 通用规范

```rust
// ✅ 错误处理：用 anyhow::Result 配合 ? 操作符
pub async fn do_something(path: &Path) -> anyhow::Result<()> {
    let output = std::process::Command::new("git")
        .arg("-C").arg(path)
        .args(["rev-parse", "HEAD"])
        .output()
        .context("failed to run git rev-parse")?;
    Ok(())
}

// ✅ 日志：用 tracing，结构化字段
tracing::info!(path = %repo_path.display(), user = %username, "Starting upload-pack");
tracing::error!(error = %e, "git index-pack failed");

// ❌ 不要用 println! / eprintln! 输出日志
println!("starting server");  // ❌
```

### async 规范

```rust
// ✅ 函数签名：泛型约束写明 Unpin
pub async fn write_pkt_line<W: AsyncWrite + Unpin>(writer: &mut W, ...) -> Result<()>

// ✅ BufReader：只在需要 read_pkt_line 的地方包装，用完立即 drop
{
    let mut reader = BufReader::new(&mut *stream);
    let result = process_push(repo_path, &mut reader).await?;
}  // BufReader drop 在这里，之后 stream 可以继续用于写

// ✅ 调用系统命令：用 tokio::process::Command 做异步
let mut cmd = tokio::process::Command::new("git")
    .arg("-C").arg(repo_path)
    .args(["index-pack", "--fix-thin", "--stdin"])
    .stdin(Stdio::piped())
    .spawn()?;
```

### 错误处理规范

```rust
// ✅ 库 crate：用 thiserror 定义错误类型
#[derive(thiserror::Error, Debug)]
pub enum AuthError {
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("user not found: {0}")]
    UserNotFound(String),
}

// ✅ 应用层/lib crate：用 anyhow::Result
pub async fn authenticate(username: &str, password: &str) -> anyhow::Result<User>

// ❌ 不要用 unwrap() / expect() 在生产路径中
let sha = output.stdout.first().unwrap();  // ❌
```

### 注释规范

关键算法和协议细节**必须**有注释说明：

```rust
// ✅ 解释"为什么"，而不只是"做什么"
// Git receive-pack 的 report-status 响应必须整体作为 band-1 sideband 数据发送。
// 不能先发 sideband flush 再发 plain pkt-lines——客户端在收到 sideband flush 后
// 就会停止读取，后续的 plain pkt-lines 将永远不会被读取。
// 参考：通过 GIT_TRACE_PACKET=1 对真实 git-receive-pack 抓包验证。
async fn send_response<W: AsyncWrite + Unpin>(...) -> Result<()> {
```

---

## 提交规范

遵循 [Conventional Commits](https://www.conventionalcommits.org/)：

```
<type>(<scope>): <description>

[body]

[footer]
```

### Type

| Type | 说明 |
|------|------|
| `feat` | 新功能 |
| `fix` | Bug 修复 |
| `docs` | 文档更新 |
| `refactor` | 重构（不改变行为） |
| `test` | 测试相关 |
| `chore` | 构建/依赖/工具相关 |
| `perf` | 性能优化 |

### Scope

使用 crate 名：`rg-git`、`rg-ssh`、`rg-http`、`rg-core`、`rg-db`、`rg-ci`、`rg-cli`

### 示例

```
feat(rg-ssh): implement SSH git push with sideband-64k report-status

Fix the SSH receive-pack response encoding: report-status pkt-lines
must be wrapped in band-1 sideband data, not sent as plain pkt-lines
after a sideband flush.

Closes #12
```

```
fix(rg-git): use read_pkt_line instead of read_line in process_push

Using read_line() caused UTF-8 parse failures when encountering binary
packfile data, since it tried to read the pkt-line length header as text.
```

---

## 测试规范

### 单元测试

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_write_pkt_line() {
        let mut buf = Vec::new();
        write_pkt_line(&mut buf, &PktLine::text("hello")).await.unwrap();
        assert_eq!(&buf, b"000ahello\n");
    }
}
```

### 集成测试（端到端）

集成测试以 shell 脚本形式维护在 `scripts/e2e_test.sh`（待创建）：

```bash
#!/usr/bin/env bash
set -e

# 重建测试环境
pkill -f "target/release/ironforge" 2>/dev/null || true
sleep 0.3

# 创建裸仓库
rm -rf /tmp/if_test_repos
mkdir -p /tmp/if_test_repos
git init --bare /tmp/if_test_repos/testuser/testrepo.git

# 启动服务器
./target/release/ironforge serve \
  --repo-root /tmp/if_test_repos \
  --host-key /tmp/ironforge_host_key \
  > /tmp/if_test.log 2>&1 &
SERVER_PID=$!
sleep 1.5

# SSH Clone + Push
SSH_OPT="ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null"
rm -rf /tmp/if_clone
GIT_SSH_COMMAND="$SSH_OPT" git clone ssh://git@localhost:2222/testuser/testrepo /tmp/if_clone
cd /tmp/if_clone
git config user.email "test@test.com" && git config user.name "Test"
echo "hello ironforge" > test.txt
git add test.txt && git commit -m "test commit"
GIT_SSH_COMMAND="$SSH_OPT" git push origin main
echo "✅ SSH push: exit $?"

kill $SERVER_PID
```

---

## 分支管理

```
main          ← 稳定分支，只接受经过测试的 PR
dev           ← 开发主干
phase/2-auth  ← Phase 2 用户认证功能分支（示例）
fix/ssh-eof   ← Bug 修复分支（示例）
```

PR 合并到 `main` 前要求：
1. `cargo build --release` 通过
2. `cargo clippy` 无 error
3. 端到端 SSH + HTTP clone/push 测试通过

---

## Phase 开发计划

### ✅ Phase 0：基建（已完成）
- Cargo workspace
- 日志系统（tracing）
- CLI（clap）
- 基础错误处理

### ✅ Phase 1：Git 协议层（已完成，2026-04-24）
- pkt-line 协议
- git-upload-pack（clone/fetch）—— SSH + HTTP
- git-receive-pack（push）—— SSH + HTTP
- sideband-64k 多路复用
- SSH 服务端（russh）
- HTTP 服务端（Axum）

### ⏳ Phase 2：用户系统（下一步）
- `rg-db`：SeaORM 实体 + SQLite 迁移
- `rg-core/auth`：用户注册/登录（argon2 + JWT）
- SSH Key 管理 + 公钥认证
- 仓库权限（Public/Private）
- REST API 基础（`/api/v1/...`）

### ⏳ Phase 3：Issue + Pull Request
### ⏳ Phase 4：Wiki + LFS + Webhook
### ⏳ Phase 5：CI/CD 引擎

完整计划见 [ARCHITECTURE.md](ARCHITECTURE.md)。
