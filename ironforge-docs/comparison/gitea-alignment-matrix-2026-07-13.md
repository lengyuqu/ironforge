# IronForge 对齐 Gitea 1.26.4 可验证矩阵

> 基线：Gitea 1.26.4、IronForge `39e33a9` 之后的当前工作区
> 机器可读明细：[gitea-gap-list.csv](./gitea-gap-list.csv)
> 本文是 `ALIGN-001` 的基线产物；任务状态仍以 [gitea-alignment-progress-2026-07-13.md](./gitea-alignment-progress-2026-07-13.md) 为准。

## 1. 计分口径

完成度使用 100 分加权矩阵，不再用“文件、表、端点数量”或简单功能条目平均数估算。

- 权重反映小团队自托管 Gitea 的真实使用频率、迁移阻力和生产风险；
- `DONE`：完整用户/协议路径已有代码与测试证据，获得该项全部分值；
- `PARTIAL`：主路径存在，但兼容语义、权限、真实客户端、跨数据库或 UI/E2E 尚未闭环，按证据获得部分分值；
- `MISSING`：没有可用闭环，得 0 分；
- Generic package fallback 不计对应专用协议完成；
- 未执行字段、静默跳过和错误 capability 广告不能计为完成；
- IronForge 独有能力（例如 MCP）不用于抬高 Gitea 对齐分数；
- 每一行必须包含代码证据、测试证据、状态和后续任务 ID。

旧报告的约 85% 是按“功能名是否存在”统计，未扣除协议细节、真实客户端、完整 Actions runtime、对象存储、队列和恢复能力，因此不再作为当前完成度事实源。

## 2. 当前得分

| 领域 | 权重 | 已得分 | 完成度 | 主要扣分项 |
|---|---:|---:|---:|---|
| Git 协议 | 12 | 11.25 | 93.8% | 大仓库性能基线 |
| 仓库管理 | 12 | 8.35 | 69.6% | Push Mirror、upload-archive、模板仓库、Go to file/目录删除 |
| Issue/PR/Wiki | 16 | 14.85 | 92.8% | YAML Issue Form、PR/Review 评论 Reaction、Lock/Pin/Dependency |
| CI/Actions | 14 | 9.0 | 64.3% | outputs、运行时条件、Service Container、第三方 Action |
| Package Registry | 11 | 8.2 | 74.5% | 治理、Terraform/Go、Linux 发行版和长尾专用协议 |
| 身份与权限 | 10 | 8.0 | 80.0% | LDAP 生命周期、Proxy/PAM/SMTP、用户关系、Unit 权限 |
| API/Webhook/集成 | 8 | 6.5 | 81.3% | Gitea contract、响应兼容、用户/组织/系统 Webhook |
| 运维生产化 | 11 | 6.5 | 59.1% | S3/MinIO、Redis 与剩余任务入队、备份仅 SQLite 且无自动化测试、升级与 HA |
| Web/Admin | 6 | 5.2 | 86.7% | 高频细节、外部身份生命周期、任务/队列管理页 |
| **合计** | **100** | **77.85** | **77.9%** | — |

当前统一口径为 **77.9%**（2026-10-21 复核更新，此前为 75.4%）。`GIT-003` 完成真实客户端矩阵，`GIT-004` 完成 shallow/deepen，`GIT-002` 完成 partial clone/filter 后，Git 协议领域达到 93.8%，M0 目标已达到。`CI-200` 已通过 ADR 冻结第三方 Actions 的安全和兼容边界，但 runtime 尚未实现，因此不增加分值。`STORAGE-001` 完成统一 BlobStorage、本地原子 backend 和五类持久 Blob 迁移；`ISSUE-101` 完成 Gitea 兼容 Markdown Issue/PR Template；`ISSUE-103` 完成四类附件的流式上传/下载、权限、配额、Web 与浏览器 E2E。S3/MinIO、迁移/repair、YAML Issue Form 仍未计为完成。

2026-10-21 复核：7 月 14 日之后落地的能力此前未回填到本矩阵，现按代码与测试证据补记——多 Assignee（`ISSUE-105`，+0.4，DONE）、Issue/评论 Reaction（`ISSUE-104`，+0.25，PR/Review 评论仍缺，保持 PARTIAL）、失败 Job 重跑（`CI-201`，+0.5，DONE：补 11 项专项测试，并修复下游 Stage 不会重建导致 Pipeline 误判成功的缺陷）、全实例备份恢复（`OPS-501`，+0.5，仅 SQLite 且无自动化测试，保持 PARTIAL）、持久化后台队列（`QUEUE-001`，+0.5，webhook/邮件/mirror 已入队，无 Redis，保持 PARTIAL）、Blame（`REPO-104`，+0.35，Go to file 与目录删除未做，保持 PARTIAL）。同时发现 `GET /repos/{owner}/{name}/log` 的 `path` 参数被忽略，文件历史不可用；同日已修复（`rg_git::history::commits_touching_path`，与 `git log -- <path>` 对拍），不单独计分，blob 页头已加“历史”入口。

## 3. 基线结论

1. IronForge 已具备核心 Git 托管、Issue/PR/Wiki、基础 CI、主要包协议、组织权限和 Web 管理能力。
2. `GIT-001` 先消除错误能力广告；`GIT-003` 把 HTTP/SSH、V1/V2 的真实客户端矩阵接入主 CI并修复 V2 acknowledgments section 顺序；`GIT-004` 与 `GIT-002` 分别完成 shallow/deepen 和 partial clone/filter，当前广告为 `fetch=shallow filter`。
3. 最大的迁移阻力是 Gitea Actions 仍为有限 YAML 适配层，不具备第三方 Action runtime；`ADR-0001` 已确定专用外部 Runner、临时容器、默认断网、Secret 最小注入和 fork 审批边界。
4. 最大的生产化差距是 S3/MinIO 与 legacy 迁移/repair、持久化 Queue、全实例 Backup/Restore 和 HA/升级演练；统一 BlobStorage 基础已由 `STORAGE-001` 完成。
5. 旧 CSV 中 PostgreSQL、Wiki 历史、CI 日志队列、维护模式、搜索高亮等缺失状态均已被当前代码推翻；本次已重建而非继续增量修补旧状态。
6. Gitea 1.26.4 的 Arch、Chef、CRAN 包能力已补入矩阵，并登记为 `PKG-306`，避免无意遗漏。

## 4. 更新规则

每次任务完成时：

1. 更新 `gitea-gap-list.csv` 对应行的 IronForge 行为、证据、状态和得分；
2. 重新计算领域得分和总分；
3. 更新本文“当前得分”；
4. 更新进度台账的状态快照和变更记录；
5. 如果 Gitea 对比版本升级，新增独立基线变更记录，不能静默替换 1.26.4。
