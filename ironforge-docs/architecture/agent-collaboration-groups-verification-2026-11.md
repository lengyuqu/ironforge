# 跨仓协调组（P2 X02）：实现与验证记录

> 日期：2026-11-04。范围：[P2 设计](agent-collaboration-p2-design-2026-11.md) §2（身份/权限）、§3（`collab_coordination_groups` / `_links`）、§4（R12）、验收 M01–M05。
> 按[证据分层](../../CLAUDE.md)逐层陈述，**前一层不能替代后一层**。未执行的写在「未验证」。

## 1. 与设计的取舍（及原因）

| 设计 | 实际 | 原因 |
|---|---|---|
| 第一页建立 10 分钟**共享可见 ID 快照**，翻页逐项重新鉴权 | **无快照表。** 游标是 `(created_at, id)` 的 keyset 位置（HMAC，绑定账号 + 锚仓 + 组 + 排序版本），**每一页对看到的每个关联重新鉴权** | 少一张带 TTL 的共享表，天然多实例/重启可续（测试里重启服务后用旧游标继续翻页）。代价见下 |
| 已有快照“不扩容” | **游标之后**新变得可见的项（重新授权、仓库恢复、新增关联）会出现在后续页；**游标之前**的不会补；重新翻页能发现全部 | keyset 的固有语义；设计本意（不泄露、不漏判、重启可发现新内容）仍成立，但“同一次翻页内集合固定”不成立 |
| `CURSOR_EXPIRED` | 无（游标不过期；坏游标/他人游标/跨组游标一律 400 `invalid cursor`） | 没有快照就没有过期 |
| 快照上限 1000 个**可见**链接 | 每组**活跃**关联上限 1000（含看不见的），超出 409 `GROUP_LINK_LIMIT_REACHED` | 限制单次授权扫描的工作量；错误只对同时拥有锚仓+目标仓写权限的人可见（见 §4 残余） |
| link 字段 `target_task_id?, commit_sha?` | 至少要有其一（可以同时有）；无“仅仓库”关联 | 保守：仓库级关联没有业务需要，且会成为更宽的权限信号 |
| `created_by` | `created_by_user_id`（账号）+ `actor_kind/actor_id`（门：`session`/`user`）；**无外键** | 账号硬删除会级联删会话/任务（踩坑 #22）；不能让它顺带删掉别人的组 |
| 目标可见性 = 目标仓“读权限” | = 目标仓**协作读者**（所有者/协作者/组织成员/写者，即 `is_collab_member`） | 关联视图会带出目标任务的标题/状态，属协作数据；与 P0 的 `require_collab_read` 同口径。公开仓库里的普通登录用户**看不到**指向它的关联（比设计保守） |
| 审计 | `collab.group.create / link_add / link_remove`，详情只含 id | 未用 `rg_core::audit::record`（见 §5：它写死 `id = 0`，只有第一条能成功） |
| 全局幂等表 | 每个对象自带 `client_*_id` + `payload_hash`（与首版/P0 一致）；移除用 `remove_client_id` + `remove_payload_hash` 记在 link 行上 | 沿用既有约定 |

**接口**（前缀 `/api/v1/repos/{owner}/{name}/collaboration`，均已登记 OpenAPI）：`POST /groups`、`GET /groups/{group_id}`（组字段 + 第一页 `links`，`?limit=`）、`GET|POST /groups/{group_id}/links`、`DELETE /groups/{group_id}/links/{link_id}`（JSON 体 `{client_request_id, expected_group_version}`）。写请求带未知字段会被拒绝（M10 精神）。**没有“列出锚仓所有组”的接口**——组 id 丢了无法重新发现（未做）。

## 2. 实现证据

