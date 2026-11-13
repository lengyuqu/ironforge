# 路由元数据注册表：现状核实与分阶段落点（Proposed）

> 答复 issue #14「路由元数据注册表」。核实基线 `75d8231`（2026-11-12），全部行号由 `sed -n` / `grep -n` 逐条打印过。**2026-11-13 状态更新：R1 已落地**（`0b05502` = PR #55；phase 1 `eb1736f` + phase 2 `4d34e69`）——`required_pat_scope`、`path_in_allowlist` 的拒绝表、`ref_limited_write_allowed` 已改由 `crates/rg-core/src/auth/route_meta.rs` 派生，`repo_scope_for` 已删除，`Route` 新增 `other_repos` 字段与 `x()` 构造器；**下文 §1 / §2③ / §3 / §4 / §5 / §5.1 的行号一律是历史快照**（§1 与 §3 的**表头**都写 `75d8231`；§3 的「注册路由数」与「`required_pat_scope`」两行已就地补现状值，§4 兜底锚点同；凡行内另给 `1c0a87e` 或「R1 后」的，以那处括注为准），读现状以 `route_meta.rs:600-760` 与 `lib.rs:2079` 为准；**`repo_scope_for` 在现状已删除**（全仓仅 `rg-http/tests/route_metadata_tests.rs:41` 一句注释），表中出现该符号只能按历史快照读。
> 本文只写**核实过的事实**与**可执行的落点**；凡是改鉴权语义、改 OpenAPI 契约、加路由字段的部分一律停在 L2（见 §6），不在本流程落地。

## 0. 结论

- **方向成立，但不是一次重构。** 五张表里只有两张是「路由属性」（`idempotency_supported` 的秘密面、`pat_scope` 的读写面），另外两张的判据是**请求体内容**和**handler 里有没有写检查**——这两种事实声明不出来，只能写下来并被人复核。
- **可用的骨架比设想的窄。** `capabilities::REGISTRY` 今天只有 **27 条 capability / 82 条路由 / 65 个不同路径**，且 82 条路径**全部**在 `/collaboration/**` 下；全站是 `rg-http/src/lib.rs` **323** 处 `.route(`（全仓 `crates/` **342** 处；现状 **346**，多出的 4 处全在 R1 新增的 `rg-http/tests/route_metadata_tests.rs`）。把它升格成全站唯一清单 = 新增 **241** 条声明（323 − 82）。**计数口径订正（2026-11-12）**：`grep -c '^\s\+r('` = 75 会漏掉 7 处写成 `routes: &[r(` 的单路由能力（`capabilities.rs:271`/`:399`/`:741`/`:752`/`:763`/`:774`/`:785`），必须先括号匹配再计数；上一版 75 / 58 / 248 全由这条错口径推出。
- **「未声明即拒绝」这层已经守住了。** 三张表今天都是 fail closed（§2 第 3 条）。真正没被守住的是「声明与 handler 一致」，那是**测试**能治的，不是元数据能治的。因此本条的价值主要在「少维护几张手写表」，不在「堵一个现存漏洞」。

## 1. 五张表的真实位置

| 表 | 代码位置（本基线） | 真正的判定维度 | 能否由 method+path 派生 |
|---|---|---|---|
| `required_pat_scope` / `repo_scope_for` | `crates/rg-http/src/lib.rs:2076` / `:2137`（**R1 后：`required_pat_scope` 在 `:2079`，`repo_scope_for` 已删除**，前缀族表迁 `route_meta.rs:704-751`） | 读 vs 写、`/collaboration/**` 写走 `repo:collab` | 能（`75d8231` 前缀族表 `:2106-2127`；R1 后迁 `route_meta.rs:729-741`） |
| `path_in_allowlist` 的拒绝表 | `crates/rg-core/src/auth/pat_scope.rs:174`，拒绝模式在 `:189` | **请求体里是否引用别的仓库** | **不能**：同一条路由换个体就换结论 |
| `ref_limited_write_allowed` | 同上 `:299`（`1c0a87e` `:301`；R1 后 `:307-309` 三行仅委托），放行集合 `:312-332`（`1c0a87e` `:312-335`） | 操作会不会移动 ref **且** handler 自己查分支 | **不能**：后半句是代码事实 |
| `idempotency_supported` | `crates/rg-http/src/idempotency.rs:64` | 响应里是否含令牌/秘密 + 是否创建型 | 能（但只能人工审出「有没有秘密」） |
| `capabilities::REGISTRY` | `crates/rg-core/src/collaboration/capabilities.rs:133`，`Route` 三字段 `:84-88` | 能力分组 + `Access` 原子需求 + MCP 工具归属 | 半能（只覆盖 `/collaboration/**`） |

