# T08 Task 确认 — [#62] 接受 unified diff 的提交端点（apply-patch）

> 权威状态在 `tasks/T08.md`；本文件记录约束确认、触碰路径与风险。执行细节见 `docs/plans/T08-plan.md`。

## 1. 约束确认

| 约束 | 结论 |
| --- | --- |
| 端点 `POST /repos/{owner}/{name}/commits/apply-patch` | 是，字段 `branch` / `expected_sha` / `patch` / `message?` / `dry_run?` |
| 分支不存在**不创建** | 是，`TipAdvanced { actual: None }` → 409 `REF_ADVANCED`（与 pick 一致） |
| `expected_sha` 与分支头不一致 → 409 `REF_ADVANCED` | 是，预览前判定 + 落库时 `PreviousValue::MustExistAndMatch` CAS |
| `dry_run` 不落 ref、不写审计、不触发 CI/webhook | 是；连对象都不写（预演用临时对象库），返回 tree / 文件变更摘要 |
| 成功返回 `commit_sha` / `tree_sha` / `files_changed` / `dry_run` | 是，另附 `branch` / `tip_before` / `changes[]` |
| 临时 index 路线（`read-tree` → `apply --cached` → `write-tree` → `commit-tree`） | 是，已实测可行；未退回自实现 diff 解析（plan §3） |
| 全程 `run_with_env` 传 `GIT_INDEX_FILE`，不 checkout 工作区 | 是；补丁经 0600 临时文件传入（网关不支持 stdin） |
| 临时 index 落 tempfile 目录并 Drop 清理 | 是，自写 `Scratch`（`tempfile` 只是 dev-dependency，抬到运行时需要改 `Cargo.toml`，本任务禁止） |
| `--3way` 口径明确 | 是，不启用；上下文不匹配即 409（plan §5） |
| 路径穿越 / 绝对路径 / `.git` → 400 | 是，`check_patch_path`，单测 5 例 + 集成 3 例；`git apply` 自身拒绝为第二层 |
| patch ≤ 1 MiB、文件 ≤ 500，超限明确选 400/413 | 选 **400**，理由见 plan §6；不 fail-open |
| 新 blob 过 R7，命中 422 `SECRET_DETECTED` | 是，复用 `service::scan_commit_content`（真实运行）与 `secret_scan::scan_files`（dry run，不写审计） |
| 权限 `can_write_repo`；agent token 走推送同款 scope 校验 | 是；`route_meta` 声明 `repo(...).checked().idem()` |
| ref 用 CAS | 是，`PreviousValue::MustExistAndMatch(expected_sha)` |
| 触发 CI、webhook、审计 | 是，复用 `announce_branch_move`（与文件提交同一套 post-push 钩子）+ `repo.apply_patch` 审计；`trigger_type` 偏差见 plan §8 |
| 路由三处同时补齐 | 是：`build_routes` / `openapi.rs` / `route_meta.rs`，两个契约测试全绿 |
| 不做 MCP 工具 | 是，未触碰 `rg-mcp` |
| 只改 owned paths | 是，清单见 §2 |
| 禁止改 `Cargo.*` / migrations / `CLAUDE.md` / `ironforge-docs/` / `.cnb.yml` / `.github/` / `scripts/` / `deploy/` | 是，全部未触碰 |
| `lib.rs` 只动 `build_routes` | 是，只加了一条 `.route(...)` 与一行注释 |
| 不 push main，不 force-push | 是，只推 `agent/t08-apply-patch` |

## 2. 触碰路径

生产代码：

- `crates/rg-git/src/apply.rs`（新建；含内嵌单测）
- `crates/rg-git/src/lib.rs`（`pub mod apply;` + 模块文档里「git CLI 用法」一段的例外说明）
- `crates/rg-core/src/repo/patch.rs`（新建）
- `crates/rg-core/src/repo/mod.rs`（`pub mod patch;`）
- `crates/rg-http/src/api/repo_patch.rs`（新建）
- `crates/rg-http/src/api/mod.rs`（`pub mod repo_patch;`）
- `crates/rg-http/src/lib.rs`（仅 `build_routes` 里一条路由 + 一行注释）
- `crates/rg-http/src/openapi.rs`（`paths(...)` 一行）
- `crates/rg-core/src/auth/route_meta.rs`（`ROUTES` 一行声明）

测试：

- `crates/rg-http/tests/apply_patch_tests.rs`（新建，12 个用例）
- `crates/rg-http/tests/route_metadata_tests.rs`（oracle `legacy::branch_limited_write` 增加 `["commits", "apply-patch"]`，
  照 cherry-pick / revert 先例；不改任何断言强度）

文档：

- `docs/plans/T08-plan.md`（新建）
- `docs/plans/T08-task.md`（本文件，新建）
- `tasks/T08.md`（仅 `State` → `completed` 与收尾记录）

未触碰：`Cargo.toml`、`Cargo.lock`、`crates/rg-db/src/migrations/`、`CLAUDE.md`、`ironforge-docs/`、
`.cnb.yml`、`.github/`、`scripts/`、`deploy/`、`rg-mcp`、`crates/rg-git/src/cli_gateway.rs`。

## 3. 依赖

`Depends on: none`。无新依赖：只用 `gix`、`anyhow`、`thiserror`、`tokio`、`axum`、`serde` 等既有依赖。
`tempfile` 未提升为运行时依赖（plan §3）。

## 4. 风险确认

| 风险 | 处置 |
| --- | --- |
| 补丁内容任意（写入路径） | 先校验再解析：长度、文件数、路径；扫描后才提交；拒绝路径不写对象（plan §3.1 实测钉住） |
| 上下文模糊导致「悄悄应用」 | 不用 `--3way`；不匹配即 409，分支不动 |
| 临时目录被劫持 | `create_dir`（不跟随软链接）+ `0700` + 进程唯一名；`Drop` 清理 |
| 两次 apply 得到不同树 | `rg-core` 比较预览树与提交树，不同即 500，不掩盖 |
| 与推送路径效果不一致 | 复用 `announce_branch_move`、`scan_commit_content`、同一套审计写法；唯一差异是 `trigger_type` 硬编码 `"push"`（plan §8 已登记） |
| 路由元数据漏登记 | `route_metadata_tests` / `openapi_registration_tests` 全绿（「每个路由都要有 metadata」守卫未被绕过） |
| 大补丁拖慢服务 | 1 MiB / 500 文件硬上限；git 调用有网关 120s 超时 |
| 新增测试与并行用例互相干扰 | 每个用例独立 `Fixture`（独立 sqlite + 独立 repo_root），审计按 action 过滤 |
