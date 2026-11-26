# IronForge — AI 协作上下文

> 所有 AI 工具的统一深度入口。**当前状态以本文件与 `docs/design/` 为准**；实现细节以代码为准。
> 状态：维护中 ｜ 最后更新：2026-11-26（0.5.1）｜ 来源：仓库代码 / 运行结果

---

## 项目简介

**IronForge**（铁匠铺）是用 Rust 从零实现的、**面向 AI Agent 编程的 Git 托管平台**。目标内存 <50 MB（本机实测空载约 84 MiB，见 [docs/performance.md](docs/performance.md)）、单二进制部署，在完整托管基本盘（仓库/Issue/PR/Wiki/CI/包注册表/企业认证/审计/代码搜索）之上，把 Agent 能力做成一等公民：Agent 主体身份与最小权限令牌、可预览/可重试的写入、改-推-看 CI-修 的短闭环、MCP、多 Agent 协作。

评估新功能先问「是否让 Agent 更省 token、更可控、更可追溯」，而不是「Gitea 有没有」。Gitea 差距清单只作参考，不是目标。

- 阶段：Phase 1~21 全部完成
- 技术栈：Rust（Axum 0.8 / SeaORM 1.1）+ SvelteKit 5，SQLite / PostgreSQL / MySQL，gix 0.91 + Git CLI gateway（仅测试与 mirror）

---

## 关键约定

### 仓库分工与推送

- **GitHub（`lengyuqu/ironforge`）**：开发主仓库，CI 权威来源；PR、合并、Issue 用 `gh`。
- **CNB**：只读镜像，不是开发上游；发布时按需同步。推送需要 `git -c credential.helper= -c credential.helper='!cnb git-credential' push ...`（`-c` 必须在子命令前）。
- 本地 `main` 的上游可能仍指向 `cnb/main`；推 GitHub 时显式写远端。检查目标分支与远端状态，禁止强制推送。
- 推送成功不代表 CI 通过；汇报时分开写本机命令、GitHub run id 与提交 SHA。

### Agent 生命周期护栏

> 执行任何变更操作前必须阅读 [`.ai/guardrails.md`](.ai/guardrails.md)。

等级：🔴 BLOCK（禁止） / 🟠 CONFIRM（确认） / 🟡 WARN（警告） / 🟢 ALLOW（允许）。批量删除、强推、改已执行迁移一律禁止。

### 证据分层

事实来源优先级：仓库代码 > 运行结果 > 项目文档 > 经验推测。关键事实标 `确认` / `推测` / `待验证`；文档通过不等于业务运行通过。

| 层级 | 可证明 | 不能替代 |
|---|---|---|
| 实现 | 文件存在于 diff/SHA | 命令通过 |
| 任务验证 | 分支上命令通过 | 合并后仍通过 |
| 集成 | 合并后基线门禁通过 | 真实运行或发布 |
| 运行 | 浏览器/后端/数据库链路实际执行 | 生产发布 |
| 远端 | live ref 等于目标 SHA | CI 或部署成功 |

### HTTP 路由前缀

Git 协议端点是 `/git/<owner>/<repo>/...`，不是 `/<owner>/<repo>`：

```
GET  /git/<owner>/<repo>/info/refs?service=git-upload-pack
POST /git/<owner>/<repo>/git-upload-pack
POST /git/<owner>/<repo>/git-receive-pack
GET  /health
```

REST 在 `/api/v1`。接口文档见 Swagger UI 与 `crates/rg-http/src/openapi.rs`。

### 命令

```sh
scripts/check.sh --quick   # cargo fmt + clippy
scripts/check.sh           # + rg-http / rg-core 测试
scripts/check.sh --full    # 全 workspace，与 CI 等价
cargo test -p <crate>      # 单 crate
```

快速开始、Docker 与 Git 操作示例见 [README.md](README.md)；开发规范见 [CONTRIBUTING.md](CONTRIBUTING.md)。

### MCP

`ironforge-mcp` 支持 stdio 与 streamable HTTP（每个连接带自己的令牌）。工具清单与接入见 [`.ai/README.md`](.ai/README.md)。

---

## 当前实现（0.5.1）

