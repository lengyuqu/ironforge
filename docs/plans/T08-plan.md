# T08 执行计划 — [#62] 接受 unified diff 的提交端点（apply-patch）

> 权威约束见 `tasks/T08.md`；本文件记录方案取舍、临时 index 路线结论、支持的 diff 子集、
> 上限口径、`--3way` 决策、安全校验清单与实跑结果。
> 基线 `0892c1e`，分支 `agent/t08-apply-patch`，worktree `../ironforge-t08`。

## 1. 交付边界

做：

1. `POST /api/v1/repos/{owner}/{name}/commits/apply-patch`（路由 + OpenAPI + route_meta 三处同步）。
2. `rg-git::apply`：临时 index + 临时对象库的 `git apply --cached` / `write-tree` / `commit-tree` / ref CAS。
3. `rg-core::repo::patch`：分支保护判定、补丁预演 → 密钥扫描 → 提交，HTTP 与领域错误映射所需的结果枚举。
4. `rg-http::api::repo_patch`：契约、dry-run 摘要、审计、推送同款副作用。

不做（任务明确划出）：

- MCP 工具 `apply_patch`（后续小任务）。
- `--3way` 三方合并（见 §5）。
- 任何 `Cargo.toml` / `Cargo.lock` 改动（见 §3）。

## 2. 端点契约

```
POST /api/v1/repos/{owner}/{name}/commits/apply-patch
```

```jsonc
{
  "branch": "feature/x",          // 必填，分支必须已存在，不存在不创建
  "expected_sha": "<40/64 hex>",  // 必填，乐观锁
  "patch": "diff --git ...",      // 必填，unified diff 文本
  "message": "optional",          // 缺省生成
  "dry_run": false                // 或 X-Dry-Run: 1
}
```

成功 `200`：

```jsonc
{
  "dry_run": false,
  "branch": "main",
  "tip_before": "<old sha>",
  "commit_sha": "<new sha>",
  "tree_sha": "<new tree>",
  "files_changed": 2,
  "changes": [{"path": "a.txt", "status": "modified"}, {"path": "b.txt", "status": "added"}]
}
```

`dry_run: true` 返回同一组 `tree_sha` / `files_changed` / `changes`，外加与其他 REST 提交端点一致的
`would[]`（`ref_update` + `commit`）与 `not_evaluated: ["webhook","ci_trigger","notification"]`；
没有 `commit_sha`，因为不写 commit。

失败：

| 情形 | 状态 | code / reason |
| --- | --- | --- |
| 字段缺失/类型错 | 422 | axum JSON 拒绝（与全仓一致） |
| 空补丁、非 diff、超限、路径越界 | 400 | `BAD_REQUEST` |
| 未认证 / 无写权限 / 分支保护 | 401 / 403 / 403 | 与 `contents`、`commits` 相同 |
| 补丁不适用（上下文不匹配等） | 409 | `reason: PATCH_CONFLICT`，`details.paths` |
| 补丁应用后树不变 | 409 | `reason: NOTHING_TO_APPLY` |
| `expected_sha` 不符 / 分支不存在 | 409 | `reason: REF_ADVANCED`，`retryable: true` |
| 新 blob 命中 R7 硬规则 | 422 | `SECRET_DETECTED` |

## 3. 临时 index 路线：可行性结论（已验证）

**结论：可行，按推荐路线实现，未走自实现 diff 解析。**

实测（macOS / git 2.x，裸仓）：

```
GIT_INDEX_FILE=<scratch>/index git read-tree <expected_sha>
GIT_INDEX_FILE=<scratch>/index git apply --cached <patch-file>
GIT_INDEX_FILE=<scratch>/index git write-tree
git commit-tree <tree> -p <expected_sha> -m <message>
```

- 裸仓没有工作区，`--cached` 全程只改 index，不触碰任何工作区；`git apply` 没有要求 worktree。
- 补丁通过**文件**传入：`GitCommandGateway` 的 `run/run_with_env` 只接管 stdout/stderr，
  stdin 保持继承，无法安全写补丁；落一个 0600 的临时文件比扩网关能力（`cli_gateway.rs` 不在 owned paths）更小。
