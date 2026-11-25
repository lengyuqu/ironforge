# T32 执行记录 — 大仓库性能基线（固定数据集 + 阈值）

> 计划见 `docs/plans/T32-plan.md`，权威约束见 `tasks/T32.md`，实测数字与阈值依据见
> `docs/plans/T32-baseline-report.md`。
> 基线 `64861cf9`（`main`），分支 `agent/t32-perf-baseline`，worktree
> `../ironforge-t32`。

## 1. 开工核对

```
$ git log --oneline -1
64861cf9 chore(tasks): seed batch 4 (T32-T36) for the four production gaps
$ ls crates/rg-git/tests
ls: crates/rg-git/tests: No such file or directory
$ grep -A3 '\[dev-dependencies\]' crates/rg-git/Cargo.toml
[dev-dependencies]
walkdir = "2"
tempfile = "3"
```

任务卡的判断成立：仓里既没有 `benches/`，也没有 `crates/rg-git/tests/`。`tempfile`
与 `walkdir` 已经是 `rg-git` 的 dev-dependencies，所以**不需要改 `Cargo.toml`**，
临时目录管理、磁盘统计都能就地做。

编译统一用共享 target 目录（4 个 Agent 同时在同一 workspace 编译，`cargo` 会排队等
文件锁）：

```
export CARGO_TARGET_DIR=/Users/yuqu/vibeCodeing/ironforge/target
```

## 2. 改动

| 文件 | 改动 |
| --- | --- |
| `crates/rg-git/tests/perf_fixture.rs` | 新建。三档 `DatasetSpec`、确定性生成器、增量 pack 生成、内容摘要、磁盘占用 |
| `crates/rg-git/tests/perf_baseline.rs` | 新建。6 个测量探针、采样与 p50/p95、`THRESHOLDS` 表、环境指纹、4 个 `#[ignore]` 测试 |
| `docs/plans/T32-plan.md` | 新建。设计决定与取舍 |
| `docs/plans/T32-task.md` | 本文件 |
| `docs/plans/T32-baseline-report.md` | 新建。实测数字、阈值、Gitea 对比、已知瓶颈 |
| `tasks/T32.md` | 仅 `State` / 验收相关字段 → `completed` |

`Cargo.toml` / `Cargo.lock` / `scripts/` / `crates/rg-git/src/` **一行未改**。

## 3. 实现要点

### 3.1 数据集确定性

文件正文只由 `(path, commit index)` 派生：路径先过 FNV-1a（常量种子，不用
`DefaultHasher`——它跨 Rust 版本不保证稳定），再与 commit 序号混入播种 64 位 LCG；
提交时间固定为 `1_700_000_000 + commit index`，不读系统时钟。因此两次生成的对象
字节相同、tip SHA 相同，由 `perf_dataset_is_deterministic` 断言（实测两次都是
`digest=d90caeb4334cfa8d`、`tip=60b42ed8…`）。

### 3.2 生成代价 O(files + commits)

第 0 个提交写入全部 blob 并物化整棵树；之后每个提交只改一个文件，并且只重写
「根 → `g` → `d` → `s`」四层 tree，兄弟目录直接复用缓存的 tree id。如果每提交重建
全树，MEDIUM 就是 2000 × 20 000 = 4000 万次 entry 序列化，跑不动。

### 3.3 增量 push 用真实增量 pack

`ops::pack_for_wants` 的 `input_object_expansion` 固定为 `TreeContents`，会把新提交
的整棵树塞进 pack，不按 `haves` 剪枝：SMALL 上「改 1 KiB 文件」生成 1 322 903 字节。
用它造增量请求，测出来与首推几乎一样，「增量」就失去意义。所以 fixture 用
gix-pack 的 `TreeAdditionsComparedToAncestor` 自己生成真实增量（SMALL：8 个对象 /
3875 字节）。`TreeContents` 的行为**不改实现**，作为已知瓶颈写进报告。

### 3.4 测量边界

计时只覆盖服务端处理：客户端 pack 生成、push 目标仓的准备都在计时之外。clone 的
输出写进只计数的 `AsyncWrite` sink（不落盘、不占 60 MB 内存）。push 走
`handle_receive_pack_http`、clone 走 `handle_upload_pack_http`，即真实的协议入口，
不是同形的辅助函数。

