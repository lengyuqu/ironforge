# 多 Agent 协作 P1 U02：决定追溯 实现与验证记录

> 日期：2026-11-04。范围：[P1 设计](agent-collaboration-p1-design-2026-11.md) §2（`collab_decisions`、`collab_decision_changes`）、§7（R11）、验收 N04–N05。
> 按[证据分层](../../CLAUDE.md)逐层陈述，**前一层不能替代后一层**。未执行的写在「未验证」。

## 1. 与设计的取舍（及原因）

| 设计 | 实际 | 原因 |
|---|---|---|
| 请求字段 `client_request_id` | `client_decision_id`（创建）/ `client_change_id`（变更），各自带 `payload_hash`，唯一键 `(actor_kind, actor_id, client_*_id)` | 沿用 P0 的“每对象自带幂等键”约定；同键同体回放 200，异体 409 `IDEMPOTENCY_KEY_REUSED`；回放检查先于前置条件 |
| 状态变更携带期望版本 | 决定**没有版本字段**；终结事件由 `collab_decision_changes.decision_id` 唯一键仲裁，输家得 409 `DECISION_ALREADY_CHANGED`（`details` 给出赢家的 `change_id/kind/replacement_decision_id`） | 决定不可变，没有可锁的状态行；单条 `INSERT` 即可裁决，不需要事务（避开 SQLite 读后写 `BUSY`，见 CLAUDE.md 踩坑 #21） |
| “替代环”由事务内检查 | **用序号排序让环不可能出现**：新增列 `collab_decisions.seq`（每任务 1,2,3…，`(task_id, seq)` 唯一），替代决定的 `seq` 必须**大于**被替代者。每条边严格向前，不论并发如何交错都无环 | 环检查需要跨行锁，PostgreSQL/MySQL 的行为与 SQLite 不同且本机无法验证；排序规则只读不可变字段，与数据库后端无关。**代价**：比设计更严——用较早的决定去“替代”较晚的会被拒（`REPLACEMENT_NOT_NEWER`），即使并无环。真正成环时报 `DECISION_CYCLE`（沿 superseded 链走，上限 1000 步，超出 fail-closed 为 `DECISION_CHAIN_TOO_LONG`） |
| 排序键 | 用 `seq` 而不是 `created_at` | MySQL `timestamp` 为秒精度，同秒两条决定的 `(created_at, id)` 顺序由随机 UUID 决定，会让“更晚”失去意义。`seq` 创建时 `max+1`，撞唯一键重试（与工作包 `revision` 同做法） |
| 列表游标 | 沿用共享 `cursor.rs`，把 `seq` 编成 `KeysetPosition.at`（epoch 秒）；scope 为 `decisions:v1:…:order=seq_asc` | 不改共享文件；游标仍带 HMAC 与 scope 校验 |
| 答复与问题的归属 | `answer_id` 须配 `question_id`；问题须属于本任务、本仓库；答复须是该问题的答复。**答复属于该问题**由服务层校验，数据库仅有 `answer_id → collab_answers(id)` 外键 | `collab_answers` 只有 `id` 可被引用（唯一键在 `question_id`），不得改动既有表；答复不可变且每问题至多一个，校验不会被事后破坏 |
| 终结事件的“同任务” | 数据库层：复合外键 `(decision_id, task_id, repo_id)` 与 `(replacement_decision_id, task_id, repo_id)` 均指向 `collab_decisions(id, task_id, repo_id)` | 即使服务层有缺陷，也无法跨任务/跨仓库替代 |
| 任务版本 | `context.task_version` 缺省取当前版本；给出且不等于当前版本 → 409 `VERSION_CONFLICT`（`details.current_version`） | 与问题创建一致 |
| 新鲜度 | 直接复用 `question::freshness`（任务版本不同 → stale；有 `head_sha` 时需任务声明了分支才能判断，否则 unknown） | 设计要求“与 P0 问答的检查规则一致” |
| 谁能追加终结事件 | 任何有写权限的主体（会话或用户门），事件记录 `actor_kind/actor_id` | 设计未限定创建者；保守起见不另加限制，来源可追溯 |

**不做**：决定草稿（LLM）、把决定接入分支保护/合并（明确禁止）、Web UI、上下文包（U05）里的决定节。

## 2. 实现证据（文件 / 接口）

- 迁移 `m20261104_000004_create_collab_decisions`（只建新表 `collab_decisions`、`collab_decision_changes`；`has_table` 守卫；`down` 先删子表；复合外键；表名与实体一致）。已登记到 `migrations/mod.rs`，`rg-db/src/lib.rs` 的回滚窗口 `Some(8)` → `Some(9)`（该窗口随各会话迁移合并后需由合并方再核对）。
- 实体 `collab_decision`、`collab_decision_change`；DB 操作 `rg-db/src/ops/collab_decision_ops.rs`；服务 `rg-core/src/collaboration/decision.rs`。
- REST（前缀 `/api/v1/repos/{owner}/{name}/collaboration`，均已登记 OpenAPI，文案声明“决定不是审批、不授予 Git/CI/合并权”）：`POST|GET /tasks/{id}/decisions`、`GET /decisions/{id}`、`POST /decisions/{id}/changes`。
- 稳定错误码：`DECISION_ALREADY_CHANGED`、`DECISION_CYCLE`、`REPLACEMENT_NOT_NEWER`、`DECISION_CHAIN_TOO_LONG`（均带 `error.details`，其中前三个已被测试断言），沿用 `VERSION_CONFLICT`、`IDEMPOTENCY_KEY_REUSED`。链接错配、自引用、跨任务替代为 400。
- MCP：`collab_create_decision / collab_list_decisions / collab_get_decision / collab_change_decision`（`TOOL_NAMES` 28 → 32）；`.ai/README.md` 已补。
- 读权限沿用 `require_collab_read`（所有者/协作者/组织成员/写者），写权限 `require_collab_write` + `write_actor`；所有客户端文本带 `origin: user_content`。