- 迁移 `m20261104_000006_create_collab_groups`：只建 `collab_coordination_groups`、`collab_coordination_links`；`has_table` 守卫，`down` 先删 links；复合外键 `(group_id, anchor_repo_id) → groups(id, anchor_repo_id)` 与 `(target_task_id, target_repo_id) → collab_tasks(id, repo_id)`（任务为 NULL 时不检查）；活跃唯一性用 `active_key`（SHA-256，移除时置 NULL，唯一索引对 NULL 不冲突，三种数据库通用）。回滚窗口 `Some(8)` → `Some(9)`。
- `rg-core/src/collaboration/group.rs`（服务 + `page_links`）、`rg-db/src/ops/collab_group_ops.rs`（条件 `UPDATE` 作为事务第一条语句；回放检查在事务之前；CAS 失败时再按幂等键检查一次，使“自己和自己的并发副本”回 200 而不是 409）。
- `rg-http/src/api/collab_groups.rs`；MCP：`collab_create_group / get_group / add_group_link / list_group_links / remove_group_link`（共 33 个 `collab_*`），工具描述里写明 title/description 的可见范围警告；`CreateGroupRequest` 字段文档与创建响应的 `notice` 同文。
- 共享文件的最小改动：`api/mod.rs`、`lib.rs`（4 条路由）、`openapi.rs`（5 项）、`entities/mod.rs`、`ops/mod.rs`、`collaboration/mod.rs`、`migrations/mod.rs`、`rg-db/lib.rs`（窗口 +1）、`api/collab.rs`（仅把 `is_collab_member` 改为 `pub(crate)`）、`CLAUDE.md` 表一行、`.ai/README.md` 三行、`rg-mcp/tests/collab_mcp.rs` 末尾追加一个测试。

## 3. 验证

| 层级 | 结论 | 证据 |
|---|---|---|
| 任务验证（SQLite，真 HTTP + 真 Git 裸仓库，多仓多账号） | ✅ | `cargo test -p rg-http --test collab_groups_tests`：9 项通过，连续 3 轮稳定 |
| 存储层（SQLite） | ✅ | `cargo test -p rg-db --test collab_groups_store`：5 项（复合外键拒绝错误锚/错误仓任务；6 路同版本只 1 个赢家；同键 6 路并发 = 1 个创建 + 5 个回放且版本只加 1；重复活跃目标回滚版本；移除后目标可重新关联、5 路并发移除仅 1 个生效） |
| 门禁（本地） | ✅ | `cargo clippy --workspace --all-targets -- -D warnings` 无告警；`cargo test -p rg-db --lib`（27，含迁移窗口与表名守卫）、`-p rg-http --lib`（85，含 OpenAPI 守卫）、`-p rg-core --lib collaboration`（25，含 group 2 项）、`-p rg-mcp`（`collab_mcp` 14 项含新增 `x02_group_tools_map_to_the_rest_api`，lib 5 项）通过；既有 `collab_p0 / visibility / task_message / rules` 回归通过 |
| PostgreSQL / MySQL | ❌ 未验证 | 本机无这两个后端；迁移（复合外键含可空列、NULL 唯一索引）、条件 `UPDATE`、唯一冲突识别未在其上实测。`collab_groups_tests` 与 `collab_groups_store` 都读 `IRONFORGE_TEST_DATABASE_URL`，可原样复跑 |
| 多实例 | ⚠️ 仅“重启” | 用 `Env::restart()`（同库新服务实例）验证旧游标可续翻；没有两个同时运行的实例、也没有共享缓存场景 |
| 合并后集成 / CI / 浏览器 / UI | ❌ 未验证 | 未推送，无 UI |

### 验收对照

