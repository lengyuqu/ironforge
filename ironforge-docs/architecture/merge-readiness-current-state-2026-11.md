# 合并就绪与冲突详情：现状核实（issue #17）

> **对象**：issue #17 的 NPC 细化方案（评论 `2120150507300745216`，`2026-11-10T13:44:06Z`，15403 字符，首行 `## @cnb.biewen(别问) Issue #17 细化方案：合并就绪诊断 + 更新分支`）。
> **BASE**：`ae11e36`（`cnb/main`，2026-11-12T01:52Z 拉取，`git rev-list --left-right --count HEAD...cnb/main` = `0 0`）。
> **方法**：每条断言在 BASE 树上用 `awk 'NR==n'` / `sed -n` 打印原文；gix API 断言对照 `~/.cargo/registry/src/rsproxy.cn-*/gix-merge-0.23.0`、`gix-0.90.0` 源码（第 4 次进入轮订正：`Cargo.lock` 在 `ae11e36` / `0a9a7dd` / HEAD 三棵树都是 `gix 0.90.0`，`gix-merge` 是 `0.23.0`；registry 里另存的 `gix-0.84.0` 目录是 `72d0ce8` 时代的旧快照，勿再引用）。
> **结论**：方案主体可用（冲突数据确实不用新原语）；**33 条现状断言：成立 21 / 部分成立 10 / 不成立 2**（N19 于第 2 次进入轮翻案改判成立，见 §8 末段）。真正卡住落地的不是代码，是三处**判定语义**（§4）和一处**端点命名冲突**（§6）。
> **现状漂移（第 2 次进入轮复取，`649a814`）**：本文所有 `path:line` 按 `ae11e36` 树取值。`rg-http/src/lib.rs` 由 4045 行减到 3992 行 ⇒ `spawn_post_push_hooks` 定义 `:3202`→`:3141`、push handler 调用点 `:3163`→`:3095`、`repo_content.rs:1382`→`:1384`；`auth/pat_scope.rs` 的 `ref_limited_write_allowed` `:301`→`:307`（文档行 `:299`→`:305`）。**其余被引文件与 `ae11e36` 逐行相同的说法已于第 3 次进入轮收紧**：两处例外都不由本节改动产生 —— `branch_protection/service.rs:426` 是本流程 `0a9a7dd`（第 1 次进入轮）改的 1 行注释，行号不变、文本已换；`api/repo_commits.rs` 由 `cf6943d`（提交身份特性）改 `5+/2−`，本文别处未引用它的行号（原写「本文未引用」，而该句本身就在引用它，措辞已订正）。`git diff ae11e36..HEAD --stat` 逐文件可复算。

---

## 1. 合并链路现状（真值表）

| 环节 | 位置 | 现在做什么 |
|---|---|---|
| 权限 | `rg-http/src/api/pulls.rs:556` | `require_write`（写权限即可，不需要 admin） |
| 草稿 | `pulls.rs:572` | `is_draft` → 409 `CONFLICT`，纯文本 `"draft pull requests cannot be merged"` |
| 分支保护 | `pulls.rs:574` → `rg-core/src/branch_protection/service.rs:427` | `check_merge_allowed`，见 §2 |
| 实时 head | `pulls.rs:590-591` | `rg_core::pull_request::live_head_sha`（`service.rs:1360`；fork PR 读 fork 仓库） |
| 期望 head | `pulls.rs:597-608` | 不符 → 409 **`REF_ADVANCED`** + `details.current_sha/expected_sha` + `hint`（`AppError::conflict_because`，`error.rs:266`） |
| 状态 | `pulls.rs:609` → 再查一次 `rg-core/src/pull_request/service.rs:1321` | `state != "open"` → 409 `CONFLICT`；core 侧是 dry-run 之外的二次校验，另加 `is_draft`（`:1327`）与 `claim_merge` 并发占用（`:1335`） |
| 合并计算 | `service.rs:1943` `gix_merge_commits_to_tree` | 返回 `(tree_id, Vec<gix::merge::tree::Conflict>)`；**有冲突就丢向量换成文本** |
| 失败响应 | `pulls.rs:678` `MergeError::Core(e) => AppError::from(e)` | 409 `CONFLICT` + message `"merge conflict detected: N files with conflicts"`，**无 `reason` / `retryable` / `hint` / `details`** |

