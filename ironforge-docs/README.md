# IronForge 分析报告索引

**生成时间**: 2026-05-09 ~ 2026-07-05
**项目代号**: IronForge (Rust Git 托管平台)

---

## 当前文档

### 0. 项目架构重盘分析步骤
**文件**: `project-architecture-analysis-plan-2026-07.md`
**生成时间**: 2026-07-05

用于后续多轮重建项目架构文档和前后端结构分布文档的分析计划。按代码基线、系统入口、后端 crates、数据库、HTTP/API、Git/SSH、前端、横切能力、扩展能力、测试部署等 10 轮推进。

### 0.1 项目架构重盘分析记录
**文件**: `project-architecture-analysis-notes-2026-07.md`
**生成时间**: 2026-07-05（第 0-10 轮已完成）

按分析步骤逐轮沉淀代码事实、架构解读、待核验项和可进入最终文档的内容。第 0-10 轮已完成，覆盖代码基线、系统入口、后端 crates、数据库、HTTP/API、Git/SSH、前端、安全横切、扩展能力、测试部署和最终文档收敛。当前正式入口是 `project-architecture-2026-07.md`、`frontend-backend-structure-2026-07.md` 和 `architecture-followups-2026-07.md`。

### 0.2 项目架构总览（2026-07）
**文件**: `project-architecture-2026-07.md`
**生成时间**: 2026-07-05

基于第 0-10 轮源码分析和修复波次回填整理的项目架构总览，覆盖系统定位、运行入口、Rust workspace、数据模型、HTTP/Git/SSH、安全、CI/Package/MCP、测试部署与当前架构口径修正。

### 0.3 前后端结构分布（2026-07）
**文件**: `frontend-backend-structure-2026-07.md`
**生成时间**: 2026-07-05

整理后端 crates/API/领域模块、前端 SvelteKit 路由/API client/store/i18n/components，以及页面到 REST/Git/OCI/WebSocket/Runner/MCP 能力的映射关系。

### 0.4 架构差异与后续待办（2026-07）
**文件**: `architecture-followups-2026-07.md`
**生成时间**: 2026-07-05

按 P0/P1/P2 整理本轮重盘发现的安全、权限、部署、运维、文档口径和工程化后续任务。2026-07-05 的修复波次已把 P0/P1 安全、权限和部署强优先级项清零，当前保留的主要是长期生产化和技术债方向。

### 0.5 架构修复执行计划（2026-07）
**文件**: `architecture-remediation-plan-2026-07.md`
**生成时间**: 2026-07-05

把 `architecture-followups-2026-07.md` 的 P0/P1 缺口转成可执行修复波次，覆盖认证会话、仓库级权限、Runner/部署、CI 回归和 P1 安全运维硬化，并列出建议 PR 切分与验收矩阵。当前这些波次已完成首轮实现与文档回填，作为修复过程记录和验收矩阵保留。

---

### 1. Gitea vs IronForge 功能对比 v3.1
**文件**: `gitea-vs-ironforge-2026.md`
**生成时间**: 2026-06-16（v3.1，基于代码实际状态修正）

基于 Gitea 1.26 与 IronForge Phase 1-21 的全面功能对比分析，含完成度评估（~85%）和差距识别。

---

### 2. Gitea 功能差距清单
**文件**: `gitea-gap-list.csv`  
**生成时间**: 2026-06-16（v3.1 同步更新）

功能差距清单 CSV（含 60+ 条逐一状态标注，程序化处理友好）。

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
| `archive/gitea-feature-gap-analysis.md` | 反映 Phase 17 之前状态，已被 v2.0/v3.1 替代 | `gitea-vs-ironforge-2026.md` |
| `archive/gix-migration-status-report.md` | 反映 Phase 18 之前状态，数据已过时 | CLAUDE.md 中 gix 迁移状态 |
| `archive/gix-migration-feasibility-analysis.md` | 可行性评估已完成使命 | CLAUDE.md / AGENT.md 中说明 |

---

## 项目状态总览

| 维度 | 状态 | 备注 |
|------|------|------|
| 架构重盘 | ✅ 完成 | 第 0-10 轮分析完成，已生成架构总览、前后端结构分布和 followups |
| P0/P1 修复 | ✅ 首轮完成 | 认证、权限、Runner、部署、CI token、Artifact、MCP runtime、CSP、API client 拆分等已回填 |
| 数据库 | SQLite-only | PostgreSQL 是后续生产化方向，不是当前能力 |
| MCP | stdio 可用 | `--sse` 当前 fail-fast，SSE transport 是后续方向 |
| Package Registry | Native + Generic fallback | Cargo/npm/Maven/PyPI/Docker/NuGet/RubyGems/Helm/Composer/Generic 为 native/明确实现，其余 type 标注 Generic fallback |
| 前端 API client | ✅ 已拆分 | `client.svelte.ts` 为 38 行纯 re-export，领域实现分布在独立模块 |

