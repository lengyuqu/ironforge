# OPT-LOOP 账本（IronForge）

> 协议来源：`/Users/yuqu/Documents/mybook/Projects/CNB-NPC-LOOP/README.md`（v0.3）。
> 本文件是这套闭环的**跨轮记忆**。规则：**纯追加**；标为「不成立」的条目禁止删除（否则同一错误会被无限重放）。
> 本文件不是产品文档，不参与 `docs/` 的漂移巡检范围。

## 轮次语义（照抄协议 §3 / §4.2，勿改）

- **issue = 容器**：`open` → 6 轮 → `close` + `BATCH_COMPLETE`
- **评论 = 进度**：以 `## 第 K 轮收尾` 开头的评论条数 `C` = 已完成轮数。单次运行被切断后，下次运行数评论即知从 `C+1` 续跑——进度可从平台侧完整重建，不依赖本地文件。
- **本文件 = 记忆**：「不成立」清单 + 批次索引 + 每轮判定与 SHA。
- **单发模式**（非 `[opt-round]` 容器的存量 issue）回帖首行用 `## 本轮收尾`，不带轮次号，不参与批次计数。

## IronForge 本地化参数

> 协议警告：§2.2 的延迟参数是 agent2ssh 单仓库实测（n=19，p100 20min46s），**不可直接沿用**。本仓库规模大一个量级（10 crate / 191K 行 Rust / 664 个 `.rs` / 20K 行文档），须自行标定。

| 参数 | 当前值 | 状态 |
|---|---|---|
| 轮询间隔 | 60 s | 沿用协议 |
| 等待上限 | 起始 60 s × 30 | 待标定（本批 n=1） |
| **首扫延迟（实测）** | **3 min 43 s**（`09:21:41Z → 09:25:24Z`，n=1）；复核 **4 min 22 s**（n=2）、**2 min 16 s**（n=3）、**5 min 20 s**（n=4）、**28 min 15 s**（n=5） | 与 agent2ssh 的 min 2:31 同量级；仓库大一个量级但首扫未变慢，因为 NPC 只读文件不构建。**n=5 破了 p100**：那轮它自己 `cargo build --release`（23 m 28 s）做实测复现。即「NPC 要不要构建」是延迟的唯一尾巴，不是排队 |
| 验证命令（文档轮） | `scripts/check.sh --quick` | **实测 115 s 冷 / 2 s 热**（文档写 ~40 s，是忽略 clippy 首次编译的口径） |
| 验证命令（代码轮） | `scripts/check.sh`（约 7 min） | 已确认存在 |
| 验证命令（收批） | `scripts/check.sh --full` | 全 workspace |
| 本机/本仓提交身份 | `cnb.biewen` | 实测（issue #11 作者 = 本机推送身份） |
| 已知 flaky | webhook 重试时序、`merge_queue_ci_tests`（环境） | 差分判据须 allowlist |

### 分级口径（本项目专用，覆盖协议 §4.4 的通用表）

| 级别 | 范围 | 动作 |
|---|---|---|
| **L1** | `docs/`、`ironforge-docs/`、`CLAUDE.md`/`AGENT.md`/`ARCHITECTURE.md`/`CONTRIBUTING.md`/`README.md` 的文档与注释；只增不减的断言强度；i18n 键值一致性 | 修 + 验证 + 提交 + 推 cnb |
| **L2** | DB 迁移顺序、Git 协议线格式（pkt-line / sideband / `--fix-thin`）、认证权限语义（LDAP/SSO/TOTP/JWT）、gix↔CLI 替换、OpenAPI 与前端契约、任何改 schema 的 | **不动 main**，写进待拍板清单 |
| **禁区** | `.cnb.yml`、`.github/`、`scripts/`、`Cargo.toml`/`Cargo.lock`、`deploy/` | 只报告 |

---

## 批次索引

### batch#1(issue #11) · 2026-11-10 · git 协议文档与实现一致性

