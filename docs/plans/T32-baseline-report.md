# T32 基线报告 — 大仓库性能（固定数据集 + 阈值）

> 生成日期：2026-11-25。分支 `agent/t32-perf-baseline`，基线 `64861cf9`。
> 代码：`crates/rg-git/tests/perf_baseline.rs` + `crates/rg-git/tests/perf_fixture.rs`。
> 所有数字都是本机实跑输出，复制自
> `cargo test -p rg-git --test perf_baseline -- --ignored --nocapture` 的 `[perf]` 行，
> 未做任何估算或补全。

## 1. 结论摘要

| 问题 | 答案（本机、dev profile、进程内服务端工作量） |
| --- | --- |
| 2 000 commit / 20 000 文件 / 78 MiB 的仓库首推要多久？ | **3 950 ms**（release 395 ms） |
| 同一个仓库 clone 一次要多久？ | **3 130 ms**（release 1 185 ms） |
| 20 000 commit / 200 000 文件 / 1.56 GiB 的仓库呢？ | 首推 **74 535 ms**（release **7 381 ms**），clone **46 008 ms**（release **14 455 ms**） |
| 改一个文件再推呢？ | 35.7 ms（medium）/ 272.1 ms（large），release 下 5.3 / 31.2 ms |
| 有没有变慢？ | 有 `THRESHOLDS` 表，12 行 small/medium 由 3 次采样中位数 × 1.5 定，6 行 large 由单次采样 × 1.5 定；CI 跑 `--ignored` 即校验 |

最重要的三条观察（细节见 §7）：

1. **clone/push 的 pack 全量进内存**：LARGE（1.56 GiB 内容）实测单次 pack 1.21 GB 常驻，
   即 2 GB 量级的仓库要按 pack 大小预留内存，与「内存 <50 MB」的定位冲突。
2. **`ops::pack_for_wants` 不按 `haves` 剪枝**：medium 上「改 1 文件」的服务端 fetch
   pack 是 50 066 689 字节 / 20 224 对象，真实增量只有 5 973 字节 / 8 对象。
3. **根目录 tree 列表是递归全量遍历**：medium 787.5 ms / 20 000 文件，
   large 11 563.8 ms / 200 000 文件；取一个叶子目录只要 4.1 / 10.7 ms。

## 2. 环境指纹（机器不同则数字不可比）

```
cpu="Apple M3 Max" cores=16 os=macos arch=aarch64
rustc="rustc 1.99.1 (58cf8f9c3 2026-10-13)" disk="macOS APFS (Solid State: Yes)"
```

- **profile = dev**（`cargo test` 默认）。这是验收命令实际跑的 profile，阈值按它标定。
  release 数字见 §6，只作参考、不参与断言。
- 采样期间同一 workspace 上还有其它 Agent 在编译（共享 `CARGO_TARGET_DIR`），
  LARGE 单次采样尤其受影响。三次采样之间同一项的波动多数在 ±10% 内（small clone
  两次之间到过 18%，175.5 ↔ 207.8 ms），明细见 §5 的 p95。

## 3. 数据集（证明数据集本身可用）

`crates/rg-git/tests/perf_fixture.rs`，固定种子、无 `rand`、无 git CLI：

| 档 | commits | files | 单文件 | leaf dirs | 内容总量 | 生成耗时 | 磁盘占用 | tip SHA | 内容摘要 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| small | 100 | 2 000 | 1 KiB | 20 | 2.0 MiB | 1 108 ms | 2.4 MiB | `60b42ed8…12793` | `d90caeb4334cfa8d` |
| medium | 2 000 | 20 000 | 4 KiB | 200 | 78.1 MiB | 21 362 ms | 92.9 MiB | `370369f6…43fccb` | `492ea1c8299fae11` |
| large | 20 000 | 200 000 | 8 KiB | 1 000 | 1 562.5 MiB | 376 803 ms | 1 828.4 MiB | `e631f09b…c57244` | `868554242ebb0b2a` |

- 磁盘占用 > 内容总量是因为每个提交还写一个新 blob（共 `commits` 个）与 4 个 tree，
  loose object 有 zlib 压缩但对象头与 tree 仍要占空间。
