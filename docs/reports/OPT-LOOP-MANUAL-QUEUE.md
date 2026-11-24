# OPT-LOOP 手工 issue 队列（IronForge）

> 来源：协议 `~/Documents/mybook/Projects/CNB-NPC-LOOP/README.md` **§5.2**（v0.10，两段式消费）。本文只写 **IronForge 的队列现状与本仓规则**，动机与平台实测看协议正文。
> 执行方：`OPT-LOOP 消费者（IronForge，每小时）` automation 的**第一段**。

## 1. 当前队列（2026-11-11 20:40 快照，**会过时，勿长期当事实**）

| issue | 标题 | 评论数 | 最新评论 |
|---|---|---|---|
| #13 | `[design]` Agent 主体身份与 agent 友好化路线图 v2 | 3 | NPC 分析（`is_npc=true`） |
| #14 | 路由元数据注册表 | 1 | NPC 分析 |
| #15 | 统一事件总线 | 1 | NPC 分析 |
| #16 | 多文件原子提交 | 3 | NPC 分析（挂 PR） |
| #17 | 可合并性与冲突详情 | 1 | NPC 分析 |
| #18 | 临时 ref 与预检 CI | 1 | NPC 分析（挂 PR #20） |
| #19 | 通用审批原语 | 1 | NPC 分析 |
| #31 | `[opt-round]` batch#5 | 5 | 批次容器，**不进第一段** |

查询与判据：

```bash
cnb issues list-issues --repo lengyuqu/ironforge --state open --page-size 30
# 分组：标题含 [opt-round] → BATCH；其余 → MANUAL
cnb issues list-issue-comments --repo lengyuqu/ironforge --number <N> --sort "-created" --page-size 1
# 最新评论首行是 `## 本轮收尾` 且没有更新的人工/NPC 回复 → 跳过（防重复消费）
```

## 2. 选靶规则（顺序固定）

1. 只取**非** `[opt-round]` 的 open issue；
2. **先按 `priority` 排，再按编号升序**：`P0` > `P1` > `P2` > `P3` > 空；同级取编号最小；
3. 标题含 `[hold]` → **跳过**（本条是本仓扩展，不在协议内：给「只想讨论、不想被自动改」的 issue 一个豁免开关）；
4. 最新评论已是 `## 本轮收尾` → 跳过；
5. 本机做不了的靶子（Windows 实机、真实崩溃注入、需外部服务的）→ 跳过，并在结束语写明原因；
6. **每条 issue 每小时最多一轮**，一条做完就进第二段，不连清两条。

> **插队与豁免**（2026-11-11 20:55 补）：想让自己的 issue 先被清，设 `priority: P0`（`cnb issues update-issue --repo lengyuqu/ironforge --number <N> --priority P0`）；当前 `#13` 是 P1，其余 `#14–#19` 无 priority，所以默认顺序是 `#13 → #14 → … → #19`。不想被自动消费，标题加 `[hold]`，或在正文首行写「仅讨论，勿改动」并由人工在下一轮前加 `[hold]`。

## 3. 没有 NPC 分析时（`comment_count = 0`）

1. 先发一条 `@CodeBuddy` 首扫请求（正文写清范围与「只读、别写收尾标记」，见协议 F23）；
2. `60 s × 5` 轮询；
3. 超时用 issue body 里的信息推进本轮（不要空转）；
4. 若最新评论**已经**是本流程发的请求且仍无回复 → **不重复发**，跳过该 issue，防止每小时刷一条。

## 4. 单发轮的不变式（违反了就污染批次计数）

- 回帖首行 `## 本轮收尾`，**不带轮次号**，发在**手工 issue** 上；
- **不占批次 `C`**、不影响 `C >= 6` 的关批条件；
- 独立 worktree：`opt/issue-<N>-manual-<日期>`，与批次轮的 WT **不共用**；
- 账本记 `### 单发轮 · issue #<N> · <日期>`（形态见下）；
- **不 close issue**：单发轮只消费一轮指摘，关单看该 issue 自己的验收标准。

账本段形态（与协议 T3 对齐）：

```markdown
### 单发轮 · issue #<N> · <YYYY-MM-DD>
- 选靶：非 [opt-round]，编号最小；最新评论 <comment id>（<created_at>，NPC 分析）
- 判定：成立 a / 部分成立 b / 不成立 c
- 动作：L1 <SHA>；L2 写入 <条目号>
- 验证：BASE=<SHA>，check.sh --quick <EXIT / 耗时>
- 回帖：<comment id>（<created_at>）｜标记 `## 本轮收尾`（不占 C）
- issue 状态：保持 open
```

## 5. 与批次段的关系

| | 第一段 单发轮 | 第二段 批次循环 |
|---|---|---|
| 对象 | MANUAL 里选**一条** | 编号最大的 `[opt-round]` |
| 标记 | `## 本轮收尾` | `## 第 K 轮收尾` |
| 占 C | **不占** | 占 |
| WT | `issue-<N>-manual-<日期>` | `issue-<N>-r<K>` |
| 回帖对象 | 手工 issue | 批次 issue |

两段共用 180 min 墙钟。第一段 10–16 min，做完立刻进第二段；**顺序不可颠倒**。

生产者侧的背压闸门**只认 `[opt-round]`**：手工队列再长也不阻止开新批。这是 v0.10 的刻意取舍，不要在生产者里「顺手加一条等队列清空」。

## 6. IronForge 特有

- 这批手工 issue 多数是**设计/PR 已开**的题（#13 设计、#16/#18 挂 PR）。单发轮很可能只产出 L2，这不算失败：**必须给 `文件:行号` + 建议方案**，然后登记进 ledger 的同一张待拍板表（不要新开表）。
- `#31` 批次在跑时，第一段照样先做——手工 issue 等的正是这个空档。
