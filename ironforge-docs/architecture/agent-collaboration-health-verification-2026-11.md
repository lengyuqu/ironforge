# 多 Agent 协作 P2 X03：协作健康指标——实现与验证记录

> 日期：2026-11-04。范围：[P2 设计](agent-collaboration-p2-design-2026-11.md) §3（`collab_metric_events`）、§5（R13）、验收 M06–M09。
> 按[证据分层](../../CLAUDE.md)逐层陈述，**前一层不能替代后一层**；未执行的写在「未验证」。

## 1. 与设计的取舍（及原因）

| 设计 | 实际 | 原因 |
|---|---|---|
| 可选 `collab_metric_rollups` | **不建**，全部由事件实时计算 | 设计允许；少一个会过期的缓存表，M09 的“重建可重复”变成“重算可重复” |
| 事件在业务事务内写入 | 在 **ops 层**现有事务里写（`collab_task_ops` / `collab_handoff_ops` / `collab_question_ops` / `collab_takeover_ops`），**未改** core 服务文件 | 事务句柄只在 ops 层；每处钩子 3–6 行。创建任务/交接/问题原本是单条 `INSERT`，现包进事务 |
| 任务状态变化需要“旧状态” | 状态变更时，事务第一条是对该行的**空写**（`SET updated_at = updated_at … WHERE version = expected`），再读旧状态，再做真正的条件 `UPDATE` | 不能先 `SELECT`（SQLite 读后写会 `SQLITE_BUSY`，CLAUDE.md 踩坑 21）；`RETURNING` 不跨三库。仅在请求带 `status` 时多这一条写 |
| 事件时间 | 服务端 `now`，并在写入时**抬高到该任务最近事件时间**（`record()`） | 处理器先取时钟、后提交；两个竞争写可能以相反顺序入库，产生负时长。抬高后同一任务时间线单调。仍然是服务器时间 |
| 事件顺序 | 额外列 `task_version`（状态事件为变更后的任务版本），用作同时间戳的次序 | 设计的列清单未含，但“不假设 ID 顺序等于提交顺序”需要一个确定的次序；仅新表，无 ALTER |
| `source_type` / 来源标注 | 列值 `task/handoff/question/takeover/baseline_snapshot`；响应里每个指标另有 `source`（`user_content` 或 `platform_event`）与 `timing_source`（恒为 `platform_event`） | 任务状态是客户端自报：值来自 `user_content`，只有变化的**时刻**由平台记录 |
| 初始快照 | **仅显式**：`POST …/health/baseline`（仓库管理员）；不在 GET 里懒写 | GET 带写入副作用不稳妥。快照只给**没有任何事件**的对象写“于 T 观察到”（任务状态、pending 交接、open 问题），已有事件的对象不动；重复调用不新增 |
| 过期路径声明 | 返回 `status: unknown`、`reason: PATH_DECLARED_AT_NOT_RECORDED`，**值为 null**；不计入顶层 `partial` | 需要不存在的“路径声明时间”列，不用任务 `updated_at` 顶替 |
| “只用 `to` 之前的事件” | 过去窗口的结果**不会**因之后到达的事件而改变；首个事件落在 `to` 之后的任务在该 `to` 视为未知 | 稳定性优先于“事后推断”（后者会让历史数字漂移） |
| 普通成员可见 | **仅仓库管理员**（`can_admin_repo`，权限缓存在请求前失效） | 设计默认值；待决项 §9.2 未决 |

**未做**：聚合/多仓、rollup、事件保留与删除策略（设计的 180 天/365 天仅为建议，**未实施**）、导出、Web UI、MCP 工具、对用户/会话/Agent 的任何排名（设计禁止）。

## 2. 实现证据

