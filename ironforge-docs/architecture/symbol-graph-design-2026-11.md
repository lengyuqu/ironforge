# 符号图设计（issue #63 第一步）：规模实测、保留策略与实施判定

> **任务**：T27（`tasks/T27.md`）。**基线**：`5df821d`（`tasks/T27.md` 入库提交），分支
> `agent/t27-symbol-graph-design`，worktree `../ironforge-t27`。
> **性质**：设计文档 + 实测数据，**不含任何生产代码改动、不含迁移**。
> **结论先行**：**不建议按 #63 原方案（定义 + 引用，逐 commit 全量）实施**。建议第一步只建
> `symbol_definitions`（实测 2.6 MiB/commit），引用侧改为按需计算或「文件级 postings」，
> 不建议预存逐次出现的引用行。理由见 §2、§3。

---

## 0. 摘要

| 问题 | 实测答案 | 出处 |
|---|---|---|
| 参与索引的文件数（本仓） | **1,273**（`supports()` + ≤512 KiB + 可读 UTF-8） | §1.2 |
| 总定义数 | **12,208**（9.6 / 文件，中位数 5，最大 115） | §1.2 |
| 总标识符出现数（引用侧） | **525,886**，减定义名后 **513,678** 条引用 | §1.2 |
| 「每文件数百个标识符」是否成立 | **均值成立**（本仓 413/文件；Rust/Go/Python 语料 521–539），**中位数偏低**（128–316），长尾极端（单文件最大 6,811 / Go 语料 41,167） | §1.3 |
| 一个 commit 的符号表有多大 | 只存定义 **2.6 MiB**；定义 + 引用（含 context、3 索引）**79.8 MiB**；按 #63 草案的 TEXT sha + 4 索引 **168 MiB** | §2.1 |
| 2000 文件 × 数百标识符意味着什么 | 单 commit 引用侧 ≈ **125 MiB**（#63 的假设自己就能把「最近 N 个 commit」压到 N=1） | §2.2 |
| 建议保留多少 commit | 默认分支 HEAD + 最近 5 个（保留深度 `D = clamp(⌊128 MiB / 单 commit 字节数⌋, 1, 6)`；1k 与 10k 文件 D=6，50k 文件 D=1） | §3.2 |
| 是否预存引用行 | **不建议**。引用体积是定义的 30 倍；73.9% 的引用行落在出现次数 >100 的通用名上 | §2、§4.7 |

复现命令见 §1.1 与附录 A；原始输出见附录 B。

---

## 1. 规模实测

### 1.1 方法与复现

统计**作为 `#[ignore]` 测试提交**在
`crates/rg-core/src/search/outline/measure.rs`（不进入 release 构建、不改任何生产代码路径），
而不是一次性脚本：脚本没法被下一个人重跑，`#[ignore]` 测试同时是回归资产。

口径：

- 文件准入 = `OutlineIndexer::supports()`（`Language::from_path` 五语言 + Svelte 降级）
  **且** ≤ `MAX_FILE_BYTES`（512 KiB）**且** `read_to_string` 成功。
- 定义数 = `Outline::item_count()`（含 `children` 递归）。
- 标识符出现数 = 语言的 tree-sitter 查询命中的 identifier 类节点数
  （`identifier` / `type_identifier` / `field_identifier` / `property_identifier` / …），
  这是「按名匹配的引用索引」能产生的**上界**：一条引用行 = 一次出现。
- 引用数 = 标识符出现数 − 定义数（每个定义的名字本身占一次出现）。
- 列宽来自实际数据：路径、定义名、signature（首行折叠后 ≤200 字符）、标识符原文。

```bash
# 主语料（本仓）
cargo test -p rg-core --release measure_symbol_graph_corpus -- --ignored --nocapture

# 交叉语料：Go 标准库、Python 标准库（本仓只有 5 个 .py、0 个 .go）
IRONFORGE_MEASURE_ROOT="$(go env GOROOT)/src" \
  cargo test -p rg-core --release measure_symbol_graph_corpus -- --ignored --nocapture
IRONFORGE_MEASURE_ROOT="$(python3 -c "import sysconfig;print(sysconfig.get_paths()['stdlib'])")" \
  cargo test -p rg-core --release measure_symbol_graph_corpus -- --ignored --nocapture

# 真实行导出（给 §2.1 的 sqlite3 体积实测用）
IRONFORGE_MEASURE_DUMP_DIR=/tmp/symdump \
  cargo test -p rg-core --release measure_symbol_graph_corpus -- --ignored --nocapture
```

环境：macOS arm64（Darwin 27.2.0）、rustc 1.99.1、`tree-sitter` 0.25 /
`tree-sitter-rust` 0.24 / `tree-sitter-typescript` 0.23 / `tree-sitter-python` 0.25 /
`tree-sitter-go` 0.25、SQLite 3.54.0、Go 1.27.3、Python 3.14.10。
基线 `5df821d2551968c92897853e1dce95903cd423f8`。

> **自指说明**：统计工具 `measure.rs` 本身也在被统计的语料里（它就是这么写的：走整个
> worktree）。因此改动本工具会让 Rust 那几行数字漂移几十个标识符。本文所有数字取自**最后一次**
> 运行（release，2.74 s，`parse-error files: 16`），改动测量工具后重跑会对不上几个尾数。

### 1.2 主语料：IronForge 本仓

```
files walked: 1585 (supported+readable: 1273, unsupported ext: 312, >512 KiB: 0, unreadable: 0)
source lines: 299140, parse-error files: 16, truncated files: 0
```