### 3.5 阈值

阈值 = 实测算术中位数 × 1.5，向上取整（> 50 ms 取到 5 的倍数）。`LARGE` 故意没有
阈值行：没实测过的档不许有上限，否则就是编数字。断言失败信息同时打印实测中位数、
p95、样本数与阈值。

## 4. 验收

### 4.1 `cargo test -p rg-git --test perf_baseline -- --ignored --nocapture` 跑完 SMALL + MEDIUM 并打印结构化数字

```
running 5 tests
[perf] skipped materialise reason=no-target (set IRONFORGE_PERF_KEEP_DIR and IRONFORGE_PERF_DATASET=small|medium|large)
[perf] skipped dataset=large reason=manual-only (set IRONFORGE_PERF_LARGE=1 to run, ~2 GiB)
[perf] env cpu="Apple M3 Max" cores=16 os=macos arch=aarch64 profile=dev rustc="rustc 1.99.1 (58cf8f9c3 2026-10-13)" disk="macOS APFS (Solid State:               Yes)"
[perf] dataset=small commits=100 files=2000 file_bytes=1024 dirs=20 build_ms=1108.0 disk_mib=2.4 content_mib=2.0 tip=60b42ed8217e5ce5d9d6de6b4a5a5eb803a12793 digest=d90caeb4334cfa8d entries=2000
[perf] note dataset=small push_incremental pack_objects=8 pack_bytes=3875 history_pack_bytes=1679402 ops_incremental_pack_bytes=1322903 ops_incremental_objects=2025
[perf] summary dataset=small op=push_initial n=3 median_ms=130.1 p50_ms=130.1 p95_ms=130.9 bytes=1683038
[perf] summary dataset=small op=push_incremental n=3 median_ms=10.1 p50_ms=10.1 p95_ms=11.3 bytes=3875
[perf] summary dataset=small op=clone n=3 median_ms=182.6 p50_ms=182.6 p95_ms=204.7 bytes=1683190
[perf] summary dataset=small op=log_first_page n=3 median_ms=9.2 p50_ms=9.2 p95_ms=10.6 bytes=100
[perf] summary dataset=small op=tree_root n=3 median_ms=69.4 p50_ms=69.4 p95_ms=79.6 bytes=2000
[perf] summary dataset=small op=tree_deep n=3 median_ms=4.1 p50_ms=4.1 p95_ms=4.2 bytes=100
[perf] dataset=medium commits=2000 files=20000 file_bytes=4096 dirs=200 build_ms=21361.9 disk_mib=92.9 content_mib=78.1 tip=370369f6c2c970200726a9653c75baabe343fccb digest=492ea1c8299fae11 entries=20000
[perf] note dataset=medium push_incremental pack_objects=8 pack_bytes=5973 history_pack_bytes=61464474 ops_incremental_pack_bytes=50066689 ops_incremental_objects=20224
[perf] summary dataset=medium op=push_initial n=3 median_ms=3949.5 p50_ms=3949.5 p95_ms=4058.7 bytes=61470180
[perf] summary dataset=medium op=push_incremental n=3 median_ms=35.7 p50_ms=35.7 p95_ms=37.2 bytes=5973
[perf] summary dataset=medium op=clone n=3 median_ms=3129.6 p50_ms=3129.6 p95_ms=3254.4 bytes=61474897
[perf] summary dataset=medium op=log_first_page n=3 median_ms=9.5 p50_ms=9.5 p95_ms=11.3 bytes=100
[perf] summary dataset=medium op=tree_root n=3 median_ms=787.5 p50_ms=787.5 p95_ms=819.4 bytes=20000
[perf] summary dataset=medium op=tree_deep n=3 median_ms=4.1 p50_ms=4.1 p95_ms=5.5 bytes=100
[perf] threshold dataset=small op=push_initial measured_ms=130.1 threshold_ms=195 verdict=ok n=3 p95_ms=130.9
…（12 行 small/medium 阈值判定全部 verdict=ok，逐行见报告 §5）
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 71.95s
```

LARGE 另外手工跑过一次（n=1，dev，12 分 28 秒）：

