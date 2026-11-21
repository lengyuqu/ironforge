# 注册中心跨仓库发现与 MCP 读取工具 P2a 验证记录（2026-11-21）

> **状态**：任务分支 `feat/registry-discovery-p2a`，**栈在 `feat/mcp-server-manifest-p1b`（PR #8）之上，而 #8 又在 `feat/skills-registry-p1`（PR #7）之上**；未提交、未推送、未经 CI。
> 设计与分期见 [docs/plans/skills-registry-plan.md](../../docs/plans/skills-registry-plan.md)（§15 记录与原计划的差异）。上游：[skills P1](skills-registry-p1-verification-2026-11.md)、[MCP 清单 P1b](mcp-server-manifest-p1b-verification-2026-11.md)。

## 0. 先说没做的

**P2 的 agent 发布审批门（P2b）没有做**，而且核实后发现计划的前提不成立：授权原语 #19 在 `rg-core`/`rg-db` 里有，但**没有任何 REST 端点**，模块之外也没有 `authorization::request` 的调用方。P2b 因此是一个单独的大任务（新 `Action`、请求/批准/拒绝端点、agent 准入检查、把 `consume` 与写版本放进同一个事务——现在的通用 `service::publish` 不在事务里），需要负责人先确认。结果是：**发现接口上线后，"agent 默认不能发布到全局发现"这条约束并不存在**，持 `repo:write` 的 agent 令牌可以在**公开**仓库发布，所有人都能在发现里看到。目前只有标注（`publisher.kind`、`origin: user_content`、MCP 的 `ownership: unverified`）和"没有安装工具"。

另外，计划 §11 的两个待决问题这次是按"按计划"取了计划里写明的默认处理，不是负责人明确回答。

## 1. 实现了什么（实现证据）

| 部分 | 位置 |
|---|---|
| 候选查询（同类型、仓库未删除、注册表启用、`id` 升序，`LIKE` 用 `ESCAPE '!'`） | `rg-db/src/ops/registry_discovery_ops.rs` |
| 先鉴权再分页的发现服务 | `rg-core/src/package_registry/discovery.rs` |
| REST：`GET /api/v1/skills`、`GET /api/v1/mcp-servers` | `rg-http/src/api/registry_discovery.rs`；路由在 `rg-http/src/lib.rs`；元数据在 `rg-core/src/auth/route_meta.rs`；OpenAPI 已登记 |
| 5 个只读 MCP 工具 | `rg-mcp/src/tools/mod.rs`：`search_skills`、`resolve_skill`、`get_skill`、`search_mcp_servers`、`get_mcp_server` |
| 能力标志 | `api_capabilities` 增加 `registry.discovery.v1` |
| P1b 缺陷修复 | `mcp_service.rs`：版本元数据存 `description`，列表/详情取最新版本的 |
| 测试 | `rg-db`：1；`rg-http/tests/registry_discovery_tests.rs`：11；`rg-mcp/tests/read_tools_mcp.rs`：新增 6 |

要点：
- **先鉴权再分页**：按 package id 升序走候选，逐项 `can_read_repo`（与其他读取同一个判定，含 agent 的所有者上限），攒满 `limit` 为止；不是先取一页再过滤。不可读项被跳过，不留占位，响应里没有总数（顶层只有 `entries` 与 `next_cursor`，有测试断言）。
- **短页**：一次请求最多看 200 个候选。没攒满就返回短页，**仍带 `next_cursor`**，调用者续读即可。这是唯一能从响应里看出"后面还有候选"的地方，和该调用者看到注册表里有任何条目时得到的信息相同。
- 参数：`limit` 1–100（默认 20，超过是 400 `INVALID_LIMIT`，不静默截断），`q` ≤200 字符（`INVALID_QUERY`），坏 `cursor` 是 400 `INVALID_CURSOR`。`q` 是名称/描述的大小写不敏感子串，`%`、`_` 按字面处理。
- 匿名调用者只见公开仓库；已删除仓库的条目对谁都不出现（包括所有者）；没有任何未撤回版本的条目不列出；停用的注册表（`enabled = false`）不进发现。
- 每项：owner/repo/name、描述、最新未撤回版本、摘要（skill：`content_digest`、`archive_digest`、`source_verified`；MCP：`digest`、`ownership: "unverified"`）、`publisher`（最新版本的发布者，`ActorRef`，账号已删为 `null`）、`updated_at`、`origin: user_content`。列表里的摘要与同一版本 `resolve` 返回的一致（有测试）。
- 仓库白名单令牌被拒：白名单只放行 `/repos/{owner}/{name}` 路径，所以全局路径一律 403（与 `/search` 同理），有测试；不受限的 `repo:read` 令牌可用。
- MCP 工具：路径参数在发请求前校验（skill 名 `[a-z0-9-]` ≤64；MCP 名 `namespace/server` 两段；`owner`/`repo`/`version` 同现有规则），非法参数**不发任何请求**（有测试）。`get_*` 不给版本就先 resolve，并在结果里写明解析到了什么。没有安装或发布工具；工具描述里写明文字是发布者写的（`origin: user_content`），测试逐个检查。