| 领域 | 模块 | 说明 |
|---|---|---|
| Git 协议 | `rg-git` | V1/V2、pkt-line、sideband、upload-pack、receive-pack；gix 原生 |
| SSH | `rg-ssh` | russh 0.65，公钥/密码认证、Deploy Key |
| HTTP | `rg-http` | Axum 0.8，REST + Git Smart HTTP + OCI + WebSocket + OpenAPI + SPA |
| 业务 | `rg-core` | auth / repo / issue / pr / wiki / lfs / webhook / review / branch_protection / collaborator / org / notification / email / package_registry / mirror / board / time_tracking / import / audit / search / code_indexer / collaboration |
| 数据库 | `rg-db` | SeaORM 1.1 + SQLite/PostgreSQL/MySQL，启动自动迁移 |
| CI | `rg-ci` + `rg-runner` + `rg-ci-isolation` | Pipeline、Docker/外部 Runner、composite action、缓存/矩阵/环境/OIDC/保留、容器加固 |
| 前端 | `web/` | SvelteKit 5 SPA，中英双语（key 一致性由 `scripts/i18n-key-check.mjs` 校验） |
| MCP | `rg-mcp` | Tools + Resources，stdio 与 streamable HTTP |

细节见 [ARCHITECTURE.md](ARCHITECTURE.md) 与 `docs/design/`。

---

## 未实现 / 技术债

- **Unit 级权限**：仍是仓库级 read/write/admin；ADR-0002 已裁决 8 个 unit，等实现（见 `docs/design/authorization.md` §9）。
- **多实例 / 滚动升级**：只支持单实例；DB、仓库存储、启动迁移、周期任务四处未外置（`docs/operations.md`）。
- **第三方 Action**：只支持 composite 展开；node/docker 继续 fail closed，需 `actions-v1` runner。
- **通用审批原语**：库已实现，但未接到合并/直推/发版/环境释放，只有 PR 头部漂移会使旧票过期。
- **内存定位**：目标 <50 MB，实测空载约 84 MiB；大仓库 clone 峰值 299 MiB（见 docs/performance.md）。
- **符号索引**：只存定义，不存引用；大仓库根目录 tree 仍是递归全量遍历。
- **Blob**：legacy 路径只兼容读取，一致性扫描与在线迁移未做；S3 只覆盖 Blob，Git 仓库仍在本地盘。
- **数据库迁移工具**：不提供 SQLite → PostgreSQL/MySQL 的数据迁移。
- **终端用户能力**：YAML Issue Forms、多 assignee、模板继承、实例级默认模板未做。
- **协作**：跨实例/外部平台联邦、跨仓调度、强制完成条件未做。
- **Agent**：组织拥有的 Agent 未做；Agent 评审规则靠「审批人必须是真人」保证。

---

## 重要踩坑

> 只列仍会咬人的；版本升级过程与一次性核实记录在 git 历史，不再保留。

1. **pkt-line 必须用 `read_pkt_line`**，不能按行读；长度是 4 位十六进制，含自身 4 字节。
2. **receive-pack 的 report-status 要整体塞进 sideband band 1**；先发 sideband flush 再发 plain pkt-lines 是错的。
3. **push 收到的是 thin pack**：校验/索引前必须补齐 base 对象，不能假设自包含。
4. **HTTP `info/refs` 的 Content-Type 随 service 变化**：`application/x-git-{upload,receive}-pack-advertisement`，否则真实 git 客户端不认。
5. **SQLite FTS5 触发器**：删除要走 `INSERT INTO fts(fts, ...) VALUES('delete', ...)`，直接 `DELETE` 会报错或留残缺索引。
6. **迁移 `#[derive(Iden)]` 默认是单数表名**；与实体 `table_name` 不一致时，写迁移要显式 `.table(...)`。
7. **迁移要幂等**（`has_table` / `has_column`）；改 `AppState` 字段会打断所有测试构造器，必须一起改。
8. **russh 0.65 的通道开启是两阶段握手**；关闭顺序要等 `channel_eof` / `channel_close`，否则客户端挂住。
9. **HTTPS 要显式装 rustls crypto provider**（`aws-lc-rs` 或 `ring`），否则启动时 panic。
10. **YAML 统一经 `rg_core::yaml`**，不要直接依赖解析库；前端/后端对空文档、锚点的处理要一致。
11. **MySQL 没有 `ON CONFLICT DO NOTHING`**；跨三库的 SQL 用条件插入或先查后写。
12. **SQLite 长事务先读后写，并发下会 500**；缩短事务或加重试，不要假设隔离级别能救。
13. **仓库删除是软删除，账号删除是硬删除**；查询默认过滤 soft-deleted，权限判断不要漏。
14. **MySQL 时间戳只存整秒**；排序不能只靠 `(created_at, id)`，同一秒内会抖。
15. **给 git 传令牌走 `GIT_CONFIG_COUNT` 环境变量**，不要写进 URL/命令行；gix 不认这套，要用 `open::Options::config_overrides`。
16. **`POST/DELETE /contents` 会 spawn 后置钩子**；测试里「只移动分支」要绕开，否则会等后台任务或产生额外事件。
17. **收紧字段格式时复查所有 `parse` 失败分支**；旧客户端/迁移数据可能依赖宽松解析。
18. **gix 升级要成组升子 crate**（`gix-*` 版本联动），否则同一个类型会有两个版本，编译错误很难读。
19. **协作接口**：读要仓库读权限、写要仓库写权限；Agent 写还要会话凭据；跨仓对象 404；错误用 `{error:{code,message,request_id}}`。
20. **泛型 `&C` 不参与 deref coercion**；写 trait 参数时用 `&C where C: Deref<Target = ...>` 或直接取 `&T`。

