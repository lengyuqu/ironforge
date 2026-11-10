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

### batch#2(issue #12) · 2026-11-10 · rg-core auth 凭据链路

- 开批：`2026-11-10T13:12:45Z`（issue 创建即触发，正文结尾 `@CodeBuddy`；NPC 流水线 `cnb-gc6-1k72pdras` 已 pending，platform status 自述 SHA `2191cba`，与本批基线一致）
- 基线提交：`2191cba`（cnb/main HEAD，2026-11-10）
- 范围：`crates/rg-core/src/auth/` 10 文件共 2946 行（pat_scope 656 / sso 512 / oci_token 395 / password 364 / jwt 244 / ldap 240 / ci_token 203 / ssh_key 140 / totp 109 / encryption 83；排除 `mod.rs` 12 行与 `ci_oidc.rs` 146 行）。行数以 `git show 2191cba:<path> | wc -l` 统计，非工作区版本
- 引导问题 4 条：P1 常见密码检测 `take_while` 前导字母剥离可绕过（`password.rs:235`）；P1 `TooCommon` 把明文密码写进 `Display`（`password.rs:69-72`）；P2 `verify_code` 无重放记录、调用方是否拦截待核（`totp.rs:37`/`:51`）；P2 `validate_mfa_challenge` 用 `Validation::default()` 无 iss/aud（`jwt.rs:135-139`）
- 已知空档（写进正文）：TOTP SHA1 为 RFC 6238 标准；JWT HS256+7d 为已知设计；`feat/agent-principals` 分支（agent 账户，本机未推送）不在基线内；LDAP/SSO 宽松错误处理为刻意决策；待拍板 #1–#11 属第 1 批范围
- 反向清单回灌：判定不成立 4 条（含撤回的 sideband/pkt_line 行号争议）+ 对方已撤回 4 项 + 第 6 轮行号错项 5 条
- **新增要求**（第 1 批收敛建议落地）：每条结论必须附自证形式（`sed`/`grep` 输出或实测），不接受纯断言
- NPC 首条分析：**已到**，`2026-11-10T13:16:43Z`（延迟 4 min 24 s；自述基线 `2191cba`，与开批记录一致）
- 6 轮进度：**4 / 6**
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

