# Agent 技能 / MCP 服务注册中心 — 设计与分期计划

> **状态**：P1（Skill，每仓库）已在分支 `feat/skills-registry-p1` 实现，证据与未覆盖项见 [skills-registry-p1-verification-2026-11.md](../../ironforge-docs/architecture/skills-registry-p1-verification-2026-11.md)；P1b（MCP 清单）及之后**未开始**。下文 §1–§12 是原计划；实际与计划的差异见文末 §13。
> 编号未分配（T 编号由负责人分配，批准后再改文件名）。
> 证据标注：`确认` = 读过仓库代码；`外部` = 来自网络检索，实现前须对着官方文档再核；`待核实` = 未读代码或未验证。

## 1. 目标与非目标

**目标**：让 agent 在 IronForge 上发现、核对、固定、读取两类东西——

1. **Skill**：一个目录，根下有 `SKILL.md`（YAML frontmatter + Markdown 正文），可带脚本和参考文件。
2. **MCP 服务清单**：一份描述「某个 MCP server 怎么装、怎么连」的 `server.json`。

**非目标（本计划不做）**：

- 不替 agent 安装、不执行任何 skill 里的脚本、不启动任何 MCP 服务。
- 不探测 MCP 服务是否在线（见 §6，避免 SSRF 面）。
- 不做评分、排名、"信任分"。只给可核对的事实标签。
- 不做公共联邦（不与官方 MCP Registry 互相同步）。

## 2. 现有基础（确认）

- 包注册表按 `(repo_id, package_type)` 一行（`package_registry`），其下 `packages`、`package_versions`、`package_files`（`rg-db/src/entities/`）。所有包都挂在某个仓库下。
- 适配器 trait `PackageAdapter`（`rg-core/src/package_registry/adapter.rs`）：`extract_metadata` / `validate` / `content_type_for_file`；`get_adapter` 对未知类型回落 generic。`package_types::ALL` 是合法类型白名单，`publish` handler 会据此 400。
- `package_versions` 有 `metadata`（文本 JSON）、`sha256`（**只取第一个文件的哈希**）、`is_yanked`。
- **通用 `publish` handler 把 `metadata: None` 传给服务**（`rg-http/src/api/packages.rs`，`PublishInfo { metadata: None, .. }`）：适配器提取出的元数据没有落库，只用了名称/版本/描述等字段。skill 需要的 frontmatter 与来源信息必须走新的专用路径写入 `metadata`。
- **重复发布同一版本，服务只 `warn!` 并返回已有版本，不比较内容**（`service::publish`）。对 skill 这不可接受（见 §4.3），专用路径必须自己比对摘要，且**不改**通用行为。
- 仓库里没有任何 skill 相关代码；`semver 1.0.28` 已在 `Cargo.lock`；`rg-core` 已依赖 `flate2`、`tar`、`serde-saphyr`（经 `rg_core::yaml` 使用）。
- `rg-git/src/ops.rs` 有 `blob_at`（按 rev:path 读单个 blob）与 `archive`；**没有现成的"递归列目录树"函数**，需要新增（gix 实现，`待核实` 具体 API）。
- 新增 REST 路由必须在 `rg_core::auth::route_meta::ROUTES` 声明 PAT scope、是否触达其他仓库、分支受限令牌能否写、幂等键是否安全（CLAUDE.md；`route_metadata_tests` 会校验漂移）。新增 MCP 工具须同步登记 `rg-mcp` 的工具表。
- 授权原语 #19（`rg-core/src/authorization/`）：请求绑定 `(repo, action, subject, head_sha)`，`consume` 必须与被授权的写在同一事务；**`request()` 的注释说 agent 一侧的准入检查"等请求端点出现时再接"——agent 经 REST 提交授权请求的入口是否已存在，`待核实`**，它决定 P2 的审批门工作量。

## 3. 总体方案

两个新包类型，复用现有存储与表，不新建表（P1 不需要迁移，见 §7）：

| 类型 | `package_type` | 制品 | 是否托管内容 |
|---|---|---|---|
| Skill | `skill` | 确定性 `tar.gz`（含 `SKILL.md`） | 是 |
| MCP 服务清单 | `mcp-server` | 单个 `server.json` | **否**，只存清单，指向外部包或远程 URL |