## 2. 验证（任务验证，本机，提交前的工作树）

| 层 | 命令 | 结果 |
|---|---|---|
| SQLite 全工作区 | `cargo test --workspace --no-fail-fast` | 176 个测试目标，**1791 passed，0 failed，16 ignored**（ignored 为既有；比 P1b 的 1773 多 18 项新测试） |
| PostgreSQL 17（Docker） | 临时脚本跑 `cargo test -p rg-http --no-fail-fast --test mcp_servers_registry_tests --test skills_registry_tests --test registry_discovery_tests` | 11 + 11 + 11 passed |
| MySQL 8.4（Docker） | 同上 | 11 + 11 + 11 passed |
| Clippy | `cargo clippy --workspace --all-targets -- -D warnings` | 干净（仅既有的 `workspace.package.homepage` 清单警告） |
| 格式 | `cargo fmt --all` | 已应用 |

PostgreSQL/MySQL 的结果来自一次性脚本；`scripts/ci-db-tests.sh` 的清单加了 `registry_discovery_tests`，**没有用该脚本跑过**。

**测试给出的几次真实反馈：**
- 第一次在 PostgreSQL / MySQL 上跑，11 项里 9 项失败：这些测试默认"库里只有我发布的条目"，共享数据库里别的测试发布的条目也出现在匿名列表里。SQLite 每个测试独立库所以看不出。已改成每个测试用一个唯一标记命名条目并只查该标记，分页/可见性的断言不变。
- `route_metadata_tests` 的"派生决定等于旧手写表"失败：`/skills` 不在任何路由族里，旧规则给 `admin`，而发现接口只该要 `repo:read`。把 `/skills`、`/mcp-servers` 加进 `route_meta::family_scope` 的仓库族，并同步更新该测试里冻结的旧表。
- MCP 工具描述测试发现 `search_mcp_servers` 的描述没有写明"文字是发布者写的"，只说"同 search_skills"，看这一条描述的模型读不到警告。已补上，测试现在逐个工具检查。
- clippy 报 `contains_key` 后 `insert`，已改。

## 3. 没有验证的

- **没有 CI。** 分支没有推送，也依赖未合并的 #7、#8。
- **没有真实 MCP 客户端、没有 `ironforge-mcp --http`、没有真实服务端 + 真实 `ironforge-mcp` 的端到端**（计划 §9 的手工脚本没写）。MCP 工具只用记录路由的桩服务测过"调了哪条路由、参数怎么编码、非法参数不发请求"；路由本身另由 HTTP 集成测试覆盖，两段没有在同一次运行里串起来。
- **权限收回的时效没有测**：`can_read_repo` 对私有仓库有 30 秒缓存，所以刚收回读权限的人最多还能在发现里看到条目 30 秒，与其他读取一致。没有测，也没有绕过缓存。
- **没有负载测试**：每次请求最多扫 200 个候选，每个可读候选一次版本查询；匿名也能调用，是否需要更严的限流没有评估（现有全局限流中间件照常适用）。
- 没有并发发布时发现接口读到半写入状态的专门测试。
- 搜索只有 `LIKE`：大表上无法走索引（前导通配符），没有评估规模；也没加索引迁移。
- 没有测组织所有的仓库在发现里显示组织名（代码里 `org_id` 有值时取组织名，未单独断言）；已删除账号作为发布者显示 `null` 也没有单独测。
- 没有测 CI 作业令牌调用发现接口（按现有 `extract_user_id` 语义它被当作匿名）。

## 4. 已知限制与风险

- **见 §0**：没有审批门。公开仓库里的 agent 发布会被展示给所有人。
- 发现是对外广告：条目文字是发布者写的，任何 agent 都不应据此自动安装或运行；`resolve` 的摘要进锁文件、`get_*` 返回的 `content_digest`/`digest` 应与锁文件核对。
- 排序是发布先后，没有相关度；同一查询在三种数据库上顺序相同。
- 每仓库读取（P1/P1b）不看 `package_registry.enabled`，而发现看；两者不一致，没有统一。