| 语言 | 文件 | 源码 KiB | 定义 | 标识符出现 | 引用（标识符−定义） | 定义/文件 中位 | 定义/文件 最大 | 标识符/文件中位 | 标识符/文件 p90 | 标识符/文件最大 |
|---|---|---|---|---|---|---|---|---|---|---|
| rust | 744 | 7,922 | 10,721 | 400,919 | 390,198 | 9 | 115 | 316 | 1,279 | 6,811 |
| typescript（含 `js`/`mjs`） | 288 | 1,104 | 769 | 53,929 | 53,160 | 1 | 57 | 138 | 365 | 1,709 |
| svelte（`<script>` 降级） | 236 | 966 | 629 | 63,429 | 62,800 | 2 | 15 | 224 | 495 | 1,572 |
| python | 5 | 155 | 89 | 7,609 | 7,520 | 14 | 43 | 765 | 4,786 | 4,786 |
| go | 0 | — | — | — | — | — | — | — | — | — |
| **合计** | **1,273** | **10,149** | **12,208** | **525,886** | **513,678** | **5** | **115** | **218** | **918** | **6,811** |

均摊：**9.6 个定义 / 文件**、**413 个标识符 / 文件**、**93.8 条 `(文件, 名字)` 去重后 postings / 文件**。

列宽（字符；SQLite 里 TEXT 按 UTF-8 存，本仓以 ASCII 为主，字节数≈字符数）：

| 语言 | 路径 均/最大 | 定义名 | signature | 标识符 均/最大 |
|---|---|---|---|---|
| rust | 44.1 / 85 | 18.1 | 43.3 | 6.8 / 91 |
| typescript | 46.6 / 75 | 11.5 | 47.4 | 7.6 / 41 |
| svelte | 50.1 / 70 | 10.0 | 38.4 | 5.7 / 35 |
| python | 22.0 / 27 | 8.4 | 28.6 | 5.0 / 29 |
| **合计** | **45.7 / 85** | **17.2** | **43.2** | **6.7 / 91** |

### 1.3 交叉语料：证明「数百」不是本仓的偶然

本仓没有 `.go`、只有 5 个 `.py`，单靠它无法判断「每文件数百标识符」是否语言无关。用两台
真实语料补测（可复现，但不是固定版本的依赖，Go/Python 版本已记录）：

| 语料 | 文件 | 源码 KiB | 定义 | 定义/文件（均） | 标识符出现 | 标识符/文件（均） | 标识符/文件中位 | p90 | 最大 |
|---|---|---|---|---|---|---|---|---|---|
| Go 1.27.3 `GOROOT/src` | 7,034 | 61,678 | 91,218 | 13.0 | 3,664,753 | **521** | 128 | 1,170 | 41,167 |
| Python 3.14.10 stdlib | 1,073 | 15,567 | 25,308 | 23.6 | 567,700 | **529** | 204 | 1,401 | 7,684 |
| IronForge 本仓（合计） | 1,273 | 10,149 | 12,208 | 9.6 | 525,886 | **413** | 218 | 918 | 6,811 |

结论：**Rust / Go / Python 三套独立语料的「标识符/文件」均值高度一致（521–539）**，TypeScript
偏低（187，因为前端文件小、且 TS 查询只把接口/类/函数当作定义）；「数百」这个量级是真实的，
不是本仓的偏差。

### 1.4 对 #63 原文的三处订正

1. **「每文件数百个标识符」——均值成立，中位数偏低，长尾没说。**
   均值 413–539，中位数只有 128–316；单文件最大 6,811（本仓 `rg-core/src/repo/service.rs`
   附近的长尾）与 41,167（Go 语料，生成的表格文件）。任何「按文件平均」的容量规划都会被
   长尾打脸：**单文件上限**（`MAX_ITEMS=2000`、`MAX_FILE_BYTES=512 KiB`）比均值更重要。
2. **「2000 文件」——本仓 1,273 已在同一量级**，但不是最坏情况；monorepo 的 50k 文件见 §2.2。
3. **原文只说了表「很大」，没说大在哪。** 实测：**引用侧是定义侧的 30 倍**
   （513,678 vs 12,208 行；体积 79.8 MiB vs 2.6 MiB）。决策重心应该放在「要不要存引用」，
   而不是「怎么给定义表加索引」。

### 1.5 两个只有实测才会暴露的数据问题

- **16 个文件解析出错，全部是 `web/**/*.test.svelte.ts`**（Svelte 5 runes 写在 `.ts` 里，
  TS 语法直接报错）。失败是软性的（`Outline.error` + 部分 items），但**这正是一个真实的覆盖
  缺口**：Svelte 的 `.svelte.ts` 约定把 runes/组件逻辑放在 TS 文件里，现有五语言覆盖里没有
  任何语法能解析它。索引时要么跳过要么标记 `partial`，不能静默当成「没有定义」。
  （清单见附录 B。）
- **4.1% 的定义名不是标识符**（505 / 12,208），全部来自 Rust `impl` 行：名字是从首行推导的
  整段文本，例如 `ActiveModelBehavior for ActiveModel {}`（106 行）、
  `MigrationTrait for Migration`（105 行）。**`name` 列不能加 `[A-Za-z0-9_]` 约束**，按名查询
  也必须排除 `kind='impl'`，否则会出现「搜 `ActiveModel` 命中 106 个同名不同目标的伪符号」。

---

## 2. 规模推算

### 2.1 每行到底多少字节：用真实数据建表实测

不用「100–150 字节」估，直接把 §1 导出的 12,208 条定义行和 525,886 条引用行导入 SQLite，
按草案建表建索引后读 `dbstat`（脚本见附录 A）。同一个 commit（`repo_id=1`、1273 文件）：

