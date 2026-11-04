# 多 Agent 协作 X01：能力发现与契约兼容——实现与验证记录

> 日期：2026-11-04。范围：[P2 设计](agent-collaboration-p2-design-2026-11.md) §6（R14“发现与版本”）与验收 M10–M11，任务 X01。**无迁移。**
> 按[证据分层](../../CLAUDE.md)逐层陈述，前一层不能替代后一层；未执行的写在「未验证」。

## 1. 实现了什么

| 项 | 位置 |
|---|---|
| 能力注册表（唯一静态来源） | `rg-core/src/collaboration/capabilities.rs`：`REGISTRY`（12 个能力：discovery、sessions、tasks、messages、handoffs、rules、takeovers、questions、work_packages、inbox、completion_status、deletion_impact），每个能力列 REST 路由（方法 + 路径 + 访问要求）与 MCP 工具名 |
| 发现端点 | `GET /api/v1/collaboration/capabilities`（任意已登录账号，不含任何仓库信息）；`GET /api/v1/repos/{owner}/{name}/collaboration/capabilities`（需协作读权限，另返回调用者的 `repo_collab_read / repo_write`）。实现 `rg-http/src/api/collab_capabilities.rs` |
| 响应 | `{contract_version "1.0", server_version, capabilities[{name,version,status,requires,mcp_tools}], limits, auth_modes, mcp_transport{transports:["stdio"],tool_count,…}, deprecations[]}`；`limits` 取自服务实际执行的常量，`tool_count` 取自注册表 |
| fail-closed 枚举助手 | `rg-core/src/collaboration/compat.rs`：`Known<T>` / `ContractEnum` / `UnknownValue`，枚举 `TaskStatus`、`QuestionStatus`、`RecipientKind` |
| 写请求拒绝未知字段 | 全部 15 个协作写请求结构（含嵌套 `RecipientBody`、`ContextBody`、`EvidenceRefBody`）加 `#[serde(deny_unknown_fields)]`；新提取器 `rg-http/src/api/collab_json.rs::CollabJson` 把所有请求体拒绝变成干净的 400 |
| MCP ↔ REST 对照表 | `rg-mcp/src/collab_contract.rs::TOOL_ROUTES`（28 行：工具 → 方法、路径、query、body 属性） |
| 防漂移测试 | `rg-http/tests/collab_contract_tests.rs`（§3）；`rg-mcp` 单测校验对照表与 `plan()` 实际发送一致 |

### 新增 / 变化的错误码（均为 400）

| `error.code` | 触发 | `details` |
|---|---|---|
| `UNKNOWN_FIELD` | 写请求含端点未定义的字段（含嵌套对象） | `{field}` |
| `INVALID_JSON` | 不是合法 JSON | — |
| `INVALID_REQUEST_BODY` | 类型错误 / 缺必填字段 / 取值非法 | — |
| `UNKNOWN_ENUM_VALUE` | 安全关键枚举取到未识别值（任务 `status`、问题 `status` 过滤、`recipient.kind`） | `{field, value, allowed[]}` |

## 2. 兼容性规则（可执行 + 文档）

**版本** `major.minor`（`CONTRACT_MAJOR/MINOR`）。

* 新增可选响应字段、新增能力、新增可选枚举值 → 升 minor；
* 删除字段、改变既有语义、新增**必填**请求字段 → 新 major 或新端点；
* 客户端兼容判定：同 major 且客户端 minor ≤ 服务 minor（`capabilities::client_is_compatible`）。

**读（响应）**：客户端忽略未知字段与未知的非关键枚举值。会改变授权、终态或安全行为的枚举，未知值按 `unknown` 处理并 fail closed（拒绝 / 视为“未完成”“未满足”），**不得默认成功**。服务端自身在读取库内值时同样遵守：`question::may_answer` 遇到未识别的 `recipient_kind` 返回“不可答复”（测试：把行改成 `team` 后答复得到 403，问题仍为 `open`）。

