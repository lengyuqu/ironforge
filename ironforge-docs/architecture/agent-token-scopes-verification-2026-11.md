# Agent 令牌：细分的仓库 scope（验证记录，2026-11-06）

> **状态**：已实现，仅 SQLite 验证 ｜ **可信度**：确认（附运行证据）｜ **来源**：仓库代码 + 本次运行
> 对应 [harness 原生化设计](harness-native-design-2026-11.md) §4.4 的第一步。完整的 agent 令牌（仓库白名单、可写分支前缀、只读标记、令牌身份贯穿到下游）**没有**做。

## 为什么先做这个

现状（修改前）：PAT 校验通过后会被换成该用户的 1 小时完整 JWT，下游代码看不到令牌的任何限制；scope 只有 `repo`/`user`/`admin`，`repo` 等于该用户在其所有仓库上的全部读写。把这样的令牌交给 agent，等于交出整个账号的写权限，这使得开放 agent 写工具在安全上站不住。

## 做了什么

`repo` 之外新增三个层级 scope，**每级包含它下面的**：

| scope | 允许 |
|---|---|
| `repo:read` | 读（`GET`/`HEAD`/`OPTIONS`）、`git clone`/`fetch`、WebSocket 日志流 |
| `repo:collab` | 以上 + 写仓库的协作区（`/repos/{owner}/{name}/collaboration/**` 下的非读请求） |
| `repo:write` | 以上 + 其余全部仓库写入，`git push` |
| `repo` | 历史的全量仓库 scope，包含以上全部；旧令牌行为不变 |

- 判定在既有的 PAT 中间件里按 **HTTP 方法 + 路径** 做：读 → `repo:read`；协作区写 → `repo:collab`；其他写 → `repo:write`。`/orgs`、`/ci`、`/imports`、`/search`、`/ai` 等其余仓库族路径同理。**"是不是读"只看方法**：只读语义的 `POST` 会被当作写，宁可拒绝。
- git over HTTP：`upload-pack`/`info/refs?service=git-upload-pack` 需 `repo:read`，`receive-pack`/`info/refs?service=git-receive-pack` 需 `repo:write`；CI 日志 WebSocket 需 `repo:read`。
- 协作路径识别要求 `repos/{owner}/{name}/collaboration` 精确段，且不含 `.`/`..`；名为 `collaboration` 的仓库、`collaborationx`、`../issues` 等相似路径都按普通写入处理。
- 令牌创建接口接受新 scope，存储为固定顺序（`repo,repo:write,repo:collab,repo:read,user,admin`）；可与 `user` 组合，如 `repo:read,user`。账号族（`/users`、`/notifications`）和管理族仍分别需要 `user`、`admin`，仓库族令牌不能借此签发别的令牌。
- **没有**数据库迁移：scope 字段本来就是自由文本。Web 的令牌创建表单加了说明文字。
- 修改了 `rg-core/src/auth/pat_scope.rs`（护栏要求确认，项目负责人在选择该方案时确认）。

## 它**不**做什么

- 不限制到某个仓库，也不限制可写的分支/ref（那需要给令牌加列并把令牌身份带到下游，是完整方案的内容）。令牌仍可访问其主人有权访问的**所有**仓库。
- 令牌被换成的 JWT 仍是完整身份，**限制只在 PAT 中间件和 git 入口这一层**；任何绕过该中间件的入口都不受限（目前已知入口：REST、git over HTTP、CI 日志 WebSocket，均已覆盖；SSH 不使用 PAT）。
- `repo:read` 不等于"看不到敏感内容"：读请求仍受用户自己的权限约束，旧的 `repo` 令牌本来就能读的，新的只读令牌也能读（例如 webhook 配置）。
- 不改变 agent 会话机制：会话仍是标签，不是安全边界。

## 证据

| 层级 | 结果 |
|---|---|
| 任务验证（SQLite） | `rg-core` 单测：阶梯与规范排序；`rg-http` 库内单测 7 项：方法/路径到 scope 的映射、协作路径的精确匹配与近似路径、未知路由 fail closed；`pat_scope_ladder_tests` 7 项（真实 HTTP + 私有仓库）：读、普通写、协作区写、协作路径的相似写法、`git fetch`/`git push`（`info/refs`）、仓库族令牌不能签发令牌或读 `/users/me`、组合 scope 的规范化；既有 `pat_api_tests`、`cli_contract_tests`、`client_metrics_tests`、`jwt_revocation_tests`、`oci_permission_tests`、`repo_access_matrix_tests`、`error_semantics_accounts_tests` 仍通过 |
| 运行验证（真实服务 + 真实 `ironforge-mcp`，SQLite） | `repo:read` 令牌：MCP 的 `read_dir` 成功，`collab_start_session` 被 403 `personal access token scope denied` 拒绝；`repo:collab` 令牌：`collab_start_session`、`collab_create_task`、`collab_list_tasks` 成功，直接 `POST …/issues` 被 403 |

## 未覆盖

- 仅 SQLite；PostgreSQL/MySQL 未跑（本次没有新 SQL，也没有迁移）。
- 未在真实 `git` 客户端上做 clone/push（只验证了 `info/refs` 的授权判定；`upload-pack`/`receive-pack` 的 POST 走同一个 `extract_actor_id`）。
- CI 未看。
- 没有仓库/分支限制、没有 `read_only` 之外的策略、没有审计里的令牌身份：这些是完整 agent 令牌的内容，见设计 §4.4.2–4.4.4。