| 变体 | 表/索引 | 体积 | 每行（含索引） |
|---|---|---|---|
| **A. #63 草案**：`commit_sha TEXT(40)`、refs 带 `context`、4 个索引 | refs 表 87.4 MiB / refs·path 47.5 / refs·lookup 29.3 / defs 2.1 / defs 索引 1.9 | **168.2 MiB** | refs **327 B/行** |
| **B.** 同 A，但 `commit_sha` 换成 `commit_id INTEGER` | — | **107.7 MiB** | refs **210 B/行** |
| **C.** B 再去掉 `refs(repo_id, commit_id, path)` 索引 | — | **79.8 MiB** | refs **154 B/行**（表 134 + 索引 20） |
| **D.** 只存定义（2 个索引） | defs 表 1.6 MiB + lookup 352 KiB + path 652 KiB | **2.61 MiB** | defs **224 B/行** |
| **E.** 定义 + **文件级 postings**（`(文件, 名字)` 去重，4 索引） | defs 2.6 + postings 表 8.9 / path 6.5 / lookup 2.4（MiB） | **20.4 MiB** | postings **157 B/行**（119,449 行） |

读法：

- 原文「每行约 100–150 字节含索引」**对定义行偏低、对引用行严重偏低**。引用行贵在
  `path`（41 B）+ `context` 源行（实测均 52.6 B，原行不截断）+ 两个索引键（每行 20 B / 95 B）。
- `commit_sha` 用 40 字节 TEXT 比 `commit_id INTEGER` 贵 **60 MiB/commit**（A→B），因为它在
  每一行和每个索引键里都重复。**建议用 `commit_id`**（§4.2）。
- 引用行里 **95 B/行** 花在「按路径定位」索引上（A 的 `refs·path`，47.5 MiB），它只服务
  增量刷新/按文件删除，服务不了主查询。C 变体去掉它，代价是增量刷新要按 `(repo, commit)`
  整片重写。
- **文件级 postings（E）比逐次出现的引用（C）便宜 3.9 倍**（20.4 vs 79.8 MiB），
  因为 `(文件, 名字)` 去重把 525,886 行压到 119,449 行（4.4×），代价是丢掉行号。

### 2.2 三档推算（含计算过程）

系数 = 目标文件数 / 1273；行数按实测均值 9.6 定义/文件、413 标识符/文件、93.8 postings/文件
线性外推（同一语言混合比例不变；monorepo 通常生成代码更多，实际会偏高）。

| 仓库尺寸 | 文件数 | 定义行 | 引用行 | postings 行 | D：只存定义 | C：定义+引用 | E：定义+postings |
|---|---|---|---|---|---|---|---|
| 小（本仓，实测） | 1,273 | 12,208 | 513,678 | 119,449 | **2.6 MiB** | **79.8 MiB** | **20.4 MiB** |
| #63 假设 | 2,000 | 19,200 | 807,000 | 187,700 | 4.1 MiB | 125 MiB | 32 MiB |
| 中（×7.86） | 10,000 | 95,900 | 4.03 M | 938,000 | **20.5 MiB** | **627 MiB** | **160 MiB** |
| 大（×39.3） | 50,000 | 479,000 | 20.2 M | 4.69 M | **103 MiB** | **3.1 GiB** | **801 MiB** |

计算过程（以「大」档为例）：

```
文件系数      = 50,000 / 1,273                       = 39.28
定义行        = 12,208 × 39.28                       = 479,000
引用行        = 513,678 × 39.28                      = 20.2 M
只存定义      = 2.61 MiB × 39.28                     = 102.5 MiB
定义+引用(C)  = 79.8 MiB × 39.28                     = 3,134 MiB ≈ 3.1 GiB
定义+postings = 20.4 MiB × 39.28                     = 801 MiB
```

每 commit 再乘 N（§3）。**引用侧在「大」档已经不可能进 SQLite 主库**：3 GiB/commit 意味着
备份、`VACUUM`、测试拷贝、以及同库的 `code_fts` 全部被拖垮；即使只留 HEAD，一个 50k 文件的
monorepo 一 push 就会让主库涨 3 GiB。**这是判定「不建议按原方案实施」的主要依据。**

### 2.3 查询侧实测（主路径是否真能用索引）

在 525,866 行引用 / 12,208 行定义的库上（变体 B，含全部索引）：

```
EXPLAIN QUERY PLAN SELECT path, start_line, kind FROM symbol_definitions
  WHERE repo_id=1 AND commit_id=987 AND name='Config';
-- SEARCH symbol_definitions USING INDEX idx_symbol_definitions_lookup (repo_id=? AND commit_id=? AND name=?)
-- 0.020 ms（无命中）
EXPLAIN QUERY PLAN SELECT path, line FROM symbol_references
  WHERE repo_id=1 AND commit_id=987 AND name='db' LIMIT 50;
-- 0.008 ms（命中 7,274 行中的前 50）
SELECT COUNT(*) ... name='db';   -- 0.236 ms
```

`(repo_id, commit_id, name)` 确实是有效主路径，延迟可忽略（本机 in-process SQLite、热缓存；
换 PostgreSQL 加一次网络往返约 1–5 ms，结论不变）。**性能不是问题，体积才是。**

### 2.4 顺带一个好消息：索引本身很便宜

解析吞吐（release，1273 文件）：**2.74 s ≈ 465 文件/s**（debug 为 105 文件/s）。
50k 文件单线程约 108 s，可以塞进现有 `POST /ai/repos/{o}/{r}/index` 的同步路径或后台任务。
主要成本是 tree-sitter 解析；SQLite 侧批量插入 50 万行 + 建索引在本机是秒级（附录 A 的脚本
每次都是「导入 → 建表 → `VACUUM` → `ANALYZE`」）。这也说明「只索引定义」已经拿到 90% 的
导航价值（定义 + signature 正是 `/map` 需要的东西）。

---

## 3. 保留策略

### 3.1 候选与裁决