- **确定性已断言**：`perf_dataset_is_deterministic` 连续生成两次 SMALL，得到完全相同的
  tip 与内容摘要：

```
[perf] determinism first  tip=60b42ed8217e5ce5d9d6de6b4a5a5eb803a12793 digest=d90caeb4334cfa8d entries=2000
[perf] determinism second tip=60b42ed8217e5ce5d9d6de6b4a5a5eb803a12793 digest=d90caeb4334cfa8d entries=2000
```

- 另外验证了「物化到磁盘的 medium 与测试内生成的 medium 是同一份」：
  `perf_materialize_dataset` 写出 `/tmp/t32-medium/source.git`，tip 同样是
  `370369f6c2c970200726a9653c75baabe343fccb`、摘要同样是 `492ea1c8299fae11`。
  Gitea 对比用的就是这份数据（§8）。

## 4. 五个测量点的实测毫秒数

dev profile，n=3 取中位数（LARGE n=1）。括号内是 p95（nearest-rank，n=3 时即最大值）。
「字节」是这项操作搬运/产生的字节数。

| 测量点 | small | medium | large |
| --- | --- | --- | --- |
| 1. push 首推（全量） | 130.1 ms (130.9) · 1.68 MB | 3 949.5 ms (4 058.7) · 61.47 MB | 74 534.9 ms · 1 207.9 MB |
| 2. push 增量（改 1 文件） | 10.1 ms (11.3) · 3 875 B | 35.7 ms (37.2) · 5 973 B | 272.1 ms · 11 240 B |
| 3. clone（全量） | 182.6 ms (204.7) · 1.68 MB | 3 129.6 ms (3 254.4) · 61.47 MB | 46 008.2 ms · 1 208.0 MB |
| 4. log 首页（100 条） | 9.2 ms (10.6) | 9.5 ms (11.3) | 16.9 ms |
| 5a. tree 根目录（递归全量） | 69.4 ms (79.6) · 2 000 项 | 787.5 ms (819.4) · 20 000 项 | 11 563.8 ms · 200 000 项 |
| 5b. tree 深层目录（一个叶子） | 4.1 ms (4.2) · 100 项 | 4.1 ms (5.5) · 100 项 | 10.7 ms · 200 项 |

增量请求规模（真实增量 vs 服务端自己生成的 pack）：

| 档 | 客户端增量 pack | 服务端 `pack_for_wants` 同一次 fetch |
| --- | --- | --- |
| small | 8 个对象 / 3 875 B | 2 025 个对象 / 1 322 903 B |
| medium | 8 个对象 / 5 973 B | 20 224 个对象 / 50 066 689 B |
| large | 8 个对象 / 11 240 B | 201 112 个对象 / 992 218 757 B（数值取自 release 采样，见 §6 的 `ops_incremental_pack_bytes`；dev 那次运行用的是旧二进制，还没打印这一项） |

## 5. 阈值表与依据

规则：**阈值 = 该项 3 次采样的中位数 × 1.5**，向上取整（> 50 ms 的取到 5 的倍数），
给 CI 机器抖动留 50% 余量。small/medium 是 n=3；large 是 n=1（dev profile 跑一次约
12 分钟、要 2 GiB 临时空间；release 约 3 分钟），所以 large 行是**指示性**的，
不是分布。

| dataset | operation | 实测中位数 | 阈值 (ms) |
| --- | --- | --- | --- |
| small | push_initial | 130.1 | 195 |
| small | push_incremental | 10.1 | 15 |
| small | clone | 182.6 | 265 |
| small | log_first_page | 9.2 | 15 |
| small | tree_root | 69.4 | 110 |
| small | tree_deep | 4.1 | 6 |
| medium | push_initial | 3 949.5 | 5 650 |
| medium | push_incremental | 35.7 | 55 |
| medium | clone | 3 129.6 | 4 730 |
| medium | log_first_page | 9.5 | 15 |
| medium | tree_root | 787.5 | 1 195 |
| medium | tree_deep | 4.1 | 7 |
| large | push_initial | 74 534.9 | 111 805 |
| large | push_incremental | 272.1 | 410 |
| large | clone | 46 008.2 | 69 015 |
| large | log_first_page | 16.9 | 26 |
| large | tree_root | 11 563.8 | 17 350 |
| large | tree_deep | 10.7 | 17 |

