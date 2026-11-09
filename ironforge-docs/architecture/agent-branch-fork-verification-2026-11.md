# 删除分支、fork PR 的 `refs/pull`、PR 开时的 base 提交（验证记录，2026-11-09）

> 接 [agent-gaps-verification-2026-11.md](agent-gaps-verification-2026-11.md)。那一篇「未做」清单里的三项在这里补完：服务端删分支、fork PR 的 `refs/pull`、`base_sha_when_opened`。**邮件通知偏好按负责人安排暂不做。远程 MCP 传输在另一个分支，不在本文。**

## 一、做了什么

### 1. 删除分支：`DELETE /repos/{o}/{n}/branches/{branch}` 与 MCP `delete_branch`

`branch` 可以带斜杠（`agent/pass`）。需要仓库写权限。拒绝都是 409，带稳定的 `error.reason`：

| 情形 | reason | 说明 |
|---|---|---|
| 默认分支 | `DEFAULT_BRANCH` | 先改默认分支 |
| 保护规则覆盖（含通配 `release/*`，与推送用同一个匹配函数） | `BRANCH_PROTECTED` | `details.rule` 是命中的规则 |
| 仍有未关闭 PR 使用（作 head，含 fork PR 的 head；或作 base） | `OPEN_PULL_REQUESTS` | `details.pull_requests` 列出 `{repo_id, number}` |
| `expected_sha` 与当前不符，或检查后分支又动了 | `REF_ADVANCED` | `details.current_sha`；删除本身是 compare-and-swap（`rg_git::ops::delete_ref_if`） |

其余：分支不存在 404；名字不合法（空格、`~^:?*[\`、`..`、`.lock` 结尾等）400；`dry_run`（查询参数或 `X-Dry-Run: 1`）跑完全部检查但不删。已合并/已关闭 PR 的 `refs/pull/<n>/head` 不受影响（测试验证）。成功后写审计 `repo.branch_delete`（带 `access_token_id`）并发 `branch.deleted` webhook / 事件日志。

**令牌**：受分支限制的令牌只能删限制范围内的分支（处理函数里 `ensure_token_may_write_branch`），所以 `ref_limited_write_allowed` 新增了 `branches/<名字>`，**`branches/protection/**`（保护规则设置）仍然拒绝**。一个字面叫 `protection/...` 的分支因此无法用受限令牌删除，也无法用这条路由删除（静态路由优先）——可接受的已知限制。

**没做**：`git push --delete` 仍被 receive-pack 拒绝（`deletion not supported`），没有第二个删除入口。MCP `delete_branch` 不带幂等键（重复删除得到 404，本身就明确）。

### 2. fork PR 的 `refs/pull/<n>/head`

新增 `rg_core::pull_request::publish_head_ref`：在**目标仓库**写 `refs/pull/<n>/head`；fork PR 先把 fork 的对象复制进目标仓库（复用 `copy_objects_from_source`，按目标已有提交剪枝）。创建 PR 与推送到 head 分支两处都改用它。

**顺带修了一个潜伏的 bug**：此前推送到 fork 的 head 分支时，钩子把 `refs/pull/<n>/head` 写进**被推送的仓库（fork）**，而 `<n>` 是目标仓库的 PR 编号——既写错地方，也可能覆盖 fork 自己第 `<n>` 个 PR 的锚点。测试断言 fork 里没有这个 ref。

创建 PR 的 `dry_run` 现在对 fork PR 也报告 `publishes_pull_ref: true`；`export` 的 `anchors.head_ref` 对 fork PR 不再是 `null`。**此前已存在的 fork PR 没有回填**，要到它的 head 下一次被推送才会有 ref。

### 3. `base_sha_when_opened`

PR 创建时的 base 分支尖端记进不可变的 `pull_request_opened` 事件元数据（`base_sha`），不加列、不迁移。`export` 的 `anchors.base_sha_when_opened` 读它；记录功能上线前开的 PR 为 `null`，`not_stored` 列出它，开放 PR 与已合并 PR 的其他锚点（`base_tip_sha`、`base_sha_at_merge`）不变。

## 二、证据

| 层级 | 结果 |
|---|---|
| 任务验证（SQLite） | `branch_delete_tests` 5 项（真实 HTTP + 真实 git）：未关闭 PR 拦住 → 关闭后删成功 → `refs/pull/1/head` 仍可 `ls-remote` → 再删 404；默认/保护（先建分支再加规则）/不存在/不合法；`expected_sha` 过期 409 且带当前提交、dry-run（查询与头）不删、对的 sha 能删；匿名 401、陌生人 403；受限令牌删范围外分支 403、删保护规则设置 403、删范围内成功、审计带令牌 id。`fork_pr_ref_tests` 2 项：fork PR 的 ref 出现在目标仓库且对象可读（`cat-file -t` 是 commit）、fork 里没有该 ref、fork 的 head 分支推进后目标里的 ref 跟着走、export 有锚点；PR 记住开时的 base（base 分支之后前进，`base_sha_when_opened` 仍是旧值，`not_stored` 为空）。**fork 的那条在不带修复时失败**（已用 stash 验证）。单元：`valid_branch_name`、`ref_limited_write_allowed`（含 `branches/protection` 拒绝）。MCP：`delete_branch` 的 schema、参数校验、路径 `branches/agent/x?expected_sha=…&dry_run=true`。 |
| 全量（SQLite） | `cargo test -p rg-http -p rg-core -p rg-db -p rg-mcp -p rg-git --no-fail-fast`：129 个测试程序、1127 项通过、0 失败、11 项 ignored；`cargo clippy … --all-targets -- -D warnings` 干净，`cargo fmt --check` 干净。 |
| 后端验证（PostgreSQL 17 / MySQL 8.4，真实容器） | `scripts/ci-db-tests.sh postgres agent` 与 `mysql agent`（每个目标全新数据库）：8 个目标全部通过，含新增的 `branch_delete_tests` 5 项、`fork_pr_ref_tests` 2 项（该子集已把这两个目标并入）。第一次在真实数据库上跑时 `branch_delete_tests` 有 1 项失败，是**测试**写得不对：这两个后端上同一文件的测试共用一个库，审计查询取到了别的测试的记录，已按用户名过滤；SQLite 每个测试一个库，看不出这类问题。 |

## 三、未覆盖 / 已知限制

- 没有在 git233 的 Actions 上跑过；PR 上的 `rg-http` 集成测试现在只有 `rg-http` 自己的文件变了才跑（本 PR 改了 `rg-http`，所以会跑）。
- 受限令牌的真实端到端（真实 `ironforge-mcp` + 真实服务调 `delete_branch`）没有写进 `scripts/agent-e2e.py`。
- 镜像仓库上 `refs/pull/*`（含 fork PR 的）仍会在下一次同步被清掉（见上一篇）。
- 已存在的 fork PR 没有回填 `refs/pull`；开放 PR 的 `base_sha_when_opened` 对旧 PR 为 `null`。
- 删除分支只看「用这个分支作 head（含以本仓库为源的 fork PR）或作 base 的未关闭 PR」；其他把它当依据的东西（CI 配置里引用的分支名、外部链接）不检查。
- 单个仓库没有保留期/回收站：删除的分支只能靠 `refs/pull/<n>/head`（有 PR 的）或知道提交 id 找回。