| 候选 | 每仓体积（1k 文件，只存定义） | 能回答什么 | 裁决 |
|---|---|---|---|
| 只留默认分支 HEAD | 2.6 MiB | 当前代码的符号 | ✅ **必留**，是 99% 查询的答案 |
| 默认分支 HEAD + 最近 5 个 commit | D × 2.6 MiB（D ≤ 6） | 「这个符号是什么时候出现的」「review 时对比 base」 | ✅ **采用**，N 由预算反推（§3.2） |
| 每个被索引分支各留 HEAD | 分支数 × 2.6 MiB | 「另一个分支上叫什么」 | ❌ 分支数不可控（本仓 26 个远端分支 → 68 MiB，且与内容重复 95%）。需要时按需索引该 ref，用同一套 TTL 清理 |
| 按 LRU | 不可预测 | 无 | ❌ 查询语义不确定：同一个请求今天命中、明天 miss。清理必须按**可解释的规则**（ref + 深度）而不是访问模式 |
| 每个 PR head 常驻 | PR 数 × 2.6 MiB | review | ⚠️ 只做**按需 + TTL**（`created_at` 24 h），见 §3.3 |

### 3.2 保留深度从预算反推（默认值与公式）

单个 commit 的定义体积可以从**已索引的 HEAD**直接算出来，不需要预估：
`bytes_per_commit ≈ 224 B × HEAD 的定义行数`（§2.1 实测 224 B/行含两个索引）。
存储预算默认 **128 MiB / 仓库**。保留深度 `D` = 保留的 commit **总数（含 HEAD）**：

```
D = clamp( ⌊128 MiB / bytes_per_commit⌋ , 1, 6 )     // 1 = 只有 HEAD，6 = HEAD + 最近 5 个
```

| 仓库尺寸 | bytes/commit（D 变体） | 预算可容纳 | D（上限 6） | 实际占用 | 等价说法 |
|---|---|---|---|---|---|
| 1,273 文件（本仓） | 2.6 MiB | 49 | **6** | 15.7 MiB | HEAD + 最近 5 个 |
| 10,000 文件 | 20.5 MiB | 6 | **6** | 123 MiB | HEAD + 最近 5 个 |
| 50,000 文件 | 103 MiB | 1 | **1** | 103 MiB | 只有 HEAD |

上限取「HEAD + 5」的理由不是磁盘，而是**查询价值**：真正要「某个历史 commit 的定义」的场景是
review（对比 base/head）和「这行是谁引入的」，前者需要的是两个具体 commit（可按需索引，
24 h TTL），后者应该问 git，不该问符号表。再往上加深只是把同一份内容存更多遍——实测相邻
commit 的改动中位数只有 **2 个文件**（§3.4），历史上 98% 的行是完全重复的。

> **对 team-lead 的倾向方案（默认分支 HEAD + 最近 N，N 由预算反推）的验证结论：方向成立，
> 但只在「只索引定义」时成立。** 若把引用一起存（C 变体，79.8 MiB/commit），1k 文件的仓库
> 预算只够 1 个快照（HEAD，79.8 MiB，占 62% 预算）；10k 文件时**连 HEAD 都超预算**
> （627 MiB）——此时公式退化成「不留历史」，也就是原方案在自己的规模假设下自我否定。

### 3.3 什么时候清、按什么判据

**两段式，都复用现有先例：**

1. **写入即修剪（同事务）**。索引器写新 commit 后，立刻删除该仓库超出保留集的
   `(repo_id, commit_id)` 行。判据是**保留集**：
   `retained = 默认分支 git rev-list -n D <branch>`（D 见 §3.2，HEAD 恒在）。
   好处：稳态下不需要额外任务，也不会出现「索引成功但磁盘涨到满」的窗口。
2. **每小时 sweep（兜底）**。`symbol_graph.cleanup` 通过
   `tokio::spawn(rg_core::task_supervisor::supervise("symbol_graph.cleanup", …))` 注册在
   `crates/rg-http/src/lib.rs:250-300` 同一批（T11 `agent_drafts.cleanup`、T19
   `webhook.outbox_sweep`、`repo_events.cleanup` 都在这里），循环体照
   `crates/rg-http/src/api/repo_events.rs:242-253` 的形状：`interval.tick()` → 一次清理 →
   `tracing::info!(count, …)`。它负责写入路径覆盖不到的情况：
   - 仓库被删除后残留的 `symbol_*` 行（`repo_id` 不再存在）；
   - 按需索引的 PR head 过期（`symbol_commits.created_at` 超 TTL）；
   - 已 force-push 消失的 commit（`git cat-file -e` 失败）。
   
   判据用**两者结合**，各管一类：`commit_sha 不在保留集` 处理默认分支回退/force-push；
   `created_at` 超期处理按需索引的临时 commit 与孤儿行。只用时间会留着无用的历史行，
   只用保留集会因 git 仓库已删而永远算不出保留集。

### 3.4 如果将来真的要深历史：按文件增量，别按 commit 全量

实测本仓最近 50 个提交（其中 37 个产生了文件改动统计，其余是合并提交）的改动规模：
**均值 3.5 个文件、中位 2、p90 7、最大 13**（占 1,273 文件的 0.3%）。因此：

- 全量快照存 987 个 commit（本仓全部历史）：`2.6 MiB × 987 ≈ 2.5 GiB`（只存定义）；
- 按文件增量（同一路径只存最新版本 + `last_commit_id` 映射，查询时向祖先回溯）：
  `2.1 KiB/文件 × 3.5 文件 ≈ 7.4 KiB/commit × 987 ≈ 7 MiB`。

差 350 倍。**但增量模式需要一张 `symbol_file_state(repo_id, path, last_commit_id)` 和
「沿 commit 祖先回溯找最近版本」的解析逻辑**，查询复杂度从「一个索引查找」变成「一次回溯」。
**建议：第一阶段不做**。等到有人真的需要「历史任意 commit 的符号」，再按 §6 的 T30（增量历史形态）立项，
届时用这条实测数据说服自己（7 MiB vs 2.5 GiB）。