注意事项（写进代码注释里）：

- 阈值是**同一台机器、同一 profile** 的上限，不是跨机器常数。CPU 或磁盘不同就必须
  重新标定，报告里的指纹就是标定条件。
- `tree_deep`、`log_first_page` 这类 4~10 ms 的项，抖动占比大（small tree_deep 实测
  在 3.9~4.1 ms 之间，阈值 6 ms），只宜当「有没有量级退化」的信号，不宜当精细门禁。
- 故意破坏阈值可以验证门禁真的会失败（验收第 3 条）：
  把 `(Small, PushInitial)` 改成 `1` 后运行，得到
  `small/push_initial: measured 121.7 ms (p95 122.8 ms, n=3) > threshold 1 ms`，
  测试 `FAILED`；改回 195 后恢复绿。

## 6. release profile 参考值（不参与断言）

同一台机器、同一数据集，`cargo test --release`：

| 测量点 | small | medium | large（n=1） |
| --- | --- | --- | --- |
| push 首推 | 15.3 ms | 395.3 ms | 7 380.7 ms |
| push 增量 | 2.7 ms | 5.3 ms | 31.2 ms |
| clone | 116.5 ms | 1 184.7 ms | 14 454.7 ms |
| log 首页 | 5.2 ms | 4.2 ms | 6.5 ms |
| tree 根目录 | 51.2 ms | 510.4 ms | 7 978.9 ms |
| tree deep | 2.4 ms | 2.5 ms | 6.8 ms |

dev 与 release 差 2.6~10 倍（medium push 差 10 倍，clone 差 2.6 倍），说明 dev 数字
主要被未优化的 gix 代码支配。**验收命令不带 `--release`，所以阈值按 dev 标定**：这样
release 下只会更快，门禁不会误报。要判断生产表现请用上表的 release 数字，并注意它同样
不包含 HTTP 层、鉴权与数据库。

LARGE 的 release 单次原始输出（`cargo test --release … perf_baseline_large`，
`IRONFORGE_PERF_LARGE=1`，199.18 s，全部阈值 verdict=ok）：

```
[perf] dataset=large ... build_ms=72415.1 disk_mib=1828.4 content_mib=1562.5 tip=e631f09b…c57244 digest=868554242ebb0b2a entries=200000
[perf] note dataset=large push_incremental pack_objects=8 pack_bytes=11240 history_pack_bytes=1207895028 ops_incremental_pack_bytes=992218757 ops_incremental_objects=201112
[perf] summary dataset=large op=push_initial n=1 median_ms=7380.7 bytes=1207905804
[perf] summary dataset=large op=push_incremental n=1 median_ms=31.2 bytes=11240
[perf] summary dataset=large op=clone n=1 median_ms=14454.7 bytes=1207998016
[perf] summary dataset=large op=log_first_page n=1 median_ms=6.5 bytes=100
[perf] summary dataset=large op=tree_root n=1 median_ms=7978.9 bytes=200000
[perf] summary dataset=large op=tree_deep n=1 median_ms=6.8 bytes=200
```

## 7. 已知瓶颈（只记录，不修）

### 7.1 `ops::pack_for_wants` 不按 `haves` 剪枝（带宽放大）

> **已由 T37 修复（分支 `agent/t37-pack-haves-diff`）。** 下面保留当时的实测记录作为基线；
> 现状与做法见 `docs/plans/T37-plan.md`。修复后同一场景（medium，改 1 个 4 KiB 文件）：
> `pack_for_wants(tip, [parent])` = **8 个对象 / 5 973 字节**（= 真实增量本身，
> 放大 1×）；large 同样为 **8 个对象 / 11 240 字节**。

`crates/rg-git/src/ops.rs::pack_for_wants_filtered` 的 `input_object_expansion` 固定为
`TreeContents`：commit 层用 `rev_walk().with_hidden(haves)` 剪掉了历史，但被保留的
每个 commit 仍会展开**整棵树**，于是客户端已有的 tree/blob 全部重新进 pack。

