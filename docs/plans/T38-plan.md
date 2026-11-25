# T38 执行计划 — pack 流式化

> 权威约束见 `tasks/T38.md`。基线 `3dfdcd11`（main），分支 `agent/t38-pack-streaming`，
> worktree `../ironforge-t38`。
>
> 本文件记录：改了什么、为什么这么改、RSS 前后实测（含方法）、协商期失败点排查、
> 超时语义变化、正确性证据、**没达到的验收线及其原因**、未做项。

## 1. 一句话

一次 large（20 000 commits / 200 000 files / 1.8 GiB）clone 的服务端峰值 RSS
**2 521 MiB → 299 MiB**（V1：2 541 → 271 MiB），并发 3 个 **6 536 MiB → 782 MiB**，
clone 耗时 78.5 s → 64.2 s。剩下的大头（净增量 215 MiB）不是我们的缓冲，而是 gix 并行
entry 流水线的工作集，与 pack 字节数解耦（§6.4 有拆解）。

## 2. 根因（两处，都实测确认）

1. `ops::generate_pack_with` 把编码结果写进 `Vec<u8>`（旧 `ops.rs:1092`）。large 数据集
   实测该 Vec 约 1.21 GB。
2. `rg-http` 的 V1/V2 upload-pack 用 `duplex(64 KiB)` + `read_to_end` 把响应**再收一份**
   到第二个 `Vec`。所以 2 521 MiB ≈ 两个 1.21 GB 加上基数。

只修第 2 处（rg-http 不再收）就已经把 large 从 2 521 MiB 降到 1 438 MiB，这一步先单独
量了一次，用来确认两处的相对贡献。

**还要修 `protocol/v2.rs`**：真实 `git clone` 默认走 protocol v2，V2 的 pack 由
`v2.rs::handle_fetch` 调用 `generate_packfile()` 拿 `Vec<u8>` 再 `write_sideband_data`。
只改 `upload_pack.rs`（V1）对 large clone 的 RSS 没有作用。这一点在动手前单独向
team-lead 报告过（越界项）。

## 3. 实现

### 3.1 `crates/rg-git/src/ops.rs`：泛型化，旧签名保留

新增（全部 `pub`，`W: std::io::Write`，返回 `W`）：

| 新函数 | 旧函数（保留，变成薄包装） |
|---|---|
| `generate_pack_into` | `generate_pack` |
| `generate_pack_filtered_into` | `generate_pack_filtered` |
| `generate_pack_filtered_with_tips_into` | `generate_pack_filtered_with_tips` |
| `pack_universe_into` | `pack_universe` |
| `pack_for_wants_into` | `pack_for_wants` |
| `pack_for_wants_opts_into` | `pack_for_wants_opts` |
| `pack_for_wants_filtered_into` | `pack_for_wants_filtered` |

旧函数一律

```rust
let mut out = Vec::new();
pack_for_wants_into(..., &mut out)?;
Ok(out)
```

所以 `pack_universe` 的调用方（`fork_repo`、`rg-core`）、全部现有测试一个字都不用改。
`FromEntriesIter::new(in_order, &mut out, ...)` 的 `W` 就是 `std::io::Write`
（gix-pack 0.77 本来就支持），pack 逐 entry 编码后直接交给 sink。

同一文件里还加了 `pack_thread_limit()` / `parse_pack_threads()`：
`IRONFORGE_PACK_THREADS` 会同时传给 `count::objects` 与 `iter_from_counts` 的
`Options.thread_limit`，用来把「内存 ↔ 速度」交给运维，默认不变（§6.6）。

### 3.2 `crates/rg-git/src/protocol/upload_pack.rs`：`stream_pack`

```text
spawn_blocking                        async 侧
──────────────                        ────────
PackSink (impl Write)                 mpsc::Receiver<Vec<u8>>
  ├ 攒满 65 515 B 就 hand_off()        └ 每块包成 band-1 pkt-line 写出去
  └ blocking_send  ← 队列深度 4  ←── 背压
```

