# IronForge 对齐 Gitea 1.26.4 进度台账

> 建立日期：2026-07-13
> 最近更新：2026-07-13
> 对比基线：Gitea 1.26.4、IronForge `6052062` 及当前工作区
> 任务定义：[gitea-detail-alignment-plan-2026-07-13.md](./gitea-detail-alignment-plan-2026-07-13.md)
> 本文职责：只维护任务状态、证据、当前工作和变更记录；范围、依赖、估算与验收门仍以任务定义文档为准。

## 1. 状态定义

| 状态 | 含义 | 进入条件 | 退出条件 |
|---|---|---|---|
| `DONE` | 已完成 | 代码、测试、文档和兼容性验收全部通过 | 发现回归时退回 `DOING` 或 `BLOCKED` |
| `DOING` | 修改中 | 已开始产生代码、测试或对齐矩阵成果 | 达到 DoD 后转 `DONE`；遇到外部阻塞转 `BLOCKED` |
| `READY` | 可开始 | 范围、依赖和验收标准明确 | 开始实施后转 `DOING` |
| `TODO` | 待开始 | 已进入计划但尚未满足开工条件 | 依赖和验收标准明确后转 `READY` |
| `BLOCKED` | 已阻塞 | 存在明确外部依赖、技术限制或待决策事项 | 阻塞解除后转 `READY` 或 `DOING` |

状态约束：

- 同一时间原则上只保留一个主任务为 `DOING`；
- `DONE` 必须同时附代码证据、测试证据和验收说明，不能只凭接口、表或页面存在；
- 纯文档任务至少需要文档链接、审阅结论和后续任务入口；
- 状态变化必须追加到本文末尾的变更记录；
- 规划文档与本文状态冲突时，以本文的“任务状态总表”为当前状态事实源。

## 2. 当前快照

| 指标 | 数量 | 说明 |
|---|---:|---|
| 总任务 | 52 | 来自细节功能计划 M0–M5 |
| `DONE` | 0 | 尚无对齐任务完成全部 DoD |
| `DOING` | 1 | `ALIGN-001` |
| `READY` | 1 | `CI-200`，暂不抢占当前主任务 |
| `TODO` | 50 | 等待依赖、验收标准或排期 |
| `BLOCKED` | 0 | 当前没有确认的外部阻塞 |

当前主任务：`ALIGN-001` — 建立代码可验证的 Gitea 对齐矩阵，废止旧 CSV 中的过时状态。

下一代码任务：`GIT-001` — 修正 Protocol V2 shallow/deepen；在完整实现和真实客户端验证前，不得继续广告未兑现的能力。

## 3. 已完成的前置基础能力

以下能力已在本轮细节对齐计划前完成，用于说明当前起点；它们不计入上述 52 个增量任务的 `DONE` 数量。

| 基础项 | 状态 | 已完成范围 | 代码/测试证据 |
|---|---|---|---|
| 多数据库首轮运行兼容 | `DONE` | SQLite、PostgreSQL、MySQL migration、CRUD、计数器、FTS、认证并发与服务 `/health` smoke | `crates/rg-core/tests/multi_backend_smoke.rs`、`.github/workflows/regression.yml` |
| Gitea Actions 有限兼容基础 | `DONE` | YAML 转换、Secrets、Variables、Matrix、Cache、本地 Reusable Workflow、Environment 审批、OIDC；不支持语义 fail closed | `crates/rg-ci/src/gitea_actions.rs`、`crates/rg-ci/src/condition.rs`、`crates/rg-http/tests/ci_secrets_tag_protection_tests.rs` |
| Pull Mirror | `DONE` | Pull mirror 创建、更新、删除、定时/手动同步和设置页 | `crates/rg-core/src/mirror/service.rs`、`crates/rg-http/src/api/mirrors.rs`、`web/src/routes/[owner]/[repo]/settings/mirror/+page.svelte` |
| HTTP 仓库归档下载 | `DONE` | 按 ref 下载 zip/tar.gz；不等同于 `git archive --remote` / upload-archive | `crates/rg-http/src/api/archive.rs` |
| LDAP/OIDC/MFA 登录闭环 | `DONE` | LDAP 登录、OIDC Discovery/PKCE、MFA challenge、登录审计和管理员解锁 | `crates/rg-core/src/auth/`、`crates/rg-http/src/api/sso.rs`、`crates/rg-http/src/api/mfa.rs`、`crates/rg-http/tests/admin_sso_audit_tests.rs` |

