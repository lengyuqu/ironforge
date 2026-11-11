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
| 验证命令（文档轮） | `scripts/check.sh --quick` | **实测 115 s 冷 / 2 s 热**（文档写 ~40 s，是忽略 clippy 首次编译的口径） |
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

### batch#4(issue #27) · 2026-11-11 · rg-http issue/PR/review API 层

- 开批：`2026-11-11T02:43:08Z`（issue 创建即触发，正文结尾 `@CodeBuddy`，标题已绑定 `[opt-round] batch#4(issue #27)`）
- 基线提交：`b7a0ab2`（cnb/main HEAD，2026-11-11；本机 main 检出同点、工作区干净，直接本地记账，无需 worktree）
- 范围：`crates/rg-http/src/api/` 4 文件共 4086 行（issues 1681 / reviews 1261 / pulls 902 / labels 242）。行数以 `git show b7a0ab2:<path> | wc -l` 统计
- 引导问题 4 条：P2 死校验 `validate_optional_text(...).ok()` 丢弃 Err 共 4 处（`issues.rs:531-536`/`:653-658`、`labels.rs:143-148`、`pulls.rs:293-298`）；P2 `update_issue` 鉴权前取值（`issues.rs:603-606` 在 `:635` can_read 之前调 `get_issue`，404/403 可区分私有仓 issue 存在性）；P3 label 颜色 create/update 双口径（3 位 hex 创建 201、更新 400，`labels.rs:139-142` vs `rg-core/src/label/service.rs:92-99`）；P3 列表页 assignee N+1（`issues.rs:1176`，milestone 已批量、assignee 未）
- 反向清单回灌：**不成立 33 条全量**（batch#1/#2/#3 累计，含 batch#3 第 2–6 轮新增的 23–33）+ 我方自证推翻 8 项 + 对方已撤回 6 项 + 对方错项 5 条，均注明「不要重复提」
- 已知空档新增：issue 创建只需读权限为刻意设计（`issues.rs:500-504` 注释，ISSUE-105）；`pulls.rs:627-629` dry_run 不评 conflicts 为注释自述的已知语义
- 凭据事故（下次照做）：开批前 `git fetch cnb` 报 Repository Not Found；第 9 次配方（`username=oauth2` 灌 keychain）**覆盖不中**——keychain 实际条目 `username=cnb`，须以 `username=cnb` 覆盖 `git credential approve` 才恢复
- NPC 首扫：`2120347578683269120`，`2026-11-11T02:47:11Z`（开批后 4 min 03 s；`author.is_npc=true`，首行 `@cnb.biewen(别问) batch#4 只读审查完成`，自述基线 `b7a0ab2` 并逐文件核过行数）
- 6 轮进度：**3 / 6**（第 1 轮收尾 `2120361552162230272`、第 2 轮收尾 `2120377967560794112`、第 3 轮见下 `consume 3 / 6` 节）

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

**改动 SHA**：`7111d97`（L1，`a996cab..7111d97`，2+/2−，906 行不变）+ 本条 ledger 记录；推 `cnb`。**回填**：见下节（评论 id 与回读 C=3）。

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

**并行修复观察（第 4 轮写入，后续轮次必读）**：`cnb` 上已有两个基于 `9f18fd7` 的分支在修本批的 L2 条目：

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

## 判定不成立（原文保留，请勿重复提）

见上文同名小节（本清单已累计 **1..35** 条）。本行原为开批时的占位「（暂无）」，已失效并就地订正，勿再当「无任何不成立条目」读