`check_merge_allowed` 的**三个生产调用点**（readiness 若另写一套判定，必然与这三处漂移）：`pulls.rs:574`（合并执行）、`service.rs:1115`（auto-merge 可用性）、`crates/rg-core/src/pull_request/merge_queue.rs:267`（合并队列准入；本文件给全路径 —— 理由是 `crates/rg-core` 一个 crate 内就有 **19** 个同名 `service.rs` 分属不同模块（`git ls-files '*service.rs'` = 19，`cut -d/ -f2 | sort -u` = 1），只写文件名的锚点读者定不到唯一位置；第 4 次进入轮订正：原写「本仓多个 crate 下都有同名 `service.rs`」，实测只跨 1 个 crate）。

**「状态」查两次**：handler 的 `:609` 覆盖 dry-run 与真合并两条路，core 的 `merge_pr_expecting_head`（`service.rs:1321`）再查一次（防止 PR 在 handler 校验后被并发改动）。NPC 把 draft / 状态 / 保护 / `expected_head_sha` 都归到 `pulls::merge_pr` 名下（§2 N3）——**这条是准确的**；`check_merge_allowed` 内部那三类（签名 / 批准数 / CI）才是另一层。

## 2. 逐条核实（33 条）

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| N1 | `gix_merge_commits_to_tree`（`service.rs:1943`）遇 `conflicts` 非空就 `CoreError::Conflict("merge conflict detected: N files")`，文件与区块全丢 | **成立** | `:1967` 取 `outcome.tree_merge.conflicts`、`:1968` 判空、`:1969` 只 `tracing::warn!`、`:1970-1973` 才 `return Err`；返回类型本来就带 `Vec<Conflict>`，两个调用点（`:1896` / `:1927`）都写成 `_conflicts`（`:1897` / `:1928` 才是被调函数那行） |
| N2 | `live_head_sha` / `update_ref` / `unified_patch` 都有，不需要新原语 | **成立** | `service.rs:1360`、`rg-git/src/ops.rs:85`、`pull_request/gix_patch.rs:36` |
| N3 | 前置校验分散在 `pulls::merge_pr`（draft / 状态 / 保护 / `expected_head_sha`）与 `check_merge_allowed`（签名 / 批准数 / CI） | **成立** | draft `:571`、状态 `:609`、保护 `:574`、`expected_head_sha` `:597` 全在 handler；签名/批准数/CI 在 `check_merge_allowed`。唯一要收窄的是「没有机器码」：`REF_ADVANCED` 已是机器码，纯文本只覆盖保护类判定 |
| N4 | `/context` 的 `not_evaluated` = `["mergeable","branch_protection"]`，在 `ai_pr_context.rs:386` | **成立** | 行号精确；同行 `:387` 是 `"consistency": "per_section_live_read"` |
| N5 | `anchors.base_sha_when_opened` 已有 | **部分成立** | 只存在于 `pr_export.rs:99-113`：从 `pr_event_ops::list_by_pr` 里找 `pull_request_opened` 事件的 `metadata.base_sha`，取不到就 `not_stored: ["base_sha_when_opened"]`；core 无通用入口 |
| N6 | `observed_sha` 可能领先库里的 `head_sha`，`live_head_sha` 已在做 | **成立** | `service.rs:1356-1358` 文档原文「This is the live tip, which can be ahead of the stored `head_sha`」 |
| N7 | `count_current_approvals` 以 `commit_id == head_sha` 过滤 | **部分成立** | 函数名是 `count_current_human_approvals`（`rg-db/src/ops/pr_review_ops.rs:57`）；`commit_id == Some(head_sha)`（`:74`）成立，但还有两条它没写：每 reviewer 只取最后一条（`:62-68`，后来的 `request_changes` 顶掉 `approve`）、只算 `Kind == human`（`:84-86`） |
| N8 | `require_status_check=true` 但 `required_status_checks` 为 null 时「只要求整体 success」 | **不成立** | `service.rs:467-469` 是三层 `if`：`require_status_check` → `Some(checks_json)` → `if let Ok(Vec<String>)`。null 直接跳过**什么都不查**，连「整体 success」都不要 |
| N9 | `required_status_checks` 坏 JSON 被 `if let Ok` 静默吞掉 ⇒ 配置坏了 = 放行 | **成立** | 同上三层，`Err` 分支无日志、无错误 |
| N10 | `SIGNED_COMMITS_REQUIRED` 永远过不去 | **成立** | `:442-446` 无条件 `forbidden`，文案自陈「server-side PR merge commits are not signed」 |
| N11 | `repo.merge_commits(...)` → `outcome.tree_merge.conflicts: Vec<Conflict>` | **成立** | `service.rs:1962-1964` |
| N12 | `Conflict::entries()` → `[Option<ConflictIndexEntry>; 3]`，别直接读字段 | **成立** | `gix-merge-0.23.0/src/tree/mod.rs:166` 字段 + `:165` 文档「access it through `Self::entries()`」 |
| N13 | `content_merge()` → `Option<ContentMerge>`，`Resolution::Conflict` 才是真冲突；括注「`CompleteWithAutoResolvedConflict` 是有 marker 但已自解」 | **部分成立** | API 名与返回值成立（`:297`、`:420-425`、`blob/mod.rs:19-28`）；括注**错**：该变体原文是 "complete and **free of conflict markers**"（`blob/mod.rs:22-24`，`:25` 是变体名本身），带 marker 的只有 `Resolution::Conflict` |
| N14 | `Err(ResolutionFailure)` 变体直接映射 `kind`，列 5 个变体 | **部分成立** | 5 个变体名都存在；但 enum 共 **11** 个变体（`:367-413`），未覆盖 6 个；且 §2 的 `kind` 七值里 `mode`、`add_add` 无任何变体产出，`unsupported` 只由 `Unknown` 产出 |
| N15 | 区块用「冲突时写进对象库的 marker blob」，不重新 diff | **成立** | gix-merge 树合并通过 `write_blob_to_odb` 回调落库：`tree/function/resolve.rs:35` 与 `:77`（"newly merged blobs through `write_blob_to_odb`"）。**这条如果按常识猜就会判错**——`tree::Outcome` 的文档说树是 "unwritten"，但 blob 是写了的 |
| N16 | `changes_in_resolution()` 取左右条目 | **成立** | `tree/mod.rs:271` |
| N17 | `emit_body` 用同一条管线（`InternedInput` + `diff_with_slider_heuristics` + `UnifiedDiff` + `ContextSize::symmetrical(3)`） | **成立** | `gix_patch.rs:305` / `:309` / `:314`（入口 `:285`） |
| N18 | 复用 `gix_patch::parity_tests` 的 fixture | **成立** | `gix_patch.rs:446` `mod parity_tests`，`:708` 有「fixture 必须出现在参考 diff 里」的断言 |
| N19 | `blob > 1 MiB` 的 content 阈值「和 blame 的阈值一致」 | **成立（第 2 次进入轮翻案）** | `rg-git/src/blame.rs:16` = `pub const MAX_BLAME_BYTES: usize = 1024 * 1024;`，`:90` = `if blob.data.len() > MAX_BLAME_BYTES {` 真在拒（`:91-94` 返 `TooLarge`），`:14-15` 的文档自述「Files larger than this are refused」。上一版判「不成立」是漏读这一行：`git grep -n '1024 \* 1024' -- crates/` 在 `0a9a7dd` 与 `649a814` 两棵树都是 **20** 行命中，`blame.rs:16` 就在其中（第 4 次进入轮补 scope：不带 `-- crates/` 时 `0a9a7dd` 是 **32** 行 / **18** 文件、HEAD 是 **36** 行，「20 行 / 10 文件」只在 `crates/` 口径下成立） |
| N20 | 二进制判定用 `Operation::SourceOrDestinationIsBinary` | **成立** | `gix_patch.rs:316` |
| N21 | `crates/rg-core/src/pull_request/merge_queue.rs:441` 已有 `spawn_blocking` 先例 | **部分成立** | 先例在，但行号是 `:415` 与 `:463`；`:441`（`"merge-group pipeline #{} is {}"`）在 `ensure_merge_group_ci`（fn 头 `:387`）内、`:432` 那个 `match pipeline.status.as_str()` 的分支里 —— `finish_entry` 是 `:120-151`、函数体内 `match` 计数 **0**，它只是 `:435` 在该分支里**被调用**（第 4 次进入轮订正：原写「`:441` 落在 `finish_entry` 的 match 分支里」，两处都错） |
| N22 | `update-branch` 的 409 复用 `AppError::conflict_because`，`details.current_sha` 已有 | **成立** | `error.rs:266`；`pulls.rs:599-605` 与 `:669-675` 等 5 处已在传 `current_sha`/`expected_sha` |
| N23 | `rg_git::ops::update_ref` 是 `PreviousValue::Any` 不能直接用；抄 `delete_ref_if` 的写法 | **部分成立** | `ops.rs:98` 确为 `Any`；但 `delete_ref_if`（`:132`）用的是 `MustExistAndMatch`（`:145`），真正同款是 `:426` 与 `:1591` 的 `ExistingMustMatch` |
| N24 | 存在 `SERVER_COMMIT_NAME` / `SERVER_COMMIT_EMAIL` | **成立** | `service.rs:1857-1858`（`IronForge` / `noreply@ironforge.local`），`commit_as_server` 用它（`:1859`） |
| N25 | `publish_head_ref` 已处理 fork 的对象复制 | **成立** | `service.rs:1396`，文档明写「objects are copied into the target repository first」 |
| N26 | 写完走同一个 `spawn_post_push_hooks`（`rg-http/src/lib.rs:3190`） | **部分成立** | 函数签名在 `:3202`；但原写的「`:3190` 是上一个函数的 `}`」**是错的**——`:3190` 是 `StatusCode::GATEWAY_TIMEOUT,`（`:3189` 起的元组表达式内），上一函数真正在 `:3196` 收 `}`、`:3198` 起才是本函数的文档注释（现状对应 `:3141` / `:3135` / `:3137`）。且它是 `pub(crate)`，rg-core 调不到 ⇒ 若 `update_branch` 落在 core，钩子必须由 handler 触发 |
| N27 | 权限面在 `pat_scope.rs:299` 的 `ref_limited_write_allowed` | **部分成立** | 函数在 `:301`，`:299` 是它的文档行（现状 `:307` / `:305`） |
| N28 | `pulls.rs:629` 的 `not_evaluated` 含 `"conflicts"` | **部分成立** | 内容对，行号偏 1：`":630`；`:628-629` 是「冲突只有真合才知道」的两行注释。另：该数组是 **4 项**（`["conflicts","webhook","ci_trigger","notification"]`），本文只记了 `conflicts`；readiness 若继承这份列表，另三项要不要跟进未定 |
| N29 | 合并失败现在只回一句冲突文本 | **成立** | `MergeError::Core(e) => AppError::from(e)`（`pulls.rs:678`）→ `CoreError::Conflict` 映射到 `AppError::Conflict(msg)`（`error.rs:170`），该变体只有 `code=CONFLICT` + message（`:54` / `:83` / `:98`） |
| N30 | 新路由加在 `lib.rs:841` 附近 | **成立** | `:841` 是 `// Pull Requests`，`:842-843` 是 `"/repos/{owner}/{name}/pulls"` 那条 `route` |
| N31 | 不登记 capability 注册表的话 `collab_contract_tests` 会要求登记 | **不成立** | `collab_contract_tests.rs:47-85` 的 `registered_collab_routes()` 在 `:79` 明确 `if !path.contains("collaboration") { continue; }` ⇒ `/pulls/{n}/merge-readiness` 根本不在采集范围，防漂移测试不会管它。结论（别塞进协作表）仍对，理由错 |
| N32 | `check_merge_allowed` 是内联逻辑，规则判定得抽出来共用 | **成立** | `service.rs:427-540` 单函数内联四类判定，无独立求值函数；三个生产调用点见 §1 |
| N33 | `merge_trees` 已有，成本不高（issue 正文） | **部分成立** | gix 原语已在 rebase 重放里用（`rg-git/src/ops.rs:351`），但那是 tree 级、绑定重放循环内部，不是可复用服务；合并路径用的是 `merge_commits` 而非 `merge_trees` |

