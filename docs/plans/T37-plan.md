# T37 执行计划 — 增量 fetch 的对象级差集

> 权威约束见 `tasks/T37.md`。基线 `a0cf7c27`（main），分支 `agent/t37-pack-haves-diff`，
> worktree `../ironforge-t37`。
>
> 本文件记录：路线选择的实测依据、`TreeAdditionsComparedToAncestor` 的真实行为、实现与边界、
> 验收数字、内容正确性证据、未做项。

## 1. 结论

改一行级别的事：`ops::pack_for_wants_filtered` 原来用
`ObjectExpansion::TreeContents` 展开**每个**待打包 commit 的整棵树，现在对 walk 出来的
commit 改用 `ObjectExpansion::TreeAdditionsComparedToAncestor`（对象级差集），
shallow 加深路径保持原样。

medium（2000 commits / 20000 files，改 1 个 4 KiB 文件）实测：

| 指标 | T32 基线 | 现在 | 真实增量（`git push` 的 pack） |
|---|---|---|---|
| 对象数 | 20 224 | **8** | 8 |
| 字节数 | 50 066 689 | **5 973** | 5 973 |

即服务端发出的 pack **与真实增量逐字节等量**，放大倍数从 ≈8 380× 降到 1×。

## 2. 根因核对（与任务卡一致）

- `pack_for_wants_filtered` 确实按 haves 剪了 commit 遍历：`rev_walk(starts).with_hidden(hidden)`，
  `hidden = peel_list(haves)`（shallow 分支另有语义）。commit 层没有问题。
- 漏的是对象层：`input_object_expansion: TreeContents` 对每个 input commit 做
  `gix_traverse::tree::breadthfirst` 全树展开。于是"只剩 1 个待打包 commit"仍会发出
  20 000 个 blob —— 20 224 ≈ 20 000 文件数，正是这条线索。

## 3. 路线选择：为什么不自己 walk tree（路线 A），也不收窄 input（路线 B）

任务卡怀疑 `TreeAdditionsComparedToAncestor` 会"自己走祖先链"，从而把 haves 剪掉的祖先
重新引入。**实测否定这个怀疑**：它只与直接父提交比对，不递归祖先。

### 3.1 读源码确认

`gix-pack-0.77.0/src/data/output/count/objects/mod.rs:170-268`：

- 外层那个 `loop` 只用于 tag 剥壳（`Tag =>` 继续，`Commit|Tree|Blob => break`），
  **不是**祖先遍历。
- 遇到 commit：把 commit 本身入包；把当前 tree 入包；对**每个直接父提交**：把父 commit 和
  父树入包，然后 `gix_diff::tree(parent_tree, current_tree, ...)`，把 Addition/Modification
  的对象入包（Deletion 跳过，submodule 的 `EntryMode::Commit` 跳过）。
- 无父提交（root）→ 对该 commit 的树做一次 `breadthfirst` 全展开。
- merge：对每个父都算一遍，取并集。

也就是说它实现的正是任务卡路线 A 的语义，而且已经处理了 merge 并集、root、submodule。

### 3.2 小实验（40 文件 / 6 commit 的仓库，改 1 个文件）

实验当初写在 `crates/rg-git/tests/t37_probe.rs`，量完即删；数字如下：

| 展开方式 | 对象数 |
|---|---|
| `TreeContents`（旧行为，input = 剪枝后的 1 个 commit） | 48 |
| `TreeAdditionsComparedToAncestor`（input = 同一个 commit） | **7** |
| `TreeAdditionsComparedToAncestor`（input = 全 6 个 commit） | 73 |
| `TreeContents`（root commit） | 48 |

- 第 2 行 = 目标量级：7 个对象 = 新 commit + 变化路径上的 tree + 新 blob + 父 commit + 父 tree。
- 第 3 行说明"input 放全历史"才会得到全量（因为它对每个 input 都算，且 root 全展开），
  与"它自己走祖先"无关。**input 用剪枝后的 walk 结果正合适**。
- 附带确认：全历史 fetch（无 haves）下两种展开得到的对象集合**完全相同** —— 因为 walk 闭包内
  每个父提交本身也在 input 里，父 commit/父 tree 本来就在集合内。所以 clone 路径换过去
  只变快不变内容。

结论：直接用 gix 的 `TreeAdditionsComparedToAncestor`，不自己 walk tree（少一份可能写错的
树遍历代码），也不走路线 B/C。

