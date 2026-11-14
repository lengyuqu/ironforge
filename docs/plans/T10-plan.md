# T10 执行计划 — [#39] 代码智能 · `/ai/repos/{o}/{r}/map` 端点 + token 预算裁剪

> 权威约束见 `tasks/T10.md`。基线 `faec7c9`，分支 `agent/t10-repo-map`，worktree `../ironforge-t10`。
> 本文件记录端点契约、裁剪算法的实现映射、token 估算口径与误差、性能实测数字，以及 Svelte 降级在输出中的呈现方式。

## 1. 交付边界

做：

1. `rg-core::search::map`：预算裁剪算法（`build`）、`RepoMap` 响应结构、`SkipReason` 分类。
2. `GET /api/v1/ai/repos/{owner}/{name}/map?ref=&budget=&path=` handler（`rg-http/src/api/ai.rs`），实时解析、不落库。
3. 契约三处同步：`rg-http/src/lib.rs` 路由、`openapi.rs`、`rg-core/src/auth/route_meta.rs`。
4. 单测（裁剪算法，16 个）+ 集成测试（HTTP 契约，10 个）。

不做（明确排除）：符号图（call/reference）、向量后端、MCP 工具、索引持久化、审计写入。

未改：`Cargo.toml` / `Cargo.lock`（本任务无新依赖）、`crates/rg-db/src/migrations/`、`CLAUDE.md`、`ironforge-docs/`、`.cnb.yml`、`.github/`、`scripts/`、`deploy/`、`crates/rg-mcp/`、T09 的 `search/outline/` 三个文件。

## 2. 端点契约

```
GET /api/v1/ai/repos/{owner}/{name}/map?ref=<branch>&budget=<tokens>&path=<subdir>
```

| 参数 | 必填 | 行为 |
| --- | --- | --- |
| `ref` | 否 | 分支/标签/SHA，缺省 `repo.default_branch`；走 `repo.rev_parse_single`，与 `ai_repo_tree` 同一套解析 |
| `budget` | 否 | 默认 4000，有效区间 `[64, 32000]`；非整数 / 越界 → 400 `INVALID_BUDGET` |
| `path` | 否 | 只映射该子目录；不存在或是文件 → 404 |

响应字段：

```jsonc
{
  "ref": "main", "commit_sha": "…40 hex…",
  "budget": 4000, "estimated_tokens": 3991, "truncated": true,
  "directories": [ { "path": "crates/rg-core/src", "files": 42, "languages": ["rust"] } ],
  "files": [
    { "path": "crates/rg-core/src/search/map.rs", "language": "rust",
      "items": [ { "name": "build", "kind": "function", "start_line": 200, "end_line": 260,
                   "signature": "pub fn build(", "exported": true,
                   "children": [ … ] } ],
      "items_omitted": true,            // 仅当本条文件的 items 被预算丢过
      "notice": "svelte: degraded outline, …" }  // 仅当抽取有保留
  ],
  "skipped": [ { "path": "README.md", "reason": "unsupported language" } ]
}
```

`skipped.reason` 取值：`unsupported language` / `too many files` / `file too large` / `binary content` / `unreadable` / `parse error` / `budget`。

错误码：

| 场景 | 状态 | `error.code` |
| --- | --- | --- |
| 仓库不存在 / 私有仓库无权限 | 404 / 403（无 token 401） | `NOT_FOUND` / `FORBIDDEN` / `UNAUTHORIZED`（与其余 AI 端点完全一致，不泄露存在性） |
| `ref` 不存在 | 404 | `REF_NOT_FOUND` |
| `path` 不存在 | 404 | `NOT_FOUND` |
| `budget` 非法 | 400 | `INVALID_BUDGET` |

`budget` 按字符串手解析（`MapQuery.budget: Option<String>`），否则 axum 的 `Query` 提取器会先把它变成通用的 400 校验错误，拿不到 `INVALID_BUDGET`。

## 3. token 估算口径与误差

