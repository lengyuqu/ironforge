# Agent 令牌：分支白名单（验证记录，2026-11-06）

> **状态**：已实现，SQLite 全量验证，PostgreSQL/MySQL 只验证了迁移 ｜ **可信度**：确认（附运行证据）｜ **来源**：仓库代码 + 本次运行
> 接 [agent-token-repo-allowlist-verification-2026-11.md](agent-token-repo-allowlist-verification-2026-11.md)（仓库白名单）和 [agent-token-scopes-verification-2026-11.md](agent-token-scopes-verification-2026-11.md)（scope 阶梯）。设计见 [harness 原生化设计](harness-native-design-2026-11.md) §4.4 的 `ref_prefixes`。**令牌身份贯穿到下游、审计里的令牌身份、按分支的服务端文件提交没有做。**

## 做了什么

令牌可以限制只能**写**哪些分支：`POST /api/v1/users/tokens` 新增 `refs: ["agent/*", …]`（可省略，省略 = 不限，旧令牌全部不变）。

- **存储**：`access_tokens.ref_allowlist`，可空 TEXT，内容是 JSON 数组。**新增迁移** `m20261106_000002_add_access_token_ref_allowlist`（表名显式为 `access_tokens`；`up` 幂等，`down` 为空）。按护栏，新增迁移和修改 `rg-core/src/auth/` 都需确认，项目负责人已确认本方案。
- **创建时校验**：
  - 必须同时带 `repos`（分支限制只在仓库限制之内才有意义），因此也只能带仓库类 scope。
  - 每项是分支名模式，`*` 匹配任意一段字符（与分支保护同一个匹配函数，`rg_core::branch_protection::service::ref_matches_pattern`，`*` 可以跨 `/`）。
  - 只允许字母、数字和 `- _ . / *`；必须以字面字符开头，所以 `*`、`*/x` 这类能放开所有分支的写法被拒；不允许空段、`.`/`..` 段、`..`、首尾 `/`、结尾 `.` 或 `.lock`、单个模式超过 200 字符；其他 glob 语法（`? [ ] { }`）被拒。
  - 去重并排序后存储；最多 20 个；**空数组被拒绝**。
- **强制点**：
  - **git push（HTTP）**：`receive-pack` 新增 `writable_refs` 参数，更新的 ref 不在 `refs/heads/<模式>` 里一律以 `error` 拒绝，消息 `this access token may not write to this ref`。tag、`refs/notes` 等其他命名空间也在限制之外。多个 ref 的一次推送里，名单内的会成功、名单外的被拒（和分支保护的行为一致，不是原子的）。拒绝发生在读 pack 之前（全部被拒时直接丢弃 pack）。
  - **REST 写请求**：PAT 中间件对带分支限制的令牌，只放行**不会移动分支或 tag** 的写：`collaboration/**`、`issues/**`、创建 PR（`POST pulls`）、`PATCH pulls/{n}`、PR 评论与评论解决、提交评审、增删评审人。其余写请求（`contents`、`merge`、`auto-merge`、`merge-queue`、`suggestion(s)/apply`、`releases`、`wiki`、`mirror`、`pipelines`、`statuses`、评审 `dismiss`、`hooks`、分支保护、协作者、`lfs`、`fork` 等）一律 403；以后新增的路由默认也被拒，直到加进 `ref_limited_write_allowed`。读不受影响。
  - 原因：REST 处理函数拿到的是 PAT 中间件换出来的完整 JWT，不知道是哪个令牌发的请求，没法在处理函数里按分支判断；所以收窄成白名单，而不是在每个会动 ref 的处理函数里逐个加检查。
- **比较规则**：存储的名单解析失败时按"空名单"处理，即**什么分支都不能写**，损坏的限制不会变成"不限"。
- 令牌列表和创建响应带 `refs`；Web 令牌表单新增"分支（可选）"，列表显示"分支：…"。
- Agent 的流程因此是：推送 `agent/*` 分支 → 开 PR → 由人合并。

## 它**不**做什么（重要）

