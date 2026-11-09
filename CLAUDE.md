# IronForge — AI 协作上下文

> **Claude Code 默认入口**，也是所有 AI 编程助手的深度参考文档。
> 提供踩坑记录、依赖版本速查、常见错误排查和实现现状清单。
> 轻量概览见 [AGENT.md](AGENT.md)；完整文档索引见 [ironforge-docs/README.md](ironforge-docs/README.md)。

> **状态**：维护中 ｜ **可信度**：确认 ｜ **来源**：仓库代码 / 文档 / 运行结果 ｜ **最后更新**：2026-11-02

---

## 项目简介

**IronForge**（铁匠铺）是一个用 Rust 从零实现的轻量级 Git 托管平台，对标 Gitea/Forgejo。内存 <50MB、单二进制部署、全功能（仓库/Issue/PR/Wiki/CI/包注册表/企业认证/审计/代码搜索）。Phase 1~21 全部完成。

---

## 关键约定

### 仓库分工与推送顺序

> **来源**：项目负责人约定（2026-11-05）。此约定取代此前以 git233 为开发主仓库的约定。

- **CNB（`cnb`）**：开发主仓库。开发阶段以 CNB 为主要上游，代码的开发、集成和验证优先在 CNB 完成。
- **git233（`origin`）**：自建私人仓库。CNB 完成相应开发工作后，再将需要同步的提交推送到 git233。
- **GitHub（`github`）**：公开发布仓库。准备发布时，在 CNB 完成开发工作后，才推送到 GitHub 并进行 GitHub CI；日常开发不为同步而推送 GitHub。
- **CI 现在跑在 git233 的 Gitea Actions 上**（2026-11-06，项目负责人：CNB 的 Runner 用量也耗尽了，只能用 Gitea 的）。工作流就是 `.github/workflows/regression.yml`（Gitea 兼容读取），触发：对 `main` 的 PR 与 `workflow_dispatch`。**全量只手动触发**（2026-11-09，项目负责人：每次提交都跑全量成本太高；改动足够大时由负责人要求手动跑）：没有 push 到 `main` 的触发，也没有夜间定时；合并后 main 没有自动 CI。**PR 的测试范围**——`changes` job（`scripts/ci-changes.py`，纯标准库，`--self-test` 可本地自检）按与合并基点的差异分类：只改文档 → 什么都不跑；只改 `web/` → 只跑前端；改了 crate → 整个 workspace 的 clippy 加这些 crate 及依赖它们的 crate 的测试（从 Cargo 清单读依赖，不写死）；`Cargo.toml`/`Cargo.lock`、`.github/`、`scripts/`、`deploy/` 或脚本不认识的文件 → 所有 crate 的测试。**`rg-http` 的集成测试（最重的一块）只在 `rg-http` 自己的文件变了时才在 PR 上跑**，其余情况被略过并给出 notice，要跑就手动触发全量。脚本算不出范围时告警并跑全量（失败即关闭）。同一 PR 有新推送时旧 run 被 `concurrency` 取消。CI 里 `CARGO_PROFILE_DEV_DEBUG=line-tables-only`，并尽量用 mold 链接（`scripts/ci-fast-link.sh`，装不上或试链接失败就用默认链接器）。范围外的 job 是 skipped，不是绿勾；**未在 git233 上实跑验证前，不要把这些 job 设为必需检查**。验证一个分支：推到 `origin` 后开 PR（`tea pulls create --repo aicodeing/ironforge …`），或手动 `tea actions workflows dispatch regression.yml --ref <分支> --repo aicodeing/ironforge`；看结果用 `tea api /repos/aicodeing/ironforge/actions/runs`、`…/runs/<id>/jobs`、`…/jobs/<job_id>/logs`。网络类步骤（apt 安装、Rust 工具链下载）经 `scripts/ci-retry.sh`/`ci-apt-install.sh`/`ci-install-rust.sh` 带重试，`dtolnay/rust-toolchain` 失败时用 `ci-install-rust.sh` 再装一次（`continue-on-error` + `steps.rust.outcome != 'success'`；**未在 git233 实跑验证**）。`rg-mcp` 的测试夹具（`collab_mcp`/`read_tools_mcp`/`write_tools_mcp`）的桩服务按连接起线程并带读超时，`rpc` 超过 60 秒无回应会杀掉子进程并带 stderr 失败，不再卡到 job 超时。Runner（fnos/zos）能访问 crates.io、npm、docker.gitea.com，GitHub 只是不稳定，**Docker Hub 直连不通**：PostgreSQL/MySQL 冒烟的服务镜像读仓库变量 `POSTGRES_IMAGE`/`MYSQL_IMAGE`（未设置时拉不到镜像，这两个 job 会失败；设置哪个镜像站由负责人决定，不要自行指向第三方镜像站）；Swagger UI 已 vendored；`cargo audit` 取不到漏洞库时在非 GitHub 环境降为警告（**审计没有运行**）。GitHub 仍只在发布时推送。`.cnb.yml` 保留（CNB 额度恢复后可用；push 只自动跑 `lint`，全量 6 条流水线手动触发 `cnb build start-build … --event api_trigger`），数据库套件仍在本地跑 `scripts/ci-db-tests.sh postgres|mysql`。**未覆盖**：`docker-image.yml`（镜像发布）、`collab-e2e.py` 各场景、浏览器烟测，这些仍需手动。
- 同步前检查目标分支和远端状态，保持提交历史一致，禁止强制推送。各仓库同步结果和 CI 结果分别核验；推送成功不代表 CI 通过或发布完成。
- 这些名称是当前检出的远端配置；执行前仍需核实实际远端和分支。此约定不自动修改 Git 已配置的跟踪上游，拉取或推送时应明确选择目标。

### Agent 生命周期护栏

> ⚠️ **所有 AI Agent 执行变更操作前必须阅读 [`.ai/guardrails.md`](.ai/guardrails.md)**

护栏等级：🔴 BLOCK（禁止） / 🟠 CONFIRM（需确认） / 🟡 WARN（需警告） / 🟢 ALLOW（允许）。

### 证据分层与可信度标注

> 对齐 mybook `Templates` 通用规范：文档中的关键事实应标注来源与可信度，验证声明逐层区分，不得用前一层替代后一层。

**事实来源优先级**（高 → 低）：仓库代码 > 运行结果（命令输出/测试/实库验证）> 项目文档 > 经验与推测（仅作候选，执行成功前不得标为"当前通过"）。

**可信度标注**：关键事实标注 `确认` / `推测` / `待验证`。本文件中"✅ 已完成"类声明应尽量附上验证证据（测试命令、实库结果或 CI 状态）；仅有文档/静态检查通过的，不得声称业务运行已通过。

**证据分层**（逐层陈述，前层不可替代后层）：

| 层级 | 可证明 | 不能替代 |
|------|--------|----------|
| 实现证据 | 文件存在于明确 diff 或 SHA | 命令通过 |
| 任务验证 | 任务分支上的当前命令通过 | 合并后仍通过 |
| 集成验证 | 合并后的基线通过门禁 | 真实运行或发布 |
| 运行验证 | 浏览器/设备/后端/数据库链路已实际执行 | 生产发布 |
| 远端验证 | live remote ref 等于目标 SHA | CI 或部署成功 |
| 发布验证 | 制品/迁移/部署/观察达到发布条件 | 无 |

### HTTP 路由前缀

Git HTTP 端点路由前缀是 `/git/`（**不是**直接 `/<owner>/<repo>`）：

```
GET  /git/<owner>/<repo>/info/refs?service=git-upload-pack
POST /git/<owner>/<repo>/git-upload-pack
POST /git/<owner>/<repo>/git-receive-pack
GET  /health
```

### 命令与测试

编译、启动、测试命令详见 [README.md](README.md) 的「快速开始」和「开发」章节。
Git 协议测试命令模板见 [README.md](README.md) 的「Git 操作」章节。
完整回归测试入口见 [CONTRIBUTING.md](CONTRIBUTING.md) 的「测试规范」章节。

### AI Agent 集成（MCP Server）

MCP Server（`ironforge-mcp`，stdio-only）暴露仓库数据给 AI Agent。完整配置和使用指南见 [`.ai/README.md`](.ai/README.md)。

---

## 实现现状（2026-07-14）

### 功能概览

Phase 1~21 全部完成。核心能力：

