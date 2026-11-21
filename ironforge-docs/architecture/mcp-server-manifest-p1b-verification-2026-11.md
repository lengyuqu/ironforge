# MCP 服务清单注册 P1b 验证记录（2026-11-21）

> **状态**：任务分支 `feat/mcp-server-manifest-p1b`，**栈在 `feat/skills-registry-p1`（PR #7，未合并）之上**；未经 CI。
> 设计与分期见 [docs/plans/skills-registry-plan.md](../../docs/plans/skills-registry-plan.md)（§14 记录与原计划的差异）。Skill 部分见 [skills-registry-p1-verification-2026-11.md](skills-registry-p1-verification-2026-11.md)。

## 1. 实现了什么（实现证据）

`mcp-server` 包类型：每仓库一个注册表，存放官方 MCP Registry 形状的 `server.json` **清单**，按摘要不可变，按语义版本约束解析。**不托管服务、不抓取清单里的任何 URL、不看它指向的包。** 没有跨仓库发现、MCP 工具、所有权核验、直接上传之外的发布路径。

| 部分 | 位置 |
|---|---|
| 清单校验、规范化、摘要 | `rg-core/src/package_registry/mcp_manifest.rs`（`analyze`） |
| 发布 / 读取 / 解析服务 | `rg-core/src/package_registry/mcp_service.rs` |
| 适配器（接入 `package_types::MCP_SERVER` 与适配器表） | `rg-core/src/package_registry/adapters/mcp_server.rs` |
| skill 与清单共用的版本选择 | `rg-core/src/package_registry/version_select.rs`（从 `skill_service.rs` 抽出） |
| REST（5 个端点） | `rg-http/src/api/mcp_servers.rs`；路由在 `rg-http/src/lib.rs`；元数据在 `rg-core/src/auth/route_meta.rs`；OpenAPI 已登记 |
| 能力标志 | `api_capabilities` 增加 `registry.mcp_servers.v1` |
| 测试 | `rg-core`：`mcp_manifest` 13 项；`rg-http/tests/mcp_servers_registry_tests.rs`：11 项 |

要点：
- **端点**：`POST …/mcp-servers`（体 `{"server": <server.json>}`，`deny_unknown_fields`）、`GET …/mcp-servers`、`GET …/mcp-servers/{namespace}/{server}`、`…/resolve?constraint=`、`…/versions/{v}`。名字是 `namespace/server`，两个路径段。
- **校验**（来源见 `mcp_manifest.rs` 头注释）：取自发布的 schema 的规则，加上本注册表**更严**的几条（版本必须是单个规范 semver、`packages`/`remotes` 至少一项、远程 URL 必须 https 或本机 http 且无内嵌凭据、包版本不是范围、体积与条数上限）。拒绝一律是 400（过大是 413）加稳定码：`MCP_NAME_INVALID`、`MCP_DESCRIPTION_INVALID`、`MCP_VERSION_INVALID`、`MCP_NO_ENDPOINT`、`MCP_PACKAGE_INVALID`、`MCP_REMOTE_INVALID`、`MCP_MANIFEST_INVALID`、`MCP_MANIFEST_TOO_LARGE`。未知 `registryType` 与未知字段保留（`known_registry_type: false`）。
- **不可变**：按规范化清单（键排序、无多余空白）的摘要；同版本同摘要 → 200 `existing:true`，不同 → 409 `VERSION_IMMUTABLE`（`details` 含两个摘要）。键序、空白不同的同一清单是同一个摘要（有测试）。
- **解析**：与 skill 共用 `version_select`：`latest`/空 = 最高**稳定**版本，预发布只在约束里点名时匹配，yank 的版本不会被选中，无匹配 404 `NO_MATCHING_VERSION` 带 `details.available`。
- **读取**：版本接口从存储读回 `server.json`，**先核对摘要**再返回；返回清单原文（`manifest`，`origin: user_content`）、`digest`、`size`、`declared_schema`，以及从清单读出的 `endpoints`（`packages[]` 与 `remotes[]` 摘要，`insecure_local` 标出本机 http）。
- **所有权**：每个响应固定 `ownership: "unverified"`——名字、npm 包、远程 URL 归谁，是发布者的声明，注册表没有核验。
- 权限：读 = 仓库读（CI 令牌需 `packages:read`），发布 = 仓库写；路由元数据里没有 `.open()`/`.checked()`，分支受限令牌发布被拒（有测试）。审计 `mcp_server.publish` 只在新发布时写一次（有测试）。
- 通用 `POST …/packages/mcp-server/publish` 拒绝该类型（有测试）；yank 沿用通用 `PATCH …/packages/mcp-server/{name}/{v}/yank`，名字里的 `/` 写成 `%2F`（有测试，yank 后 `resolve` 跳过、取消后恢复）。

