# IronForge 项目长期记忆

## 项目概述
- **名称**: IronForge（铁匠铺）- Rust Git 托管平台
- **位置**: `/Users/yuqu/vberCodeing/ironforge/`
- **Rust**: 1.95.0 (stable) | **二进制**: `ironforge`
- **GitHub**: https://github.com/lengyuqu/ironforge (public)

## 技术栈
- HTTP: Axum 0.8 + axum-server(tls-rustls) | SSH: russh 0.51
- Git: gix 0.84 + git CLI gateway（raw git 已全消除）| ORM: SeaORM 1.1（SQLite）
- 认证: Argon2 + JWT HS256 | CI/CD: serde_yaml + sh/docker
- 前端: SvelteKit 5 SPA | i18n: 中英双语（199 key）

## Crate 结构
- **rg-cli**: CLI 入口 | **rg-core**: 业务逻辑 | **rg-git**: Git 协议 | **rg-ssh**: SSH 服务端
- **rg-http**: HTTP + REST API + WebSocket | **rg-db**: DB 实体/迁移/ops
- **rg-ci**: CI/CD 引擎 | **rg-runner**: Runner Agent | **rg-mcp**: MCP 服务器

## Phase 进度（全部完成 ✅）
Phase 1-21 全部完成（Phase 21: Package Registry 10 种适配器（9 native + generic）+ OCI、LDAP/SSO/2FA、审计日志、数据迁移、Mirror/Board/Tracking、代码搜索、SSH V2）

## 文档结构速查
```
ironforge/
├── CLAUDE.md              # AI 深度参考（踩坑/依赖/现状）
├── AGENT.md               # AI 统一入口（轻量概览）
├── ARCHITECTURE.md        # 架构设计
├── CONTRIBUTING.md        # 开发规范
├── README.md              # 项目说明 + 快速开始
├── docs/                  # 核心设计文档（6 篇）
├── ironforge-docs/        # 分析报告（architecture/analysis/comparison/ci/testing + archive/）
├── .ai/README.md          # AI Agent 接入指南
└── deploy/README.md       # Observability 部署说明
```

## 最新对比文档
- `ironforge-docs/comparison/gitea-vs-ironforge-2026.md` — 2026-06-16 完整对比报告（v3.1，2026-07-09 数据对齐）
- `ironforge-docs/comparison/gitea-gap-list.csv` — 差距清单 CSV（2026-06-16 同步更新）
- **核心完成度**: 约 85%
- **已完成**: 包注册表 10 种适配器（9 native: Docker/OCI/npm/PyPI/Maven/Cargo/NuGet/Helm/RubyGems/Composer + generic fallback；Go 等走 generic）、企业认证（LDAP+OAuth2+TOTP+审计日志）、数据迁移（GitHub/GitLab 导入）、邮件通知（SMTP+lettre）、运维（SQLite WAL/PRAGMA/JWT env/RateLimiting/Prometheus）、Least-privilege Token、前端包注册表页面、密码重置、Composer 适配器、CI/CD 日志写队列、Git CLI 网关、Pipeline 可视化、Wiki Markdown 渲染/TOC/删除、GPG 签名 UI、审计日志归档、软删除统一、Subpath 归档下载
- **剩余技术债**: gix 原生迁移 70%（16 处 CLI 经 GitCommandGateway 保留：Diff×4/Fetch×2/Rebase×4/Pack×3/GPG×2/Clone×1）；raw `Command::new("git")` 已全部消除（2026-07-04，防回归守卫 `test_no_raw_git_command_in_crates` 无豁免通过）

## gix 迁移状态（2026-07-04 更新）
- 进度 ~70%（16 处 git CLI 保留，gix API 覆盖其余，已消除 7 处 merge/commit/ref CLI）
- 2026-06-06: 完成 merge×4, commit×2, ref-delete×1 的 gix 替换（pull_request/service.rs）
- 2026-07-04: raw `Command::new("git")` 全部消除——`repo/service.rs` 13 处（auto_init/create_or_update_file/delete_file）迁移至网关；网关新增 `run_with_env` 支持 commit 身份 env；防回归守卫移除 `repo/service.rs` 豁免后通过。⚠️ 这些仍走 CLI（经网关），gix 原生进度不变。
- 剩余 CLI（经网关，gix 原生待 Phase 3）: Diff×4（patch 字节对齐难，按回退条款保留）, Fetch×2, Rebase×4（gix-rebase 是 "idea"）, Pack×3, GPG×2, Clone×1
- gix 版本 0.84（Cargo.lock 锁定 0.84.0）

