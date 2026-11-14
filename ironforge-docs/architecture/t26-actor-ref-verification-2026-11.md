# T26 API 响应携带主体类型：验证记录（2026-11）

> **状态**：已实现，在 `claude/t26-actor-ref-6cb784` 分支，未 push、未开 PR ｜ **可信度**：确认（本机命令输出） ｜ **来源**：仓库代码、本机测试 ｜ **验证范围**：仅 SQLite，**未经 CI**；PostgreSQL / MySQL 未跑

对应 CNB issue #37。设计卡片：`agent-principals-design-and-roadmap-2026-11.md` §4 T26。

## 做了什么

**`ActorRef`**（`rg-core/src/agent.rs`，追加在 T24 块之后）：

```json
{ "id": 7, "username": "alice-agent-ci", "kind": "agent",
  "owner_id": 3, "owner_username": "alice" }
```

平铺，不嵌套。人类的 `owner_id`/`owner_username` 为 `null`；agent 的 `owner_id` 取 `users.owner_user_id`，`owner_username` 只在所有者行已不存在时为 `null`。`resolve_actors(db, ids)` 批量解析：去重后一次 `IN (...)` 查账号，再一次查这些 agent 的所有者，**至多两条查询**，与行数无关（新增 `user_ops::find_by_ids`，每 500 个 id 一块；无迁移、无新表）。不存在的 id（账号已被删除）不在结果里，调用方渲染 `null`。

**落地的四处**（新键，旧字段一个不动）：

| 位置 | 新键 | 做法 |
|---|---|---|
| `GET …/pulls/{n}` | `author_ref` | `pulls.rs` 详情里 `body["author_ref"] = …`，与 fork 的 `head_repo_*` 后置补充同一写法 |
| `GET …/pulls` | 每行 `author_ref` | `api/actor_ref.rs::annotate`：把 `Vec<Model>` 包成 `Vec<WithActorRef<Model>>`（`#[serde(flatten)]` 实体 + 一个键），整页一次批量解析 |
| `GET …/pulls/{n}/reviews` | 每行 `reviewer_ref` | 同上；返回仍是裸数组 |
| issue 评论（`CommentResponse`，列表与创建/更新响应） | `author_ref` | 结构体加字段；列表一次批量解析 |
| `GET /admin/audit/logs`、`/admin/audit/logs/{id}` | 每行 `user_ref` | `AuditLogEntry` 加字段，保留 `user_id`、`username`、`on_behalf_of_user_id` |

**为什么是 `*_ref` 新键而不是 `author`/`reviewer`/`user`**（issue 复核评论指出）：前端 `PullRequest.author`、`IssueComment.author` 已被声明为 `string | null`（`web/src/lib/types/entities.ts`），并在 `PrHeader.svelte`、`PullList.svelte` 渲染；把它改成对象会让现有前端类型冲突。命名规则：`<id 字段去掉 _id>_ref`（`author_id`→`author_ref`，`reviewer_id`→`reviewer_ref`，`user_id`→`user_ref`）。设计文档原文的 `author`/`reviewer`/`user` 因此**没有**采用。

**取舍：PR 列表/详情不引入大 DTO。** 详情沿用已有的 `json!(pr)` 后补一个键；列表用泛型小包装 `WithActorRef<T>`（一个 `flatten` 的实体加一个 `HashMap<&str, Option<ActorRef>>`），不新写与实体逐字段对应的结构体，所以实体新增字段时不会漏掉。代价：列表元素类型不再是实体本身，OpenAPI 里列表响应本来就是 `serde_json::Value`，无变化。创建 PR 的响应（`POST …/pulls`）**未**加。

**OpenAPI**：rg-core 不依赖 utoipa，故在 `api/actor_ref.rs` 放一个仅文档用的 `ActorRefSchema`（`ToSchema`），登记进 `openapi.rs` 的 `components.schemas`，`AuditLogEntry.user_ref` 用 `#[schema(value_type = Option<ActorRefSchema>)]` 引用；`actor_ref.rs` 的单测钉死镜像与真实 `ActorRef` 的序列化键一致。PR、评审、评论三处的响应在 OpenAPI 里本来就是 `serde_json::Value`/无 schema，未改，也就没有给它们的 `*_ref` 写文档化 schema（见「未做」）。

**能力发现**：`GET /api/v1/collaboration/capabilities` 的 `api_capabilities` 增加 `actors.ref.v1`（`collab_capabilities.rs`，一行）。**没有**往 `capabilities::REGISTRY` 加 `actors.identity`：`collab_contract_tests` 的采集器只收路径含 `collaboration` 的路由，四组非协作路由会让 `stale` 断言变红；防漂移测试口径未改。

