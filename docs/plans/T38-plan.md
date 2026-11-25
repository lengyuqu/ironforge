# T38 执行计划 — pack 流式化

> 权威约束见 `tasks/T38.md`。基线 `3dfdcd11`（main），分支 `agent/t38-pack-streaming`，
> worktree `../ironforge-t38`。
>
> 本文件记录：改了什么、为什么这么改、RSS 前后实测（含方法）、协商期失败点排查、
> 超时语义变化、正确性证据、**没达到的验收线及其原因**、未做项。

## 1. 一句话

一次 large（20 000 commits / 200 000 files / 1.8 GiB）clone 的服务端峰值 RSS
**2 521 MiB → 299 MiB**，并发 3 个 **6 536 MiB → 782 MiB**，clone 耗时 78.5 s → 64.2 s。
剩下的大头不是我们的缓冲，而是 gix 并行 entry 流水线的工作集（§6 有分阶段实测）。

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
失败」的是对象库读失败（磁盘/损坏），这属于任务卡接受的那一类。残留的粗糙之处是：
want 不可解析这类**协议错误**发生时，`"packfile"` 头已经写出去了（旧代码也是这样，只是
旧代码能把 500 发出来）。要彻底改善，得把 section 头的写出推迟到「第一个 entry 成功
编码」之后，那是协议层改动，不在本任务范围（见 §9 未做项）。

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

### 6.2 单次 clone

| 场景 | 改动前 | 改动后 | 变化 |
|---|---|---|---|
| large 全量 clone 峰值 | **2 521 MiB** | **299 MiB** | −88% |
| large 全量 clone 空闲基线 | 84 MiB | 84 MiB | — |
| medium 全量 clone 峰值 | 262 MiB | **161 MiB** | −39% |
| medium 空闲基线 | 83 MiB | 84 MiB | — |
| RFC：只改 rg-http（未改 v2.rs）时的 large | — | 1 438 MiB | 说明第二个 Vec ≈ 1.08 GB |

数据集来自 `perf_fixture`（确定性）：medium tip `370369f6…`，large tip `e631f09b…`。

### 6.3 并发 3 个 large clone

| | 峰值 RSS | 三个 clone 的 wall time |
|---|---|---|
| 改动前 | **6 536 MiB** | 148.1 s（最快 146.7 s） |
| 改动后 | **782 MiB** | 128.0 s（最慢 127.9 s） |

三个 clone 全部 `git fsck --strict` 通过，HEAD 均为 `e631f09b…`。

### 6.4 没到 200 MB / 400 MB —— 剩下的内存在 gix 里

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
| 8 | 123 MiB | 56.6 s |
| 16（默认） | 208 MiB | 39.2 s |

`chunk_size` 试过 1 / 2 / 10 / 40，峰值 208 / 228 / 208 / 213 MiB —— 无关
（`gix_parallel::optimize_chunk_size_and_thread_limit` 会按 items/threads 重算它）。
结论：**剩下的 200 MiB 是 gix 并行 entry 流水线的工作集**（生产端 16 路，消费端单线程，
`InOrderIter` 里在飞的 chunk），不是我们的缓冲。

代价对比，所以**没有**为了 RSS 数字去砍并行度：

- 要 single clone < 200 MiB（总 RSS）→ 需要 `thread_limit` ≤ 4 → large clone
  **95.2 s**，比 T37 基线 38.5 s 慢 **147%**，远破「不劣化 > 20%」。
- 要并发 3 个 < 400 MiB → 每 clone ≤ 约 105 MiB → `thread_limit` ≤ 3 → 更慢。

两者与性能线直接冲突，任务卡没有预料到 gix 自身的工作集这么大。本任务的选择是：
**保留默认并行度（零性能代价，RSS 降 8.4×），把这条实测证据交上去**，由 team-lead 决定
是否接受 299 MiB，或另开任务做「pack 线程数上限 + 内存优先级」的策略。

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

## 9. 未做项

- **receive-pack 流式化**：不做，代码里写了原因（§3.4）。
- **把 `"packfile"` section 头推迟到第一个 entry 编码成功之后**：可以让「want 不可解析」
  这类错误重新变成 HTTP 500，但需要改协议写出顺序（还会牵涉 stateless RPC 的
  `ResponseEnd`），属于协议层改动。
- **pack 线程数上限 / 内存优先级**：§6.4 的数据说明这是唯一能把 RSS 再压下去的手段，
  但它与「不劣化 > 20%」冲突，且属于运维策略（任务卡也把「并发 pack 数量上限/限流」
  划到范围外）。
- **`counts` 本身的内存**（321 106 个 `Count`）：不是瓶颈（§6.4 第一个阶段 74 MiB，
  含 odb 与遍历状态）。
- **`pack_universe` 走磁盘的优化**（`fork_repo`）：非协议路径，未动。

## 10. 复现

```bash
# 数据集（一次，large 约 6 分钟 / 2.1 GiB）
IRONFORGE_PERF_KEEP_DIR=/tmp/t38-large IRONFORGE_PERF_DATASET=large \
  cargo test -p rg-git --test perf_baseline -- --ignored --nocapture perf_materialize_dataset

# 服务 + 真客户端 + RSS 采样：见 §6.1 的脚本，核心是
#   ironforge serve --repo-root … --db-url sqlite://… --jwt-secret …
#   while :; do ps -o rss= -p "$PID"; sleep 0.05; done | sort -n | tail -1
#   git clone http://…/owner/repo.git

cargo test -p rg-git        # 含 pack_streaming / 分帧单测
cargo test -p rg-http       # 含 V1/V2 端到端 clone
cargo test -p rg-ci
```