**术语订正（不计入 33 条）**：它给的 readiness 示例用 `"consistency": "live"`，而现有 `/context` 的词表是 `"per_section_live_read"`（`ai_pr_context.rs:387`）。同一语义两套词会在 agent 侧变成两个枚举值，建议沿用既有词表。

## 3. `ResolutionFailure` 全量映射（它的表只覆盖 5 / 11）

`gix-merge-0.23.0/src/tree/mod.rs:367-413` 共 11 个变体，建议 `kind` 取 9 值（括号内为它原文的 `kind`）：

| `ResolutionFailure` | 建议 `kind` | 说明 |
|---|---|---|
| `OursModifiedTheirsDeleted` | `delete_modify`（同它） | 一侧改、一侧删 |
| `OursDeletedTheirsRenamed` | `delete_rename` | **它未覆盖** |
| `OursRenamedTheirsRenamedDifferently` | `rename_rename`（同它） | 两侧改成不同路径 |
| `OursRenamedTheirsRenamedToSameLocation` | `rename_same_target` | **它未覆盖** |
| `OursAddedTheirsAddedTypeMismatch` | `type_mismatch`（同它） | 同路径异类型 |
| `OursModifiedTheirsRenamedTypeMismatch` | `type_mismatch` | **它未覆盖**，与上一条共用 kind |
| `OursModifiedTheirsDirectoryThenOursRenamed` | `dir_file` | **它未覆盖** |
| `OursDirectoryTheirsNonDirectoryTheirsRenamed` | `dir_file` | **它未覆盖**，共用 kind |
| `SubmoduleMerge` / `SubmoduleAddAdd` | `submodule`（同它） | 两条并一 |
| `Unknown` | `unsupported`（同它） | 兜底 |