1. **B2-1 `reject_common` 对「前导非字母」的密码整段失效**（P1，实测复现）：`crates/rg-core/src/auth/password.rs:233-237` 的 `take_while` 只取前导字母段，`1Password1!` / `!Password1` / `0Admin123!` 全部 HTTP 201 注册成功，只有 `Password1!` 被拦。修法二选一：①取**最长**字母段再比对；②先剥离所有非字母字符再比对。影响面只在校验点（存量密码不受影响）：`crates/rg-core/src/user/service.rs:166`（注册）与 `:755`（重置）。**第 2 轮定稿建议：选 ①（最长字母段）**。依据（逐条复算，非沙盘估算）：方案 ② 在 `Welcome1!x` 上会**丢覆盖**——现状 `stripped="welcome"` 命中词表被拦，剥全部非字母得 `welcomex` 不命中 ⇒ 放行；方案 ① 在该组及 `1Password1!`/`!Password1` 上均为「只扩大、不缩小」。若仍选 ②，必须补 `Welcome1!x` 回归用例。另注：`Passw0rd!` 在现状/①/②下**全部放行**（`stripped="passw"`），l33t 归一化超出本项范围。
2. **B2-2 `TooCommon` 把明文密码回显进响应体**（P1，实测复现）：`password.rs:69-73` 的 `Display` 带 `pwd`，经 `user/service.rs:169`/`:758` 的 `e.to_string()` 与 `rg-http/src/error.rs:171`→`:114-120`（H-05 只脱敏 `InternalError`）出现在未鉴权 `POST /users/register` 的 400 响应体。最小修法：`TooCommon` 不再携带值（`Display` 改为固定文案），调用方零改动。**附带核查项**：其余把用户输入拼进 `Display` 的错误变体应一并清点。
3. **B2-3 TOTP 无重放保护**（P2，代码事实）：`totp.rs:37`/`:51` 是纯函数，`api/mfa.rs:327` 也不记录已用码；`challenge` cookie 5 min TTL（`jwt.rs:125`）远大于 30 s 步长，同一码可重复换 JWT。需「已用码」存储（带 TTL 的表或内存 LRU）⇒ 存储/schema 改动。
4. **B2-4 `check_repo_access` 是可见性盲的死函数**（P2→P3）：`oci_token.rs:204-217` 无令牌即 `return required_action == "pull"`，完全无视仓库公开性；全仓零生产调用（唯一「调用方」`rg-http/src/oci.rs:198` 把返回值 `let _ =` 丢弃）。建议删除，或改成接收 `can_read_repo` 回调的签名。本轮已把 `oci.rs:73` 那句「caller should use check_repo_access」的误导注释改掉。
5. **B2-5 `derive_key` 是裸 SHA-256 且与 JWT 共用 secret**（P3，事实）：`encryption.rs:12-19` 单轮 `Sha256(jwt_secret)`。换 HKDF/加盐会让 `TOTP secret`、`SSO refresh token`、`CI secret` 的全部既有密文失效 ⇒ 数据迁移级改动，不建议在本流程内做；若要做，先写重加密迁移（读旧键 → 写新键 → 双键过渡）。
6. **B2-6 `mask_values` 的 4 字节下限无兜底**（P3）：`encryption.rs:62` 对短值静默不遮蔽；生产唯一写入点 `api/ci_secrets.rs:94` 由 `:86` 强制 4..=65536，但 `ci_secret_ops::upsert` 自身无校验（`rg-ci/src/runner.rs:1522` 是 `#[cfg(test)]`，模块起于 `:1121`）。建议把长度校验下沉到 ops 层。**第 2 轮修正（重要）**：`mask_values` 的入参是**解密后的明文**（`api/runners.rs:796` `decrypt`→`:809` 收集→`:537` 传入；`rg-ci/src/runner.rs:754` `decrypt`→`:758` push→`:528`/`:535`），**密文从不进入该函数**。故「<4 字节入参 ⇒ 漏遮」是唯一后果：既不会「被意外遮住」（NPC 英文复核的说法），也不会「secret 在 CI 里消失」（中文复核的说法），两种替代描述均不成立。
7. **B2-7 SSO CSRF `expected` 原文入 warn 日志**（P3）：`rg-http/src/api/sso.rs:386-390`；另，失败路径 `:391` 直接 return、不清 state cookie（只有两条成功路径 `:521-522`、`:543-544` 清）。建议日志只留长度/前缀。
8. **B2-8 登录时序泄露账号存在性**（P2，未实测）：`user/service.rs:232` 对不存在的用户提前 return，跳过 `:246` 的 Argon2；同文件 `forgot_password` 已有 H-5 拉平（`:654`/`:662` 各 `sleep(100ms)`）。修法：对不存在用户也跑一次固定 dummy hash 的校验。（行号基准 `2ffe7e8`；PR #47 之后 `:238-241` 另有 agent 账户分支，等价返回 `INVALID_CREDENTIALS`）
9. **B2-9 `iat`/`nbf` 从不校验**（P3，无利用路径）：`jwt.rs:139` 用 `Validation::default()`；jsonwebtoken 的 `ClaimsForValidation` 无 `iat` 字段、`validate_nbf` 默认 `false`，但仓内无签发 `nbf` 的路径。只记录，不建议加自定义校验。
10. **B2-10 `extract_user` 的哨兵 0 与丢弃调用**（P3）：`rg-http/src/oci.rs:61-78` 在 OCI token 分支返回 `Some(0)`，唯一调用点 `:198` 丢弃返回值，`api_version_check` 恒 401 —— 要么删函数，要么让它真参与鉴权。
11. **B2-11 OCI scope 票据的校验侧不看仓库可见性**（第 3 轮写入，P2，与 B2-4 同根）：`crates/rg-http/src/oci.rs:129-144` 只要 token 里的 scope 与 `{owner}/{repo}` 字面相等且含动作就 `return Ok((true, None))`（`:139`），**跳过**函数里唯一的 `can_read_repo`（`:147-150` 只在无 token 分支）。`OciTokenClaims`（`oci_token.rs:42-70`）无 `repo_id`/`visibility`，`matches_repo`（`:113-115`）是字符串等值。签发侧当前是唯一且正确的（`oci.rs:343` 唯一生产调用点，按 `can_read_repo` 裁剪 `:297-307`），所以**现状后果是「仓库在票据有效期内（`ttl` = 300 s 认证 / 60 s 匿名，`:342`）转私有后旧 token 仍可拉」**，而不是任意拉取。修法与 B2-4 合并处理：校验侧补一次 `can_read_repo`，或把 `check_repo_access` 改成回调式签名。
12. **B2-12 Runner 没有租户维度，且空标签双向全匹配**（第 3 轮写入，**P1，跨租户**）：`runners` 表无 `repo_id`（`crates/rg-db/src/entities/runner.rs:8-21`；建表迁移 `crates/rg-db/src/migrations/m20260510_000001_create_runners.rs` 同样没有），调度只看标签与 stage 状态（`crates/rg-db/src/ops/pipeline_ops.rs:909-911` Runner 无标签→领任意 Job、`:918-920` Job 无标签→给任意 Runner、`:932-953` 只判 stage/pipeline 状态），Runner 鉴权只校验「token 哈希命中某行且该行 id == 路径 id」（`crates/rg-http/src/api/runners.rs:1019-1026`）。因此任一被管理员批准注册的 Runner（`runners.rs:153-156` `require_admin`）会领到**别的仓库**的待调度 Job，并按 `pipeline.repo_id`（`:352-355`）拿到该仓库**全部** Secret 的明文。ADR-0001 §1 第 5 行原本只承认「外部 Runner 会接收（本仓）解密值」，跨仓这一层是 2026-11-10 新确认的，已补进 ADR §7 R-1/R-2。修法：给 `runners` 加仓库/组织归属并让调度按归属过滤 ⇒ schema 改动。
13. **B2-13 suggestion 触发让 fork 内容在共享 Runner 上跑，无需审批**（第 3 轮写入，P2）：`crates/rg-http/src/api/reviews.rs:161-168` 只要求对 PR 源仓库（fork）有写权限，`:200` 就在源仓库路径上查 `.gitea/workflows`，`:206-210` 以 `repo_id = source_repo.id`(`:207`)、`trigger_type = "suggestion"`(`:210`) 起 Pipeline，workflow 内容取自该 commit（`crates/rg-ci/src/lib.rs:599-608`）且无任何来源校验。ADR §2.7.4 的「fork PR 需维护者批准」只覆盖 base 仓库上的 PR 事件，对这条路径没有落点（ADR §7 R-3）。单独看只算「任意代码落到共享 Runner」，与 B2-12 叠加才是跨租户 Secret 暴露。
14. **B2-14 密码长度按字节计、文案按字符说**（第 3 轮写入，P3）：`crates/rg-core/src/auth/password.rs:193`/`:199` 用 `password.len()`（字节），而 `:53`/`:58` 的 `Display` 写 "characters"。后果双向：`密码密码`（4 CJK 字符 = 12 B）满足 `min_length = 12`；长 CJK 密码会被 `max_length` 误拒且报出「got 60」而用户只输了 20 字。修法二选一——判据改 `chars().count()`（改变校验语义）或文案改 bytes（改变对外文案）。本轮只落注释（`7ae3cf5`）。

