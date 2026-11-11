# 路由元数据注册表：现状核实与分阶段落点（Proposed）

> 答复 issue #14「路由元数据注册表」。核实基线 `75d8231`（2026-11-12），全部行号由 `sed -n` / `grep -n` 逐条打印过。
> 本文只写**核实过的事实**与**可执行的落点**；凡是改鉴权语义、改 OpenAPI 契约、加路由字段的部分一律停在 L2（见 §6），不在本流程落地。

## 0. 结论

- **方向成立，但不是一次重构。** 五张表里只有两张是「路由属性」（`idempotency_supported` 的秘密面、`pat_scope` 的读写面），另外两张的判据是**请求体内容**和**handler 里有没有写检查**——这两种事实声明不出来，只能写下来并被人复核。
- **可用的骨架比设想的窄。** `capabilities::REGISTRY` 今天只有 **27 条 capability / 75 条路由**，且 75 条路径**全部**在 `/collaboration/**` 下；全站是 `rg-http/src/lib.rs` **323** 处 `.route(`（全仓 `crates/` 342 处）。把它升格成全站唯一清单 = 新增 **248** 条声明。
- **「未声明即拒绝」这层已经守住了。** 三张表今天都是 fail closed（§2 第 3 条）。真正没被守住的是「声明与 handler 一致」，那是**测试**能治的，不是元数据能治的。因此本条的价值主要在「少维护几张手写表」，不在「堵一个现存漏洞」。

## 1. 五张表的真实位置

| 表 | 代码位置（本基线） | 真正的判定维度 | 能否由 method+path 派生 |
|---|---|---|---|
| `required_pat_scope` / `repo_scope_for` | `crates/rg-http/src/lib.rs:2076` / `:2137` | 读 vs 写、`/collaboration/**` 写走 `repo:collab` | 能（前缀族表 `:2106-2127`） |
| `path_in_allowlist` 的拒绝表 | `crates/rg-core/src/auth/pat_scope.rs:174`，拒绝模式在 `:189` | **请求体里是否引用别的仓库** | **不能**：同一条路由换个体就换结论 |
| `ref_limited_write_allowed` | 同上 `:301`，放行集合 `:312-335` | 操作会不会移动 ref **且** handler 自己查分支 | **不能**：后半句是代码事实 |
| `idempotency_supported` | `crates/rg-http/src/idempotency.rs:64` | 响应里是否含令牌/秘密 + 是否创建型 | 能（但只能人工审出「有没有秘密」） |
| `capabilities::REGISTRY` | `crates/rg-core/src/collaboration/capabilities.rs:133`，`Route` 三字段 `:84-88` | 能力分组 + `Access` 原子需求 + MCP 工具归属 | 半能（只覆盖 `/collaboration/**`） |

`REGISTRY` 的 `Route` 已经是 `method` + `path` + `access`（`:84-88`），条目用 `const fn r(...)`（`:90-96`）构造，`&'static` 表。**结论：要扩的是字段，不是从零建系统**；但扩字段要改 75 处 `r(...)` 调用点，不是纯增量。

## 2. 三个隐含假设的裁决

**① 「五张表都是路由元数据」——不成立。**
`path_in_allowlist` 拒绝的是**跨仓引用**，判据在请求体里，且三个字段各不相同：

- `ForkRequest.org`（`crates/rg-http/src/api/repos.rs:878-880`）
- `TransferRequest.new_owner`（`crates/rg-http/src/api/repos.rs:986`）
- 协作组的 `target_repo_id`（`crates/rg-http/src/api/collab_groups.rs:63`）

代码里的拒绝模式是 `["fork", ..] | ["forks", ..] | ["transfer", ..] | ["collaboration", "groups", ..]`（`pat_scope.rs:189`）——**四个模式、三类语义**（`forks` 是 `fork` 的复数别名，不是独立家族）。改成声明后仍需人手确认「这个 handler 的体里到底有没有别人的仓库 id」。

