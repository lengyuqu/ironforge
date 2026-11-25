# T32 执行计划 — 大仓库性能基线（固定数据集 + 阈值）

> 权威约束见 `tasks/T32.md`。基线 `64861cf9`，分支 `agent/t32-perf-baseline`，
> worktree `../ironforge-t32`。实际执行与验收输出见
> `docs/plans/T32-task.md`，实测数字与阈值依据见
> `docs/plans/T32-baseline-report.md`。

## 1. 要回答的两个问题

`tasks/T32.md` 说得很清楚：仓库里没有任何 `benches/`、没有固定数据集、没有阈值，
所以下面两件事今天无法回答：

1. 一个 5 万 commit、50 万文件、2 GB 的仓库推进来，IronForge 是 3 秒还是 3 分钟？
2. 下次改动 `upload-pack` 之后，有没有变慢？

本任务只建立**可重复、有绝对数字**的基线，**不做优化**。第 2 个问题的判据是阈值，
第 1 个问题的判据是同一份数据集上的实测数字。

## 2. 为什么是 `tests/` 里的 `#[ignore]` 测试，而不是 criterion bench

硬约束：**不得修改 `Cargo.toml` / `Cargo.lock`**。criterion 需要
`[dev-dependencies]` 加 crate、`[[bench]]` 加 target，两者都在禁区内。因此：

- 基线写成 `crates/rg-git/tests/perf_baseline.rs` 的 `#[ignore]` 测试，
- 用 `std::time::Instant` 手工计时，
- 数据集生成逻辑是 Rust 测试 fixture（`crates/rg-git/tests/perf_fixture.rs`），
  不是 shell 脚本（`scripts/` 也是禁区）。

`tempfile` 已在 `crates/rg-git/Cargo.toml` 的 `[dev-dependencies]` 中（与 `walkdir`
一起），所以临时目录管理和磁盘统计都不需要新依赖。

## 3. 数据集设计

| 档 | name | commits | files | 单文件 | leaf dirs | 文件内容 |
| --- | --- | --- | --- | --- | --- | --- |
| SMALL | `small` | 100 | 2 000 | 1 KiB | 20 | 2.0 MiB |
| MEDIUM | `medium` | 2 000 | 20 000 | 4 KiB | 200 | 78.1 MiB |
| LARGE | `large` | 20 000 | 200 000 | 8 KiB | 1 000 | 1.6 GiB |

关键设计点（都在 `perf_fixture.rs` 的模块文档里写明了）：

1. **确定性**。文件内容只由 `(path, commit index)` 派生：路径先过 FNV-1a，再与
   commit 序号混入，用它播种 64 位 LCG 填充正文。提交时间是
   `BASE_TIMESTAMP + commit index`（固定值，不读系统时钟）。因此**两次生成产生
   完全相同的对象与相同的 tip SHA**，这一点由 `perf_dataset_is_deterministic` 断言。
   不用 `rand`（不在依赖里），也不用 `DefaultHasher`（跨版本不保证稳定）。
2. **生成代价是 `O(files + commits)` 而不是 `O(files × commits)`**。第 0 个提交写入
   全部文件并物化整棵树；之后每个提交只改写**一个**文件，并且只重写从根到该文件的
   那 4 层 tree（兄弟目录复用缓存 tree id）。否则 2 000 × 20 000 = 4000 万次 tree
   entry 序列化，MEDIUM 就生成不动了。
3. **目录布局三级**：`g<top>/d<mid>/s<leaf>`，文件均匀铺到 `dirs` 个 leaf 上。
   这样「根目录」和「深层目录」是两次真正不同的遍历：根是 20 000 个文件的递归全量
   列出，深层目录是 100 个文件的子树。
4. **内容用可打印 ASCII**，不是不可压缩随机字节：真实源码仓的 pack 会压缩，用随机
   字节会把 pack 尺寸放大 2~3 倍，绝对数字失去参考价值。LCG 字母流压缩率接近文本，
   量级接近真实仓库。

## 4. 五个测量点怎么落到真实入口

任务卡要求测的是**协议路径**，不是「长得像」的辅助函数。因此：

| # | 探针 | 实际调用 |
| --- | --- | --- |
| 1 | push 首推（全量对象） | `protocol::receive_pack::handle_receive_pack_http`，请求字节 = 1 条 update command + flush + 全量 pack（`ops::pack_universe` 生成，**计时前**生成） |
| 2 | push 增量（改 1 文件） | 同一个入口；目标仓先用「到 tip 父提交为止」的 pack 准备好（**不计时**），再计时包含 1 个 blob + 4 个 tree + 1 个 commit 的增量 pack |
| 3 | clone | `protocol::upload_pack::handle_upload_pack_http`，请求 = 一条 `want <tip> side-band-64k` + flush；输出写进只计数的 sink，不落盘 |
| 4 | log 首页（100 条） | 与 `rg-http/src/api/repo_content.rs::get_commit_log` 同形：`rev_walk` 取 100 个 commit id，再逐个 `message_raw()` + `author()` |
| 5 | tree 根目录 / 深层目录 | `rg_git::ops::list_tree_files`（递归、`max_entries = usize::MAX`） |

**为什么增量 pack 要自己造**：`ops::pack_for_wants` 的 `input_object_expansion` 固定为
`TreeContents`，它把新提交的**整棵树**都放进 pack，不按 `haves` 剪枝。用它来造增量
请求，1 KiB 改动的 pack 会有 1.3 MB（SMALL 实测），测出来和首推几乎一样，「增量」
就没意义了。所以 fixture 用 gix-pack 的 `TreeAdditionsComparedToAncestor` 自己生成
真实增量（SMALL 实测 8 个对象 / 3 875 字节）。这个 `TreeContents` 行为本身作为
**已知瓶颈**记录进报告，不改实现。

## 5. 重复次数、统计口径与阈值

- 每项跑 3 次（LARGE 跑 1 次），取**中位数**。
- p50 = 中位数；p95 用 nearest-rank，n=3 时就是最大值。报告里明说，不用更平滑的
  估计掩盖样本少的事实。
- 阈值 = **实测中位数 × 1.5**，向上取整（> 50 ms 的取到 5 的倍数）。1.5 倍是给
  CI 机器抖动留的余量；阈值是**同一台机器、同一个 profile** 的上限，不是跨机器常数。
- 断言失败信息必须同时含实测毫秒数、p95、样本数和阈值——否则「变慢了」不可诊断。
- 环境指纹（CPU 型号、核数、OS/arch、`rustc -V`、磁盘类型、profile）每次运行都打印，
  报告里原样记录。**不同机器数字不可比**。
- `LARGE` 只手工跑：测试名上加 `#[ignore]`，并且要求 `IRONFORGE_PERF_LARGE=1` 才真正
  执行，否则打印一行 skip 并返回。`THRESHOLDS` 的 Large 行来自**单次**采样 × 1.5，
  注释里写明它是指示性的——单样本不是分布，但比「完全不给上限」更有用，因为手工跑
  LARGE 时至少有个参照。没实测过的数字一律不填。

## 6. 明确不做

- 不做任何性能优化（`crates/rg-git/src/` 一行不改）。
- 不加 criterion、不建 `benches/`、不碰 `Cargo.*`。
- 不把 LARGE 塞进默认测试；`--ignored` 下也要显式给环境变量才跑。
- 不做 Gitea 同机对比的自动化：本机有没有 docker 决定能不能做，做不到就在报告里
  写「未做」并说明原因，不估算。
