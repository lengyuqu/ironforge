# Skill 注册中心 P1 验证记录（2026-11-21）

> **状态**：任务分支 `feat/skills-registry-p1`，**未提交、未合并、未推送、未经 CI**。
> 设计与分期见 [docs/plans/skills-registry-plan.md](../../docs/plans/skills-registry-plan.md)（§13 记录了与原计划的差异）。

## 1. 实现了什么（实现证据）

`skill` 包类型，每仓库一个注册表，从 Git 引用发布，按摘要不可变，按语义版本约束解析。**没有**跨仓库发现、MCP 工具、MCP 服务清单、直接上传、agent 发布审批门。

| 部分 | 位置 |
|---|---|
| 递归列树、按 id 读 blob（不检出、不过滤、有条目上限） | `rg-git/src/ops.rs`：`list_tree_files`、`read_blobs` |
| 制品格式、校验、确定性打包、严格读取 | `rg-core/src/package_registry/skill_bundle.rs` |
| 适配器（接入通用适配器表与 `package_types`） | `rg-core/src/package_registry/adapters/skill.rs` |
| 发布 / 解析 / 读取服务 | `rg-core/src/package_registry/skill_service.rs` |
| REST（8 个端点） | `rg-http/src/api/skills.rs`，路由在 `rg-http/src/lib.rs`，元数据在 `rg-core/src/auth/route_meta.rs`，OpenAPI 已登记 |
| 能力标志 | `api_capabilities` 增加 `registry.skills.v1` |
| 测试 | `rg-git`：4；`rg-core`：24（`skill_`）；`rg-http/tests/skills_registry_tests.rs`：11 |

要点：
- 版本由服务端从某个 commit 的树构建；记录 commit、请求里的 ref 名、目录；`source_verified` 只在 `built_by = "server"` 时为真。
- 不可变按**未压缩 tar** 的摘要（`content_digest`）判定：同版本同摘要 → 200 `existing:true`；不同 → 409 `VERSION_IMMUTABLE`（`reason` 同名，`details` 含两个摘要与已存 commit）。通用 `publish` 仍是"返回已有版本、不比内容"，**未改**，但它现在拒绝 `skill` 类型。
- 解析：Cargo 风格 semver 约束；`latest`/空 = 最高**稳定**版本；预发布只在约束里点名时匹配；yank 的版本不会被选中；无匹配 404 `NO_MATCHING_VERSION` 且 `details.available` 列出可用版本。
- 读取：清单、`SKILL.md` 文本（`content`）、单个文本文件（≤1 MiB、必须 UTF-8）、归档下载（`x-checksum-sha256`、`x-content-digest`）。每次读取都校验归档摘要与解包后摘要，不一致一律 500（日志里有细节，响应里没有路径）。
- 发布者写的文字（`description`、`frontmatter`、文件内容）响应里标 `origin: user_content`。`allowed-tools` 只记录原文，不执行、不授予。
- 权限：读 = 仓库读（CI 任务令牌需 `packages:read`）；发布 = 仓库写。发布不动分支或标签，路由元数据里没有 `.open()`/`.checked()`，所以分支受限令牌被拒（有测试）。审计 `skill.publish` 只在新发布时写一次，重放不写（有测试）。
- 拒绝项（均有稳定错误码）：非法名称/描述/frontmatter、frontmatter 块 >16 KiB、符号链接/子模块/`..`/绝对路径/`.git` 段/大小写冲突/文件与目录同名、>200 个文件、>20 MiB、`SKILL.md` >256 KiB、压缩包 >5 MiB、gzip 炸弹、非法 ref（只允许分支/标签/提交 id，不允许 `^ ~ : @{`）、版本缺失/非规范 semver/带构建元数据/与 frontmatter 的 `metadata.version` 不一致、保留名 `publish`。

## 2. 验证（任务验证，本机，提交前的工作树）

| 层 | 命令 | 结果 |
|---|---|---|
| SQLite 全工作区 | `cargo test --workspace --no-fail-fast` | 174 个测试目标，**1749 passed，0 failed，16 ignored**（ignored 为既有） |
| PostgreSQL 17（Docker） | 临时脚本只跑 `cargo test -p rg-http --test skills_registry_tests`（`IRONFORGE_TEST_DATABASE_URL`） | 11 passed |
| MySQL 8.4（Docker） | 同上 | 11 passed |
| Clippy | `cargo clippy --workspace --all-targets -- -D warnings` | 干净（仅既有的 `workspace.package.homepage` 清单警告） |
| 格式 | `cargo fmt --all` | 已应用，仅动到本任务文件 |

**一次真实的发现**：`package_versions.metadata` 是 `TEXT` 列，MySQL 上上限 65,535 字节（在 MySQL 8.4 严格模式下实测：往 `TEXT` 写 70,000 字节报 `ERROR 1406 Data too long`）。原设计把逐文件清单存进该列，200 个长路径的 skill 会写不进去。因此：元数据只存 frontmatter、摘要、来源、解包体积；frontmatter 块 ≤16 KiB，序列化后 ≤48 KiB；文件清单每次从归档重建。`a_skill_at_the_file_limit_with_long_paths_publishes` 覆盖该场景（200 个文件、240 字节深层路径），在三个后端都通过。**注意：我没有拿修复前的代码在 MySQL 上复现过这次失败**，上面只证明限制存在、修复后的代码在 MySQL 通过。

## 3. 没有验证的

- **没有 CI。** 分支没有推送，GitHub Actions 没有跑过。
- `scripts/ci-db-tests.sh` 的目标清单里加了 `skills_registry_tests`，但**没有用该脚本跑过**；上表的 PostgreSQL/MySQL 结果来自等价的一次性脚本（只跑这一个目标，每个后端一个全新容器）。
- 没有真实 MCP 客户端、真实反向代理、浏览器或 `ironforge-mcp` 的端到端；没有 MCP 工具（P2）。
- 没有并发发布同一版本的压力测试；竞争路径靠 `(package_id, version)` 唯一索引兜底，代码里有"失败后重查并比对摘要"的处理，**没有专门测试**。
- 没有大体积边界的性能数据：读清单/`content` 会解压整个归档（上限 5 MiB 压缩、约 25 MiB 解压）并算摘要，P1 接受这个代价，没有测过延迟。
- gzip 实现换版本后，同一棵树重新打出的 `.tar.gz` 字节可能不同；不可变判定用的是未压缩 tar 的摘要，所以不受影响，但已存归档的 `archive_digest` 才是客户端下载校验该用的值。
- 外部格式（Agent Skills 的字段与名称规则）按检索摘要实现，**没有逐字对照官方规范**；名称上限 64、`compatibility` 上限 500 等数值待对照。
- 未登录用户能否发现公开仓库里的 skill：沿用 `require_read_with_ci_scope` 的现有语义，没有单独核对。

## 4. 已知限制与风险

- P1 中任何持 `repo:write` 的 agent 令牌都能在**该仓库**发布 skill（没有审批门，P2）；因为没有全局发现，别的仓库的 agent 不会被动加载到它，但同仓库的读取者会。
- 仓库白名单令牌（`repos`）能访问白名单内的 `…/skills/**`（路由元数据是 `repo(...)`），不能访问别的仓库。
- 删除 skill 版本沿用通用 `DELETE …/packages/skill/{name}/{version}`，该路径没有为 skill 单独测试。yank 用通用 `PATCH …/yank`，有测试。
