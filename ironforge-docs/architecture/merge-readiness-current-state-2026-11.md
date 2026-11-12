# 合并就绪与冲突详情：现状核实（issue #17）

> **对象**：issue #17 的 NPC 细化方案（评论 `2120150507300745216`，`2026-11-10T13:44:06Z`，15403 字符，首行 `## @cnb.biewen(别问) Issue #17 细化方案：合并就绪诊断 + 更新分支`）。
> **BASE**：`ae11e36`（`cnb/main`，2026-11-12T01:52Z 拉取，`git rev-list --left-right --count HEAD...cnb/main` = `0 0`）。
> **方法**：每条断言在 BASE 树上用 `awk 'NR==n'` / `sed -n` 打印原文；gix API 断言对照 `~/.cargo/registry/src/rsproxy.cn-*/gix-merge-0.23.0`、`gix-0.84.0` 源码。
> **结论**：方案主体可用（冲突数据确实不用新原语）；**33 条现状断言：成立 20 / 部分成立 10 / 不成立 3**。真正卡住落地的不是代码，是三处**判定语义**（§4）和一处**端点命名冲突**（§6）。

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

`check_merge_allowed` 的**三个生产调用点**（readiness 若另写一套判定，必然与这三处漂移）：`pulls.rs:574`（合并执行）、`service.rs:1115`（auto-merge 可用性）、`merge_queue.rs:267`（合并队列准入）。

**「状态」查两次**：handler 的 `:609` 覆盖 dry-run 与真合并两条路，core 的 `merge_pr_expecting_head`（`service.rs:1321`）再查一次（防止 PR 在 handler 校验后被并发改动）。NPC 把 draft / 状态 / 保护 / `expected_head_sha` 都归到 `pulls::merge_pr` 名下（§2 N3）——**这条是准确的**；`check_merge_allowed` 内部那三类（签名 / 批准数 / CI）才是另一层。