- **为什么不用 `SyncIoBridge`**：`rg-git` 的 `Cargo.toml` 里没有 `tokio-util`，而任务卡
  禁止改 `Cargo.toml`。`tokio::sync::mpsc::Sender::blocking_send` 在 `spawn_blocking`
  线程里是合法用法（它只在 async worker 上调用才 panic），效果与 `SyncIoBridge` 等价：
  同步写阻塞 → 有界队列满 → 生产者停下来。
- **有界**：4 × 64 KiB = 256 KiB，这是流式 pack 的全部内存预算。没有用
  `unbounded_channel`（那等于把问题原样搬回来）。
- **`JoinHandle` 必达**：`stream_pack` 一定 `handle.await`，区分 `JoinError`（panic/取消）
  与业务错误；拼接泵与生产者两边的错误，客户端先断时返回泵的错误、同时 `warn!` 记下
  生产者的真实原因。
- **分帧逐字节等价**：`PACK_CHUNK = MAX_PKT_LINE_LEN - 1 = 65 515`，与旧
  `sideband::write_sideband_data` 的分片大小一致。单测
  `sideband_framing_matches_the_buffered_writer` 用不规则写入（7/4096/65515/1/999 字节
  混合）证明输出与 `write_sideband_data(&data)` **逐字节相同**。

### 3.3 `crates/rg-git/src/protocol/v2.rs`：V2 fetch 也流式

`generate_packfile()`（返回 `Vec`）→ `generate_packfile_into(out)`；`handle_fetch` 里

```rust
write_pkt_line(writer, &PktLine::text("packfile")).await?;   // 位置未变
write_sideband_progress(writer, "Enumerating objects: done.\n").await?;
let pack_size = super::upload_pack::stream_pack(writer, use_sideband, move |out| {
    generate_packfile_into(..., out)?; Ok(())
}).await?;
write_sideband_progress(writer, "Done.\n").await?;
write_sideband_flush(writer).await?;
```

section 头、"Enumerating/Done" 进度、flush 的位置与内容都没变，变的只是 pack 不再先在
内存里攒齐。协商 / ls-refs / object-info 一行未动。

### 3.4 `crates/rg-http/src/lib.rs`：响应体流式

V1 与 V2 两个分支统一走 `streamed_git_response()`：

- handler 在独立 task 上跑，写进 `duplex(64 KiB)`；
- 响应是 `200 OK` + `Body::from_stream(ReaderStream::new(read_half))` —— 超集为
  **没有 `Content-Length`，chunked**，git 客户端无感；
- handler 的错误在 task 里 `tracing::error!` 打出真实原因（带 repo 与格式化后的完整
  `{:#}` 链），因为状态码已经发出去了，日志是唯一线索；
- `receive-pack` 那处 `read_to_end` **保留**，并在代码里写明原因（响应是 push 校验完
  才有的几百字节 report-status，流式化会让「被拒绝的 push」无法用状态码表达）。

## 4. 协商期失败是否都早于「流出」

要求是：pack 开始流之前，所有可能失败的事都已经做完。逐条排查 `handle_fetch` /
`handle_upload_pack_http`：

| 步骤 | 位置 | 是否早于 pack 流出 |
|---|---|---|
| 仓库存在 / 读权限 / 维护模式 | `rg-http` handler，进 streamed 之前 | ✅ 仍是真正的 HTTP 状态码（404/403/503） |
| 请求 pkt-line 解析（want/have/capabilities） | `read_want_have_impl` / `read_command_request` | ✅ 在写 pack 之前；失败 → 空响应 + `error!` |
| filter 规格校验（`blob:limit=` 语法等） | `validate_fetch_features` | ✅ 在 pack 之前，失败直接返回错误 |
| shallow 边界计算（`build_shallow_update`） | handle_fetch 开头 | ✅ 在 pack 之前 |
| **want 解析 / peel / rev_walk** | `ops::pack_for_wants_filtered_into` 内部、**在第一个 pack 字节之前** | ⚠️ 在 pack 之前完成，但**在 `"packfile"` section 头之后** |
| 计数（`count::objects`） | 同上 | ⚠️ 同上 |
| entry 编码读对象 | 同上 | ❌ 会在流出中途失败 |

