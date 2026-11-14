# T04 Task 确认 — R7 推送时密钥扫描

> 权威状态在 `tasks/T04.md`；本文件记录约束、路径与风险确认。执行细节见 `docs/plans/T04-plan.md`。

## 1. 约束确认

| 约束 | 结论 |
| --- | --- |
| 在 Git 写入路径上做**增量**密钥扫描 | 是。只 diff `old..new` 的树，不 walk 历史 |
| 命中分级：硬拒 / 记事件 | 是。私钥头、云 AK、平台 PAT 硬拒；JWT、高熵记事件 |
| 不阻断正常推送 | 是。警告级规则只留事件；扫描失败 fail-open 且有事件 |
| 扫描器放 `rg-git`（`rg-git` 不依赖 `rg-core`） | 是。`crates/rg-git/src/secret_scan.rs` |
| 只在 `:481` 与 `:486` 之间插入 | 是。`enforce_signed_commit_policies` 之后、改 ref 循环之前 |
| 不碰 R6 关注区（ref 命名空间 / TTL / 分支保护交互） | 是。只加一次函数调用，未改既有策略逻辑 |
| 超限 fail-open + 强制审计事件 | 是。>500 文件整批跳过并逐 ref 产出 `secret_scan_skipped`（ref、文件数、原因） |
| 依赖只加 `regex = { workspace = true }` | 部分：workspace **没有**声明 `regex`，按任务文件允许的兜底写法改为 `regex = "1"`（解析到已有 `Cargo.lock` 的 1.12.3，零新增 crate）；未手改 `Cargo.lock`，未改根 `Cargo.toml` |
| 命中不得把明文写进错误/日志 | 是。`Finding` 只有 `path`/`line`/`rule`；有测试断言消息里没有 `PRIVATE KEY` |

## 2. 触碰路径

生产代码

- `crates/rg-git/src/secret_scan.rs`（新建，扫描器）
- `crates/rg-git/src/lib.rs`（`pub mod secret_scan;`）
- `crates/rg-git/src/protocol/receive_pack.rs`（插入点 + 事件 + 拒绝）
- `crates/rg-git/Cargo.toml`（`regex`）
- `crates/rg-core/src/repo/service.rs`（三条提交路径扫描 + 审计行）
- `crates/rg-core/src/error.rs`（`CoreError::Coded` 变体 + `secret_detected` 构造器）

为**保持 workspace 编译**而必须修改的两处穷尽 `match`（各一个臂）：

- `crates/rg-http/src/error.rs`（`From<CoreError> for AppError`，不加臂则 `rg-http` 不编译）
- `crates/rg-core/src/pull_request/merge_queue.rs`（`CoreError` 穷尽 match）

未触碰：`crates/rg-http/src/lib.rs`、`openapi.rs`、`route_meta.rs`、`crates/rg-db/src/migrations/`、根 `Cargo.toml`、`CLAUDE.md`、`ironforge-docs/`、`.cnb.yml`、`.github/`、`scripts/`、`deploy/`。

> 关于 `rg-http/src/error.rs` 与 `merge_queue.rs`：两者不在 `tasks/T04.md` 的字面 owned paths 列举里，但都在「禁止修改」清单之外，且是新增 `CoreError` 变体后的机械性编译依赖（Rust 的穷尽 match 不允许漏臂）。`rg-http/src/error.rs` 的改动只是把新变体映射到既有 `AppError::Coded`，不新增路由、不改 OpenAPI。

## 3. 依赖

`Depends on: none`。使用既有 API：`gix` 的 tree diff、`rg_git::ops::commit_tree_edits`、`rg_db::entities::audit_log`、`rg_core::audit::record`。

新增直接依赖仅 `regex = "1"`（`rg-git`），`Cargo.lock` diff 只有一行 `"regex"` 加入 `rg-git` 的依赖列表。

## 4. 风险确认

| 风险 | 处置 |
| --- | --- |
| 高熵规则误报阻断正常推送 | 高熵与 JWT 都只「记事件 + 警告」，永不拒；且 doc/lock/fixtures/testdata 默认在允许列表 |
| Git SHA / 校验和触发高熵 | 十六进制熵上限 4.0 < 4.5，另有回归测试钉死 |
| 首次导入大仓被硬拒 | fail-open：>500 文件整批跳过，并逐 ref 产出 `secret_scan_skipped`（不得静默） |
| 大文件拖慢推送 | 单文件 >1 MiB 跳过并产出事件；内容逐 blob 读取后即释放 |
| 扫描失败导致推送失败 | fail-open + `secret_scan_skipped` 事件 + `warn` 日志（单点可改为 fail-closed） |
| 明文泄漏到日志/响应 | `Finding` 不含匹配文本；report-status 与审计 `details` 只有 path/line/rule；有断言 |
| 新增 `CoreError` 变体破坏既有 match | 逐一编译验证；两处穷尽 match 已补臂；`rg-core` 290+ 测试与 `rg-http` 全量 clippy 通过 |
| push 侧事件没有 DB 落地 | 明确记录为遗留（§9 of plan）：`set_audit_sink` + 结构化日志已就位，接线点在 `rg-http` 的 push 后钩子（禁改） |
| 本机验收 ≠ CI | 四条验收命令真实跑过；SQLite 之外的库未跑（无迁移、无 SQL 变更，风险低） |