## 3. 验证

| 层级 | 结论 | 证据 |
|---|---|---|
| 任务验证（SQLite，真实 HTTP） | ✅ | `cargo test -p rg-http --test collab_decision_tests`：11 项通过（连续 5 次运行均通过） |
| N04 | ✅ SQLite | 合法的 问题+答复 链接成功并可追溯（会话/用户两扇门、`head_sha`、`origin`）；7 类错配（他问题的答复、他任务的问题、答复无问题、未知/非法 id、他仓库的问题、未答复问题配答复）均 400 且**不产生记录**；任务版本过期 409 `VERSION_CONFLICT`、未知 commit 400、空标题 400；同键同体回放 200（即使任务版本已变）、异体 409；陌生人 403 / 只读协作者可读不可写 / 匿名 401 |
| N05 | ✅ SQLite | 3 路并发 撤销/撤销/替代 × 5 轮：每轮恰 1 个 201，其余 409 `DECISION_ALREADY_CHANGED` 且 `details` 指向赢家；`x→y` 与 `y→x` 并发 × 5 轮：恒为 `x→y` 成功、反向被拒；3 节点链上闭环报 `DECISION_CYCLE`、无环但较早报 `REPLACEMENT_NOT_NEWER`；他任务决定、自引用、缺/多余替代字段均 400；历史保留（原文/上下文/创建时间不变、替代者的 `supersedes`、列表同时显示两条及事件）；新鲜度 fresh / stale（任务版本变或分支前进）/ unknown（无声明分支）正确区分，且 stale 决定保持 `active`、不被自动撤销 |
| 并发创建 | ✅ SQLite | 6 路并发创建得 `sequence` 1..6 无重复；分页（`limit=4`）按序无缺无重；同键 4 路竞争只产生 1 条 |
| MCP | ✅ | `rg-mcp` 全部测试通过（含新增 `u02_*`：路径/方法/会话头/幂等键生成/不泄密、坏参数不发请求） |
| 门禁（本地） | ✅ | `cargo clippy --workspace --all-targets -- -D warnings` 无告警；`cargo test -p rg-db --lib` 26 通过（含迁移回滚窗口测试）；`cargo test -p rg-http --lib` 85 通过（含 OpenAPI 守卫）；`cargo test -p rg-mcp` 全部通过 |
| 合并后集成 / CI | ❌ 未验证 | 未推送，未并入其他会话的改动；`TOOL_NAMES`、`openapi.rs`、`lib.rs` 路由、迁移回滚窗口是多会话共同编辑点，合并后需重跑 |
| PostgreSQL / MySQL | ❌ 未验证 | 本机没有这两种数据库。迁移（复合外键、`(task_id, seq)` 与 `decision_id` 唯一键）和竞争行为只在 SQLite 上实测。设计上刻意不依赖行锁：终结事件靠单条 `INSERT` + 唯一键，成环靠 `seq` 排序；`IRONFORGE_TEST_DATABASE_URL` 可复用同一套测试 |
| 真实运行（双客户端、真实仓库） | ❌ 未验证 | N14 属 U06；未接入 `scripts/collab-e2e.py` 场景 |
| 浏览器 / UI | ❌ 未验证 | 无 UI |

## 4. 被验证的对象

分支 `claude/collab-decisions-f7b655`，基于 `4c933d4` 加本次提交；macOS arm64，SQLite；未推送、未与其他会话合并。未运行全量 `cargo test --workspace`，也未执行 `scripts/collab-e2e.py`。

## 5. 待决 / 已知限制

- **替代必须更晚**比设计严格：想把“先写下的最终决定”挂成“稍后补记的旧决定”的替代者，需要按时间顺序补记，而不是反向。若产品认为过严，可改为在事务内加锁检测环——但要先在 PostgreSQL/MySQL 上实测锁行为。
- 决定被删除的路径与其他协作表相同（任务/仓库硬删除才级联；仓库软删除不会）。
- 终结事件**没有**“撤销撤销”：一旦 `revoked`/`superseded` 就是终局；要恢复需新建决定。
- 任何有写权限的主体都能撤销他人的决定（事件里留有 `actor`）；是否限制为创建者或用户门，需产品决定。
- 校验“答复属于该问题”依赖服务层；若将来出现绕过服务层的写入路径，需要给 `collab_answers` 增加 `(id, question_id)` 唯一键后改为复合外键（需要对既有表的新迁移，当前授权范围外）。