---

## 依赖版本速查

| 依赖 | 版本 | 备注 |
|---|---|---|
| axum | 0.8.9 | `Router::nest` 类型约束严格 |
| tower / tower-http | 0.5 / 0.6 | |
| russh | 0.65.4 | 两阶段通道 |
| sea-orm / sqlx | 1.1.20 / 0.8.6 | 三后端都编入，按 URL 选 |
| gix / gix-pack | 0.91 / 0.78 | 子 crate 成组升级 |
| tokio | 1 | `full` |
| jsonwebtoken | 12.0.0 | `rust_crypto`；密钥构造返回 `Result`（空密钥被拒） |
| argon2 / password-hash | 0.5 / 0.5 | |
| lettre | 0.11.24 | rustls |
| utoipa | 5 | Swagger UI 已 vendored |
| clap / reqwest | 4 / 0.12 | |

完整清单见 `Cargo.toml` 的 `[workspace.dependencies]` 与 `Cargo.lock`。

---

## 常见错误排查

| 现象 | 先查 |
|---|---|
| `database disk image is malformed` | 用备份 restore；不要原地修 |
| `SQLITE_BUSY` / 写请求 5s 后 500 | 是否有长写事务；只有建仓/wiki 有 BUSY 重试 |
| `VERSION_CONFLICT` | 重新取对象拿最新 `version`，不要盲目重试旧版本 |
| `SYMBOLS_NOT_INDEXED` | 该 commit 还没索引；先调 `POST /ai/.../index` |
| `SECRET_DETECTED` | 提交里有硬规则密钥；删掉或走允许列表 |
| `AUTHORIZATION_*` | 票据已消费/过期/被拒；重新发起审批 |
| `couldn't fetch advisory database` | 非 GitHub 环境的降级警告；此时审计没有运行 |

---

## 文档导航

| 文档 | 用途 |
|---|---|
| [README.md](README.md) | 快速开始、Docker、REST/Git 示例 |
| [ARCHITECTURE.md](ARCHITECTURE.md) | 架构总览与技术选型 |
| [CONTRIBUTING.md](CONTRIBUTING.md) | crate 边界、编码规范、测试要求 |
| [`.ai/README.md`](.ai/README.md) | MCP / REST 接入与工具清单 |
| [`.ai/guardrails.md`](.ai/guardrails.md) | 变更操作权限边界 ⭐ |
| [docs/README.md](docs/README.md) | 文档索引与维护规则 |
| [docs/design/agent-collaboration.md](docs/design/agent-collaboration.md) | 多 Agent 协作 |
| [docs/design/authorization.md](docs/design/authorization.md) | 授权、审批、团队同步 |
| [docs/design/agent-principals.md](docs/design/agent-principals.md) | Agent 主体身份 |
| [docs/design/ci.md](docs/design/ci.md) | CI 执行与安全边界 |
| [docs/design/data.md](docs/design/data.md) | 数据库、Blob、附件、模板 |
| [docs/design/code-intelligence.md](docs/design/code-intelligence.md) | 仓库地图与符号索引 |
| [docs/design/adr-0001-third-party-actions.md](docs/design/adr-0001-third-party-actions.md) | 第三方 Action ADR |
| [docs/git-protocol.md](docs/git-protocol.md) | Git 协议实现细节 |
| [docs/git-client-matrix.md](docs/git-client-matrix.md) | 真实客户端兼容矩阵 |
| [docs/performance.md](docs/performance.md) | 大仓库基线与阈值 |
| [docs/operations.md](docs/operations.md) | 备份恢复、升级、支持边界 |
| [docs/releasing.md](docs/releasing.md) | 发布流程与文档同步门禁 |
| [PLANNING.md](PLANNING.md) / [TASKS.md](TASKS.md) | 多 Agent 任务协议（启用时读） |

> 文档规则：不再保留带日期的快照、逐次核实记录和已完成任务的执行日志；这些内容留在 git 历史。文档只写**当前仍成立的事实**，并注明代码入口。