结论：**「会产生 pack 字节」的所有失败点都已在流出开始前完成**；唯一「流到一半才可能
失败」的是对象库读失败（磁盘/损坏），这属于任务卡接受的那一类。

**V2 没有任何「优雅报错」的可能，必须写清楚**：`"packfile"` section 头在 pack 生成**之前**
就写出去了（位置与旧代码一致，不能挪，否则破坏分节顺序）。因此包生成中途失败时，
V2 **不可能**再补一个 band-3 错误帧，也不可能改状态码——客户端只会看到 `early EOF`。

V1 同理（`NAK` 先发）：旧实现能返回 500 是因为它攒完了才写，而那正是本任务要消掉的行为。

残留的粗糙之处：want 不可解析这类**协议错误**也发生在 `"packfile"` 头之后，所以现在同样
只能断流。要彻底改善，得把 section 头的写出推迟到「第一个 entry 成功编码」之后，那是
协议层改动，不在本任务范围（见 §9 未做项）。

### 4.1 端到端验证「流中途失败」

把一个 300 KB 的仓库副本里的 blob 对象清空（模拟对象库损坏），再用真 git 客户端 clone：

```text
客户端：Cloning into '…'...
        remote: Enumerating objects: done.
        fatal: early EOF
        fatal: fetch-pack: invalid index-pack output

服务端：ERROR git response stream failed repo="failuser/broken" what="upload-pack v2"
        error=failed to write pack entry: Pack entry input iterator failed
```

这正是任务卡接受的形态：客户端只有 early EOF，**真实原因在服务端日志里**，且状态码是
200（headers 早已发出）。注意错误链到 gix 的 `Pack entry input iterator failed` 就断了，
`anyhow` 的 `{:#}` 里看不到「哪个对象读失败」——这是 gix 的错误粒度，不是我们的取舍。

## 5. 超时语义变化（如实记录）

`git_stream_timeout()`（默认 1 小时，`IRONFORGE_GIT_STREAM_TIMEOUT_SECS` 可调）**仍然
包住整个 handler**，但在流式化之后它的**可观测效果**变了：

| | 改动前 | 改动后 |
|---|---|---|
| 超时触发时客户端看到 | `504 GATEWAY_TIMEOUT` + `git operation timed out` | **截断的响应流（early EOF）** |
| 服务端 | `warn!` 日志 | `warn!` 日志（`git response stream exceeded the deadline; body cut short`） |
| 阻塞的 pack 生成 | 读端被 abort → 写 duplex 失败 | 写端 drop → `blocking_send` 失败 → gix 编码器拿到 `BrokenPipe` 提前停 |

也就是说：**截止时间仍然限制了服务端工作量**（超时会真正让 pack 生成停下来，不会继续
空转烧 CPU），但**不再体现为状态码**。文档与代码注释都按这个说法写，没有假装 504 还在。

`receive-pack` 仍是原来的语义（包住整个 handler，超时返回 504）。

## 6. RSS 实测

### 6.1 方法

- 起**真实服务**（`ironforge serve`，dev profile，`cargo build -p rg-cli` 产物），
  SQLite 临时库，`--repo-root` 下把数据集 bare 仓库软链进去，用 API 注册用户 + 建仓。
- 客户端是**真实 git 2.54**：`git clone http://…`（默认 protocol v2）。
- 采样：`ps -o rss= -p <server_pid>` 每 50 ms 一次，取最大值；另记服务空闲时 RSS 作基线。
- `cargo test` 进程不是被测进程，所有数字都来自服务进程。
- 脚本：`/tmp/t38-measure.sh`（一次性工具，未入库）。

### 6.2 单次 clone —— 按协议版本分行