- 迁移 `m20261104_000007_create_collab_metrics`：仅新表 `collab_metric_events`（唯一键 `(repo_id, source_type, source_id, event_type)`，到 `collab_tasks(id, repo_id)` 的复合外键、级联删除，`has_table` 守卫，`down` 删表）。回滚窗口测试 `Some(8)→Some(9)`。
- 事件类型：`task_created`、`task_status_changed`（`source_id = <task>:<新版本>`）、`task_status_observed`（基线）、`takeover`、`handoff_created/accepted/cancelled/observed_pending`、`question_opened/answered/cancelled/observed_open`。事件只含 id 与状态，**不含**标题、消息正文、密钥。
- 核心：`rg-core/src/collaboration/health.rs`，`compute()` 是对已加载事实的纯函数（区间运算可脱库测试）。
- REST（`/api/v1/repos/{owner}/{name}/collaboration`，已登记 OpenAPI，仅管理员）：`GET /health?from=&to=&metrics=`、`GET /health/tasks?metric=&from=&to=&limit=&cursor=`、`POST /health/baseline`。
- 指标（`metric_version: 1`）：
  - `blocked_tasks`：`blocked_at_to`（`to` 时刻仍 blocked）与 `entered_blocked_in_window`（窗口内进入 blocked 的任务数；另给 `entered_blocked_events`）**分开**返回。
  - `blocked_duration`：每个 blocked 区间与 `[max(from,coverage_start), to)` 的交集秒数之和；`left_censored_tasks`（区间起点是观察起点而非真实起点且落在窗口内）、`right_censored_tasks`（`to` 时仍未结束，截到 `to`）。
  - `handoff_waiting`：创建→accepted/cancelled 的交集；pending 截到 `to`；分别给 `accepted_in_window / cancelled_in_window / open_at_to`。notes 明示“接受不等于工作完成”。
  - `open_questions`：`open_at_to`、`open_wait_seconds_at_to`、`wait_seconds_in_window`、`answered/cancelled_in_window`。
  - `stale_path_claims`：unknown（见上）。
- 响应字段：`range`（UTC，`[from, to)`）、`metric_version`、`coverage_start`（该仓最早事件时间；无事件为 null）、`partial`、`computed_at`、`baseline.taken_at`、每指标 `source/timing_source/status(complete|partial|unknown)/value/reason/notes/drill_down`、`data_quality`（未知任务、矛盾事件、未观察的 pending/open、缺起点、负区间等**计数**）。
- 窗口规则：默认 `[now−7d, now)`，最长 90 天，`to` 不得在未来，`from<to`；非法返回 400，未知 `metrics` 返回 400。事件超过 20 万条返回 409 `HEALTH_EVENT_LIMIT`。
- 钻取 `health/tasks`：按任务 id 升序的键集分页，游标绑定仓库+指标+窗口（篡改或换指标返回 400）；每项含任务标题（标 `title_origin: user_content`）、当前自报状态、该任务贡献的数字、任务链接。可钻取指标：`blocked_at_to / entered_blocked / blocked_duration / handoff_waiting / open_questions / unknown_state`。

### 覆盖语义（M08 的核心）

`coverage_start` = 该仓**最早一条事件**的时间。其之前的任何东西既不算 0 也不推断：

- 窗口整体在 `coverage_start` 之前或无事件 → 所有指标 `unknown`、值 null、`partial=true`。
- 窗口起点早于 `coverage_start` → `partial`，只量**被覆盖**的部分，并给出 `covered_from`。
- 创建于功能上线前且此后没有任何事件的任务：状态未知，计入 `unknown_state_tasks`，不计入 blocked；指标 `partial`。上线前就存在的 pending 交接 / open 问题同理（`unobserved_*`）。已在上线前关闭、且早于覆盖起点的对象只计入 `*_closed_before_coverage`（不影响被覆盖的窗口）。
- 基线后：上述对象以“于 T 观察到”进入时间线，`left_censored` 标记为真，时长**从 T 起算**，绝不从“很久以前”起算；它们不算作“窗口内进入 blocked”。
- 首个事件是“离开 X”的变更（任务上线前就存在、之后第一次被改）：X 只从 `coverage_start` 起算，并标左删失。
- 事实上，**默认 7 天窗口在功能上线后的头 7 天内一定是 `partial`**（窗口起点早于覆盖起点）。这是有意的，不是缺陷；要得到 `complete`，窗口须从 `coverage_start` 起。

## 3. 验证

