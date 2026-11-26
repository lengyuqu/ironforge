# Changelog

本文件记录 IronForge 的版本演进。格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，
版本号遵循 [Semantic Versioning](https://semver.org/lang/zh-CN/)（0.x 阶段以 minor 为 breaking 粒度）。

## [0.5.1] - 2026-11-26

0.5.0 的 CI 把两个既有问题照了出来，这个补丁把它们一起收口：`crypto-bigint 0.5.5` 的
RUSTSEC-2026-0387，以及 6 个文件没过 `cargo fmt --check`（格式门是 Clippy job 的第一步，
所以 clippy 本身没跑到）。两者都不是 0.5.0 引入的，但它们让 0.5.0 的 CI 变红。

### Fixed

- **`jsonwebtoken` 10.3.0 → 12.0.0**：12 改用 `p256`/`p384` 0.14 与 `rsa` 0.10，
  `crypto-bigint 0.5.5` 因此整条离开依赖树（RUSTSEC-2026-0387：常量时间运算存在时序差异）。
  12 的 `EncodingKey::from_secret` / `DecodingKey::from_secret` 改为返回 `Result`
  （空 HMAC 密钥在构造时被拒），四个签发/校验入口按此适配：签发出错，校验视为无效。
- **`cargo fmt`**：`rg-ci` 与 `rg-git` 的 6 个文件按 rustfmt 重排，格式门恢复。

## [0.5.0] - 2026-11-26

本版本的主线是 **大仓库的传输与内存**：先把基线量出来，再按实测重写两处最贵的地方。
T32 用固定种子的三档数据集量出两个数字——改一个文件后的 `git fetch` 实际传的对象是客户端
真实缺失对象的 **8,380 倍**（large 数据集 88,270 倍），一次 large 仓库 clone 在服务端常驻
**1.21 GB** 的单个 `Vec<u8>`。T37 把前者改成对象级差集，T38 把后者改成流式输出；
3 路并发 large clone 从 6536 MiB 降到 782 MiB（改前在 8 GB 机器上必 OOM）。

另一半是生产化的底账：CI 能展开 composite action（`node*` / `docker` 继续 fail closed），
HA / 滚动升级 / 备份恢复第一次有可照着执行的演练手册和判定失败的标准，95 处授权点完成测绘，
CNB 导入按前两次的教训做了真机验证——又炸出 9 个「报成功而数据是错的」缺陷。

涵盖 61 个提交（fix 14 / docs 14 / chore 8 / perf 4 / feat 4 / test 3）。

### Added

- **大仓库性能基线**（T32）：三档固定数据集（固定种子、可重建）与绝对阈值写成 `#[ignore]`
  测试（`crates/rg-git/tests/perf_baseline.rs`），回答「这个改动有没有变慢」不再靠感觉。
- **CI composite action 展开**（T34）：按 ADR-0001 只做纯 YAML 展开，不碰执行沙箱；
  展开是原子的，远程缓存按子目录区分。`node*` 与 `docker` 继续 fail closed，并说明需要
  `actions-v1` runner。
- **HA / 滚动升级 / 备份恢复**（T35）：`docs/operations.md`；在 HEAD 二进制（112 条迁移）上
  重跑恢复演练，给出实测机器时间（SQLite 约 145 ms）与「什么算演练失败」的清单，并明确不外推。
- **Unit 级权限测绘**（T33）：95 处授权点逐条映射（映射表见 git 历史）与 D1–D6
  决策 ADR；本轮只测绘与决策，不改权限语义。

### Performance

- **增量 fetch 只发缺失对象**（T37）：`TreeContents` 改为按 haves 剪枝后的对象级差集。
  medium 数据集 20,224 对象 / 50 MB → 8 对象 / 5,973 B；large 201,112 对象 / 992 MB →
  8 对象 / 11,240 B。
- **pack 流式输出**（T38）：不再先拼进 `Vec<u8>` 再写 socket。large clone 服务端峰值 RSS
  2521 → **299 MiB**（净增量 2437 → 215 MiB，−91%），耗时 78.5 → 64.2 s；3 路并发
  6536 → **782 MiB**。V1 与 V2 两条路径都改（真实 `git clone` 默认 V2）。
  `IRONFORGE_PACK_THREADS` 可调生产端线程数：4 线程能到 197 MiB，但要付 +147% 耗时，
  默认不改。

### Fixed

- **CNB 导入真机验证炸出的 9 个缺陷**（T36）：approval 被丢弃、作者被记到账号 1、
  限流静默删除评论、milestone / release / wiki 报成功而没导入、标签颜色与未使用标签丢失、
  PR 评论成为孤儿、每条 PR 描述丢失、`state=all` 被拒、并发导入丢正文。
- **CI 错误不再被吞**：Gitea workflow 读取失败改为 fail closed；恢复被 T34 改坏的
  no-raw-git 守卫。
- **`jsonwebtoken` 10.3.0**：对应 CVE-25537。
- **gix 0.90 → 0.91**：随动的子 crate 一并升级。

### Known limitations

- large clone 峰值仍未到原定 200 MiB 验收线，按实测放宽到 320 MiB：根因是 gix 并行
  entry 流水线的工作集（16 路生产端各压一个 chunk），与 pack 大小解耦。
- 第三方 Action 只支持 composite；`node*` / `docker` 仍需 `actions-v1` runner（未实现）。

## [0.4.0] - 2026-11-25

本版本的主线是 **仓库迁移**：把"从别的平台搬过来"从"能跑"变成"四个平台都拿真实实例验过"。
新增 CNB 导入（issue / label / pull request），并把 Gitea、GitHub、GitLab、CNB 四条路各自跑了一次
真机迁移，修掉 13 个缺陷——几乎全部属于同一类：**迁移报告成功，而数据是错的、或数据在却取不出来**。

涵盖 119 个提交（feat 28 / fix 11 / docs 18 / test 4 / ci 2 / chore 1）。

### Added

- **CNB 导入**（`platform=cnb`）：issue、label、pull request。CNB 是自研 OpenAPI，与 Gitea/GitHub
  形状不同——响应是裸数组、`number` 是字符串、列表接口不含正文、无 milestone 概念。适配器把
  `CnbIssue` 转换成共享的内部形状，复用同一套入库逻辑，因此编号保留、标签、评论只有一处实现。
  合并状态（`is_merged`）、草稿（`is_wip`）、完整 ref（`refs/heads/...`）都在转换层翻译。
- **迁移结果的真机验证基线**：GitHub（51 PR）、Gitea（含 PR + review）、GitLab（含 MR + note）、
  CNB（34 PR）四条路都有可复现的实例与验证记录。

### Fixed（迁移层，13 个）

- **迁移把 issue / PR 重新编号**：编号与源站不符会打断正文和提交信息里所有 `fixes #3` 类引用，
  而迁移**报成功**。四条路径（GitHub/GitLab × issue/PR）改为保留源站编号（GitLab 用 `iid`）。
- **`state=all` 返回空列表**（五处：issues ×2、PRs ×2、milestones ×1）：`all` 被当作字面值去匹配
  状态列。所有 Gitea 风格的客户端都会传 `all`。
- **失败的空克隆报成功**：目标仓库在克隆前就被创建（含 `HEAD`），"已存在则跳过"因此命中自己刚建
  的空壳；而抓取到零个 ref 也是 `Ok`。改为按 ref 判断，并在失败时清掉目录。
- **往预先创建的目标目录克隆必然失败**：`gix` 拒绝在非空目录初始化。
- **GitLab 自托管实例被当作 gitlab.com 查询**、**OAuth token 用 `PRIVATE-TOKEN` 发送被拒**。
- **MR note 写进 `issue_comments` 且 `issue_id` 指向 PR**：孤儿行，统计计数而查询永远取不到。
- **Gitea 平台降级为纯 `git clone`**（issue / label / milestone / release 全部丢弃）。
- **Gitea / CNB 的字段差异**：milestone 无 `number`、release 的 `created_at` 为 `null`、
  `assignees` 为 `null`、CNB 的 `number` 是字符串。

### Changed

- 迁移的字段形状现在按**实际抓到的响应**写测试锁定，而不是按文档推断。

## [0.3.0] - 2026-11-19

本版本的主线是 **面向 agent 的协作层**：把"多人 / 多个 AI 代理同一仓库"从口头约定变成平台上可查询、
可审计、可阻断的能力。围绕它铺开了通用审批原语、事件投递可靠性、推送前密钥扫描、代码符号图与
Git 侧的新原语（cherry-pick / revert / merge-readiness / partial-clone）。

涵盖 699 个提交（feat 121 / fix 57 / refactor 44 / test 37 / docs 273 / ci 29），
任务编号 T01–T31 全部合并。

### Added（协作层，本轮最大增量）

- **任务（collab task）全生命周期**：规则入口、原子认领、接手历史、删除预览、完成状态；跨仓协调组
  （锚仓分组、双仓权限关联、先鉴权后分页、版本 CAS）；协作健康指标（平台事件表、覆盖语义、单仓健康与钻取）。
- **任务依赖图**（U01）：图版本、成环检查、有界遍历，配套 MCP 工具。
- **决定追溯**（U02）：不可变决定、唯一终结事件、新鲜度判定。
- **问答与工作包**：向指定人提问、未读徽章、阻塞通知、只读工作包。
- **完成条件**（O04）：用户自定义条件、确认快照、按 `client_request_id` 幂等，单任务条件集与快照各限 200 版。
- **任务模板**（O06）：不可变版本，任务固定模板版本并保留快照。
- **任务预算与用量**（O08）：单位与来源必填、不跨单位求和、缺失不当零、无自动暂停。
- **通知**：每用户通知偏好、单一投递出口、送达回执与持久通知、惰性评估升级（去重 + 静默窗口）。
- **声明路径重叠警告**（U03）、**字节预算任务上下文包**（U05）、**个人收件箱已读标记**、
  **能力发现与契约兼容**（X01）、**会话能力/环境声明**（O07，自报、带版本、过期标记、无授权含义）、
  **诊断端点与 MCP 诊断工具**（O08/O10）。
- **Agent 主体与身份**：agent principal、token 身份、审计 `on_behalf_of` 记录"代理为谁执行"；
  "user door 只服务人、agent 必须走 session"的人机分流；通知发给 agent 的 owner 而非 agent 本身。
- **Agent 草稿分支命名空间**：带 TTL、不触发通知、可跑 CI。

### Added（通用审批与事件可靠性）

- **通用审批原语**（`#19`）：审批表、状态机、求值器，以及 `scope_expand` 的执行面（目标 scope 绑定进票据）。
  同时修复了 PR 评审越权。
- **PR 头漂移阻断**：push 时同步 PR 头，指针过期时以 `HEAD_SHA_STALE` 阻断旧票合并。
- **事件写入事务化**（`#15`）：`issue.opened`、`pull_request.merged`、`release.created` 在各自事务内落库，
  仅在提交后投递；`pr.closed` 事件顺序修正。
- **事件信封与投递**：统一信封、投递头、5xx/429 入队重试；最小 outbox（投递状态 + 兜底补投）。
- **CI 配额与用量**（`#40`）：配额覆盖 push 触发、精确错误码；用量计数 `usage_counters`。

### Added（代码理解与 AI）

- **符号图**（`#63`）：符号定义表、写时裁剪（保留近 N 次提交）、查询端点与 `find_definition` MCP 工具；
  记录**每提交的索引完整度**，使 `partial` 标记有实际含义。
- **结构化大纲抽取**（`#39`，5 种语言）；repo map 端点，带 token 预算裁剪。
- **Agent 客户端 API**：PR 上下文端点、PR 创建 dry-run、PR 导出、REST 提交的身份与 push 钩子、
  `commit_files`（多文件一次提交、一次 CI）、streamable-HTTP 传输（`ironforge-mcp --http`）。

### Added（Git 能力）

- **partial-clone filter 生效**：`blob:none` / `blob:limit` / `tree:0` / `combine:` / `object:type=` 不再降级为全量。
- **服务端 cherry-pick / revert**（`#48`）：端点 + MCP 写工具。
- **合并就绪度**（`#17`）：`merge_analysis`（冲突文件/类型/区块、合并提交对象、带期望值的 ref 更新）、
  `GET merge-readiness`、`POST update-branch`。
- **`apply-patch` 端点**（`#62`）：接受 unified diff。
- **分支删除**：REST 与 `git push --delete` 走同一道守卫，SSH 侧补审计。
- **`refs/pull/<n>/head`**、release 按 tag 查询、`repair-pull-refs`（补齐从未建出 pull ref 的 PR）。

### Added（安全）

- **推送前密钥扫描**（`#38`）：在 ref 移动前扫描新引入的 blob（私钥、云凭证、平台 token、高熵串告警），
  支持 allowlist 与行内抑制标记，审计事件经 sink 上报。
- **PAT 最小权限**：仓库白名单、分支白名单、仓库 scope 阶梯；绑定到指定仓库/分支的 token 无法越界。
- **路由元数据注册表**（R1）：四张权限表由注册表派生，附漂移测试。
- **CI 容器隔离**（`#41`）：作业容器默认以调用进程 `uid:gid` 运行（此前为容器默认用户）。
- 首次注册用户自动成为管理员。

### Added（前端）

- **协作 UI**：任务列表与详情、收件箱、问答应答/取消、依赖编辑器、用户改派、接手历史、预算卡片、
  决定与只读工作包、完成条件卡片（含快照）、跨仓协调组与健康页、协作模板/搜索/规则/设置页。
- **设计系统收口**：单一按钮系统、全局表单控件默认值与 `FormField`、共享对话框/toast/空态/加载态、
  `ErrorBanner` / `SuccessBanner` / `.surface`、全局标题尺度与页头布局。
- 表单错误改为交互后显示并聚焦首个非法字段；`ApiError` 保留 `error.details`。
- 原创 IronForge 锻造标识替换占位 logo；中英双语补全。

### Added（运维）

- `ironforge create-user` 运维建号；`GET /users/{username}` 公开资料；组织成员列表补 `username`/`display_name`。
- 可观测性：WS 通道序列预先物化为 0、CLI 视角指标。

### Changed（行为变化，升级时注意）

- **数据库迁移 20+ 张新表/新列**（`collab_*`、`repo_events`、`idempotency_keys`、`usage_counters`、
  `authorization_requests`、`notification_preferences`、`symbol_*`、`agent_draft_namespaces` 等），
  迁移文件总数 108。启动时自动迁移，已有实例升级前请先备份。
- **Webhook 信封**改为统一信封，接收方若解析旧结构需同步调整。
- **`pull_request.merged` / `release.created` / `issue.opened`** 改为事务内写入，投递发生在提交之后。
- **MySQL** 下按 namespace 强制 live 仓库名唯一。
- 备份恢复默认进入 quiesce 状态；恢复前先校验备份，并新增 O03b 恢复演练脚本。

### Fixed

- PR 合并写入 `pr.base_branch` 而非仓库 HEAD（`#60`）。
- 自批守卫下沉到 core；PR 评审越权修复；agent 判定统一到 `is_human_account`。
- `auto_merge_pinned_head_tests` 的 flaky（不再与 post-push 钩子竞争）。
- `test_no_raw_git_command_in_crates` 守卫发现的裸 `git` 调用（T16 测试夹具改走网关）。

### Documentation

- `CLAUDE.md` / `ARCHITECTURE.md` / `AGENT.md` 按 T30 刷新到当前实现现状；T01–T31 的计划与结项文档
  由 git 历史保留；一致性对比报告推进到 v3.1。

### Quality

- `cargo test --workspace` 全通过，0 失败（2026-11-19 实跑）；`cargo fmt --check` 与
  `cargo clippy --workspace --all-targets -D warnings` 均通过。
- 前端 `svelte-check` 0 error（48 条 Svelte 5 `state_referenced_locally` 警告）。

### Known limitations

- **mirror 的 clone 与同步仍走 git CLI**（同 0.2.0）：gix 无 `fetch --prune` 等价能力，且 clone 后清理
  上游带入的 `refs/pull/*` 无对应 API。已改为两条显式 refspec + transport lockdown，
  见 `CLAUDE.md` 技术债表。
- Pack 生成不做 delta 压缩（全量对象，功能正确，大仓带宽略高）。
- 无内建 gc/repack 维护命令。
- 多实例部署的进程内状态未打通：权限缓存（30s TTL）、限流令牌桶、通知去重窗口各自独立计数。

## [0.2.0] - 2026-09-07

本版本的核心里程碑：**核心热路径 git CLI 调用归零，100% gix 原生**。运行时不再依赖系统安装的 git
完成 Git 协议、对象读写与 pack 生成/摄取；仅 mirror 的 clone 与同步仍调用 git CLI（见下方 Known
limitations），部署即拷贝单个二进制 + 静态前端，消除了热路径上的 CLI 版本漂移、stderr 解析与子进程管理问题。

### Changed（架构级，breaking）

- **git CLI 退役**：原经 `GitCommandGateway` 调用 git CLI 的全部 11 处生产路径（pack 生成/摄取、
  rev-list、fork fetch、文件编辑提交、验签等）替换为 gix 原生实现；`cli_gateway.rs` 现仅用于测试造数据
  与 mirror 的 clone/同步，防回归守卫 `test_no_raw_git_command_in_crates` 继续生效。
- 新增 gix 原生底层能力（`rg-git/src/ops.rs`）：`commits_between` / `commit_graph` / `generate_pack` /
  `pack_universe` / `pack_for_wants` / `index_pack_bytes` / `copy_objects_from_source` / `commit_tree_edits` /
  `verify_commit_details_with_repo`。
- fork 同步改为进程内对象拷贝（`copy_objects_from_source`），不再 spawn `git fetch`；
  fork 创建不再 `clone --bare`，改为 gix 裸库创建 + 引用复制 + pack 复制。
- `/health` 的 git 检查项由探测 CLI 网关改为直接报告 `"builtin"`（gix 进程内提供，恒可用）。

### Known limitations

- Pack 生成不做 delta 压缩（全量对象，功能正确但大仓带宽略高）。
- 不支持 partial-clone filter（`--filter=blob:none` 等客户端选项收到时降级为全量）。
- 无内建 gc/repack 维护命令（SQLite + loose/pack 对象随时间可手动维护）。

### Documentation

- README / ARCHITECTURE / CONTRIBUTING / CLAUDE / AGENT / docs/git-protocol 全面与
  "CLI 已退役" 现状对齐；迁移过程记录见 git 历史。

### Quality

- 全 workspace 416 个测试通过（0 失败）；touched crates clippy 无警告。

### Added（2026-10-23 增量，随 v0.2.0 首个公开发布一并交付）

- **CI 工作区改为 gix 原生 linked worktree**：`Repository::add_worktree` / `remove_worktree` 替换
  `git worktree add --detach` / `remove --force`（新增 `worktree-mutation` feature）。`Force::DiscardChanges`
  等价 `--force` 但仍尊重 operator lock；崩溃残留的注册会在下次准备时自动 prune。
- **导入裸克隆改为 gix 原生**：`prepare_clone_bare` + `fetch_only` 替换 `git clone --bare`，
  私有源继续走 git credential helper。
- **PR patch 对拍扩展**：尾随空行增删、无尾换行 + 末行删除、纯删除 + 纯新增混合，与 `git diff` 仍字节级一致。
- **依赖升级**：gix 0.87.1 → 0.90.0（错误体系并入分类式 `gix::Error`、`merge_base` 返回 `Option`、
  移除 `gix::features::{progress,parallel}`），russh 0.51.1 → 0.64.1。

### Security（2026-10-23）

- 关闭代码审计 #1 的 7 项：PAT 改用 `OsRng` 256-bit、SSO cookie 改 HMAC-SHA256、出站 HTTP 复用受限 client、
  认证端点常开限流、Git 流式传输加 deadline、host runner fail-closed、release 资产附 SHA-256 校验和。
- 全仓声明 MIT 许可：workspace `license` 继承 + LICENSE 全文 + `web/package.json`。

### Known limitations（发布时补充）

- mirror 的 clone 与同步仍走 git CLI：gix 0.90 的 clone fetch 不按根级 `refs/*` 通配写 ref，
  且没有 `fetch --prune` 等价能力；枚举命名空间会丢失未知命名空间，属语义回退。
  现状与解除条件见 `CLAUDE.md` 技术债表。

## [0.1.0] - 2026-06 之前

初始开发版本（内部迭代，Phase 1~19：Git 协议服务端、SSH/HTTP 传输、REST API、CI、SSO/MFA、
PR 工作流、LFS、OCI registry、MCP 集成等）。未发布正式 tag。