## 4. 实现（`crates/rg-git/src/ops.rs`，+146 / −22 行）

1. 抽出 `count_pack_objects(db, input_ids, expansion)`：原来内联在
   `generate_pack_filtered_with_tips` 里的 `output::count::objects` 调用。
2. 新增 `enum PackObjectExpansion { TreeContents, Incremental { conservative } }` 与
   `generate_pack_with(..., expansion)`；`generate_pack_filtered_with_tips` 保持原签名、
   固定走 `TreeContents`（`generate_pack`/`pack_universe` 的 fork、V1 空 want 兜底不受影响）。
3. `Incremental` 分支：
   - walk 出来的 commits → `TreeAdditionsComparedToAncestor`；
   - `conservative`（直接 want 的 annotated tag / tree）→ 另一趟 `TreeContents`，再按
     object id 去重合并（同一对象只能有一个 pack entry）。
4. `pack_for_wants_filtered`：
   - want 按 kind 分流：`Tag`/`Tree` → `anchored_inputs`（全展开），`Blob` → `plain_inputs`
     （只要它自己），`Commit` → walk 起点；
   - `shallow_boundaries` 非空 → 整个请求退回 `TreeContents`（见 §5.2）；
   - 否则 `Incremental { conservative: &anchored_inputs }`。

## 5. 边界处理

| 边界 | 处理 | 依据 |
|---|---|---|
| 空 haves / 首次 clone | 走 `Incremental`，但 walk 是完整闭包，对象集合与 `TreeContents` 逐对象相同（§3.2 第 3 行）；实测 clone 更快 | 不能少发；不变量由 `filtered_packs_leave_out_blobs_and_trees_but_keep_requested_objects` 的精确计数断言守住 |
| `shallow_boundaries` 非空（加深） | **整单退回 `TreeContents`**，与改动前完全一致 | boundary commit 的树必须全量；客户端"拥有 boundary commit 但不拥有其树"，差集在这里无法自证安全。牺牲这条路径的带宽，换 0 风险 |
| `shallow_mode = true` + 空 boundary（unshallow） | 走 `Incremental`；`hidden` 为空 ⇒ walk 是全历史，root 全展开，链式 diff 覆盖全部对象 | shallow 客户端确实拥有它手上 commit 的完整树 |
| `filter`（`blob:none`/`tree:0`/`blob:limit`/`object:type=*`） | **先差集，后 filter**（`counts.retain` 位置未动） | 两者正交 |
| 直接 want 的 blob | 只发它自己 | lazy fetch 语义 |
| 直接 want 的 tree | 全展开（`requested` 同时保证 filter 不丢它） | 与改动前一致，保守 |
| annotated tag want | tag 对象 + 全展开 | 与改动前一致；tag 的 target commit 不在 walk 里，无法证明客户端有它的父树 |
| merge commit | gix 对每个父算并集 | 任务卡要求并集 |
| root commit | gix 全展开 | 客户端确实什么都没有 |
| "up-to-date" fetch（want == have） | walk 为空 → 空 pack | 与改动前一致 |

## 6. 验收数据（本机实测，dev profile，与 T32 同机同 profile）

### 6.1 增量 fetch 的对象数/字节数

复现：`cargo test -p rg-git --test perf_baseline -- --ignored --nocapture perf_baseline_medium`
打印的 `[perf] note ... ops_incremental_objects/ops_incremental_pack_bytes` 就是
`pack_for_wants(tip, haves=[parent])` 的产物。

| 场景 | T32 基线 | 现在 | 通过线 | 结论 |
|---|---|---|---|---|
| medium 改 1 文件 fetch | 20 224 obj / 50 066 689 B | **8 obj / 5 973 B** | ≤100 obj / ≤200 KB | ✅ |
| large 改 1 文件 fetch | 201 112 obj / 992 218 757 B | **8 obj / 11 240 B** | ≤100 obj / ≤1 MB | ✅ |
| small 改 1 文件 fetch | （未记录该项） | 8 obj / 3 875 B | — | — |

medium 的 8 个对象 = 新 commit、变化路径上的 4 个 tree、新 blob、父 commit、父 tree。
后两个是 gix 该模式的固有开销（客户端已有，重复发送无害），合计 < 200 B 级别。

### 6.2 回归（5 个测量点，medians，n=3）

