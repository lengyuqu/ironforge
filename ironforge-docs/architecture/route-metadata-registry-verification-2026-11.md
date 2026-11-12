# 路由元数据注册表（R1）：实现与验证记录（2026-11-13）

> **状态**：已实现（P0–P2 的行为等价替换 + 防漂移测试），SQLite 下本机验证，**未经 CI** ｜ **可信度**：确认（附运行证据）｜ **来源**：仓库代码 + 本次运行
> 落实 issue #14。设计与分析见 [route-metadata-registry-2026-11.md](route-metadata-registry-2026-11.md)。授权：项目负责人已批准本任务涉及的认证/权限改动（guardrails 的 CONFIRM 项）。**无新迁移，无对外接口变化。**

## 做了什么

新增 `rg_core::auth::route_meta`（`crates/rg-core/src/auth/route_meta.rs`）。每条 REST 路由在一处声明元数据，原来四张手写表改为从声明**派生**，函数签名不变：

| 原手写判定 | 现在 | 声明字段 |
|---|---|---|
| `required_pat_scope` / `repo_scope_for`（`rg-http/src/lib.rs`） | `route_meta::pat_scope_for` | `PatScope`：`repo(..)` / `user(..)` / `admin(..)`，协作路由为 `RepoCollab` |
| `path_in_allowlist` 的跨仓拒绝表（`rg-core/src/auth/pat_scope.rs`） | `route_meta::reaches_other_repos` | `other_repos(..)`（capabilities 里用 `x(..)`） |
| `ref_limited_write_allowed`（同上） | `route_meta::branch_limited_write_allowed` | `BranchLimit`：缺省 `Refused`，`.open()`，`.checked()` |
| `idempotency_supported`（`rg-http/src/idempotency.rs`） | `route_meta::idempotency_replayable` | `.idem()`（响应不含令牌/秘密才可加），缺省不支持 |

- 非协作路由：`ROUTES` 里 229 行（`/api/v1` 下全部在 `build_routes` 注册的路由，去重后）。
- 协作路由（`/repos/{owner}/{name}/collaboration/**`，82 条 / 65 个路径）：**不重复声明**，由 `capabilities::REGISTRY` 派生——一律 `RepoCollab`、不动 ref（`Open`）、不支持幂等键；只有协作组 5 条用 `x(..)` 标 `other_repos`。`Route` 新增字段 `other_repos`，`r(..)` 不变，新增 `x(..)`。
- 匹配：`{x}` 单段，`{*x}` 一段或多段；多个声明同时匹配时字面量 > 参数 > 通配（与 axum 的优先级一致）。路径必须恰好一个前导 `/`，含空段、`.`、`..` 的路径不匹配任何声明。
- `GUARDS`：一行**非路由**的 fail-closed 守卫 `branches/protection/{*rest}`。原因：`branches/protection/a/b` 不被任何静态路由匹配，会落到 `branches/{*branch}`（删分支）；旧表按名字拒绝，不加守卫就会对分支受限令牌变松。

## 设计决定（以及为什么不是“全部由路由模板推出”）

issue 评论里已有的结论经代码核实后仍成立，实现时遇到的两个事实把落点定死了：

1. **五张表不是一种东西。** `path_in_allowlist` 判的是请求体里是否引用别的仓库，`ref_limited_write_allowed` 的放行还取决于 handler 是否自己查分支——这两件事只有写 handler 的人知道，声明只是把它们**记在一处**，不能凭空推出。所以字段是人填的；能自动对的，由测试对。
2. **现有测试把“不是路由的路径”也钉死了**：`pat_scope_tests` 要求 `POST /repos/o/r/collaboration`（不是路由）需要 `repo:collab`、`POST /ci/pipelines`（不是路由）需要 `repo:write`；`path_in_allowlist` 的单测要求 `/repos/alice/demo/forked` 放行。不改断言的前提下，**未声明的路径在已知家族里必须保留家族默认**，因此运行期“未声明即 admin”对 `/repos` 等家族做不到。折中：
   - **已注册的路由永远不走家族默认**（防漂移测试强制），漏声明的新路由让测试失败，而不是悄悄继承默认；
   - 家族默认（`family_scope`）原样保留旧前缀规则，未知前缀仍是 `admin`；
   - 分支白名单与幂等键对未声明路径是**拒绝**（旧表在 `issues/**` 等处是通配放行，现在更严）。

## 行为变化

**已注册的路由：零变化**——`route_metadata_tests` 把旧的四张表原样抄成 oracle，对 `lib.rs` 里每条路由（读/写两种方法）逐项比对，全部相等。

**不是已注册路由的路径**（只会得到 404/405 的探测请求）：
- 分支受限令牌：旧表对 `collaboration/**`、`issues/**`、`contents/*`、`branches/*` 下**未注册的子路径**放行（之后仍是 404），现在 403。1129/4754 个探测路径比旧表更严；**没有一个更松**（测试断言“拒绝不会变成放行”）。
- `Idempotency-Key`：旧表对 `issues/**`、`labels/**` 等下未注册的子路径与 `//repos/...`（双前导斜杠）接受，现在 400。
- PAT scope 与跨仓拒绝：与旧表在全部探测路径上完全相等（含 `/reposfoo`、`/api-docsx` 这类按前缀匹配的怪路径，因为家族默认保留了原代码）。

## 防漂移测试

`crates/rg-http/tests/route_metadata_tests.rs`（9 项），读 `rg-http/src/lib.rs` 源文本得到路由清单（路由器不能自我枚举，同 `collab_contract_tests.rs` 的做法）：