- 开批：`2026-11-10T09:21:41Z`（issue 创建即触发，未额外交触发评论）
- 基线提交：`5624605`（cnb/main HEAD）
- 范围：`docs/git-protocol.md` + `crates/rg-git/src/{pkt_line,sideband,protocol/upload_pack,protocol/receive_pack,protocol/v2}.rs`（约 4300 行）
- 轮次上限：6（本批先跑 1 轮做标定）
- NPC 首条分析：**已到**，`2026-11-10T09:25:24Z`，6 条指摘 + 3 条推测，基线自述已核对 `5624605`
- NPC 第 2 轮复核：**已到**，`2026-11-10T09:35:33Z`（`@CodeBuddy` 触发成功，延迟 4 min 22 s）
- NPC 第 3 轮复核：**已到**，`2026-11-10T09:41:46Z`（延迟 2 min 16 s）
- NPC 第 4 轮复核：**已到**，`2026-11-10T09:51:47Z`（延迟 5 min 20 s）
- NPC 第 5 轮复核：**已到**，`2026-11-10T10:22:30Z`（延迟 28 min 15 s——它本轮自己 `cargo build --release` 花了 23 m 28 s，延迟首次突破 20 min 46 s 的 p100）
- 6 轮进度：5 / 6

### consume 1 / 6

- 判定：**成立 4 / 部分成立 1 / 不成立 1**（另有 1 条推测经代码核实后升级为待验证项）
- 动作：L1 文档 `91686a9`；L2 写入本文件「待拍板」
- 验证：`scripts/check.sh --quick` 通过（115 s）；`BASE=5624605`、`B=0`（文档改动，无 test binary 受影响）
- 回帖：`2120086860385902592`，`2026-11-10T09:31:11Z`，首行 `## 第 1 轮收尾`（回读校验通过）

逐条判定：

| # | NPC 指摘 | 判定 | 处置 |
|---|---|---|---|
| 1 | V1 upload-pack 未收到 NUL 时漏掉 connection 级 capability，导致裸 packfile 与 HTTP 广告矛盾 | **不成立** | 协议规定 capability 只能出现在**第一个** want 行；客户端若要 side-band-64k 必须在首行声明。未声明即等于不要 sideband，服务端裸发 packfile 是合规降级。另 `git-protocol-e2e.sh:176/187` 已覆盖 `protocol.version=1` 的 HTTP clone 与 fetch，若真会失败该用例不会绿 |
| 2 | §4 capability 清单漏 `delete-refs`，且 §4「删除暂不支持」过期 | **成立** | 已补 `delete-refs` 并改写删除语义（只允许 `refs/heads/*`，策略在 `BranchDeleter`） |
| 3 | §3 只在 SSH 成立，未标注 HTTP 分叉 | **成立** | 已标注为「已知不一致（待拍板）」，收缩广告还是补实现留 L2 |
| 4 | §1 `read_pkt_line(reader: &mut BufReader<R>)` 签名过期 | **成立** | 已改为泛型 `AsyncRead + Unpin`，并补 `read_pkt_lines_until_flush` / `PktLine::text` 自动补 `\n` |
| 5 | 坑 2 与坑 3 讲同一件事，坑 3 是噪音 | **部分成立** | 同源判断成立、坑 3「现象与原因自相矛盾」成立；但不采纳「删除」，改为写明同源 + 两个症状对应哪个错位时机 |
| 6 | 四处协议关键行为文档未写（EOF 折叠、不排空 stdin、谁 flush、thin pack 方向） | **成立** | 三处已补进 §1/§2/§3/§4；空仓库伪 ref 一条见下表 |

推测项处置（本轮只核实、不修）：

| NPC 推测 | 核实结果 |
|---|---|
| 空仓库广告 `capabilities^{}<service>` 名字可能不被 git 识别 | **代码层面成立**（`upload_pack.rs:332-335` 把 service 拼在 `capabilities^{}` 后面，git 只认名字恰为 `capabilities^{}` 的伪 ref）。已在 §3 记为「待验证」，未做客户端实测 |
| `read_to_end` 与 BufReader 预读边界 | 未核实，留待下一轮 |
| `v2.rs` `skip_until_flush` 不消费 delim 后参数段 | 未核实，留待下一轮 |

### consume 2 / 6

- 判定：**成立 2 / 部分成立 1 / 不成立 0**（含 1 条对上一轮自身结论的自我推翻）
- 动作：L1 文档（本轮提交）；L2 新增 4 项进待拍板
- 验证：`scripts/check.sh --quick` 通过（2 s，缓存热）；`BASE=fdcfb15`、`B=0`；纯文档，无 test binary 受影响
- 回帖：`2120088953280700416`，`2026-11-10T09:39:30Z`，首行 `## 第 2 轮收尾`（回读校验通过）