⇒ 它 §2 JSON 里的 `mode` 与 `add_add` 两个枚举值**没有任何变体产出**（模式冲突在此 enum 里被并入 `type_mismatch` 族，add/add 无独立变体）；`mode` 只能来自 `Conflict::entries()` 的 mode 位比较，得另写一层，不能只看 `resolution`。

**源码取证（第 2 次进入轮补，回应复核要求「把 gix 源码 `sed -n` 原文贴进文档」）**：`sed -n '367,413p' gix-merge-0.23.0/src/tree/mod.rs` 共 **11** 个变体，逐条行号 —— `:370 SubmoduleMerge`、`:373 SubmoduleAddAdd`、`:375 OursRenamedTheirsRenamedToSameLocation`、`:377 OursRenamedTheirsRenamedDifferently`、`:382 OursModifiedTheirsDirectoryThenOursRenamed`、`:393 OursDirectoryTheirsNonDirectoryTheirsRenamed`、`:399 OursAddedTheirsAddedTypeMismatch`、`:406 OursModifiedTheirsRenamedTypeMismatch`、`:408 OursDeletedTheirsRenamed`、`:410 OursModifiedTheirsDeleted`、`:413 Unknown`；`Conflict::entries` 的三位注释在 `:161-163`（`entries[0] => Base`）、`:166` 是字段、`:165` 要求走 `Self::entries()`。`blob/mod.rs:22-24` 原文「Conflicts were resolved automatically, even thought the result is complete / and free of conflict markers. / This can only be the case for text-file content merges.」（`:25` 是变体名 `CompleteWithAutoResolvedConflict,`），`:26` 才是 `A conflict is still present in the form of conflict markers.`（`:27` 是变体名 `Conflict,`）⇒ 本节 N13 的括注订正得到原文支持。