`REGISTRY` 的 `Route` 已经是 `method` + `path` + `access`（`:84-88`），条目用 `const fn r(...)`（`:90-96`）构造，`&'static` 表。**结论：要扩的是字段，不是从零建系统**；但扩字段要改 **82** 处构造点（现状 **77** 个 `r(...)` + **5** 个 `x(...)`；其中 7 处 `r(` 是 `routes: &[r(` 的内联写法，见 §0），不是纯增量。

## 2. 三个隐含假设的裁决

**① 「五张表都是路由元数据」——不成立。**
`path_in_allowlist` 拒绝的是**跨仓引用**，判据在请求体里，且三个字段各不相同：

- `ForkRequest.org`（`crates/rg-http/src/api/repos.rs:878-880`）
- `TransferRequest.new_owner`（`crates/rg-http/src/api/repos.rs:986`）
- 协作组的 `target_repo_id`（`crates/rg-http/src/api/collab_groups.rs:63`）

代码里的拒绝模式是 `["fork", ..] | ["forks", ..] | ["transfer", ..] | ["collaboration", "groups", ..]`（`pat_scope.rs:189`）——**四个模式、三类语义**（`forks` 是 `fork` 的复数别名，不是独立家族）。改成声明后仍需人手确认「这个 handler 的体里到底有没有别人的仓库 id」。

**② 「声明一次就能自动派生」——部分成立。**
`idempotency_supported` 的文档注释（`idempotency.rs:59-63`）本身就是判据：「creating a repository, or writing under … Everything else, in particular anything that returns a token or a secret, is out」。这一条可以派生。
但 `ref_limited_write_allowed` 放行 `contents/**`、`POST commits`、`DELETE branches/<name>`（`75d8231` `:323` / `:326` / `:332`；`1c0a87e` `:325` / `:328` / `:334`；后者排除 `branches/protection/**`）**只因为**三个 handler 里真的调了 `ensure_token_may_write_branch`：

- `crates/rg-http/src/api/repo_content.rs:675`（删分支）、`:1453`、`:1596`（后两个是现状；`1c0a87e` 为 `:1451` / `:1591`）
- `crates/rg-http/src/api/repo_commits.rs:166`（批量提交）

新增一条写路由时，「我调了没有」只有写它的人知道；声明写错时编译和静态检查都不会响。
补充（2026-11-12，行号为 `1c0a87e`）：`ref_limited_write_allowed(path: &str)`（`:301`；`75d8231` `:299`；R1 后 `:307-309` 仅委托 `route_meta::branch_limited_write_allowed`）**不看 method**，读/写的区分由调用方完成——`pat_auth_middleware` 先算 `is_read`（`crates/rg-http/src/lib.rs:2281`；现状 `:2086`）再短路；它也只接受 `["repos", …]` 族（`:309` 的 `let-else`；`75d8231` `:307`；现状在 `route_meta.rs:675`），`/ai/repos/**` 的写请求一律落到 `false`（见 §6 第 5 条）。

