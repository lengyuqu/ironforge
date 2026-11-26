# IronForge — AI Agent 协作指南

> 轻量统一入口。大多数 AI 工具会优先读取本文件；深度上下文在 [CLAUDE.md](CLAUDE.md)。
> 最后更新：2026-11-26（0.5.1）

## 30 秒了解项目

**IronForge**（铁匠铺）是用 Rust 从零实现的、**面向 AI Agent 编程的 Git 托管平台**：目标内存 <50 MB（本机实测空载约 84 MiB）、单二进制，具备仓库/Issue/PR/Wiki/CI/包注册表/企业认证/审计/代码搜索，并把 Agent 主体身份、最小权限令牌、可重试写入、MCP 与多 Agent 协作做成一等公民。

- 阶段：Phase 1~21 全部完成
- 技术栈：Rust（Axum / SeaORM）+ SvelteKit 5，SQLite / PostgreSQL / MySQL，gix + 仅测试/mirror 使用的 git CLI 网关
- 当前版本：0.5.1

## 关键文件

| 文件 | 用途 | 何时读 |
|---|---|---|
| [CLAUDE.md](CLAUDE.md) | 约定、当前实现、技术债、踩坑、错误排查 | **每次开始工作前** |
| [.ai/guardrails.md](.ai/guardrails.md) | 变更操作的权限边界 | **执行任何变更前** ⭐ |
| [docs/README.md](docs/README.md) | 文档索引与维护规则 | 找文档时 |
| [docs/design/](docs/design/) | 当前设计：协作、授权、Agent、CI、数据、代码智能 | 改对应模块时 |
| [ARCHITECTURE.md](ARCHITECTURE.md) | 架构总览与技术选型 | 理解模块边界时 |
| [CONTRIBUTING.md](CONTRIBUTING.md) | crate 边界、编码规范、测试要求 | 写代码时 |
| [.ai/README.md](.ai/README.md) | MCP / REST 接入与工具清单 | 让 AI 调 IronForge 时 |
| [PLANNING.md](PLANNING.md) / [TASKS.md](TASKS.md) | 多 Agent 任务协议 | 启用并行任务时 |

## 按任务延伸阅读

- 改 Git 协议 → [docs/git-protocol.md](docs/git-protocol.md) + CLAUDE.md 踩坑
- 改协作/Agent → [docs/design/agent-collaboration.md](docs/design/agent-collaboration.md)、[docs/design/agent-principals.md](docs/design/agent-principals.md)
- 改权限/审批 → [docs/design/authorization.md](docs/design/authorization.md)
- 改 CI/Actions → [docs/design/ci.md](docs/design/ci.md) + [ADR-0001](docs/design/adr-0001-third-party-actions.md)
- 改存储/附件/模板 → [docs/design/data.md](docs/design/data.md)
- 改代码智能 → [docs/design/code-intelligence.md](docs/design/code-intelligence.md)
- 部署/恢复/升级 → [docs/operations.md](docs/operations.md)
- 性能回归 → [docs/performance.md](docs/performance.md)

## 硬规则

1. 变更前读 `.ai/guardrails.md`；批量删除、强推、改已执行迁移一律禁止。
2. 文档只写当前仍成立的事实；带日期的快照、逐次核实、已完成任务日志不留在仓库（git 历史里有）。
3. 推送成功不等于 CI 通过；汇报时分列本机、GitHub run、远端 SHA。