**② 「声明一次就能自动派生」——部分成立。**
`idempotency_supported` 的文档注释（`idempotency.rs:59-63`）本身就是判据：「creating a repository, or writing under … Everything else, in particular anything that returns a token or a secret, is out」。这一条可以派生。
但 `ref_limited_write_allowed` 放行 `contents/**`、`POST commits`、`DELETE branches/<name>`（`:325` / `:328` / `:334`，后者排除 `branches/protection/**`）**只因为**三个 handler 里真的调了 `ensure_token_may_write_branch`：

- `crates/rg-http/src/api/repo_content.rs:675`（删分支）、`:1451`、`:1591`
- `crates/rg-http/src/api/repo_commits.rs:166`（批量提交）

新增一条写路由时，「我调了没有」只有写它的人知道；声明写错时编译和静态检查都不会响。

**③ 「未声明即拒绝能防漂移」——部分成立，且这一层今天已经在生效。**
- `path_in_allowlist`：路径不在名单内仓库下、或命中拒绝模式 → `false`（`:174-191`）。
- `ref_limited_write_allowed`：不落在 `:312-335` 的 `matches!` 里 → `false`。
- `required_pat_scope`：未知前缀 → `Some("admin")`（`crates/rg-http/src/lib.rs:2127`，「Fail closed: a PAT must not reach an unrecognised route by default」）。

所以「忘记登记」的后果是**被拒**，不是被放行。真正会漏的是：某条路由**已经登记**，但它的 handler 后来改了、不再检查分支。这类漂移靠 `ref_effect` 字段也发现不了，只能靠测试把「登记的放行集合」与「实际调用 `ensure_token_may_write_branch` 的路由」对齐（§7）。

## 3. 需要订正的计数与行号

| 复核对象 | 原文说法 | 实测（基线 `75d8231`） | 判据 |
|---|---|---|---|
| capability 数 | 30 | **27** | `grep -c '^    Capability {'`（REGISTRY 段） |
| 注册路由数 | 78 | **75** | `grep -c '^\s\+r('`（REGISTRY 段） |
| 全站 `.route()` | 298 | **342**（`rg-http/src/lib.rs` 单独 **323**；它自己的基线 `cba96ba` 上是 320） | `grep -rn '\.route(' --include=*.rs crates/ \| wc -l` |
| 「1200 个判断点」（298×4） | 1192 | **300**（75×4）；只做 P0 是 **150**（75×2） | 同上 |
| `path_in_allowlist` 位置 | `pat_scope.rs:293` | **`:174`**，拒绝表 `:189`；`:293` 落在 `ref_limited_write_allowed` 的文档注释里 | `grep -n '^pub fn path_in_allowlist'` |
| `ref_limited_write_allowed` 位置 | `:300` | **`:299`** | `grep -n '^pub fn ref_limited_write_allowed'` |
| `required_pat_scope` / `repo_scope_for` | `lib.rs:2064` / `:2125` | **`crates/rg-http/src/lib.rs:2076`** / **`:2137`** | `grep -n 'fn required_pat_scope\|fn repo_scope_for'` |
| `idempotency_supported` | `idempotency.rs:64` | `:64` ✅ | 原文即 `pub fn idempotency_supported(path: &str) -> bool {` |
| `REGISTRY` 覆盖范围 | 只覆盖 `/collaboration/**` | ✅ 75 条路径全部含 `/collaboration` | 逐条列举 |
| 建议的 `Route` 代码块 | `／/ ↓ 新增` | 全角斜杠，**编译不过** | 复制进 `rustc` 即报错 |

`sessions` 一类路径写成 `const SESSIONS`（`crates/rg-core/src/collaboration/capabilities.rs:130`）后复用，所以「按路径去重」与「按路由计数」不是同一个数：75 条路由对应 58 个不同路径。

## 4. 一次到位的成本