1. 每条注册路由都有声明、每条声明都有注册路由（双向；声明 shape 不重复）——新增路由漏声明、删路由留下声明都会失败，失败信息直接指向要加的位置；
2. 注册路由从不由家族默认回答；
3. 对每条路由，四个派生判定与旧表相等（oracle）；
4. 在 4754 个探测路径（每条路由的后缀/前缀/`..`/双斜杠/插入 `collaboration` 等变形）上：scope 与跨仓与旧表相等，分支限制与幂等**不更松**；
5. 活入口 `path_in_allowlist` / `ref_limited_write_allowed` / `idempotency_supported` 与 oracle 一致；
6. 具体性：`branches/protection` 拒绝、`branches/agent/x` 走 handler 自检、`/repos/templates/labels` 是静态路由；
7. **`.checked()` 路由的 handler 必须真的调用 `ensure_token_may_write_branch`**（解析路由表达式里的 `api::module::fn`，读对应源码函数体）——这是 §7 里设计提出的安全门；
8. 路由清单提取自检（>250 条、含若干已知路由、`.route(` 个数与字面量路径个数一致）。

**变异检查（本机实跑，改完已还原）**：① 在 `lib.rs` 注册 `/repos/{owner}/{name}/zz-new-route` 而不声明 → `every_registered_route_is_declared…` 与 `a_registered_route_is_never_answered_from_the_family_defaults` 失败，信息点名该路由；② 给 `merge-queue` 声明 `.checked()` → handler 自检测试失败（`api::pulls::list_merge_queue does not call ensure_token_may_write_branch`）及 oracle 比对失败；③ 删掉 `milestones` 的声明 → 双向对齐、oracle、活入口测试失败。

## 没做 / 做不到

- **OpenAPI `x-ironforge-*` 扩展字段**：未做（对外契约变更，设计里列在 L2；注册表已含派生所需的全部字段，后续可从 `lookup` 注入）。
- **跨仓引用的请求体字段对齐**（设计 §7 第二条）：未做。`other_repos` 是否真对应 handler 里的字段（`ForkRequest.org`、`TransferRequest.new_owner`、协作组 `target_repo_id`）只能靠人；新增带跨仓引用的路由仍须手工选 `other_repos(..)` / `x(..)`。`reaches_other_repos` 额外保留了 `fork`/`forks`/`transfer`/`collaboration/groups` 四个家族的**永久通配拒绝**，所以这四个家族下以后新增的路由即使漏标也被拒。
- **运行期“未声明即拒绝”对 scope 不成立**（见设计决定 2）：`/repos`/`/orgs`/`/ci`/`/artifacts`/`/imports`/`/search`/`/ai`/`/ws` 下不是已注册路由的路径仍按家族默认判 scope。要改成 admin 需要改 `pat_scope_tests` 里钉死的断言，属于对外行为变化，**没做**，等项目负责人决定。
- **声明与 handler 的一致性**：`.open()` 是否真的不动 ref、`.idem()` 响应是否真无秘密，静态检查查不出；只有 `.checked()` 有上述 handler 调用测试。
- **按方法的声明**：所有声明以路径为单位（旧表也是），不区分方法；读/写由调用方按 HTTP 方法判定。
- **R14-4（`ref_limited_write_allowed` 不认 `/ai/repos/**`）**：保持不变（仍拒绝，`/ai/repos/.../index` 声明为 `Refused`）。

## 观察到的既有行为（未改）

`GET /api/v1/collaboration/capabilities`（全局能力发现，不在仓库下）按旧前缀表落到“未知前缀 → `admin`”，所以 PAT 访问它需要 `admin` scope；仓库级 `…/collaboration/capabilities` 只需 `repo:read`。注册表里把它声明为 `admin(..)` 以保持原样；若认为这是缺陷，改这一行即可（对外行为变化，未擅自改）。

## 与 `rg-http/src/lib.rs` 的冲突面

只动两处：`normalized_api_path` / `required_pat_scope`（约 2057–2095 行，原 `repo_scope_for` 整体删除，函数体换成调用 `route_meta::pat_scope_for`，并挪正了 doc 注释顺序）。**没有碰** `build_routes`、`pat_to_bearer_jwt`、`spawn_post_push_hooks` 及 push 事件载荷附近。同时改动该文件其他区间不会冲突；但**新增路由**的会话必须同时在 `route_meta::ROUTES`（协作路由在 `capabilities::REGISTRY`）加声明，否则 `route_metadata_tests` 失败。

## 验证

环境：本机 `Darwin`，SQLite；**未经 CI**；PostgreSQL/MySQL **未跑**（本次不涉及 SQL 与迁移）。

| 命令 | 结果 |
|---|---|
| `cargo clippy --workspace --all-targets -- -D warnings` | 通过 |
| `cargo test -p rg-core` | 278 passed（最终提交后复跑，见下） |
| `cargo test -p rg-http`（全量，118 个测试二进制，含上面的集成测试） | **748 passed / 0 failed**（phase 2 提交上，`EXIT=0`） |
| `cargo test -p rg-http --lib`（含 `pat_scope_tests`、`idempotency` 单测） | 102 passed |
| `cargo test -p rg-http --test route_metadata_tests` | 9 passed |
| `pat_scope_ladder_tests` / `collab_contract_tests` / `idempotency_tests` / `pat_ref_allowlist_tests` / `pat_repo_allowlist_tests` / `pat_api_tests` / `branch_delete_tests` / `branch_protection_contents_tests` / `batch_commit_tests` / `openapi_*_tests` / `agent_principal_tests` / `mcp_remote_tests` | 全部通过，**断言未改** |