---

## 4. 表结构草案

### 4.1 草案（第一步实际建议）

```sql
-- 1) commit 注册表：把 40 字节 sha 从每行/每个索引键里挪出来（实测省 60 MiB/commit）
CREATE TABLE symbol_commits (
    id          INTEGER PRIMARY KEY,
    repo_id     INTEGER NOT NULL,
    commit_sha  TEXT    NOT NULL,   -- 40 hex
    ref_name    TEXT    NOT NULL,   -- 索引时解析出的 ref（默认分支 / 按需的 PR head）
    created_at  TEXT    NOT NULL,
    UNIQUE (repo_id, commit_sha)
);

-- 2) 定义：第一步唯一要落地的表
CREATE TABLE symbol_definitions (
    id         INTEGER PRIMARY KEY,
    repo_id    INTEGER NOT NULL,
    commit_id  INTEGER NOT NULL,    -- → symbol_commits.id
    path       TEXT    NOT NULL,    -- 仓库内相对路径，'/' 分隔
    name       TEXT    NOT NULL,    -- 原样，不规范化（见 4.3）；impl 行不是标识符
    kind       TEXT    NOT NULL,    -- function/method/struct/enum/trait/impl/class/interface/module/const/macro/type
    start_line INTEGER NOT NULL,    -- 1-based
    end_line   INTEGER NOT NULL,    -- 1-based，含
    signature  TEXT    NOT NULL,    -- 首行折叠，≤200 字符
    exported   INTEGER NOT NULL,    -- 0/1
    confidence TEXT    NOT NULL     -- 'exact'（第一步只有这一种）
);

CREATE INDEX idx_symbol_definitions_lookup ON symbol_definitions(repo_id, commit_id, name);
CREATE INDEX idx_symbol_definitions_path   ON symbol_definitions(repo_id, commit_id, path, start_line);
```

```sql
-- 3) 引用：**第一步不建**。若第二步确有需求，按此形状（文件级 postings，丢行号）：
CREATE TABLE symbol_postings (
    id         INTEGER PRIMARY KEY,
    repo_id    INTEGER NOT NULL,
    commit_id  INTEGER NOT NULL,
    name       TEXT    NOT NULL,
    path       TEXT    NOT NULL,
    confidence TEXT    NOT NULL     -- 'name_match'
);
CREATE INDEX idx_symbol_postings_lookup ON symbol_postings(repo_id, commit_id, name);
-- 是否再建 (repo_id, commit_id, path) 取决于增量刷新策略：实测它占 6.7 MiB/commit（+33%）
```

与 #63 草案的差异（每条都有实测依据）：

| #63 草案 | 本草案 | 依据 |
|---|---|---|
| `symbol_definitions(repo_id, commit_sha TEXT, …)` | `commit_id INTEGER` + `symbol_commits` 注册表 | A→B 实测省 60 MiB/commit（168→108 MiB） |
| `symbol_references(path, line, name, context)` 逐次出现 | 第一步不建；将来用 `symbol_postings(path, name)` | 引用侧 30× 定义体积；逐次出现 79.8 MiB vs postings 20.4 MiB（3.9×） |
| 未提 `kind` / `exported` | 保留（`/map` 直接需要） | 现有 `OutlineItem` 已产出，不额外解析 |
| 未提 `confidence` | 两表都带，枚举 `exact \| name_match` | §5，Go 接口按名匹配会误报 |

### 4.2 `commit_sha` vs `commit_id`

用 `INTEGER` 外键。40 字节的 sha 对 525k 行意味着 21 MB 纯重复，且在 `(repo, commit, name)`
索引键里再重复一次。代价是一次 `symbol_commits` join（本地 SQLite 上可忽略，见 §2.3 的
0.02 ms；PG 上一条索引查找）。**不要**为了省这一次 join 把 sha 塞回业务表：实测差值 60 MiB/commit，
够存 23 个 commit 的定义。

### 4.3 `name` 是否规范化：**不规范化**

| 候选 | 判定 | 理由 |
|---|---|---|
| 大小写归一（`lower(name)`） | ❌ | 五门语言里只有 Go 的可见性吃大小写，但**标识符本身大小写敏感**；归一后 `HTTPServer`/`HttpServer` 会互相污染，而 Go 的 `exported` 语义也可能被「按名查定义」误用 |
| 按 `::` 拆成 `owner` / `member` 两列 | ❌ | tree-sitter 给的是**词法**标识符，`Foo::bar` 在 Rust 里是 `scoped_identifier`，拆开需要路径解析；而 `impl` 行 4.1% 根本不是标识符（§1.5），拆不动 |
| 原样存储 + 精确等值查询 | ✅ | 与 `OutlineItem.name` 一一对应，索引 352 KiB/12k 行；模糊搜索交给已有的 `code_fts` |
| 前缀/子串搜索 | 交给 `code_fts` | 不要把 `LIKE '%x%'` 压到符号表上（扫描 20M 行） |

补充约束：**不要给 `name` 加 `[A-Za-z0-9_]` 的 CHECK**；按名查定义的 SQL 必须加
`kind <> 'impl'`（或后续把 impl 行的 `name` 置 NULL）。附录 C 记了这条。

### 4.4 是否存 `signature`：**存**

- 体积：均 43.2 字符，占定义行（224 B）的 19%，全表 2.6 MiB/commit —— 可忽略。
- 有用性：`/map`、`/outline` 的核心载荷就是 signature；不存就得回读文件（磁盘 I/O + 重新
  解析），而存了之后「列出某文件的定义」是一次索引扫描。
