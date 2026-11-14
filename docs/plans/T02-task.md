# T02 Task — MCP 写工具集补 cherry-pick / revert

> 本文记录任务约束确认、实际改动、验收证据与未实现项结论。
> 状态权威仍是 `tasks/T02.md`。

## 约束确认

- [x] 只补 MCP 暴露，服务端端点已存在（`crates/rg-http/src/api/repo_pick.rs:92`、`:149`）。
- [x] 工具与现有写工具同构：描述项（`list_tools`）→ 分发（`call_tool` 的 `match`）→
      参数校验（`pick_commit_tool`）三处同步。
- [x] 校验规则取自服务端实现，不凭猜（逐条对照见 `T02-plan.md`「先读事实」表）。
- [x] 未改 `crates/rg-mcp/src/collab.rs`、`crates/rg-http/**`、`openapi.rs`、`route_meta.rs`、
      `CLAUDE.md`、`ironforge-docs/`、`Cargo.*`、迁移目录。
- [x] 工具名 snake_case 无前缀：`cherry_pick_commit`、`revert_commit`。
- [x] 错误映射沿用 `send_write`：`Error: HTTP <status>: <服务端原始 body>`，模型可读
      `reason` / `details.current_sha` / `details.paths`。

## 实际改动

| 文件 | 改动 |
| --- | --- |
| `crates/rg-mcp/src/tools/mod.rs` | `list_tools` 新增 2 个工具描述；`call_tool` 新增 2 条分发；新增 `commit_id_arg`、`full_commit_id_arg`、`optional_mainline`、`pick_commit_tool` 与两个入口函数；新增 3 个单元测试 |
| `crates/rg-mcp/tests/write_tools_mcp.rs` | stub 新增 4 条 pick 路由；schema 断言扩展；新增 4 个集成测试（请求形状、dry_run/mainline、拒绝透传、参数前置校验） |
| `docs/plans/T02-plan.md`、`docs/plans/T02-task.md` | 本任务计划与记录 |
| `tasks/T02.md` | `State` → `completed` |

### 工具参数

`cherry_pick_commit` / `revert_commit` 均为：

- 必填：`owner`、`repo`、`sha`、`branch`、`expected_sha`
- 可选：`mainline`（≥1 的整数）、`message`、`dry_run`、`idempotency_key`
- 请求：`POST /repos/{owner}/{repo}/commits/{sha}/cherry-pick|revert`
- body：`{ branch, expected_sha, mainline?, message?, dry_run? }`
- `idempotency_key` 仅作为 `Idempotency-Key` 头，不进入 body（两条路由均为 `.idem()`）

### 与服务端一致的校验

| 参数 | MCP 侧 | 服务端真实行为 |
| --- | --- | --- |
| `sha` | 4–64 位十六进制；否则 `sha must be a hex commit id of 4-64 characters` | 非 hex / 非 4–64 → 404（`rg-git/src/pick.rs:225`） |
| `branch` | 非空白必填 | 空分支名 400；不存在的分支 → 409 `REF_ADVANCED`（不创建） |
| `expected_sha` | 40 或 64 位十六进制全 id；否则 `expected_sha must be a full commit id …` | 与 tip 用完整 object id 比较，短 id 永不相等 → 409 `REF_ADVANCED` |
| `mainline` | 可选，≥1 的整数 | 非 merge 传 → 400 `MAINLINE_INVALID`；merge 不传 → 400 `MAINLINE_REQUIRED` |
| `message` | 可选，纯空白拒绝 | 纯空白 400（`rg-core/src/repo/pick.rs:57`） |
| `dry_run` | 仅接受布尔 | `X-Dry-Run` / body 均支持；dry run 不写对象、不移动分支 |
| `idempotency_key` | 1–255 个 `[A-Za-z0-9._:-]` | 路由声明 `.idem()`，重复键回放首次响应 |

## 未实现项：#48「unified diff 输入」

**结论：服务端缺「接收 unified diff/patch 并应用」的端点 ⇒ MCP 侧不实现，也不自造端点。**

依据：

- 路由表中不存在 `POST …/patch`、`…/diff`、`…/apply`（`crates/rg-http/src/lib.rs` 全文检索无匹配）。
- 仅有的两个 `apply` 写路由是
  `POST /repos/{owner}/{name}/pulls/{number}/comments/{id}/suggestion/apply` 与
  `POST …/pulls/{number}/suggestions/apply`（`crates/rg-http/src/lib.rs:930`、`:934`）。
  它们的请求体是 `{ comment_ids: Vec<i64> }`（`crates/rg-http/src/api/reviews.rs:70`），
  作用是把**已存储在服务端的 review suggestion** 应用到 PR，不接受调用方自带 diff。
- `unified_diff` 在代码中只作为输出/解析用途出现（`rg-core/src/pull_request/service.rs:667`
  的 `split_unified_diff` 等），不是写入口。

因此 `#48` 第三项需要先新增服务端端点（会触碰路由注册、OpenAPI、`route_meta`，
且属于本任务禁改区域），不在 T02 范围内。建议另开任务，由服务端先定义
「diff 输入 + expected_sha + 冲突语义」的合同，再补 MCP 暴露。

## 验收证据（本机实际运行）

```
$ cargo test -p rg-mcp
     Running unittests src/lib.rs
running 20 tests
test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running unittests src/main.rs
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/collab_mcp.rs
running 21 tests
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/read_tools_mcp.rs
running 10 tests
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/write_tools_mcp.rs
running 16 tests
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Doc-tests rg_mcp
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

```
$ cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 06s
```

```
$ scripts/check.sh --quick
==> cargo fmt --all --check
    (3s)
==> cargo clippy --workspace --all-targets -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.52s
    (0s)
==> checks passed in 3s (quick)
```

## 依赖与风险

- Depends on：none。
- 未新增依赖、未改 `Cargo.toml` / `Cargo.lock`。
- 风险：`expected_sha` 的全 id 校验比服务端错误码更早；若将来服务端接受缩写
  `expected_sha`，需要同步放宽此校验（当前实现不允许，见 `rg-git/src/pick.rs:246`）。
- 风险：`mainline` 仅在本地校验为正整数，父提交数量与实际合法性由服务端判定，
  避免 MCP 侧重复读取仓库。