| # | NPC 第 2 轮指摘 | 判定 | 处置 |
|---|---|---|---|
| 1 | 我对第 1 条的驳回**理由**有两处错：Form B 不是协议形态而是宽松解析；e2e V1 用例只断言「不含 version 2」，走的是 Form A | **成立** | 结论不变（该输入不可达），但反向清单的理由已按此改写，见下「勘误」 |
| 2 | 第 3 条标注里我把广告串抄漏 `side-band-64k` 与 `agent=`，等于把「唯一必须一致的能力」漏了 | **成立（我错）** | 已改成逐项对照表；实际行号是 `:2638`，NPC 写的 `:2639` 偏一行 |
| 3 | V1 不解析 `shallow`，`--depth` 在 V1 上会静默全量 | **部分成立** | 代码事实成立（`upload_pack.rs:246` 丢弃未知行，全文无 shallow 解析）；但「静默全量」不成立——两侧广告串都不含 `shallow`，按协议客户端不会发起 `--depth`。已写进文档并把客户端行为标「未实测」 |
| 4 | 把 V2 的 `fetch=shallow filter` 与 HTTP V1 分叉并列，易被读成 V2 也是空头 | **成立** | 已拆成独立小节，filter 单列 |
| 5 | 第 2/4/6 条确认解决、第 5 条处置获接受 | — | 记录在案 |
| 6 | 跨轮观察：`--quick` 覆盖不到文档里的 `path:line` 引用 | **成立** | 用 shell 做了一次全量机械核对，24 条引用查出 5 处我方错误（3 处漏 `crates/` 前缀、2 处行号偏移），已全部修正。校验脚本属 `scripts/`（禁区），只报告不落地 |

### consume 3 / 6

- 判定：**成立 4 / 部分成立 2 / 不成立 1**
- 动作：L1 文档 `cac765a`；L2 无新增
- 验证：`BASE=21b7e50`、`B=0`；`check.sh --quick` 1 s（热）；32 条 `path:line` 重新解析
- 回帖：`2120090703664410624`，`2026-11-10T09:46:27Z`

| # | NPC 第 3 轮指摘 | 判定 | 处置 |
|---|---|---|---|
| 1 | `thin-pack` 应与两个 ❌ 分开，属「广告了但主动降级」 | 成立 | 表改三档 |
| 2 | `ofs-delta` 标 ✅ 站不住，该路径产不出 OFS_DELTA | **不成立** | 反证：gix-pack 0.77 `data/output/entry/mod.rs:185` 的 `DeltaRef → Header::OfsDelta`；`allow_thin_pack` 只约束跨 pack 引用。已补「thin delta ≠ OFS_DELTA」 |
| 3 | `no-done` 时序描述不准 | 部分成立 | 改为时序描述 + 标「源码推导，未复现」 |
| 4 | shallow 那段 `所以` 是因果混淆 | 成立 | 拆成「已证实 / 未验证」，并写明 e2e 只覆盖 V2 |
| 5 | 机械校验：`ops.rs:1055` / `sideband.rs:25` / `pkt_line.rs:59` 三处错 | 部分成立 1 / **不成立 2** | `ops.rs` 正确值是 **1056**（不是我原写的 1055，也不是它说的 1054）；`sideband.rs:25` 与 `pkt_line.rs:59` 我方引用正确，反诉成立 |
| 6 | Form B 段落未同步 + 「第 46 位」不实证 | 成立 | 重写该小节，删 macOS 归属 |
| 7 | 懒取回注释在 `:949-950` 而非 `:933-936` | 成立 | 已改 |

**方法论收获（写死在这里，别再踩）**：机械校验只能证明「那一行有内容」，不能证明「那一行是那句话说的东西」。两个方向都会出假阳性——本轮就是一边我引错了行（`1055`→`1056`，脚本放行），另一边 NPC 拿「那一行有代码」反推我的引用错（`sideband.rs:25`、`pkt_line.rs:59`，实际都对）。这类校验要配一句人工确认：**引用的行必须能读出命题本身**。

### consume 4 / 6