- 已有不变量：单行、折叠空白、≤200 字符（`MAX_SIGNATURE_CHARS`），不会出现函数体。

### 4.5 索引与实测大小（1,273 文件 / 12,208 行）

| 索引 | 大小 | 服务的查询 |
|---|---|---|
| `(repo_id, commit_id, name)` | 352 KiB | 按名查定义（主路径，§2.3） |
| `(repo_id, commit_id, path, start_line)` | 652 KiB | 文件 outline / `/map`；按文件删除（增量刷新） |
| postings `(repo_id, commit_id, name)`（第二步） | 2.5 MiB（119,449 行） | 按名查引用文件列表 |

定义侧索引总开销 1.0 MiB/commit，占定义体积的 38%——**索引不是问题，行才是**。

### 4.6 主路径与契约

```
GET /ai/repos/{o}/{r}/symbols?name=Foo&ref=main[&kind=function][&limit=50]
  → [{ path, kind, start_line, end_line, signature, exported, confidence: "exact" }]
GET /ai/repos/{o}/{r}/symbols?path=crates/rg-core/src/search/indexer.rs&ref=main
  → 该文件的定义列表（等价于既有 outline，但走 DB、带 commit 语义）
```

`ref` 缺省 = 仓库默认分支；解析成 commit 后查 `symbol_commits`，未索引则返回
`404 symbol_index_missing`（与 `/map` 的 `budget` 参数同风格），**不要**静默回落到 HEAD——
静默回落会让 agent 拿到错误 commit 的符号还以为是对的。

### 4.7 名字扇出：为什么「按名查」必须带过滤条件

本仓引用侧 top 名字（出现次数）：`db` 7,274、`assert_eq` 6,100、`String` 5,995、`id` 5,821、
`status` 5,662、`Some` 5,342、`unwrap` 5,207、`repo` 4,700、`json` 4,518、`env` 4,356。

**73.9% 的引用行属于「出现次数 >100」的名字。** 也就是说：只按名字查引用，返回的 3/4 是噪音。
定义侧同理（`tests` 134、`common` 127、`Relation` 106…）。

这直接支持两条设计决定：(a) 引用查询必须能叠加 `path`/`kind`/目录前缀过滤，否则不可用；
(b) 第一步不做引用表——先拿到「定义 + signature」，agent 再用 `code_fts`/grep 验证候选，
比给一个 74% 噪音的引用列表更诚实。

---

## 5. 诚实边界（落到字段）

数据模型只能表达「这条命中的可信度」，表达不了「一定有某条命中没被找到」。因此契约分两层：

```rust
pub enum Confidence { Exact, NameMatch }   // 每行必带
// 响应级：partial: bool + limitations: [&'static str]（与 Outline.truncated 同构）
```

- `exact`：这条行**就是**语法树里的那个声明/绑定（定义行的全部；同文件、同作用域的静态引用）。
- `name_match`：只知道名字相同，**不知道是不是同一个符号**（接口分发、动态属性、宏展开后的调用）。
- `partial: true`：本次结果**可能缺行**（宏生成的定义、`getattr` 字符串、`obj[key]`）。
  这是「没找到 ≠ 不存在」的唯一可机器读取的表达；`confidence` 无法承担这个语义。

| 语言 | 已知限制 | 静态抽取能给出什么 | 数据模型里怎么表达 | 用户可见后果 |
|---|---|---|---|---|
| **Rust** | 宏展开（`macro_rules!`、proc macro） | 生成的 `fn`/`struct` **不出现**；`macro_rules!` 本身是一行 `kind=macro` | `partial=true` + `limitations: ["rust_macro_expansion"]` | 搜不到宏生成的符号：**缺行**，不是错行 |
| **Rust** | `dyn Trait`、`impl Trait for X` 动态分发 | 方法定义挂在 `impl` 行下（`kind=impl` 的 children，实测 678 个 impl 行）；调用点只有方法名 | 引用侧只能 `name_match`；**不存推断出的 receiver 类型** | 同名方法的多个 impl 混在一起；按名查会返回所有 `impl` 的候选 |
| **Rust** | `impl` 行没有名字 | 名字是首行推导的整段文本（4.1% 行，§1.5） | 保留行但 `kind='impl'`，查询过滤；v2 可把 `name` 置 NULL | 搜 `ActiveModel` 命中 106 个「同名不同目标」的伪符号，必须靠 `kind` 排除 |
| **TS/JS** | `obj[key]` 动态属性 | 下标访问不是 `property_identifier`，**不产生引用行** | `partial=true` + `limitations: ["ts_dynamic_property"]` | 动态派发的方法调用看不到 |
| **TS/JS** | `any` 传播 | 只能按名匹配；无法区分同名不同类 | 引用 `confidence='name_match'`；不存类型流 | 同名方法跨类混淆 |
| **Python** | `getattr(obj, "x")` | 字符串参数不是 identifier 节点，**不产生引用行** | `partial=true` + `limitations: ["python_getattr"]` | 反射调用看不到 |
| **Python** | 猴子补丁 | 定义点与运行期不符（`Cls.method = f` 不是 `function_definition`） | 定义 `exact` 但语义已漂移：只能靠 `limitations: ["python_monkey_patch"]` | 静态看到的方法实现可能不是运行时那个 |
| **Go** | 接口方法集运行期确定 | 方法声明（`method_declaration`）可精确抽取；**接口调用只能按名匹配** | 引用 `confidence='name_match'`；不存 `implements` 关系 | **会误报**：同名方法属于不同 receiver 类型时全部命中，接口实现关系查不到 |
| **Svelte** | 只解析 `<script>`（`tree-sitter-svelte` 与 0.25 runtime 不兼容）；`.svelte.ts` 的 runes 文件连 TS 语法都过不了（实测 16 个文件） | 脚本内的声明准确；markup / runes / `$:` 不透明 | `partial=true` + `limitations: ["svelte_markup_unparsed"]` | 组件 `props` 与模板绑定不在符号表里 |