| 测量点 | T32 基线 | 现在 | 变化 | 上限（+20%）| 结论 |
|---|---|---|---|---|---|
| small clone | 182.6 ms | 165.4 ms | −9.4% | 219 ms | ✅ |
| medium clone | 3 129.6 ms | 3 173.7 ms | +1.4% | 3 756 ms | ✅ |
| medium 首推 | 3 949.5 ms | 3 651.1 ms | −7.6% | 4 739 ms | ✅ |
| small 首推 | 130.1 ms | 120.1 ms | −7.7% | 156 ms | ✅ |
| medium 增量推 | 35.7 ms | 33.6 ms | −5.9% | 43 ms | ✅ |
| medium log 首页 | 9.5 ms | 8.7 ms | −8.4% | 11.4 ms | ✅ |
| medium tree 根目录 | 787.5 ms | 736.8 ms | −6.4% | 945 ms | ✅ |
| medium tree deep | 4.1 ms | 3.8 ms | −7.3% | 4.9 ms | ✅ |

clone 变快是因为旧实现要对**每个** commit 做一次全树 breadthfirst（2000 × 20000 次 entry
访问），现在只对 root 全展开、其余做单文件 diff。

### 6.3 large 档（`IRONFORGE_PERF_LARGE=1`，dev，n=1，全部 verdict=ok）

数据集：20 000 commits / 200 000 files / 1.56 GiB 内容，生成 315 141 ms，tip
`e631f09b…c57244`（与 T32 同一 tip，说明数据集确定性一致）。

| 测量点 | T32 基线（dev, n=1） | 现在（dev, n=1） | 变化 | 阈值 | 结论 |
|---|---|---|---|---|---|
| 增量 fetch 对象数/字节 | 201 112 obj / 992 218 757 B | **8 obj / 11 240 B** | −99.999% | ≤100 obj / ≤1 MB | ✅ |
| 首推 | 74 534.9 ms | 69 239.2 ms | −7.1% | 111 805 ms | ✅ |
| 增量推 | 272.1 ms | 252.6 ms | −7.2% | 410 ms | ✅ |
| clone | 46 008.2 ms | 38 525.1 ms | **−16.3%** | 69 015 ms | ✅ |
| log 首页 | 16.9 ms | 11.8 ms | −30% | 26 ms | ✅ |
| tree 根目录 | 11 563.8 ms | 11 589.9 ms | +0.2% | 17 350 ms | ✅ |
| tree deep | 10.7 ms | 10.7 ms | 0% | 17 ms | ✅ |

large 的 clone 提速最明显（−16.3%）：旧实现要给 20 000 个 commit 各做一次 200 000 entry
的全树遍历，共约 4×10⁹ 次 entry 访问；现在只有 root commit 全展开。

## 7. 内容正确性（最重要）

新增 `crates/rg-git/tests/incremental_fetch.rs`（11 个测试）。所有"客户端"都是**真实
git**：pack 落盘到 `.git/objects/pack/`，用 `git index-pack` 安装（真客户端的安装步骤），
再用 `git fsck --strict` / `git cat-file` / `git status --porcelain` / 文件字节比对验收。
不只用我们自己的 reader 自证。

| 验收项 | 测试 | 做法 |
|---|---|---|
| 逐字节一致 | `incremental_fetch_reproduces_the_work_tree_byte_for_byte`、`modified_file_content_is_updated` | 客户端 = 首 clone pack + 增量 pack；`reset --hard` 后对 `ls-tree -r` 列出的每个文件做 `std::fs::read` 与源仓库对比 |
| `git fsck --strict` | 上述各测试 + `haves_less_fetch_still_sends_everything` | 在客户端仓库直接跑 |
| 删除 | `deleted_file_disappears_from_the_client` | 源仓库 `git rm` 后 fetch；断言客户端**没有**该路径、`status --porcelain` 为空、整树逐字节一致 |
| 重命名 | `renamed_file_lands_at_its_new_path` | 源仓库 `git mv` 后 fetch；断言旧路径消失、新路径内容一致、整树一致 |
| 对象集合完整性 | `assert_client_has_every_object`（被 6 个测试调用） | 对 `git rev-list --objects <tip>` 的每个对象在客户端 `git cat-file -e` |
| merge 并集 | `merge_commit_delta_contains_every_parent_difference`、`branching_history_fetches_cleanly_from_one_have` | 两侧各自有独立改动再 merge，取并集后仍满足上面的对象完整性 + 整树比对 |
| 多 commit 区间 | `many_new_commits_scale_with_the_delta_not_with_the_tree` | 30 个新 commit 从 1 个旧 have 拉，对象数不随树规模放大 |
| 无 haves 全量 | `haves_less_fetch_still_sends_everything` | 新客户端 + 空 haves：对象完整、fsck 通过、工作树干净 |
| filter 正交 | `partial_clone_filter_still_applies_after_the_delta` | `blob:none` 在差集之后生效，对象数严格变少 |
| shallow 加深不回退 | `shallow_boundary_commit_still_gets_its_whole_tree` | `shallow_boundaries=[base]` 的 pack 里 boundary 树的**每个** blob 都在 |