**`add_add` 的产出来源（本轮补，原文此前整篇没交代）**：普通文件 add/add **不来自** `ResolutionFailure` —— 两个 `AddAdd` 变体都被限定住：`:371-373` 原文「added different **submodule** commits at the same path」，`:397-403` 原文「was added (or renamed into place) with a different **mode** than theirs, e.g. blob and symlink」。普通 add/add 是一次内容合并：base 位为 `None`（`tree/mod.rs:161`）+ 两侧同路径不同内容 ⇒ `content_merge()` 给 `Resolution::Conflict`。所以 `add_add` 与 `mode` 同属「读 `entries()` 判出来」的一层，**不能只看 `Err` 变体，也不能只看 `resolution`**。

## 4. 判定语义三处必须对齐（否则 readiness 与合并执行漂移）

1. **审批**：唯一口径是 `count_current_human_approvals`（`pr_review_ops.rs:57`）。三条隐含规则：每 reviewer 取最后一条、`commit_id` 必须等于当前 `head_sha`（`head_sha` 为空时要求 `commit_id` 也为空）、**只算 `kind = human`**。readiness 必须复用该函数，不能自己数 `pr_reviews`。
2. **状态检查**：`require_status_check=true` 时存在**两个静默放行口**——`required_status_checks` 为 `null`、以及 JSON 解析失败；只有「解析成功 + 有 pipeline + pipeline 非 success + 逐 job 名匹配」这条正常路径才会拒。readiness 若如实报 `UNKNOWN`，就与合并执行的不拒相矛盾（§7 R17-1）。
3. **签名**：`require_signed_commits` 是本仓唯一「恒不可满足」的门（服务端合并提交无签名），readiness 报 `FAIL` 才与执行一致；报 `NOT_APPLICABLE` 会让 agent 白跑一次 403。