**口径**：`estimated_tokens = ceil(json_chars / 4)`，`json_chars` 是**整个响应体**（含 `ref`/`commit_sha`/`budget`/`estimated_tokens` 自身）序列化后的字节数（ASCII 边界下字节数 = 字符数；非 ASCII 路径/签名按 UTF-8 字节计，偏保守）。

实现见 `RepoMap::measure()`：先把 `estimated_tokens` 置 0 序列化得到基数，再对位数做不动点迭代（最多 4 轮，实际 1–2 轮收敛），避免"估算数字本身改变长度"的循环依赖。

**误差**：

| 内容 | 实际 tokens / 4 字符估算 | 说明 |
| --- | --- | --- |
| ASCII 源码（标识符、签名、路径） | 0.85–1.0× | BPE 对 `snake_case`/`::` 通常 3–4 字符一个 token，估算略微保守 |
| 中文注释、CJK 路径 | 1.5–2.5× | CJK 多为 1.5–2 字节/token，估算偏低，这是**已知误差**；此类仓库实际消耗可能超过 `estimated_tokens` |

因此 `estimated_tokens` 是"软上限"：保证的是"按 4 字符/token 口径不超过预算"，不保证任何第三方便携 tokenizer 的结果也不超。计划里显式记录这一点，避免把估算当成精确计费。

**自检**：`build` 在裁剪收敛后调用 `measure()` 实测，若仍超预算则继续按"items → files → skipped → directories"降级，直到实测 `estimated_tokens <= budget`，因此对外恒定成立（单测 `estimated_tokens_never_exceed_the_budget` 在 `64..=2000` 每 7 个预算点扫描）。

`MIN_BUDGET = 64` 的理由：空响应（`{"ref":…,"commit_sha":…,"budget":…,"estimated_tokens":…,"truncated":…,"directories":[],"files":[],"skipped":[]}`）本身约 120 字符 ≈ 30 tokens，低于此值的预算无法诚实满足。

## 4. 裁剪五步的实现映射（对应 `tasks/T10.md` §2）

| 任务要求 | 实现位置 | 做法 |
| --- | --- | --- |
| ① 只考虑源码文件，其余进 `skipped`，不占预算 | `ai.rs::collect_source_blobs` | 遍历 git tree，`Language::from_path(path).is_supported()` 为假 → `skipped(unsupported language)`，**不读 blob**；symlink / submodule 是指针不是源码，静默跳过（不谎报语言） |
| ② 目录聚合优先 | `map.rs::aggregate_directories` | 按父目录聚合 `files` 计数与语言集合（`BTreeMap`，输出有序）；目录层每个条目约 60 字符，`directories` 在裁剪中最后才动 |
| ③ 文件排序：导出项数量 × 类型重要度 | `map.rs::node_weight` / `kind_weight` | `Struct/Class/Trait/Interface/Enum/Type = 3`，`Function/Impl/Module = 2`，`Method/Const/Macro/Other = 1`，导出项 ×2；子项（方法/字段）权重并入父项。文件权重 = 根项权重之和，降序排列，同权按路径升序保证稳定 |
| ④ 截断顺序：先丢 items（文件保留 + `items_omitted`），再整文件进 `skipped(budget)`；不切断 signature | `map.rs` 裁剪主循环 + `FileState::drop_least_important_item` | 从**权重最低的文件**开始：每次丢该文件当前权重最低的**整个根项**（保留源顺序），置 `items_omitted: true`；该文件项被清空后才整文件移出 `files`。signature 从不截断——item 要么完整保留要么整项丢弃 |
| ⑤ `estimated_tokens` 口径写进 plan + 自检 ≤ budget | 本文 §3 + `RepoMap::measure` + 安全网循环 | 见 §3；裁剪后实测再校一次，不够继续降级 |
| ⑥ `truncated` 当且仅当有内容因预算被丢弃 | `map.rs` | `truncated = 有 item 被丢 || 有文件被移出 || 有 skipped 条目因预算被省略 || 目录被裁` |