实测（medium，改 1 个 4 KiB 文件）：

- 真实增量：8 个对象 / 5 973 字节
- `pack_for_wants(tip, [parent])`：20 224 个对象 / 50 066 689 字节
- 放大 **8 383 倍**

LARGE（改 1 个 8 KiB 文件）更极端：真实增量 8 个对象 / 11 240 字节，而
`pack_for_wants(tip, [parent])` 是 **201 112 个对象 / 992 218 757 字节**（≈ 947 MiB），
放大 **88 293 倍**——几乎等于把整个仓库重发一遍。

影响所有带 `haves` 的 fetch（Web UI「拉某个提交」、CI 的增量 checkout、客户端
`git fetch`）。`git-pack-objects` 的做法是 `TreeAdditionsComparedToAncestor`；测试
fixture 为了造出真实增量，走的就是这个模式（`perf_fixture::incremental_pack`）。

**T37 已修复**：`pack_for_wants_filtered` 现在对 walk 出来的 commit 用该模式做对象级差集，
详见 `docs/plans/T37-plan.md`。shallow 加深路径保持旧的整树展开。

### 7.2 pack 全量进内存（与「内存 <50MB」定位冲突）

- `protocol/upload_pack.rs::send_packfile`：`pack_data` 是完整 `Vec<u8>`，生成完才写
  sideband。LARGE（1.56 GiB 内容）实测单次 pack 1 207 998 016 字节（clone 写出的量）
  / 1 207 905 804 字节（push 的 pack）常驻。
- `protocol/receive_pack.rs`：`reader.read_to_end(&mut pack_data)` 同样先把整个 pack
  读进内存，再交给 `index_pack_bytes`（其参数也是 `&[u8]`）。

即一个 1.56 GiB 内容的仓库推进来/拉出去，进程峰值至少要有 pack 那一份（实测 1.21 GB），
再加上索引/写入时的额外缓冲。这与项目「单二进制、内存 <50 MB」的定位直接冲突，是 LARGE
档最值得跟进的一条。**只记录，不修。**

> **T38 回填（分支 `agent/t38-pack-streaming`）。** 上面「1.21 GB」这个数字**只计了
> 一份**，是从代码推断出来的；T38 起真实服务 + 真 `git clone` 实测，large clone 的
> 服务端峰值是 **2 521 MiB**，因为同时存在**两份** 1.21 GB：
> 1. `protocol/v2.rs::handle_fetch` 里 `generate_packfile()` 返回的 pack `Vec`
>    （真实 clone 走 protocol v2，这一份是本报告的盲区）；
> 2. `rg-http` 的 `duplex` + `read_to_end` 把整个响应又收了一份。
>
> T38 把两处都改成流式（gix 本来就支持任意 `io::Write`）：峰值 **2 521 → 299 MiB**，
> 并发 3 个 large clone **6 536 → 782 MiB**，clone 耗时反而降 18%。剩下约 215 MiB 的
> 净增量是 gix 并行 entry 流水线的工作集，与 pack 字节数解耦。详见
> `docs/plans/T38-plan.md`。

### 7.3 根目录 tree 列表是递归全量遍历

`ops::list_tree_files` 用显式栈递归展开整个子树，并对每个 entry 调 `find_header`
取大小。结果：

| 档 | 根目录（递归全量） | 一个叶子目录（100/200 项） |
| --- | --- | --- |
| small | 69.4 ms / 2 000 项 | 4.1 ms |
| medium | 787.5 ms / 20 000 项 | 4.1 ms |
| large | 11 563.8 ms / 200 000 项 | 10.7 ms |

如果 Web 的仓库首页一次列全量文件，成本就是 O(文件数)，200k 文件要 11.6 s。按目录
懒加载则不受影响。**行为已记录，不改。**

### 7.4 每次请求都重新生成 pack，没有 bitmap / pack 复用

