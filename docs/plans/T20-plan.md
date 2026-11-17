# T20 执行计划 — [#19 步骤6] `scope_expand` 执行面：repo 系列内提权

> 权威约束见 `tasks/T20.md`。基线 `c004880`，分支 `agent/t20-scope-expand`，worktree `../ironforge-t20`。
> 实际触碰路径、验收输出与遗留风险见 `docs/plans/T20-task.md`。

## 1. 「扩权后 agent 到底拿到什么」——本任务的定义

**批准一次 `scope_expand` = 把同一个 agent token 的 `access_tokens.scopes` 原地提升到目标值。不重新签发
token，不重启会话，不改变 `user_id` / `repo_allowlist` / `ref_allowlist`。**

- agent token 的 scope 只能落在 `repo` 系列（`repo` / `repo:write` / `repo:collab` / `repo:read`）。
  `agent::validate_agent_token_scopes` 在创建时已经挡掉 `user` / `admin`，执行面再挡一次，
  所以「扩权」不可能越过这条线。
- 生效方式：PAT 中间件每次请求都用 `token_ops::find_by_hash` 从数据库重新读取令牌
  （`rg-http/src/lib.rs::resolve_pat`），scope 判定用的是刚读出来的 `pat.scopes`，**没有缓存**；
  中间件换出的 JWT 只带 `tid`（令牌 id），不带 scope。因此同一个 token 的下一个请求立即按新 scope 判定。
- 绑定对象：`subject = token_id`（十进制字符串），目标写在请求参数 `target_scopes` 里。
- 代价：扩权是**永久的**。当前没有「缩权」API，收回权力的方式只有删除或替换令牌。
  本任务不引入 scope 过期列（会需要迁移），这一点在第 7 节作为遗留风险明确写出。

## 2. `has_scope` 层级现状（实际读代码核实，2026-11-17）

`crates/rg-core/src/auth/pat_scope.rs`：

- `satisfying("repo:read")  = ["repo:read", "repo:collab", "repo:write", "repo"]`
- `satisfying("repo:collab") = ["repo:collab", "repo:write", "repo"]`
- `satisfying("repo:write")  = ["repo:write", "repo"]`
- `satisfying("repo")        = []`（只匹配自身或 `admin`）

即阶梯为 `repo:read` < `repo:collab` < `repo:write` < `repo`。**当前实现已经让 `repo:write` 满足
`repo:collab`**（`pat_scope.rs:346` 的 `repository_scopes_form_a_ladder` 用例明确断言
`("repo:write", "repo:collab", true)`），反向（`repo:collab` 满足 `repo:write`）为 `false`
（同测试断言）。反向清单里记的「漏了 `repo:collab → repo:write`」在基线上已不存在；本任务不再修改
`pat_scope.rs`，只复用其 `has_scope`，因此不会把这条层级重新写一遍、也就不会写出第二套真相。

## 3. 「扩大」怎么判定

`scope_expand::widening_of(current, target)`：

1. 两边都过 `pat_scope::normalize_scopes`（去重、按 `ALLOWED_SCOPES` 顺序规范化，保证比较不受书写顺序影响）。
2. `target` 过 `agent::validate_agent_token_scopes` —— 任何非 `repo*` 的 scope 直接 `invalid_input`。
3. 对 `current` 的**每一个** scope 要求 `pat_scope::has_scope(target, scope)` 为真。
   这证明目标覆盖当前的全部权限；覆盖不了就是收缩或平移，拒绝。
4. `target == current` 也拒绝（没有变宽，消耗一张票没有意义）。

判定不是「字符串不相等」，而是逐 scope 走中间件同一个 `has_scope`：

| current | target | 结果 |
| --- | --- | --- |
| `repo:read` | `repo:write` | 允许（write 覆盖 read） |
| `repo:collab` | `repo:write` | 允许（write 覆盖 collab，已核实） |
| `repo:write` | `repo:read` | 拒绝（read 不覆盖 write） |
| `repo:write` | `repo:collab` | 拒绝（collab 不覆盖 write） |
| `repo:read` | `repo:write,user` | 拒绝（`validate_agent_token_scopes`） |
| 任意 | `admin` | 拒绝（同上） |

## 4. 谁能批、几票、为什么 7 天

- `admins: true`、`writers: false`、`collaborators: false`、`required_approvals: 1`。
  扩权把只读令牌变成可写令牌，影响的是整个仓库的写能力，不该由 writer 或协作区成员自批；
  repo owner 天然满足 `can_admin_repo`。
- 申请人与批准人：
  - agent 永远不能批准（`evaluator::ensure_human_decider`，403 `AGENT_MAY_NOT_APPROVE`）。
  - agent 发起的请求，通用 `ensure_separate_people` 比较的是人的 id，对 agent 申请人天然不生效。
    为了让「申请人 ≠ 批准人」在这个 action 上真的成立，`decide` 增加了
    `scope_expand::ensure_decider_is_not_the_agent_owner`：**agent 的主人不能批准自己 agent 的扩权**
    （403 `AUTHORIZATION_SELF_APPROVAL`）。否则 owner 点一下就把自己的 agent 提权了，等于没有第二个人。
