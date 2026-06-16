# IronForge 项目长期记忆

## 项目概述
- **名称**: IronForge（铁匠铺）- Rust Git 托管平台
- **位置**: `/Users/yuqu/vberCodeing/ironforge/`
- **Rust**: 1.95.0 (stable) | **二进制**: `ironforge`
- **GitHub**: https://github.com/lengyuqu/ironforge (public)

## 技术栈
- HTTP: Axum 0.8 + axum-server(tls-rustls) | SSH: russh 0.51
- Git: gix 0.83 + git CLI fallback | ORM: SeaORM 1.1（SQLite）
- 认证: Argon2 + JWT HS256 | CI/CD: serde_yaml + sh/docker
- 前端: SvelteKit 5 SPA | i18n: 中英双语（199 key）

## Crate 结构
- **rg-cli**: CLI 入口 | **rg-core**: 业务逻辑 | **rg-git**: Git 协议 | **rg-ssh**: SSH 服务端
- **rg-http**: HTTP + REST API + WebSocket | **rg-db**: DB 实体/迁移/ops
- **rg-ci**: CI/CD 引擎 | **rg-runner**: Runner Agent | **rg-mcp**: MCP 服务器

## Phase 进度（全部完成 ✅）
Phase 1-20 全部完成（最后: Phase 20 工程化 ✅，Phase 19 P2 功能 ✅）

## Phase 进度（全部完成 ✅）
Phase 1-21 全部完成（Phase 21: Package Registry / LDAP/SSO/2FA / Audit / Mirror / Board / Tracking / 代码搜索 / SSH V2）

## 文档结构速查
```
ironforge/
├── CLAUDE.md              # AI 深度参考（踩坑/依赖/现状）
├── AGENT.md               # AI 统一入口（轻量概览）
├── ARCHITECTURE.md        # 架构设计
├── CONTRIBUTING.md        # 开发规范
├── README.md              # 项目说明 + 快速开始
├── docs/                  # 核心设计文档（6 篇）
├── ironforge-docs/        # 分析报告（当前 5 篇，archive/ 下 3 篇过时报告）
├── .ai/README.md          # AI Agent 接入指南
└── deploy/README.md       # Observability 部署说明
```

## 最新对比文档
- `ironforge-docs/gitea-vs-ironforge-2026.md` — 2026-06-07 完整对比报告（v2.0）
- `ironforge-docs/gitea-gap-list.csv` — 差距清单 CSV（可用 Excel 打开）
- **核心完成度**: 约 80%（vs 旧版 40-50%）
- **最大差距**: Package Registry（16 种包类型，完全缺失，P0）
- **P1 差距**: 邮件通知完整集成（LDAP/SSO/2FA ✅、数据迁移导入 ✅、审计日志 ✅）
- **IronForge 独有优势**: MCP AI Agent 集成（rg-mcp）、纯 Rust 栈、gix 迁移（70%）

## gix 迁移状态（2026-06-06 更新）
- 进度 ~70%（16 处 git CLI 保留，gix API 覆盖其余，已消除 7 处 merge/commit/ref CLI）
- 2026-06-06: 完成 merge×4, commit×2, ref-delete×1 的 gix 替换（pull_request/service.rs）
- 剩余 CLI: Diff×4（可尝试 blob-diff）, Fetch×2（需 pack transfer）, Rebase×4（gix-rebase 是 "idea"）, Pack×3, GPG×2, Clone×1
- gix 版本 0.83（最新 0.84，仅 SHA256 + edition 提升，无功能变化）

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
