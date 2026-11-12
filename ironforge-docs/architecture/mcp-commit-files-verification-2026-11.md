# MCP `commit_files` 验证记录（2026-11-13）

> **状态**：已实现，本机实测 ｜ **可信度**：确认（附运行证据）｜ **来源**：仓库代码 + 本次运行
> 对应 CNB issue #48 第 1 项。第 2 项（unified diff）的决策见 [mcp-commit-files-and-patch-decision-2026-11.md](mcp-commit-files-and-patch-decision-2026-11.md)；第 3 项未做。

## 改了什么

| 文件 | 内容 |
|---|---|
| `crates/rg-mcp/src/tools/mod.rs` | 工具定义、`call_tool` 分发、`tool_commit_files` 与 `batch_file_entry` 校验 |
| `crates/rg-mcp/tests/write_tools_mcp.rs` | 桩 API 增加 `POST …/commits`；`tools/list` 的 `required` 断言加入 `commit_files`；新增 3 个测试 |
| `scripts/agent-e2e.py` | 新增"commit_files 对照 commit_file 循环"步骤 |
| `.ai/README.md` | MCP 工具表加一行 |

没有改 rg-http（路由、`capabilities.rs`、`pat_scope.rs` 都没动）：服务端的 `POST /commits` 早已存在，已在 `idempotency_supported` 与 `ref_limited_write_allowed` 里。能力注册表只登记 `collab_*` 工具，所以无需登记。

## 证据

| 层级 | 结果 |
|---|---|
| 任务验证 | `cargo test -p rg-mcp`：lib 17、`collab_mcp` 21、`read_tools_mcp` 10、`write_tools_mcp` 12（含新增 3 项）全部通过。新增三项：一次调用只发一个请求且四类动作原样进入请求体、`idempotency_key` 作为头发出；409 `REF_ADVANCED` 带 `current_sha` 作为工具错误返回；17 种坏参数（缺 `branch`/`expected_sha`/`message`、空 `files`、未知 `operation`、`update` 缺 `sha`、多余字段、非字符串字段等）都在发请求前被拒绝，桩收到 0 个请求 |
| 运行验证 | `cargo build -p rg-cli -p rg-mcp && python3 scripts/agent-e2e.py` 退出码 0（真实服务 + 真实 `ironforge-mcp` stdio + 主机执行 CI + 限制在仓库和 `agent/*` 的令牌，SQLite）。新增步骤：**对照**：4 次 `commit_file` → 4 个 commit、4 个流水线；**批量**：先造 3 个种子文件，再一次 `commit_files`（update + delete + move + create）→ 分支上恰好多 1 个 commit（其父是调用前的 tip），`agent/batch` 的流水线数恰好 +1，该流水线 success；move 后的文件内容正确，被删/被移走的路径 404；缺 `expected_sha` 不发请求；过期 `expected_sha` → `REF_ADVANCED` 且分支没动；`dry_run` 分支没动；同 `idempotency_key` 重放得到同一个 commit；受限令牌对 `main` 批量提交 → 403。原有的 `commit_file`/`wait_pipeline`/`create_pr` 等步骤同一次运行中也通过 |
| 门禁 | `cargo clippy --workspace --all-targets -- -D warnings`：无告警（本机，未经 CI）；`cargo fmt --check -p rg-mcp` 通过 |

## 未覆盖

- **未经 CI**；没有跑 PostgreSQL/MySQL（本改动只在 MCP 转发层，未涉及数据库）。
- `scripts/agent-e2e-remote.py`（`--http` 传输）未跑；`commit_files` 走同一个 `call_tool`，但没有在 HTTP 传输上实测。
- `commit_files` 不能创建分支（REST 的设计：`expected_sha` 必填且分支必须存在），新分支要先用 `commit_file` 建；没有为此加参数。
- 服务端对 `files` 条数没有上限，MCP 侧也没加。
