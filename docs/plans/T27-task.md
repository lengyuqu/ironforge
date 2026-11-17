# T27 执行记录 — 符号图设计文档与规模实测（#63 第一步）

> 计划见 `docs/plans/T27-plan.md`，权威约束见 `tasks/T27.md`。
> 基线 `5df821d`，分支 `agent/t27-symbol-graph-design`，worktree `../ironforge-t27`。

## 1. 开工核对

```
$ git fetch cnb && git rebase cnb/main
Current branch agent/t27-symbol-graph-design is up to date.
$ git log --oneline -1
5df821d docs(tasks): add T27 for the symbol-graph design and size measurement (#63 step 1)
$ rustc -V; sqlite3 --version | cut -d' ' -f1
rustc 1.99.1 (58cf8f9c3 2026-10-13)
3.54.0
```

本仓五语言实际文件数（`git ls-files`）：`rs 743 / ts 245 / mjs 42 / js 1 / svelte 236 / py 5 / go 0`。
Go 为 0，直接决定了必须补交叉语料（§3）。

## 2. 改动

| 文件 | 改动 |
| --- | --- |
| `ironforge-docs/architecture/symbol-graph-design-2026-11.md` | **新建**，六节 + 三个附录（复现脚本、原始输出、与 #63 差异清单） |
| `ironforge-docs/README.md` | 架构表 +1 行 |
| `crates/rg-core/src/search/outline/measure.rs` | **新建**，`#[cfg(test)]` 语料统计（`#[ignore]` 测试 + 可选 TSV 导出），无生产代码 |
| `crates/rg-core/src/search/outline/mod.rs` | +2 行：`#[cfg(test)] mod measure;` |
| `docs/plans/T27-plan.md`、`docs/plans/T27-task.md` | 本任务文档 |
| `tasks/T27.md` | `State` → `completed`（`TASKS.md` 由协调者更新） |

**没有**：迁移、`Cargo.toml`/`Cargo.lock`、`CLAUDE.md`、`.cnb.yml`、`.github/`、`scripts/`、
`deploy/`、`rg-mcp/` 的改动。`cargo fmt --all` 只重排了新文件，未触及既有文件。

## 3. 实测数据

### 3.1 主语料（IronForge `5df821d`，release 2.75 s）

```
files walked: 1585 (supported+readable: 1273, unsupported ext: 312, >512 KiB: 0, unreadable: 0)
source lines: 299126, parse-error files: 16, truncated files: 0
```

| 语言 | 文件 | 定义 | 标识符出现 | 引用 | 定义/文件 中位 | 标识符/文件 中位 | p90 | 最大 |
|---|---|---|---|---|---|---|---|---|
| rust | 744 | 10,721 | 400,919 | 390,198 | 9 | 316 | 1,279 | 6,811 |
| typescript（含 js/mjs） | 288 | 769 | 53,929 | 53,160 | 1 | 138 | 365 | 1,709 |
| svelte | 236 | 629 | 63,429 | 62,800 | 2 | 224 | 495 | 1,572 |
| python | 5 | 89 | 7,609 | 7,520 | 14 | 765 | 4,786 | 4,786 |
| **合计** | **1,273** | **12,208** | **525,886** | **513,678** | **5** | **218** | **918** | **6,811** |

### 3.2 交叉语料（补 Go/Python 的空档）

| 语料 | 文件 | 定义 | 定义/文件 | 标识符出现 | 标识符/文件（均） | 中位 |
|---|---|---|---|---|---|---|
| Go 1.27.3 `GOROOT/src` | 7,034 | 91,218 | 13.0 | 3,664,753 | 521 | 128 |
| Python 3.14.10 stdlib | 1,073 | 25,308 | 23.6 | 567,700 | 529 | 204 |

结论：Rust/Go/Python 三套独立语料的「标识符/文件」均值 521–539、本仓 413 —— **#63 的「每文件
数百个标识符」在均值上成立**；但中位数只有 128–316，单文件最大 6,811（本仓）/ 41,167（Go）。
原来那句话没错，只是没说长尾。

### 3.3 同一份数据、5 种 schema 的真实体积（`dbstat`）