- 为什么 `expires_at` 是 7 天而不是其它 action 的 30 分钟：
  - 环境放行 / 直接推送绑的是「一个正在等待的 job / 一次正在进行的 push」，30 分钟外世界已经变了；
  - 合并 / 发布绑 `head_sha`，一天内有效；
  - 扩权不绑定某个提交或 job，它对应「agent 在接下来一段时间要干的活」。审批人可能不在线，
    30 分钟会把大量正常申请拖成过期重提；7 天是设计里给的数，也把「批一次、几个月后还能兑换」
    压在一个工作周期内。
  - 注意：7 天限制的是**票据**，不是扩权后的权限本身（见第 1 节与第 7 节）。

## 5. 执行面事务（`consume_scope_expand`）

```
ensure_human_decider(actor)                        # agent 不能执行
begin
  读 request：action 必须是 scope_expand；subject 必须等于 token_id（否则 invalid_input）
  读 repo（request.repo_id）
  load_expandable_token(txn, repo, token_id)       # 见第 6 节
  widening_of(token.scopes, target)                # 见第 3 节
  UPDATE access_tokens SET scopes = target
      WHERE id = token_id AND scopes = previous    # CAS，防并发扩权互相覆盖
  consume(txn, request_id, actor, effect_hash)     # 未批准/已消费/过期在这里拒绝
commit
```

任何一步失败都显式 `rollback`：**token 的 scopes 回到原值，票据保持原状态**（`scope_expand` 是
`is_retryable`，失败后可以重试）。测试 9 用「批准后票据被别的路径先消费」构造出「更新已发生、
consume 失败」的确定性场景，断言 token 未变。

`effect_hash = sha256("scope_expand:{request_id}:{token_id}:{target_scopes}")`（64 位小写 hex）。
同一请求 + 同一 token + 同一目标恒定；不同目标不同值，审计可区分一次扩权实际执行成什么。
之所以把 `request_id` 也放进去：同一 token 可以先后有多张扩权票，哈希应能区分是哪张票产生的效果。

## 6. 哪个 token 可以被扩权（跨仓库边界）

请求绑定到仓库，批准人是**该仓库**的管理员。如果 token 还能触达别的仓库，那么 A 仓库的管理员批准
一次扩权就等于替 B 仓库做了决定。因此 `load_expandable_token` 要求：

1. token 存在且未过期（过期令牌不能靠一次批准复活）；
2. token 的 owner 是**活跃的 agent**，且该 agent 有活跃的**人类 owner**（`scope_expand` 只针对 agent 令牌；
   人类令牌可能带 `user`，不属于这个 action 的语义）；
3. `repo_allowlist` **恰好只有** `["<owner>/<name>"]`，且等于请求所属仓库。

第 3 条是刻意收紧：带多个仓库的令牌、以及没有 allow-list 的令牌（= 不限仓库）都会被拒绝。
理由是没有 `parameters` 列能把批准范围钉在单个仓库上，链路上唯一能表达「这次扩权只影响哪个仓库」
的就是令牌自身的仓库白名单。若将来允许「一次批准多个仓库」，应先有绑定仓库集合的参数列，
而不是让一个仓库的管理员替其它仓库签字。

## 7. 已知限制与遗留风险

- **目标 scope 没有写进票据。** `authorization_requests` 没有 `parameters` 列（T06 §2.3 的决定），
  本任务禁止迁移，所以批准的是「把 token 12 扩宽」而非「扩到 `repo:write`」。执行面能拒绝收缩和
  `user`/`admin`，但无法证明执行的目标就是申请时展示给批准人的目标。补法是新增 `parameters` 列并把它
  纳入 `payload_hash`；这是后续任务，不是本任务可以顺手做的（改 `rg-db` 迁移 + `payload_hash` 格式）。
- **扩权永久。** 没有 scope 过期列，也没有缩权接口；收回靠删除/替换令牌。7 天只是票据寿命。
- **HTTP 端点不存在。** 仓库当前没有 `POST /authorization*` 路由（step 1 明确把路由留给后续任务；
  `authorization/mod.rs` 顶部注释也写明），而本任务禁止新开路由。因此申请侧与执行面都停在
  `rg-core` API：任何将来的 handler 只需按 `request.action` 调用 `consume_scope_expand`，
  不需要新增状态机。测试在 core 层验证 agent 能申请、不能批准/执行。
- **没有真实并发压测。** 事务性用「票据已被并发消费」的确定性等价场景验证；CAS + 单事务是并发正确的
  结构保证，但没有起两个事务抢锁。