- 临时 index / 临时对象目录放在一个 `0700` 的 scratch 目录里，`Drop` 递归删除。
  **不引入 `tempfile` 运行时依赖**：`tempfile` 目前只是 `rg-git` 的 dev-dependency，
  提到 `[dependencies]` 需要改 `Cargo.toml`（本任务禁止）。自写 ~50 行 scratch 目录：
  进程 id + 原子计数器命名，`create_dir` 名字冲突即重试（因此不会有跟随预置软链接的问题），
  unix 上 `mode(0o700)`。

### 3.1 两阶段：预演写不到仓库，拒绝路径零残留

`git apply --cached` 会把新 blob 写进对象库。若「应用 → 扫描 → 发现密钥 → 422」，
仓库里就会留下未被引用的 blob。为避免这一点，`rg-git::apply` 拆成两步：

1. **`preview_patch`**（预演）：`GIT_OBJECT_DIRECTORY=<scratch>/objects`
   + `GIT_ALTERNATE_OBJECT_DIRECTORIES=<repo>/objects`。补丁产生的 blob/tree 全部落进 scratch，
   随 scratch 一起删除；仓库一个字节都不变。返回 tree、变更清单，以及新增/修改文件的**内容**
   （≤1 MiB 的才读，扫描本来也会跳过更大的）。
2. **`commit_patch`**（落库）：同一序列在真实对象库里重跑，`commit-tree` 写提交，
   ref 用 `PreviousValue::MustExistAndMatch(expected_sha)` CAS 推进。

`rg-core` 在两步之间做密钥扫描，命中即 422，因此**被拒绝的补丁不写 blob、不写 tree、不写 commit、不动 ref**。
已用 `git count-objects -v` 前后比对在单测和集成测试里钉住（冲突、越界、超限、密钥四类拒绝路径）。

`commit_patch` 会复核「预览树 == 提交树」，不一致按内部错误处理（仓库在两步之间发生了不该发生的变化）。

### 3.2 其他实现细节

- `git apply --numstat -z` 用作**路径来源**：与 `git apply` 同一个解析器，输出已做 `a/`/`b/` 前缀剥离与
  引号反解析，避免自己再写一个更弱的 diff 解析器。它不写任何东西。
- 变更清单用 `git diff-tree -r -z --no-commit-id --raw <old_tree> <new_tree>`（A/M/D/T；重命名按 git 默认
  显示为 D + A）。
- 读 blob 前先 `git cat-file -s`，超过 `secret_scan::MAX_FILE_BYTES`（1 MiB）的不读（扫描也会跳过）。
  进程数上界：1 + 500 个文件 × 2，且只在补丁通过全部校验后发生。
- 所有 git 调用带 `LC_ALL=C`，错误文本可解析（`git apply` 失败时按 `error: <path>: <reason>` 提取冲突路径）。
- 提交身份通过 `GIT_AUTHOR_*` / `GIT_COMMITTER_*` 传给 `commit-tree`，不依赖仓库配置。
- ref 用 gix 的 `edit_reference` + `PreviousValue::MustExistAndMatch`（与 `pick.rs` 同一守卫），
  reflog 写 `apply-patch: <subject>`。

## 4. 支持的 diff 子集

| 情形 | 支持 | 说明 |
| --- | --- | --- |
| 新增文件 | ✅ | `--- /dev/null` / `+++ b/x`，含 `new file mode` |
| 删除文件 | ✅ | `+++ /dev/null` |
| 修改文件 | ✅ | 上下文必须精确匹配 |
| 重命名 | ✅ | 按 diff 本来的样子 = 删除 + 新增；不需要 rewrite 检测 |
| 类型变更（文件 ↔ 符号链接等） | ✅ | 报告为 `type_changed` |
| 二进制补丁（`GIT binary patch`） | ✅ | 由 git 自己处理；二进制 blob 不参与文本密钥扫描（与推送路径一致） |
| 模式变更（100644 → 100755） | ✅ | 与内容变更一起落地 |
| 多个文件一个补丁 | ✅ | ≤ 500 个路径 |
| `diff -u old new`（无 `a/` `b/` 前缀） | ❌ | 默认 `-p1`；返回 400「不是 git 能应用的 unified diff」 |
| `git format-patch` 邮件格式（含 commit 元信息） | ⚠️ | 头部的 commit 元信息被忽略，只有 diff 部分生效；不承诺保留作者 |
| 上下文模糊匹配 | ❌ | 不用 `--3way`（§5） |