- 判定：**确认 2 / 不成立 1 / 部分成立 1**
- 动作：L1 文档 `6c9e1ce`；L2 无新增
- 验证：`BASE=2a11e2e`、`B=0`；`check.sh --quick` 2 s（热）；32 条 `path:line` 重新解析
- 回帖：`2120092667387224064`，`2026-11-10T09:54:15Z`

| # | NPC 第 4 轮指摘 | 判定 | 处置 |
|---|---|---|---|
| 1 | `ofs-delta` ✅ 成立，撤回 | 确认 | 采纳其补强：gix-pack `Mode` 只有 1 个变体、`mode` 唯一读点 `:97` 只决定排序；`allow_thin_pack` 形参真名是 `pack_offset_to_oid`（OID 解析器）。已写进 §4 |
| 2 | `sideband.rs:25` / `pkt_line.rs:59` 我方正确，撤回 | 确认 | 无改动 |
| 3 | V1 + `--depth` 会真实发 `deepen 2`，客户端把全量包当浅克隆 | **不成立** | 本机起实例实测：`fatal: Server does not support shallow clients`，trace 只有 ref 广告、无 want/deepen；V2 对照 `--depth=1` 成功。已按实测重写文档并加「加 `shallow` 进广告串会让本条失效」的警告 |
| 4 | 机械校验 3 条 | 部分成立 | `rg-http/src/lib.rs:441` → `:458-466`/`:454`/`:581`（已改）；`upload_pack.rs:332-335` 与 `mirror/service.rs:244` 引用恰当，不改 |

**实测配方（可复现，写下来免得下次重造）**：

```bash
# 1. 起实例
target/release/ironforge serve --repo-root /tmp/ift/repos --http-addr 127.0.0.1:18131 \
  --jwt-secret <any> --db-url "sqlite:////tmp/ift/t.db?mode=rwc" &
# 2. 注册 + 建仓 + 推 3 个提交（POST /api/v1/users/register → POST /api/v1/repos → git push 到 /git/<user>/<repo>）
# 3. 判定
git -c protocol.version=1 clone --depth=1 http://127.0.0.1:18131/git/<user>/<repo> <dst>
#   → fatal: Server does not support shallow clients（发出任何 want 之前）
git -c protocol.version=2 clone --depth=1 ... 
#   → 成功，rev-list --count = 1，.git/shallow 写入边界
```

**观察**：NPC 第 4 轮开头说「你两条都赢」并主动撤回，说明反向清单 + 逐条证据的机制在起作用；但它同时把上一轮自己提的「机械校验」当成对**我方**的成立前提反过来用，导致两条误判。反向清单需要记录「已撤回」状态，避免下一轮再把撤回项当成争议点重提。

### consume 5 / 6

- 判定：**成立 3 / 部分成立 1 / 确认 2**（另有 1 条是对方主动撤回我方第 4 轮结论，0 条新增「不成立」）
- 动作：L1 文档 `19df4eb` + L1 注释 `b9d22c1`；L2 新增 2 项进待拍板
- 验证：`BASE=58784f0`；`scripts/check.sh --quick` 通过（53 s，缓存热）。**口径说明**：改动全为 `.md` + `//!`/`///` 注释，无可编译语义变化，所以只有一次「含改动的树」的运行，未单独跑改前基线（`58784f0` 本身是 ledger-only 提交，上一轮绿的延续）
- 回帖：见下（本轮回帖 ID 记入下次运行的起点）