**③ 「未声明即拒绝能防漂移」——部分成立，且这一层今天已经在生效。**
- `path_in_allowlist`：路径不在名单内仓库下、或命中拒绝模式 → `false`（`:174-191`）。
- `ref_limited_write_allowed`：不落在 `:312-332` 的 `matches!` 里 → `false`（R1 后判定改在 `route_meta.rs` 的 `BranchLimit` 声明上，`pat_scope.rs:307-309` 只做转发）。
- `required_pat_scope`：未知前缀 → `Some("admin")`（R1 后落在 `crates/rg-core/src/auth/route_meta.rs:750`（`family_scope` 末行，原 `lib.rs:2127`），`/api-docs`、`/health`、`/metrics` 在 `:706` 先 `return None`；「Fail closed: a PAT must not reach an unrecognised route by default」）。

所以「忘记登记」的后果是**被拒**，不是被放行。真正会漏的是：某条路由**已经登记**，但它的 handler 后来改了、不再检查分支。这类漂移靠 `ref_effect` 字段也发现不了，只能靠测试把「登记的放行集合」与「实际调用 `ensure_token_may_write_branch` 的路由」对齐（§7）。

## 3. 需要订正的计数与行号

| 复核对象 | 原文说法 | 实测（基线 `75d8231`） | 判据 |
|---|---|---|---|
| capability 数 | 30 | **27** | `grep -c '^    Capability {'`（REGISTRY 段） |
| 注册路由数 | 78 | **82** | 括号匹配计数（`r(` 起点逐个匹配到闭合 `)`）；`grep -c '^\s\+r('` 只得 **75**，漏 7 处 `routes: &[r(` |
| 不同路径数 | 无 | **65** | 82 条路由按 `path` 去重；差额 17 = 17 个「同路径多方法」族。**不要用行首计数**：本基线 `grep -c '^\s\+r('` 只得 75（**现状 70**——R1 把 5 条跨仓路由改成 `x(` 构造器，`capabilities.rs:559`/`:564`/`:569`/`:574`/`:579`）⇒ 两套并列读法都等于 82：`1c0a87e` = 行首 `r(` **75** + `routes: &[r(` 内联 **7** + `x(` **0**；现状 = 行首 `r(` **70** + 内联 **7** + `x(` **5**（现状三口径去重后分别是 54 / 7 / 4 个路径，两两交集为空，合计 65）；`:159`/`:160`（现状 `:181`/`:182`）的路径仍是 `const SESSIONS`（现状 `:152`，本基线与 `1c0a87e` 为 `:130`），行内无字面量 |
| 全站 `.route()` | 298 | **342**（`rg-http/src/lib.rs` 单独 **323**；它自己的基线 `cba96ba` 上是 320；**现状 346**，+4 全来自 R1 新增的 `rg-http/tests/route_metadata_tests.rs`，生产侧仍是 323） | `grep -rn '\.route(' --include=*.rs crates/ \| wc -l` |
| 「1200 个判断点」（298×4） | 1192 | **328**（82×4）；只做 P0 是 **164**（82×2） | 同上 |
| `path_in_allowlist` 位置 | `pat_scope.rs:293` | **`:174`**，拒绝表 `:189`；`:293` 落在 `ref_limited_write_allowed` 的文档注释里 | `grep -n '^pub fn path_in_allowlist'` |
| `ref_limited_write_allowed` 位置 | `:300` | **`:299`** | `grep -n '^pub fn ref_limited_write_allowed'` |
| `required_pat_scope` / `repo_scope_for` | `lib.rs:2064` / `:2125` | **`crates/rg-http/src/lib.rs:2076`** / **`:2137`**（R1 后 `required_pat_scope` 在 `:2079`；**`repo_scope_for` 已删除**） | `grep -n 'fn required_pat_scope'`；`grep -rn 'fn repo_scope_for' crates/` = **0** |
| `idempotency_supported` | `idempotency.rs:64` | `:64` ✅ | 原文即 `pub fn idempotency_supported(path: &str) -> bool {` |
| `REGISTRY` 覆盖范围 | 只覆盖 `/collaboration/**` | ✅ 82 条路径全部含 `/collaboration` | 逐条列举 |
| 建议的 `Route` 代码块 | `／/ ↓ 新增` | 全角斜杠，**编译不过** | 复制进 `rustc` 即报错 |