## 5. `--3way` 口径：**不启用**

`git apply --3way` 会在上下文不匹配时退回三方合并，可能自动解出调用方没有写过、也没有被报告的中间态
（甚至把冲突写进 index）。这与端点语义「把这段 diff 精确地落到分支上」冲突，并且会让「冲突 = 409 且分支不动」
这条保证失效。

因此：

- 上下文不匹配 → 409 `PATCH_CONFLICT`，带上 git 指出的路径，分支不动。
- 调用方需要三方语义时应走 cherry-pick / revert 端点，或自己 rebase 后重新生成补丁。
- 若将来要开，必须先定义「什么算冲突」「自动解出的结果怎么在 commit message 里标注」，
  并在本文件的验收项里加对应用例。

## 6. 体积上限：**选 400**，不是 413

- 补丁文本 ≤ **1 MiB**（`MAX_PATCH_BYTES`），影响路径 ≤ **500**（`MAX_PATCH_FILES`，与 R7 的
  `secret_scan::MAX_FILES` 同值，避免「扫描不了但能提交」的口径冲突）。
- 超限返回 **400 `BAD_REQUEST`**，理由：这两个上限是请求体自身的形状约束（补丁太大 / 文件太多），
  与「拒绝一个语义正确的请求」不同；413 在本仓库没有先例，且容易被网关的
  `DefaultBodyLimit`（axum 默认 2 MiB）混淆成两种来源的 413。400 的 message 明确写出实际值与上限。
- **不 fail-open**：超限直接拒绝，不截断、不扫描子集。注意 axum 自身还有 2 MiB 请求体上限，
  所以 1 MiB 的补丁一定能进入 handler（JSON 转义后仍有余量）。

## 7. 安全校验清单

| # | 校验 | 位置 | 失败 | 测试 |
| --- | --- | --- | --- | --- |
| S1 | 分支名合法（gix `FullName`） | `rg-core::repo::patch::validate` | 400 | 集成：空/非法分支 |
| S2 | `expected_sha` 是 40/64 hex | 同上 | 400 | 集成：短 sha / 非 hex / 空 |
| S3 | `message` 非空白、`patch` 非空白 | 同上 | 400 | 集成：空补丁、纯空白 |
| S4 | 补丁 ≤ 1 MiB | `rg-git::apply::validate_patch` | 400 | 单测 + 集成 |
| S5 | 路径 ≤ 500 | 同上 | 400 | 单测 + 集成 |
| S6 | 路径不是绝对路径 | `check_patch_path` | 400 | 单测 + 集成（`+++ b//etc/passwd`） |
| S7 | 路径不越界（`..`、`.`、空组件） | 同上 | 400 | 单测 + 集成（`+++ b/../../evil.txt`、`b/x/../y`） |
| S8 | 路径不在 `.git` 内（大小写不敏感） | 同上 | 400 | 单测 + 集成（`b/.git/config`、`b/sub/.GIT/hooks/*`） |
| S9 | 路径无控制字符、≤ 4096 字节 | 同上 | 400 | 单测（随 S6 分支覆盖） |
| S10 | 新 blob 过 R7 硬规则 | `rg-core::repo::patch` | 422 `SECRET_DETECTED` | 单测（扫描器既有）+ 集成 |
| S11 | 写权限 `can_write_repo` | `resolve_and_check_write_access` | 403 | 集成 |
| S12 | 分支保护 `require_pr` / `require_signed_commits` | `check_direct_commit_allowed` | 403 | 集成 |
| S13 | 分支限定的 token 只能写自己的分支 | `ensure_token_may_write_branch` | 403 | 集成 |
| S14 | 分支必须存在、`expected_sha` 必须等于当前 tip | `rg-git::apply` + ref CAS | 409 `REF_ADVANCED` | 单测 + 集成 |
| S15 | 路径穿越的二次防线 | `git apply` 自身拒绝 `invalid path` | 400/409 | 与 S6–S8 同例 |

`git apply` 自身也会拒绝越界路径（"invalid path"），我们的校验在其之前，保证返回码与文案稳定（400 而不是翻译过的 CLI 错误）。