| # | NPC 第 5 轮指摘 | 判定 | 处置 |
|---|---|---|---|
| 1 | V1 + `--depth`：它按我给的配方实测复现，撤回上一轮反对 | 对方撤回 | 反向清单记「已撤回」（见下），本节不再讨论 |
| 2 | §4 把出站写成 `EntriesToBytesIter`，方向错了；`EntriesToBytesIter` 其实是入站链路的环 | **成立（我错两处）** | 已重写该段。①类型方向确实反了：出站是 `output::bytes::FromEntriesIter`（`ops.rs:843` 构造、定义 `data/output/bytes.rs:10`，吃 `output::Entry`），`EntriesToBytesIter` 定义在 `data/input/entries_to_bytes.rs:14`、全 crate 唯一新建点 `bundle/write/mod.rs:111`，就在 `LookupRefDeltaObjectsIter` 之后。②我引的 `bundle/write/mod.rs:104` 也是错的，该行是 `data::input::EntryDataMode::KeepAndCrc32`，正确值是 `:100` |
| 3 | §6 路由引用 `:458-466`/`:454`/`:581` 已改对；`upload_pack.rs:332-335`、`mirror/service.rs:244` 接受不改 | 确认 | 无改动 |
| 4 | `cli_gateway::spawn_async` 是死代码，doc 还在说「used for pack-objects, index-pack」 | **成立** | 已改注释（模块文档 `cli_gateway.rs:12-14` + 方法文档 `:255-261`），写明「当前全仓无调用方」及真实的出站/入站路径。注意：这次改动本身使 `spawn_async` 与守卫测试各下移 5 行（`257`→`262`、`365`→`370`） |
| 5 | 防回归守卫有真实旁路：`process::execute_command(program, …)` 程序名是运行时变量，字面量 grep 抓不到，且其 doc 示例就是 `execute_command("git", …)` | **部分成立** | 机制成立、严重度被高估：全仓 `execute_command` **零调用点**（唯一出现是它自己的 doc 示例 `process.rs:49`），`execute_script` 只有一处测试调用（`process.rs:229`，跑 shell 字符串，非 git）。所以是**潜在**缺口而非现存的越权路径。另纠两处行号：`execute_command` 在 `:51` 不是 `:47`；`execute_script` 在 `:15` 不是 `:13`。收紧守卫要动测试逻辑 + 白名单，属代码改动 → 进 L2 |
| 6 | `v2.rs:806` 静默空 `acked` 是真问题，但它同意属 L2、不催 | 确认 | 保持 L2 #5，无新动作 |
| 7 | `pack_for_wants*` 三层：`pack_for_wants` / `_filtered` 稳定，`_opts` 是生产不可达的测试缝 | **成立** | 9 个调用点逐个打印核对，与其表一致。已写进文档新小节（§3 `pack_for_wants*` 三层的稳定性） |

**方法论（第三次同类）**：NPC 连续三轮出现「结论方向对、行号错 1–4 行」的模式（本轮 `ops.rs:841`→`843`、`bundle/write/mod.rs:104` 判「对」→实际 `:100`、`process.rs:47`→`:51`、`:13`→`:15`）。因此**不能因为它的行号错就驳回整条**，也不能因为它说「对」就放过——必须自己 `sed -n` 打出那一行，看它是否真的承载了命题。本轮第 2 条正是「它的结论对、它的行号错、我的行号也错」三者叠加。

### 反向清单勘误（结论不变，理由替换）

第 1 条仍判**不成立**，但理由从「协议允许的两种 want 形态都已覆盖」改为：

- 真实 git 客户端要 sideband 就必须在**首个** want 行声明 capability，不存在「不声明却期待 side-band-64k」的客户端；
- `upload_pack.rs:205-224` 的 Form B 是**容忍畸形输入的宽松解析**，不是协议允许的第二种形态，账本不能把它写成协议义务；
- `scripts/git-protocol-e2e.sh:172-190` 的三个 V1 用例只断言「trace 中不含 version 2」，实际走的是 Form A（首行 `\0` 后带 `side-band-64k`），因此它证明的是「声明了就成立」，不能证明「不声明也不会失败」。

### 待拍板（L2，本流程不动 main）

