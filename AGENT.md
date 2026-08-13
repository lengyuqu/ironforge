# IronForge — AI Agent 协作指南

> 本文件是 IronForge 项目的 **AI 助手统一入口**。
> 大多数 AI 编程助手（Codex、Trae、CodeBuddy、WorkBuddy 等）会**优先自动读取本文件**，也会同时读取 `CLAUDE.md`。
> Claude Code 默认自动读取 `CLAUDE.md`，但同样会读取本文件。
> **建议**：先通读本文件获取概览，再根据任务深入 `CLAUDE.md` 或其他文档。

> **状态**：维护中 ｜ **可信度**：确认 ｜ **来源**：仓库代码 / 文档 / 运行结果 ｜ **最后更新**：2026-08-14

---

## 快速定位（30 秒了解项目）

**IronForge**（铁匠铺）是一个用 Rust 从零实现的轻量级 Git 托管平台，对标 Gitea/Forgejo。

- **目标**: 内存 <50MB、单二进制部署、全功能（仓库/Issue/PR/Wiki/CI/包注册表/企业认证/审计/代码搜索）
- **阶段**: Phase 1~21 全部完成
- **技术栈**: Rust (Axum/SeaORM) + SvelteKit 5，SQLite，gix (gitoxide) + Git CLI gateway

### 关键文件速查

| 文件 | 用途 | 何时读取 |
|------|------|--------|
| `CLAUDE.md` | 最完整的 AI 协作上下文（踩坑记录、依赖版本、常见错误、实现现状清单） | **每次开始工作前必读** |
| `.ai/guardrails.md` | **Agent 生命周期护栏（权限边界规则）** | **执行任何变更操作前必读** ⭐ |
| `ironforge-docs/README.md` | 文档索引（单一事实来源） | 找任何分析报告时先读 |
| `ironforge-docs/architecture/project-architecture-2026-07.md` | 当前代码事实架构总览 | 设计新功能、核验模块边界时 |
| `ironforge-docs/architecture/frontend-backend-structure-2026-07.md` | 当前前后端结构和页面/API 映射 | 修改前端页面、API client 或 HTTP handler 时 |
| `ironforge-docs/architecture/architecture-followups-2026-07.md` | 当前已修复项、P2 长期方向和旧口径修正 | 判断风险、技术债和后续方向时 |
| `ARCHITECTURE.md` | 架构设计文档（技术选型决策、模块设计、数据模型、核心子系统） | 了解设计背景和架构决策 |
| `CONTRIBUTING.md` | 开发规范、crate 边界规则、编码规范 | 写新代码时 |
| `README.md` | 快速开始、REST API 示例 | 首次接触项目时 |
| `.ai/README.md` | AI Agent 接入指南（MCP + REST API + prompt 模板） | 需要让 AI 工具调用 IronForge 时 |
| `templates/` | 多 Agent 协作骨架（PLANNING.md / TASKS.md / tasks/Txx.md） | 启用多 Agent 并行开发时 |

---

## Agent 生命周期护栏

> ⚠️ **执行任何变更操作前，必须先阅读 [`.ai/guardrails.md`](.ai/guardrails.md)**

护栏等级：🔴 BLOCK / 🟠 CONFIRM / 🟡 WARN / 🟢 ALLOW。覆盖数据库迁移、部署、删除、安全认证、Git 协议核心、数据完整性六类高风险操作。

---

## 按任务类型延伸阅读

### 修改 Git 协议相关代码
→ `docs/git-protocol.md` + `CLAUDE.md` 踩坑清单

### 开发新功能 / 规划下一步
→ `CLAUDE.md` 实现现状表格 → `ironforge-docs/architecture/` 系列架构文档 → `CONTRIBUTING.md` 编码规范

### gix 迁移 / 替换 git CLI 调用
→ `CLAUDE.md` 技术债与后续方向

### CI/CD Runner 开发
→ `ironforge-docs/ci/ci-runner-architecture.md`

### 前端开发（SvelteKit）
→ `ironforge-docs/architecture/frontend-backend-structure-2026-07.md`

---

## 各 AI 工具的读取习惯

| AI 工具 | 自动读取的文件 | 深度参考 |
|---------|-------------|---------|
| **Claude Code** | `CLAUDE.md`（默认）+ `AGENT.md` | `CLAUDE.md` 提供最完整细节 |
| **Codex / Trae / CodeBuddy** | `AGENT.md`（优先）+ `CLAUDE.md` | `CLAUDE.md` 提供踩坑记录和依赖版本 |
| **WorkBuddy** | `.workbuddy/memory/MEMORY.md` | `AGENT.md` + `CLAUDE.md` + 2026-07 架构文档 |

> `AGENT.md` 是轻量级统一入口，`CLAUDE.md` 是深度上下文。两者互补，建议搭配使用。

---

## 分析报告索引

`ironforge-docs/` 已按主题分子目录整理，完整索引见 [`ironforge-docs/README.md`](ironforge-docs/README.md)：

| 子目录 | 内容 |
|------|------|
| `architecture/` | 架构总览、前后端结构、后续待办、分模块分析、DB 多后端 |
| `analysis/` | 改进与优化整合报告 |
| `comparison/` | Gitea 功能对比 + 差距清单 |
| `ci/` | CI Runner 架构 |
| `testing/` | 功能测试指南 + 前后端审计 |
| `archive/` | 过程文档与过时报告（仅供追溯） |

---

*本文件与 `CLAUDE.md` 保持同步更新。如发现有遗漏或不一致，请同步修正两文件。*
