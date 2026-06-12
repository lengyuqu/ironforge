# IronForge 项目长期记忆

## 项目概述
- **名称**: IronForge（铁匠铺）- Rust Git 托管平台
- **位置**: `/Users/laocai/Desktop/帮我做个方案/ironforge/`
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

## 待完成（2026-06-07 更新）
- Mirror / Board / Time Tracking → ✅ 已完成（2026-06-07）
- /search/code 端点 → ✅ 已完成（FTS5 + CLI `index-repo` 命令）
- SSH Protocol V2 (HTTP) → ✅ 已完成
- Runner OpenAPI 注解 → ✅ 已完成
- **Package Registry（P0）**: ✅ 核心框架 + 协议适配器（2026-06-07 下午），DB+存储+服务+REST API+CLI+PackageAdapter trait + Cargo/npm/Generic 适配器 + sparse index + npm registry 端点
- **LDAP/SSO/2FA（P1）**: ✅ 完成（2026-06-07），LDAP 认证（ldap3 v0.11 + SearchEntry pattern）、OAuth2 SSO（GitHub/GitLab，reqwest 直连）、TOTP 2FA（totp-rs v5.7 + QR SVG）、AES-256-GCM 加密存储、5 个 DB 迁移 + 4 个实体 + 4 个 ops + 4 个 core 服务 + 8 个 REST API 端点
- **审计日志（P1）**: ✅ 完成（2026-06-07），append-only 审计日志表 + admin-only 查询 API + `audit!` 宏便捷记录，1 个 DB 迁移 + 1 个实体 + 1 个 ops + 1 个 core 服务 + 2 个 REST API 端点
- **数据迁移导入（P1）**: ✅ 完成（2026-06-07 晚间），GitHub/GitLab → IronForge 全量导入（repo/label/milestone/issue+comment/PR+review/release），CLI + REST API
- **Gitea Actions 兼容（P2）**: 自研 CI/CD 不兼容 Actions 生态
- **SSH Git Protocol V2（P2）**: ✅ 已完成（2026-06-07 确认），rg-ssh exec_request 正确路由到 handle_v2_stream，ls-refs/fetch/object-info 全部实现
- **邮件通知完整集成（P1）**: 模块存在，未完全集成

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
18. **oauth2 crate v5 类型状态过于复杂**: v5.0 `BasicClient` builder 返回不同类型状态标记（`EndpointSet`/`EndpointNotSet`），与 `exchange_code()`/`authorize_url()` 的方法签名不兼容。推荐直接用 `reqwest` 实现 OAuth2 流程（手动构造 URL + form POST），避免依赖 oauth2 crate 的类型状态系统。
19. **aes-gcm Nonce 类型参数**: `Nonce::<Aes256Gcm>` 解析为 `GenericArray<u8, AesGcm<...>>` 而非 `GenericArray<u8, U12>`，导致 `ArrayLength<u8>` 不满足。正确用法：`Nonce::from_slice(&bytes)` 让编译器推断类型。
20. **SeaORM ops 导入**: `use sea_orm::entity::prelude::*;` 不包含 `Set`/`NotSet`/`QueryOrder`，必须用 `use sea_orm::*;`
21. **axum 0.8 Host extractor**: 需要 `host` feature，未启用时用 `HeaderMap` + `headers.get("host")` 替代

### 待完成更新（2026-06-07）
- /search/code 端点 ✅ 已完成（FTS5 + CLI 索引触发）
- SSH Protocol V2 ✅ 已完成（ls-refs/fetch 全面修复）
- Runner OpenAPI 注解 ✅ 已完成（6 个 route_layer 端点 + 6 个 ToSchema 类型 + 清理 4 处未使用导入）