**B2-11 表述勘误（第 4 轮写入）**：本条原写「跳过函数里**唯一**的 `can_read_repo`（`:147-150` 只在无 token 分支）」，不准确。`crates/rg-http/src/oci.rs` 的 `check_access`（`:86` 起）内共有两处 `can_read_repo`：`:114`（会话 JWT 的 pull，配 `:118` 的 `can_write_repo`）与 `:148`（匿名）。准确表述是「**Bearer 分支（`:128-144`）一处都不查**，会话 JWT 与匿名分支都查」。实质结论（Bearer 只做 scope 字面匹配、`matches_repo` 为字符串等值）不变。详细链路已迁到 `ironforge-docs/architecture/registry-auth-visibility-verification-2026-11.md`。

**并行修复观察（第 4 轮写入，后续轮次必读）**：`cnb` 上已有两个基于 `9f18fd7` 的分支在修本批的 L2 条目：

| 分支 | HEAD | 提交标题 | 覆盖条目 | 触及文件 |
|---|---|---|---|---|
| `fix/oci-token-visibility` | `43e3092` | `fix(oci): re-check repository visibility instead of trusting the token's scope` | **B2-11** | `rg-core/src/auth/oci_token.rs`（+94）、`rg-http/src/oci.rs`（+58）、`rg-http/tests/oci_permission_tests.rs`（+101） |
| `fix/password-strength-bypass` | `79c6b62` | `fix(auth): close the common-password bypass and stop echoing the password` | **B2-1 + B2-2** | `rg-core/src/auth/password.rs`（+117 段）、`rg-core/src/auth/ssh_key.rs`、`rg-http/src/oci.rs`（2 行） |

含义：①这两条一旦合入 main，`password.rs` / `oci.rs` / `ssh_key.rs` 的行号基准全部改变，凡引用这三个文件的行号必须重取；②`registry-auth-visibility-verification-2026-11.md` 的结论会被 `43e3092` 修掉，合并后应把该文状态从「未修复」改为「已修复」并补运行证据；③本流程仍**不动**这两个分支（L2 只记录，实现由分支侧跑）。

### 判定不成立（原文保留，请勿重复提）