| 层级 | 结论 | 证据 |
|---|---|---|
| 任务验证（SQLite，真 HTTP + 真库） | ✅ | `cargo test -p rg-http --test collab_health_tests`：15 项通过（M06 ×2、M07 ×2、M08 ×2、M09 ×9） |
| 纯函数单测 | ✅ | `rg-core` `collaboration::health::tests` 16 项（交集/删失/重开/基线/缺事件/矛盾事件/窗口规则/重算可重复）；`cargo test -p rg-core --lib` 217 通过 |
| 既有协作测试回归 | ✅ | `collab_handoff/p0/rules/secret_leak/visibility/task_message` 全部通过；`collab_session_tests` 单独运行 10/10 通过（见下方偶发项） |
| 门禁（本地） | ✅ | `cargo clippy --workspace --all-targets -- -D warnings` 无告警；`rg-db --lib` 26 项（含回滚窗口测试）、`rg-db --test collab_store` 14 项、`rg-http --lib` 85 项（含 OpenAPI 守卫）、`rg-mcp` 全部通过 |
| 新库迁移 | ✅ | 空 SQLite（草稿目录，未触碰任何真实库）上 `ironforge migrate`：新增 `collab_metric_events`，表名与实体一致；唯一键/外键/两个索引在 `.schema` 中可见；重复执行 “No pending migrations” |
| 业务与事件原子 | ✅ SQLite | 预先占住下一次状态变更要写的那条事件：PATCH 返回 500，任务状态与版本**回滚**为原值 |
| 并发 | ✅ SQLite | 8 路竞争 PATCH（同一 expected_version）：1 个 200、其余 409，恰好 1 条状态事件；accept/cancel 竞争恰好 1 条终态事件；6 路同键创建问题 1 条 `question_opened`；5 路并发基线每个对象只有 1 条观察事件，无 500 |
| 重算可重复 | ✅ | 同一 `to` 连续两次计算，`metrics` 与 `data_quality` 完全相同；钻取分页 3 页恰好覆盖 5 个任务各一次 |
| 权限 | ✅ SQLite | 写者 / 只读协作者 / 陌生人对 3 个接口均 403，未登录 401，管理员（所有者与管理员协作者）200；他人私有仓 403/404 且不含任务 id；同一所有者的另一仓数字互不混入；被拒绝的基线不写事件 |
| 不排名、不泄漏 | ✅ | 响应字符串断言不含会话 id/展示名、消息正文、问题正文，也不含 `session/agent/rank/owner_session` 字样 |
| PostgreSQL / MySQL | ❌ 未验证 | 本机无这两个后端。唯一键、复合外键、条件 `UPDATE`、事务内“先写后读”的空写锁定在这两个库上**未实测**；`IRONFORGE_TEST_DATABASE_URL` 可复用同一套测试 |
| 合并后集成 / CI | ❌ 未验证 | 未推送；与其它 P2 会话的合并未做（共享注册文件的冲突需合并时核对） |
| 大数据量 | ❌ 未验证 | 计算会把 `to` 之前的全部事件载入内存（上限 20 万条后返回 409）；未做容量/耗时测量，也未实现保留期清理 |
| 浏览器 / UI / MCP | ❌ 无 | 无界面，无 MCP 工具 |

**已知偶发**：全量并行运行多个协作测试二进制时，`collab_session_tests::t02_revoked_write_access_…` 曾失败一次（其“控制断言”依赖 30 秒权限缓存仍然温热）；同一二进制单独运行 10/10 通过。该测试不涉及本次改动，未作处理，但未排除是负载下的时序问题，故如实记录。

## 4. 未覆盖的风险与待决

1. **事件与历史的一致性假设**：覆盖语义假设“功能上线后所有变更都会写事件”。直接改库（运维 SQL、导入）绕过钩子的变更不会有事件，会表现为缺事件/矛盾事件，计入 `data_quality`，但不会被修复。
2. **任务删除**：账号硬删除会级联删除任务，其事件随之消失，窗口内历史指标会变小；仓库软删除不触发级联。保留/删除策略（设计 §5、§9.2）**未定**，本次也没有清理作业。
3. **基线时间精度**：基线对象统一记录为“调用时刻”，即使对象在更早就处于该状态；这是有意的保守，代价是低估等待时间（均标左删失）。对 pending 交接/open 问题，其行上的 `created_at` 其实是服务端真实时间，理论上可更准确，但那会让 `coverage_start` 被拉到“基线之前”的历史，所以没有这么做。
4. 钻取接口每页都会重算整个窗口（O(事件数)）；未引入 P0 风格的共享短期快照。
5. 管理员权限判定取自数据库（请求前使缓存失效），多实例撤权延迟沿用现有语义，**未单独测试**多实例。
6. `to` 之后才到达的事件不会改变已过去窗口的结果（见 §1），这也意味着“首个事件晚于 `to`”的任务在该窗口里是 unknown。
