# 协作 P1 双客户端闭环（U06 / N14）验证记录

> 日期：2026-11-05。`scripts/collab-e2e.py --scenario p1`。**可信度**：确认（运行验证，逐层标注范围）。

## 做了什么

在一个全新的临时 SQLite 库上启动**真实**的 `ironforge` 服务，用一个真实 Git 仓库（经服务 push），让两类不同客户端共用同一个账号：

- **A / C**：真实的 `ironforge-mcp` 二进制（stdio，仓库范围 PAT，各自独立的会话文件）；
- **B**：纯 HTTP 客户端（Python `urllib`，登录 JWT + 自己持有的会话密钥）；
- 用户门（无会话头）代表人工界面。

没有 mock。流程（每步都在脚本里断言）：

1. 路径预警：A、B 各建一个声明了同一文件的任务 → `collab_get_overlaps` 返回对方任务，`confidence=declared_current`、`authority=hint`。
2. 依赖：A 的任务依赖 B 的任务 → `blocked_by` 由**声明**状态派生（`status_source=declared`）。
3. 消息与送达：A 给 B 发消息，未确认前送达状态不是 `read`；B 确认回执后才变 `read`。
4. 依赖完成提醒：B 声明任务 `done` → A 收到恰好一条 `dependency_finished` 通知，A 确认。
5. 决定：A 记录一条决定，列表里为 `active`。
6. 收件箱已读标记：A 的 `assigned_task` 项先是 `unread`，标记后为 `read`；B 更新该任务后重新变 `unread`，且仍显示上次读的版本。
7. 完成条件（只展示）：A 定义 `manual_confirmation` + `ci_passed_for_commit`；任务被声明 `done` 不被阻止；评估为 manual `unsatisfied`、CI `unknown`，`declared_done_without_evidence=true`；用户门存一份快照；用户确认后 manual `satisfied`、CI 仍 `unknown`、整体 `incomplete`；快照仍是确认前的评估。
8. A 的 MCP 进程被杀；B 在分支上 push 新提交并写 12 条长进展消息。
9. **全新的 MCP 客户端 C**（新会话，不继承任何密钥）只靠 `collab_get_context` 恢复：看到 A 的决定、依赖、`head_sha` 等于服务端独立观测到的分支头、`consistency` 声明“不是快照”；用 `max_bytes=8192` 且只要 `decisions,messages` 时，被丢弃的消息数量和原因写在 `truncation` 里，保留的是最新的消息。
10. 扫描服务端日志与两个 MCP 进程的 stderr：没有会话密钥泄露。

## 环境与结果（运行验证）

| 项 | 值 |
|---|---|
| 机器 | macOS 27.2 arm64（本机），Python 3.14.10，git 2.54.0 |
| 仓库提交 | `377a53f`（工作树有未提交的 `scripts/collab-e2e.py` 修改） |
| 数据库 | SQLite，全新临时库 |
| `--scenario p1` | ✅ 连跑 4 遍全部通过（14 步） |
| 回归：`--scenario p0`、`--scenario t12` | ✅ 各 1 遍通过（本轮对收件箱/完成状态响应的改动没有破坏它们） |

## 没有被证明的

- **只有一台机器、一个进程内的服务**：没有跨主机，没有多实例（`x04 --multi-instance` 是另一回事，本轮没跑）。
- **“两个不同客户端”是 MCP 二进制 + 脚本化 HTTP 客户端**，不是两个不同厂商的真实 AI 编程助手；没有真实模型参与，没有验证模型能否正确使用这些工具。
- **只跑了 SQLite**；P1 功能的 PostgreSQL/MySQL 验证是 HTTP 集成测试层面的，这个脚本没有在它们上跑（`IRONFORGE_E2E_DB_URL` 仅支持 x04/ci-compat）。
- 人工界面（Web UI）不在此脚本内，另一会话的分支未合并，**未验证**。
- 脚本不在 CI 里运行，是手工入口；结果不等于合并后基线或远端 CI 通过。
- 没做的功能（强制模式、O03b 恢复演练）自然也不在其中。

## 复现

```bash
cargo build -p rg-cli -p rg-mcp
python3 scripts/collab-e2e.py --scenario p1 --json-out /tmp/p1.json
```