`package_types::ALL` 加入这两项；新增 `SkillAdapter`、`McpServerAdapter`。在其上加**面向 agent 的专用读取面**——通用 `/packages/{type}/…` 路由保持原样可用。

### 3.1 Skill 制品格式

- 依据开放的 Agent Skills 规范（`外部`：agentskills.io）：`name` 与 `description` 必填；`name` 全小写、不以连字符开头、无连续连字符；`description` 1–1024 字符；可选 `license`、`compatibility`、`metadata`（字符串到字符串映射）、`allowed-tools`（实验性、空格分隔字符串）。**实现前须对照官方规范核对这些约束。**
- `SkillAdapter::validate` 检查：
  - 是 gzip tar；根目录恰有一个 `SKILL.md`；frontmatter 可由 `rg_core::yaml` 解析；`name` 合规且**等于包名**；`description` 非空且 ≤1024。
  - 拒绝：符号链接、硬链接、设备/FIFO、绝对路径、含 `..` 的路径、重复路径、非 UTF-8 路径。
  - 上限（**提议值，待评审**）：≤200 个文件、解包后 ≤20 MiB、`SKILL.md` ≤256 KiB、压缩包 ≤5 MiB。超限是 400，不是静默截断。
- `allowed-tools` 与 `compatibility` 原样记录、原样返回，**注册表不据此授予或限制任何权限**。

### 3.2 MCP 服务清单格式

- 采用官方 MCP Registry 的 `server.json` 形状（`外部`）：`name`（反向 DNS 风格）、`description`、`version`，以及 `packages[]`（`registryType` 如 npm/pypi/nuget/cargo/oci/mcpb，`identifier`，`version`，`transport`，环境变量与参数声明）和/或 `remotes[]`（`type`: `streamable-http` | `sse`，`url`，`headers`）。至少要有其一。**`$schema` 日期版本有多个，实现前必须核对当前版本。**
- `McpServerAdapter::validate`：JSON 可解析、必填字段齐、`packages`/`remotes` 至少一项、`remotes[].url` 必须是 `https`（`http` 仅允许 localhost 并标记）、字段长度上限、未知 `registryType` 保留但标 `unknown_registry_type`（不拒绝，避免被上游新增类型卡住）。
- **所有权声明 ≠ 所有权验证**：官方注册表会校验 npm 包里的 `mcpName` 等。我们的第一期对外部包**不做**该校验，响应里写 `ownership: "unverified"`。仅当 `packages[]` 指向**本实例**托管的包（`oci`/`npm`/`cargo` 等且在本实例可解析）时，可标 `hosted_here: true`——此时才可给"包存在"的服务端核验结果。

## 4. 版本、来源与不可变

### 4.1 从 Git 引用发布（Skill 的首选路径）

`POST /repos/{owner}/{name}/skills/publish`，体：`{ "path": "skills/pdf", "ref": "v1.2.0", "version": "1.2.0"?, "client_request_id": ... }`

1. 服务端把 `ref` 解析成 commit（拒绝解析不出的 ref）。
2. 读取 `path` 下的树，**自己**打包成确定性 tar.gz：条目按路径排序、mtime=0、uid/gid=0、名称清空、模式规整为 0644/0755、固定 gzip 参数。同一棵树在任何机器上得到同一摘要。
3. 版本号：请求体 `version`，或 frontmatter 的 `metadata.version`；两者都给必须一致；必须是合法 semver。
4. 记录 `source: { repo_id, ref, commit, path, built_by: "server" }` 与逐文件清单（路径、大小、sha256）到 `package_versions.metadata`。

这样"这个版本来自该仓库该 commit"是**服务端核验过的事实**（对应 CLAUDE.md 证据分层的"远端验证"一层，不是自报）。

### 4.2 直接上传（兼容路径）

也允许上传现成 tar.gz。此时 `source.built_by: "publisher"`，`source_verified: false`，响应里如实标注；摘要照算。

### 4.3 不可变