```
[perf] dataset=large commits=20000 files=200000 file_bytes=8192 dirs=1000 build_ms=376802.9 disk_mib=1828.4 content_mib=1562.5 tip=e631f09b8c9853d3d688accbba51459c68c57244 digest=868554242ebb0b2a entries=200000
[perf] sample dataset=large op=push_initial rep=1 ms=74534.9 bytes=1207905804
[perf] sample dataset=large op=push_incremental rep=1 ms=272.1 bytes=11240
[perf] sample dataset=large op=clone rep=1 ms=46008.2 bytes=1207998016
[perf] sample dataset=large op=log_first_page rep=1 ms=16.9 bytes=100
[perf] sample dataset=large op=tree_root rep=1 ms=11563.8 bytes=200000
[perf] sample dataset=large op=tree_deep rep=1 ms=10.7 bytes=200
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out; finished in 744.94s
```

### 4.2 同一数据集生成两次，文件内容哈希一致

```
[perf] determinism first  tip=60b42ed8217e5ce5d9d6de6b4a5a5eb803a12793 digest=d90caeb4334cfa8d entries=2000 build_ms=1108.6
[perf] determinism second tip=60b42ed8217e5ce5d9d6de6b4a5a5eb803a12793 digest=d90caeb4334cfa8d entries=2000 build_ms=1211.3
```

tip SHA 与内容摘要（每项 `path\0blob_sha` 的 FNV-1a）都相同。

### 4.3 故意把阈值改到 1 ms 必须失败且错误信息含实测值

把 `(DatasetName::Small, Operation::PushInitial, 195)` 临时改成 `1`：

```
[perf] threshold dataset=small op=push_initial measured_ms=121.7 threshold_ms=1 verdict=FAIL n=3 p95_ms=122.8
performance baseline failed:
small/push_initial: measured 121.7 ms (p95 122.8 ms, n=3) > threshold 1 ms
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 4 filtered out; finished in 3.48s
```

改回 195 后恢复绿（见 4.1）。

### 4.4 `cargo clippy --workspace --all-targets -- -D warnings` 干净

```
$ cargo clippy -p rg-git --tests -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2m 47s
$ cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 33.71s
```

两次都没有 warning（`-D warnings` 会把 warning 变成错误，所以「Finished」即代表干净）。

### 4.5 报告里的数字与实际运行输出一致

`docs/plans/T32-baseline-report.md` §4/§5 的每个数字都取自 4.1 的 `[perf]` 行，
逐项核对过；报告里额外出现的数字只有两类，来源都标在同一节：

- release 参考值（§6）：来自 `cargo test --release …` 的同一批 `[perf]` 行；
- Gitea 数字（§8）：来自 `git push/clone` 的 `/usr/bin/time -p` 与 `curl -w
  %{time_total}`，同节写明了命令与偏差。

## 5. 遗留与边界

1. **没有做优化**。`crates/rg-git/src/` 一行未改；§7 的五条瓶颈（不剪枝的
   `pack_for_wants`、pack 全量进内存、根目录递归遍历、无 pack/bitmap 复用、
   大推送跳过 secret scan）都只记录，各自应另开任务。
2. **阈值只对这台机器成立**。换 CPU / 换磁盘要重新标定；报告里的指纹就是标定条件。
   `tree_deep`、`log_first_page` 这些 4~10 ms 的项抖动占比大，只宜当量级信号。
3. **dev profile 与 release 差 2.6~10 倍**。验收命令不带 `--release`，阈值按 dev
   标定（release 只会更快，不会误报）。生产判断请用报告 §6 的 release 数字。
4. **LARGE 的 dev 单次采样受同机负载影响**（采样时同 workspace 还有 4 个 Agent
   在编译）。release 单次（199 s）比 dev（745 s）干净得多，两者都保留了原始输出。
5. **Gitea 对比不是同层比较**：IronForge 侧是进程内服务端工作量，Gitea 侧是端到端。
   报告 §8 已逐项标注边界，没有把「客户端 pack 生成」算进 IronForge 的数字。
6. **Gitea 的 `tree 根目录` 没有对比项**（contents API 是单层语义），已在报告写明
   「未做」而不是拿一个不等价的操作凑数。