真实客户端默认 V2；`PROTO=1` 用 `git -c protocol.version=1` 强制走 V1。两条路径都是
真实 `git clone`、都验了 `fsck --strict`。**峰值增量 = 峰值 − 空载基线（84 MiB）**，
这是流量化真正的判据（拆解见 §6.4，结论见 §6.5）。

| 场景 | 改前峰值 | 改后峰值 | 空载基线 | 净增量（改前 → 改后） | 改前耗时 | 改后耗时 |
|---|---|---|---|---|---|---|
| **large V2**（默认） | 2 521 MiB | **299 MiB** | 84 MiB | **2 437 → 215 MiB**（−91%） | 78.5 s | **64.2 s** |
| **large V1**（`protocol.version=1`） | 2 541 MiB | **271 MiB** | 84 MiB | **2 457 → 187 MiB**（−92%） | 73.4 s | **59.4 s** |
| medium V2 | 262 MiB | **161 MiB** | 84 MiB | **178 → 77 MiB**（−57%） | 6.06 s | **5.05 s** |

两次 large 的 HEAD 都是 `e631f09b8c9853d3d688accbba51459c68c57244`，fsck 全绿。
V1 比 V2 少十几 MiB，因为它没有 V2 的 `"packfile"` section 头与两条进度帧，差异在噪声量级。

中间态：只改 rg-http、尚未改 `v2.rs` 时 large 是 **1 438 MiB** —— 由此可算出第二个
`Vec` 约 1.08 GB，与 `v2.rs` 的 pack Vec 各占一半（§2）。

数据集来自 `perf_fixture`（确定性）：medium tip `370369f6…`，large tip `e631f09b…`。

### 6.3 并发 3 个 large clone（V2）

| | 峰值 RSS | 净增量（−84 MiB 空载） | 三个 clone 的 wall time |
|---|---|---|---|
| 改动前 | **6 536 MiB** | 6 452 MiB | 148.1 s（最快 146.7 s） |
| 改动后 | **782 MiB** | **698 MiB** | 128.0 s（最慢 127.9 s） |

净增量 ≈ 3 × 233 MiB，**线性**——不随总传输量（约 5.4 GB）膨胀，也不随并发数超线性增长。
三个 clone 全部 `git fsck --strict` 通过，HEAD 均为 `e631f09b…`。

### 6.4 那 215 MiB 净增量是什么（固定开销拆解）

`ps` 只能给总量，所以先用一个**不起服务**的探针（`io::sink()` 直接调
`pack_for_wants_into`，进程基线 9 MiB）把流水线分段：

| 阶段 | 峰值 RSS |
|---|---|
| 进程刚起 | 9 MiB |
| 计数（`count::objects`）完成，321 106 个 entry | 74 MiB |
| entry 迭代器就绪 | 85 MiB |
| 编码 + 写出期间 | **208 MiB** |

再把各项拆开（large，321 106 个 entry）：

| 项 | 值 | 依据 |
|---|---|---|
| 服务空载 RSS（无请求） | 84 MiB | 起服务不 clone，采样 |
| `counts` Vec | **17.2 MiB** | 321 106 × `size_of::<Count>()` = 321 106 × 56 B |
| 计数阶段的遍历 / odb / tree-diff 瞬时 | ≈ 48 MiB | 探针 74 − 9（进程） − 17.2（counts） |
| 编码期「在飞」entry（**主项**） | ≈ 123 MiB | 探针 208 − 85；机制见下 |
| 其它（axum、DB pool、duplex、allocator slack） | 余量 ≈ 27 MiB | 299 − 84 − 17.2 − 48 − 123 |

「在飞 entry」的机制不是猜的：gix-pack 的 `Entry` 文档自己写着
*“Some of these will be in-flight and in memory while waiting to be written. Memory
requirements depend on the amount of compressed data they hold.”* 而
`gix_parallel::optimize_chunk_size_and_thread_limit` 会把 chunk 大小重算成
`items / (threads × 2)`，上限 1000 —— large 下就是 **1000 个 entry / chunk**。
16 个生产端各压着一到两个 chunk（每个 chunk ≈ 1000 × 平均压缩后 ≈ 3.9 KiB ≈ 3.8 MiB），
消费端（编码器）单线程跟不上 16 路生产，于是 16 个 chunk 同时在内存里。