## 4. 当前修改中

### ALIGN-001 — 代码可验证的 Gitea 对齐矩阵

状态：`DOING`

完成情况：

- [x] 固定 Gitea 1.26.4 和 IronForge `6052062` 基线；
- [x] 建立独立进度台账和统一状态定义；
- [x] 确认 Protocol V2 `fetch=shallow` 存在“已广告但未执行”问题；
- [ ] 从旧 `gitea-gap-list.csv` 迁移仍然有效的功能条目；
- [ ] 为每项能力补齐 Gitea 行为、IronForge 行为、代码证据和测试证据；
- [ ] 统一旧报告的 85% 与新计划 68%–72% 完成度口径；
- [ ] 明确 Arch、Chef、CRAN 等未列入 M3 的 Gitea Package 能力是补入 backlog 还是显式排除；
- [ ] 修正里程碑估算与原子任务合计不一致的问题；
- [ ] 归档或重建旧 `gitea-gap-list.csv`；
- [ ] 完成审阅并将 `GIT-001` 转为 `READY`。

当前证据：

- Capability 广告：`crates/rg-git/src/protocol/v2.rs::send_capability_advertisement`；
- HTTP capability 广告：`crates/rg-http/src/git_v2.rs::build_v2_capability_sync`；
- 参数未执行：`crates/rg-git/src/protocol/v2.rs::handle_fetch` 中 `_shallows`、`_deepen`、`_filter`；
- 旧状态源：`ironforge-docs/comparison/gitea-gap-list.csv`。

## 5. 任务状态总表

### M0：基线与 Git 协议正确性

| ID | 状态 | 任务 | 当前说明 |
|---|---|---|---|
| ALIGN-001 | `DOING` | 建立代码可验证的 Gitea 对齐矩阵 | 正在迁移旧矩阵并统一完成度口径 |
| GIT-001 | `TODO` | Protocol V2 shallow/deepen 或停止能力广告 | 等待 ALIGN-001；已确认能力误报 |
| GIT-002 | `TODO` | Partial clone/filter | 依赖 GIT-001 |
| GIT-003 | `TODO` | Git 真实客户端兼容矩阵 | 依赖 GIT-001 |
| PERF-001 | `TODO` | 大仓库性能基线 | 依赖 GIT-003 |

### M1：仓库与 Issue 高频体验

| ID | 状态 | 任务 | 当前说明 |
|---|---|---|---|
| REPO-101 | `TODO` | Push Mirror | 已有 Pull Mirror；需新增多目标 Push Mirror |
| REPO-102 | `TODO` | upload-archive / `git archive --remote` | 现有 HTTP archive API 不等同于该协议 |
| REPO-103 | `TODO` | Template Repository | 未开始 |
| REPO-104 | `TODO` | Blame、Go to file、目录删除 | 未开始 |
| REPO-105 | `TODO` | 自动生成 Release Notes | 未开始 |
| REPO-106 | `TODO` | 仓库内 OpenAPI 安全渲染 | 未开始 |
| ISSUE-101 | `TODO` | Markdown Issue/PR Template | 未开始 |
| ISSUE-102 | `TODO` | YAML Issue Form | 依赖 ISSUE-101 |
| ISSUE-103 | `TODO` | Issue/PR/评论附件 | 依赖 STORAGE-001 或先冻结本地存储接口 |
| ISSUE-104 | `TODO` | Reaction | 未开始 |
| ISSUE-105 | `TODO` | 多 Assignee | 当前仍为单个 `assignee_id` |
| ISSUE-106 | `TODO` | Lock/Pin | 未开始 |
| ISSUE-107 | `TODO` | Issue 依赖与自动引用 | 未开始 |

### M2：CI 与 Gitea Actions 兼容