**`CONTRACT_MINOR` 的决定：不 bump（保持 1.1）。** 理由：(1) `contract_version` 是协作契约的版本（注册表里的能力与路由），本次没有改任何协作路由、能力、请求或响应；(2) 本仓此前通过 `api_capabilities` 旗标宣告同类增量（如 `repo.settings.patch.v1`，提交 `f082426`）也没有 bump；(3) 契约文档写明高 minor 只表示"新增可选内容"，而这里的新键对消费者本来就是可选、可忽略的，按旗标判断即可，bump 反而会让客户端把"契约变了"与"协作面变了"混为一谈。设计卡片 §4 的"`CONTRACT_MINOR` 1 → 2"与路线图 B2 行的"并 bump `CONTRACT_MINOR`"因此**不照做**；如果项目负责人想把它视为契约增量，只需改 `capabilities.rs:31` 一个常量并同步 `collab_contract_tests` 里写死版本的断言（本次未碰）。

## 测试

新增：

- `rg-core` 单元测试（`agent::actor_ref_tests`，3 项）：人类无所有者/agent 带所有者名与 id；未知 id 缺席、所有者已删时保留 `owner_id` 而 `owner_username` 为 `null`；3 个所有者 + 30 个 agent + 重复 id 共 33 个账号，用 sea-orm 的 `set_metric_callback` 数到**恰好 2 条查询**。
- `rg-http/tests/actor_ref_tests.rs`（8 项，SQLite）：PR 详情（人类作者、agent 作者带所有者；旧键 `author_id`/`title` 仍在，且**没有**新增 `author` 键）、PR 列表（同一页人类+agent）、PR 作者账号被删（`author_ref` 键存在且为 `null`，请求仍 200）、评审列表（人类与 agent 评审人）、issue 评论（列表与创建响应；`author` 仍是用户名字符串）、审计行（人类 / agent / 无用户 / 已删用户 / 单条端点，旧字段仍在）、`actors.ref.v1` 旗标、OpenAPI 含 `ActorRefSchema` 与 `AuditLogEntry.user_ref`。
- `rg-http` 单元测试 `actor_ref::tests`（1 项）：OpenAPI 镜像字段与真实类型一致。

**去掉实现应失败**：把 `audit.rs`、`issues.rs`、`pulls.rs`、`reviews.rs`、`collab_capabilities.rs` 还原到改动前、保留测试后实跑：8 项中 7 项失败。第 8 项（账号被删时 `author_ref` 为空）原先断言 `is_null()`，在键缺失时也会通过，已改为断言键存在且为 `null`。

## 命令与结果（本机，未经 CI，仅 SQLite）

| 命令 | 结果 |
|---|---|
| `cargo clippy --workspace --all-targets -- -D warnings` | 干净 |
| `cargo test -p rg-core` | 285 passed / 0 failed / 6 ignored |
| `cargo test -p rg-db` | 77 passed / 0 failed / 1 ignored |
| `cargo test -p rg-http` | 780 passed / 0 failed / 2 ignored，含 `actor_ref_tests`（8）、`collab_contract_tests`、`route_metadata_tests`、`openapi_registration_tests`、`openapi_docs_auth_tests`、`agent_principal_tests`、`pat_scope_ladder_tests` 全绿 |

说明：第一次后台运行被打断，`rg-http` 的结果取自随后完整跑完的一次（`EXIT 0`），`rg-core`、`rg-db` 随后重跑；三者都在最终代码上。`route_metadata_tests` 无需改动：本次没有新增路由。

## 没做 / 未决

- **未做**：PostgreSQL / MySQL（本次新增的唯一 SQL 是 `users.id IN (...)`，无 `ON CONFLICT`、无迁移；新测试用 `spawn_test_app_with_db`，是 SQLite 专用，也不在 `scripts/ci-db-tests.sh` 名单里）。
- **未做**：`POST …/pulls`（创建）、评审的 `timeline`、review（代码行）评论列表（`ReviewComment`，字段 `author_id`）、issue 列表/详情的作者、`/users/me` 与 `/admin/users` 的 `kind`，均按任务范围不铺开。
- **未做**：PR、评审、评论三处响应在 OpenAPI 中仍是无类型 `Value`/无 schema，所以 `*_ref` 只在审计行上有文档化 schema；给它们配具名响应 DTO 是更大的改动。
- **未做**：前端未改，`web/src/lib/types/entities.ts` 没有声明新键；现有前端不读新键，不受影响。要让 UI 显示 agent 标记，需要另行读取 `author_ref`。
- **未决（取舍）**：解析失败（数据库错误）时与账号已删一样渲染 `null`，只记 debug 日志——与 PR 详情里 fork 信息的既有 fail-soft 规则一致，但客户端无法区分两种 `null`。
- **未决（审计）**：`user_ref` 来自**当前**账号行，`username` 是写入时冗余的名字；账号改名后两者不同。`user_ref.kind` 反映的是现在的账号类型，不是事件发生时的。
- **未决（折叠到后续）**：设计卡片 §4 T26 与路线图 B2 行里"bump `CONTRACT_MINOR`"的表述应更新为本文的决定；这两份文档按任务约定未改。
