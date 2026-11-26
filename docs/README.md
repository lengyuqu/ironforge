# IronForge 文档索引

> 文档只写**当前仍成立的事实**，并注明代码入口。带日期的快照、逐次核实记录、已完成任务的执行日志不保留——git 历史里有。

## 入口文档

| 文件 | 用途 |
|---|---|
| [../README.md](../README.md) | 项目说明、快速开始、REST / Git 示例 |
| [../AGENT.md](../AGENT.md) | AI 工具轻量入口（概览 + 文件速查） |
| [../CLAUDE.md](../CLAUDE.md) | 深度协作上下文（约定、当前实现、技术债、踩坑、错误排查）⭐ |
| [../ARCHITECTURE.md](../ARCHITECTURE.md) | 架构总览与技术选型 |
| [../CONTRIBUTING.md](../CONTRIBUTING.md) | crate 边界、编码规范、测试要求 |

## 设计（当前实现）

| 文件 | 内容 |
|---|---|
| [design/agent-collaboration.md](design/agent-collaboration.md) | 同账号多 Agent 协作：会话、任务、消息、交接、依赖、决定、完成条件、草稿分支 |
| [design/agent-principals.md](design/agent-principals.md) | Agent 账号、owner 上限、令牌、审计、通知、提交身份 |
| [design/authorization.md](design/authorization.md) | 仓库三档权限、PAT 阶梯、路由元数据、审批原语、LDAP 组同步、Unit 级权限决策 |
| [design/ci.md](design/ci.md) | CI 配置、Runner、第三方 Action 边界、Secret、密钥扫描、配额 |
| [design/adr-0001-third-party-actions.md](design/adr-0001-third-party-actions.md) | ADR：第三方 Action 执行与安全边界（Accepted） |
| [design/data.md](design/data.md) | 数据库后端、BlobStorage、附件、Markdown 模板、备份边界 |
| [design/code-intelligence.md](design/code-intelligence.md) | 仓库地图、符号索引、PR 上下文包 |

## 指南与基线

| 文件 | 内容 |
|---|---|
| [git-protocol.md](git-protocol.md) | Git Smart Protocol 实现细节与协议踩坑 |
| [git-client-matrix.md](git-client-matrix.md) | 真实 git/ssh/curl 客户端兼容矩阵与自动化入口 |
| [performance.md](performance.md) | 大仓库固定数据集、基线数字、阈值、已修性能问题 |
| [operations.md](operations.md) | 支持边界、失效矩阵、备份恢复、单实例升级、多实例阻塞项 |

## Agent 接入

| 文件 | 内容 |
|---|---|
| [../.ai/README.md](../.ai/README.md) | MCP / REST 接入、工具清单、协作 REST、令牌 scope |
| [../.ai/guardrails.md](../.ai/guardrails.md) | 变更操作的权限边界（BLOCK / CONFIRM / WARN / ALLOW） |
| [../.ai/prompts/](../.ai/prompts/) | Prompt 模板 |

## 多 Agent 任务协议（启用时读）

| 文件 | 内容 |
|---|---|
| [../PLANNING.md](../PLANNING.md) | 稳定协作约束、共享契约、认领与合并规则 |
| [../TASKS.md](../TASKS.md) | 任务状态、认领/完成协议、检查清单 |
| [../templates/](../templates/) | PLANNING / TASKS / task 模板 |

## 维护规则

1. **新增能力先改代码与测试，再改文档**；文档必须写出代码入口，不能只写结论。
2. 不写「当前 / 现在」这种会过期的相对表述；需要基线时写版本号（如 0.5.1）。
3. 不保留带日期的快照、一次性核实、任务执行日志、完成清单；这些内容由 commit / PR / CHANGELOG 承载。
4. 一个主题只保留一篇文档；同一事实不要在两处维护。旧文档在删除前先把仍成立的部分合并进当前文档。
5. 新增设计文档放在 `docs/design/`，文件名用稳定的主题名，不带日期。
6. 文档中的命令与路径必须可执行/可解析；CI 的文档范围判定会跳过只改文档的 PR。