「按路径去重」与「按路由计数」不是同一个数：**82 条路由对应 65 个不同路径**，差额 **17** 全部来自 17 个「同路径多方法」族（如 `tasks/{id}/messages` 上 GET + POST）。`const SESSIONS`（`crates/rg-core/src/collaboration/capabilities.rs:130`，被 `:159`/`:160` 两条 `r(...)` 复用；现状 `:152` / `:181`/`:182`）只贡献其中 **1** 条，不是主因——上一版把它写成主因，归因过窄。

## 4. 一次到位的成本

「全站唯一清单 + 未声明即拒绝」要求 `required_pat_scope` 的每一族都在表里：git、OCI、WebSocket、runners、metrics、`/health`、`/api-docs`。危险的是**缺省填真值**的半张表——漏登记一族就按那个真值放行或拒绝，与今天的 fail-closed 兜底同构（兜底是 `admin`——R1 后落在 `crates/rg-core/src/auth/route_meta.rs:750`（`family_scope` 末行，原 `lib.rs:2127`），能撞上它的是**未知前缀**；`/api-docs`、`/health`、`/metrics` 在 `route_meta.rs:706` 就先 `return None`，根本走不到兜底）；缺省填 `PatScope::Undeclared` 哨兵并回落前缀表时，半张表与今天逐字相同（§5.1 第 2 条）。

按 241 条新增路由 × 4 个字段，是 **964** 个判断点；即使只做 P0（2 个字段）也是 482 个。这不是一次 PR 的体量，也不该由一个只有文档权限的流程来改。

## 5. 分阶段落点

| 阶段 | 内容 | 消费方 | 前置条件 | 本流程是否做 |
|---|---|---|---|---|
| **P0** | `Route` 加 `pat_scope` + `response_secret`；`REGISTRY` 仍只管 `/collaboration/**` | `idempotency_supported` 改查表；`required_pat_scope` 对 `/collaboration/**` 改查表，其余族保持前缀表 | **82** 处构造点（`r(` / `x(`）补齐，**或**改用哨兵缺省回落前缀表（§5.1）；两条派生路径的等价性测试 | 否（L2） |
| **P1** | 加 `x_repo_ref`，替掉 `path_in_allowlist` 的四个拒绝模式（`:183-184` 两族共用同一张拒绝表；今天只有 `/repos/**` 族可达这四个模式——`/ai/repos/**` 的 7 条路由无 fork/transfer，连 `collaboration/groups` 也没有） | `repo:read`/`repo:write` 判定前的跨仓引用检查 | 三条请求体字段名与 `serde` 键集合对齐测试 | 否（L2） |
| **P2** | 加 `ref_effect`，替掉 `ref_limited_write_allowed` | 分支白名单收窄 | `contents`/`commits`/`branches` 三族的 handler 调用点全在案（今天 4 处，§2②）；`scripts/agent-e2e-remote.py` 的分支白名单用例回归 | 否（L2，安全关键） |
| **P3** | `Capability.routes` 派生自全站 `REGISTRY`，打开「未声明即拒绝」 | 全站 PAT scope | 241 条路由全部登记 | 否（L2） |

### 5.1 P0 的两个前置约束（2026-11-12 补，待拍板）