- **M01**（只读者只见锚仓）：读者拿到的列表只含自己有权读的目标；响应文本中不含隐藏仓库名/任务标题/提交 SHA/任务 id/仓库 id，不含 `total`、`count`、`hidden`；有隐藏项的组与空组的响应键集合相同；读者看不到 `version`；对“隐藏目标仓”与“不存在的仓库 id”添加关联的 404 响应的 `code` 和 `message` 逐字相同；移除指向隐藏目标的关联与移除不存在的关联同为 404，且版本不变；锚仓创建响应带 `notice`。
- **M02**：8 路同键同体 = 1×201 + 7×200，同一个 link，版本只加 1；同键异体 409 `IDEMPOTENCY_KEY_REUSED`；重复目标 409 `LINK_ALREADY_EXISTS`（版本不变）；6 路不同关联同一期望版本只有 1 个成功，其余 `VERSION_CONFLICT` 且 `current_version` 正确；前置条件失败（提交不在目标仓、任务不存在）不改版本。移除同理（5 路同键、两路不同关联抢同一版本、`LINK_ALREADY_REMOVED`、移除后可重新关联）。
- **M03**：翻页中撤销 → 下一页空且 `has_more=false`；重新授权（游标之后）→ 出现；游标之前新授权的 → 本次翻页不出现、重新翻页出现；目标仓转入组织后读到新的 owner（按 id 解析）；软删除目标 → 从可见列表消失、行保留；“恢复”（本实例没有恢复接口，**用直接写库清 `deleted_at` 模拟**）→ 重新出现；锚仓软删除后组路由 404。
- **M04**：105 条看不见的链接排在 3 条可见链接之前，`limit=2` 仍返回 2 条且 `has_more=true`（扫描跨过 100 的批大小）；第二页 1 条、`has_more=false`；正好填满一页时不谎称还有更多；一个什么都看不到的读者得到空页且 `has_more=false`、响应里没有 105/108 之类的数字；他人游标/乱码/被篡改的游标 400。
- **M05**：目标仓的松散提交对象被删除后，同一链接 `commit_state` 由 `available` 变 `unavailable`，`sha` 不变；已成功请求的重试仍回放 200；新请求被拒（400）。

## 4. 已知残余与未验证

1. **`version` 是一个侧信道。** 它在每次关联变更（包括指向读者看不到的仓库的）时递增。因此只给锚仓写者返回（读者没有），但锚仓写者仍可从 `VERSION_CONFLICT.current_version` 推断“发生过变更”。不泄露内容和数量以外的东西，但**不是**零信息；若要彻底消除需把 CAS 改成按目标仓分片，超出范围。
2. **`GROUP_LINK_LIMIT_REACHED`** 反映包含不可见关联在内的总数，仅对同时有锚仓和目标仓写权限的调用者出现。
3. **鉴权与写入之间有时间窗**：权限在请求入口按库内数据判定（先 `invalidate_perm_cache_user`），但“撤权”与“写入”之间没有事务级互斥；并发撤权的那一刻仍可能让一次写入通过。**没有针对该窗口的并发测试**（设计提示“需专门验证”）。权限缓存是进程内的：本实现每个请求先使目标仓的 (仓库, 用户) 缓存失效再判定，因此不依赖 30 秒缓存，但这只在“做出判定的那个实例”里生效。
4. **时间侧信道未处理**：扫描 105 条不可见链接比空组慢；未做恒定时间处理。
5. **目标任务被账号硬删除** → 外键级联删除指向它的关联行（审计行里的 id 仍在），其他读者看到的是该关联消失。
6. 只有“提交存在”被验证（对象类型为 commit），不代表在任何分支上、通过 CI 或被审阅。
7. 创建组/关联的接口未对 Agent 会话做额外限制：与其它协作写接口一致，带会话头则以会话为 actor，否则以“用户门”写入。

## 5. 顺带发现（未改，建议另开）

`rg_core::audit::record`（`rg-core/src/audit/audit.rs`）插入时写死 `id: Set(0)`。在 SQLite 上第一条审计之后的写入都会因主键冲突而失败（只记 `tracing::error`），`audit!` 宏也走这条路径。本模块因此直接插入 `audit_log` 行（`id` 由数据库分配）。该函数是否在生产里被使用、其他数据库是否同样受影响，我没有核实。