---

## 下一步建议

1. **生产化**: PostgreSQL 支持、fresh DB migration smoke 常态化、备份恢复 runtime smoke。
2. **协议与生态**: MCP SSE transport、Package 专用协议补全、OCI/Package 更深度兼容测试。
3. **Git 技术债**: 每次 gix 升级复查 pack/rebase/archive/unified diff 等阻塞项，优先迁移可对拍验证的只读路径。
4. **文档维护**: 新增能力先更新 `project-architecture-2026-07.md` 和 `frontend-backend-structure-2026-07.md`，风险项进入 `architecture-followups-2026-07.md`。

---

## 相关链接

- [ARCHITECTURE.md](../ARCHITECTURE.md) — 历史架构设计背景
- [CLAUDE.md](../CLAUDE.md) — AI 协作上下文（最完整的踩坑记录和实现现状）
- [AGENT.md](../AGENT.md) — AI 助手统一入口
- [Gitea 1.26 发布说明](https://blog.gitea.com/release-of-1.26.0/)
- [gix (gitoxide) 项目](https://github.com/Byron/gitoxide)

---

## 更新历史

| 日期 | 更新内容 |
|------|----------|
| 2026-07-05 | 完成架构修复计划 Wave 4 第二十四批修复：前端 API client 剩余领域全部拆分，`client.svelte.ts` 降至 38 行纯 re-export |
| 2026-07-05 | 完成架构修复计划 Wave 4 第二十三批修复：Auth、Releases、Issues、Pull Requests/Reviews、Pipelines、Wiki API 阶段性拆分 |
| 2026-07-05 | 完成架构修复计划 Wave 4 第二十二批修复：Boards、Time Tracking、Search API 阶段性拆分 |
| 2026-07-05 | 完成架构修复计划 Wave 4 第二十一批修复：Runner 管理 API 从主 client 拆分到 `runners.ts` |
| 2026-07-05 | 完成架构修复计划 Wave 4 第二十批修复：Package Registry API 从主 client 拆分到 `packages.ts` |
| 2026-07-05 | 完成架构修复计划 Wave 4 第十九批修复：API client WebSocket helper 拆分到独立模块并保持主入口 re-export |
| 2026-07-05 | 完成架构修复计划 Wave 4 第十八批修复：Markdown sanitizer 改为 allowlist，并新增 smoke 测试 |
| 2026-07-05 | 完成架构修复计划 Wave 4 第十七批修复：前端旧领域 API 文件改为兼容 re-export，消除重复实现漂移 |
| 2026-07-05 | 完成架构修复计划 Wave 4 第十六批修复：Package Registry 前端标注 native adapter 与 Generic fallback |
| 2026-07-05 | 完成架构修复计划 Wave 4 第十五批修复：README/CONTRIBUTING 回归入口统一到现有 `scripts/` 自动化脚本 |
| 2026-07-05 | 完成架构修复计划 Wave 4 第十四批修复：CSP `connect-src` 支持跨域 API/WS origin 配置 |
| 2026-07-05 | 完成架构修复计划 Wave 4 第十三批修复：pipelines 页面接入 job log WebSocket，日志弹窗实时追加 runner 输出 |
| 2026-07-05 | 完成架构修复计划 Wave 4 第十二批修复：MCP 文档和 CLI 统一为 stdio-only，`--sse` 明确报错 |
| 2026-07-05 | 完成架构修复计划 Wave 4 第十一批修复：Git HTTP/SSH receive-pack 在 ref 更新前执行 protected branch 拒绝 |
| 2026-07-05 | 完成架构修复计划 Wave 4 第十批修复：Docker runtime 镜像包含 `ironforge-runner` / `ironforge-mcp`，compose runner 示例改用独立 runner |
| 2026-07-05 | 完成架构修复计划 Wave 4 第九批修复：新增 SQLite `backup-db` / `restore-db` CLI，并补部署运维命令 |
| 2026-07-05 | 完成架构修复计划 Wave 4 第八批修复：Artifact raw 上传保存文件、新增下载端点并补 repo read 权限 |
| 2026-07-05 | 完成架构修复计划 Wave 4 第七批修复：`/metrics` registry 未初始化时返回 503，不再 panic |
| 2026-07-05 | 完成架构修复计划 Wave 4 第六批修复：MCP stdio 入口显式创建 Tokio runtime，避免 tools/resources 调用 panic |
| 2026-07-05 | 完成架构修复计划 Wave 4 第五批修复：CI_JOB_TOKEN 接入 repo content/archive 和 package 只读 HTTP 路径 |
| 2026-07-05 | 完成架构修复计划 Wave 4 第四批修复：外部 runner 指定 image 时 Docker 不可用直接失败，不再回退本地执行 |
| 2026-07-05 | 完成架构修复计划 Wave 4 第三批修复：Rate Limit 默认忽略转发头，新增 trusted proxy 配置后才读取 `X-Forwarded-For` / `X-Real-IP` |
| 2026-07-05 | 完成架构修复计划 Wave 4 第二批修复：SSO callback 设置 HttpOnly cookie 并 redirect，修复 state/PKCE cookie 覆盖和路径问题 |
| 2026-07-05 | 完成架构修复计划 Wave 4 首批修复：LDAP TLS 默认开启证书校验，仅显式 insecure 时跳过 |
| 2026-07-05 | 完成架构修复计划 Wave 3 第三批修复：新增 GitHub Actions 回归 workflow，覆盖后端、前端、迁移和 compose config |
| 2026-07-05 | 完成架构修复计划 Wave 3 第二批修复：Compose secret 改为 `.env`，Prometheus target 对齐 `ironforge:8080` |
| 2026-07-05 | 完成架构修复计划 Wave 3 首批修复：Runner 注册改为 admin 授权，CLI/runner 自动注册支持 auth token |
| 2026-07-05 | 完成架构修复计划 Wave 2 第四批修复：SSH Git repo-level 读写权限校验 |
| 2026-07-05 | 完成架构修复计划 Wave 2 第三批修复：OCI `/v2` pull/push 权限和 token scope 授权 |
| 2026-07-05 | 完成架构修复计划 Wave 2 第二批修复：Package REST/protocol 权限、publish validate、package 表名单复数纠偏迁移 |
| 2026-07-05 | 完成架构修复计划 Wave 2 首批修复：Pipeline API 接入仓库读写权限并补 ID 归属校验 |
| 2026-07-05 | 完成架构修复计划 Wave 1 首批修复：cookie-aware 用户/Admin/SSO 账号接口、MFA disable 校验、SSO state 收紧 |
| 2026-07-05 | 新增架构修复执行计划：将 P0/P1 followups 拆成认证、权限、Runner/部署、CI 和 P1 硬化修复波次 |
| 2026-07-05 | 完成第 10 轮收敛：生成项目架构总览、前后端结构分布、架构差异与待办三份正式文档 |
| 2026-07-05 | 架构重盘分析记录追加第 9 轮：测试、构建、部署与运维 |
| 2026-07-05 | 架构重盘分析记录追加第 8 轮：CI/Runner、Package Registry、MCP 与扩展能力 |
| 2026-07-05 | 架构重盘分析记录追加第 7 轮：安全、认证、配置与横切能力 |
| 2026-07-05 | 架构重盘分析记录追加第 6 轮：前端路由、状态与前后端映射 |
| 2026-07-05 | 架构重盘分析记录追加第 5 轮：Git、SSH 与协议层 |
| 2026-07-05 | 架构重盘分析记录追加第 4 轮：HTTP API、Git HTTP 与实时通道 |
| 2026-07-05 | 架构重盘分析记录追加第 3 轮：领域模型、数据库和迁移链路 |
| 2026-07-05 | 架构重盘分析记录追加第 2 轮：后端 crate 职责和依赖关系 |
| 2026-07-05 | 架构重盘分析记录追加第 1 轮：整体系统分层和运行入口 |
| 2026-07-05 | 新增项目架构重盘分析记录文档，完成第 0 轮基线确认 |
| 2026-07-05 | 新增项目架构重盘分析步骤文档，作为新架构文档与前后端结构文档的分析入口 |
| 2026-05-09 | 创建 gix 迁移可行性分析 |
| 2026-05-10 | 创建 gix 迁移状态报告 + CI Runner 架构 + Gitea 差距分析 |
| 2026-05-10 | 创建本文档索引 |
| 2026-06-07 | 新增 v2.0 对比报告 + CSV 清单，Phase 21 完成 |
| 2026-06-07 | 文档对齐（Phase 21 状态 + gix 迁移最新数据） |
| 2026-06-16 | 文档归档：过时报告移至 archive/，精简索引，统一引用路径 |
| 2026-06-16 | 文档同步 v3.1：完成度 80%→85%，状态对齐代码实际完成情况 |