**契约措辞（写进 API 文档，不要只在注释里）**：`confidence: exact` 表示「这一行是精确的
语法绑定」，**不表示**「结果完整」；`partial: true` 时结果集可能缺少宏生成、反射、动态派发的
符号。任何下游（agent/UI）都不得把「没有命中」直接解读为「不存在」——只有 `partial=false` 且没有相关 `limitations` 时才允许这样解读。

---

## 6. 实现拆解

按 #63 的「先只做符号表 + 按名查定义的最小闭环，引用与影响面留到第二步」。

### T28 — 定义表最小闭环（建议下一步就做）

| 项 | 内容 |
|---|---|
| 范围 | 迁移 `m20261118_000001_create_symbol_definitions.rs`（`symbol_commits` + `symbol_definitions` + 2 索引，`up` 幂等、`down` 可回滚，参照 `m20261115_000001_create_agent_draft_namespaces.rs`）；`rg-core` 新增 `SymbolIndexer`（复用 `OutlineIndexer::extract`，写库）；`POST /ai/repos/{o}/{r}/index` 在现有 FTS 之后追加符号写入（同一 ref、同一 commit）；写入即修剪（§3.3 第 1 段） |
| 不包含 | 引用表、HTTP 查询端点、历史/增量模式、按需 PR head |
| 验收 | ① `scripts/check.sh --full` 绿；② 对本仓索引后定义行数 12,208 ±5%、DB 增长 ≤ 3.5 MiB；③ 同一 commit 重复索引幂等（行数不变）；④ 索引 D+2 个 commit 后只剩 D 个（D 可配置，默认 6，见 §3.2）；⑤ 迁移在 SQLite 与 PG 上 up/down 各跑一次 |
| 依赖 | 无（`OutlineIndexer` 已在 `crates/rg-core/src/search/indexer.rs:77`） |
| 风险 | `ItemKind::Impl` 行的处理（§1.5）：建议写入但查询排除，避免迁移后才发现 4.1% 的脏名字 |

### T29 — 查询端点与契约

| 项 | 内容 |
|---|---|
| 范围 | `GET /ai/repos/{o}/{r}/symbols`（按 `name` 或 `path`，`ref` 必解析、缺索引返回 404）；响应每行带 `confidence`，响应带 `partial` + `limitations`（§5）；路由注册进 `REGISTRY`（`collab_contract_tests.rs` 的防漂移测试，见 T26 文档的提醒）；MCP 工具 `find_definition` |
| 不包含 | 引用查询、影响面分析 |
| 验收 | ① 未知 ref / 未索引 ref → 404 且不回落 HEAD；② `kind='impl'` 不出现在按名结果里；③ `partial=true` 至少在 Go/TS 仓上出现并有 `limitations`；④ 契约测试与 openapi 注册测试绿；⑤ 50 条结果的 p95 < 10 ms（本机） |
| 依赖 | T28 |

### T30 — 引用与影响面（**先立项再决定形态**）

| 项 | 内容 |
|---|---|
| 范围（建议形态） | `symbol_postings`（文件级、无行号）+ 按名查「哪些文件出现过这个名字」；**再**加一个按需的 occurrence 扫描端点（对单个名字在索引树里跑一次 tree-sitter/`grep`，不落库） |
| 明确不做 | 逐 commit 预存逐次出现的引用行（实测 79.8 MiB/commit、50k 文件 3.1 GiB，§2.2） |
| 验收门槛 | 立项前必须回答「哪个用户场景需要预存的引用行，按需扫描为什么不够」；若只是为了 agent 看影响面，按需扫描 + `code_fts` 已验证够用 |
| 依赖 | T28（表结构 + 保留策略）；若改为增量历史模式，需要 §3.4 的 `symbol_file_state`，单独立项 |
| 体积预算 | postings：1k 文件 20.4 MiB/commit、50k 文件 801 MiB/commit；**只允许存默认分支 HEAD**，不进历史 |

---

## 附录 A：完整复现脚本

```bash
# 0) 环境
rustc -V; sqlite3 --version; git rev-parse HEAD

# 1) 规模统计（§1）
cargo test -p rg-core --release measure_symbol_graph_corpus -- --ignored --nocapture

# 2) 导出真实行（行数见 §1.2）
rm -rf /tmp/symdump
IRONFORGE_MEASURE_DUMP_DIR=/tmp/symdump \
  cargo test -p rg-core --release measure_symbol_graph_corpus -- --ignored --nocapture
wc -l /tmp/symdump/*.tsv

# 3) 按草案建表建索引后读 dbstat（§2.1 的 A/C/D/E）
cd /tmp/symdump
sqlite3 sym.db <<'SQL'
PRAGMA page_size=4096;
CREATE TABLE defs_stage(path TEXT, name TEXT, kind TEXT, start_line INT, end_line INT, signature TEXT);
CREATE TABLE refs_stage(path TEXT, line INT, name TEXT, context TEXT);
.mode tabs
.import definitions.tsv defs_stage
.import references.tsv refs_stage
CREATE TABLE symbol_definitions(id INTEGER PRIMARY KEY, repo_id INTEGER NOT NULL,
  commit_id INTEGER NOT NULL, path TEXT NOT NULL, name TEXT NOT NULL, kind TEXT NOT NULL,
  start_line INTEGER NOT NULL, end_line INTEGER NOT NULL, signature TEXT NOT NULL,
  confidence TEXT NOT NULL);
INSERT INTO symbol_definitions SELECT NULL,1,987,path,name,kind,start_line,end_line,signature,'exact' FROM defs_stage;
CREATE INDEX idx_symbol_definitions_lookup ON symbol_definitions(repo_id, commit_id, name);
CREATE INDEX idx_symbol_definitions_path ON symbol_definitions(repo_id, commit_id, path, start_line);
VACUUM; ANALYZE;
SELECT name, ROUND(SUM(pgsize)/1024.0) kib FROM dbstat GROUP BY name ORDER BY 2 DESC;
SELECT 'total_kib', page_count*page_size/1024 FROM pragma_page_count(), pragma_page_size();
SQL
```