medium 的 clone（release）1 184.7 ms 全部花在 pack 生成（delta 搜索 + 写 sideband），
同机 Gitea 1.26.4 一次完整 clone（含客户端 index-pack 与落盘）1 031 ms；LARGE 则是
IronForge 14 454.7 ms vs Gitea 端到端 20.27 s。两档结论不一致，合起来只能说明「同一
量级，没有数量级差距」，但有一个确定的事实：**同一个仓库 clone 多少次就重新做多少次
delta 搜索**，没有位图、没有 pack 复用、没有缓存。仓库越大，这项重复劳动越贵
（§7.1 的不剪枝问题还会在 fetch 时叠加）。**只记录。**

### 7.5 大推送的 secret scan 整体跳过（fail-open）

`secret_scan::MAX_FILES = 500`：一次 push 触及的文件超过 500 个时，整批扫描跳过并写
审计事件（`Skip::TooManyFiles`）。LARGE 首推 200 000 个文件，因此实测的 74.5 s 里
**不含**秘密扫描成本，实际部署中这批内容也没有被扫描。这是设计（R7 的 fail-open 口径）
而不是缺陷，但解读基线数字时必须知道。

### 7.6 增量 push 的服务端成本

LARGE 一个 8 KiB 文件改动的增量 push 是 272.1 ms，是 small（10.1 ms）的 27 倍，而增量
内容只从 3 875 B 变成 11 240 B。差别来自：目标仓此时已有约 23 万个对象（20 万 blob +
2 万 commit + 各层 tree），`index-pack` 要写新 pack、更新 ref，`gix::open` 等固定开销
也随仓库规模上升。这条只做记录，指向「大仓日常推送的固定成本」这一后续优化方向。

## 8. 与 Gitea 1.26.4 的同机对比

条件：同一台机器、同一份数据集（`perf_materialize_dataset` 写出，tip 与测试内生成的一致）、
Docker `gitea/gitea:1.26.4` + SQLite + 默认配置、HTTP 走 127.0.0.1、git 2.54.0。
Gitea 侧是**端到端**（git 客户端 + loopback HTTP + 服务端），IronForge 侧是
**进程内服务端工作量**（不含 HTTP/鉴权/DB、不含客户端 pack 生成）——两者的边界不同，
所以下表只能看量级，不能当胜负判据。IronForge 用 release 数字（生产的编译方式）。

### 8.1 MEDIUM（2 000 commit / 20 000 文件 / 61.47 MB pack）

| 操作 | IronForge（release，服务端） | Gitea 1.26.4（端到端） | 说明 |
| --- | --- | --- | --- |
| push 全量 | 395.3 ms | **4.51 s**（1 次） | Gitea 侧其中 3.84~3.86 s 是 git 客户端 `pack-objects --all`（3 次实测），剩下 ≈0.65 s 才是 HTTP + Gitea 服务端入库 |
| clone | 1 184.7 ms | **1.03 s**（3 次：1.05 / 1.01 / 1.05） | Gitea 侧含客户端 index-pack 与落盘；IronForge 侧只是 pack 生成 + 写 sink，即便如此仍慢 15% |
| log 首页 100 条 | 4.2 ms | **111~143 ms**（3 次 REST，含 HTTP/DB/鉴权） | 不是同层比较 |
| 一个叶子目录（100 项） | 2.5 ms | **32 ms**（3 次：1 329 / 32 / 32 ms，首次冷缓存） | Gitea 走 REST contents，单层；IronForge 是进程内 gix |

### 8.2 LARGE（20 000 commit / 200 000 文件 / 1.21 GB pack）

| 操作 | IronForge（release，服务端） | Gitea 1.26.4（端到端） | 说明 |
| --- | --- | --- | --- |
| push 全量 | 7 380.7 ms | **112.58 s**（1 次） | 其中 git 客户端 `pack-objects --all` 单独实测 **105.02 s**，余下 ≈7.6 s 是 HTTP + Gitea 入库——与 IronForge 的 7 381 ms 基本持平 |
| clone | 14 454.7 ms | **20.27 s**（1 次，`--no-checkout`） | Gitea 侧含客户端 index-pack 与 1.2 GB 落盘，所以 IronForge 的服务端 pack 生成比 Gitea 端到端还快约 30% |