这也解释了为什么**改 `chunk_size` 没有用**（试过 1/2/10/40，峰值 208/228/208/213 MiB）：
它被 gix 覆盖了。唯一有效的旋钮是 `thread_limit`。

**净增量与 pack 大小的关系**：它不随 pack 字节数增长，而随「在飞 entry 数 × 单个对象大小」
走。实测证据：

- V1 187 MiB 与 V2 215 MiB 同量级（两条路径同样 321 106 个对象）；
- 并发 3 个时 782 − 84 = 698 ≈ 3 × 233，**线性**、没有随总传输量（5.4 GB）膨胀；
- 改前是 2 437 MiB 净增量、且与 pack 一比一 —— 那才是「与 pack 大小成正比」。

### 6.5 绝对线 200 MB 为什么达不到，以及为什么仍然达标

原任务卡的绝对线是「large < 200 MB、并发 3 < 400 MB」。按 §6.4 的拆解，这两条在
**空载基线本身就有 84 MiB**、且 gix 流水线工作集 208 MiB 的前提下不可能达到——除非把
并行度砍到 `thread_limit ≤ 4`。所以判据改成「**峰值 RSS 相对空载的净增量**」
（team-lead 已据此修订 `tasks/T38.md` 的行：large ≤ 320 MiB、并发 3 ≤ 850 MiB、
medium ≤ 180 MiB，硬线仍是「并发 3 个不得 OOM」）。

在 large 数据集上用 `io::sink()` 直接调 `pack_for_wants_into`（不起服务、不联网），
单独量 pack 流水线的峰值：

| 阶段 | RSS |
|---|---|
| 进程刚起 | 9 MiB |
| 计数（`count::objects`）完成，321 106 个 entry | 74 MiB |
| entry 迭代器就绪 | 85 MiB |
| 编码 + 写出期间的峰值 | **208 MiB** |

按 `thread_limit` 扫一遍（同一数据集、同一 sink）：

| thread_limit | 峰值 RSS | 耗时 |
|---|---|---|
| 1 | 78 MiB | 331.8 s |
| 2 | 81 MiB | 173.0 s |
| 4 | 113 MiB | 95.2 s |
| **8（内存优先档）** | **123 MiB** | **56.6 s** |
| 16（默认） | 208 MiB | 39.2 s |

`chunk_size` 试过 1 / 2 / 10 / 40，峰值 208 / 228 / 208 / 213 MiB —— 无关
（`gix_parallel::optimize_chunk_size_and_thread_limit` 会按 items/threads 重算它）。
结论：**剩下的 200 MiB 是 gix 并行 entry 流水线的工作集**（生产端 16 路，消费端单线程，
`InOrderIter` 里在飞的 chunk），不是我们的缓冲。

本任务的取舍：**保留默认并行度，不用时间换内存**（RSS 降 8.4×，clone 反而快 18%）。
砍并行度能把绝对值压到目标，但 4 线程要付 +147% 时间、且 3 并发仍到不了 400 MiB；
8 线程是折中最优（§6.6），做成运行时旋钮交给运维，默认不动。team-lead 的裁决
（接受实测值、按净增量口径验收、`IRONFORGE_PACK_THREADS` 并入本任务）即基于这组数据。

### 6.6 `IRONFORGE_PACK_THREADS`：把「内存 ↔ 速度」交给运维

`ops.rs` 读 `IRONFORGE_PACK_THREADS`，同时传给 `count::objects` 与 `iter_from_counts` 的
`Options.thread_limit`。**不设 = 全部核心**（历史行为完全不变）；`0` / 空值 / 非法值
一律 `warn!` 后回落默认，不会被字面当成 1 线程——否则一个 typo 会表现为「神秘的 8 倍
变慢」，而不是一条能看见的告警。每次 pack 生成读一次环境变量（一次 `getenv`，相对
秒级 pack 可忽略），换来实现可测（`tests/pack_threads.rs`）。

