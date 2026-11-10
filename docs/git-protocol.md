# Git 协议实现细节

> 本文档记录 IronForge 实现 Git Smart Protocol（V1 与 V2）过程中的关键技术细节、协议规范和踩坑经验。
> 适合需要维护或扩展 `rg-git` crate 的开发者阅读。

---

## 目录

1. [pkt-line 协议格式](#1-pkt-line-协议格式)
2. [Sideband-64k 多路复用](#2-sideband-64k-多路复用)
3. [git-upload-pack（clone/fetch）](#3-git-upload-packclonefetch)
4. [git-receive-pack（push）](#4-git-receive-packpush)
5. [SSH 传输层（russh）](#5-ssh-传输层russh)
6. [HTTP 传输层（Axum）](#6-http-传输层axum)
7. [踩坑记录](#7-踩坑记录)
8. [Git Smart Protocol V2](#8-git-smart-protocol-v2)

---

## 1. pkt-line 协议格式

### 格式

```
<4 hex digits of total length><payload>
```

- 4 字节长度头用十六进制表示，**包含自身 4 字节**
- 理论 payload 上限为 `0xffff - 4 = 65531` 字节
- 实现限制到 **65516 字节**（`pkt_line::MAX_PKT_LINE_LEN`，`crates/rg-git/src/pkt_line.rs:12`）；超长会直接 `bail!`，不截断
- `0000` 是 flush packet，表示当前"阶段"结束

### 示例

```
# "hello\n"（6字节 payload）→ 总长 10
000ahello\n

# Flush packet
0000

# Delimiter packet (V2)
0001

# Response-End packet (V2)
0002
```

### 特殊 Packet（V2）

Protocol V2 引入了两种新的特殊 packet：

| Packet | 十六进制 | 说明 |
|--------|----------|------|
| Flush | `0000` | 结束当前阶段（与 V1 相同） |
| Delimiter | `0001` | 分隔消息的不同部分（V2 新增） |
| Response-End | `0002` | 无状态连接中响应的结束（V2 新增） |

### 实现位置

`crates/rg-git/src/pkt_line.rs`

`PktLine` 枚举的四个变体与上表一一对应：`Data(Vec<u8>)` / `Flush` / `Delim` / `ResponseEnd`。

关键函数（均为自由函数，泛型 `AsyncRead`/`AsyncWrite`，不限定 `BufReader`）：
- `read_pkt_line<R: AsyncRead + Unpin>(reader: &mut R)` — 读一个 pkt-line
- `read_pkt_lines_until_flush` — 读到 flush 为止
- `write_pkt_line(writer, pkt)` — 写一个 pkt-line
- `write_flush(writer)` — 写 `0000`
- `PktLine::text(&str)` — 构造文本包，**缺 `\n` 时自动补**（长度按补后的字节数算）

### ⚠️ 常见错误

**绝对不能用 `read_line()` 读 pkt-line。**

`BufReader::read_line()` 会把 `004ahello\n` 中的 `004a` 当成文本内容一起读出来。当遇到 packfile 的二进制数据时，会产生 UTF-8 解析错误：`stream did not contain valid UTF-8`。

正确做法：
```rust
let mut reader = BufReader::new(stream);
loop {
    match read_pkt_line(&mut reader).await? {
        PktLine::Flush => break,
        PktLine::Data(bytes) => { /* ... */ }
    }
}
```

---

## 2. Sideband-64k 多路复用

### 概念

sideband-64k 允许服务端在同一个连接上发送多种类型的数据：

| Band | 说明 |
|------|------|
| Band 1 (`\x01`) | 主数据流（packfile 或 report-status） |
| Band 2 (`\x02`) | 进度消息（显示在客户端 stderr） |
| Band 3 (`\x03`) | 错误消息（fatal error） |

### 格式

```
<pkt-line 包含 1字节 band prefix + payload>
```

例如，发送 10 字节的 band-1 数据：

```
# total = 4(header) + 1(band) + 10(data) = 15 = 0x0f
000f\x01<10 bytes data>
```

### Flush 语义

sideband flush (`0000`) 表示整个 sideband 流结束。客户端收到后停止读取 sideband 数据。

**谁负责 flush**：`write_pkt_line` 与 `write_flush` 都**不**调用 `flush()`，写下去就返回（`pkt_line.rs:59`、`:88`）。只有 `write_sideband_data` 在末尾显式 `writer.flush()`（`sideband.rs:25`）；`write_sideband_progress` / `write_sideband_error` / `write_sideband_flush` 都不 flush（`sideband.rs:30`、`:41`、`:52`）。因此调用方在收尾处需自己 flush 一次，否则最后一帧可能停在缓冲区里。

### 实现位置

`crates/rg-git/src/sideband.rs`

关键函数：
- `write_sideband_data(writer, data)` — 发 band-1 数据（按 `SIDEBAND_MAX = 65515` 自动分块，收尾 flush）
- `write_sideband_progress(writer, message)` — 发 band-2 进度
- `write_sideband_error(writer, message)` — 发 band-3 错误
- `write_sideband_flush(writer)` — 发 sideband flush `0000`

---

## 3. git-upload-pack（clone/fetch）

### 协议流程（Smart Protocol V1）

```
Client                          Server
  |                               |
  |  GET /info/refs?service=      |
  |  git-upload-pack              |
  |------------------------------>|
  |                               |
  |  <service header pkt-line>    |
  |  <ref advertisement>          |
  |  <flush>                      |
  |<------------------------------|
  |                               |
  | POST /git-upload-pack         |
  | want <sha1>\0<capabilities>   |
  | want <sha2>                   |
  | <flush>                       |
  | done                          |
  |------------------------------>|
  |                               |
  | NAK                           |
  | <packfile in sideband-64k>    |
  | <sideband flush>              |
  |<------------------------------|
```

SSH 模式省略 `GET /info/refs` 步骤，直接通过 exec_request 建立双向 stream。

**协商后的输入侧**（文档以前没写，但影响正确性判断）：

- `done` 或 flush 之后**不再排空 stdin**（`upload_pack.rs:46` 读完 want/have 直接发 NAK + pack）。push 侧相反：没有可应用的更新时会 `drain_pack()`（`receive_pack.rs:462`、`:620`）。
- `read_pkt_line` 把 `UnexpectedEof` **折叠成 `PktLine::Flush`**（`pkt_line.rs:108-111`）。两者叠加的后果是：客户端半途断开在服务端看来等于「正常结束协商」，`wants` 为空时直接写一个 flush 就 `Ok(())`（`upload_pack.rs:48-51`），不会报错。排查「客户端没收到 pack 但服务端日志无异常」时要先想到这条。
- **V1 没有 shallow 语义**：want/have 循环只认 `want` / `have` / `done`，其余行一律当未知行丢弃（`upload_pack.rs:246` 的 `Unknown want/have line, ignoring`）；整个文件对 `shallow` / `deepen` 没有任何解析。因此 **V1 即使收到 `deepen` 也只会全量打包**。真实客户端会不会在 V1 上发 `deepen` 未单独验证：e2e 的 `--depth=2` 用例固定用 V2（`scripts/git-protocol-e2e.sh:280-282`），V1 + depth 的组合不在覆盖内。

### 引用广告格式

```
<pkt-line: "sha1 refname\0caps\n">  ← 第一行附带 capabilities
<pkt-line: "sha2 refs/heads/main\n">
...
<flush: 0000>
```

空仓库分支会发一行伪 ref：`0000000000000000000000000000000000000000 capabilities^{}<service>\0<caps>`（`upload_pack.rs:332-335`）。git 只识别名字**恰好**是 `capabilities^{}` 的伪 ref，本实现把服务名拼在了花括号后面，客户端因此可能收不到 capability 列表（低危：空仓库无对象可协商）。此点尚未在客户端侧实测，记为待验证。

### Capabilities（SSH 侧 `rg-git` 广告的）

```
side-band-64k ofs-delta agent=ironforge/0.1
```

> SSH 侧的协商只实现简单的 `NAK → packfile` 流程，因此**不广告** `multi_ack` / `multi_ack_detailed` / `no-done`（`crates/rg-git/src/protocol/upload_pack.rs:324` 有对应注释，`NAK` 在 53/91/135 行发送）。

### HTTP 与 SSH 的 capability 分叉（已知不一致，待拍板）

HTTP 的 V1 广告串（`crates/rg-http/src/lib.rs:2638`）实际是：

```
multi_ack_detailed no-done side-band-64k thin-pack ofs-delta agent=ironforge/0.1
```

逐项对照（分三档：空头 / 广告了但主动降级 / 两侧一致）：

| capability | SSH V1 | HTTP V1 | 服务端行为 |
|---|---|---|---|
| `side-band-64k` | 广告 | 广告 | ✅ 两侧都走 band-1 |
| `ofs-delta` | 广告 | 广告 | ✅ 写出端会产 OFS_DELTA：`DeltaRef` 直接映射成 `OfsDelta` 头（gix-pack `data/output/entry/mod.rs:185`）。`allow_thin_pack: false` 只影响「base 不在本 pack 里」的 delta，不关掉 OFS_DELTA |
| `thin-pack` | 不广告 | **广告** | ⚠️ **降级而非空头**：客户端据此会请求 thin pack，服务端永远给非 thin pack（`ops.rs:833`）。非 thin 是合法 pack，能收，只是丢掉了客户端想要的带宽优化 |
| `multi_ack_detailed` | 不广告 | **广告** | ❌ 空头：只有单发 `NAK`（`upload_pack.rs:54`），没有任何 ACK 分支 |
| `no-done` | 不广告 | **广告** | ❌ 空头：只在 `multi_ack_detailed` 同时协商时才生效；本实现收首个输入包就回一个裸 `NAK`，此后不再发协商包。客户端会一直发 `have` 直到等到 `ACK … ready`，等不到（时序为源码推导，未实测复现） |

即：HTTP 的 V1 串里混了两个空头（`multi_ack_detailed`、`no-done`）和一个「广告了但主动降级」（`thin-pack`）。SSH 侧三个都不广告。客户端默认走 V2 不经过这条路径，`protocol.version=0/1` 的 HTTP 客户端才会命中。收缩广告串还是补实现，属协议决策（L2），不在文档侧决定。

### want/have 行：Form A 是协议形态，Form B 是宽松解析

**Form A**（协议形态，capability 只出现在**第一个** want 行、`\0` 之后）：
```
want <sha1>\0side-band-64k ofs-delta\n
```

**Form B**（首行用空格而非 `\0` 分隔）：
```
want <sha1> side-band-64k ofs-delta\n
```

Form B **不是**协议允许的第二种形态，是解析器对畸形输入的容忍：`upload_pack.rs:214-221` 对任何 `want ` 行都做 `split_once(' ')`，所以第 2、3 行带上空格内容同样会被当作 capability 吃下去。真实 git 的 `send_fetch_request` 对 V0 固定发 `<want>\0<caps>`，Form B 更可能来自旧版 JGit / libgit2（来源未经取证，勿当作事实引用）。

解析顺序：先看是否含 `\0`（有则 Form A），否则 `strip_prefix("want ")` 后 `split_once(' ')`——**不是**「检查第 46 位」。

### Pack 生成

已 gix 化（`rg_git::ops`）：
- **全量打包**（upload-pack V0）：`pack_universe` — refs 枚举 → `rev_walk` 全提交
  → `count::objects(TreeContents)` 展开 → `iter_from_counts` + `FromEntriesIter` 写 V2 pack
- **增量打包**（fetch 协商）：`pack_for_wants` — wants/haves 预解析（tag 剥离），
  haves 作为 `with_hidden` 排除；shallow 请求以边界提交替代 haves 并用
  `TreeContents` 补齐边界树内容，等价 `--shallow <sha>` revs 语义
- partial-clone：**V1 不协商 filter**（`upload_pack.rs:373` 注释「V1 has no shallow/partial-clone negotiation here」，shallow/partial 都按空处理）；**V2 支持**——广告串含 `fetch=shallow filter`（`v2.rs:48`），解析 `filter <spec>` 后交给 `ops::pack_for_wants_filtered`（`v2.rs:546`、`ops.rs:952`）
- **懒取回（promisor）**：`filter blob:none` 克隆之后，客户端补对象时直接 `want <blob-sha>` / `want <tree-sha>`。这类 want **不走进历史遍历**，按 `extra_inputs` 原样打包（`ops.rs:949-950` 的函数注释），否则「want 一个 blob」会被当成想拉全部历史。单测断言 pack 里恰好 1 个对象（`ops.rs:1816` 的 `lazy fetch under {spec}`），e2e 在真客户端上断言 trace 出现 `want <blob>`（`scripts/git-protocol-e2e.sh:341-346`）

---

## 4. git-receive-pack（push）

### 协议流程

```
Client                          Server
  |                               |
  | <ref advertisement>           |
  | <flush>                       |
  |<------------------------------|
  |                               |
  | old_sha new_sha refname\0caps | ← update commands
  | old_sha new_sha refname       |
  | <flush>                       |
  | <packfile (raw bytes)>        |
  |------------------------------>|
  |                               |
  | <sideband band-1:             |
  |   report-status pkt-lines>    |
  | <sideband flush>              |
  |<------------------------------|
```

### Update Command 格式

```
<old_sha> <new_sha> <refname>\0<capabilities>\n   ← 第一条包含 capabilities
<old_sha> <new_sha> <refname>\n                   ← 后续条目
<flush: 0000>
<packfile binary data>
```

特殊 SHA：
- `old_sha` 全为 0：创建新 ref
- `new_sha` 全为 0：删除 ref。**只允许删 `refs/heads/*`**，tag 等其它命名空间一律 `ng ... only branches can be deleted`（`receive_pack.rs:389`）
- 删除不带 pack 数据，命令本身直接按 `ok` 入队，真正的策略判定在 apply 阶段由调用方传入的 `BranchDeleter` 守卫完成（默认分支、保护规则、开放 PR、token 分支上限，与 `DELETE /repos/{o}/{n}/branches/{x}` 同一套检查），见 `receive_pack.rs:16-20`

### Thin Pack 处理

客户端发送的是 **thin pack**（只包含 delta base 不一定在 pack 里的对象）。
已 gix 化（`ops::index_pack_bytes`）：pack 字节缓冲后经
`gix_pack Bundle::write_to_directory` 摄取，thin delta base 从目标仓库对象库
lookup 补全（等价 `--fix-thin`），pack + idx 落盘 `objects/pack` 后移除 `.keep`。

> **方向要分清**：`--fix-thin` 只在**收**的一侧有意义。我们**发**出去的 pack 从不含 thin delta——`ops.rs:833` 的 `iter_from_counts` 选项写死 `allow_thin_pack: false`；补全发生在 `index_pack_bytes` 调用 `Bundle::write_to_directory` 时传入 `Some(repo.objects.clone())` 作为缺失 base 的查询源（`ops.rs:1056`）。
>
> 注意 **thin delta ≠ OFS_DELTA**：`allow_thin_pack: false` 关掉的是「base 不在本 pack 里」的 delta，OFS_DELTA（base 在同 pack 内、用偏移量表示）照常产出——`DeltaRef` 直接映射成 `OfsDelta` 头（gix-pack `data/output/entry/mod.rs:185`）。这也是 §3 里 `ofs-delta` 判 ✅ 的依据。

### Report-Status 响应格式（关键！）

这是最容易出错的地方。**经过 `GIT_TRACE_PACKET=1` 对真实 git-receive-pack 抓包验证**的正确格式：

```
# 一个 sideband band-1 pkt-line，payload 是 report-status pkt-lines 序列
<pkt-line: \x01 + "000eunpack ok\n" + "0017ok refs/heads/main\n" + "0000">
# sideband flush
0000
```

错误的做法（**不要这样做**）：
```
# ❌ 先发 sideband flush，再发 plain pkt-lines
0000                    ← 客户端以为 sideband 结束了
000eunpack ok\n         ← 客户端不会读这里！
...
```

正确的实现（见 `receive_pack.rs` 的 `send_response()`）：

```rust
// 1. 把 report-status 序列写入内存 buf
let mut report_buf: Vec<u8> = Vec::new();
write_pkt_line(&mut report_buf, &PktLine::text("unpack ok")).await?;
for result in results {
    if result.status == "ok" {
        write_pkt_line(&mut report_buf, &PktLine::text(&format!("ok {}", result.refname))).await?;
    } else {
        write_pkt_line(&mut report_buf, &PktLine::text(&format!("ng {} {}", result.refname, result.message))).await?;
    }
}
write_flush(&mut report_buf).await?;

// 2. 整体作为 band-1 sideband 发出
sideband::write_sideband_data(writer, &report_buf).await?;

// 3. sideband flush 结束整个 sideband 流
sideband::write_sideband_flush(writer).await?;
```

### Capabilities（SSH 与 HTTP 一致）

```
report-status report-status-v2 delete-refs side-band-64k agent=ironforge/0.1
```

> `delete-refs` 缺了就 `git push --delete` 会被客户端拒绝在本地（`receive_pack.rs:279` 有注释；HTTP 侧 `crates/rg-http/src/lib.rs:2642` 同串）。

---

## 5. SSH 传输层（russh）

### 会话生命周期

```
russh::server::Server::new_client()  → 创建 SshHandler
SshHandler::channel_open_session()   → 存储 Channel
SshHandler::exec_request()           → 解析 git 命令，tokio::spawn 处理
  └── handle_upload_pack_stream() 或 handle_receive_pack_stream()
    └── exit_status_request()        → 发退出码
    └── stream.shutdown()            → 发 SSH EOF
    └── stream drop                  → channel close
```

### exit_status 和 stream.shutdown() 的顺序

**必须严格按以下顺序执行**：

```rust
// ① 先发 exit-status（此时 channel 还活着，可以接受请求）
handle.exit_status_request(channel_id, exit_code).await?;

// ② 再 shutdown stream（发 SSH EOF）
// 这确保所有写入 stream 的数据都已发送到客户端
stream.shutdown().await?;

// ③ stream drop → channel close（自动发生）
```

如果在 `shutdown()` 之前不发 `exit_status`，客户端可能在等待 exit code 时被阻塞。
如果不调用 `shutdown()`，russh 内部缓冲区中的数据可能在 stream drop 时丢失。

### exec_request 的命令格式

git 客户端发送：
```
git-upload-pack '/owner/repo'
git-receive-pack '/owner/repo.git'
```

处理规则：
1. 按第一个空格分割：service / path
2. 去除 path 的首尾引号（单引号或双引号）
3. 去除 path 开头的 `/`
4. 先查完整路径，再尝试加 `.git` 后缀

### ChannelStream 行为注意事项

russh 的 `ChannelStream` 实现了 `AsyncRead + AsyncWrite`，但有几个特殊行为：

1. **每次 `write_all` 调用立即发出 SSH channel data packet**（没有内部 buffer），所以小碎片写入会产生很多小 packet。这是正常的，russh 内部会合并或分帧。

2. **`flush()` 是 no-op**（在 `ChannelStream` 上没有实际效果）。数据可靠性由 `shutdown()` 保证。

3. **在 `BufReader` 中包装 stream 读 pkt-line 时**，BufReader 内部预读缓冲必须在 `process_push` 返回前 drop，否则 stream 无法继续用于写响应：

```rust
// ✅ 正确：用块限制 BufReader 生命周期
let results = {
    let mut reader = BufReader::new(&mut *stream);
    process_push(repo_path, &mut reader).await?
};  // BufReader drop 在这里
send_response(stream, &results).await?;
```

---

## 6. HTTP 传输层（Axum）

### 路由结构

同一组 Git Smart HTTP 路由注册了**两遍**（`crates/rg-http/src/lib.rs:441` 根级、`:581` 的 `build_routes` 内）：

```
# ① 根级：git 客户端的标准形式，必须有
GET  /{owner}/{repo}/info/refs
POST /{owner}/{repo}/git-upload-pack
POST /{owner}/{repo}/git-receive-pack

# ② /git 前缀：兼容别名，由 .nest("/git", git_routes) 挂上
GET  /git/{owner}/{repo}/info/refs
POST /git/{owner}/{repo}/git-upload-pack
POST /git/{owner}/{repo}/git-receive-pack

GET  /health
```

> 两个前缀都能用，但**根级才是 git 客户端实际请求的形式**（`/{owner}/{repo}.git/info/refs`，`.git` 后缀由 handler 剥离）。只挂 `/git` 会让客户端拿到 SPA fallback 的 HTML，报 `fatal: not a valid ref`。`scripts/git-protocol-e2e.sh:103` 用的是带 `/git` 前缀的别名。

### Pipe 桥接模式

HTTP 请求体（`Bytes`）是同步的，但 `handle_upload_pack_http` 等函数期望 `AsyncRead`。
用 `tokio::io::duplex` 管道桥接：

```rust
// 把 request body 写入 pipe
let (pipe_read, mut pipe_write) = tokio::io::duplex(body.len() + 1024);
tokio::spawn(async move {
    let _ = pipe_write.write_all(&body).await;
});

// 把 handler 输出写入另一个 pipe，再读回来作为 response body
let (mut buf_reader, mut buf_writer) = tokio::io::duplex(64 * 1024);
handle_upload_pack_http(&repo_path, pipe_read, &mut buf_writer).await?;
buf_writer.flush().await?;
drop(buf_writer);  // ← 必须 drop 才能让 read_to_end 结束！
let mut output = Vec::new();
buf_reader.read_to_end(&mut output).await?;
```

### info/refs 响应格式

info/refs 的响应**不完全等同于** SSH 的 ref advertisement，需要额外包一层：

```
# Service header（一个 pkt-line）
<pkt-line: "# service=git-upload-pack\n">
# Flush
0000
# 然后是 ref advertisement（同 SSH）
<ref advertisement pkt-lines>
0000
```

Content-Type 响应头必须正确设置（见 CLAUDE.md 踩坑 #6）。

---

## 7. 踩坑记录

以下是 Phase 1 开发过程中实际踩过的坑，**每个都花费了较长时间排查**。

### 坑 1：用 read_line 读 pkt-line 导致 UTF-8 错误

**现象**：`stream did not contain valid UTF-8`  
**原因**：`BufReader::read_line()` 把 pkt-line 的 4 字节长度头（如 `004a`）当成文本内容读了进来，碰到 packfile 二进制数据更是直接崩溃。  
**修复**：改用 `read_pkt_line()`，正确解析 4 字节长度头后只返回 payload。

### 坑 2：receive-pack 响应 `bad band #110`

**现象**：`error: remote unpack failed: bad band #110`  
**原因**：`#110` 是 `0x6e` 即字母 `n`，来自 "unpack ok" 的第一个字节。服务端把 `unpack ok` 作为 plain pkt-line 发送，但客户端在 sideband 模式下把 `u` (`0x75`) 当成 band number 读取。  
**修复**：把 report-status 整体用 sideband band-1 包装发送。

### 坑 3：receive-pack 响应 `bad line length character: unpa`

**现象**：`fatal: bad line length character: unpa`；或换一种客户端状态表现为 `fatal: the remote end hung up unexpectedly`。
**原因**：与坑 2 同源（report-status 没有整体走 band-1），只是错位的位置不同——先发 sideband flush `0000` 再发 plain pkt-lines 时，客户端已把 sideband 流判为结束：仍在读的路径把 `unpa` 当长度头（`bad line length character`），已停读的路径则表现为 hang up。两个报错不是两个 bug，具体报哪个取决于客户端读到错位字节的时机。
**修复**：report-status 必须在 sideband flush **之前**整体作为 band-1 发出（与坑 2 同一条修复；正确顺序见 §4 的 `send_response()`）。

### 坑 4：SSH 数据丢失（russh stream.shutdown() 时机）

**现象**：服务端日志显示 "Receive-pack response sent"，但客户端只收到 sideband flush `0000`，后续数据丢失。  
**原因**：stream 在 task 结束时被 drop，但 russh 可能在 channel close 时丢弃尚未发出的缓冲数据。  
**修复**：在 stream drop 之前显式调用 `stream.shutdown().await`，确保所有数据都被发出后再发 SSH EOF。

### 坑 5：thin pack 导致 index-pack 失败

**现象**：`git index-pack failed: error: pack has X unresolved deltas`  
**原因**：客户端发送 thin pack（delta base 可能是已有对象，不在 pack 中），不加 `--fix-thin` 无法解析。  
**修复**：CLI 时代使用 `git index-pack --fix-thin --stdin`；gix 迁移后由 `ops::index_pack_bytes`（`gix::odb::pack::Bundle::write_to_directory`）等价完成——thin pack 的缺失 base 直接向目标 ODB 查找补全。

### 坑 6：空 repo 的 git rev-parse HEAD 返回字面 "HEAD"

**现象**：在空的 bare repo 中 `git rev-parse HEAD` 不返回错误码，而是输出字面字符串 `"HEAD"`。  
**原因**：`HEAD` 指向 `refs/heads/main`，但 main 分支还不存在。rev-parse 返回成功状态码但输出未解析的符号引用。  
**修复**：解析结果后需校验是否为 40 位 hex SHA，否则忽略：

```rust
if sha.len() == 40 && sha.chars().all(|c| c.is_ascii_hexdigit()) {
    Some(sha)
} else {
    None
}
```

### 坑 7：argon2 0.5 的 SaltString::generate 需要 CryptoRngCore

**现象**：`the trait bound ThreadRng: CryptoRngCore is not satisfied`  
**原因**：rand 0.9 的 `rng()` 返回 `ThreadRng`，不满足 `password_hash::rand_core::CryptoRngCore`。  
**修复**：

```rust
use password_hash::rand_core::OsRng;
let salt = SaltString::generate(&mut OsRng);
```

### 坑 8：Command::arg() 不允许 NUL 字节

**现象**：`nul byte found in provided data`  
**原因**：git update command 第一行格式是 `old_sha new_sha refname\0capabilities`，把整行传给 `Command::arg()` 会被 OS 拒绝（NUL 在 C 字符串中表示结束）。  
**修复**：在 `\0` 处分割，只取 refname 部分：

```rust
let clean_line = if line.contains('\0') {
    line.split('\0').next().unwrap_or(line)
} else {
    line
};
```

---

## 8. Git Smart Protocol V2

Protocol V2 是对 V1 的重大改进，提供了更清晰的命令架构和更好的性能。

### 与 V1 的主要区别

| 特性 | V1 | V2 |
|------|----|----|
| Ref 广告方式 | 首次请求全部推送 | 按需请求（ls-refs 命令） |
| 命令复用 | 无 | 支持多命令复用同一连接 |
| 协议格式 | 固定服务名 | 命令行参数化 |
| 无状态支持 | 差 | 原生支持 |
| 浅克隆 | 需额外协商 | 内置支持 |

### V2 协议流程

```
Client                          Server
  |                               |
  | GET /info/refs?service=       |
  |   Git-Protocol: version=2      |
  |------------------------------>|
  |                               |
  | version 2                     | ← 版本声明
  | ls-refs                       | ← 支持的能力
  | fetch=shallow                 |
  | object-format=sha1            |
  | 0000                          |
  |<------------------------------|
  |                               |
  | POST /git-upload-pack         |
  | command=ls-refs               | ← 命令请求
  | 0001                          | ← 分隔符
  | ref-prefix refs/heads/         |
  | peel                           |
  | 0000                           |
  |------------------------------>|
  |                               |
  | <ref advertisement>           |
  | 0000                           |
  |<------------------------------|
  |                               |
  | POST /git-upload-pack         |
  | command=fetch                  | ← fetch 命令
  | want <sha>                     |
  | have <sha>                    |
  | 0001                          |
  | done                          |
  | 0000                           |
  |------------------------------>|
  |                               |
  | ACK <sha>                      | ← 确认
  | <packfile in sideband>        |
  | 0000                           |
  |<------------------------------|
```

### HTTP 模式下的 V2 激活

客户端通过 HTTP 头激活 V2：

```http
GET /git/owner/repo/info/refs?service=git-upload-pack HTTP/1.1
Git-Protocol: version=2
```

服务端响应 V2 capability advertisement，然后客户端发送命令。

### Capability Advertisement 格式

```
<length>version 2\n
<length>agent=ironforge/0.1\n
<length>ls-refs\n
<length>fetch=shallow\n
<length>object-format=sha1\n
<length>server-option\n
0000
```

### V2 命令格式

**ls-refs 命令**：
```
command=ls-refs\n
0001
ref-prefix <prefix>\n
peel\n
symrefs\n
0000
```

**fetch 命令**：
```
command=fetch\n
want <sha>\n
have <sha>\n
0001
filter blob:none\n
deepen 10\n
done\n
0000
```

### 实现位置

`crates/rg-git/src/protocol/v2.rs`

关键函数：
- `handle_v2()` — HTTP 模式 V2 处理器
- `handle_v2_stream()` — SSH 模式 V2 处理器
- `send_capability_advertisement()` — 发送能力声明
- `read_command_request()` — 解析命令请求
- `handle_ls_refs()` — 处理 ls-refs 命令
- `handle_fetch()` — 处理 fetch 命令

### 踩坑记录

#### 坑 9：V2 需要处理 Delim/ResponseEnd packet

**现象**：`read_pkt_line()` 在收到 `0001` 或 `0002` 时返回新的枚举变体。  
**原因**：V2 引入了新的 packet 类型。  
**修复**：在所有 pkt-line 读取处处理 `PktLine::Delim` 和 `PktLine::ResponseEnd`。

---

## 参考资料

- [Git Protocol V2 Reference](https://git-scm.com/docs/protocol-v2)
- [Git Pack Protocol Reference](https://git-scm.com/docs/pack-protocol)
- [Git HTTP Backend](https://git-scm.com/docs/git-http-backend)
- [Git Smart HTTP Transfer Protocols](https://git-scm.com/docs/http-protocol)
- [russh 文档](https://docs.rs/russh/)
- [gitoxide (gix)](https://github.com/Byron/gitoxide) — 生产路径仅剩 1 处 CLI：mirror 同步的 `git clone --mirror`（首次）与 `git fetch --prune origin` + 两条显式 refspec（后续），经 `GitCommandGateway` 并带 transport lockdown，见 `crates/rg-core/src/mirror/service.rs:244`、`:327`。其余热路径已 100% gix 原生（含 rebase 与 pack 生成的 gix 自研实现），评估见 `gix-migration-assessment.md`