## 踩坑经验（完整版 — 代码注释已补充）
1. **pkt-line**: 用 `read_pkt_line`，注意 flush=0000
2. **receive-pack report-status**: 整体用 sideband 多路复用，不可直接吐 pkt-line
3. **thin pack**: `git index-pack` 必须加 `--fix-thin`，否则报错
4. **for-each-ref**: 不列 HEAD；用 gix `repo.references().all()` 替代
5. **HTTP Content-Type**: Smart HTTP 对 `info/refs` 响应必须用 `application/x-git-*-advertisement`
6. **Axum nest()**: 所有嵌套路由必须共享相同 `State<AppState>`
7. **Axum IntoResponse**: handler 返回类型必须一致，不能混用 `(StatusCode, Json)` 和 `Html`
8. **PaginatedResponse**: 必须用 `serde_json::to_value(resp)` 包装后返回
9. **Axum TLS**: 用 `axum_server::bind_rustls()`，不能用 `axum::serve()`
10. **SeaORM 批量删除**: 用 `Entity::delete_many().filter(...).exec(db)`
11. **SeaORM 单行更新**: 先 `find_by_id` 再 `into_active_model`
12. **russh fingerprint()**: 必须传 `HashAlg::Sha256`
13. **russh Auth::Reject**: 必须带 `partial_success: false`
14. **SQLite FTS5 触发器**: 用 `DELETE FROM fts WHERE rowid = old.id`，不要用 `'delete'` 命令语法
15. **mod.rs 缺少模块声明**: 级联错误通常意味着子模块未被 `mod.rs` 列出
16. **Git Smart HTTP 路由**: git 客户端请求 `/{owner}/{repo}.git/info/refs`，必须注册根级路由 + `strip_git_suffix()` 剥离 `.git` 后缀
17. **pkt-line 格式**: `# service=` 行必须 pkt-line 包裹；长度计算 = payload + 4(头) + 1(\n)；用 `write!` 不是 `writeln!`
18. **upload-pack stdin EOF**: `git pack-objects --stdout` stdin 是 `Stdio::piped()` 必须立即 `take+drop` 关闭，否则进程等待 EOF 永久阻塞
19. **tokio duplex 死锁**: HTTP handler 用 `duplex(64KB)` 作输出缓冲区时，必须 spawn 并发 reader_task 防止 write/read 死锁（pack > 64KB 时 write 阻塞等 read 消费）
20. **V2 Smart HTTP POST**: capability advertisement 在 info/refs GET 发送，POST handler 不应再发——用 `handle_v2_http()` 跳过 advertisement

## DB 迁移清单
m000001~m000009: users/repos/issues/PR/wiki/LFS/webhooks/CI/reviews/protection/collaborators/orgs/notifications
m20260508_000001~000005: labels/watches/release_assets/deleted_at+fork_id/commit_statuses/FTS5
m20260607_000006~000011: alter_users_auth/oauth_accounts/mfa_backup_codes/login_logs/sso_providers/audit_logs（LDAP/SSO/2FA/审计日志）

## 文档入口
- **必读**: `ironforge/CLAUDE.md`（AI 统一入口，含踩坑记录）
- **架构**: `ironforge/ARCHITECTURE.md`
- **规范**: `ironforge/CONTRIBUTING.md`
- **Git 协议**: `ironforge/docs/git-protocol.md`
- **分析报告**: `ironforge-docs/README.md`

## 前端要点
- Svelte 5 runes: `$state` / `$derived` / `$effect`
- i18n: `createT()` + `$t()` | 翻译文件: `web/src/lib/i18n/translations/`
- PaginatedResponse 需 `resp.data` 解包

### 新增踩坑（2026-06-07）
16. **gix !Send 陷阱**: `gix::Repository` 含 `RefCell`（`!Send`），async fn 中不得跨 `.await` 持有，必须用同步块 `{ let repo = ...; ...; /* drop */ }` 收集数据后再 async I/O
17. **oauth2 crate v5 类型状态过于复杂**: v5.0 `BasicClient` builder 返回不同类型状态标记（`EndpointSet`/`EndpointNotSet`），与 `exchange_code()`/`authorize_url()` 的方法签名不兼容。推荐直接用 `reqwest` 实现 OAuth2 流程（手动构造 URL + form POST），避免依赖 oauth2 crate 的类型状态系统。
18. **aes-gcm Nonce 类型参数**: `Nonce::<Aes256Gcm>` 解析为 `GenericArray<u8, AesGcm<...>>` 而非 `GenericArray<u8, U12>`，导致 `ArrayLength<u8>` 不满足。正确用法：`Nonce::from_slice(&bytes)` 让编译器推断类型。
19. **SeaORM ops 导入**: `use sea_orm::entity::prelude::*;` 不包含 `Set`/`NotSet`/`QueryOrder`，必须用 `use sea_orm::*;`
20. **axum 0.8 Host extractor**: 需要 `host` feature，未启用时用 `HeaderMap` + `headers.get("host")` 替代

### 新增踩坑（2026-06-18 本地调试）
21. **Git Smart HTTP 路由前缀**: git 客户端请求 `/{owner}/{repo}.git/info/refs`，不在 `/git` 前缀下。必须在根路由器注册 git smart HTTP 路由，否则 SPA fallback 兜住返回 HTML，git 报 "not valid"
22. **pkt-line `# service=` 行必须包裹**: Smart HTTP 的 `# service=git-upload-pack\n` 行是 pkt-line 数据，必须有长度头（`001e# service=...\n`），不能裸写。裸写导致 git V1 客户端报 `bad line length character: # se`
23. **pkt-line 长度计算含 \n**: pkt-line payload 末尾的 `\n` 是数据的一部分，长度头 = payload_bytes.len() + 4（头）+ 1（`\n`）。`writeln!` 会多加 `\n` 导致长度不一致，应改用 `write!`
24. **空仓库 V1 info/refs**: 空仓库的 dummy ref 行格式应为 `<null SHA> HEAD\0<capabilities>\n`，不能用 `capabilities^service` 等自定义格式