## 5. 现状：冲突数据已经够，缺的是出口

`gix_merge_commits_to_tree` 已经把 `Vec<Conflict>` 拿到手（`:1967`），只是 `:1968-1973` 把它换成了字符串。所以 issue 正文「merge_trees 已有、成本不高」的方向对，但落点要改：**真正缺的不是合并原语，是「冲突向量 → JSON」的出口和一条不吞冲突的调用路径**（`gix_merge_no_ff` / `gix_squash_merge` 两处都得改签名或加一条旁路）。

## 6. 端点命名冲突（必须先拍板）

| 来源 | 端点 | 载荷 |
|---|---|---|
| 本仓既有设计 `agent-principals-design-and-roadmap-2026-11.md` §R5 | `GET …/pulls/{n}/mergeability` | `{mergeable, conflicts:[{path,hunks}], protection:[{rule,ok,code}], required_checks:{pending,failed}}` |
| issue #17 本方案 | `GET …/pulls/{n}/merge-readiness` + `POST …/pulls/{n}/update-branch` | 嵌套 gates + 机器码 + blocks + actions |

两者都未实现，可自由选，**但不能都写进文档**——现在两份文档对同一功能给了两个端点名与两套字段，谁先实现另一个就变成死文档。

## 7. 待拍板（L2，本流程不动 main）

