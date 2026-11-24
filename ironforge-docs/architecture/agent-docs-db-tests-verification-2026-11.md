# 过期文档与 DB 测试目标列表补齐（验证记录，2026-11-09）

> **注（2026-11-24）**：文中"没在 git233 Actions 上跑"属于当时的验证边界。git233 已移除，CI 现在全部在 GitHub 托管 runner 上。

> 基线 main `04433f8`。本任务**不改 Rust 代码**，只让 `CLAUDE.md` 与 `scripts/ci-db-tests.sh` 与已合并的实现一致。

## 一、改动

### 1. `CLAUDE.md`「Agent 令牌 scope 阶梯」行里两句过期表述

| 原句 | 现句 |
|---|---|
| 同仓 PR 的 `refs/pull/<n>/head`（…；**fork PR 没有**） | 同仓与 fork PR 的 `refs/pull/<n>/head`（…；**fork 的对象复制进目标仓库**，见 `publish_head_ref`） |
| **仍未做**：SSE、**fork PR 的 `refs/pull`**。服务端仍**不支持**通过 git push 删分支（`deletion not supported`）**也没有删分支 REST**。 | **仍未做**：SSE（事件流）。删分支有 REST `DELETE /repos/{o}/{n}/branches/{branch}` 与 MCP `delete_branch`（拒绝默认分支、受保护分支、有未关闭 PR 的分支，`expected_sha`/`dry_run`），但 `git push --delete` 仍被 receive-pack 拒绝。开 PR 时的 base 提交记在不可变的 `pull_request_opened` 事件里（`anchors.base_sha_when_opened`，之前开的 PR 为 null）；带 `expected_head_sha` 的合并由 `merge_pr_expecting_head` 钉死该提交（head 已前进则 409 `REF_ADVANCED`）。 |

只改这两处，行内其余内容未动（该行很长，重写会丢信息）。

依据（实现侧只读核对，未改）：

- `rg_core::pull_request::publish_head_ref` — `crates/rg-core/src/pull_request/service.rs:1335`。
- REST 删分支 — `crates/rg-http/src/api/repo_content.rs:641`（`DELETE /repos/{owner}/{name}/branches/{branch}`，审计 `repo.branch_delete` 在 `:714`）。
- MCP 删分支 — `crates/rg-mcp/src/tools/mod.rs:150`（声明）、`:278`（分发）、`:729`（实现）。
- `base_sha_when_opened` — `crates/rg-http/src/api/pr_export.rs:99/143/145`。
- `HeadMoved` — `crates/rg-core/src/pull_request/service.rs:1183/1423/1479`，REST 映射 409 在 `crates/rg-http/src/api/pulls.rs:667`。

### 2. `scripts/ci-db-tests.sh` 的 agent 目标列表

```diff
 for t in pat_ref_allowlist_tests file_commit_semantics_tests idempotency_tests \
          repo_events_tests pr_context_tests pr_export_tests branch_delete_tests \
-         fork_pr_ref_tests; do
+         fork_pr_ref_tests merge_pinned_head_tests repo_settings_tests wiki_tests; do
```

新增三个（都是真调 `spawn_test_app_on_env_db`、此前不在任何列表里）：`merge_pinned_head_tests`、`repo_settings_tests`、`wiki_tests`。

头部注释同步改写：说明第 3 步的 `collab_*` 循环也走真后端（`collab_common::Env` 读 `IRONFORGE_TEST_DATABASE_URL`），并指向 `grep -l spawn_test_app_on_env_db crates/rg-http/tests/*.rs` 作为权威清单。

### 3. **没有**加 `collab_health_tests`

原任务要求「补上 `collab_health_tests`，若与 `all` 重复则只留一处」。核实结果是**只留 `all` 里那一处**：

- `crates/rg-http/tests/collab_health_tests.rs` 通过 `collab_common::Env` 建库，而 `crates/rg-http/tests/collab_common/mod.rs:37` 读 `IRONFORGE_TEST_DATABASE_URL` —— 也就是说 `all` 的第 3 步（`for f in crates/rg-http/tests/collab_*_tests.rs`）**已经在真后端上跑它**。
- 该文件不调用 `spawn_test_app_on_env_db`（`grep -c` = 0），放进第 4 步的固定列表只会在 `all` 下重复跑一遍，对 `agent` 子集也没有新增覆盖类型。

