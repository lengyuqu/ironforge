# Agent 令牌：仓库白名单（验证记录，2026-11-06）

> **状态**：已实现，SQLite 全量验证，PostgreSQL/MySQL 只验证了迁移 ｜ **可信度**：确认（附运行证据）｜ **来源**：仓库代码 + 本次运行
> 接 [agent-token-scopes-verification-2026-11.md](agent-token-scopes-verification-2026-11.md)（scope 阶梯）。设计见 [harness 原生化设计](harness-native-design-2026-11.md) §4.4。**可写分支前缀、令牌身份贯穿到下游、审计里的令牌身份没有做。**

## 做了什么

令牌可以限制到若干仓库：`POST /api/v1/users/tokens` 新增 `repos: ["owner/name", …]`（可省略，省略 = 不限，旧令牌全部不变）。

- **存储**：`access_tokens.repo_allowlist`，可空 TEXT，内容是 JSON 数组。**新增迁移** `m20261106_000001_add_access_token_repo_allowlist`（表名显式为 `access_tokens`；`up` 幂等，`down` 为空）。按护栏，新增迁移和修改 `rg-core/src/auth/` 都需确认，项目负责人在选择该范围时确认。
- **创建时校验**：每项必须是 `owner/name`，只允许字母、数字、`-`、`_`、`.`，拒绝 `.`/`..`；去重并排序后存储；最多 50 个；**空数组被拒绝**（要表达"不限"请省略该字段）；带 `repos` 的令牌**只能带仓库类 scope**（`repo`、`repo:write`、`repo:collab`、`repo:read`），带 `user` 或 `admin` 会被 400 拒绝——这样受限令牌既不能访问账号/管理 API，也不能签发别的令牌。不检查仓库是否存在。
- **强制点**：
  - REST（PAT 中间件）：只放行 `/repos/{owner}/{name}[/…]` 和 `/ai/repos/{owner}/{name}[/…]` 且 `owner/name` 在名单内；仓库族下的其余一切都拒绝（`/repos` 的列表与创建、`/search`、`/orgs`、`/imports`、按 id 寻址的 `/ci`、`/artifacts`、`/ws`）。名单内的仓库里，通往**其他**仓库的路由也拒绝：`fork`、`forks`、`transfer`、`collaboration/groups/**`（跨仓协调组）。
  - git over HTTP（`info/refs`、`upload-pack`、`receive-pack`）：处理函数已知 `owner/repo`，名单外一律拒绝。
  - CI 日志 WebSocket（按 job id 寻址，无法对应仓库）：受限令牌一律拒绝。
- **比较规则**：`owner/name` 精确、区分大小写；空段、`.`、`..` 段一律拒绝；存储的名单解析失败时按"空名单"处理，即**什么都不放行**，损坏的限制不会变成"不限"。
- 令牌列表和创建响应带 `repos`；Web 令牌表单新增"仓库（可选）"，列表显示"仅限 …"。

## 它**不**做什么（重要）

- **只限制路径所指的仓库**。请求体里引用其他仓库的路由，只有上面列出的几条被显式拒绝；新增这类路由时必须同步加入 `path_in_allowlist` 的拒绝表，否则受限令牌可借它触及名单外的仓库（只要令牌主人对那个仓库有权限）。
- **不限制分支/ref**：名单内的仓库里，令牌仍可推任何分支（受其 scope 和主人权限约束）。
- 令牌仍被换成**完整身份的 JWT**，限制只在中间件和 git 入口这一层；绕过 PAT 中间件的入口不受限（已知入口 REST、git over HTTP、CI 日志 WebSocket 均已覆盖，SSH 不使用 PAT）。
- 受限令牌不能列仓库（`/repos/{owner}` 和 `/repos/explore` 都在名单外），所以 MCP 的 `list_repos` 对它会被拒绝（读写 `owner/repo` 的工具正常）。
- 审计日志里仍没有令牌身份（要有，需要把令牌身份带到下游）。

## 证据

| 层级 | 结果 |
|---|---|
| 任务验证（SQLite） | `rg-core` 单测 9 项（含新增 4 项）：名单校验与规范化（空、无斜杠、`..`、空格、编码、NUL、通配、上限 50）、损坏的存储值不放行、路径精确匹配（大小写、前缀、近似、空/点段、不绑定仓库的路由）、名单内通往其他仓库的路由被拒；`pat_repo_allowlist_tests` 5 项（真实 HTTP、私有仓库）：创建时校验与规范化与列表显示、受限令牌读写名单内仓库而在同一用户的另一个仓库被 403 且无限令牌同样请求成功、全局/组织/搜索/导入/账号/签发令牌/fork/forks/协作组都 403、git `info/refs` 的 fetch 与 push 都只对名单内仓库放行、无名单令牌行为不变；`rg-db` 全部通过（迁移回滚窗口测试 `Some(19)` → `Some(20)`）；scope 阶梯测试和相邻 PAT 套件仍通过；前端 960 通过 |
| 迁移（PostgreSQL 17 / MySQL 8.4，真实容器） | `multi_backend_smoke` 在两个后端通过；`access_tokens.repo_allowlist` 存在，`text`、可空 |
| 运行验证（真实服务 + 真实 `ironforge-mcp`，SQLite） | 令牌 `scopes=repo:collab`、`repos=[collab-e2e/pair-work]`，同一用户另有私有仓库 `secret-repo`。名单内仓库：`collab_start_session`、`collab_create_task` 成功；名单外仓库：`collab_start_session` 被 403 `personal access token scope denied`，`read_file` 被 403；`list_repos` 被 403；直接 REST：名单内仓库 `GET` 200、名单外 403、用该令牌签发 `admin` 令牌 403 |
| 入口排查 | 令牌只在 `resolve_pat`（`rg-http/src/lib.rs`）一处按哈希解析，其调用方只有三处——REST 中间件（`api_v1` 与文档路由）、git over HTTP 的三个处理函数、CI 日志 WebSocket——都已执行 scope 与白名单。OCI 的 `/v2/token` 只接受用户名+密码，OCI 的 Bearer 只接受用户 JWT 和 OCI 令牌，**不接受 PAT**，所以没有绕过；LFS、包注册表在 `api_v1` 下，受同一中间件约束（LFS 的 `POST …/lfs/objects/batch` 在下载时也是 POST，只读令牌因此不能批量下载 LFS，这是按方法判定"宁可拒绝"的已知代价） |

### 运行验证中发现的无关旧问题

MCP 的 `read_dir` 请求了不存在的路由（落到前端兜底页，返回 200 的 HTML）、`read_file` 得到 405、`list_repos` 请求了不存在的 `GET /repos`，读类工具还把失败放进正常结果。这些与令牌无关，已在随后的修复中处理，见 [mcp-read-tools-verification-2026-11.md](mcp-read-tools-verification-2026-11.md)。

## 未覆盖

- PostgreSQL/MySQL 只验证了迁移和既有冒烟，没有在这两个后端跑 PAT 的 HTTP 测试。
- 没有用真实 `git` 客户端 clone/push（只验证 `info/refs` 的授权判定）。
- CI 未看。