| 编号 | 优先级 | 内容 | 为什么是 L2 |
|---|---|---|---|
| **R17-1a** | P1 | 两个静默放行口（null / 坏 JSON）先加**可观测性**：`service.rs:534-535` 两个闭括号前各加一条 `tracing::warn!`，readiness 如实报 `UNKNOWN` + 机器码 `STATUS_CHECK_NOT_CONFIGURED` | **零行为改动**，可立即做；先量出「有多少仓库会因 fail closed 被拒」，再谈 R17-1b |
| **R17-1b** | P1 | 同上改 fail closed（需 migration 说明 + 按仓库灰度：warning-only → 拒合） | 改的是既有仓库的合并行为（现在能合的以后会被拒），且必须先有 R17-1a 的数据 |
| **R17-2** | P1 | 端点与字段以哪份文档为准（`/mergeability` vs `/merge-readiness` + 机器码）。**验收条件**：若选 `/merge-readiness`，`agent-principals-design-and-roadmap-2026-11.md` §R5 的 `/mergeability` 必须同步改或显式标废弃——该段末尾（现 `:518`）已声明「本段的 `/mergeability` 与 §6 冲突、以 §6 裁决为准」，但下一段（现 `:520`）仍写着 `GET …/mergeability`，属「有标注、无废弃」 | OpenAPI 与前端契约 |
| **R17-3** | P2 | readiness 的审批结论必须复用 `count_current_human_approvals`（human-only + 最新一条 + commit_id） | 与 auto-merge / merge queue 三处共用同一判定 |
| **R17-4** | P2 | `kind` 枚举按 §3 补到 9 值；`mode` 需另读 `entries()` 的 mode | 响应 schema |
| **R17-5** | P2 | 新增 `update_ref_if`（`PreviousValue::ExistingMustMatch`）；`delete_ref_if` 现用 `MustExistAndMatch`，不能直接抄 | rg-git ops 层新增原语 |
| **R17-6** | P2 | fork PR 的 `update-branch` 要往**他人仓库**的分支写，写权归属未定义（目标库 maintainer 能否改 fork 分支） | 权限语义 |
| **R17-7** | P3 | `spawn_post_push_hooks` 是 `pub(crate)`（`lib.rs:3141`；基线 `ae11e36` 时是 `:3202`）⇒ 钩子只能由 handler 触发；`update_branch` 若落在 core，CI/webhook 会静默不发。现有 2 个调用点全在 `rg-http`（`lib.rs:3095`、`repo_content.rs:1384`），core 里零调用 | 分层与副作用边界 |

## 8. 判「不成立」的两条（勿重复提）

1. **N8**：`require_status_check=true` + `required_status_checks=null` ≠「只要求整体 success」，而是**什么都不查**（`service.rs:467-469` 三层 `if`）。
2. **N31**：`collab_contract_tests` 只采集路径里含 `collaboration` 的路由（`collab_contract_tests.rs:79`），新 pull 路由不在范围内，防漂移测试不会要求登记。

**已翻案（勿再按旧结论引用）**：N19 上一版被判「不成立」，第 2 次进入轮实测 `rg-git/src/blame.rs:16` 有 `MAX_BLAME_BYTES = 1024 * 1024`、`:90` 真在用它拒文件 ⇒ 改判**成立**，§2 表与上面的分布计数已同步；反向清单里那条旧结论随台账本轮条目一并作废。

## 9. 未做

- 未改任何代码行为：本轮只核实现状 + 订正文档与一处注释（`check_merge_allowed` 的文档行写明 null/坏 JSON 的放行语义）。
- 未实测冲突提取：`write_blob_to_odb` 的结论来自 gix-merge 源码（`resolve.rs:35`/`:77`），不是跑出来的；要落地仍应先用一个真冲突 fixture 验一遍 `content_merge().merged_blob_id` 能用 `find_object` 取到。
- 未评工作量估计（5.5 人天）与上限常量（50 文件 / 20 块 / 4 KiB）——纯设计取值，没有可核实对象。
- **第 2 次进入轮（`649a814`）只做文档订正**：N19 翻案、`add_add` 产出来源、gix 源码原文取证、R17-1 拆 a/b、R17-2 补验收条件、`lib.rs` / `pat_scope.rs` 锚点漂移登记；`crates/` **零行**改动，未动 `Route`、未动任何合并行为。
- 已修（`d2a6524`，issue #17 第 5 次进入轮）：`ironforge-docs/README.md` 索引行原写「20 成立 / 10 部分 / 3 不成立」，与本文 §2 的 21 / 10 / 2 不一致，现已订正为 21 / 10 / 2 —— 本行原文「留待下一轮 L1 订正」已过期，改为现状陈述。
