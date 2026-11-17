# T27 执行计划 — 符号图设计文档与规模实测（#63 第一步）

> 权威约束见 `tasks/T27.md`。基线 `5df821d`（`tasks/T27.md` 入库提交），分支
> `agent/t27-symbol-graph-design`，worktree `../ironforge-t27`。实际执行与验收输出见
> `docs/plans/T27-task.md`。

## 1. 要回答什么问题

issue #63 要求「先做设计文档再动手」，并在原文里给了一个**未经实测的规模假设**：

> 表规模会很大（2000 文件 × 每文件数百个标识符）。先做设计文档再动手：估算行数、定保留
> 策略（只留最近 N 个 commit？只留默认分支？）、定清理任务。

所以本任务的第一性问题不是「表怎么设计」，而是**「这个规模假设成立吗，按它设计出来的表能不能
装下」**。文档的结论必须允许推翻原方案。

## 2. 测量方法：为什么用 `#[ignore]` 测试而不是临时脚本

`tasks/T27.md` 给了两个选项（临时脚本 / `#[ignore]` 测试）。选**后者**，提交为
`crates/rg-core/src/search/outline/measure.rs`：

| | 临时脚本 | `#[ignore]` 测试（采用） |
|---|---|---|
| 可复现 | 脚本删了就没了，文档里的数字不可复查 | `cargo test … -- --ignored --nocapture` 一条命令 |
| 与代码同步 | 依赖内部 API 的脚本会漂移，且没人知道它漂了 | 编译在 clippy/test 里，`OutlineIndexer` 变了会编译失败 |
| 侵入性 | 不改 `crates/` | 改 `crates/`，但只有 `#[cfg(test)]` + `#[ignore]`，不进 release 产物、不改任何生产路径 |

边界控制：`mod.rs` 只加两行（`#[cfg(test)] mod measure;`），其余全是新文件；无 `Cargo.toml`
改动（`tree-sitter*` 已是 `rg-core` 的普通依赖）；无迁移。

## 3. 三个测量口径的取舍

1. **定义数**：直接 `Outline::item_count()`，与生产代码同一函数，无歧义。
2. **引用侧规模**：这是表规模的主要来源，必须实测。口径定为「tree-sitter AST 里的
   identifier 类节点数」，而不是正则数 token：
   - 正则会把注释、字符串、关键字算进去（本仓有大量 markdown/testdata）；
   - AST 节点数是**按名匹配的引用索引能产生的上界**（一条引用行 = 一次出现），正好对应
     #63 想估的那张表；
   - 再减去定义数（每个定义名本身占一次出现），得到「引用行」下界。
3. **每行字节**：不套用原文的「100–150 字节」，而是把导出的真实行按草案建表建索引，读
   SQLite `dbstat`。同一份数据量 5 种 schema 变体（TEXT/INTEGER sha、有无 path 索引、
   逐次出现 vs 文件级 postings、有无 references 表），让「表结构决策」也有实测支撑。

`IRONFORGE_MEASURE_DUMP_DIR` 是第二阶段加的（只为导出 TSV），默认不写任何文件。

## 4. 语料选择

本仓 `.go` = 0、`.py` = 5，单靠它无法判断「每文件数百标识符」是否语言无关。补两套真实语料：

- Go 1.27.3 `GOROOT/src`（7,034 个 .go，含 testdata 与生成代码）；
- Python 3.14.10 stdlib（1,073 个 .py）。

两者都记录版本与路径，命令可重跑；文档里明确标注「交叉语料，不是仓库内固定数据」。
不引入任何网络下载语料（不可复现）。

## 5. 文档结构（六节对应 `tasks/T27.md`）

| 节 | 内容 | 关键决策 |
|---|---|---|
| §0 | 摘要表 | 结论先行 |
| §1 | 规模实测 | 口径、复现、三套语料、**对原文假设的三处订正**、两个数据问题（16 个解析失败、4.1% 名字不是标识符） |
| §2 | 规模推算 | 5 种 schema 的实测体积 → 三档推算（含乘法过程）→ 查询延迟 |
| §3 | 保留策略 | 候选裁决表 → `D = clamp(⌊128 MiB/bytes⌋,1,6)` → 写入即修剪 + 每小时 sweep → 深历史用增量 |
| §4 | 表结构草案 | `commit_id` 外键、`name` 不规范化、存 `signature`、索引清单、名字扇出 |
| §5 | 诚实边界 | `confidence: exact \| name_match` 逐行 + `partial`/`limitations` 逐响应；四语言逐条落字段 |
| §6 | 实现拆解 | T28 定义表闭环 / T29 查询端点 / T30 引用（先立项再定形态） |
| 附录 | 复现脚本、原始输出、与 #63 的差异清单 | 可核对 |

## 6. 判定标准（写文档时自我约束）

- 结论必须二选一：**建议 / 不建议按原方案实施**，不允许「视情况而定」。
- 每个数字都要能指到「哪条命令 / 哪张表 / 哪个 `dbstat` 行」。
- 引用侧如果不可接受，必须给替代方案（文件级 postings / 按需扫描），不能只说不行。
- 受 team-lead 委托验证「默认分支 HEAD + 最近 N，N 由磁盘预算反推」：要用实测给出 N 的
  反推公式与三档结果，并说明该方向在「只存定义」与「定义+引用」两种前提下的结论差异。

## 7. 收尾

1. `ironforge-docs/architecture/symbol-graph-design-2026-11.md`（新建）
2. `ironforge-docs/README.md` 架构表 +1 行
3. 本文档与 `docs/plans/T27-task.md`
4. `tasks/T27.md` 的 `State` → `completed`（`TASKS.md` 由协调者更新）
5. `scripts/check.sh --quick`（fmt + clippy `-D warnings`，覆盖新测试模块）
6. 提交并 `git push -u cnb HEAD`（只推分支）