1. **HTTP V1 广告串里混了三个未实现能力**（`crates/rg-http/src/lib.rs:2638`）：`multi_ack_detailed` / `no-done` / `thin-pack` 服务端一个都没实现（只有单发 `NAK`），而 SSH 侧只广告实现了的。二选一：收缩广告串，或补多 ACK。影响面：`protocol.version=0/1` 的 HTTP 客户端。
2. **空仓库伪 ref 名**（`upload_pack.rs:332-335`）：是否改成标准 `<null-sha> capabilities^{}\0<caps>`。
3. **upload-pack 不排空 stdin**（对比 `receive_pack.rs:462`）：SSH 下影响小，HTTP 下 body 仍在灌 duplex，是否需要 `drain`。
4. `mirror/service.rs:167` 注释与 `run_git_remote_update` 函数名仍在说 `git remote update`，实际执行 `git fetch --prune origin`（`service.rs:327`）。改名属代码改动，范围外。
5. **`v2.rs:806` 的 ack 扫描吞错**：`gix::open(repo_path).ok()` 在仓库打不开时静默返回空 `acked`，`write_acknowledgments` 于是发 `NAK`（`v2.rs:1272-1291` 的测试把这个形状钉住了）。丢 ACK 比报错更难查。
6. **`cli_gateway.rs` 的 `spawn_async` 零调用**：`crates/rg-git/src/cli_gateway.rs:257-280` 定义了但全仓无调用方，模块文档 `:6` 却在宣传它；防回归测试 `test_no_raw_git_command_in_crates` 只管 `Command::new("git")`，守不住这种死 API。
7. **`ops.rs:933-936` 的懒取回路径没进文档**：want 直接给 blob/tree 时按 `extra_inputs` 原样打包、不走进 history walk（promisor 懒取回），`ops.rs:1772-1820` 有测试（`lazy fetch under {spec}` 断言 pack 恰好 1 个对象），e2e `:340-348` 在真客户端上跑过。**已于第 1 轮补进文档**（§3 懒取回那条），当前代码引用是 `ops.rs:949-950`（本行原写的 `:933-936` 是旧行号，已过期）。
8. **防回归守卫抓不到运行时程序名**（第 5 轮写入）：`test_no_raw_git_command_in_crates`（`cli_gateway.rs:370`）只做两个字面量匹配（`:396-397`），而 `crates/rg-core/src/platform/process.rs:51` 的 `execute_command(program, args)` 程序名是运行时变量、`:15` 的 `execute_script` 直接 `sh -c`，两者都可绕过。当前 `execute_command` **零调用点**（仅 `:49` doc 示例写着 `execute_command("git", …)`），`execute_script` 仅 `:229` 一处测试调用 —— 故为潜在缺口。两个方向：①测试把 `execute_command(` / `execute_script(` 也纳入扫描 + 白名单；②给 `execute_command` 加 `#[doc(hidden)]` 并禁止 `rg-git` 外使用，把 git 调用强制收进网关。要动测试逻辑，属 L2。
9. **`pack_for_wants_opts` 是生产不可达的测试缝**（第 5 轮写入）：`ops.rs:930` 只被 `pack_for_wants`（`:925`，写死 `false`）与一个测试（`:1875`）调用，`shallow_mode: true` 在生产走不到。选项：删掉它、把测试改调 `filtered`（三层压两层），或留着并在文档里标非稳定（**文档侧已标**，见 §3）。删代码属 L2。

### 判定不成立（原文保留，请勿重复提）

1. 「V1 upload-pack 未收到 NUL 时会漏掉 connection 级 capability，但 flush 与 `# service=` 行上的 capability 不一致……能力协商靠「客户端好心多打一个空格」才成立，缺它则 clone/fetch 失败，而不是降级。」—— 反证：`upload_pack.rs:205-224` 的 Form A/Form B 分支覆盖了协议允许的两种形态；git 的 capability 只出现在第一个 `want` 行，客户端要么声明 `side-band-64k`（走 sideband），要么不声明（服务端裸发 packfile 属合规降级）。`scripts/git-protocol-e2e.sh:172-190` 的 `protocol.version=1` HTTP push/clone/fetch 用例长期为绿，是该路径可用的直接证据。**以下已核实为不成立，不要重复提。**

2. 「V1 + `--depth` 会真实发送 `deepen 2`，客户端把全量 pack 当浅克隆收下」—— 反证：本服务 V1 广告不含 `shallow`，客户端在发任何 `want` 之前即 `fatal: Server does not support shallow clients`（本地实例实测，trace 无 `want`/`deepen`）；V2 对照 `--depth=1` 正常。NPC 的 trace 含 `upload-pack> shallow <oid>`，该行本服务从不产生，故其服务端不是 IronForge。

2. 「`ofs-delta` 标 ✅ 站不住，`iter_from_counts` 这条路径根本产不出 OFS_DELTA」—— 反证：gix-pack 0.77 `src/data/output/entry/mod.rs:185` 把 `DeltaRef { object_index }` 直接映射为 `data::entry::Header::OfsDelta`（`:84` 另有 `OfsDelta { base_distance }` 写出分支）；`allow_thin_pack` 的文档（`iter_from_counts.rs:382-391`）只约束「base 不在本 pack 里」的 delta。`allow_thin_pack: false` 不等于「不产 OFS_DELTA」。