- 新增专用比较：同一 `(包, 版本)` 再次发布，**摘要相同 → 200 幂等；摘要不同 → 409 `VERSION_IMMUTABLE`**，附 `reason`、`details.existing_digest`。
- 不修改通用 `service::publish` 的"返回已有版本"行为（别的包类型依赖它）；专用路径在调用前先查先比。
- 版本记录中的摘要字段统一用 `sha256:<hex>`。`package_versions.sha256` 对 skill 只有一个文件，所以等于整包摘要，无需改"只取第一个文件"的逻辑；MCP 清单同理。

### 4.4 撤回

沿用现有 yank（`is_yanked`）。被 yank 的版本：`resolve` 不会选中，除非请求精确版本；读取时 `yanked: true` 明示。

## 5. 面向 agent 的接口

全部返回紧凑 JSON；用户写的文字字段（描述、frontmatter、清单内容）带 `origin: "user_content"`；列表用 keyset 分页；错误带稳定机器码。

**每仓库**（沿用 `require_repo_read/write`）：

| 方法与路径 | 作用 |
|---|---|
| `POST /repos/{o}/{n}/skills/publish` | 从 Git 引用发布（写权限） |
| `GET /repos/{o}/{n}/skills` | 本仓库的 skill 列表 |
| `GET /repos/{o}/{n}/skills/{name}` | 版本列表、最新非 yank 版本 |
| `GET …/skills/{name}/versions/{v}` | 清单：frontmatter、文件清单、摘要、来源、`yanked` |
| `GET …/skills/{name}/versions/{v}/content` | **只返回 `SKILL.md` 文本**（服务端从 tar 中取，受上限约束），agent 不必下载解包 |
| `GET …/skills/{name}/versions/{v}/files/{path}` | 单个文件文本（同样有上限，二进制拒绝） |
| `GET …/skills/{name}/versions/{v}/archive` | 整包下载，带 `x-checksum-sha256` |
| `GET …/skills/{name}/resolve?constraint=^1.2` | 解析为**精确版本 + 摘要**，供写入锁文件；约束不满足 404 `NO_MATCHING_VERSION`，带可用版本列表 |
| `PUT/POST/GET …/mcp-servers[/{name}[/versions/{v}]]` | MCP 清单的发布与读取，形状同上，无 `content`/`archive` |

**跨仓库发现**（人和 agent 都能用）：

- `GET /api/v1/skills?q=&limit=&cursor=`、`GET /api/v1/mcp-servers?q=&…`
- **先鉴权再分页**：每页逐项重新做 `can_read_repo`，不可读项静默过滤，不返回命中总数（与协作组一致，防探测）。
- 每项：`owner/repo/name`、`description`、`latest_version`、`digest`、`source_verified`、`publisher`（用 T26 的 `ActorRef`：`kind` human/agent、所有者）、`yanked`、`updated_at`。
- 搜索第一期用名称/描述的 `LIKE`（三库通用）并设上限，**不**引入 FTS；规模变大再评估（现有 FTS5 仅 SQLite）。

**MCP 工具**（`rg-mcp`，只读）：`search_skills`、`get_skill`（默认返回清单 + `SKILL.md`）、`resolve_skill`、`search_mcp_servers`、`get_mcp_server`。**没有"安装"工具**：写文件到本地由客户端决定。发布类工具放到后面单独评审。

## 6. 安全与信任

skill 是会被其他 agent 当指令加载的文本，注册表因此是**持久化提示注入**的投放点。设计上：