- **不限制读**：clone/fetch、读文件、读 PR 都照常。
- **不是按分支的服务端提交**：受限令牌不能用 REST 的文件编辑（`contents`），哪怕目标是 `agent/*`。要做 `commit_files` 这类工具，需要把令牌身份带进 JWT，再让 `check_direct_commit_allowed` 按分支检查——这是下一步，没有做。
- **评审不受限**：受限令牌可以提交评审（包括批准）。令牌属于某个账号，评审以该账号名义产生；批准是否算数由现有分支保护规则决定，本次没有改。如果要让 Agent 的批准不计入必需批准数（设计文档 §4.4.3 的 `human_approval_required`），是单独的工作。
- **PR 本身可以由受限令牌创建、编辑、评论**，但 PR 的 head 分支是否在名单内不检查（创建 PR 不移动 ref）。
- 仍然用完整身份的 JWT 访问下游，限制只在中间件和 git 入口这一层；SSH 不使用令牌，所以不受此限制。
- 审计日志里仍没有令牌身份。
- 新增会移动 ref 的 REST 路由不需要改限制代码（默认被拒）；新增想让受限令牌用的写路由则必须加进 `ref_limited_write_allowed`。

### 顺带发现（未改）

`rg-http/src/git_v2.rs::handle_git_receive_pack_v2` 调用的是不带任何拒绝规则的 `handle_receive_pack_http`，既不检查分支保护，也不检查令牌范围。它**没有挂到任何路由上**（全仓库只有定义），所以现在没有影响；但一旦被接到路由，它就是绕过分支保护和这套限制的入口。建议删除它，或接入前改成和 `handle_receive_pack` 同样的检查。

## 证据

| 层级 | 结果 |
|---|---|
| 任务验证（SQLite） | `rg-core` 单测 13 项（含新增 4 项）：模式校验与规范化（空、`*`、前导 `*`、`..`、空段、`.lock`、空格、反斜杠、NUL、`{} ? []`、超长、超过 20 个）、损坏的存储值不放行任何分支、只有名单内分支可写且 tag/其他命名空间/近似名都不行、受限令牌的 REST 写白名单（含一批应拒绝的路径和近似前缀）；`pat_ref_allowlist_tests` 4 项（真实 HTTP、真实 `git`）：创建时校验与列表显示；真实 `git push` 到 `agent/one`、`agent/nested/two` 成功，到 `main`、`agent`、`agentx/one`、`other/agent/one`、`refs/tags/v1`、`refs/tags/agent/one`、`refs/notes/commits` 全部被拒且服务器上没有这些 ref，`main` 没动；一次推送带 `agent/three` 与 `main` 时只有 `agent/three` 入库；同 scope 无分支限制的令牌能推 `main`；REST 上受限令牌可读、开 issue、建协作会话、开 PR、评论 PR，而文件新建/修改/删除、合并、auto-merge、merge-queue、应用建议、发布、流水线、状态、webhook、分支保护、wiki、fork 全部 403，同样的文件编辑用无分支限制的令牌成功；只有仓库限制的令牌与完全无限制的令牌行为不变。相邻套件（仓库白名单 5 项、scope 阶梯 7 项、流式 git 推送往返、`rg-db` 27 项含迁移回滚窗口 `Some(20)` → `Some(21)`、`rg-git` 61 项）仍通过。前端：`settings` 目录组件测试 75 项通过（含新增 2 项），i18n 键一致（en/zh-CN 各 2071）。 |
| 运行验证（真实服务 + 真实 `git` + 真实 `ironforge-mcp`，SQLite） | 令牌 `scopes=repo:write`、`repos=[collab-e2e/pair-work]`、`refs=[agent/*]`。`refs` 不带 `repos` 创建返回 400。`git push` 到 `agent/fix-parser` 成功，到 `main`、`agent`、`refs/tags/v1` 被拒（`this access token may not write to this ref`），服务器上只有 `main` 和 `agent/fix-parser`。REST：开 PR、评论 PR、开 issue 都是 201；文件编辑、合并 PR、发布各 403；读目录 200。MCP：`collab_start_session`、`collab_create_task`、`read_dir` 正常。 |
| 迁移 | 只在 SQLite 验证（迁移回滚窗口测试）。列定义与上一个迁移相同（可空 TEXT、显式表名、幂等 `up`），PostgreSQL/MySQL 未单独跑。 |
| 入口排查 | 见仓库白名单文档：令牌只在 `resolve_pat` 解析，调用方是 REST 中间件、git over HTTP、CI 日志 WebSocket。分支限制在前两处生效；WebSocket 是只读，且受限令牌（必有 `repos`）已被整体拒绝。 |

## 未覆盖

- PostgreSQL/MySQL 没有跑 PAT 的 HTTP 测试，也没有跑这次的迁移。
- 没有跑 `scripts/git-protocol-e2e.sh`、浏览器里点一遍令牌表单。
- SSH 不使用令牌，所以没有测。
- CI 未看。