1. 「V1 upload-pack 未收到 NUL 时会漏掉 connection 级 capability，但 flush 与 `# service=` 行上的 capability 不一致……能力协商靠「客户端好心多打一个空格」才成立，缺它则 clone/fetch 失败，而不是降级。」—— 反证：`upload_pack.rs:205-224` 的 Form A/Form B 分支覆盖了协议允许的两种形态；git 的 capability 只出现在第一个 `want` 行，客户端要么声明 `side-band-64k`（走 sideband），要么不声明（服务端裸发 packfile 属合规降级）。`scripts/git-protocol-e2e.sh:172-190` 的 `protocol.version=1` HTTP push/clone/fetch 用例长期为绿，是该路径可用的直接证据。**以下已核实为不成立，不要重复提。**

2. 「V1 + `--depth` 会真实发送 `deepen 2`，客户端把全量 pack 当浅克隆收下」—— 反证：本服务 V1 广告不含 `shallow`，客户端在发任何 `want` 之前即 `fatal: Server does not support shallow clients`（本地实例实测，trace 无 `want`/`deepen`）；V2 对照 `--depth=1` 正常。NPC 的 trace 含 `upload-pack> shallow <oid>`，该行本服务从不产生，故其服务端不是 IronForge。

2. 「`ofs-delta` 标 ✅ 站不住，`iter_from_counts` 这条路径根本产不出 OFS_DELTA」—— 反证：gix-pack 0.77 `src/data/output/entry/mod.rs:185` 把 `DeltaRef { object_index }` 直接映射为 `data::entry::Header::OfsDelta`（`:84` 另有 `OfsDelta { base_distance }` 写出分支）；`allow_thin_pack` 的文档（`iter_from_counts.rs:382-391`）只约束「base 不在本 pack 里」的 delta。`allow_thin_pack: false` 不等于「不产 OFS_DELTA」。

3. 「`sideband.rs:25` 应为 `:22`」「`pkt_line.rs:59` 应为 `:61-62`」—— **NPC 已于第 4 轮撤回**。反证：`sideband.rs:25` 正是 `writer.flush().await?;`（22 是 `payload.extend_from_slice(chunk)`）；`pkt_line.rs:59` 是 `write_pkt_line` 的函数定义，引用它正是为了说明该函数不调用 flush（61-62 是长度检查，与命题无关）。

以下 5 条由 **batch#2 第 1 轮 NPC 自行判定不成立**，消费者逐条复核后确认其判定正确，一并锁死：

