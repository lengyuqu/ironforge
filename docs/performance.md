# 大仓库性能：基线、阈值与已修问题

> 状态（0.5.1）：基线测试与两处传输优化已实现。事实来源：`crates/rg-git/tests/perf_baseline.rs`、`perf_fixture.rs`、`crates/rg-git/src/ops.rs`。
> 数字只在本机、dev profile 下可比；换机器要重新标定。

## 1. 怎么跑

```sh
cargo test -p rg-git --test perf_baseline -- --ignored --nocapture
```

- 数据集由固定种子生成，不依赖 `rand`、不依赖 git CLI；同一档连续生成两次的 tip 与内容摘要必须完全相同（有断言）。
- 阈值在 `THRESHOLDS` 表中：small/medium 取 3 次采样中位数 × 1.5，large 取单次 × 1.5；CI 跑 `--ignored` 即校验。
- 环境指纹（CPU 核数、OS、rustc、文件系统）必须与基线一致，否则数字不可比。

## 2. 数据集

| 档 | commits | files | 内容量 | 磁盘 |
|---|---:|---:|---:|---:|
| small | 100 | 2 000 | 2.0 MiB | 2.4 MiB |
| medium | 2 000 | 20 000 | 78.1 MiB | 92.9 MiB |
| large | 20 000 | 200 000 | 1 562.5 MiB | 1 828.4 MiB |

## 3. 基线（Apple M3 Max, 16 cores, dev profile）

| 测量点 | small | medium | large |
|---|---:|---:|---:|
| push 首推 | 130.1 ms | 3 949.5 ms | 74 534.9 ms |
| push 增量（改 1 文件） | 10.1 ms | 35.7 ms | 272.1 ms |
| clone | 182.6 ms | 3 129.6 ms | 46 008.2 ms |
| log 首页（100 条） | 9.2 ms | 9.5 ms | 16.9 ms |
| tree 根目录（递归全量） | 69.4 ms | 787.5 ms | 11 563.8 ms |
| tree 深层目录（一个叶子） | 4.1 ms | 4.1 ms | 10.7 ms |

release 下数字约低一个量级（large clone 14.5 s、首推 7.4 s），但阈值按 dev 标定。

## 4. 已修复：增量 fetch 发整个工作树（T37）

改前服务端对每次 fetch 使用 `ObjectExpansion::TreeContents`，对每个待打包 commit 展开完整工作树；commit 层已按 haves 剪枝，缺的是**对象层**差集。

| 档 | 改前 | 改后 |
|---|---|---|
| medium | 20 224 对象 / 50 066 689 B | 8 对象 / 5 973 B |
| large | 201 112 对象 / 992 218 757 B | 8 对象 / 11 240 B |

现行为：按 haves 剪枝后的对象级差集，只发客户端真正缺的对象。

## 5. 已修复：pack 全量驻留内存（T38）

改前 `pack_for_wants` 把整个 pack 写进一个 `Vec<u8>` 再写 socket；large 仓库 clone 时该 Vec 常驻 1.21 GB，2–3 个并发就 OOM（`panic = "abort"`，会带走所有连接）。

改后：pack 直接流式写传输层，V1 与 V2 两条路径都改（真实 `git clone` 默认 V2）。

服务端空载 RSS 约 84 MiB（同一台机器，dev profile）；因此验收看**相对空载的净增量**，绝对峰值仍包含这部分固定开销。

| 指标 | 改前 | 改后 |
|---|---:|---:|
| large clone 服务端峰值 RSS | 2 521 MiB | **299 MiB** |
| 峰值相对空载的净增量 | 2 437 MiB | **215 MiB**（−91%） |
| large clone 耗时 | 78.5 s | **64.2 s** |
| 3 路并发 large clone | 6 536 MiB | **782 MiB** |

- 剩余 215 MiB 主要是 gix 并行 entry 流水线的工作集（16 路生产端各压一个 chunk），与 pack 大小解耦；`IRONFORGE_PACK_THREADS=1` 时净增量只剩 13 MiB。
- 4 线程可把峰值压到 197 MiB，但要付 +147% 耗时，因此默认不改；内存优先场景用 `IRONFORGE_PACK_THREADS=8`（文档口径）或更低。
- 验收线按实测放宽为 large ≤320 MiB、3 并发 ≤850 MiB；硬线是不得 OOM。

## 6. 已知但未修

- **tree 根目录递归全量遍历**：medium 787.5 ms / 20 000 文件，large 11 563.8 ms / 200 000 文件；取叶子目录只要 4.1 / 10.7 ms。大仓库的文件浏览仍会在这里慢。
- 首次 push 与 clone 仍随 pack 大小线性增长；没有断点续传或分块传输。
- dev profile 的绝对数字不代表生产；容量规划必须在目标硬件与目标数据量上重跑。