两处偏离字面描述、但更符合"给定预算产出最有信息量的结构视图"这一目标，记录在此：

1. **逐文件裁剪，而非"先把所有低权重文件的 items 全丢光"**。2000 文件 × 24 个声明时，先丢光全部 items 会把预算花在 2000 个空壳上，最终 `files` 变成 0。实测（本地 2000 文件）改前 `budget=4000` 得到 `files=0, skipped=350`；改后 `files=15, skipped=3`。现在的顺序是：最低权重文件先丢 item → 该文件空了就整文件移出 → 再处理下一个，直到预算够用。
2. **`skipped` 不参与"内容"竞争，只用剩余预算**。若把每条 `skipped` 也算进裁剪总量，2000 条 "budget" 记录会吃掉全部预算，同样把 `files` 挤成 0。现在先按内容（目录 + 文件）裁到预算内，再把**剩余额度**按 `unsupported language` 等真实拒绝项 → `budget` 项的顺序填 `skipped`，填不下的条目直接省略（`truncated` 已表明地图不完整）。因此预算很紧时可能看不到 `budget` 条目——这是有意的取舍，内容优先。

## 5. 规模与性能保护

| 保护 | 值 | 实现 |
| --- | --- | --- |
| 单次解析文件数 | 2000（`map::MAX_FILES`） | 源码 blob 路径排序后取前 2000，其余进 `skipped(too many files)`，不读内容 |
| 单文件字节上限 | 512 KiB（T09 `MAX_FILE_BYTES`） | 遍历 tree 时先用 `find_header` 读 size，超限直接 `skipped(file too large)`，不进解析队列；`read_source` 再校验一次 |
| 解析并发 | 8（`MAP_PARSE_THREADS`） | `std::thread::scope` 分块，每条线程 `open_repo` 一次、处理自己的连续分片，结果按分片顺序合并（确定性） |
| 二进制 | NUL 字节检测 | `read_source` 提前判 `binary content`，`extract_outline` 内还有一层 |
| UTF-8 | `std::str::from_utf8` | 非法 → `skipped(unreadable)`，不再进 tree-sitter |

### 5.1 实测（2000 文件，每文件 12 个 `pub struct` + 12 个 `impl`（各 2 方法），约 3.6 KB/文件）

测量方法：`crates/rg-http/tests/map_perf_probe.rs`（`#[ignore]` 探针，建仓 → push → HTTP 请求计时，串行 3 次）。测试仓与 worktree 同机同 worktree，`MAC` arm64。

| 构建 | budget | 耗时 | body | `estimated_tokens` | `files` | `skipped` |
| --- | --- | --- | --- | --- | --- | --- |
| debug | 4000 | 1.07 s / 1.08 s / 1.09 s | 15 961 B | 3 991 | 15 | 3 |
| debug | 32000 | 1.76 s / 1.76 s / 1.76 s | 127 956 B | 31 989 | 32 | 8 |

release 数字见 §5.2。

结论：**debug 构建下 2000 文件约 1.1 s（budget=4000）/ 1.8 s（budget=32000），低于 3 s 目标**，未通过下调 `MAX_FILES` 达标（上限保持 2000）。

耗时构成（按代码路径推断，未单独插桩）：tree-sitter 解析占绝大部分（budget 从 4000 到 32000 解析量完全相同，但耗时差 0.7 s 来自裁剪与序列化的 item 数量差异，说明解析本身约 1 s）；git blob 读取与 tree 遍历是本地 pack 的顺序读，远小于解析。

### 5.2 release 实测

同一探针，`cargo test --release -p rg-http --test map_perf_probe -- --ignored`：

| 构建 | budget | 耗时 | body | `estimated_tokens` | `files` | `skipped` |
| --- | --- | --- | --- | --- | --- | --- |
| release | 4000 | 0.138 s | 15 961 B | 3 991 | 15 | 3 |
| release | 32000 | 0.166 s | 127 956 B | 31 989 | 32 | 8 |