1. **内容永远是数据**：响应里的 SKILL.md、frontmatter、清单字段一律标 `origin: user_content`；文档里写明客户端不得因此提升权限。
2. **按摘要固定**：`resolve` 返回的摘要进锁文件；`archive` 与 `content` 响应带摘要头，客户端可核对。
3. **agent 发布受限**（P2）：agent 令牌默认**不能**发布会出现在跨仓库发现里的条目。要发布须走授权原语 #19：`action = "skill.publish"`，`subject = "<name>@<version>"`，`head_sha = 来源 commit`，由人批准，`consume` 与写版本同事务。**这部分的前提是 agent 提交授权请求的入口，见 §2 待核实项。**
4. **令牌范围**：读需 `repo:read`；发布需 `repo:write`。分支受限令牌对新 REST 写路由默认被拒（现有规则），本功能**不**为它们开例外。跨仓库发现路由不属于任何单一仓库，声明为 `other_repos`，仓库白名单令牌会被拒绝——白名单 agent 用每仓库路由。
5. **不出站**：注册表不抓取 `remotes[].url`、不解析 `identifier` 指向的外部制品，所以本功能不新增 SSRF 面。将来若加在线探测，必须走 `net_guard`。
6. **审计**：发布、yank、删除版本写审计（`audit::record_write`，带 `access_token_id` 与 `on_behalf_of`）。
7. **不做**：不扫描 skill 内容中的恶意指令（无法可靠做到，宣称做到比不做更糟）。可选复用 `rg-git/src/secret_scan.rs` 在发布时拒绝含疑似密钥的文件——**待评审是否纳入 P1**。

## 7. 数据与迁移

- P1 **无迁移**：frontmatter、来源、文件清单都放 `package_versions.metadata`（JSON）。
- 若 P2 的跨仓库搜索需要按类型/更新时间走索引，再评估加索引的迁移；一旦加迁移，需同步 `Migrator::down` 窗口断言（CLAUDE.md：现有 107 条、回滚 33 步）并跑三库。
- 存储沿用 `PackageStorage` / `BlobStorage`，键为 `packages/{owner}/{repo}/skill/{name}/{version}/{filename}`。

## 8. 接入清单（防漂移）

- `ROUTES` 新增行：每仓库读 `repo(...)`；`publish` 与清单写 `repo(...)` 且**不**加 `.checked()`/`.open()`（对分支受限令牌默认拒）；是否 `.idem()`：响应无秘密，`publish` 可加，但发布的幂等已由 `client_request_id` + 摘要比较保证，先不加；跨仓库发现用 `other_repos`。
- `capabilities` 注册表（`api_capabilities`，参照 T26 的 `actors.ref.v1`）：新增 `registry.skills.v1`、`registry.mcp_servers.v1`，**只广告真实存在的能力**。
- OpenAPI 注解与守卫测试：`api/` 下平铺新文件（`skills.rs`、`mcp_servers.rs`，不递归）。
- `rg-mcp` 工具表同步，测试覆盖。
- 文档：`ironforge-docs/architecture/` 加验证文档；CLAUDE.md 实现清单加一行（只写已验证的层）。

## 9. 测试计划

| 层 | 内容 |
|---|---|
| 适配器单测 | frontmatter 合法/非法；`name` 与包名不一致；符号链接、`..`、绝对路径、重复路径、超限；`server.json` 缺字段、无 packages/remotes、`http` 远程 |
| 确定性 | 同一棵树在不同 mtime/不同顺序下打包摘要相同；改一个字节摘要变 |
| 发布 | 从 tag 发布；ref 解析失败；`version` 与 `metadata.version` 冲突；相同摘要 200、不同摘要 409 `VERSION_IMMUTABLE`；`source.built_by` 标注 |
| 读取 | `content` 只返回 `SKILL.md`；`files/{path}` 的上限与二进制拒绝；`resolve` 约束命中、yank 跳过、无匹配 404 |
| 权限 | 私有仓库的条目对无权者在发现里不出现、不计数；撤权后下一次请求即生效（协作端点那种先失效缓存的做法**不**自动适用，需明确选择）；白名单令牌访问发现路由被拒 |
| 路由元数据 | `route_metadata_tests` 通过；capabilities 防漂移测试通过 |
| 三库 | SQLite 全量；`scripts/ci-db-tests.sh` 清单加新测试目标，PostgreSQL 17 与 MySQL 8.4 各跑一遍（MySQL 注意同秒排序，列表按 `(updated_at, id)` 并列只断言合同允许的顺序） |
| 端到端 | 真实服务 + 真实 `ironforge-mcp`：agent 搜索 → resolve → 读 `SKILL.md` → 摘要核对（手工脚本，不进 CI，需如实写在验证文档里） |

## 10. 分期