1. **`pat_scope` 不能从 `Access` 派生。** `Access::atoms()`（`capabilities.rs:68-79`）是能力原子（`repo_collab_read` / `repo_write` / `session_credentials` …），而 `required_pat_scope` 的读写判据是 **HTTP method**（`crates/rg-http/src/lib.rs:2135` 原话「"Read" is decided by the HTTP method alone, so a `POST` that only reads is treated as a write」）。两套判据今天已经分叉，实例是两条**读语义但非 GET** 的协作路由：`PUT /repos/{owner}/{name}/collaboration/inbox/read`（`capabilities.rs:382`，`CollabReadSession`）与 `POST /repos/{owner}/{name}/collaboration/notices/{id}/ack`（`:701`，同为 `CollabReadSession`）——按 method 判据它们是写，按 `Access` 是读。⇒ `PatScope::from_access(access)` 会产出与今天不同的值，**必须逐条显式标注**。
2. **缺省值只能是「未声明」哨兵，不能是任何真值。** 82 处构造点可以一处不改：给 `r()` / `x()` 的缺省填 `PatScope::Undeclared`，`required_pat_scope` 查表遇到 `Undeclared` 时回落现有前缀表（`:2106-2118` 的 `REPO_SCOPED` 常量与短路口；现状 `route_meta.rs:729-738`）——行为与今天逐字相同，再按能力逐条补真值。危险的是「缺省当成某个真值」（那才会出现半张表比现状更不安全），不是「缺省当成未声明」。上一版 §4 把这两种缺省混为一谈，本条订正。

**为什么 P0 之后每一阶段都是 L2**：改的是**谁能做什么**（鉴权语义）与**对外的 OpenAPI 文档**，两者都在本流程的禁区里。P0 本身也动 `Route` 的 `pub` 结构，属「改 schema」的外延。

## 6. L2 边界：本流程不动的东西

1. `Route` 加任何字段、`REGISTRY` 升格为全站清单、删除 `repo_scope_for` —— 鉴权语义变更。
2. OpenAPI 加 `x-ironforge-*` 扩展字段 —— 前端与客户端契约变更（`openapi.rs:29` 是手写 `paths(...)` 列表，扩展要逐路径注入）。
3. `x_repo_ref` 替掉 `path_in_allowlist` —— 直接改变 PAT 的拒绝面。
4. `idempotency_supported` 从函数改查表 —— 表面上无行为变化，但一旦表写漏，幂等键会从「400 拒绝」变成「静默接受」，属行为变更。
5. **两族口径不一致（R14-4，P3）**：`path_in_allowlist` 认 `["repos", …]` 与 `["ai", "repos", …]` 两族（`pat_scope.rs:183-184`，现状未变），`ref_limited_write_allowed` 只认前者（`1c0a87e` `:309`；**R1 后机制换了、结论没变**：`/ai/repos/{owner}/{name}/index` 在 `route_meta.rs:447` 是 `repo(...)`，而 `repo()` → `declare()` 的缺省 `branch_limit` 就是 `Refused`（`route_meta.rs:130`），整块没链 `.open()`）。后果：带 `ref_allowlist` 的 token 对该路由（`POST`，`lib.rs:1779-1781`）恒拒——fail-closed，但与另一张表的接受面不对称。统一任一方向都改变 PAT 拒绝面 ⇒ L2。

## 7. 落地时要补的测试（今天没有）

- **数量对齐**：注册路由数 == `rg-http/src/lib.rs` 里 `/collaboration` 前缀的 `.route(` 数（现有 `collab_contract_tests.rs` 已经在做方向一致的检查，`normalize()` 在 `:30`，把 `{x}` 统一成 `{}` 以容忍参数改名）。
- **请求体字段对齐**：凡 `x_repo_ref: Body([...])` 的路由，列出的键必须出现在该路由请求体 schema 的属性集合里。
- **handler 自检对齐**：`ref_effect != None` 的路由集合，必须与 `grep -rn 'ensure_token_may_write_branch'` 的调用点集合一致（今天 4 处，见 §2②）。这条是 P2 的安全门。

## 8. 不建议塞进元数据的东西

`Access`（`crates/rg-core/src/collaboration/capabilities.rs:49-79` 已有原子需求）、会话凭据（`CollabReadSession` 等已表）、审计覆盖、MCP 工具映射（`mcp_tools` 已表）。路由声明一旦开始承载「什么都能管」的字段，就会变成第二份 `CLAUDE.md`，而它不会比 `CLAUDE.md` 更容易维护。