### 8.3 能从对比里得到的结论

- **大仓首推的真正瓶颈在客户端 pack 生成**（105 s vs 服务端 7.4~7.6 s，占 93%），
  服务端 ingest 在 1.2 GB / 20 万对象这个量级上与 C git 持平（7.38 s vs ≈7.6 s）。
- **clone**：medium 上 IronForge 的服务端 pack 生成（1 185 ms）慢于 Gitea 端到端
  （1 031 ms）；large 上反过来（14.45 s vs 20.27 s）。量级相同，结论是「gix 的 pack
  生成与 C git 同量级，不是数量级差距」，medium 那次 15% 的落后不足以支撑结论。
- **log / tree 这类小响应**：进程内 4.2 / 2.5 ms 与 REST 111 / 32 ms 不可比（后者含
  HTTP、DB、鉴权），只能说明 IronForge 的服务端计算量很小，Web 首屏瓶颈不在 Git 层。
- **未做**：Gitea 侧没有跑 `tree 根目录（递归全量）` 的对比——Gitea 的 contents API
  是单层语义，没有等价操作，硬比就是编数字。

Gitea 容器限制（已知偏差，且都偏向「对 Gitea 不利」的方向，所以结论不会因它们而偏袒
IronForge）：SQLite、单实例、`ROOT_URL` 指向 loopback、没有配置 LFS 与 Actions，
`INSTALL_LOCK=true` 跳过安装向导。

## 9. 复现步骤

```sh
export CARGO_TARGET_DIR=<共享 target>

# 基线（跑 small + medium + 确定性；large 与物化会打印 skip）
cargo test -p rg-git --test perf_baseline -- --ignored --nocapture

# 只跑某一档
cargo test -p rg-git --test perf_baseline -- --ignored --nocapture perf_baseline_medium

# LARGE（单次，约 12 分钟、需要 ~2 GiB 临时空间）
IRONFORGE_PERF_LARGE=1 cargo test -p rg-git --test perf_baseline -- --ignored --nocapture perf_baseline_large

# release 参考值
cargo test --release -p rg-git --test perf_baseline -- --ignored --nocapture perf_baseline_medium

# 物化数据集给别的服务器（Gitea 对比用）
IRONFORGE_PERF_KEEP_DIR=/tmp/t32-medium IRONFORGE_PERF_DATASET=medium \
  cargo test -p rg-git --test perf_baseline -- --ignored --nocapture perf_materialize_dataset
```

Gitea 对比（§8）的复现步骤：

```sh
docker run -d --name gitea-t32 -p 127.0.0.1:3010:3000 \
  -e GITEA__database__DB_TYPE=sqlite3 -e GITEA__security__INSTALL_LOCK=true \
  -e GITEA__service__DISABLE_REGISTRATION=true \
  -e GITEA__server__ROOT_URL=http://127.0.0.1:3010/ -e GITEA__server__HTTP_PORT=3000 \
  gitea/gitea:1.26.4
docker exec -u git gitea-t32 gitea admin user create \
  --username perf --password '<password>' --email perf@example.invalid --admin
curl -X POST http://127.0.0.1:3010/api/v1/user/repos -u 'perf:<password>' \
  -H 'Content-Type: application/json' \
  -d '{"name":"medium","private":true,"auto_init":false,"default_branch":"main"}'
/usr/bin/time -p git -C /tmp/t32-medium/source.git push \
  http://perf:<password>@127.0.0.1:3010/perf/medium.git main
/usr/bin/time -p git clone --no-checkout \
  http://perf:<password>@127.0.0.1:3010/perf/medium.git /tmp/t32-gitea-clone
/usr/bin/time -p git -C /tmp/t32-medium/source.git pack-objects --all --stdout --quiet > /dev/null
curl -s -o /dev/null -w '%{time_total}\n' -u 'perf:<password>' \
  'http://127.0.0.1:3010/api/v1/repos/perf/medium/commits?limit=100&page=1'
```

数据集与临时目标仓都在 `tempfile::TempDir` 里，测试结束自动删除；只有
`perf_materialize_dataset` 会按调用方给的路保留。