| ID | 状态 | 任务 | 当前说明 |
|---|---|---|---|
| CI-200 | `READY` | 第三方 Action 执行与安全边界 ADR | 可开始，但不抢占 ALIGN-001 |
| CI-201 | `TODO` | 仅重跑失败 Job | 依赖 CI-200 |
| CI-202 | `TODO` | Step/Job outputs | 依赖 CI-200 |
| CI-203 | `TODO` | 运行时条件函数 | 当前 `always()`、`failure()` 被显式拒绝 |
| CI-204 | `TODO` | Service Containers | 依赖 CI-200 |
| CI-205 | `TODO` | Reusable Workflow 扩展 | 依赖 CI-202 |
| CI-206 | `TODO` | 第三方 `uses:` Action 执行器 | 依赖 CI-200、CI-204 |
| CI-207 | `TODO` | Workflow 依赖图和失败定位 UI | 依赖 CI-201 |
| CI-208 | `TODO` | Actions 兼容测试集 | 依赖 CI-201～CI-206 |

### M3：Package Registry 与 API 生态

| ID | 状态 | 任务 | 当前说明 |
|---|---|---|---|
| PKG-301 | `TODO` | Package cleanup、保留和配额 | 与 STORAGE-001 协同设计 |
| PKG-302 | `TODO` | Terraform State Registry | 未开始 |
| PKG-303 | `TODO` | Go Package Registry | 未开始 |
| PKG-304 | `TODO` | Alpine/Debian/RPM | 未开始 |
| PKG-305 | `TODO` | Conan/Conda/Pub/Swift/Vagrant | 未开始；长尾协议需按需求排序 |
| API-301 | `TODO` | Gitea OpenAPI 差异和 contract test | 依赖 ALIGN-001 |
| API-302 | `TODO` | 高频兼容端点 | 依赖 API-301 |
| API-303 | `TODO` | 响应、分页、错误码和 Token scope 兼容 | 依赖 API-301 |
| HOOK-301 | `TODO` | 用户/组织/系统 Webhook | 依赖 API-301 |

### M4：身份、权限与管理细节

| ID | 状态 | 任务 | 当前说明 |
|---|---|---|---|
| AUTH-401 | `TODO` | LDAP 周期同步和组织/团队映射 | 基于现有 LDAP 登录实现扩展 |
| AUTH-402 | `TODO` | Reverse Proxy Header Authentication | 未开始 |
| AUTH-403 | `TODO` | PAM/SMTP Authentication Source | 需先确认部署需求 |
| USER-401 | `TODO` | 用户 Block/Unblock | 未开始 |
| USER-402 | `TODO` | Follow/Unfollow | 未开始 |
| USER-403 | `TODO` | User Badge | 未开始 |
| PERM-401 | `TODO` | 仓库 Unit 级权限 | 未开始 |
| ADMIN-401 | `TODO` | 系统配置、任务和队列管理页 | 依赖 QUEUE-001 |

### M5：生产化与可恢复性

| ID | 状态 | 任务 | 当前说明 |
|---|---|---|---|
| STORAGE-001 | `TODO` | 统一 BlobStorage trait | 后续附件、S3 和完整备份的前置任务 |
| STORAGE-002 | `TODO` | S3/MinIO 后端 | 依赖 STORAGE-001 |
| QUEUE-001 | `TODO` | 持久化后台任务抽象 | 未开始 |
| QUEUE-002 | `TODO` | Redis queue、重试和死信 | 依赖 QUEUE-001 |
| OPS-501 | `TODO` | 全实例 backup/restore | 依赖 STORAGE-001 |
| OPS-502 | `TODO` | 升降级与恢复演练 | 依赖 OPS-501 |
| OPS-503 | `TODO` | 三数据库长期压测与故障注入 | 依赖 PERF-001 |
| OPS-504 | `TODO` | 多节点和 HA 支持边界 | 依赖 STORAGE-002、QUEUE-002 |

## 6. 状态更新模板

任务开始时：

```markdown
| YYYY-MM-DD | TASK-ID | READY → DOING | 开始范围；明确不做什么 | 负责人 |
```

任务完成时必须记录：

```markdown
| YYYY-MM-DD | TASK-ID | DOING → DONE | 代码链接；测试命令和结果；文档链接；已知限制 | 负责人 |
```

任务阻塞时：

```markdown
| YYYY-MM-DD | TASK-ID | DOING → BLOCKED | 阻塞原因；已尝试方案；解除条件 | 负责人 |
```

## 7. 变更记录

| 日期 | 任务 | 状态变化 | 说明 | 负责人 |
|---|---|---|---|---|
| 2026-07-13 | ALIGN-001 | `READY → DOING` | 建立进度台账；开始统一状态源、完成度口径和证据矩阵 | 待指定 |