release 下 2000 文件全量解析约 **0.14–0.17 s**，远低于 3 s；debug 的 1.1–1.8 s 主要是未优化的 tree-sitter 代码。报告两个数字是因为验收命令 `cargo test` 默认跑 debug。

## 6. 五语言覆盖与 Svelte 降级

抽取层来自 T09（`extract_outline`），本任务只消费：

| 语言 | 扩展名 | 抽取方式 | 地图中的表现 |
| --- | --- | --- | --- |
| Rust | `.rs` | `tree-sitter-rust` 真 AST | `struct/enum/trait/impl/fn` 层级，`impl` 内方法为 children，`pub` → `exported: true` |
| TypeScript | `.ts/.mts/.cts/.js/.jsx/.mjs/.cjs` | `tree-sitter-typescript`（TS grammar 是 JS 超集） | `class/interface/function`，`export` → `exported` |
| TSX | `.tsx` | `tree-sitter-typescript` 的 TSX grammar | 同上 |
| Python | `.py/.pyi` | `tree-sitter-python` | `class/def`，无前导 `_` 视为导出 |
| Go | `.go` | `tree-sitter-go` | `type/struct/interface/func`，首字母大写视为导出 |
| Svelte | `.svelte` | **降级**：文本定位 `<script>` 块，块内用 TS grammar 解析；markup 不解析 | 见下 |

**Svelte 降级如何在输出中体现**（不假装完整）：

1. 该文件在 `files` 里的 `language` 是 `"svelte"`；
2. 必带 `notice`：`"svelte: degraded outline, only <script> declarations are parsed (markup is not)"`；
3. 行号已按 `<script>` 内容起始行偏移回 `.svelte` 文件（`start_line`/`end_line` 是真实文件行号，不是块内行号）；
4. 只有 markup 没有任何 `<script>` 的文件 → `items: []` 且无 error，会作为 `files` 里 `items: []` 的条目出现（`items_omitted` 不置位，因为不是预算导致的）；
5. `<script>` 内有语法错误时，`notice` 追加 `partial outline: …`。

集成测试 `map_flags_svelte_as_a_degraded_outline` 断言 `notice` 同时含 `svelte` 与 `markup`。

## 7. 权限与一致性

- 鉴权：`repo_access::require_read(&state, &headers, &owner, &name)`，与 `ai_repo_tree` / `ai_search_code` / `ai_repo_summary` 同一入口，未自造。
- 私有仓库：未带凭据 401、非成员 403、成员 200（集成测试 `map_requires_read_access_to_a_private_repository`）。
- `ref` 解析走 `repo.rev_parse_single`；空仓库（unborn HEAD）返回空地图而非错误，与 `/tree` 一致；解析失败 → `CoreError::coded(404, "REF_NOT_FOUND", …)`，经 `From<CoreError> for AppError` 保留 code。
- 审计：只读端点，不写审计（与 `ai_repo_tree` 一致）。
- 返回结构是普通 `Json<RepoMap>`，不加缓存头（现有 AI 端点同样不加）。

## 8. 契约三处

| 位置 | 改动 |
| --- | --- |
| `crates/rg-http/src/lib.rs` | `/ai/repos/{owner}/{name}/map` → `get(api::ai::ai_repo_map)`，紧挨 `/tree` |
| `crates/rg-http/src/openapi.rs` | `crate::api::ai::ai_repo_map` 加入 `paths(...)` |
| `crates/rg-core/src/auth/route_meta.rs` | `repo("/ai/repos/{owner}/{name}/map")` |

`rg-http/tests/route_metadata_tests.rs` 与 `openapi_registration_tests.rs` 会分别校验路由 ↔ route_meta、`#[utoipa::path]` ↔ openapi 列表；两处都补了，测试是绿的。`RepoMap` 定义在 `rg-core`（无 utoipa 依赖），因此 OpenAPI 里用 `body = serde_json::Value` 并在 description 中完整描述字段，避免为文档在 `rg-core` 引入 utoipa。