**写（请求）**：服务端**拒绝**未识别字段与未识别枚举值（避免 `expected_versoin` 之类的拼写错误静默变成无版本保护的写入）。这是一次**刻意的行为变更**，见 §4。

**废弃政策**（建议，项目可调）：

1. 在 `capabilities::DEPRECATIONS` 追加一项 `{capability, what, replacement, announced, earliest_removal}`，`earliest_removal` 至少晚于 `announced` **90 天**（`DEPRECATION_WINDOW_DAYS`；单测校验）；
2. 窗口内旧行为保持可用并有回归测试，发现端点的 `deprecations[]` 同步广告；
3. 升 minor；窗口结束后的移除属于破坏性变更，升 major 或换端点。

**新增能力**（供其他会话合并后接入）：在 `REGISTRY` 末尾追加**一个** `Capability { .. }` 元素（路由、MCP 工具名），模块头有说明。漏填会被 §3 的测试以可读信息拦下。

## 3. 验证

| 层级 | 结论 | 证据 |
|---|---|---|
| 实现证据 | ✅ | 本分支提交（见 git log） |
| 任务验证（SQLite，真实 HTTP） | ✅ | `cargo test -p rg-http --test collab_contract_tests`：15 项通过（见下）；`rg-core --lib` 210 项（含 `compat`×4、`capabilities`×5）；`rg-mcp` 全部（含 `collab_contract`×2）；`rg-http --lib`（含 OpenAPI 守卫）；`rg-db --lib` 26 项 |
| 既有协作回归 | ✅ | `collab_p0 / handoff / rules / secret_leak / session / task_message / visibility`、`openapi_*` 全部通过（未改测试） |
| 门禁 | ✅ | `cargo clippy --workspace --all-targets -- -D warnings` 无告警 |
| 真实运行 | 见 §5 | `scripts/collab-e2e.py --scenario p0` |
| 合并后集成 / CI / PostgreSQL / MySQL / 多实例 | ❌ 未验证 | 本机无 PG/MySQL；本任务无 SQL，但**未**在这两个后端运行同一测试 |

`collab_contract_tests` 覆盖：

* **M11** 能力列表恰为注册表；**对每个广告的路由用 `DELETE` 探测真实服务器**（路由存在则 405 且 `Allow` 含该方法）；`mcp_transport.tool_count == rg_mcp::collab::TOOL_NAMES.len()`；实例端点未登录 401 且响应不含仓库名；仓库端点未登录 401、成员 200（含各自 `repo_write`）、公开仓库非成员 403、**私有仓库非成员与不存在的仓库响应完全相同（404、同 code/message）**。
* **M10** 旧客户端只读自己认识的字段，能解析带未来字段的响应（能力与任务）；任务 `status` 取未知值 → 400 `UNKNOWN_ENUM_VALUE` 且任务未变；任务/问题列表的未知 `status` 过滤 → 400；未知 `recipient.kind` → 400 且未创建问题；库内未知 `recipient_kind` → 答复 403；**每个写端点族**（sessions、tasks 创建/更新、messages 发送/已读、handoffs 创建/接受/取消、takeovers 认领/重指派、questions 创建/答复/取消、work-packages）带多余字段 → 400 `UNKNOWN_FIELD` 且 `details.field` 正确；嵌套对象多余字段同样拒绝；被拒请求无副作用，去掉拼写错误后同一请求成功；非法 JSON / 缺字段 / 类型错 → 400（不再是 422）。
* **防漂移**（无服务器）：① `lib.rs` 中所有含 `collaboration` 的路由（含方法）== 注册表；② 注册表每个操作都在 OpenAPI 文档中；③ 每个写请求体 schema（含嵌套）都是 `additionalProperties:false`；④ `TOOL_NAMES` == 注册表中的工具 == `TOOL_ROUTES`；每个工具指向一个已注册、被其所属能力拥有、在 OpenAPI 里存在的操作，其 query 参数 ⊆ OpenAPI 参数、body 属性 ⊆ 请求 schema 属性（否则服务端会回 `UNKNOWN_FIELD`）。已手动验证守卫有效：从注册表删去一条路由，测试以“registered but not in …REGISTRY (add a Route to the owning Capability)”失败。