另：`cargo test -p rg-git` 的 123 个单元测试全绿（含
`filtered_packs_leave_out_blobs_and_trees_but_keep_requested_objects` 对每种 filter 的
**精确对象数**断言、`pack_for_wants_unshallow_sends_full_history`、
`protocol::upload_pack::haves_tests::v1_fetch_prunes_haves_instead_of_sending_everything`）。

**唯一失败项与本次改动无关**：`cli_gateway::tests::test_no_raw_git_command_in_crates`
断言 `crates/` 下没有裸 `Command::new("git")`，而 `crates/rg-ci/src/action_fetch.rs`（2 处）、
`crates/rg-ci/tests/composite_actions.rs`（1 处）、`crates/rg-ci/tests/pipeline_action_wiring.rs`
（2 处）在基线 `a0cf7c27` 上就已经存在。已用 `git show HEAD:<path> | grep` 核实是既有违规，
不在本任务允许改动的范围内。

## 8. 风险与未做项

1. **父对象缺失的仓库**：新模式要读父提交与父树。若服务端仓库损坏/被裁剪掉父对象，
   pack 生成会报错（旧实现不读父，能出包）。这类仓库本来也无法完成正常 clone 语义；
   错误是显式的 HTTP 500，不会静默少发对象。未加"父缺失就退回全展开"的兜底，因为那需要
   每个 commit 一次 `contains` 查询。
2. **每个 commit 多 2 个对象**（父 commit + 父树，客户端已有）：N 个新 commit 的 fetch 会多
   发 2N 个小对象。medium 1 个 commit 的场景可忽略；若将来要榨干这 2N，需要自己写 diff
   collector（路线 A），或等 gix 提供"不含父"的模式。
3. **shallow 加深路径没优化**（整单退回 `TreeContents`）。这是有意的 0 风险选择，不是遗漏。
   若要优化，需要先证明客户端拥有 boundary commit 的树，再对 boundary 之外的 commit 用差集。
4. **未做**：pack 流式化（T38）、bitmap/pack 复用、`send_packfile` 的内存驻留、
   `secret_scan` 的 `MAX_FILES`。

## 9. 复现

```text
# 单元 + 集成测试
CARGO_TARGET_DIR=<共享 target> cargo test -p rg-git

# 增量 fetch 的对象数（medium / large）
CARGO_TARGET_DIR=<共享 target> cargo test -p rg-git --test perf_baseline -- \
  --ignored --nocapture perf_baseline_medium
IRONFORGE_PERF_LARGE=1 CARGO_TARGET_DIR=<共享 target> cargo test -p rg-git \
  --test perf_baseline -- --ignored --nocapture perf_baseline_large

# 内容正确性
CARGO_TARGET_DIR=<共享 target> cargo test -p rg-git --test incremental_fetch -- --nocapture
```

## 10. 变更清单

| 文件 | 内容 |
|---|---|
| `crates/rg-git/src/ops.rs` | 差集逻辑（+146 / −22） |
| `crates/rg-git/tests/incremental_fetch.rs` | 新增，11 个测试（对象数 + 内容正确性） |
| `crates/rg-git/tests/perf_baseline.rs` | 只改注释：`server_side_increment` 不再是"整树重发" |
| `crates/rg-git/tests/perf_fixture.rs` | 只改注释：`incremental_pack` 不再是"服务端的替代品" |
| `docs/plans/T32-baseline-report.md` | §7.1 补注：已由 T37 修复，附实测数字 |
| `docs/plans/T37-plan.md` | 本文件 |

未改：`Cargo.toml` / `Cargo.lock` / `scripts/` / `deploy/` / `.cnb.yml` / `.github/` /
rg-http / rg-core / rg-ci / 迁移 / 任何协议线格式与协商逻辑。
