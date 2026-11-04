# 多 Agent 协作 P1 U01 任务依赖：实现与验证记录

> 日期：2026-11-04。范围：[P1 设计](agent-collaboration-p1-design-2026-11.md) §2（`collab_dependency_graphs`、`collab_task_dependencies`）、§3（R06）与验收 N01–N03。
> 按[证据分层](../../CLAUDE.md)逐层陈述，**前一层不能替代后一层**。未执行的写在「未验证」。

## 1. 与设计的取舍（及原因）

| 设计 | 实际 | 原因 |
|---|---|---|
| 边表仅 `(task_id, depends_on_task_id)` 唯一 | 额外存 `client_dependency_id` + `payload_hash` + `graph_version`，唯一键 `(created_by_kind, created_by_id, client_dependency_id)` | 沿用 P0 的“每对象自带幂等键”，不建全局去重表；`graph_version` 记录该边产生的图版本，便于回放时解释 |
| 增删“携带 `client_request_id`” | **添加**按对象幂等：同键同体 200 回放，同键异体 409 `IDEMPOTENCY_KEY_REUSED`。**删除**不落库，幂等靠状态：边不存在即 `200 {removed:false}`，且先于版本比较 | 删除后没有对象可挂幂等键，新增表超出本任务范围。重试删除不会误删“被重新添加的边”：重新添加必然推进图版本，重试带的旧版本得 `VERSION_CONFLICT`。`client_request_id` 对删除只做格式校验 |
| 图版本行的创建 | 无行读作版本 0；首条边以 `expected_graph_version=0` 插入版本行（置 1）；并发的第二个 0 因主键冲突得 `VERSION_CONFLICT` | 不用 `ON DELETE/ON CONFLICT DO NOTHING`（MySQL 不可用，CLAUDE.md 踩坑 #19） |
| 100 条上限的错误码 | 与节点/边上限共用 `GRAPH_LIMIT_EXCEEDED`，`error.details = {limit: direct_dependencies\|nodes\|edges, max}` | 客户端只需分支一个码；细分在 `details` |
| 重复边 | 409 `DEPENDENCY_EXISTS`（设计未给码）；同键同体的重复请求则是回放 | 稳定机器码，不让客户端解析消息 |
| 自依赖 | 400；跨仓或不存在的依赖目标统一 404（消息相同） | 不泄漏其他仓库任务是否存在 |
| 依赖变更是否递增任务 `version` | **不**递增，也不改任务 | 依赖只是声明；等待方任务不应因此“被修改”，N01 断言等待方前后完全一致 |
| 遍历上限的配置 | `GraphLimits{1000,5000,100}` 是存储层参数，REST 固定用默认值 | 既可用真实默认值走 HTTP 测，也可在存储层测试收紧参数 |
| 完成提醒（notices） | **未实现** | 任务明确排除；`GET` 返回 `blocked_by`，通知发射器之后可直接用它 |

## 2. 实现证据（文件 / 接口）

- 迁移 `m20261104_000003_create_collab_dependencies`：只建新表 `collab_dependency_graphs`（`repo_id` 主键，FK→`repositories`）与 `collab_task_dependencies`（主键 `(task_id, depends_on_task_id)`；`(task_id, repo_id)` 与 `(depends_on_task_id, repo_id)` 两个复合外键→`collab_tasks(id, repo_id)`，**数据库层面**禁止跨仓边）；`has_table` 守卫；`down` 先删边表再删图表；`rg-db` 回滚窗口 `8 → 9`。
- `rg-db`：实体 `collab_dependency_graph`、`collab_task_dependency`；`ops/collab_dependency_ops.rs`（`add_edge`、`remove_edge`、有界 BFS、`graph_version`）。
- `rg-core/src/collaboration/dependency.rs`：校验、幂等、错误映射、`list`（`blocked_by` 派生）。
- REST（均已登记 OpenAPI）：`GET|POST /api/v1/repos/{owner}/{name}/collaboration/tasks/{id}/dependencies`，`DELETE …/dependencies/{depends_on_id}`（JSON 体 `{client_request_id, expected_graph_version}`）。读用 `require_collab_read`，写用 `require_collab_write` + `write_actor`（带会话头则校验会话，边记 `created_by_kind=session`，否则 `user`）。
- MCP：`collab_get_dependencies`、`collab_add_dependency`、`collab_remove_dependency`（共 31 个 `collab_*`），写类工具需会话，缺幂等键自动生成。

### 事务与并发（核心）

`add_edge` 在事务内按此顺序：①**第一条语句**是写：对图版本行 `UPDATE … SET version=version+1 WHERE repo_id AND version=expected`（或首条边时插入版本行）；②数 `task_id` 的直接依赖；③有界 BFS 判断“依赖目标能否经依赖边到达本任务”；④插边；⑤提交。任一拒绝都回滚，版本递增随之撤销，因此失败不留痕。回放检查与“边已存在”检查放在事务**之前**（CLAUDE.md 踩坑 #21）。

同一仓库的所有边变更因此被版本行串行化：两个携带同一 `expected_graph_version` 的请求只有一个能过条件 `UPDATE`，另一个得 `VERSION_CONFLICT`；携带最新版本的请求则在已提交图上做环检查。这是“并发 A→B 与 B→A 不会同时成功”的依据，不依赖进程内锁。

## 3. 验证