## 8. 副作用（与推送路径一致）

- **ref**：`PreviousValue::MustExistAndMatch(expected_sha)`，不盲写。
- **CI / webhook / PR head / 事件日志**：复用 `repo_content::announce_branch_move`（与批量提交、文件提交同一套
  `spawn_post_push_hooks`）。
  - ⚠️ 口径差异：任务建议 `trigger_type` 用 `"api"` 或 `"apply_patch"`，但该字段在
    `rg-http/src/lib.rs` 的 post-push 钩子里**硬编码为 `"push"`**，而 `lib.rs` 除 `build_routes` 外禁止改。
    为与「文件提交走同一套事件」保持一致（`repo.batch_commit` 也是 `"push"`），这里沿用 `"push"`，
    并如实登记为偏差。`triggered_by` 是 actor id。
- **审计**：`repo.apply_patch`，details 含 `branch` / `commit_sha` / `before` / `tree` / `files_changed` / `paths`；
  密钥扫描的拒绝/告警/跳过由 `service::scan_commit_content` 写 `repo.secret_scan_*`。
- **dry_run**：不落 ref、不写对象、不写审计（含密钥扫描审计）、不触发 CI/webhook，
  只返回 `tree_sha` / `files_changed` / `changes` / `would[]`。dry-run 里若扫描会命中，仍返回 422
  （它就该回答「真跑会不会被拒」），但不写审计行。

## 9. 路由三处同步

| 位置 | 改动 |
| --- | --- |
| `rg-http/src/lib.rs` (`build_routes`) | 新增 `.route("/repos/{owner}/{name}/commits/apply-patch", post(api::repo_patch::apply_patch))`，紧邻 cherry-pick / revert |
| `rg-http/src/openapi.rs` | `paths(...)` 增加 `crate::api::repo_patch::apply_patch` |
| `rg-core/src/auth/route_meta.rs` | `repo(".../commits/apply-patch").checked().idem()`：写 ref 且 handler 调 `ensure_token_may_write_branch`；响应不含密钥，可幂等重放 |

`checked()` 使 `route_metadata_tests` 的 oracle 需要同步新增 `["commits", "apply-patch"]`
（测试文件里的 `legacy::branch_limited_write` 就是「注册表之前的表」，新增路由按 cherry-pick 的先例扩展，
不是放宽断言）。`.idem()` 与 oracle 的 `["repos", _, _, "commits", ..] → true` 一致。

## 10. 验收（实跑输出见 §12）

```
cargo test -p rg-git
cargo test -p rg-core
cargo test -p rg-http
cargo clippy --workspace --all-targets -- -D warnings
scripts/check.sh --quick
```

覆盖清单：

| 要求 | 覆盖 |
| --- | --- |
| 新增 + 修改文件补丁成功 | `apply_patch_tests::a_patch_lands_as_one_commit_and_is_announced_and_audited`；单测同名 |
| 上下文不匹配 409 且 ref 不动 | `a_patch_that_does_not_apply_is_a_409_and_the_branch_stays` + `count-objects` 比对 |
| `dry_run` 零副作用 | `a_dry_run_reports_the_change_and_writes_nothing`（refs / objects / 审计三重比对） |
| 路径穿越被拒 | `paths_outside_the_repository_are_a_400`（3 例 × dry_run）+ 单测 5 例 |
| 超限被拒 | `oversized_patches_are_a_400` + 单测（1 MiB / 501 文件） |
| 补丁内密钥被 R7 拦截 | `a_secret_in_the_patch_is_a_422_and_is_audited`、`a_secret_is_refused_on_a_dry_run_without_an_audit_row` |
| 分支不存在不创建 | `a_missing_branch_or_stale_expected_sha_is_ref_advanced` |
| 删除 / 重命名 / 二进制 | 单测 `deletes_renames_and_binary_patches_are_supported` |

## 11. 遗留风险

1. **R7 的 1 MiB 文件上限对写入路径是 fail-open**：单个新 blob > 1 MiB 时不扫描（与推送路径完全一致）。
   二进制补丁（压缩 delta）可以用很小的补丁造出 > 1 MiB 的文件，从而绕过文本扫描。这是 R7 既有口径，
   本次不改变；若要收紧，应改 `secret_scan` 的全局策略而不是只在这个端点加锁。
