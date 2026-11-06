# MCP 读类工具修复（验证记录，2026-11-06）

> **状态**：已修复，SQLite 实测 ｜ **可信度**：确认（附运行证据）｜ **来源**：仓库代码 + 本次运行
> 起因：验证令牌仓库白名单时发现 MCP 的读类工具早就坏了，与令牌无关。

## 问题（修改前）

`ironforge-mcp` 的五个读类工具里，三个请求了服务器上不存在或方法不对的路由：

| 工具 | 修改前请求 | 服务器实际路由 | 现象 |
|---|---|---|---|
| `read_dir` | `GET /repos/{o}/{n}/tree/{ref}?path=` | `GET /repos/{o}/{n}/tree?ref=&path=` | 落到前端兜底页，**200 的 HTML** 被当成目录内容 |
| `read_file` | `GET /repos/{o}/{n}/contents/{path}?ref=` | 读文件是 `GET /repos/{o}/{n}/blob/{path}`；`contents` 只支持 POST/DELETE | 405 |
| `list_repos` | `GET /repos` | 列表是 `GET /repos/{owner}`、公开列表是 `GET /repos/explore`；`/repos` 只有 POST | 同上，兜底页或错误 |

另外：工具失败（被拒绝、不存在、参数缺失）被放进**正常结果**，文本以 `Error:` 开头，`isError` 为假，只看标志的调用方会把失败当成功；`owner`/`repo` 来自模型，未经校验直接拼进 URL 路径。`get_issue`、`get_pr` 的路由本来就对。

## 做了什么（`rg-mcp/src/tools/mod.rs`）

- `read_dir` → `GET …/tree`，`path`、`ref` 作查询参数，**为空就不发**，所以默认分支不必叫 `main`（旧描述写"默认 main"，已改为默认分支）。
- `read_file` → `GET …/blob/{逐段百分号编码的路径}`（`ref` 有才发），只返回文本内容；二进制文件报错并写明大小，不再倾倒 base64。
- `list_repos` → 给 `owner` 走 `GET /repos/{owner}`，不给走 `GET /repos/explore`（只含公开仓库；没有 `owner` 就无法列出调用者自己的私有仓库，工具描述已写明）。
- **失败是真正的工具错误**：`isError: true`，文本以 `Error:` 开头并带状态码。
- **API 回答不是 JSON 也算错误**：未知路由返回前端页面加 200，不能当数据。
- 参数校验：`owner`/`repo` 只允许 1–100 个 `[A-Za-z0-9._-]` 且不是 `.`/`..`；`number` 必须为正整数；路径不得含空段、`.`、`..`；**不合法的参数不发任何请求**。

## 证据

| 层级 | 结果 |
|---|---|
| 任务验证 | `rg-mcp` 单测 4 项（名称、数字、路径编码与不可上跳、查询串）；`read_tools_mcp` 9 项：真实 `ironforge-mcp` 二进制 + 记录请求的桩 API，断言每个工具请求的确切路由（含编码、`ref` 仅在给出时发送）、文本只返回内容、二进制报错、403/404 为 `isError`、非 JSON 回答为错误、11 种坏参数一个请求都不发、令牌以 Bearer 发送。`rg-mcp` 既有的 11 个单测通过 |
| 运行验证（真实服务 + 真实 `ironforge-mcp`，SQLite，私有仓库 + 真实 push 的提交 + 真实 Issue，`repo:read` 令牌） | `read_dir` 根目录与 `src`、`read_file` 的 `src/main.rs` 与 `README.md@main` 返回真实内容；二进制文件、缺失文件、不存在的 Issue 均为错误（带 404 或"二进制"说明）；`get_issue 1` 返回真实 Issue；`list_repos` 给 `owner` 列出仓库，不给 `owner` 时私有仓库不出现（`data: []`）；`owner=a/b` 被拒绝 |

## 未覆盖

- `get_pr` 没有在真实服务上跑（路由本来就对、未改，桩测试覆盖了路径）。
- 没有为文件大小设上限：大文件会整体返回给模型。
- CI 未看。