## 2. 逐条核实（33 条）

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| N1 | `gix_merge_commits_to_tree`（`service.rs:1943`）遇 `conflicts` 非空就 `CoreError::Conflict("merge conflict detected: N files")`，文件与区块全丢 | **成立** | `:1967` 取 `outcome.tree_merge.conflicts`、`:1968-1973` 直接 `return Err`；返回类型本来就带 `Vec<Conflict>`，两个调用点（`:1897` / `:1928`）都写成 `_conflicts` |
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
| N13 | `content_merge()` → `Option<ContentMerge>`，`Resolution::Conflict` 才是真冲突；括注「`CompleteWithAutoResolvedConflict` 是有 marker 但已自解」 | **部分成立** | API 名与返回值成立（`:297`、`:420-425`、`blob/mod.rs:19-28`）；括注**错**：该变体原文是 "complete and **free of conflict markers**"（`blob/mod.rs:22-25`），带 marker 的只有 `Resolution::Conflict` |
| N14 | `Err(ResolutionFailure)` 变体直接映射 `kind`，列 5 个变体 | **部分成立** | 5 个变体名都存在；但 enum 共 **11** 个变体（`:367-413`），未覆盖 6 个；且 §2 的 `kind` 七值里 `mode`、`add_add` 无任何变体产出，`unsupported` 只由 `Unknown` 产出 |
| N15 | 区块用「冲突时写进对象库的 marker blob」，不重新 diff | **成立** | gix-merge 树合并通过 `write_blob_to_odb` 回调落库：`tree/function/resolve.rs:35` 与 `:77`（"newly merged blobs through `write_blob_to_odb`"）。**这条如果按常识猜就会判错**——`tree::Outcome` 的文档说树是 "unwritten"，但 blob 是写了的 |
| N16 | `changes_in_resolution()` 取左右条目 | **成立** | `tree/mod.rs:271` |
| N17 | `emit_body` 用同一条管线（`InternedInput` + `diff_with_slider_heuristics` + `UnifiedDiff` + `ContextSize::symmetrical(3)`） | **成立** | `gix_patch.rs:305` / `:309` / `:314`（入口 `:285`） |
| N18 | 复用 `gix_patch::parity_tests` 的 fixture | **成立** | `gix_patch.rs:446` `mod parity_tests`，`:708` 有「fixture 必须出现在参考 diff 里」的断言 |
| N19 | `blob > 1 MiB` 的 content 阈值「和 blame 的阈值一致」 | **不成立** | 本仓 blame（`rg-git/src/blame.rs`）无任何 size 上限常量；全仓 `1024 * 1024` 的命中只有 issue 模板、附件、幂等缓存、HTTP body limit，无一是 blame |
| N20 | 二进制判定用 `Operation::SourceOrDestinationIsBinary` | **成立** | `gix_patch.rs:316` |
| N21 | `merge_queue.rs:441` 已有 `spawn_blocking` 先例 | **部分成立** | 先例在，但行号是 `:415` 与 `:463`；`:441` 落在 `finish_entry` 的 match 分支里 |
| N22 | `update-branch` 的 409 复用 `AppError::conflict_because`，`details.current_sha` 已有 | **成立** | `error.rs:266`；`pulls.rs:599-605` 与 `:669-675` 等 5 处已在传 `current_sha`/`expected_sha` |
| N23 | `rg_git::ops::update_ref` 是 `PreviousValue::Any` 不能直接用；抄 `delete_ref_if` 的写法 | **部分成立** | `ops.rs:98` 确为 `Any`；但 `delete_ref_if`（`:132`）用的是 `MustExistAndMatch`（`:145`），真正同款是 `:426` 与 `:1591` 的 `ExistingMustMatch` |
| N24 | 存在 `SERVER_COMMIT_NAME` / `SERVER_COMMIT_EMAIL` | **成立** | `service.rs:1857-1858`（`IronForge` / `noreply@ironforge.local`），`commit_as_server` 用它（`:1859`） |
| N25 | `publish_head_ref` 已处理 fork 的对象复制 | **成立** | `service.rs:1396`，文档明写「objects are copied into the target repository first」 |
| N26 | 写完走同一个 `spawn_post_push_hooks`（`rg-http/src/lib.rs:3190`） | **部分成立** | 函数在 `:3202`（`:3190` 是上一个函数的 `}`）；且它是 `pub(crate)`，rg-core 调不到 ⇒ 若 `update_branch` 落在 core，钩子必须由 handler 触发 |
| N27 | 权限面在 `pat_scope.rs:299` 的 `ref_limited_write_allowed` | **部分成立** | 函数在 `:301`，`:299` 是它的文档行 |
| N28 | `pulls.rs:629` 的 `not_evaluated` 含 `"conflicts"` | **部分成立** | 内容对，行号偏 1：`":630`；`:628-629` 是「冲突只有真合才知道」的两行注释 |
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
| **R17-1** | P1 | `required_status_checks` 的两个静默放行口（null / 坏 JSON）：readiness 报 `UNKNOWN` 且合并执行改 fail closed | 改的是既有仓库的合并行为（现在能合的以后会被拒） |
| **R17-2** | P1 | 端点与字段以哪份文档为准（`/mergeability` vs `/merge-readiness` + 机器码） | OpenAPI 与前端契约 |
| **R17-3** | P2 | readiness 的审批结论必须复用 `count_current_human_approvals`（human-only + 最新一条 + commit_id） | 与 auto-merge / merge queue 三处共用同一判定 |
| **R17-4** | P2 | `kind` 枚举按 §3 补到 9 值；`mode` 需另读 `entries()` 的 mode | 响应 schema |
| **R17-5** | P2 | 新增 `update_ref_if`（`PreviousValue::ExistingMustMatch`）；`delete_ref_if` 现用 `MustExistAndMatch`，不能直接抄 | rg-git ops 层新增原语 |
| **R17-6** | P2 | fork PR 的 `update-branch` 要往**他人仓库**的分支写，写权归属未定义（目标库 maintainer 能否改 fork 分支） | 权限语义 |
| **R17-7** | P3 | `spawn_post_push_hooks` 是 `pub(crate)`（`lib.rs:3202`）⇒ 钩子只能由 handler 触发；`update_branch` 若落在 core，CI/webhook 会静默不发 | 分层与副作用边界 |

## 8. 本次判「不成立」的三条（勿重复提）

1. **N8**：`require_status_check=true` + `required_status_checks=null` ≠「只要求整体 success」，而是**什么都不查**（`service.rs:467-469` 三层 `if`）。
2. **N19**：本仓 blame **没有** 1 MiB 阈值（`rg-git/src/blame.rs` 无 size 上限）；`1024*1024` 的命中全在 issue 模板 / 附件 / 幂等缓存 / HTTP body limit。
3. **N31**：`collab_contract_tests` 只采集路径里含 `collaboration` 的路由（`collab_contract_tests.rs:79`），新 pull 路由不在范围内，防漂移测试不会要求登记。

## 9. 未做

- 未改任何代码行为：本轮只核实现状 + 订正文档与一处注释（`check_merge_allowed` 的文档行写明 null/坏 JSON 的放行语义）。
- 未实测冲突提取：`write_blob_to_odb` 的结论来自 gix-merge 源码（`resolve.rs:35`/`:77`），不是跑出来的；要落地仍应先用一个真冲突 fixture 验一遍 `content_merge().merged_blob_id` 能用 `find_object` 取到。
- 未评工作量估计（5.5 人天）与上限常量（50 文件 / 20 块 / 4 KiB）——纯设计取值，没有可核实对象。