档位（large 数据集，pack 阶段实测）：

| `IRONFORGE_PACK_THREADS` | pack 生成 | pack 阶段内存 | 建议场景 |
|---|---|---|---|
| 16（默认，不设） | 39.2 s | 208 MiB | ≥8 GB 空闲内存，追求 clone 速度 |
| **8** | **56.6 s** | **123 MiB** | 4–8 GB，均衡 —— **最优点** |
| 4 | 95.2 s | 113 MiB | <4 GB 或并发 clone 多 |
| 1 | 331.8 s | 78 MiB | 只作对照，实际不可用 |

8 线程是这张表的最优点：内存降 **41%**，pack 阶段只多 17 秒；相对 4 线程（内存再降
10 MiB、时间多 39 秒），交易明显更好。

端到端交叉验证（medium，真实服务 + 真 clone；16 线程那行是 §6.2 的数字）：

| 设置 | medium 峰值 | 净增量 | clone 耗时 |
|---|---|---|---|
| 不设（16） | 161 MiB | 77 MiB | 5.05 s |
| `=8` | 136 MiB | **51 MiB** | 5.87 s（+16%） |
| `=1` | **98 MiB** | **13 MiB** | 38.91 s |

`=1` 这行同时证明两件事：旋钮**真的生效**（峰值 161 → 98、耗时 5 → 39 s），以及
「净增量几十 MB」在关掉并行后确实成立（13 MiB）——反过来说，那 200 MiB 就是并行流水线，
不是泄漏，也不是我们的缓冲。

**默认值故意不改成 8。** 299 MiB 单 clone、782 MiB 三并发在 4 GB 以上机器都安全；
而默认 8 线程会让每个用户 clone 慢约 26%。为最紧的内存场景牺牲多数场景，等于替用户做
了他们没要求的交易。要省内存的部署自己设一个环境变量即可。

正确性不随旋钮变：`tests/pack_threads.rs` 用 `IRONFORGE_PACK_THREADS=1` 生成的 pack 与
默认设置生成的 pack 各自 `git index-pack` 进独立仓库，比对
`git cat-file --batch-all-objects --batch-check` 的 `(oid, type, size)` 列表，完全相等。

## 7. 性能

同机、同数据集、同 profile（dev）、同一 git 客户端，端到端 wall time：

| 场景 | 改动前 | 改动后 | 变化 |
|---|---|---|---|
| large clone | 78.5 s | **64.2 s** | −18% |
| medium clone | 6.06 s | **5.05 s** | −17% |
| large ×3 并发 | 148.1 s | **128.0 s** | −14% |

变快的原因很直接：不再有 1.21 GB 的 Vec 反复扩容拷贝，也不需要「攒完再发」，客户端从
第一个 chunk 就开始收。

ops 层（`pack_for_wants_into` → `io::sink()`，large）：**39.2 s**，T37 报告的同类数字
是 38.5 s（不同 harness，`perf_baseline` 会把数据集一并算进去），差 +1.8%，在噪声内。

## 8. 正确性

1. **真实 git clone large + `git fsck --strict` 通过** — 单次、并发 3 次、before/after
   四次 clone 的 HEAD 都是 `e631f09b8c9853d3d688accbba51459c68c57244`，fsck 全绿。
2. **流式与旧 Vec 路径产出的对象集合一致** — `crates/rg-git/tests/pack_streaming.rs`：
   两条路径各产一个 pack，分别 `git index-pack` 进独立仓库，比较
   `git cat-file --batch-all-objects --batch-check` 的 `(oid, type, size)` 排序列表，
   全量 / 增量 / filter / universe 四个场景全部相等。按任务卡要求**不比 pack 字节**。
