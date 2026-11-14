# T10 Task 确认 — [#39] 代码智能 · `/ai/repos/{o}/{r}/map` 端点 + token 预算裁剪

> 权威状态在 `tasks/T10.md`；本文件记录约束确认、触碰路径、验收命令真实输出与遗留风险。算法细节与实测数据见 `docs/plans/T10-plan.md`。

## 1. 约束确认

| 约束 | 结论 |
| --- | --- |
| 只做 #39 第二步：`/map` 端点 + 预算裁剪 | 是；未做符号图、向量后端、MCP 工具、索引持久化 |
| 直接用 T09 的 `extract_outline`，不重新实现解析 | 是；`rg-http` 只调用 `extract_outline`，未碰 `search/outline/` 三个文件 |
| 不改 `Cargo.toml` / `Cargo.lock` | 是，`git diff --stat` 无 `Cargo.*` |
| 端点契约：`ref`/`budget`/`path`，字段语义齐全 | 是，见 plan §2 |
| `budget` 默认 4000、上限 32000、非法 → 400 `INVALID_BUDGET` | 是，另设下限 64（空响应本身约 30 tokens），理由见 plan §3 |
| 裁剪不是"列全部再截断" | 是，五步实现映射见 plan §4；两处偏离字面描述（逐文件裁剪、`skipped` 只用剩余预算）已在 plan §4 记录并给出改前/改后实测对比 |
| 只考虑源码文件，其余进 `skipped(unsupported language)`，不占预算 | 是；先判扩展名再读 blob |
| 目录聚合优先 | 是，`directories` 在裁剪中最后才动 |
| 文件按「导出项数 × 类型重要度」降序 | 是，权重表见 plan §4（类型 3 / 函数 2 / 其余 1，导出 ×2） |
| 先丢 items（文件保留 + `items_omitted`），再整文件进 `skipped(budget)`；不切断 signature | 是；`signatures_are_never_shortened` 在 6 个预算档位下逐字比对 |
| `estimated_tokens` 口径写进 plan 且自检 ≤ budget | 是；口径 `ceil(json_chars/4)`（整响应体），`estimated_tokens_never_exceed_the_budget` 扫描 64–2000 |
| `truncated: true` 当且有内容因预算被丢弃 | 是 |
| 单次最多 2000 文件、单文件 512 KiB、并发上限 8 | 是；`map::MAX_FILES` / T09 `MAX_FILE_BYTES` / `MAP_PARSE_THREADS = 8`（`std::thread::scope` 分块，每线程 `open_repo` 一次） |
| 2000 文件超 3 秒要如实报告，不许偷偷降上限 | 未超；debug 1.07–1.76 s、release 0.138–0.166 s，上限保持 2000，数据见 plan §5 |
| 权限与 `ai_repo_tree` 同路径，未授权不泄露存在性 | 是；`repo_access::require_read`，集成测试覆盖 401/403/200 |
| `ref` 不存在 → 404 `REF_NOT_FOUND` | 是 |
| 只读端点不写审计 | 是，未调用任何审计写入 |
| 契约三处同时补齐 | 是：`lib.rs` 路由、`openapi.rs`、`route_meta.rs`，两个漂移测试全绿 |
| 不改 migrations / `CLAUDE.md` / `ironforge-docs/` / `.cnb.yml` / `.github/` / `scripts/` / `deploy/` / `rg-mcp` | 是，全部未触碰 |
| 只推分支，不 push main，不 force-push | 是，只推 `agent/t10-repo-map` |

## 2. 触碰路径

生产代码：

- `crates/rg-core/src/search/map.rs`（新建，裁剪算法与响应结构）
- `crates/rg-core/src/search/map/tests.rs`（新建，16 个单测）
- `crates/rg-core/src/search/mod.rs`（`pub mod map;`）
- `crates/rg-core/src/auth/route_meta.rs`（加 1 行 `repo("/ai/repos/{owner}/{name}/map")`）
- `crates/rg-http/src/api/ai.rs`（加 `MapQuery`、`ai_repo_map` handler 与解析/并发辅助）
- `crates/rg-http/src/lib.rs`（**仅路由注册段**加 1 行）
- `crates/rg-http/src/openapi.rs`（加 1 行 `paths(...)`）

测试：

- `crates/rg-http/tests/ai_repo_map_tests.rs`（新建，10 个集成测试）
- `crates/rg-http/tests/map_perf_probe.rs`（新建，`#[ignore]` 性能探针，用于复现 plan §5 的数字）

文档：

- `docs/plans/T10-plan.md`（新建）
- `docs/plans/T10-task.md`（本文件，新建）
- `tasks/T10.md`（仅 `State` → `completed`）

未触碰：`Cargo.toml` / `Cargo.lock`、`crates/rg-core/src/search/outline/{mod,queries,tests}.rs`、`crates/rg-core/src/search/indexer.rs`、`crates/rg-db/src/migrations/`、`CLAUDE.md`、`ironforge-docs/`、`.cnb.yml`、`.github/`、`scripts/`、`deploy/`、`crates/rg-mcp/`。

## 3. 验收命令真实输出

### 3.1 `cargo test -p rg-core`

```
test result: ok. 359 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 10.75s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 0 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.58s
test result: ok. 1 passed; 0 failed; 5 ignored; 0 measured; 0 filtered out
```

其中 `search::map::tests` 16 个：

