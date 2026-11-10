# Registry（OCI）鉴权：票据校验侧不看仓库可见性（验证记录，2026-11-11）

> **状态**：未修复 ｜ **可信度**：确认（代码逐行核对，**无运行证据**）｜ **来源**：`cnb/main` `9f18fd7` 静态复核
> 台账条目：`docs/reports/OPT-LOOP-LEDGER.md` 待拍板 **B2-11**（本流程不动 main，只记录）。
> 本文从 [ADR-0001](../ci/adr-0001-third-party-actions-execution.md) §7 移出：那条 ADR 冻结的是 CI Runner 与第三方 Actions 的边界，OCI 可见性没有对应验收条款，留在 §7 只能靠「有人记得」关闭。

## 结论

OCI Bearer token 的**签发侧**按 `can_read_repo` 裁剪，**校验侧**只做 scope 字面匹配。后果不是「任意拉取」，而是**撤销滞后**：仓库在票据有效期内转私有，旧票据仍可继续拉 60–300 秒。

## 链路

**校验侧**（`crates/rg-http/src/oci.rs`，`check_access` 起于 `:86`，被 `require_access` `:162` 调用）：

| 分支 | 行 | 判据 |
|---|---|---|
| 会话 JWT | `:99-126` | `:114` `can_read_repo` / `:118` `can_write_repo`，**看可见性** |
| OCI Bearer | `:128-144` | `:138` `matches_repo(owner, repo) && has_action(...)` 成立即 `:139` `return Ok((true, None))`，**不查仓库** |
| 匿名 | `:146-150` | `:148` `can_read_repo(..., None)`，**看可见性** |

`:138` 的 `matches_repo` 是字符串等值（`crates/rg-core/src/auth/oci_token.rs:114`：`self.scope_type == "repository" && self.name == format!("{}/{}", owner, repo)`），票据里没有任何仓库 id 或可见性字段——`OciTokenClaims`（同文件 `:42-70`）只有 `iss/sub/aud/exp/iat/nbf/jti/scope` 八个字段。

所以「可见性」只在这条链上被查两次（`:114` 会话 JWT、`:148` 匿名），**Bearer 分支一次都没有**。第 3 轮曾把这个函数写成「跳过函数里唯一的 `can_read_repo`」，不准确：抛开会话 JWT 分支，函数内共有两处调用，这一点已按第 4 轮复核更正。

**签发侧**（同文件 `get_token`，起于 `:231`）：

- `:296-330` 逐条解析请求的 scope，`:297` `has_action("pull")` → `:298-304` `can_read_repo`，`:309-321` push 走 `can_write_repo`；不通过的 scope 不进 `granted_scopes`。
- `:342` `let ttl = if username == "anonymous" { 60 } else { 300 };`，`:358` 以 `expires_in` 回显。

即：今天签不出「无权限仓库」的票据，**但签出去的票据在有效期内不跟随授权变化**。

## 严重度口径

- **不是** 越权签发：攻击者无法为自己读不到的仓库换取票据。
- **是** 撤销滞后：仓库转私有 → 已在手的票据继续可拉，窗口 = 剩余 TTL（认证 300 s / 匿名 60 s）。
- 与 ADR-0001 §7 的 R-1/R-2/R-3 同形（实例级对象 + 缺少租户侧校验），但**不同类**：那三条是「配置/触发路径上没有归属概念」，这一条是「有校验点、有 TTL、缺一次复查」。故不并入那条 ADR。

## 未验证

- 未做运行时复现（需先起实例、签票据、转私有、在窗口内重放）。窗口由 `:342` 的 TTL 决定，属代码可读事实，不是估时。
- 未核 `registry:catalog:*` 那条分支（`:331-336`）对私有仓库的暴露面。
- 修复方案（校验侧补一次 `can_read_repo`，或把 `check_repo_access` 改成回调式签名）属代码改动，见台账 B2-11 / B2-4。
