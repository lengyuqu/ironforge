# Registry（OCI）鉴权：票据校验侧看仓库可见性（验证记录，2026-11-11）

> **状态**：**已修复**（`43e3092`，PR #24 合入 `9b6bdde`）｜ **可信度**：确认（代码逐行核对 + 仓内 in-process 回归测试存在）｜ **本文行号基准**：`cnb/main` `a94839b`；第 6 轮在 `e531f40` 上复核，`oci.rs` / `oci_token.rs` / `oci_permission_tests.rs` 自 `a94839b` 起未变（`git diff --stat a94839b..e531f40` 只命中 `password.rs` 与本目录两份文档），行号仍成立
> 台账条目：`docs/reports/OPT-LOOP-LEDGER.md` 待拍板 **B2-11**（已关闭）。
> 本文从 [ADR-0001](../ci/adr-0001-third-party-actions-execution.md) §7 移出：那条 ADR 冻结的是 CI Runner 与第三方 Actions 的边界，OCI 可见性没有对应验收条款，留在 §7 只能靠「有人记得」关闭。
>
> **修订史**：第 4 轮（`3be1fa0`）写下「未修复」版本；第 5 轮因 PR #24 合入而整体重写——旧版所有行号都基于 `9f18fd7`，且结论已作废。保留这段是因为它的判定逻辑仍可当对照组：**签发侧裁剪 ≠ 校验侧复查**，缺的正是后者。

## 结论

修复前：OCI Bearer token 的**签发侧**按 `can_read_repo` 裁剪，**校验侧**只做 scope 字面匹配。后果不是「任意拉取」，而是**撤销滞后**——仓库在票据有效期内转私有，旧票据仍可继续拉，窗口 = 剩余 TTL（认证 300 s / 匿名 60 s）。

修复后：Bearer 分支在认下 scope 之后**必须再查一次当前可见性**（`crates/rg-http/src/oci.rs:157-172`），`pull` 走 `can_read_repo(.., claims.uid)`（`:159`），`push` 走 `can_write_repo`（`:164`）且匿名单据直接判否（`:169`）。撤销滞后窗口归零：可见性一改，下一请求即失效。

## 链路（当前 `a94839b`）

**校验侧**（`crates/rg-http/src/oci.rs`，`check_access` 起于 `:86`，被 `require_access`（定义 `:186`）调用）：

| 分支 | 行 | 权限查询 | 看可见性 |
|---|---|---|---|
| 会话 JWT | `:100-126` | `:114` `can_read_repo` / `:118` `can_write_repo` | 是 |
| OCI Bearer | `:128-175` | scope 匹配 `:149-155` **之后**，`:159` `can_read_repo(.., claims.uid)` / `:164` `can_write_repo` | 是（修复后） |
| 匿名 | `:177-181` | `:179` `can_read_repo(.., None)` | 是 |

计数口径（上一版在这里被复核抓到过两次，第 6 轮自查又抓出一次少算）：`oci.rs` 全文件共 **7 处** `can_read_repo` / `can_write_repo` 调用——校验侧 5 处（`:114` / `:118` / `:159` / `:164` / `:179`）、签发侧 2 处（`:329` / `:342`）。上一版写「5 处」是把 Bearer 分支修复后新增的 `:159` / `:164` 漏掉了。**本计数只含这两个函数的调用**：`:152` 的 `matches_repo` + `has_action`、签发侧 `:328` / `:340` 的 `has_action` 属票据授权面判定，不计入。核对命令 `grep -n 'can_read_repo\|can_write_repo' crates/rg-http/src/oci.rs` 命中 11 行，其中 7 行是调用、4 行是注释（`:82` / `:142` / `:146` / `:375`）。

`:152` 的 `matches_repo` 仍是字符串等值（`crates/rg-core/src/auth/oci_token.rs:123-125`：`self.scope_type == "repository" && self.name == format!("{}/{}", owner, repo)`），票据里没有任何仓库 id 或可见性字段——`OciTokenClaims`（同文件 `:42-80`）只有 `iss/sub/aud/exp/iat/nbf/jti/scope/uid` 九个字段，其中 `uid`（`:74`）就是修复新增的复查凭据。

**为什么必须有 `uid`**：`can_read_repo` 不接受用户名，只接受 `Option<i64>`。修复前票据只带 `sub`（用户名，`oci_token.rs:47` 的文档注释明写「username (or `anonymous`)」），校验侧拿不到 id，所以「再查一次」在数据上做不到——这是这条缺陷的**结构性成因**，不是漏了一行调用。`oci.rs:141-144` 因此把「`uid` 缺失的旧票据按匿名处理（只对公开仓库有效）」写成显式规则。

