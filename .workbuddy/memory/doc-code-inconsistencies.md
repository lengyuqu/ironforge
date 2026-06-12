---
name: doc-code-inconsistencies
description: 项目文档与代码实际状态不一致的问题清单，需定期核对更新
metadata:
  type: project
---

## 文档与代码不一致问题（2026-05-22 审查发现）

以下文档内容与代码实际状态存在偏差，未来修改相关功能时需同步修正文档。

### 1. p0-prd.md 技术栈描述错误 ✅ 已修正
**文件**: `ironforge/docs/p0-prd.md:11`
**问题**: 描述为 "Rust (Actix-web/SeaORM) + Svelte 5"
**实际**: HTTP 框架使用的是 **Axum 0.8**（`Cargo.toml` 中 `axum = "0.8"`），不是 Actix-web
**影响**: 会给新开发者造成困惑，可能导致错误的技术决策
**修正**: 2026-05-22 已改为 "Rust (Axum/SeaORM) + Svelte 5"

### 2. ARCHITECTURE.md gix 版本号过时 ✅ 已修正
**文件**: `ironforge/ARCHITECTURE.md:70`
**问题**: 描述为 "gix (gitoxide) 0.66+"
**实际**: `Cargo.toml` 中 `gix = "0.83"`
**影响**: 依赖版本信息过时
**修正**: 2026-05-22 已改为 "gix (gitoxide) 0.83+"

### 3. CLAUDE.md 仓库结构错误 ✅ 已修正
**文件**: `ironforge/CLAUDE.md:21-38`
**问题**: `web/` 被错误地列在 `crates/` 目录下；缺少 `rg-runner/`
**实际**: `web/` 在 `ironforge/web/` 根目录下；`rg-runner/` 是独立 workspace member（`ironforge-runner` 二进制）
**影响**: 新开发者找不到 web 目录位置，遗漏 Runner Agent 二进制
**修正**: 2026-05-22 已修正目录结构，添加 `rg-runner/` 说明

### 4. CLAUDE.md DB 迁移列表不完整 ✅ 已修正
**文件**: `ironforge/CLAUDE.md:105`
**问题**: 只列出 m20260424_000001~000009 + m20260508_000001~000005
**实际**: 还有 m20260510_000001~000004（runners/pipeline_jobs/artifacts）+ m20260511_000001~000003（pr_head_repo_id/indexes/fts5_triggers）
**影响**: 迁移历史记录不完整
**修正**: 2026-05-22 已补充完整迁移列表

### 5. CLAUDE.md CLI 子命令描述不完整 ✅ 已修正
**文件**: `ironforge/CLAUDE.md:153`
**问题**: 只提到 `serve` 和 `create-repo`
**实际**: 还有 `migrate`（数据库迁移）和 `runner`（Runner Agent 模式）子命令
**影响**: CLI 功能描述不完整
**修正**: 2026-05-22 已补充 `migrate` 和 `runner` 子命令

### 6. Gitea Gap Analysis 报告时间窗口 ✅ 已处理
**文件**: `ironforge-docs/gitea-feature-gap-analysis.md:16-19`
**问题**: 报告声称 CI/CD Runner (0%)、Job 执行 (0%)、Artifact 管理 (0%) 完全未实现
**实际**: Phase 17（2026-05-10 下午）已完成 Runner 调度、独立 Agent 二进制、Artifact 管理、WebSocket 日志推送。CI/CD 完成度实际远高于 5%。
**根因**: 报告生成于 2026-05-10 上午，Phase 17 在同一天下午完成，时间窗口重叠导致数据滞后。
**处理**: 2026-05-22 已在报告顶部添加时间窗口声明，指引读者查阅 `CLAUDE.md` 获取最新状态。

### 7. gix 迁移报告时间窗口 ✅ 已处理
**文件**: `ironforge-docs/gix-migration-status-report.md:13` vs `ironforge/CLAUDE.md`
**问题**: 
- 迁移报告声称 "18 处 git CLI 调用" 待替换
- `CLAUDE.md` 声称 "13 处 git CLI 保留"
- 两者相差 5 处
**实际**: 两个数字都是正确的。报告生成于 Phase 18 之前（18 处），Phase 18 迁移了 5 处后剩余 13 处。差异源于时间窗口不同，不是数据错误。
**根因**: 报告生成于 2026-05-10，Phase 18 在同一天稍后完成，导致报告数字未及时更新。
**处理**: 2026-05-22 已在报告顶部添加时间窗口声明，说明 "18 处" 是 Phase 18 之前的状态，Phase 18 后减少至 13 处。指引读者查阅 `CLAUDE.md` 获取最新进度。

---

## 建议的文档维护流程

1. **每次 Phase 完成后**: 检查 `ironforge-docs/` 下的分析报告是否需要更新数据
2. **技术栈变更时**: 同步更新 PRD 和架构文档中的技术选型描述
3. **每季度**: 做一次文档与代码的完整一致性审查