| 领域 | 模块 | 说明 |
|------|------|------|
| Git 协议 | `rg-git` | V1/V2、pkt-line、sideband、upload-pack、receive-pack（gix 原生，CLI 网关仅测试用） |
| SSH 服务 | `rg-ssh` | russh 0.64，公钥/密码认证，Deploy Key |
| HTTP 服务 | `rg-http` | Axum 0.8，REST + Git Smart HTTP + OCI + WebSocket + OpenAPI + SPA |
| 业务逻辑 | `rg-core` | auth/user/repo/issue/pr/wiki/lfs/webhook/review/branch_protection/collaborator/org/notification/email/package_registry/mirror/board/time_tracking/import/audit/search/code_indexer |
| 数据库 | `rg-db` | SeaORM 1.1 + SQLite/PostgreSQL/MySQL，自动迁移 |
| CI/CD | `rg-ci` + `rg-runner` + `rg-ci-isolation` | YAML 解析、Pipeline 执行器、Docker Runner、外部 Runner、Secrets/Matrix/Cache/Environment/OIDC/Retention；容器加固策略（镜像校验 + 资源/capability 限制）由 `rg-ci-isolation` 供内置与外部 runner 共用 |
| 前端 | `web/` | SvelteKit 5 SPA，中英双语 i18n（en / zh-CN 各 1306 key，`scripts/i18n-key-check.mjs` 校验一致） |
| MCP | `rg-mcp` | stdio-only，暴露 Tools + Resources 给 AI Agent |

### 详细实现清单