## 2. 验证（任务验证，本机，提交前的工作树）

| 层 | 命令 | 结果 |
|---|---|---|
| 清单单测 | `cargo test -p rg-core --lib package_registry::` | 67 passed（含 `mcp_manifest` 13 项与既有 skill 单测） |
| 集成 | `cargo test -p rg-http --test mcp_servers_registry_tests` | 11 passed（SQLite） |
| 重构回归 | `skills_registry_tests`、`route_metadata_tests`、`collab_contract_tests` | 11 / 16 / 9 passed |

| SQLite 全工作区 | `cargo test --workspace --no-fail-fast` | 175 个测试目标，**1773 passed，0 failed，16 ignored**（ignored 为既有；比 P1 的 1749 多 24 = 13 + 11 项新测试） |
| PostgreSQL 17（Docker） | 临时脚本跑 `cargo test -p rg-http --test mcp_servers_registry_tests --test skills_registry_tests`（`IRONFORGE_TEST_DATABASE_URL`） | 11 + 11 passed |
| MySQL 8.4（Docker） | 同上 | 11 + 11 passed |
| Clippy | `cargo clippy --workspace --all-targets -- -D warnings` | 干净（仅既有的 `workspace.package.homepage` 清单警告） |
| 格式 | `cargo fmt --all` | 已应用，仅动到本任务文件 |

PostgreSQL/MySQL 的结果来自一次性脚本；`scripts/ci-db-tests.sh` 的清单加了 `mcp_servers_registry_tests`，**没有用该脚本跑过**。

**一次真实的发现**：第一次在 PostgreSQL / MySQL 上跑，`publishing_is_audited_once` 失败——共享数据库里其他测试的发布也写进了同一张审计表，按 `action` 数行会数到别人的。SQLite 每个测试独立库所以看不出。已改为同时按用户名过滤；**P1 的 skill 审计测试有同样的缺陷**（它在 P1 的 PostgreSQL/MySQL 运行里碰巧通过），一并改了。

## 3. 没有验证的

- **没有 CI。** 分支没有推送，也依赖未合并的 PR #7。
- **`server.json` 规则来自子 agent 对发布 schema 的读取报告**，我没有自己再读原文。它**没有读** `quickstart.mdx`（命名空间认证）与 `package-types.mdx`（各注册表的所有权核验，含 npm `mcpName`），所以官方对所有权的要求**未核实**，本实现也完全不做核验。子 agent 没有确认 `2025-12-11` 是否是最新的日期版本。
- schema 里只在文字中写明的规则（版本范围被拒、`fileSha256` 对 MCPB 必填、`runtimeHint` 与 `runtimeArguments` 配套、icon 必须 https）**没有强制**，除"包版本不是范围"之外。`icons`、`_meta` 不检查内容，原样保存。
- 没有并发发布同一版本的专门测试（靠 `(package_id, version)` 唯一索引与失败后重查）。
- 没有真实 MCP 客户端、浏览器、`ironforge-mcp` 的端到端；没有 MCP 工具（P2）。
- 清单里的 JSON 重复键：解析按"后者生效"，规范化后只存一份；没有专门拒绝。
- 对 `registryBaseUrl`/`websiteUrl`/`repository.url` 只检查是 http(s) URL、无内嵌凭据，**不**限制到官方注册表要求的那几个主机。

## 4. 已知限制与风险

- 清单是**数据**：agent 不应把其中的 `description`、环境变量名、参数当成指令执行或据此自动安装；`origin: user_content` 就是这个提示。注册表不执行、不安装任何东西。
- P1b 中任何持 `repo:write` 的 agent 令牌都能在**该仓库**发布清单；因为没有全局发现，别的仓库不受影响，同仓库读取者会看到（审批门在 P2）。
- 仓库白名单令牌能访问白名单内的 `…/mcp-servers/**`，不能访问别的仓库。
- 在规则收紧后，按旧规则存下的清单仍可读：`endpoints` 会变成 `null`，`manifest` 与摘要照常返回。