把实测导出的 12,208 定义行 + 525,866 引用行导入 SQLite（脚本见设计文档附录 A；导出时行数比
§3.1 少 20 行，因为统计工具自身也在语料里，对体积结论无影响）：

| 变体 | 体积（1 commit / 1,273 文件） | 每行含索引 |
|---|---|---|
| A #63 草案（TEXT sha + refs 带 context + 4 索引） | **168.2 MiB** | refs 327 B |
| B A + `commit_id INTEGER` | **107.7 MiB** | refs 210 B |
| C B 去掉 refs 的 path 索引 | **79.8 MiB** | refs 154 B |
| D 只存定义（2 索引） | **2.61 MiB** | defs 224 B |
| E 定义 + 文件级 postings | **20.4 MiB** | postings 157 B |

原文「每行约 100–150 字节（含索引）」对定义行偏低（224 B）、对引用行严重偏低（154–327 B）。

### 3.4 查询侧（证明瓶颈是体积不是延迟）

```
SEARCH symbol_definitions USING INDEX idx_symbol_definitions_lookup (repo_id=? AND commit_id=? AND name=?)
SELECT COUNT(*) … name='Config'   -- 0.020 ms
SELECT COUNT(*) … name='db'       -- 0.236 ms（7,274 行）
```

名字扇出：7,274（`db`）/ 6,100（`assert_eq`）/ 5,995（`String`）…，
**73.9% 的引用行属于出现次数 >100 的名字**。

## 4. 关键判定

1. **不建议按 #63 原方案（定义 + 逐次出现的引用，逐 commit 全量）实施。**
   引用侧体积是定义侧的 30 倍；50k 文件时 3.1 GiB/commit，连 HEAD 都装不进主库；
   2000 文件（#63 自己的假设）单 commit 已达 125 MiB，保留 N 个 commit 只能是 N=1。
2. **建议第一步只建 `symbol_definitions`**（2.61 MiB/commit @1,273 文件），
   `commit_sha` 用 `INTEGER` 外键（省 60 MiB/commit），存 `signature`（43 B/行，`/map` 直接用）。
3. **保留策略**：默认分支 HEAD + 最近 5 个（深度 `D = clamp(⌊128 MiB / bytes_per_commit⌋, 1, 6)`；
   1k/10k 文件 D=6，50k 文件 D=1）。写入即修剪 + 每小时 `symbol_graph.cleanup`
   （沿用 `repo_events.cleanup` 的注册与循环形状）。不做「每分支 HEAD」「LRU」。
4. **引用侧替代方案**：第二步若确有需求，用**文件级 postings**（20.4 MiB/commit，比逐次出现
   便宜 3.9×）或**按需扫描**；逐次出现的引用行永不预存。
5. **诚实边界落字段**：每行 `confidence: exact | name_match`，响应级 `partial` + `limitations`
   （宏展开 / getattr / obj[key] / Go 接口分发 / Svelte markup 都只有这一种可机器读取的表达）。

## 5. 验收

```
$ scripts/check.sh --quick
==> cargo fmt --all --check
==> cargo clippy --workspace --all-targets -D warnings
    Finished `dev` profile … in 55.01s
==> checks passed in 57s (quick)
```

## 6. 偏差与未做项

| 项 | 说明 |
|---|---|
| 交叉语料不是仓库内数据 | Go/Python 标准库版本已写进文档（1.27.3 / 3.14.10），命令可重跑；未下载语料（不可复现且需网络） |
| `measure.rs` 进仓库 | `tasks/T27.md` 允许「临时脚本或 `#[ignore]` 测试」；选后者以保证可复现，代价是 `rg-core` 多一个 `#[cfg(test)]` 模块（不影响 release 产物） |
| 名称统计包含声明名 | 引用 = 标识符出现 − 定义数，已在设计文档 §1.1 写清口径；若按「同文件去重」则是 119,449 条 postings |
| `cargo test -p rg-core` 未全量跑 | 只跑了 `measure_symbol_graph_corpus`（`--ignored`）与 `check.sh --quick`（fmt+clippy）；设计任务不触碰生产逻辑，协调者的 `--full` 会覆盖 |