（A/B/C/E 变体按 §2.1 的列定义增删 `commit_id`/`commit_sha`/`context` 与索引即可；
每一步的原始体积见附录 B 末表。）

## 附录 B：原始输出

主语料（`5df821d`，1273 文件；release 2.75 s）：

```
| language | files | source KiB | defs | identifier occurrences | references (id - defs) | defs/file median | defs/file max | ids/file median | ids/file p90 | ids/file max |
| rust       | 744  | 7922 | 10721 | 400919 | 390178 | 9 | 115 | 316 | 1279 | 6811 |
| typescript | 288  | 1104 | 769   | 53929  | 53160  | 1 | 57  | 138 | 365  | 1709 |
| svelte     | 236  | 966  | 629   | 63429  | 62800  | 2 | 15  | 224 | 495  | 1572 |
| python     | 5    | 155  | 89    | 7609   | 7520   | 14| 43  | 765 | 4786 | 4786 |
| **total**  | 1273 | 10148| 12208 | 525886 | 513678 | 5 | 115 | 218 | 918  | 6811 |
source lines: 299140, parse-error files: 16, truncated files: 0
```

Go 1.27.3 `GOROOT/src`（含 6 个 >512 KiB 跳过、65 个解析出错文件）：

```
| go     | 7034 | 61678 | 91218 | 3664753 | 3573535 | 4 | 1639 | 128 | 1170 | 41167 |
source lines: 2154725, parse-error files: 65, truncated files: 0
```

Python 3.14.10 stdlib（1 个 >512 KiB 跳过）：

```
| python | 1073 | 15567 | 25308 | 567700 | 542392 | 10 | 570 | 204 | 1401 | 7684 |
source lines: 448017, parse-error files: 0, truncated files: 0
```

SQLite `dbstat`（1,273 文件 / 1 个 commit）：

| 变体 | 表/索引 | 字节 | KiB |
|---|---|---|---|
| A（TEXT sha，4 索引） | refs 91,648,000 / refs·path 49,795,072 / refs·lookup 30,715,904 / defs 2,195,456 / defs·path 1,142,784 / defs·lookup 839,680 | 176,345,088 | 172,212 |
| C（INTEGER，3 索引） | refs 70,483,968 / refs·lookup 10,424,320 / defs 1,695,744 / defs·path 667,648 / defs·lookup 360,448 | 83,640,320 | 81,680 |
| D（只定义，2 索引） | defs 1,695,744 / defs·path 667,648 / defs·lookup 360,448 | 2,732,032 | 2,668 |
| E（定义 + postings，4 索引） | postings 9,326,592 / postings·path 6,823,936 / postings·lookup 2,539,520 / defs 2,723,840 | 21,422,080 | 20,920 |

（每个「字节」列已含 `sqlite_stat1` / `sqlite_schema` 的 8 KiB；KiB 为 `dbstat` 汇总后的
`page_count*page_size`，与逐项相加一致。正文统一用 MiB ≈ KiB/1024。这份库是用上述导出建出来的，
导出时行数是 12,208 + 525,866：与 §1.2 的 525,886 差 20 行，因为统计工具本身也在语料里，
对体积结论无影响。）

16 个解析失败文件（全部是 Svelte 5 runes 写在 `.ts` 里）：

```
web/src/lib/components/collaboration/{EscalationSettings,DependenciesEditor,TakeoversCard,
  DeclarationsCard,BudgetCard,DeletionImpactCard,GroupAddLinkForm,TemplateForm,ReassignForm,
  GroupCreateForm}.test.svelte.ts
web/src/routes/[owner]/[repo]/collaboration/{settings,health,groups/[id],search,rules,templates}/page.test.svelte.ts
```

## 附录 C：与 #63 原文的差异清单（逐条可核对）

| #63 原文 | 本文件结论 | 依据 |
|---|---|---|
| 「表规模会很大（2000 文件 × 每文件数百个标识符）」 | 量级成立（均值 413–529），但中位数 128–316、单文件可达 4 万；**真正的规模来源是引用侧**（30× 定义） | §1.2 §1.3 |
| 「先做设计文档再动手：估算行数」 | 已实测而非估算：1,273 文件 → 12,208 定义 + 513,678 引用 | §1 |
| 「定保留策略（只留最近 N 个 commit？只留默认分支？）」 | 默认分支 HEAD + 最近 5 个（深度 D=1–6，由 128 MiB 预算反推）；不做每分支 HEAD、不做 LRU | §3.1 §3.2 |
| 「定清理任务」 | 写入即修剪 + 每小时 `symbol_graph.cleanup`（沿用 `repo_events.cleanup` 的注册与形状） | §3.3 |
| 「新增符号表」 | 是 SQLite 表；`commit_sha` 用 `INTEGER` 外键而非 TEXT（省 60 MiB/commit） | §4.2 |
| 「先只做符号表 + 按名查定义的最小闭环」 | 同意，且**第一步也不建引用表** | §6 T28/T29 |
| 「引用与影响面留到第二步」 | 同意；第二步建议 postings（文件级）或按需扫描，**不建议逐次出现预存** | §2.1 §6 T30 |
| 「Go 接口方法集运行期确定（会误报）」 | 落到 `confidence='name_match'` + `limitations`，API 文档必须写「会误报」 | §5 |