4. 「`validate_mfa_challenge` 用 `Validation::default()` ⇒ 有 alg 混淆余量 / 缺 iss/aud 即缺陷」—— **不成立**。jsonwebtoken 9.3.1（`Cargo.lock:3512-3513`）的 `~/.cargo/registry/.../jsonwebtoken-9.3.1/src/validation.rs:164-167` 把 `Default` 钉成 `Algorithm::HS256`、`:115` 强制 `exp` 存在、`:120` `leeway: 60`、`:124` `validate_exp: true`；且 `jwt.rs:47-49` 的域分离密钥已阻断与普通会话串用。
5. 「`pat_scope` 的 `*` 能跨 `/` 是缺陷」—— **不成立**。`pat_scope.rs:559-567` 的用例把 `refs/heads/agent/x` 与 `refs/heads/agent/a/b` 一并断言为允许，`agent/*` 覆盖子路径是刻意语义。
6. 「`has_scope` 层级漏了 `repo:collab → repo:write`」—— **不成立**。文档表 `pat_scope.rs:10-15` 写明 collab 是「the above, plus write the collaboration area」，不含 `repo:write`。
7. 「`ldap.rs` 的 RFC 4515 转义不全」—— **不成立**。`ldap.rs:32-48` 覆盖 `\0 ( ) * \` 全部五个必备字符与所有非 ASCII（转 `\xx`）。
8. 「SSO CSRF state 用弱随机」—— **不成立**。`rg-core/src/auth/sso.rs:87-91` 是 `rand::thread_rng()`（ChaCha12 CSPRNG）+ `Alphanumeric` ×32 ≈ 190 bit，且 `rg-http/src/api/sso.rs:381-392` 有签名 cookie 双向比对。

以下 5 条由 **batch#2 第 2 轮**判定不成立（两份 NPC 复核各占，均经复算否证），一并锁死：

9. 「`oci.rs:73` 的 "it has no callers" **holds**（成立）」—— **不成立**。`grep -rn check_repo_access crates/` 有 6 处 `assert!` 调用（`oci_token.rs:321 :329 :353 :360 :367 :375`）+ 2 处测试函数名（`:318` `:339`）。准确说法是 "no **production** callers"，本轮已按此改写。
10. 「`password.rs` 词表里 8 条含数字/`-` 的条目（`abc123`/`trustno1`/`password1`/`password123`/`ec2-user`/`admin123`/`root123`/`test123`）**永远匹配不上**」—— **不成立**。`abc123`、`trustno1`、`ec2-user` 原样输入会被 `:241` 的整串精确匹配拦下（逐条复算）。正确表述：这 8 条在 `:237` 前导段分支下不可达，只能靠 `:241` 命中。
11. 「`ssh_key.rs` 注释里的 `...TKA_7j9...` 吃掉了 `q`，是不可比对的假串」—— **不成立**。`'TKA_7j9' in url` 为 `True`；该片段两端是省略号，本就是子串。
12. 「`ssh_key.rs:143` 的 docstring 只提 `/`，`+` 出现在 body 的 index 21」—— **不成立（双向错）**。当时文本为 `:142-143`「contains both `/` and `+`」；且 `std[21]` 是 `m`，全串**无** `+`。附带结论（我方错，已修）：见下「我方已自证推翻」。
13. 「`mask_values` 的 `str::replace` 会把同一串的 base64url 变形也遮掉」—— **不成立**。`str::replace` 是字面量子串替换，遮 `TKA/7j9` 不影响 `TKA_7j9`。
14. 「B2-6 的后果不是漏遮，而是 secret 在 CI 里消失」／「密文 19+ 字符 ⇒ 2 字节 secret 会被意外遮住」—— **两条均不成立**。`mask_values` 只吃**解密后明文**（`api/runners.rs:796`→`:537`、`rg-ci/src/runner.rs:754`→`:528`），密文从不进入；短明文唯一后果是不被遮蔽。

15. 「`password.rs:192` 把边界写成「4 个 CJK 字符」是错的，应该是「3 个」」—— **不成立**。`strict()` 是 `min_length: 12`（`password.rs:171`），判定在 `:193` `if password.len() < self.min_length`：3 个 CJK = **9 字节** → `9 < 12` 成立 → `TooShort`（过不了）；4 个 CJK = 12 字节 → 条件为假 → 通过。对方自己的句子也承认「3-char CJK fails at the 9-vs-12 boundary」，与其改法自相矛盾。正确写法是「4 个、只是刚好过线（no margin）」。**已核实为不成立，不要重复提。**

### 我方已自证推翻（勿当结论引用）

| 我上轮写的 | 实际 | 处置 |
|---|---|---|
| `ssh_key.rs` 注释「the digest contains **both** `/` and `+`」 | 复算 `std = uh6xDCBClTKA/7j9qTAuVmcAbusr4NwgLrWjafhfMM4`：`+` **不存在**，`/` 在 index 12 | 第 2 轮已改为「contains `/` at index 12 and no `+`」（`210f338`） |
| `ssh_key.rs:9`「byte-identical to `ssh-keygen -l -E sha256`」 | `ssh-keygen` 输出整行，本函数只回 `SHA256:<fp>` 字段 | 已改为「matches the `SHA256:<digest>` field」 |
| `encryption.rs:57` 未说明 `mask_values` 的入参形态 | 它是**明文**，且 `ci_secret_ops::upsert` 自身不校验长度 | 已在同一条注释里写明两点 |

### 对方已撤回（勿再作为争议点）

第 5 轮 NPC 在 §7 自行撤回三项，均已由其本机实测/源码证明，此后不再讨论：

| 被撤回项 | 撤回依据（它自己的话） |
|---|---|
| 「V1 + `--depth` 会发 `deepen`，全量包被当浅克隆收下」 | 它 `cargo build --release` 后实测三组对照：V1+depth exit 128 `fatal: Server does not support shallow clients`、V1 无 depth ✅、V2+depth ✅ 且 `.git/shallow` 写入。补测 `fetch --deepen=2` 同样失败。原话「你第 1 条复核请求我给不了反证」 |
| 「`ofs-delta` 标 ✅ 站不住」 | 「撤回，`mod.rs:185` 行号精确」 |
| 「`sideband.rs:25` / `pkt_line.rs:59` 引错」 | 「撤回」（第 4 轮已撤，第 5 轮重申） |
| 「`skip_until_flush` 在 EOF 时空转」 | 第 6 轮它自己撤回：「`skip_until_flush` 只认字面 `Flush`，而 `read_pkt_line` 在 EOF 时返回的正是 `Flush`，所以它会正常退出。洞在 `read_command_request` 对 `0004` 空行的处理上」 |

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

（暂无）