```
test search::map::tests::a_budget_equal_to_the_full_estimate_is_not_truncated ... ok
test search::map::tests::a_file_only_leaves_the_map_after_its_items_are_gone ... ok
test search::map::tests::a_tight_budget_keeps_a_few_complete_outlines_not_many_shells ... ok
test search::map::tests::ample_budget_keeps_every_item_and_reports_no_truncation ... ok
test search::map::tests::budget_is_clamped_into_the_supported_range ... ok
test search::map::tests::directories_aggregate_counts_and_languages ... ok
test search::map::tests::empty_repository_yields_an_empty_map ... ok
test search::map::tests::estimated_tokens_never_exceed_the_budget ... ok
test search::map::tests::items_are_dropped_before_the_whole_file_is_dropped ... ok
test search::map::tests::only_unsupported_paths_all_go_to_skipped ... ok
test search::map::tests::outline_errors_without_items_become_skipped_entries ... ok
test search::map::tests::ranking_prefers_type_heavy_files ... ok
test search::map::tests::signatures_are_never_shortened ... ok
test search::map::tests::svelte_is_flagged_as_a_degraded_outline ... ok
test search::map::tests::the_serialised_body_is_within_four_chars_per_token_of_the_estimate ... ok
test search::map::tests::tiny_budget_keeps_the_directory_layer_only ... ok
```

### 3.2 `cargo test -p rg-http`

全部 test target 通过，0 失败（含新增 `ai_repo_map_tests` 10 个）：

```
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.02s   # ai_repo_map_tests
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s    # route_metadata_tests
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.18s    # openapi_registration_tests
# 其余 target 见 `cargo test -p rg-http` 输出，均 0 failed
```

`ai_repo_map_tests` 明细：

```
map_lists_source_files_and_reports_the_rest_as_unsupported ... ok
map_ranks_structs_above_helpers_and_keeps_signatures_whole ... ok
map_flags_svelte_as_a_degraded_outline ... ok
map_trims_to_a_small_budget_without_exceeding_it ... ok
map_rejects_an_invalid_budget ... ok
map_reports_a_missing_ref_with_its_own_code ... ok
map_scopes_to_a_subdirectory ... ok
map_of_an_empty_repository_is_empty_not_an_error ... ok
map_requires_read_access_to_a_private_repository ... ok
map_reports_large_and_binary_files_as_skipped ... ok
```

### 3.3 `cargo clippy --workspace --all-targets -- -D warnings`

```
    Checking rg-core v0.2.0
    Checking rg-http v0.2.0
    Checking rg-ssh v0.2.0
    Checking rg-ci v0.2.0
    Checking rg-cli v0.2.0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 18.45s
```

0 warning / 0 error。

### 3.4 `scripts/check.sh --quick`

```
==> cargo fmt --all --check
    (1s)
==> cargo clippy --workspace --all-targets -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.83s
    (2s)
==> checks passed in 3s (quick)
```

### 3.5 性能（`crates/rg-http/tests/map_perf_probe.rs`，2000 文件）

```
debug   budget=4000   elapsed=1.073s  estimated=3991  files=15  skipped=3
debug   budget=32000  elapsed=1.760s  estimated=31989 files=32  skipped=8
release budget=4000   elapsed=0.138s  estimated=3991  files=15  skipped=3
release budget=32000  elapsed=0.166s  estimated=31989 files=32  skipped=8
```

（debug 三次重复 1.07/1.08/1.09 s，1.76/1.76/1.76 s，稳定。）

## 4. 裁剪单测清单（对应 `tasks/T10.md` §2 的必须覆盖场景）

| 场景 | 测试 |
| --- | --- |
| 预算极小（只容得下目录） | `tiny_budget_keeps_the_directory_layer_only`（`MIN_BUDGET`）、`a_file_only_leaves_the_map_after_its_items_are_gone`（120） |
| 预算刚好 | `a_budget_equal_to_the_full_estimate_is_not_truncated`（用高预算版本的 `estimated_tokens` 作为下一次预算，逐字段相等且不 truncated） |
| 预算充裕 | `ample_budget_keeps_every_item_and_reports_no_truncation` |
| 全非源码仓库 | `only_unsupported_paths_all_go_to_skipped` |
| 空仓库 | `empty_repository_yields_an_empty_map` |
| 附加 | `estimated_tokens_never_exceed_the_budget`（64–2000 扫描）、`signatures_are_never_shortened`、`ranking_prefers_type_heavy_files`、`items_are_dropped_before_the_whole_file_is_dropped`、`a_tight_budget_keeps_a_few_complete_outlines_not_many_shells`、`directories_aggregate_counts_and_languages`、`outline_errors_without_items_become_skipped_entries`、`svelte_is_flagged_as_a_degraded_outline`、`budget_is_clamped_into_the_supported_range`、`the_serialised_body_is_within_four_chars_per_token_of_the_estimate` |

## 5. 遗留风险（详版见 plan §10）

1. `estimated_tokens` 是 `ceil(chars/4)` 口径的**软上限**；CJK 内容下第三方 tokenizer 结果可能高 1.5–2.5×。
2. 预算极紧时 `skipped` 里可能看不到 `budget` 条目（内容优先，剩余额度才填 `skipped`）；客户端只能靠 `truncated` + `directories[].files` 计数判断完整性。
3. `MAX_FILES=2000` 的截断按路径字典序取前 2000（重要度需解析后才知道），大仓里字典序靠后的"更重要"文件可能先被排除。
4. 无缓存：每次请求重新遍历 + 解析同一 commit（索引持久化在任务范围外）。
5. symlink / submodule 静默跳过（不计入 `skipped`），`SkipReason` 未覆盖它们。
6. `path` 只接受目录；指向文件 404（与 `/tree` 一致）。
7. 未在真实大仓（>2000 源文件、含 CJK 注释）上验证；数字来自生成的 2000 文件夹具。
8. 未跑 CI（CNB/GitHub 当前无可用运行环境），以上均为本机结果。