## 4. 与设计的取舍 / 行为变更

| 项 | 说明 |
|---|---|
| 写请求拒绝未知字段 | **行为变更**：以前多余字段被静默忽略。MCP 工具只发送 `pick` 过的已知键，且由 `TOOL_ROUTES` 测试证明 body 属性 ⊆ schema，故内置客户端不受影响；第三方客户端若带了多余字段会得到 400 `UNKNOWN_FIELD`。GET 的查询参数**不**拒绝未知键（非写请求）。 |
| 请求体错误 422 → 400 | 协作写端点的缺字段/类型错以前是 axum 默认的纯文本 422；现统一 400 + 稳定 code。无既有测试依赖 422。非 JSON 的 `Content-Type`/超大请求体保留原状态码（415/413）并带 `INVALID_REQUEST_BODY`。 |
| 提取器先于鉴权 | `CollabJson` 是 handler 的参数，请求体错误（400）会先于 401/403 返回——与此前 422 的次序一致，不泄露数据，但客户端应先保证登录。 |
| **修正既有 OpenAPI 错误** | 带 `IntoParams` 的查询结构（`ListQuery`、`TaskListQuery` 等 6 个）此前被文档化为 `in: path, required: true`——由漂移测试暴露。已加 `#[into_params(parameter_in = Query)]`，生成客户端依赖此前的错误文档会受影响（实际服务端行为未变）。 |
| 仓库发现的 403/404 | 私有仓库的非成员得 404（与不存在无异），公开仓库非成员得 403（与其他协作读端点一致）。其他协作端点对私有仓库非成员仍是 403/404 可区分——**未改动**，属既有行为，建议另行评估。 |
| 能力“启用”语义 | 当前没有运行时开关：能力 = 构建中存在并已挂路由。因此“关闭/开启”(M11) 只证明了“广告 == 实际路由”，**未**覆盖运行时开关（尚不存在）；日后引入开关时，`capabilities` 响应须按开关过滤，并补测试。 |
| 旧版降级 | 未实现自动降级逻辑：客户端用 `contract_version` + `client_is_compatible` 自行决定。 |
| 版本值 | 首版 `1.0`；各能力版本均 `1.0`、`stable`。 |
| `Known<T>` 的使用面 | 应用于任务状态、问题状态过滤、`recipient.kind` 的请求解析，以及 `may_answer` 的库内值。其余枚举（消息 `kind`、证据 `kind` 等）仍由服务层校验函数拒绝，不影响授权/终态，未改。 |

## 5. 未验证 / 风险

* PostgreSQL / MySQL：未运行（本任务无新 SQL）。
* 多实例：能力响应为静态数据，无进程内状态；仓库端点沿用“绕过 30 秒权限缓存”的既有做法。
* 与其他并行会话合并：它们新增的 `collab_*` 工具和 `/collaboration` 路由**会让本任务的漂移测试失败**，直到各自补上注册表条目、`TOOL_ROUTES` 行和 OpenAPI 登记——这是有意的；合并者按失败信息逐条补即可。它们新增的写请求结构也应加 `deny_unknown_fields`（`write_request_schemas_declare_…` 会检查）并使用 `CollabJson`。
* `scripts/collab-e2e.py --scenario p0`：见 §6。

## 6. 真实运行

`python3 scripts/collab-e2e.py --scenario p0`（真 `ironforge` 服务器 + 真 `ironforge-mcp` 二进制 + HTTP 客户端 + 真 Git 仓库，SQLite）在启用 `deny_unknown_fields` 后 [03]–[17] 全部通过，说明内置 MCP 工具发送的请求体未被新的未知字段拒绝误伤。该脚本未覆盖能力发现端点（后者由 `collab_contract_tests` 的真实 HTTP 测试覆盖）。