### 4. `.ai/README.md` 核对结果：无需改动

- `:19`/`:24` 远程 MCP（`ironforge-mcp --http`、`POST /mcp`、请求头带令牌、`IRONFORGE_PAT` 在 `--http` 下不用）与 `rg-mcp/src/remote.rs` 一致。
- `:164` `delete_branch`（`DEFAULT_BRANCH`/`BRANCH_PROTECTED`/`OPEN_PULL_REQUESTS` 三个 409 `reason`、`expected_sha` → `REF_ADVANCED`、`dry_run`、受限令牌只能删范围内分支、`git push` 删分支仍被拒）与实现一致 —— `git push` 那句现在成立，B4 落地后需要再改。
- `:167` `export_pr` 的锚点（含 fork PR 对象复制、`base_sha_when_opened` 与 `anchors.not_stored`）与 `pr_export.rs` 一致。

## 二、验证

### 任务验证（静态）

| 检查 | 结果 |
|---|---|
| `bash -n scripts/ci-db-tests.sh` | 通过（无语法错误） |
| 列表与实现对齐 | `diff <(脚本里的 for 列表 \| sort) <(grep -l spawn_test_app_on_env_db crates/rg-http/tests/*.rs \| basename \| sort)` → 无差异（11 项完全一致） |
| Rust 代码改动 | 无。因此没有跑 clippy/fmt（它们检查的对象没变）；代价是本记录不能为 Rust 侧背书 |

### 后端验证（真实 PostgreSQL 17 / MySQL 8.4，Docker，每个目标全新库）

`scripts/ci-db-tests.sh postgres agent` 与 `mysql agent`，两个后端均 `exit=0`，11 个目标 55 项测试全通过：

| 目标 | PostgreSQL 17 | MySQL 8.4 |
|---|---|---|
| `pat_ref_allowlist_tests` | 7 passed | 7 passed |
| `file_commit_semantics_tests` | 5 | 5 |
| `idempotency_tests` | 5 | 5 |
| `repo_events_tests` | 5 | 5 |
| `pr_context_tests` | 4 | 4 |
| `pr_export_tests` | 7 | 7 |
| `branch_delete_tests` | 5 | 5 |
| `fork_pr_ref_tests` | 2 | 2 |
| `merge_pinned_head_tests`（新） | 4 | 4 |
| `repo_settings_tests`（新） | 2 | 2 |
| `wiki_tests`（新） | 9 | 9 |
| 合计 | **55 passed / 0 failed** | **55 passed / 0 failed** |

三个新目标此前从未在 PG/MySQL 上跑过，本次是它们第一次的真后端证据（例：`repo_settings_tests` 的并发设置写入、`wiki_tests` 的版本号前进，都依赖数据库语义）。

## 三、未覆盖

- **没有在 git233 Actions 上跑过**：上面全部是本机 Docker 的结果。CI 里这两个后端是否可用、是否与本地同版本，本记录不回答。
- **只跑了 `agent` 子集，没跑 `all`**：`all` 的 `rg-core` 冒烟、`rg-db`/`rg-http` 的 `collab_*` 循环本次未重跑（本次改动不影响它们的执行方式：第 3 步的循环一行未动）。
- **其余 rg-http 目标仍只用 SQLite**：除这 11 个之外的目标不调 `spawn_test_app_on_env_db`，本任务不扩范围。哪些值得改成也读环境变量，留给 A2（协作套件 PG/MySQL 补验证）判断。
- **`capabilities` 的 `mcp_transport.transports` 仍广告 `["stdio"]`**：远程 HTTP 传输已上线但注册表未变（`rg-http/src/api/collab_capabilities.rs:132`，`collab_contract_tests.rs:434` 把它钉死）。这是**已知偏差**，本次只确认它已被 `CLAUDE.md` 的 MCP 远程传输段记录，未改代码、未改注册表 —— 要加 `"http"` 是契约变更，需要单独决定。
- **`CLAUDE.md` 里其它「仅 SQLite」字样**（协作人类通知、协作 Web UI 等）本次未动：本次没有为它们取得 PG/MySQL 证据，等 A2 的真实结果再改。