「全站唯一清单 + 未声明即拒绝」要求 `required_pat_scope` 的每一族都在表里：git、OCI、WebSocket、runners、metrics、`/health`、`/api-docs`。半张表比现状更危险——`required_pat_scope` 的未知前缀兜底是 `admin`（`crates/rg-http/src/lib.rs:2127`），漏登记 `GET /health` 就是全员 403。

按 248 条新增路由 × 4 个字段，是 **992** 个判断点；即使只做 P0（2 个字段）也是 496 个。这不是一次 PR 的体量，也不该由一个只有文档权限的流程来改。

## 5. 分阶段落点

| 阶段 | 内容 | 消费方 | 前置条件 | 本流程是否做 |
|---|---|---|---|---|
| **P0** | `Route` 加 `pat_scope` + `response_secret`；`REGISTRY` 仍只管 `/collaboration/**` | `idempotency_supported` 改查表；`required_pat_scope` 对 `/collaboration/**` 改查表，其余族保持前缀表 | 75 处 `r(...)` 调用点补齐；两条派生路径的等价性测试 | 否（L2） |
| **P1** | 加 `x_repo_ref`，替掉 `path_in_allowlist` 的四个拒绝模式 | `repo:read`/`repo:write` 判定前的跨仓引用检查 | 三条请求体字段名与 `serde` 键集合对齐测试 | 否（L2） |
| **P2** | 加 `ref_effect`，替掉 `ref_limited_write_allowed` | 分支白名单收窄 | `contents`/`commits`/`branches` 三族的 handler 调用点全在案（今天 4 处，§2②）；`scripts/agent-e2e-remote.py` 的分支白名单用例回归 | 否（L2，安全关键） |
| **P3** | `Capability.routes` 派生自全站 `REGISTRY`，打开「未声明即拒绝」 | 全站 PAT scope | 248 条路由全部登记 | 否（L2） |

**为什么 P0 之后每一阶段都是 L2**：改的是**谁能做什么**（鉴权语义）与**对外的 OpenAPI 文档**，两者都在本流程的禁区里。P0 本身也动 `Route` 的 `pub` 结构，属「改 schema」的外延。

## 6. L2 边界：本流程不动的东西

1. `Route` 加任何字段、`REGISTRY` 升格为全站清单、删除 `repo_scope_for` —— 鉴权语义变更。
2. OpenAPI 加 `x-ironforge-*` 扩展字段 —— 前端与客户端契约变更（`openapi.rs:29` 是手写 `paths(...)` 列表，扩展要逐路径注入）。
3. `x_repo_ref` 替掉 `path_in_allowlist` —— 直接改变 PAT 的拒绝面。
4. `idempotency_supported` 从函数改查表 —— 表面上无行为变化，但一旦表写漏，幂等键会从「400 拒绝」变成「静默接受」，属行为变更。

## 7. 落地时要补的测试（今天没有）

- **数量对齐**：注册路由数 == `rg-http/src/lib.rs` 里 `/collaboration` 前缀的 `.route(` 数（现有 `collab_contract_tests.rs` 已经在做方向一致的检查，`normalize()` 在 `:30`，把 `{x}` 统一成 `{}` 以容忍参数改名）。
- **请求体字段对齐**：凡 `x_repo_ref: Body([...])` 的路由，列出的键必须出现在该路由请求体 schema 的属性集合里。
- **handler 自检对齐**：`ref_effect != None` 的路由集合，必须与 `grep -rn 'ensure_token_may_write_branch'` 的调用点集合一致（今天 4 处，见 §2②）。这条是 P2 的安全门。

## 8. 不建议塞进元数据的东西

`Access`（`crates/rg-core/src/collaboration/capabilities.rs:49-79` 已有原子需求）、会话凭据（`CollabReadSession` 等已表）、审计覆盖、MCP 工具映射（`mcp_tools` 已表）。路由声明一旦开始承载「什么都能管」的字段，就会变成第二份 `CLAUDE.md`，而它不会比 `CLAUDE.md` 更容易维护。