| 层级 | 结论 | 证据 |
|---|---|---|
| 任务验证（SQLite） | ✅ | `cargo test -p rg-http --test collab_dependency_tests`：14 项（见下） |
| N01（不含提醒） | ✅ SQLite | 依赖状态为声明、`blocked_by` 随 done/reopen 重新派生、等待方任务前后完全一致；回放/异体/过期版本/重复边/自依赖/未知任务/非法输入；删除幂等与版本 |
| N02 | ✅ SQLite | 真实 HTTP + `tokio::sync::Barrier` 多线程：8 轮 A→B 与 B→A 同版本并发，每轮至多一个 201，其余 `VERSION_CONFLICT`/`DEPENDENCY_CYCLE`，终图无环（Kahn 校验）；3 元环 T0→T1→T2→T0 三客户端带重试并发：恰好 2 成功 + 1 个 `DEPENDENCY_CYCLE`（带 `cycle_path`）；链 T3→T4→T5 上 4 个回边并发全部 `DEPENDENCY_CYCLE`；4 个同键同体并发恰得 1 条边。**未使用 sleep** |
| N03 | ✅ SQLite | 单任务 100 条直接依赖上限（第 101 条 409，图版本/边数不变，删一条后可再加）；1001 节点链走默认 1000 节点上限 → `GRAPH_LIMIT_EXCEEDED(nodes)`，边数不变、版本回滚；120 节点 >5000 边 → `GRAPH_LIMIT_EXCEEDED(edges)`；恰在边界内的图仍被完整检查（成环仍被发现）；存储层收紧参数的三类上限；跨仓目标与不存在目标响应一致且不含对方标题，其余端点对他仓任务 404，**直接插入跨仓边被外键拒绝**；只读协作者可读不可写、陌生登录用户读写皆 403、撤销写权限后下一次请求即 403（读写都是）、未认证 401；图版本按仓库独立 |
| MCP | ✅ | `rg-mcp` 真二进制 + 记录式假 API：`u01_*` 2 项（路径/方法/会话头/幂等键、坏参数不发请求、带 `details` 的 409 透传），`t11` 列表断言含 3 个新工具，`every_advertised_tool_has_a_definition_and_a_plan` 通过 |
| 门禁（本地） | ✅ | `cargo clippy --workspace --all-targets -- -D warnings` 无告警；其余见 §5 |
| 合并后集成 / CI | ❌ 未验证 | 仅在本分支工作树验证；未推送、未与其他 6 个并行会话的分支合并；共享注册文件的合并冲突待父会话处理 |
| PostgreSQL / MySQL | ❌ 未验证 | 本机无这两种数据库。测试套件按 `IRONFORGE_TEST_DATABASE_URL` 设计可复用，但**从未在它们上运行**。以下假设需在这两个后端实测：①条件 `UPDATE` 行锁足以串行化同仓写者（PostgreSQL READ COMMITTED、MySQL REPEATABLE READ 下，首条语句之后才发出的 SELECT 才建立快照）；②首条边的版本行 `INSERT` 在 PostgreSQL 中唯一冲突后事务进入 aborted，代码随即回滚再读，未实测；③复合外键与 `down` 顺序 |
| 浏览器 / UI | ❌ 未做 | 无 Web UI |
| 真实双客户端（N14） | ❌ 未做 | 不在本任务范围 |

## 4. 已知限制与待决

- **图越大越慢**：每次添加都做 BFS，读最多 5000 行；不做增量维护。上限为设计建议值，需依真实负载调整。
- **版本行是仓库级热点**：同仓所有依赖写入串行，冲突客户端必须重读重试。这是设计要求（“不能只锁单个任务”），不是缺陷，但并发度有上限。
- **图版本读取一致性**：`GET` 先读版本再读边，并发修改时版本可能比边旧、不会比边新；旧版本会在写入时被拒绝，是安全方向。未使用快照事务。
- **幂等键作用域**是“操作者 + 键”而非“仓库 + 键”：同一操作者在两个仓库复用同一个键会得到 `IDEMPOTENCY_KEY_REUSED`（与接手、问题等对象一致）。
- **已 `done` 的任务**可以作为依赖，也可以被添加依赖；本实现不限制（保守：不禁止声明）。`blocked_by` 只比较声明状态 `done`。
- **没有“谁依赖我”的反向查询**与提醒；之后的通知发射器需要反向索引时，`idx_collab_task_dependencies_target` 已建。
- 删除边不留历史。若需要审计“谁在何时移除了依赖”，需要另建事件表（本任务不新增）。
- 任务本身目前没有删除接口；仓库软删除保留所有行，账号硬删除会经 `collab_tasks` 级联删除相关边。

## 5. 门禁结果（本分支工作树，SQLite，未提交前最后一次运行）

| 命令 | 结果 |
|---|---|
| `cargo clippy --workspace --all-targets -- -D warnings` | 无告警 |
| `cargo test -p rg-http --test collab_dependency_tests` | 14 通过（并发项另重复运行多轮，修复一次“同键并发请求误报 `DEPENDENCY_EXISTS`”后稳定） |
| `cargo test -p rg-db --lib` | 26 通过（含迁移回滚窗口测试 `Some(9)` 与实体/迁移表名一致性扫描） |
| `cargo test -p rg-http --lib` | 85 通过（含 OpenAPI 守卫） |
| `cargo test -p rg-http --test openapi_registration_tests` | 3 通过 |
| `cargo test -p rg-mcp` | 15 + 5 通过 |
| 协作回归：`collab_p0_tests` 19、`collab_session_tests` 10、`collab_task_message_tests` 8、`collab_handoff_tests` 8、`collab_visibility_tests` 1 | 全部通过 |

调试中发现并修复的缺陷：并发的两个**相同**请求（同键同体）中，晚到者在“按键查找”时还看不到对方的提交，随后“边已存在”检查却看到了，结果返回 409 `DEPENDENCY_EXISTS` 而不是回放。现在“边已存在”若由同一操作者、同一键创建，则按回放处理（`existing_edge`）。

未运行：全量 `cargo test --workspace`（只跑了与改动相关的目标）；PostgreSQL / MySQL；合并后的联合验证。
