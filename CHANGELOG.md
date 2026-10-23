# Changelog

本文件记录 IronForge 的版本演进。格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，
版本号遵循 [Semantic Versioning](https://semver.org/lang/zh-CN/)（0.x 阶段以 minor 为 breaking 粒度）。

## [0.2.0] - 2026-09-07

本版本的核心里程碑：**生产路径 git CLI 调用归零，100% gix 原生**。运行时不再依赖系统安装的 git，
部署即拷贝单个二进制 + 静态前端，彻底消除 CLI 版本漂移、stderr 解析与子进程管理问题。

### Changed（架构级，breaking）

- **git CLI 退役**：原经 `GitCommandGateway` 调用 git CLI 的全部 11 处生产路径（pack 生成/摄取、
  rev-list、fork fetch、文件编辑提交、验签等）替换为 gix 原生实现；`cli_gateway.rs` 仅保留用于测试造数据，
  防回归守卫 `test_no_raw_git_command_in_crates` 继续生效。
- 新增 gix 原生底层能力（`rg-git/src/ops.rs`）：`commits_between` / `commit_graph` / `generate_pack` /
  `pack_universe` / `pack_for_wants` / `index_pack_bytes` / `copy_objects_from_source` / `commit_tree_edits` /
  `verify_commit_details_with_repo`。
- fork 同步改为进程内对象拷贝（`copy_objects_from_source`），不再 spawn `git fetch`；
  fork 创建不再 `clone --bare`，改为 gix 裸库创建 + 引用复制 + pack 复制。
- `/health` 的 git 检查项由探测 CLI 网关改为直接报告 `"builtin"`（gix 进程内提供，恒可用）。

### Known limitations

- Pack 生成不做 delta 压缩（全量对象，功能正确但大仓带宽略高）。
- 不支持 partial-clone filter（`--filter=blob:none` 等客户端选项收到时降级为全量）。
- 无内建 gc/repack 维护命令（SQLite + loose/pack 对象随时间可手动维护）。

### Documentation

- README / ARCHITECTURE / CONTRIBUTING / CLAUDE / AGENT / docs/git-protocol 全面与
  "CLI 已退役" 现状对齐；迁移全程记录见 `docs/gix-migration-assessment.md`。

### Quality

- 全 workspace 416 个测试通过（0 失败）；touched crates clippy 无警告。

### Added（2026-10-23 增量，随 v0.2.0 首个公开发布一并交付）

- **CI 工作区改为 gix 原生 linked worktree**：`Repository::add_worktree` / `remove_worktree` 替换
  `git worktree add --detach` / `remove --force`（新增 `worktree-mutation` feature）。`Force::DiscardChanges`
  等价 `--force` 但仍尊重 operator lock；崩溃残留的注册会在下次准备时自动 prune。
- **导入裸克隆改为 gix 原生**：`prepare_clone_bare` + `fetch_only` 替换 `git clone --bare`，
  私有源继续走 git credential helper。
- **PR patch 对拍扩展**：尾随空行增删、无尾换行 + 末行删除、纯删除 + 纯新增混合，与 `git diff` 仍字节级一致。
- **依赖升级**：gix 0.87.1 → 0.90.0（错误体系并入分类式 `gix::Error`、`merge_base` 返回 `Option`、
  移除 `gix::features::{progress,parallel}`），russh 0.51.1 → 0.64.1。

### Security（2026-10-23）

- 关闭代码审计 #1 的 7 项：PAT 改用 `OsRng` 256-bit、SSO cookie 改 HMAC-SHA256、出站 HTTP 复用受限 client、
  认证端点常开限流、Git 流式传输加 deadline、host runner fail-closed、release 资产附 SHA-256 校验和。
- 全仓声明 MIT 许可：workspace `license` 继承 + LICENSE 全文 + `web/package.json`。

### Known limitations（发布时补充）

- mirror 的 clone 与同步仍走 git CLI：gix 0.90 的 clone fetch 不按根级 `refs/*` 通配写 ref，
  且没有 `fetch --prune` 等价能力；枚举命名空间会丢失未知命名空间，属语义回退。
  现状与解除条件见 `CLAUDE.md` 技术债表。

## [0.1.0] - 2026-06 之前

初始开发版本（内部迭代，Phase 1~19：Git 协议服务端、SSH/HTTP 传输、REST API、CI、SSO/MFA、
PR 工作流、LFS、OCI registry、MCP 集成等）。未发布正式 tag。
