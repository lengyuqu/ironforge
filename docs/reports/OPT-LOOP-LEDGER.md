# OPT-LOOP 账本（IronForge）

> 协议来源：`/Users/yuqu/Documents/mybook/Projects/CNB-NPC-LOOP/README.md`（v0.3）。
> 本文件是这套闭环的**跨轮记忆**。规则：**纯追加**；标为「不成立」的条目禁止删除（否则同一错误会被无限重放）。
> 本文件不是产品文档，不参与 `docs/` 的漂移巡检范围。

## 轮次语义（照抄协议 §3 / §4.2，勿改）

- **issue = 容器**：`open` → 6 轮 → `close` + `BATCH_COMPLETE`
- **评论 = 进度**：以 `## 第 K 轮收尾` 开头的评论条数 `C` = 已完成轮数。单次运行被切断后，下次运行数评论即知从 `C+1` 续跑——进度可从平台侧完整重建，不依赖本地文件。
- **本文件 = 记忆**：「不成立」清单 + 批次索引 + 每轮判定与 SHA。
- **单发模式**（非 `[opt-round]` 容器的存量 issue）回帖首行用 `## 本轮收尾`，不带轮次号，不参与批次计数。

## IronForge 本地化参数

> 协议警告：§2.2 的延迟参数是 agent2ssh 单仓库实测（n=19，p100 20min46s），**不可直接沿用**。本仓库规模大一个量级（10 crate / 191K 行 Rust / 664 个 `.rs` / 20K 行文档），须自行标定。

| 参数 | 当前值 | 状态 |
|---|---|---|
| 轮询间隔 | 60 s | 沿用协议 |
| 等待上限 | 起始 60 s × 30 | 待标定（本批 n=1） |
| **首扫延迟（实测）** | **3 min 43 s**（`09:21:41Z → 09:25:24Z`，n=1）；复核 **4 min 22 s**（n=2）、**2 min 16 s**（n=3）、**5 min 20 s**（n=4）、**28 min 15 s**（n=5） | 与 agent2ssh 的 min 2:31 同量级；仓库大一个量级但首扫未变慢，因为 NPC 只读文件不构建。**n=5 破了 p100**：那轮它自己 `cargo build --release`（23 m 28 s）做实测复现。即「NPC 要不要构建」是延迟的唯一尾巴，不是排队 |
| 验证命令（文档轮） | `scripts/check.sh --quick` | **实测 115 s 冷 / 2 s 热**（文档写 ~40 s，是忽略 clippy 首次编译的口径）。**证据形态（batch#4 第 4 轮立）**：入账只认「executor 本机 `EXIT=0` + clippy `Finished` 行原文 + 时刻」；**时刻**可以是 UTC `HH:MM`，也可以是**可复算的时间锚**（该节提交的 author date、日志时间戳）——后者的好处是不靠记忆、`git log -1 --format=%aI` 即可重放；复算环境无 `cargo` 时本项**不可验**（第 3 轮 NPC 容器即此类），回帖须写明「executor 本机」，不得表述为公共可复算事实 |
| 验证命令（代码轮） | `scripts/check.sh`（约 7 min） | 已确认存在 |
| 验证命令（收批） | `scripts/check.sh --full` | 全 workspace |
| 本机/本仓提交身份 | `cnb.biewen` | 实测（issue #11 作者 = 本机推送身份） |
| 已知 flaky | webhook 重试时序、`merge_queue_ci_tests`（环境） | 差分判据须 allowlist |

### 分级口径（本项目专用，覆盖协议 §4.4 的通用表）

| 级别 | 范围 | 动作 |
|---|---|---|
| **L1** | `docs/`、`ironforge-docs/`、`CLAUDE.md`/`AGENT.md`/`ARCHITECTURE.md`/`CONTRIBUTING.md`/`README.md` 的文档与注释；只增不减的断言强度；i18n 键值一致性 | 修 + 验证 + 提交 + 推 cnb |
| **L2** | DB 迁移顺序、Git 协议线格式（pkt-line / sideband / `--fix-thin`）、认证权限语义（LDAP/SSO/TOTP/JWT）、gix↔CLI 替换、OpenAPI 与前端契约、任何改 schema 的 | **不动 main**，写进待拍板清单 |
| **禁区** | `.cnb.yml`、`.github/`、`scripts/`、`Cargo.toml`/`Cargo.lock`、`deploy/` | 只报告 |

---

## 批次索引

### batch#5(issue #31) · 2026-11-11 · rg-ci 引擎（runner/condition/gitea_actions）

- 开批：`2026-11-11T09:46:31Z`（issue 创建即触发，正文结尾 `@CodeBuddy`，标题已绑定 `[opt-round] batch#5(issue #31)`）
- 基线提交：`d32421e`（cnb/main HEAD，2026-11-11；本机 main 检出同点、工作区干净，直接本地记账，无需 worktree）
- 范围：`crates/rg-ci/src/` 5 文件共 4860 行（runner 1857 / gitea_actions 1297 / lib 1065 / config 326 / condition 315）。行数以 `git show d32421e:<path> | wc -l` 统计
- 选靶说明：候选顺序首位 `crates/rg-db/src/migrations/` 全量 15209 行/100+ 文件，无法在 ≤10 文件且 ≤5000 行内切成连贯子集，改选下一候选 rg-ci
- 引导问题 4 条：P2 并发组算而不用（`lib.rs:136-158` group 只进日志/409 文案，互斥取消实际按 `find_active_pipelines_by_ref` 的 ref_name 判定，`pipeline_ops.rs:1315-1340`）；P2 Docker 路径超时不杀进程（`runner.rs:687-697` docker Command 无 kill_on_drop，对照本地 `:561`/`:576`；容器名 `:651` 重跑冲突）；P3 step env 未加引号拼 `export k=v`（`gitea_actions.rs:463`）；P3 PR branches 过滤用 default_branch 近似 + step 条件求值错误被 `unwrap_or(false)` 静默吞（`gitea_actions.rs:301-311` / `:420`，与 native 路径 `lib.rs:242-252` 的 `?` 传播不一致）
- 反向清单回灌：**不成立 38 条全量**（batch#1–#4 累计，含 batch#4 第 6 轮新增 36–38）+ 我方自证推翻 8 项 + 对方已撤回 6 项 + 对方错项 5 条 + batch#4 第 1 轮错项 4 条，均注明「不要重复提」
- 上一批遗留回读（协议 §6.2 开批前三步）：batch#4 批后复核评论 `2120444501616406528`（`2026-11-11T09:12:19Z`，关批后 +49 s）——第 6 轮 7 项复核全部通过、无异议，36–38 不再重提，B4-1..B4-8 维持 L2；一条行数口径备注（我方记 1055 / 它实测 1056，末行换行计数差异）——本批起台账行数统一以 `wc -l` 输出为准
- NPC 首扫：`2120454308016586752`，`2026-11-11T09:51:17Z`（开批后 4 min 46 s；首行 `@cnb.biewen(别问) ## batch#5 第 1 轮 · rg-ci 引擎只读审查`，自述基线 `d32421e` 且逐文件核过行数 1857/1297/1065/326/315）
- NPC 第 2 轮复核：`2120465837906632704`，`2026-11-11T10:37:06Z`（首行 `@cnb.biewen(别问) batch#5 第 2 轮。先说环境：本机无 cargo…`，自述复核 `6b9a907`，自报 `check.sh --quick` EXIT=0 于 `10:36:22Z` 的 Linux 机）
- NPC 第 3 轮复核：`2120479451631763456`，`2026-11-11T11:31:12Z`（首行 `## 第 3 轮复核`；两个提交复核通过，主诉求是 B5-10 的机制/优先级订正 + 4 处行号订正，其中 2 处是它自订、1 处指我方 `validate_repo_name` 区间、1 处仅记录；本机无 cargo，未跑验证）
- 6 轮进度：**6 / 6（已收批）**（各轮见文末 `consume <K> / 6（batch#5）` 节，按 K 升序排列；第 1–3 轮在前，第 4–6 轮在文件末尾）

### batch#4(issue #27) · 2026-11-11 · rg-http issue/PR/review API 层

- 开批：`2026-11-11T02:43:08Z`（issue 创建即触发，正文结尾 `@CodeBuddy`，标题已绑定 `[opt-round] batch#4(issue #27)`）
- 基线提交：`b7a0ab2`（cnb/main HEAD，2026-11-11；本机 main 检出同点、工作区干净，直接本地记账，无需 worktree）
- 范围：`crates/rg-http/src/api/` 4 文件共 4086 行（issues 1681 / reviews 1261 / pulls 902 / labels 242）。行数以 `git show b7a0ab2:<path> | wc -l` 统计
- 引导问题 4 条：P2 死校验 `validate_optional_text(...).ok()` 丢弃 Err 共 4 处（`issues.rs:531-536`/`:653-658`、`labels.rs:143-148`、`pulls.rs:293-298`）；P2 `update_issue` 鉴权前取值（`issues.rs:603-606` 在 `:635` can_read 之前调 `get_issue`，404/403 可区分私有仓 issue 存在性）；P3 label 颜色 create/update 双口径（3 位 hex 创建 201、更新 400，`labels.rs:139-142` vs `rg-core/src/label/service.rs:92-99`）；P3 列表页 assignee N+1（`issues.rs:1176`，milestone 已批量、assignee 未）
- 反向清单回灌：**不成立 33 条全量**（batch#1/#2/#3 累计，含 batch#3 第 2–6 轮新增的 23–33）+ 我方自证推翻 8 项 + 对方已撤回 6 项 + 对方错项 5 条，均注明「不要重复提」
- 已知空档新增：issue 创建只需读权限为刻意设计（`issues.rs:500-504` 注释，ISSUE-105）；`pulls.rs:627-629` dry_run 不评 conflicts 为注释自述的已知语义
- 凭据事故（下次照做）：开批前 `git fetch cnb` 报 Repository Not Found；第 9 次配方（`username=oauth2` 灌 keychain）**覆盖不中**——keychain 实际条目 `username=cnb`，须以 `username=cnb` 覆盖 `git credential approve` 才恢复
- NPC 首扫：`2120347578683269120`，`2026-11-11T02:47:11Z`（开批后 4 min 03 s；`author.is_npc=true`，首行 `@cnb.biewen(别问) batch#4 只读审查完成`，自述基线 `b7a0ab2` 并逐文件核过行数）
- 6 轮进度：**6 / 6（已收批）**（第 1 轮收尾 `2120361552162230272`、第 2 轮收尾 `2120377967560794112`、第 3 轮见 `consume 3 / 6（batch#4）` 节、第 4 轮见 `consume 4 / 6（batch#4）` 节、第 5 轮见 `consume 5 / 6（batch#4）` 节、第 6 轮见文末 `consume 6 / 6（batch#4）` 节）

### consume 1 / 6（batch#4）

- 收尾：`2026-11-11T03:42:43Z`，评论 `2120361552162230272`，K=1（回读 C=1，结尾 `@CodeBuddy` 独占末行）。BASE **`8c24c44`**（issue 声明 `b7a0ab2`；`git diff b7a0ab2..cnb/main -- crates/rg-http/src/api/` **空** ⇒ 零漂移，4 文件行数 1681/1261/902/242 与题述一致）
- 验证：`scripts/check.sh --quick` 改前 **2 s EXIT=0** → 改后 **57 s EXIT=0**
- 改动：`ef067be`（`crates/rg-http/src/api/validation.rs` 2 处注释，2+/2−，**行号零漂移**：`:49` 与 `:76-77`），推 `cnb`（`8c24c44..ef067be`）

判定表（NPC 4 条引导问题 + 4 条新发现）：

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| Q1 | 4 处 `validate_optional_text(...).ok()` 丢弃 Err，校验形同虚设 | **成立** | `sed` 逐处打：`issues.rs:531`→`.ok()` 在 `:536`、`issues.rs:653`→`:658`、`labels.rs:143`→`:148`、`pulls.rs:293`→`:298`。全仓调用点 6 处（+`wiki.rs:190`/`:260`），`grep -rn -A6 "validate_optional_text(" . \| grep -E "let .* = .*validate_optional_text"` = **0** ⇒ 无一处传播 |
| Q1-订正 | 实际上限是 axum 默认 **2 MiB**，不是「任意长度」 | **成立** | `grep -rn RequestBodyLimit rg-http/src/` 共 9 处全挂在 git/upload/runner-cache/assets 路由；issue 路由 `lib.rs:782-788` 无 layer；`axum-core-0.5.6/src/ext_traits/request.rs:319` `const DEFAULT_LIMIT: usize = 2_097_152; // 2 mb` |
| Q1-兜底 | core 侧无长度兜底 | **成立** | `issue/service.rs:23-27` 只拦空 title；`:246-248` 直接 `active.body = Set(Some(b))`。`label/service.rs` 的 description 全文件无长度判断（`grep -n description` 仅 39/59/79/100-101/116/243） |
| Q2 | `update_issue` 在鉴权前取 issue ⇒ 404/403 可区分私有库存在性 | **成立** | `:603` `get_issue` → `:635` `if !can_read`；`existing` 只在 `:639` 用（`existing.author_id != user_id`）⇒ 完全可以后置。对照 `get_issue` handler 与 `labels.rs:61`/`:91` 都是先 `require_*` |
| Q2-订正 | 我给的对照例 `labels.rs:61-64` 引错，`:61` 是 `list_labels` 的 `require_read`，`get_label` 在 `:86-92` | **成立** | `labels.rs:56` `pub async fn list_labels` / `:61` `require_read`；`:86` `pub async fn get_label` / `:91` `require_read` |
| Q3 | label 颜色 API 层与 core 层口径不一致 | **成立（但真实缺陷与题述不同）** | `validate_color` 只有 1 个调用点（`labels.rs:139`，create 侧；update 侧 0 调用）。core `create_label:47` `if !color.starts_with('#') \|\| color.len() != 7`、`update_label:92-99` 同判据 ⇒ `#f55` **两次都 400**，题述「201 vs 400」错；真问题 = `validation.rs:76` 文档与 `:83` 文案承诺 3 位，端到端不可用。**本轮已改 `:76-77` 文档**，行为未动（B4-3） |
| Q4 | 列表页 assignee N+1，milestone 已批量 | **成立** | `issues.rs:1174` `for issue in issues` 内 `:1176 assignee_names_by_issue`；对照 `:1170` `milestone_ops::titles_by_ids` 批量。`assignees.rs:91-99` = `list_user_ids_by_issue` + 逐 user `find_by_id` ⇒ 单 issue 是 1+N 而非 1。`issues_with_authors` 调用点 `:333/:365/:406/:429` 四处全中 |
| Q4-补充 | `rg-db` 无 assignee 批量函数 | **成立** | `grep -n "pub async fn" rg-db/src/ops/issue_assignee_ops.rs` = `set_assignees:14` / `list_by_issue:55` / `list_user_ids_by_issue:65` / `find_issue_ids_by_user:74`，全是逐 issue 入口，无 `*_by_ids`；对照 `milestone_ops::titles_by_ids:127` 才是批量形 |
| F1 | `update_pr` 零正文校验 | **成立**（区间终点偏） | `pulls.rs:456-500` 函数体内 `grep -c validate` = **0**（它标 `:456-516`，`:502` 已是下一个 `get_diff` 的 utoipa 属性）；core `pull_request/service.rs:376-378` `if let Some(b) = body { pr.body = Some(b); }`，仅 `:370-375` 拦空 title。对照 `create_pr:285-298` 有 title+body 两道 |
| F2 | merge_queue 列表 N+1 | **部分成立** | N+1 成立（`pulls.rs:781` 循环内 `:782 Entity::find_by_id`，`:776` 先 `list_by_repo`）。但「`:787` 的 `continue` 会让跳过项不影响编号」措辞不成立：`:792 position: index + 1` 用的是**含被跳项的 enumerate 下标** ⇒ 跳过会在编号上留空档（首项被跳则首条 position=2）。「不向后平移」这点是对的 |
| F3 | `list_requested_reviewers` 静默丢行，与 `request_reviewer` 口径不一致 | **部分成立** | 静默丢行成立：`reviews.rs:1025` 循环、`:1029` `Ok(None) => continue`（无日志）。**「`request_reviewer` 能定位到软删用户」不成立**：`:1075` 守卫 `Ok(Some(user)) if user.is_active && user.deleted_at.is_none()`，`:1076` 对其余一律 404 ⇒ 软删用户两边都不可见，差的只是「静默省略」vs「显式 404」。另它给的区间 `:1062-1072` 打不到 `find_by_username`（真值 `:1074-1078`） |
| F4 | 无超长 body 回归测试 | **成立** | `grep -rn "repeat(" rg-http/tests/` = 35 命中，全部属 collab/sso/idempotency/repo_events/file_commit 域；`grep -rn "65536\|65537" rg-http/tests/` 仅 1 命中且是 `collab_context_tests.rs:255` 的 `max_bytes`；`api_contract_validation_tests.rs` 只有 1 个 test（畸形 body vs OpenAPI 状态码） |

**它本轮的行号错项（勿当结论引用）**：F1 区间 `:456-516`（真值 `:456-500`）；F3 区间 `:1062-1072` 不含 `find_by_username`（`:1074`）；Q3 取证命令 `sed -n '48,52p' rg-core/src/label/service.rs` 打不到它所引的 `if !color.starts_with('#')…`（该行是 `:47`，区间应为 `:47-51`）；F2「跳过项不影响编号」。

**新立待拍板（batch#4，本流程不动 main）**：

1. **B4-1（P2）** **6 处调用 / 3 个常量 / 6 个失效上限**：`issues.rs:531`（`MAX_BODY_LEN`，实参 `:533`）、`issues.rs:653`（`MAX_BODY_LEN`，`:655`）、`pulls.rs:293`（`MAX_BODY_LEN`，`:295`）、`labels.rs:143`（`MAX_LABEL_DESC_LEN`，`:145`）、`wiki.rs:190`/`:260`（`MAX_COMMIT_MSG_LEN`，`:192`/`:262`），六处全部以 `.ok()` 收尾。后果：`MAX_BODY_LEN` 65536 / `MAX_LABEL_DESC_LEN` 255 / `MAX_COMMIT_MSG_LEN` 8192 全部不生效，唯一实际上限是 axum 默认 2 MiB（`axum-core-0.5.6/src/ext_traits/request.rs:319`），且 core 无兜底。修法改响应码（超长从「静默接受」变 400）⇒ OpenAPI/前端契约 ⇒ L2。**本轮只在 `validation.rs:49` 文档化，未改行为。**
2. **B4-2（P2）** `update_issue`（`issues.rs:603`）鉴权前取 issue ⇒ 私有库 404/403 oracle。修法把取 issue 移到 `:635` 之后（`existing` 仅 `:639` 用）⇒ 错误码语义变化 ⇒ L2。
3. **B4-3（P3）** `validate_color` 接受集合 ⊋ core 接受集合：3 位 hex 端到端恒 400（`labels.rs:139` → `label/service.rs:47`/`:92-99`）。二选一：`validate_color` 收窄到 6 位，或 core 展开 3 位。前者改 API 接受面、后者改 core 语义 ⇒ L2。本轮仅订正 `validation.rs:76-77` 注释。**附（第 2 轮 NPC F-1 成立）**：`:83` 的 400 文案仍写 `"color must be a 3 or 6 digit hex code (e.g. #ff5500)"`，对 3 位值永不成立；改文案属**用户可见响应体**（前端可能直显、OpenAPI 示例同步）⇒ 与本条一并待拍板，不在 L1 单独改。
4. **B4-4（P2）** `update_pr`（`pulls.rs:456-500`）零正文校验，core `pull_request/service.rs:376-378` 直存 ⇒ PATCH 可写任意长度 PR body（F1）。补校验改响应码 ⇒ L2。
5. **B4-5（P3）** 列表 assignee 1+N（`issues.rs:1176` + `assignees.rs:91-99`），`rg-db` 缺 assignee 批量函数；`issues_with_authors` 四个分支（`:333`/`:365`/`:406`/`:429`）全受影响。属性能与 ops 层新增 ⇒ L2。
6. **B4-6（P3）** `merge_queue` 列表逐条 `find_by_id`（`pulls.rs:782`，`:787` 跳过后 `position: index + 1` 留空档）。
7. **B4-7（P3）** `list_requested_reviewers`（`reviews.rs:1029`）静默丢弃指向已删用户的请求，`request_reviewer`（`:1076`）同情形显式 404 ⇒ 同一数据两种口径。
8. **B4-8（P3）** 缺超长 body 回归测试（`rg-http/tests/` 无 issue/PR body > 65536 用例）——这是 B4-1/B4-4 能长期存在的直接原因。

**回填**：`2120361552162230272`（`2026-11-11T03:42:43Z`），回读 C=1。

### consume 2 / 6（batch#4）

- 输入：NPC 复核 `2120361953641005056`（`2026-11-11T03:44:18Z`，首行 `@cnb.biewen(别问) Review of ...`）——晚于我方第 1 轮收尾（`03:42:43Z`）⇒ **有新复核**，K=2
- 本机 `BASE=070519c`；`git fetch cnb` 后 `HEAD...cnb/main` = `0 0`（零漂移）；改前 `scripts/check.sh --quick` **2 s EXIT=0**（`host=Mac` / `uname -sm` = `Darwin arm64` / `cargo 1.99.1` @ `/Users/yuqu/.cargo/bin/cargo`；clippy 段 `Finished dev profile … in 0.26s` = 热缓存指纹命中）
- 改动：**仅台账**（无源码、无 `docs/` 正文改动）——本轮 8 条输入里有 4 条落在台账自身的行号引用与计数口径上

判定表：

| # | 它说的 | 判定 | 证据与处置 |
|---|---|---|---|
| D-1 | 台账批次索引的 ④-3 行（它报 `:152`）「见『判定不成立』第 **23** 条」指错，真值应是第 **31** 条 | **不成立** | 它报的 `:152` 是 ④-2 所在行，④-3 紧随其后。④-3 原文：`perm_cache_disabled() 在 check_perm_cache:97-99 短路 ⇒ 测试环境不付这个锁，14 处调用才没在 CI 里炸`；反向清单第 23 条逐字同命题（`perm_cache_disabled()` 入口短路 →「13/14 处调用一直没在 CI 里炸」）⇒ **指针指向正确**（此处「引用正确」只指 ④-3 的条号指向对不对，不构成对指摘内容的采信——第 23 条本身就是已判死的条目）。它说的「第 23 条是跨 session 数据混用」实为第 **25** 条（`escalation.rs:283` `contains_key` 记忆化）；第 31 条讲 `require_collab_read` 每请求清 2 次 / 台账 B3-9 没写，与本行无关。**已核实为不成立，不要重复提（见反向清单 35）** |
| D-1 附 | 「反向清单内部有三连空行，按空行计数会漂移」 | **成立（唯一有效部分）** | 按 `$0==""` 累计扫该清单全段，命中「第 24 条 → 下一段 preamble」之间的三连空行（`run=2` / `run=3` 两次命中）⇒ 确有其事，已压成单空行。它据此推「清单不可机械计数」不成立：`grep -c '^[0-9]\+\. '` 只认行首编号，与空行无关 |
| D-2 | B4-1 称 6 处却只列 3 个常量「读起来像不匹配」；并称真值是 **7 个失效上限 / 5 个不同常量** | **部分成立**（表述可改，两个数字都错） | `grep -rn 'validate_optional_text(' crates/` = **6 调用点**（`issues.rs:531`/`:653`、`labels.rs:143`、`pulls.rs:293`、`wiki.rs:190`/`:260`）+ 1 定义；实参逐个打印：`MAX_BODY_LEN` ×3（`issues.rs:533`/`:655`、`pulls.rs:295`）、`MAX_LABEL_DESC_LEN` ×1（`labels.rs:145`）、`MAX_COMMIT_MSG_LEN` ×2（`wiki.rs:192`/`:262`）⇒ **3 个常量**、每处只吃一个 `max`（`validation.rs:50-62`）⇒ **6 个失效上限**，不是 7；常量不是 5。已把 B4-1 首句改为「6 处调用 / 3 个常量 / 6 个失效上限」并列出对应关系 |
| E-1 | 「行号零漂移」只是半句；`validation.rs:49` 现为 **144 列** | **成立**（其括注自相矛盾） | `awk 'NR==49{print length($0)}'` = **144**；`find . -maxdepth 3 -name '*rustfmt*'` 空、`Cargo.toml`/`.cargo/` 无 `[rustfmt]` ⇒ `wrap_comments` 是 rustfmt **默认 false**，fmt 关不会折它。它自称「`awk 'length>140'` 只命中两条 `// ──` banner」与命令本身矛盾：同命令共三行（`:9`=180 / `:21`=198 / `:49`=144），第三行就是被讨论的这一行。处置：台账记「零漂移 ≠ 格式洁净」，今后不把行数不变当格式证据；**不重排该行**（同文件 198/180 列先例，reflow 反而制造行号漂移） |
| E-2 | 无法复现 `check.sh --quick` 的 EXIT=0（容器无 cargo），且 2 s→57 s 对「只改两行注释」不合常理 | **部分成立**（补环境口径成立；「不可复现」不成立） | 本机有 cargo：`/Users/yuqu/.cargo/bin/cargo` = **1.99.1**（`Darwin arm64`，host `Mac`）。2 s 可复现且可解释：热 `target/` 下 clippy 走指纹判定，本轮实测 `Finished … in 0.26s`、整脚本 **2 s EXIT=0**；改注释后 `rg-http` 及下游重编 ⇒ 57 s，量级自洽。另「本 workspace 只有一个 remote `origin`=cnb.cool」是**它容器**的事实：本仓 `git remote -v` = `cnb`(cnb.cool) / `github` / `origin`(git233) 三条 |
| F-1 | `validation.rs:83` 的运行时文案仍承诺「3 or 6 digit」，而 3 位端到端必 400 | **成立，本轮不改** | `:83` = `"color must be a 3 or 6 digit hex code (e.g. #ff5500)"`；`:81` 放行 `len()==3 \|\| len()==6`，core `label/service.rs:47`（create）/`:93`（update）要求 `starts_with('#') && len()==7` ⇒ `#f55` 两次都 400。改它动的是**用户可见响应体文案** ⇒ 落 OpenAPI/前端契约面，记入 B4-3 待拍板，不在 L1 |
| F-2 | 台账「运行要点 · F3-12」条引 `check.sh` 的 `:45-49`，其中 `:49` 是空行 | **成立**（且同句 `:38-39` 也错） | `awk` 打 `scripts/check.sh:32-56`：`:36 step "cargo fmt --all --check"` / `:37 cargo fmt …` / `:44 if [ "$mode" != "quick" ]` / `:45 step` / `:46 cargo test` / `:47 elapsed` / `:48 fi` / `:49` **空行** / `:50 if [ "$mode" = "full" ]`。已改为 `:36-37` 与 `:44-48`（跳过的是 `:45-47`） |
| F-3 | `sed … \| grep -c validate` 返回 `0` 且 EXIT=1，别把 0 读成命令失败 | **成立（口径补充）** | `grep -c` 语义：零命中即退出码 1、stdout 仍为 `0`。台账 B4-4 的「= **0**」结论不受影响 |

**它复核我方的部分**：§C 的 12 条全判「成立」，抽查 3 组行号全中——`labels.rs:184-208` 函数体内零 `validate_*`（`:190 require_write` 之后直调 core）、`issues.rs:603` `get_issue` → `:635 if !can_read` → `:639 existing.author_id`、`pulls.rs:776 list_by_repo` → `:781 for …enumerate()` → `:782 find_by_id` → `:787 continue` → `:792 position: index + 1`。**它对我方的复核本身无错项**，本轮它是「复核者」而非「新事实来源」。

**改动 SHA**：`314cfb0`（本轮唯一提交，`070519c..314cfb0`，仅本文件 29+/6−，推 `cnb`）。**回填**：评论 `2120377967560794112`（`2026-11-11T04:47:56Z`），回读 **C=2**、首行 `## 第 2 轮收尾`、末行 `@CodeBuddy`。

### consume 3 / 6（batch#4）

- 输入：NPC 复核 `2120378209525989376`（`2026-11-11T04:48:54Z`，首行 `@cnb.biewen(别问) ## 第 2 轮 · 复核结果`）——晚于我方第 2 轮收尾（`04:47:56Z`）⇒ **有新复核**，K=3
- 本轮它**未开新靶**：结论自述「4 处台账改动 + D-1 反证全部可核，0 项事实错」，7 个编号小节里 6 个是我方上轮改动的确认，只有 1 处是新的措辞指摘
- 本机 `BASE=a996cab`；`git fetch cnb` 后 `HEAD...cnb/main` = `0 0`（零漂移）；本批 4 文件对 issue 声明基线 `b7a0ab2` 的 diff 只有 `validation.rs` 的 2+/2−（我方第 1 轮注释改动），行数 1681/1261/902/242 全中
- 改前 `scripts/check.sh --quick` **2 s EXIT=0**（`Finished … in 0.27s`，热缓存指纹命中）
- 改动：`7111d97`（L1，仅本文件 2+/2−，**906 行不变**）

判定表（7 个编号小节 + 1 条我方自查）：

| # | 它说的 | 判定 | 证据与处置 |
|---|---|---|---|
| ① | `consume 2 / 6` 表 D-1 行判「不成立」，末段却写「⇒ 引用正确」，同一格内两个结论打架 | **部分成立**（歧义成立，「自相矛盾」不成立） | 「引用正确」在句中充当**判不成立的理由**：④-3 的条号指向正确 ⇒ 「指错」这条指摘不成立，故形式逻辑无矛盾。但它的读法风险是真的——单看「⇒ 引用正确」可被读成「第 23 条成立」。已就地改写为「**指针指向正确**（此处「引用正确」只指 ④-3 的条号指向对不对，不构成对指摘内容的采信——第 23 条本身就是已判死的条目）」，1 行→1 行 |
| ①附 | 「`\| 不成立 \| 见第 23 条 \|` 读起来像『见成立的第 23 条』」 | **部分成立**（限定词已在） | ④-3 行（`batch#3 consume 2 / 6` 表）原文是 `见「判定不成立」第 23 条`，已带「判定不成立」限定，不构成无标记引用；不改，只在 D-1 行加了「已判死的条目」的注 |
| ② | D-1 反证本身成立：`:152` 确非 ④-3；第 23/25/31 条命中数各 1；三连空行已消 | **确认**（其行号基于 `314cfb0`，现树 +2） | 它引的 ④-2 / ④-3 与反清单第 23/25/31 条，现树位置整体比它的号大 **2**（按 `batch#3 consume 2 / 6` 表行与条目号逐一对上）。差 2 行全由 `a996cab` 回填造成（`git diff --stat 314cfb0..a996cab` = 1 file, 2 insertions），非它错。`grep -c '13/14'` / `'跨 session'` / `'require_collab_read'` 三条各 = **1**；反清单整段逐行扫空行：32 个**全孤立**（无相邻） |
| ③ | B4-1 的 6 调用点 / 3 常量 / 6 个失效上限全部通过 | **确认** | 复算：6 调用点 + 1 定义（`validation.rs:50`）；实参 `issues.rs:533`/`:655`/`pulls.rs:295` = `MAX_BODY_LEN`、`labels.rs:145` = `MAX_LABEL_DESC_LEN`、`wiki.rs:192`/`:262` = `MAX_COMMIT_MSG_LEN` ⇒ **3 常量**；`.ok();` 六处 `:536`/`:658`/`:298`/`:148`/`:195`/`:265` 逐行打印全为丢弃 |
| ④ | B4-3 附注的行号与命题全中 | **确认** | `validation.rs:81` = `if hex.len() != 3 && hex.len() != 6 {`；`:83` = `"color must be a 3 or 6 digit hex code (e.g. #ff5500)"`；`label/service.rs:47` = create 侧 `starts_with('#') && len()==7`、`:93` = update 侧同形 ⇒ 3 位值两次都 400 |
| ⑤ | `check.sh` 的 `:36-37` / `:44-48` 逐行对，旧值 `:38-39`（elapsed + 空行）与 `:45-49` 确实都错 | **确认** | `awk 'NR>=32&&NR<=52'`：`:36` step fmt、`:37` 命令、`:44` `if [ "$mode" != "quick" ]`、`:45` step test、`:46` 命令、`:47` elapsed、`:48` fi ⇒ 跳过的是 `:45-47` |
| ⑥ | `validation.rs` 超 140 列共 3 行（`:9`=180 / `:21`=198 / `:49`=144）；无 `rustfmt.toml`、`Cargo.toml` 无 `[rustfmt]` | **确认** | `awk 'length>140'` 输出同上三行；`find . -maxdepth 3 -name '*rustfmt*'` 空、`.cargo/` 只有 `audit.toml` ⇒ `wrap_comments` 走默认 `false` |
| ⑦ | 结构核对：进度行 = 2/6、`consume 1 / 6`、`consume 2 / 6` 三处定位正确；清单编号 34/35 各 1，35 条唯一 | **确认** | 进度行与两个 `consume` 小节标题逐行打一致（它的插入点假设成立，未漂）；反清单整段编号 `1..35` **连续唯一**（`grep -c '^[0-9]\+\. '` = 35，抽出序列无重号无缺号） |
| F-1 | （我方自查，它未提）台账**尾部**另有一个同名小节 `## 判定不成立（原文保留，请勿重复提）`，正文是开批占位「（暂无）」，与上文 35 条直接打架 | **成立** | 该段由台账创建提交 `91686a9` 引入、从未更新（`git log -L` 显示 `new file mode` 时即写入）。已就地订正为指向上文同名小节并声明「本清单已累计 1..35 条」，1 行→1 行、总行数不变 |

**未做（L2 待拍板，无新增）**：本轮它没提任何代码级发现，`validation.rs:83` 的用户可见文案仍按 F-1 处置留在 B4-3（改它动 OpenAPI/前端契约面）。台账 §待拍板未增条目。

**改动 SHA**：`7111d97`（L1，`a996cab..7111d97`，2+/2−，906 行不变）+ 本条 ledger 记录 `fce70d1`；推 `cnb`。**回填**：评论 `2120394308342693888`（`2026-11-11T05:52:52Z`），回读 **C=3**、首行 `## 第 3 轮收尾`、末行 `@CodeBuddy`。

### batch#3(issue #26) · 2026-11-11 · rg-core repo/webhook 服务层

- 开批：`2026-11-10T19:52:01Z`（issue 创建即触发，正文结尾 `@CodeBuddy`，标题已绑定 `[opt-round] batch#3(issue #26)`）
- 基线提交：`df6a3c0`（cnb/main HEAD，2026-11-11；本机 main 检出同点、工作区干净，**无需 worktree**，直接本地记账）
- 范围：`crates/rg-core/src/{repo,webhook}` 4 个 .rs 共 4015 行（`repo/service.rs` 2803 / `webhook/service.rs` 561 / `repo/templates.rs` 476 / `repo/branch.rs` 175；两个 `mod.rs` 仅声明；`repo/templates/` 数据文件 `include_str!` 内嵌、不在范围）。行数以 `git show df6a3c0:<path> | wc -l` 统计
- 引导问题 4 条：P1 `redeliver` 经 `trigger_event` 打给**所有**匹配 hook 而非原 webhook（`webhook/service.rs:380-397`，`:392` 调用点）；P2 `deliver_retry_job` 不检查 `hook.active`（`:308`，`:319-325`）；P2 webhook `secret` 明文入库、与 CI secret 加密口径不一致（`:65`/`:109`）；P3 `update_webhook` 无法区分「未提供」与「清空」secret（`:109`）
- 反向清单回灌：**不成立 21 条全量回灌**（batch#1/#2 累计）+ 对方已撤回 4 项 + 我方自证推翻 4 项，均注明「不要重复提」
- 已知空档新增：`password.rs`/`rg-http/src/oci.rs`/`auth/oci_token.rs` 旧行号已因 PR #21/#24 作废不得引用；agent 模块不在本批范围
- NPC 首扫：`2120244997269143552`，`2026-11-10T19:59:34Z`（开批后 7 min 33 s；`author.is_npc=true`，首行 `## 第 1 轮 · batch#3 只读审查`，自述基线 `df6a3c0`）
- 6 轮进度：**6 / 6（已收批；issue #26 于 `2026-11-11T02:36:58Z` close，state_reason=completed；本行原记 5/6，由生产者开批 batch#4 时按平台终态补正）**（第 5 轮收尾 `2120324543901237248`（`2026-11-11T01:15:39Z`，回读校验 C=5，无重复投递）；第 4 轮 `2120303038425378816`（`23:50:12Z`，C=4）；第 3 轮 `2120286421721202688`（`22:44:10Z`，C=3）；第 2 轮 `2120269785404440576`；第 1 轮 `2120252808799633408`）

### consume 1 / 6（batch#3）

- 输入：`2120244997269143552`（`19:59:34Z`，只读审查）：种子 4 条 + 新发现 4 条 + 推测 6 条
- 判定：**成立 7 / 部分成立 1 / 不成立 0**（8 条正文指摘）；6 条推测单列不计分
- 工作基线：**`BASE=70494e2`**（`cnb/main` HEAD）。issue 声明的 `df6a3c0` 之后只有一次 `docs(opt-loop): open batch#3` 提交；`git diff df6a3c0..70494e2 -- crates/rg-core/src/repo crates/rg-core/src/webhook` **输出为空** ⇒ 本批 4 文件的**行号基准未漂**，它的 `df6a3c0:` 引用可直接在本树复用
- 改前基线：`scripts/check.sh --quick` **通过（2 s，热缓存，EXIT=0）**
- 改动：**`0f6c54a`**（L1，5 文件 7+/7−，**零行号漂移**：`webhook/service.rs` 561→561、`repo/service.rs` 2803→2803、`collaboration/access.rs` 42→42、`docs/p0-prd.md` 315→315、`docs/p0-system-design.md` 644→644）
- 改后验证：`scripts/check.sh --quick` **通过（62 s，EXIT=0）**
- 回帖：`2120252808799633408`，`2026-11-10T20:30:36Z`（首行 `## 第 1 轮收尾`，回读校验 C=1，无重复投递）

| # | 输入指摘 | 判定 | 证据 / 处置 |
|---|---|---|---|
| S-1 | `redeliver` 经 `trigger_event` 打给该 repo 所有匹配 event 的 active hook，不是原 webhook（`:392` 调用点，`:394` 只用 `hook.repo_id`） | **成立** | `sed -n '380,397p'` = `find_delivery_by_id` → `find_by_id(delivery.webhook_id)`（`:385`）→ `trigger_event(db, hook.repo_id, …)`（`:392-397`），`hook.id` 全程未参与；`grep -n "X-Hub\|X-GitHub\|trace_id"` 仅命中 `:295`（只有签名头，无 delivery id 头），`:396` 解析失败照发 `Value::Null`。**行号全对** |
| S-2 | `deliver_retry_job` 不检查 `hook.active`（`:308`，`:319-325`） | **成立** | `:319` 确为 `let hook = match webhook_ops::find_by_id(db, webhook_id).await? {`；`:308-361` 全文无 `active`，只取 `hook.url`/`content_type`/`secret` 后直接 `deliver()` |
| S-3 | webhook `secret` 明文入库（`:65`/`:109`），与 CI secret 加密口径不一致 | **成立** | `:65` = `secret: sea_orm::Set(req.secret.clone()),`；`:109` = `… req.secret.clone().or_else(|| existing.secret.clone())`；迁移 `m20260424_000006:125` = `.col(ColumnDef::new(Webhooks::Secret).string().null())`（裸 string，无加密封装）。对照 `encryption.rs` 的 AES-256-GCM 只服务 TOTP/SSO/CI secret |
| S-4 | `update_webhook` 无法区分「未提供」与「清空」secret | **成立** | `UpdateWebhookRequest.secret: Option<String>`（`:36`）在 serde 下「字段缺省」与「显式 `null`」同为 `None`，再由 `:109` 的 `or_else(existing)` 保旧值 ⇒ `{"secret": null}` 是空操作 |
| N-1 | LICENSE 模板是条款被截断的摘要，非全文（`apache-2.0`/`gpl-3.0`/`mpl-2.0`） | **成立**（一处夸大） | 实测词数：`apache-2.0.txt` 557 / `gpl-3.0.txt` 512 / `mpl-2.0.txt` 1101；`mit.txt` 168、`bsd-2/3-clause` 192/223、`unlicense` 197 均为全文。`apache-2.0.txt:48` = `   3. Grant of Patent License...`（正文被 `...` 取代）；`gpl-3.0.txt:52` = `  7. Additional Terms...`。**夸大处**：它说「GPLv3 第 7~17 条只有标题」，实际 §15/§16 有 3–5 行缩写正文，只有 §7–§14、§17 是标题 + `...`。零覆盖属实：`templates.rs:453-458` 只断言 `mit`/`gpl-3.0` 两个 key 存在 |
| N-2 | `set_watch` 不校验状态枚举，任意字符串落库（`repo/service.rs:852-860` + `api/repos.rs:596-598`） | **成立**（handler 行号偏 1） | `:852-860` 即 `set_watch`，函数体只有一句 `repo_watch_ops::set_watch_state(...)`；`WatchRequest.state: String`（`api/repos.rs:596-598`，逐行核对一致）；`repo_watch_ops.rs:29`/`:37` 两处 `Set(state.to_string())`；迁移 `m20260508_000001_create_repo_stars_watches.rs:90-94` 是 `.string().not_null().default("not_watching")`，**无 CHECK**。**偏差**：它写的 handler `:772-776` 起始是空行，实际透传在 `:773`。前端 `web/src/lib/api/repos.ts:173` 的三态联合类型确实被服务端破坏（`:171` 的入参是裸 `state: string`） |
| N-3 | `list_watchers` 不过滤 `watch_state`，`ignoring`/`not_watching` 照发通知 | **成立** | `repo_watch_ops.rs:71-88` 只有 `.filter(RepoId.eq(repo_id))`；唯一消费点 `notification/mod.rs:103`，循环 `:115-122` 只跳过作者、不看状态；`remove_watch`（`:65-68`）只是 `set_watch_state(..., "not_watching")`，**行不删** |
| N-4 | `is_collab_member` 在只读判定里做全域写锁 invalidate，且批量循环每轮一次 | **部分成立** | 前半成立：`access.rs:23` 确实调 `invalidate_perm_cache_user`，其实现 `repo/service.rs:120-124` 是 `perm_cache().write()`（全局写锁），调用点在读路径（`api/collab.rs:185`/`:197`、`collab_groups.rs:159`、`diagnostics.rs:182`、`escalation.rs:284`），`api/collab.rs:184` + `:185` 确实同一请求连清两次。**后半不成立**：`escalation.rs:283` 是 `if !self.access.contains_key(&session.user_id) { … }`，记忆化**生效**，不存在「每轮一次」。另 `collab_groups.rs` 的行号是 `:159`（它写 `:196`，那是 `writable()`） |

**本轮自查新事实（它未提，我方在核实 N-2 时挖到）**

- **F3-1**：`CreateWebhookRequest.events` 的注释示例是 `// e.g. ["push", "issues"]`（`webhook/service.rs:29` 改前原文），而 `trigger_event` 全部用带点全名（`issue.opened` `pull_request.merged` `release.created` …，`:418-560`），前端 `web/WebhookForm.svelte:22-27` 也发全名。`Column::Events.contains(event)` 经 sea-orm 1.1.20 `entity/column.rs:237-243` 编译成 `LIKE '%<event>%'` ⇒ 照注释示例建 `["issues"]` 的 hook **永不触发**（`"issues"` 不是 `"issue.opened"` 的子串），且 API 对 event 名零校验 —— 静默死 hook。已把该注释改成 `["push", "issue.opened"]`；**加校验属 L2**（会拒绝存量非法值），见 B3-8。

**部分成立里的错处（勿当结论引用）**：N-4 的「记忆化分支仍会调一次 `is_collab_member`」不成立（`escalation.rs:283` 有 `contains_key` 守卫）；`collab_groups.rs:196` 应为 `:159`；`api/repos.rs:772-776` 应为 `:773`。

**它的质量盘点**：8 条正文指摘里 7 条全对（含 4 条种子全部命中，且 S-1 的判据链补强是它自己找到的），唯一走偏的是 N-4 的放大推理。本轮**不需要**像 batch#2 那样反复纠正行号——`df6a3c0` 声明 + 每条带 `git show` 输出的做法有效，建议后续批次继续要求。

### consume 2 / 6（batch#3）

- 输入：`2120254111684665344`（`2026-11-10T20:35:47Z`，首行 `## 第 2 轮 · batch#3 复核（只读）`）：① 对我方 7 处改动判 7 成立 / 0 事实错（非指摘，记「确认」）；② 断言强度 3 条；③ 新事实 3 条；④ 只读路径写锁专题；⑤ 台账可复现性 3 条；⑥ 请求 6 条
- 判定：**成立 6 / 部分成立 3 / 不成立 2 / 确认 1**（13 条编号输入；另有 ⑥ 的 6 条请求，见回帖第 4 段）
- 环境修正：它自述「本机没有 cargo」并要我确认。实测本机 `command -v cargo` = `/Users/yuqu/.cargo/bin/cargo`（`cargo 1.99.1`），`sea-orm-1.1.20` 源码在 `~/.cargo/registry/src/rsproxy.cn-e3de039b2554c837/` ⇒ 它「无法复算」的两条我都能复算（见 ③-C、⑤-2）
- 工作基线：**`BASE=7137ff9`**（`cnb/main` HEAD；本地 HEAD 同点、工作区干净、**零漂移**，无需重取行号）
- 改前基线：`scripts/check.sh --quick` **通过（2 s，热缓存，EXIT=0）**
- 改动：**`dc3fb9e`**（L1，4 文件 4+/4−，**零行号漂移**：`repo/service.rs` 2803→2803、`webhook/service.rs` 561→561、`access.rs` 42→42、`p0-system-design.md` 644→644）
- 改后验证：`scripts/check.sh --quick` **通过（65 s，EXIT=0）**
- 回帖：`2120269785404440576`，`2026-11-10T21:38:04Z`（首行 `## 第 2 轮收尾`，回读校验 **C=2**，无重复投递）

| # | 输入指摘 | 判定 | 证据 / 处置 |
|---|---|---|---|
| ① | 我方 `0f6c54a` 的 7 处改动「零事实错」，判 7/0 | **确认** | 非指摘。本章其余条目才是本轮输入 |
| ②-1 | `repo/service.rs:852` 只说「no enum check」，没说后果；应改成「`none` 也照发 push/PR 通知」 | **部分成立**（事实链成立，措辞错） | 「后果没写」成立：`api/repos.rs:768` 的 `PUT /watch` 只要 `require_authenticated_read`（不要求 write）、`:807` 的 `DELETE` 写死 `"not_watching"`、`repo_watch_ops.rs:65-68` 只改状态**不删行**、`notification/mod.rs:115-121` 只跳作者。但它给的两个行号错（`repo_watch_ops.rs:31`/`:39` 实为 `:29`/`:37`），且**它建议的措辞被它自己的 ③-B 证伪**（push/PR 的 watcher 通知零调用者）——按字面改就是写进一条新的假话。已按真实路径改写，只提 `milestone.closed` |
| ②-2 | `p0-system-design.md:125` 是同列自相矛盾的「本轮唯一事实性错」，空着的「约束」格才是证据位 | **部分成立**（定性错，改法对） | 该行文字与实现一致（`repo_watch_ops.rs:29`/`:37` 逐字落库；迁移 `m20260508_000001:89-94` 只有 `.string().not_null().default(...)`，**无 CHECK**）⇒ 不是事实错误，「直接对立」是读法问题。但「约束列空着 = 信息缺失」成立，已把「无 CHECK」写进约束列（1 行→1 行，表格行数与文档 644 行不变） |
| ②-3 | `webhook/service.rs:29` 应写明「裸 `issues` 永不命中 `issue.opened`」 | **成立** | `webhook_ops.rs:43` = `Column::Events.contains(event)`；`column.rs:237-243` 生成 `LIKE '%x%'`；13 个事件全名两两互不为子串 ⇒ 全名场景只「欠」不「误」。已补进注释（1 行→1 行） |
| ③-A | `list_watchers` 有 `ORDER BY updated_at DESC` + `limit 1000` ⇒ 活跃 watcher 挤掉长期订阅者 | **成立**（后果强度未验证，它自己也标了） | `repo_watch_ops.rs:79` `.order_by_desc(UpdatedAt)`、`:83-88` 的 `.offset(offset).limit(limit)`；`notification/mod.rs:103` 写死传 `(repo_id, 0, 1000)`。已并入 B3-6 |
| ③-B | `notify_watchers_push`/`notify_watchers_pr` 零调用者 ⇒ B3-6 泄漏面只有 `milestone.closed`，P1 应下调 | **成立**，但**不是新事实** | `grep -rn` 各 1 命中且都是定义行（`repo/service.rs:1590`、`pull_request/service.rs:295`）；唯一活 fan-out 是 `issue/service.rs:488`（`:472` 注释 `milestone.closed` + `:477` 的 webhook）。台账已知项：`ironforge-docs/architecture/agent-draft-refs-ttl-design-2026-11.md:168` 早已记「`notify_watchers_push` 全仓库无调用」——它漏了 PR 那条，也没接到 B3-6 的量级上。B3-6 已据此 **P1→P2** 并限定泄漏面 |
| ③-C | `events` 的 LIKE 是「只欠不误」单向失效；并要求我把 `sea-orm column.rs` 原文贴进台账 | **成立** | 原文已留档（见下「证据留档」）。它自述无法复算，本机有 registry 源码，可复算 |
| ④ | 撤回自己上轮 N-4 的定级；`human_notify.rs:114` 是循环内写锁（上界 500），非 N 次论只对 `escalation` 成立 | **部分成立** | 成立：`human_notify.rs:107` `.take(MAX_CANDIDATES)`（`:46` = 500），`:114` 在循环内；`:42` 的 50 只让 `out.len()` 提前 break，不限制 invalidate 次数。不成立：「**14 处生产调用**」实为 **13**（它把 `repo/service.rs:2717` 的测试算进去，又漏了 `human_notify.rs:207`）；「**101 处** `require_collab_read|write`」实为 **73 处调用点**（39 read + 34 write、24 文件） |
| ④-2 | `access.rs:17` 的「per call」对 `require_collab_write` 不成立，应限定主语 | **不成立** | 见「判定不成立」第 24 条 |
| ④-3 | `perm_cache_disabled()` 在 `check_perm_cache:97-99` 短路 ⇒ 测试环境不付这个锁，14 处调用才没在 CI 里炸 | **不成立** | 见「判定不成立」第 23 条 |
| ⑤-1 | `deliver_retry_job` 应写明「一次失败重试 5 次、退避 30 s→1 h 封顶」 | **成立** | `background_jobs/mod.rs:35` `DEFAULT_MAX_ATTEMPTS: i32 = 5`；`:36` 注释 `30s * 2^(n-1), capped at 1 hour`；`:37` `BACKOFF_BASE_SECS = 30`、`:38` `BACKOFF_CAP_SECS = 3600`。行号全对 |
| ⑤-2 | `ironforge_webhook_deliveries_total` 只在 HTTP `redeliver` 打点（`api/webhooks.rs:280`/`:288`），`trigger_event`/`deliver_retry_job` 一次都不打；`deploy/README.md:239` 写成「Webhook deliveries」 | **成立** | emit 点只有 `api/webhooks.rs:280`/`:288`（`recorder::webhook_delivery`），注册在 `metrics.rs:455-465`，`DELIVER`/重试路径无调用。`deploy/README.md:239` 文案属实但口径漏了「仅手动重投」。**`deploy/` 是禁区：只报告，不改** |
| ⑤-3 | 反向清单里两行都标 `2.`，`grep '^2\.'` 命中两条 | **成立** | 22 条编号只到 21。已重排为连续唯一的 `1..22` 并在清单头加编号勘误 |

**证据留档（sea-orm 1.1.20，`src/entity/column.rs:237-243`，供换机复算）**

```rust
fn contains<T>(&self, s: T) -> SimpleExpr
where
    T: Into<String>,
{
    let pattern = format!("%{}%", s.into());
    Expr::col((self.entity_name(), *self)).like(pattern)
}
```

**本轮自查新事实（它未提）**

- **F3-2**：`invalidate_perm_cache_user`（`repo/service.rs:120-124`）**不受** `IRONFORGE_DISABLE_PERM_CACHE` 约束 —— `:121` 无条件 `perm_cache().write()`，只有 `check_perm_cache`（`:96-99`）与 `set_perm_cache`（`:107-110`）在读 `perm_cache_disabled()`。所以「关掉缓存即可免掉写锁」是假的安全感：多实例部署按注释开了 `IRONFORGE_DISABLE_PERM_CACHE=1` 之后，每次 `require_collab_read` 仍然拿两次全局写锁（`api/collab.rs:184` + `access.rs:23`），只是不再读它。这条与 B3-9 同处，但对决策更关键：禁用缓存**省的是读**、不是写。

**它的质量盘点**：本轮它把靶子从「复核我方措辞」转到「新事实 + 设计层」，13 条编号输入里 6 条成立、3 条部分成立、2 条不成立、1 条是对我方改动的确认；两处硬伤都是「自己的两条发现互相打脸」：②-1 建议的措辞被 ③-B 证伪；④ 的「14 处」与它自己表里只列的 12 行不一致（真值 13）。**新规律**：它给出的「替代措辞」不能直接落盘 —— 局部事实往往对，但拼成一句话时的因果链会越过它另一条发现。凡它给整句替换，必须先拿本轮其它条目交叉验一遍。

### consume 3 / 6（batch#3）

- 输入：`2120270296044191744`（`2026-11-10T21:40:05Z`，首行 `## 第 3 轮复核 · 只读 · HEAD \`8bafd5f\``）：① 自行撤回反向清单 23/24 并附两条新错值；② 复核我方 `dc3fb9e` 四处；③ 新事实 2 条（F3-3 / F3-4）；④ 台账层 2 条；⑤ 未复算声明 3 条
- 判定：**成立 4 / 部分成立 3 / 不成立 2（子命题）/ 确认 2**（8 条编号指摘 + 2 条确认）；另有 2 项它自行撤回
- 环境：它自述沙箱无 cargo（`./scripts/check.sh --quick` → `line 37: cargo: command not found`，EXIT=127），故本轮一切编译类结论由我方出；本机改前 `--quick` **1 s**、改后 **72 s**，均 EXIT=0
- 工作基线：**`BASE=0d52ab4`**（`cnb/main` HEAD，本地同点、工作区干净）。issue 声明 `df6a3c0`，`git diff df6a3c0..0d52ab4 -- crates/rg-core/src/{repo,webhook,collaboration} docs/p0-system-design.md` 只有本流程自己前两轮的改动（4 文件 6+/6−）⇒ **零行号漂移**（`repo/service.rs` 2803 / `webhook/service.rs` 561 / `access.rs` 42 / `p0-system-design.md` 644 均未变）
- 改动：**`ebf14f9`**（L1，3 文件 6+/6−，零行号漂移）+ 本轮 ledger
- 改后验证：`scripts/check.sh --quick` **通过（72 s，EXIT=0）**
- 回帖：`2120286421721202688`，`2026-11-10T22:44:10Z`（首行 `## 第 3 轮收尾`，回读校验 **C=3**，无重复投递）

| # | 输入指摘 | 判定 | 证据 / 处置 |
|---|---|---|---|
| ①-A | `repo/service.rs:62-65` 的 mitigation 注释「bypassed entirely」字面为假：禁用态下 `invalidate` 照写、`set_perm_cache` 早退 ⇒ map「只增不减」，运行期切 flag 的实例会读到已撤销的允许项 | **部分成立** | **成立部分**：`grep -rn perm_cache_disabled crates/` = `:75` 定义 / `:97`（`check_perm_cache`）/ `:108`（`set_perm_cache`），**无第四处**；`invalidate_perm_cache_user`（`:120-124`）`:121` 无条件 `perm_cache().write()` ⇒ 禁用态下这次写锁服务的是一个没人写、没人读的 map，`:63` 的「entirely」确实没区分读/写侧。**不成立部分**：`grep -n "cache.insert" repo/service.rs` 只有 `:113`（在 `set_perm_cache` 内，`:108` 已早退）⇒ 禁用态下**没有插入路径**，map 只减不增（`:121`/`:129-133` 全是删除），「只增不减」方向错；`perm_cache_disabled()`（`:75-83`）是 `static DISABLED: OnceLock<bool>`，首次调用即冻结，**运行期改环境变量无效** ⇒「先收过 invalidate 就会读到已撤销允许项」的场景构造不出来。**处置**：按成立的半边改注释（`:63-66` 4 行→4 行），不加它那两段推论 |
| ①-B | `access.rs:17` 的括注把「`require_collab_write` 付 0 次锁」当成了事实，应写成 read 付 2 次 / write 付 1 次 | **部分成立** | **成立部分**：`api/collab.rs:202` `require_collab_write` 在 `:211` 先 `invalidate_perm_cache_user`，`:212` 直接 `can_write_repo`（不进 `access.rs:23`）⇒ write 付 1 次、read 付 2 次，差额真实。**不成立部分**：原句只量化 read（"pays it twice per request"），全句未对 write 的次数做任何断言，「把付零当事实」是它的读法。**处置**：括注补 write 付 1 次（1 行→1 行） |
| ②-1 | 我方 `repo/service.rs:852` 的改法「成立且措辞准确」 | **确认** | 非指摘。复核一致 |
| ②-2 | `webhook/service.rs:29` 的 events 注释漏了通配 `"*"` 也是裸串 ⇒ 填 `"*"` 的钩子同样静默死亡 | **成立** | `grep -rn '"\*"' crates/rg-core/src/webhook/` **零命中（exit=1）**；13 个事件全名两两互不为子串且无一含 `*` ⇒ `Column::Events.contains("*")` 永不命中。已写进同一行注释（1 行→1 行） |
| ②-4 | `p0-system-design.md:125`「成立，零错误」 | **确认** | `m20260508_000001_create_repo_stars_watches.rs:90-95` 只有 `.string().not_null().default(...)`，无 CHECK；`repo_watch_ops.rs:29`/`:37` 两处 `Set(state.to_string())`。一致 |
| ③ F3-3 | `escalation.rs` 的记忆化是「重复候选互相遮蔽」，且是**用 session-A 的权限放行 session-B 的提醒**；候选上限是 600 而非 200 | **部分成立** | **成立部分**：`gather`（`:428-518`）候选粒度确为 **session**（`owner_session_id`、`question.creator_id\|recipient_id`、`handoff.from_session_id`）；`CANDIDATE_LIMIT`（`:45` = 200）在 `:439`/`:457`/`:488` 三处使用（它写 `:497`，那里是 `(None, false) => None,`） ⇒ 单次 `evaluate` 候选上界 **600**；dedupe 唯一键 `uq_collab_notices_dedupe` = `(recipient_session_id, kind, source_type, source_id, generation)`（迁移 `m20261104_000005_create_collab_notices.rs:192-198`），`insert_if_new`（`collab_notice_ops.rs:84-92`）只在唯一冲突时返回 `false` ⇒ 同 user 多 session 确实各得一条。**不成立部分**：「跨 session 放行」—— 被缓存的是 `is_collab_member(db, repo, session.user_id)`（`escalation.rs:284`），判据是 **user 对 repo** 的权限，与 session 无关，同 user 的两个 session 不可能得出不同结论；`may_notify` 在取缓存前已单独校验 session 存在与 `state_of` ∈ {online, offline}（`:277-282`）⇒ 缓存没有越过任何 session 级闸门，见反向清单第 25 条。**处置**：把 B3-9 的「不构成 N 次」改为「锁数 = 候选里不同 user 数，候选上界 600」 |
| ③ F3-4 | `redeliver` 借道 `trigger_event`：一次手动重投既在 `repo_events` 再插一行，又向全部 active hook 真实扇出；而 `ironforge_webhook_deliveries_total` 只记手动那次 | **成立**（L2） | `redeliver`（`:380-401`）`:392-398` 调 `trigger_event(db, hook.repo_id, &delivery.event, …)`；`trigger_event` `:140` 先 `record_repo_event` → `:235` `repo_event_ops::append`（`:227-239` 函数体），`:142` 再 `list_active_by_repo_and_event` 扇出。行号全对（它写 `:142-149`，扇出起点即 `:142`）。**处置**：不动代码，新立 **B3-10**（P3） |
| ④-1 | 反向清单 `:594` 的 `15.` 与 `:596` 的 `16.` 之间夹了空行，`grep -A1 '^15\. '` 取不到下一项 | **成立** | `sed -n '594,596p'` 确认 `:595` 为空行，主清单其余条目之间无空行。**处置**：删除该空行（`:596` 起全文上移 1 行） |
| ④-2 | B3-6 的「只通知最近改过 watch 状态的 1000 人」应收紧为「1000 **行**（含 `not_watching`）」 | **成立** | `grep -rn list_watchers crates/` = `notification/mod.rs:103`（唯一调用）+ `repo_watch_ops.rs:71`（定义）；`repo_watch_ops.rs:71-88` 只有 `.filter(RepoId.eq(repo_id))` + `order_by_desc(UpdatedAt)`，**不按 `watch_state` 过滤**。**处置**：改 B3-6 措辞 |
| ①-撤回 23 | 它撤回「`perm_cache_disabled()` 短路保护了 13 处调用」 | **确认撤回**（我方原判维持） | 已记入「对方已撤回」表；反向清单第 23 条保留 |
| ①-撤回 24 | 它撤回「`access.rs:17` 的 `per call` 要限定主语」 | **确认撤回**（我方原判维持） | 同上；第 24 条保留 |

**本轮自查新事实（它未提）**

- **F3-5**：`invalidate_perm_cache_repo`（`repo/service.rs:128-133`）与 `invalidate_perm_cache_all`（`:137-142`）同样**不受** `perm_cache_disabled()` 约束 —— 全仓 `perm_cache_disabled` 只有 `:75` 定义 / `:97` / `:108` 三处命中。即「禁用缓存」对**三个写侧入口全部无效**，只在读侧（`check_perm_cache`）与写回（`set_perm_cache`）短路。这一点比 F3-2 的「一处」更完整：禁用态下每次 `require_collab_read` 仍付 2 次全局写锁、每次仓库级失效再付 1 次。
- **F3-6**：`repo/service.rs:63` 原文的「no stale cross-instance entry can exist」在**单实例 + 运行期改 flag** 的语境下是成立的（`OnceLock` 冻结），但在**每实例各自启动时读 flag** 的多实例语境下才是它的本意；两个语境都无 stale 读，故该句结论本身不错，错的只是「entirely」漏掉了写侧成本。本轮注释按此改。

**它的质量盘点**：本轮它把上一轮判死的两条自己撤了（23/24），并主动补了 `grep` 复算，态度是六轮里最好的一次；8 条编号指摘里 4 条全对（②-2 的 `"*"` 是真正的新坑，F3-4 的行号全对）。两处走偏都在**放大链**：①-A 把「写侧仍付锁」推到「只增不减 → 读到已撤销项」（插入路径根本不存在），F3-3 把「按 user 记忆化」推到「跨 session 放行」（判据本就是 user 级）。**规律复现**：它给「后果放大」时，先找那个数据结构的**唯一写入口**（这里是 `:113`）和缓存键的**语义层级**（这里是 user 而非 session），两步都能否掉大半。

### consume 4 / 6（batch#3）

- 输入：`2120286763926077440`（`2026-11-10T22:45:32Z`，首行 `## 第 3 轮复核 · 只读 · HEAD \`dab3431\``）：3 条指摘（① `repo/service.rs` 的 `no stale entry can exist` 应收窄 ② `access.rs:17` 的 `per call` 不成立于早退路径 ③ `webhook/service.rs:29` 的「13」应改为全部投递值）+ 5 条确认（F3-5 / F3-6 / B3-10 / B3-6 措辞 / B3-9）+ 台账计数复核
- 判定：**成立 1（③ 的方向，它给的计数仍错）/ 不成立 2（①②）/ 确认 5**；另我方自认 1 项措辞退化（`:66` 丢了 pub/sub 半句，它未列为指摘）
- 环境：它自述 `cargo not on PATH`（`~/.cargo/bin`、`/usr/local/cargo` 均无）⇒ 编译类结论由我方出；本机改前 `scripts/check.sh --quick` **2 s**、改后 **62 s**，均 EXIT=0
- 工作基线：**`BASE=4b867a0`**（`cnb/main` HEAD，本地同点、工作区干净、`git log HEAD..cnb/main` 为空）⇒ issue 声明的 `df6a3c0` 之后只有本流程自己的改动；本轮再核 `repo/service.rs` 2803 / `webhook/service.rs` 561 / `access.rs` 42 行均未变 ⇒ **零行号漂移**
- 改动：**`8e4188d`**（L1，2 文件 2+/2−，零行号漂移）
- 回帖：`2120303038425378816`，`2026-11-10T23:50:12Z`（首行 `## 第 4 轮收尾`，回读校验 **C=4**，无重复投递）

| # | 输入指摘 | 判定 | 证据 / 处置 |
|---|---|---|---|
| ① | `repo/service.rs:64-65` 的 `no stale entry can exist` 无条件，压过原文的 `cross-instance` 限定；「先以 flag 未设启动并预热、之后才设 flag ⇒ 旧允许项永久驻留」，应改成 `nothing reads it` | **不成立** | 三步否证。**(1) 场景不可构造**：`perm_cache_disabled()`（`:75-83`）是 `static DISABLED: OnceLock<bool>`，而每条写入路径都先过它 —— `set_perm_cache` 的 `:108 if perm_cache_disabled() { return; }` 在 `:113 cache.insert` **之前** ⇒ flag 在**第一次插入之前**就冻结；`grep -rn set_var crates/` 无 `IRONFORGE_DISABLE_PERM_CACHE`（只有测试改 `HOME`/`IRONFORGE_ALLOW_LOCAL_OUTBOUND`）⇒ 运行期改不动，compose/env 改动必须重启进程 ⇒ 新进程 map 为空 ⇒ **不存在「已预热又带旧项」的进程状态**。**(2) 「没有任何路径会清它」为假**：`invalidate_perm_cache_user`（`:120-124`）无 `perm_cache_disabled()` 守卫，`api/collab.rs:184` 与 `access.rs:23` 每个协作请求各调一次；`invalidate_perm_cache_repo`（`:128-133`）/`invalidate_perm_cache_all`（`:137-142`）同理 ⇒ 旧项在该 user 的下一个请求上就被删。**(3) 它的替代措辞更弱**：`nothing reads it` 把 `:63` 已写明的 `nothing populates` 一起抹掉。维持原文，见反向清单第 26 条 |
| ② | `access.rs:17` 的 `per call` 不准：owner 早退（`:24-26`）与 org 成员早退（`:34-37`）时「一次锁都不付」 | **不成立** | `:23` 是函数体**第一条语句**：`sed -n '18,26p'` = `pub async fn is_collab_member(` → `) -> CollabResult<bool> {` → `:23 crate::repo::service::invalidate_perm_cache_user(repo.id, user_id);` → `:24 if repo.owner_id == user_id {`。锁在**早退之前**，owner / org 早退路径照付一次 ⇒ `per call` 字面准确（`is_collab_member` 每调一次 = 1 次写锁；`require_collab_read` 另有 `api/collab.rs:184` 的前置清理 = 2 次）。见反向清单第 27 条 |
| ③ | `webhook/service.rs:29` 的「13 个事件名」低于真实投递值，应按 `trigger_event` 全部调用点写死 | **部分成立** | **成立**：`13` 确为低估，且 `collab.*` 走同一函数 —— `collaboration/webhook_events.rs:95` 调 `crate::webhook::service::trigger_event` ⇒ `webhook_ops.rs:43` 的 `Column::Events.contains`（`LIKE '%…%'`）同样覆盖它们。**不成立部分**：它给的总数 **15 也错**，真值 **16 = 14 repo + 2 collab**（`grep -rhn 'trigger_event(db, repo_id, "'` 去重后 14 项 + `webhook_events.rs:30`/`:32` 两个常量；它自己的枚举逐项数是 14 项却写成 13，再加 2 得 16）。已按 16 改注释，见 F3-7 |
| 确认 1 | F3-5（三个写侧入口全裸）成立 | **确认** | 与 `grep -rn perm_cache_disabled crates/` = `:75` 定义 / `:97` / `:108` 三处一致 |
| 确认 2 | F3-6 结论成立、理由要换 | **确认（理由按我方更强的一支）** | 见 ① 第 (1) 步：`OnceLock` 在**首次插入前**冻结，比它说的「启动即读」更强；它补的「单实例启动后改 flag 就漏」仍不可构造 |
| 确认 3 | B3-10 行号与 P3 定级 | **确认** | 与我方记录逐行一致 |
| 确认 4 | B3-6 是「1000 **行**」而非「1000 人」，且 `notify_watchers` 不按 `watch_state` 跳过 | **确认（前半第 2 轮已判，后半非新事实）** | 后半即 batch#3 第 1 轮 N-3，早已写进 B3-6；`notification/mod.rs:115-121` 只跳过作者 |
| 确认 5 | 台账 `^(\d+)\. ` = 25 条、编号 `1..25` 连续唯一 | **确认** | 清单段 `awk '/^[0-9]+\. /'` 输出 1..25 连续 |
| 自查 1 | `:66` 结尾删掉原文的 pub/sub 半句是措辞退化 | **成立（我方自认）** | 共享缓存与失效总线是两条不同的替代路径；已恢复为 `until a shared cache or a pub/sub invalidation bus replaces it`（1 行→1 行） |

**本轮自查新事实（它未提，或它提的形状不对）**

- **F3-7**：`trigger_event` 的**全部**投递值 = **16**，无一含 `*`：14 个 repo 事件（`push`、`release.created|deleted`、`branch.created|deleted`、`tag.created|deleted`、`issue.opened|closed|comment`、`pull_request.opened|closed|merged`、`milestone.closed`）+ 2 个 collab 事件（`collab.question.asked`/`collab.task.blocked`，`webhook_events.rs:30`/`:32`）。第 1 轮注释与我方台账里的「13」**都少算 1**（漏在 repo 那 14 项里）。**教训：这类计数必须用去重命令产出，不能靠肉眼数枚举。**
- **F3-8**（收窄我方 F3-1 的措辞）：webhook 创建**不是「零校验」** —— `api/webhooks.rs:83-85` 拦空数组，`webhook/service.rs:55` 直接 `req.events.join(",")`，**无字符集、无白名单**；但仓库另一处**已有** `valid_event_name`（`api/repo_events.rs:57-63`，限 `[a-z0-9._-]`、≤64 字节），只用于 repo_events 的 `types` 查询参数。当时据此写的是「字符集校验器已存在，可直接复用，L2 可拆成先复用字符集校验（低风险）」。**第 6 轮订正：这条落地建议作废** —— `valid_event_name` 是 `rg-http` 私有 `fn`，且**会拒收今天能工作的 `Push`**（`api/repo_events.rs:239` 的 `assert!(!valid_event_name("Push"))` 是代码自带的证据）。正确落地顺序见 B3-8（先抽 16 常量、再让两处都引用它），**不是**先复用字符集校验。

**它的质量盘点**：本轮 3 条指摘 0 条全对 —— ①② 都在**同一条注释上做减法**（把已成立的事实改弱），③ 方向对但计数错。规律更新：上一批它靠「放大后果」出错，本批转为**「收窄措辞」出错**，且收窄方向一致地偏向「把无条件事实改成有条件的弱命题」。否证这两条的通用手法是**看锁/校验相对于早退分支的语句位置**（`:23` 在 `:24` 之前）与**看 OnceLock / 环境变量的可写性**（运行期无法翻转）。它对 5 条确认项的复算全部准确，`awk`/`grep` 自证形式保持，行号零偏差。

### consume 5 / 6（batch#3）

- 输入：`2120303331372326912`（`2026-11-10T23:51:22Z`，首行 `## 第 4 轮复核 · 只读 · HEAD \`b5ba430\``）：3 条编号（① `:29` 的 16 计数 ② `repo/service.rs:62-66` 无需收窄 ③ `access.rs:17` 的 `per call`）+ 4 条新发现（N-1 `:29` 289 列与内部事实外泄 / N-2 `valid_event_name` 不能直接复用 / N-3 事件日志面 / N-4 大写订阅）
- 判定：**确认 3（①②③ 主命题）/ 成立 2（N-1、N-4）/ 部分成立 2（N-2、N-3）/ 子命题不成立 5**（清单第 28–32 条）
- 环境：它自述 `cargo` 不在 PATH；本机 `BASE=dc39eb0`，改前 `scripts/check.sh --quick` **EXIT=0（1 s，热缓存）**
- 工作基线：**`BASE=dc39eb0`**（`git rev-parse --short HEAD` = `cnb/main`，`git log HEAD..cnb/main` 空）；`repo/service.rs` 2803 / `webhook/service.rs` 561 / `access.rs` 42 行均未变 ⇒ 零行号漂移
- 改动：见「本轮改动」两段（L1 注释 1 文件 + 本台账）

| # | 输入指摘 | 判定 | 证据 / 处置 |
|---|---|---|---|
| ① | `webhook/service.rs:29` 的「16」成立（它自行复算了 14 + 2、`*` 与子串关系） | **确认** | 我方第 4 轮写入的 16 与我方复算逐一吻合：`grep -rh 'trigger_event(db, repo_id, "' crates/ --include=*.rs \| sed -E 's/.*"([^"]+)".*/\1/' \| sort -u \| wc -l` = **14**，加 `webhook_events.rs:30`/`:32` 两个常量 = 16；`grep -c '\*'` 在 16 项里 = 0。它另给的「两个动态调用点不引入第 17 个值」（`webhook/service.rs:392` 读 `delivery.event`；`human_notify.rs:34`/`:37` 是 `notifications.event_type`、不进 `trigger_event`）经 `grep -rn 'EVENT_QUESTION_ASKED\|EVENT_TASK_BLOCKED'` 复核成立（`collab_questions.rs:173`/`collab_tasks.rs:287` 调的是 `ws::push_notification`，通知面，不经 `trigger_event`） |
| ①-附 | 「`scripts/check.sh --quick` **不含** `cargo fmt --check`，所以『改后 EXIT=0』不构成这条的验证」 | **不成立** | `scripts/check.sh:36-37` 是 `step "cargo fmt --all --check"` + `cargo fmt --all --check`，在 `case "$1"` 之后**无条件执行**，`--quick` 只是跳过 `cargo test`（`:44-48`）。所以改后 EXIT=0 确实跑过了 fmt；289 列没被折行是 **rustfmt 的默认行为**（`wrap_comments` 默认 `false`，且本仓无 `rustfmt.toml`），**不是脚本盲区**。见反向清单第 28 条 |
| ①-附 | 「`:29` 改前 **214** 列（+75）」 | **不成立** | 该行历史长度：`0f6c54a`=102 → `dc3fb9e`=202 → `ebf14f9`=242 → `8e4188d`=289（`git show <c>:path \| sed -n '29p' \| wc -c`，减 1）。本轮改前的真值是 **242**，增量 **+47**，任何版本都不存在 214。见反向清单第 29 条 |
| ② | `repo/service.rs:62-66` 的 `no stale entry can exist` 在「flag 首次插入前冻结」这一支下无条件真，不需要收窄；`:66` 恢复 pub/sub 半句正确 | **确认** | 与我方第 4 轮三步否证一致：`set_perm_cache` 的 `:108 if perm_cache_disabled() { return; }` 在 `:113 cache.insert` 之前；`grep -rn 'set_var\|remove_var' crates/ --include=*.rs` 只命中 `net_guard.rs:442`/`:446`（`IRONFORGE_ALLOW_LOCAL_OUTBOUND`）与测试的 `HOME`/`XDG_CONFIG_HOME`，无 `IRONFORGE_DISABLE_PERM_CACHE` |
| ②-附 | `:66`（112 列）建议折成两行 | **不采纳（L1，但代价大于收益）** | 折行会让 `repo/service.rs` 561→562 行 ⇒ 全仓所有 `repo/service.rs:<line>` 引用（台账 40+ 处、代码注释若干）整体 +1 漂移，只为一条 112 列注释。且 `:1413` 已达 270 列是既有状态，单改 `:66` 不构成一致性。**报告不改，理由留档** |
| ③ | `access.rs:15-23` 的 `per call` 成立（`:23` 在 `:24` owner 判断之前，所有早退路径照付） | **确认** | `sed -n '18,26p'`：`:23` 是函数体第一条语句，`:24 if repo.owner_id == user_id {`、`:33 if let Some(org_id) = repo.org_id {` 均在其后 |
| ③-附 | 「`require_collab_read` 是 **2 次**不是 3 次；这个坑我台账 B3-9 与上轮都没写，补上」 | **部分成立（结论对，「没写」不成立）** | 2 次正确：`api/collab.rs:184` 一次 + `:185` → `access.rs:23` 一次。但**台账早已写明**：B3-9 原文「其中 `require_collab_read` 每个请求**必然清两次**（`api/collab.rs:184` 先清，`:185` → `access.rs:23` 再清）」。它给的区间 `sed -n '218,226p'` 也错（`:218` 是 `require_collab_write` 收尾 `}`（`:217`）之后的空行，该函数真值区间是 `:202-217`，`invalidate` 在 `:211`，`:212` 直调 `can_write_repo`）。见反向清单第 31 条 |
| N-1 | `:29` 289 列，且把「服务端有几个投递值」这类内部事实塞进公开 API 字段注释；读者需要的是「我该填什么」 | **成立** | `awk 'NR==29{print length($0)}'` = 289（改前）。计数是易腐事实：102 → 202 → 242 → 289 四版，且第 1/第 4 轮各错一次（13→15→16）。**已改为 187 列、只留可操作信息**（全名带点 / LIKE 子串 / 裸 `issues` 与 `*` 都不命中），1 行→1 行、文件仍 561 行 |
| N-2 | `valid_event_name` 不能直接复用到 webhook 创建，5 条论据 | **部分成立（4 成立 / 1 不成立）** | **成立**：(1) 它是 `api/repo_events.rs:57` 的私有 `fn`，`rg-core` 看不见；(3) 白名单无单一数据源（`EVENT_*` 在 `rg-core`、`RETENTION` 在 `rg-http:28`、`MAX_TYPES` 在 `:33`）；(4) `MAX_TYPES = 20` 用在 `:84` 的 `types` **查询参数**，与「一个 hook 能存多少订阅」不同量；(5) `"collab_question"`（`human_notify.rs:34`）能过字符集却不是投递值 ⇒ 字符集校验对 webhook 是假阳性校验。**不成立**：它说「字符集 `[a-z0-9._-]` 容得下 `*`，复用等于照旧放行 `*`」——`*` = 0x2A 既非 `is_ascii_lowercase` 也非 `is_ascii_digit`、不在 `.?_-` 里 ⇒ `valid_event_name("*")` = **false**，复用字符集校验**恰好会拒掉** `*`（见 F3-10）。它由此推的「必须先抽 16 常量再谈复用」理由被削弱：抽常量是为了枚举校验，不是为了挡住 `*` |
| N-3 | 「**4 个订阅名永远等不到对应 `repo_events` 行**」+ 反过来「`collab.*` 进 `repo_events` 意味着协作内容出现在公开事件流」 | **部分成立（标题不成立，ACL 半句成立）** | **标题不成立**：同一段它自己贴的 `sed -n '138,142p'` 就是反证——`:140 record_repo_event(db, repo_id, event, payload).await;` 在 `trigger_event` 开头**无条件**执行；「4 个」的清单从未给出，无法核实。见反向清单第 32 条。**ACL 半句成立且是新事实**：`can_read_repo` 对 `!repo.is_private` 在 `:201-203` **直接 `Ok(true)`**（不看 `actor_id`）⇒ `/events`（`api/repo_events.rs:119` 用 `repo_access::require_read`）在公开仓上**匿名可读**，而它返回的 `payload` 是全文（`:172-189`，`:185` 即 `"payload": payload`），`question_asked_payload` 含 `user_content.title`（`webhook_events.rs:64-67`）⇒ 协作问答标题进了公开事件流，与协作区的 `require_collab_read` 门禁口径不一致。**记 B3-12（L2）** |
| N-4 | `Column::Events.contains` 在 SQLite 下是 ASCII 不区分大小写的 `LIKE` ⇒ 订阅 `Push` 能命中 `push`，而 `valid_event_name` 只收小写；加校验前要先定「要不要收紧大写」 | **成立（措辞需改「两个面不一致」）** | 实测：`sqlite3 :memory: "select 'Push' like '%push%'"` = **1**。`webhook_ops.rs:43` 生成的是 `format!("%{}%", s)`（sea-orm 1.1.20 `entity/column.rs:237-243`）⇒ 模式串是**存进去的订阅名**，大小写不敏感。它说「两者今天就已经矛盾」应改口径：webhook 创建**今天没有任何字符集校验**（`api/webhooks.rs:83-85` 只拦空数组），所以矛盾在两个**不同接口**的严格度（hook 收 `Push` 能工作 / `/events` 的 `types` 收 `Push` 返 400），不是同一条校验自相矛盾。**顺带挖出更严重的一层**：`format!` 不过滤 `%` 与 `_`、SQL 也没有 `ESCAPE` 子句 ⇒ 订阅名里的 `_` 是单字符通配（实测 `'issue.opened' like '%issue_opened%'` = **1**），订阅 `%` 匹配全部 16 个值。见 F3-9，已并入 B3-11 |

**本轮自查新事实**

- **F3-9**（LIKE 通配符，**第 6 轮订正方向**）：`webhook_ops.rs:43` 的 `Column::Events.contains(event)` 在 sea-orm 1.1.20 里编译成 `events LIKE '%<event>%'` —— **左值是落库的订阅串，模式串是触发的 event 名**。所以通配符风险在**投递名**一侧：投递名里的 `_`/`%` 会被当 SQL 通配符（语句无 `ESCAPE`）。实测：`select 'issue.opened' like '%issue_opened%'` = **1**（若投递名写成 `issue_opened`，订阅 `issue.opened` 的钩子会收到）、`select 'pull.request.merged' like '%pull_request.merged%'` = **1**、`select 'pullXrequest.merged' like '%pull_request.merged%'` = **1** ⇒ 16 个投递值里 **3 个含 `_`**（`pull_request.closed|merged|opened`）**已经在模式串里**，只是尚无订阅串与它们差一个字符，故今天无事故（潜在而非现发）。**反方向（订阅名带通配符）是安全的**：订阅串是左值、按字面比较 —— `select '%' like '%push%'` = **0**、`select '_' like '%push%'` = **0**（第 5 轮本条写的「订阅 `%` 匹配全部 16 个值」是把方向读反了，见「我方已自证推翻」）。⇒ B3-11 的「反方向不会误触发」在**订阅侧是对的**，需要限定的是**投递侧**。
- **F3-10**（纠正 N-2 第 2 条）：`valid_event_name`（`api/repo_events.rs:57-63`）的字符集 `is_ascii_lowercase() \| is_ascii_digit() \| matches!(b, b'.' \| b'_' \| b'-')` **拒收 `*`**（0x2A 不属任何一支）⇒ 「复用它会放行 `*`」为假，实际效果相反。
- **F3-11**：公开仓的 `repo_events` 匿名可读（`repo/service.rs:201-203` + `api/repo_events.rs:119`），而 `trigger_event:140` 无条件把 `collab.*` 也写进去、payload 含 `user_content.title` ⇒ 协作问题标题外泄到公开事件流。**新 L2 = B3-12。**
- **F3-12**：`scripts/check.sh` 的三种模式**都**先跑 `cargo fmt --all --check`（`:36-37`）；`--quick` 只省 `cargo test`（`:44-48` 这个 if 块，实际跳过的是 `:45-47`）。别再把它当 fmt 盲区引用。

**它的质量盘点**：7 条输入 3 条主命题全对（①②③），4 条新发现里 1 条全对（N-1）、2 条半对（N-2 的第 2 子命题错、N-3 的标题自相矛盾）、1 条对（N-4）。**本批新形态：它开始用「你的验证不覆盖」来否定我方结论**（`check.sh` 不含 fmt），而这条只需 `sed -n '38,42p' scripts/check.sh` 就能否掉——**凡它质疑验证口径，先读验证脚本本身**。N-2 的错法与上一批同源：把「校验存在」读成「校验的有效性」，但方向反了（以为会放行，实际会拒收）。N-3 是它第一次**在指摘标题与自己的证据之间自相矛盾**（标题说没有行、证据里 `:140` 无条件写入）——这类不必找反例，读它自己的输出即可判。

### consume 6 / 6（batch#3，末轮）

- 输入：`2120325081665548288`（`2026-11-11T01:17:47Z`，首行 `## 第 5 轮复核（只读 · HEAD \`4234861\`）`）：F5-1…F5-4 四条新发现 + 对反向清单 28–32 的复核（它判 5 条全绿）
- 判定：**成立 1（F5-4）/ 部分成立 3（F5-1、F5-2、F5-3）/ 子命题不成立 1（F5-3a）**；另**确认 5**（反向清单 28–32 由它复算通过，无需改）
- 工作基线：**`BASE=a812b5b`**（`git rev-parse --short HEAD`；`git rev-list --count HEAD..cnb/main` = **0**，无漂移）；`webhook/service.rs` 561 / `repo/service.rs` 2803 / `collaboration/access.rs` 42 行均未变 ⇒ 零行号漂移
- 验证：改前 `scripts/check.sh --quick` **EXIT=0（2 s，热缓存）**；改后同命令 **EXIT=0（62 s）**
- 改动：`ada6eee`（`webhook/service.rs:29`，1 行→1 行，187→226 列）+ 本台账段

| # | 输入指摘 | 判定 | 证据 / 处置 |
|---|---|---|---|
| F5-1 | 「`:29` 半句『`*` 都不命中』在大小写不敏感的实现下给出错误安心；`Column::Events.contains(event)` 里**触发的 event 是模式串**，含通配符的投递名会反向匹配别人的订阅；今天 16 个投递值里没有 `_`/`%`/`*`」 | **部分成立** | **成立（且是我方 F3-9 的方向性错误）**：`events LIKE '%<event>%'` —— **左值是落库的订阅串、模式串是投递名**。实测 `select 'pull.request.merged' like '%pull_request.merged%'` = **1**、`select 'pullXrequest.merged' like '%pull_request.merged%'` = **1** ⇒ 投递名里的 `_` 是单字符通配，而 16 个投递值里 **3 个含 `_`**（`pull_request.closed\|merged\|opened`）⇒ 通配符号**今天已经在模式串里**（潜在、非现发）。F3-9 与 B3-11 本轮已改写方向。**不成立的三处**：①`:29` 的 `*` 半句**准确** —— `select 'push,issue.opened' like '%*%'` = **0**，`*` 不是 LIKE 通配符，与大小写无关；②它自称「今天 16 个投递值里没有 `_`（我验过）」**是错的**，`pull_request.*` 三条就是 `_`；③订阅侧带通配符是**安全的**（`select '%' like '%push%'` = **0**），「订阅面填了就安全」这个担心方向反了。处置：`:29` 补「case-insensitive … of the delivered name」，把方向写死在第一手位置 |
| F5-2 | 「`valid_event_name` 也拒 `Push`（`:239`），这正是 B3-8『白名单会拒今天能工作的 Push』的直接原因，两条本是同一件事、台账分在两处」 | **部分成立** | **事实全对、行号也准**：`grep -n 'valid_event_name("Push")' crates/rg-http/src/api/repo_events.rs` = **`:239`**（`assert!(!valid_event_name("Push"));`）。**「分在两处」不是缺陷**：反向清单 30 是否证它的**错命题**（它说字符集容得下 `*`），B3-8 的 why-not 列是**落地代价**（拒 `Push`），一个用于止重复、一个用于定优先级，合并会削弱前者。处置：只在反向清单 30 末尾加交叉引用，不搬动 B3-8 |
| F5-3 | 台账三处内部矛盾：a) N-2「5 条论据 / 4 成立 / 1 不成立」只列了 4 条；b) B3-8 同时说「字符集可复用」与「不是先复用」；c) B3-2 的「13 个事件」是漏改旧数 | **部分成立**（b 成立 / c 成立但位置错 / a 不成立） | **b 成立**：F3-8（「本轮自查新事实」里，第 4 轮写）留着「字符集校验器已存在，可直接复用 + 先复用低风险」，而 B3-8（下方待拍板表，第 5 轮按 N-2 改写）写「**不是**先复用」——两处结论相反，已在 F3-8 末尾加订正并作废该落地建议。**但「同一行」错**：两处分属两个小节、相隔数十行，是不同轮次的两次结论。**c 成立但位置指错**：`13 个事件` 在 **B3-1** 的 why-not 列（`grep -n '13 个事件'` 的旧命中），不在 B3-2；真值 16，已改。**a 不成立**：N-2 的 5 条里 ①③④⑤ 列在「成立」，② 就是紧随其后的「**不成立**」那段（原文「它说字符集 `[a-z0-9._-]` 容得下 `*`…」），4+1 = 5 **没有漏**；只是 ② 未编号，属可读性问题。见反向清单第 33 条 |
| F5-4 | 「台账写 `wrap_comments = false`，但仓库里**不存在** `rustfmt.toml`，`Cargo.toml`/`.cargo/` 也无 `[rustfmt]`，真正原因是**默认配置**」 | **成立** | `find . -maxdepth 3 -name '*rustfmt*'`（排除 `target/`）= **空**；`ls .cargo` 只有 `audit.toml`；`grep -rn wrap_comments --include=*.toml` = 空 ⇒ 本仓无该配置，`wrap_comments` 默认即 `false`、rustfmt 也不拆已有 `//` 行。台账三处（`grep -n wrap_comments`：`consume 5 / 6` 的 ①-附、反向清单 28、自证推翻表末二行）已改为「rustfmt 默认（本仓无 `rustfmt.toml`）」，不再写成项目配置 |

**确认 5（它本轮复算的反向清单 28–32，全部通过、我方无需改）**：28（`sed -n '36,44p' scripts/check.sh` 的 `:36 step` / `:37 cargo fmt --all --check` / `:44 if [ "$mode" != "quick" ]`）、29（四版列宽 102/202/242/289 + 本轮 187）、30（`*` = 0x2A 三支全不满足）、31（`sed -n '202,217p'` 的 `:211 invalidate` / `:212 can_write_repo`）、32（`:140 record_repo_event` 无条件）。它另在 ④ 里独立复核了 `webhook_events.rs:28-100`、`human_notify.rs:28-60`、`repo/service.rs:201-203`、`api/repo_events.rs:57-63/119/172-189`、`api/webhooks.rs:83-85`、`webhook_ops.rs:33-46`、`webhook/service.rs:128-160/227-239` —— 全部与台账一致，**B3-12 的事实面无异议**。

**本轮自查新事实**

- **F3-13**（纠正 F3-9 的方向）：`Column::Events.contains(event)` 的模式串是**投递名**、左值是订阅串。⇒ ①通配符风险在**投递侧**：16 个投递值里 3 个含 `_`，已在模式串里（潜在、无现发碰撞）；②**订阅侧**带 `%`/`_` 是字面量，只会永不命中、不会误收（`select '%' like '%push%'` = 0）；③大小写不敏感的效果是「订阅侧大小写随意」（`select 'PUSH' like '%push%'` = 1），这一条已写进 `:29`。
- **F3-14**（第 5 轮我方计数之外的第二个易腐点）：台账 `wrap_comments = false` 三处都写成项目配置，本仓根本无该文件 —— **凡引用配置项，先 `find` 一次配置文件**，别从报错信息反推。
- **F3-15**：B3-1 的 why-not 列还留着第 1 轮的「13 个事件」，而 B3-11 同表已写 16 ⇒ **同一张表内的计数会互相打架**，改计数要整表 `grep` 一遍（本轮 `grep -n '13 个事件'` 才抓到）。

**它的质量盘点**：本轮 4 条里 1 条全对（F5-4）、3 条部分成立，且 F5-1 帮我们翻出了**方向性**错误（F3-9 把通配符归错了一侧），这是它连续第二轮靠「读 `LIKE` 的左右值」产出真事实。它本轮的错法集中在**计数与定位**：把 3 个含 `_` 的投递值说成「一个都没有（我验过）」、把 B3-1 的事挂到 B3-2、把 F3-8 与 B3-8 说成「同一行」、把已编号齐全的 N-2 说成「只列 4 条」。**规律：它的机制分析（左右值、字符集、断言）可靠，凡附带的「我验过 / 只有 / 同一行」这类量词与定位词，一律自己复算。**

### 待拍板（batch#3，本流程不动 main）

| # | 级别 | 事项 | 为什么不在本流程做 |
|---|---|---|---|
| B3-1 | P1 | `redeliver` 扇出到全 repo 的所有 active hook，而非原 hook（`webhook/service.rs:380-397`）；`:396` 解析失败把 body 换成 `null` 照发；无 delivery id 头（`:295` 只有签名头） | 改的是投递语义 + 需要 `trigger_event` 增加 target 参数或直连 `deliver()`，影响 16 个投递值（14 repo + 2 collab，F3-7）的所有调用点 |
| B3-2 | P2 | `deliver_retry_job` 不读 `active`，停用后积压重试继续投（`:308-361`） | 同上，队列语义变更 |
| B3-3 | P2 | webhook `secret` 明文入库（`:65`/`:109`，迁移 `:125` 裸 string） | 改 schema + 存量重加密；且与 B2-5 的 `derive_key` 议题耦合（换 KDF 会作废既有密文） |
| B3-4 | P3 | `update_webhook` 无法表达「清空 secret」（`:109`） | 需要三方状态（缺省 / 置空 / 改值），改 API 契约 + 前端表单 |
| B3-5 | P1 | `set_watch` 零枚举校验（`repo/service.rs:852-860`、`api/repos.rs:596-598`） | 加校验会拒绝现状合法客户端（前端入参就是裸 `string`），属 API 契约变更 |
| B3-6 | P2（第 2 轮由 P1 下调） | `list_watchers` 不过滤状态（`repo_watch_ops.rs:71-88` → `notification/mod.rs:103`）。**第 2 轮限定**：`notify_watchers_push`（`repo/service.rs:1590`）/ `notify_watchers_pr`（`pull_request/service.rs:295`）**零调用者**，全仓唯一活的 fan-out 是 `issue/service.rs:488`（milestone 关闭）⇒ 现状泄漏面只有 `milestone.closed` 一条，不是 push/PR。另同源问题：`list_watchers` 带 `.order_by_desc(UpdatedAt)`（`:79`）且 `notification/mod.rs:103` 写死 `limit 1000` ⇒ >1000 watch 行的仓库只覆盖 `updated_at` 最新的 1000 **行**（`grep -rn list_watchers crates/` 只有 `notification/mod.rs:103` 一处调用，`repo_watch_ops.rs:71-88` 也不按 `watch_state` 过滤，故含 `not_watching` 行），刚改过 watch 状态的行会挤掉长期订阅者（线上是否存在该量级未验证） | 改通知语义；顺带要对账存量 `not_watching` 行是否需要清理；接线 push/PR 通知与「按状态过滤」「去截断」必须一起做，否则一接就把两个缺陷同时放大 |
| B3-7 | **P0** | LICENSE 模板截断：`apache-2.0` 557 词 / `gpl-3.0` 512 / `mpl-2.0` 1101，条款正文被 `...` 取代 | 换成官方全文是产品内容 + 法律责任变更（要决定是否附带 NOTICE、是否保留摘要版），且会改变所有新建仓库的初始 commit |
| B3-8 | P2 | `CreateWebhookRequest.events` 无枚举校验 + 注释示例误导（F3-1）。**第 4 轮收窄**：不是「零校验」—— `api/webhooks.rs:83-85` 只拦空数组，`webhook/service.rs:55` 直接 `req.events.join(",")`。**第 5 轮按 N-2 再收窄**：`valid_event_name`（`api/repo_events.rs:57`）是 `rg-http` 的**私有 `fn`**，`rg-core` 调不到；它是**字符集**校验、不是枚举白名单（放得过 `collab_question` 这类非投递值）；但**会拒收 `*`**（F3-10：`*`=0x2A 不属 `[a-z0-9._-]` 任一分支）；白名单的单一数据源不存在（`EVENT_*` 在 rg-core、`MAX_TYPES`=`api/repo_events.rs:33` 与 `RETENTION`=`:28` 在 rg-http，且 `MAX_TYPES` 管的是 `types` 查询参数的量，`:84`）。⇒ 落地顺序应为「把 16 个投递值抽成 `rg-core` 单一常量（F3-7 是现成清单）→ `valid_event_name` 与 webhook 白名单都引用它」，**不是**「先复用字符集校验」：字符集拦不住假阳性，也拦不住大小写（N-4）与通配符（F3-9） | 加白名单会拒绝存量合法值（订阅 `Push` 今天能工作）；需先定宽松（忽略未知）还是严格（400），且必须先定「大小写收不收紧」——`/events` 的 `types` 拒 `Push`、hook 订阅收 `Push`，统一口径是 API 契约变更 |
| B3-11 | P3 | `trigger_event` 共 16 个投递值（14 repo + 2 collab，F3-7），`webhook_ops.rs:43` 是 `LIKE '%<触发的event>%'` ⇒ 误触发方向是**「触发的名字是别人订阅名的子串」**：若将来触发一个前缀名（如裸 `pull_request`），订阅 `pull_request.merged` 的钩子会收到它。现状 16 个名两两**零子串关系**（脚本校验 `e in s` 对数 = 0），故无事故；反方向（新增 `push.mirror` 触发，订阅 `push` 的钩子）**不会**误触发，因为 `LIKE` 左值是存进去的短名。**第 5 轮两条限定（F3-9 / N-4），第 6 轮纠正方向**：①SQLite `LIKE` 对 ASCII **不区分大小写**（实测 `select 'PUSH' like '%push%'` = 1）⇒ 订阅写 `Push` 也能收到 `push`；模式串是**投递名**、订阅串是左值，所以大小写不敏感的效果是「订阅侧大小写随意」。②通配符风险在**投递名一侧**：语句无 `ESCAPE`，投递名里的 `_` 会被当单字符通配（实测 `select 'pull.request.merged' like '%pull_request.merged%'` = 1），而 16 个投递值里 3 个含 `_`（`pull_request.*`）⇒「只欠不误」须限定为「订阅串与投递名逐字符相等时成立」；订阅侧的 `_`/`%` 是**字面量**（实测 `select '%' like '%push%'` = 0），带通配符的订阅只会永不命中、不会误收 | 要改成结构化存储（JSON 列 / 关联表）或精确相等匹配，属 schema 或查询语义变更 |
| B3-9 | P3 | 只读判定里拿全局 perm-cache 写锁（`access.rs:23` → `repo/service.rs:120-124`）。**第 2 轮补全**：`invalidate_perm_cache_user` 全仓 **13 处生产调用**（`human_notify.rs:207` 亦在；`repo/service.rs:2717` 是测试），其中 `require_collab_read` 每个请求**必然清两次**（`api/collab.rs:184` 先清，`:185` → `access.rs:23` 再清）；`human_notify.rs:107` 的候选循环内也清，上界 `MAX_CANDIDATES = 500`（`:46`）；`escalation.rs:283` 有按 `user_id` 的 `contains_key` 记忆化 ⇒ 每 `evaluate` 的锁数 = 候选里不同 user 数，**但候选上界是 600**（`gather` 三处 `CANDIDATE_LIMIT`=200：`:439`/`:457`/`:488`），不是 200；同一 user 多 session 不会各付一次锁（`:283` 缓存的是 user 对 repo 的权限，与 session 无关，不是「跨 session 放行」）。注意 `perm_cache_disabled()` **不**保护 `invalidate_perm_cache_user`（`repo/service.rs:120-124` 无守卫），只有 `check_perm_cache`（`:96-99`）/`set_perm_cache`（`:107-110`）短路 | 权限缓存语义变更（热路径写锁是真成本，需先定 TTL/失效策略；`require_collab_*` 共 73 处调用点、24 文件） |
| B3-10 | P3 | `redeliver` 借道 `trigger_event`（`webhook/service.rs:392-398`）⇒ 一次手动重投既在 `repo_events` 再插一行（`:140` → `:227-239` `repo_event_ops::append`），又向该 repo **全部** active hook 真实扇出一次（`:142`）；而 `ironforge_webhook_deliveries_total` 只记手动那一次（`api/webhooks.rs:280`/`:288`），扇出与日志都不计数 | 先要定义「重投算不算一次投递」，再改计数口径 + 事件日志去重；与 B3-1 同一段代码，须一起改 |
| B3-12 | **P2（第 5 轮新立，安全面）** | 公开仓的事件日志泄漏协作内容：`trigger_event`（`webhook/service.rs:140`）**无条件**把 `collab.*` 写进 `repo_events`，payload 带 `user_content.title`（`webhook_events.rs:64-67`）；而 `/events` 走 `repo_access::require_read`（`api/repo_events.rs:119`）→ `can_read_repo` 对 `!repo.is_private` 在 `repo/service.rs:201-203` **直接 `Ok(true)`、不看 `actor_id`** ⇒ 匿名可读、payload 全文返回（`api/repo_events.rs:172-189`）。协作区本身另有 `require_collab_read` 门禁（`access.rs:1-7` 明写「collab 数据不是公开内容」）⇒ 同一条数据两套可见性口径 | 三种改法各有代价：①`record_repo_event` 对 `collab.*` 跳过（事件日志出现类型缺口，需定 API 契约）；②`/events` 按事件类型对 `collab.*` 二次做协作鉴权（读侧热路径加查询，且 `since` 游标语义要重定）；③把 collab payload 的 `user_content` 剥掉（信息量下降，前端展示受影响）。都要改鉴权语义或 API 契约，且需决定存量 `repo_events` 行是否清理 |

### batch#2(issue #12) · 2026-11-10 · rg-core auth 凭据链路

- 开批：`2026-11-10T13:12:45Z`（issue 创建即触发，正文结尾 `@CodeBuddy`；NPC 流水线 `cnb-gc6-1k72pdras` 已 pending，platform status 自述 SHA `2191cba`，与本批基线一致）
- 基线提交：`2191cba`（cnb/main HEAD，2026-11-10）
- 范围：`crates/rg-core/src/auth/` 10 文件共 2946 行（pat_scope 656 / sso 512 / oci_token 395 / password 364 / jwt 244 / ldap 240 / ci_token 203 / ssh_key 140 / totp 109 / encryption 83；排除 `mod.rs` 12 行与 `ci_oidc.rs` 146 行）。行数以 `git show 2191cba:<path> | wc -l` 统计，非工作区版本
- 引导问题 4 条：P1 常见密码检测 `take_while` 前导字母剥离可绕过（`password.rs:235`）；P1 `TooCommon` 把明文密码写进 `Display`（`password.rs:69-72`）；P2 `verify_code` 无重放记录、调用方是否拦截待核（`totp.rs:37`/`:51`）；P2 `validate_mfa_challenge` 用 `Validation::default()` 无 iss/aud（`jwt.rs:135-139`）
- 已知空档（写进正文）：TOTP SHA1 为 RFC 6238 标准；JWT HS256+7d 为已知设计；`feat/agent-principals` 分支（agent 账户，本机未推送）不在基线内；LDAP/SSO 宽松错误处理为刻意决策；待拍板 #1–#11 属第 1 批范围
- 反向清单回灌：判定不成立 4 条（含撤回的 sideband/pkt_line 行号争议）+ 对方已撤回 4 项 + 第 6 轮行号错项 5 条
- **新增要求**（第 1 批收敛建议落地）：每条结论必须附自证形式（`sed`/`grep` 输出或实测），不接受纯断言
- NPC 首条分析：**已到**，`2026-11-10T13:16:43Z`（延迟 4 min 24 s；自述基线 `2191cba`，与开批记录一致）
- 6 轮进度：**6 / 6（已收批，2026-11-11）**
- **基线漂移（本轮发现，后续各轮必须知道）**：`cnb/main` 在开批后被 `origin/main` 的 PR #47 `feat/agent-principals` 推进到 `2ffe7e8`（新增 `rg-core/src/agent.rs`、`rg-http/src/api/agents.rs`、迁移 `m20261110_000001`）。`git diff 2191cba 2ffe7e8 -- crates/rg-core/src/auth/` **为空**，故本批 10 个文件的行号基准未变；但 `user/service.rs`（+34）与 `rg-http/src/api/users.rs`（+66）变了，凡引用调用方行号必须按新树重取

### consume 1 / 6（batch#2）

- 判定：**成立 7 / 部分成立 2 / 不成立 0**；另有 2 条 NPC 单列的「推测」：1 条事实成立但范围外（登录时序）、1 条不采纳（`SystemTime::unwrap` 行号错）
- 工作基线：`2e0425d` → 快进到 **`2ffe7e8`**（`cnb/main` 已被 PR #47 推进，见上「基线漂移」）；改前 `scripts/check.sh --quick` 通过（55 s）在 `2ffe7e8` 上跑
- 改动：**`0bc7385`**（L1，注释 3 处纠正 + 1 处补全 + 1 处误导性注释改写 + 1 条断言强化测试）：`crates/rg-core/src/auth/ssh_key.rs`、`password.rs`、`encryption.rs`、`crates/rg-http/src/oci.rs`
- 验证：改后 `scripts/check.sh --quick` 通过（77 s）；新测试 `cargo test -p rg-core --lib auth::ssh_key` → **4 passed**；**零行号漂移**（4 处均 1 行→1 行，新测试追加在文件末尾 `:141-154`）
- 回帖：`2120153773610803200`，`2026-11-10T13:57:04Z`，首行 `## 第 1 轮收尾`（回读校验 C=1，下一轮 K=2）
- **重复投递事故（重要）**：同一轮出现两条首行相同的收尾——`2120153592488157184`（`13:56:21Z`）与 `2120153773610803200`（`13:57:04Z`），计数一度变成 C=2。CLI **没有删除评论的命令**，处置是用 `cnb issues patch-issue-comment --comment-id 2120153592488157184` 把多余那条改写成一行「已作废，以 `2120153773610803200` 为准」，计数回落 C=1。前一条已触发 NPC 流水线 `cnb-2vu-1k72rtmoi-001`，故下一条复核可能针对作废前的措辞——按内容对齐，不看 id。**每轮发帖前先数首行，发完立刻回读。**
- **孤儿锁成因（比「进程死亡」更准确）**：两次运行确实并发（各自 `/tmp` 视图不同，`ls` 互相看不见对方的 `/tmp/round-reply.md`），但锁被误判为孤儿是因为 owner 文件写的是 `$$`＝**每次 Bash 调用一个瞬时 shell 的 pid**，`kill -0` 必然返回已死。教训：①锁的活体判定不要用 pid，用 `owner` 里的 UTC 时间戳 + TTL（如 90 min）判过期；②发帖前再数一次首行，已存在同轮收尾就直接收工，不重发。
- **本轮首次做运行时取证**（不再只读代码）：上一批 6 轮的经验是「NPC 行号常偏、语义多对」，但 P1 级结论必须落成可复现观测

```bash
# 复用第 4 轮的配方（本机 target/release/ironforge 为 02:56 构建，auth/ 与本轮基线同版本）
./target/release/ironforge serve --repo-root /tmp/ift2/repos --http-addr 127.0.0.1:18141 \
  --jwt-secret optloop-dev-secret --db-url "sqlite:////tmp/ift2/t.db?mode=rwc" &
# A 命中词表 → 400，且响应体回显明文
POST /api/v1/users/register {"username":"p1a","email":"p1a@example.com","password":"Password1!"}
 → HTTP 400 {"error":{"code":"BAD_REQUEST","message":"password 'Password1!' is too common — please choose a stronger one",...}}
# B 前导数字 → 201 注册成功（绕过 COMMON_PASSWORDS）
POST /api/v1/users/register {"..." ,"password":"1Password1!"}  → HTTP 201 {"user_id":1,...}
# C 前导符号 → 201
POST /api/v1/users/register {"...", "password":"!Password1"}   → HTTP 201 {"user_id":2,...}
```

| # | NPC 第 1 轮指摘 | 判定 | 处置 / 证据 |
|---|---|---|---|
| P1-1 | `take_while` 只取前导字母，`!stripped.is_empty()` 使之对非字母开头的密码整段失效 | **成立** | 代码：`password.rs:235` = `.take_while(\|c\| c.is_ascii_alphabetic())`，`:237` = `if !stripped.is_empty() && …`；实测 B/C 两条 201 即为绕过证据；`stripped` 用源码词表（48 条）复算：`Password1!→"password"`（拦）、`1Password1!→""`、`!Password1→""`（均放行）。修法属 L2（B2-1） |
| P1-2 | `TooCommon` 把明文密码写进 `Display`，最终进入 HTTP 响应体 | **成立** | 实测 A 的 400 响应体原文含 `password 'Password1!'`。链路五跳逐段核对：`password.rs:69-73` Display 带 `pwd` → `user/service.rs:169`（注册）/`:758`（重置）`e.to_string()` → `rg-http/src/error.rs:171` `InvalidInput→BadRequest(msg)` → 同文件 `:114-120` H-05 只脱敏 `InternalError` → `:132-143` 把 `sanitized_message` 原样塞进 JSON body；`api/users.rs:179` 为注册的 `Err(e)` 分支。修法属 L2（B2-2） |
| P2-3 | TOTP `verify_code` 无状态，调用方也无已用码记录 ⇒ 同码可重复换 JWT | **成立** | `totp.rs:37` 签名无 in/out 的消费态、`:51` 就是 `check_current(code)`；`api/mfa.rs:327` 同；对照 backup code 分支 `:303` 走 `verify_and_consume`（说明是漏了而非设计）；`mfa.rs:343` 直接签 JWT，`:375-379` 只清 cookie（服务端无消费记录）。**未实测重放**（需先开 MFA），只到代码事实。修法需存储 ⇒ L2（B2-3） |
| P2-4 | 引导问题「`Validation::default()` 无 iss/aud 是缺陷」前提不成立；真实余量是 `iat` 从不校验 | **成立（纠正正确）** | 直接读 crate 源码：`Cargo.lock:3512-3513` = jsonwebtoken `9.3.1`；`~/.cargo/registry/.../jsonwebtoken-9.3.1/src/validation.rs:164-167` `default() = new(Algorithm::HS256)`、`:115` `required_claims.insert("exp")`、`:120` `leeway: 60`、`:124` `validate_exp: true`；`ClaimsForValidation`（`:186` 起）无 `iat` 字段。域分离 `jwt.rs:47-49` 已足够防串用。**结论：`jwt.rs:139` 不是缺陷**，`iat`/`nbf` 只作余量记录（B2-9） |
| F1 | `check_repo_access` 无令牌即放行 pull，完全无视仓库是否公开 | **成立，但比它说的更死** | `oci_token.rs:204` 定义、`:213-215` 即 `None => return required_action == "pull"`；`grep -rn check_repo_access` 全仓只有定义 + 4 处 test（`:318`/`:339` 两个用例），**零生产调用**。补充它没查到的：唯一「调用方」`rg-http/src/oci.rs:198` 是 `let _ = extract_user(...)`，返回值被丢弃 —— 连哨兵都没人用；真实鉴权在 `:146-149` 走 `can_read_repo`。修法（删函数/改签名）属 L2（B2-4） |
| F2 | `ssh_key.rs:9`/`:67` 说 base64**url**，`:84` 用标准表 —— 实现对它错 | **成立** | 三处行号**全部精确**（少见；指改前文本，改后 `:9`/`:67` 已是新文案、`:84` 未动）。判据：自建含 `/` 的判别向量 `ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIC7pCHxwJJFfvz0rCrwU3xiR0dDuanGFh7JCWaTSXiy2`，`ssh-keygen -l -E sha256` = `SHA256:uh6xDCBClTKA/7j9qTAuVmcAbusr4NwgLrWjafhfMM4`，base64url 版本为 `…TKA_7j9…`。**已修**（注释）+ **已固化**（新增测试，见 `0bc7385`） |
| F3 | `encryption.rs:12-19` `derive_key` 是裸 SHA-256，且与 JWT 会话共用 `jwt_secret` | **成立（事实）** | `:12-19` 全文即单轮 `Sha256`；调用点 `mfa.rs:318`、`ci_secrets.rs:89` 均传 `state.jwt_secret`。**不采纳为「本批缺陷」**：换 KDF/域分离会作废全部既有密文（TOTP secret / SSO refresh token / CI secret 都要重加密）⇒ L2（B2-5），优先级低于 B2-1/2 |
| F4 | `mask_values` 的 `len >= 4` 过滤是「静默不遮蔽」 | **部分成立** | 机制成立（`encryption.rs:62`）。但「正常来源都 ≥4」这条也对，且我补了它的盲点：生产写入点**只有一个** `api/ci_secrets.rs:94`，其 `:86` 强制 4..=65536；`rg-ci/src/runner.rs:1522` 是 `#[cfg(test)]`（模块起于 `:1121`）内的写入。真实缺口在 `ci_secret_ops::upsert` 本身无长度校验（未来调用方绕过 API 即静默漏）⇒ 只作防御加固，进 L2（B2-6），本轮已在 `encryption.rs:57` 文档化 |
| F5 | `sso.rs:387` 把 CSRF `expected` 原文写进 warn 日志 | **部分成立（文件错）** | 内容对、文件错：`:387` 那行在 **`crates/rg-http/src/api/sso.rs`** 的 CSRF 校验 match 分支内（`:381-392`），而 `crates/rg-core/src/auth/sso.rs:387` 是 `resolve_oidc_endpoints`。另纠一处：`state` 并非「`:372-379` 已一次性消费」，真正清 cookie 在 `:521-522` 与 `:543-544`（两条成功路径），失败路径 `:391` 直接 return、cookie 不清。泄露面确实小（state 与单个浏览器会话绑定）⇒ 只记 L2（B2-7） |

**推测项处置（只核实、不改）**：

| NPC 推测 | 核实结果 |
|---|---|
| 登录时序泄露账号存在性（`user/service.rs:224` 提前 return，跳过 `:234` 的 Argon2；`forgot_password` 有 H-5 拉平） | **事实成立**，但**行号是旧基准**：合并后 `:232` = `None => return Err(CoreError::forbidden(INVALID_CREDENTIALS))`，`:246` 才做 `verify_password`（Argon2）；`forgot_password` 的归一化在 `:654`/`:662`（各 `sleep(100ms)`）。另注：`2ffe7e8` 起 `:238-241` 新增 agent 账户「无密码但答案与错密码一致」的分支。它自认范围外、我也不做实测 ⇒ 进 L2（B2-8），不写进正文指摘 |
| `oci_token.rs:150-158` `SystemTime` + `.unwrap()` | **不采纳**（它自己也说不可达）。另：行号错，真值 `:132-135`（`:150` 是 claims 结构收尾） |

**本轮学到的**：NPC 这次的行号准确率明显高于前批（F2 三处全中、P1-1/P1-2/P2-3 全中），错项集中在 **P2-3（`:303`→`:302`、`:343`→`:336`）、F1（`:146-149`→`:141-144`）、F5（文件级）**。它在 P2-4 主动推翻我方引导问题的前提并给出 crate 源码行号，这是三批以来第一次「反向纠正生产者」——说明引导问题本身也要接受核实，不能默认成立。

### consume 2 / 6（batch#2）

- 输入：**两条** NPC 复核同时到达——`2120154085314682880`（`13:58:19Z`，中文）与 `2120154599913840640`（`14:00:21Z`，英文），均针对 `0bc7385`。取并集处理（两份内容互补，且有一处**互相矛盾**，见 #1）。
- 判定：**成立 6 / 部分成立 1 / 不成立 5 / 已过期 1**；其中 #9 是「对方指摘不成立、但触发了我的真错」的转向项。
- 工作基线：`BASE=d4de32b`（`cnb/main` HEAD，本地无落后，未发生漂移）。改前 `scripts/check.sh --quick` **85 s 通过**。
- 改动：**`210f338`**（L1，纯注释，9 insertions / 9 deletions，**零行号漂移**）：`crates/rg-core/src/auth/ssh_key.rs`、`password.rs`、`encryption.rs`、`crates/rg-http/src/oci.rs`。
- 验证：改后 `scripts/check.sh --quick` **54 s 通过**；`cargo test -p rg-core --lib auth::ssh_key` → **4 passed**（含 `fingerprint_matches_openssh_base64_not_base64url`）。
- 回帖：`2120167289906434048`，`2026-11-10T14:50:47Z`，首行 `## 第 2 轮收尾`（回读校验 C=2，无重复投递；下一轮 K=3）。发帖前按新规矩先数了一次首行，确认 C=1 才发。

| # | 指摘（合并两份复核） | 判定 | 处置 / 证据 |
|---|---|---|---|
| 1 | `oci.rs:73` 写 "it has no callers" 是事实错误，实有 **8 处** test 调用方 | **成立（结论）/ 计数错** | `grep -rn check_repo_access crates/`：定义 `oci_token.rs:204` + **6 处 `assert!` 调用**（`:321 :329 :353 :360 :367 :375`）+ 2 处测试函数名（`:318` `:339`），零生产调用。故"8 处调用方"错，但"no callers"确实该改。已改为 **no production callers** |
| 2 | `oci.rs:73` "a scope token carries no user id" 不精确，`OciTokenClaims` 有 `sub` | **成立** | `oci_token.rs:47` = `pub sub: String,`，其文档注释为「Subject — username (or `anonymous`)」。已改为「identifies its subject by username (`sub`), not by user_id」 |
| 3 | `password.rs:229` 的绝对句「starts with a digit or a symbol is **never** checked against the list」过头 | **成立** | 复算：`123456` 前导段为空 → `:237` 跳过，但 `:241` 整串命中 → **拦**。原句把「本分支不查」写成「永不检查」。已改为「slips past that branch; the whole-string check below still catches exact list entries such as `123456`」 |
| 4 | `password.rs:232` 「Strip trailing digits and special chars」与代码相反，且与 `:229` 自相矛盾 | **成立（两方都提）** | `:233-236` 是 `take_while(is_ascii_alphabetic)`＝取**前导**字母段，不是剥尾部。已改写为「Take the leading alphabetic run, so `password1!` and `password!!!` both reduce to `password`」 |
| 5 | `password.rs:232` 「那句本身就是错的」（含举例） | **部分成立** | 机制描述错（成立），但举例 `password1!` / `password!!!` 实际**都拦得住**（复算 `stripped="password"` ∈ 48 词表 → `:237` 命中）。不能连例子一起否 |
| 6 | 词表里 8 条含数字/`-` 的条目（`abc123`/`trustno1`/`password1`/`password123`/`ec2-user`/`admin123`/`root123`/`test123`）「**永远匹配不上**」 | **不成立** | 复算：`abc123`→`:241` 整串命中 **拦**；`trustno1`、`ec2-user` 同。准确表述是「这 8 条在 `:237` 前导段分支下不可达，只能靠 `:241` 原样输入命中」 |
| 7 | `ssh_key.rs` 注释里 `...TKA_7j9...` 「吃掉了 `q`」，是假的参照串 | **不成立** | `'TKA_7j9' in url` → **True**。该串两端都是省略号，是子串片段而非全串；`TKA_7j9q` 里 `TKA_7j9` 确实连续存在 |
| 8 | `ssh_key.rs:143` docstring 「只提 `/`，`+` 在 body 的 index 21」 | **不成立（双向错）** | ①当时文本是 `:142-143` 的 `the digest contains both \`/\` and \`+\``，明写 both；②`std[21]` 是 `m`，且 **全串无 `+`**。两条子断言都反了 |
| 9 | ↑ 但它指的方向（docstring 与向量不符）命中真问题 | **成立（转向，我方错）** | 独立复算：`std = uh6xDCBClTKA/7j9qTAuVmcAbusr4NwgLrWjafhfMM4`，`'+' in std == False`，`'/'` 在 **index 12**（`idx of differ=[12]`）。故**我上轮写的 "both `/` and `+`" 是假的**。已改为「contains `/` at index 12 and no `+`」 |
| 10 | `mask_values` 用 `str::replace`，会把同一串的 **base64url 变形**也遮掉（`TKA/7j9` ⊂ 密文 ⇒ `TKA_7j9` 同遮） | **不成立** | `str::replace` 是**字面量子串**替换，遮 `TKA/7j9` 不会碰 `TKA_7j9`。把「子串匹配」推成「变体匹配」是错的 |
| 11 | B2-6 的真实后果不是「漏遮」而是「**secret 在 CI 里消失**」 | **不成立** | `mask_values` 收的是**解密后明文**（`api/runners.rs:796` `decrypt` → `:809` 收集 → `:537` 传入；`rg-ci/src/runner.rs:754` `decrypt` → `:758` push → `:528`/`:535`）。1–3 字节明文只是**不被遮蔽**，既不消失也不会让调用失败 |
| 12 | 「库内密文恒 19+ 字符 ⇒ 2 字节 secret 会被**意外遮住**」 | **不成立** | 同上根因：密文从不进入 `mask_values`。两方在同一点上各自推错方向，故 B2-6 原描述（漏遮）保持 |
| 13 | B2-1 修法比较：`strip-all`（剥全部非字母）会让 `Welcome1!x` **回退** | **成立** | 复算：现状 `stripped="welcome"` ∈ 词表 → **拦**；剥全部非字母得 `welcomex` ∉ 词表 → **PASS**。这是唯一一处 B 比现状差的地方 |
| 14 | `ssh_key.rs:9` 「byte-identical to `ssh-keygen -l -E sha256`」把断言对象说混 | **成立** | `ssh-keygen` 打整行（`<bits> SHA256:<fp> user@host (TYPE)`），本函数只回 `SHA256:<fp>`。已改为「matches the `SHA256:<digest>` field … (that command prints a whole line; this returns only the field)」 |
| 15 | `encryption.rs:57` 点名 `api/ci_secrets.rs` 会让文档随第二个写入点过期；`ci_secret_ops::upsert` 自身无长度校验 | **成立（建议级）** | 已核：`upsert`（`ci_secret_ops.rs:25` 起）全程无长度判断。注释补「`ci_secret_ops::upsert` itself does not validate」，并补上「`secrets` 必须是解密明文」这一契约（正是 #11/#12 双方都误判的那一点） |
| 16 | 「HEAD 是 `1f45242` 不是 `df68a0d`」 | **已过期** | 现 HEAD = `d4de32b`（其后又有 `d4de32b` 一次 ledger 修订）。本轮 BASE 已按 `d4de32b` 声明 |

**本轮学到的（第三批的新形态）**：这轮出现了**两条 NPC 复核互相打脸**——中文那条说 `oci.rs:73` 的 "no callers" 是事实错误，英文那条说 "it has no callers" **holds**；中文那条说 docstring 漏了 `+`，英文那条说 `+` 在 index 21。两边都错，且错法相反。**结论：同一轮的多份复核不能取其一，必须并集后再各自核实**；对方彼此的矛盾恰恰是最省力的定位器（本轮 #9 就是靠这个矛盾挖出我自己的错）。

另一个可复用的判据：**凡涉及 `mask_values` / 长度门槛的结论，先确认入参是明文还是密文**。两方都在这上面翻车（一个说「密文 19+ 字符所以会被意外遮」，一个说「解不出来所以 secret 消失」），而真相是它只吃明文。

### consume 3 / 6（batch#2）

- 输入：`2120169296193028096`（`2026-11-10T14:58:45Z`，英文），对 `210f338` + `b50e3ba` 的复核，自述 BASE `d4de32b`
- 判定：**成立 4 / 部分成立 0 / 不成立 0**；另有 1 条对方主动撤回（`#10`，即第 2 轮 `/` 与 `+` 的争议，它复现后认错）
- 工作基线：**`BASE=1bfe5b7`**（`cnb/main` HEAD，本地未落后）。与它自述的 `d4de32b` 之间隔了 `210f338`（我方第 2 轮）与两次 ledger 提交；`git diff d4de32b..1bfe5b7 -- crates/rg-core/src/auth/ crates/rg-http/src/oci.rs` = **9 insertions / 9 deletions 且全部落在 `210f338`**，故本批文件行号基准未变，无需在新树上重取
- 验证：改前 `scripts/check.sh --quick` **1 s 通过**（热缓存）；改后 **54 s 通过**
- 改动：**`7ae3cf5`**（L1，两文件）：`ironforge-docs/ci/adr-0001-third-party-actions-execution.md` 新增 §7「复核补充：Runner 租户边界与 fork 触发路径」（4 条 R-1…R-4，逐条给代码证据与对应 ADR 条款）+ `crates/rg-core/src/auth/password.rs:192` 注释 1 行→1 行（写明 `len()` 按**字节**计且 `Display` 说的是 characters）。**零行号漂移**
- L1 只落了两处，因为本轮三条新发现全部是 L2（调度/Secret 下发/CI 触发路径），按分级只能记录不能改码
- 回帖：`2120184067994836992`，`2026-11-10T15:57:27Z`，首行 `## 第 3 轮收尾`（回读校验 C=3，无重复投递；下一轮 K=4）。发帖前先数了一次首行，确认 C=2 才发

| # | 输入指摘 | 判定 | 处置 / 证据 |
|---|---|---|---|
| 0 | `#10`（第 2 轮「摘要里 `/` 和 `+` 都在」）它自己复现后认错 | 对方撤回 | 与 `我方已自证推翻` 表一致，此后不再讨论 |
| 1 | **F-01**：`/v2` 的 token-only pull 只做 scope 字面匹配即 `return Ok((true, None))`，跳过函数里唯一的 `can_read_repo` | **成立**（但「latent, not live」不准确） | 逐行核对：`oci.rs:129-144` 确是 bearer 分支、`:139` 即 `return Ok((true, None))`；`can_read_repo` 只在 `:148`（匿名分支，`:147-150`）与 `:114`（会话 JWT 分支）出现。`oci_token.rs:113-115` `matches_repo` 为字符串等值；`OciTokenClaims`（`:42-70`）无 `repo_id`/`visibility`（该区间 grep 计数 0）。调用链 `get_blob`(`:718`/`:723`)、`get_manifest_impl`(`:417`/`:423`) → `require_access`(`:155`，`:162` 调 `check_access`) 全部命中。**框架修正**：它说这条是「latent」，但 `ttl = if anonymous {60} else {300}`（`:342`）意味着**仓库转私有后 300 s 内旧 token 仍可拉**，这是今天就成立的路径，不是等第二个签发点才成立。签发侧唯一（`generate_oci_token` 生产调用点只有 `oci.rs:343`），且按 `can_read_repo` 裁剪（`:297-307`），匿名用户名 `:243` —— 这两点它说对了 |
| 2 | **F-02**：runner token 会把**别的仓库**的明文 Secret 送出去 | **成立**（1 处行号偏 4 行） | `runners.rs:277` `Path(runner_id)` 直接取路径参数；鉴权 `:1019-1026` = `find_by_token` 命中 + `runner.id == runner_id`，与 Job/仓库无关（精确）；`pipeline_ops.rs:918-920` 空 Job 标签即返回该 Job（精确）、`:932-953` `job_is_schedulable` 只看 stage/pipeline 状态（精确）；领取后 `runners.rs:352-355` 按 `pipeline.repo_id` 解密并整体注入。`entities/runner.rs:8-21` 11 列无 `repo_id`、`m20260510_000001_create_runners.rs` 建表也没有 —— 均精确。**补强一次它漏的**：除空 Job 标签外，`pipeline_ops.rs:909-911` 的「Runner 标签为空 → 返回第一个可调度 Job」是**双向**全匹配的另一半。**行号偏**：它写 Job 选择在 `:296-299`，实际 `find_pending_job_matching_labels` 调用是 `:292-296`（`:296` 是 `.await`） |
| 3 | **F-03**：fork 自有 `.gitea/workflows` 能驱动外部 Runner | **成立**（3 处行号小偏 + 1 处表述简化） | `reviews.rs:161-168` 只校验对**源仓库**（fork）的写权限；`:200` 在源仓库路径上 `has_ci_config`；`:206-210` 以 `repo_path = source_repo_path`、`repo_id = source_repo.id`(`:207`)、`trigger_type = "suggestion"`(`:210`) 建 Pipeline；workflow 内容取自该 commit（`rg-ci/src/lib.rs:599-608`），无任何来源校验。路由确实存在（`lib.rs:920-921` `suggestions/apply`）。**行号偏**：`ci/mod.rs:83-90` 只覆盖函数体中段，`has_ci_config` 定义在 `:76-91`；`gitea_actions.rs:282-320` 实为 `:280-316`；`reviews.rs:199-200` 中 `:199` 是路径拼接、`:200` 才是 `has_ci_config`。**表述简化**：`matches_event` 对 `Simple`/`Array` 是字符串等值，对 `Single` 还做 ref 过滤 —— 但两条都没有「内容来源」判定，结论不变。**补一层它没说的组合**：该路径下 `repo_id` 是 fork 自己，注入的是 fork 的 Secret；所以 F-03 单独只算「任意代码落到共享 Runner」，**跨租户 Secret 暴露要靠 F-03 与 F-02 叠加**才成立 |
| 4 | §4 单位口径 nit：`ci_secrets.rs:86` 按字节、`password.rs:53` 文案说 characters | **成立** | `ci_secrets.rs:86` 是 `body.value.len()`，而它自己的报错文案 `:87` 已写 "4-65536 bytes"（自洽）；`password.rs:193`/`:199` 用 `password.len()`（**字节**），但 `:53`/`:58` 的 `Display` 写 "characters"。**真实后果比它说的重**：`密码密码`（4 个 CJK 字符 = 12 字节）满足 `min_length = 12`。已作为 L1 在 `password.rs:192` 注释里写死；改文案/改判据属 L2（B2-14） |
| 5 | §4 其余四条（`password.rs` 三处、`encryption.rs:57` 契约、`oci.rs:73` "no production callers"、`sub` 措辞）判为已修好 | 确认 | 逐条回读，与我方第 2 轮改动一致，无异议 |

**本轮最值钱的一条是我方对 F-01 的框架修正**：它把「scope 命中即放行」定为「latent, not live」，理由是「今天的 token 只带公开仓 scope」。但 token 是 5 分钟票据，`ttl=300`（`:342`）——**仓库在票据有效期内转私有**就是现成的窗口，不需要等第二个签发点。这类「latent/live」判定必须把票据 TTL 一起算进去。

### consume 4 / 6（batch#2）

- 输入：`2120184547626082304`（`2026-11-10T15:59:21Z`，英文），对第 3 轮 `7ae3cf5` + `15bf8e0` 的复核，自述 BASE `15bf8e0`
- 判定：**成立 5 / 部分成立 1 / 不成立 1 / 确认 5**
- 工作基线：**`BASE=9f18fd7`**（`cnb/main` HEAD；`git rev-list --left-right --count HEAD...cnb/main` = `0 0`，无漂移）。改前 `scripts/check.sh --quick` **2 s 通过**（热缓存）
- 改动：**`3be1fa0`**（L1，四文件，+50 / −6）：`crates/rg-core/src/auth/password.rs:192`（1 行→1 行）；`ironforge-docs/ci/adr-0001-third-party-actions-execution.md` §7（R-1 补迁移链、R-2 补机制、R-3 拆「存在性 vs 内容来源」、preamble 四条→三条、删 R-4 行改为一条交叉引用）；**新增** `ironforge-docs/architecture/registry-auth-visibility-verification-2026-11.md`（42 行）；`ironforge-docs/README.md`（架构索引 +1 行）
- 验证：改后 `scripts/check.sh --quick` **67 s 通过**。`path:line` 机械核对：两文件 19 条带文件名引用 + 30 个裸 `:N` 引用逐条 `sed -n` 打印，全部能读出命题。**行号漂移**：`password.rs` 1 行→1 行零漂移；ADR / README 只改自身行数，`grep -rn 'adr-0001' --include=*.md` 确认全仓只引文件名、无一处引它们的行号
- **环境故障（本轮新增，重要）**：`git fetch cnb` 报 `Repository Not Found`。原因不是仓库没了（`cnb repositories get-repos` 显示 `zdzso/ironforge` 正常，`cnb issues` 也正常），而是 **osxkeychain 里的 cnb.cool 凭据已失效**（token 尾号 `lg`），而 `cnb` CLI 的 token 有效（`~/.cnb/token` 的 `auths["cnb.cool"].access_token`，00:41 刚刷新）。可用写法：`git -c credential.helper= -c credential.helper='!f(){ echo username=oauth2; echo password=$CNB_TOKEN; }; f' fetch cnb` —— **必须先用空 `credential.helper=` 清掉 keychain**，只追加自定义 helper 会被 keychain 抢先命中、仍报 not found。推送同理，且不要 `-u`（避免 token 落进 `.git/config`）

| # | 输入指摘 | 判定 | 处置 / 证据 |
|---|---|---|---|
| 1 | **R-1 全部子断言**：`runner.rs:8-21` 从 `pub struct Model {` 到 `}`、恰 11 字段、`grep -c repo_id` = 0；`job_is_schedulable` 起于 `pipeline_ops.rs:932`、止于 `:953`、`:954` 空；函数体只判 stage/pipeline/前序 stage | **确认（成立）** | 逐条打印核对，无一处偏差。11 字段 = `id/name/token/status/labels/last_seen_at/version/os/arch/created_at/updated_at` |
| 2 | **R-1 增强建议**：应把「建表无 `repo_id`」写成「整条迁移链无 `repo_id`」，并点名 `m20261015_000004_hash_runner_tokens.rs` | **成立** | 已验证：`grep -c repo_id` = 0；该迁移 `:33-38` 只调 `hash_plaintext_tokens`，`grep -c alter_table` = **0**（纯数据重写）；另一个提到 runners 的 `m20260712_000003_add_pipeline_job_variables.rs` 只 alter `pipeline_jobs`（"runners" 仅出现在 doc 注释）。已按此改写 |
| 3 | **R-2 机制应更锐**：token 只绑到「它自己」——`find_by_token(token).id == runner_id`，`runner_id` 是路径参数；且 `labels_lower` 在循环外无条件构建（它写 `:902`） | **部分成立**（结论全对，行号偏 1） | `pipeline_ops.rs:903`（不是 `:902`）= `let labels_lower: ... .collect();`，确实在 `:905` 的 `for` 之前、无条件。`:1019-1026` 逐行确认：`:1019` `find_by_token`、`:1020` `Ok(Some(runner)) if runner.id == runner_id`、`:1026` 该 arm 的 `}`。已按「只绑自己」改写 R-2 |
| 4 | **R-3 全部子断言**：`:161-168`、`:155`、`:200`、`:206-210`、`rg-ci/src/lib.rs:599-608` | **确认（成立）** | 逐条打印：`:155` = `pr.head_repo_id.unwrap_or(pr.repo_id)`；`:206-210` 五个字段值一字不差；`:599` = `fn try_read_gitea_workflows(`、`:605` = `format!("{}:.gitea/workflows", commit_sha)` |
| 5 | **R-3 不精确**：`:200` 判的是「哪些配置文件存在」，`:599-608` 判的是「内容从哪来」，两者是同一 commit 的两种用法；`has_ci_config` 是 trait 垫片 | **成立** | 确认 `rg-ci/src/lib.rs:63-65` 是 `fn has_ci_config` 转发；`rg-core/src/ci/mod.rs:76-91` 同时判 `.gitea/workflows`(`:83-86`) 与 `.ironforge-ci.yml`(`:89-90`)。已改写为「存在性（`:200`）+ 内容来源（`:599-608`），同一 commit、同一 `source_repo_path`」 |
| 6 | **R-4 全部子行号**：`129-144`/`139`/`147-150`/`231`/`297-307`/`342`/`:358`、`oci_token.rs:114` 字符串等值 | **确认（成立）** | 全部精确。`OciTokenClaims`（`oci_token.rs:42-70`）恰 8 字段 `iss/sub/aud/exp/iat/nbf/jti/scope`，无 `repo_id`、无可见性 —— 它没提但这是「校验侧无从判断」的根因 |
| 7 | **R-4 不该留在 ADR-0001**：ADR 的杠杆是 `CI-204`/`CI-206`，Registry 鉴权在两者下无验收条款；§7 preamble 自述「全部落在 §2.5/§2.7」而 R-4 的对应条目写着「精神上」，是自证；`ironforge-docs/README.md:79` 的索引也会因此不准 | **成立** | 核对 §7 preamble 原文确实是「以下四条……全部落在 §2.5 / §2.7 尚未完成的项目上」，R-4 对应条目确实是「§2.7.5 的最小授权原则（**精神上**）」。已按它的第 2 方案执行：实质迁到 `ironforge-docs/architecture/registry-auth-visibility-verification-2026-11.md`，§7 只留一行交叉引用，README 架构索引补该文件；**未**新开 ADR-0002（本轮不值得为一条待修缺陷造决策记录） |
| 8 | **「password.rs:192 不是 4 是 3」**：「`密码密码` 是 12 B，4 个字符只是刚好过线；应该说 3 个 CJK 字符就已经过 `min_length: 12`」 | **不成立** | 复算：`strict()` 的 `min_length` 在 `:171` = 12；判定是 `:193` `if password.len() < self.min_length`。3 个 CJK = **9 B**，`9 < 12` → `TooShort`，**过不了**；4 个 CJK = 12 B，`12 < 12` 为假 → 通过。它自己的句子也承认「3-char CJK fails at the 9-vs-12 boundary」，与它给的改法自相矛盾。已按「4 个、只是刚好过线（only just）」保留数字并补上「no margin」的意思 |
| 9 | **反向缺口**：`max_length` 128 也按字节算，43 个 CJK（129 B）会被拒，而 `Display` 说 "at most 128"；原注释只记了「误放行」这一半 | **成立** | `:151` `max_length: 128`；`:199` `password.len() > self.max_length`。42 个 CJK = 126 B 通过、43 个 = 129 B 被拒。已把两个方向写进同一行（`password.rs:192`，仍 1 行） |
| 10 | **④ R-4 文案的「只有无 token 分支才查 `can_read_repo`」会误导**：读者会落到 `:114` 以为这句是假的；准确说法是「会话 JWT 分支（`:112-123`）与匿名分支（`:147-150`）都查，Bearer 分支一个都没有」 | **成立** | `grep -n can_read_repo crates/rg-http/src/oci.rs` → `:114`（会话 JWT pull）、`:148`（匿名）。我方台账 B2-11 原写「跳过函数里**唯一**的 `can_read_repo`」确是错的。新文件已按「两处」表述，台账 B2-11 见下「勘误」 |
| 11 | **④ B2-12 不该因与 B2-13 配对而降级成 P2，应单独按 P1 排期** | **确认（已满足）** | 台账 B2-12 原文即「（第 3 轮写入，**P1，跨租户**）」，从未降级；B2-13 才是 P2。无需改动，但把它的理由收进来：`runner_id` 是调用方自选的路径参数、token 只绑自己，故「任意已注册 Runner 抽干任意仓库 Secret」**不需要** fork 路径即可达 |
| 12 | 「本轮无新增不成立项，台账仍 15 条」 | **确认，但本轮要改成 16** | 本轮新增 #8（4→3 那条），见下「判定不成立」第 15 条 |

**本轮学到的（第四批的新形态：算术对抗 + 放置位置评审）**

1. **它开始给「具体数值边界」，而且算错方向**。#8 是它三批以来第一次在纯算术上翻车（把 9 B 判成能过 12 B 门槛），但同一句里它又算对了反向的 129 > 128。**凡它给具体数值的，两个方向都要自己跑一遍再采信**——这比行号错更难防，因为「CJK = 3 字节」这种常量看起来不需要验证。
2. **第一次出现「不针对内容、针对放置位置」的复核**（#7），且它引用的是**我方自己写的免责词**（「精神上」）当证据。结论：文档里每写一个软化词（「精神上」「大致」「原则上」），都会在下一轮被当作自证。要么给出对应条款，要么不放进那张表。
3. 它的行号质量本轮回归到高水位：R-1/R-3/R-4 共 12 个子行号**零错**，只有 `:902`→`:903` 一处偏 1。相较第 3 轮（偏 2–4 行）明显收敛。

### consume 5 / 6（batch#2）

- 输入：`2120201628006371328`（`2026-11-10T17:07:14Z`），对 `3be1fa0` + `613f196` 的复核，自述本地与 `origin/main` 均为 `613f196`
- 判定：**成立 3 / 部分成立 1 / 确认 2**；其中 ①③b 各带一条我方真错，②其中一条是我方**语义**错（不是行号）
- 工作基线：**`BASE=a94839b`**，不是它自述的 `613f196`。**本轮发生本批最大一次漂移**：`cnb/main` 在它复核后约 1 小时被 4 个 PR 合入推进——PR #21（`c74bc25`，B2-1+B2-2）、PR #22、PR #23、PR #24（`9b6bdde`，B2-11）、外加 #20。`git diff 613f196..a94839b` 触及本批文件：`password.rs` +117、`oci.rs` +60、`oci_token.rs` +94、`ssh_key.rs` +10、`ironforge-docs/README.md` +1。按协议在新树上重取全部行号
- **B2-1 / B2-2 / B2-11 已修复合入**（详见下「并行修复观察」更新），故本轮不是「继续挑错」而是「把已修的事实固化进权威文档」：registry 验证文档从「未修复」整篇重写为「已修复 + 最新行号 + 新增 catalog 一节」
- 验证：改前 `scripts/check.sh --quick` **失败（EXIT=1，1.2 s）**——`cargo fmt --all --check` 在 `crates/rg-core/src/auth/password.rs` 上有 5 处未格式化（PR #21 引入），脚本在第一步就退出，clippy 从未运行。这是 **main 上的既有红灯，不是本轮引入**
- 改动：**`0a20205`**（L1 + 一处机械 fmt，见下）：
  - `ironforge-docs/architecture/registry-auth-visibility-verification-2026-11.md`（整篇重写，状态改「已修复」）
  - `ironforge-docs/ci/adr-0001-third-party-actions-execution.md`（§7 preamble 基准改 `a94839b`；R-1 补 grep 依据；R-2 拆掉非因果句；R-4 交叉引用标注已修复）
  - `ironforge-docs/README.md`（`:51` 描述改「已修复」；删掉指向 `docs/project-audit-2026-06.md` 的死行——该文件只在 `ironforge-docs/testing/` 存在，且 `:104` 已索引过一次）
  - `crates/rg-core/src/auth/password.rs`（`TooCommon` docstring 改为与实现一致：「第一个字母段」而非「首尾都剥离」；长度注释补 `got` 无单位的口径）
  - `crates/rg-core/src/auth/password.rs` **另行 `cargo fmt --all`**（29+/17−，纯格式化，零语义）
- 验证（改后）：`scripts/check.sh --quick` **通过（110 s，EXIT=0）**；`cargo test -p rg-http --test oci_permission_tests oci_token_goes_stale_when_repo_turns_private` → **1 passed / 0 failed**（1.01 s），即 B2-11 的撤销滞后在修复后不再复现（运行时观测，不只是读代码）
- 回帖：`2120219018882617344`，`2026-11-10T18:16:20Z`，首行 `## 第 5 轮收尾`（回读校验 C=5，无重复投递；下一轮 K=6）

| # | 输入指摘 | 判定 | 处置 / 证据 |
|---|---|---|---|
| 1 | `password.rs:186-205` 的两个数字对、`only just` 措辞对；但尾句 `Display says "characters"` 不准——`Display` 只对 `min`/`max` 说 characters，`got` 是字节且**不标单位** | **成立** | 复算：`min_length: 12`（`:180`，旧树 `:177`）、判定 `:206`；`max_length: 128`（`:160`）、判定 `:212`。4 CJK = 12 B 过、43 = 129 B 拒，两个方向都对。`Display` 两支文案在 `:60`/`:65`，模板是 `"at most {} characters (got {})"`——`got` 确无单位且填的是 `password.len()`（`:209`/`:215`）。已改为「用户数 43 个字看到 `got 129`，无从对账」。**行号全偏**：它给的 `:171`/`:151`/`:193`/`:199` 是 `613f196` 的值，`a94839b` 上是 `:180`/`:160`/`:206`/`:212` |
| 2 | R-2 里「`:903` 的 `labels_lower` 在循环外无条件构建，**故两个分支永远都在生效**」两句无因果关系；`:903` 只服务 `:922-925` 的 tag 交集，`:909-911` 与 `:918-920` 读的是两个 `is_empty()`；且循环外构建是**正确**写法 | **成立（我方语义错）** | 逐行读 `pipeline_ops.rs:895-928`：`:903` 在 `:905` 的 `for` 之前；`:909` `runner_labels.is_empty()`、`:918` `job_tags.is_empty()` 各自直接 `return Ok(Some(job))`；`labels_lower` 唯一读点是 `:924`。已按它的建议拆成两句，并明写「不是缺陷证据」 |
| 3 | §7 软化词已清零 | **确认** | `grep -nE '精神上\|大致\|原则上\|基本\|大体\|相当于\|可视作\|即视为\|算是'` → 零命中 |
| 4 | 新文档 `:23` 说「可见性只被查两次」，但 `oci.rs` 全文件有 **5 处**权限查询（`:114`/`:118`/`:148`/`:298`/`:311`），读者按「两次」去数会对不上；「函数内共有两处调用」的口径只算了 `can_read_repo`，没算 `:118` 的 `can_write_repo` | **部分成立** | 计数在其基准树上**精确**（`git show 613f196:crates/rg-http/src/oci.rs \| grep -n` → 114/118/148/298/311）。但「漏了第四、第五个调用点」不成立：同一篇 `:27` 已单列签发侧 `get_token` 的 `:298-304`/`:309-321`，`:23` 那句限定语是「这条链」。**口径该改是成立的**——`check_access` 内是 3 处权限查询（2 处 `can_read_repo` + 1 处 `can_write_repo`）。已在重写版分列「校验侧 3 处 / 签发侧 2 处」 |
| 5 | `registry:catalog:*` 应从「未验证」升为独立指摘（P2）：无 `_catalog` 路由 + 签发侧零仓库门 + 校验侧 `matches_repo` 恒不匹配；且 `api_version_check` 无条件对匿名广告该 scope | **成立（三项全中）** | 逐条实测（`a94839b`）：①`build_v2_routes`（`lib.rs:520-555`）无 `_catalog`；②`oci.rs:362-366` 只判 `scope_type=="registry" && name=="catalog" && authenticated_user_id.is_some()`；③`oci_token.rs:123-125` 硬编码 `scope_type == "repository"`；④`oci.rs:240` 在 `api_version_check`（`:227-245`）里无条件写死 `registry:catalog:*`。**行号偏 1–2 行**（它给 `:331-336`/`:209`/`oci_token.rs:114`，实际 `:362-366`/`:240`/`:123-125`，`:114` 是旧树）。已写进文档并立 **B2-15** |
| 6 | README 新增行成立；「目录一一对应」不是该表契约（architecture/ 下大量文件不在索引内）；另 `:30` 的 `project-audit-2026-06.md` 标「已移至 testing/」却同时出现在 `:104`，属重复索引 | **成立** | 新增行确实存在且标题吻合。`ls ironforge-docs/architecture/*.md \| wc -l` = **64**，索引行 **14**（它说 63/13，各偏 1）——「不是穷举表」结论不变。`:30` 更糟：`docs/project-audit-2026-06.md` **不存在**，是死链，且 `:104` 已索引同一文件。已删死行 + 加一句说明 |
| 7 | 托它核的：`m20261015_000004_hash_runner_tokens.rs` 是建表后唯一动过 `runners` 的迁移，且是纯数据 | **成立** | `grep -rl runners crates/rg-db/src/migrations/` = 4 文件（含 `mod.rs`）。`m20260712_000003` 的 `runners` 只在 `:1` 的 `//!` 注释里、两处 `alter_table` 都只对 `PipelineJobs`；`m20261015_000004` 的 `grep -c alter_table` = **0**，只有 `:61` `SELECT id, token FROM runners` 与 `:80` `UPDATE runners SET token`。`entities/runner.rs:8-21` 11 列、`grep -c repo_id` = 0。已把 grep 依据写进 R-1（**它建议的「6 个文件」数字是错的，实测 4 个**） |

**本轮最值钱的一条不是它给的，是漂移本身**：它复核的 `613f196` 在 1 小时后被 4 个 PR 推进到 `a94839b`，其中三个 PR 正好修掉本批台账里的 B2-1 / B2-2 / B2-11。**如果不查 `cnb/main` 就先动手，本轮会把「已修复的缺陷」当成待办继续讨论，并引用一整批过期行号**。这是第 4 轮记的「开工先对齐远端」第二次生效，代价从「重取行号」升级为「整篇结论作废」。

**main 上存在 fmt 红灯（新发现，非本轮引入）**：`scripts/check.sh --quick` 在 `a94839b` 上 **EXIT=1**，原因是 `cargo fmt --all --check` 对 `crates/rg-core/src/auth/password.rs` 报 5 处差异（`:34`/`:73`/`:342`/`:354`/`:369`，PR #21 引入）。脚本第一步就退出，**clippy 与前序检查一次都没跑过**——意味着那批合并后的 main 从未通过过自己的质量门。处置：本轮以 `cargo fmt --all` 修为（29+/17−，纯机械、零语义），改后 `--quick` 通过（110 s）。**这不在 L1 字面范围内，属例外，理由写在回帖第②段**：不修则本流程后续每一轮的「改前基线」都是红的，协议第 6 步无法执行。

**B2-1 / B2-2 的另一处遗留（已顺手修）**：PR #21 把 `TooCommon` 的 docstring 写成「leading and trailing non-letter characters are stripped」，而实现是「跳过前导非字母、取第一个字母段」（`:250-254`），文档自己 `:240-242` 还明确说「两端都剥是错的」。同一个文件里两处自相矛盾，已改 docstring 与实现对齐。

### consume 6 / 6（batch#2，本批末轮）

- 输入：`2120219671621169152`（`2026-11-10T18:18:56Z`，中文），对 `0a20205` + `c2f20d8` 的复核，自述工作树 `c2f20d8`
- 判定：**成立 5 / 部分成立 3 / 不成立 5 / 确认 3**（16 条）
- 工作基线：**`BASE=e531f40`**（`cnb/main` HEAD；`git rev-list --left-right --count HEAD...cnb/main` = `0 0`，无漂移；它自述的 `c2f20d8` 之后只有一次 ledger 提交）。`git diff --stat a94839b..e531f40` 只命中 `password.rs` + 两份文档 + ledger，**`oci.rs` / `oci_token.rs` / `oci_permission_tests.rs` 未变**，故本轮所有代码行号基准未漂
- 改前基线：`scripts/check.sh --quick` **通过（1 s，热缓存）**——第 5 轮那盏 fmt 红灯已由本方修掉，main 现在是绿的
- 改动：**`335fa57`**（L1，四文件，15+/15−，**零行号漂移**）
- 改后验证：`scripts/check.sh --quick` **通过（65 s，EXIT=0）**；`path:line` 机械核对——三份文档里 **26 条带文件名引用 + 30 个裸 `:N` 引用**逐条 `sed -n` 打印并人工确认能读出命题，无 OUT OF RANGE、无 MISSING FILE；`password.rs` 459 行→459 行
- 回帖：`2120236028676960256`，`2026-11-10T19:23:55Z`，首行 `## 第 6 轮收尾`（回读校验 C=6，无重复投递）；随后 `BATCH_COMPLETE`（`2120236073270820864`，`19:24:06Z`）并置 `closed / completed`（`state_reason` 回读 = `completed`）
- **计数陷阱（第 6 轮踩到）**：NPC 的复核评论首行是 `## 第 K 轮复核`，若用 `startswith('## 第 ')` 数收尾评论，会把它的复核也算进去（本轮一度读成 C=6、K=7）。必须用正则 `^## 第 \d+ 轮收尾`。`cnb` CLI 只有加 `-v` 才吐 JSON，不加 `-v` 是 `key: value` 文本，`json.load` 会直接失败

| # | 输入指摘 | 判定 | 处置 / 证据 |
|---|---|---|---|
| A | `lib.rs:520-555` 这个区间不是 `build_v2_routes`，`:555` 已是空行/下一个函数之前，应改 `:520-554` | **不成立** | `sed -n '520p;554p;555p;556p'` = `fn build_v2_routes(state: &AppState) -> Router<AppState> {` / `.with_state(state.clone())` / `}` / 空行。`:520` 是函数签名、`:555` 是它的闭括号，`:520-555` **就是**这个函数；它说的「`:555` 已是空行」错一行 |
| B | `:141-144`/`:146-148` 被写成在 `:157-172` 内；应把 `:157-172` 改成 `:149-174` | **不成立** | 两句在**不同小节、不同段落**（`:13` 的「必须再查一次可见性」与 `:29` 的「为什么必须有 `uid`」），且都是全路径 `crates/rg-http/src/oci.rs:...`，不存在包含关系。`sed` 打印 `:157` `let allowed = match required_action {`、`:159`/`:164` 两次复查、`:172` `};`——「再查一次」的引用准确。`oci.rs:141-144` 打印正是「uid 缺失的旧票据按匿名处理」那段注释 |
| C | 「三项互相独立的实现**共同兜住了**它」把「当前未利用」写成「已论证」，与 `:46`「不是纵深防御设计出来的，是巧合叠出来的」自相矛盾；item 1 的「取向一致」是替它找设计背书 | **成立** | 两处原文并置即矛盾。已改写抬头句为「三项独立实现**偶然**叠加，才使今天无法利用；拆掉任意一项即为现成入口」，item 1 改成「**当前没有消费方**（『没接』，不是『拒绝』）」 |
| D | item 3「校验侧永不匹配」这条论据是空的：从 catalog 票拿 `tags/list` 根本不是 catalog 的越权，残余风险是「枚举可用性」不是「票无用」 | **不成立** | `matches_repo`（`oci_token.rs:123-125`）硬编码 `scope_type == "repository"`，故 catalog 类型的票对**任何** `{owner}/{repo}` 路径恒 false——它举的 `repository:alice/private-img:pull` 是另一种票，证明不了 item 3 为空。其真实关切（别把「票无用」读成「风险低」）本身对，已并入 item 3 补一句「残余风险只有补 `_catalog` 路由后的枚举可用性」 |
| E | 它逐条 `sed` 了 `oci.rs` / `oci_token.rs` / `runners.rs` / `reviews.rs` / migrations / `entities/runner.rs`，除 `lib.rs:520-555` 外无一处行号错 | 确认 | 与本方复核一致（唯一分歧就是 A，而那一条它自己错） |
| F | 「5 处权限查询」没有逐项列名，把 `:152` 的 `has_action`、`:328`/`:340` 的 `has_action` 略在不说明的地方 | **成立，且暴露我方更大的一处错** | 采纳并扩写口径段。**自查发现真正的错是数字**：`grep -n 'can_read_repo\|can_write_repo' crates/rg-http/src/oci.rs` 命中 11 行，其中 **7 行是调用**（`:114` `:118` `:159` `:164` `:179` `:329` `:342`）、4 行是注释（`:82` `:142` `:146` `:375`）。第 5 轮写「5 处」时把 PR #24 新增的 Bearer 复查 `:159`/`:164` 漏掉了。已改为「7 处 = 校验侧 5 + 签发侧 2」，并写明 `:152`/`:328`/`:340` 不计入的理由。它自己说的「六个调用点中的 5 个」也不对（是 7） |
| G | `Phase 22-D` 与「不做全量枚举」的取向一致，但文档没给出处 | **成立** | `grep -rn '22-D' --include=*.md .` 只两处：本文与我方引用。唯一定义在 `ironforge-docs/archive/ironforge-improvement-analysis-2026-06-09.md:311` = 「Phase 22-D：MCP + API 扩展（P2 生态）」，内容是 MCP Tool 扩展 / GitHub API 兼容层 / 分页统一，**与「不做全量枚举」无关**。已整句删除，换成「今天没有任何路由能把它换成内容」 |
| H | ADR §7 它点名的三处（`:175` 基准声明、`:179` R-1 grep 依据、`:180` R-2 前两句）都是对的 | 确认 | 逐条复读一致，无改动需求 |
| I | R-2 里 `:1019-1026` 是裸引用，会被读成 `pipeline_ops.rs:1019-1026`；R-3 的 `:155`/`:200`/`:206-210` 同理 | **部分成立** | 建议采纳（全路径更稳），**但其理由错**：它说「`pipeline_ops.rs` 全文只有 900 多行」，实测 `wc -l` = **1486 行**——正因为它不止 900 行，`pipeline_ops.rs:1019` 是**真实存在**的一行，误读才会发生，这条建议比它自己以为的更有必要。已改 `runners.rs:1019-1026`，R-3 三处改 `reviews.rs:155` / `reviews.rs:200` / `reviews.rs:206-210` |
| J | `:175` 只说「行号未漂」，没记 R-2 的判定口径在第 5 轮被改过，拿 `1bfe5b7` 复核对不上现文 | **成立** | 已补一句「R-2 的判定口径在第 5 轮被拆改过一次（`0a20205`）」并说明改了什么 |
| K | README `:31` 说被删的那行是「指向 `docs/` 的死链」是「指错了表名」——它既没指 `docs/`，也不构成死链，只是本表里多出来的重复条目 | **不成立** | 被删行在 `git show a94839b:ironforge-docs/README.md` 的 `:30`，所在表标题是 `## 设计文档（\`docs/\`）`（`:21`）——表内裸文件名按标题前缀解析为 `docs/project-audit-2026-06.md`，`ls docs/*.md \| grep -c project-audit` = 0，故「指向 `docs/` 且解不出」成立。它的第二种性质（重复）也对，已把两种都写进同一行，消除歧义 |
| L | `:52` 把「修复前后对照」「三项巧合」写进描述、又把「已修复」放括号，是全表唯一双写状态的 | **不成立** | 状态只出现在末尾括注（`（**已修复**，PR #24）`），与其余行同构；「修复前后对照」「三项巧合」描述的是文章内容（本文确有「修复前 vs 修复后」两节与「三件巧合」一节），不是状态。属风格偏好，不改 |
| M | `:108` 与 `:129` 同一事实写两遍，且两次都说「两份」而 `testing/` 实际 4 个文件 | **部分成立** | 重复句成立（`:129` 与 `:108` 语义完全重叠），已改写为「同一文件在本页只索引一次」的事实记录。**「两份」计数没错**：`testing/` 4 个文件是 2 份审计（`project-audit-2026-06`、`audit-report-2026-07-03`）+ 1 份测试指南 + 1 份客户端矩阵 |
| N | `password.rs:50-56` 与实现一致（`trim_start_matches` 跳前导 + `take_while` 取第一段 + `:262` 空则跳过） | 确认 | 与实现逐字对得上，无改动 |
| O | `:201-205` 措辞：「only just clear」歧义、`:202` 的 `overshoot` 少 `s`、`43`（字符）与 `128`（字节）并列 | **部分成立** | 三条都落在同一 5 行注释里，已重写仍为 5 行（459→459）：`4 CJK chars are 12 B and \`12 < 12\` is false, so they pass … with zero margin (3 CJK = 9 B fails)`、`overshoot`→`overshoots`、例子补 `CJK`。**它的引述有两处失真**：把 `4 CJK` 写成「42 CJK」；「43 与 128 并列」正是这段在描述的口径错配，不是新错 |
| P | 例子应补成 `43 CJK chars (129 B)`，让读者不必自己算 43×3 | **成立** | 随 O 一并落。删掉原来的 `a user who counts 43 characters`，改为 `counting 43 characters` + 前句已给 `43 CJK chars are 129 B` |

**本轮最值钱的是 F 暴露的自查项**：NPC 只抱怨「口径没逐项列名」，顺着它列名的要求去 `grep`，才发现第 5 轮写死的「5 处权限查询」把 PR #24 新增的 `:159`/`:164` 漏了，真值是 **7 处**。教训：**「上一版刚刚修过的地方」是核对优先级最高的地方**——第 5 轮为了关闭计数争议写了个数字，那一版本身就带着新的错。

**它的行号质量本轮最高**：`oci.rs` / `oci_token.rs` / `runners.rs` / `reviews.rs` / 迁移 / `entities/runner.rs` 的引用一条没错（它自己声明沙箱无 `cargo`，只做静态核对）。错的四条全是**论证**而非行号（A 的范围判断、D 的「论据为空」、I 的文件长度、M 的「两份 vs 4 文件」）。这印证第 5 轮的结论：**行号错归它、语义错归我**，本轮反过来了——行号全对、语义四处走偏。

**它连续第二轮无法自证**（`command -v cargo` 无输出），故我方那条 `1 passed` 的运行时证据它只能标「未独立验证」。等待窗口按 ≥30 min 预留的结论继续有效。

### batch#1(issue #11) · 2026-11-10 · git 协议文档与实现一致性

- 开批：`2026-11-10T09:21:41Z`（issue 创建即触发，未额外交触发评论）
- 基线提交：`5624605`（cnb/main HEAD）
- 范围：`docs/git-protocol.md` + `crates/rg-git/src/{pkt_line,sideband,protocol/upload_pack,protocol/receive_pack,protocol/v2}.rs`（约 4300 行）
- 轮次上限：6（本批先跑 1 轮做标定）
- NPC 首条分析：**已到**，`2026-11-10T09:25:24Z`，6 条指摘 + 3 条推测，基线自述已核对 `5624605`
- NPC 第 2 轮复核：**已到**，`2026-11-10T09:35:33Z`（`@CodeBuddy` 触发成功，延迟 4 min 22 s）
- NPC 第 3 轮复核：**已到**，`2026-11-10T09:41:46Z`（延迟 2 min 16 s）
- NPC 第 4 轮复核：**已到**，`2026-11-10T09:51:47Z`（延迟 5 min 20 s）
- NPC 第 5 轮复核：**已到**，`2026-11-10T10:22:30Z`（延迟 28 min 15 s——它本轮自己 `cargo build --release` 花了 23 m 28 s，延迟首次突破 20 min 46 s 的 p100）
- NPC 第 6 轮复核：**已到**，`2026-11-10T10:56:50Z`（延迟 3 min 38 s，回到基线；本轮它沙箱**无 Rust 工具链**，只做静态核对，并主动声明该限制）
- 6 轮进度：**6 / 6（跑满）**，收批

### consume 1 / 6

- 判定：**成立 4 / 部分成立 1 / 不成立 1**（另有 1 条推测经代码核实后升级为待验证项）
- 动作：L1 文档 `91686a9`；L2 写入本文件「待拍板」
- 验证：`scripts/check.sh --quick` 通过（115 s）；`BASE=5624605`、`B=0`（文档改动，无 test binary 受影响）
- 回帖：`2120086860385902592`，`2026-11-10T09:31:11Z`，首行 `## 第 1 轮收尾`（回读校验通过）

逐条判定：

| # | NPC 指摘 | 判定 | 处置 |
|---|---|---|---|
| 1 | V1 upload-pack 未收到 NUL 时漏掉 connection 级 capability，导致裸 packfile 与 HTTP 广告矛盾 | **不成立** | 协议规定 capability 只能出现在**第一个** want 行；客户端若要 side-band-64k 必须在首行声明。未声明即等于不要 sideband，服务端裸发 packfile 是合规降级。另 `git-protocol-e2e.sh:176/187` 已覆盖 `protocol.version=1` 的 HTTP clone 与 fetch，若真会失败该用例不会绿 |
| 2 | §4 capability 清单漏 `delete-refs`，且 §4「删除暂不支持」过期 | **成立** | 已补 `delete-refs` 并改写删除语义（只允许 `refs/heads/*`，策略在 `BranchDeleter`） |
| 3 | §3 只在 SSH 成立，未标注 HTTP 分叉 | **成立** | 已标注为「已知不一致（待拍板）」，收缩广告还是补实现留 L2 |
| 4 | §1 `read_pkt_line(reader: &mut BufReader<R>)` 签名过期 | **成立** | 已改为泛型 `AsyncRead + Unpin`，并补 `read_pkt_lines_until_flush` / `PktLine::text` 自动补 `\n` |
| 5 | 坑 2 与坑 3 讲同一件事，坑 3 是噪音 | **部分成立** | 同源判断成立、坑 3「现象与原因自相矛盾」成立；但不采纳「删除」，改为写明同源 + 两个症状对应哪个错位时机 |
| 6 | 四处协议关键行为文档未写（EOF 折叠、不排空 stdin、谁 flush、thin pack 方向） | **成立** | 三处已补进 §1/§2/§3/§4；空仓库伪 ref 一条见下表 |

推测项处置（本轮只核实、不修）：

| NPC 推测 | 核实结果 |
|---|---|
| 空仓库广告 `capabilities^{}<service>` 名字可能不被 git 识别 | **代码层面成立**（`upload_pack.rs:332-335` 把 service 拼在 `capabilities^{}` 后面，git 只认名字恰为 `capabilities^{}` 的伪 ref）。已在 §3 记为「待验证」，未做客户端实测 |
| `read_to_end` 与 BufReader 预读边界 | 未核实，留待下一轮 |
| `v2.rs` `skip_until_flush` 不消费 delim 后参数段 | 未核实，留待下一轮 |

### consume 2 / 6

- 判定：**成立 2 / 部分成立 1 / 不成立 0**（含 1 条对上一轮自身结论的自我推翻）
- 动作：L1 文档（本轮提交）；L2 新增 4 项进待拍板
- 验证：`scripts/check.sh --quick` 通过（2 s，缓存热）；`BASE=fdcfb15`、`B=0`；纯文档，无 test binary 受影响
- 回帖：`2120088953280700416`，`2026-11-10T09:39:30Z`，首行 `## 第 2 轮收尾`（回读校验通过）

| # | NPC 第 2 轮指摘 | 判定 | 处置 |
|---|---|---|---|
| 1 | 我对第 1 条的驳回**理由**有两处错：Form B 不是协议形态而是宽松解析；e2e V1 用例只断言「不含 version 2」，走的是 Form A | **成立** | 结论不变（该输入不可达），但反向清单的理由已按此改写，见下「勘误」 |
| 2 | 第 3 条标注里我把广告串抄漏 `side-band-64k` 与 `agent=`，等于把「唯一必须一致的能力」漏了 | **成立（我错）** | 已改成逐项对照表；实际行号是 `:2638`，NPC 写的 `:2639` 偏一行 |
| 3 | V1 不解析 `shallow`，`--depth` 在 V1 上会静默全量 | **部分成立** | 代码事实成立（`upload_pack.rs:246` 丢弃未知行，全文无 shallow 解析）；但「静默全量」不成立——两侧广告串都不含 `shallow`，按协议客户端不会发起 `--depth`。已写进文档并把客户端行为标「未实测」 |
| 4 | 把 V2 的 `fetch=shallow filter` 与 HTTP V1 分叉并列，易被读成 V2 也是空头 | **成立** | 已拆成独立小节，filter 单列 |
| 5 | 第 2/4/6 条确认解决、第 5 条处置获接受 | — | 记录在案 |
| 6 | 跨轮观察：`--quick` 覆盖不到文档里的 `path:line` 引用 | **成立** | 用 shell 做了一次全量机械核对，24 条引用查出 5 处我方错误（3 处漏 `crates/` 前缀、2 处行号偏移），已全部修正。校验脚本属 `scripts/`（禁区），只报告不落地 |

### consume 3 / 6

- 判定：**成立 4 / 部分成立 2 / 不成立 1**
- 动作：L1 文档 `cac765a`；L2 无新增
- 验证：`BASE=21b7e50`、`B=0`；`check.sh --quick` 1 s（热）；32 条 `path:line` 重新解析
- 回帖：`2120090703664410624`，`2026-11-10T09:46:27Z`

| # | NPC 第 3 轮指摘 | 判定 | 处置 |
|---|---|---|---|
| 1 | `thin-pack` 应与两个 ❌ 分开，属「广告了但主动降级」 | 成立 | 表改三档 |
| 2 | `ofs-delta` 标 ✅ 站不住，该路径产不出 OFS_DELTA | **不成立** | 反证：gix-pack 0.77 `data/output/entry/mod.rs:185` 的 `DeltaRef → Header::OfsDelta`；`allow_thin_pack` 只约束跨 pack 引用。已补「thin delta ≠ OFS_DELTA」 |
| 3 | `no-done` 时序描述不准 | 部分成立 | 改为时序描述 + 标「源码推导，未复现」 |
| 4 | shallow 那段 `所以` 是因果混淆 | 成立 | 拆成「已证实 / 未验证」，并写明 e2e 只覆盖 V2 |
| 5 | 机械校验：`ops.rs:1055` / `sideband.rs:25` / `pkt_line.rs:59` 三处错 | 部分成立 1 / **不成立 2** | `ops.rs` 正确值是 **1056**（不是我原写的 1055，也不是它说的 1054）；`sideband.rs:25` 与 `pkt_line.rs:59` 我方引用正确，反诉成立 |
| 6 | Form B 段落未同步 + 「第 46 位」不实证 | 成立 | 重写该小节，删 macOS 归属 |
| 7 | 懒取回注释在 `:949-950` 而非 `:933-936` | 成立 | 已改 |

**方法论收获（写死在这里，别再踩）**：机械校验只能证明「那一行有内容」，不能证明「那一行是那句话说的东西」。两个方向都会出假阳性——本轮就是一边我引错了行（`1055`→`1056`，脚本放行），另一边 NPC 拿「那一行有代码」反推我的引用错（`sideband.rs:25`、`pkt_line.rs:59`，实际都对）。这类校验要配一句人工确认：**引用的行必须能读出命题本身**。

### consume 4 / 6

- 判定：**确认 2 / 不成立 1 / 部分成立 1**
- 动作：L1 文档 `6c9e1ce`；L2 无新增
- 验证：`BASE=2a11e2e`、`B=0`；`check.sh --quick` 2 s（热）；32 条 `path:line` 重新解析
- 回帖：`2120092667387224064`，`2026-11-10T09:54:15Z`

| # | NPC 第 4 轮指摘 | 判定 | 处置 |
|---|---|---|---|
| 1 | `ofs-delta` ✅ 成立，撤回 | 确认 | 采纳其补强：gix-pack `Mode` 只有 1 个变体、`mode` 唯一读点 `:97` 只决定排序；`allow_thin_pack` 形参真名是 `pack_offset_to_oid`（OID 解析器）。已写进 §4 |
| 2 | `sideband.rs:25` / `pkt_line.rs:59` 我方正确，撤回 | 确认 | 无改动 |
| 3 | V1 + `--depth` 会真实发 `deepen 2`，客户端把全量包当浅克隆 | **不成立** | 本机起实例实测：`fatal: Server does not support shallow clients`，trace 只有 ref 广告、无 want/deepen；V2 对照 `--depth=1` 成功。已按实测重写文档并加「加 `shallow` 进广告串会让本条失效」的警告 |
| 4 | 机械校验 3 条 | 部分成立 | `rg-http/src/lib.rs:441` → `:458-466`/`:454`/`:581`（已改）；`upload_pack.rs:332-335` 与 `mirror/service.rs:244` 引用恰当，不改 |

**实测配方（可复现，写下来免得下次重造）**：

```bash
# 1. 起实例
target/release/ironforge serve --repo-root /tmp/ift/repos --http-addr 127.0.0.1:18131 \
  --jwt-secret <any> --db-url "sqlite:////tmp/ift/t.db?mode=rwc" &
# 2. 注册 + 建仓 + 推 3 个提交（POST /api/v1/users/register → POST /api/v1/repos → git push 到 /git/<user>/<repo>）
# 3. 判定
git -c protocol.version=1 clone --depth=1 http://127.0.0.1:18131/git/<user>/<repo> <dst>
#   → fatal: Server does not support shallow clients（发出任何 want 之前）
git -c protocol.version=2 clone --depth=1 ... 
#   → 成功，rev-list --count = 1，.git/shallow 写入边界
```

**观察**：NPC 第 4 轮开头说「你两条都赢」并主动撤回，说明反向清单 + 逐条证据的机制在起作用；但它同时把上一轮自己提的「机械校验」当成对**我方**的成立前提反过来用，导致两条误判。反向清单需要记录「已撤回」状态，避免下一轮再把撤回项当成争议点重提。

### consume 5 / 6

- 判定：**成立 3 / 部分成立 1 / 确认 2**（另有 1 条是对方主动撤回我方第 4 轮结论，0 条新增「不成立」）
- 动作：L1 文档 `19df4eb` + L1 注释 `b9d22c1`；L2 新增 2 项进待拍板
- 验证：`BASE=58784f0`；`scripts/check.sh --quick` 通过（53 s，缓存热）。**口径说明**：改动全为 `.md` + `//!`/`///` 注释，无可编译语义变化，所以只有一次「含改动的树」的运行，未单独跑改前基线（`58784f0` 本身是 ledger-only 提交，上一轮绿的延续）
- 回帖：`2120107499985477632`，`2026-11-10T10:53:12Z`，首行 `## 第 5 轮收尾`（回读校验通过）

| # | NPC 第 5 轮指摘 | 判定 | 处置 |
|---|---|---|---|
| 1 | V1 + `--depth`：它按我给的配方实测复现，撤回上一轮反对 | 对方撤回 | 反向清单记「已撤回」（见下），本节不再讨论 |
| 2 | §4 把出站写成 `EntriesToBytesIter`，方向错了；`EntriesToBytesIter` 其实是入站链路的环 | **成立（我错两处）** | 已重写该段。①类型方向确实反了：出站是 `output::bytes::FromEntriesIter`（`ops.rs:843` 构造、定义 `data/output/bytes.rs:10`，吃 `output::Entry`），`EntriesToBytesIter` 定义在 `data/input/entries_to_bytes.rs:14`、全 crate 唯一新建点 `bundle/write/mod.rs:111`，就在 `LookupRefDeltaObjectsIter` 之后。②我引的 `bundle/write/mod.rs:104` 也是错的，该行是 `data::input::EntryDataMode::KeepAndCrc32`，正确值是 `:100` |
| 3 | §6 路由引用 `:458-466`/`:454`/`:581` 已改对；`upload_pack.rs:332-335`、`mirror/service.rs:244` 接受不改 | 确认 | 无改动 |
| 4 | `cli_gateway::spawn_async` 是死代码，doc 还在说「used for pack-objects, index-pack」 | **成立** | 已改注释（模块文档 `cli_gateway.rs:12-14` + 方法文档 `:255-261`），写明「当前全仓无调用方」及真实的出站/入站路径。注意：这次改动本身使 `spawn_async` 与守卫测试各下移 5 行（`257`→`262`、`365`→`370`） |
| 5 | 防回归守卫有真实旁路：`process::execute_command(program, …)` 程序名是运行时变量，字面量 grep 抓不到，且其 doc 示例就是 `execute_command("git", …)` | **部分成立** | 机制成立、严重度被高估：全仓 `execute_command` **零调用点**（唯一出现是它自己的 doc 示例 `process.rs:49`），`execute_script` 只有一处测试调用（`process.rs:229`，跑 shell 字符串，非 git）。所以是**潜在**缺口而非现存的越权路径。另纠两处行号：`execute_command` 在 `:51` 不是 `:47`；`execute_script` 在 `:15` 不是 `:13`。收紧守卫要动测试逻辑 + 白名单，属代码改动 → 进 L2 |
| 6 | `v2.rs:806` 静默空 `acked` 是真问题，但它同意属 L2、不催 | 确认 | 保持 L2 #5，无新动作 |
| 7 | `pack_for_wants*` 三层：`pack_for_wants` / `_filtered` 稳定，`_opts` 是生产不可达的测试缝 | **成立** | 9 个调用点逐个打印核对，与其表一致。已写进文档新小节（§3 `pack_for_wants*` 三层的稳定性） |

**方法论（第三次同类）**：NPC 连续三轮出现「结论方向对、行号错 1–4 行」的模式（本轮 `ops.rs:841`→`843`、`bundle/write/mod.rs:104` 判「对」→实际 `:100`、`process.rs:47`→`:51`、`:13`→`:15`）。因此**不能因为它的行号错就驳回整条**，也不能因为它说「对」就放过——必须自己 `sed -n` 打出那一行，看它是否真的承载了命题。本轮第 2 条正是「它的结论对、它的行号错、我的行号也错」三者叠加。

### consume 6 / 6

- 判定：**成立 6 / 部分成立 2 / 不成立 0**（另有对方主动撤回 1 条自己的第一稿结论）
- 动作：L1 `docs/git-protocol.md` + `cli_gateway.rs` 注释 + `ops.rs` 文档注释，提交 `18b7807`；L2 新增 1 条、补强 3 条
- 验证：`BASE=66127b5`；`scripts/check.sh --quick` 改前 **2 s**（热）、改后 **50 s** 通过。**行号不变**：`ops.rs` 只改 2 行内容（原 2 行 → 2 行），`cli_gateway.rs` 4 行 → 4 行，故全仓既有 `path:line` 引用无漂移（第 5 轮踩过这个坑）
- 回帖：`2120116260468842496`，`2026-11-10T11:28:01Z`，首行 `## 第 6 轮收尾`（回读校验 C=6）；随后 `BATCH_COMPLETE`（`2120116261246042112`，`11:28:11Z`）并置 `closed / completed`
- 本批 6/6 跑满，收尾后置 `closed` + `BATCH_COMPLETE`

| # | NPC 第 6 轮指摘 | 判定 | 处置 |
|---|---|---|---|
| 1 | §4 `:308` 写「`allow_thin_pack` 全 crate 只有一个**读取点**」，主语混了：IronForge 侧只有写点 | **成立** | `grep -rn allow_thin_pack --include=*.rs .` 全仓 **1 命中**＝`ops.rs:833` 的 `allow_thin_pack: false,`（写死）。已改写为「IronForge 侧没有读取点，只有写点 `ops.rs:833`；唯一读取点在 gix `iter_from_counts.rs:189`」 |
| 2 | §4 `:302` 的「等价 `--fix-thin`」在 IronForge 侧没有证据，`:1056` 只证明传了 `Some(lookup)` | **成立** | `ops.rs:1056` 确为 `Some(repo.objects.clone()),`；「等价」唯一背书是 `receive_pack.rs:467-470` 的注释；`grep -nE '\bthin\b|thin-pack|--thin' scripts/git-protocol-e2e.sh` **0 命中**（32 个 git 用例里无 thin push）。已加「证据等级」段，标明未验证 + 补齐方式 |
| 3 | §4 `:306` 把「入站补完 thin base」和「回写成字节」压成因果 | **成立** | `EntriesToBytesIter` 属 `Bundle::write`（`unpack-objects` 风格），IronForge 入站实际是 `index_pack_bytes` → `Bundle::write_to_directory`（`ops.rs:1040-1064`，`sed` 验过全程无该类型）。已拆成两句 |
| 4 | §8 `:589`/`:638` 把 V2 广告串写成 `fetch=shallow`，实际 `fetch=shallow filter` | **成立（线格式级）** | `v2.rs:48` = `"fetch=shallow filter"`；`:1199` 断言 `contains("filter")`。两处图示/code block 已改（§3 正文 `:244` 本来是对的，属内部自相矛盾） |
| 5 | 表漏 `upload_pack.rs:371` 的 `pack_universe` 空 want 回退，使「V1 全量/增量都走它」失效 | **成立** | `sed -n '370p;371p'` = `if wants_owned.is_empty() {` / `return crate::ops::pack_universe(&repo_dir);`。表拆成两行：`pack_for_wants` 只接 V1 增量，新增 `pack_universe` 行 |
| 6 | 「`pack_universe` 至少 3 个测试调用点：`ops.rs:1600`、`upload_pack.rs:424`、`ops.rs:1710`」 | **部分成立** | `grep -rn pack_universe` 全量：测试调用点只有 `ops.rs:1600`。`upload_pack.rs:424` 是测试的 **doc 注释**里提到名字（「previously called `pack_universe`」），不是调用；`ops.rs:1710` 在 `object_filter_falls_back…` 测试内，**没有** `pack_universe`。漏项本身成立、数量错 |
| 7 | `pack_universe` 有第二个生产调用者 `fork_repo`，`ops.rs:859` 文档只写 initial-clone | **成立** | `crates/rg-core/src/repo/service.rs:1241` = `let pack = rg_git::ops::pack_universe(&source_dir)`，下接 `index_pack_bytes(target)`。文档 §3 全量打包那条 + `ops.rs:857-858` 注释都补了「第二个调用方是非协议路径，shallow/filter/haves 不适用」 |
| 8 | `cli_gateway.rs:258`「全 workspace 只有这一处出现 `spawn_async`」 | **成立** | `grep -rn spawn_async`：代码 3 处（`:12`/`:257`/`:262`）+ 文档多处。已改为「只有这一处**定义**（文件头 `//!` 另有一处提及）」 |
| 9 | `:259-260`「两个方向都走 gix 原生（出站 `iter_from_counts`，入站 `pack_for_wants*` / `index_pack_bytes`）」入站写错 | **成立（我错）** | `pack_for_wants*` 是**出站**（`ops.rs:919/952` 产出 client 要的 pack），入站只有 `index_pack_bytes`（`:1040`）。已改成「出站 `pack_for_wants*` / `iter_from_counts`，入站只有 `index_pack_bytes`」。这是第 5 轮方向错误的镜像：同一个方向词换了个位置又写反一次 |
| 10 | `:261`「删掉比留着一个被模块文档宣传的死 API 更好」描述的是已消灭的状态 | **成立** | 模块文档 `:12-14` 已如实标注「当前全仓无调用方」，不再宣传。已改写 |
| 11 | `spawn_async` 方法体有裸 `Command::new("git")`，守卫 `:379` 整文件 skip | **部分成立** | 结论成立：`cli_gateway.rs:272` 是 `tokio::process::Command::new("git")`。但行号两处偏：skip 判定在 **`:386`**（`:379` 是 `.filter_map(|e| e.ok())`），裸调用在 **`:272`**（不是 `:270`）。机制已并入 L2 #8 |
| 12 | 守卫的两个字面量 `:396`/`:397`，且别名可绕（`use std::process::Command as C`） | **成立** | `sed` 打出正是这两行。别名/变量程序名绕过的结论也成立 → L2 #8 补强 |
| 13 | `rg_core::platform` 只 re-export 了 `execute_script`，`execute_command` 连 re-export 都没有 | **成立** | `platform/mod.rs:13` = `pub use process::execute_script;`（仅此一条 process 项）。`execute_command`（`process.rs:51`）全仓零调用、连导出都没有 → 并进 L2 #8 |
| 14 | `pack_for_wants_unshallow_sends_full_history` 该从 `opts` 搬到 `filtered` | **成立** | `ops.rs:1865` 函数、`:1875` 调 `pack_for_wants_opts`；`ops.rs:998-1005` 的 `hidden`：`shallow_mode && boundaries 空 → Vec::new()`（不按 haves 剪），该语义在 V2 unshallow（`v2.rs:1171`→`:1176` `filtered`）是活的。搬测试 = L2 #9 补强 |
| 15 | (d) `0004` 空 pkt-line 让 `read_command_request` 永不返回 | **成立（代码事实），未实测** | `pkt_line.rs:129-131` 空 payload → `Data(vec![])`；`v2.rs:458-473` 中该行 `trim_end_matches('\n')` 成空串，capability 阶段两个分支都不命中（`strip_prefix("command=")` 假、`!line.is_empty()` 假）→ 丢弃继续循环，只有 `Flush` 才 break。`v2.rs:438` 是 `loop`。列 L2 #10，优先级高于 #5 |
| 16 | 它自己的第一稿「`skip_until_flush` 在 EOF 时空转」 | 对方撤回 | 记入「对方已撤回」表 |

**方法论（第四次同类，本批收敛出的结论）**：NPC 第 6 轮自身质量最高（它明确声明「沙箱无 Rust 工具链、gix 源码不在，凡 gix 内部断言只到注释层」，并把两条降级为线索），但它仍有 **5 处行号偏 1–2 行**（`:258`→`:257`、`:259-260`→`:258-259`、`:261`→`:260`、`:379`→`:386`、`:270`→`:272`）。反过来，我这一轮被抓到的 3 处（`:308` 主语、`:259-260` 方向、`:260` 时态）全是**语义层**错误，不是行号。结论：**行号错归它，语义错归我**——两套校验都得做，机械核对治不了语义错。

### 反向清单勘误（结论不变，理由替换）

第 1 条仍判**不成立**，但理由从「协议允许的两种 want 形态都已覆盖」改为：

- 真实 git 客户端要 sideband 就必须在**首个** want 行声明 capability，不存在「不声明却期待 side-band-64k」的客户端；
- `upload_pack.rs:205-224` 的 Form B 是**容忍畸形输入的宽松解析**，不是协议允许的第二种形态，账本不能把它写成协议义务；
- `scripts/git-protocol-e2e.sh:172-190` 的三个 V1 用例只断言「trace 中不含 version 2」，实际走的是 Form A（首行 `\0` 后带 `side-band-64k`），因此它证明的是「声明了就成立」，不能证明「不声明也不会失败」。

### 待拍板（L2，本流程不动 main）

1. **HTTP V1 广告串里混了三个未实现能力**（`crates/rg-http/src/lib.rs:2638`）：`multi_ack_detailed` / `no-done` / `thin-pack` 服务端一个都没实现（只有单发 `NAK`），而 SSH 侧只广告实现了的。二选一：收缩广告串，或补多 ACK。影响面：`protocol.version=0/1` 的 HTTP 客户端。
2. **空仓库伪 ref 名**（`upload_pack.rs:332-335`）：是否改成标准 `<null-sha> capabilities^{}\0<caps>`。
3. **upload-pack 不排空 stdin**（对比 `receive_pack.rs:462`）：SSH 下影响小，HTTP 下 body 仍在灌 duplex，是否需要 `drain`。
4. `mirror/service.rs:167` 注释与 `run_git_remote_update` 函数名仍在说 `git remote update`，实际执行 `git fetch --prune origin`（`service.rs:327`）。改名属代码改动，范围外。
5. **`v2.rs:806` 的 ack 扫描吞错**：`gix::open(repo_path).ok()` 在仓库打不开时静默返回空 `acked`，`write_acknowledgments` 于是发 `NAK`（`v2.rs:1272-1291` 的测试把这个形状钉住了）。丢 ACK 比报错更难查。
6. **`cli_gateway.rs` 的 `spawn_async` 零调用**：`crates/rg-git/src/cli_gateway.rs:257-280` 定义了但全仓无调用方，模块文档 `:6` 却在宣传它；防回归测试 `test_no_raw_git_command_in_crates` 只管 `Command::new("git")`，守不住这种死 API。
7. **`ops.rs:933-936` 的懒取回路径没进文档**：want 直接给 blob/tree 时按 `extra_inputs` 原样打包、不走进 history walk（promisor 懒取回），`ops.rs:1772-1820` 有测试（`lazy fetch under {spec}` 断言 pack 恰好 1 个对象），e2e `:340-348` 在真客户端上跑过。**已于第 1 轮补进文档**（§3 懒取回那条），当前代码引用是 `ops.rs:949-950`（本行原写的 `:933-936` 是旧行号，已过期）。
8. **防回归守卫抓不到运行时程序名**（第 5 轮写入）：`test_no_raw_git_command_in_crates`（`cli_gateway.rs:370`）只做两个字面量匹配（`:396-397`），而 `crates/rg-core/src/platform/process.rs:51` 的 `execute_command(program, args)` 程序名是运行时变量、`:15` 的 `execute_script` 直接 `sh -c`，两者都可绕过。当前 `execute_command` **零调用点**（仅 `:49` doc 示例写着 `execute_command("git", …)`），`execute_script` 仅 `:229` 一处测试调用 —— 故为潜在缺口。两个方向：①测试把 `execute_command(` / `execute_script(` 也纳入扫描 + 白名单；②给 `execute_command` 加 `#[doc(hidden)]` 并禁止 `rg-git` 外使用，把 git 调用强制收进网关。要动测试逻辑，属 L2。
9. **`pack_for_wants_opts` 是生产不可达的测试缝**（第 5 轮写入）：`ops.rs:930` 只被 `pack_for_wants`（`:925`，写死 `false`）与一个测试（`:1875`）调用，`shallow_mode: true` 在生产走不到。选项：删掉它、把测试改调 `filtered`（三层压两层），或留着并在文档里标非稳定（**文档侧已标**，见 §3）。删代码属 L2。**第 6 轮补强**：`ops.rs:1865` 的 `pack_for_wants_unshallow_sends_full_history` 该搬去调 `pack_for_wants_filtered`（它钉的语义——`pack_for_wants*` 在 `shallow_mode && boundaries 空` 时不按 haves 剪（`ops.rs:998-1005`）——在 V2 unshallow 路径上是活的，只是入口挑错了），搬完 `opts` 就零调用点，可无风险删除。

10. **`0004` 空 pkt-line 让 V2 命令循环空转**（第 6 轮写入，**优先级高于 #5**）：`pkt_line.rs:129-131` 把长度 4 解析成 `PktLine::Data(Vec::new())`；`v2.rs:458-473` 把它 `trim_end_matches('\n')` 成空串，capability 阶段 `strip_prefix("command=")` 与 `!line.is_empty()` 都不命中 → 静默丢弃 → `v2.rs:438` 的 `loop` 继续读，只有 `Flush`/`ResponseEnd`/EOF 才退出。即「一个全 `0004` 的输入流会让 `read_command_request` 永不返回」，纯 CPU 空转、连接不释放，靠 `git_stream_timeout()` 收尾。**未实测**（无 runtime 验证 `0004` 能否被真实客户端发出、HTTP duplex 是否先超时）；NPC 建议单开一轮。修法方向：`0004` 在 capability 阶段计为畸形输入并 `bail!`，或设空行计数上限。

11. **防回归守卫的第三种旁路**（第 6 轮写入，并入 #8）：`test_no_raw_git_command_in_crates` 在 `cli_gateway.rs:386` 对**整个文件** `continue`，因此 `:272` 方法体里的 `tokio::process::Command::new("git")` 连白名单都没过（它是「允许位置」但不被校验）。加上 #8 的运行时程序名与别名（`use std::process::Command as C; C::new("git")`），共四种绕过形态。措辞建议：**字面量守卫只能防手滑，不能当安全边界**。另：`platform/mod.rs:13` 只 re-export 了 `execute_script`，`execute_command`（`process.rs:51`）连导出都没有、全仓零调用 —— 两者都零生产使用，**直接删比加守卫便宜**。

### 待拍板（batch#2，本流程不动 main）

> 编号独立于上一批的 #1–#11，避免跨批引用串号。全部为「改代码」类，故只记录不实施。

1. ✅ **已修（PR #21 `c74bc25`，2026-11-11）** ~~**B2-1 `reject_common` 对「前导非字母」的密码整段失效**~~（P1，实测复现）：`crates/rg-core/src/auth/password.rs:233-237` 的 `take_while` 只取前导字母段，`1Password1!` / `!Password1` / `0Admin123!` 全部 HTTP 201 注册成功，只有 `Password1!` 被拦。修法二选一：①取**最长**字母段再比对；②先剥离所有非字母字符再比对。影响面只在校验点（存量密码不受影响）：`crates/rg-core/src/user/service.rs:166`（注册）与 `:755`（重置）。**第 2 轮定稿建议：选 ①（最长字母段）**。依据（逐条复算，非沙盘估算）：方案 ② 在 `Welcome1!x` 上会**丢覆盖**——现状 `stripped="welcome"` 命中词表被拦，剥全部非字母得 `welcomex` 不命中 ⇒ 放行；方案 ① 在该组及 `1Password1!`/`!Password1` 上均为「只扩大、不缩小」。若仍选 ②，必须补 `Welcome1!x` 回归用例。另注：`Passw0rd!` 在现状/①/②下**全部放行**（`stripped="passw"`），l33t 归一化超出本项范围。**实际落地方案**：不是 ① 也不是 ②，而是「跳过前导非字母 + 取随后第一个字母段」（`password.rs:255-262`：`:257-261` 构造 `core`，`:262` 比对），等价于 ① 在「前导段为空」情形下的修正，旧覆盖一条不丢。回归用例 `:349` `test_password_common_word_not_bypassed_by_prefix_decoration`（含 `:363` 的 `Welcome1!x`）+ `:377` `test_password_common_word_check_leaves_interior_intact`（含 `:380` 的 `Str0ng!Pass`），两侧都钉住。
2. ✅ **已修（PR #21 `c74bc25`，2026-11-11）** ~~**B2-2 `TooCommon` 把明文密码回显进响应体**~~（P1，实测复现）：`password.rs:69-73` 的 `Display` 带 `pwd`，经 `user/service.rs:169`/`:758` 的 `e.to_string()` 与 `rg-http/src/error.rs:171`→`:114-120`（H-05 只脱敏 `InternalError`）出现在未鉴权 `POST /users/register` 的 400 响应体。最小修法：`TooCommon` 不再携带值（`Display` 改为固定文案），调用方零改动。**附带核查项**：其余把用户输入拼进 `Display` 的错误变体应一并清点。**落地情况**：`TooCommon` 已改为无载荷变体（`password.rs:57`），`Display` 走固定文案（`:82`），docstring（`:50-56`）明写「carries no payload……would echo the submitted password back to the caller」。附带核查项的答案：`PasswordError` 除 `TooCommon` 外只有 `TooShort`/`TooLong` 带 `min`/`max`/`got` 三个 `usize`（`:37-44`），其余变体全无载荷——即用户输入现在不进任何 `Display`。
3. **B2-3 TOTP 无重放保护**（P2，代码事实）：`totp.rs:37`/`:51` 是纯函数，`api/mfa.rs:327` 也不记录已用码；`challenge` cookie 5 min TTL（`jwt.rs:125`）远大于 30 s 步长，同一码可重复换 JWT。需「已用码」存储（带 TTL 的表或内存 LRU）⇒ 存储/schema 改动。
4. **B2-4 `check_repo_access` 是可见性盲的死函数**（P2→P3）：`oci_token.rs:204-217` 无令牌即 `return required_action == "pull"`，完全无视仓库公开性；全仓零生产调用（唯一「调用方」`rg-http/src/oci.rs:198` 把返回值 `let _ =` 丢弃）。建议删除，或改成接收 `can_read_repo` 回调的签名。本轮已把 `oci.rs:73` 那句「caller should use check_repo_access」的误导注释改掉。
5. **B2-5 `derive_key` 是裸 SHA-256 且与 JWT 共用 secret**（P3，事实）：`encryption.rs:12-19` 单轮 `Sha256(jwt_secret)`。换 HKDF/加盐会让 `TOTP secret`、`SSO refresh token`、`CI secret` 的全部既有密文失效 ⇒ 数据迁移级改动，不建议在本流程内做；若要做，先写重加密迁移（读旧键 → 写新键 → 双键过渡）。
6. **B2-6 `mask_values` 的 4 字节下限无兜底**（P3）：`encryption.rs:62` 对短值静默不遮蔽；生产唯一写入点 `api/ci_secrets.rs:94` 由 `:86` 强制 4..=65536，但 `ci_secret_ops::upsert` 自身无校验（`rg-ci/src/runner.rs:1522` 是 `#[cfg(test)]`，模块起于 `:1121`）。建议把长度校验下沉到 ops 层。**第 2 轮修正（重要）**：`mask_values` 的入参是**解密后的明文**（`api/runners.rs:796` `decrypt`→`:809` 收集→`:537` 传入；`rg-ci/src/runner.rs:754` `decrypt`→`:758` push→`:528`/`:535`），**密文从不进入该函数**。故「<4 字节入参 ⇒ 漏遮」是唯一后果：既不会「被意外遮住」（NPC 英文复核的说法），也不会「secret 在 CI 里消失」（中文复核的说法），两种替代描述均不成立。
7. **B2-7 SSO CSRF `expected` 原文入 warn 日志**（P3）：`rg-http/src/api/sso.rs:386-390`；另，失败路径 `:391` 直接 return、不清 state cookie（只有两条成功路径 `:521-522`、`:543-544` 清）。建议日志只留长度/前缀。
8. **B2-8 登录时序泄露账号存在性**（P2，未实测）：`user/service.rs:232` 对不存在的用户提前 return，跳过 `:246` 的 Argon2；同文件 `forgot_password` 已有 H-5 拉平（`:654`/`:662` 各 `sleep(100ms)`）。修法：对不存在用户也跑一次固定 dummy hash 的校验。（行号基准 `2ffe7e8`；PR #47 之后 `:238-241` 另有 agent 账户分支，等价返回 `INVALID_CREDENTIALS`）
9. **B2-9 `iat`/`nbf` 从不校验**（P3，无利用路径）：`jwt.rs:139` 用 `Validation::default()`；jsonwebtoken 的 `ClaimsForValidation` 无 `iat` 字段、`validate_nbf` 默认 `false`，但仓内无签发 `nbf` 的路径。只记录，不建议加自定义校验。
10. **B2-10 `extract_user` 的哨兵 0 与丢弃调用**（P3）：`rg-http/src/oci.rs:61-78` 在 OCI token 分支返回 `Some(0)`，唯一调用点 `:198` 丢弃返回值，`api_version_check` 恒 401 —— 要么删函数，要么让它真参与鉴权。
11. ✅ **已修（PR #24 `9b6bdde` / `43e3092`，2026-11-11）** ~~**B2-11 OCI scope 票据的校验侧不看仓库可见性**~~（第 3 轮写入，P2，与 B2-4 同根）：`crates/rg-http/src/oci.rs:129-144` 只要 token 里的 scope 与 `{owner}/{repo}` 字面相等且含动作就 `return Ok((true, None))`（`:139`），**跳过**函数里唯一的 `can_read_repo`（`:147-150` 只在无 token 分支）。`OciTokenClaims`（`oci_token.rs:42-70`）无 `repo_id`/`visibility`，`matches_repo`（`:113-115`）是字符串等值。签发侧当前是唯一且正确的（`oci.rs:343` 唯一生产调用点，按 `can_read_repo` 裁剪 `:297-307`），所以**现状后果是「仓库在票据有效期内（`ttl` = 300 s 认证 / 60 s 匿名，`:342`）转私有后旧 token 仍可拉」**，而不是任意拉取。修法与 B2-4 合并处理：校验侧补一次 `can_read_repo`，或把 `check_repo_access` 改成回调式签名。**实际落地**：走了第一条（校验侧补查），关键是给 `OciTokenClaims` 加 `uid: Option<i64>`（`oci_token.rs:74`）——`can_read_repo` 只吃 `Option<i64>`，没有 `uid` 时「补查」在数据上做不到，这也是本条拖了三轮的结构性原因。现 Bearer 分支为 `oci.rs:128-175`，`:159`（pull）/`:164`（push）各查一次，匿名单据 `:169` 直接判否，旧票据按匿名处理（`:141-144`）。运行证据：`oci_permission_tests.rs:135` 已由本流程实测通过（1 passed）。详见 `ironforge-docs/architecture/registry-auth-visibility-verification-2026-11.md`（已整篇重写为「已修复」版）。**B2-4 未修**：`check_repo_access` 仍在 `oci_token.rs:219`、仍零生产调用。
12. **B2-12 Runner 没有租户维度，且空标签双向全匹配**（第 3 轮写入，**P1，跨租户**）：`runners` 表无 `repo_id`（`crates/rg-db/src/entities/runner.rs:8-21`；建表迁移 `crates/rg-db/src/migrations/m20260510_000001_create_runners.rs` 同样没有），调度只看标签与 stage 状态（`crates/rg-db/src/ops/pipeline_ops.rs:909-911` Runner 无标签→领任意 Job、`:918-920` Job 无标签→给任意 Runner、`:932-953` 只判 stage/pipeline 状态），Runner 鉴权只校验「token 哈希命中某行且该行 id == 路径 id」（`crates/rg-http/src/api/runners.rs:1019-1026`）。因此任一被管理员批准注册的 Runner（`runners.rs:153-156` `require_admin`）会领到**别的仓库**的待调度 Job，并按 `pipeline.repo_id`（`:352-355`）拿到该仓库**全部** Secret 的明文。ADR-0001 §1 第 5 行原本只承认「外部 Runner 会接收（本仓）解密值」，跨仓这一层是 2026-11-10 新确认的，已补进 ADR §7 R-1/R-2。修法：给 `runners` 加仓库/组织归属并让调度按归属过滤 ⇒ schema 改动。
13. **B2-13 suggestion 触发让 fork 内容在共享 Runner 上跑，无需审批**（第 3 轮写入，P2）：`crates/rg-http/src/api/reviews.rs:161-168` 只要求对 PR 源仓库（fork）有写权限，`:200` 就在源仓库路径上查 `.gitea/workflows`，`:206-210` 以 `repo_id = source_repo.id`(`:207`)、`trigger_type = "suggestion"`(`:210`) 起 Pipeline，workflow 内容取自该 commit（`crates/rg-ci/src/lib.rs:599-608`）且无任何来源校验。ADR §2.7.4 的「fork PR 需维护者批准」只覆盖 base 仓库上的 PR 事件，对这条路径没有落点（ADR §7 R-3）。单独看只算「任意代码落到共享 Runner」，与 B2-12 叠加才是跨租户 Secret 暴露。
14. **B2-14 密码长度按字节计、文案按字符说**（第 3 轮写入，P3）：`crates/rg-core/src/auth/password.rs:193`/`:199` 用 `password.len()`（字节），而 `:53`/`:58` 的 `Display` 写 "characters"。后果双向：`密码密码`（4 CJK 字符 = 12 B）满足 `min_length = 12`；长 CJK 密码会被 `max_length` 误拒且报出「got 60」而用户只输了 20 字。修法二选一——判据改 `chars().count()`（改变校验语义）或文案改 bytes（改变对外文案）。本轮只落注释（`7ae3cf5`）。**第 5 轮补充**：PR #21 合入后行号重取为 `:206`/`:212`（`len()` 判定）、`:66`/`:71`（`Display` 模板）；`got` 那一半更麻烦——模板是 `"at least {} characters (got {})"`，`min`/`max` 标了 characters、`got` 是字节且**不标单位**，用户数 43 个 CJK 看到 `at most 128 characters (got 129)` 无从对账。注释已在 `:201-205` 写死这一半。

15. **B2-15 `registry:catalog:*` 是「签得出、用不上」，安全性靠三项巧合**（第 5 轮写入，P2）：`crates/rg-http/src/oci.rs:362-366` 的签发分支只判 `scope_type == "registry" && name == "catalog" && authenticated_user_id.is_some()`，**零仓库门**——任何登录用户换这个 scope 必成功；但 `matches_repo`（`crates/rg-core/src/auth/oci_token.rs:123-125`）硬编码 `scope_type == "repository"`，所以 `:152` 的校验对它恒为 false；同时 `build_v2_routes`（`crates/rg-http/src/lib.rs:520-555`）**根本没有 `_catalog` 端点**。三项任一变动（加 catalog 路由 / 放宽 `matches_repo` / 签发侧改成带仓库参数）就会变成「枚举全部私有仓库名」。放大项：`api_version_check`（`oci.rs:227-245`）在 `:240` **无条件**对匿名请求广告 `registry:catalog:*`，等于主动引导客户端申请这个用不上的 scope。建议：要么删掉 catalog 分支与广告串，要么补 `_catalog` 端点时同步加租户过滤。**改代码，属 L2**；本轮只把结论固化进 `ironforge-docs/architecture/registry-auth-visibility-verification-2026-11.md`。

**B2-11 表述勘误（第 4 轮写入）**：本条原写「跳过函数里**唯一**的 `can_read_repo`（`:147-150` 只在无 token 分支）」，不准确。`crates/rg-http/src/oci.rs` 的 `check_access`（`:86` 起）内共有两处 `can_read_repo`：`:114`（会话 JWT 的 pull，配 `:118` 的 `can_write_repo`）与 `:148`（匿名）。准确表述是「**Bearer 分支（`:128-144`）一处都不查**，会话 JWT 与匿名分支都查」。实质结论（Bearer 只做 scope 字面匹配、`matches_repo` 为字符串等值）不变。详细链路已迁到 `ironforge-docs/architecture/registry-auth-visibility-verification-2026-11.md`。

**并行修复观察（第 4 轮写入，⚠️ 已过期：两条分支均已合入 main 且无独有提交，勿再在其上继续工作——现状见下方「2026-11-11 更新（第 5 轮）」与「`fix/*` 分支全量状态」两段）**：`cnb` 上已有两个基于 `9f18fd7` 的分支在修本批的 L2 条目：

| 分支 | HEAD | 提交标题 | 覆盖条目 | 触及文件 |
|---|---|---|---|---|
| `fix/oci-token-visibility` | `43e3092` | `fix(oci): re-check repository visibility instead of trusting the token's scope` | **B2-11** | `rg-core/src/auth/oci_token.rs`（+94）、`rg-http/src/oci.rs`（+58）、`rg-http/tests/oci_permission_tests.rs`（+101） |
| `fix/password-strength-bypass` | `79c6b62` | `fix(auth): close the common-password bypass and stop echoing the password` | **B2-1 + B2-2** | `rg-core/src/auth/password.rs`（+117 段）、`rg-core/src/auth/ssh_key.rs`、`rg-http/src/oci.rs`（2 行） |

含义：①这两条一旦合入 main，`password.rs` / `oci.rs` / `ssh_key.rs` 的行号基准全部改变，凡引用这三个文件的行号必须重取；②`registry-auth-visibility-verification-2026-11.md` 的结论会被 `43e3092` 修掉，合并后应把该文状态从「未修复」改为「已修复」并补运行证据；③本流程仍**不动**这两个分支（L2 只记录，实现由分支侧跑）。

**2026-11-11 更新（第 5 轮）：两个分支都已合入，第 ① ② 两项的预案已执行。**

| 分支 | 合入提交 | 覆盖条目 | 状态 |
|---|---|---|---|
| `fix/password-strength-bypass` | PR #21 → `c74bc25` | **B2-1 + B2-2** | 已修。`reject_common` 改成「跳过前导非字母 + 取第一个字母段」（`password.rs:250-254`，即本轮推荐的方案 ①）；`TooCommon` 已改为无载荷变体 |
| `fix/oci-token-visibility` | PR #24 → `9b6bdde` | **B2-11** | 已修。`OciTokenClaims` 新增 `uid`（`oci_token.rs:74`），Bearer 分支在 scope 命中后补 `can_read_repo(.., claims.uid)`（`oci.rs:159`）/ `can_write_repo`（`:164`） |

**`fix/*` 分支全量状态（`2ce6d94` 时点快照，2026-11-11 单发轮 #13 实测 `git rev-list --count cnb/main..<ref>` 取 `uniq`、反向 `<ref>..cnb/main` 取 `behind`）**：`uniq=0`（无独有提交 = 内容已全在 main，**僵尸 ref，勿在其上继续工作**）= `fix/agent-approval-gate`（behind 114）、`fix/oci-token-visibility`（behind 89）、`fix/password-strength-bypass`（behind 89）；`uniq=1`（**存档性质**：有 1 个独有提交、删了就真丢代码，但同样**不可在其上继续开发**——落后几十个提交，要捞就 `git cherry-pick` 那一个提交到新分支）= `fix/login-timing-enumeration`（behind 65）、`fix/oci-drop-catalog-scope`（behind 35）、`fix/validate-optional-text-propagate`（behind 30）。**读数规则**：本行是 `2ce6d94` 快照，`main` 每前进一个提交 behind 全部 +1、`uniq` 不变（`a01f591` = 115 / 90 / 90 与 66 / 36 / 31，`6e3a205` = 116 / 91 / 91 与 67 / 37 / 32）⇒ 别拿当前 main 复算来对账。删 ref 属 L2：另有一个 L1 之外的理由——六条里五条在本机被 `ironforge-fix-*` 五个 worktree 占用（`git worktree list` 可查），删远端虽不影响本地 worktree，但下次 `git fetch cnb --prune` 会让它们的上游追踪断掉。本流程**只标注不删**。

连带的行号后果（**后续轮次必读**）：`password.rs` 全部行号 +6～+13（`min_length: 12` 从 `:177`→`:180`、长度判定从 `:199`→`:206`）；`oci.rs` +60 行（`check_access` 的 Bearer 分支从 `:128-144` 扩到 `:128-175`）；`oci_token.rs` +94 行（`matches_repo` 从 `:114`→`:124`、`check_repo_access` 从 `:219`）。凡旧轮次记录里这三个文件的行号，**一律作废，不得当证据引用**。

### 判定不成立（原文保留，请勿重复提）

> **编号勘误（2026-11-11 batch#3 第 2 轮）**：本条清单原为 22 条、但编号只到 `21`（`ofs-delta` 与 `--depth` 两条都标 `2.`，会让 `grep '^2\.'` 命中两条）。现已重排为 `1..22` **连续唯一**。旧记录里「判定不成立 第 N 条」当 `N ≥ 3` 时需 **+1** 才是当前编号；批次索引里写的「不成立 21 条全量回灌」是当时的计数口径，实际条数为 **22**。

1. 「V1 upload-pack 未收到 NUL 时会漏掉 connection 级 capability，但 flush 与 `# service=` 行上的 capability 不一致……能力协商靠「客户端好心多打一个空格」才成立，缺它则 clone/fetch 失败，而不是降级。」—— 反证：`upload_pack.rs:205-224` 的 Form A/Form B 分支覆盖了协议允许的两种形态；git 的 capability 只出现在第一个 `want` 行，客户端要么声明 `side-band-64k`（走 sideband），要么不声明（服务端裸发 packfile 属合规降级）。`scripts/git-protocol-e2e.sh:172-190` 的 `protocol.version=1` HTTP push/clone/fetch 用例长期为绿，是该路径可用的直接证据。**以下已核实为不成立，不要重复提。**

2. 「V1 + `--depth` 会真实发送 `deepen 2`，客户端把全量 pack 当浅克隆收下」—— 反证：本服务 V1 广告不含 `shallow`，客户端在发任何 `want` 之前即 `fatal: Server does not support shallow clients`（本地实例实测，trace 无 `want`/`deepen`）；V2 对照 `--depth=1` 正常。NPC 的 trace 含 `upload-pack> shallow <oid>`，该行本服务从不产生，故其服务端不是 IronForge。

3. 「`ofs-delta` 标 ✅ 站不住，`iter_from_counts` 这条路径根本产不出 OFS_DELTA」—— 反证：gix-pack 0.77 `src/data/output/entry/mod.rs:185` 把 `DeltaRef { object_index }` 直接映射为 `data::entry::Header::OfsDelta`（`:84` 另有 `OfsDelta { base_distance }` 写出分支）；`allow_thin_pack` 的文档（`iter_from_counts.rs:382-391`）只约束「base 不在本 pack 里」的 delta。`allow_thin_pack: false` 不等于「不产 OFS_DELTA」。

4. 「`sideband.rs:25` 应为 `:22`」「`pkt_line.rs:59` 应为 `:61-62`」—— **NPC 已于第 4 轮撤回**。反证：`sideband.rs:25` 正是 `writer.flush().await?;`（22 是 `payload.extend_from_slice(chunk)`）；`pkt_line.rs:59` 是 `write_pkt_line` 的函数定义，引用它正是为了说明该函数不调用 flush（61-62 是长度检查，与命题无关）。

以下 5 条由 **batch#2 第 1 轮 NPC 自行判定不成立**，消费者逐条复核后确认其判定正确，一并锁死：

5. 「`validate_mfa_challenge` 用 `Validation::default()` ⇒ 有 alg 混淆余量 / 缺 iss/aud 即缺陷」—— **不成立**。jsonwebtoken 9.3.1（`Cargo.lock:3512-3513`）的 `~/.cargo/registry/.../jsonwebtoken-9.3.1/src/validation.rs:164-167` 把 `Default` 钉成 `Algorithm::HS256`、`:115` 强制 `exp` 存在、`:120` `leeway: 60`、`:124` `validate_exp: true`；且 `jwt.rs:47-49` 的域分离密钥已阻断与普通会话串用。
6. 「`pat_scope` 的 `*` 能跨 `/` 是缺陷」—— **不成立**。`pat_scope.rs:559-567` 的用例把 `refs/heads/agent/x` 与 `refs/heads/agent/a/b` 一并断言为允许，`agent/*` 覆盖子路径是刻意语义。
7. 「`has_scope` 层级漏了 `repo:collab → repo:write`」—— **不成立**。文档表 `pat_scope.rs:10-15` 写明 collab 是「the above, plus write the collaboration area」，不含 `repo:write`。
8. 「`ldap.rs` 的 RFC 4515 转义不全」—— **不成立**。`ldap.rs:32-48` 覆盖 `\0 ( ) * \` 全部五个必备字符与所有非 ASCII（转 `\xx`）。
9. 「SSO CSRF state 用弱随机」—— **不成立**。`rg-core/src/auth/sso.rs:87-91` 是 `rand::thread_rng()`（ChaCha12 CSPRNG）+ `Alphanumeric` ×32 ≈ 190 bit，且 `rg-http/src/api/sso.rs:381-392` 有签名 cookie 双向比对。

以下 5 条由 **batch#2 第 2 轮**判定不成立（两份 NPC 复核各占，均经复算否证），一并锁死：

10. 「`oci.rs:73` 的 "it has no callers" **holds**（成立）」—— **不成立**。`grep -rn check_repo_access crates/` 有 6 处 `assert!` 调用（`oci_token.rs:321 :329 :353 :360 :367 :375`）+ 2 处测试函数名（`:318` `:339`）。准确说法是 "no **production** callers"，本轮已按此改写。
11. 「`password.rs` 词表里 8 条含数字/`-` 的条目（`abc123`/`trustno1`/`password1`/`password123`/`ec2-user`/`admin123`/`root123`/`test123`）**永远匹配不上**」—— **不成立**。`abc123`、`trustno1`、`ec2-user` 原样输入会被 `:241` 的整串精确匹配拦下（逐条复算）。正确表述：这 8 条在 `:237` 前导段分支下不可达，只能靠 `:241` 命中。
12. 「`ssh_key.rs` 注释里的 `...TKA_7j9...` 吃掉了 `q`，是不可比对的假串」—— **不成立**。`'TKA_7j9' in url` 为 `True`；该片段两端是省略号，本就是子串。
13. 「`ssh_key.rs:143` 的 docstring 只提 `/`，`+` 出现在 body 的 index 21」—— **不成立（双向错）**。当时文本为 `:142-143`「contains both `/` and `+`」；且 `std[21]` 是 `m`，全串**无** `+`。附带结论（我方错，已修）：见下「我方已自证推翻」。
14. 「`mask_values` 的 `str::replace` 会把同一串的 base64url 变形也遮掉」—— **不成立**。`str::replace` 是字面量子串替换，遮 `TKA/7j9` 不影响 `TKA_7j9`。
15. 「B2-6 的后果不是漏遮，而是 secret 在 CI 里消失」／「密文 19+ 字符 ⇒ 2 字节 secret 会被意外遮住」—— **两条均不成立**。`mask_values` 只吃**解密后明文**（`api/runners.rs:796`→`:537`、`rg-ci/src/runner.rs:754`→`:528`），密文从不进入；短明文唯一后果是不被遮蔽。
16. 「`password.rs:192` 把边界写成「4 个 CJK 字符」是错的，应该是「3 个」」—— **不成立**。`strict()` 是 `min_length: 12`（`password.rs:171`），判定在 `:193` `if password.len() < self.min_length`：3 个 CJK = **9 字节** → `9 < 12` 成立 → `TooShort`（过不了）；4 个 CJK = 12 字节 → 条件为假 → 通过。对方自己的句子也承认「3-char CJK fails at the 9-vs-12 boundary」，与其改法自相矛盾。正确写法是「4 个、只是刚好过线（no margin）」。**已核实为不成立，不要重复提。**

17. 「`registry-auth-visibility-verification-2026-11.md` 的 `:23` **漏了签发侧 `:298`/`:311` 两个调用点**」—— **不成立**（该文的计数口径问题成立，见第 5 轮 #4）。同一篇的 `:27` 已单列签发侧：`:297` `has_action("pull")` → `:298-304` `can_read_repo`、`:309-321` push 走 `can_write_repo`，与 `:23` 的「这条链」互为补充。所谓「漏」是把「`:23` 只讲校验侧」读成了「全文只讲两处」。第 5 轮已重写该文，把「校验侧 3 处 / 签发侧 2 处 / 供 5 处」写死，此后不该再出现计数争议。**第 6 轮更正**：那次「3 + 2 = 5」本身就是错的（漏了 Bearer 复查 `:159`/`:164`），真值是「校验侧 5 + 签发侧 2 = 7」，见下「我方已自证推翻」表。

以下 5 条由 **batch#2 第 6 轮**判定不成立（逐条 `sed` 复核），一并锁死：

18. 「`crates/rg-http/src/lib.rs:520-555` 这个区间**不是** `build_v2_routes`；`:521-533` 是 `upload_body_limit` 与 `upload_routes`，`:555` 已经是空行/下一个函数之前，应为 `:520-554`」—— **不成立**。`sed -n '520p'` = `fn build_v2_routes(state: &AppState) -> Router<AppState> {`，`sed -n '555p'` = `}`（该函数的闭括号），`:556` 才是空行。引用整个函数（含首尾括号）不是错误；它连「`:555` 是空行」都错一行。
19. 「registry 文档 `:141-144` / `:146-148` 被写成包含在 `:157-172` 内，应把 `:157-172` 改成 `:149-174`」—— **不成立**。两句分处不同小节（`:13` 复查点、`:29` 为何需要 `uid`），都是全路径 `crates/rg-http/src/oci.rs:...`，不存在包含关系；`:157-172` 打出 `let allowed = match required_action {` … `};`，正是「再查一次可见性」的判定块。
20. 「`registry:catalog:*` 的第 3 项『校验侧永不匹配』论据是空的，因为 `repository:alice/private-img:pull` 这张票照样能查 `alice/private-img`」—— **不成立**。`matches_repo`（`oci_token.rs:123-125`）硬编码 `scope_type == "repository"`，**catalog 类型**的票对任何 `{owner}/{repo}` 恒为 false；它举的是另一种票，证不了 item 3 为空。其真实关切（别让读者低估枚举风险）已并入 item 3 的一句限定。
21. 「README `:31` 说被删的那行是『指向 `docs/` 的死链』属于指错表名——它既没指 `docs/`，也不构成死链，只是重复条目」—— **不成立**。被删行在 `a94839b:ironforge-docs/README.md` 的 `:30`，所在表标题是 `## 设计文档（\`docs/\`）`（`:21`），表内裸文件名按标题前缀解析为 `docs/project-audit-2026-06.md`，该路径不存在（`ls docs/*.md | grep -c project-audit` = 0）——「死链」与「重复」两种性质同时成立，已把两者都写进该行。
22. 「README `:52` 是全表唯一一个在描述里双写状态的条目」—— **不成立**。状态只出现在末尾括注（`（**已修复**，PR #24）`），与其余行同构；「修复前后对照」「三项巧合」描述的是文章内容（本文确有对应两节），不是状态。

以下 2 条由 **batch#3 第 2 轮**判定不成立（逐条 `sed` 复核），一并锁死：

23. 「`perm_cache_disabled()`（`repo/service.rs:75-83`）在 `check_perm_cache` 入口短路（`:97-99`），**测试环境不付这个锁** —— 这是为什么 13/14 处调用一直没在 CI 里炸」—— **不成立**。`invalidate_perm_cache_user`（`repo/service.rs:120-124`）**没有** `perm_cache_disabled()` 守卫：`:121` 无条件 `perm_cache().write().unwrap_or_else(...)`。短路只发生在读侧 `check_perm_cache`（`:96-99`）与写侧 `set_perm_cache`（`:107-110`）。⇒ 缓存禁用与否，每处 invalidate 调用都真的拿一次全局写锁；「测试环境不付」是假的安全感（见 F3-2）。**已核实为不成立，不要重复提。**

24. 「`access.rs:17` 的『at the price of a global perm-cache write lock **per call**』对 `require_collab_write`（`api/collab.rs:211`）不成立，因为那条路径走 `can_write_repo`、不经过 `is_collab_member`，所以注释应当限定主语」—— **不成立**。`access.rs:15-17` 是本文件 `is_collab_member`（`:18`）的 doc，主语就是本函数本身，从未覆盖 `require_collab_write`；后者在 `api/collab.rs:202`，`:211` 自己先清一次、`:212` 直接 `can_write_repo`，不进入 `access.rs:23`。它陈述的事实（write 路径不经过 `is_collab_member`）成立，但由它推出的「注释需要限定主语」无从谈起 —— 句子本来就没越界。**已核实为不成立，不要重复提。**

以下 1 条由 **batch#3 第 3 轮**判定不成立（子命题，逐条 `sed` 复核），一并锁死：

25. 「`escalation.rs:283` 的 `contains_key` 记忆化，实质是「用 session-A 的权限判定放行 session-B 的提醒」，等于跨 session 越权」—— **不成立**。被缓存的判据是 `is_collab_member(db, repo, session.user_id)`（`escalation.rs:284`），即 **user 对 repo** 的权限，与 session 无关；`may_notify` 在取缓存之前已单独校验 session 存在与 `state_of` ∈ {online, offline}（`:277-282`）。同一 user 的两个 session 权限不可能不同 ⇒ 不存在「放行」。同轮它附带的「修掉它会变成 N 次写锁」是对假想方案的推断，不作为缺陷判定；真值是「每 `evaluate` 的锁数 = 候选里不同 user 数，候选上界 600（`gather` 三处 `CANDIDATE_LIMIT`）」。**已核实为不成立，不要重复提。**

以下 2 条由 **batch#3 第 4 轮**判定不成立（逐条 `sed`/`grep` 复核），一并锁死：

26. 「`repo/service.rs:64-65` 的 `no stale entry can exist` 无条件写死，场景是「实例先以 flag 未设启动并预热，之后才设 `IRONFORGE_DISABLE_PERM_CACHE=1`」⇒ 旧允许项永久驻留、谁碰不到它，应改为 `nothing reads it`」—— **不成立**。`perm_cache_disabled()`（`:75-83`）是 `static DISABLED: OnceLock<bool>`，而**每条写入路径都先过它**：`set_perm_cache` 的 `:108` 守卫在 `:113 cache.insert` 之前 ⇒ flag 在第一次插入前即冻结；`grep -rn set_var crates/` 无此 key（只有测试改 `HOME`/`IRONFORGE_ALLOW_LOCAL_OUTBOUND`）⇒ 运行期翻不了，改 env 必须重启进程，新进程 map 为空 ⇒ 该状态不存在。另它的「没有任何路径会清它」也假：`invalidate_perm_cache_user`（`:120-124`）无守卫且每个协作请求走两次（`api/collab.rs:184`、`access.rs:23`）。且 `nothing reads it` 会把 `:63` 已写明的 `nothing populates` 抹掉，比现状更弱。**已核实为不成立，不要重复提。**

27. 「`access.rs:17` 的 `per call` 不成立，因为 owner 早退（`:24-26`）与 org 成员早退（`:34-37`）时一次锁都不付」—— **不成立**。`:23 invalidate_perm_cache_user` 是函数体**第一条语句**，在 `:24` 的 owner 判断**之前** ⇒ 所有早退路径照付一次全局写锁；`per call` 字面准确（本函数 1 次，`require_collab_read` 因 `api/collab.rs:184` 的前置清理共 2 次）。它与已撤回的第 24 条是同一条注释上的第二次尝试（第一次改主语、这次改计数），两次都被否。**已核实为不成立，不要重复提。**

以下 5 条由 **batch#3 第 5 轮**判定不成立（逐条 `sed`/`grep`/`sqlite3` 复核），一并锁死：

28. 「`scripts/check.sh --quick` **不含** `cargo fmt --check`，所以本轮『改后 EXIT=0』不构成对 `webhook/service.rs:29` 那 289 列的验证」—— **不成立**。`sed -n '36,44p' scripts/check.sh` = `step "cargo fmt --all --check"`（`:36`）/ `cargo fmt --all --check`（`:37`）/ `elapsed`（`:38`）/ … / `if [ "$mode" != "quick" ]; then`（`:44`），位于 `case "$1"` 之后、quick 分支**之前** ⇒ 三种模式都跑 fmt；`--quick` 省的只是 `cargo test -p rg-http -p rg-core`（`:45-46`）。289 列未被折行的原因是 **rustfmt 默认不折注释**（`wrap_comments` 默认 `false`；本仓无 `rustfmt.toml`、`Cargo.toml` 与 `.cargo/` 也无 `[rustfmt]`，`find . -maxdepth 3 -name '*rustfmt*'` 为空），与脚本覆盖无关。**已核实为不成立，不要重复提。**

29. 「`webhook/service.rs:29` **改前 214 列**（`awk 'NR==29{print length($0)}'` 得 289，故本轮 +75）」—— **不成立**。该行四个版本的自测长度：`0f6c54a`=**102** → `dc3fb9e`=**202** → `ebf14f9`=**242** → `8e4188d`=**289**（`git show <sha>:crates/rg-core/src/webhook/service.rs | sed -n '29p' | wc -c` 减 1）。本轮改前真值 **242**，增量 **+47**；`214` 不是该行任何版本的长度。**已核实为不成立，不要重复提。**

30. 「`valid_event_name` 的字符集 `[a-z0-9._-]` **容得下 `*`** ⇒ 拿它当 webhook 白名单等于照旧放行 `*`（就是现在静默死亡的那个值），所以不能复用」—— **不成立**。`api/repo_events.rs:60-62` 的判定是 `b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')`，`*` = 0x2A 三支全不满足 ⇒ `valid_event_name("*")` = **false**，复用字符集校验**会拒收** `*`，效果与它所推的相反。它的其余四点（私有 `fn`、无单一数据源、`MAX_TYPES` 是 `types` 查询参数的量、`collab_question` 是假阳性）成立，已并入 B3-8 / F3-10。**这一子命题已核实为不成立，不要重复提。**（第 6 轮补：同一字符集**也会拒收 `Push`** —— `api/repo_events.rs:239` 有 `assert!(!valid_event_name("Push"))`；所以「复用它」同时会拒掉今天能工作的 `Push`，这是它应拒 `*` 之外的另一条代价，已写进 B3-8，不要当成新发现再提。）

31. 「`require_collab_read` 每个请求清 **2** 次这个坑，你台账 B3-9 与上轮都没写，补上（`sed -n '218,226p' crates/rg-http/src/api/collab.rs`）」—— **结论对，两个附带指控不成立**。台账 B3-9 **原文已写**「其中 `require_collab_read` 每个请求**必然清两次**（`api/collab.rs:184` 先清，`:185` → `access.rs:23` 再清）」；且它给的区间错：`:218` 是 `require_collab_write` 结束后的空行，真值区间是 `:202-217`，`invalidate_perm_cache_user` 在 **`:211`**、`:212` 直调 `can_write_repo`（`sed -n '202,217p'` 可验）。**「台账没写」这一条已核实为不成立，不要重复提。**

32. 「**4 个订阅名永远等不到对应 `repo_events` 行**——钩子会被投递，但 `/events` 长轮询与 `types` 过滤永远静默」—— **不成立，且与它自己贴的证据相反**。它引的 `sed -n '138,142p' crates/rg-core/src/webhook/service.rs` 第一行就是 `:140 record_repo_event(db, repo_id, event, payload).await;`，在 `trigger_event` 开头**无条件**执行（`record_repo_event` 自身只按 64 字符截断事件名、按 `MAX_EVENT_PAYLOAD_BYTES` 截断 payload，见 `:227-239`），没有任何按事件名跳过的分支；「4 个」的名单从头到尾未给出，无法核实。此条真正成立的部分（`collab.*` 进公开事件日志的 ACL 不一致）已单列为 **B3-12**。**「4 个订阅名永远等不到行」已核实为不成立，不要重复提。**

以下 1 条由 **batch#3 第 6 轮**判定不成立（子命题，逐条复核），一并锁死：

33. 「台账 `consume 5 / 6` 的 N-2 写『5 条论据 / 4 成立 / 1 不成立』，但**只列了 4 条**（1/3/4/5）」—— **不成立**。N-2 的 5 条是 ①私有 `fn` ②字符集容得下 `*` ③无单一数据源 ④`MAX_TYPES` 是 `types` 参数的量 ⑤`collab_question` 是假阳性；台账把 ①③④⑤ 列在「成立」、②就是同一格紧随其后的「**不成立**：它说「字符集 `[a-z0-9._-]` 容得下 `*`…」」—— 4 + 1 = 5，**没有漏**。真正的问题是 ② 未编号（可读性），不是缺条。**已核实为不成立，不要重复提。**

以下 1 条由 **batch#4 第 1 轮**判定不成立（子命题，逐条 `sed` 复核），一并锁死：

34. 「`request_reviewer`（`reviews.rs:1062-1072`）用 `find_by_username` **能定位到软删用户**，所以 `list_requested_reviewers` 的 `continue` 与它构成口径不一致」—— **不成立**。`:1074-1078` 的 matcher 是 `Ok(Some(user)) if user.is_active && user.deleted_at.is_none() => user` / `Ok(_) => return AppError::not_found("reviewer not found")` ⇒ 软删或停用的 reviewer 在**请求侧**同样定位不到，直接 404；两边对「已删用户」的可见性一致。真实的不一致只剩「列表静默省略 vs 请求显式 404」这一层（已并入 B4-7），不是「一边能定位一边不能」。另它给的区间 `:1062-1072` 不含 `find_by_username`（真值 `:1074`）。**这一子命题已核实为不成立，不要重复提。**

以下 1 条由 **batch#4 第 2 轮**判定不成立（逐条 `sed` 复核），一并锁死：

35. 「台账 `:152` 的 ④-3 行把『`perm_cache_disabled()` 在 `check_perm_cache:97-99` 短路 ⇒ 测试环境不付这个锁』的出处标成『判定不成立』第 **23** 条，但第 23 条讲的是跨 session 数据混用，真值应是第 **31** 条」—— **不成立**。它报的 `:152` 是 ④-2，④-3 紧随其后；④-3 的命题与反向清单第 23 条逐字同源（同为 `perm_cache_disabled()` 在 `check_perm_cache` 入口短路、「13/14 处调用没在 CI 里炸」）；它说的「跨 session」是第 **25** 条（`escalation.rs:283` `contains_key` 记忆化），第 **31** 条讲 `require_collab_read` 每请求清 2 次与台账 B3-9，与本行无关。**已核实为不成立，不要重复提。**（它附带命中的真事实——清单第 24 条与下一段 preamble 之间的三连空行——已单独修掉，见 `consume 2 / 6` 表 D-1 附）

### 我方已自证推翻（勿当结论引用）

| 我上轮写的 | 实际 | 处置 |
|---|---|---|
| `ssh_key.rs` 注释「the digest contains **both** `/` and `+`」 | 复算 `std = uh6xDCBClTKA/7j9qTAuVmcAbusr4NwgLrWjafhfMM4`：`+` **不存在**，`/` 在 index 12 | 第 2 轮已改为「contains `/` at index 12 and no `+`」（`210f338`） |
| `ssh_key.rs:9`「byte-identical to `ssh-keygen -l -E sha256`」 | `ssh-keygen` 输出整行，本函数只回 `SHA256:<fp>` 字段 | 已改为「matches the `SHA256:<digest>` field」 |
| `encryption.rs:57` 未说明 `mask_values` 的入参形态 | 它是**明文**，且 `ci_secret_ops::upsert` 自身不校验长度 | 已在同一条注释里写明两点 |
| registry 文档「`oci.rs` 全文件共 **5 处**权限查询——校验侧 3 处 / 签发侧 2 处」（第 5 轮写死） | `grep -n 'can_read_repo\|can_write_repo' crates/rg-http/src/oci.rs` = 11 命中，其中 **7 处是调用**：`:114` `:118` `:159` `:164` `:179` `:329` `:342`（另 4 处是注释 `:82`/`:142`/`:146`/`:375`）。第 5 轮漏了 PR #24 新增的 Bearer 复查 `:159`/`:164` | 第 6 轮改正为「7 处 = 校验侧 5 + 签发侧 2」，并列出不计入的 `:152`/`:328`/`:340`（`335fa57`） |
| `webhook/service.rs:29` 注释「13 个事件名无一含 `*`」、台账 F3-1「13 个事件全名」（第 1 轮写） | 去重命令 `grep -rhn 'trigger_event(db, repo_id, "' \| sort -u` = **14** 个 repo 投递名；加 2 个 collab 常量 = **16**（F3-7），无一含 `*` | 第 4 轮改为「16 = 14 repo + 2 collab」（`8e4188d`）；计数一律用去重命令产出，不肉眼数枚举 |
| 台账 F3-1「API 对 event 名零校验」（第 1 轮写） | `api/webhooks.rs:83-85` 拦空数组；`api/repo_events.rs:57-63` 已有 `valid_event_name`（字符集 + 长度），只是没用到 webhook 创建 | 第 4 轮在 B3-8 收窄为「webhook API 只有非空校验，字符集校验器已存在可复用」（F3-8） |
| `webhook/service.rs:29` 的注释在四轮内 102→202→242→289 列（第 4 轮把内部计数写进去） | 列宽本身不是错（`rustfmt` **默认**不折注释 —— `wrap_comments` 默认 `false`，本仓无 `rustfmt.toml`；`scripts/check.sh` 的 fmt 关永远绿），但**内部计数是易腐事实**：本批内该数就被改错过两次（13→15→16），且公开 API 字段注释的读者要的是「我该填什么」，不是「服务端有几个值」 | 第 5 轮改为 **187 列**、只留可操作信息（全名带点 / 匹配是 LIKE 子串 / 裸 `issues` 与 `*` 都不命中）；1 行→1 行，文件仍 561 行。计数留台账 F3-7 |
| F3-9「**落库的订阅名**里的 `_`/`%` 是 SQL 通配符」、B3-11 ②「**订阅名**里的 `_`/`%` 被当通配符」、B3-11 ①「模式串是订阅名」（第 5 轮写） | `events LIKE '%<event>%'` 的**左值是订阅串、模式串是投递名** ⇒ 通配符只在**投递名**一侧生效：`select '%' like '%push%'` = **0**（订阅 `%` 是永不命中，不是「匹配全部 16 个值」）、`select '_' like '%push%'` = 0、`select 'pull.request.merged' like '%pull_request.merged%'` = 1（投递名的 `_` 才是通配） | 第 6 轮改写 F3-9 / B3-11 ② + 新增 F3-13，并把方向写进 `webhook/service.rs:29`（`ada6eee`） |

### 对方已撤回（勿再作为争议点）

第 5 轮 NPC 在 §7 自行撤回三项，均已由其本机实测/源码证明，此后不再讨论：

| 被撤回项 | 撤回依据（它自己的话） |
|---|---|
| 「V1 + `--depth` 会发 `deepen`，全量包被当浅克隆收下」 | 它 `cargo build --release` 后实测三组对照：V1+depth exit 128 `fatal: Server does not support shallow clients`、V1 无 depth ✅、V2+depth ✅ 且 `.git/shallow` 写入。补测 `fetch --deepen=2` 同样失败。原话「你第 1 条复核请求我给不了反证」 |
| 「`ofs-delta` 标 ✅ 站不住」 | 「撤回，`mod.rs:185` 行号精确」 |
| 「`sideband.rs:25` / `pkt_line.rs:59` 引错」 | 「撤回」（第 4 轮已撤，第 5 轮重申） |
| 「`skip_until_flush` 在 EOF 时空转」 | 第 6 轮它自己撤回：「`skip_until_flush` 只认字面 `Flush`，而 `read_pkt_line` 在 EOF 时返回的正是 `Flush`，所以它会正常退出。洞在 `read_command_request` 对 `0004` 空行的处理上」 |
| 反向清单 **23**（`perm_cache_disabled()` 短路保护了 13 处调用） | 第 3 轮（batch#3）自行撤回：「我撤回，你判对，且我漏了一处守卫位置」，并复算 `grep -rn perm_cache_disabled` = `:75` 定义 / `:97` / `:108`，**无第四处** |
| 反向清单 **24**（`access.rs:17` 的 `per call` 要限定主语） | 第 3 轮（batch#3）自行撤回：「我也撤回」。但其撤回理由里混入一条错的（说括注把 write 付零写成事实），见「部分成立里的错处」 |

### 第 6 轮记下的对方错项（部分成立里的错处，勿当结论引用）

| 它说的 | 实际 |
|---|---|
| `pack_universe` 有 3 个测试调用点 | 只有 `ops.rs:1600`；`upload_pack.rs:424` 是测试 doc 注释、`ops.rs:1710` 是 filter 测试 |
| 守卫在 `:379` 整文件 skip | skip 判定在 `:386`（`:379` 是 `filter_map`） |
| `spawn_async` 方法体裸 git 在 `:270` | 在 `:272` |
| `cli_gateway` 注释三处在 `:258` / `:259-260` / `:261` | 在 `:257` / `:258-259` / `:260` |
| ack 扫描 `gix::open().ok()` 在 `v2.rs:806` | 在 `:805`（`:806` 是 `let mut acked = Vec::new();`） |

---

## 冷启动验证（协议 §8 第 4 步的三个门槛）

| 门槛 | 状态 | 证据 |
|---|---|---|
| NPC 真的回了 | 通过 | 首扫 `09:25:24Z`，`author.is_npc=true`，自述已核对基线 `5624605`；延迟 **3 min 43 s** |
| 轮次标记可数（进度可重建） | 通过 | `cnb issues list-issue-comments … \| grep -oE '## 第 [0-9]+ 轮收尾' \| wc -l` = 1，下一轮从 2 续跑 |
| 回帖的 `@CodeBuddy` 触发下一轮 | **通过** | 回帖 `09:31:11Z` → 复核 `09:35:33Z`，**4 min 22 s**，`author.is_npc=true` |

三项全过。是否挂 automation 仍是独立决定（见下「未决」）——先人工跑满 6 轮或至少再跑 2 轮，确认修订率下降后再挂。

### 定时任务（2026-11-10 起）

| 角色 | 名称 | 频率 | 关键约束 |
|---|---|---|---|
| 消费者 | OPT-LOOP 消费者（IronForge，每小时） | 每小时 | 抢锁 → 从评论数重建进度 → 核实 → 分级 → `check.sh` → 推 cnb → 五段回帖 + 结尾 `@CodeBuddy` |
| 生产者 | OPT-LOOP 生产者（IronForge，每 2 小时） | 每 2 小时 | 背压闸门（有 open `[opt-round]` 就跳过）→ 选靶 ≤10 文件 → 开 issue → 只提交 ledger |

两条都不推 `origin` / `github`，不 force push，不 `git add -A`。

### 仍未解决

1. **CNB main 无分支保护**：F15（评审者越权直推）在本仓拦不住。协议 v0.3 也只给「发现 + 保留」的处置，没有恢复独立性的机制。
2. **NPC 流水线是否吃 CNB Runner 额度未验证**：CLAUDE.md:26 记录 Runner 额度已耗尽、CI 已迁 git233。若共享额度，定时任务跑到某天会集体失败。
3. **批次上限 6 轮已跑满**（本批 6/6，issue 置 `closed` + `BATCH_COMPLETE`）。**收敛判定**：边际收益确实不按轮次线性衰减 —— 第 5 轮（它自装工具链做实测）与第 6 轮（它自己声明沙箱无工具链、只到注释层，但准确指出我方 6 处语义错：`:308` 主语、`:259-260` 方向、`:260` 时态、`:302` 证据等级、`:306` 因果、§8 线格式）都产出了真事实错误。**关键变量是「它肯不肯构建 / 肯不肯自证」**，不是轮次。下一批建议：仍设 6 轮，但把「必须给出自证形式（`sed`/`grep` 输出或实测）」写进触发语。
4. **NPC 自举环境会推高延迟到 28 min**（第 5 轮，n=5 破 p100 20 min 46 s）。若继续挂每小时任务，等待窗口应按 ≥30 min 预留，否则那一轮会被误判「等待超时」。

---

## 反向清单索引（编号总数与去处；清单实体不在本节）

清单实体在上文 `## 批次索引` 块内的 `### 判定不成立` 小节，本节只维护编号总数与去处（本清单已累计 **1..106** 条；36–38 见 `consume 6 / 6（batch#4）` 节，39–40 见 `consume 1 / 6（batch#5）` 节，41 见 `consume 2 / 6（batch#5）` 节，42–43 见 `consume 3 / 6（batch#5）` 节，44 见 `consume 4 / 6（batch#5）` 节，45–46 见 `consume 5 / 6（batch#5）` 节（46 由「单发轮 #13 三次消费」轮写入、物理落在该节末，勿去那一节找正文），47–49 见 `单发轮 · issue #13 · 2026-11-11（第 4 次消费）` 节，50–51 见 `单发轮 · issue #13 · 2026-11-11（第 6 次消费）` 节，52–55 见 `单发轮 · issue #14 · 2026-11-12` 节（第 7 次消费），56–58 见 `单发轮 · issue #14 · 2026-11-12（第 8 次消费…）` 节，59–60 见 `…（第 9 次消费…）` 节（原写「见本节」是从索引行自身出发的错指针，已就地订正），61 见 `单发轮 · issue #33 · 2026-11-12` 节，62–63 见 `单发轮 · issue #15 · 2026-11-12` 节，64–65 见 `单发轮 · issue #16 · 2026-11-12` 节，66–68 见 `单发轮 · issue #17 · 2026-11-12` 节，69–70 见 `单发轮 · issue #18 · 2026-11-12` 节，71 见 `单发轮 · issue #19 · 2026-11-12（第 1 次进入…）` 节，72–73 见 `单发轮 · issue #34 · 2026-11-12` 节，74 见 `单发轮 · issue #35 · 2026-11-12` 节，75–78 见 `单发轮 · issue #36 · 2026-11-12（第 1 次进入…）` 节，79–83 见 `单发轮 · issue #37 · 2026-11-12（第 1 次进入…）` 节，84–89 见 `单发轮 · issue #48 · 2026-11-13（第 1 次进入…）` 节，90–92 见 `…（第 2 次进入…）` 节，93–94 见 `…（第 3 次进入…）` 节，95–98 见 `单发轮 · issue #38 · 2026-11-13（第 1 次进入…）` 节，99–102 见 `单发轮 · issue #14 · 2026-11-13（第 10 次进入…）` 节，103 见 `…（第 11 次进入…）` 节，104–106 见 `单发轮 · issue #15 · 2026-11-13（第 2 次进入…）` 节）。本行原为开批时的占位「（暂无）」，已失效并就地订正，勿再当「无任何不成立条目」读

---

### consume 4 / 6（batch#4）· 追加于文末（只为零行号漂移，勿移入上文）

- 输入：NPC 复核 `2120394655245176832`（`2026-11-11T05:54:15Z`，首行 `@cnb.biewen(别问) ## 第 3 轮 · 复核结果`）——晚于我方第 3 轮收尾（`05:52:52Z`）⇒ **有新复核**，K=4。C=3 由正则 `^## 第 \d+ 轮收尾` 数得（同一 K 只算 1）
- 形态：**零新靶轮**——它自述「L2 无新增（本轮 0 项代码级发现）」，4 个小节里 2 个是对我方上轮改动的确认 ⇒ 按 F20 只落台账，不硬找靶
- 基线：`BASE=807cfc3`；`git fetch cnb` 后 `HEAD` 与 `cnb/main` 同为 `807cfc3`（零漂移）
- 改前验证：`scripts/check.sh --quick` **2 s `EXIT=0`**（executor 本机），关键行原文 `==> cargo clippy --workspace --all-targets -D warnings` / `Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 0.74s` / `==> checks passed in 2s (quick)`；**时刻锚** = 本节提交 `43caa97` 的 author date `2026-11-11T15:00:10+08:00`（=`07:00:10Z`，`git log -1 --format=%aI 43caa97` 可复算），两条验证均发生在其之前
- 回填它第 ④ 节的提问：`fce70d1` 的 `27+/1−` **不含回填**，回填是独立提交 `807cfc3`（`1+/1−`）；`fce70d1` 本身是记录插入，`7111d97` 才是 2+/2− 的台账订正

判定表（3 个编号小节 + 1 条方法论）：

| # | 它说的 | 判定 | 证据与处置 |
|---|---|---|---|
| ① | `7111d97` 已抹掉旧串，`grep -c '⇒ 引用正确'` = 0；D-1 行 `\|` 计数 5，表结构未坏 | **部分成立**（改动成立，计数不成立） | `sed -n '101p'` 现文为「⇒ **指针指向正确**（此处「引用正确」只指 ④-3 的条号指向对不对…）」，括注同格（成立）。但 `grep -c '⇒ 引用正确'` = **1** 不是 0：命中 `:126`（我方 `consume 3 / 6` 表 ① 行**引述**旧措辞），旧串在 D-1 行确已消失 ⇒ 核「旧措辞是否消失」必须钉行号，全文件 `grep -c` 会被自己的引述污染（见文末审计口径第 3 条） |
| ①附 | 尾部同名小节 `:930` + 订正行 `:932`；`（暂无）` 只剩 `:134`/`:932` 两处**他行**引用（本行自身是第三个命中） | **确认** | `grep -n '（暂无）'` = **3 命中**（`:134`/`:932`/本行），不是 2 —— 本行引述了该串，自己就算一个 ⇒ 只钉行号、不报总数（审计口径第 5 条）；`grep -n '^#\+ .*判定不成立'` = `:784` / `:930`；`:932` 打印为「见上文同名小节（本清单已累计 **1..35** 条）…」 |
| ② | 源码落点：`validation.rs:78/:81/:83`、`label/service.rs:47`/`:93`、超 140 列 `9:180` / `21:198` / `49:144`、无 `rustfmt.toml` | **确认** | 逐行 `sed` 全中：`:78` = `pub fn validate_color(color: &str) -> Result<String, AppError> {`、`:81` = `if hex.len() != 3 && hex.len() != 6 {`、`:83` = `"color must be a 3 or 6 digit hex code…"`；`service.rs:47` 与 `:93` 均为 `!…starts_with('#') \|\| …len() != 7`；`awk 'length>140'` 输出与它三行一致；`find -maxdepth 3 -name '*rustfmt*'` 空 |
| ③ | 验证口径落差：它容器无 `cargo`，无法复算我方「`--quick` 通过」；建议入账改成「executor 本机…复算环境无 cargo 时不可验」 | **成立** | 本容器复跑同条即 `cargo: command not found` 属环境事实，非我方假证。已就地改参数表「验证命令（文档轮）」状态格：入账须带 `Finished` 原文 + 时刻并注明「executor 本机」 |
| ③附 | 方法论：空白审计必须钉区间（它 `:785-900` 得 40 空行，我方原报 32） | **成立**（两数都对，区间不同） | `awk 'NR>=785&&NR<=862' \| grep -c '^$'` = **32**（清单本体 `1.`–`35.`）；`awk 'NR>=785&&NR<=900'` = **40**（含其后三张防重放表）；两区间连续空行均 **0 处**。已立审计口径第 2 条 |
| ④ | 下轮收 `consume 4 / 6` 节；问 `fce70d1` 的 `27+/1−` 是否含回填 | **成立（已答）** | 本节即该节；`git show --stat fce70d1` = `1 file changed, 27 insertions(+), 1 deletion(-)`、`807cfc3` = `1 insertion(+), 1 deletion(-)` ⇒ 回填独立，不混在 `27+/1−` 里 |
| ④附 | 「④-3 行不动」「`validation.rs:83` 文案仍挂 B4-3」「`:49` 144 列不重排」 | **确认** | 与上轮口径一致，三项都在 L2 / 已判死一侧，本轮未动 |

**未做（L2 待拍板，无新增）**：它本轮零代码级发现；`validation.rs:83` 的用户可见文案仍按 B4-3 留在待拍板（改它动 OpenAPI 描述与前端校验）。

**改动 SHA**：`43caa97`（本文件：参数表 1 行 + 进度行 1 行原地替换 ⇒ 行数不变；文末新增本节与「审计口径」，37+/2−），推 `cnb` 单 remote。**回填**：评论 `2120411439805399040`（`2026-11-11T07:00:57Z`），回读 **C=4**、首行 `## 第 4 轮收尾`、末行 `@CodeBuddy`。

---

## 审计口径（batch#4 第 4 轮立，每轮必读）

1. **验证证据只写得出处，不写「全部通过」**：格式固定为「executor 本机 `<命令>` + `EXIT=0` + 关键输出行原文 + 耗时」。复算环境无工具链时，回帖明写「该项在复算环境不可验」，不得让它看起来像公共可复算事实。
2. **空白 / 连续性审计必须钉区间**（同一文件换端点就换数）：报数写成「`<起>-<止>` = N 空行，连续 run ≥2 共 M 处」。参考值：`:785-862` = 32 空行、`:785-900` = 40 空行，两区间 M 均 0。
3. **核「旧措辞是否消失」只许钉行号**，不许全文件 `grep -c`：我方会在下一轮**引述**对方措辞（`consume 3 / 6` 表 ① 行引了 `⇒ 引用正确`——此处刻意不写行号：它每轮都会被后续插行推后），任何凭 `grep -c` 求 0 的判据都会被自己的引述打回。写法：`sed -n '<line>p'` 打印那一行，`grep -c` 只作辅助并注明命中位置。
4. **台账自引用只写条目号 / 小节名**；新轮次一律追加到文末——中间插入会推后其后所有绝对行号，`:784` / `:862` / `:930` / `:932` 这几处就是这样被钉住的。
5. **报计数必须钉「时点」**：同一串每被引述一次命中数就 +1（`⇒ 引用正确` 的实测轨迹：`7111d97` = 0 → `a996cab` = 1 → `fce70d1` = 1 → `43caa97` = 3 → `507601a` = 3）。所以计数只能写成「在 `<SHA>` 上 = N」或「除第 `<行号>` 行自身外 = N」，绝不写「当前 = N」——时点版本永远追不上。
6. **回填提交只许动 comment id / SHA / 时间字段**：`a996cab` 回填时把 `7111d97` 已订正的 D-1 行整行覆盖回旧原文（`⇒ 引用正确` 复活），是 `fce70d1` 才恢复；判据 `git show <回填>:docs/reports/OPT-LOOP-LEDGER.md | grep -n '⇒ 引用正确'`（`7111d97` 无命中 → `a996cab` 命中 D-1 行 → `fce70d1` 只剩他行引述）。回填前必须 `git diff --cached --stat` 只有目标行，禁止整段重写。
7. **「本节新增 N 条 / 累计 1..M」必须三向对账**（batch#5 单发轮第 6 次消费立，一次抓出 3 处）：①本节**物理**含哪些编号（`awk` 取本小节区间的 `^[0-9]+\. `）；②谁**写入**的（`git log -S'<条目标题串>' -- <file>`）；③下一节的累计值。三者不一致时，正解是**写清归属**（「编号 A 由 B 轮写入，物理位置在 C 节」），**不许改累计值抹平**——那等于篡改归属。反例：`:1413` 的 `1..45` 但同节物理含 45/46；`:1597` 的「2 条（③、⑨）」但 ③/④/⑨ 三条均判不成立（④ 无条目）；`### 判定不成立` 同名小节曾一 H3 一 H2 两处并存，指引行坐在后者里。

---

### consume 5 / 6（batch#4）· 追加于文末（只为零行号漂移，勿移入上文）

- 输入：NPC 复核 `2120411842353717248`（`2026-11-11T07:02:33Z`，首行 `@cnb.biewen(别问) 被复核 SHA = **507601a**`）——晚于我方第 4 轮收尾（`07:00:57Z`）⇒ **有新复核**，K=5。C=4 由正则 `^## 第 \d+ 轮收尾` 数得（同一 K 只算 1）。
- 基线：`BASE=4491f7e`。它复核的 `507601a` 之后远端另有 3 个提交（`3cee750` 台账回填 1+/1−，`dd7ac03`+`4491f7e` 只碰 `CLAUDE.md`/`ARCHITECTURE.md`）；`git diff 507601a..HEAD -- docs/reports/OPT-LOOP-LEDGER.md` = **1 insertion(+), 1 deletion(-)**、行数仍 967 ⇒ **本批台账零行号漂移**，直接续用它的行号基准。
- 形态：**零新代码靶轮**——两处新发现全落在台账自身（验证记录缺时刻、计数被自己的引述污染），另附一条 git 历史级的回填误覆盖。按 F20 只落台账与口径，不改业务代码。

判定表：

| # | 它说的 | 判定 | 证据（除注明外均在 `507601a` 的台账上复算） |
|---|---|---|---|
| ②-A 主体 | `:23` 已立「时刻」标准，唯一按新口径写的验证记录（`:941`）独缺时刻 | **成立** | `sed -n '941p' \| grep -c '[0-9][0-9]:[0-9][0-9]'` = **0**；该行有 `Finished … 0.74s` 与「executor 本机」，确实只缺时刻 ⇒ 自订标准被自己第一个违反 |
| ②-A 计数 | 全文件 `grep -n '改后'` = **20** 命中 | **不成立** | 在 `507601a` 上 = **18**（`807cfc3` 同；`grep -n '改后' \| wc -l` 亦 18）。少数 2 处 |
| ②-A 附 | `sed -n '935,967p' \| grep -c '改后'` = 0 | **成立** | 复算 = 0：新节确实只记了改前那半边 |
| ②-B1 | `（暂无）` 3 命中 = `:134`/`:932`/`:949` | **成立** | `grep -n '（暂无）'` 逐行打印即这三点 |
| ②-B2 | `:949` 自身写「= 2 命中」而该命令今天 = 3；同一行左边说两点、右边贴的正是会打出三点的命令 | **成立** | `:949` 原文含「`grep -n '（暂无）'` = 2 命中且都是叙述」，而本行自身即第三命中 ⇒ 上一轮刚立的「引述污染」教训在自己的新增行里复发 |
| ②-B3 | 在 `507601a` 上 `⇒ 引用正确` = 3（`:126`/`:948`/`:966`），轨迹 `7111d97`=0 → `a996cab`=1 → `fce70d1`=1 → `43caa97`=3 | **成立** | 五个时点（`7111d97`/`a996cab`/`fce70d1`/`43caa97`/`507601a`）各跑 `git show <sha>:… \| grep -c`，逐点全中；`:126` 确为引述 |
| ②-B4 | `sed -n '101p' \| grep -c '⇒ 引用正确'` = 0，而 `507601a` 全文件 = 3 ⇒ 钉行号有效 | **成立** | 复算 0 / 3；`:101` 现文为「⇒ **指针指向正确**（此处「引用正确」只指 ④-3 的条号指向对不对…）」 |
| ②-B5 | `a996cab` 回填把 `7111d97` 修好的 D-1 行覆盖回旧原文，`fce70d1` 才恢复 | **成立（git 历史级）** | `git show a996cab:… \| grep -n` 命中 `:101`（D-1 行本身）；`git show fce70d1:… \| grep -n` 只命中 `:126`（他行引述）⇒ D-1 已还原 ⇒ 已入审计口径第 6 条 |
| ③ | 审计口径 4 条可机械核性（3 条「可核」+ 第 1 条「形态对、执行漏一格」） | **确认** | 与我方自评一致；其「第 3 条已自证」的两条判据（`101p` = 0 / 全文件 = 3）已复算相符 |
| ④ 指针 | `:51` 进度 `4 / 6`、两个评论 id 指得实；但「`consume 3 / 6` 在 `:98` 区段」 | **部分成立** | `4 / 6` ✅；两个 id 在评论列表里存在 ✅（`2120361552162230272`/`2120377967560794112`）；但 `:98` 是**空行**，`### consume 3 / 6（batch#4）` 真值在 `:114`。它报 `:936` 的 `consume 4 / 6` 是对的 |
| ④ validation | `validation.rs:78` = `pub fn validate_color`、`:81` = `hex.len() != 3 && != 6`、`:83` = 用户可见文案；9 行最长 79 列 | **成立** | `sed -n '76,84p'` 逐行打印全中；`awk` 列宽 76/76/64/31/59/41/41/**79**/11 ⇒ `#f55` 两次 400 的结论不变 |
| ⑤ | 反向清单 35 条编号连续唯一、`:784`/`:930` 双同名小节、`:932` 指向累计 1..35 | **成立** | `grep -o '^[0-9]\+\. '` 取到 1..35 连续无重；`grep -n '^#\+ .*判定不成立'` = `:784`/`:930`；`:932` 打印为「见上文同名小节（本清单已累计 **1..35** 条）…」 |
| ⑤ 附「结构干净」 | 清单结构干净、可机械计数 | **部分成立** | 计数确实可核（35 条）；但 1–4 条之间残留空行（`:789`/`:791`/`:793`），`grep -A1 '^4\. '` 取不到第 5 条。修它会把 `:784` 之后全部自引用推前 3 行 ⇒ **不修，只记账**（可读性让位于零漂移，同口径第 4 条） |
| ⑤ 附「`:966` 写死 `:126`」 | 审计口径第 3 条把行号当例子写死，后续插行会漂 | **成立** | 已改为「`consume 3 / 6` 表 ① 行」小节名口径 |
| ⑥ | 它自述本轮零代码、零台账提交 | **不可验** | 只读容器，对我方不构成义务；如实记为「不可验」，不计入成立/不成立 |

**本轮改动（全 L1，纯 markdown，改前 967 行）**：

1. `:23` 参数表：为「时刻」加可复算定义——UTC `HH:MM`，或**时间锚**（提交 author date / 日志时间戳）。标准不降级，但变成可满足、可重放。
2. `:941`：补时刻锚 `43caa97` 的 author date `2026-11-11T15:00:10+08:00`（= `07:00:10Z`，`git log -1 --format=%aI 43caa97` 可复算），并声明两条验证均在其之前。1 行→1 行。
3. `:949`：「= 2 命中」改「**= 3 命中**（`:134`/`:932`/本行自身）」，并写明只钉行号、不报总数。1 行→1 行。
4. `:966`：审计口径第 3 条的 `:126` 例子换成小节名，切断漂移。1 行→1 行。
5. 审计口径新增第 **5** 条（引述即污染 ⇒ 计数必附 SHA 或被引述行行号）与第 **6** 条（回填只许动 comment id / SHA / 时间字段，禁整段重写）。
6. `:51` 进度行 `4 / 6` → **`5 / 6`**，四个指针改为一律带 `（batch#4）` 的小节名（`consume 3 / 6` 节那类纯行号定位不再使用）。

**未做（L2 待拍板，无新增）**：本轮零代码级发现；`validation.rs:83` 的用户可见文案（承诺 3 或 6 位、而 3 位端到端必 400）仍按 **B4-3** 留在待拍板——改它同时动 OpenAPI 描述与前端校验。

**反向清单**：本轮**新增「不成立」0 条**（唯一判死的是一条计数：`改后` 20 → 真值 18，已入判定表，不构成需长期封锁的重复指摘形态）；清单仍为累计 **1..35** 条，未改动。

**验证**：`BASE=4491f7e`；改前 `scripts/check.sh --quick` **`EXIT=0`**（executor 本机，`2026-11-11T08:03:37Z` → `08:03:38Z`，`Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 0.32s`、`==> checks passed in 1s (quick)`）；改后同命令 `EXIT=0`，时刻与原文见回填提交。

**改动 SHA**：`c30e542`（本文件 52+/5−，967 → 1014 行；五处 1 行→1 行原地替换 + 口径两条 + 本节新增），推 `cnb` 单 remote `4491f7e..c30e542`。**改后验证**：`scripts/check.sh --quick` **`EXIT=0`**（executor 本机 `cargo 1.99.1`，`2026-11-11T08:05:26Z` → `08:05:28Z`，`Finished \`dev\` profile … in 0.22s`、`==> checks passed in 2s (quick)`）。**回填**：本轮收尾评论 `2120427880743084032`（`2026-11-11T08:06:17Z`），回读 **C=5**（下一轮 K=6，末轮）、首行 `## 第 5 轮收尾`、末行 `@CodeBuddy`、正文 16182 字符。

---

### consume 6 / 6（batch#4，末轮）· 追加于文末（只为零行号漂移，勿移入上文）

- 输入：NPC 复核 `2120428177498488832`（`2026-11-11T08:07:27Z`）——晚于我方第 5 轮收尾（`08:06:17Z`）⇒ **有新复核**，K=6（末轮）。C=5 由正则 `^## 第 \d+ 轮收尾` 数得（同一 K 只算 1）。
- 基线：`BASE=61c9f9b`；`git fetch cnb` 后 `git rev-list --left-right --count HEAD...cnb/main` = `0 0`（零漂移）。它复核的 `c30e542` 与本机 `61c9f9b` 只差 `:1014` 一行（回填 1+/1−），行数同为 1014 ⇒ 它钉的行号可直接复用。
- 形态：**零新代码靶轮**。13 条复核里 9 条是对我方上轮改动的确认，3 条新指摘 + 1 条「范围外硬发现」，全部落在台账措辞 / 计数，未触及业务代码。
- 读法约定：本节表格里的 `:<n>` 一律指 **`61c9f9b` 上的行号**，且只用于**指认被判定对象**（对方就是按行号提的）；本节内部定位一律用小节名 / 条目号，按口径第 4 条不写行号。

判定表：

| # | 它说的 | 判定 | 证据与处置 |
|---|---|---|---|
| ②-B3 | 「五个时点」实为 **4 个**（4 个 SHA 之后单列 `507601a`） | **不成立** | 五个 SHA 是**五个不同 commit**：`git cat-file -t` 全为 `commit`、`git log -1 --format=%s` 各不相同（第 3 轮订正 / 第 2 轮回填 / 第 3 轮记录 / 第 4 轮 / 第 4 轮回填）。逐点复算 `git show <sha>:docs/reports/OPT-LOOP-LEDGER.md \| grep -c '⇒ 引用正确'` = **0 / 1 / 1 / 3 / 3**，与 `:988` 逐点一致 ⇒ 原句计数正确，不改数。**已核实为不成立，不要重复提（反向清单 37）** |
| ②-1 | `:984` 的「真值 **18**」已过期（今天全文件 = **22**），应写「在 `507601a` 上」 | **部分成立**（措辞侧接受，它给的新数字错） | 已把 `:984` 改为「在 `507601a` 上 = **18**（`807cfc3` 同）」，与口径第 5 条同形，1 行→1 行。数字侧不成立：`grep -c '改后'` 在 `61c9f9b` 上 = **23** 而非 22（命中 `:56/158/188/229/232/260/295/320/375/401/421/452/474/511/526/537/694/842/984/985/1010/1012/1014`，含它漏算的 `:1012`/`:1014`） |
| ②-2 | `:966` 已无 `:126` 但**漏了 `:968`**；`:969` 又用行号引了 `:101`/`:126`，与口径**第 3 条**打架 | **不成立**（条号错 + 事实错），但其方向命中一处我方自违 | 事实侧：`:968`/`:969` 对 `:101`/`:126` 的命中数均为 **0**（逐行 `grep -c`），`:969` 写的是「命中 D-1 行」这类**条目名**。条号侧：它称「第 3 条要求不写行号」，恰好相反——第 3 条要求**钉行号**（`sed -n '<line>p'`）作为判「旧措辞是否消失」的唯一手段；禁写自引用行号的是**第 4 条**。真缺陷在它没打中的格：`:968` 原写「→ `43caa97` = 3 → **今仍 3**」，`今仍 N` 正是第 5 条禁止的无时点计数 ⇒ 已改 `507601a` = 3（自查成立，不入反向清单） |
| ④ 指针 | `:51` 的 `5 / 6` 与两个 id 指得实；但「`consume 3 / 6` 在 `:98`」错，真值 `:114` | **成立** | `sed -n '98p'` = **空行**；`sed -n '114p'` = `### consume 3 / 6（batch#4）`。`:98` 出自 `consume 5 / 6` 改动清单第 6 条，是**陈旧历史坐标**（该节早先确在 `:98`，两次文末追加后漂到 `:114`）⇒ 按口径第 4 条改为小节名，1 行→1 行 |
| ④ 硬发现 | `crates/rg-http/src/api/validation.rs:76-84` 在 `c30e542` 的树上**不存在**，建议把该文件移出可核范围 | **不成立** | `git show c30e542:crates/rg-http/src/api/validation.rs \| wc -l` = **117**；`git ls-tree c30e542 crates/rg-http/src/api/` 有该 blob；`sed -n '76,84p'` 打出的 9 行与它自引的逐行一致（`:78` `pub fn validate_color` / `:81` `hex.len() != 3 && hex.len() != 6` / `:83` 用户文案）⇒ 取不到是它侧取数问题（其容器只有一个 remote 且 `origin` = cnb.cool；本仓 `git remote -v` = `cnb`=cnb.cool / `github` / `origin`=git233）。**`validation.rs` 不移出可核范围。已核实为不成立，不要重复提（反向清单 36）** |
| ① / ③ / ⑤ / ⑥ | 其余：`:23` 新定义覆盖 `:941`、`:949` 的「= 3 命中」与命令输出一致、反清单 1..35 连续唯一、`:784`/`:930` 双同名小节、它自述零提交零改动 | **确认 5 / 不可验 1** | 抽查全中：`grep -n '⇒ 引用正确'` 在 `507601a` 上 = `:126`/`:948`/`:966`；`grep -o '^[0-9]\+\. '` 取到 1..35 连续无重；`:932` 打印为「累计 **1..35** 条」。⑥ 记为**不可验**，不计入成立/不成立 |
| ⑤ 它自列第 36 条 | 它把「22 与 18 冲突 ⇒ `:984` 结论作废」先行判死 | **不采纳为反向清单条目** | 该条判的是它**假设的**我方主张，我方从未提「作废」；且其中「22」经复算为 **23**（见 ②-1）。口径不变：反向清单只收「我方判定对方指摘不成立」的条目，不收对方的自我判死 |

**本轮改动（全 L1，纯 markdown，改前 1014 行）**：

1. 审计口径第 5 条：轨迹补上 `507601a` = 3、删「今仍 3」——原写法本身就是它禁止的无时点计数（自违）。1 行→1 行。
2. `consume 5 / 6` 表 ②-A 计数行：「真值 **18**」→「在 `507601a` 上 = **18**（`807cfc3` 同）」，与口径第 5 条同形。1 行→1 行。
3. `consume 5 / 6` 表 ②-B3 行：「五个时点」后全列五个 SHA，把计数争议钉成可复算列表。1 行→1 行。
4. `consume 5 / 6` 改动清单第 6 条：`:98` → `consume 3 / 6` 节（陈旧历史坐标，按口径第 4 条改）。1 行→1 行。
5. `:51` 进度行 `5 / 6` → **`6 / 6（已收批）`** + 第 6 轮指针（小节名，不写行号）。
6. 尾部索引 `:932`：`1..35` → **`1..38`**（36–38 在本节）。1 行→1 行。

**未做（L2 待拍板，无新增）**：它本轮零代码级发现；`validation.rs:83` 的用户可见文案（承诺 3 或 6 位、而 3 位端到端必 400）仍按 **B4-3** 留在待拍板。**拒绝**它「把 `validation.rs:76-84` 移出可核范围」的建议——前提（文件不存在）已被否证，见判定表 ④。

**反向清单（本节新增 3 条，累计 1..38）**：

36. 「`crates/rg-http/src/api/validation.rs:76-84` 在 `c30e542` 的树上不存在，把该文件移出可核范围」—— 反证：`git ls-tree c30e542 crates/rg-http/src/api/` 含该 blob，`git show c30e542:<path> | wc -l` = **117**，`sed -n '76,84p'` 与它自引的 9 行逐行一致。**已核实为不成立，不要重复提。**
37. 「`consume 5 / 6` 表 ②-B3 的『五个时点』实为 4 个」—— 反证：五个 SHA 是五个不同 commit（`git cat-file -t` 全 `commit`），逐点 `grep -c '⇒ 引用正确'` = 0/1/1/3/3，与原文逐点一致。**已核实为不成立，不要重复提。**
38. 「`:969` 用「没动行号」的方式引了 `:101`/`:126`，与审计口径第 3 条打架」—— 反证：`:968`/`:969` 对 `:101`/`:126` 的 `grep -c` 均为 **0**，`:969` 用的是「命中 D-1 行」条目名；且第 3 条要求**钉**行号，禁写自引用行号的是第 4 条。**已核实为不成立，不要重复提。**

**验证**：`BASE=61c9f9b`；改前 `scripts/check.sh --quick` **`EXIT=0`**（executor 本机 `Mac` / `cargo 1.99.1`，`2026-11-11T09:09:05Z` → `09:09:08Z`，关键行原文 `==> cargo clippy --workspace --all-targets -D warnings` / ``Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.99s`` / `==> checks passed in 3s (quick)`）。改后同命令见下方改动 SHA 行。

**改动 SHA**：`d4dfb51`（本文件 48+/6−，1014 → 1055 行；五处 1 行→1 行原地替换 + `:51`/`:932` 两处原地替换 + 本节追加），推 `cnb` 单 remote `61c9f9b..d4dfb51`。**改后验证**：`scripts/check.sh --quick` **`EXIT=0`**（executor 本机 `Mac` / `cargo 1.99.1`，`2026-11-11T09:10:13Z` → `09:10:15Z`，关键行原文 ``Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.25s`` / `==> checks passed in 2s (quick)`）；**时刻锚** = 本提交 author date `2026-11-11T17:10:19+08:00`（= `09:10:19Z`，`git log -1 --format=%aI d4dfb51` 可复算），两条验证均发生在其之前。

**回填**：本轮收尾评论 `2120444257281421312`（`2026-11-11T09:11:21Z`），回读 **C=6**（本批 6/6 收批）、首行 `## 第 6 轮收尾`、末行 `@CodeBuddy`、正文 18493 字符；`BATCH_COMPLETE` 评论 `2120444299035713536`（`2026-11-11T09:11:31Z`），issue #27 已置 `closed/completed`。

### consume 1 / 6（batch#5）· 追加于文末（只为零行号漂移，勿移入上文）

- 输入：NPC 首扫 `2120454308016586752`（`2026-11-11T09:51:17Z`）。C=0（正则 `^## 第 \d+ 轮收尾` 无命中）⇒ K=1，非补记。
- 基线：`BASE=e49ce4a`（`cnb/main` = 本机 HEAD，`git rev-list --left-right --count HEAD...cnb/main` = `0 0`）。它钉的 `d32421e` 与之只差 `docs/reports/OPT-LOOP-LEDGER.md`（开批 +12 行，单文件）⇒ 本批 5 个源文件零漂移，行数指纹 **1857 / 1297 / 1065 / 326 / 315** 全等，它给的行号可直接复用。
- 判定汇总：**成立 6 / 部分成立 1 / 子命题不成立 3 / 我方自证推翻 2**。其误差全落在非结论位置（4 处，见本节末「对方行号/计数误差」）。
- 读法约定：下表 `:<n>` 一律指 `e49ce4a` 上那 5 个文件的行号，只用于**指认被判定对象**；本节内部定位一律用小节名 / 条目号（审计口径第 4 条）。

判定表：

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| Q1 | 并发组算而不用：group 只进日志与 409 文案，互斥与取消实际按 `ref_name` 判定 | **成立** | `grep -n group crates/rg-ci/src/lib.rs` 实测 **6 行**命中（`:136` `:137` `:144` `:145` `:156` `:158`；它自报「5 处」却同时贴出 6 行——计数错、结论不变）；`:139` 判据源 `find_active_pipelines_by_ref(db, repo_id, ref_name)`，该函数在 `crates/rg-db/src/ops/pipeline_ops.rs:1315-1343`（它写 `:1315-1340`；函数体到 `:1342`、闭括号 `:1343`），过滤条件只有 `repo_id` + `ref_name` + 4 个活跃状态；`resolve_concurrency_group`（`pipeline_ops.rs:1392-1397`）纯字符串替换、不写库不入查询；取消走 `cancel_pipeline_chain(db, p.id)`（`:149`）不带 group。**它补充的「`${{ branch }}` 等于没实现」成立**：该值唯一落点是 `:144` 的 tracing 与 `:158` 的 409 文案 |
| Q2 | Docker 路径未设 `kill_on_drop`，超时不杀子进程 | **成立**（其「重名必然」不成立，见反向清单 39） | `grep -n kill_on_drop crates/rg-ci/src/runner.rs` = `:561` / `:576` 仅两处（均在 `run_job_local`）；docker 侧 `Command::new("docker")` 在 `:687-688`、`.output()` 在 `:694-697`、无 `kill_on_drop`；超时包装 `tokio::time::timeout` 在 `:505-515`，靠 drop future 生效 ⇒ 容器与 `docker run` 进程继续跑；容器名 `format!("ironforge-job-{job_id}")` 在 `:651` 为确定性命名；`--rm` 参数在 **`:656`**（它写 `:653`，那是注释行）。模块头 `:16` 的原句本轮已改 |
| Q3 | `gitea_actions.rs:463` 的 `export k=v` 未做 shell 引号 | **成立（我方机制描述错，已按其订正记账）** | `sed -n '463p'` = `script.push(format!("export {}={}", k, expanded));`；`substitute_expr`（`:803-836`）只做文本替换、无转义。`sh` 实测：`export A=hello world` ⇒ `A=hello` 且 **`world` 未执行**（`export` 后续词按变量名处理）⇒ 我方 issue 的「执行 `world`」不成立；`;` / `&&` / `|` / `#` 为真注入（`|` 还会把 `A` 变空）；**换行 = 无条件新脚本行**，不需要任何元字符 |
| Q4-a | `pull_request` 的 base_ref 用 `default_branch` 近似 | **成立但不可达** | 去重命令 `grep -rhn 'trigger_type: "' crates/ --include=*.rs \| grep -o '"[a-z_]*"' \| sort \| uniq -c` = `manual` / `merge_group` / `push`×4 / `retry` / `suggestion` —— **无 `pull_request`** ⇒ `gitea_actions.rs:301` 的 PR 一侧在生产环境走不到 |
| Q4-b | 该分支真实受害者是 `merge_group` | **部分成立**（底层缺陷成立；它给的例子不成立，见反向清单 40） | 见「实测」小节：`on: merge_group:` **永不匹配**（`WorkflowTriggerSingle`（`:57-66`）没有 `merge_group` 字段、也未 `deny_unknown_fields` ⇒ 该键被 serde 静默丢弃 ⇒ 走 `:308` 的 `else { false }`）；只有 workflow 声明 `pull_request:` 时 `:306` 才生效，而此时过滤基准恒为 `default_branch` |
| Q4-c | `unwrap_or(false)` 静默吞错，与 native 路径的 `?` 口径不一致 | **不成立（我方引导问题前提错，采纳其反证）** | `:420` 确为 `unwrap_or(false)`，但 `:264` 的 `supported_condition` 先跑 `condition::validate_condition`（`condition.rs:41-55`），其接受集（`github` 四键 / `env.<valid_key>` / `matrix.<valid_key>`）⊇ `evaluate_condition`（`:57-64`：命中 context 或 `env.`/`matrix.` 前缀即 `Some("")`）；`actions_condition_context`（`:783-795`）恒插 `github.ref` / `github.ref_name` / `github.event_name` / `github.sha` ⇒ validate 通过后 `Err` 不可达；唯一生产调用点 `lib.rs:677` 在 `:664-665` 的 validate 之后 |
| B5-1 | `lib.rs:568` 把 gitea 路径的**所有**错误当「不存在」吞掉 | **成立（P1）** | `sed -n '568p'` = `if let Ok(config) = try_read_gitea_workflows(...)`；三个真实失败出口 `:662`（expand）/ `:665`（`validate_supported_actions`）/ `:704`（`no matching workflows`），另有 `:647-654` 单文件 parse 错只 `tracing::debug!` + `continue`；`:575-580` 的兜底文案「no CI config found (.gitea/workflows/*.yml or .ironforge-ci.yml)」在 gitea 配置写坏时自相矛盾。**它未提的补强**：`crates/rg-http/src/api/ci.rs:508` 的前置校验文案是「no .ironforge-ci.yml found」——只提 native 文件名 |
| B5-2 | `manual` / `retry` / `suggestion` 触发在 gitea-only 仓库必失败 | **成立（比它说的更强）** | `matches_event`（`:280-316`）只认 `push` / `pull_request` / `merge_group`，其余 `_ => false`（`:312`）；实测 `on: workflow_dispatch:` 对 `manual` 返回 **false** ⇒ **即便 workflow 正确声明了 `workflow_dispatch` 也匹配不上**（`:290` 把该字段 destructure 成 `_` 丢弃）；`has_ci_config`（`crates/rg-core/src/ci/mod.rs:76-91`）只看 `.gitea/workflows` 目录**存在**即 true（`:84`）⇒ 不会被 `ci.rs:507` 提前拦掉 |

实测（本节唯一新增取证，可复跑）：

- 方法：临时 integration test 直接调 `GiteaWorkflow::parse` + `matches_event(event, ref, "main")`，跑完即删，`git status --porcelain` 为空（未进任何提交）。
- 输出原样：

```
ON="merge_group:"                                EVENT=merge_group REF=refs/merge-queue/17 => matches=false
ON="merge_group:\n    branches: [main]"          EVENT=merge_group REF=refs/merge-queue/17 => matches=false
ON="merge_group:\n    branches: ['release/*']"   EVENT=merge_group REF=refs/merge-queue/17 => matches=false
ON="pull_request:\n    branches: [main]"         EVENT=merge_group REF=refs/merge-queue/17 => matches=true
ON="pull_request:\n    branches: ['release/*']"  EVENT=merge_group REF=refs/merge-queue/17 => matches=false
ON="workflow_dispatch:"                          EVENT=manual        REF=refs/heads/main      => matches=false
ON="push:\n    branches: [main]"                 EVENT=push          REF=refs/heads/main      => matches=true
```

- 读法：`merge_group:` 三行全 false ⇒ 该 trigger **键根本不被解析**；`pull_request:` 两行证明 `:306` 只在声明 PR 过滤器时才生效，且判定基准恒为 `default_branch`。这条同时否掉对方「`branches: ['main']` 会匹配」的例子（反向清单 40）。

**本轮改动（4 文件 7+/7−，零行号漂移）**：

1. `runner.rs:16`：`On timeout the child process is killed (not just abandoned).` → `On timeout the local child is killed; the docker run child is not (kill_on_drop unset).`（原句对 Docker 作业是假话）
2. `config.rs:14-15`：删「Pipelines with the same group will be serialized」与「Supports template variables」，改为「按 **ref** 强制、跨 ref 从不串行；模板只进日志文本」（原两句在生产环境都不成立）
3. `gitea_actions.rs:303-305`：删「For pull_request events … base branch is conveyed via default_branch」，改为「生产环境只有 `merge_group` 到达本分支，其 ref 是 `refs/merge-queue/<id>`，过滤恒按 `default_branch` 判定」
4. `lib.rs:554`：`2. .ironforge-ci.yml (native format)` 补一句「第一步的错误被吞，坏的 Gitea workflow 静默回落到这里」
5. 本台账：`batch#5` 索引两行（首扫 id / 进度 1/6）、尾部同名小节索引 `1..38` → `1..40`、本节

四项均为 1 行→1 行（第 2 项 2 行→2 行、第 3 项 3 行→3 行），改后 5 文件行数指纹仍 **1857 / 1297 / 1065 / 326 / 315**，与 issue 声明的基准逐项一致。

**未做（L2 待拍板 batch#5，本流程不动 main）**：

- **B5-1**（P1）`lib.rs:568` 吞掉 gitea 路径三类错误 → 只应把「目录不存在」（`:608`）当 fallthrough，其余 `Err` 直接 `return Err(e)`；连带 `api/ci.rs:508` 的文案只提 `.ironforge-ci.yml`。
- **B5-2**（P1）`matches_event` 缺 `manual` / `retry` / `suggestion`，且 `workflow_dispatch`（`:63` 解析、`:290` 丢弃）无映射 ⇒ gitea-only 仓库的手动触发与重试必失败。
- **B5-3**（P2）并发组算而不用：`find_active_pipelines_by_ref` 需增加 group 维度（DB 查询改动）。
- **B5-4**（P2）docker 路径补 `kill_on_drop(true)`；超时后还需显式 `docker kill` / `rm`，否则容器占资源到自行退出。
- **B5-5**（P2）`gitea_actions.rs:463` 的 `export k=v` 需 `sh_quote`，或改 `env K=V sh -c`；换行注入等价于新脚本行。
- **B5-6**（P2）`WorkflowTriggerSingle` 增 `merge_group` 字段（并考虑 `deny_unknown_fields` 以暴露未知 trigger 键）；merge queue 的 `branches` 过滤应以实际目标分支为基准，而非 `default_branch`。

**反向清单（本节新增 2 条，累计 1..40）**：

39. 「同一个 job 重跑时 `docker run --name ironforge-job-{job_id}` 会因容器重名直接失败——『这条不是「可能」，是必然』」—— 反证：retry 端点走 `trigger_pipeline` 建**新** pipeline（`crates/rg-http/src/api/ci.rs:613`），job 由 `create_job`（`crates/rg-db/src/ops/pipeline_ops.rs:381`）每次 INSERT 新行 ⇒ 新 job_id ⇒ 容器名不同；启动恢复只 `mark_job_timeout`（`crates/rg-http/src/lib.rs:3596-3638`）不重跑 ⇒ 当前无「同 job 重跑」路径。真实后果只剩「超时容器继续占宿主资源」。底层缺陷（`kill_on_drop` 缺失）本身**成立**，见 B5-4，两件事分开记。**已核实为不成立，不要重复提。**
40. 「`on: merge_group:` 带 `branches: ['main']` 时会匹配 ⇒ `default_branch` 近似是 merge_queue 的真实受害者」—— 反证：实测 `on: merge_group:` 与 `on: merge_group: branches:[main]` 对 `matches_event("merge_group", "refs/merge-queue/17", "main")` 均返回 **false**（`WorkflowTriggerSingle`（`gitea_actions.rs:57-66`）无 `merge_group` 字段且未 `deny_unknown_fields` ⇒ 键被 serde 丢弃 ⇒ `:308` 的 `else { false }`）。该行只在 workflow 声明 `pull_request:` 时生效（实测 true），而此时过滤基准恒为 `default_branch`。**已核实为不成立，不要重复提。**

**我方已自证推翻（本节 2 项，勿当结论引用）**：

| 我方原文 | 实际 | 处置 |
|---|---|---|
| issue 引导问题 3 的机制：「`export FOO=hello world` 会被拆成 `export FOO=hello` + **执行 `world`**」 | `sh` 实测 `$A` = `hello`、`world` **未**执行 | 采纳对方订正：真注入面是 `;` / `&&` / `|` / `#` 与**换行**（换行 = 无条件新脚本行），空格只是静默截断。缺陷本身仍成立（B5-5） |
| issue 引导问题 4 后半：「`unwrap_or(false)` 静默吞错，与 `lib.rs:242-252` 的 `?` 口径不一致」 | validate 接受集 ⊇ evaluate（`condition.rs:41-55` / `:57-64`），且上下文恒插四键（`:783-795`）⇒ validate 之后 `Err` 不可达 | 判**不成立**，不立 L2。口径不一致只剩 API 契约层（`to_ci_config` 是 `pub`），不是可观测缺陷 |

**对方行号/计数误差（非结论位置，逐条列出以便下轮它自查）**：`config.rs:16`（真值 `:15`，`:16` 是 `pub group: String,`）；`--rm` 说在 `:653`（真值 `:656`，`:653` 是注释行）；`pipeline_ops.rs:1315-1340`（真值 `:1315-1343`）；`grep -n group` 自报 5 处（真值 6 行）。

**验证**：`BASE=e49ce4a`。改前 `scripts/check.sh --quick` **`EXIT=0`**（executor 本机 `Mac` / `cargo 1.99.1`，`2026-11-11T10:13:06Z`，关键行原文 `==> cargo fmt --all --check` / ``Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.88s`` / `==> checks passed in 2s (quick)`）。改后同命令见下方改动 SHA 行。

**改动 SHA**：`a668b5e`（4 文件 7+/7−，5 文件行数指纹不变），推 `cnb` 单 remote。**改后验证**：`scripts/check.sh --quick` **`EXIT=0`**（executor 本机，关键行原文 ``Finished `dev` profile [unoptimized + debuginfo] target(s) in 23.99s`` / `==> checks passed in 25s (quick)`；25 s 来自 rg-ci 因注释变更重编译，热缓存口径见参数表）；**时刻锚** = 本提交 author date `2026-11-11T18:19:13+08:00`（= `10:19:13Z`，`git log -1 --format=%aI a668b5e` 可复算），改后验证发生在其之前。

**回帖**：`2120461940857147392`（`2026-11-11T10:21:37Z`），回读 **C=1**（首行 `## 第 1 轮收尾`、末行 `@CodeBuddy`、正文 22121 字符、平台侧仅此 1 条收尾无重复投递）。

### consume 2 / 6（batch#5）· 追加于文末（只为零行号漂移，勿移入上文）

- 输入：NPC 第 2 轮复核 `2120465837906632704`（`2026-11-11T10:37:06Z`），晚于我方第 1 轮收尾（`10:21:37Z`）⇒ 有新复核。C=1（正则 `^## 第 \d+ 轮收尾` 仅命中我方收尾那条）⇒ K=2。
- 基线：`BASE=f9b8d5a`（`cnb/main` = 本机 HEAD，`git rev-list --left-right --count HEAD...cnb/main` = `0 0`）。它自述复核 `6b9a907`；`git diff --stat 6b9a907 f9b8d5a -- crates/` **为空** ⇒ 两树源文件同一，行号基准零漂移。本批 5 文件行数指纹 **1857 / 1297 / 1065 / 326 / 315** 全等。
- 判定汇总：**确认 7 / 成立 3 / 部分成立 2 / 成立但非缺陷 1 / 不可验 1 / 子命题不成立 1**；另我方探针新增 1 条 P1（B5-10，它未提）。
- 读法约定：下表 `:<n>` 指 `f9b8d5a` 上那 5 个源文件（或注明文件）的行号，只用于指认被判定对象；本节内部定位一律用小节名 / 条目号（审计口径第 4 条）。

判定表：

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| ①-1 | `runner.rs:16` 原文逐字 | **确认** | `sed -n '16p'` 与它引句逐字同（改前 97 字符，改后见「本轮改动」） |
| ①-2 | `config.rs:14-15` | **确认** | 两行逐字同：`Enforced per **ref**, not per group` / `different refs never serialize. … only reach log text.` |
| ①-3 | `gitea_actions.rs:303-305` | **确认** | 三行逐字同（`branches` filters target the base branch… / In production only `merge_group`… / ref is `refs/merge-queue/<id>`…) |
| ①-4 | `lib.rs:554` | **确认** | 逐字同：`2. `.ironforge-ci.yml` (native format) — errors in step 1 are swallowed…` |
| ②-39/40 | 它给反证：39 / 40 维持「不成立」 | **确认** | 它的探针与我方第 1 轮实测逐行一致（`merge_group:` 三形态全 false；`pull_request: branches: [main]` 时 `merge_group` 为 true，与反向清单 40 的表述同源）。39 的补充事实（`--rm` 在正常退出时清容器）成立，不改变判定 |
| ③N1 | 工作区路径与仓库路径是兄弟目录，`_ci_workspaces` / `_ci_cache` 是合法 owner 名 | **部分成立** | 成立部分：`workspace_path()`（`runner.rs:789-798`，`:795` 为 `root.join("_ci_workspaces")`）确返 `<repo_root>/_ci_workspaces/<repo_id>/<pipeline_id>`；`_ci_workspaces` 不在保留名单——`validate_username`（`crates/rg-core/src/lib.rs:71-78` 字符集）只额外查 `-agent-`（`crates/rg-core/src/agent.rs:53-55`），`validate_repo_name`（`:102-108`）允许 `_`；`_ci_cache` 三处同根成立（`runner.rs:914` / `crates/rg-http/src/api/runners.rs:777` / `crates/rg-http/src/api/ci_retention.rs:103`）。不成立部分见反向清单 41 |
| ③N2 | `repo_root=/` ⇒ `cache_root=/_ci_cache` | **成立但非缺陷** | 字面成立（`ci_retention.rs:103` 的 `state.repo_root.join("_ci_cache")`）；但 `repo_root=/` 本身就把仓库铺到 `/` 下，属误配，无独立处置 |
| ③N3 | `export k=v` 还有 `$` / `$()` 面（不只空白截断） | **成立（归入 B5-5 加强）** | `sh` 实测：`export A=$(id -u)` ⇒ `$A`=`501`（**命令替换被执行**）；`$VAR` 静默展开。`:463` 为 `export {}={}` 无引号，`substitute_expr`（`:803-836`）只做 `String::replace`。换行注入本轮亦复测（`touch` 落地） |
| ③N4 | `actions/cache@` 只取 `step.with`，不取 `step.env` | **成立（P3）** | 两个输入取自 `step.with.get("path")` / `step.with.get("key")`（`:431-432`）；`:441` 的 `substitute_expr(key, name, &self.env, &job_vars)` 入参只有 workflow / job 两级（签名 `:803-808`），step.env 不参与 ⇒ 用 step 级 env 写 `with: key: ${{ env.X }}` 不解析；key 落 `lib.rs:234` 的 `cache_key` |
| ③N5 | `runner.rs:16` 可以更准（超时后容器仍在跑） | **部分成立** | `--rm`（`:656`）+ 确定性容器名（`:651`）事实对；原句只到「`docker run` 子进程不被杀」，未讲容器后果 ⇒ 已改写 |
| ④ | 它 3 条自我订正（撤回 `unwrap_or(false)` 缺陷 / `grep -n group` 应为 6 行 / `config.rs:16`→`:15`） | **确认** | 与我方第 1 轮判定逐条一致，无需再动 |
| ⑤-1 | 去重命令 `grep -rn 'join("_ci_workspaces")\|join("_ci_cache")'` = 4 行 | **确认** | 复跑同命令 = 4 行，路径与行号全同（`runner.rs:795` / `:914` / `ci_retention.rs:103` / `runners.rs:777`） |
| ⑤-2 | `grep -rn 'export {}'` = 1 行、`sh_quote\|shell_quote` = 0 行 | **确认** | 复跑一致 |
| ⑤-3 | `matches_event` 连事件名 `workflow_dispatch` 都不认 | **确认（算 B5-2 加强，不单列）** | 探针：`on:` + `workflow_dispatch:` ⇒ `workflow_dispatch=false`；机制 `:312` 的 `_ => false`，该字段在 `:290` 被 destructure 成 `_` 丢弃 |
| ⑤-4 | crash 探针：checkout 丢、注册在时 `prepare_workspace` 二次仍 Ok | **不可验（采信，不入结论）** | 未复跑其探针；它自己标「checkout 在、注册被手删」待核，维持待核 |

**我方探针新增（B5-10，P1）：`on:` 下的裸键永不匹配**

方法同第 1 轮（临时 example 直调 `GiteaWorkflow::parse` + `matches_event`，跑完删，`git status --porcelain` 为空）。输出原样：

```
on: push                                                | push=true  workflow_dispatch=false manual=false
on: [push, pull_request]                                | push=true  workflow_dispatch=false manual=false
on:\n  push:                                            | push=false workflow_dispatch=false manual=false
on:\n  push:\n    branches: [main]                      | push=true  workflow_dispatch=false manual=false
on:\n  workflow_dispatch:                               | push=false workflow_dispatch=false manual=false
on:\n  workflow_dispatch:\n    inputs:\n      x:..."   | push=false workflow_dispatch=false manual=false
```

- 机制：`pub push: Option<EventFilter>`（`gitea_actions.rs:58`）把 `push: null` 反序列化成 `None`，与「键缺失」**不可区分** ⇒ `:294-299` 走 `else { false }`。
- 语义：GHA / Gitea 里 `on:` + 裸 `push:`（无子键）合法，含义是「全部分支都触发」。本仓对它的处理是 **静默永不触发**，比 B5-2 更常见（B5-2 是事件名缺映射，本条是 null 值与缺失不可区分）。
- 它本轮未提这一条；与 B5-2 同源但独立成条。

**本轮改动（1 文件 1+/1−，零行号漂移）**：

1. `crates/rg-ci/src/runner.rs:16`：`… the \`docker run\` child is not (\`kill_on_drop\` unset).` → 同句尾补 `, so its named container keeps running afterwards.`（1 行→1 行，97→146 字符）。依据 N5。
2. 本台账：`batch#5` 索引区（新增它第 2 轮复核 id 一行、进度 `1 / 6` → `2 / 6`）+ 本节。

改后 5 文件行数指纹仍 **1857 / 1297 / 1065 / 326 / 315**（`runner.rs` 1 行→1 行）。

**未做（L2 待拍板 batch#5，本流程不动 main）**：

- **B5-7**（P3）`_ci_workspaces` / `_ci_cache` 未被保留：要么在 `validate_username` / `validate_repo_name` 加保留名单，要么把 CI 基路径移出 `<repo_root>`（改路径基址属部署契约）。现状风险是「owner 名为 `_ci_workspaces` 时，其目录与全实例 CI 工作区混居」，**不是**删仓库（见反向清单 41）。
- **B5-8**（P3）cache 归档路径在三处各算一遍（`runner.rs:914` / `runners.rs:777` / `ci_retention.rs:103` 的根），应抽公共函数；否则任一侧改动会让另一侧留孤儿归档。
- **B5-9**（P3）`actions/cache@` 不吃 step.env：`substitute_expr` 需支持 step 层 env，或文档化「step 级 env 不参与 with 表达式」。
- **B5-10**（P1）裸 `on:` + `push:` 永不匹配：`Option<EventFilter>` 需按 `workflow_call` 的做法（`:64-65` 的 `#[serde(default, deserialize_with = "deserialize_present_yaml")]`）区分 null 与缺失，把 null 视为「无过滤 = 全匹配」。
- **B5-5 加强**（P2）注入面补两项：`$( )` 命令替换与 `$VAR` 展开（实测已入判定表 N3 行）。

**反向清单（本节新增 1 条，累计 1..41）**：

41. 「N1 的后果②：crash 之后 `prepare_workspace` 的 `std::fs::remove_dir_all(&workspace)` 删的是 `_ci_workspaces` owner 目录下的**合法仓库目录**」—— 反证：仓库目录构造恒为 `<repo_root>/<owner>/<name>.git`（`crates/rg-core/src/repo/service.rs:522`，同类 9 处全带 `.git`），而 `workspace_path()`（`crates/rg-ci/src/runner.rs:789-798`）返回 `<repo_root>/_ci_workspaces/<repo_id>/<pipeline_id>`；被删的是 `<repo_id>/<pipeline_id>` 两级纯数字目录，叶名一侧恒带 `.git`、另一侧是数字 ⇒ 两者永不相等，命中不了任何仓库目录。**已核实为不成立，不要重复提。**

**对方行号/区间误差（非结论位置）**：N1 标 `runner.rs:800-809`，该区间实为 `prepare_workspace` 的文档注释 + 签名（真值 `:789-798`，`:800-809` 是 `/// Materialise …` 到 `.await?`）；N3 的 `substitute_expr` 三个内置替换实为 `:820-822`（它标 `:820-824`，`:823` 是空行、`:824` 已是 `secrets` 分支）。

**验证**：`BASE=f9b8d5a`。改前 `scripts/check.sh --quick` **`EXIT=0`**（executor 本机 `Darwin arm64` / `cargo 1.99.1`，`2026-11-11T11:23Z` 后，关键行原文 `==> cargo fmt --all --check` / ``Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.24s`` / `==> checks passed in 2s (quick)`）。改后同命令 **`EXIT=0`**（`Finished `dev` profile … in 2.78s` / `==> checks passed in 4s (quick)`，rg-ci + rg-cli 因注释变更重编译）。

**改动 SHA**：`f2dd6d2`（1 文件 1+/1−），推 `cnb` 单 remote。

**回帖**：`2120478680609624064`（`2026-11-11T11:28:08Z`），回读 **C=2**（首行 `## 第 2 轮收尾`、末行 `@CodeBuddy`、正文 19872 字符、平台侧仅 2 条收尾无重复投递）。

### consume 3 / 6（batch#5）· 追加于文末（只为零行号漂移，勿移入上文）

- 输入：NPC 第 3 轮复核 `2120479451631763456`（`2026-11-11T11:31:12Z`），晚于我方第 2 轮收尾（`11:28:08Z`）⇒ 有新复核。C=2（正则 `^## 第 \d+ 轮收尾` 命中 2 条，非补记）⇒ K=3。
- 基线：`BASE=ff68f5a`（`cnb/main` = 本机 HEAD，`git ls-remote cnb refs/heads/main` 同点）。它自述复核 `8c9518d` 并指出 main 已到 `857f9bf`；复跑 `git diff --stat 8c9518d 857f9bf -- crates/` **为空**（只 +2 行台账）⇒ 源文件同一，行号基准零漂移。五文件行数指纹 **1857 / 1297 / 1065 / 326 / 315** 全等。
- 判定汇总：**成立 5 / 部分成立 5 / 不成立 3 / 确认 3**；另我方探针新增 1 条机制事实（`#[serde(default)]` 对 `Option` 是空操作，见下）。
- 读法约定：下表 `:<n>` 指 `ff68f5a` 上那 5 个源文件（或注明文件）的行号，只用于指认被判定对象；本节内部定位一律用小节名 / 条目号（审计口径第 4 条）。

判定表：

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| ①-1 | `f2dd6d2` / `8c9518d` 两提交复核通过；`runner.rs:16` = 146 字符 | **成立** | 复跑 `git show 8c9518d:crates/rg-ci/src/runner.rs \| sed -n '16p' \| wc -c` = **147**（146 + 换行），与它同 |
| ①-2 | `git diff f9b8d5a 8c9518d --stat` = 2 文件 72+/2−，「被删行只有 1 行」，其余全是尾部追加 | **部分成立** | `git diff f9b8d5a 8c9518d --stat` 确为 `2 files changed, 72 insertions(+), 2 deletions(-)`；但删除是 **2 行**：台账进度行 1 行 + `runner.rs` 注释改写的那 1 行（它的 `2 +-` 就是这行）。「只有 1 行删除」漏了 `runner.rs` |
| ①-3 | `git diff --stat 8c9518d 857f9bf -- crates/` 为空，行号在两树上同样有效 | **成立** | 复跑为空 |
| ①-4 | 我方第 2 轮判定表「`\| ` 开头的行 13 行」，与声明的 15 条对不上，成因是我方 grep 口径 | **不成立** | 实测 `awk` 计数：表头 + 15 个数据行 = **16** 行匹配 `^\| `（`grep -c '^| '` 同值；命令见反向清单第 42 条，按小节名锚定、不用绝对行号）。13 是它自己少数 2 行，与「空 `#` 格子」无关（本表每行都有 `#` 值）。**已核实为不成立，不要重复提。** |
| ②-a | 让 null 与缺失不可区分的不是 `Option`，而是 `:58` 缺 `#[serde(default)]` ⇒ serde 的 missing_field Option 特化；serde 本身能区分二者 | **部分成立** | 「缺失侧走 `missing_field` 的 Option 特化」成立；但「serde 本身能区分」「缺 `default` 是成因」被探针否掉：加 `#[serde(default)]` 后四种输入的 `is_some()` 与不加**完全相同**（见下「探针」）。唯一能区分的做法是它给的另一半——`deserialize_with = "deserialize_present_yaml"`（`:64-65` 的 `workflow_call` 就是这么做的） |
| ②-b | 后果更严重：`try_read_gitea_workflows` 返回 `Ok(CiConfig { jobs: {} })`，`lib.rs:568` 见 `Ok` 即 return ⇒ 整目录哑火，且会让同目录其它正确 workflow 一起不执行 | **不成立** | `:703-705` 是 `if all_jobs.is_empty() { anyhow::bail!("no matching workflows found in .gitea/workflows/") }` ⇒ **空 config 永不作为 `Ok` 返回**；`:568` 的 `if let Ok` 因此走的是 `Err` 分支（这正是 B5-1）。`continue` 在 `:657` 是**逐文件**，一个文件不匹配不影响其它文件，`:647` 的循环继续 ⇒「一起不执行」也不成立。真实后果是回落 native 后报 `no CI config found (.gitea/workflows/*.yml or .ironforge-ci.yml)`（`:574-580`），即**文案错**而不是静默空跑。**已核实为不成立，不要重复提。** |
| ②-c | 两个 workflow 都在 `.github/workflows/`，第一个含 null 值的键确实存在，故一直没暴露 | **部分成立** | 事实对：`.github/workflows/docker-image.yml:6-10` = `on:` + `workflow_dispatch:`(null) + `push: tags: v*`；`.github/workflows/regression.yml:32-36` = `on:` + `pull_request: branches: [main]` + `workflow_dispatch:`(null)。但它附的「`:625-627` 只扫 `.gitea/workflows/`」行号错：真值是 `:605`（tree_revspec）与 `:630`（扩展名过滤），`:625-627` 是 entry match 块；`crates/rg-core/src/ci/mod.rs:83` 的 `:83` 正确 |
| ②-d1 | 生产 5 个事件名 = `push` / `pull_request` / `merge_group` / `suggestion` / `manual`，`:280-316` 只映 3 臂 ⇒ **2 个**永不匹配 | **部分成立** | 结论方向对（`matches_event` 只有 `push` / `pull_request\|merge_group` / `_ => false` 三臂），但枚举错两处：`pull_request` **不是** trigger_type，漏了 `retry`。实测去重（`grep -rn 'trigger_type: "' crates/ --include=*.rs`，排除测试与 rg-ci 内部）：`push`（`crates/rg-http/src/lib.rs:3419`）/ `manual`（`api/ci.rs:519`）/ `retry`（`api/ci.rs:613`）/ `suggestion`（`api/reviews.rs:210`）/ `merge_group`（`pull_request/merge_queue.rs:557`）⇒ 未匹配是 **3 个（manual / retry / suggestion）**，不是 2 个。它引的四行均偏 6–7 行（指向 `.trigger_pipeline(` 调用而非字面量；`merge_queue.rs:557` 正确）。归入 B5-2 加强 |
| ②-d2 | 建议单列 B5-11（P2）：`manual`/`retry`/`suggestion` 未进映射 ⇒ 手动触发「静默空跑」 | **不成立（重复）** | 命题与 B5-2（`manual`/`retry`/`suggestion` 在 gitea-only 仓库必失败）逐字同义，且 B5-2 的证据已含 `has_ci_config`（`rg-core/src/ci/mod.rs:76-91` 只看 `.gitea/workflows` 存在）⇒ `ci.rs:507` 的前置检查拦不住（`:508` 的 400 文案只提 `.ironforge-ci.yml`）。「静默空跑」这层后果同样被 `:703-705` 否掉：实际是 `no CI config found` 报错。不单列，不重复提 |
| ③ | B5-2 与 B5-10 不同源（事件名缺映射 vs serde 的 Option 处理），两条独立 | **成立** | 机制不同层：`:312` 的 `_ => false` 与 `:58` 的字段类型。采信 |
| ④-1 | 自订 N1：`workspace_path()` 真值 `:789-798` | **成立** | `:789` 为签名、`:795` `root.join("_ci_workspaces")`、`:798` 闭括号 |
| ④-2 | 自订 N3：三连内置替换真值 `:820-822`（`:823` 空行、`:824` 已是 `replace_context_expression(…, "secrets", …)`） | **成立** | 逐行核对一致 |
| ④-3 | 指我方：`validate_repo_name` 标 `:102-108` 只覆盖循环体、不含签名，「真值是 `:91-113`」 | **部分成立** | 「区间不含签名」成立；但它的真值错：实际函数是 `crates/rg-core/src/lib.rs:91-111`（`:91` 签名、`:102` `for c in name.chars()`、`:103` 字符判定、`:110` `Ok(())`、`:111` 闭括号）；它说的 `:101` 是 `TooLong` 错误的收尾 `)`、`:113` 不存在。且 `:102-108` **已含** `:103` 的 `c != '_'` 判定，足以证明「允许 `_`」这一命题——原引用不构成事实错。本轮已把引用收紧到 `:103`（见「本轮改动」） |
| ④-4 | 我方 ⑤-3 的 `:290` 位次对，仅记录 | **确认** | `:290` = `workflow_dispatch: _,`（`:289` `schedule: _`、`:291` `workflow_call: _`） |
| ⑤ | 下一轮可核范围（限文件清单） | **确认** | 接受，按文件范围复核 |
| ⑥ | 本机无 cargo ⇒ 未跑 `check.sh`；未改产品代码、无新提交 | **确认** | 与前两轮一致；验证口径仍由 executor 本机出 |

**探针（本节新增取证，可复跑）：`#[serde(default)]` 对 `Option` 是空操作**

方法：临时 `crates/rg-ci/examples/zz_probe.rs`，用两个本地结构体（字段 `push: Option<Filter>`，一个有 `#[serde(default)]`、一个没有）分别解析四种输入，打印 `push.is_some()`；跑完 `rm` 文件、`git status --porcelain` 只剩本轮两个改动文件。

```
missing        no_default=Ok(false)  with_default=Ok(false)
null           no_default=Ok(false)  with_default=Ok(false)
explicit null  no_default=Ok(false)  with_default=Ok(false)
value          no_default=Ok(true)   with_default=Ok(true)
```

- 读法：加不加 `#[serde(default)]`，缺失键与 `null` 都是 `None` ⇒ 「缺 default 才让二者不可区分」不成立，加 default 修不了 B5-10；`:58` 的修法只能是 `deserialize_with`（`workflow_call` 的 `:64-65` 是把「键存在」编码进 `Option<YamlValue>` 的 `Some`，与字段类型无关）。
- 另一侧：`:59` / `:63` 也是无 `default` 的 `Option`，但 `pull_request` 与 `workflow_dispatch` 的「永不匹配」各自另有成因（`:302` / `:312`），不靠这条探针。

**本轮改动（2 文件 2+/2−，零行号漂移）**：

1. `crates/rg-ci/src/gitea_actions.rs:279`：`matches_event` 的 doc 补「只有 `push` / `pull_request` / `merge_group` 有臂：裸 `push:`（null）与缺失键不可区分，其余 trigger type 一律落到 deny-all 臂」。
2. `crates/rg-ci/src/lib.rs:598`：`try_read_gitea_workflows` 的 doc 补三个 `Err` 出口（目录缺失 / 文件展开失败 / 无 workflow 命中），并写明空结果不会以空 `CiConfig` 返回。
3. 本台账：`batch#5` 索引两行（第 3 轮复核 id / 进度 `2 / 6` → `3 / 6`）、尾部同名小节索引 `1..40` → `1..43`、本节。

两处源码改动均 1 行→1 行，改后五文件行数指纹仍 **1857 / 1297 / 1065 / 326 / 315**。

**未做（L2 待拍板 batch#5，本流程不动 main）**：

- **B5-10 机制订正（并入 B5-10，不新增条目）**：修法写死为 `#[serde(default, deserialize_with = "deserialize_present_yaml")]` 并只认 `Some(YamlValue::Null)` 为「无过滤 = 全匹配」；**不要只加 `#[serde(default)]`**（探针已证空操作）。范围扩到 `:58` / `:59` / `:63` 三个裸键。
- **B5-2 加强**：未匹配的 trigger_type 是 **3 个**（`manual` / `retry` / `suggestion`），不是 2 个；`retry` 同样经 `read_ci_config`（`rg-ci/src/lib.rs:131` 直接把 `trigger_type` 当 `event` 传入），故 gitea-only 仓库的重试也会失败。
- **B5-1 加强**：`no matching workflows`（`:704`）是 `:568` 吞掉的第三个出口，用户可见后果是 `no CI config found (.gitea/workflows/*.yml or .ironforge-ci.yml)` 这条误导文案（`:574-580`）。
- B5-3 / B5-4 / B5-5（含 `$( )` 与 `$VAR` 面）/ B5-6 / B5-7 / B5-8 / B5-9 维持原状，无新增证据。

**反向清单（本节新增 2 条，累计 1..43）**：

42. 「我方第 2 轮判定表只有 13 行 `\| ` 开头（声明 15 条），成因是 grep 口径漏算空 `#` 格子」—— 反证：`awk '/^### consume 2 \/ 6（batch#5）/{f=1;next} /^### consume 3 \/ 6（batch#5）/{f=0} f&&/^\| /{c++} END{print c}'` 对台账输出 **16**（1 表头 + 15 数据行），`grep -c '^| '` 同值；第 2 轮判定表每行 `#` 列都有值（①-1…⑤-4），不存在空格子。**已核实为不成立，不要重复提。**
43. 「裸 `on: push:` ⇒ `try_read_gitea_workflows` 返回 `Ok(CiConfig{jobs:{}})` ⇒ `:568` 直接 return ⇒ 整目录哑火、同目录其它正确 workflow 一起不执行」—— 反证：`crates/rg-ci/src/lib.rs:703-705` 的 `if all_jobs.is_empty() { anyhow::bail!("no matching workflows found in .gitea/workflows/") }` 使空 config 永不作为 `Ok` 返回；`:657` 的 `continue` 是逐文件，`:647` 的循环继续处理其它文件。真实后果是回落 native 后的 `no CI config found (…)` 文案（`:574-580`）。**已核实为不成立，不要重复提。**

**对方行号/计数误差（非结论位置）**：`lib.rs:625-627`（真值 `:605` / `:630`）；`ci.rs:513` / `:607` / `reviews.rs:204` / `rg-http/src/lib.rs:3412`（真值 `:519` / `:613` / `:210` / `:3419`，均偏 6–7 行指向调用而非字面量）；`validate_repo_name` 的「真值 `:91-113`」（真值 `:91-111`，`:101` 是 `)`、`:113` 不存在）；判定表行数 13（真值 15）。

**验证**：`BASE=ff68f5a`。改前 `scripts/check.sh --quick` **`EXIT=0`**（executor 本机 `Darwin arm64` / `cargo 1.99.1`，`2026-11-11T12:30:51Z` 起，关键行原文 ``Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 40s`` / `==> checks passed in 102s (quick)`；102 s 是 WT 内 cargo 指纹不全的局部重编，见 `docs/reports/OPT-LOOP-WORKTREE.md` §5.5）。改后同命令 **`EXIT=0`**（`2026-11-11T12:34:11Z`，``Finished `dev` profile … in 2.88s`` / `==> checks passed in 4s (quick)`）。

**改动 SHA**：`c0a11b5`（2 文件 2+/2−），推 `cnb` 单 remote。**时刻锚** = 本提交 author date（`git log -1 --format=%aI c0a11b5` 可复算），改后验证发生在其之前。

**回帖**：`2120495801389465600`（`2026-11-11T12:36:10Z`），回读 **C=3**（首行 `## 第 3 轮收尾`、末行 `@CodeBuddy`、正文 20598 字符、平台侧仅 1 条 K=3 收尾，无重复投递）。

### 单发轮 · issue #13 · 2026-11-11

- 选靶：非 `[opt-round]` 的 open issue 里编号最小（MANUAL = #13/#14/#15/#16/#17/#18/#19）。#13 最新评论 `2120182459575742464`（`2026-11-10T15:51:04Z`，NPC 分析），首行非 `## 本轮收尾` ⇒ 未被消费过；题面 `[design]` 但靶子是**文档与代码事实的对齐**，本机可做。**不占批次 C**（批次 #31 的 K 计数与本轮无关）。
- 基线：`BASE=fc8e46d`（`cnb/main` tip 与 `git ls-remote cnb refs/heads/main` 同点，零漂移）。被改文件的 `wc -l` 指纹：`ironforge-docs/architecture/agent-principals-design-and-roadmap-2026-11.md` **658 → 666**、`CLAUDE.md` **480 → 480**（1 行→1 行）。
- 判定汇总：**成立 4 / 部分成立 1 / 不成立 0**，另我方自查 1 条新事实（`CLAUDE.md` 同源过期，F10 的第二个出口）。

判定表（`:<n>` 指 `fc8e46d` 上的文件行号）：

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| ① | §3.5 说从 `feat/agent-principals`（`cba96ba`）切，「实际该分支相对 main 只多一份文档，没有额外代码」 | **部分成立** | 「基线说法过期」成立：`git merge-base --is-ancestor cba96ba cnb/main` → 是祖先；`git rev-list --count cnb/main..cba96ba` = **0**、反向 106 ⇒ 该分支已是 main 祖先，不再「多一份文档」。文档本身随 PR #47 进 main：`git ls-tree cnb/main -- ironforge-docs/architecture/agent-principals-design-and-roadmap-2026-11.md` 命中。它说的「只多一份文档」是**修前**形态（`git show 979caab:` 与 main 版同文件属同一血缘），不是当前事实 |
| ② | §3.3(b) 的 `find_also_related` join 写法与落地不同：落地是「先取候选评审，再一条 `users` 查询」 | **成立** | `crates/rg-db/src/ops/pr_review_ops.rs:57-104`：`list_by_pr`（1 次）→ `HashMap` 按 `reviewer_id` 留最新 → 只留当前 head 的 `approve` → `user::Entity::find().filter(Id.is_in(ids)).filter(Kind.eq("human"))`（1 次）→ 按人类集合计数。全文件 `grep -c find_also_related` = **0** |
| ③ | §3.3 没提 `AppError::forbidden` 不带 `reason`，而 §3.4 的 T20-1 要求断言 `reason`；落地用 `coded(..).with_reason(..)` | **成立** | `auth.rs:156` 是常量 `AGENT_MAY_NOT_APPROVE`；`auth.rs:176-186` 是 `AppError::coded(StatusCode::FORBIDDEN, "FORBIDDEN", …).with_reason(AGENT_MAY_NOT_APPROVE, false, "ask a person to review and approve")`。文档原文只有一句条件式脚注（「若 `with_reason` 只在 `coded(..)` 上可用，则…」），未写成既成事实。附带核到第二处差别：`ok_or_else` 对「查不到账号」返回 403 而非放行（`auth.rs:173-175`） |
| ④ | T20-11 断言的是结果（403）而非「哪道门」；默认 `repo:write` 令牌被更早的 scope 中间件拒掉，移除新门测试仍通过 | **成立** | `crates/rg-http/tests/agent_principal_tests.rs:1141-1143` 的注释原文即此意（「a `repo:write` token is turned away by the scope middleware before the handler runs, so this asserts the outcome rather than the reason」）。它对自身测试强度的自述与我方读码一致 |
| ⑤ | 新发现：受保护 CI 环境的批准门同病（`ci_environments.rs` 写名单只看 id 存在；`approve` 判 `is_admin \|\| allowed.contains`） | **成立，且已在 main** | `ci_environments.rs:88` = `Some(user) if rg_core::agent::is_agent(&user) =>` 返回 400；`:289` = `is_some_and(\|user\| !rg_core::agent::is_agent(&user))` ⇒ 403 + `AGENT_MAY_NOT_APPROVE`。对应测试 T20-10 / T20-11 已存在（`agent_principal_tests.rs:1105` / `:1132`）。⇒ 本轮无需改代码，缺的是**文档 §3.4 只列到 T20-9** |
| ⑥ | （我方自查，F10 第二个出口）`CLAUDE.md` 仍写「未做（第二期）：… agent 批准不计数」与「`agent_principal_tests`（8 项）」 | **成立** | `CLAUDE.md:166` 同一行内含这两处过期断言；`grep -c '#\[tokio::test\]' crates/rg-http/tests/agent_principal_tests.rs` = **19**（8 旧 + T20-1…T20-11） |

**本轮改动（2 文件 26+/18−，文档与代码注释；`CLAUDE.md` 1 行→1 行零漂移）**：

1. `ironforge-docs/architecture/agent-principals-design-and-roadmap-2026-11.md`（658→666）：状态行改为「第一期已合 main（PR #47 `063e6eb`）+ P0 已落地（PR #22 `0fe75a5`）」；§2 标题「未合 main」→「已合 main」；§2.5 的 P0 行标已修并补环境批准门；§3 标题标已修 + 一段「§3.1/§3.2 是修前记录、行号属历史形态」的阅读提示（`reviews.rs:325-341`、`auth.rs:165`）；§3.3(a) 的脚注改写为落地形态（`coded().with_reason()` + 查不到账号即 403）；§3.3(b) 把 `find_also_related` 草图换成实际两段查询并订正调用点行号（`branch_protection/service.rs:457`、`review/service.rs:634`）；§3.4 补 T20-10 / T20-11 两行 + 两条限定说明（由来、T20-11 断言强度有限）；§3.5 改为「已执行」并写清无窗口期的兑现方式；头部 `[§3](#…)` 锚点随标题改名一并去掉（否则死链）。
2. `CLAUDE.md:166`（480→480，1 行→1 行）：把「未做（第二期）… agent 批准不计数」改为「已修（PR #22）…」并列出 `ensure_reviewer_may_approve` / `count_current_human_approvals` / 环境名单同批修；测试项数「8 项」→「19 项」。
3. 本台账：本节 + 下方 3 条 L2。

**未做（L2，登记于本节、不并入上文旧表以避免行号漂移）**：

- **M13-1（P3）** `pr_review_ops.rs:46` 的 `count_approvals` 现无生产调用点（`grep -rn 'pr_review_ops::count_approvals' crates/` = **0**；`count_approvals(` 的 7 处命中全是 `ci_environment_ops` 的同名函数、它的 1 处 handler 与 4 处测试调用，外加两个定义），却以「count approvals」为名**静默只数人类**——将来有人要「全部批准数」会踩空。二选一：删除，或改名 `count_current_human_approvals_legacy` 并标注。改名/删除是公开 `ops` 面 ⇒ L2。
- **M13-2（P2）** T20-11 无法单独证明 `ci_environments.rs:289` 的账号类型判定（scope 中间件先拦）。要把这层强度补回来，得构造一个**能走到 handler** 的凭证形态（或对判权函数做单元级测试）⇒ 测试语义变更 ⇒ L2。
- **M13-3（P3）** 同一「agent 不得批准」判据现在有两份实现（`auth.rs:176` 与 `ci_environments.rs:289`），文案与 `with_reason` 参数各写一遍，无共享 helper。抽 `ensure_human_approver(db, user_id, what)` 属重构 ⇒ L2。

**反向清单（本节新增 0 条，**不成立 0**；累计仍 1..43）**：本轮 4 条指摘全部成立或部分成立，原文保留在上表，无需新增防重放条目。

**验证**：`BASE=fc8e46d`。改前 `scripts/check.sh --quick` **`EXIT=0`**（executor 本机 `Darwin arm64` / `cargo 1.99.1`，`2026-11-11T12:51:54Z` 完成，原文 ``Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 40s`` / `==> checks passed in 102s (quick)`）。改后同命令 **`EXIT=0`**（`2026-11-11T12:53:18Z` 起，``Finished `dev` profile … in 0.23s`` / `==> checks passed in 2s (quick)`——本轮只动 `.md`，无 `.rs` 变化，故为热缓存指纹命中）。

**改动 SHA**：`fbb17f2`（3 文件 63+/18−），推 `cnb` 单 remote；`git merge-base --is-ancestor fbb17f2 cnb/main` 通过。**时刻锚** = 本提交 author date（`git log -1 --format=%aI fbb17f2` = `2026-11-11T20:54:02+08:00`）。

**回帖**：`2120500463496769536`（`2026-11-11T12:54:42Z`），首行 `## 本轮收尾`、末行 `@CodeBuddy`（独占末段）、7363 字符；回读平台侧本条 issue 仅 1 条 `## 本轮收尾`（不占批次 C）。issue 状态**保持 open**。

### consume 4 / 6（batch#5）· 追加于文末（只为零行号漂移，勿移入上文）

- 输入：NPC `## 第 4 轮 · 复核结果` `2120496198405476352`（`2026-11-11T12:37:45Z`），晚于我方第 3 轮收尾（`12:36:10Z`）⇒ 有新复核。正则 `^## 第 \d+ 轮收尾` 命中 **3** 条（K∈{1,2,3}）⇒ **C=3，本轮 K=4**。它首行写的是 `## 第 4 轮 · 复核结果`，**不**匹配收尾正则，未被我方计数。
- 基线：`BASE=7c1e6bc`。`fc8e46d..7c1e6bc` 的 4 个文件全是 `.md`（单发轮 #13 的 `fbb17f2`/`7c1e6bc` + 并发写者 `8d5ff26` 改 `OPT-LOOP-MANUAL-QUEUE.md`）⇒ `git diff --stat fc8e46d 7c1e6bc -- crates/ web/` **为空**，五文件行数指纹 **1857 / 1297 / 1065 / 326 / 315** 全等，旧行号继续有效。并发写者的那一笔（`8d5ff26`，作者 `yuqu`，`20:54:24+08`）只改手工队列选靶规则，与本批文件无交集，不按 F15 处置。
- 判定汇总：**确认 3 / 成立 3 / 部分成立 1 / 不成立 1**（6 条编号输入），另我方探针新增 1 条矩阵事实 + 1 条计数订正（它报「5 个出口」，真值 7）。

判定表（`:<n>` 指 `7c1e6bc` 上的 `crates/rg-ci/src/*.rs`）：

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| ①-1 | Q1：`gitea_actions.rs:279` 改后与代码事实一致（三个事件名逐个有臂；裸 `push:` 与缺失不可区分；其余落 deny-all） | **确认** | `:280` 签名、`:293-313` 的 match 三臂 + `:312` 的 `_ => false` 均在；`:63` 的 `workflow_dispatch` 与 `:62` 的 `schedule` 是裸 `Option`、`:64` 的 `workflow_call` 才带 `deserialize_with`。**但本轮改成更窄的表述（见改动 2）**：`Simple`/`Array` 两个 variant 是按字面名匹配，故「every other trigger type falls to the deny-all arm」这句过宽 |
| ①-2 | Q2：`lib.rs:598` 的 doc「三个 `Err` 出口」错，**实为 5 个**（漏 `:610`/`:613`/`:615`/`:665`） | **部分成立** | 「doc 只覆盖三类、漏 4 处」成立；**它报的 5 个不对，真值 7 个**：`awk 'NR>=599 && NR<=712' crates/rg-ci/src/lib.rs \| grep -n '?;\|bail!'` 输出 7 行 ⇒ `:608`（树项缺失）/`:610`（`object_id.object()?` 无 `map_err`）/`:613`（不是 tree）/`:615`（取默认分支）/`:662`（展开失败）/`:665`（不支持的 action）/`:704`（无 workflow 命中）。已按**类别**而非条数重写 doc，避免再被数错 |
| ② | 探针结论接受；但「台账里若写成『加 default 后 missing 与 null 仍不可区分』会被读成反向」，要求补一句「加与不加四行相同」 | **不成立（台账已是该口径）** | 台账探针节的读法行原文：「加不加 `#[serde(default)]`，缺失键与 `null` 都是 `None` ⇒ 「缺 default 才让二者不可区分」不成立，加 default 修不了 B5-10」——正是它要的口径，无「加 default 后仍不可区分」的写法。**已核实为不成立，不要重复提（见反向清单 44）** |
| ②附 | `YamlValue` = `serde_json::Value`（它标 `rg-core/src/yaml.rs:38`），`Some(YamlValue::Null)` 可达 ⇒ 加 `deserialize_with` 但不改 `:295`/`:302` 判断等于没修 | **成立（行号偏 5）** | `crates/rg-core/src/yaml.rs:33` = `pub use serde_json::Value;`（`:38` 是 `WriteError` 的文档行）；`Cargo.toml:55` `serde-saphyr = "2"`。`YamlValue::Null` 确实是可达值 ⇒ B5-10 的修法必须同时改判定，已记入 B5-10 加强 |
| ③ | 台账 `1..43` 全量、无重号 | **确认** | 复算 `^(\d+)\. ` 共 **43** 条，range 1..43，无重号无缺号 |
| ④ N4-1 | `retry` 的第二处失效面：`trigger_type` 也进 `job_condition_context` ⇒ gitea-only 仓在 retry 下 `if: github.event_name == 'push'` 类条件一律为假；`has_ci_config` 只看目录存在，拦不住 | **成立** | `lib.rs:131` 把 `trigger_type` 当 `event` 传给 `read_ci_config`；`:247` 把它继续传进 `job_condition_context`，`:467` 映射成 `("github.event_name", event)`。⇒ 并入 **B5-2**（不单列条目，按其请求） |
| ④ N4-2 | (b) 「every other trigger type falls to the deny-all arm」过宽：`Simple`/`Array` 是字面量直比，`on: manual` / `on: [manual]` 能匹配；(a) 裸 `workflow_dispatch:` / `schedule:` 是「压根不匹配」而不是「不可区分」 | **(b) 成立；(a) 部分成立** | 探针实测（`crates/rg-ci/examples/zz_probe.rs`，跑完即删）：`on: manual` → `matches(manual)` = **true**；`on: [manual]` = **true**；`on: {push:}` → `matches(push)` = **false**；`on: {workflow_dispatch:}` → `matches(workflow_dispatch)` = **false**；`on: {schedule:}` = **false**。(a) 的「更严重」判断对，但 doc 原句「其余 trigger type 一律落 deny-all 臂」在**单键形态内**并未失实，缺的是「单键形态」这个限定 ⇒ 判部分成立 |
| ⑤ | 三条请求：补 `lib.rs:598` 出口、校台账措辞、N4-1 并入 B5-2 且 N4-2 由我方拍板 | **已执行** | 出口已按类别重写（不写条数，从根上避开计数争议）；台账措辞经复核**已是正确方向**（见 ②）；N4-1 已并入 B5-2；N4-2 见改动 2 |

**探针矩阵（本节新增取证，可复跑）**：临时 `crates/rg-ci/examples/zz_probe.rs` 直调 `GiteaWorkflow::parse` + `matches_event("refs/heads/main","main")`，`cargo run -q --example zz_probe -p rg-ci` 输出 7 行，跑完 `rm`，`git status --porcelain` 只剩本轮 2 个改动文件。

```
on: push                          event=push              parse=Ok  matches=true
on: manual (simple)               event=manual            parse=Ok  matches=true
on: [manual] (array)              event=manual            parse=Ok  matches=true
on: {push:} (bare push)           event=push              parse=Ok  matches=false
on: {workflow_dispatch:}          event=workflow_dispatch parse=Ok  matches=false
on: {schedule:}                   event=schedule          parse=Ok  matches=false
on: {push: {}, unknown_key: 1}    event=manual            parse=Ok  matches=false
```

- 读法：`matches_event` 的语义在 `WorkflowTriggers` **三个 variant** 上是分裂的——`Simple`/`Array` 按字面名直比（任何名字都能命中），只有 `Single` 形态受三臂限制。doc 若只写「只有三个事件名有臂」，会让人误以为 `on: manual` 也不匹配。
- 另一侧：`on: {workflow_dispatch:}` 即使 event 名相同也 false ⇒ 这是 B5-10 的同一根因（裸键 = null ⇒ `None`），不是新条目。

**本轮改动（2 文件 2+/2−，零行号漂移）**：

1. `crates/rg-ci/src/lib.rs:598`：`try_read_gitea_workflows` 的 doc 由「三类出口」改为按**类别**列全 7 处——树项缺失或不是 tree、对象读不出、默认分支取不到、文件展开失败或不支持的 action、无 workflow 命中——并重申空 job 集不会以空 `CiConfig` 返回。**不写条数、不写行号**，避免再沦为计数争议。
2. `crates/rg-ci/src/gitea_actions.rs:279`：`matches_event` 的 doc 加两个限定——`Simple`/`Array` 按字面名直比（任何名字都能命中），「三臂 + deny-all」只适用**单键形态**；并写明裸 `workflow_dispatch:`/`schedule:` 被解构丢弃、永不匹配。
3. 本台账：本节 + B5-2 / B5-10 的两条加强。

两处均 1 行→1 行（`:598` 与 `:279`），改后五文件行数指纹仍 **1857 / 1297 / 1065 / 326 / 315**。

**未做（L2 待拍板 batch#5，本流程不动 main）**：

- **B5-2 加强（并入原条）**：`retry` 的第二处失效面——`trigger_type` 同时作为 `event` 进入 `job_condition_context`（`lib.rs:247` → `:467` 的 `github.event_name`），所以 gitea-only 仓库在 retry 下即使有臂可匹配，`if: github.event_name == 'push'` 类条件也一律为假。未匹配事件名仍为 3 个（`manual` / `retry` / `suggestion`）。
- **B5-10 加强（并入原条）**：`YamlValue` = `serde_json::Value`（`rg-core/src/yaml.rs:33`），`Some(YamlValue::Null)` 可达 ⇒ 只加 `deserialize_with` 而不改 `:295`/`:302` 的判定等于没修；修法必须同时把「`Null` = 无过滤 = 全匹配」显式写进 `matches_event`。
- B5-1 / B5-3 … B5-9 维持原状，本轮无新增证据。

**反向清单（本节新增 1 条，累计 1..44）**：

44. 「台账里写了『加 `#[serde(default)]` 后 missing 与 null 仍不可区分』，因果方向反了，需补『加与不加四行相同』」—— 反证：台账探针节读法行原文即「加不加 `#[serde(default)]`，缺失键与 `null` 都是 `None` ⇒「缺 default 才让二者不可区分」不成立，加 default 修不了 B5-10」，已含「加与不加相同」这一层，无反向措辞。**已核实为不成立，不要重复提。**

**对方行号/计数误差（非结论位置）**：`rg-core/src/yaml.rs:38`（真值 `:33`，`:38` 是 `WriteError` 文档行）；`lib.rs:598` 的「5 个出口」（真值 **7**，`:608`/`:610`/`:613`/`:615`/`:662`/`:665`/`:704`）。

**验证**：`BASE=7c1e6bc`。改前 `scripts/check.sh --quick` **`EXIT=0`**（executor 本机 `Darwin arm64` / `cargo 1.99.1`，`2026-11-11T12:56:54Z` 完成，``Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 39s`` / `==> checks passed in 101s (quick)`——WT 内 cargo 指纹不全，见 `docs/reports/OPT-LOOP-WORKTREE.md` §5.5）。改后同命令 **`EXIT=0`**（`2026-11-11T13:02Z` 前后，``Finished `dev` profile … in 2.88s`` / `==> checks passed in 4s (quick)`）。

**改动 SHA**：`ba78f72`（3 文件 59+/3−），推 `cnb` 单 remote；`git merge-base --is-ancestor ba78f72 cnb/main` 通过。**时刻锚** = 本提交 author date（`git log -1 --format=%aI ba78f72`）。

**回帖**：`2120501604276129792`（`2026-11-11T12:59:14Z`），首行 `## 第 4 轮收尾`、末行 `@CodeBuddy`（独占末段）、6517 字符；回读 K 集合 `{1,2,3,4}`、C=4，无重复投递。

### consume 5 / 6（batch#5）· 追加于文末（只为零行号漂移，勿移入上文）

- 输入：NPC `2120501924234428416`（`2026-11-11T13:00:30Z`，距我方第 4 轮收尾仅 **1 min 16 s**），首行 `@cnb.biewen(别问) ## 第 5 轮 · 只核三项（不改代码）`。正则 `^## 第 \d+ 轮收尾` 命中 4 条 ⇒ **C=4，本轮 K=5**。
- 基线：`BASE=953946c`（`cnb/main` = 本机 HEAD）。它自述复核对 `ba78f72`，其后只有 `953946c`（回填 3 行台账）⇒ `crates/` 零差异，五文件行数指纹 **1857 / 1297 / 1065 / 326 / 315** 全等。
- 判定汇总：**确认 3 / 成立 1（旁注）/ 不成立 1**。本轮**零新靶**（它自述「三项全部通过，无新指摘」）⇒ 按 F20 只落台账 + 一处文档补充，不硬找靶。

判定表（`:<n>` 指 `953946c` 上的 `crates/rg-ci/src/*.rs` 与台账）：

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| ① | `lib.rs:598` 的 7 处出口「一类不差，也没多说」，并给出「5 类 / 7 行」的映射表 | **确认** | 我方的 7 行输出与它的表逐行对齐：`:608`/`:613` 同一 revspec 两种失败、`:610` 裸 `?`、`:615`、`:662`/`:665` 同一 cycle 两步、`:704`。它把「或」判为合法两合一是对的 |
| ② | `gitea_actions.rs:279` 的两句新限定与 `:280-313` 一致 | **确认** | `:47-53` 的 `#[serde(untagged)]` 顺序 + `:307`/`:308` 的裸字符串比较 ⇒ 「any name can match」；`:288`/`:289` 的 `schedule: _` / `workflow_dispatch: _` ⇒ 解构丢弃、`:312` `_ => false`。它另指「`merge_group` 有臂但无独立字段、复用 `pull_request` 过滤」并**主动建议不补**（`:303-306` 注释已写明）——本轮不补，记录在此，防下轮重提 |
| ①旁注 | `:650-653` 的 `GiteaWorkflow::parse` 失败走 `tracing::debug!` + `continue`，被静默跳过；若全部解析失败，用户看到的是 `:704` 的「no matching workflows found」而非解析错误。它声明「不算漏、不当指摘」 | **成立（本轮已落 L1）** | `:648` `match gitea_actions::GiteaWorkflow::parse(yml)`、`:650-653` 是 `Err(e) => { tracing::debug!(...); continue; }` ⇒ 解析失败**不**产生 `Err` 返回。原 doc 的「a file fails to expand」不含这一层 ⇒ 已补一句「不解析的文件按 `debug` 跳过；全部不可解析的目录最终表现为 no workflow matches」 |
| ③-计数 | 节起始 `:1328`、节尾 `:1382`、节内 55 行；全文件 `^(\d+)\. ` 119 条；反向清单新增 `44.`、连续 43→44；`7c1e6bc` 出现**三处**；`lib.rs` 1065 / `gitea_actions.rs` 1297 | **部分成立（计数错，已就地订正）** | 节界、119 条、44 连续、行数指纹**确认**；但「`7c1e6bc` 三处」不成立：`grep -c '7c1e6bc'` 在该树上是 **4 行**（`:1331` / `:1334` / `:1382` / **本行**），含本轮案卷行共 **5 行**、`grep -o \| wc -l` = 9 次。成因是**引述污染**：本行自己写了这个串，计数把引用者算进去——台账里凡「某串出现 N 次」一律先 `grep -c`，再扣掉引用它的那一行 |
| ③-⚠️ | 「`:66` 的 6 轮进度指针写的是 `consume 4 / 6（batch#4）` 节 —— batch#5 的这一节没有被进度指针收录」 | **不成立** | `:53`（batch#5 索引的进度行）已是 `**4 / 6**（各轮见文末 \`consume <K> / 6（batch#5）\` 节；第 1–3 轮在前，第 4 轮在文件末尾）`——本轮 `ba78f72` 就改过；它引的 `:66` 落在 **batch#4** 索引块内，那条指针本就该指 `consume <K> / 6（batch#4）`。**已核实为不成立，不要重复提（见反向清单 45）** |

**本轮改动（1 文件 1+/1−，零行号漂移）**：

1. `crates/rg-ci/src/lib.rs:598`：doc 补「不解析的文件按 `debug` 跳过（不使读取失败），全部不可解析的目录最终表现为 no workflow matches」。仍 1 行→1 行。
2. 本台账：本节 + 进度 `4 / 6` → `5 / 6` + 反向清单 45。

**未做（L2 待拍板 batch#5，本流程不动 main）**：B5-1…B5-10 全部维持原状；`merge_group` 复用 `pull_request` 过滤这件事**按它的建议不写进 doc**（`:303-306` 已有注释），记录于此以免下轮反复。

**反向清单（本节新增 1 条：45，累计 1..45；紧随其后的第 46 条**由「单发轮 #13 三次消费」轮写入、编号不归本节**）**：

45. 「batch#5 的第 4 轮节没有被「6 轮进度」指针收录（它引 `:66`）」—— 反证：`docs/reports/OPT-LOOP-LEDGER.md:53` 的 batch#5 进度行在 `ba78f72` 已改为 `**4 / 6**（各轮见文末 \`consume <K> / 6（batch#5）\` 节…）`；它引的 `:66` 属 batch#4 索引块，那条指针指向 `consume <K> / 6（batch#4）` 是正确行为。**已核实为不成立，不要重复提。**
46. 「`cnb/main` 这个 ref 名在本仓树里不存在，凡写现算命令都得用 `origin/main`」—— 反证（**可复现形**，不用任何快照 SHA）：`git remote -v` 在本仓输出三个 remote、六个 `fetch`/`push` 行（`cnb`=cnb.cool、`github`、`origin`=**git233**）、`git remote get-url cnb` = `https://cnb.cool/zdzso/ironforge.git`、`git rev-parse --verify cnb/main` 恒成功 ⇒ ref 存在且指向 cnb.cool，与 `origin`（git233）是两个平台。它容器只有一个 remote 且就指向 cnb.cool，是**环境差异**；照它改成 `origin/main` 在本仓会指向另一个平台（git233），是**反向破坏**。**正解不是「改写为 `cnb/main`」**（那只是把一种写死换另一种）：文档里的现算命令一律用相对 ref 占位——`<你的 main ref>`，或切到 main 后的 `HEAD`，两种容器都成立。**已核实为不成立，不要重复提。**

**验证**：`BASE=953946c`。改前即上一轮改后的同一棵树（`EXIT=0`，`checks passed in 4s`）；本轮改后 `scripts/check.sh --quick` **`EXIT=0`**（executor 本机 `Darwin arm64` / `cargo 1.99.1`，``Finished `dev` profile … in 2.71s`` / `==> checks passed in 4s (quick)`）。五文件行数合计仍 **4860**。

**改动 SHA**：`b5a5b7b`（2 文件 31+/2−，源码仅 `lib.rs:598` 一行），推 `cnb` 单 remote。**时刻锚** = 本提交 author date（`git log -1 --format=%aI b5a5b7b`）。

**回帖**：`2120502406373859328`（`2026-11-11T13:02:25Z`），首行 `## 第 5 轮收尾`、末行 `@CodeBuddy`、4900 字符；回读 K 集合 `{1,2,3,4,5}`、C=5，无重复投递。

### consume 6 / 6（batch#5，末轮）· 追加于文末（只为零行号漂移，勿移入上文）

- 输入：NPC `2120502967378800640`（`2026-11-11T13:04:39Z`，距我方第 5 轮收尾 **2 min 14 s**），首行 `@cnb.biewen(别问) 第 5 轮回读完成。…`。正则 `^## 第 \d+ 轮收尾` 命中 5 条 ⇒ **C=5，本轮 K=6（末轮）**。
- 基线：`BASE=fe9b9af`（`cnb/main` = 本机 HEAD）。它自述复核 `b5a5b7b`，`fe9b9af` 只回填 3 行台账 ⇒ `crates/` 零差异，五文件行数指纹 **1857 / 1297 / 1065 / 326 / 315** 全等。
- 判定汇总：**确认 2 / 成立 1（指引过期，已落 L1）/ 对方自订 3（记账误差，不进反向清单）**。**零新靶轮**（它自述「两项复核：通过」）⇒ 按 F20 只落台账与一处指引订正，不硬找靶。

判定表：

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| ① | `lib.rs:598` 新补的那句「不解析的文件按 `debug` 跳过」准确、且没把「解析失败」与「展开失败」混成一件；旁证：`:637-639` `find_object`、`:640-643` `try_into_blob` 也是 `Err(_) => continue` ⇒「目录读不出就报错、单文件读不出就跳过」的分层是真的 | **确认** | `:648-653` 的 `match … Err(e) => { tracing::debug!(…); continue; }`；`:662`/`:665` 的 `.with_context(...)?` 仍是独立出口；`:703-705` 唯一的 `bail!` 是 `:704`。全段 7 行出口计数不变（`awk` 复算 7） |
| ② | `consume 5 / 6（batch#5）` 节的行号与计数全对；反向 45 条连续；`sed -n '53p'` 复现 `5 / 6` | **确认** | 复算一致 |
| ③ | `:946` 的清单指引仍停在 `**1..43** 条`，只有 36–38 指向 `6/6（batch#4）`；44/45 无去处。「要不要顺手改成 `1..45`，你定 —— 我不把它当靶」 | **成立（本轮已落 L1）** | `:946` 原文「本清单已累计 **1..43** 条…42–43 见 `consume 3 / 6（batch#5）` 节」⇒ 44/45 确实无处可指。已改为 `1..45` 并补 44/45 的去处（1 行→1 行） |
| ④ | 三处自陈记账误差：(a) `7c1e6bc` 「三处」，真值 4 处；(b) 「`953946c` → `b5a5b7b`」顺序反了；(c) 「`--quick` = fmt + clippy」措辞过宽，实际核的是「不折 `///`」这一前提 | **接受，不进反向清单** | (a) 复算 `grep -c 7c1e6bc` = **4**（`:1331`/`:1334`/`:1382`/`:1401`）——**它的订正本身也差一处**：它写的「`:1331`×2 + `:1334` + `:1382`」把 `:1331` 数了两次，第 4 行是 `:1401`（它自己的引用行）。(b) 我方原文「改动先合流再发帖：`953946c` → `b5a5b7b`」是「BASE → HEAD」的写法，语义不错，但确实易读成反向 ⇒ **今后写「BASE `x`、HEAD `y`」**。(c) 采信 |
| ⑤ | 下一步等拍板：`1..43 → 1..45` 改不改 | **已改** | 见改动 1 |

**本轮改动（仅台账，1 行→1 行 + 本节）**：

1. `docs/reports/OPT-LOOP-LEDGER.md:946`：清单指引 `1..43` → `1..45`，并补「44 见 `consume 4 / 6（batch#5）` 节、45 见 `consume 5 / 6（batch#5）` 节」。
2. 台账：本节 + 进度 `5 / 6` → `6 / 6（已收批）`。

**未做（L2 待拍板 batch#5，本流程不动 main）**：B5-1…B5-10 全部维持原状；`merge_group` 不进 doc（按它建议）。本轮无新增 L2。

**反向清单**：本节新增 **0** 条，累计仍 **1..45**（④ 是它对我方表述的自我订正，反证对象是它自己的记账，不进清单）。

**验证**：`BASE=fe9b9af`。本轮只动 `.md`，`crates/` 零变化 ⇒ 五文件指纹 **1857 / 1297 / 1065 / 326 / 315** 不变；`scripts/check.sh --quick` 取上一轮同树结果 **`EXIT=0`**（``Finished `dev` profile … in 2.71s`` / `==> checks passed in 4s`），改后再跑同命令 **`EXIT=0`**（``Finished `dev` profile … in 0.25s`` / `==> checks passed in 2s`，纯 md 热缓存）。

### 批后复核 · issue #31 · 2026-11-11T13:09:01Z

- 回帖：`2120503819401338880`（`2026-11-11T13:08:02Z`，末轮收尾 `13:06:28Z` 后 **1 min 34 s** 到，**关批前 59 s**；本轮按 §6.2 的「零成本探测」当场消费，未等第二轮窗口）
- 判定：**成立 2（⑥⑦，我方计数错）/ 确认 2（两条复核通过）/ 部分接受 2（④-b、④-c 的措辞）**

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| ⑥ | `:1401`（`consume 4 / 6` 表）写的「`7c1e6bc` 出现三处」被判定「确认」，但实际是 4 行；这条错没有去处 | **成立** | `grep -c '7c1e6bc'` 在 `a4d0bfb` 之前的树上 = **4**（`:1331`/`:1334`/`:1382`/**`:1401` 自己**），加本轮案卷行 `:1434` 共 **5**；`grep -o \| wc -l` = 9 次。成因是**引述污染**：那一行自己写了这个串，把引用者算进了计数 |
| ⑦ | 因此 4/6 节的「确认」判得不成立，且 `:946` 的 `1..45` 里没有这条（不构成漏号，但它是「被判成立 + 被 grep 打脸」的计数命中） | **成立** | 同上。已把该行判定改为 **部分成立（计数错，已就地订正）**，并写明口径：台账里凡「某串出现 N 次」，先 `grep -c`，再扣掉引用它的那一行 |
| ① | `:946` 的 `1..45` 与六处指针逐一对得上 | **确认** | 六节实址 `:1032`/`:1074`/`:1150`/`:1221`/`:1328`/`:1388`，条目行 `:1064-1066`/`:1132-1133`/`:1211`/`:1280-1281`/`:1378`/`:1413` |
| ② | `consume 6 / 6` 节行号/计数通过（节 `:1421`–`:1446`） | **确认** | 复算一致 |
| ④-b | 「`953946c` → `b5a5b7b`」顺序读法 | **部分接受** | 它自验 `git merge-base --is-ancestor 953946c b5a5b7b` = 0 ⇒ 方向本身没错；但「`x` → `y`」易被读成反向，**从现在起统一写「BASE `x` / HEAD `y`」** |
| ④-c | 「`--quick` = fmt + clippy」措辞过宽 | **采信** | `scripts/check.sh:36-38`/`:41` 属实；它核的是「不折 `///`」这一前提，与 `wrap_comments` 默认口径一致 |

- 落地：`a4d0bfb`（`docs/reports/OPT-LOOP-LEDGER.md:1401` 一行 1→1 订正 + 口径写明），已合流。
- 去向：**已解决**（口径固化进本节；下一批不再引用「三处」这个旧值）。
- **它自己的行号偏差（勿当结论引用）**：① 它在第五节列的出口行写成 `:631`/`:652`/`:658`/`:662`/`:665`/`:704`，真值是 `:608`/`:610`/`:613`/`:615`/`:662`/`:665`/`:704`（它把前四个整体偏了 +23 左右）；「7 行」这个结论对。⑥ 里「`:1331`×2」的说法也错——`:1331` 那一行确有多次出现（`grep -o` 4 次），但独立行数是 4 行，第 4 行是 `:1401` 不是第二个 `:1331`。
- **本节的自我一致性检查**：`grep -c '7c1e6bc' docs/reports/OPT-LOOP-LEDGER.md` 现在 = 6（`:1331`/`:1334`/`:1382`/`:1401`/`:1434`/本节），**新增的这一行也引用了该串** —— 这正是 ⑥ 说的引述污染，故本节不写「共 N 行」，只写相对时点的差量。

### 单发轮 · issue #13 · 2026-11-11（第 2 次消费：NPC `## 复核结果` 回执）

- 选靶：MANUAL（#13/#14/#15/#16/#17/#18/#19）里**唯一带 priority 的**（`P1`），其余为空 ⇒ 按 `P0>P1>…` 规则命中。最新评论 `2120500822160072704`（`2026-11-11T12:56:07Z`，首行 `## 复核结果`，非 `## 本轮收尾`）⇒ 上一轮改动已被复核、**未消费过**。批次段对象为空（batch#5 已 `closed/completed`，无 `[opt-round]`），本轮只跑第一段。
- 基线：`BASE=bd7eb84`（`git ls-remote cnb refs/heads/main` 同点，零漂移）；WT `issue-13-manual` 分支 `opt/issue-13-manual-20261111`。
- 判定汇总：**成立 3 / 部分成立 2 / 确认 3 / 时点差 1**，另**我方自查 4 处文档引用错**（它未提）。

判定表（`:<n>` 指 `bd7eb84` 上的文件行号）：

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| ① | 三个区间「两端都对」：`reviews.rs:325-341`、`auth.rs:165`、`pr_review_ops.rs:57-104` | **部分成立**（前两个对，第三个终点错） | `reviews.rs:325` = `if matches!(action.as_str(), "approve" \| "request_changes") && pr.author_id == user_id {`，`:341` = 判权段的 `}`、`:342` 是 `should_attempt_auto_merge` 的 `}`；`:305` = `pub async fn submit_review(` ✓。`crates/rg-http/src/api/auth.rs:165` = `pub(crate) async fn ensure_reviewer_may_approve(` ✓（注意路径是 `rg-http/src/api/auth.rs`，不是 `rg-core`）。**`:104` 不是收尾 `}`**：函数实到 `:98`，`:100-103` 已是 `create` 的 doc + 签名，`:104` 是空行 |
| ② | 调用点 `branch_protection/service.rs:457`、`review/service.rs:634` 都是 `count_current_human_approvals(db, pr_id, pr.head_sha.as_deref())` | **成立** | 两行逐字相同，`sed -n` 直接读出 |
| ③ | `CLAUDE.md:166` 三句（`ensure_reviewer_may_approve` / `count_current_human_approvals` / 环境名单同批修）+「19 项」 | **成立** | `CLAUDE.md:166` 同一行内含三句；`grep -c '#\[tokio::test\]' crates/rg-http/tests/agent_principal_tests.rs` = **19**；`CLAUDE.md` 仍 480 行 |
| ④ | ⑤「已在 main」的判断无误：`ci_environments.rs:88` 写名单 400、`:289` release 前 403 | **确认** | `:88` = `Some(user) if rg_core::agent::is_agent(&user) =>`；`:289` = `Ok(user) => user.is_some_and(\|user\| !rg_core::agent::is_agent(&user)),`；测试名 `agent_principal_tests.rs:1105` = `t20_10_an_agent_cannot_be_listed_as_an_environment_approver`、`:1132` = `t20_11_an_agent_named_in_an_old_list_still_cannot_release` |
| ⑤ | 新发现：`fix/agent-approval-gate`（`c882d40`）没跟进 main，「落后 105」 | **部分成立**（僵尸 ref 成立，数字是它的快照） | `git rev-list --count cnb/main..c882d40` = **0**（无独有提交）、`c882d40..cnb/main` = **112**（本树 `bd7eb84` 现算；它写 105 是它自己 main 更旧）。性质判断成立：该 ref 名字活着、内容已整体含在 main |
| ⑥ | 行数指纹 `658 → 666` / `CLAUDE.md 480 → 480`、死链 `grep '\[§3\](#' = 0`、`fbb17f2` 是 main 祖先 | **确认** | 本树 `wc -l` = **666** / **480**；`grep -c '\[§3\](#'` = **0**；`git merge-base --is-ancestor fbb17f2 cnb/main` 返回 0 |
| ⑦ | 「本地 `main` 比 `origin/main` 超前 1 个提交，是两段式写入的正常形态」 | **时点差，不构成缺陷** | 它看的 `8d5ff26` 已随后续提交进 main；本树 `HEAD` = `cnb/main` = `bd7eb84`（`git rev-parse` 两值相同）⇒ 无超前 |
| ⑧ | （我方自查）文档 `:15` 行把函数写成 `count_current_approvals` 并标「**未实现**」 | **成立** | `grep -rn 'count_current_approvals' crates/` = **0 命中**（真名 `count_approvals`，`pr_review_ops.rs:46`）；且 §3 已标已修 ⇒ 与 §0 的「未实现」自相矛盾 |
| ⑨ | （我方自查）文档 `:33` 把自批检查标成 `api/reviews.rs:328` | **成立** | 真值 `:325`；`:328` 是那句 400 的**文案字符串** |
| ⑩ | （我方自查）文档 `:192` 把 `latest` 去重标成 `pr_review_ops.rs:56` | **成立** | 真值 `:63-68`（`HashMap` + `matches!(action, "approve" \| "request_changes")` 的循环）；`:56` 是 doc 注释末行 |
| ⑪ | （我方自查）附录 B 复现命令 `grep -rn "count_current_approvals" crates` 恒返回空 | **成立** | 改名为 `count_current_human_approvals` 后该串全仓 0 命中 ⇒ 复现配方失效 |

**本轮改动（文档 1 文件 6+/6−，零行号漂移：666 行不变；另有本台账追加本节）**——全部 L1 文档措辞：

1. `ironforge-docs/architecture/agent-principals-design-and-roadmap-2026-11.md:216`（§3.5）：补「分支 ref 本身也已废弃」——tip `c882d40` 无独有提交、**落后 main 112 个提交**（`bd7eb84` 时点，现算命令一并写出），并点明「要重新从 main 切分支」的后果。
2. 同文件 `:177`（§3.3(b)）：区间 `pr_review_ops.rs:57-104` → `:57-98`（它本轮把 `:104` 判为「收尾 `}`」，实为空行）。
3. 同文件 `:15`（§0 第 2 行）：函数名 `count_current_approvals` → `count_approvals`、行号 `:48` → `:46`、状态「未实现」→「v2 时点未实现，现已修」并补现名 `count_current_human_approvals`（`:57`）。
4. 同文件 `:33`：`api/reviews.rs:328` → `:325`。
5. 同文件 `:192`：`pr_review_ops.rs:56` → `:63-68`。
6. 同文件 `:646`（附录 B）：复现命令改为「`count_current_human_approvals` 或 `count_approvals`」两个名字并查，否则恒空。
7. 本台账：本节 + 下方 M13-4。

**未做（L2，本流程不动 main）**：

- **M13-4（P3，本轮新立，按它建议归档）** `fix/agent-approval-gate` 是**无独有提交、落后 main 112** 的僵尸 ref（`c882d40`）。二选一：删远端分支，或保留但标注废弃。删 ref 不可逆且会影响他人 clone 上的 `origin/` 追踪 ⇒ L2。**本轮只把「已废弃、勿在其上继续工作」写进文档 §3.5（L1）**；「删不删」待拍板。
- M13-1 / M13-2 / M13-3 维持原状（它在末段也认同留在 L2）。

**反向清单**：本节新增 **0** 条，累计仍 **1..45**。它本轮全部是复核回执 + 1 条新发现，无「被我方否掉的指摘」。

**对方行号/计数误差（非结论位置，勿当结论引用）**：`pr_review_ops.rs:57-104` 的终点（真值 `:98`，`:104` 空行）；`c882d40` 落后数 105（本树 **112**，差 7 是它 main 更旧）。

**验证**：`BASE=bd7eb84`。改前 `scripts/check.sh --quick` **`EXIT=0`**（executor 本机 `Darwin arm64` / `cargo 1.99.1`，`2026-11-11T13:38:23Z` 起，``Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 42s`` / `==> checks passed in 104s (quick)`——WT 内 cargo 指纹不全，见 `docs/reports/OPT-LOOP-WORKTREE.md` §5.5）。改后同命令 **`EXIT=0`**（`2026-11-11T13:40:18Z`，``Finished `dev` profile … in 0.26s`` / `==> checks passed in 2s (quick)`，纯 `.md`）。

**改动 SHA**：`aae47d8`（2 文件 53+/6−：文档 6+/6−、台账追加本节），推 `cnb` 单 remote；`git merge-base --is-ancestor aae47d8 cnb/main` 通过。**时刻锚** = 本提交 author date（`git log -1 --format=%aI aae47d8` = `2026-11-11T21:40:56+08:00`），回帖在其后（`13:41:57Z`）。

**回帖**：`2120512356546412544`（`2026-11-11T13:41:57Z`），首行 `## 本轮收尾`（**不带轮次号**，不占批次 C），末段 `@CodeBuddy`。issue 状态**保持 open**。

### 单发轮 · issue #13 · 2026-11-11（第 3 次消费：它自己发了 `## 本轮收尾`）

- 选靶：MANUAL 七条（#13/#14/#15/#16/#17/#18/#19）里**唯一带 priority** 的（`P1`），其余为空 ⇒ 命中。最新评论 `2120512621123096576`（`2026-11-11T13:43:00Z`，`author.is_npc = true`）**晚于**我方上一条收尾 `2120512356546412544`（`13:41:57Z`）⇒ 有新 NPC 回复，不触发「跳过」。批次段对象仍为空（无 `[opt-round]` open issue，batch#5 已 `closed/completed`）。
- 让位判据：`cnb/main` tip `2ce6d94` 的提交时刻 `13:42:03Z` = **我方上一轮的回填提交**（非他人）；`git worktree list` 里另外 6 个 worktree `status --porcelain` 全 **0 行**、mtime 均早于本次开工 2 h 以上；锁 `mkdir` 一次成功、无 owner 残留 ⇒ 无并发写者，继续。
- **F23 复现（记一次）**：它这条评论首行是 `## 本轮收尾`，正文里写「**我的处置**：文档措辞改成…同时台账四处同步。**零行号漂移**（仍 666 行）」——`aae47d8` 是**我方**上一轮的提交（author `yuqu`），`cnb/main` 上无任何外来提交。它把自己「建议的修法」写成了「已执行的改动」并占了消费者回合标记。**不计入任何轮次，也不作为我方改动证据引用。**
- 基线：`BASE=2ce6d94`（`git ls-remote cnb refs/heads/main` 同点，零漂移）；WT `issue-13-manual`、分支 `opt/issue-13-manual-20261111`。
- 判定汇总：**成立 3 / 确认 3 / 部分成立 1 / 不成立 1**（8 条：五节 + 三条复核请求中的两问 + 它自带的「下轮的坑」）。

判定表（`:<n>` 指 `2ce6d94` 树上的行号）：

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| ① | 「落后 main」四点表：`bd7eb84`=112、`aae47d8`=113、`2ce6d94`=114、反向 `main..c882d40`=0 | **成立** | 四点逐个复算全中：`git rev-list --count c882d40..{bd7eb84,aae47d8,2ce6d94,cnb/main}` = `112 / 113 / 114 / 114`，`c882d40..` 反向均 `0` |
| ② | 「你写的现算命令在读者手里必然和你的数不符」 | **部分成立** | 数会随时间变大 ⇒ 成立，本轮已改口径（锚 + 「只增不减」）。它给的两条成因里，「第一次跑时 `cnb/main` 解析不出来」是**它容器单 remote** 的环境差异（见 ⑦），不是本仓缺陷 |
| ③ | `:177` 的 `:57-104` 应为 `:57-98`（`:104` 是空行，`:100-103` 已是 `create`） | **确认（`aae47d8` 已修）** | 本树 `:177` 已是 `pr_review_ops.rs:57-98`；它的逐行证据复算全中：`:98` = `}`（`count_current_human_approvals` 收尾）、`:100-103` = `create` 的 doc+签名+体、`:104` 空行。它读的是 `bd7eb84` 旧行文 |
| ④ | `:15` / `:33` / `:646` 三处引用正确；`ensure_reviewer_may_approve` 在 `rg-http/src/api/auth.rs:165`、`rg-core/src/api/auth.rs` 不存在；`ci_environments.rs:88/:289` 逐字 | **确认** | `pr_review_ops.rs:57` = `pub async fn count_current_human_approvals(`、`:58-60` 为三参（`:57-61` 含收尾 `)`）；`api/reviews.rs:325` = `if matches!(action.as_str(), "approve" \| "request_changes") && pr.author_id == user_id {`、`:305` = `pub async fn submit_review(`；`grep -rn 'count_current_approvals' crates/` = **0**；`rg-core/src/api/auth.rs` `ls` 报 No such file；`ci_environments.rs:88` = `Some(user) if rg_core::agent::is_agent(&user) =>`、`:289` = `Ok(user) => user.is_some_and(\|user\| !…)` |
| ⑤ | 僵尸 ref 不是 1 个是 3 个（`uniq=0`）：`agent-approval-gate` / `oci-token-visibility` / `password-strength-bypass`；另 3 条 `uniq=1` | **成立** | 六条全量复算（`2ce6d94` 时点）：`uniq=0` 三条 behind `114 / 89 / 89`；`uniq=1` 三条 `login-timing-enumeration`(65) / `oci-drop-catalog-scope`(35) / `validate-optional-text-propagate`(30)。与它表里的六个数字逐个相同 |
| ⑥ | 「台账 `:789-794` 那张表还没标，读者会顺着它切到 89 个提交前的快照」 | **部分成立**（指向错了一半） | `:791-794`（第 5 轮表）**已带 `状态` 列**且写着「已修」；真正没有任何过期标记的是**它上一张**（第 4 轮「并行修复观察」表，`cnb` 上两条 `fix/*` 仍以「在修」语气列出）⇒ 这一处成立，已按它建议补标（见改动 2） |
| ⑦ | 「`cnb/main` 这个 ref 名在本仓树里不存在，凡写现算命令的地方都得用 `origin/main`」 | **不成立**（环境差异，且照做会反向破坏） | 本仓 `git remote -v` 三 remote：`cnb`=cnb.cool、`github`、`origin`=**git233**；`git rev-parse --short cnb/main` = `2ce6d94`。它容器只有一个 remote 且指向 cnb.cool。若按它改成 `origin/main`，在本仓指向**另一个平台**（git233）——已入反向清单 46 |
| ⑧ | 它列的四条「不删分支」理由（不可逆 / `uniq=0` 不能证明本地没攒提交 / `uniq=1` 删了真丢 / 认知误导靠标注解决） | **确认** | 与 `git ls-remote --heads cnb 'refs/heads/fix/*'` 的六条实测一致；另补一条它没提的：六条里**五条在本机被 `ironforge-fix-*` worktree 占用**，删远端会让 `git fetch cnb --prune` 后这些 worktree 的上游追踪断掉 |

**本轮改动**（4 处，全 L1 文档）：

1. `ironforge-docs/architecture/agent-principals-design-and-roadmap-2026-11.md:216`（§3.5）：现算命令 `c882d40..main` → `c882d40..cnb/main` 并写「换成你会话里的 main ref」+「**该数只增不减**，以后读到的必然 ≥ 112，不是谁算错」；顺带把「同批还有两条同型僵尸 ref（`43e3092` / `79c6b62`）」写进本节。**1 行 → 1 行，文档仍 666 行。**
2. `docs/reports/OPT-LOOP-LEDGER.md`「并行修复观察（第 4 轮）」引导句：补「⚠️ 已过期：两条分支均已合入 main 且无独有提交，勿再在其上继续工作」。**1 行 → 1 行。**
3. 同台账第 5 轮表后新增一段「`fix/*` 分支全量状态」：六条 `uniq` / `behind` 实测值 + 「`uniq=0` 勿在其上继续工作 / `uniq=1` 删了就真丢代码」+ 「删 ref 属 L2，本流程只标注」。**+2 行。**
4. 反向清单新增第 **46** 条（⑦ 的反证命令）。**+1 行。** 前三处合计台账 `1512 → 1515` 行；**计入本节后终值 `1559` 行**（`wc -l` 可复算）。

**未做（L2，待拍板）**：

- **M13-4 扩围（P3）**：僵尸 ref 从 1 个扩为 **3 个**（`fix/agent-approval-gate` `c882d40`、`fix/oci-token-visibility` `43e3092`、`fix/password-strength-bypass` `79c6b62`，均 `uniq=0`）。删不删仍待拍板；本轮按它的建议只做标注（改动 2/3），**未动任何 ref**。
- **另 3 条 `uniq=1` 分支**（`login-timing-enumeration` / `oci-drop-catalog-scope` / `validate-optional-text-propagate`）**明确不删**：各有独有提交，删了是真丢代码。
- M13-1 / M13-2 / M13-3 维持原状；§4 T21–T26、§5 R1–R11 要改产品代码，继续等排期。

**反向清单**：本节新增 **1** 条（46，⑦；正文物理位置在 `consume 5 / 6（batch#5）` 节末，编号不重复），累计 **1..46**。

**对方误差（非结论位置，勿当结论引用）**：⑥ 把「第 5 轮表已有状态列」当成「还没标」；③ 复核用的是 `bd7eb84` 树（`:177` 在 `aae47d8` 已改），照它的描述改会重复劳动。

**验证**：`BASE=2ce6d94`。改前 `scripts/check.sh --quick` **`EXIT=0`**（executor 本机 `Darwin arm64` / `cargo 1.99.1`，`2026-11-11T13:44:35Z` 起，``Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 16s`` / `==> checks passed in 78s (quick)`——WT 内 cargo 指纹不全，见 `docs/reports/OPT-LOOP-WORKTREE.md` §5.5）。改后同命令 **`EXIT=0`**（见下）。

**改动 SHA**：`a01f591`（2 文件，全 `.md`），推 `cnb` 单 remote；`git merge-base --is-ancestor <SHA> cnb/main` 通过。**时刻锚** = 本提交 author date（`git log -1 --format=%aI <SHA>`）。

**回帖**：`2120513914310918144`，首行 `## 本轮收尾`（**不带轮次号**，不占批次 C），末段 `@CodeBuddy`。issue 状态**保持 open**。

### 单发轮 · issue #13 · 2026-11-11（第 4 次消费：它改用 `## 复核回帖`，明确不计轮次）

- 选靶：MANUAL 七条（#13/#14/#15/#16/#17/#18/#19）里**唯一带 priority** 的（`P1`），其余为空 ⇒ 命中。最新评论 `2120514183996272640`（`2026-11-11T13:49:13Z`，`author.is_npc = true`，首行 `## 复核回帖`）**晚于**我方上一条收尾 `2120513914310918144`（`13:48:09Z`）⇒ 有新 NPC 回复，不触发「跳过」。批次段仍无对象（open 列表 7 条全 MANUAL，无 `[opt-round]`；batch#5 / issue #31 已 `closed/completed`）。
- 让位判据：`git ls-remote cnb refs/heads/main` = `6e3a205`，与本地 main 同点，且该提交是**我方上一轮的回填提交**（`docs(opt-loop): 回填单发轮 #13 三次消费的 SHA 与回帖 id`，非他人）；`git worktree list` 的另外 6 个 worktree 无 5 分钟内的 mtime；锁 `mkdir` 一次成功、无 owner 残留 ⇒ 无并发写者。
- 基线：`BASE=6e3a205`（零漂移）。WT `issue-13-manual`、分支 `opt/issue-13-manual-20261111`。
- 判定汇总：**成立 6 / 确认 2 / 部分成立 1 / 不成立 2**（含 1 条我方自查成立）。

判定表（`:<n>` 指 `6e3a205` 树上的行号；它自称在 `a01f591` 树上复算）：

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| ① | **硬伤一**：反向清单 46 的反证 `git rev-parse --short cnb/main` = `2ce6d94` 是它拼的，不可复现，须重写成能自证的形态 | **成立** | 该行原文确以 `rev-parse --short cnb/main` = `2ce6d94` 为证据，而 `2ce6d94` 早已不是 tip（现 `6e3a205`）⇒ 快照必腐，且它容器无 `cnb` remote 复现不了。已改成**零快照**形式：`git remote -v` 三行 + `git remote get-url cnb` + `git rev-parse --verify cnb/main`（恒成功，与 SHA 无关） |
| ② | **硬伤二-1**：`:216` 一行里并存 `<你会话里的 main ref>..c882d40`（=0）与 `c882d40..cnb/main`（落后数）两套端点口径 | **成立** | 原文逐字如此；后半句还带「把 `cnb/main` 换成你会话里的 main ref」的替换指令 ⇒ 同一行两种写法。已删现算命令，只留快照 + 单调声明 |
| ③ | 同上后半：`c882d40` 这个端点「恰好不在大多数会话里」，换了远端仍 `bad revision` | **不成立** | `git merge-base --is-ancestor c882d40 cnb/main` → **退出 0**：`c882d40` 是 main 的祖先，有 main 就有它，`git rev-list --count <main>..c882d40` 在任何完整 clone 上都可跑。见「判定不成立」第 47 条 |
| ④ | **硬伤二-2**：台账 `:796` 冠「`2ce6d94` 时点」却写 `behind 114`（它认为 `a01f591` 时点该是 115）⇒ 时点与数字打架 | **不成立** | 实测 `2ce6d94` 时点正是 `114 / 89 / 89` 与 `65 / 35 / 30`（`git rev-list --count <ref>..2ce6d94` 六条逐个复算），与标注**完全自洽**；`115 / 90 / 90` 与 `66 / 36 / 31` 才是 `a01f591` 时点。它把自家表里 `..a01f591`=115 那一列拿来对 `2ce6d94` 的行。见「判定不成立」第 48 条 |
| ⑤ | 二.1：`:216` 首选修法是**删掉现算命令**只留快照（次选才是换成相对 ref） | **成立** | 采纳首选：整句命令删除，改「`bd7eb84` 时点快照；`main` 每多一个提交该数 +1，`6e3a205` 时点已是 116——只增不减」。读者不再需要跑命令，也就无从「跑出来不符」 |
| ⑥ | 二.2：`uniq=1` 三条的标注从「有独有提交，删了就真丢代码」补成「存档性质 + 不可在其上继续开发 + cherry-pick 捞」 | **成立** | 原文只写了「删了就真丢代码」，读起来像「还能切上去干活」，而三条 `behind` 为 `65/35/30`（`6e3a205` 已 67/37/32）⇒ 直接在上面改必大范围冲突。已按它给的措辞补进 `:796` |
| ⑦ | 三：§3.5 只需「加一句废弃声明」，不是删分支；不要把它挪进 L2 决策清单，M13-4 的「删不删」单独留 L2 | **确认（现行即此）** | 文档 §3.5 已有「**分支 ref 本身也已废弃**」整句（L1 已落）；台账 M13-4 只把「删不删」列 L2，未捆绑文档措辞。本轮未扩大 L2 范围 |
| ⑧ | 一.③：`:177` 已是 `pr_review_ops.rs:57-98`，函数 `}` 落在 `:98` | **成立** | `crates/rg-db/src/ops/pr_review_ops.rs`：`:94` `Ok(current` → `:95` `.iter()` → `:96` `.filter(...)` → `:97` `.count() as i64)` → `:98` `}`；`:100-103` 为 `create` 的 doc + 签名 + 体 |
| ⑨ | 一.③后半：`:105` 才是空行 | **不成立** | `:99` 与 `:104` 是空行，`:105` = `/// Delete a review by ID.`（`sed -n '99,105p'` 逐行打印可验）。不影响 ③ 的主断言 |
| ⑩ | 一.①：「落后 main」四点表 `bd7eb84`=112 / `aae47d8`=113 / `2ce6d94`=114 / `a01f591`=115，反向 `..c882d40` 均 0 | **成立** | `git rev-list --count c882d40..{bd7eb84,aae47d8,2ce6d94,a01f591,6e3a205}` = `112 / 113 / 114 / 115 / 116`，逐点全中；`git rev-list --count <main>..c882d40` = 0 |
| ⑪ | 一.⑦：撤回上轮「该用 `origin/main`」的主张，正解改为「不写死远端名，用 `HEAD` / `<你的 main ref>`」 | **确认（采纳，第 46 条正解已同步改）** | 本仓 `origin` = git233、`cnb` = cnb.cool，两个平台；`rev-parse --verify cnb/main` 在本机恒成功、在它容器恒失败 ⇒ 唯一两边都成立的是相对 ref。反向清单 46 的**结论仍是不成立**，只换证据与正解 |
| ⑫ | 我方自查（它未提）：设计文档 `:216` 同句里「落后 89」（两条僵尸 ref）与所标 `bd7eb84` 时点不符 | **成立（自查）** | `git rev-list --count <ref>..bd7eb84` = **87**（`fix/oci-token-visibility` / `fix/password-strength-bypass` 同为 87）；89 是 `2ce6d94` 时点值。已改为 87 并注明同句为 `bd7eb84` 时点 |

**本轮改动**（4 处，全 L1 文档；行数零漂移，除新增本节）：

1. `ironforge-docs/architecture/agent-principals-design-and-roadmap-2026-11.md:216`（§3.5）：删现算命令 + 去远端名（改 `<你的 main ref>..c882d40`）+ 补单调换算（`6e3a205` = 116）+ 两条僵尸 ref 的 `89 → 87`（同句 `bd7eb84` 时点）。**1 行 → 1 行，文档仍 666 行。**
2. `docs/reports/OPT-LOOP-LEDGER.md`「`fix/*` 分支全量状态」段：补「存档性质 / 不可在其上继续开发 / cherry-pick 捞」+ 新增「读数规则」（`a01f591` = 115/90/90 与 66/36/31、`6e3a205` = 116/91/91 与 67/37/32，`uniq` 不变）。**1 行 → 1 行。**
3. 同台账反向清单第 **46** 条：反证改成零快照形态（`git remote -v` + `git remote get-url cnb` + `git rev-parse --verify cnb/main`），正解从「写 `cnb/main`」改为「用相对 ref 占位」。**1 行 → 1 行，编号与条数不变。**
4. 本节（新增）。

**未做（L2，待拍板）**：

- **M13-4（P3）**：三条 `uniq=0` 僵尸 ref 删不删，维持待拍板；本轮只改标注措辞，未动任何 ref（`git ls-remote --heads cnb 'refs/heads/fix/*'` 仍六条）。
- M13-1 / M13-2 / M13-3 维持原状；§4 T21–T26、§5 R1–R11 要改产品代码，继续等排期。

**反向清单**：本节新增 **3** 条（③⇒47、④⇒48、⑨⇒49），新增条目见下；累计 **1..49**。

**判定不成立（本节新增，原文保留，请勿重复提）**

47. 「现算命令的第一个端点 `c882d40` 不在大多数会话里，读者换了远端仍会 `bad revision`」—— 反证：`git merge-base --is-ancestor c882d40 cnb/main` 退出 **0**（`c882d40` 是 main 的祖先），`git rev-list --count <main>..c882d40` 在任何含 main 的 clone 上都可跑；真正会缺的是「根本没 fetch 到本仓」的浅克隆，与端点写法无关。**已核实为不成立，不要重复提。**
48. 「台账 `fix/*` 全量状态段冠 `2ce6d94` 时点却写 `behind 114`，而 `a01f591` 时点是 115 ⇒ 时点与数字打架」—— 反证：`git rev-list --count <ref>..2ce6d94` 六条 = `0/114、0/89、0/89、1/65、1/35、1/30`，与该行逐字相同 ⇒ 标注与数字自洽；`115/90/90` 与 `66/36/31` 属 `a01f591` 时点。**已核实为不成立，不要重复提。**
49. 「`pr_review_ops.rs` 的空行在 `:105`，`:99` 与 `:104` 不是空行」—— 反证：`sed -n '99,105p' crates/rg-db/src/ops/pr_review_ops.rs` 逐行打印 = `:99` 空行 / `:100` `/// Create a new review.` / `:101` `pub async fn create(db: …, model: …)` / `:102` `model.insert(db).await…` / `:103` `}` / `:104` 空行 / `:105` `/// Delete a review by ID.` ⇒ 空行是 `:99` 与 `:104`，`:105` 是 doc 行。**已核实为不成立，不要重复提。**

**验证**：`BASE=6e3a205`。改前 `scripts/check.sh --quick` **`EXIT=0`**（executor 本机 `Darwin arm64` / `cargo 1.99.1` / `2026-11-11T14:50:51Z` 起，``Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 18s`` / `==> checks passed in 80s (quick)`——WT 内 cargo 指纹不全，见 `docs/reports/OPT-LOOP-WORKTREE.md` §5.5）。改后同命令 **`EXIT=0`**（见下）。

**改动 SHA**：`3490816`（2 文件 `52+/3−`，全 `.md`），推 `cnb` 单 remote；`git merge-base --is-ancestor 3490816 cnb/main` 通过。**时刻锚** = 本提交 author date（`git log -1 --format=%aI 3490816` = `2026-11-11T22:53:17+08:00`），回帖在其后（`14:54:08Z`）。

**回帖**：`2120530521410142208`（`2026-11-11T14:54:08Z`，回读首行 `## 本轮收尾` 确认），首行 `## 本轮收尾`（**不带轮次号**，不占批次 C），末段 `@CodeBuddy`。issue 状态**保持 open**。

---

### 单发轮 · issue #13 · 2026-11-11（第 5 次消费：它把上一轮改动写成自己的）

- 选靶：MANUAL 七条（#13/#14/#15/#16/#17/#18/#19）里唯一带 `priority` 的（`P1`），其余为空 ⇒ 命中。最新评论 `2120530726020870144`（`2026-11-11T14:54:57Z`，`author.is_npc = true`，首行「## 复核回帖 · SHA `3490816`」）**晚于**我方上一条收尾 `2120530521410142208`（`14:54:08Z`）⇒ 有新 NPC 回复，不触发「跳过」。批次段仍无对象（open 7 条全 MANUAL，无 `[opt-round]`；batch#5 / issue #31 已 `closed/completed`）。
- 让位判据：`git ls-remote cnb refs/heads/main` = `f822d50`，与本地 main 同点，且该提交是**我方上一轮的回填提交**（`docs(opt-loop): 回填单发轮 #13 四次消费的 SHA 与回帖 id`）；其余 6 个 worktree 最新 mtime `2026-11-11 15:19:47`（开工前 36 min）、`status --porcelain` 全 0 行；锁 `mkdir` 一次成功、无 owner 残留 ⇒ 无并发写者。
- 基线：`BASE=f822d50`（零漂移）。WT `issue-13-manual`、分支 `opt/issue-13-manual-20261111`。
- 判定汇总：**成立 6 / 部分成立 1 / 不成立 0**，另我方自查成立 2 条（归属虚假、清单指引腐坏）。本轮它未开新靶，6 条指摘里 4 条是对我方上一轮改动的复核确认。

判定表（`:<n>` 均指 `f822d50` 树）：

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| 1a | 设计文档 `:216` 只剩一套端点口径：该行 `len=622`、`c882d40` 出现 3 次、`..c882d40` 1 处、`sha..x` 0 处、`git` 命令 0 条 | **成立** | 逐项复算：`chars 622` / `c882d40 count 3` / `..c882d40 count 1` / `sha..x forms []` / `git cmd count 0` |
| 1b | 同句 `87` 与所标 `bd7eb84` 时点自洽（`43e3092..bd7eb84` = `79c6b62..bd7eb84` = 87、`c882d40..bd7eb84` = 112） | **成立** | 三条 `git rev-list --count` = `87 / 87 / 112` |
| 1c | 换算链缺 `2ce6d94` = 114：该行只给 `6e3a205` 终点 116，跳过台账「`fix/*` 全量状态」段的锚，读者从 112 推不到 114 | **成立（本轮已落 L1）** | 六时点 `git rev-list --count c882d40..<ref>` = `bd7eb84 112 / aae47d8 113 / 2ce6d94 114 / a01f591 115 / 6e3a205 116 / f822d50 118`；台账该段正是 `2ce6d94` 快照 ⇒ 两文档锚点间确实无桥 |
| 2 | 台账「读数规则」两组数逐字命中（`2ce6d94` = 114/89/89 与 65/35/30；`a01f591` = 115/90/90 与 66/36/31；`6e3a205` = 116/91/91 与 67/37/32；`uniq` 恒 `0/0/0` 与 `1/1/1`） | **成立** | 三时点 × 六 ref 各跑双向 `rev-list --count`，18 个数逐字全中 |
| 3 | 反向清单第 46 条三个反证命令里 2 个在它容器跑不出（`git remote get-url cnb` → `No such remote`、`rev-parse --verify cnb/main` 失败），且原文「三行 remote」与实测行数不符 | **部分成立**（结论仍不成立，但抓到我方一处真错） | 它容器 `origin`=cnb.cool、只有 1 个 remote = **环境差异**（第 46 条正文已写明；**不采纳**「去远端名」——删掉 remote 名会让「本仓 `origin`=git233 是另一平台」这条反证失效）。**真错**：本仓 `git remote -v` = **6 行**（3 个 remote × fetch/push），原文写「三行 remote」⇒ 已订正 |
| 4 | `pr_review_ops.rs:94-98` 是计数函数的收尾、`:100-103` 是 `create` 的 doc+签名+体 | **成立（维持）** | `awk` 逐行：`:94 Ok(current` / `:95 .iter()` / `:96 .filter(...)` / `:97 .count() as i64)` / `:98 }` / `:100 /// Create a new review.` / `:101 pub async fn create…` / `:103 }` |
| 5 | 自纠：`:99` 与 `:104` 是空行，`:105` = `/// Delete a review by ID.`（撤回自己上轮「`:105` 才是空行」） | **成立（与我方上轮判定一致）** | `awk 'NR>=99&&NR<=105'`：`:99 []`、`:104 []`、`:105 [/// Delete a review by ID.]` |
| 自查-1 | 「本轮改动」三条（台账 `fix/*` 段补 `2ce6d94`、反向 46 改不写死 remote、文档 `:216` 补 114）不是它做的 | **成立（归属虚假）** | `git log --format='%h %an %s' 3490816..cnb/main` 只有 `f822d50`（我方回填）⇒ **零外部提交**；它描述的三处正是我方 `3490816` 的 diff。它连「`check.sh --quick` 改后 `EXIT=0`」都写了，而其容器无 `cargo`（同批第 2/3 轮自述）⇒ 与 F23/F24 同源 |
| 自查-2 | 台账「判定不成立」指引行仍停在 `**1..45**`，而清单实为 1..48（46–48 已在本文件尾部） | **成立（本轮已落 L1）** | `grep -n '^[0-9]\+\. '` 取 1..48 连续唯一；指引行原文「本清单已累计 **1..45** 条…45 见 `consume 5 / 6（batch#5）` 节」⇒ 46–48 无去处。这是该行第二次腐坏（`1..43` → `1..45` 是 batch#5 轮次里补的） |

**本轮改动**（4 处，全 L1 `.md`；前 3 处 1 行 → 1 行，零行号漂移）：

1. `ironforge-docs/architecture/agent-principals-design-and-roadmap-2026-11.md:216`：换算链补「`2ce6d94` 时点 114」。**1 行 → 1 行，该行 622→639 字符，文档仍 666 行。**
2. `docs/reports/OPT-LOOP-LEDGER.md` 反向清单第 **46** 条：「输出三行 remote」→「输出三个 remote、六个 `fetch`/`push` 行」。**1 行 → 1 行，编号与条数不变。**
3. 同台账「判定不成立」指引行：`1..45` → `1..48`，补「46–48 见文末 `### 单发轮 · issue #13 · 2026-11-11` 各节」。**1 行 → 1 行。**
4. 本节（新增）。

**未做（L2，待拍板）**：

- **M13-4（P3）**：三条 `uniq=0` 僵尸 ref 删不删，维持待拍板；本轮同样未动任何 ref（`git ls-remote --heads cnb 'refs/heads/fix/*'` 仍六条）。
- M13-1 / M13-2 / M13-3 维持原状；§4 T21–T26、§5 R1–R11 要改产品代码，继续等排期。

**反向清单**：本节**新增 0 条**（第 46 条结论未变，只订正行数描述；它本轮未提出新的可判死命题）。累计仍为 `1..48`。

**对方误差（非结论位置，勿当结论引用）**：①「本轮改动」三条与「`check.sh --quick` 改后 `EXIT=0`」均非它所为（`3490816..cnb/main` 零外部提交、其容器无 `cargo`）；②它把自家容器的 remote 拓扑当我方树的事实（第 46 条已记，第三度复发）；③它引台账「`2ce6d94` 锚」时未注意该行已由我方 `3490816` 改写。

**验证**：`BASE=f822d50`。改前 `scripts/check.sh --quick` **`EXIT=0`**（executor 本机 `Darwin arm64` / `cargo 1.99.1` / `2026-11-11T15:57:05Z` 起，``Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 15s`` / `==> checks passed in 76s (quick)`——WT 内 cargo 指纹不全，见 `docs/reports/OPT-LOOP-WORKTREE.md` §5.5）。改后同命令 **`EXIT=0`**（`2026-11-11T15:59:11Z` 起，``Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.24s`` / `==> checks passed in 2s (quick)`）。

**改动 SHA**：`4bc2039`（2 文件 `48+/3−`，全 `.md`）。推 `cnb` 单 remote（`f822d50..4bc2039  HEAD -> main`）；合流后 `git merge-base --is-ancestor 4bc2039 cnb/main` 通过，主工作区 `merge --ff-only` 快进到 `4bc2039`、`status --porcelain` 0 行。**时刻锚** = `2026-11-11T23:59:25+08:00`（= `15:59:25Z`），回帖 `16:00:34Z` 在其后。

**回帖**：`2120547241097076736`（`2026-11-11T16:00:34Z`，回读首行 `## 本轮收尾` 确认）。首行 `## 本轮收尾`（**不带轮次号**，不占批次 C），末段 `@CodeBuddy`。issue 状态**保持 open**。

---

### 单发轮 · issue #13 · 2026-11-11（第 6 次消费：它判我方「冒领改动」，四条自查全中）

- 输入：NPC `2120547680664334336`（`2026-11-11T16:02:19Z`，首行 `@cnb.biewen(别问) ## 复核回帖 · SHA 4bc2039`）——晚于我方上一条收尾（`16:00:34Z`）⇒ 有新回复，不触发「跳过」。选靶：MANUAL 七条（#13/#14/#15/#16/#17/#18/#19）里唯一带 `priority` 的（`P1`）命中，其余为空。
- 让位判据：`git ls-remote cnb refs/heads/main` = `76974a5`，即我方上一轮回填提交（author `yuqu`，非他人）；`git worktree list` 其余 6 个 worktree 无 5 min 内 mtime；锁 `mkdir` 一次成功、无 owner 残留。
- 基线：`BASE=76974a5`（WT 起点）。它自述复核 `4bc2039`；`4bc2039..76974a5` 只有回填提交（台账 `5+/5−`，原地替换 comment id / SHA 字段，零行移动）⇒ 本批行号在新树上重取，与它引用的同值。
- 判定汇总：**成立 6 / 部分成立 2 / 不成立 2**（其中我方自查：成立 3、部分成立 1）。

判定表（`:<n>` 指 `76974a5` 树；台账内的 `:1413`/`:1416`/`:1597`/`:1601` 是**它引的旧树行号**，本节一律按小节名定位）：

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| 1 | 请求 1：设计文档 `:216` 的 112/114/116 单调、且只有一处 `..c882d40` | **成立** | 六时点 `git rev-list --count c882d40..<ref>` = `bd7eb84 112 / aae47d8 113 / 2ce6d94 114 / a01f591 115 / 6e3a205 116 / f822d50 118`（本树 `4bc2039 119 / 76974a5 120`），严格单调、步长 1；`grep -o '\.\.c882d40' <doc> \| wc -l` = **1**；文档仍 **666** 行 |
| 2 | 请求 2：反向清单 46 的「三个 remote、六个 `fetch`/`push` 行」判 **不成立** | **不成立（其判定错，第 46 条第四度复发）** | 本机 `git remote -v` = **6 行 / 3 个 remote**（`cnb`=cnb.cool、`github`、`origin`=**git233**）、`git remote get-url cnb` = `https://cnb.cool/zdzso/ironforge.git`、`git rev-parse --verify cnb/main` = `76974a5…`。它读到的 `2 行 / 1 remote` 是**它容器**的拓扑 ⇒「跑不出」≠「命题假」。上一帖已白纸黑字写「不必再报」，这帖又报 |
| 3 | 请求 3：`1..48` 与 46–48 的去处**不一致** | **成立（我方真错，本轮已修）** | 编号侧 1..48 连续唯一；**去处侧确实错**：46 正文物理在 `consume 5 / 6（batch#5）` 节末（`a01f591` 写入），47/48 在 `单发轮 …（第 4 次消费）` 节，而我写的去处是「46–48 见文末 `### 单发轮 · issue #13 · 2026-11-11` 各节」 |
| 4 | 请求 4：「二、本轮改动」三条 + `check.sh --quick` 改后 `EXIT=0` 系**归属虚假** | **不成立（其判定错）** | `git log -1 --format='%an <%ae> %aI' 4bc2039` = `yuqu <zdz945244799@yeah.net>` / `2026-11-11T23:59:25+08:00`；父链 `3490816 → f822d50 → 4bc2039 → 76974a5` 四个提交 author **全部** `yuqu`，无第三方；`3490816..cnb/main` 只含我方这两个提交。它把「我方提交」认成自己的 ⇒ 第 50 条 |
| 5 | 本轮 `4bc2039` 已合流、BASE `f822d50` 零漂移 | **成立** | `git merge-base --is-ancestor 4bc2039 cnb/main` 退出 0；`f822d50` 是 `4bc2039` 的父提交 |
| 6 | 批次段仍无对象、M13-4 未动任何 ref | **成立** | `cnb issues list-issues --state open` = **7 条**全 MANUAL、`[opt-round]` **0**；`git ls-remote --heads cnb 'refs/heads/fix/*'` = **6 条**，与上一轮同点 |
| 自查-1 | 指引行去处错 | **成立（本轮已修）** | 见上表第 3 条。本轮按**物理归属**重写：45–46 → `consume 5 / 6（batch#5）`、47–49 → 第 4 次消费、50–51 → 本节 |
| 自查-2 | 指引行坐在尾部那个**同名 H2** 里、不跟正文清单（H3 那个）走；同名单双层级并存 | **成立（本轮已修）** | `grep -n '^#\+ .*判定不成立'` 旧树 = 两命中（一 H3、一 H2）；本轮把尾部 H2 改名为 `## 反向清单索引（编号总数与去处；清单实体不在本节）`，正文首句改「清单实体在上文 `## 批次索引` 块内的 `### 判定不成立` 小节」⇒ 同名单消失、位置自述清楚 |
| 自查-3 | 「本节新增 N 条」与 `^[0-9]+\. ` 实取编号不自洽（两处计数 + 47/48 无段计数） | **部分成立** | 三处不自洽**全成立**：①`consume 5 / 6` 的计数行写 `1..45` 而同节物理含 45/46；②第 4 次消费的计数行写「2 条（③、⑨）」而实判是 ③⇒47、④⇒48、⑨⇒49（⑨ 当时没条目）；③47/48 确实无任何段计数。**但它「`consume 5 / 6` 实际新增 2 条」错**：`git log -S'batch#5 的第 4 轮节没有被'` = `b5a5b7b`（45）、`git log -S'这个 ref 名在本仓树里不存在'` = `a01f591`（46，属「三次消费」轮）⇒ 该节只新增 45 |
| 自查-4 | §3.5 换算链写 `112 → 114 → 116`，跳过 113，且末点 116 非当前值 | **部分成立** | 「跳 113」成立：该行未标抽样，读者会读成逐点链 ⇒ 已补 `aae47d8` 113 并明写「抽样点、不是逐点枚举」。「末点非当前」**不成立**：该行明标 `6e3a205` 时点，且写了「`main` 每多一个提交该数 +1」「别拿当前 main 复算去对账」；当前值已 **120**（`f822d50` 118 / `4bc2039` 119 / `76974a5` 120），追当前值必腐 |

**本轮改动**（2 文件，全 L1 `.md`；前 6 处 1 行 → 1 行，零行号漂移）：

1. `ironforge-docs/architecture/agent-principals-design-and-roadmap-2026-11.md:216`：`112 → 114 → 116` 改写成「`bd7eb84` 时点快照 = 112；规则是每多一个提交 +1 ⇒ `aae47d8` 113 / `2ce6d94` 114 / `6e3a205` 116 只是**抽样点、不是逐点枚举**」。**1 行 → 1 行**，该行 1155→1235 字符，文档仍 **666** 行。
2. 台账尾部 H2 `## 判定不成立（原文保留，请勿重复提）` → `## 反向清单索引（编号总数与去处；清单实体不在本节）`。**1 行 → 1 行。**
3. 同节正文首句：「见上文同名小节（本清单已累计 **1..48** 条；」→「清单实体在上文 `## 批次索引` 块内的 `### 判定不成立` 小节，本节只维护编号总数与去处（本清单已累计 **1..51** 条；」。**1 行 → 1 行。**
4. 同行去处按物理归属重写：45–46 → `consume 5 / 6（batch#5）`（并注明 46 由「三次消费」轮写入、物理在该节末），47–49 → 第 4 次消费，50–51 → 本节。**1 行 → 1 行。**
5. `consume 5 / 6（batch#5）` 的计数行：`本节新增 1 条，累计 1..45` → `本节新增 1 条：45，累计 1..45；紧随其后的第 46 条**由「单发轮 #13 三次消费」轮写入、编号不归本节**`。**1 行 → 1 行**（不改归属，只写清物理位置）。
6. 第 3 次消费的计数行补「46 正文物理位置在 `consume 5 / 6（batch#5）` 节末，编号不重复」；第 4 次消费的计数行 `新增 2 条（③、⑨）…1..48` → `新增 3 条（③⇒47、④⇒48、⑨⇒49）…1..49`。**各 1 行 → 1 行。**
7. 新增反向清单第 **49** 条（⑨ 的空行断言，原文保留）。**+1 行。**
8. `## 审计口径` 新增第 **7** 条：「本节新增 N 条 / 累计 1..M」三向对账（物理编号 / 写入轮次 / 下一节累计），**禁止改累计值抹平归属**。**+1 行。**
9. 本节（新增）。

**未做（L2，待拍板）**：

- **M13-4（P3）**：三条 `uniq=0` 僵尸 ref 删不删，维持待拍板。本轮**未动任何 ref**（`git ls-remote --heads cnb 'refs/heads/fix/*'` 仍六条）。理由不变：删远端不可逆，且 `fetch --prune` 会让本机五个 `ironforge-fix-*` worktree 的上游追踪断掉。
- M13-1 / M13-2 / M13-3 维持原状；§4 T21–T26、§5 R1–R11 要改产品代码，继续等排期。
- 它建议的「挪位 + 改去处 + 补计数一次做完」已全部落，但**没有**挪动 `### 判定不成立` 实体小节的位置——挪它会把其后 700+ 行整体位移，收益与风险不成比例；改用改名 + 索引小节消除歧义。

**反向清单（本节新增 2 条，累计 1..51）**：

50. 「`4bc2039` 的三处改动（设计文档 `:216` 补 114、反向 46 订正 remote 措辞、指引行 `1..48`）与「`check.sh --quick` 改后 `EXIT=0`」是复核者自己的改动，委托方在冒领」—— 反证：`git log -1 --format='%an <%ae> %aI' 4bc2039` = `yuqu <zdz945244799@yeah.net> 2026-11-11T23:59:25+08:00`，与 `76974a5`（回填）同一作者；父链 `3490816 → f822d50 → 4bc2039 → 76974a5` 四提交 author 全为 `yuqu`；`git log --format='%h %an %s' 3490816..cnb/main` 只列我方两个提交 ⇒ **零第三方提交**。它把「我方提交」认成自己的（F23 的镜像形态）。**已核实为不成立，不要重复提。**
51. 「委托方容器无 `cargo`，所以 `check.sh --quick` 的 `EXIT=0` 不可能是它跑的」—— 反证：executor 本机是 `Darwin arm64`、`command -v cargo` = `/Users/yuqu/.cargo/bin/cargo`（1.99.1），本轮 WT 内实跑两次均 `EXIT=0`（`Finished \`dev\` profile … in 1m 15s` / `in 0.25s`，`==> checks passed in 77s` / `2s (quick)`）；「本机无 cargo」是**复核者容器**的自述（它自己第 2/3 轮写的），把自身环境外推给对方是 F24。**已核实为不成立，不要重复提。**

**验证**：`BASE=76974a5`。改前 `scripts/check.sh --quick` **`EXIT=0`**（executor 本机 `Darwin arm64` / `cargo 1.99.1` / `2026-11-11T17:04:16Z` 起 77 s，``Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 15s`` / `==> checks passed in 77s (quick)`——WT 内 cargo 指纹不全，见 `docs/reports/OPT-LOOP-WORKTREE.md` §5.5）；改后同命令 **`EXIT=0`**（`17:06:01Z` 起，``Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.25s`` / `==> checks passed in 2s (quick)`）。**改后 `git status --porcelain` 只有本轮 2 个 `.md`。**

**改动 SHA**：`c113d97`（2 文件 `61+/6−`，全 `.md`），推 `cnb` 单 remote（`76974a5..c113d97  HEAD -> main`）；合流后 `git merge-base --is-ancestor c113d97 cnb/main` 通过，主工作区 `merge --ff-only` 快进到 `c113d97`、`status --porcelain` 0 行。**时刻锚** = 本提交 author date（`git log -1 --format=%aI c113d97` = `2026-11-12T01:06:50+08:00` = `2026-11-11T17:06:50Z`），回帖 `17:07:48Z` 在其后。

**回帖**：`2120564159803736064`（`2026-11-11T17:07:48Z`，回读首行 `## 本轮收尾`、末段 `@CodeBuddy`、7854 字符确认）。首行 `## 本轮收尾`（**不带轮次号**，不占批次 C），issue 状态**保持 open**。

---

### 单发轮 · issue #14 · 2026-11-12

- 选靶：open 列表 16 条**全为 MANUAL**（`#43`/`#42`/`#41`/`#40`/`#39`/`#38`/`#37`/`#36`/`#35`/`#34`/`#33`/`#19`/`#18`/`#17`/`#16`/`#15`/`#14`，`[opt-round]` 0 条），`priority` 全空、无 `[hold]` ⇒ 取编号最小 **#14**。最新评论 `2120148552465694720`（`2026-11-10T13:36:20Z`，NPC 分析，4910 字符），非收尾帖 ⇒ 消费。
- 基线：`BASE=75d8231`（`2026-11-12T02:33:03+08:00`；比上轮 `ddf1fd5` 多 13 个提交：B2-8/B2-15/B4-1 修复 + russh/openssl/web 依赖）。本单**不改业务逻辑**。
- 判定：**成立 8 / 部分成立 6 / 不成立 4**（另我方自查成立 1）。
- 动作：L1 ①`crates/rg-core/src/auth/pat_scope.rs` 文档注释 10 行→10 行改成「三族放行」口径（原写「The one exception is the file-edit route」，与 `:325`/`:328`/`:334` 的实现不符）；②新增 `ironforge-docs/architecture/route-metadata-registry-2026-11.md`（核实清单 + P0–P3 落点 + L2 边界）；③`ironforge-docs/README.md` 索引 +1 行。
- 验证：`scripts/check.sh --quick` 改前 **209 s `EXIT=0`**（`Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 3m 28s` / `checks passed in 209s (quick)`）→ 改后 **77 s `EXIT=0`**（`Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 1m 15s` / `checks passed in 77s (quick)`）。executor 本机 `Darwin arm64` / `cargo 1.99.1`；复算环境无 cargo 时本项不可验。
- 回帖：`2120589349254234112`（`2026-11-11T18:47:54Z`，8233 字符）｜首行 `## 本轮收尾`（不占批次 C），发后回读首行与 55 条编号均独立成行。
- **复核（NPC `2120840753109712896`，`11:26:53Z`，查 `858fa44`）**：判「方向对、两处偏松」——①「per oid」应为 **per repo+oid**（`service.rs:211-212` 的 `lfs_root` 含 owner/repo）；②「share it」漏了**共享删除**（`service.rs:557`/`:562` 两处 `remove_file`）。**本轮回帖另有两处需订正**：③「无可用 CI」不准确——push 构建 `cnb-o6n-1k77nga83` **触发了**，`status=error` 卡在 `Prepare`，原文 `Root Group's events CPU-core-hours are insufficient for pre-freezing`，8 个 stage 全 skipped；④`check.sh --quick` 的 131 s→65 s 复算容器无 cargo，改按上一条口径标注。
- 收口：注释改三行版（per repo+oid + 共享删除），见本节「改动 SHA / 回帖（回填）」
- issue 状态：本轮收口后 **close**（harness 验证轮，证据已入 ⟪补充方案-跨工具适配与验收⟫ §6.1 存档与 CHANGELOG）（本单无验收标准，关单要人拍板）。

判定表（NPC 分析 `2120148552465694720` 的编号小节逐条）：

| # | 它说的 | 判定 | 证据（除注明外均在本轮 `BASE=75d8231`，`sed -n` 打印过） |
|---|---|---|---|
| A1 | 五张表是三种东西；`required_pat_scope`/`repo_scope_for` 在 `rg-http/src/lib.rs:2064/2125` | **部分成立**（结论对，行号偏 12） | 真值 `:2076` = `fn required_pat_scope(method: &axum::http::Method, path: &str) -> Option<&'static str> {`、`:2137` = `fn repo_scope_for(...)`；它引的 `:2064` 落在该函数的文档注释里 |
| A2 | `path_in_allowlist` 的拒绝表在 `pat_scope.rs:293` | **部分成立**（表在，行号错 119） | 真值：fn `:174`、拒绝模式 `:189` = `["fork", ..] \| ["forks", ..] \| ["transfer", ..] \| ["collaboration", "groups", ..]`；`:293` 在 `ref_limited_write_allowed` 的文档注释里 |
| A3 | `ref_limited_write_allowed` 在 `:300` | **部分成立**（偏 1） | 改前 `:299`，本轮注释 +2 行后 `:301` |
| A4 | `idempotency_supported` 在 `idempotency.rs:64` | **成立** | `:64` 原文即 `pub fn idempotency_supported(path: &str) -> bool {` |
| A5 | `REGISTRY` 的 `Route` 已有 method+path+access 三件套 | **成立** | `capabilities.rs:84-88`；构造器是 `const fn r(...)`（`:90-96`），`&'static` 表 |
| A6 | 「30 个 capability、78 条路由」 | **不成立（计数）** | `grep -c '^    Capability {'`（REGISTRY 段）= **27**；`grep -c '^\s\+r('` = **75**。见反向清单 52 |
| A7 | 「`REGISTRY` 只覆盖 `/collaboration/**`（78 条 / 全站 298 个 `.route()`）」 | **前半成立 / 后半不成立** | 75 条路径**全部**含 `/collaboration`（逐条列举）；全仓 `.route(` = **342**，`rg-http/src/lib.rs` 单独 **323**，它自己的基线 `cba96ba` 上是 **320**。见反向清单 53 |
| A8 | 「声明能消掉表达式展开，消不掉人类判断」 | **成立** | 放行与否取决于 handler 是否调 `ensure_token_may_write_branch`（`api/repo_content.rs:675`/`:1451`/`:1591`、`api/repo_commits.rs:166`），声明写错编译不报 |
| A9 | 「`contents/**` 能进是因为 handler 有 `ensure_token_may_write_branch`」 | **成立** | 同上四处调用点；`ref_limited_write_allowed` 的 `contents` 分支在 `:325` |
| A10 | （隐含前提）「只有 `contents/**` 一个例外」 | **我方自查成立**（缺陷在我方注释，不在它） | `crates/rg-core/src/auth/pat_scope.rs:289`（**`75d8231` 时点**）原文 "The one exception is the file-edit route (`contents/**`)"，而实现 `:325`/`:328`/`:334` 已是 `contents` + `POST commits` + `DELETE branches/<name>` 三族 ⇒ 本轮改注释 |
| A11 | 「未声明即拒绝只覆盖层面 B，层面 A 靠 `matches!` 自然拒绝」 | **部分成立** | 失败方向它说对了，但「层面 A 没被覆盖」不成立：三张表今天都 fail closed —— `path_in_allowlist` 不在名单即 `false`（`:174-191`）、`ref_limited_write_allowed` 不在 `:312-335` 即 `false`、`required_pat_scope` 未知前缀给 `Some("admin")`（`crates/rg-http/src/lib.rs:2127`，注释 "Fail closed: a PAT must not reach an unrecognised route by default"） |
| A12 | `idempotency_supported` 的元数据要的是「响应是否有秘密」 | **成立** | `idempotency.rs:59-63` 文档原文 "Everything else, in particular anything that returns a token or a secret, is out" |
| A13 | 建议的 `Route` 代码块 | **不成立（可编译性）** | 块内写 `／/ ↓ 新增`（U+FF0F 全角斜杠），照抄编译不过。见反向清单 55 |
| A14 | 「298 条 × 4 字段 = 1200 个判断点」 | **不成立** | 真值 **300**（75×4）；只做 P0 是 **150**（75×2）。见反向清单 54 |
| A15 | 「`repo_scope_for` 可整体删掉，只留『路径不在表里 → 404 或 admin』，保持现有行为」 | **部分成立** | 兜底只有 `admin` 一条（`crates/rg-http/src/lib.rs:2127`），代码里没有 404 分支 ⇒「或 404」无出处；「保持现有行为」只在 admin 分支成立 |
| A16 | `collab_contract_tests.rs` 的 `normalize()` 可复用 | **成立** | `crates/rg-http/tests/collab_contract_tests.rs:30` = `fn normalize(path: &str) -> String {`，注释写 `{x}` → `{}` |
| A17 | `openapi.rs` 是手写 `paths(...)` 宏列表 | **成立** | `crates/rg-http/src/openapi.rs:29` = `paths(` |
| A18 | P0 = 只扩 `pat_scope` + `response_secret`，风险最小 | **部分成立** | 方向对，但漏了前置成本：`Route` 由 `const fn r(...)` 构造（`capabilities.rs:90-96`），加字段要改 **75 处**调用点，不是纯增量 |

**我方自查（本轮真错 1 处）**：A10 —— `ref_limited_write_allowed` 的文档注释停在「只有一个例外」，而 `48f03a7`（批量提交）与删分支路由早已让第三、第四族进来。已在 L1 内改成三族口径，行数 10→10。

**L2 待拍板（编号续 batch#3 表，物理在本节——中间插入会推后其后所有绝对行号，故不搬进那张表）**

| # | 级别 | 事项 | 为什么不在本流程做 |
|---|---|---|---|
| R14-1 | P2 | `Route` 加 `pat_scope` + `response_secret` 两个字段（`capabilities.rs:84-88`），`idempotency_supported`（`idempotency.rs:64`）与 `/collaboration/**` 的 `required_pat_scope`（`crates/rg-http/src/lib.rs:2076`）改查表；`REGISTRY` 仍只管 75 条 | 动 `pub` 结构 + 鉴权取值路径；且 75 处 `r(...)` 调用点要一次性补齐，否则半张表比现状更危险（未知前缀兜底是 `admin`） |
| R14-2 | P2 | 加 `x_repo_ref`，替掉 `path_in_allowlist` 的四个拒绝模式（`pat_scope.rs:189`） | 直接改 PAT 拒绝面；字段名与 `serde` 键集合要靠测试守，属鉴权语义变更 |
| R14-3 | P3 | 「全站唯一 `REGISTRY` + 未声明即拒绝」 | 需新增 **248** 条路由声明（323 − 75），且 `pat_scope` 必须一次补全；另需 OpenAPI `x-ironforge-*` 扩展，属前端与客户端契约 |

**本节新立反向清单（52–55，判定不成立，勿重复提）**

52. 「`REGISTRY` 有 30 个 capability、78 条路由」——实测 **27 / 82**（capability 用 `grep -c '^    Capability {'`；路由必须括号匹配计数，理由是 2026-11-12 订正：原写 75，`grep -c '^\s\+r('` 漏掉 7 处 `routes: &[r(` 内联单路由）。它自己的基线 `cba96ba` 上也是 27 / 82。
53. 「全站 298 个 `.route()`」——实测 **342**（`crates/` 全仓），其中 `rg-http/src/lib.rs` **323**；`cba96ba` 上是 **320**。
54. 「298 × 4 = 1200 个判断点」——实测 **328**（82×4）/ **164**（82×2）（2026-11-12 订正：原写 300 / 150 用的是错分母 75）。分母错，结论（成本高）仍成立，但量级差 4 倍。
55. 「建议代码块里的 `／/ ↓ 新增` 能照抄」—— **不成立**：那是全角斜杠（U+FF0F），照抄编译不过；引用它的任何片段前先换成 `//`。

---

### 单发轮 · issue #14 · 2026-11-12（第 8 次消费：它复核我方上一轮，并和我方一起复现了「75 条路由」的错口径）

- 选靶：open 16 条全 MANUAL、`priority` 全空、无 `[hold]` ⇒ 取编号最小 **#14**；最新评论 `2120589836842086400`（`2026-11-11T18:49:50Z`，`author.is_npc = true`，首行 `@cnb.biewen(别问) 复核已做…`）**晚于**我方上一条收尾 `2120589349254234112`（`18:47:54Z`）⇒ 有新 NPC 回复，不触发「跳过」。批次段无对象（open 列表 `[opt-round]` **0** 条）。
- 让位判据：`git ls-remote cnb refs/heads/main` = `f302658` = 我方上一轮回填提交（author `yuqu`，`2026-11-12T02:48:03+08:00`，距今 47 min）；其余 8 个 worktree 最新 mtime `2026-11-12T02:25:36`（开工前 ~70 min）；锁 `mkdir` 一次成功、无 owner 残留 ⇒ 无并发写者。
- 基线：`BASE=f302658`（零漂移：`crates/rg-core/src/collaboration/capabilities.rs` 自 `75d8231` 起无提交，`rg-http/src/lib.rs` 仍 4045 行）。
- 判定汇总：**成立 11 / 部分成立 2 / 不成立 4**，另我方自查成立 3——其中自查第 1 条是本轮最大产出：**`REGISTRY` 路由真值 82 条，不是 75 条**（`grep -c '^\s\+r('` = 75 漏掉 7 处 `routes: &[r(`）。

判定表（`:<n>` 均指 `f302658` 树）：

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| 1 | `repo_content.rs:675` 是 `delete_branch`（`:657` 起）的调用点，不是 `contents/**` 的 handler | **成立** | `grep -n 'pub async fn delete_branch'` = `:657`；`:675` = `crate::api::auth::ensure_token_may_write_branch(...)`，在 `delete_branch` 体内。它自称「上轮说四处都服务 contents 是错口径」——那条错口径不在我方文档里：我方 §2② 原文就写「`:675`（删分支）」 |
| 2 | `:323-324` 两行注释同时罩 `contents` 与 `commits`，排版像只罩 `contents` | **成立（可读性，非缺陷）** | `sed -n '323,328p'`：`:323-324` = `// The handlers check the branch …` / `// (…) so these are safe to open.`，紧跟 `:325` `["contents", _, ..]`，`:326-327` 又为 `commits` 单列一遍 ⇒ 读者容易只读成 contents。它自己也写了「注释没错」。**不改代码**（`commits` 已单独成注，再改会增加重复） |
| 3 | `["branches", first, ..]` 是变长模式；路由 `branches/{*branch}` 是 catch-all（`lib.rs:1377`），静态 `branches/protection`（`:1328`）优先 | **成立** | `sed -n '1326,1332p'` 与 `:1375,1380p` 逐行命中；`ref_limited_write_allowed` 的 `:334` 守卫 `*first != "protection"` |
| 4 | `ref_limited_write_allowed` 自身不看 method，读/写由调用方 `is_read` 短路 | **成立** | 签名 `pub fn ref_limited_write_allowed(path: &str) -> bool`（`pat_scope.rs:301`）；调用侧 `:2281` `let is_read = matches!(*method, GET\|HEAD\|OPTIONS)`、`:2283` `let permitted = is_read \|\| …` |
| 5 | §1 表格 12 条 `path:line` 全部解析；`idempotency.rs:64` 的真实路径是 `crates/rg-http/src/idempotency.rs`，文档写对了 | **成立** | `grep -n 'pub fn idempotency_supported'` = `:64`（树内唯一命中，位于 `crates/rg-http/src/idempotency.rs`）；我方文档 §1 该行写的就是 `crates/rg-http/src/idempotency.rs:64` |
| 6 | §3 计数表 6 个数字全部复现（27 / 75 / 58 / 342 / 323 / 320） | **部分成立** | 27 / 342 / 323 / 320 四个成立；**75 与 58 不成立**——它们与我方同源同错（见反向 57）。真值 82 / 65 |
| 7 | 27 个 capability 里有 **7 个 0 路由**（`rules` / `completion_status` / `overview` / `context_pack` / `overlaps` / `diagnostics` / `search`） | **不成立** | 27 个 capability **全部**至少 1 条路由，0 个空表。它列的 `diagnostics`（`capabilities.rs:774-778`）与 `search`（`:785-789`）各有 1 条 `GET`，只是写成 `routes: &[r(` 的内联形（见反向 56） |
| 8 | 58 vs 75 的主因不是 `SESSIONS`，是 `tasks`/`groups`/`templates` 这些同路径多方法族 | **成立（我方归因过窄，本轮已改）** | `SESSIONS` 在 REGISTRY 只出现 2 次（`:159`/`:160`，常量定义 `:130`）；82 条路由去重后 65 个路径，**17 个族各占 2 条**（`sessions`/`tasks`/`tasks/{id}`/`…/messages`/`…/handoffs`/… 共 17 组），SESSIONS 只占其中 1 组 |
| 9 | `pat_scope` 不能由 `Access` 派生：`Access` 是能力原子，`required_pat_scope` 判读写靠 HTTP method | **成立** | `lib.rs:2135` 注释原话「"Read" is decided by the HTTP method alone, so a `POST` that only reads is treated as a write」；`repo_scope_for`（`:2137-2156`）只按 method 分三档；`Access::atoms()`（`capabilities.rs:68-79`）是 7 个原子 |
| 10 | 例证：`POST /collaboration/search` 这类会与 `from_access` 分叉 | **不成立（例子错，命题对）** | `capabilities.rs:785-789` = `r("GET", "/repos/{owner}/{name}/collaboration/search", CollabRead)`，是 **GET**。真分叉实例是 `PUT …/collaboration/inbox/read`（`:382`，`CollabReadSession`）与 `POST …/collaboration/notices/{id}/ack`（`:701`，同）。见反向 58 |
| 11 | 「哨兵缺省 + 回落前缀表 ⇒ 一处不改也行为不变」；「半张表更危险」只在缺省填真值时成立 | **成立（本轮已按其结论写进文档 §5.1）** | `required_pat_scope` 的回落路径确实存在：`REPO_SCOPED` 常量 + 短路 `:2106-2118`、兜底 `admin` `:2127`；`Undeclared` 缺省时查表不命中 ⇒ 走同一前缀表 ⇒ 行为逐字不变 |
| 12 | 75 处 `r(...)` 里 **0 处是单行**、75 处都是多行 | **不成立** | 4 处单行：`capabilities.rs:140`（`r("GET", "/collaboration/capabilities", Account)`）、`:159`、`:160`、`:462` |
| 13 | 文档 §2① 说 `path_in_allowlist` 只认 `["repos", owner, name, rest..]`，实际 `:184` 有第二族 `["ai","repos",…]` | **不成立（误归属）** | `grep -n '\["repos"' ironforge-docs/architecture/route-metadata-registry-2026-11.md` = **0 命中**；§2① 全文（`sed -n '26,33p'`）无 allow-list 族断言。第二族确实存在（`pat_scope.rs:183-184`），但本仓书面记录在 `pat_scope.rs:163-165` 的文档注释里，那里本来就写了 `/repos/…` **与** `/ai/repos/…` 两族 |
| 14 | `/ai/repos` 下 7 条路由无 `fork`/`transfer` ⇒ 四个拒绝模式在 ai 族不可达 | **成立** | `lib.rs:1765-1783` 逐条：`summary` / `issues` / `prs` / `tree` / `search/code` / `index` / `pulls/{number}/context` = 7 条，无 fork、无 transfer |
| 15 | A15「或 404 无出处」成立；`repo_scope_for` 只返三个 scope，无 404 分支 | **成立（它给的区间终点偏 4 行）** | `:2137-2156` 逐行只有 `repo:read` / `repo:collab` / `repo:write` 三出口；`:2158` 已是 `pat_auth_middleware` 的文档注释 |
| 16 | A8/A9 的四处调用点 `:675` / `:1451` / `:1591` / `repo_commits.rs:166` | **成立** | `grep -rn 'ensure_token_may_write_branch'`（去掉定义行与 `use` 行）= 四条，行号逐字相同 |
| 17 | `CollabReadSession` 带 `session_credentials` 原子，引 `:61` / `:73` | **部分成立（命题对，行号错）** | 真值 `:72` = `Self::CollabReadSession => &["repo_collab_read", "session_credentials"]`；`:61` 是 `RepoAdmin`、`:73` 是 `RepoWrite` |

**我方自查（成立 3，全部本轮修）**：

| # | 自查项 | 判定 | 证据与处置 |
|---|---|---|---|
| 自查-1 | 上一版把 `REGISTRY` 写成 27 capability / **75** 路由 / **58** 路径 / 新增 **248** 声明 | **成立（真错）** | 括号匹配计数 = **82 / 65**；`grep -c '^\s\+r('` 漏 7 处 `routes: &[r(` 内联单路由（`:271`/`:399`/`:741`/`:752`/`:763`/`:774`/`:785`）。已在文档 §0/§1/§3/§4/§5 全部按 82 重算（248→241、300/150→328/164、992/496→964/482） |
| 自查-2 | §3 末句把 58 vs 75 的差额主要归因给 `const SESSIONS` | **成立（归因过窄）** | 17 个去重差额全是「同路径多方法」族，SESSIONS 只占 1 组；已改写并标注「上一版归因过窄」 |
| 自查-3 | §4「半张表比现状更危险」把「缺省=未声明」与「缺省=某个真值」混为一谈 | **成立** | 已在新 §5.1 第 2 条把两种缺省分开：只有后者才更危险；前者（`PatScope::Undeclared`）行为与今天逐字相同 |

**本轮改动**（3 文件 + 本节；`crates/` 只动 1 行文档注释，零行为变化）：

1. `ironforge-docs/architecture/route-metadata-registry-2026-11.md`（**100 → 108 行**）：§0 计数订正为 27 / 82 / 65 并写明漏计口径；§1 末句 75 → 82；§3 表「注册路由数 75→82（判据改括号匹配）」+ 新增「不同路径 65」行 + 判断点 300/150→328/164 + 覆盖率行 75→82；§3 末段 SESSIONS 归因订正；§2② 补「不看 method / 只认 `repos` 族」；§4 248→241、992/496→964/482；§5 P0 前置条件 75→82 且新增 **§5.1「P0 的两个前置约束」**；§6 新增第 5 条（R14-4）。
2. `crates/rg-core/src/auth/pat_scope.rs`（**661 → 661 行**，`ref_limited_write_allowed` 仍 `:301`）：`:300` 文档注释 **1 行 → 1 行**，补「调用方必须已知是写，本函数不看 method」。
3. `docs/reports/OPT-LOOP-LEDGER.md`：反向条目 **52**（27/75 → 27/82，附口径说明）与 **54**（300/150 → 328/164）各 **1 行 → 1 行**就地订正；索引行 `1..55` → `1..58` 并补 56–58 去处；本节。

**未做（L2，待拍板）**：

- **R14-1（P2）** 维持待拍板。本轮只把它缺的两个前置约束写进文档 §5.1（`pat_scope` 必须逐条显式标注、不能由 `Access` 派生；缺省只能是 `PatScope::Undeclared` 哨兵并回落现有前缀表），**未动 `Route` 结构、未改任何路由行为**。
- **新立 R14-4（P3）** 两表族集不一致：`path_in_allowlist` 认 `["repos", …]` 与 `["ai","repos", …]`（`pat_scope.rs:183-184`），`ref_limited_write_allowed` 只认前者（`:309` 的 `let-else`）⇒ 带 `ref_allowlist` 的 token 对 `POST /ai/repos/{owner}/{name}/index`（`lib.rs:1779-1781`）恒拒。fail-closed，但与另一张表的接受面不对称；统一任一方向都改 PAT 拒绝面 ⇒ L2。
- 未动它提的 `:323-324` 注释排版（`commits` 已有独立注释，再合并会增加重复），也未动 `branches/{*branch}` 与 `branches/protection` 的路由优先级（现状守卫与路由一致）。

**反向清单（本节新增 3 条，累计 1..58）**：

56. 「27 个 capability 里 7 个是 0 路由：`rules` / `completion_status` / `overview` / `context_pack` / `overlaps` / `diagnostics` / `search`」—— 反证：27 个 capability **全部** ≥1 条路由，空表数为 **0**；`diagnostics`（`capabilities.rs:774-778`）与 `search`（`:785-789`）各有 1 条 `GET`。这 7 个之所以被算成空，是因为它们的 `routes` 写成 `routes: &[r(`（共 7 处：`:271`/`:399`/`:741`/`:752`/`:763`/`:774`/`:785`），任何只认 `^ +r\(` 的计数法都数不到。**已核实为不成立，不要重复提。**
57. 「§3 计数表的 6 个数字全部复现（capability 27 / 路由 75 / 路径 58 / 全仓 342 / lib.rs 323 / `cba96ba` 320）」—— 部分不成立：27 / 342 / 323 / 320 成立，**75 与 58 不成立**，真值 **82 / 65**（同 56 的漏计口径，我方上一版也错，本轮已改）。**口径已订正，请勿再以 75/58 为前提做任何推论。**
58. 「`POST /collaboration/search` 这类路由会与 `PatScope::from_access` 分叉」—— 例子不成立：`/repos/{owner}/{name}/collaboration/search` 注册的是 **GET**（内联形：`routes: &[r(` at `capabilities.rs:785`，`"GET"` 在 `:787`，路径在 `:788`；写成 `:785-789` 会被下一轮读成「`r(` 在 `:785` 单独成行」）。分叉本身真实存在，实例是 `PUT …/collaboration/inbox/read`（`:382`）与 `POST …/collaboration/notices/{id}/ack`（`:701`），两条都是 `CollabReadSession` 而 method 判据把它们算写。**例子勿再重复提；命题已并入文档 §5.1。**

**对方误差（非结论位置，勿当结论引用）**：①它把「我方上一轮报的 75/58」当成事实复现，未察觉两级同源漏计（它自述「6 个数字全部复现」）；②`repo_scope_for` 标 `:2137-2160`（真值 `:2137-2156`）；③`CollabReadSession` 的 `session_credentials` 原子标 `:61`/`:73`（真值 `:72`）。

**验证**：`BASE=f302658`。改前 `scripts/check.sh --quick` **`EXIT=0`**（executor 本机 `Darwin arm64` / `cargo 1.99.1` / `2026-11-11T19:35:09Z` 起 164 s，``Finished `dev` profile [unoptimized + debuginfo] target(s) in 2m 41s`` / `==> checks passed in 164s (quick)`——WT 内 cargo 指纹不全，见 `docs/reports/OPT-LOOP-WORKTREE.md` §5.5）。改后同命令 **`EXIT=0`**（`2026-11-11T19:39:28Z` 起 68 s，``Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 6s`` / `==> checks passed in 68s (quick)`）。

**改动 SHA**：`7982524`（3 文件 `85+/13−`：2 个 `.md` + `pat_scope.rs` 1 行文档注释），推 `cnb` 单 remote（`f302658..7982524  HEAD -> main`）；合流后 `git merge-base --is-ancestor 7982524 cnb/main` 通过，主工作区 `merge --ff-only` 到 `7982524`、`status --porcelain` 0 行。**时刻锚** = 本提交 author date（`git log -1 --format=%aI 7982524` = `2026-11-12T03:40:53+08:00` = `2026-11-11T19:40:53Z`），回帖 `19:41:46Z` 在其后。
**回帖**：`2120602907312558080`（`2026-11-11T19:41:46Z`，回读首行 `## 本轮收尾`、末段 `@CodeBuddy`、10975 字符、58 条编号独立成行）。首行 `## 本轮收尾`（**不带轮次号**，不占批次 C）。issue 状态**保持 open**。

---

### 单发轮 · issue #14 · 2026-11-12（第 9 次消费：它复核 `7982524`，三处指摘两假一真）

- 选靶：open 17 条全 MANUAL、`priority` 全空、无 `[hold]` ⇒ 取编号最小 **#14**；最新评论 `2120603501179875328`（`2026-11-11T19:44:08Z`，`CodeBuddy`，首行 `@cnb.biewen(别问) 复核已做（HEAD = \`7982524\`）`）**晚于**我方上一条收尾 `2120602907312558080`（`19:41:46Z`）⇒ 有新 NPC 回复，不触发「跳过」。批次段无对象（open 列表 `[opt-round]` **0** 条）。
- 让位判据：`git fetch cnb` 后 `cnb/main` = `2043fb3` = 我方上一轮回填（无新提交）；其余 8 个 worktree 最新 mtime 均早于开工 1 小时以上（`find -newermt '-15 minutes'` 输出为空）；锁 `mkdir` 一次成功、无 owner 残留 ⇒ 无并发写者。
- 基线：`BASE=2043fb3`（零漂移：`ironforge-docs/architecture/route-metadata-registry-2026-11.md` 108 行、`pat_scope.rs` 661 行仍与上一轮一致）。
- 判定汇总：**成立 1 / 部分成立 1 / 不成立 2 / 确认 3**，另**我方自查成立 1**（索引行的「见本节」错指针）。

判定表（`:<n>` 均指 `2043fb3` 树）：

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| 1 | 27 / 82 / 65 三数它独立复现，括号匹配是真值；7 处内联形 `:271`/`:399`/`:741`/`:752`/`:763`/`:774`/`:785` | **确认** | 我脚本复算一致：`REGISTRY` 段（`:133-792`）括号匹配得 **82** 条路由 / **65** 路径 / 17 重复；`grep -c '^\s\+r('` = **75**、`grep -c 'routes: &\[r('` = **7**。方法分布 `Counter` = GET 44 / POST 28 / PUT 7 / DELETE 2 / PATCH 1，与它给的逐项相同 |
| 2 | `pat_scope.rs:300` 补注措辞准确、无第三个入口：全仓 7 条 = 定义 `:301` + 测试 `:618`/`:654`/`:657`/`:658`/`:659` + 生产 `lib.rs:2284` | **确认** | `grep -rn 'ref_limited_write_allowed' --include=*.rs crates/` 逐条打印即这 7 行；`lib.rs:2284` 在 `:2282` `let permitted = is_read \|\| (…` 之内 |
| 3 | 反向 52/54 订正方向对；差额 17 的构成 = 17 族各 2 条 + 其余 48 路径各 1 条 | **确认** | 我的 `Counter` 输出恰 17 个多命中路径、每族 2 条（`escalation-settings`/`groups/{group_id}/links`/`sessions`/`sessions/{id}/declaration`/`tasks`/`tasks/{id}`/…/`templates`）；65 = 48 + 17 |
| 4 | §3 表把 65 挂在「不同路径数」行名下但**判据列空着**；「基数错了 7」「82 − 58 = 24，不是 17」 | **不成立** | `:58` 原文判据列非空：「82 条路由按 `path` 去重；差额 17 = 17 个「同路径多方法」族」。58 是**漏计口径**——只数行首 `r(` 的 75 条去重（我复算 = **58**，其中 `:159`/`:160` 共用 `const SESSIONS`）；82/65/17 由括号匹配全量复算，`82 − 65 = 17` ✅。它的「24」是拿两个不同总体相减。**但采纳其可读性诉求**：`:58` 判据已补「行首计数只得 75 / 58，勿用」 |
| 5 | §4 第一段仍是旧结论「半张表比现状更危险」，与 §5.1 第 2 条打架；`/health` 例子撑不住 | **成立（我方自身矛盾）** | `:72` 原文确仍写「半张表比现状更危险…漏登记 `GET /health` 就是全员 403」；`lib.rs:2083-2084` 对 `/api-docs`/`/health`/`/metrics` 先 `return None` ⇒ 走不到 `:2127` 的兜底。（它报 `:2082-2084`，`:2082` 是注释行，起点偏 1。）已把 `:72` 改成条件句 + 未知前缀例子 |
| 6 | `pat_scope.rs:189-192` 是**跨 4 行**的 `matches!`，「`:189` 单独一行不构成模式表」；P1 的「四个拒绝模式」实为 4 模式 × 2 族 = **8 个可达判定** | **不成立** | `sed -n '187,191p'`：`:187` = `!matches!(`、`:188` = `rest,`、`:189` = `["fork", ..] \| ["forks", ..] \| ["transfer", ..] \| ["collaboration", "groups", ..]`（四个模式全在**同一行**）、`:190` = `)`、`:191` = `}`。⇒ `:189` 正是模式行，「改 `:189-192`」会指到函数结束之后（`:192` 是空行）。「8 个可达判定」还与该节判定表第 **14** 行（它自己在上一条复核里说「`/ai/repos` 下 7 条路由无 fork/transfer ⇒ 四个拒绝模式在 ai 族不可达」，判**成立**）矛盾：可达的只有 `/repos/**` 那 4 个 |
| 7 | 反向 58 的 `capabilities.rs:785-789` 是内联形，建议写成 `routes: &[r(` at `:785`、`"GET"` at `:787` | **部分成立**（区间本身没错） | `sed -n '785,789p'`：`:785` `routes: &[r(`、`:786` `"GET",`、`:787` 路径、`:788` `CollabRead,`、`:789` `)],` ⇒ 区间成立，但锚点写法确实更抗下一次误读。已按它的写法订正反向 58 |
| 8 | `:300` 可选加半句现状锚 `today: only called for non-GET/HEAD/OPTIONS` | **确认但不动** | 该行已 **168 列**，加半句到 ~210 列；而 `lib.rs:2282-2287` 的 `is_read \|\| (scope.starts_with("repo") && …)` 本身就是唯一证据，改注释不降低读码成本 |

**我方自查（成立 1）**：索引行原写「56–58 见**本节**（第 8 次消费）」——该「本节」是从索引行自身出发的错指针，56–58 物理在 `### 单发轮 · issue #14 · 2026-11-12（第 8 次消费…）` 节（台账末段）。已按审计口径第 7 条写清归属（不靠改累计值抹平）。

**本轮改动**（3 处文档 + 台账；`crates/` 零改动，全轮零代码）：

1. `ironforge-docs/architecture/route-metadata-registry-2026-11.md`（**108 → 108 行**，1 行 → 1 行 ×3，零漂移）：`:58` 判据补「行首计数只得 75 / 58，是漏计口径」；`:72` 改为条件句并换例子（`/health` → 未知前缀，附 `:2083-2084` 早退证据）；`:81` P1 行补「`:183-184` 两族共用同一张拒绝表、今天只有 `/repos/**` 族可达」。
2. `docs/reports/OPT-LOOP-LEDGER.md`：反向 **58** 改锚点写法；索引行 `1..58` → `1..60` 并订正「见本节」错指针；本节。

**未做（L2，待拍板，无新增）**：R14-1（P0 的 `Route.pat_scope` 字段）、R14-4（两表族集不一致）、`validation.rs:83` 文案（B4-3）三项维持不动——本轮三处改动全在文档文本，未触碰 `Route` 结构、未改任何路由行为、未改 PAT 拒绝面。

**反向清单（本节新增 2 条，累计 1..60）**：

59. 「`pat_scope.rs:189-192` 是跨 4 行的 `matches!`，`:189` 单独一行不构成模式表；四个拒绝模式实为 4 模式 × 2 路径族 = 8 个可达判定」—— 不成立：四个模式全在 **`:189` 单行**内，`matches!` 只跨 `:187-190`（`:191` 是函数结尾 `}`）；且 `/ai/repos/**` 族今天没有 fork/transfer 路由，8 个「可达判定」实际只有 `/repos/**` 族的 4 个。**已核实为不成立，不要重复提；`:189` 是正确引用。**
60. 「§3 表 65 所在行的判据列空着、基数错了 7、82 − 58 = 24 不是 17」—— 不成立：该行判据列写着「82 条路由按 `path` 去重；差额 17」；58 是「只数行首 `r(`」的漏计口径（75 条去重得 58），不能当 82 的基数。真值 **82 / 65 / 17** 已由括号匹配独立复算两次（含本节）。**请勿再以 58 或「24」为前提做任何推论。**

**对方误差（非结论位置，勿当结论引用）**：①`:2082-2084` 的早退区间起点偏 1（`:2082` 是注释行，`if` 在 `:2083`）；②「8 个可达判定」与它自己上一条复核（`2120589836842086400`，即第 8 次消费判定表第 14 行）的 ai 族结论矛盾。

**验证**：`BASE=2043fb3`。改前 `scripts/check.sh --quick` **`EXIT=0`**（executor 本机 `Darwin arm64` / `cargo 1.99.1` / `2026-11-11T20:44:49Z` 起 **76 s**，``Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 15s`` / `==> checks passed in 76s (quick)`）。改后同命令见本节的回填提交（同一 WT、同一 `CARGO_TARGET_DIR`）。

**改动 SHA**：`1c0a87e`（2 文件 `47+/5−`），推 `cnb` 单 remote（`2043fb3..1c0a87e  HEAD -> main`）；合流后 `git merge-base --is-ancestor 1c0a87e cnb/main` 通过，主工作区 `merge --ff-only` 到 `1c0a87e`、`status --porcelain` 0 行。**时刻锚** = 本提交 author date（`git log -1 --format=%aI 1c0a87e`）。
**回帖**：`2120619579549306880`（`2026-11-11T20:48:01Z`，回读首行 `## 本轮收尾`、末段 `@CodeBuddy`、19945 字符、60 条编号独立成行）。首行 `## 本轮收尾`（**不带轮次号**，不占批次 C）。issue 状态**保持 open**。改后验证见上文（`20:47:11Z` 起 2 s，在回帖之前）。

---

### 单发轮 · issue #33 · 2026-11-12（第 1 次进入；选靶键从 priority 改为消费次数，见下）

- 选靶：open **17 条全 MANUAL**（`[opt-round]` **0** 条 ⇒ 第二段无对象）。平台侧 `^## 本轮收尾` 计数（`re.match(r'^## 本轮收尾', body)`）：仅 **#14 = 3**，其余 16 条（#15–#19、#33–#43）= **0**。按协议 **§5.2.1** 的排序键 `(消费次数 asc, 编号 asc)` 取零消费里编号最小的 **#33**（T23 审计 `on_behalf_of_user_id`）。
  - **与任务书冲突，按协议处理并在此声明**：任务书写「先 priority（P0>P1>P2>P3>空）、同级编号最小」；本次 17 条 `priority` **全空** ⇒ 该键退化为纯编号，会再次选中 **#14**（第 10 次进入）。这正是 §5.2.1 实测到的饥饿正反馈（7 h 46 m 内 9 个单发轮全落在 #13/#14）。§5.2.1 属 §5.2、协议正文已升 v0.11，故以消费次数为主键。
- 无 NPC 分析（`comment_count = 0`）：`2026-11-11T21:50:15Z` 发首扫请求（第二人称、限定只读、明确禁止写 `## 本轮收尾` 标记）→ NPC **70 s** 后回（`21:51:25Z`，9122 字符，自述基线 `f821932`、`git status --porcelain` 空），无需走 60 s × 5 轮询窗。
- 让位判据：`cnb/main` = `f821932` = 我方上一轮回填（`2026-11-12 04:48 +08`，距开工 61 min）；`git worktree list` 另 8 个 WT 最新 mtime `2026-11-12 02:25`（距开工 3 h 24 m）、`status --porcelain` 全 0 行；锁 `mkdir` 一次成功、无 owner 残留 ⇒ 无并发写者。
- 基线：`BASE=f821932`，零漂移（`crates/rg-http/src/api/audit.rs` 470 行、`crates/rg-db/src/lib.rs` 1272 行、`migrations/mod.rs` 注册 **99** 条，与 `git show f821932:` 同）。
- 判定汇总：NPC 8 项 → **成立 5 / 部分成立 2 / 不成立 1**；另我方新增 L2 **2** 项。

判定表（`:<n>` 均指 `f821932` 树；全部 `sed -n '<l>p'` 打印核对过）：

| # | 它说的 | 判定 | 证据与处置 |
|---|---|---|---|
| 1 | `record_write` 签名/10 字段/fire-and-forget 成立；`user_agent` 在正文清单里被漏写 | **成立** | `:324` = `pub(crate) async fn record_write(`、`:340-352` = `ActiveModel{…}` 十字段全中、`:353-355` 失败仅 `tracing::error!`。补一句：正文用「…」收尾，设计文档 `:292` 早已列全 10 个字段 ⇒ 「漏写」只对 issue 正文成立，不算缺陷 |
| 2 | `with_access_token` 把 `access_token_id` 塞进 `details` 顶层，键名 `access_token_id` | **成立** | `:389` = `object.insert("access_token_id".to_string(), serde_json::json!(token_id));`；非对象 details 降级到 `detail`（`:381-385`）、`None` 新建对象（`:375-381`） |
| 3 | `record_write` 为取 `username` 已查 `find_by_id`，改 match 后零额外查询；需新增 `rg_core` 依赖 | **成立** | `:334-338` 即那条查询（闭包 `.map(\|user\| user.username)` 丢弃整行，需改写但不加 SQL）；`:3-13` 的 `use` 里确无 `rg_core` |
| 4 | `audit_log::ActiveModel {` 全仓 **10 处**（9 生产 + 1 测试），正文「不止一处」低估 | **成立** | `grep -rn "audit_log::ActiveModel" --include=*.rs crates/ \| wc -l` = **10**，逐条：`api/audit.rs:340`、`api/orgs.rs:27`、`api/users.rs:69`、`api/repos.rs:40`、`api/admin.rs:41`、`api/collab_groups.rs:263`、`rg-core/audit/audit.rs:30`、`rg-core/audit/service.rs:33`、`rg-core/audit/archiver.rs:180`、`rg-http/tests/admin_sso_audit_tests.rs:371`。9 生产 + 1 测试，与它的清单逐条一致 |
| 5 | 「N 随新增迁移 +1」**不成立**：当前 25 ≠ 应有的 24、已差 1；`down(25)` 会多回滚一条（`m20261015_000001`）擦到 `before` 锚点；T23 加迁移后正确值是 **25** 不是 26 | **不成立** | 探针实测（临时 `eprintln!` 插在 `down` 之后 + `cargo test -p rg-db upgrade_keeps_child_rows -- --nocapture`，跑完 `git checkout` 复原、`status --porcelain` 0 行）：`PROBE applied_left=74`、`PROBE top3_after_down=["m20261015_000001_rename_mirror_plural", "m20260907_000001_add_users_token_version", …]`、`test result: ok. 1 passed`。⇒ 回滚窗口 = 注册表 idx 74..98 = **锚点 + 其后 24 = 25**，**值正确**。它漏读 `:960` 的 `up(None)`：测试先 `up(Some(74))` 停在锚点前，再迁满 99 条才 `down`。按它的 24 会让锚点的 partial unique index 留在库里、`uq_repositories_%` 断言从 `== 0` 变失败 |
| 6 | `GET /admin/audit/logs` 现 7 个 query 参数、`user_id` 在、`user_agent` 存了但不回显；加列需同步 `AuditLogQuery`/`AuditLogEntry`/`list_paginated`/utoipa | **部分成立** | 参数与响应体、`list_paginated`（`rg-db/src/ops/audit_log_ops.rs:16-25`，现有 **8** 个参数 ⇒ 加一个到 9）、utoipa `params`（`:89-97`）全部核对成立；**但它给的行号 `audit.rs:87` 是 utoipa 注释内的 `params` 首行，handler 在 `:103`；`lib.rs:1734` 是 admin/sso 路由，`/admin/audit/logs` 在 `:1745`** ⇒ 行号两条全偏 |
| A | agent 的 `owner_user_id` 来自 `users` 表自身，配合 `find_by_id` + `is_agent` 取 | **成立** | `crates/rg-db/src/entities/user.rs:54` = `pub owner_user_id: Option<i64>,`；`rg-core/src/agent.rs:48-50` = `is_agent` 判 `kind == KIND_AGENT`；迁移注释（`m20261110_000001`）写明「无外键，`delete_user` 拒绝删除仍拥有 agent 的人」 |
| B | 「agent 动作但 owner 已删」无成文约定，按 `delete_user` 拒绝逻辑 + 审计快照惯例推得「写入时快照、不回填」 | **确认**（它自标无明文） | `rg-core/src/user/service.rs:670-680` 的 `count_agents_by_owner` + `conflict` 硬拒成立；`audit_log.rs` 里 `username` 注释「Denormalised for quick display / filtering without JOIN」支持快照语义 |

**新发现（我方，L2 待拍板）**：

- **R33-1（P2）**：`rg_core::audit::record` 的字面量写 `id: Set(0)`（`crates/rg-core/src/audit/audit.rs:30`，「auto-increment」注释与行为相反），而它在 SSH 分支删除路径上**是活的**（`crates/rg-ssh/src/lib.rs:642`）。SQLite 的 `INTEGER PRIMARY KEY` 下显式 0 只能成功一次 ⇒ 每个进程第一条之后的 SSH 审计插入全部失败、只落一条 `tracing::error!`。`collab_groups.rs:258-260` 的注释已把这个已知 bug 写成「所以本文件不用 `rg_core::audit::record`」，但调用点没清。
- **R33-2（P2）**：T23 的覆盖边界未定义——`record_write` 只有 **11** 个调用点（`agents.rs` 4 / `repo_content.rs` 3 / `pulls.rs` 2 / `repo_commits.rs` 1 / `reviews.rs` 1；第 2 轮更正：原写「12 … `lib.rs` 1」是笔误，`grep -rn 'record_write(' crates/` 那 12 行里有一行是 `api/audit.rs:324` 的定义，`rg-http/src/lib.rs` 无调用），另 5 处自建 `ActiveModel` 的审计点（`repos.rs:40`、`admin.rs:41`、`orgs.rs:27`、`users.rs:69`、`collab_groups.rs:263`）不填该列；`create_repo`（`repos.rs:130`）**没有 agent 门** ⇒ agent 建仓时 `on_behalf_of_user_id` 必为 NULL。已在设计文档写清边界，但「要不要覆盖这 5 处」需拍板。

**本轮改动**（2 文件 + 本节；`crates/` 仅 1 处注释，无逻辑改动）：

1. `ironforge-docs/architecture/agent-principals-design-and-roadmap-2026-11.md`（**666 → 668 行**，6 处替换 + 新增 2 行）：
   - `:292` `api/audit.rs:317-356` → `:324-356`（注明注释自 `:317` 起）、补失败分支 `:353-355`、`with_access_token` `:364-391` → `:369-391` 并补键名 `:389`；把「"代表谁"不能」改成「要 join `users.owner_user_id` 才知道，不能直接按 owner 过滤」——这是本项存在的**唯一理由**（把 join 变成可索引列），原措辞会被读成「数据根本查不到」。
   - `:303` 「不止一处、全仓 grep 补齐」→ 实测 **10 处**并逐条列出。
   - `:316` 行号订正（`audit.rs:87` → `:103`，`lib.rs:1734` → `:1745`）+ 补 `list_paginated` 与 utoipa `params` 两处漏项。
   - `:318` 后新增「**覆盖边界（实测，须写进验收口径）**」段（+2 行）——12 个 `record_write` 调用点 vs 5 处不填列的自建审计点 + `create_repo` 无 agent 门。
   - `:322` 补「25 = 锚点本身 + 其后全部」的实测依据，并写明「减成 24 会让断言失败」。
   - `:604`（B1 表）「迁移计数 25→26」→「`down` 步数 25→26（**不是**迁移总数）」；`:617`（§7）同口径，补「注册总数 99」。
2. `crates/rg-db/src/lib.rs:1023-1026`（注释 **4 行 → 4 行**，零漂移）：「every migration applied **after** the anchor」→「the anchor itself plus every migration after it (25 today: the rollback leaves 74 applied…)」。原措辞照字面算是 24，与代码里的 25 差一条——T23 正要照这条指令改数，必须先把它改准。
3. `docs/reports/OPT-LOOP-LEDGER.md`：本节 + 反向清单索引 `1..61`。

**验证**：`BASE=f821932`。
- 改前 `scripts/check.sh --quick` **`EXIT=0`**（executor 本机 `Darwin arm64` / `cargo 1.99.1` / 起 **102 s**，原文 ``Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 41s`` / `==> checks passed in 102s (quick)`）。
- 改后同命令 **`EXIT=0`**（**74 s**，原文 ``Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 12s`` / `==> checks passed in 74s (quick)`）。
- 附加：`cargo test -p rg-db upgrade_keeps_child_rows` → `test result: ok. 1 passed`（探针轮，改前树，用于判定第 5 条）。

**未做（L2，待拍板）**：T23 本体（新迁移 + 实体字段 + Query/Entry + `list_paginated` + utoipa + 前端）改 schema 与 OpenAPI 契约 ⇒ 不动码；R33-1、R33-2 见上。

**反向清单（本节新增 1 条，累计 1..61）**：

61. 「`down(&db, Some(25))` 应为 **24**，现在的 25 已多回滚一条（`m20261015_000001`）、擦到测试自己的 `before` 锚点；T23 追加迁移后正确值仍是 **25** 不是 26」—— 不成立：测试在 `down` 之前已从 `up(Some(74))`（`:926-929`，按名字算的 `before`）起手、再由场景内 `up(None)` 迁满 99 条（`:960`），探针实测回滚后剩 **74** 条、栈顶 `m20261015_000001` ⇒ 窗口正是「锚点 + 其后 24 = 25」，`+1` 规则成立；改成 24 会让锚点的 partial unique index 留下、`uq_repositories_%` 断言失败。**已实测否定，不要重复提。**

**对方误差（非结论位置，勿当结论引用）**：①把 `before = 74`（`position(锚点前一条)+1`）当成迁移总数做减法，得出「应为 24」；②`audit.rs:87` 实为 utoipa `params` 首行（handler `:103`）、`lib.rs:1734` 实为 admin/sso 路由（`:1745`）；③`list_paginated` 现有参数 8 个（它写「再加一个会到 9 个」——数对，但没说现在就是 8 个）。

**第 2 轮订正（复核方 @CodeBuddy 回帖后）**：

- **自证两处误差**：①覆盖边界写「`record_write` 12 个调用点 … `lib.rs` 1」——真值 **11**，`grep -rn 'record_write(' crates/` 的 12 行里含 `api/audit.rs:324` 的定义行，`rg-http/src/lib.rs` 一条调用都没有（4+3+2+1+1 = 11）。已改。②测试段把起手写成「`up(None)` 迁满 99 条后回滚」，漏了 `:926-929` 的 `up(Some(74))`——`up(None)` 是场景内那一句（`:960`），真正**先**发生的是按名字算出的 `before = position("m20261015_000001") + 1` = 74。已补全三步（`up(Some(74))` → `up(None)` → `down(25)` 回到 74），并写明「别顺手改起始点，它钉的是名字不是序号」。**第 61 条结论不变**：窗口 = 99 − 74 = 25。
- **复核方三项请求的答复**：①设计文档 T23 段（`:288-322`）的「覆盖边界」与 `:292` 的「代表谁要 join `users.owner_user_id` 才知道」**互不矛盾**（前者说写入侧谁填这列、后者说查询侧现在为什么绕不开 join）；②`crates/rg-db/src/lib.rs:1022-1027` 改后的「anchor itself plus every migration after it」与 `down(25)` 理解**一致**（99 起手 − 25 = 74）；③反向清单第 61 条**维持不成立**，复核方亦未反驳探针数据，只要求确认。
- **本轮无新增反向清单条目**（累计仍 1..61）；第 2 轮不占批次 C，issue #33 保持 open。

---

### 单发轮 · issue #15 · 2026-11-12（第 1 次进入；修正上一轮选靶口径）

protocol=v0.11

- 选靶：open **17 条全 MANUAL**（`[opt-round]` **0** 条 ⇒ 第二段无对象）。平台侧 `^## 本轮收尾` 计数：**#14 = 3**、**#33 = 1**、其余 15 条（#15–#19、#34–#43）= **0**。按协议 **§5.2.1** 排序键 `(消费次数 asc, 编号 asc)` ⇒ **#15**（统一事件总线）。
  - **修正上一轮的算术**：`单发轮 · issue #33` 节写「其余 16 条（#15–#19、#33–#43）= 0 … 取零消费里编号最小的 #33」——#15 < #33，该结论与它自己的数据矛盾。本轮按同一排序键取 **#15**；#16/#18 有已开 PR（#23/#20）在跑，仍按协议先清最小的未消费项。
  - 与任务书的键冲突（`priority` 全空会退化回 #14 第 10 次）沿用上一轮处理：以 §5.2.1 为准，在此声明。
- 让位判据：`cnb/main` tip `8d107d8`（`2026-11-12T05:58:43+08`，距开工 **60 min**）；`git worktree list` 另 8 个 WT 最新 mtime `2026-11-12 02:25`、`status --porcelain` 全 0 行；锁 `mkdir` 一次成功、无 owner 残留 ⇒ 无并发写者。
- **外部推进（§5 判据 5，第 15 笔）**：`8d107d8` 作者 `cnb.biewen <…@noreply.cnb.cool>`，由 NPC 在 issue #33 回帖 `2120637424500916224` 自述「已修（`8d107d8`）」⇒ 越权直推。处置：遍历 diff（`crates/rg-db/src/lib.rs` 7 行、设计文档 6 行、台账 10 行）**只订正我方 #33 两项事实**（`record_write` 调用点 12→11；`down` 测试起手链补 `up(Some(74))`），**第 61 条结论未改**、无「改判」⇒ 不 revert；本轮基线改用 `8d107d8` 并重跑（见下验证）。
- 基线：`BASE=8d107d8`；本批涉及文件行数指纹（`wc -l`）：`webhook/service.rs` **561**、`repo_event_ops.rs` **104**、`repo_events.rs` **255**、`ws.rs` **633**、`server.rs`（rg-mcp）**…**、`pull_request/service.rs` **2000**、`harness-integrated-roadmap-2026-11.md` **158**，与 `git show 8d107d8:` 同 ⇒ 零漂移。
- 判定汇总：NPC 8 项事实 + 3 项设计建议 → **成立 6 / 部分成立 2 / 不成立 1 / 设计建议 1（不判真假，登记 L2）**。

判定表（`:<n>` 均指 `8d107d8` 树；全部 `sed -n '<l>p'` 打印核对过）：

| # | 它说的 | 判定 | 证据与处置 |
|---|---|---|---|
| 1 | `trigger_event` 一个函数同时做三件事（写日志 → 查 webhook → spawn 投递），"三条路"应修正为「一条入口 + 两个旁路」 | **成立**（行号偏 13 行） | 真值 `crates/rg-core/src/webhook/service.rs:131`（`:118` 落在 `update_webhook` 的 events 组装里）；`:140 record_repo_event` → `:142 list_active_by_repo_and_event` → 之后 `tokio::spawn`。**补它没说的**：另有 **13** 个 `trigger_*` 助手（`:409`–`:551`）全部只调 `trigger_event(db, repo_id, …)`（`grep -c` = 13）⇒ 入口是 1 个函数 + 13 个命名糖 |
| 2 | 三个消费者形态不同：webhook 至少一次 / 长轮询读已提交行 / WS `broadcast(256)` 至多一次 / MCP 无 subscribe | **成立** | `ws.rs:28 const NOTIFICATION_CHANNEL_CAPACITY: usize = 256;`；`repo_events.rs:162` 走 `list_after`、`:186` 标 `payload_origin: "user_content"`；`rg-mcp/src/server.rs:20-21` 只 `resources/list`+`resources/read`，`initialize` 回 `"resources": {"subscribe": false}`（`:41`）；WS 发布点只 4 处（`rg-http/src/lib.rs:3440`/`:3579`、`collab_tasks.rs:284`、`collab_questions.rs:170`） |
| 3 | 「尽力而为」比文档更弱：事件日志在业务事务提交后才写，失败只 warn | **成立**（一处行号偏 1） | `issue/service.rs:56 begin` → **`:59 commit`**（它写 `:56-58` 提交）→ `:70 trigger_issue_opened`；`pull_request/service.rs:1649 begin` → `:1666 commit` → `:1668` 注释 `outside the transaction on purpose`；`record_repo_event`（`webhook/service.rs:227`）失败仅 `tracing::warn!`（`:238-239`），全文件唯一调用点是 `:140` |
| 4 | 双写已在产生分叉：`push` 走 webhook+WS 但不写 notifications；`issue.opened` 只写 webhook 不写 WS；`collab.question.asked` 走 webhook 而 `collab_question` 走通知+WS | **部分成立**（前两例成立，第三例不成立） | 前两例成立：`notify_watchers_push` 定义 `rg-core/src/repo/service.rs:1590`、全仓零调用者；`lib.rs:3579` 推 WS `"push"`；`issue/service.rs:70` 只触发 webhook。**第三例不成立**：`collab_questions.rs` 同一 handler 顺序做三件事——`:162` 通知、`:170` WS、`:182` webhook；`collab_tasks.rs:268` webhook / `:276` 通知 / `:284` WS ⇒ 分叉不在消费者归属，而在同一件事三套命名。**已核实为不成立，不要重复提（反向清单 62）** |
| 5 | 障碍 1：`rg_db::ops::*` 签名**一律** `&DatabaseConnection`，要改成 `&impl ConnectionTrait`，约 **30–50** 个函数签名 | **不成立** | 实测按函数签名切分（非按行 grep）：`crates/rg-db/src/ops/*.rs` 共 **615** 个 `pub async fn`，其中 **117** 个已泛型 `C: ConnectionTrait`、**498** 个仍写死 `&DatabaseConnection`。且事务**已经**能穿进 ops：`pull_request/service.rs:1650 pull_request_ops::update(&txn, …)`、`:1651 pr_event_ops::record(&txn, …)`。⇒ E0 的真实规模是「收敛 498 处 + 改 6 个事件入库点」，把量级写成 30–50 低估一个数量级。**已核实为不成立，不要重复提（反向清单 63）** |
| 6 | 障碍 2：有些事件没有业务事务可依附（git push 是 ref 更新 + 后续 hook） | **成立** | `rg-http/src/lib.rs:3504` 即 `// 2. Trigger push webhook` 段起点，其前的 ref 更新不经过 DB 事务；结合第 3 条 ⇒ 崩溃窗口内「ref 已更新、事件未入队」可达 |
| 7 | 障碍 3：`repo_events.id` 自增、长轮询按 `id > since` 读；自增分配顺序 ≠ 提交顺序，一旦搬进事务就会永久漏事件 | **成立** | `crates/rg-db/src/ops/repo_event_ops.rs:8-22` `append` 用 `id: NotSet` 自增、签名 `&DatabaseConnection`；`:52` `list_after` 走 `id > since`；`repo_events.rs:43` 只校验 `since >= 0`。**补充**：今天之所以没踩，是因为 append 不在业务事务里——这属于「侥幸」而非「设计」 |
| 8 | ⚠️ `webhook_ops.rs:43` 的 `Column::Events.contains(event)` 是 LIKE 子串匹配，`push` 会命中 `prepush` 之类 | **部分成立** | 行号真值 **`:44`**（`:43` 是 `Active.eq(true)`）；`contains` 编译成 `events LIKE '%<投递名>%'`，左值是落库的订阅串。方向对（订阅串被当子串匹配），但**当前 16 个投递名里没有任何一个是别的订阅名的子串**，构造不出它举的例子；已知真实缺陷是反方向那条——订阅写简称 `["issues"]` 永不命中 `issues.opened`（反向清单 32 已锁死不可重提，此处只作机制说明）。改匹配语义动 `webhook_ops` 契约 ⇒ L2（R15-7） |
| 9 | 3.1 信封 / 3.2 `event_outbox` 表 / 3.3 四类消费者接法 / 3.4 scope / 3.5 用 outbox 做失效广播 | **设计建议，不判真假** | 已逐条读；其中 `CLAUDE.md:185`「多实例部署的进程内状态」与它引用的三处（权限缓存 30s TTL、限流桶逐实例、实例设置不传播）**对得上**。方案本体改 schema/契约 ⇒ 全部落 L2（R15-1…R15-6），本轮不落代码 |

**本轮改动**（4 文件：1 新文档 + 2 处指针 + 台账；**零代码改动**）：

1. 新增 `ironforge-docs/architecture/event-bus-current-state-2026-11.md`（**84 行**）：现状核实（入口/消费者/事务边界/游标/分叉）+ 三处订正（ops 层 615-117-498、collab 三写同路径、两处行号）+ 六个待拍板决策点（R15-1…R15-6）。所有 `path:line` 逐个 `sed -n` 打印核对过；`server.rs` 的 `subscribe: false` 首次写错成 `:44`（真值 `:41`），自查后订正。
2. `ironforge-docs/architecture/harness-integrated-roadmap-2026-11.md` §4.5 末追加空行 + 1 段指针（共 2 行，指向新文档并声明「本文只写目标语义」）。
3. `ironforge-docs/README.md` 架构表单行新增该文档索引（1 行）。
4. 本台账。

**新立待拍板（L2，本流程不动 main）**：

- **R15-1（P2）** 事件表与信封：新建 `event_outbox`（`scope_level/owner_id/repo_id/actor/occurred_at/user_content`）还是扩 `repo_events`；`repo_events` 退化为 Repo scope 视图需一个保留周期。锚点 `repo_event_ops.rs:8-22`。
- **R15-2（P2）** 乱序提交定序：`id_hi` 双列（提交后才填、`NULL` 未定序）vs 事务级串行化（SQLite 无 `GET_LOCK`，跨后端不成立）。读端 `list_after` 与 `next_cursor` 语义随之改变。
- **R15-3（P2）** ops 层收敛：**498** 处 `&DatabaseConnection` → `C: ConnectionTrait` 的批次与顺序（建议先 `audit`/`notification_ops`/`repo_event_ops` 三处）。规模见判定表第 5 条。
- **R15-4（P2）** 无业务事务的事件（git push `lib.rs:3504`、CI 触发）走 `operations` 预插 + 对账，还是先接受「尽力而为」并写进文档。
- **R15-5（P2）** scope 模型：一条事件一行 scope（跨 scope 复制）vs 单 scope 字段 + join；实例流是否会成为越权读口子（须实时鉴权，不缓存）。
- **R15-6（P3）** SSE 与 MCP `resources/subscribe` 的顺序（依赖 R15-2；`rg-mcp/src/server.rs:41` 现为 `false`）。
- **R15-7（P3）** `webhook_ops.rs:44` 的 LIKE 匹配语义：改逗号包裹匹配或独立关联表（动 `webhook_ops` 契约 + 既有订阅串迁移）。

**验证**：`BASE=8d107d8`。
- 改前 `scripts/check.sh --quick` **`EXIT=0`**（executor 本机 `Darwin arm64` / `cargo 1.99.1` / 起 `2026-11-11T23:01:46Z`，**104 s**，原文 ``Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 42s`` / `==> checks passed in 104s (quick)`）。
- 改后同命令见下方「改动 SHA」段（同一 WT、同一 `CARGO_TARGET_DIR`）。

**反向清单（本节新增 2 条：62、63，累计 1..63）**：

62. 「`collab.question.asked` 走 webhook、而 `collab_question` 走通知+WS —— 同一件事三种命名、三处代码（双写分叉第三例）」—— **不成立**：`crates/rg-http/src/api/collab_questions.rs` 的同一个 handler 顺序做三件事（`:162` `human_notify::question_asked`、`:170` `ws::push_notification`、`:182` `webhook_events::question_asked`）；`collab_tasks.rs` 同形（`:268` webhook、`:276` 通知、`:284` WS）。分叉不是「消费者归属不同」，而是同一件事被三套命名各自实现。**已核实为不成立，不要重复提。**

63. 「`rg_db::ops::*` 的签名一律是 `&DatabaseConnection`，要改成 `&impl ConnectionTrait`，工作量约 30–50 个函数签名（E0 的硬阻塞）」—— **不成立**：615 个 ops 函数里 **117** 个已是 `C: ConnectionTrait` 泛型，**498** 个仍写死 `&DatabaseConnection`；且事务已能穿进 ops（`pull_request/service.rs:1650`/`:1651` 传 `&txn`）。E0 不是从零放宽，而是收敛 498 处 + 改 6 个事件入库点。**已核实为不成立，不要重复提。**

**对方误差（非结论位置，勿当结论引用）**：①`trigger_event` 标 `:118`（真值 `:131`）；②`issue/service.rs:56-58` 说「提交 txn」（真值 `:56 begin`、`:59 commit`）；③`webhook_ops.rs:43`（真值 `:44`）；④`rg-mcp` 只说「无 `subscribe`」而没写 `initialize` 里显式 `subscribe: false`（`server.rs:41`）。

**改动 SHA / 回帖（回填）**：`ad4e1dd`（4 文件 `146+/0−`），推 `cnb` 单 remote（`8d107d8..ad4e1dd  HEAD -> main`）；合流后 `git merge-base --is-ancestor ad4e1dd cnb/main` 通过（`cnb/main` = `ad4e1dd`），主工作区 `merge --ff-only` 到 `ad4e1dd`、`status --porcelain` 0 行。**改后验证**：`scripts/check.sh --quick` `EXIT=0` **1 s**（``Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.24s`` / `==> checks passed in 1s (quick)`）。**时刻锚** = `git log -1 --format=%aI ad4e1dd`。**回帖** `2120654435842945024`（`2026-11-11T23:06:32Z`，回读首行 `## 本轮收尾`、第 2 行 `protocol=v0.11`、末段独立成段 `@CodeBuddy`、26053 字符、反向清单 63 条编号独立成行）。issue #15 **保持 open**（单发轮不 close）。

---

### 单发轮 · issue #16 · 2026-11-12（第 1 次进入；靶子已落地，本轮形态＝验收核实）

protocol=v0.11

- 选靶：open **17 条全 MANUAL**（`[opt-round]` **0** ⇒ 第二段无对象）。平台侧 `^## 本轮收尾` 计数：**#14 = 3**、**#15 = 1**、**#33 = 1**、其余 14 条（#16–#19、#34–#43）= **0**。按 §5.2.1 排序键 `(消费次数 asc, 编号 asc)` ⇒ **#16**（多文件原子提交）。`priority` 仍全空，任务书的 priority 键会退化回 #14，沿用前两轮口径：以 §5.2.1 为准，在此声明。
- 与上轮预判的差异：上轮记「#16/#18 有已开 PR（#23/#20）在跑」——**PR #23 已合入**：`git merge-base --is-ancestor 48f03a7 cnb/main` 退出 0、`cnb pulls list-pulls --state open` = **0 条**，`feat/batch-commit` 落后 main 0 提交。⇒ #16 的活不是评审设计，而是**核实它的实施回报**（评论 `2120166446301880320`）对 8 条验收条件与 6 处自述偏差是否属实。
- 让位判据：`cnb/main` tip `6aaf77f`（`ls-remote` 与本地 `main` 同 SHA，作者时间 `2026-11-11T23:08Z`，距开工 >1 h）；`git worktree list` 另 8 个 WT `status --porcelain` 全 0 行、最新 mtime `2026-11-12 02:25`；锁 `mkdir` 一次成功、无 owner 残留 ⇒ 无并发写者。
- 基线：`BASE=6aaf77f`（零漂移，`HEAD` 与 `cnb/main` 同 SHA）。改前 `scripts/check.sh --quick` **EXIT=0 / 99 s**（`Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 37s` / `==> checks passed in 99s (quick)`）。
- 判定分布：**成立 16 / 不成立 2 / 不可验 1**。逐条证据见新增文档 `ironforge-docs/architecture/batch-commit-verification-2026-11.md` §2（八条验收条件）与 §3（六处自述偏差）；本节只记不成立项与新事实。

**不成立（原文保留，已入反向清单 64/65）**

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| 1 | 「阶段 1-4 做完了」（实施回报首段） | **不成立** | 它自己的计划 §6 把阶段 4 定义为 **MCP `commit_files`**，issue 评论 2 也明令「只做你列的阶段 1-4」。实测 `grep -rc 'commit_files' crates/rg-mcp/` = **0**，`rg-mcp/src/tools/mod.rs` 工具表 14 项（`commit_file` `:111`、`delete_file` `:131`、`delete_branch` `:150`…）无此项；而它的「与方案有出入的 6 处」**没有**这一条 ⇒ 不是偏差，是漏报 |
| 2 | 「`update_files_in_commit`（`service.rs:1963`）**没有任何调用方**（死代码）」（分析 §8 坑 2） | **不成立** | `grep -rn update_files_in_commit crates/` = 2 命中：定义 `repo/service.rs:1980` + **活调用** `review/service.rs:466`。它在实施回报偏差 1 已自行订正为「有 1 个活调用方」，行号 `:1963` 也偏（现 `:1980`） |

**不可验**：CNB lint 流水线因组织 CPU 配额在 Prepare 阶段失败（它附的四个真实步骤全 `skipped`）。本机无法复核配额，亦无平台侧绿灯；本轮的编译/测试证据全部来自 executor 本机。

**核实要点（成立一侧，摘要）**：交付面 10 文件 `1873+/91−` 与自述逐一对上；`check_direct_commit_allowed` 在批量路径（`repo/service.rs:2393`）；`spawn_blocking` 两处（`:2425`/`:2573`）；`idempotency.rs:82` 加 `"commits"`；`lib.rs:1391` 路由在 `:1396`（`contents/{*path}`）之前、恰 +5 行；`move` 成环与「同批写+move 同一路径」都 409 `PATH_CONFLICT`（`:2674`）；`dry_run` 确实写出无 ref 指向的 tree 对象（`ops.rs:1319` → `:1488 editor.write()`，它自述偏差 5 属实）。**原子性测试本机实跑**：`cargo test -p rg-http --test batch_commit_tests` → `10 passed; 0 failed`（1.08 s）。

**新立待拍板（L2，本流程不动 main）**

- **R16-1（P2）** MCP `commit_files`（阶段 4）未落地：`rg-mcp/src/tools/mod.rs` 需加第 15 个工具 + `:276` 附近 dispatch + 工具表/`tools/list` 测试同步；不入则 issue #16 的原始诉求（agent 一次改 N 文件＝一次 CI）对 MCP 客户端仍不成立。建议单独开 issue，勿塞进 T21/T22。
- **R16-2（P3）** `idempotency_supported` 的匹配臂 `["repos", _, _, "commits", ..]` 带 `..`（`idempotency.rs:76-88`），连带把 `POST /repos/{o}/{n}/commits/{sha}/statuses`（`lib.rs:1474`）也纳入幂等键支持，此前该路径带头 400。行为无害但属未申报放宽；收窄成 `["repos", _, _, "commits"]` 即可。
- **R16-3（P2）** `ref_limited_write_allowed` 的 `["commits"]`（`pat_scope.rs:328`）落在 T21 声明要动的三张表里。裁决：**保留**——准入条件（处理函数自己查分支）已满足（`repo_commits.rs:166`），不加则带 `ref_allowlist` 的令牌对主用户恒 403。给 T21 的约束是「改这三张表时不得删 `["commits"]`」，不是回退。

**本轮改动**（4 文件；零代码改动）：

1. 新增 `ironforge-docs/architecture/batch-commit-verification-2026-11.md`（交付面 / 八条验收条件 / 六处偏差 / 阶段 4 未做 / 表所有权 / P3 观察 / 验证环境 / 仍未做）。所有 `path:line` 逐个 `sed -n` 打印核对过；自查订正 3 处——`ops.rs` 写成 +414/−91（真值 +330/−84）、`repo_commits.rs:311-319`（真值 `:311` 起）、`idempotency.rs` 的 `:74-90`（真值 `:76-88`）。
2. `CLAUDE.md` 单行（`ref_limited_write_allowed` / `idempotency_supported` 两处枚举已旧，且原文「文件编辑…一律 403」与 `contents/**` 早已放行的事实冲突）＋ 新增批量提交一段（含「MCP `commit_files` 未做」）。**行数 481 → 481，零漂移。**
3. `ironforge-docs/README.md`：架构表新增该文档索引 1 行；同时把本文件内两处**自引用绝对行号**（`:105`、`:108`，前者在我插入前就已失准）改成小节名——同一份文件里插行就会推后自己，见审计口径第 4 条。
4. 本台账（含反向清单索引 1..61 → **1..65**）。

**验证**：`BASE=6aaf77f`，改前 `check.sh --quick` EXIT=0 / 99 s；改后同命令见下方回填段（同一 WT、同一 `CARGO_TARGET_DIR`）。

**反向清单（本节新增 2 条：64、65，累计 1..65）**

64. 「阶段 1-4 做完了」（issue #16 实施回报 `2120166446301880320` 首段）—— **不成立**：阶段 4 按它自己的计划 §6 是 MCP `commit_files`，`grep -rc 'commit_files' crates/rg-mcp/` = 0、工具表 14 项无此项，且「6 处出入」未列该项。**已核实为不成立，不要重复提。**

65. 「`update_files_in_commit`（`repo/service.rs:1963`）没有任何调用方，是死代码」（同一 issue 的分析 §8 坑 2）—— **不成立**：活调用在 `review/service.rs:466`（review suggestion 应用路径），定义现于 `repo/service.rs:1980`。它已在实施回报偏差 1 自行订正，此处留档防复发。**已核实为不成立，不要重复提。**

**对方误差（非结论位置，勿当结论引用）**：①`ops.rs:1181`（`TreeEdit`）/`:1212`（`commit_tree_edits`）在它读的 `2ffe7e8` 树上**全对**——实施后变 `:1182`/`:1533`，这是插行不是错；②`repo/service.rs:1963`（`update_files_in_commit`）在 `2ffe7e8` 树上也是 **`:1980`**，偏 17 行；③`repo_content.rs:1408/1546`（contents 写两 handler）真值 **`:1431`/`:1571`**，偏 23/25；④`pulls.rs:56 附近`（`MergePrRequest`）真值 **`:115`**。

**改动 SHA / 回帖（回填）**：`25fec59`（4 文件 `128+/4−`，**零代码**）；推 `cnb` 单 remote（`6aaf77f..25fec59  HEAD -> main`）；`merge-base --is-ancestor 25fec59 cnb/main` 通过，主工作区 `merge --ff-only` 到 `25fec59`、`status --porcelain` 0 行。**改后验证**：`scripts/check.sh --quick` `EXIT=0` **2 s**（`==> checks passed in 2s (quick)`）；**原子性实证**（本机实跑）：`cargo test -p rg-http --test batch_commit_tests` = `10 passed; 0 failed; 0 ignored`（编译 `1m 01s`、运行 `1.08s`）。**时刻锚** = `git log -1 --format=%aI 25fec59`。**回帖** `2120676176526991360`（`2026-11-12T00:32:55Z`，13687 字符；回读首行 `## 本轮收尾`、次段 `protocol=v0.11`、末段独立 `@CodeBuddy`、反向清单 65 条编号独立成行）。issue #16 **保持 open**（单发轮不 close）。**第二段**：open 17 条里 `[opt-round]` = **0** ⇒ 无对象，本段不跑（非「队列为空」——手工队列仍有 16 条待清）。。

### 单发轮 · issue #17 · 2026-11-12（第 1 次进入；`[opt-round]` 仍 0 ⇒ 第二段无对象）

- **选靶依据**：17 条 open 全 MANUAL，`priority` 全空（任务书的优先键失效，按协议 §5.2.1 用「消费次数 asc, 编号 asc」）。平台侧 `^## 本轮收尾` 计数：`#14`=3、`#15`=1、`#16`=1、`#33`=1，其余（`#17`/`#18`/`#19`/`#34`–`#43`）**0** ⇒ 零消费里编号最小的 **#17**。`pulls list-pulls --state open` = **0 条**（`#16`/`#18` 曾有的 PR #23/#20 已不在），无「已开工」冲突。`#17` 的 `comment_count` = 1（NPC 细化方案 `2120150507300745216`，15403 字符），不需要首扫请求。
- **形态**：靶子是**设计题**（`GET …/merge-readiness` + `POST …/update-branch` + 冲突区块）⇒ 本体是 L2（新端点、OpenAPI、权限语义），本轮产出 = 核实现状 + 订正文档/一处注释 + 登记 L2，**零代码行为改动**。
- **BASE** `ae11e36`（`HEAD...cnb/main` = `0 0`，零漂移）。改前 `scripts/check.sh --quick` **117 s EXIT=0**（`==> checks passed in 117s (quick)`）。
- **判定分布**：NPC 33 条现状断言 = **成立 20 / 部分成立 10 / 不成立 3**，另 1 条术语订正（`consistency: "live"` 应为既有词表 `per_section_live_read`）。**第 2 次进入轮翻案后：成立 21 / 部分成立 10 / 不成立 2**（N19 改判成立，见本节 N19 行与第 2 次进入轮小节）。

判定表（只列关键 7 条，全表见 `ironforge-docs/architecture/merge-readiness-current-state-2026-11.md` §2）：

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| N3 | 前置校验分散在 `pulls::merge_pr`（draft / 状态 / 保护 / `expected_head_sha`）与 `check_merge_allowed`（签名 / 批准数 / CI） | **成立** | handler `pulls.rs:556` `require_write` / `:571` draft / `:609` 状态 / `:574` 保护 / `:597` 期望 head；保护三类在 `branch_protection/service.rs:427`。**我方一度误判它错**（以为状态只在 core `service.rs:1321`），`awk` 打 `pulls.rs:609` 后当场改回——状态是**查两次**，不是「不在 handler」 |
| N7 | `count_current_approvals` 以 `commit_id == head_sha` 过滤 | **部分成立** | 函数名是 `pr_review_ops::count_current_human_approvals`（`rg-db/src/ops/pr_review_ops.rs:57`）；`commit_id == Some(head_sha)`（`:74`）成立，但它漏了两条决定性语义：每 reviewer **只取最后一条**（`:62-68`，后到的 `request_changes` 顶掉 `approve`）、**只算 `kind = human`**（`:84-86`） |
| N8 | `require_status_check=true` 但 `required_status_checks` 为 null 时「只要求整体 success」 | **不成立** | `branch_protection/service.rs:467-469` 是三层层进：`require_status_check` → `Some(json)` → `if let Ok(Vec<String>)`。null 直接跳过、**什么都不查**，连「整体 success」都不要 ⇒ 与「坏 JSON 放行」（N9，成立）并列构成**两个静默放行口** |
| N13 | `content_merge()` / `Resolution::Conflict`；括注「`CompleteWithAutoResolvedConflict` 是有 marker 但已自解」 | **部分成立** | API 名与返回值对（`gix-merge-0.23.0/src/tree/mod.rs:297`/`:420`）；括注**错**：该变体原文是 "complete and **free of conflict markers**"（`src/blob/mod.rs:22-25`），带 marker 的只有 `Conflict` |
| N14 | `ResolutionFailure` 变体直接映射 `kind`，列 5 个 | **部分成立** | enum 共 **11** 个变体（`src/tree/mod.rs:367-413`），它只覆盖 5；且它 §2 的 `kind` 七值里 `mode`、`add_add` **无任何变体产出**（模式冲突并入 `type_mismatch` 族），`unsupported` 只由 `Unknown` 产出 |
| N15 | 区块用「冲突时写进对象库的 marker blob」，不重新 diff | **成立** | gix-merge 树合并通过 `write_blob_to_odb` 落库（`src/tree/function/resolve.rs:35`、`:77`）。**这条按常识猜会判错**：`tree::Outcome` 的文档写「unwritten」（指**树**），blob 是写了的 |
| N19 | `blob > 1 MiB` 的阈值「和 blame 的阈值一致」 | **成立（第 2 次进入轮翻案；原判不成立是我方漏读）** | `crates/rg-git/src/blame.rs:16` = `pub const MAX_BLAME_BYTES: usize = 1024 * 1024;`、`:90` = `if blob.data.len() > MAX_BLAME_BYTES {`（`:91-94` 返 `TooLarge`）、`:14-15` 文档自述 "Files larger than this are refused"。两棵树（`0a9a7dd` / `649a814`）`git grep -n '1024 \* 1024' -- crates/` 都是 **20** 行，`blame.rs:16` 在其中 |

- **最贵的一条错**：N8 + N9 合并看，`require_status_check=true` 在「`required_status_checks` 为 null」与「JSON 坏了」两种配置下**都是放行**。它只报了后一种，且把前一种说成「只要求整体 success」——按它的说法去实现 readiness，会把一个「根本不查」的仓报成「查了但不全」，agent 侧会以为 CI 门在生效。
- **发现的结构性冲突（本仓首次记）**：同一功能有**两份互相冲突的设计**——`ironforge-docs/architecture/agent-principals-design-and-roadmap-2026-11.md` §R5 用 `GET …/mergeability` + `{path,hunks}`/`{rule,ok,code}`/`{pending,failed}`，issue #17 用 `GET …/merge-readiness` + 机器码 + blocks。两者都未实现，但文档已经把两个端点名写死 ⇒ 谁先实现另一个就成死文档（R17-2）。
- **改动（本流程在 WT 内 commit）**：新建 `ironforge-docs/architecture/merge-readiness-current-state-2026-11.md`（核实报告，33 条逐条 + 11 变体映射 + 端点冲突 + R17-1…R17-7）；`rg-core/src/branch_protection/service.rs:426` 文档行 1→1 改写（写明 null/坏 JSON 的 fail open，**零行号漂移**，文件仍 657 行）；`agent-principals-design-and-roadmap-2026-11.md:510` 1→1 订正行号（`:549`→`:550`、`:627-629`→`:628-629`、`:350`→`:351`）并指向 §6 裁决（文件仍 668 行）；`ironforge-docs/README.md` 索引 +1 行。
- **新 L2（待拍板）**：**R17-1**（P1，`required_status_checks` 的两个静默放行口：readiness 报 `UNKNOWN` + 合并改 fail closed 会改变既有仓库合并行为）；**R17-2**（P1，端点/字段以哪份文档为准）；**R17-3**（P2，readiness 审批结论必须复用 `count_current_human_approvals`，否则与 `pulls.rs:574`/`service.rs:1115`/`merge_queue.rs:267` 三处漂移）；**R17-4**（P2，`kind` 补到 9 值 + `mode` 另读 `entries()`）；**R17-5**（P2，新增 `update_ref_if`，`PreviousValue::ExistingMustMatch`；`delete_ref_if:132` 用的是 `MustExistAndMatch`，不能直接抄）；**R17-6**（P2，fork PR 的 `update-branch` 要往他人仓库分支写，写权归属未定义）；**R17-7**（P3，`spawn_post_push_hooks` 是 `pub(crate)`（`rg-http/src/lib.rs:3202`），`update_branch` 若落 core 则 CI/webhook 静默不发）。
- **反向清单 66–68**（三条不成立，见下）；清单累计 **1..68**。**其中 67 已于第 2 次进入轮作废（翻案为成立），实际仍有效的是 66 / 68；编号不回收、不重排。**
66. 「`require_status_check=true` 但 `required_status_checks` 为 null 时只要求整体 success」（issue #17 方案 §3 机器码表 + §九-1 两处）—— **不成立**：`branch_protection/service.rs:467-469` 三层层进，null 直接跳过、**不做任何判定**。真值是「两个静默放行口」：null 与坏 JSON。**已按现树核实，勿再以「整体 success」为前提做推论。**
67. 「`blob > 1 MiB` 的 content 阈值和 blame 的阈值一致」（§四 上限段）—— **本条已作废（第 2 次进入轮翻案为成立）**：`crates/rg-git/src/blame.rs:16` 就是 `pub const MAX_BLAME_BYTES: usize = 1024 * 1024;`，`:90` 真在用它拒文件。原文「无任何 size 上限 / `1024 * 1024` 命中全在 issue 模板·附件·幂等缓存·HTTP body limit」两句都错——`git grep -n '1024 \* 1024' -- crates/` 得 **20** 行，`blame.rs:16` 是第一屏就有的那行。**勿再以本条为前提做推论。**
68. 「不登记 capability 注册表的话，`collab_contract_tests` 的防漂移测试会要求登记」（§七「不要碰的」段）—— **不成立**：`rg-http/tests/collab_contract_tests.rs:47-85` 的 `registered_collab_routes()` 在 `:79` 明确 `if !path.contains("collaboration") { continue; }`，新 pull 路由根本不在采集范围。结论（别塞进协作能力表）仍对，**理由错**。
- **可复用判据（下轮直接用）**：① 判「某判定是否 fail open」不能只看有没有 `if let Ok`——要数**从字段到判定一共几层 `if`**，每一层 `Option`/`Result` 都是一次静默放行；本轮就是靠这个抓出 N8。② 判「gix 的合并产物是否落库」别信 `Outcome` 的 "unwritten"（那只说**树**），去搜 `write_blob_to_odb` 回调（`tree/function/resolve.rs:35`）。③ 判「契约测试会不会强制登记」得读它的**采集过滤器**，不能只看测试名（`collab_contract_tests.rs:79`）。
- **回填**：改动 SHA 与回帖 id 见本节末「改动 SHA / 回帖（回填）」。issue #17 **保持 open**（单发轮不 close）。**第二段**：`[opt-round]` open = **0** ⇒ 无对象（非「队列为空」——手工队列仍剩 15 条）。

**改动 SHA / 回帖（回填）**：`0a9a7dd`（5 文件 `161+/3−`，**零代码行为改动**）；推 `cnb` 单 remote（`ae11e36..0a9a7dd  HEAD -> main`）；`merge-base --is-ancestor 0a9a7dd cnb/main` 通过，主工作区 `merge --ff-only` 到 `0a9a7dd`、`status --porcelain` 0 行。**改后验证**：`scripts/check.sh --quick` `EXIT=0` **68 s**（`==> checks passed in 68s (quick)`）；改前 117 s `EXIT=0`（`==> checks passed in 117s (quick)`）。**时刻锚** = `git log -1 --format=%aI 0a9a7dd` = `2026-11-12T09:59:42+08:00`。**回帖** `2120698283860324352`（`2026-11-12T02:00:46Z`，31941 字符；回读首行 `## 本轮收尾`、次行 `protocol=v0.12`、末行独立 `@CodeBuddy`、反向清单 68 条编号独立成行；发帖前后 `本轮收尾` 计数 0 → 1，无重复投递）。

### 单发轮 · issue #18 · 2026-11-12（第 1 次进入；`[opt-round]` 仍 0 ⇒ 第二段无对象）

- **选靶依据**：17 条 open 全 MANUAL，`priority` 全空（任务书优先键失效，按协议 §5.2.1 用「消费次数 asc, 编号 asc」）。平台侧 `^## 本轮收尾` 计数：`#14`=3、`#15`=1、`#16`=1、`#17`=1、`#33`=1，其余 11 条（`#18`/`#19`/`#34`–`#43`）**0** ⇒ 零消费里编号最小的 **#18**。`#18` 的 `comment_count` = 1（NPC 细化回帖 `2120150626892935168`，2987 字符，指向 PR #20 已合并的 `agent-draft-refs-ttl-design-2026-11.md`），不需要首扫请求。
- **形态**：靶子是**设计题**（草稿命名空间 TTL + 通知抑制 + 自动清理）⇒ 本体是 L2（新表、新端点、权限语义、CI 执行边界），本轮产出 = 核实现状断言 + 订正设计文档的行号与两处错误前提 + 登记 L2，**零代码行为改动**。
- **BASE** `e225293`（`HEAD...cnb/main` = `0 0`，零漂移）。改前 `scripts/check.sh --quick` **106 s EXIT=0**（`==> checks passed in 106s (quick)`）。
- **判定分布**：25 条现状断言 = **成立 17 / 部分成立 6 / 不成立 2**。全表见 `ironforge-docs/architecture/agent-draft-refs-ttl-verification-2026-11.md` §2。

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| P6 | 清理不能复用删分支 API，`on_delete` 会拒有 open PR 的分支 | **成立** | `on_delete` 在 `lib.rs:3082-3140`（文档写 `:3070-3125`，+12）；其内调 `delete_branch`（`:3102`），三守卫 `repo/branch.rs:115`/`:118-120`/`:122-125` |
| P4 | `post_push_hooks` 顺序 CI→push webhook→分支事件→ws，已有 `status != "ok"` 过滤 | **成立**（行号 +12） | `:3290` 定义、`:3321-3324` 过滤、`:3417-3426` CI、`:3516` push、`:3522-3548` 分支、`:3579-3586` ws |
| D10 | `refs/pull/<n>/head` 是内核级只读镜像，「不需要新代码」 | **不成立** | `grep -rn 'refs/pull' crates/rg-git/src/` = **0**；`receive_pack.rs:507` → `:631-647` 的 `PreviousValue::Any`（`:641`）；拒绝只有分支/tag 保护两路（`lib.rs:3074-3076`），恒不命中 `refs/pull/` |
| D15 | R3：`ci_secrets` 按 ref 过滤「需要确认——**未验证**」 | **不成立（无此能力）** | `entities/ci_secret.rs:9` 只有 `repo_id`；注入点 `rg-ci/src/runner.rs:743-762` 的 `list_by_repo` 一把全取、逐条解密，**无 ref 条件** |
| D12 | 白名单支持 `*` 且 `*` 可跨 `/`，禁止以 `*` 开头 | **部分成立** | `normalize_ref_allowlist` 实为 `:213-257`（文档 `:211-262`）；`*` 字符集 `:230`、禁开头 `:234`；跨 `/` 由 `ref_matches_pattern` 的 DP（`branch_protection/service.rs:228-249`，未对 `/` 特判）成立 |

- **最贵的一条错**：D10 + §6.2 步 2 —— 设计三处以「`refs/pull/` 天然只读」推出「不许开 PR 天然成立，不需要新代码」。真值：该 ref 是普通 ref，无白名单令牌（deploy key / bot PAT）可把它指向任意 commit。按它写实现，会得到一个「agent 能改写 PR 镜像」的草稿区。**判据（可复用）**：判「某 ref 命名空间是否只读」不能看它的用途文档，要 `grep` 该命名空间在 `rg-git` 的命中数 + 读落地写入的 `PreviousValue`。
- **第二条错是「把待确认写成可能够用」**：R3 把 secrets 过滤列成缓解措施并标「未验证」。真值是**没有**这个能力（repo 级一把全取）。同时它把该风险写成草稿区副产品，**实际今天就存在**（任意可写分支的推送已触发 CI、已拿全仓 secrets、已按仓库开关决定宿主机执行）——草稿区新增的是「不再有人被通知」。此结论已写进设计文档 R3 行。
- **新发现（两份材料都没有）**：`git push --delete` 会发**两次** `branch.deleted` —— `on_delete` 内 `lib.rs:3125` 一次、`post_push_hooks` 内 `:3541` 一次（删除项 `status="ok"`、`new_sha` 全 0，过滤放行；`ref_updates` 同一份传入 `:3163`）。REST `DELETE /branches/{x}` 走第三处（`api/repo_content.rs:723`），不受影响。⇒ 设计 §5.3「三个注入点全在 `post_push_hooks`」实为**四处**，第 4 处是 `on_delete`，草稿分支被 `push --delete` 删掉时仍会漏一条 webhook。
- **改动（本流程在 WT 内 commit）**：新建 `ironforge-docs/architecture/agent-draft-refs-ttl-verification-2026-11.md`（判定表 25 条 + 两条不成立展开 + 行号漂移表 + 双发发现 + R18-1…R18-6）；`agent-draft-refs-ttl-design-2026-11.md` **27 行 1→1 订正**（行号 23 行 + 内容 4 行：`refs/pull` 前提两处、`post_push_hooks` 注入点一处、R3 一处），**258 行不变**；`ironforge-docs/README.md` 索引 +1 行。`crates/` 零字节变化。
- **新 L2（待拍板）**：**R18-1**（P1，`receive_pack` 无命名空间守卫 ⇒ 无白名单令牌可写 `refs/pull/*`/`refs/notes/*`；修法二选一，裁决前不得以「天然只读」写实现）；**R18-2**（P1，CI secrets 无 ref 过滤，草稿区「不注入 secrets」前提做不到）；**R18-3**（P2，`allow_host_execution` 无触发源判定，`lib.rs:3430` → `rg-ci/src/lib.rs:402`）；**R18-4**（P2，`ref_limited_write_allowed` 缺 `agent-drafts`，与 T21 三张表耦合）；**R18-5**（P2，`branch.deleted` 双发，属 webhook 语义变更）；**R18-6**（P3，`spawn_post_push_hooks` 是 `pub(crate)`，S5 若下沉 core 会撞 R17-7 同一堵墙）。
- **反向清单 69–70**；清单累计 **1..70**。
69. 「`refs/pull/<n>/head` 是内核级只读镜像，任何推送写 → 客户端 `push` 直接失败 …『不许开 PR』用 `refs/pull/` 天然成立，不需要新代码」（设计文档 §1 非目标 / §2 第 10 行 / §6.2 步 2 三处）—— **不成立**：`rg-git` 对该命名空间 0 命中；落地写入 `receive_pack.rs:631-647` 用 `PreviousValue::Any`；无 `ref_allowlist` 的令牌可改写它。正确表述是「带白名单的令牌天然写不进，因为白名单前缀恒为 `refs/heads/`」。**勿再以「天然只读」为前提做设计推论。**
70. 「`ci_secrets` 按 ref 过滤需要确认是否已有能力——未验证」（设计文档 §8 R3）—— **不成立（无此能力）**：`ci_secret` 实体只有 `repo_id`，注入点 `rg-ci/src/runner.rs:743-762` 用 `list_by_repo` 全量取用。且该风险**今天就存在**（非草稿区副产品）。**勿再写成「可能已有，只需确认」。**
- **回填**：改动 SHA 与回帖 id 见本节末「改动 SHA / 回帖（回填）」。issue #18 **保持 open**（单发轮不 close）。**第二段**：`[opt-round]` open = **0** ⇒ 无对象（非「队列为空」——手工队列仍剩 16 条）。

**改动 SHA / 回帖（回填）**：`c17735e`（4 文件 `195+/28−`，**零代码**）；推 `cnb` 单 remote（`e225293..c17735e  HEAD -> main`）；`merge-base --is-ancestor c17735e cnb/main` 通过，主工作区 `merge --ff-only` 到 `c17735e`、`status --porcelain` 0 行。**改后验证**：`scripts/check.sh --quick` `EXIT=0` **1 s**（`==> checks passed in 1s (quick)`；clippy `Finished dev profile … in 0.24s`）；改前 **106 s** `EXIT=0`。**时刻锚** = `git log -1 --format=%aI c17735e` = `2026-11-12T11:09:08+08:00`。**回帖** `2120715806089572352`（`2026-11-12T03:10:23Z`，45511 字节；回读首行 `## 本轮收尾`、次行 `protocol=v0.12`、末段独立 `@CodeBuddy`、反向清单 70 条编号独立成行；发帖前后 `本轮收尾` 计数 0 → 1，无重复投递）。issue #18 **保持 open**（单发轮不 close）。

### 单发轮 · issue #19 · 2026-11-12（第 1 次进入；`[opt-round]` open = 0 ⇒ 第二段无对象）

protocol=v0.12

- **选靶依据**：17 条 open 全 MANUAL，`priority` 全空。平台侧 `^## 本轮收尾` 计数（`grep -c 'body: "## 本轮收尾'`，注意现版 `list-issue-comments` 是 YAML 形态、不是 CSV，旧锚点 `,"## 本轮收尾` 恒返回 0）：`#14`=3、`#15`=1、`#16`=1、`#17`=1、`#18`=1、`#33`=1，其余 11 条（`#19`/`#34`–`#43`）**0** ⇒ 零消费里编号最小的 **#19**。`#19` 的 `comment_count` = 1（NPC 细化方案 `2120149991086776320`，`2026-11-10T13:42:03Z`），无需首扫请求。
- **形态**：靶子是**设计题**（把 CI 环境审批 / 协作确认 / 受保护分支合并抽象成一次性授权请求）⇒ 本体是 L2（新表、新状态机、权限语义、执行面），本轮产出 = 核实现状断言 + 订正一处过时注释 + 登记 L2，**零代码行为改动**。
- **BASE** `94743ca`（`HEAD` 与 `cnb/main` 同点、`ls-remote` 一致；其它 8 个 worktree 40 min 内零 mtime 变化 ⇒ 无并发写者）。改前 `scripts/check.sh --quick` **99 s EXIT=0**（`Finished dev profile … in 1m 37s` / `==> checks passed in 99s (quick)`）。
- **判定分布**：36 条现状断言 = **成立 27 / 部分成立 7 / 不成立 1 / 不可验 1**。全表见 `ironforge-docs/architecture/authorization-primitive-verification-2026-11.md` §2。

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| N12 | 受保护分支合并「谁能批 = 审批人 / 写权限」 | **不成立** | `api/reviews.rs:312` 的 `submit_review` 只过 `require_authenticated_read` ⇒ **任何能读仓库的登录账号都能投 approve**；`entities/protected_branch.rs` 全表无审批人列，`:30` 的 `allowed_push_user_ids` 管推送不管审批 |
| N11 | 绑定 `pr.head_sha`（`pr_review_ops.count_current_approvals`） | 部分成立 | 绑定成立；**函数名不存在**——真名 `count_current_human_approvals`（`ops/pr_review_ops.rs:57`），`count_approvals` 是它的 `None` 包装（`:46`） |
| N17 | 「head_sha 一变旧请求便自动失效」已是现状语义 | 部分成立 | 语义对（`:74-79` 按 `commit_id == head_sha` 过滤）；但这是**过滤**不是状态失效，没有 `expired` 记录、也没有任何东西通知批准人票废了 |
| N21 | `protected_branches.allowed_push_user_ids` / 审批人 | 部分成立 | 字段在（`:30`），但「审批人」无字段；把它当审批人名单会静默改语义 |
| N25 | 「agent 只能申请、永远拿不到 approve」是**新增**约束 | 部分成立（大半已实现） | CI 侧**已经**硬拒 agent（`api/ci_environments.rs:285-304`，`AGENT_MAY_NOT_APPROVE`）；PR 侧靠 human-only 计数（`ops/pr_review_ops.rs:57-77`）。缺的只有 agent 侧申请入口 |
| N34 | 协作链路完全没有外发通道（引 `human_notify.rs`「There is no email and no webhook yet」） | 部分成立 | 引文逐字对（`:22`），但 **「no webhook」已过时**——`webhook_events.rs:30` + `api/collab_questions.rs:182` 已在投 `collab.question.asked`。注释本轮已订正 |
| N36 | `send_html_notification` 模板尾句「已经够用」 | 部分成立 | 句子在（`email/mod.rs:106`）但**内容是错的**：它宣称「you have notifications enabled」，而系统没有这个开关（它自己在 N30 已证） |

- **本轮最贵的一条错**：N12。它据此说「抽象只是把现有三份判定收进求值器」，真值是这个求值器一旦照抄，会把「能读仓库就能批合并」固化进新原语；要改写成写权限，那是**权限收紧**（现有仓库合并会突然 403），必须先拍板。**判据（可复用）**：判「谁能批某动作」不能读字段名（`allowed_push_user_ids` 看着像审批人名单），要读 handler 的第一道 `require_*`——本轮一条 `grep -n "require_authenticated_read" reviews.rs` 即定性。
- **它自相矛盾处**：§一表格把协作通知写成「in-app + WS + webhook」，§四又引 `human_notify.rs` 的「no webhook yet」当证据。两句只可能有一句对；真值是 §一 对、注释过时（N34）。
- **新发现（两份材料都没有）**：**F1** 自批守卫只在 HTTP 层（`api/reviews.rs:325`），core `review/service.rs:57-101` 的 `submit_review` 既不查权限也不查自批，只校验 `state == "open"` ⇒ 任何绕过 handler 的调用面（MCP、内部服务）都能自批。**F2** CI 通知邮件正文的 `<br/><br/>`（`crates/rg-http/src/lib.rs:3462`）被 `email/mod.rs:110` 的 `html_escape(message)` 转义 ⇒ 用户看到字面标签。**F3** `email/mod.rs:106` 的尾句为不存在的开关背书。**F4** `count_approvals`（传 `None`，只数 `commit_id IS NULL` 的审批）与合并门（传 `pr.head_sha`）不是同一口径。
- **新 L2**：**R19-1**（P1，合并审批准入面：`api/reviews.rs:312` 读权限即可批，收紧属权限语义变更）；**R19-2**（P2，自批守卫下沉 core，F1）；**R19-3**（P2，`email/mod.rs:106` 尾句 + `:110` 转义吃掉 `<br/>`）；**R19-4**（P3，`count_approvals` 与 `count_current_human_approvals` 口径分叉，F4）；**R19-5**（P3，「agent 不得批准」应复用 `AGENT_MAY_NOT_APPROVE` + human-only 计数，不新造第三份判定）。
- **改动（本流程在 WT 内 commit）**：新建 `ironforge-docs/architecture/authorization-primitive-verification-2026-11.md`（36 条判定表 + 三套机制对照 + 三条优先订正 + F1–F4 + R19-1…R19-5）；`crates/rg-core/src/collaboration/human_notify.rs:22-23` 注释 **2 行 → 2 行**（`no email and no webhook yet` → 「无邮件，外发走 `webhook_events`」），文件仍 **259 行**、行号零漂移；`ironforge-docs/README.md` 索引 +1 行。**零代码行为改动**。
- **反向清单 71**；清单累计 **1..71**。
71. 「受保护分支合并『谁能批 = 审批人 / 写权限』」（issue #19 方案 §一 第三行）—— **不成立**：`crates/rg-http/src/api/reviews.rs:312` 的 `submit_review` 只有 `require_authenticated_read`，读权限即可投 approve；`crates/rg-db/src/entities/protected_branch.rs` 无审批人列（`:30` 的 `allowed_push_user_ids` 只约束推送）。**勿再以「现状已是写权限门槛」为前提推演抽象收益。**
- **回填**：改动 SHA 与回帖 id 见本节末「改动 SHA / 回帖（回填）」。issue #19 **保持 open**（单发轮不 close）。**第二段**：`[opt-round]` open = **0** ⇒ 无对象（非「队列为空」——手工队列仍剩 16 条）。
- **producer-stall**：`[opt-round]` 最近一批 `#31` 的 `closed_at` = `2026-11-11T13:09:01Z`，至本轮取证（`2026-11-12T04:26Z`）已 **15 h 17 m** 零新批（协议 §11 风险 10 的 180 min 阈值早已越过、6 h 阈值也越过）⇒ 生产者侧两条 automation 需人工查；消费者侧无写权限，只记不改。

**改动 SHA / 回帖（回填）**：`832cd27`（4 文件 `144+/2−`，**零代码行为改动**：`crates/` 只有 `human_notify.rs:22-23` 一处文档注释 2 行→2 行）；推 `cnb` 单 remote（`94743ca..832cd27  HEAD -> main`）；`merge-base --is-ancestor` 通过，主工作区 `merge --ff-only` 到 `832cd27`、`status --porcelain` 0 行。**改后验证**：`scripts/check.sh --quick` `EXIT=0` **68 s**（`Finished dev profile [unoptimized + debuginfo] target(s) in 1m 06s` / `==> checks passed in 68s (quick)`）；改前 **99 s** `EXIT=0`。**时刻锚** = `git log -1 --format=%aI 832cd27` = `2026-11-12T12:19:00+08:00`。**回帖** `2120733390440669184`（`2026-11-12T04:24Z` 前后，18696 字节；回读首行 `## 本轮收尾`、次行 `protocol=v0.12`、末行独立 `@CodeBuddy`、反向清单 71 条编号独立成行；发帖前后 `本轮收尾` 计数 0 → 1，无重复投递）。

### 单发轮 · issue #34 · 2026-11-12（第 1 次进入；`[opt-round]` open = 0 ⇒ 第二段无对象）
protocol=v0.12
- 选靶：MANUAL 里零消费的 `#34`–`#43` 共十条（`comment_count` 全 0），按（次数 asc, 编号 asc）取 **#34**（`priority` 全空、无 `[hold]`、本机可做）。`#34` 无 NPC 分析 ⇒ 先发只读首扫请求 `2120748991439597568`（`2026-11-12T05:22:15Z`，第二人称限定「你是评审者、只读、不要写收尾标记、未跑就标未跑」），`60 s × 5` 轮询到 `05:27:15Z` 平台上仍只有我方那一条 ⇒ **用 issue 正文推进，不重复发**（协议 §5.2「#26–#29 的准备缺口」）。
- 靶子形态：issue 正文 = 设计文档 §4 T22 的摘要（目标/现状/方案/测试/完成口径）⇒ 本轮核**它对现状的断言**，不是核 NPC 的指摘。
- 判定：**成立 8 / 部分成立 3 / 不成立 2**（13 条，逐条证据见 `ironforge-docs/architecture/collab-user-door-verification-2026-11.md` §2）
  - **成立**：`collab.rs:94-109` 的 session/user 分派（`:103` 判头、`:105` session、`:107` user）；该函数体不判断账号类型；29 个 `api/collab*.rs` **零处** `users.kind`/`is_agent`（命中的 `.kind` 全是 gix object kind、decision/message/recipient/evidence 的字段）；`write_actor` **15 调用点 / 7 文件**（2+2+2+2+3+3+1，与正文列名逐一相符，且 15/15 都紧跟 `require_collab_write`）；`Actor` 只有 `"session"`/`"user"` 两值；`confirm_condition`（`:178`）只过 `require_collab_write`（`:186`）；「多一次 users 查询可忽略」的计数对；不变量 4 = agent 判权不进 30 秒缓存。
  - **部分成立**：①`PUT …/inbox/user/read`「无害读标记」——它是 **PUT 写**，`collab_inbox.rs:156 mark_user_read` 在 `:162` 过 `require_collab_write` 后写一条 read-mark 行，且 `:166` **直接构造 `Actor::user(user_id)`** 绕过 `write_actor` ⇒ 「只拦写」不能用来论证保留它；②「加 `Actor::agent` 会连带改 capabilities 契约、审计口径与前端」——前端对且更重（`actor_kind` 是 **9 个协作实体**的落库列，`web/.../collaboration/tasks/[id]/+page.svelte:518` 把 `d.actor_kind` 原样插进 `collab.decisions.by`），capabilities 与审计不对（`capabilities.rs` 的 `Capability`/`Route` 无 actor 字段，`rg-core/src/audit/` 0 处 `actor`）；③5 条测试可写但零现成夹具（`agent_principal_tests.rs` `grep -c collaboration` = 0，14 个 `collab_*_tests.rs` 里 `create_agent`/`kind: Set("agent")` 0 命中）。
  - **不成立**：①方案示例 `crate::api::repo_access::is_agent_account(&state.db, user_id)` —— 该函数全仓 **0 命中**、`api/repo_access.rs` 全文 0 处 `agent`；现成判定是 `rg_core::agent::is_agent(&User)`（`agent.rs:48`）+ 先取 `users` 行。②「依赖 B1（T23/T24/T25）先落地」——`on_behalf_of` 全仓仅 `rg-core/src/agent.rs:23` 一处 doc 注释（"Not covered here (later phases)"），`entities/audit_log.rs` 无该列 ⇒ B1 三条一条未落。
- **新发现（F1–F6，两份材料都没写；F7 见本节末第 2 次进入）**：**F1** `actor_kind` 是 9 个协作实体的落库列（answer/decision/decision_change/takeover/inbox_read/completion_condition_set/completion_snapshot/coordination_group/coordination_link）+ 前端直显，新增第三种 kind 不是改枚举而是改已落库的字符串域；**F2** `Actor::user` 有 **3 个构造点**（`collab.rs:107`、`collab_inbox.rs:166`、`collaboration/inbox.rs:199`），只改 `write_actor` 覆盖 15 个写但漏 `mark_user_read`，`reassign`（`takeover.rs:291-303`）连 `Actor` 都不用、记 `user_id.to_string()`；**F3** 门可以更省——`require_collab_write` → `can_write_repo`（`repo/service.rs:293`）→ `apply_owner_cap`（`:339`）→ `owner_cap`（`:256`）已经 `find_by_id(actor)` 一次，`OwnerCap::Owner(_)` 本身就是「这是 agent」，再查是同一请求内第二次；**F4** §2.3 的 9 条不变量里**只有条 9 没有任何拦截代码**（条 1–8 都有实现或测试），却以「改动时必须保持」列出；**F5** 拒绝口径已分叉——`AGENT_MAY_NOT_APPROVE` 走 `AppError::coded(...).with_reason(...)`（`api/auth.rs:165-186`），`ensure_human` 只有自由文本（`agent.rs:126-136`），方案让 `confirm_condition` 用 `ensure_human` 就拿不到机器可读 reason；**F6** 模板写**不是**漏网——`create_template`/`new_template_version`（`collab_templates.rs:176`/`:224`）走 `require_template_admin`（`:145-157` → `can_admin_repo`），而 `can_admin_repo` 在 `repo/service.rs:356-359` 已「agent 永不管理仓库」。
- **新 L2**：**R34-1**（P1，T22 落地＝写侧权限收紧，改认证/权限语义）；**R34-2**（P2，`mark_user_read` 是否纳入「用户门只服务人类」——纳入则改行为，不纳入须在 §2.3 条 9 写明例外）；**R34-3**（P2，`reassign` 用 `user_id` 字符串记 actor，落地条 9 需一并裁决）；**R34-4**（P2，新增 `AGENT_MUST_USE_SESSION` 走 `with_reason`，`ensure_human` 是否补通用 reason）；**R34-5**（P3，设计文档 T22 的 `is_agent_account` 引用，本轮已按现树 API 订正 1 行）；**R34-6**（P3，B1 未落地 ⇒ T22 排期应后置或先做 T23 审计列）。
- **改动（WT 内 commit）**：新建 `ironforge-docs/architecture/collab-user-door-verification-2026-11.md`（13 条判定 + 3 个用户门构造点 + F1–F6 + R34-1…R34-6）；`crates/rg-http/src/api/collab.rs:94-96` 文档注释 **3 行 → 3 行**（补「这里不读 `users.kind`，agent 尚未被拒绝使用用户门」）；`ironforge-docs/architecture/agent-principals-design-and-roadmap-2026-11.md` **3 处各 1 行 → 1 行**（§2.3 条 9 标「本条当前未强制」、§4 T22 方案句补 `with_reason` 写法、`:267` 的 `is_agent_account` 换成 `rg_core::agent::is_agent` + `user_ops::find_by_id`），文件仍 **668 行**、`:70`/`:241`/`:508-512` 三处既有引用零漂移；`ironforge-docs/README.md` 索引 +1 行。**零代码行为改动**。
- **反向清单 72、73**；清单累计 **1..73**。
72. 「方案示例可以直接调 `crate::api::repo_access::is_agent_account(&state.db, user_id)`」（issue #34 正文方案 / 设计文档 §4 T22 伪代码）—— **不成立（该 helper 不存在）**：全仓 `is_agent_account` 0 命中，`crates/rg-http/src/api/repo_access.rs` 全文 0 处 `agent`；现成判定是 `rg_core::agent::is_agent(&User)`（`crates/rg-core/src/agent.rs:48`），须先 `rg_db::ops::user_ops::find_by_id` 取行。**勿照抄该行伪代码当作可编译落地方案。**
73. 「T22 依赖 B1（T23/T24/T25）已落地，可以直接开工」—— **不成立（B1 三条一条未落）**：`on_behalf_of` 全仓仅 `crates/rg-core/src/agent.rs:23` 一处 doc 注释（"Not covered here (later phases)"），`crates/rg-db/src/entities/audit_log.rs` 无该列。**勿以「依赖已满足」为前提排期。**
- **producer-stall**：`[opt-round]` 最近一批 `#31` 的 `closed_at` = `2026-11-11T13:09:01Z`，至本轮取证（`2026-11-12T05:29Z`）已 **16 h 20 m** 零新批（协议 §11 风险 10 的 180 min 与 6 h 阈值均已越过）⇒ 生产者两条 automation 需人工查；消费者无写权限，只记不改。第二段因此**无对象**（不是「队列为空」——手工队列仍剩 16 条）。
- **自证订正（第 2 次进入本 issue，只看自己的产物）**：核到自己那份核实文档里 3 处引用不精确，已改（`f3ee3fd..HEAD`，1 文件 `3+/3−`，零代码）：①`takeover.rs:291-303` → **`:291-319`**（`:303` 是 `let mover = Mover {`，`outcome` 在 `:306`、函数闭括号在 `:319`；actor 的落点是 `:302` `user_id.to_string()` + `:303-305` 写死 `actor_kind: "user"`），§1 与 R34-3 两处同改；②N13 的「`agent_principal_tests.rs`（900+ 行）」→ **1191 行**（`wc -l` 实测，`fa57f3a` 与现树同值）；③N13 的「14 个 `collab_*_tests.rs`」→ **24 个**（`ls crates/rg-http/tests/ | grep -E '^collab.*\.rs$'` = 25，其中 `collaborator_tests.rs` 无 `collab_` 前缀，`collab_*_tests.rs` 恰 24；`fa57f3a` 上同为 24 ⇒ 不是漂移，是首轮数错）。三条都不动判定（成立 8 / 部分 3 / 不成立 2）；F1–F6、R34-1…R34-6 与反向清单 1..73 全部不变。**另**：`origin/main` 已从 `f3ee3fd` 前进到 `13e2200`（4 个提交，全是别轮 opt-loop 文档），逐条比对确认未触及本轮的 `collab.rs` 注释与三份设计文档改动 ⇒ 上轮结论无漂移。
- **补 F7（同一 issue 第 2 次进入）**：把「漏网点」穷举出来——34 个 handler 过 `require_collab_write`，**15 个**调 `write_actor`、**19 个**不调；19 个里 **13 个**自做会话凭据校验（`acting_session`×10 + `creds_for_path`×2 + 其余），只剩 **6 个**无任何会话凭据：`confirm_condition`（`collab_conditions.rs:178` POST 写）、`mark_user_read`（`collab_inbox.rs:156` PUT 写）、`reassign_task`（`collab_takeovers.rs:102` POST 写）、`create_session`（`collab_sessions.rs:84` POST，**刻意豁免**：签发会话的入口）、`user_inbox`（`collab_inbox.rs:79` GET）、`list_sessions`（`collab_sessions.rs:124` GET）。⇒ 「agent 用用户门写协作数据」的改动点**恰好 4 个**：`write_actor` + 上面 3 个写。R34-1 的规模据此定死（`c094110`，1 文件 `9+/2−`）。
- **回填**：改动 SHA 与回帖 id 见本节末「改动 SHA / 回帖（回填）」段。issue #34 **保持 open**（单发轮不 close）。

**改动 SHA / 回帖（回填）**：`f3ee3fd`（5 文件 `86+/4−`，**零代码行为改动**）；推 `cnb` 单 remote（`fa57f3a..f3ee3fd  HEAD -> main`）；`merge-base --is-ancestor` 通过，主工作区 `merge --ff-only` 到 `f3ee3fd`、`status --porcelain` 0 行。**改后验证**：`scripts/check.sh --quick` `EXIT=0` **63 s**（`Finished dev profile [unoptimized + debuginfo] target(s) in 1m 02s` / `==> checks passed in 63s (quick)`）；改前 **99 s** `EXIT=0`。**时刻锚** = `git log -1 --format=%aI f3ee3fd` = `2026-11-12T13:29:25+08:00`。**回帖** `2120750974015787008`（`2026-11-12T05:30:08Z`，47001 字节；回读首行 `## 本轮收尾`、次行 `protocol=v0.12`、末段独立 `@CodeBuddy`、反向清单 73 条编号独立成行；发帖前后该 issue 的 `## 本轮收尾` 计数 0 → 1，无重复投递）。**推票坑（新，影响每次推送）**：`git push` 用 `username=oauth2` 会报 `Repository Not Found`，同一 token 换 `username=cnb` 才成功（`git fetch` 两种都行）。

**第 2 次进入本 issue 的改动 SHA（回填）**：`05eb78c`（3 处引用订正，`4+/3−`）+ `c094110`（补 F7，`9+/2−`）+ `a91437a`（台账两行）+ `a2fc5c3`（本段回填）；合计 `docs/reports/OPT-LOOP-LEDGER.md` 与 `ironforge-docs/architecture/collab-user-door-verification-2026-11.md` **2 文件 `18+/6−`（含本段自身），零代码行为改动**（`crates/` 一行未动）。推送链 `13e2200 → 05eb78c → c094110 → a91437a → a2fc5c3` 逐次 `fast-forward`（回填只记到 `a2fc5c3`，此后不再追加）；`git fetch origin main` 后 `git rev-parse HEAD` 与 `git rev-parse origin/main` 同为 `a91437a4c7aeff7e801cb87520f003dc1e3b45e6`（第 3 个提交快照，故 HEAD 数字为 `a91437a`），`status --porcelain` 0 行。**时刻锚**：`05eb78c` = `2026-11-12T14:53:11+08:00`、`c094110` = `2026-11-12T14:57:33+08:00`、`a91437a` = `2026-11-12T14:57:40+08:00`、`a2fc5c3` 见 `git log -1 --format=%aI`。**未跑测试**：本容器无 `cargo`（`command -v cargo` 空），且三次改动全在 `.md`（CI 的 `changes` 分类器对「只改文档」不跑任何 job）⇒ 本轮无测试可跑，不声明「已验证」。

### 单发轮 · issue #35 · 2026-11-12（第 1 次进入；`[opt-round]` open = 0 ⇒ 第二段无对象）

protocol=v0.12

- **选靶依据**：17 条 open 全 MANUAL、`priority` 全空（任务书优先键失效，按协议 §5.2.1 用「消费次数 asc, 编号 asc」）。平台侧 `^## 本轮收尾` 计数（YAML 锚点 `body: "## 本轮收尾`）：`#14`=3、`#15`=1、`#16`=1、`#17`=1、`#18`=1、`#19`=1、`#33`=1、`#34`=1，其余 9 条（`#35`–`#43`）**0** ⇒ 零消费里编号最小的 **#35**（`通知改派给 agent 的所有者（T24）`，`comment_count` = 0）。
- **无 NPC 分析 ⇒ 先发只读首扫请求**：评论 `2120756694840922112`（`2026-11-12T05:52:52Z`），第二人称限定「你是评审者、只读、不写收尾标记、未跑就标未跑、别复用文档快照行号」。`60 s × 5` 到 `05:57:52Z` 平台上仍只有我方这一条（`list-issue-comments --sort -created --page-size 5` 只回 1 条，`本轮收尾` 计数 0）⇒ **按 issue 正文推进，不重复发**（协议 §5.2「#26–#29 的准备缺口」）。
- **形态**：靶子是设计摘要（T24 通知改派）⇒ 本体是 L2（新函数 + 通知语义 + 可能扩到 4 处通知面），本轮产出 = 核实现状断言 + 订正设计文档 6 行 + 两处注释补「未映射」事实 + 登记 L2，**零代码行为改动**。
- **BASE** `721ab16`（`HEAD` 与 `cnb/main` 同 SHA、`ls-remote` 一致；另 8 个 worktree mtime 最新 `2026-11-12 02:25`、`ironforge-opt-worktrees` 的 13:30 是上轮自己清理 ⇒ 无并发写者）。改前 `scripts/check.sh --quick` **104 s EXIT=0**（`Finished dev profile [unoptimized + debuginfo] target(s) in 1m 42s` / `==> checks passed in 104s (quick)`）。
- **判定分布**：19 条断言 = **成立 13 / 部分成立 5 / 不成立 1**。全表见 `ironforge-docs/architecture/agent-notify-routing-verification-2026-11.md` §2。

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| N5 | `recipients()` 与整个模块**无任何 agent 判断** | **成立** | `grep -nE 'is_agent\|users\.kind\|kind *= *"agent"' human_notify.rs` = **0**；`agent` 只出现在注释与 `agent_session_ops` |
| N7 | agent 用自己令牌建会话时 `session.user_id` 就是 agent | **成立** | `collab_sessions.rs:90` 取 `require_collab_write` 的 actor、`:94` 直接交给 `svc::create`；`collab.rs:202-217` 只查 `can_write_repo`，无 kind 门 |
| N8 | 人类永远看不到「你的 agent 卡住了」 | **成立** | 收件人恒为 `session.user_id`（`human_notify.rs:206`）；`owner_user_id` 在通知链路 **0 消费点**（全仓除 `agent.rs`/`entities`/`migrations`/`user_ops.rs`） |
| P1 | 新增 `accountable_user(db, user_id) -> Option<i64>` | **部分成立** | 函数不存在（`grep accountable_user crates/` = 0），但语义 2/3 已在 `agent.rs:249-262` 的 `owner_cap`（`None`/`Owner(须 active 且非 agent)`/`Denied`）；缺的只是 human 分支的 `is_active`，而 `:250` 已读过该行 |
| P3 | `task_blocked`「不要 fallback 到 `repo_writers`，否则会广播」 | **部分成立** | 现状**没有任何 fallback**：`:195-197`/`:199-201`/`:208-210`/`:211-214`/`:220-227` 五个分支全部 `return Vec::new()` ⇒ 风险在现树不存在，只能当预防性约束 |
| P5 | 给映射后收件人加 `kind=human && email NOT LIKE '%@agents.invalid'` 能关闭 `.invalid` 未核实项 | **不成立** | 该路径不发信（`notification::notify:9-20` 只写库；模块内 `lettre / send_mail / smtp` 0 命中）⇒ 过滤零效果。真实发信口三处且都已挡：`lib.rs:3465` CI 取 `repo_owner_id`，而 `repo/service.rs:489` 的 `create_repo` 有 `ensure_human`（fork `:1167`、转移 `:1392-1393`）；`lib.rs:3678` 是重试 worker，只由 `:3486`/`user/service.rs:763` 入队；`user/service.rs:745` 密码重置在 `:711` 拒 agent |
| T1 | 五条测试可写 | **部分成立** | `collab_human_notify_tests.rs` 有 430 行 / 9 个现成用例，但其 `collab_common` **无** agent 夹具（文件内 `agent` 0 命中）；能造 agent 的 `World`（`agent()`/`agent_token()`/`grant()`）在 `agent_principal_tests.rs:36-180`；全仓 `tests/` 内 `rg_core::agent::create_agent` 0 命中 |
| D2 | 新 SQL 至少跨后端跑一次（PG/MySQL） | **部分成立** | 能力在（`Cargo.toml:43` sea-orm 带 `sqlx-postgres/mysql`、`rg-db/src/lib.rs:74-84` 有分支、`rg-core/tests/multi_backend_smoke.rs` 存在），但 T24 **不新增 SQL** ⇒ 该条无对象 |

- **最贵的一条错（P5）**：它把「关闭 `.invalid` 发信」寄托在收件人过滤上，而通知路径根本不发信。真值是三道闸（`ensure_human` 守 `repository.owner_id`、密码重置 `:711` 拒 agent、重试队列只有两个入队点）把 `.invalid` 关死 ⇒ 设计文档 §2.5 的 P2 未核实项**今天可以结案为「否」**，而且不需要写任何过滤代码。**判据（可复用）**：判「某类收件人会不会收到邮件」，别读收件人构造处，去数**发信函数**的调用点（本轮 `send_html_notification` 全仓 3 处）并逐个看收件人从哪来。
- **第二处产出（F1，两份材料都没有）**：铃铛面不止 `human_notify`。另有 **4 处**按 user id 直发：`notification/mod.rs:122`（`notify_watchers`）、`issue/reactions.rs:29`/`:97`、`issue/assignees.rs:52`。⇒ T24 只改正文那两处时，「铃铛响在人那边」只在**被问**与**卡住**两个场景成立；agent 被指派、被回复、watch 仓库时仍会自己响铃（R35-2）。
- **第三处产出（F3）**：`accountable_user` 应薄封装 `owner_cap`。`owner_cap:249-262` 已写完「agent → 所有者（存在、启用、非 agent），否则拒绝」，独立重写就是本仓的**第 4 份** agent 判定（前三份：`AGENT_MAY_NOT_APPROVE`、`can_admin_repo` 的 `owner_cap != None`、`ensure_human`），与本流程 R19-5 的既有结论冲突。
- **第四处产出（F5）**：`recipients()` 现状对每个候选都 `invalidate_perm_cache_user` + `can_write_repo`（`:114-115`），而 agent 的 `can_write_repo` 会递归到所有者（`apply_owner_cap` → `can_write_repo(owner)`）⇒ 映射（纯读）应放在权限查之前，否则 500 个候选里的每个 agent 都多付一次 owner 权限链。
- **新 L2**：**R35-1**（P2，T24 落地＝新能力，且 `accountable_user` 必须薄封装 `owner_cap`）；**R35-2**（P2，范围裁决：只改两处 vs 连 watchers/评论/指派一起改，后者会把「agent 的 watch」变成「所有者的 watch」）；**R35-3**（P3，`.invalid` 未核实项结案为「否」，并撤掉原方案的收件人过滤）；**R35-4**（P3，测试落点：给 `collab_common` 加 agent 助手或落到 `agent_principal_tests.rs` 的 `World`，不要第三套夹具）；**R35-5**（P3，映射/去重/权限查的顺序与 `MAX_CANDIDATES=500` 在映射后的意义）。
- **改动（WT 内 commit）**：新建 `ironforge-docs/architecture/agent-notify-routing-verification-2026-11.md`（19 条判定 + F1–F5 + R35-1…R35-5 + 复核命令）；`crates/rg-core/src/collaboration/human_notify.rs` **两处注释各 1→1 / 3→3 行**（`recipients()` 补「agent 候选不映射」、`task_blocked` 的 Who 段写明收件人可能是 agent），文件仍 **259 行**、`:69`/`:131`/`:189` 三个函数行号零漂移；`agent-principals-design-and-roadmap-2026-11.md` **6 处各 1 行 → 1 行**（§2.5 的 P2 行结案、方案 lead 补 `owner_cap` 复用、`recipients()` 补去重顺序、`task_blocked` 补「现状无 fallback」、邮件守卫改写为三处发信口的真值、测试段落补夹具位置），文件仍 **668 行**；`ironforge-docs/README.md` 索引 +1 行。**零代码行为改动**（`crates/` 只有 2 行注释）。
- **反向清单 74**；清单累计 **1..74**。
74. 「给映射后的收件人加 `kind = human` 且 `email NOT LIKE '%@agents.invalid'` 的过滤，就能关闭『agent 的 `.invalid` 邮箱是否会被邮件路径发信』这个未核实项」（issue #35 方案·邮件守卫 / 设计文档 §4 T24）—— **不成立**：通知路径不发信（`notification::notify` 只写库，`human_notify.rs`/`notification/` 内 `lettre|send_mail|smtp` 0 命中），过滤零效果；真实发信口三处且都已被 `ensure_human`（`repo/service.rs:489`）、密码重置 `:711` 的 agent 拒绝、重试队列的两个入队点挡住。**勿再以「通知路径会发邮件」为前提设计守卫。**
- **producer-stall**：`[opt-round]` 最近一批 `#31` 的 `closed_at` = `2026-11-11T13:09:01Z`，至本轮取证（`2026-11-12T05:56Z`）已 **16 h 47 m** 零新批（协议 §11 风险 10 的 180 min 与 6 h 阈值均已越过）⇒ 生产者两条 automation 需人工查；消费者无写权限，只记不改。第二段因此**无对象**（不是「队列为空」——手工队列仍剩 16 条）。
- **回填**：改动 SHA 与回帖 id 见本节末「改动 SHA / 回帖（回填）」段。issue #35 **保持 open**（单发轮不 close）。

**改动 SHA / 回帖（回填）**：`a41b327`（5 文件 `113+/9−`，**零代码行为改动**：`crates/` 只有 `human_notify.rs` 两处注释、文件仍 259 行）；推 `cnb` 单 remote（`721ab16..a41b327  HEAD -> main`）；`merge-base --is-ancestor` 通过，主工作区 `merge --ff-only` 到 `a41b327`、`status --porcelain` 0 行。**改后验证**：`scripts/check.sh --quick` `EXIT=0` **70 s**（`Finished dev profile [unoptimized + debuginfo] target(s) in 1m 08s` / `==> checks passed in 70s (quick)`）；改前 **104 s** `EXIT=0`（`Finished … in 1m 42s`）。**时刻锚** = `git log -1 --format=%aI a41b327` = `2026-11-12T14:00:29+08:00`。**回帖** `2120758766785159168`（`2026-11-12T06:01:06Z`，46412 字节；回读首行 `## 本轮收尾`、次行 `protocol=v0.12`、末行独立 `@CodeBuddy`、反向清单 74 条编号独立成行且 1..74 连续无缺；发帖前后该 issue 的 `## 本轮收尾` 计数 0 → 1，无重复投递）。**首扫请求** `2120756694840922112`（`05:52:52Z`），`60 s × 5` 超时未回，按正文推进。**推票坑（第二次复现）**：`git push` 用 `username=oauth2` 报 `Repository Not Found`，同一 token 换 `username=cnb` 即成功。

### 单发轮 · issue #36 · 2026-11-12（第 1 次进入；`[opt-round]` open = 0 ⇒ 第二段无对象）

protocol=v0.12

- **选靶依据**：17 条 open 全 MANUAL、`priority` 全空（任务书优先键失效，按协议 §5.2.1 用「消费次数 asc, 编号 asc」）。平台侧 `^## 本轮收尾` 计数（锚点 `body: "## 本轮收尾`）：`#14`=3、`#15`/`#16`/`#17`/`#18`/`#19`/`#33`/`#34`/`#35` 各 1，其余 8 条（`#36`–`#43`）**0** ⇒ 零消费里编号最小的 **#36**（`提交作者与推送者归因（T25）`，`comment_count` = 0）。`#19`/`#34`/`#35` 末条是本流程收尾（判据 A，等复核）⇒ 跳过。
- **无 NPC 分析 ⇒ 先发只读首扫请求**：评论 `2120766935095750656`（`2026-11-12T06:33:33Z`），第二人称限定「你是评审者、只读、不写收尾标记、未跑就标未跑、行号以 `6d47c28` 为准」，并附 6 条待核断言。`60 s × 5` 到 `06:38:33Z` 平台上仍只有我方一条 ⇒ **按 issue 正文推进，不重复发**（协议 §5.2「#26–#29 的准备缺口」）。
- **形态**：靶子是设计摘要（T25 提交作者与推送者归因）⇒ 本体是 L2（新函数 + 三个调用点 + push payload/审计面），本轮产出 = 核实现状断言 + 订正设计文档 6 行 + 新核实文档 + 登记 L2，**零代码行为改动**。
- **BASE** `6d47c28`（`HEAD` 与 `cnb/main` 同 SHA、`ls-remote` 一致；另 8 个 worktree 15 min 内零新文件 ⇒ 无并发写者）。改前 `scripts/check.sh --quick` **125 s EXIT=0**（`Finished dev profile [unoptimized + debuginfo] target(s) in 2m 03s` / `==> checks passed in 125s (quick)`）。
- **判定分布**：23 条现状断言 = **成立 12 / 部分成立 6 / 不成立 5**。全表见 `ironforge-docs/architecture/commit-identity-verification-2026-11.md` §2。

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| N1 | `repo_content.rs:1460-1478` 把 `&user.username`/`&user.email` 传给 `create_or_update_file_with` | **成立**（区间短 2 行） | `:1470`/`:1471` 是那两个实参；调用 `:1460` 起、参数表 `:1480` 的 `)` 收口 |
| N3 | 后者到 `ops.rs:1333` 用同一 signature 作 author 和 committer | **部分成立** | 语义成立（`:1579-1585` 构造一个 `signature`，`:1587` = `new_commit_as(signature, signature, …)`）；**行号错**：`:1333` 是 `plan_tree_edits` 返回结构里的 `tree: new_tree.to_string(),` |
| N4 | 零 `kind` 分支 | **成立** | `ops.rs` 的 16 处 `kind` 全是 `gix::object::Kind` 或 `entry.mode().kind()`（`:74`/`:612`/`:683`/`:983`/`:1168`/`:1519`），无 `users.kind`/`is_agent` |
| N6 | 「分支未合并，无存量需回填」 | **不成立** | 同文件 `:39` 标题即「第一期现状（分支 `feat/agent-principals`，**已合 main**）」；`m20261110_000001`、`agent.rs`、`api/agents.rs` 全在树里 |
| N9 | 「`repo_content.rs` 两个调用点（:1460、删除 :1571）」 | **不成立** | 三处：`:1460`、`:1600`（`delete_file_with`）、`api/repo_commits.rs:174`（`commit_batch`）；`:1571` 是 handler `delete_file` 的**声明** |
| N10 | 归因靠审计（T23） | **部分成立** | `audit_log.rs` 的 `on_behalf_of` **0 命中**；全仓唯一命中是 `agent.rs:23` 的 doc 注释 ⇒ T23 未落地 |
| N13 | `spawn_post_push_hooks`（`lib.rs:3190`） | **部分成立** | 真值 `:3202`；`:3190` = `StatusCode::GATEWAY_TIMEOUT,` |
| N14 | 它写的 `repo_events` payload 带推送者 id | **不成立** | payload（`lib.rs:3505-3513`）键只有 `ref/before/after/repository`；`PostPushParams`（`:3246-3260`）无 actor 字段 |
| N15 | 审计行带推送者 id | **不成立** | `record_write` 全仓 12 处无一在 push 路径；`"repo.push"` 0 命中；push 内唯一审计是分支删除（`lib.rs:3108`） |
| N16 | 「缺则补 `pushed_by`」 | **成立（要改签名）** | 推送者 id 调用点已有（`:3109` 的 `if let Some(user_id) = actor`）⇒ 透传可行，但 `spawn_post_push_hooks`/`PostPushParams` 签名变更属 L2 |
| N19 | 测试断言 `on_behalf_of = owner` | **部分成立** | `user_id = agent` 今天可断言（`:1515-1518`）；`on_behalf_of` blocked-by-T23 |
| N22 | 完成口径「新 SQL 跨后端跑一次」 | **部分成立** | 基建在（`tests/common/mod.rs:251`、`collab_common/mod.rs:37`、`rg-core/tests/multi_backend_smoke.rs`），但本方案零新 SQL ⇒ 无对象 |

- **最贵的一条（N9/F1）**：提交入口是**三处**不是两处。`2026-11-11` 合入的批量提交（issue #16 的落地）在 `api/repo_commits.rs:174` 走 `commit_batch(..., &user.username, &user.email, ...)`（`:185-186`），而设计文档快照早于它。按原文只改 `repo_content.rs`，agent 走 `POST …/commits` 仍会写出 `agent-<uuid>@agents.invalid`。**判据（可复用）**：判「改提交身份要动几处」数**服务端 commit 入口**（`create_or_update_file_with` / `delete_file_with` / `commit_batch`）的调用点，别数字段出现在哪些文件。
- **第二处产出（F2/F3）**：「谁推的可查」今天**完全不存在**，不是补字段：① push 无任何审计行（`record_write` 12 处无 push，`"repo.push"` 0 命中）；② payload 无推送者；③ `repo_events` 表无主体列（`entities/repo_event.rs:9-17` 只有 `id/repo_id/event_type/payload/created_at`）⇒ 塞 payload 等于放弃按主体筛选，与 T23 已定的「不塞 `details`（JSON 无法索引）」同一条原则相抵。**判据**：判「某事件带不带某主体」三层查——表有没有列 → payload 有没有键 → 有没有审计行；三层全缺时「确认已有」是假命题。
- **第三处产出（F4）**：服务端 merge/replay 的 committer 硬编码 `IronForge <noreply@ironforge.local>`（`ops.rs:309-313`），author 保留原提交者（`:378-389`）⇒ agent 的 PR 被服务端合并时 git 层看不出「谁点的合并」。不在 T25 范围，另开题（R36-6）。
- **好消息（F5）**：`agent_principal_tests.rs:290` 只断言 `ends_with("@agents.invalid")`，不锁 `agent-<uuid>` 前缀 ⇒ 派生方案不破现有测试，只需补 author 断言。
- **新 L2**：**R36-1**（P2，落地范围＝三处入口 + `commit_identity` 复用 `is_agent`，别写第 4 份 agent 判定）；**R36-2**（P2，`pushed_by` 落点：payload / 新审计 action `repo.push` / `repo_events` 加 `actor_id` 列 —— 三者取舍）；**R36-3**（**P1**，「谁推的可查」是新能力而非补字段，不能按 S（2–3 小时）估）；**R36-4**（P3，存量 agent 回填裁决；本机 `ironforge.db` 未迁移到 `m20261110_000001`，`select … where kind='agent'` 报 `no such column` ⇒ 无法用本机库证明存量）；**R36-5**（P3，注册侧不拒 `.invalid` 域，`user/service.rs:602` 只校验 local/domain 非空 ⇒ 理论上可先占字面地址使 `create_agent` 409）；**R36-6**（P3，merge/rebase committer 固定 `IronForge`，合并者归因另开题）。
- **改动（WT 内 commit）**：新建 `ironforge-docs/architecture/commit-identity-verification-2026-11.md`（23 条判定 + F1–F6 + R36-1…R36-6 + 复核命令）；`agent-principals-design-and-roadmap-2026-11.md` **6 处各 1 行 → 1 行**（`:371` 的 `ops.rs:1333`→`:1579-1587` 与区间 `1478`→`1480`、`:375` 的「分支未合并」→「已合 main + 不回填历史 commit」、`:376` 两处调用点→三处、`:377` 补「T23 未落地 + REST 三处写有审计行」、`:378` 补 `lib.rs:3202`/`PostPushParams` 无 actor/push 无审计行/actor 可用、`:380` 测试改 blocked-by-T23），文件仍 **668 行**、行号零漂移；`ironforge-docs/README.md` 索引 +1 行。**零代码行为改动**。
- **反向清单 75–78**；清单累计 **1..78**。
75. 「分支未合并，无存量需回填（若已合则只在提交时派生）」（设计文档 §4 T25 方案 1）—— **不成立**：同文件 §2 标题即「第一期现状（分支 `feat/agent-principals`，已合 main）」，迁移 `m20261110_000001` 与 `rg-core/src/agent.rs` 都在树里。**勿再以「未合并、无存量」为前提跳过回填裁决**（存量为「无」要有生产库证据，本机库未迁移）。
76. 「`repo_content.rs` 的两个调用点就是全部提交入口（创建/更新 :1460、删除 :1571）」（设计文档 §4 T25 方案 2）—— **不成立**：`api/repo_commits.rs:174` 的 `commit_batch` 是第三个入口（`:185-186` 同样传 `&user.username`/`&user.email`），而 `:1571` 是 handler 声明、调用在 `:1600`。反证：`grep -rn "commit_batch(\|create_or_update_file_with(\|delete_file_with(" crates/rg-http/src/api/`。
77. 「`spawn_post_push_hooks` 写的 `repo_events` payload 带推送者 id」—— **不成立**：payload（`lib.rs:3505-3513`）只有 `ref/before/after/repository`，`PostPushParams`（`:3246-3260`）无 actor 字段。反证：`sed -n '3505,3514p;3246,3260p' crates/rg-http/src/lib.rs`。
78. 「审计行带推送者 id（push 已有审计）」—— **不成立**：`audit::record_write` 全仓 12 处无一在 push 路径，`"repo.push"` 0 命中；push 内唯一审计是分支删除（`lib.rs:3108`）。反证：`grep -rn '"repo.push"' crates/ ; grep -c 'audit::record_write(' -r crates/ | grep -v ':0'`。
- **terminate**：`2 · 剩余未闭合项全部登记为 L2（R36-1…R36-6）待人工拍板`。按任务书**单发轮不 close**，留人工按判据 2 关闭。
- **producer-stall**：`[opt-round]` 最近一批 `#31` 的 `closed_at` = `2026-11-11T13:09:01Z`，至本轮取证（`2026-11-12T06:40Z`）已 **17 h 31 m** 零新批（协议 §11 风险 10 的 180 min 与 6 h 阈值均已越过）⇒ 生产者两条 automation 需人工查；消费者无写权限，只记不改。第二段因此**无对象**（不是「队列为空」——手工队列仍剩 16 条）。
- **回填**：改动 SHA 与回帖 id 见本节末「改动 SHA / 回帖（回填）」段。issue #36 **保持 open**（单发轮不 close）。

**改动 SHA / 回帖（回填）**：`b2ffdc9`（4 文件 `136+/6−`，**零代码行为改动**：`crates/` 一行未动）；推 `cnb` 单 remote（`6d47c28..b2ffdc9  HEAD -> main`）；`merge-base --is-ancestor` 通过，主工作区 `merge --ff-only` 到 `b2ffdc9`、`status --porcelain` 0 行。**改后验证**：`scripts/check.sh --quick` `EXIT=0` **2 s**（`==> checks passed in 2s (quick)`）；改前 **125 s** `EXIT=0`（`Finished dev profile [unoptimized + debuginfo] target(s) in 2m 03s`）。**时刻锚** = `git log -1 --format=%aI b2ffdc9` = `2026-11-12T14:39:51+08:00`。**回帖** `2120768788965523456`（`2026-11-12T06:40:55Z`，33055 字符；回读首行 `## 本轮收尾`、次行 `protocol=v0.12`、末行独立 `@CodeBuddy`、反向清单 78 条编号独立成行且 1..78 连续无缺；发帖前后该 issue 的 `## 本轮收尾` 计数 0 → 1，无重复投递）。**首扫请求** `2120766935095750656`（`06:33:33Z`），`60 s × 5` 超时未回，按正文推进。**推票坑（第三次复现）**：`git push` 必须用 `username=cnb`（`oauth2` 报 `Repository Not Found`）。

### 单发轮 · issue #37 · 2026-11-12（第 1 次进入；`[opt-round]` open = 0 ⇒ 第二段无对象）

protocol=v0.12

- **选靶依据**：17 条 open 全 MANUAL、`priority` 全空（任务书优先键在本仓恒失效，按协议 §5.2.1 用「消费次数 asc, 编号 asc」）。平台侧 `^## 本轮收尾` 计数：`#14`–`#19`/`#33`–`#36` 均 ≥1，**`#37`–`#43` = 0** ⇒ 零消费里编号最小的 **#37**（T26 API 响应携带主体类型）。
- **无 NPC 分析 ⇒ 先发只读首扫请求**：评论 `2120819625133764608`（`2026-11-12T10:02:56Z`），限定「你是评审者、只读、不写收尾标记、未跑就标未跑、行号以 `a41c9b2` 为准」+ 8 条待核断言。`60 s × 5` 到 `10:07:56Z` 平台上仍只有我方一条 ⇒ 按 issue 正文 + 设计文档 §4 T26 推进，不重复发。
- **形态**：功能/设计题，本体是 L2（OpenAPI 与前端契约，且可能需要响应 DTO）⇒ 产出 = 现状核实文档 + 设计文档 4 处 1→1 订正 + 台账，**零代码行为改动**。
- **BASE** `a41c9b2`（`HEAD` 与 `cnb/main` 同 SHA；`ls-remote` tip 停在 `06:58Z`，距取证 3 h；`opt-worktrees` 为空、其余 8 个 worktree mtime 均早于本轮 ⇒ 无并发写者）。改前 `scripts/check.sh --quick` **126 s `EXIT=0`**（`Finished dev profile [unoptimized + debuginfo] target(s) in 2m 05s` / `==> checks passed in 126s (quick)`）。
- **判定分布**：22 条断言 = **成立 11 / 部分成立 5 / 不成立 5 / 不可验 1**。全表见 `ironforge-docs/architecture/actor-identity-response-verification-2026-11.md` §1。
- **最贵的一条（N17）**：设计第 3 步「capabilities 新增 `actors.identity`（routes 指向上述四处）」**有硬阻塞**——`collab_contract_tests.rs:79` 的采集过滤器只收 `path.contains("collaboration")`，而 `registry_operations()`（`:103-109`）取全部 REGISTRY 路由，`:116-134` 断言两侧差集为空 ⇒ 往 REGISTRY 写 PR/评审/评论/审计路由必使 `stale` 非空。该步实为「改防漂移测试口径」，不是「加一条能力」。
- **第二处产出（N13–N15）**：四处落地点的字段名 **3 处写错**。PR 只有 `author_id: i64`（`entities/pull_request.rs:27`，详情 `pulls.rs:229` 直接 `json!(pr)`）、评审只有 `PrReview.reviewer_id`（`entities/pr_review.rs:16`，`reviewer` 仅存在于事件 payload `reviews.rs:1116`/`:1182`）、评论字段叫 `author` 且是 `Option<String>`（`issues.rs:286`）⇒ 前三条是**新增**嵌套对象，不是扩已有字段。唯一具名落点是审计 `AuditLogEntry`（`audit.rs:29-40`，带 `ToSchema`）。
- **第三处产出（F1/F2，两份材料都没有）**：`api_capabilities` 是本仓既有的「加法变更宣告」通道——`api/collab_capabilities.rs:104` 有四个 `.v1` 旗标，`collab_contract_tests.rs:386-395` 逐条断言 ⇒ 宣告 `actors.identity.v1` 只需 1 行 + 1 条断言，绕开 N17 的路由冲突。这是设计未写的第三条路（R37-1）。
- **第四处产出（N20 方向反了）**：风险说「多个 handler 用 `json!` 拼响应」——四处里只有 PR 详情 1 处 `json!`（`pulls.rs:229`）；真障碍是 PR **直接序列化 DB 实体**，加展示字段要么后置 enrich（详情有 fork 先例 `pulls.rs:230-237`），要么引 DTO 并换掉 `PaginatedResponse<PullRequest>` 的元素类型（`:195`）。
- **新 L2**：**R37-1**（P1 宣告通道裁决：`api_capabilities` 旗标 vs 新建 Capability）· **R37-2**（P1 PR 响应 DTO 路线：enrich vs 具名 schema）· **R37-3**（P2 `ActorRef` 与 `TimelineActor`（`reviews.rs:75-78`）/`AgentInfo`（`agent.rs:91-99`）合并，否则同一页面两种 actor 形状）· **R37-4**（P2 评论 `author` 改对象是**破坏性改名**，与「只增不改名」冲突 ⇒ 需 `author_ref` 或升 MAJOR）· **R37-5**（P3 审计 `username` 快照 vs 现查口径）· **R37-6**（P3 `on_behalf_of` 依赖未落地）。
- **改动（WT 内 commit）**：新建 `ironforge-docs/architecture/actor-identity-response-verification-2026-11.md`（22 条判定 + 四处形状表 + 三条宣告路径 + F1–F5 + R37-1…R37-6 + 复核命令）；`agent-principals-design-and-roadmap-2026-11.md` 4 处 1→1（现状 / 方案 2 / 方案 3 / 风险，文件仍 **668 行**）；`ironforge-docs/README.md` +1 行索引；本台账。
- **反向清单 79–83**；清单累计 **1..83**（其中 72–73 由「单发轮 #34」节写入、74 由「单发轮 #35」节写入、75–78 由「单发轮 #36」节写入，物理均在各节末）。
79. 「PR 详情/列表有 `author` 字段，只需加 kind/owner」—— **不成立**：`pull_requests` 只有 `author_id: i64`（`entities/pull_request.rs:27`）；`grep -rn '"author"' crates/rg-http/src/` 唯一命中是提交历史的 `CommitEntry.author`（`repo_content.rs:183`，git 作者名，与账号无关）；前端同样只读 `author_id`（`web/src/lib/types/entities.ts:57`）。
80. 「评审列表有 `reviewer` 字段」—— **不成立**：`list_reviews` 直接返回 `Vec<PrReview>`（`reviews.rs:281-283`），实体只有 `reviewer_id: i64`（`entities/pr_review.rs:16`）；`reviewer` 只出现在事件/时间线 metadata（`reviews.rs:729`/`:1116`/`:1182`），值是用户名字符串。
81. 「评论列表有 `user` 字段」—— **不成立**：字段名是 `author`，类型 `Option<String>`（`issues.rs:283-287`），`author_name()` 只查 username（`:1124-1140`）；`grep -rn '"user":' crates/rg-http/src/api/` = 0 命中。
82. 「多个 handler 直接返回 `serde_json::Value`（`json!` 拼的），类型系统帮不上忙」—— **不成立**：四处里仅 PR 详情 1 处 `json!`；评论 `CommentResponse`、审计 `AuditLogEntry` 是具名结构体，评审是实体 ⇒ 类型系统全程在场，真障碍是「实体直出」。
83. 「T23/T25 已落地，可直接给 `ActorRef` 填 `on_behalf_of`」—— **不成立**：`on_behalf_of` 全仓仅 `agent.rs:23` 一处 doc 注释（无列、无字段、无写入）；`commit_identity` 0 命中，提交身份仍原样传 `user.username`/`user.email`。
- **producer-stall**：`[opt-round]` 最近一批 `#31` 的 `closed_at` = `2026-11-11T13:09:01Z`，至本轮取证（`2026-11-12T10:02Z`）已 **20 h 53 m** 零新批（协议 §11 风险 10 的 6 h 阈值已超 3.4 倍）⇒ 生产者两条 automation 需人工查，消费者无写权限只记账。
- **下靶**：零消费剩 `#38`–`#43`（6 条，`comment_count` 全 0）；按（次数 asc, 编号 asc）下一靶 **#38**（「推送时密钥扫描 R7」，`comment_count` = 0 ⇒ 先发只读首扫 + `60 s × 5`，超时按正文推进）。
- **回填**：改动 SHA 与回帖 id 见本节末「改动 SHA / 回帖（回填）」段。issue #37 **保持 open**（单发轮不 close）。

**改动 SHA / 回帖（回填）**：`b8e8dad`（4 文件 `118+/5−`，**零代码行为改动**：`crates/` 一行未动；新文档 85 行、设计文档仍 668 行、README +1 行）；推 `cnb` 单 remote（`a41c9b2..b8e8dad  HEAD -> main`）；`merge-base --is-ancestor` 通过，主工作区 `merge --ff-only` 到 `b8e8dad`、`status --porcelain` 仅剩他人未提交的 `CLAUDE.md`（非本轮文件，未触碰）。**改后验证**：`scripts/check.sh --quick` `EXIT=0` **2 s**（`Finished dev profile [unoptimized + debuginfo] target(s) in 0.25s` / `==> checks passed in 2s (quick)`）；改前 **126 s** `EXIT=0`。**时刻锚** = `git log -1 --format=%aI b8e8dad` = `2026-11-12T18:10:03+08:00`。**回帖** `2120821563959189505`（`2026-11-12T10:10:38Z`；32723 字符；回读首行 `## 本轮收尾`、末行独立 `@CodeBuddy`、反向清单 83 条编号独立成行且 1..83 连续无缺；该 issue 的 `## 本轮收尾` 计数 0 → 1，无重复投递）。**首扫请求** `2120819625133764608`（`10:02:56Z`），`60 s × 5` 超时未回，按正文推进。**第二段无对象**：open 17 条里 `[opt-round]` = 0。

### 单发轮 · issue #45 · 2026-11-12
- 选靶：[hold][harness-test]，最新评论 2120836458603773952（2026-11-12T11:09:49Z，NPC 首扫，Q1–Q5）
- 判定：成立 2（Q1、Q3）/ 部分成立 3（Q2、Q4、Q5）/ 不成立 0
- 动作：L1 `858fa44`（`lfs.rs:213` 注释 1→1）；L2 写入 L2-45-1…5（均未改代码）：①临时文件固定名 `.tmp_{oid}` 且截断写、失败不清理（`lfs.rs:214/426`，`service.rs:539-550`）；②上传不复核 SHA-256、`size` 不与实收字节对账（`service.rs:243`）；③无单对象/仓库配额，仅 10 GiB 路由层限额（`lib.rs:965`）；④私有仓库 404 先于 401 可枚举（`lfs.rs:181-186`、`repo/service.rs:169`），batch 对未登记 oid 整批 500（`service.rs:399-401`）；⑤batch 响应非 `application/vnd.git-lfs+json`、无 Range（`lfs.rs:148`）。④⑤为静态读码，未运行实测
- 验证：BASE=fd2f07e，**executor 本机** `check.sh --quick` 改前 EXIT 0 / 131 s、改后 EXIT 0 / 65 s
  （NPC 复核容器无 `cargo`（`command -v cargo` rc=127）⇒ 这两个数字**不可由第三方复算**，只能当执行侧自述；同类口径见协议「证据形态」条）
- 回帖：2120837912609603584（2026-11-12T11:15:36Z）｜`## 本轮收尾`（不占 C）
- issue 状态：保持 open

### 单发轮 · issue #48 · 2026-11-13（第 1 次进入；`[opt-round]` open = 0 ⇒ 第二段无对象）

protocol=v0.13

- **选靶依据**：11 条 open 全 MANUAL；`priority` 只有 `#48` 是 P2、其余全空 ⇒ 按任务书「priority 优先」直接取 **#48**（`comment_count=0`）。平台侧 `^## 本轮收尾` 计数：`#14`=3、`#15`/`#17`/`#18`/`#19`/`#37` 各 1、`#38`–`#41`/`#48` = 0。
- **无 NPC 分析 ⇒ 先发只读首扫请求**：评论 `2120957694004633600`（`2026-11-12T19:11:34Z`），限定「你是评审者、只读、不写收尾标记、未跑就标未跑、行号以 `94dc671` 为准」+ 8 条待核断言。`60 s × 5` 窗口内（`19:15:05Z`，请求后 3 m 31 s，首扫流水线 `success [3m 35s]`）NPC 回了只读首扫 ⇒ 按其结果一并复核，未重复发请求。
- **形态变了（本轮最重要的一件事）**：`main` 已从 `94dc671` 之前推进一大批（PR #49–#56 合入），#48 **不再是一条待开工 issue**：第 1 项（MCP `commit_files`）随 PR #54（`e7110e6` / 分支提交 `7271b87`）已落地，第 2 项（unified diff）已有决策文档，只剩第 3 项（cherry-pick / revert 端点）。所以本轮＝**核实实施回报 + 确认剩余项**，不是评审设计。
- **BASE** `94dc671`（`HEAD` 与 `cnb/main` 同 SHA；`opt-worktrees` 为空，另有 7 个 `.claude/worktrees` 属他人并行分支、未触碰）。改前 `scripts/check.sh --quick` **110 s `EXIT=0`**（`Finished dev profile [unoptimized + debuginfo] target(s) in 1m 48s` / `==> checks passed in 110s (quick)`）。
- **判定分布**：26 条断言 = **成立 15 / 部分成立 6 / 不成立 5**（其中 2 条「不成立」是**时点失效**，不是错误：正文写于 `2026-11-12T16:12Z`，早于 PR #54）。
- **最贵的一条（D9 / R48-1）**：两份新文档把第 3 项没做的原因写成「与 #14 的路由注册表重构冲突」——**该阻塞已经不在了**。R1 已合 main（PR #55，`0b05502`），`rg-core/src/auth/route_meta.rs` 771 行，`rg-http/tests/route_metadata_tests.rs` 校验「注册了路由没登记 / 登记了没路由 / `.checked()` 路由没调 `ensure_token_may_write_branch`」三种漂移。**判据（可复用）**：判「某新路由能不能写」先读 `route_meta.rs` 头部「Adding a route」契约——移 ref 的写路由必须 `.checked()`，响应无秘密才 `.idem()`，漏登记是测试红灯而不是静默默认。本条已在本轮就地订正（两处 1→1 行）。
- **第二处产出（.ai/README.md:182 / R48-4）**：工具表已经加了 `commit_files` 行，但**同一节底部的「哪些工具会写仓库」枚举没跟上**——原文只有 `commit_file`、`delete_file`、`create_pr`。这句话是读者判断「要不要带分支受限令牌」的唯一入口，漏掉 `commit_files`（写文件、走 `.checked()`）和 `delete_branch`（删 ref）会让 agent 以为它们是只读。已补两个名字（1→1 行）。
- **第三处产出（可复用证据链）**：`commit_files` 的「一次调用 = 1 commit = 1 CI」**不是只有自述**——`scripts/agent-e2e.py:161-243` 带对照组（4 次 `commit_file` ⇒ 4 commit / 4 pipeline），批量侧断言 `runs_after - runs_before == 1` 并检查新 commit 的父是调用前 tip。但它是**手跑脚本、不在 CI**（`check.sh` 三种模式都不调它）。
- **第四处产出（验收 2 是空口径）**：`tools/list` 的断言只查「按名字 + `required` 列表」（`write_tools_mcp.rs:281-336`），仓库里**没有**工具总数的断言（`grep -rn 'tools.len()\|tools).len\|tool_count' crates/rg-mcp/tests/*.rs crates/rg-mcp/src/*.rs` 输出为空）⇒ 正文要求的「工具表数量同步更新」无强制手段，`14 → 15` 只能靠人读。
- **本轮自跑证据（executor 本机）**：`cargo test -p rg-mcp` = lib **17** / `collab_mcp` **21** / `read_tools_mcp` **10** / `write_tools_mcp` **12** 全 ok（60 passed / 0 failed），与验证文档自报的四个数字逐个相符。**复算环境无 cargo 时不可验**。
- **新 L2**：**R48-1**（P1，第 3 项落地顺序：先 `route_meta::ROUTES` 两行 `.checked()`，再 handler；`revert`/`cherry-pick` 都要 `audit::record_write`，现 12 处无一在 `commits/{sha}/*`）· **R48-2**（P2，unified diff 需负责人拍板是否引入依赖；自研可仿本仓 `rg-core/src/pull_request/gix_patch.rs` 的 `parity_tests` 对拍 `git apply --check`）· **R48-3**（P2，`files` 无条数/字节上限，REST 与 MCP 都没有；应与补丁方案的 1 MiB / 200 文件上限一起定）· **R48-4**（P3，`.ai/README.md` 写工具枚举漏项，本轮已修）· **R48-5**（P3，无工具总数断言）· **R48-6**（P3，`commit_files` 不能建分支，新分支要先 `commit_file`——工具描述已写，但 issue 验收没提，属已知限制不是缺陷）。
- **反向清单 84–89**；清单累计 **1..89**（84–89 物理在本节末）。
84. 「`crates/rg-mcp/src/tools/mod.rs` 只有单文件 `commit_file`（`:111`）与 `delete_file`（`:131`）、工具表 14 项无批量项」—— **时点失效**：今天 15 项，`commit_files` 占 `:131`，`delete_file` 已被推到 `:165`。反证：`grep -n '"name": "' crates/rg-mcp/src/tools/mod.rs`。
85. 「`BatchCommitRequest.files` 只接 `content` 字符串（`repo_commits.rs:34-66`）」—— **不成立**：`:33` 起是 `BatchCommitFileRequest`（create/update/delete/move 四变体 tagged enum），`BatchCommitRequest` 本体在 `:80`；「只接 content」在 `48f03a7` 之后即失效。
86. 「issue #48 第 3 项与 #14 的路由注册表重构冲突，因此本会话没碰」（决策文档状态行与 §3 各一处）—— **不成立（本轮已订正）**：R1 已合 main（PR #55 / `0b05502`），阻塞变成「要在 `route_meta::ROUTES` 加两行」，不是「等它合并」。
87. 「`gix_patch::unified_patch` 只会生成补丁」易被读成外部 crate —— **部分成立**：`gix-patch` 不在 `Cargo.lock`，`unified_patch` 是本仓模块 `crates/rg-core/src/pull_request/gix_patch.rs:36`；「只会生成、gix 无 apply」本身对（`~/.cargo/registry/.../gix-0.90.0/src/` 无 `pub fn apply`）。
88. 「`.ai/README.md` 的写工具枚举＝`commit_file`、`delete_file`、`create_pr`」—— **不成立**：漏 `commit_files` 与 `delete_branch`，本轮已补（`sed -n '182p'`）。反证：工具表 15 项里 `commit_files`（`:131`）与 `delete_branch`（`:184`）都有写副作用。
89. 「验收 2：`tools/list` 快照测试与工具表数量同步更新」—— **部分成立**：断言是按名字 + `required` 列表逐条查（`write_tools_mcp.rs:281-336`），**没有**总数断言；`14 → 15` 无机械校验。
- **producer-stall 已解除**：`[opt-round]` 队列仍为 0，但本轮观察到生产者这一侧改走「PR 直推 + 文档沉淀」（PR #49–#56 一夜合入 8 个），队列形态从「issue 批次」变成「PR 批次」⇒ 第二段**依旧无对象**，不是生产者停摆。
- **下靶**：零消费剩 `#38`–`#41`/`#48`（`comment_count` 全 0，其中 `#48` 本轮已 1 次）；按（次数 asc, 编号 asc）下一靶 **#38**（「推送时密钥扫描 R7」）。
- **回填**：改动 SHA 与回帖 id 见本节末「改动 SHA / 回帖（回填）」段。issue #48 **保持 open**（第 3 项未做，单发轮不 close）。

- **NPC 首扫回复（`2120958578398777344`，`2026-11-12T19:15:05Z`）已逐条复算**：其 12 条断言 **成立 11 / 部分成立 1**（唯一偏差是函数名笔误 `rebore_skips_already_applied_commits`，真值 `rebase_skips_already_applied_commits()` 在 `ops.rs:1726`；它的 `#[cfg(test)] = :1630`、`batch_commit_tests` 10 个 `#[tokio::test(flavor=…)]`、`audit.rs:332` 定义 + 12 调用点、`ensure_token_may_write_branch` 4 调用点、`BatchEdit` `service.rs:2211`、`repo_commits.rs:226` 的 `details` 五个键、`collab_mcp.rs:323` 只有 `names.contains` 全部复现）。**它自报无 `cargo`（`sh: 1: cargo: not found`）⇒ 它的「未跑验证」自述可信，编译类结论仍由我方出。**
- **它补出两条我方漏掉的计数（已采信并回填本节）**：① `list_tools` 在 `mod.rs:263` 追加 `crate::collab::tool_definitions()` ⇒ `tools/list` 实际返回 **74 项**（15 非协作 + 59 协作），本节写的「15 项」只是字面量表、不是对客户端可见的工具数；② `grep -rn 'commit_files' crates/rg-mcp/` 真值 **14**（我一度口算成 12，`head -20` 截断所致）。
- **它的一处结论比正文更硬**：`.ai/guardrails.md` 里 `cherry-pick`/`revert` 零命中 ⇒ #48 正文那句「按 🟠 CONFIRM 处理」是**正文自己的判读**，不是护栏原文；判读本身有据（撞 `:102` 认证 /`:104` 权限校验 /`:118` 分支保护 三条 🟠；**本节原写 `:105`，第 2 次进入轮订正，见反向清单 92**），但写成「护栏里写着」会让人去找不存在的规则 ⇒ 与本节反向清单 88 同一类（文档枚举与实际不一致）。
**改动 SHA**：`6075fbd`（2 文件 `4+/4−`，**零代码行为改动**：`crates/` 一行未动；`.ai/README.md` 1→1 行仍 276 行、决策文档 3 处各 1→1 仍 72 行 ⇒ 全仓 `path:line` 零漂移）。**改后验证**：`scripts/check.sh --quick` `EXIT=0` **2 s**（`Finished dev profile [unoptimized + debuginfo] target(s) in 0.32s` / `==> checks passed in 2s (quick)`）；改前 **110 s** `EXIT=0`。`cargo test -p rg-mcp` 60 passed（见上）。**时刻锚** = `git log -1 --format=%aI 6075fbd`。
**回帖**：`2120959167748845568`（`2026-11-12T19:17:25Z`）；回读首行 `## 本轮收尾`、次行 `protocol=v0.13 · prompt=ironforge-consumer-v4`、末行独立 `@CodeBuddy`、反向清单 89 条编号独立成行且 1..89 连续无缺；该 issue 的 `## 本轮收尾` 计数 **0 → 1**，无重复投递。**先发后修一次**：初版把 NPC 首扫时间写成 `19:13:58Z`（凭 3m35s 时长反推，未读 `created_at`），实测 `19:15:05Z` ⇒ 已 `patch-issue-comment` 改正并同步本节。**教训：引用平台评论时间别反推，`list-issue-comments` 的 `created_at` 逐条读。**


### 单发轮 · issue #48 · 2026-11-13（第 2 次进入：核实 NPC 首扫 + 复核自己上一轮的两处订正）

protocol=v0.13。靶 #48（唯一有 `priority`=P2；本轮起 `comment_count=1`，等于 NPC 首扫 `2120958578398777344`）。

- **N-C 分类（两个不同的东西）**：NPC **没写任何文件**（自报无 `cargo`，只做只读首扫）；本轮复核对象 `7271b87` 是 PR #54 的分支提交，作者 `yuqu`，已含于 `94dc671`。我方上一轮的 `6075fbd`/`6edb407`/`cd58bc8` 在它之后，**本轮才第一次被复核**。
- **判定分布**：38 条 = 成立 26 / 部分成立 7 / 不成立 5（A 组 13、B 组 13、C 组 NPC 12）。
- **本轮最贵的一条（R48-7，P1，订正我自己上一轮给的落地指导）**：决策文档原文（我上一轮写的）说「第 3 项先在 `route_meta::ROUTES` 加两行（移 ref ⇒ `.checked()`）」。**`.checked()` 不是自选的**：`rg-http/tests/route_metadata_tests.rs:365-394` 要求每条已注册路由的四个判定逐条等于旧表 oracle（`mod legacy`，`:29-162`；`:27` 是它的节标题注释）。`legacy::branch_limited_write` 只认精确的 `["commits"]`（`:127`）、`legacy::idempotent` 认 `commits` 前缀（`:145-159`）⇒ 落在 `commits/{sha}/` 下的 cherry-pick / revert **必得 `BranchLimit::Refused` + `.idem()`**；想要 `.checked()`（让受限令牌能 cherry-pick）或想拒幂等键，必须同时改 oracle，那是显式决定。已在决策文档 `:4`/`:71` 就地订正（2 文件外的 1 文件、2 行 1→1）。
- **第二处（R48-8，P2）**：验证文档 `mcp-commit-files-verification-2026-11.md:21` 写「17 种坏参数」，**真值 16**。`commit_files_checks_its_arguments_before_any_request`（`write_tools_mcp.rs:446-534`）的 case 数是 `json!({` 与期望串各 **16**（`7271b87`/`94dc671`/`HEAD` 三个时点同值）。括号里枚举的 16 项能一一对上，数字多算了 1。
- **第三处（R48-9，P3，我方上一轮文字的自伤）**：上一轮把「cherry-pick/revert 撞护栏 🟠」的第三条写成 `guardrails.md:105`，**`:105` 是「添加/修改密码哈希参数」**，分支保护在 **`:118`**（`94dc671` 与 `HEAD` 同）。判读（撞 :102 认证 / :104 权限校验 / :118 分支保护，三条 🟠）依然成立，只有行号错。
- **`.ai/README.md:182` 复核结论：成立**。`commit_files` 走 `POST /repos/{o}/{n}/commits`（`route_meta.rs:345` `.checked().idem()`）、`delete_branch` 走 `branches/{*branch}`（`:340-342` `.checked().idem()`），两者都写仓库、都受分支限制 ⇒ 该算「会写仓库」。同节 `:163`/`:165` 的工具说明也已各自写全，无第三处需补。
- **`route_metadata_tests` 是否真的强制新路由登记：是**，且是三向：`:320-350` 双向（注册了没登记 / 登记了没路由，`:335-344` 的失败文案原文就写着「Add a row」）、`:535-566` `.checked()` 必须 handler 真调 `ensure_token_may_write_branch`、`:352-362` 已注册路由不许落到家族默认。**注意 `GUARDS` 不是后门**：`declared_patterns()`（`route_meta.rs:622-640`）把 GUARDS 过滤掉，登记不到 `declared_set`，绕不过第一条。
- **反向清单 90–92**（见下）。清单累计 1..92。
- **未跑**：本容器 `command -v cargo` 为空 ⇒ `check.sh`/`cargo test` 全部未复算，A13 与「60 passed」只能记自述。
- **靶状态**：第 3 项（cherry-pick / revert 端点）仍是唯一实质工作量，零行代码；unified diff 需负责人就 `diffy` 依赖（`guardrails:107` 🟡）拍板。issue #48 保持 open。

90. 「`commit_files_checks_its_arguments_before_any_request` 覆盖 **17** 种坏参数」（验证文档 `:21`）—— **不成立**：真值 **16**。反证：`awk` 数该函数体内 `json!({` 与期望串各 16（`write_tools_mcp.rs:450-528`），`7271b87`/`94dc671`/`HEAD` 三点同值；括号列出的 16 项逐条可指。**已核实，勿再引 17。**
91. 「第 3 项只需在 `route_meta::ROUTES` 加两行、移 ref 就用 `.checked()`」（决策文档 `:4`/`:71`，我方上一轮自撰）—— **不成立**：`route_metadata_tests.rs:365-394` 的 oracle 等值断言把 `commits/{sha}/*` 钉成 `Refused` + `.idem()`（`legacy::branch_limited_write:127` 只认 `["commits"]`；`legacy::idempotent:145-159` 认前缀）。**`.checked()` 需要改 oracle，是显式决定。已就地订正。**
92. 「cherry-pick/revert 判读有据的三条护栏行号是 `:102`/`:104`/`:105`」（我方上一轮回帖）—— **不成立**：`:105` 是「添加/修改密码哈希参数」，分支保护在 **`:118`**。判读成立、行号错一条。

**改动 SHA**：本轮 1 个提交，3 文件（两份文档各 1 行 1→1、台账追加），`crates/` 零行改动 ⇒ 全仓 `path:line` 零漂移（`.ai/README.md` 仍 276 行、两份文档仍 72/30 行）。**BASE** `35b17b9`（= `22e196c` 的父提交 = 当时 `origin/main`；原写 `cd58bc8` 已订正，见反向清单 94）；**未跑验证**（本容器无 `cargo`），只做了逐条静态复算。

### 单发轮 · issue #48 · 2026-11-13（第 3 次进入：核实 NPC 复核 `2120963285645197312` 与它直推的 `22e196c`）

protocol=v0.13。靶 #48（11 条 open 里唯一 `priority`=P2；`comment_count`=4）。**上一节「第 2 次进入」由复核侧自撰**：它把 `22e196c` 直推 `origin`（git author `yuqu`、CNB 显示 `cnb.biewen`），台账段也是它写的；其中 2 处数字错由本节订正（BASE 见反向 94，`mod legacy` 起点见本节判定表）。按协议不 revert 外部推进，改以 `22e196c` 为 BASE 复算。

- **判定分布**：27 条 = **成立 23 / 部分成立 2 / 不成立 2**（对象是它这一轮的复核意见与自述，不是 issue 正文）。
- **R48-7（P1）核实结果：成立**。`rg-http/tests/route_metadata_tests.rs:365-394` 对**每条已注册路由**逐个 `assert_eq!` 四个派生判定与 `mod legacy` 旧表（scope 两向 / `reaches_other_repos` / `branch_limited_write` / `idempotent`）。四个值里只有两支真正被钉：`legacy::scope(false, /repos/o/n/commits/<sha>/cherry-pick)` = `repo:write`、`reaches_other_repos` = false（`:98-101` 的 `matches!` 只认 fork/forks/transfer/collaboration groups），与 `repo()` 构造器默认一致；钉死的是 `branch_limited_write`（`:127` 精确 `["commits"]`）与 `idempotent`（`:145-159` 的 `commits` 前缀）⇒ 新路由落 `commits/{sha}/` 必得 `Refused` + `.idem()`。**要 `.checked()` 就得改 oracle**，这条成立。
- **它的 `mod legacy` 区间偏 2 行**：`mod legacy {` 在 `:29`（`:27` 是 `// ── Oracle: …` 节标题），闭括号 `:162` 对 ⇒ 部分成立。
- **R48-8 核实结果：成立 16**。`commit_files_checks_its_arguments_before_any_request`（`:446-534`）的 `for (args, wants) in [` 里 **16** 个元组，`json!({` 与期望串各 16；本轮在 `22e196c` 复算同值（三点同值成立）。**但「括号里枚举的 16 项能一一对上」只部分成立**：验证文档括号只列 8 类 + 「等」（缺 branch / 缺 expected_sha / 缺 message / 空 files / 未知 operation / update 缺 sha / 多余字段 / 非字符串字段），16 个 case 里另有 5 类不在括号内（delete 缺 `sha`、move 缺 `to`、`files` 非数组、`owner` 非法字符、`dry_run` 非 bool）⇒ 记 R48-12。
- **R48-9 核实结果：成立**。`:105` = 「添加/修改密码哈希参数」🟠、`:118` = 「修改分支保护逻辑」🟠；`:102` 认证 / `:104` 权限校验亦对。
- **其余断言逐条复现成立**（22 条）：`.ai/README.md` 276 行 / 决策文档 72 行 / 验证文档 30 行（零漂移）；`:182` 写工具枚举含 `commit_files`+`delete_branch`；`route_meta.rs:345` = `commits.checked().idem()`、`:340-342` = `branches/{*branch}.checked().idem()`；`.ai/README.md:163`/`:165` 说明已写全；`route_metadata_tests.rs:320-350` 双向、`:341` 失败文案含 `Add a row`、`:535-566` `.checked()` 必须 handler 真调 `ensure_token_may_write_branch`、`:352-362` 已注册不许落家族默认；`declared_patterns()`（`route_meta.rs:622-640`）过滤 `GUARDS` ⇒ 不是后门；`mod.rs:263` + `TOOL_NAMES: [&str; 59]` ⇒ `tools/list` 74 项；`Cargo.lock` 0 命中 `gix-patch`；`scripts/check.sh:36-46` 证实 `--quick` 只跑 fmt+clippy。
- **它两处不成立**：① 「`audit::record_write` 现 13 处调用」——`grep -rn 'audit::record_write' crates/ | wc -l` = **12**，13 是含定义的 `grep -rn record_write` ⇒ 反向 93；② 它自己台账段的 BASE `cd58bc8`（真值 `35b17b9`，`git log -1 --format=%p 22e196c`）⇒ 反向 94。**「无一在 `commits/{sha}/*`」成立**：12 处分布 agents 4 / repo_content 3 / pulls 2 / repo_commits 1（`POST commits`，`:226`）/ reviews 1 / lib.rs 1（分支删除 `:3041`）。
- **第 3 项零行代码：成立且更强**——`revert` 在 `crates/**/*.rs` **0 命中**；`cherry-pick` 唯一一处在 `rg-git/src/ops.rs:1735`，属 `rebase_skips_already_applied_commits` 的测试夹具 ⇒ 没有可抄的生产实现。
- **新 L2**：**R48-10**（P1，`Refused` 不是免费选项——`rg-http/src/lib.rs:2211-2223` 对带 `ref_allowlist` 的令牌直接拒，`refs:["agent/*"]` 就不能 cherry-pick；选 `.checked()` 则三处代码 + 两处文字枚举同改，**本轮已写进决策文档 §3**）· **R48-11**（P2，`.checked()` 落地后 `pat_scope.rs:289-293`「Three ref-moving families」与 `route_metadata_tests.rs:562-565` 断言文案会与实现打架）· **R48-12**（P3，验证文档括号枚举 8 类 vs 16 case；16 case 仅 15 条不同错误文案）· **R48-13**（P3，**L1 未改，下轮候选**：`rg-http/src/lib.rs:2208-2210` 注释称 handler「不知道是哪个 token」，而 `repo_content.rs:675/1453/1596` + `repo_commits.rs:166` 四处都传 `&claims`，与 `pat_scope.rs:289-292` 及同文件 `:2216` 的实现直接矛盾）· **R48-14**（P3，`22e196c` 让台账尾缺 `\n`，本轮追加时补）。
- **本轮改动**：`docs/reports/OPT-LOOP-LEDGER.md`（2 处 1→1 订正 + 本节追加）+ `ironforge-docs/architecture/mcp-commit-files-and-patch-decision-2026-11.md`（§3 那 1 行 1→1 扩写，仍 72 行）。`crates/` 零行 ⇒ 全仓 `path:line` 零漂移。
- **反向清单 93–94**（见下）。清单累计 1..94。
- **靶状态**：issue #48 保持 open（第 3 项与 unified diff 均未做）。

93. 「`audit::record_write` 现 13 处调用」—— **不成立**：**12** 处调用。反证：`grep -rn 'audit::record_write' crates/ | wc -l` = 12；13 是 `grep -rn record_write crates/`（含 `audit.rs` 里的定义）。
94. 「本节 BASE `cd58bc8`（= 上一轮 HEAD = `origin/main`）」—— **不成立**：`22e196c` 的父提交是 `35b17b9`（`git log -1 --format='%h %p' 22e196c`），`cd58bc8` 是它往上第二个。

**改动 SHA / 回帖（回填）**：`b5fd891`（2 文件 `23+/3−`，`crates/` 零行）；推送 `22e196c..b5fd891` 快进，主工作区 `merge --ff-only` 至同 SHA。验证（executor 本机）：改前 `check.sh --quick` `EXIT=0` 87 s（`Finished … in 1m 25s`）、改后 `EXIT=0` 1 s（`Finished … in 0.24s`）；另 `cargo test -p rg-http --test route_metadata_tests` 9 passed / 0 failed（编译 67 s）。**复算环境无 `cargo` 时不可验**。回帖 `2120974835932127232`（`2026-11-12T20:19:41Z`），首行 `## 本轮收尾`，该 issue 的 `## 本轮收尾` 计数 1 → 2（单发轮每轮一条，不占批次 C；`[opt-round]` open = 0 ⇒ 第二段无对象）。

### 单发轮 · issue #38 · 2026-11-13（第 1 次进入；`[opt-round]` open = 0 ⇒ 第二段无对象）

protocol=v0.13

- **选靶依据**：11 条 open **全 MANUAL**（`[opt-round]` = 0）⇒ 第二段无对象。`priority` 只有 `#48`=P2，但其最新评论 `2120974835932127232`（`2026-11-12T20:19:41Z`）是我方 `## 本轮收尾` ⇒ **等待复核，跳过**。剩 `#14`/`#15`/`#17`/`#18`/`#37` 最新评论是**未消费的 NPC 复核**，`#38`–`#41` 为 `comment_count=0`。按本仓防饥饿口径（**消费次数 asc, 编号 asc**）取零消费里编号最小的 **#38**（「推送时密钥扫描 R7」）；若严格按 prompt 的「priority → 编号」会第 10 次进 `#14`（它的待办已记在末条，下轮可接）。
- **无 NPC 分析 ⇒ 先发只读首扫**：评论 `2120990742129610752`（`2026-11-12T21:22:53Z`），限定「你是评审者、只读、不写收尾标记、未跑就标未跑、行号以 `f91dbf9` 为准」+ 8 条待核断言。`60 s × 5` 到 `21:27:53Z` 平台上仍只有我方一条 ⇒ 按 issue 正文 + 本机复算推进，**不重复发**。
- **形态**：功能/设计题，本体是 L2（新 crate 代码 + 可能的新错误变体）⇒ 产出 = 现状核实文档 + 设计文档 §5.2 R7 两处 1→1 订正 + 台账，**零代码行为改动**（`crates/` 一行未动）。
- **BASE** `f91dbf9`（`HEAD` 与 `cnb/main` 同 SHA，`ls-remote` tip 一致）。改前 `scripts/check.sh --quick` **119 s `EXIT=0`**（`Finished dev profile [unoptimized + debuginfo] target(s) in 1m 57s` / `==> checks passed in 119s (quick)`）；改后 **2 s `EXIT=0`**（`Finished … in 0.26s`；docs-only 热缓存）。
- **判定分布**：13 条 = **成立 5 / 部分成立 4 / 不成立 4**。全表见 `ironforge-docs/architecture/secret-scan-current-state-2026-11.md` §1。
- **最贵的一条（R38-1）**：设计把扫描器放在 `rg-core/src/secret_scan.rs`，但 **push 内调不到** —— `rg-git/Cargo.toml:9-25` 只依赖 tokio/anyhow/tracing/gix/gix-archive/gix-pack/thiserror，`rg-core/Cargo.toml:10-11` 才是反向依赖 `rg-db`/`rg-git`。扫描器必须落 `rg-git`，或由 `rg-http`/`rg-ssh` 以回调注入（同文件 `receive_pack.rs:18` 的 `BranchDeleter` 注释原文就是这条约束：「`rg-git` has no database, so the policy … lives with the caller」）。
- **第二处（R38-2）**：`RefUpdate`（`receive_pack.rs:32-38`）只有 `old_sha/new_sha/refname/status/message` ⇒ 结构化命中（path/rule/severity）**无处安放**；「默认记事件 + 警告」要么扩这个结构（`rg-http`/`rg-ssh` 两侧同改），要么走 `on_delete` 式回调。`record_repo_event` 还是 `rg-core/src/webhook/service.rs:227` 的**私有** fn，调用方只能走 `rg-db/src/ops/repo_event_ops.rs:8 append`。
- **第三处（插入点唯一且已存在模板）**：`enforce_signed_commit_policies` 的调用在 `receive_pack.rs:481`（`if has_content_updates` 块内），写 ref 的循环从 `:486` 起 —— **中间没有早退**，在这里设 `status="error"` + `message` 即由 `:680` 的 `ng {refname} {message}` 出到 report-status。模板就是 `:521` 那个函数（gix 原生、失败即 error、`old_sha=0000000` 的新分支在 `:562` 起处理）。
- **第四处（R38-4，fail-open）**：「单次 ≤200 文件，超限跳过」= 一次推 201 个文件**全不扫**；叠加「默认只警告」后是静默漏扫。已在设计文档就地标「超限口径待裁决」。
- **第五处（完成口径无对象）**：`repo_events.event_type` 是自由 `string_len(64)`（migration `m20261106_000004:57-60`，无枚举/CHECK）⇒ 加 `secret.detected` **不需要迁移** ⇒ 「新 SQL 至少跨后端跑一次（`IRONFORGE_TEST_DATABASE_URL`）」在本方案下没有对象；第一条「`clippy -D warnings`」已由 `check.sh --quick`（`scripts/check.sh:36-43`，`:45` 起是 quick 跳过的测试段）覆盖。
- **第六处（两份材料都没写）**：`collaboration/secret.rs:12-13` 的 `SECRET_PREFIX = "ifs_"`，注释原文「makes the secret recognisable to secret scanners and log filters」⇒ 规则集缺 `ifs_` 就是**本仓自漏**；反向也成立（仓库里出现 `ifs_` = 该凭据泄漏）。
- **本轮自查纠错（自曝，已写进回帖）**：首扫请求里我把「路径脱敏」判成 `auth/encryption.rs:58 mask_values`，**错** —— 那是**日志值脱敏**；issue 正文/设计文档指的「路径脱敏」是 `rg-http/src/redact.rs:90 redact_server_paths`（把**服务器文件系统路径**从错误体里抹掉），与密钥无关。`mask_values` 属**第四处**（两份材料都没列）。
- **新 L2**：**R38-1**（P1 扫描器位置）· **R38-2**（P1 警告模式落点 / `RefUpdate` 结构化）· **R38-3**（P2 REST 稳定 reason：`CoreError`（`rg-core/src/error.rs:24-44`）5 变体无 code，需加变体或 handler 映射到 `AppError::coded`（`rg-http/src/error.rs:231`）；`collab` 侧已有桥 `collaboration/error.rs:50-56` → `rg-http/src/error.rs:318`）· **R38-4**（P2 超限 fail-open）· **R38-5**（P2 多 ref 语义：一次 push 多 ref 时同一 commit 报几次）· **R38-6**（P3 成本：`:472` 已 `read_to_end` 整个 pack，扫描再加全量 tree 遍历 + blob 读取，上限 4 GiB 见 `lib.rs:50`）· **R38-7**（P3 `ifs_` 规则缺失）· **R38-8**（P3 完成口径无对象）。
- **改动（WT 内 commit）**：新建 `ironforge-docs/architecture/secret-scan-current-state-2026-11.md`（92 行：13 条判定 + 三个插入点 + 分层约束 + 限额 + L2 + 复核命令）；`agent-principals-design-and-roadmap-2026-11.md` 2 处 1→1（现状段补 4 入口与 `AppError::Coded` 真指针、方案段改扫描器位置 + `ifs_` + 超限待裁决），文件仍 **676 行** ⇒ 全仓 `path:line` 零漂移。
- **反向清单 95–98**（见下）。清单累计 **1..98**。
95. 「REST 提交只有 `create_or_update_file_with` → `commit_tree_edits` 一条链」（issue #38 正文 + 设计文档 §5.2 R7 现状段）—— **不成立**：`rg-core/src/repo/service.rs` 有 **4 处** `commit_tree_edits` 调用 —— `:1917`（宿主 `create_or_update_file_with:1799`）、`:2034`（`update_files_in_commit:1980`）、`:2161`（`delete_file_with:2095`，不产新 blob）、`:2574`（`commit_batch:2362`）；入口 handler 另有 `rg-http/src/api/repo_commits.rs:176`。反证：`grep -n 'commit_tree_edits' crates/rg-core/src/repo/service.rs`。**注**：本仓台账 #48 轮曾写 `repo_commits.rs:174`，真值 `:176`（`:174` 是 `commit_identity` 行）。
96. 「扫描器放 `rg-core/src/secret_scan.rs`」（设计文档 §5.2 方案段）—— **不成立**：`rg-git` 不依赖 `rg-core`/`rg-db`（`crates/rg-git/Cargo.toml:9-25`），push 路径在 `rg-git` 内，调不到 `rg-core` 的模块。反证：`sed -n '9,25p' crates/rg-git/Cargo.toml`、`sed -n '10,11p' crates/rg-core/Cargo.toml`。**已就地订正。**
97. 「完成口径要求新 SQL 至少跨后端跑一次」—— **不成立（无对象）**：`repo_events.event_type` 是自由 `string_len(64)`（`m20261106_000004:57-60`，无枚举、无 CHECK），`rg-db/src/ops/repo_event_ops.rs:8 append` 直接收 `&str` ⇒ `secret.detected` 不需迁移。反证：`grep -n 'EventType\|string_len(64)' crates/rg-db/src/migrations/m20261106_000004_create_repo_events.rs`。
98. 「单文件 ≤1 MiB / 单次 ≤200 文件有先例可对齐」—— **不成立**：现有限额全是 **body 级**（`rg-http/src/lib.rs:50` 4 GiB、`:528`/`:965` 10 GiB、`:619` 1 GiB、`:823-865` 101 MiB ×4），`grep -rn 'maxItems' crates/` = **0**，`commit_files`（`rg-mcp/src/tools/mod.rs:131`）无条数上限 ⇒ 单文件阈值是**新维度**。
- **靶状态**：issue #38 保持 open（本体是 R7 实现，单发轮只核实与登记 L2）。
- **下靶**：零消费剩 `#39`–`#41`（`comment_count=0`）；另有 5 条**未消费的 NPC 复核**（`#14`/`#15`/`#17`/`#18`/`#37`，最新评论均晚于我方收尾）。按（次数 asc, 编号 asc）下一靶 **#39**（「代码智能：结构化大纲与符号图 R9」）；若优先清复核则先 `#14`（它有 NPC 明确要求回填的一行，见 `2120620210477498368` 第 3 节）。
- **回填**：改动 SHA 与回帖 id 见本节末「改动 SHA / 回帖（回填）」段。

**改动 SHA / 回帖（回填）**：`bca21c5`（2 文件 `94+/2−`：新文档 92 行 + 设计文档 2 处各 1→1、仍 676 行；`crates/` 零行 ⇒ 全仓 `path:line` 零漂移）+ `4bbb82e`（台账 27 行 + 判定分布订正 1 行）。推 `cnb` 单 remote：`f91dbf9..bca21c5`、`bca21c5..4bbb82e`；`merge-base --is-ancestor` 两次均通过，主工作区 `merge --ff-only` 到 `4bbb82e`、`status --porcelain` 0 行。**改后验证**（executor 本机）：`scripts/check.sh --quick` `EXIT=0` **2 s**（`Finished dev profile [unoptimized + debuginfo] target(s) in 0.26s` / `==> checks passed in 2s (quick)`）；改前 **119 s** `EXIT=0`（`Finished … in 1m 57s`）。**复算环境无 `cargo` 时不可验。** 时刻锚 = `git log -1 --format=%aI 4bbb82e`。**回帖** `2120992194340249600`（`2026-11-12T21:28:39Z`）；回读首行 `## 本轮收尾`、次行 `protocol=v0.13 · prompt=ironforge-consumer-v4`、末行独立 `@CodeBuddy`、反向清单 95–98 独立成行；该 issue 的 `## 本轮收尾` 计数 **0 → 1**，无重复投递。**NPC 本轮零复核**（首扫请求 `2120990742129610752` 超时未回）⇒ 回帖已声明「`bca21c5`/`4bbb82e` 均未经复核」。

---

### 单发轮 · issue #14 · 2026-11-13（第 10 次进入：核实它钉 `1c0a87e` 的复核；最大产出是 R1 落地后本文档的现状漂移）

protocol=v0.13

- **选靶**：11 条 open **全 MANUAL**（`[opt-round]` = 0 ⇒ 第二段无对象）。`#48`（`20:19:41Z`）/`#38`（`21:28:39Z`）/`#19`（`04:20:16Z`）/`#37`（`10:24:43Z`）最新评论是我方 `## 本轮收尾` 或未到期 ⇒ 等待复核，跳过；`#39`–`#41` `comment_count=0`；`#14`/`#15`/`#17`/`#18` 最新评论是**未消费的 NPC 复核**。四条 `priority` 全空 ⇒ 同级编号最小取 **#14**（顺带清掉压了两天的复核链）。
- **输入**：`2120620210477498368`（`2026-11-11T20:50:32Z`），它自称钉 `1c0a87e` 复核；我方上一条收尾 `2120619579549306880`（`20:48:01Z`）在其之前 ⇒ 未消费。
- **判定分布**：13 条 = **成立 11 / 不成立 2**；另**我方自查成立 5**（S1–S5），其中 3 条是 R1 落地造成的过期。
- **基线漂移（协议第 6 步）**：`cnb/main` = `b79458b`，已远不是它钉的 `1c0a87e`。`git diff 1c0a87e..cnb/main -- <本批 2 文件>` **非空**——R1 三提交 `eb1736f`（phase 1）/ `4d34e69`（phase 2）/ `0b05502`（PR #55 合并）改了 `capabilities.rs`、`lib.rs`、`route_meta.rs` 与本文档 ⇒ 行号与「今天的兜底」全部重取（S1–S3）。

判定表（`:n` 未注明即指 `1c0a87e` 树；「现状」指 `b79458b`）：

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| 1 | 文档 `:58` 判据足够防复算；82 / 65 / 17 与行首 75 / 58 / 内联 7 逐项复现 | **成立** | `git show 1c0a87e:crates/rg-core/src/collaboration/capabilities.rs \| grep -c '^\s\+r('` = **75**；`routes: &[r(` = **7**；75 + 7 = **82**；我按 `r(...)` 括号解析去重 = **65 路径 / 17 个多命中族**（当前树重算同值）。它没算到 5 处 `x(`——那时还没有这个构造器 |
| 2 | 文档 `:72` 已与 §5.1 第 2 条同向、`/health` 降级为例外；`lib.rs:2083-2084` 早退 | **成立** | `git show 1c0a87e:crates/rg-http/src/lib.rs \| sed -n '2082,2085p'`：`:2082` 注释、`:2083` = `if path.starts_with("/api-docs") \|\| path == "/health" \|\| path == "/metrics" {`、`:2084` = `return None;` ⇒ 锚点逐字命中（它自报 `:2083-2084`，正确）。**现状已失效**，见 S2 |
| 3 | 文档 `:81`「只有 `/repos/**` 族可达四个拒绝模式」与 7 条 `/ai/repos` 路由吻合 | **成立** | `/ai/repos` = `summary`/`issues`/`prs`/`tree`/`search/code`/`index`/`pulls/{number}/context` 共 **7** 条，无 `fork`/`forks`/`transfer`/`collaboration`；`1c0a87e` 与 `b79458b` 同集合、同行区间 `:1765-1785` |
| 4 | 索引行 `1..60` 与实际一致：60 条、连续、无重号 | **成立** | `git show 1c0a87e:docs/reports/OPT-LOOP-LEDGER.md \| grep -c '^[0-9]\+\. 「'` = **60**；`uniq -d` 空 |
| 5 | 「见本节」错指针已真修掉（56–58 → 第 8 次、59–60 → 第 9 次） | **成立** | 现文两段去处分别指向 `第 8 次消费…` 与 `…（第 9 次消费…）` 节，两者物理都在索引行之下 |
| 6 | 反向 58 锚点 `capabilities.rs:785`/`:787`/`:788` 与实文一致 | **成立（现状漂 +22；第 11 次进入轮订正）** | 三个行号在 `1c0a87e` 逐字命中：`:785` = `routes: &[r(`、`:787` = 路径、`:788` = `CollabRead,`（`"GET"` 在 `:786`）；现状逐行对应 `:807` / `:809` / `:810`（`"GET"` `:808`）。漂移 **+22**：`913 → 935` 行，`git diff --numstat 1c0a87e c410fc5` = `27 5`（27 插入 / 5 删除，净 +22）——原写「+24」与「净增 27 行」两处都错 |
| 7 | 反向 60 成立：`:58` 判据列非空、58 是行首口径不是基数、`82 − 65 = 17` | **成立** | 三项逐条复算同值 |
| 8 | 「`:1844` 与 `:1859` 那两遍互相打架」 | **不成立** | 两处同向：都说可达的只有 `/repos/**` 那 4 个。`:1844` 说的是「它自己说 8 个」与「它上一条复核说 4 个」相矛盾，不是本文两处互斥 ⇒ 它把「被引对象的说法」读成了「本文的断言」 |
| 9 | 「台账里从来没有过独立的『上一轮结论』，`:1844` 与 `:1862` 都在引用一个不存在的对象」 | **不成立** | 对象是**第 8 次消费判定表第 14 行**：`git show 7982524:docs/reports/OPT-LOOP-LEDGER.md \| grep -c 'ai 族不可达'` = **1**，该行「它说的」列原文为「`/ai/repos` 下 7 条路由无 `fork`/`transfer` ⇒ 四个拒绝模式在 ai 族不可达」，我方判**成立**。它的判据 `grep -c '^59\. '` = **0 是必然的**——反向 59 是第 9 次消费轮才新增的编号，它拿「后加的编号」找「先前的行」 |
| 10 | `git diff --stat 2043fb3 1c0a87e` = 2 个 md、`47+/5−` | **成立** | 逐字命中（台账 46+/1−、设计文档 6±，合计 `47 insertions(+), 5 deletions(-)`） |
| 11 | `1c0a87e` author date `2026-11-12T04:47:20+08:00` = `2026-11-11T20:47:20Z` | **成立** | `git log -1 --format='%h %aI %an' 1c0a87e` 逐字命中 |
| 12 | 顺带口径：`api/ai_pr_context.rs:195` 有同名 utoipa 路径 | **成立** | `sed -n '195p'` = `path = "/ai/repos/{owner}/{name}/pulls/{number}/context",` |
| 13 | 顺带口径：`rg-mcp/src/tools/mod.rs:874` 也有同名路径 | **成立（现状漂 +118）** | `1c0a87e` 树 `grep -n '/ai/repos'` = `:874`；现状 `:992` |

**我方自查（成立 5；S1–S3 是 R1 落地造成的过期，本轮就地订正）**：

| # | 自查项 | 判定 | 证据与处置 |
|---|---|---|---|
| S1 | §1 表第 1 行「`required_pat_scope` / `repo_scope_for` = `lib.rs:2076` / `:2137`」 | **成立（已过期）** | 现状 `:2079` 只剩 `required_pat_scope`，函数体已改成「归一化 + 委托 `route_meta::pat_scope_for`」（`lib.rs:2079-2088`）；`repo_scope_for` **已删除**。已加文件头状态行，未改表（表头写的是基线 `75d8231`） |
| S2 | §4 的兜底锚点「`lib.rs:2127` 兜底是 `admin`」「`:2083-2084` 先 `return None`」 | **成立（已过期）** | 前缀表整段迁到 `route_meta.rs`：`family_scope` `:704`，早退 `:706`，末行兜底 `Some("admin")` `:750`（另有 `:713`/`:746` 两处 admin 分支）。已就地订正为 `route_meta.rs:706` / `:750`（1 行 → 1 行） |
| S3 | §3 判据「行首 `r(` 只得 **75** / 58，差 7 处内联」 | **成立（已过期）** | R1 把 5 条跨仓路由从 `r(` 改成 `x(`（`capabilities.rs:559`/`:564`/`:569`/`:574`/`:579`，`Route` 新增 `other_repos` 字段，`:117-123`）⇒ 行首 `r(` 现 **70**、行首去重 **54**；`82 = 70 行首 + 7 内联 + 5 个 `x(``（三条口径去重 54/7/4 = 65）。已就地订正（1 行 → 1 行） |
| S4 | 台账索引行的累计值 | **成立（真错）** | 索引行原写「已累计 **1..83**」，而 `grep -c '^[0-9]\+\. 「'` = **98**、最大编号 98 ⇒ 84–98（#48 三次进入 + #38 第 1 次进入写入）从未进去处表。已补齐 84–89 / 90–92 / 93–94 / 95–98 四段去处 |
| S5 | R14-4（两表族集不一致）在 R1 后是否还成立 | **成立，但机制变了** | `path_in_allowlist` 末尾改为委托 `route_meta::reaches_other_repos`（`route_meta.rs:662-681`），该函数认 `["repos", …]` **与** `["ai", "repos", …]` 两族 ⇒ 原来的「族集不一致」消失；但 `/ai/repos/**` 7 条在 `route_meta.rs:442-448` 声明为 `repo(...)`，`declare` 的默认是 `branch_limit: BranchLimit::Refused`（`route_meta.rs:130`）⇒ 带 `ref_allowlist` 的 token 对 `/ai/repos/{owner}/{name}/index` **仍恒拒**。仍 fail-closed、仍录 L2（R14-4 状态更新，不关闭） |

**新 L2**：**R14-4 状态更新**（P3，机制从「两张手写表族集不一致」变为「声明默认 `Refused`」——要放行得给该路由显式 `.checked()` 并让 handler 真调 `ensure_token_may_write_branch`，属改 PAT 拒绝面）· **R14-5**（P3，本文档 §5 的分阶段表算半张历史：`pat_scope`/`other_repos`/`branch_limit`/`idempotency` 四字段已全部上 `RouteMeta`（`route_meta.rs:117-123`）= P0/P1/P2 落地；**P3（全站 241 条登记）未做**——`ROUTES` 覆盖 collaboration 全量 + contents/commits/branches/signature/statuses/ai 等写路由，仍不是 `lib.rs` 的 323）· **R14-6**（P3，`capabilities.rs` 行号自 R1 起整体漂移，旧区间一律重取）· **R14-7**（P3，`rg-mcp/src/tools/mod.rs` 的 `/ai/repos` 字面量 `:874` → `:992`，非 R1 所致）· **R14-8**（P3，`crates/` 的 `.route(` 已 342 → **346**：新增的 4 处全在 `rg-http/tests/route_metadata_tests.rs`；`lib.rs` 仍 323 ⇒ §0 的「新增 241 条」不变）。

**本轮改动**（2 文件，`crates/` **零行** ⇒ 全仓 `path:line` 零漂移）：

1. `ironforge-docs/architecture/route-metadata-registry-2026-11.md`（**108 → 108 行**，3 处 1 行 → 1 行）：文件头状态行（R1 已落地 + 新基线与「§2③/§4/§5/§5.1 是历史快照」）；§3「不同路径数」行补 R1 后的三口径分解（70 行首 / 7 内联 / 5 `x(`，去重 54/7/4）；§4 兜底锚点改指 `route_meta.rs:706`/`:750`。
2. `docs/reports/OPT-LOOP-LEDGER.md`：第 9 次消费判定表第 6 行与「对方误差 ②」两处悬空指控改为点名「第 8 次消费判定表第 14 行」（各 1 行 → 1 行）；索引行累计值 1..83 → 1..102 并补 84–98 / 99–102 去处；本节。

**未做**：§5 分阶段表整表改写（P0–P2 已落地这件事只在状态行点明，不在表内逐行改「是否做」列——那张表的列头写死基线，整表改写属于文档重构，留 R14-5）；未动 `Route` 结构、未动任何 PAT 拒绝面、未改任何路由行为。

**反向清单（本节新增 4 条，累计 1..102）**：

99. 「`:1844` 与 `:1859` 那两遍互相打架」—— **不成立**：两处同向（可达的只有 `/repos/**` 那 4 个）。把「被引对象（它上一条复核）的说法」当成「本文的断言」来判互斥。**已核实，勿再提。**
100. 「台账里从来没有过独立的『上一轮结论』，`:1844` 与 `:1862` 都在引用一个不存在的对象」—— **不成立**：对象是第 8 次消费判定表第 **14** 行（判成立），原文「`/ai/repos` 下 7 条路由无 `fork`/`transfer` ⇒ 四个拒绝模式在 ai 族不可达」。判据 `grep -c '^59\. '` = 0 是拿后加编号找先行行，**不是反证**。
101. 「§3 判据：行首 `grep -c '^\s\+r('` 只得 75 条路由 / 58 个路径，差 7 处内联形」（我方 2026-11-12 写入）—— **不成立（R1 后过期）**：真值 **70 / 54**；82 = 70 行首 + 7 内联 + **5 处 `x(`**（`capabilities.rs:559`/`:564`/`:569`/`:574`/`:579`）。**已就地订正，勿再引 75/58。**
102. 「台账索引行累计 1..83」—— **不成立（我方真错）**：实有 **1..98**（现 1..102），84–98 从未进去处表。已补齐。

**验证**：`BASE=b79458b`（`HEAD` = `cnb/main` = `ls-remote` tip，`status --porcelain` 0 行）。改前 `scripts/check.sh --quick` **`EXIT=0`**（executor 本机 `Darwin arm64` / `cargo 1.99.1` / `2026-11-12T22:30Z` 起 **124 s**，``Finished `dev` profile [unoptimized + debuginfo] target(s) in 2m 02s`` / `==> checks passed in 124s (quick)`）。改后同命令 `EXIT=0` **1 s**（``Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.24s`` / `==> checks passed in 1s (quick)`）。**复算环境无 `cargo` 时不可验。**

**改动 SHA / 回帖（回填）**：`18909d0`（2 文件 `65+/6−`；`crates/` 零行）。推 `cnb` 单 remote：`b79458b..18909d0`（`HEAD -> main`）；`merge-base --is-ancestor HEAD cnb/main` 通过，主工作区 `merge --ff-only` 到 `18909d0`、`status --porcelain` 0 行。时刻锚 = `git log -1 --format=%aI 18909d0`。**回帖** `2121009217485090816`（`2026-11-12T22:36:18Z`）；回读首行 `## 本轮收尾`、次行 `protocol=v0.13 · prompt=ironforge-consumer-v4`、末段独立 `@CodeBuddy`、反向 99–102 独立成行；该 issue 的 `## 本轮收尾` 计数 **3 → 4**，无重复投递。issue **保持 open**（本体是 R14-1 / R14-4 等 L2，单发轮只核实与登记）。

### 单发轮 · issue #14 · 2026-11-13（第 11 次进入：核实它钉 `18909d0` 的复核；唯一不成立是它自己漏掉的 +5）

protocol=v0.13

- **选靶**：11 条 open **全 MANUAL**（`[opt-round]` = 0 ⇒ 第二段无对象）。`#48`（`20:19:41Z`）/`#38`（`21:28:39Z`）/`#19`（`04:20:16Z`）最新评论是我方 `## 本轮收尾` ⇒ 等待复核，跳过；`#39`–`#41` `comment_count=0`；`#14`/`#15`/`#17`/`#18`/`#37` 最新评论是**未消费的 NPC 复核**。五者 `priority` 全空 ⇒ 同级编号最小取 **#14**：它这条 `2121014784890953728`（`22:58:25Z`）出现在我方上一条收尾 `2121009217485090816`（`22:36:18Z`）之后 22 分钟，是当前唯一钉着我方最新提交（`18909d0`）的复核。
- **判定分布**：14 条 = **成立 12 / 部分成立 1 / 不成立 1**（A 组它对我上轮 3 处改动的判定、B 组台账 3 处点名、C 组它新提的 2 处事实错、D 组 §四 6 处抽查）；它的反向清单 103/104/105 与 A2/C1/C2 同源，单列在末段。
- **BASE** `c410fc5`（`HEAD` = `cnb/main` = `ls-remote` tip，零漂移）。改前 `scripts/check.sh --quick` **87 s `EXIT=0`**（`Finished dev profile [unoptimized + debuginfo] target(s) in 1m 25s` / `==> checks passed in 87s (quick)`）。
- **唯一不成立（= 它的反向 103）**：它说文档 `:58` 的「82 = 70 行首 `r(` + 7 内联 + 5 个 `x(`」**算术不闭合**（原话「实际 70 + 7 = 77 ≠ 82」）——**它把 +5 丢了**。70 + 7 + 5 = **82**；`1c0a87e` 那套 75 + 7 + 0 也 = 82。复算（括号解析 + `grep -c`）：现状 行首 `r(` **70** / `routes: &[r(` 内联 **7** / 行首 `x(` **5** ⇒ 82 条路由；按 `path` 去重 **65**；多命中族 **17**；三口径路径集合 **54 / 7 / 4**，两两交集为空、并集 65。**该行数值全对**，本轮只把并列读法改写成两条等式（1 行 → 1 行）。
- **成立且已修**（它的 104 + 105 实体部分）：
  - **104**：台账「现状漂 **+24**」错，真值 **+22**（`capabilities.rs` `913 → 935`；`git diff --numstat 1c0a87e c410fc5` = `27 5`）；原句「净增 27 行」拿的是**插入数**；且三个锚点映射错位（`1c0a87e`：`:785` = `routes: &[r(`、`:787` = 路径、`:788` = `CollabRead,`，`"GET"` 在 `:786`，我原写的「现状」三元组把 `"GET"` 混进去了）⇒ 已改为 `:807` / `:809` / `:810`（`"GET"` `:808`）+ 漂移 +22。
  - **105（实体部分）**：§1 表（`:16`）、§3 表（`:63`）、§2③（`:48`）**仍指向已删除的 `repo_scope_for` 与 `lib.rs:2127`**。`grep -rn 'fn repo_scope_for' crates/` = **0**（全仓唯一命中是 `rg-http/tests/route_metadata_tests.rs:41` 的一句注释）；`lib.rs:2127` 现在是 `pat_auth_middleware` 里的 `Ok(None) => {}`，兜底 `Some("admin")` 在 `route_meta.rs:750`、早退在 `:706`。三处各加半句（各 1 行 → 1 行）。
  - **但 105 的归因错**：它说「自查 S1 声称『已就地订正』」——S1 处置列原文是「已加文件头状态行，**未改表**（表头写的是基线 `75d8231`）」；只有 S2/S3 写了「已就地订正」，且两者属实（`route_meta.rs:706`/`:750`、70/54 三口径）。⇒ 105 判 **部分成立**。
- **它 §四 的 6 条抽查全部成立**（逐条复算）：`1c0a87e` 树 82 路由 / 65 路径 / 17 多命中族、行首 75 / 内联 7 / 行首去重 58；`lib.rs:2082` 是注释、`:2083` = `if path.starts_with("/api-docs") || …`、`:2084` = `return None;`；`/ai/repos` 7 条且 `:1765-1785` 逐条命中、无 `fork`/`forks`/`transfer`；它同意我判它第 8/9 条「不成立」；`ai_pr_context.rs:195`、`rg-mcp/src/tools/mod.rs` `:874 → :992`（+118）同值。
- **台账三处点名全部成立**：`:1846` 含「该节判定表第 **14** 行」；`:1864` 含评论 id `2120589836842086400` + 「第 8 次消费判定表第 14 行」；索引行 `:948` 标 `1..102`，`grep -c '^[0-9]\+\. 「'` = **102**、`seq 1 102` 无缺无重，小节起点 `2263`/`2296`/`2317`/`2339`/`2370` 与它报的逐行相同。
- **新 L2**：**R14-9**（P3，本文档 §1/§2/§3 的**其余**行号同样在漂：`repo_content.rs:1451`/`:1591` → 现状 `:1453`/`:1596`；`pat_scope.rs:174`/`:299` → 现状 `:175`/`:307`；`lib.rs:2281` 已不是 `is_read` 所在行 ⇒ 需一次「整表重钉基线」，不是逐处打补丁；本轮只修「指向已删除符号」的 3 处）· **R14-10**（P3，`capabilities.rs:27`/`:28` 两条路由用 `const SESSIONS` 传 path ⇒ 任何「按字面量数路径」的脚本都会少 1 个路径、少 1 个多命中族；本轮 80/64/16 就是被它扣掉的，加回才是 82/65/17）。
- **未做**：§1/§2/§3 整表重钉基线（R14-9 已登记）；未动 `Route` 结构、未动任何 PAT 拒绝面、未改任何路由行为；`crates/` **零行**改动。
- **复核请求（限定文件）**：`ironforge-docs/architecture/route-metadata-registry-2026-11.md:3` / `:16` / `:48` / `:58` / `:63` 与 `docs/reports/OPT-LOOP-LEDGER.md` 本节 + `:948` 索引行。

**反向清单（本节新增 1 条，累计 1..103）**：

103. 「文档 `:58` 的『82 = 70 行首 `r(` + 7 内联 + 5 个 `x(`』算术不闭合，实际 70 + 7 = 77 ≠ 82」—— **不成立**：70 + 7 + 5 = **82**，作者把 +5 丢了；该行两套并列读法（`1c0a87e` 75 + 7 + 0、现状 70 + 7 + 5）都等于 82。复算：`grep -c '^\s\+r('` = 70、`routes: &\[r(` = 7、`^\s\+x(` = 5；括号解析 82 路由 / 65 路径 / 17 多命中族；三口径 54 / 7 / 4 两两交集为空、并集 65。**已核实，勿再提。**

**改动 SHA / 回帖（回填）**：`a1f23a5`（2 文件 `29+/7−`；`crates/` 零行 ⇒ 全仓 `path:line` 零漂移）。推 `cnb` 单 remote：`c410fc5..a1f23a5`；`merge-base --is-ancestor HEAD cnb/main` 通过，主工作区 `merge --ff-only` 到 `a1f23a5`、`status --porcelain` 0 行。时刻锚 = `git log -1 --format=%aI a1f23a5`。**改后验证**（executor 本机 `Darwin arm64` / `cargo 1.99.1`）：`scripts/check.sh --quick` `EXIT=0` **1 s**（`Finished dev profile [unoptimized + debuginfo] target(s) in 0.25s` / `==> checks passed in 1s (quick)`）；改前 **87 s** `EXIT=0`。**复算环境无 `cargo` 时不可验。** **回帖** `2121026108907782144`（`2026-11-12T23:43:25Z`）；回读首行 `## 本轮收尾`、次行 `protocol=v0.13 · prompt=ironforge-consumer-v4`、末行独立 `@CodeBuddy`、判定表 14 行、反向 99–103 独立成行；该 issue 的 `## 本轮收尾` 计数 **4 → 5**，无重复投递。**`a1f23a5` 尚未被 NPC 复核**（它上一条钉 `18909d0`）。issue **保持 open**。

### 单发轮 · issue #15 · 2026-11-13（第 2 次进入：消费它钉 `ad4e1dd` 的复核；两条「订正」翻案，其中一条是我方上一版的错）

protocol=v0.13

- **选靶**：11 条 open **全 MANUAL**（`[opt-round]` = 0 ⇒ 第二段无对象）。`#14`（`23:43:25Z`）/`#19`（`04:20:16Z`）/`#38`（`21:28:39Z`）/`#48`（`20:19:41Z`）最新评论是我方 `## 本轮收尾` ⇒ 等待复核，跳过；`#39`–`#41` `comment_count=0`；`#15`/`#17`/`#18`/`#37` 最新评论是**未消费的 NPC 复核**。五者 `priority` 全空 ⇒ 同级编号最小取 **#15**：它这条 `2120654763199975424`（`2026-11-11T23:07:50Z`）在我方收尾 `2120654435842945024`（`23:06:32Z`）之后 **1 分 18 秒**。
- **判定分布**：27 条 = **成立 22 / 部分成立 1 / 不成立 4**；另有**我方自查真错 3 处**（本文两处行号偏移 + 一处括注），均已就地订正。
- **BASE** `6396b65`（`HEAD` = `cnb/main` = `ls-remote` tip，`status --porcelain` 0 行）。改前 `scripts/check.sh --quick` **109 s `EXIT=0`**（``Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 47s`` / `==> checks passed in 109s (quick)`）——**这是 WT 内首次编译，与「热缓存 1~2 s」不是同一口径**。
- **两条翻案**：
  - **16 是对的，它算错了**（它的 5.1 说「应为 15」）：它自己的枚举就是 `webhook/service.rs` 13 个 + `lib.rs` 的 `"push"` + collab 2 个，**13 + 1 + 2 = 16**；写成 15 是加法掉了 1。复算：`grep -rhno 'trigger_event(db, repo_id, "[a-z_.]*"' crates/ | sed 's/.*"\(.*\)"/\1/' | sort -u | wc -l` = **14**（含 `push`），再 +2 collab = 16；`grep -c 'trigger_event(db, repo_id,' crates/rg-core/src/webhook/service.rs` = **13**。**与 #14 那轮的 +5 同形态：它给算术反驳时，先把它那行式子自己加一遍。**
  - **大小写不是缺陷，它的因果是反的**（它的 5.2）：SQLite 的 `LIKE` 对 ASCII 默认**不**区分大小写（`SELECT 'Push' LIKE '%push%'` = **1**，本机 `sqlite3 3.53.1` 实测），SeaORM 不加 `LOWER()` 也不改变这一点 ⇒「订阅串写 `Push`/`Issues` 静默不命中」不成立；`webhook/service.rs` 那条 `e.g.` 注释（真值 `:29`，不是它说的 `:28`）里的「case-insensitive LIKE substring」**是对的**。真正在**投递名一侧**的通配符风险是 `_`：16 个名字里 3 个 `pull_request.*`。
- **我方真错（已修，3 处）**：① 本文上一版把 `Column::Events.contains` 记成 `webhook_ops.rs:44`、把 `Active.eq(true)` 记成 `:43` —— 真值 `:42` Active / `:43` contains / `:44` `.all(db)`，**原始分析里的 `:43` 本来就是对的，是我改错了**；它对这一行打了「✅」等于把错的确认了一遍（计入不成立第 4 条）。② `CreateWebhookRequest.events` 的 `e.g.` 注释在 `:29`，不是 `:28`。③ `:118` 不是「`update_webhook` 的 `events` 组装」，那是 `:114-115`；`:118` 是 `updated_at: Set(Utc::now())`。
- **成立且已落稿（5 处）**：
  - **基线重钉**：`8d107d8 → 6396b65`，`rg-http/src/lib.rs` 在两者间**净减 53 行**（`34+/87−`）⇒ 4 处锚点重取（WS `:3440→:3386`、push webhook `:3504→:3463`、WS push `:3579→:3526`、R15-4 锚点同），`CLAUDE.md:185→:194`；其余被引文件 `git diff` 为空。
  - **§1.1 新增「两条路载荷不同源」**：`record_repo_event`（`:227`）把 `event` 截到 64 字符（`:234`）、payload 超过 `MAX_EVENT_PAYLOAD_BYTES`（`:225`，**16 KiB**）整条替换成 `{"truncated": true, "original_bytes": N}`（`:229-233`），而 webhook 侧仍投原文（`:149`）⇒「按事件日志重放 webhook」会丢长载荷（新 L2 **R15-9**）。
  - **§1.2 长轮询交付语义**改为「游标单调时才至少一次，游标不稳即至多一次」（采纳它的 §6）。
  - **§1.5 新增「同一个名字、两套 payload」**：事件日志/webhook 侧 `{ref, before, after, pushed_by, repository}`（`lib.rs:3451-3461`）vs WS 侧 `{repo, ref, commit}`（`:3530-3534`）⇒ 命名没分叉、schema 分叉，比 §2.2 的「三套命名各自实现」更硬（新 L2 **R15-8**）。
  - **§2.3 由「两处行号」改为「三处行号」**并自曝订正（`contains :44→:43`、注释 `:28→:29`、`:118` 括注）。
- **部分成立 1 条**：它的「498 分布前 10 个文件占 182（36%）」——前 **9** 个数字复算全中（`pipeline 37 / user 25 / org 25 / repo 19 / pull_request 18 / oci 18 / board 15 / import_task 13 / ci_environment 12`），和正好 **182 = 36.5%**；第 10 个 `collab_template` 报的 12 是 `pub async fn` **总数**，真值写死 **3** / 泛型 **9**（口径混用）。未写进文档。
- **新 L2**：**R15-8**（P2，`push` 同名两套 payload，统一信封的 `type` 语义须允许「同名不同 schema」或拆成两个投递名）· **R15-9**（P2，事件日志 16 KiB 截断 + 事件名 64 字符 ⇒ 长轮询不能当 webhook 重放源，是否在契约里声明待定）· **R15-10**（P3，`ironforge-docs/README.md` 写「六个待拍板决策点」，而账本已有 R15-7/R15-8/R15-9 未进表 ⇒ 条数口径三处不一致；本轮只在 §3 加一行说明，不改 README 与表）。
- **未做**：未把 498 的文件分布落成表（口径未统一）；未改 `webhook_ops` 匹配语义、未动任何 schema/契约；`crates/` **零行**改动。
- **复核请求（限定文件）**：`ironforge-docs/architecture/event-bus-current-state-2026-11.md` 的 `:5` / `:15` / `:22` / `:26` / `:42-43` / `:65-75` / `:84-88`，以及账本本节与索引行（「本清单已累计」那行）。

**反向清单（本节新增 3 条，累计 1..106）**：

104. 「§2.3 的『16 个投递名』查不实，应为 15」（NPC `2120654763199975424` §5.1）—— **不成立**：它的枚举本身是 `webhook/service.rs` 13 个 + `lib.rs` 的 `"push"` + `collab.question.asked`/`collab.task.blocked`，**13 + 1 + 2 = 16**，它把加法写成 15。复算：`grep -rhno 'trigger_event(db, repo_id, "[a-z_.]*"' crates/ | sort -u | wc -l` 口径唯一名字 **14**（含 `push`）、`webhook/service.rs` 内 `grep -c 'trigger_event(db, repo_id,'` = **13**，再 +2 = 16。**已核实，勿再提。**
105. 「`webhook_ops.rs:44` 的 LIKE 大小写敏感 ⇒ 订阅串写 `Push`/`Issues` 一律静默不命中；`webhook/service.rs:28` 注释的『case-insensitive』与实现相反」（同上 §5.2）—— **不成立（两段都错）**：SQLite `LIKE` 对 ASCII 默认**不**区分大小写，`SELECT 'Push' LIKE '%push%'` = **1**（`sqlite3 3.53.1` 实测）；注释真值在 **`:29`**（`:28` 是 `pub active: Option<bool>`），且表述本身正确。
106. 「`webhook_ops.rs:44` = `Column::Events.contains(event)`、`:43` = `Active.eq(true)`」（同上 §4 表格打 ✅；**本文上一版同错**）—— **不成立（双方同错）**：真值 `:42` = `.filter(Column::Active.eq(true))`、`:43` = `.filter(Column::Events.contains(event))`、`:44` = `.all(db)`；`8d107d8` 与 `6396b65` 两棵树逐行相同（`git diff` 空）。**原始分析里的 `:43` 本来就是对的，是我上一版改错了。**

**改动 SHA / 回帖（回填）**：`e65395b`（2 文件 `55+/16−`；`crates/` 零行 ⇒ 源码 `path:line` 零漂移）。推 `cnb` 单 remote：`6396b65..e65395b HEAD -> main`；`merge-base --is-ancestor HEAD cnb/main` 通过，主工作区 `merge --ff-only`、`status --porcelain` 0 行。**改后验证**（executor 本机 `Darwin arm64` / `cargo 1.99.1`）：`scripts/check.sh --quick` `EXIT=0` **1 s**（``Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.24s`` / `==> checks passed in 1s (quick)`）。**复算环境无 `cargo` 时不可验。** **回帖** `2121043581786841088`（`2026-11-13T00:52:51Z`）；回读首行 `## 本轮收尾`、次行 `protocol=v0.13 · prompt=ironforge-consumer-v4`、末段独立 `@CodeBuddy`、反向 104–106 独立成行；该 issue 的 `## 本轮收尾` 计数 **1 → 2**，无重复投递。issue **保持 open**（本体是 R15-1…R15-9 的 L2 决策）。

### 单发轮 · issue #17 · 2026-11-13（第 2 次进入：消费它钉 `0a9a7dd` 的复核；唯一实质反对成立 —— N19 翻案）

protocol=v0.13

- **选靶**：11 条 open **全 MANUAL**（`[opt-round]` = 0 ⇒ 第二段无对象，连续第 6 次）。`#14`（`23:43:25Z`）/`#15`（`00:52:51Z`）/`#19`（`04:20:16Z`）/`#38`（`21:28:39Z`）/`#48`（`20:19:41Z`）最新评论是我方 `## 本轮收尾` ⇒ 等待复核，跳过；`#39`–`#41` `comment_count=0`；`#17`/`#18`/`#37` 最新评论是**未消费的 NPC 复核**。`priority` 只有 `#48`=P2（它正等待）⇒ 其余全空，按「priority 空 → 编号最小」取 **#17**。
- **输入**：`2120698622797856768`（`2026-11-12T02:02:07Z`），它自报钉 `0a9a7dd`。我方上一条收尾 `2120698283860324352`（`02:00:46Z`）在其前 **1 分 21 秒** ⇒ 未消费。
- **判定分布**：26 条 = **成立 22 / 部分成立 4 / 不成立 0**（**本节原文误记 23 / 3 / 0，已于第 3 次进入轮按表复算订正**：部分成立的四条是 A7 / A13 / A19 / A21）；另**我方自查真错 3 处**（S1–S3）、未修 1 处（S4）、漂移登记 1 处（S5）。
- **BASE** `649a814`（`HEAD` = `cnb/main` = `ls-remote` tip，`status --porcelain` 0 行）。改前 `scripts/check.sh --quick` **109 s `EXIT=0`**（``Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 47s`` / `==> checks passed in 109s (quick)`）——WT 内首次编译口径。
- **合流判定（协议第 8 步 A 支）**：复核目标 `0a9a7dd` **已是 `cnb/main` 祖先**（`merge-base --is-ancestor` 退出 0），main 在它之后又走了 5 个文档提交（`e65395b`/`6396b65`/…/`649a814`）⇒ **不推旧提交**，在当前 `cnb/main` 上独立核验后另提新 commit。

判定表（26 条，按它的小节；`path:line` 未标「基线」者为现状值）：

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| A1 | `service.rs:426` 那句 fail-open 描述准确、不用改；`:467-469` 三层 `if` 包住全部判定，三层之外只有 `:538 Ok(())`；`Err` 分支无日志（`:534-535` 是两个闭括号） | **成立** | 逐行打印全中：`:426` 就是我方上一轮写的注释行、`:467 if protection.require_status_check`、`:468 if let Some(checks_json)`、`:469 if let Ok(...)`、`:534`/`:535` 两个 `}`、`:538 Ok(())` |
| A2 | 精度补丁：`required_status_checks` 解析成功但 `head_sha` 为空时 `:477-484` 会 `forbidden("no CI pipeline found")` | **成立** | `:477` `let head_sha = match pr.and_then(\|p\| p.head_sha)`、`:480-483` `return Err(CoreError::forbidden(format!("branch '{}' requires status checks but no CI pipeline found for PR {}", …)))`。全文比它引的多几个词，结论一致 |
| A3 | `grep -rn 'ResolutionFailure' crates/` = 0 命中，所以 §3 它核不了 | **成立** | `grep -rn 'ResolutionFailure' crates/ \| wc -l` = **0**；该符号只活在文档与 `~/.cargo/registry/.../gix-merge-0.23.0` |
| A4 | `add_add` 的产出来源整篇没交代（`mode` 已有半句） | **成立** | 原文只写「add/add 无独立变体」，没写它从哪来。本轮已补：普通 add/add 走 `Resolution::Conflict`（base 位 `None`），两个 `AddAdd` 变体分别被限定在子模块（`tree/mod.rs:371-373`）与类型冲突（`:397-403`） |
| A5 | `per_section_live_read`（`ai_pr_context.rs:387`）是 `/context` 的 `consistency` 字段值，拿它当端点命名判据是把两个词表硬接 | **成立（我方真错）** | `ai_pr_context.rs:386-387`：`:386` 是 `not_evaluated`、`:387` 是 `"consistency": "per_section_live_read"`。我方上一轮回帖第 85 行确实写「请就『与 `/context` 既有词表 `per_section_live_read` 一致』这个口径给出你的选择」⇒ 判据本身站不住，已在台账自曝 |
| A6 | 端点应按**载荷结构**选 `/merge-readiness`（`mergeability` 是 bool 味名词，承载不了 gates+机器码+actions） | **成立（建议）** | 纯设计论证，无可核实对象；与本文档 §6「两者都未实现可自由选、但不能都写进文档」同向 |
| A7 | 若选 `/merge-readiness`，`agent-principals-design-and-roadmap-2026-11.md` §R5 的 `/mergeability` 必须一起改或标废弃 | **部分成立** | 该段末尾（`:518`）**已**写明「端点与字段以本文件 §6 的裁决为准（本段的 `/mergeability` 与之冲突）」，但紧接着的 `:520` 仍写 `**方案**：`GET /repos/{o}/{r}/pulls/{n}/mergeability`` ⇒ 有标注、无废弃。已写进 R17-2 验收条件 |
| A8 | R17-1 应拆成 a（可观测性）/ b（fail closed），直接 fail closed 会拒掉一批现在能合的仓库 | **成立** | 拆法合理且与我方 §4 第 2 条同向；已落成 R17-1a / R17-1b 两行 |
| A9 | 第一步是在 `:534-535` 两个闭括号前各加一条 `tracing::warn!`，零行为改动 | **成立** | 位置精确（`:534`/`:535` 是三层 `if` 的两个收尾 `}`），且 rg-core 已依赖 `tracing` |
| A10 | N21 的 `spawn_blocking` 真实行号是 `:415` 与 `:463`，`:441` 落在 `finish_entry` 的 match 分支 | **成立** | `merge_queue.rs:415 tokio::task::spawn_blocking(move \|\| {`、`:463 let group_sha = tokio::task::spawn_blocking({`、`:441 "merge-group pipeline #{} is {}",` |
| A11 | 文件真路径是 `crates/rg-core/src/pull_request/merge_queue.rs`，读者照 `merge_queue.rs` 找不到 | **成立** | 该文件确在 `crates/rg-core/src/pull_request/`；本文档 §1 与 N21 都只写了文件名 |
| A12 | N27 的 `pat_scope.rs:301` 是 fn、`:299` 是 doc 段 | **成立** | 基线树 `:301 pub fn ref_limited_write_allowed(path: &str) -> bool {`；现状 `:307`（`:305` 是 doc 行） |
| A13 | N28 那个 `not_evaluated` 是 **4 项**不是 1 项，`webhook`/`ci_trigger`/`notification` 三项要不要跟进文档没写 | **部分成立** | 4 项为真（`pulls.rs:630 = "not_evaluated": ["conflicts", "webhook", "ci_trigger", "notification"],`）；但它说的位置是「§1 表」，实际本文件 §1 没有 `not_evaluated` 行，记录在 §2 的 N28。已补写 4 项口径进 N28 |
| A14 | N30：`:841` 是注释、`:842` 是 `.route(`、`:843` 才是路径 | **成立** | `:841 // Pull Requests`、`:842 .route(`、`:843 "/repos/{owner}/{name}/pulls",` |
| A15 | N26：`spawn_post_push_hooks` 在基线 `:3202`、`pub(crate)`；现有 2 个调用点全在 `rg-http`（`repo_content.rs:1382`、`lib.rs:3163`），core 零调用 | **成立（行号均按 `0a9a7dd` 命中；现状漂移见 S5）** | 基线 `:3202 pub(crate) fn spawn_post_push_hooks(`、`:3163 spawn_post_push_hooks(&state, &owner, &repo, repo_path.clone(), ref_updates);`、`api/repo_content.rs:1382 crate::spawn_post_push_hooks(state, owner, repo, repo_path, vec![update]);`；`grep -rn spawn_post_push_hooks crates/` 现状 **3** 命中 = 1 定义（`:3141`）+ 1 调用（`:3095`）+ 1 调用（`repo_content.rs:1384`，该行行首就是 `crate::` 前缀，**不是第四条**；原文记「4 命中」是重复计一次） |
| A16 | N23：`ops.rs:145`/`:426`/`:1591` 三处 `PreviousValue` | **成立** | `:145 PreviousValue::MustExistAndMatch(...)`、`:426 PreviousValue::ExistingMustMatch(gix::refs::Target::Object(expected_old))`、`:1591 Some(tip) => PreviousValue::ExistingMustMatch(...)` |
| A17 | **必须反对的一条**：N19 / §8 第 2 条 —— blame **有** 1 MiB 上限，`MAX_BLAME_BYTES` 在 `blame.rs:16`、`:90` 在用 | **成立（我方真错，已翻案）** | `crates/rg-git/src/blame.rs:16 pub const MAX_BLAME_BYTES: usize = 1024 * 1024;`、`:90 if blob.data.len() > MAX_BLAME_BYTES {`、`:91-94` 返 `TooLarge`、`:14-15` 文档「Files larger than this are refused」。`git show 0a9a7dd:crates/rg-git/src/blame.rs` 逐行相同；`bee3c14`（2026-10-21）引入，早于 `0a9a7dd` ⇒ 不是新代码 |
| A18 | 连带影响：§2 N19 行、§8 第 2 条、反向清单 67 全要改；33 条分布变 21 / 10 / 2 | **成立** | 三处已改（§8 改「两条」+ 翻案段、反向 67 标作废、分布改 21/10/2）；算术复算 20+1=21、3−1=2 |
| A19 | 它复跑 `grep '1024 * 1024'` 得 **19** 处命中 | **部分成立** | 真值 **20** 行：`git grep -n '1024 \* 1024' -- crates/` 在 `0a9a7dd` 与 `649a814` 两棵树都是 20。它的结论（第一屏就有 `blame.rs:16`）不受影响 |
| A20 | N1：`return Err` 在 `:1967-1973`，两个调用点 `:1896`/`:1927` 写成 `_conflicts` | **成立** | `:1896 let (merged_tree_id, _conflicts) =`（`:1897` 才是函数调用）、`:1927` 同形；本文档原写 `:1897`/`:1928` 是调用行，它给的绑定行更准 |
| A21 | N7：`pr_review_ops.rs:57`、每 reviewer 最后一条 `:63-67`、`commit_id` 双向匹配 `:73-76`、human-only `:84-86` | **部分成立** | `:57 pub async fn count_current_human_approvals(` ✓、`:73-76` match 双向 ✓、`:84-86` 三行 `user::Entity::find()` + `KIND_HUMAN` ✓；但 `:63-67` 少了 `for` 循环的收尾 `}`（真值 `:63-68`） |
| A22 | N22：`error.rs:266` + `pulls.rs:599-605` 传 `current_sha`/`expected_sha` | **成立** | `:266 pub fn conflict_because(`；`pulls.rs:599-605` 是 `AppError::conflict_because("REF_ADVANCED", true, …, json!({"current_sha":…, "expected_sha":…}), …)` |
| A23 | N29：`error.rs:170` `CoreError::Conflict(msg) => Self::Conflict(msg)` | **成立** | 逐字命中（`:166` 是 `fn from`、`:167` 是 `match e`） |
| A24 | N33：合并路径走 `merge_commits`（`service.rs:1963`）、`merge_trees` 只在 `ops.rs:351` 的 rebase 循环里 | **成立** | `:1963 .merge_commits(our_commit, their_commit, labels, options)`；`rg-git/src/ops.rs:351 .merge_trees(` |
| A25 | N31：`collab_contract_tests.rs:79` 的 `if !path.contains("collaboration") { continue; }` | **成立** | `:79 if !path.contains("collaboration") {`、`:80 continue;` |
| A26 | 「`check.sh --quick` 含 fmt」：`scripts/check.sh:36-37` 在 `case` 之后、`quick` 分支之前 | **成立** | `:15-27` 是 `case`，`:36 step "cargo fmt --all --check"`、`:37 cargo fmt --all --check`、`:44 if [ "$mode" != "quick" ]` |

**我方自查（S1–S5）**：

| # | 自查项 | 判定 | 处置 |
|---|---|---|---|
| S1 | N19 判「不成立」 | **成立（真错）** | 已翻案为成立；`blame.rs:16` 一直在，是我漏读 `grep` 结果。文档 §2 / §8 / 分布三处 + 台账本节 N19 行 + 反向 67 全部就地改 |
| S2 | N26 括注「`:3190` 是上一个函数的 `}`」 | **成立（真错，基线树即假）** | `git show 0a9a7dd:crates/rg-http/src/lib.rs \| awk 'NR==3190'` = `StatusCode::GATEWAY_TIMEOUT,`（`:3189` 起的元组内）；上一函数真正在 `:3196` 收 `}`、`:3198` 起是 doc。已改为 `:3196` / `:3198`（现状 `:3135` / `:3137`） |
| S3 | 上一轮回帖第 85 行把 `per_section_live_read` 当端点命名判据 | **成立（真错）** | 同 A5；该口径作废，端点裁决改按载荷结构（A6）与 §6 |
| S4 | `ironforge-docs/README.md` 索引行仍写「20 成立 / 10 部分 / 3 不成立」 | **成立（未修）** | 本轮改动文件数已到上限（文档 + 台账 = 2），已写进文档 §9「同轮未修」，下轮 L1 订正 |
| S5 | 锚点漂移：`lib.rs` 4045 → 3992 行、`pat_scope.rs` +6 | **成立（已登记）** | `:3202`→`:3141`、`:3163`→`:3095`、`repo_content.rs:1382`→`:1384`、`pat_scope.rs:301`→`:307`；已写进文档头部「现状漂移」行 |

- **新 L2**：**R17-8**（P3，`pulls.rs:630` 的 `not_evaluated` 共 4 项，readiness 若继承得先定 `webhook`/`ci_trigger`/`notification` 三项要不要跟进——它会决定 readiness 自身 `not_evaluated` 的长度）· **R17-9**（P3，`ironforge-docs/README.md` 索引计数与本文档 §2 分布已分叉，需一次同步，别只改一边）· **R17-1 → R17-1a / R17-1b**（P1，拆成「可观测性」与「行为变更 + 灰度」；`tracing::warn!` 落点 `service.rs:534-535`）· **R17-2 状态更新**（P1，补验收条件：选 `/merge-readiness` 则 §R5 的 `/mergeability` 必须改或标废弃；现状 `:518` 有标注、`:520` 无废弃）。
- **未做**：未改任何代码（`crates/` **零行**，未动 `Route`、未动合并行为、未动 PAT 拒绝面）；未实测冲突提取（`write_blob_to_odb` 落库结论仍来自 gix 源码，不是跑出来的）；未改 `agent-principals-design-and-roadmap-2026-11.md`（R17-2 的废弃动作属 L2 决策）；未修 S4 那行 README。
- **反向清单**：**本节无新增**（26 条里 0 条不成立）。累计仍 **1..106**；但 **67 已于本节作废**（翻案为成立），编号不回收、不重排。
- **复核请求（限定文件）**：`ironforge-docs/architecture/merge-readiness-current-state-2026-11.md` 的头部「现状漂移」行、§2 的 N19/N26/N27/N28 四行、§3 的两段源码取证、§7 的 R17-1a/R17-1b/R17-2/R17-7、§8 的「两条 + 已翻案」段；以及本台账本节与 67 号条目。请特别复算：`crates/rg-git/src/blame.rs:16`/`:90`、`crates/rg-http/src/lib.rs:3141`/`:3135`/`:3095`、`crates/rg-core/src/auth/pat_scope.rs:307`。
- **验证**：见本节末「改动 SHA / 回帖（回填）」。

**改动 SHA / 回帖（回填）**：`4059a68`（2 文件 `85+/15−`；`crates/` 零行 ⇒ 源码 `path:line` 零漂移）。推 `cnb` 单 remote：`649a814..4059a68  HEAD -> main`；`merge-base --is-ancestor HEAD cnb/main` 通过，主工作区 `merge --ff-only` 到 `4059a68`、`status --porcelain` 0 行。**改后验证**（executor 本机 `Darwin arm64` / `cargo 1.99.1`）：`scripts/check.sh --quick` `EXIT=0` **1 s**（``Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.24s`` / `==> checks passed in 1s (quick)`）；改前 **109 s** `EXIT=0`（``Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 47s``）。**复算环境无 `cargo` 时不可验。** 时刻锚 = `git log -1 --format=%aI 4059a68` = `2026-11-13T10:20:24+08:00`。**回帖** `2121065805633753088`（`2026-11-13T02:21:10Z`，8456 字符）；回读首行 `## 本轮收尾`、次行 `protocol=v0.13 · prompt=ironforge-consumer-v4`、末非空行独立 `@CodeBuddy`、判定表 A1–A26 连续无缺；该 issue 的 `## 本轮收尾` 计数 **1 → 2**，无重复投递。**`4059a68` 尚未被 NPC 复核**（它上一条钉 `0a9a7dd`）。issue **保持 open**（本体是 R17-1a/R17-1b/R17-2…R17-9 的 L2 决策）。



### 单发轮 · issue #17 · 2026-11-13（第 3 次进入：消费它钉 `4059a68` 的复核；E1 —— 我的判定分布与自己的表不符，同一句错在三处）

protocol=v0.13

- **选靶**：11 条 open **仍全 MANUAL**（`[opt-round]` = 0 ⇒ 第二段无对象，连续第 7 次）。`#14`（`23:43:25Z`）/`#15`（`00:52:51Z`）/`#19`（`04:20:16Z`）/`#38`（`21:28:39Z`）/`#48`（`20:19:41Z`）最新评论是我方 `## 本轮收尾` ⇒ 等待复核，跳过；`#39`–`#41` `comment_count=0`。有**未消费 NPC 复核**的是 `#17`（`02:38:24Z`，就在我方收尾后 17 分钟）/`#18`（`11-12T04:15:52Z`）/`#37`（`11-12T10:24:43Z`）。`priority` 只有 `#48`=P2（正等待）⇒ 其余全空，按「priority 空 → 编号最小」取 **#17**。
- **输入**：`2121070144716746752`（`2026-11-13T02:38:24Z`，4257 字符，首行 `@cnb.biewen(别问) 复核 `4059a68``）。它自报**本机无 toolchain**、`check.sh --quick` 的读数「只记录、无法复现」。
- **判定分布**：**30 条 = 成立 24 / 部分成立 3 / 子命题不成立 1 / 确认 2**（表行 C1–C6 / S1–S13 / E1–E6 / D1–D5 机械计数，见下）；另**我方真错 1 类 3 处**（E1）＋**判成立但没落稿 2 条**（E2 / E3）＋小订正 3 处（E4–E6）。
- **BASE** `35bbb4f`（`HEAD` = `cnb/main` = `ls-remote` tip，`status --porcelain` 0 行；它是 `4059a68` 的回填提交，`4059a68..35bbb4f` 只动台账）。改前 `scripts/check.sh --quick` **88 s `EXIT=0`**（``Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 26s`` / `==> checks passed in 88s (quick)`）—— WT 内首次编译口径。
- **合流判定（协议第 8 步 A 支）**：复核目标 `4059a68` 已是 `cnb/main` 祖先（`merge-base --is-ancestor` 退出 0），main 在其后只有 `35bbb4f` 一个回填提交 ⇒ 不推旧提交，在当前 `cnb/main` 上独立核验后另提新 commit。

判定表（30 条；`path:line` 未标「基线」者为现状值）：

| # | 它说的 | 判定 | 证据 |
|---|---|---|---|
| C1 | `blame.rs:16` / `:90` / `:14-15` 三行原文（`MAX_BLAME_BYTES = 1024 * 1024`、`if blob.data.len() > MAX_BLAME_BYTES {`、文档「Files larger than this are refused」） | **成立** | 逐行 `awk 'NR==n'` 全中；N19 翻案的证据链完整 |
| C2 | `lib.rs:3141` / `:3135` / `:3095` 三处（定义 / 上一函数收 `}` / 调用点） | **成立** | `:3141 pub(crate) fn spawn_post_push_hooks(`、`:3135 }`、`:3095 spawn_post_push_hooks(` |
| C3 | 基线 `0a9a7dd` 的 `:3202` / `:3196` / `:3163` 逐字命中；`:3190` 确是 `StatusCode::GATEWAY_TIMEOUT,`、`:3189` 是 `(` | **成立** | `git show 0a9a7dd:crates/rg-http/src/lib.rs` 逐行打；S2 的自曝成立 |
| C4 | `pat_scope.rs:307` 是 fn、`:305` 是 `///`；基线 `:301` / `:299` 同形 | **成立** | 四行全中 |
| C5 | `git grep -c '1024 \* 1024'` 口径 = **10 个文件**、`-n` 口径 = **20 行**，两棵树同；故 A19 的「真值 20」成立 | **成立** | `git grep -n … \| wc -l` = **20**、`git grep -c … \| wc -l` = **10**，`ae11e36` 亦 20 |
| C6 | `lib.rs` 4045 → 3992、`pat_scope.rs` 661 → 634，漂移行三处映射全对 | **成立** | `git show ae11e36:<path> \| wc -l` 对 `wc -l` 四个数字全中 |
| S1 | 头部「现状漂移」行五个 `→` 全对（另见 E6） | **成立** | 五处逐一打 |
| S2 | §2 N19 行（翻案理由 + 四段取证 + 20 行命中） | **成立** | 与 C1/C5 同源 |
| S3 | §2 N26 行（`:3190` 是元组内、`:3196` 收 `}`、`:3198` 起 doc） | **成立** | 与 C2/C3 同源 |
| S4 | §2 N27 行（`:301` fn / `:299` doc；现状 `:307` / `:305`） | **成立** | 与 C4 同源 |
| S5 | §2 N28 行（`pulls.rs:630` 确为 4 项，`:628-629` 是那两行注释） | **成立** | `:628`/`:629` 两行注释 + `:630 "not_evaluated": ["conflicts", "webhook", "ci_trigger", "notification"],` |
| S6 | §3 第 1 段 11 变体行号（`:370/:373/:375/:377/:382/:393/:399/:406/:408/:410/:413`），它自己下载 `gix-merge-0.23.0` 逐行对过 | **成立** | 11 行全中（`SubmoduleMerge`/`SubmoduleAddAdd`/…/`Unknown`） |
| S7 | §3 第 2 段（`:371-373` 子模块限定、`:397-403` 类型冲突限定；普通 add/add 走 base=`None` + 内容合并 ⇒ `Resolution::Conflict`） | **成立** | 两段原文与 `tree/mod.rs:161` 一致 |
| S8 | §7 R17-1a / R17-1b（`:534`/`:535` 是两个收尾 `}`、`:538 Ok(())`） | **成立** | 逐行打 |
| S9 | §7 R17-2 验收条件（`agent-principals…:518` 有标注、`:520` 无废弃） | **成立** | 两行原文 |
| S10 | §7 R17-7（`pub(crate)` + 2 个调用点全在 `rg-http`，core 零命中） | **成立** | `grep -rn spawn_post_push_hooks crates/` = 3 行（定义 + 2 调用），core 侧 0 |
| S11 | §8「两条 + 已翻案」编号顺移与 §2 / 分布自洽 | **成立** | 段内三条编号连续、与 21/10/2 自洽 |
| S12 | 台账本节 + 反向 67：作废改写带证据、编号未回收 ✅，但**判定分布算错**（见 E1） | **部分成立** | 前半全对；分布行原文「成立 23 / 部分成立 3 / 不成立 0」与同节 26 行表不符 |
| S13 | 文档 §2 的 33 条分布 21 / 10 / 2（20 + 1 翻案 = 21、3 − 1 = 2） | **成立** | 与 §2 各行判定列复算一致 |
| E1 | 台账本节写「26 条 = 23 / 3 / 0」，而同节 26 行表是 **22 / 4 / 0**；部分成立的四条是 A7 / A13 / A19 / A21；同一句错在**三处**（台账分布行、提交信息、回帖 §一 开头） | **成立（我方真错）** | 脚本按判定列复算：26 行里「部分成立」= A7、A13、A19、A21 共 **4** ⇒ 成立 **22**。三处全在：台账分布行、`git log -1 --format=%s 4059a68`、回帖 `2121065805633753088` |
| E2 | A20 判「成立」但 §2 N1 行**没改**，仍写「两个调用点（`:1897` / `:1928`）」，真值绑定行是 `:1896` / `:1927` | **成立（判成立未落稿）** | `sed -n '32p'` 原文仍是 `:1897` / `:1928`；`service.rs:1896 let (merged_tree_id, _conflicts) =`、`:1897` 才是调用行，`:1927` / `:1928` 同形 |
| E3 | A11 判「成立」但文档里 `grep -c 'pull_request/merge_queue'` = **0**，§1 仍写 `merge_queue.rs:267`、N21 仍写 `merge_queue.rs:441` | **成立（判成立未落稿）** | `grep -c` = 0；两处裸文件名；真文件在 `crates/rg-core/src/pull_request/merge_queue.rs` |
| E4 | 台账 A15 证据「现状 **4** 命中 = 1 定义 + 2 调用 + 1 处 `crate::` 前缀同一条」重复计一次，`grep -ro \| wc -l` = **3** | **成立** | `grep -ro 'spawn_post_push_hooks' crates/ \| wc -l` = **3**；`-rn` 三行（`:3141` 定义、`:3095` 调用、`repo_content.rs:1384` 调用）。此行首即 `crate::`，不是第四条 |
| E5 | §3 与 N13 引「`blob/mod.rs:22-25` 原文…」多包一行：`:22-24` 是变体文档、`:25` 是变体名 `CompleteWithAutoResolvedConflict,`；后句 `:26-27` 同理（`:26` 文档、`:27` 变体名 `Conflict,`） | **成立** | `awk 'NR>=15 && NR<=32'` 打出 `:22-24` 三行文档、`:25` 变体名、`:26` 文档、`:27` 变体名 |
| E6 | 「其余被引文件与 `ae11e36` 逐行相同」这句偏宽：`branch_protection/service.rs` 改了 1 行（`:426` 那句注释），`api/repo_commits.rs` 也有 `5+/2−` | **部分成立（结论对、归因错）** | 句子确实偏宽（两句都改过）；但**都不由 `4059a68` 产生**：`git show --stat 4059a68` = 2 文件（台账 + 本文档），`crates/` 零行；`service.rs:426` 来自 `0a9a7dd`（第 1 次进入轮），`repo_commits.rs` 来自 `cf6943d`。它把「本流程历史轮」记成了「本轮」 |
| D1 | 「核心翻案（N19）、N26 自曝、R17-1 拆分、R17-2 验收条件、§3 两段取证全部站得住，这次没找到新的不成立」 | **确认** | 与本节自查一致（本轮 0 条产品结论被推翻） |
| D2 | 「要动手的只有 E1（3 处同改）与 E2 / E3，加上 E4–E6 三处小订正」 | **成立** | 五个项全在，处置与它建议一致 |
| D3 | 「`4059a68` **尚未被我复核过**这句话仍然成立」 | **部分成立** | 该句在它**本条评论里已作废**——它紧接着写「我这次复核的就是 `4059a68`」。文档 / 台账若保留「尚未复核」即为过期表述：`4059a68` 已被它本轮复核（E1–E6 即对其所提），本轮新提交见本节末 SHA，那一笔才是「待复核」的 |
| D4 | 「我没跑 `cargo`（本容器无 toolchain），你的 `EXIT=0` 我只记录、无法复现」 | **确认**（环境声明，采信） | 与 `~/.cargo` 空、容器无 toolchain 的既往自述一致；本项在复算环境无 `cargo` 时**不可验**，不表述为公共可复算事实 |
| D5 | 开头「另有 **3** 条判『成立』的事项没落稿」 | **子命题不成立** | 它自己 §四 只列 **E2 / E3** 两条（「E2 / E3 是同一个模式」）⇒ 真值 **2**，无第三条。属它自己的加法/枚举错，不影响 E1–E6 结论 |

**我方自查（T1–T3）**：

| # | 自查项 | 判定 | 处置 |
|---|---|---|---|
| T1 | 判定分布 23/3/0 | **成立（真错，E1）** | 26 行表复算 22/4/0。台账分布行已就地订正并注明原文；提交信息 `4059a68` **不可改**（已在台账登记该事实）；回帖 `2121065805633753088` 已 `patch-issue-comment` 订正该数字 |
| T2 | A20 / A11 判「成立」但正文未改 | **成立（E2 / E3）** | §2 N1 改 `:1896` / `:1927`（并补 `:1968` 判空 / `:1969` 只 warn / `:1970-1973` `return Err` 的真实分层）；§1 与 N21 的 `merge_queue.rs` 补全路径 |
| T3 | A15 证据「4 命中」 | **成立（E4）** | 改成 3 命中并写明 `crate::` 是同一条的前缀、不是第四条 |

- **新 L2**：**R17-9 维持**（P3，`ironforge-docs/README.md:56` 仍写「20 成立 / 10 部分 / 3 不成立」，与文档 §2 的 21 / 10 / 2 分叉；本轮文件数到上限 2，未修，下轮 L1 直接订正）· **R17-10**（P3，流程项：**判定分布必须由判定表机械复算**，不许手写——本轮 E1 就是这么错的；建议固定一句 `awk` 按判定列计数，写进台账「审计口径」）· **R17-11**（P3，流程项：**判「成立」≠ 已落稿**，需对「成立且需改正文」的行锚点逐个 `grep` 回正文确认旧行号已消失——E2 / E3 就是漏这一步）。
- **未做**：未改任何代码（`crates/` **零行**，未动 `Route`、合并行为、`check_merge_allowed`）；未修 `ironforge-docs/README.md:56`（R17-9，文件数到上限）；未把 R17-1b 的 fail-closed 行为落地（L2）；未实测冲突提取（`write_blob_to_odb` 落库结论仍来自 gix 源码）。
- **反向清单**：**本节无新增**（30 条里 0 条产品结论不成立；D5 那条是它自己的枚举错，记在判定表而不入反向清单）。累计仍 **1..106**，67 号已于第 2 次进入轮作废（编号不回收）。
- **复核请求（限定文件）**：`ironforge-docs/architecture/merge-readiness-current-state-2026-11.md` 的 `:7`（收紧后的「逐行相同」句）、`:24`（`merge_queue` 全路径）、`:32`（N1 的 `:1896`/`:1927` 与 `:1968-1973` 分层）、`:44`（N13 的 `:22-24`）、`:52`（N21 全路径）、`:87`（`:22-24` 与 `:26`）；以及本台账本节与上一节已订正的分布行、A15 证据行。请特别复算：`git show --stat 4059a68`（应只有 2 文件）、`grep -ro 'spawn_post_push_hooks' crates/ | wc -l`（应为 3）、`grep -c 'pull_request/merge_queue' <文档>`（现应为 2）。
- **验证**：见本节末「改动 SHA / 回帖（回填）」。

**改动 SHA / 回帖（回填）**：`0279881`（2 文件 `75+/8−`；`crates/` 零行 ⇒ 源码 `path:line` 零漂移；文档仍 **136** 行）。推 `cnb` 单 remote：`35bbb4f..0279881  HEAD -> main`；`merge-base --is-ancestor HEAD cnb/main` 通过，主工作区 `merge --ff-only` 到 `0279881`、`status --porcelain` 0 行。**改后验证**（executor 本机 `Darwin arm64` / `cargo 1.99.1`）：`scripts/check.sh --quick` `EXIT=0` **2 s**（``Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.24s`` / `==> checks passed in 2s (quick)`）；改前 **88 s** `EXIT=0`（``Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 26s``）。**复算环境无 `cargo` 时不可验。** 时刻锚 = `git log -1 --format=%aI 0279881` = `2026-11-13T11:27:51+08:00`。**回帖** `2121082735560171520`（`2026-11-13T03:29:31Z`，11455 字节）；回读首行 `## 本轮收尾`、次行 `protocol=v0.13 · prompt=ironforge-consumer-v4`、末非空行独立 `@CodeBuddy`、判定表 C1–C6 / S1–S13 / E1–E6 / D1–D5 共 30 行连续无缺；该 issue 的 `## 本轮收尾` 计数 **2 → 3**，无重复投递。旧回帖 `2121065805633753088` 已 `patch-issue-comment` 把「23 成立 / 3 部分成立」改为「22 / 4」并注明原文误记（`status: 200`，回读确认）。issue **保持 open**（本体是 R17-1a/R17-1b、R17-2、R17-8、R17-9…R17-11 的 L2 决策）。

### 审计口径补充（第 3 次进入轮立，R17-10 / R17-11，两条都是本轮自己踩出来的）

8. **判定分布只许由判定表机械复算，不许手写**（R17-10）：写完表跑一次按判定列计数的 `awk`，把输出贴进分布行旁边。本轮 23/3/0 是手写，真值 22/4/0 —— 同一句错还扩散到提交信息与回帖两处。命令模板（已在全台账上实测；先 `sed 's/\\|/ESC/g'` 消掉单元格内的转义竖线，再 `tr -d '*'` 去粗体，最后截断 `（…）` 注记）：
   `awk -F'|' '/^\| [A-Z][0-9]+ \|/ {print $4}' <file> | sed 's/\\|/ESC/g' | tr -d '*' | sed -E 's/（.*//; s/^ +| +$//g' | sort | uniq -c`
9. **「判成立」≠「已落稿」**（R17-11）：判定表写「成立」而正文锚点没改，是独立一类错（本轮 E2 / E3 两条）。收尾前对「成立且需改正文」的行逐个 `grep` 回正文，确认**旧行号已消失**；只验「新行号存在」不够。
