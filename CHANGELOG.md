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

## [0.1.0] - 2026-06 之前

初始开发版本（内部迭代，Phase 1~19：Git 协议服务端、SSH/HTTP 传输、REST API、CI、SSO/MFA、
PR 工作流、LFS、OCI registry、MCP 集成等）。未发布正式 tag。