**签发侧**（同文件 `get_token`，起于 `:262`）：

- `:327-353` 逐条解析请求的 scope：`:329` `can_read_repo` 决定 `pull`、`:342` `can_write_repo` 决定 `push`（且 `:341` 要求 `authenticated_user_id` 存在）；不通过的 action 不进 `granted_scopes`。
- `:376` `let ttl = if username == "anonymous" { 60 } else { 300 };`，`:398` 以 `expires_in` 回显。

## `registry:catalog:*`：签得出、用不上，但靠三件巧合

该 scope 不是「未核」，是核完了——三项**互相独立**的实现**偶然**叠加，才使今天无法利用；拆掉任意一项，它就是「枚举全部私有仓库名」的现成入口，所以这里的「不可利用」是**巧合的当前值**，不是防御设计的结果：

1. **当前没有消费方**（「没接」，不是「拒绝」）：`/v2` 路由（`crates/rg-http/src/lib.rs:520-555`，即 `build_v2_routes` 整体）只有 `api_version_check` / `get_token` / `tags/list` / `manifests` / `blobs` / `blobs/uploads`，**没有 `_catalog` 端点**——今天没有任何路由能把它换成内容。
2. **签发侧零仓库门**：`oci.rs:362-366` 只判 `scope_type == "registry" && name == "catalog" && authenticated_user_id.is_some()`，任何登录用户拿 Basic Auth 换 `registry:catalog:*` 必成功，票据 `scope` 字段会带上它。
3. **校验侧永不匹配**：`:152` 要求 `parsed.matches_repo(owner, repo)`，而 `matches_repo`（`oci_token.rs:123-125`）硬编码 `scope_type == "repository"`，`catalog` 类型恒为 false。注意这不等于「catalog 票据有一点点用」——它对**任何**仓库端点都是 false，即这张票在仓库链路上完全无效；catalog 的残余风险只有一项，且是**枚举可用性**（补上 `_catalog` 路由即可枚举私有仓库名），不是「能读到某个具体仓库」。

另有一处放大：`api_version_check`（`oci.rs:227-245`）**无条件**对匿名请求广告 `scope="registry:catalog:*"`（`:240`），等于主动引导客户端去申请一个用不上的 scope。

定级 **P2**：今天不可利用（三项同时成立），但它不是纵深防御设计出来的，是巧合叠出来的。台账条目 `B2-15`。

## 严重度口径（修复前 vs 修复后）

- 修复前**不是**越权签发：攻击者无法为自己读不到的仓库换取票据。
- 修复前**是**撤销滞后：仓库转私有 → 已在手的票据继续可拉，窗口 = 剩余 TTL。
- 与 ADR-0001 §7 的 R-1/R-2/R-3 同形（实例级对象 + 缺少租户侧校验），但**不同类**：那三条是「配置/触发路径上没有归属概念」，这一条是「有校验点、有 TTL、缺一次复查」。故不并入那条 ADR。
- R-1/R-2/R-3 本批**仍未修**，不能因为本条关闭就认为 §7 也关了。

## 证据

- 静态：上表逐行 `sed -n` 核对（`a94839b`）。
- 运行用例：`crates/rg-http/tests/oci_permission_tests.rs:135` `oci_token_goes_stale_when_repo_turns_private`——先在公开仓库换票据、请求通过，再把 `is_private` 直接改成 `true`，同一张票据必须 401（断言在 `:181-185`，文案原文在 `:184`：「a token minted while the repo was public must not survive it turning private」）。这是 in-process 应用（`spawn_test_app_with_db`），不需要外部服务。
- 对照组 `crates/rg-http/tests/oci_permission_tests.rs:192` `oci_token_still_works_for_private_repo_with_access` 钉住反向：有权限的私有仓库必须继续可用——这条是防「无脑 `can_read_repo(.., None)`」把合法 push 全 403 的（`oci.rs:146-148` 的注释写了同一句）。
- **执行记录（2026-11-11，`a94839b`）**：`cargo test -p rg-http --test oci_permission_tests oci_token_goes_stale_when_repo_turns_private` → `1 passed; 0 failed; 3 filtered out`（1.01 s，编译 1 m 08 s）。即「撤销滞后」在修复后确实不再复现，且这是运行时观测，不只是读代码。对照组那条未单独跑。

## 仍未做

- `check_repo_access`（`crates/rg-core/src/auth/oci_token.rs:219`）仍未删、仍是零生产调用的可见性盲函数，属台账 `B2-4`，与本条同根但独立。
- `sub` 与 `uid` 现在语义重叠：`sub` 只用于展示与匿名判定，鉴权已完全走 `uid`。是否删 `sub` 属于票据契约变更（对外可见的 JWT claim），未拍板。