3. **增量 clone（带 haves）** — 单测（§8.2 的 incremental 场景）+ 端到端：medium 仓库
   先 clone，服务端加一个 commit 再 `git fetch`，客户端拿到新 tip，fsck 通过，文件内容
   正确；服务端日志 `Sent V2 fetch packfile pack_size=3492 wants=1 haves=16`
   （全量那次是 61 470 180 字节），说明 T37 的 haves 剪枝在流式下仍然生效。
4. **空 want（`pack_universe` 路径）** — 单测 `streamed_universe_pack_has_the_same_objects`
   比对对象集合；V1 的 `wants.is_empty()` 分支调用的就是这个 `_into`。
5. **shallow clone** — `git clone --depth 1` 成功，`rev-parse --is-shallow-repository`
   为 true，`rev-list --count HEAD` = 1，`git fsck --strict` 通过。
   另外验了 partial clone：`git clone --filter=blob:none --no-checkout`，checkout 时懒
   加载 blob（走 `want <blob>` 的 `plain_inputs` 路径），内容正确、`git fsck` 退出码 0。

另外：`streamed_pack_reports_a_failing_writer` 保证写端失败（客户端断流）会作为错误
冒上来，而不是静默截断；`upload_pack.rs` 内 3 个单测覆盖分帧等价、非 sideband 原样透传、
生成器失败上抛。

6. **线程预算不改变内容**：`tests/pack_threads.rs` 用 `IRONFORGE_PACK_THREADS=1` 与默认
   设置各产一份 pack，对象集合（oid/type/size 排序列表）完全相等；`ops.rs` 内 3 个单测
   覆盖 `0` / 空值 / 非法值回落到默认（不会被当成 1 线程）。

## 9. 未做项

- **receive-pack 流式化**：不做，代码里写了原因（§3.4）。
- **把 `"packfile"` section 头推迟到第一个 entry 编码成功之后**：可以让「want 不可解析」
  这类错误重新变成 HTTP 500，但需要改协议写出顺序（还会牵涉 stateless RPC 的
  `ResponseEnd`），属于协议层改动。
- **pack 线程数的默认值**：`IRONFORGE_PACK_THREADS` 已经可用（§6.6），但**默认仍是
  「全部核心」**，没有替运维做选择。把 8 线程当默认值需要另一次有意识的决定。
- **并发 pack 数量上限 / 限流**：属于运维层（任务卡明确划到范围外），未做。
- **`counts` 本身的内存**（321 106 个 `Count` = 17.2 MiB）：不是瓶颈（§6.4）。
- **`pack_universe` 走磁盘的优化**（`fork_repo`）：非协议路径，未动。

## 9.1 已知限制（记录，不修）

- **gix 的 pack 错误粒度太粗**：pack 生成中途失败时，`anyhow` 链到
  `failed to write pack entry: Pack entry input iterator failed` 就断了，看不出是**哪个
  oid** 读失败（§4.1 的端到端证据）。要修得包住 gix 的 error 并附上当前 entry 的 oid，
  属于给上游打补丁，超出本任务范围。运维含义：遇到这条日志时，需要靠 `git fsck` 或
  对象库层面的检查来定位具体损坏对象。

## 10. 复现

```bash
# 数据集（一次，large 约 6 分钟 / 2.1 GiB）
IRONFORGE_PERF_KEEP_DIR=/tmp/t38-large IRONFORGE_PERF_DATASET=large \
  cargo test -p rg-git --test perf_baseline -- --ignored --nocapture perf_materialize_dataset

# 可选：内存优先档（§6.6）
IRONFORGE_PACK_THREADS=8 ironforge serve …

# 服务 + 真客户端 + RSS 采样：见 §6.1 的脚本（`PROTO=1` 走 V1），核心是
#   ironforge serve --repo-root … --db-url sqlite://… --jwt-secret …
#   while :; do ps -o rss= -p "$PID"; sleep 0.05; done | sort -n | tail -1
#   git clone http://…/owner/repo.git

cargo test -p rg-git        # 含 pack_streaming / pack_threads / 分帧单测
cargo test -p rg-http       # 含 V1/V2 端到端 clone
cargo test -p rg-ci
```
