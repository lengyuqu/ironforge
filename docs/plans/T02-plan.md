# T02 Plan — MCP 写工具集补 cherry-pick / revert

> Task: [#48] MCP 写工具集补 cherry-pick / revert / unified diff
> Branch: `agent/t02-mcp-remaining`；基线 `5509c4c`

## 结论先行

服务端在 `f21517b` 已有 cherry-pick / revert 两个写端点；本任务**只补 MCP 暴露**，不新增服务端能力、不改路由注册。

`#48` 第三项「unified diff 输入」**没有**对应服务端端点（详见 `T02-task.md`「未实现项」），因此 MCP 侧不实现，也不自造端点。

## 先读事实（决定实现形状）

| 事实 | 来源 | 对 MCP 工具的影响 |
| --- | --- | --- |
| 路由 `POST /repos/{owner}/{name}/commits/{sha}/cherry-pick`、`…/revert` | `crates/rg-http/src/lib.rs:1404` | 工具方法 POST，路径含 `sha` |
| 请求体 `PickCommitRequest { branch, expected_sha, mainline?, message?, dry_run? }`，`branch`/`expected_sha` 无默认值 | `crates/rg-http/src/api/repo_pick.rs:32` | 两者必填，**不回退 HEAD、不创建分支** |
| `sha` 只接受 4–64 位十六进制，不接受 revspec；否则 404 | `crates/rg-git/src/pick.rs:225`、`:255` | `sha` 在本地按同一规则校验（同时保证 URL 路径安全） |
| `expected_sha` 用完整 object id 与 branch tip 比较，短 id 永不相等，只会得到 409 `REF_ADVANCED` | `crates/rg-git/src/pick.rs:246` | `expected_sha` 要求 40 或 64 位十六进制全 id，提前给出清晰错误 |
| 非 merge commit 传 `mainline` → 400 `MAINLINE_INVALID`；merge commit 不传 → 400 `MAINLINE_REQUIRED`；`mainline` 为 1-based | `crates/rg-http/src/api/repo_pick.rs:351`、`crates/rg-git/src/pick.rs:274` | `mainline` 可选，仅校验为正整数（1-based），父提交数量由服务端判定 |
| `message` 若给出则不得为空白 | `crates/rg-core/src/repo/pick.rs:57` | 本地拒绝纯空白 `message` |
| 两条路由均声明 `.checked().idem()` | `crates/rg-core/src/auth/route_meta.rs:351` | 支持 `idempotency_key`，作为 `Idempotency-Key` 头透传 |
| 冲突 → 409 `MERGE_CONFLICT` + `details.paths`；分支移动 → 409 `REF_ADVANCED` + `details.current_sha`；无变更 → 409 `NOTHING_TO_APPLY` | `crates/rg-http/src/api/repo_pick.rs:315` | MCP 不改写错误，沿用 `send_write` 的 `HTTP <status>: <body>` |

## 实施步骤

1. **工具定义**：在 `crates/rg-mcp/src/tools/mod.rs::list_tools` 增加
   `cherry_pick_commit`、`revert_commit` 两个描述项，schema 与既有写工具同构
   （`owner`、`repo`、`sha`、`branch`、`expected_sha`、`mainline`、`message`、
   `dry_run`、`idempotency_key`），`required` 只含前五项。
2. **分发**：在 `call_tool` 的 `match name` 中加两条分支。
3. **校验与请求**：
   - 复用 `name_arg` / `text_arg` / `optional_text` / `flag_arg` / `idempotency_key_arg` / `send_write`；
   - 新增 `commit_id_arg`（4–64 hex）、`full_commit_id_arg`（40/64 hex）、`optional_mainline`（≥1 的 u32）；
   - 共享 `pick_commit_tool(state, args, operation)`，operation 为 `cherry-pick` / `revert`。
4. **测试**：
   - 单元测试：三个新校验函数的边界；
   - `crates/rg-mcp/tests/write_tools_mcp.rs`：schema 必填项、请求路径/体/幂等头、
     `dry_run`/`mainline` 透传、409 拒绝原因与 details、坏参数不发出请求。
5. **验收**：`cargo test -p rg-mcp`、`cargo clippy --workspace --all-targets -- -D warnings`、
   `scripts/check.sh --quick`。
6. **收尾**：`docs/plans/T02-plan.md`、`docs/plans/T02-task.md`、`tasks/T02.md` 置 `completed`，
   提交并只推送 `agent/t02-mcp-remaining` 分支。

## 验收路径

- 服务端行为不做重复测试（`rg-http` 已覆盖），MCP 测试只断言「发了什么请求」与
  「服务端拒绝如何原样返回」。
- 参数校验全部发生在 `send_write` 之前，测试用 stub 的请求计数断言「坏参数零请求」。

## 风险与边界

- 不改 `crates/rg-mcp/src/collab.rs`（`collab_*` 是另一套清单，有 `collab_contract` 守卫）。
- 不改 `crates/rg-http/src/lib.rs`、`openapi.rs`、`route_meta.rs`、`Cargo.*`、迁移、`CLAUDE.md`、`ironforge-docs/`。
- 新增工具名用 snake_case 无前缀，延续 `commit_files` 风格。