| 模块 | 文件 | 说明 |
|------|------|------|
| pkt-line 协议 | `rg-git/src/pkt_line.rs` | 完整编解码 + V2 Delim/ResponseEnd |
| sideband-64k | `rg-git/src/sideband.rs` | band 1/2/3 |
| git-upload-pack | `rg-git/src/protocol/upload_pack.rs` | SSH + HTTP 模式 |
| git-receive-pack | `rg-git/src/protocol/receive_pack.rs` | SSH + HTTP，返回 Vec\<RefUpdate\> |
| Git Protocol V2 | `rg-git/src/protocol/v2.rs` | ls-refs/fetch/object-info；shallow/deepen/partial-clone |
| V2 HTTP 集成 | `rg-http/src/git_v2.rs` | Git-Protocol: version=2 header 检测 |
| CLI 网关（测试 + mirror） | `rg-git/src/cli_gateway.rs` | git CLI 网关已从协议生产路径退役；现仅测试造数据与 mirror 同步（唯一生产例外）使用；防回归守卫 |
| SSH 服务端 | `rg-ssh/src/lib.rs` | russh 0.64，公钥/密码认证查 DB |
| HTTP 服务端 | `rg-http/src/lib.rs` | Axum 0.8，Git 协议鉴权 + 分支保护 + SvelteKit 静态资源 |
| REST API | `rg-http/src/api/` | 30+ API 模块，142 个 OpenAPI 注解 |
| 用户认证 | `rg-core/src/auth/` | argon2 + JWT HS256 + LDAP + SSO(OIDC) + TOTP MFA |
| 仓库服务 | `rg-core/src/repo/service.rs` | create_repo + can_read/can_write（集成 collaborator/org 权限） |
| Issue 服务 | `rg-core/src/issue/service.rs` | CRUD + labels + milestone + comments + Markdown 模板 |
| PR 服务 | `rg-core/src/pull_request/service.rs` | create + diff + merge(3策略) + 分支保护 + Merge Queue |
| 代码审查 | `rg-core/src/review/service.rs` | submit review + inline comments + 不可变事件流 |
| 分支保护 | `rg-core/src/branch_protection/service.rs` | require PR + require approval + required status checks + require signed commits |
| 协作者 | `rg-core/src/collaborator/service.rs` | read/write/admin 权限 |
| Wiki 服务 | `rg-core/src/wiki/service.rs` | 页面 CRUD + 版本历史 + diff |
| LFS 服务 | `rg-core/src/lfs/service.rs` | batch API + 对象上传/下载 + HMAC URL |
| 统一 BlobStorage | `rg-core/src/blob_storage.rs` | 原子本地 backend；LFS/Package/OCI/Artifact/Release 接入 |
| Webhook 服务 | `rg-core/src/webhook/service.rs` | 13 事件 + HMAC-SHA256 签名 + 投递记录 |
| CI/CD 引擎 | `rg-ci/src/` | YAML 解析 + Pipeline 执行器 + Docker Runner + Gitea Actions adapter |
| 组织系统 | `rg-core/src/org/mod.rs` | CRUD + 成员 + 团队 + 权限 |
| 通知系统 | `rg-core/src/notification/mod.rs` | 创建/列表/已读/批量已读/删除 |
| Rate Limiting | `rg-http/src/rate_limit.rs` | Token Bucket 中间件 |
| WebSocket | `rg-http/src/ws.rs` | 通知按用户隔离，Job 日志按 job_id 隔离 |
| 邮件通知 | `rg-core/src/email/mod.rs` | SMTP（lettre）+ HTML 模板 |
| Package Registry | `rg-core/src/package_registry/` | 10 种适配器 + OCI Distribution Spec v1.0 |
| 审计日志 | `rg-core/src/audit/` | audit! 宏 + 90 天归档（NDJSON.zst） |
| Mirror | `rg-core/src/mirror/` | 仓库镜像同步（git CLI 经网关执行，transport lockdown + net_guard URL 校验；唯一生产 CLI 例外） |
| Board | `rg-core/src/board/` | 看板管理（Board/Column/Card） |
| Time Tracking | `rg-core/src/time_tracking/` | 工时追踪 |
| 数据导入 | `rg-core/src/import/` | GitHub/GitLab 导入 |
| 搜索 | `rg-core/src/search/` | FTS5 全文搜索（repos/issues/wiki/code） |
| 附件 | `rg-core/src/attachment.rs` | 四类归属、统一 BlobStorage、配额、IDOR 防护 |
| 多数据库后端 | `rg-db::connect_with_pool` | SQLite/PostgreSQL/MySQL，backend-aware 迁移 |
| TLS/HTTPS | `rg-http/src/lib.rs` | axum-server + rustls |
| TOML 配置 | `rg-cli/src/main.rs` | CLI > config > defaults |
| API 分页 | `rg-http/src/pagination.rs` | PaginationParams + PaginatedResponse\<T\> |
| 代码 Blame | `rg-git/src/blame.rs` + `rg-http/src/api/repo_content.rs::get_blame` | GET /repos/:owner/:name/blame/*path；gix `blame_file`（含重命名跟踪）；二进制与超 1MiB 拒绝；与 `git blame --porcelain` 对拍测试 |
| GPG 签名 | `rg-http/src/api/repo_content.rs` | GET /repos/:owner/:name/commits/:sha/signature |
| 前端 i18n | `web/src/lib/i18n/` | locale store + localStorage + 中/英翻译 |
| CI Secrets/Matrix | `rg-ci` + `rg-runner` | AES-256-GCM 加密、Matrix 256 上限 |
| CI Environment | `rg-ci` | 受保护环境、审批人/审批数 |
| CI OIDC | `/api/v1/ci/oidc/*` | Ed25519 JWKS、5 分钟 audience-bound token |
| CI Retention | `rg-ci` | 仓库级 Artifact/Cache 保留期 |
| Tag 保护 | `protected_tags` + receive-pack | 通配 Tag 规则 |
| 签名提交强制 | `protected_branches.require_signed_commits` | pack 入库后验证 |
| LDAP/MFA | `rg-core/src/auth/ldap.rs` + `sso.rs` + `totp.rs` | RFC4515 转义、PKCE S256、五次锁定 |
| 同账号多 Agent 协作 | `rg-db` 迁移 `m20261104_000001` + `rg-core/src/collaboration/` + `rg-http/src/api/collab*.rs` + `rg-mcp/src/collab.rs` | 会话/任务/消息/交接/已读 5 表；REST `…/collaboration/*`；15 个 MCP `collab_*` 工具（首版；**当前共 59 个**，以 `rg-mcp/src/collab.rs::TOOL_NAMES` 为准，各行里的数量是当时的快照）。会话是标签不是安全边界；所有客户端字段 `origin: user_content`。证据分层见 [multi-agent-collaboration-verification-2026-11.md](ironforge-docs/architecture/multi-agent-collaboration-verification-2026-11.md) |
| 协作 P1 任务依赖（U01） | 迁移 `m20261104_000003` + `rg-core/src/collaboration/dependency.rs` + `rg-db/src/ops/collab_dependency_ops.rs` + `rg-http/src/api/collab_dependencies.rs` | 每仓库图版本行 `collab_dependency_graphs` + 边 `collab_task_dependencies`（A 等待 B）；`GET/POST …/tasks/{id}/dependencies`、`DELETE …/dependencies/{depends_on_id}`；成环检查与版本递增、插边同事务，有界遍历（1000 节点/5000 边，超限 `GRAPH_LIMIT_EXCEEDED` 失败即关闭）、单任务最多 100 条直接依赖；MCP `collab_{get,add,remove}_dependency`。`blocked_by` 由**声明**状态派生，**不是**证据；依赖被声明 `done` 时，`update_cas` 在同一事务内给每个未完成依赖任务的负责会话写 `dependency_finished` 通知（generation = 被完成任务的 version，重开再完成才有新通知；仅是提示，不是完成证据）。证据见 [agent-collaboration-p1-dependencies-verification-2026-11.md](ironforge-docs/architecture/agent-collaboration-p1-dependencies-verification-2026-11.md) |
| 协作 P0 子集 | 迁移 `m20261104_000002` + `rg-core/src/collaboration/{rules,takeover,question,work_package,inbox,retention,completion,ci_evidence}.rs` + `rg-http/src/api/collab_{rules,takeovers,questions,work_packages,inbox,retention}.rs` | 规则入口 `…/collaboration/rules`、接手/重指派、问题与答复、工作包、**实时**收件箱（无快照/无已读）、删除影响预览、`…/tasks/{id}/completion-status`（只读，声明≠证据）。协作数据**仅**所有者/协作者/组织成员/写者可读（`require_collab_read`）。MCP 已接入 13 个新工具（`collab_get_rules/get_inbox/claim_task/list_takeovers/ask_question/list_questions/get_question/answer_question/cancel_question/create_work_package/list_work_packages/get_work_package/get_completion_status`，共 28 个 `collab_*`）；用户重指派与删除预览**不**暴露给 Agent；Web UI 的首个版本（只读 + 回答/取消问题）见下一行。证据见 [agent-collaboration-p0-verification-2026-11.md](ironforge-docs/architecture/agent-collaboration-p0-verification-2026-11.md) |
| 协作 P1 决定追溯（U02） | 迁移 `m20261104_000004` + `rg-core/src/collaboration/decision.rs` + `rg-http/src/api/collab_decisions.rs` + MCP `collab_{create,list,get,change}_decision` | 不可变决定 + 唯一终结事件（`superseded`/`revoked`，DB 唯一键仲裁）；可引用本任务的问题/答复；`superseded` 的替代决定须 `seq` 更大（天然无环）；新鲜度复用问题的 `freshness`，stale 不自动撤销；决定不是审批。证据见 [agent-collaboration-decisions-verification-2026-11.md](ironforge-docs/architecture/agent-collaboration-decisions-verification-2026-11.md) |
| 跨仓协调组（P2 X02） | 迁移 `m20261104_000006` + `rg-core/src/collaboration/group.rs` + `rg-db/src/ops/collab_group_ops.rs` + `rg-http/src/api/collab_groups.rs` + `rg-mcp/src/collab.rs` | 组归锚仓；`…/collaboration/groups`、`…/groups/{id}`、`…/groups/{id}/links`（GET/POST）、`DELETE …/links/{link_id}`；5 个 MCP `collab_*group*` 工具。关联按仓库 **id**，创建组需锚仓写，增删关联需锚仓**和**目标仓写；读取先鉴权再分页（keyset + 每页逐项重新鉴权，无快照表，隐藏项静默过滤、无计数）；版本 CAS、软移除 + 审计。仅 SQLite 实测，PG/MySQL 未验证。证据见 [agent-collaboration-groups-verification-2026-11.md](ironforge-docs/architecture/agent-collaboration-groups-verification-2026-11.md) |
| 协作健康指标（P2 X03） | 迁移 `m20261104_000007` + `rg-core/src/collaboration/health.rs` + `rg-db/src/ops/collab_metric_ops.rs` + `rg-http/src/api/collab_health.rs` | 平台事件表 `collab_metric_events`（服务端时间，业务事务内写入）；仅仓库管理员：`…/collaboration/health`、`…/health/tasks`（钻取）、`POST …/health/baseline`（一次性观察基线）。覆盖起点之前不算 0（partial/unknown）；任务状态标自报；无任何会话/Agent 排名。证据见 [agent-collaboration-health-verification-2026-11.md](ironforge-docs/architecture/agent-collaboration-health-verification-2026-11.md) |
| 协作 P1 路径预警（U03） | 迁移 `m20261105_000001` + `rg-core/src/collaboration/overlap.rs` + `rg-http/src/api/collab_overlaps.rs` + MCP `collab_get_overlaps` | `collab_tasks.paths_declared_at`（可空，仅在创建带路径或更新携带 `paths` 时写，旧行为 NULL 且不回填）；`GET …/tasks/{id}/overlaps`：同仓未完成、声明过路径的其他任务，相等或在 `/` 边界上互为目录前缀才算重叠，不解释 glob、不碰 Git/文件系统、不加锁；`confidence` 仅 `declared_current`（24 小时内）/`declaration_stale`（更旧或时间未知）；按 task id 升序 keyset 分页，单次最多扫描 2000 行，页可能偏短但仍带 `next_cursor`。只是提示，没有声明≠没有冲突。证据见 [agent-collaboration-overlaps-verification-2026-11.md](ironforge-docs/architecture/agent-collaboration-overlaps-verification-2026-11.md) |
| 协作 O02 总览 | `rg-core/src/collaboration/overview.rs` + `rg-http/src/api/collab_overview.rs` | `GET …/collaboration/overview?sections=&limit=`（仓库读权限，人工入口，不进 MCP）：`awaiting_me`/`blocked_waiting`/`pending_handoffs`/`in_progress` 四个独立实时 section，每个带 `as_of`/`truncated`/`continue_with`，无跨 section 快照、无 `next_cursor`。`flags`（`unowned`/`session_not_online`）只标声明不可靠处；读者的 `awaiting_me` 是 `available:false`。证据见 [agent-collaboration-overview-verification-2026-11.md](ironforge-docs/architecture/agent-collaboration-overview-verification-2026-11.md) |
| 协作 U05 上下文包 | `rg-core/src/collaboration/context.rs` + `rg-http/src/api/collab_context.rs` + MCP `collab_get_context` | `GET …/tasks/{id}/context?max_bytes=&sections=`（读权限）：`task` 始终第一，其后 `questions`/`decisions`/`handoffs`/`work_packages`/`dependencies`/`overlaps`/`messages` 按固定优先级。`max_bytes`（默认 32 KiB，4–256 KiB）限制整个序列化响应；超限从低优先级末尾丢条目并写入 `truncation`，长字段在字符边界截断并列出，放不下任务本身返回 400 `BUDGET_TOO_SMALL`。各 section 是独立实时查询（`consistency: per_section_live_query`，无快照、无 `next_cursor`，续读走 `continue_with`）；`head_sha` 是对分支的独立观测，读不到为 `unavailable`；记录原文返回、不摘要。证据见 [agent-collaboration-context-verification-2026-11.md](ironforge-docs/architecture/agent-collaboration-context-verification-2026-11.md) |
| 协作收件箱已读标记 | 迁移 `m20261105_000002` + `rg-core/src/collaboration/inbox.rs` + `rg-db/src/ops/collab_inbox_read_ops.rs` + `rg-http/src/api/collab_inbox.rs` + MCP `collab_mark_inbox_read` | `collab_inbox_reads(repo_id, actor_kind, actor_id, item_key, observed_version, read_at)` 复合主键；`PUT …/inbox/read`（会话）与 `PUT …/inbox/user/read`（用户门，需写权限）接 `{item_key, observed_version}`。收件箱每项新增 `object_version`/`read_state`/`last_read_version`/`last_read_at`。只有**当前在该 actor 收件箱里**的项能标记（否则统一 404）；`observed_version` 超过记录当前版本返回 400；标记只增不减、幂等，并发收敛到最大值；记录出现更新版本即重新未读。只表达阅读，不改任何问题/交接/任务状态。证据见 [agent-collaboration-inbox-reads-verification-2026-11.md](ironforge-docs/architecture/agent-collaboration-inbox-reads-verification-2026-11.md) |
| 协作 O04 自定义完成条件 | 迁移 `m20261105_000003` + `rg-core/src/collaboration/conditions.rs` + `rg-db/src/ops/collab_completion_ops.rs` + `rg-http/src/api/collab_conditions.rs` + MCP `collab_{get,set}_completion_conditions` | 仅展示，**不**阻止/改变任务。`…/tasks/{id}/completion-conditions` GET/PUT（`client_request_id`+`expected_version`，每次写入下一个**不可变版本**，唯一键 `(task_id,version)` 仲裁并发，败者 409 `VERSION_CONFLICT`）、`…/versions` 历史、`…/{condition_id}/confirmations`（用户门，`manual_confirmation`，绑定当时分支头，分支移动即 stale，不是合并审批）、`…/completion-status/snapshots`（POST 把当前评估原样存档，GET 取回不重算）。四种类型：`question_answered`/`work_package_present`/`ci_passed_for_commit`/`manual_confirmation`；`completion-status` 新增 `defined`，有定义时由它决定 `declared_done_without_evidence`，原 `conditions`（推导项）不变。未做强制模式（需单独评审）。证据见 [agent-collaboration-completion-conditions-verification-2026-11.md](ironforge-docs/architecture/agent-collaboration-completion-conditions-verification-2026-11.md) |
| 协作 P1 双客户端闭环（U06/N14） | `scripts/collab-e2e.py --scenario p1` | 真实服务 + 真实 `ironforge-mcp`（A/C）+ 纯 HTTP 客户端（B）+ 真实 Git 仓库，覆盖路径预警、依赖、消息送达、依赖完成通知、决定、收件箱已读、完成条件与快照、崩溃后新客户端用上下文包恢复；仅 SQLite、单机、非真实模型、手工入口（不在 CI）。证据与未覆盖项见 [agent-collaboration-p1-e2e-verification-2026-11.md](ironforge-docs/architecture/agent-collaboration-p1-e2e-verification-2026-11.md) |
| 协作 O03b 备份/恢复演练 | `scripts/collab-e2e.py --scenario restore` + `rg-cli` 的 `backup`/`restore`（`checksums.sha256` 与恢复前校验） | 真实全量备份 → 隔离目录恢复 → 第二个实例逐视图对比协作数据（27 个视图）、真实 Git 对象可读且 `git fsck` 干净。`restore` 现在在**第一次写入之前**校验：清单与格式版本、逐文件 SHA-256（且不得有清单外文件）、数据库 `PRAGMA integrity_check`、数据库记录的迁移须全部被当前二进制认识；失败时现有实例不被改动。无 `checksums.sha256` 的旧备份仍可恢复但会警告。恢复后默认**收敛**（`restore`/`restore-db`，`--keep-live-state` 跳过）：结束全部协作会话、删密码重置令牌、把排队的 `webhook.deliver`/`email.send` 作业标 `dead`、取消运行中的 CI；PostgreSQL/MySQL 手工执行 `deploy/post-restore-quiesce.sql`；个人访问令牌原样恢复并报告数量，需运维复核。CLI 备份仅 SQLite；PostgreSQL/MySQL 用原生 dump 工具，`scripts/db-restore-drill.sh postgres|mysql` 演练转储/恢复往返（逐表行数与内容一致、迁移无待执行；不含 Git/Blob、不含恢复后收敛）。证据与未覆盖项见 [agent-collaboration-restore-drill-2026-11.md](ironforge-docs/architecture/agent-collaboration-restore-drill-2026-11.md) |
| 协作契约 X01 | `rg-core/src/collaboration/{capabilities,compat}.rs` + `rg-http/src/api/collab_{capabilities,json}.rs` + `rg-mcp/src/collab_contract.rs` + `rg-http/tests/collab_contract_tests.rs` | `GET /api/v1/collaboration/capabilities` 与仓库级 `…/collaboration/capabilities`（静态注册表，只广告实际存在的能力）；契约 `major.minor`；协作写请求 `deny_unknown_fields`（400 `UNKNOWN_FIELD`，非 422）；安全关键枚举未知值 fail closed（`UNKNOWN_ENUM_VALUE`）；路由/OpenAPI/注册表/MCP 工具表防漂移测试。新增协作路由或 MCP 工具必须同步登记注册表。证据见 [agent-collaboration-contract-verification-2026-11.md](ironforge-docs/architecture/agent-collaboration-contract-verification-2026-11.md) |
| 协作运维 O06–O08 | 迁移 `m20261104_000008` + `rg-core/src/collaboration/{template,declaration,budget}.rs` + `rg-http/src/api/collab_{templates,declarations,budgets}.rs` | 任务模板（管理员维护、不可变版本、任务固定版本+快照）、会话能力声明（自报、过期标记、`authorization: none`）、任务预算与用量（unit/source 必填、不跨单位求和、缺失=unknown、仅信息性不暂停）。MCP 新增 9 个工具。证据见 [agent-collaboration-ops-verification-2026-11.md](ironforge-docs/architecture/agent-collaboration-ops-verification-2026-11.md) |
| Agent 令牌 scope 阶梯 | `rg-core/src/auth/pat_scope.rs` + `rg-http/src/lib.rs`（`required_pat_scope`/`repo_scope_for`/`pat_actor`/`extract_actor_id`）+ `rg-http/tests/pat_scope_ladder_tests.rs` | `repo:read` < `repo:collab` < `repo:write` < `repo`（历史全量，包含各级）。PAT 中间件按**方法+路径**判定：读→read，`/repos/{o}/{n}/collaboration/**` 写→collab，其余写→write；`git fetch` 需 read，`git push` 需 write；只读语义的 POST 按写处理（fail closed）。无迁移。**不能**限制仓库或分支，PAT 仍被换成完整 JWT，限制只在中间件/git 入口层。证据见 [agent-token-scopes-verification-2026-11.md](ironforge-docs/architecture/agent-token-scopes-verification-2026-11.md)。**仓库白名单**（迁移 `m20261106_000001`，`access_tokens.repo_allowlist`）：创建令牌时 `repos: ["owner/name"]`，只能带仓库类 scope；REST 只放行名单内的 `/repos/{o}/{n}/**` 与 `/ai/repos/{o}/{n}/**`，仓库族其余路径和名单内通往其他仓库的路由（`fork`/`forks`/`transfer`/`collaboration/groups`）拒绝；git over HTTP 按 `owner/repo` 判定；按 job id 的 WebSocket 拒绝；损坏的存储值按"空名单"（全拒）处理。**新增带请求体跨仓引用的路由必须同步加入 `rg_core::auth::pat_scope::path_in_allowlist` 的拒绝表。**仓库白名单本身不限制分支。证据见 [agent-token-repo-allowlist-verification-2026-11.md](ironforge-docs/architecture/agent-token-repo-allowlist-verification-2026-11.md)。**分支白名单**（迁移 `m20261106_000002`，`access_tokens.ref_allowlist`）：创建令牌时 `refs: ["agent/*"]`（必须同时带 `repos`，`*` 与分支保护同一匹配函数，必须以字面字符开头）；`git push` 只能更新名单内的 `refs/heads/<模式>`，tag 与其他命名空间一律拒绝（`receive-pack` 的 `writable_refs` 参数；SSH 不用令牌所以传 `None`）；REST 写请求在 PAT 中间件里收窄成白名单（`ref_limited_write_allowed`：`collaboration/**`、`issues/**`、创建/编辑/评论/评审 PR），文件编辑、合并、应用建议、发布、wiki、CI 触发、commit status 等一律 403，**新增的 REST 写路由对这类令牌默认被拒**。读不受限。证据见 [agent-token-ref-allowlist-verification-2026-11.md](ironforge-docs/architecture/agent-token-ref-allowlist-verification-2026-11.md)。**令牌身份进会话**（无迁移）：PAT 中间件换出的会话 JWT 带可选 `tid`（令牌 id，普通登录没有）；`api::auth::session_access_token` 按 `tid` 重取令牌，已删除/过期/不属于该用户 → 403（删除令牌即终止它换出的会话）。`contents` 写路由因此对受限令牌放行，由 `ensure_token_may_write_branch` 在处理函数里按分支检查——**一条 REST 写路由只有在其处理函数自己检查分支后，才能加进 `ref_limited_write_allowed`**。受限令牌不能批准 PR（`ensure_session_may_approve`，评论和 `request_changes` 可以）。审计：中间件先删客户端带来的 `x-ironforge-access-token-id` 再按真实令牌写入，`api::audit::with_access_token` 把 `access_token_id` 合进已有 5 处审计的 `details`（只覆盖已有审计点）。`git_v2.rs` 的 v2 receive-pack 死代码已删除，不要再加第二个推送入口。**写语义**：错误体新增可选 `reason`/`retryable`/`hint`（`AppError::conflict_because`）；`contents` 写与 PR 合并支持 `dry_run`（体或 `X-Dry-Run: 1`）和 `expected_sha`/`expected_head_sha`（409 `REF_ADVANCED`，`details.current_sha`）；服务层 `create_or_update_file_with`/`delete_file_with`。MCP 新增 `commit_file`/`delete_file`/`create_pr`/`list_pipelines`/`wait_pipeline`，`read_file` 有 2000 行/128 KiB 窗口。**幂等键**（迁移 `m20261106_000003`，表 `idempotency_keys`）：`idempotency::idempotency_middleware` 装在 PAT 中间件里面，`(user, token, key)` 唯一，同 key 同请求重放首个响应、不同请求 422 `IDEMPOTENCY_KEY_REUSED`、仍在运行 409 `IDEMPOTENCY_IN_PROGRESS`、5xx 释放 key、24 小时过期；**只覆盖响应里没有秘密的路由**（`idempotency_supported`：`POST /repos` 与仓库下的 issues/pulls/contents/releases/labels/milestones/branches/wiki），对其他路由带头被 400 拒绝——新增路由进这张表前先确认响应里没有令牌或密钥。**事件日志**（迁移 `m20261106_000004`，表 `repo_events`）：`webhook::service::trigger_event` 开头把每个事件写进去（尽力而为，>16 KiB 存占位），`GET /repos/{o}/{n}/events?since=&types=&limit=&timeout=`（读权限，长轮询 ≤ 60 秒，保留 7 天，`payload_origin: user_content`）；MCP `wait_repo_events`。**PR 聚合上下文**：`GET /ai/repos/{o}/{n}/pulls/{n}/context?budget=`（`rg-http/src/api/ai_pr_context.rs`；近似 token 预算，优先级 CI 失败 > 未解决评论 > 变更文件 > 描述与评审 > 已解决评论，`truncated` 点名被裁的段，`counts` 永远是完整数字；用户文字带 `origin: user_content`；不计算 mergeable 与分支保护）；MCP `get_pr_context`。**后续补完（2026-11-07，证据见 [agent-gaps-verification-2026-11.md](ironforge-docs/architecture/agent-gaps-verification-2026-11.md)）**：同仓 PR 的 `refs/pull/<n>/head`（创建、推送、REST 文件提交时更新，分支删除后仍在；fork PR 没有）；`GET …/pulls/{n}/export` + MCP `export_pr`；创建 PR 的 `dry_run`；**REST 文件提交（`contents` 写）现在与 git push 一样跑 `spawn_post_push_hooks`**（PR head 同步、CI、webhook、事件日志），此前不触发，agent 提交后 `wait_pipeline` 永远等不到流水线；文件提交、PR 创建/合并/评审写审计（`audit::record_write`，带 `access_token_id`）；往不存在的分支提交时文件检查看默认分支。**MCP 远程传输（2026-11-09，streamable HTTP）**：`ironforge-mcp --http [ADDR]`（`rg-mcp/src/remote.rs`，分发共用 `rg-mcp/src/server.rs`，测试 `rg-http/tests/mcp_remote_tests.rs`）；独立进程，**不持有凭据、不做权限判断**，每次工具调用带调用者自己的 `Authorization: Bearer` 转发 REST，所以 scope 阶梯、仓库/分支白名单、幂等键、审计都由服务端既有代码决定；`initialize` 用 `GET /users/me` 探测令牌（401 拒绝，200/403 放行），会话绑定令牌 SHA-256（别的令牌得到与未知 id 相同的 404），每会话独立内存 `SessionStore`（**不读** `IRONFORGE_COLLAB_SESSION_FILE`）；`tools/call` 在客户端接受 SSE 时以 SSE 返回并每 15 秒发心跳注释；会话/并发/请求体上限与 `Origin` 默认拒绝；单实例，TLS 与按 IP 限流在反向代理，代理须关缓冲、读超时 ≥ 75 秒。工具表未变，`capabilities` 注册表的 `mcp_transport.transports` 仍是 `["stdio"]`（服务端看不到独立 MCP 进程）。**未做**：旧版 HTTP+SSE（`--sse` 仍报错）、服务器主动消息、真实 MCP 客户端与真实反向代理验证。证据见 [mcp-remote-verification-2026-11.md](ironforge-docs/architecture/mcp-remote-verification-2026-11.md)。**仍未做**：SSE（事件流）、fork PR 的 `refs/pull`。删分支有 REST `DELETE /repos/{o}/{n}/branches/{branch}`，`git push --delete` 也支持：receive-pack 广告 `delete-refs`、解析 `:<ref>` 简写，逐条走同一个 `delete_branch` 守卫，拒绝原因写进 report-status；标签与其他命名空间的删除仍拒。`scripts/agent-e2e.py` 是真实服务 + 真实 `ironforge-mcp` + 主机执行 CI 的手工脚本（需先 `cargo build -p rg-cli -p rg-mcp`）。证据见 [agent-write-path-verification-2026-11.md](ironforge-docs/architecture/agent-write-path-verification-2026-11.md) |
| 协作人类通知（问题、任务阻塞） | `rg-core/src/collaboration/human_notify.rs` + `rg-http/src/api/collab_questions.rs` + `rg-http/src/api/notifications.rs` + `web/src/lib/stores/unread.svelte.ts` | 创建问题（`repo_writers`/`user`）后给有权回答的人写站内通知（现有铃铛/WebSocket），发送时逐人重判当前权限，最多 50 人，幂等重放不重发，创建提交后尽力而为；`GET /notifications` 新增 `repo`，`collab_*` 通知在失去访问后隐藏文本；导航栏未读徽标。另：任务被声明为 `blocked`（真正的状态转移）时通知**负责会话所属账号**一人，未读不重复。另有两个 webhook 事件 `collab.question.asked`/`collab.task.blocked`（`webhook_events.rs`；载荷只含 id/标题/链接，客户端文字在 `user_content`，无问题正文）。只有这两种事件，**无邮件**（无用户偏好、无邮箱验证），仅 SQLite。证据见 [agent-collaboration-human-notify-verification-2026-11.md](ironforge-docs/architecture/agent-collaboration-human-notify-verification-2026-11.md) |
| 协作 Web UI（首版） | `web/src/routes/[owner]/[repo]/collaboration/` + `web/src/lib/api/collaboration.ts` + `web/src/lib/components/collaboration/` | 仓库「协作」标签：任务列表/详情、等待你处理（就地回答/取消问题）、概览、完成状态/依赖/重叠/决定/交接/消息/诊断（只读）。用登录身份，不发会话头；客户端字段一律当文本渲染；声明标为声明。通知与任务写入/消息/交接**无**用户侧端点，未做。2026-11-06 增补「已定义的完成条件」卡片（`ConditionsCard.svelte`）：显示条件与判定、对 `manual_confirmation` 做用户确认（绑定分支头）、记录并查看快照；仍仅展示，不阻止任何事。服务端拒绝对 `stale` 问题作答，UI 不给表单。仅 SQLite + 内置浏览器手工验证。证据见 [agent-collaboration-web-ui-verification-2026-11.md](ironforge-docs/architecture/agent-collaboration-web-ui-verification-2026-11.md)。2026-11-06 并行增补四组（各有独立证据文档，均仅 SQLite + 内置浏览器，无截图，未看手机宽度）：决定创建/变更、用户提问、工作包只读（[decisions](ironforge-docs/architecture/agent-collaboration-web-ui-decisions-verification-2026-11.md)）；依赖编辑、用户重指派、接手记录、预算与用量（[relations](ironforge-docs/architecture/agent-collaboration-web-ui-relations-verification-2026-11.md)）；协作组列表/详情与健康指标页（[groups](ironforge-docs/architecture/agent-collaboration-web-ui-groups-verification-2026-11.md)）；模板、搜索、规则、设置页（[ops](ironforge-docs/architecture/agent-collaboration-web-ui-ops-verification-2026-11.md)）。`ApiError` 现保留服务端 `error.details`（不可信数据）。**未做**：从模板创建任务（后端要求会话凭据，无用户入口）、通知的用户侧视图（产品决定）。健康基线的「已建立」现在由迁移 `m20261109_000001` 的 `collab_health_baselines`（每仓库一行）记录，新增 0 条时 `taken_at` 也不再为空（`baseline_taken_at` 取该行与旧的基线事件中较早者）；覆盖起点仍是第一条事件，基线不为指标发明起点，空仓库建基线后指标仍是 unknown，直到出现第一个事件 |

### 技术债与后续方向

**gix 迁移**：生产路径 git CLI 调用仅剩 1 处边缘点（mirror 的 clone/同步，见下表），核心热路径 100% gix 原生。Phase A+B+C-lite+Rebase+全量覆盖完成：rev-parse/update-ref/show/cat-file/verify-commit/auto_init/merge-tree/commit-tree/ls-tree/archive/rebase/pack 生成/ thin-pack 摄取/rev-list 图遍历/文件编辑直写（edit_tree+commit_as+引用乐观锁）/fork 对象搬运/fork 裸克隆/import 裸克隆 均已 gix 化；rebase 与 fork fetch 为自研实现（characterization 测试钉死语义，gix 出原生 API 时可替换）。`GitCommandGateway` 仅测试造数据与上述 1 处边缘点使用。等待 gix 上游成熟项（迁移评估见 `docs/gix-migration-assessment.md`，基于 gix 0.90.0）：

| 待办 | 阻塞原因 | 解除条件 |
|---|---|---|
| Rebase 合并 | gix-rebase 无 API | ✅ 已自研关闭：`rg_git::ops::rebase_merge`（merge_trees 显式祖先三方合并 + 拓扑重放，characterization 测试钉死语义，gix 出原生 API 时可替换） |
| Pack 生成 | ~~gix 无高层 pack 协商~~ | ✅ 已自研关闭：gix-pack `generate` 管线（count::objects(TreeContents) + iter_from_counts + FromEntriesIter），wants/haves/shallow 语义对齐 |
| Thin-pack 索引 | ~~gix 缺 thin 补全解析~~ | ✅ 已解除：`Bundle::write_to_directory`（thin base lookup = ODB，等价 --fix-thin） |
| ~~GPG 验签~~ | ✅ 已解除：gix 0.87 `Commit::verify()`/`Commit::sign()` 内建 OpenPGP/X.509/SSH 签名与验签（Phase B3 已接入 receive_pack，`command` feature） | — |
| ~~blob-diff patch~~ | ✅ 已解除（2026-09-25）：`rg_core::pull_request::gix_patch::unified_patch`，与 `git diff --find-renames` 字节级对拍通过（fixture 覆盖多 hunk/funcname/EOF 裁剪/无尾换行 marker/尾随空行增删/纯删除+纯新增混合/rename 100%+92%/mode change/binary/删除等） | 对拍测试钉死：`gix_patch::parity_tests` |
| Mirror 同步 gix 化 | 实测阻塞（2026-10-23）：`prepare_clone_bare` + `configure_remote(|r| r.with_refspecs(["+refs/*:refs/*"]))` 能建出正确 bare 仓库与 `remote.origin.*` 配置，但 **gix 0.90 的 clone fetch 不按 `refs/*` 通配写 ref**（`refs/heads/*`、`refs/pull/*` 均生效，唯独根级 `refs/*` 只留下 tags）；且 gix 无 `fetch --prune`。枚举命名空间会丢失 `refs/pull/<n>/<x>` 之外的未知命名空间，等于语义回退 | 保留 git CLI + transport lockdown（`protocol.ext/file/ssh.allow=never`）+ `net_guard::validate_mirror_url`；待 gix 支持根级 refspec 通配与 prune 后重评估 |
| ~~Import 裸克隆 gix 化~~ | ✅ 已解除（2026-10-23）：gix 0.90 `prepare_clone_bare()` + `fetch_only()`（`blocking-network-client` 隐含 credentials，私有源仍走 git credential helper） | `import/service.rs` 已替换 `git clone --bare`；`clone_repo_builds_a_bare_repository_git_can_read` 用 git 复核 bare 布局、`refs/heads/main`、tag 与重复调用幂等 |
| ~~CI worktree gix 化~~ | ✅ 已解除（2026-10-23）：gix 0.90 提供 `Repository::{prepare_add_worktree, add_worktree, remove_worktree}` + `worktrees()`（需 `worktree-mutation` feature） | `rg-ci/runner.rs` 改用 `Head::Detached(<commit>)` 建链、`Force::DiscardChanges` 清理（等价 `--force`，仍尊重 operator lock）；bare 仓库 + 脏工作区 + 注册清理由 `workspace_is_a_linked_worktree_of_the_bare_repository` 钉死 |

复查节奏：每次 gix 版本升级时过一遍（最近一次：2026-10-23 升级 0.87.1 → 0.90.0，同时 russh 0.51 → 0.64；本轮修复 `gix::features` 模块移除、`merge_base` 返回 Option、`count::objects` 错误类型收窄、`iter_from_counts` 变 fallible、tree-diff 回调返回 `gix::Result<Action>`、per-op 错误枚举并入分类式 `gix::Error`；russh 侧 `channel_open_session` 改为两阶段必须 accept；同时借 0.90 的 linked-worktree API 关掉 CI worktree 的 CLI 兜底，并补齐 patch 对拍的空行/EOF fixture）。

**多实例部署的进程内状态**（登记，未完全关闭）：权限缓存（`rg-core/src/repo/service.rs`，30s TTL，可设
`IRONFORGE_DISABLE_PERM_CACHE=1` 完全绕过）、限流令牌桶（`rg-http/src/rate_limit.rs`，各实例独立计数）、
实例设置（`rg-http/src/instance.rs`，maintenance/banner 不跨实例传播）。解除条件：共享缓存（Redis）或
pub/sub 失效总线；在此之前建议单实例部署，详见 `deploy/README.md`。

**CI 容器隔离的剩余面**：工作区 rw 挂载 + 容器内 root 仍可在宿主留下 root 属主文件；需靠 daemon 的
userns-remap 或 `rg-ci-isolation::DockerLimits::user` 收紧。镜像/资源/capability 已加固。

**安全加固台账（2026-10-23，对应 GitHub issue #1）**：

| 面 | 修复 |
|----|------|
| PAT 生成 | `rg-http/src/api/users.rs` 改用 `OsRng` 取 256-bit，去掉时间戳与 PID 回退；熵源不可用则 panic（fail-closed），不再生成可猜凭据 |
| SSO state cookie | 裸 `SHA256(secret‖":"‖value)` → HMAC-SHA256 + `Mac::verify_slice` 常量时间校验，关闭长度扩展与时序侧信道 |
| SSO/OIDC 出站 | 7 处 `reqwest::Client::new()` 收敛为共享 client：30s 请求 / 10s 连接超时、禁跟随重定向。**刻意不**拒绝内网 IP——自托管 IdP（Keycloak/GitLab）常位于内网，信任边界由部署方决定 |
| 限流 | `RateLimiter` 增加 `max_keys`（默认 10 万，满时先清过期条目再拒绝新 key，防 map 无界增长）；`/users/register`（60/60s）与 `/users/login`+`/users/forgot-password`+`/users/reset-password`（10/60s）挂常开独立限流；测试路由不挂（集成测试全部来自 127.0.0.1） |
| Git 流式超时 | HTTP upload/receive-pack 与 SSH 三个 stream handler 加 wall-clock deadline（默认 3600s，`IRONFORGE_GIT_STREAM_TIMEOUT_SECS` 可调，0 = 关闭）；HTTP 超时返回 504，子进程随 future drop 被回收 |
| rg-cli 内联 runner | Docker 不可用改为 fail-closed（不再静默落宿主 shell）；`docker run` 复用 `rg-ci-isolation::DockerLimits::hardening_args()` 并以 `--` 结束选项解析；无 image 的 job 需 `IRONFORGE_ALLOW_HOST_RUNNER=1` 才允许在宿主执行 |
| release 资产完整性 | 新增 `release_assets.sha256`（迁移 `m20261023_000001`，`up` 幂等以兼容 SQLite 回滚），上传计算、下载返回 `x-checksum-sha256` |

webhook / mirror 的 SSRF 已由 `rg-core/src/net_guard.rs`（`validate_webhook_url` / `validate_mirror_url`）覆盖；CI 已有 `cargo audit` job（`regression.yml` → `security-audit`）。

详细架构事实见 [ironforge-docs/architecture/project-architecture-2026-07.md](ironforge-docs/architecture/project-architecture-2026-07.md)。

---

## 重要踩坑（必读！）

在修改 Git 协议相关代码时，请务必了解以下已踩过的坑：

### 1. pkt-line 解析必须用 `read_pkt_line`，不能用 `read_line`

pkt-line 格式是 `<4 hex 字节长度><payload>`。长度包含自身 4 字节。
`read_line()` 会把 `004a...` 这样的长度头当成文本内容读进来，导致 UTF-8 解析失败或逻辑错误。
**正确方式**：始终使用 `rg_git::pkt_line::read_pkt_line(&mut BufReader::new(stream))`。

### 2. receive-pack 的 report-status 必须整体 sideband 封装

当服务端广告了 `side-band-64k` 能力（我们始终广告），客户端期望所有响应都通过 sideband 发送。

**错误做法**：先发 sideband flush `0000`，再发 plain pkt-lines。
**正确做法**（已验证）：

```
① 把 report-status pkt-lines 写入内存 buf（unpack ok + ok/ng ref... + 0000）
② 整体用 sideband::write_sideband_data(writer, &report_buf) 发出（band 1）
③ 调用 sideband::write_sideband_flush(writer) 发 sideband flush
```

对应代码：`rg-git/src/protocol/receive_pack.rs` 中的 `send_response()` 函数。

### 3. russh ChannelStream 的关闭顺序

SSH 会话结束时必须按以下顺序操作，否则会丢失缓冲数据：

```rust
// ① 先发 exit-status（channel 还活着）
handle.exit_status_request(channel_id, exit_code).await?;
// ② 再 shutdown stream（发 SSH EOF，让客户端知道数据发完了）
stream.shutdown().await?;
// ③ stream drop → channel close
```

对应代码：`rg-ssh/src/lib.rs` 中 `exec_request` 的 `tokio::spawn` 块。

### 4. git push 发送的是 thin pack

客户端 `git push --thin` 发送 thin pack，服务端必须用：

```bash
git index-pack --fix-thin --stdin
```

不能用普通的 `git index-pack --stdin`，否则 pack 文件不完整。

### 5. git for-each-ref 不列出 HEAD

`git for-each-ref` 只列出 refs/heads/...、refs/tags/... 等，不包括 HEAD（符号引用）。
需要额外调用 `git rev-parse HEAD` 单独解析，且要校验返回值是 40 位 hex（空 repo 返回字面 "HEAD"）。

### 6. HTTP info/refs 路由的 Content-Type

git HTTP 协议对 Content-Type 极为敏感：

- `GET /info/refs?service=git-upload-pack` → `application/x-git-upload-pack-advertisement`
- `GET /info/refs?service=git-receive-pack` → `application/x-git-receive-pack-advertisement`
- `POST /git-upload-pack` → `application/x-git-upload-pack-result`
- `POST /git-receive-pack` → `application/x-git-receive-pack-result`

### 7. argon2 0.5 的 SaltString 用法

```rust
// 正确：
use password_hash::rand_core::OsRng;
let salt = SaltString::generate(&mut OsRng);

// 错误（rand 0.9 的 rng() 不满足 CryptoRngCore）：
use rand::rng;
let salt = SaltString::generate(&mut rng()); // ❌
```

### 8. axum 0.8 的 Router::nest() 类型约束

`Router::nest()` 要求前后 Router 的 State 类型一致。
推荐做法：把所有 route handler 先组成一个完整 Router，再统一加 `.with_state(state)`。

### 9. axum TLS 必须用 axum-server

- ❌ `tokio-rustls::TlsAcceptor` + `axum::serve(TcpStream)`：`TlsStream` 无法转 `TcpStream`
- ❌ `hyper` 直接处理：`Router` 不实现 `Service<Request<Incoming>>`
- ✅ `axum-server::bind_rustls()` + `RustlsConfig::from_config()`

### 10. serde default 函数类型匹配

`#[serde(default = "fn_name")]` 的函数返回类型必须与字段完全匹配。`Option<String>` 字段不能用返回 `String` 的函数，改用 `#[serde(default)]`（Option 自动 None）。

### 11. utoipa OpenAPI 注解注意事项

- `serde_json::Value` **不能**放在 `schemas()` 列表（不实现 ToSchema）；在 path 注解中用 `request_body(content = serde_json::Value)` 代替
- 通过 `route_layer()` 注册的路由不会被 `.route()` 正则匹配发现，`__path_*` 符号缺失需手动排除
- 添加 `use utoipa::ToSchema;` 时**不能**插入到 `use axum::{` 块内（导致 proc-macro 解析失败）
- handler 名冲突（如 `register` 同时在 users 和 runners 模块）需用 `module::handler` 做 key

### 12. SQLite FTS5 触发器的 'delete' 命令

FTS5 的 `INSERT INTO fts_table(fts_table, rowid, ...) VALUES('delete', ...)` 特殊命令**不接受内容列值**，会导致 `SQL logic error`。
**正确方式**：用标准 SQL `DELETE FROM fts_table WHERE rowid = old.id` 代替。

### 13. 迁移 `#[derive(Iden)]` 生成的是**单数**表名 ⚠️

迁移里写 `#[derive(Iden)] enum Organization { Table }`，SeaORM 生成的表名是 **单数** `organization`；但实体声明的是 **复数** `#[sea_orm(table_name = "organizations")]`。两者一旦不一致，该实体所有查询运行时报 `no such table: organizations`，后续 ALTER 迁移会让服务启动崩溃。

**正确方式**：
1. 新增表时显式指定表名（`#[sea_orm(iden = "things")]`），确认与实体 `table_name` 完全一致
2. 用全新库验证：`ironforge migrate` + `sqlite3 .tables` 核对
3. 新功能模块务必补集成测试

### 14. 迁移应幂等 + AppState 字段变更要同步测试构造器

- `ALTER TABLE ... ADD COLUMN` 等非幂等语句用 `manager.has_table()/has_column()` 守卫
- **回滚（down）为空实现时 `up` 必须幂等**：否则 `Migrator::down()` 后再 `up()` 报 `duplicate column name`
- 给 `AppState` 新增字段时，**必须**同步更新 `crates/rg-http/tests/common/mod.rs::build_test_app_state`

### 15. russh 0.64 的通道开启是两阶段握手 ⚠️

`Handler::channel_open_session` 签名变为 `(channel, reply: ChannelOpenHandle, session) -> Result<(), Error>`（原为 `-> Result<bool, Error>`）。

- **必须显式 `reply.accept().await`**；丢弃 handle 会自动回 `AdministrativelyProhibited`，**拒绝所有会话**
- 编译能过但 SSH 全废，只能靠端到端验证（`scripts/git-protocol-e2e.sh` 的 SSH V1/V2 clone+push 步骤）
- 附带：`PrivateKey::random` 需要 rand_core 0.10 的 `CryptoRng`，rand 0.8 的 `OsRng` 不再满足；用 `rand::rng()`，故 `rg-ssh` 单独依赖 rand 0.10

### 16. gix 0.90 的破坏性变更清单

| 旧写法 | 新写法 |
|--------|--------|
| `gix::features::progress::Discard` | `gix::progress::Discard` |
| `gix::features::parallel::InOrderIter` | `gix::parallel::InOrderIter` |
| `merge_base() -> Result<Id>` | `Result<Option<Id>>`（无共同祖先为 `None`，需自行决定是否报错） |
| `count::objects` item 错误 | 收窄为 `gix::Error`（不再是 `Box<dyn Error>`） |
| `iter_from_counts` 直接返回迭代器 | 返回 `Result<迭代器>`，需 `?` |
| tree-diff 回调返回 `anyhow::Result<ControlFlow>` | 必须 `gix::Result<Action>`（`Action = ControlFlow<()>`） |
| `prepare_diff()?` | 错误是 `Exn<..>`，不实现 `StdError`，需 `map_err` 转字符串 |
| `object::find::existing::Error::NotFound` | `err.is_not_found()`（分类式 `gix::Error`） |

另外：rg-git 直接依赖的 `gix-pack` / `gix-archive` 必须与 gix 主版本同步升级，否则 Cargo 会解析出**两套** gix 子 crate（编译报 "set either the sha1 or sha256 feature flag"）。

### 17. HTTPS 模式必须显式指定 rustls 的 crypto provider ⚠️

`ring` 与 `aws-lc-rs`（russh 带入）同时编进来，rustls 无法自行选择，`ServerConfig::builder()` 会在**首次调用**时 panic（"Could not automatically determine the process-level CryptoProvider"）。症状：带 `--tls-cert/--tls-key` 启动即崩溃；HTTP 模式与单元测试都不受影响，所以长期没被发现。

- ✅ `ServerConfig::builder_with_provider(Arc::new(ring::default_provider())).with_safe_default_protocol_versions()`
- ❌ 依赖进程级默认 provider，或在测试里 `install_default()` 掩盖问题
- 回归守卫：`rg-http` 的 `tls_config_tests`（不安装 provider）与 `scripts/git-protocol-e2e.sh` 末尾的 HTTPS 步骤；测试证书由 `scripts/gen-test-tls.sh` 运行时生成，**仓库不提交私钥**（`.ai/guardrails.md` 凭据策略）

### 18. YAML 经 `rg_core::yaml`，不要直接依赖解析库

工作流、Issue 模板、Chart.yaml、gem 元数据都走 `rg_core::yaml::{from_str, to_string, Value}`（`Value` 即 `serde_json::Value`），一处决定方言与设置。与旧的 `serde_yaml` 的差异：

- `strict_booleans`：只有 `true/false` 是布尔，`yes/no/on/off` 保持字符串（否则 `on:` 触发器会被改写）
- `.inf/.nan` 变 `null` 而非整篇文档失败；`<<` 合并键会被展开；自定义 tag（`!ruby/object:...`）被忽略，节点按普通节点读取
- `010`、`1_000` 现在是数字而非字符串；必须保持文本时请加引号
- `Value` 的映射键按字母序，**不得依赖映射顺序**；需要固定输出顺序时用带字段顺序的 `#[derive(Serialize)]` 结构（见 `helm.rs` 的 `HelmIndex`）

### 19. 跨三数据库的 SQL：`ON CONFLICT DO NOTHING` 在 MySQL 上不可用 ⚠️

SeaORM 的 `.on_conflict(OnConflict::…do_nothing()).do_nothing()` 在 MySQL 上渲染出非法的 `INSERT IGNORE`（报 1064），SQLite/PostgreSQL 却正常——**只在 SQLite 上测试会漏掉**。需要“已存在则跳过”时：先条件 `UPDATE`，再普通 `insert`，把 `err.sql_err() == Some(SqlErr::UniqueConstraintViolation(_))` 当作“被别人抢先创建”（见 `rg-db/src/ops/collab_read_ops.rs`）。凡新增并发相关 SQL，用 `IRONFORGE_TEST_DATABASE_URL` 跑 PostgreSQL 与 MySQL（`rg-db/tests/collab_store.rs` 即按此设计）。

另：`composite FK` 要求被引用列有唯一键，且建表时要把该唯一键写在同一条 `CREATE TABLE` 里（`Table::create().index(Index::create().unique()…)`），自引用外键才能在 PostgreSQL 上创建。

### 20. 协作接口的权限与错误约定

- 协作端点每次请求前先 `invalidate_perm_cache_user(repo, user)` 再判权（`rg-http/src/api/collab.rs`），使撤权在下一次请求生效，**不改** `can_read_repo/can_write_repo` 本身；其他端点仍受 30 秒缓存约束。
- 需要稳定机器码或结构化细节的错误用 `AppError::coded(status, "CODE", msg).with_details(json)`（响应体 `error.details`），不要让客户端解析 `message`。
- 会话凭据错误（未知 id/错误密钥/他人账号/他仓库）统一返回同一个 403 `SESSION_CREDENTIALS_INVALID`，不要拆成可区分的错误（防探测）。
- `api/` 下的 OpenAPI 守卫测试只扫描 `src/api/*.rs`（不递归），新增端点文件请平铺在该目录。

---

### 21. SQLite：事务以读开头再写，并发时 500 ⚠️

事务第一条语句是 `SELECT`、随后才 `UPDATE`，在 SQLite 上会取一个无法升级的读快照；另一个写者提交后，升级写锁直接 `SQLITE_BUSY`（不等待 `busy_timeout`），表现为并发请求里零星的 500。**让写语句成为事务的第一条**（条件 `UPDATE` 做仲裁），幂等回放的 `SELECT` 放在事务**之前**。回归守卫：`collab_p0_tests::p02_racing_claims_*`（`rg-db/src/ops/collab_takeover_ops.rs`）。

### 22. 仓库删除是软删除，账号删除是硬删除

`rg_core::repo::service::delete_repo` 只软删仓库行并把 Git 目录移到 tombstone（`IRONFORGE_DELETED_REPO_RETENTION_DAYS`，默认 30 天后清 Git 数据，行永不清）。因此外键 `ON DELETE CASCADE` 在应用路径上**不会**触发；只有直接删仓库行才级联。账号删除（`user_ops::delete_by_id`）是真删除，会级联到其会话 → 其拥有的协作任务 → 任务里**所有人**的消息。计数预览见 `…/collaboration/deletion-impact`、`/admin/users/{id}/collaboration-impact`。

### 23. MySQL 时间戳只存整秒：排序不能只靠 `(created_at, id)` ⚠️

MySQL 的协作时间戳列是秒精度，同一秒创建的两条记录 `created_at` 相等，`id` 是随机 UUID，于是“最新在前”的顺序不确定；SQLite/PostgreSQL 有亚秒精度，测试根本发现不了。2026-11-04 在 MySQL 上实测暴露了工作包列表（`p09`），已改为按任务内唯一递增的 `revision` 排序，游标也携带 revision。

- 有单调序号的对象（工作包 `revision`、消息和决定的 `seq`）按序号排序，不要按时间戳
- 没有序号的列表，同秒并列是**合同允许的**，测试只断言 `(created_at, id)` 的顺序，不要假设同秒内的插入顺序
- “把时间拨回 N 小时”的测试断言留 1 秒容差（读回可能短 1 秒）
- 新建对象时间用 `rg_core::collaboration::creation_timestamp` 截到整秒，避免 MySQL 向上取整后落到“未来”
- 其他按 `(created_at, id)` 排序的协作列表（问题、接手记录、通知、交接等）在 MySQL 上同秒仍可能不定。2026-11-05 在 MySQL 8.4 上对全部 `collab_*_tests` 各用全新库连跑 3 遍均通过，说明现有测试没有断言同秒插入顺序；这**不**证明这些列表的顺序稳定，新增断言前仍须按此条处理
- 用 `IRONFORGE_TEST_DATABASE_URL` 指向**全新**库跑 HTTP 测试：先建用户的测试（如 `ci_integer_compat`）会占掉“首个账号即管理员”，使依赖 `bootstrap_admin` 的测试得到 403

## 依赖版本速查

```toml
axum            = "0.8"
axum-server     = "0.8"      # features: tls-rustls；bind_rustls 对地址类型泛型，需标注 SocketAddr
tower           = "0.5"
tower-http      = "0.6"      # features: cors, trace, fs
russh           = "0.64"   # 通道开启为两阶段：必须 accept ChannelOpenHandle
sea-orm         = "1.1"      # features: sqlx-sqlite, runtime-tokio-rustls, macros
clap            = "4"        # features: derive
tokio           = "1"        # features: full
serde           = "1"        # features: derive
serde_json      = "1"
serde-saphyr    = "2"        # YAML，纯 Rust；经 rg_core::yaml 使用，取代 serde_yaml
toml            = "0.8"
tracing         = "0.1"
tracing-subscriber = "0.3"   # features: env-filter
tracing-appender = "0.2"
tokio-rustls    = "0.26"
lettre          = "0.11"     # default-features = false, features: tokio1-rustls-tls, builder, smtp-transport
utoipa          = "5"        # ⚠️ 未纳入 workspace，在 rg-http 中硬编码
utoipa-swagger-ui = "8"      # ⚠️ 未纳入 workspace
anyhow          = "1"
thiserror       = "2"
gix             = "0.90"     # features: blocking-http-transport-curl, max-performance, blob-diff, pack-cache-lru-dynamic, merge, blame
chrono          = "0.4"      # features: serde
uuid            = "1"        # features: v4, serde
# Auth / Crypto
argon2          = "0.5"
jsonwebtoken    = "9"
password-hash   = "0.5"
rand_core       = "0.6"
aes-gcm         = "0.10"     # SSO/LDAP 敏感配置加密
hmac            = "0.12"
sha2            = "0.10"
hex             = "0.4"
base64          = "0.22"
# 2FA / TOTP
totp-rs         = "5"        # features: gen_secret, otpauth
qrcode          = "0.14"
rand            = "0.8"
# LDAP
ldap3           = "0.11"     # features: tls
# OAuth2（声明但未直接使用，SSO 通过 reqwest 手动实现）
oauth2          = "5"        # ⚠️ 未直接使用
openidconnect   = "4"        # ⚠️ 未直接使用
# 其他
zstd            = "0.13"     # LFS 压缩（⚠️ 未纳入 workspace，在 rg-core 中硬编码）
reqwest         = "0.12"     # features: json
flate2          = "1"
tar             = "0.4"
home            = "0.5"
```

---

## 常见错误排查

| 错误信息 | 原因 | 解决方案 |
|----------|------|----------|
| `fatal: the remote end hung up unexpectedly` | SSH 流关闭时机不对 | 确保按 exit_status → shutdown → drop 顺序 |
| `bad band #110` | HTTP receive-pack 响应没有 sideband 编码 | report-status 必须包在 band-1 sideband 中 |
| `bad line length character: unpa` | 发送了 plain pkt-lines 但客户端期望 sideband | 整体用 write_sideband_data 包装 |
| `stream did not contain valid UTF-8` | 用 read_line 读了 pkt-line 二进制头 | 改用 read_pkt_line |
| `nul byte found in provided data` | 向 Command::arg() 传了含 NUL 的字符串 | 先用 split('\0').next() 剥离 capabilities |
| `the feature requires unstable` | 用了需要 nightly 的 gix API | 用系统 git 命令替代 |
| `--repo-root` not found | CLI 用法错误 | 必须加 `serve` 子命令 |
| `HEAD` not found in ref list | `git for-each-ref` 不列出 HEAD | 用 gix API (`repo.references().all()`) 替代 |
| `fatal: not a valid ref` (HTTP clone) | Content-Type 不正确 | 确保 `info/refs` 响应使用正确的 advertisement Content-Type |
| `pack has delta resolution error` | thin pack 未加 `--fix-thin` | `git index-pack` 必须加 `--fix-thin` 参数 |
| handler 返回类型编译错误 | Axum handler 返回类型不一致 | 同一 handler 不能混用 `(StatusCode, Json)` 和 `Html` |
| ~~JSON 响应 `data` 字段为空~~ **此说已证伪（2026-10-16）** | 曾以为 `PaginatedResponse` 需 `to_value()` 包装；axum 0.8 的 `Json<T>` 能正确序列化泛型包装器。证据：`rg-http/src/pagination.rs` 单元测试 + `issue_tests`/`ci_permission_tests` 断言 `body["data"]` 为数组 | 直接 `Json(resp)`；**不要**用 `to_value(resp).unwrap()`——那会把可编译检查的 handler 变成请求路径 panic（500） |
| Axum TLS 报错 | 用了 `axum::serve()` 而不是 `axum_server` | TLS 必须用 `axum_server::bind_rustls()` |
| SeaORM 批量删除不生效 | 用了错误的方法 | 必须用 `Entity::delete_many().filter(...).exec(db)` |
| SeaORM 单行更新失败 | 直接构造 ActiveModel | 必须先 `find_by_id()` 再 `into_active_model()` |
| russh `fingerprint()` 编译错误 | 缺少 `HashAlg` 参数 | 必须传 `HashAlg::Sha256` |
| SSH 认证死循环 | `Auth::Reject` 未设 `partial_success: false` | 必须带 `partial_success: false` |
| FTS5 触发器语法错误 | 用了不正确的 SQL 语法 | 必须用 `DELETE FROM fts WHERE rowid = old.id` |
| 级联编译错误 | `mod.rs` 缺少子模块声明 | 检查 `mod.rs` 是否列出了所有子模块 |