**P1 — Skill 最小可用（无迁移）**
`skill` 类型与 `SkillAdapter`；新增 `rg-git` 递归列树；确定性打包；每仓库 `publish` / 读取 / `content` / `archive` / `resolve`；`VERSION_IMMUTABLE`；路由元数据与 capabilities；适配器、发布、读取、权限测试。

**P1b — MCP 清单**
`mcp-server` 类型与适配器；每仓库发布与读取；校验规则与 `ownership: unverified` 标注。与 P1 共用路由骨架，工作量小，建议紧随其后同一批合入或紧接着。

**P2 — 发现与 agent 化**
跨仓库发现（先鉴权再分页）；五个 MCP 只读工具；agent 发布审批门（依赖 #19 的 agent 入口）；视情况加索引迁移。

**P3 — 之后评估**
与官方 MCP Registry API 形状兼容的只读端点（`外部`，**API 版本须核对后才能承诺**）；在线探测（走 `net_guard`）；使用统计；组织级归属。

## 11. 待负责人决定

1. 上限数值（§3.1）是否合适。
2. P1 是否纳入发布时的密钥扫描（§6 第 7 条）。
3. 是否接受"agent 默认不能发布到全局发现"这一约束进 P2（会让 agent 自主沉淀 skill 的闭环多一次人工批准）。
4. 搜索是否接受第一期只用 `LIKE`。
5. 公共实例的"公开仓库里的 skill 是否对未登录用户可发现"——本计划按 `can_read_repo(…, None)` 的现有语义处理，**没有核对未登录访问的具体规则**，实现前须读代码确认。

## 12. 本计划没有验证的事

- 没有跑任何代码、测试或迁移。
- `rg-git` 递归列树的 gix API、授权原语的 agent 入口、未登录访问规则：均 `待核实`。
- 外部格式（Agent Skills、`server.json`）来自检索摘要，不是逐字阅读规范；实现的第一步是对着官方文档核对 §3 的字段与约束。

## 13. P1 实现与原计划的差异（2026-11-21）

| 原计划 | 实际 | 原因 |
|---|---|---|
| §4.2 同时允许直接上传 tar.gz | **P1 只支持从 Git 引用发布**；通用 `POST …/packages/skill/publish` 明确拒绝 `skill` 类型 | 上传路径需要另一套来源标注（`built_by: publisher`）和更多校验面；先把来源可核验的路径做对。`SkillSource.built_by` 已为它留位 |
| §3.1 / §7 版本元数据放 `package_versions.metadata`，含逐文件清单 | **元数据只存 frontmatter、摘要、来源、解包体积；文件清单每次从归档重建**；frontmatter 块上限 16 KiB，序列化后总量上限 48 KiB | `metadata` 是 `TEXT` 列，MySQL 上只有 64 KiB；200 个长路径的清单会写不进去。清单从归档重建同时让每次读清单都校验一遍归档与摘要 |
| §5 发布体含 `client_request_id` | 去掉 | 幂等由"同版本同内容摘要 → 200"保证，不需要另一把键 |
| — | skill 名 `publish` 被保留 | `…/skills/publish` 是静态路由段，会遮住同名 skill 的 GET |
| §5 跨仓库发现、MCP 工具 | **未做**（P2） | 按分期 |
| §5 MCP 清单 | **未做**（P1b） | 按分期 |
| §6 第 3 条 agent 发布走授权原语 #19 | **未做**（P2）。P1 中任何持 `repo:write` 的 agent 令牌都能在**该仓库**发布；但没有全局发现，发布的 skill 只在该仓库的 `…/skills` 下可见；分支受限令牌被拒 | 依赖 #19 的 agent 入口，见 §2 待核实项 |

新增事实（实现时发现）：

- `rg-git` 新增 `list_tree_files`（递归、按字节序、带条目上限）与 `read_blobs`，不做检出、不过滤，换行符按对象库原样。
- 字节级确定性只对**未压缩 tar** 成立（`content_digest`）；压缩后的 `.tar.gz` 摘要（`archive_digest`）在服务端存下后固定，换 gzip 实现重新打包可能得到不同字节，所以不可变性按 `content_digest` 判定。
- `semver` 作为 `rg-core` 的直接依赖加入（`Cargo.lock` 中已有 1.0.28，只多一条边）。
