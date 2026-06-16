# IronForge 分析报告索引

**生成时间**: 2026-05-09 ~ 2026-06-16  
**项目代号**: IronForge (Rust Git 托管平台)

---

## 当前文档

### 1. Gitea vs IronForge 功能对比 v2.0
**文件**: `gitea-vs-ironforge-2026.md`  
**生成时间**: 2026-06-07

基于 Gitea 1.26 与 IronForge Phase 1-21 的全面功能对比分析，含完成度评估和核心差距识别。

---

### 2. Gitea 功能差距清单
**文件**: `gitea-gap-list.csv`  
**生成时间**: 2026-06-07

功能差距清单（CSV 格式，便于程序化处理和导入外部工具）。

---

### 3. CI/CD Runner 架构设计
**文件**: `ci-runner-architecture.md`  
**生成时间**: 2026-05-10

CI Runner 调度系统完整架构设计，含 Agent 生命周期、Artifact 管理、Job 调度。

---

### 4. 全方位改进空间分析
**文件**: `ironforge-improvement-analysis-2026-06-09.md`  
**生成时间**: 2026-06-09

Phase 1-21 完成后 Rust/Axum/gix/russh/SvelteKit 各层的改进建议。

---

### 5. P0 包注册表更新
**文件**: `p0-update-2026-06-08.md`  
**生成时间**: 2026-06-08

Package Registry PyPI 适配器实现细节。

---

## 已归档文档

过时的分析报告已移至 `archive/` 目录，详情见 `archive/ARCHIVE.md`：

| 文件 | 归档原因 | 替代方案 |
|------|---------|---------|
| `archive/gitea-feature-gap-analysis.md` | 反映 Phase 17 之前状态，已被 v2.0 替代 | `gitea-vs-ironforge-2026.md` |
| `archive/gix-migration-status-report.md` | 反映 Phase 18 之前状态，数据已过时 | CLAUDE.md 中 gix 迁移状态 |
| `archive/gix-migration-feasibility-analysis.md` | 可行性评估已完成使命 | CLAUDE.md / AGENT.md 中说明 |

---

## 项目状态总览

| 维度 | 状态 | 备注 |
|------|------|------|
| Phase 进度 | 1-21 全部完成 | 核心功能 + CI/CD Runner + Package Registry/LDAP/SSO/2FA/Audit/Mirror/Board |
| gix 迁移 | ~70% 完成 | 剩余 19 处 CLI（PR diff/rebase/Fetch/Mirror/Import/GPG 等） |
| Gitea 功能 | 40-50% 完成 | CI/CD 深度功能 + Docker 包注册表为最大缺口 |
| 工程化 | ✅ 完成 | OpenAPI + 集成测试 + 安全 + 可观测性 |

---

## 下一步行动

1. **P0 核心缺口**: Package Registry Docker/OCI 容器镜像仓库
2. **P1 重要功能**: PR Merge 完整策略、OAuth2 增强、Actions Concurrency、Token 权限
3. **技术债**: gix 迁移剩余 19 处 CLI 调用

---

## 相关链接

- [ARCHITECTURE.md](../ARCHITECTURE.md) — 项目架构设计
- [CLAUDE.md](../CLAUDE.md) — AI 协作上下文（最完整的踩坑记录和实现现状）
- [AGENT.md](../AGENT.md) — AI 助手统一入口
- [Gitea 1.26 发布说明](https://blog.gitea.com/release-of-1.26.0/)
- [gix (gitoxide) 项目](https://github.com/Byron/gitoxide)

---

## 更新历史

| 日期 | 更新内容 |
|------|----------|
| 2026-05-09 | 创建 gix 迁移可行性分析 |
| 2026-05-10 | 创建 gix 迁移状态报告 + CI Runner 架构 + Gitea 差距分析 |
| 2026-05-10 | 创建本文档索引 |
| 2026-06-07 | 新增 v2.0 对比报告 + CSV 清单，Phase 21 完成 |
| 2026-06-07 | 文档对齐（Phase 21 状态 + gix 迁移最新数据） |
| 2026-06-16 | 文档归档：过时报告移至 archive/，精简索引，统一引用路径 |