2. **`trigger_type` 沿用 `"push"`**（§8）。CI 视图里与真实推送不可区分；要区分需要改 `lib.rs` 的 post-push 钩子。
3. **两次 git apply**：预演 + 落库各跑一次，补丁级开销翻倍（≤1 MiB，可接受）。换来的是拒绝路径零残留。
4. **`files_changed` 把重命名计为 2**（D + A）：与 `git apply` 的行为一致，是上限判断的保守方向。
5. **临时目录在系统 tmp**：若进程被 SIGKILL，scratch 目录残留（内含按内容寻址的 blob）；
   不写进仓库，下一次系统清 tmp 或人工清理即可。
6. **`git apply` 的属性过滤**：`--cached` 直接写 blob，不跑 clean/smudge；与「补丁就是内容」的预期一致，
   但对依赖 `filter=lfs` 的仓库不友好（LFS 指针需调用方自行处理）。

## 12. 验收实跑输出（本机，2026-11-14，未经 CI）

```
$ cargo test -p rg-git
test result: ok. 120 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 1.55s
   Doc-tests rg_git
test result: ok. 0 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.00s
===EXIT rg-git 0===

$ cargo test -p rg-core
（单元测试 + 各集成测试全部 `test result: ok`；doc-test 1 passed / 5 ignored）
===EXIT rg-core 0===

$ cargo test -p rg-http
（全部集成测试 `test result: ok`，含新增 `apply_patch_tests` 13 passed；无 FAILED）
===EXIT rg-http 0===
```

新增单测（`rg-git`）：

```
test apply::tests::a_patch_lands_as_one_commit_on_top_of_the_expected_tip ... ok
test apply::tests::a_custom_message_is_kept ... ok
test apply::tests::a_patch_that_does_not_fit_is_a_conflict_and_writes_nothing ... ok
test apply::tests::a_patch_that_changes_nothing_is_empty ... ok
test apply::tests::a_stale_expected_tip_and_a_missing_branch_are_ref_advanced ... ok
test apply::tests::paths_outside_the_repository_are_refused ... ok
test apply::tests::a_patch_over_the_limits_is_refused ... ok
test apply::tests::deletes_renames_and_binary_patches_are_supported ... ok
```

新增集成测试（`rg-http`）：

```
running 13 tests
test the_openapi_spec_documents_the_route_and_its_required_fields ... ok
test an_empty_or_malformed_request_is_refused ... ok
test oversized_patches_are_a_400 ... ok
test paths_outside_the_repository_are_a_400 ... ok
test a_patch_that_does_not_apply_is_a_409_and_the_branch_stays ... ok
test a_missing_branch_or_stale_expected_sha_is_ref_advanced ... ok
test a_secret_is_refused_on_a_dry_run_without_an_audit_row ... ok
test a_secret_in_the_patch_is_a_422_and_is_audited ... ok
test a_branch_limited_token_applies_only_inside_its_limit ... ok
test a_custom_message_is_kept ... ok
test a_patch_needs_write_access_and_respects_require_pr ... ok
test a_patch_lands_as_one_commit_and_is_announced_and_audited ... ok
test a_dry_run_reports_the_change_and_writes_nothing ... ok

test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.17s
```

契约守卫（`rg-http`，新增路由必须通过）：

```
$ cargo test -p rg-http --test route_metadata_tests --test openapi_registration_tests
route_metadata_tests      : 9 passed; 0 failed
openapi_registration_tests: 5 passed; 0 failed
```

```
$ cargo clippy --workspace --all-targets -- -D warnings
Finished `dev` profile [unoptimized + debuginfo] target(s) in 51.48s
===EXIT clippy 0===

$ scripts/check.sh --quick
==> cargo fmt --all --check
    (2s)
==> cargo clippy --workspace --all-targets -D warnings
    (0s)
==> checks passed in 2s (quick)
===EXIT check 0===
```

未运行：PostgreSQL / MySQL 套件（`IRONFORGE_TEST_DATABASE_URL` 未设置）、`scripts/check.sh --full`
（全 workspace 测试）、`collab-e2e.py`、浏览器烟测。本任务无新 SQL / 迁移，跨后端差异风险低。
**未经 CI**。