## 9. 单测与集成测试清单

裁剪算法（`crates/rg-core/src/search/map/tests.rs`，16 个）：

| 测试 | 覆盖 |
| --- | --- |
| `empty_repository_yields_an_empty_map` | 空仓库 |
| `only_unsupported_paths_all_go_to_skipped` | 全非源码仓库：`files`/`directories` 空，`truncated=false` |
| `ample_budget_keeps_every_item_and_reports_no_truncation` | 预算充裕：19 项全保留，无 `items_omitted` |
| `directories_aggregate_counts_and_languages` | 目录聚合（计数 + 语言集，含 TS/Rust 混合） |
| `ranking_prefers_type_heavy_files` | 排序：类型密集文件排在函数密集文件之前 |
| `tiny_budget_keeps_the_directory_layer_only` | 预算极小（`MIN_BUDGET`）：只容得下目录，`truncated=true` |
| `items_are_dropped_before_the_whole_file_is_dropped` | 预算 = 全量一半：存活文件是排序前缀；空 items 必带 `items_omitted`；被丢文件不再出现在 `files` |
| `a_tight_budget_keeps_a_few_complete_outlines_not_many_shells` | 200 文件 / 2000 tokens：顶部文件保留 items，不出现"200 个空壳" |
| `a_file_only_leaves_the_map_after_its_items_are_gone` | 预算 120：`files` 里只剩 `items: [] + items_omitted`，`skipped(budget)` 不与 `files` 重叠 |
| `signatures_are_never_shortened` | 6 个预算档位下 signature 与原始逐字相同 |
| `estimated_tokens_never_exceed_the_budget` | `64..=2000` 每 7 一步扫描，恒 `estimated <= budget` 且 `measure()` 与自报一致 |
| `a_budget_equal_to_the_full_estimate_is_not_truncated` | 预算"刚好"：内容与高预算版本完全一致，`truncated=false` |
| `outline_errors_without_items_become_skipped_entries` | `file too large` 的 outline 进 `skipped` |
| `svelte_is_flagged_as_a_degraded_outline` | Svelte `notice` |
| `budget_is_clamped_into_the_supported_range` | 越界预算被 clamp（HTTP 层已先拒绝） |
| `the_serialised_body_is_within_four_chars_per_token_of_the_estimate` | 估算口径 = ceil(chars/4) |

HTTP 契约（`crates/rg-http/tests/ai_repo_map_tests.rs`，10 个）：基础形状/排序/签名完整、Svelte notice、极小预算裁剪、`INVALID_BUDGET`（6 种非法输入）、`REF_NOT_FOUND` + `refs/heads/main` 等价、子目录作用域 + 文件路径 404、空仓库、私有仓库 401/403/200、超大文件进 `skipped(file too large)`。

## 10. 已知限制与遗留风险

1. **估算不是计费**：CJK 内容下 4 字符/token 会低估（见 §3），Anthropic/OpenAI tokenizer 结果可能高于 `estimated_tokens`。
2. **预算极紧时不列 `budget` 条目**：见 §4 偏离说明 2；`truncated=true` 是唯一提示，客户端不能从 `skipped` 反推被丢文件清单。
3. **`MAX_FILES=2000` 超出部分的判定顺序**：按路径字典序取前 2000，不是按重要度——重要度需要解析后才能算。极端大仓会把字典序靠后的"更重要"文件先排除。已知取舍，未在本次扩大上限。
4. **没有缓存**：每次请求重新遍历 + 解析；同一 commit 重复请求成本相同。索引持久化明确在本任务范围外。
5. **symlink / submodule 静默跳过**：不计入 `skipped`（它们不是源码文件），若将来需要可见性需扩展 `SkipReason`。
6. **`path` 只接受目录**：指向文件返回 404（与 `/tree` 一致）。
7. **release 数字** 见 §5.2；debug 数字已低于 3 s，但生产以 release 为准。