3. 「`sideband.rs:25` 应为 `:22`」「`pkt_line.rs:59` 应为 `:61-62`」—— **NPC 已于第 4 轮撤回**。反证：`sideband.rs:25` 正是 `writer.flush().await?;`（22 是 `payload.extend_from_slice(chunk)`）；`pkt_line.rs:59` 是 `write_pkt_line` 的函数定义，引用它正是为了说明该函数不调用 flush（61-62 是长度检查，与命题无关）。

### 对方已撤回（勿再作为争议点）

第 5 轮 NPC 在 §7 自行撤回三项，均已由其本机实测/源码证明，此后不再讨论：

| 被撤回项 | 撤回依据（它自己的话） |
|---|---|
| 「V1 + `--depth` 会发 `deepen`，全量包被当浅克隆收下」 | 它 `cargo build --release` 后实测三组对照：V1+depth exit 128 `fatal: Server does not support shallow clients`、V1 无 depth ✅、V2+depth ✅ 且 `.git/shallow` 写入。补测 `fetch --deepen=2` 同样失败。原话「你第 1 条复核请求我给不了反证」 |
| 「`ofs-delta` 标 ✅ 站不住」 | 「撤回，`mod.rs:185` 行号精确」 |
| 「`sideband.rs:25` / `pkt_line.rs:59` 引错」 | 「撤回」（第 4 轮已撤，第 5 轮重申） |

---

## 冷启动验证（协议 §8 第 4 步的三个门槛）

| 门槛 | 状态 | 证据 |
|---|---|---|
| NPC 真的回了 | 通过 | 首扫 `09:25:24Z`，`author.is_npc=true`，自述已核对基线 `5624605`；延迟 **3 min 43 s** |
| 轮次标记可数（进度可重建） | 通过 | `cnb issues list-issue-comments … \| grep -oE '## 第 [0-9]+ 轮收尾' \| wc -l` = 1，下一轮从 2 续跑 |
| 回帖的 `@CodeBuddy` 触发下一轮 | **通过** | 回帖 `09:31:11Z` → 复核 `09:35:33Z`，**4 min 22 s**，`author.is_npc=true` |

三项全过。是否挂 automation 仍是独立决定（见下「未决」）——先人工跑满 6 轮或至少再跑 2 轮，确认修订率下降后再挂。

### 定时任务（2026-11-10 起）

| 角色 | 名称 | 频率 | 关键约束 |
|---|---|---|---|
| 消费者 | OPT-LOOP 消费者（IronForge，每小时） | 每小时 | 抢锁 → 从评论数重建进度 → 核实 → 分级 → `check.sh` → 推 cnb → 五段回帖 + 结尾 `@CodeBuddy` |
| 生产者 | OPT-LOOP 生产者（IronForge，每 2 小时） | 每 2 小时 | 背压闸门（有 open `[opt-round]` 就跳过）→ 选靶 ≤10 文件 → 开 issue → 只提交 ledger |

两条都不推 `origin` / `github`，不 force push，不 `git add -A`。

### 仍未解决

1. **CNB main 无分支保护**：F15（评审者越权直推）在本仓拦不住。协议 v0.3 也只给「发现 + 保留」的处置，没有恢复独立性的机制。
2. **NPC 流水线是否吃 CNB Runner 额度未验证**：CLAUDE.md:26 记录 Runner 额度已耗尽、CI 已迁 git233。若共享额度，定时任务跑到某天会集体失败。
3. **批次上限 6 轮尚未跑满**：目前 5/6。但第 5 轮的产出质量**回升**了：它自己装了 Rust 工具链做实测复现（`cargo build --release` 23 m 28 s），抓到一条我方真实的事实错误（类型方向写反）。结论：边际收益不按轮次线性衰减，取决于它是否肯构建 —— 值得跑满 6 轮再判。
4. **NPC 自举环境会推高延迟到 28 min**（第 5 轮，n=5 破 p100 20 min 46 s）。若继续挂每小时任务，等待窗口应按 ≥30 min 预留，否则那一轮会被误判「等待超时」。

---

## 判定不成立（原文保留，请勿重复提）

（暂无）
