# T21 执行计划 — [#19 步骤6 补丁] 把目标 scope 绑进 `scope_expand` 票据

> 权威约束见 `tasks/T21.md`。基线 `3afa8a5`（含 T20 的 `scope_expand.rs`），分支
> `agent/t21-scope-expand-subject`，worktree `../ironforge-t21`。
> 实际触碰路径、测试映射与验收输出见 `docs/plans/T21-task.md`。

## 1. 要补的缺口

T20 的 `scope_expand` 只把 **token id** 写进 `subject`，目标 scope 通过执行面的参数
`target_scopes` 传入。批准人看到的是「把 token 10 扩到 `repo:write`」，但票据里只有 `"10"`，
目标没有进入票据、也没有进入 `payload_hash`。执行面只能做语义校验：

- 目标必须覆盖当前 scopes（不能收缩/平移）；
- 目标必须通过 `validate_agent_token_scopes`（不能买 `user`/`admin`）。

语义校验挡不住「批准 `repo:write`、执行传 `repo`」——`repo` 覆盖 `repo:write`，语义上是一次合法的
「扩大」，实际上比批准的范围更大。批准人对此无从约束，这是 T20 明确记为遗留风险的那条。

## 2. 为什么「subject 编码」等价于 parameters

`crates/rg-core/src/authorization/payload.rs:22`：

```rust
pub fn payload_hash(repo_id: i64, action: Action, subject: &str, head_sha: Option<&str>) -> String
```

`subject` 是 canonical form 的一部分，`payload.rs` 顶部文档同时写明 step 1 没有 `parameters` 列，
「adding one later means adding a column and a migration」。因此：

- 把目标写进 `subject` ⇒ 目标进入 `field(subject)` ⇒ 进入 `payload_hash`；
- `request()` 存下这个 hash，`approve()`（`decide` → `require_payload_unchanged`）在执行前重算并比对，
  任何对 subject 的改写都会以 `AUTHORIZATION_PAYLOAD_CHANGED` 被拒绝；
- 执行面再用同一个 subject 做精确匹配，等于「执行目标 = 批准目标」有了两步证明：
  数据库层（hash）与执行层（字符串匹配）。

也就是说：**`subject` 就是本 action 的 parameters**。不需要新列、不需要迁移、不需要改
`payload_hash` 的 canonical form（改了会让所有历史 hash 失效）。这正是 `tasks/T21.md` 选定的修法。

## 3. 格式定义

```
subject = "{token_id}:{target_scopes}"
```

- `token_id`：十进制正整数，无前导零。`0`、`-3`、`007`、`1.5`、`+10`、空串全部拒绝。
  实现用 `parse::<u64>()` + `id.to_string() == token_part` 做往返校验，再转 `i64`（防止超出 bigint）。
- `:`：第一个冒号是分隔符。scopes 自身含冒号（`repo:write`），所以用 `split_once(':')`，
  而不是按冒号切段。
- `target_scopes`：`pat_scope::normalize_scopes` 的输出形式 —— 逗号分隔、无空格、
  按 `ALLOWED_SCOPES` 顺序（`repo` → `repo:write` → `repo:collab` → `repo:read`）。
  额外要求 **输入必须已经等于规范化结果**：`"repo:read,repo:write"` 这种换序写法在解析阶段就是
  `invalid_input`，不会存进票据。理由见第 4 节。
- `target_scopes` 还必须通过 `agent::validate_agent_token_scopes`（只允许 repo 系列）。

示例：`10:repo:write`、`42:repo:write,repo:read`。

解析只有一个出口：`scope_expand::parse_subject`（`pub(crate)`）。`evaluator` 在申请/投票时用它，
`consume_scope_expand` 在执行时用它，三处不会出现两套格式。

## 4. 精确匹配与语义校验的分工

`consume_scope_expand` 里现在是两道门，**都必须通过**：

| 顺序 | 校验 | 作用 | 失败 |
| --- | --- | --- | --- |
| 1 | `request.subject == "{token_id}:{normalize(target_scopes)}"` | 证明执行目标就是批准目标 | `invalid_input` |
| 2 | `widening_of(token.scopes, target)` | 证明目标确实覆盖当前 scopes、确实更宽、不含 `user`/`admin` | `invalid_input` |

- 第 1 道门是本次补丁；第 2 道门是 T20 的语义校验，**原样保留**。两者不是替代关系：
  - 只有第 2 道门 ⇒ `repo` 可以兑换 `repo:write` 的批准（本补丁要堵的洞）；
  - 只有第 1 道门 ⇒ 一张申请时就写了「不扩、只平移」的票据（例如 `11:repo:read`，当前也是
    `repo:read`）会被执行，因为 subject 自洽；第 2 道门拒绝它。
- 为什么第 1 道门要求票据里的 scopes 是规范形式：如果允许 `"10:repo:read,repo:write"` 入库，
  执行侧拿 `target_scopes` 规范化后得到 `"10:repo:write,repo:read"`，字符串就永远对不上，
  同一语义出现两种拼写、一张票无法兑换。要求 canonical 形式让「批准目标」与「可执行目标」
  一一对应，也让 hash 绑定的是唯一拼写。
- 错误码：两道门都用 `invalid_input`（400），不用 `SCOPES_CHANGED`。`SCOPES_CHANGED`（409）在 T20 里
  专指「批准后 token 的 scopes 被别的写入改掉了」这一并发冲突；把「执行参数与批准目标不符」也塞进
  同一个码会让审计分不清是并发还是越权尝试。目标不匹配是调用方错误，400 更准确。

执行顺序上，subject 结构校验与精确匹配都放在读 token 之前：一个明显对不上的 target 不必进数据库。
`widening_of` 仍在拿到 token 之后，因为它需要 token 当前的 scopes。

## 5. 一个容易踩的坑：`ensure_decider_is_not_the_agent_owner`

T20 的这个守卫从 `request.subject.parse::<i64>()` 取 token id，失败就 `return Ok(())`。
subject 改成 `"10:repo:write"` 之后，这个解析必然失败 —— 如果不动它，「agent 的主人不能批准自己 agent
的扩权」会**静默失效**（fail open）。本任务改成 `parse_subject(&request.subject)?`：
解析失败直接报错，不会跳过守卫。`decide` 在它之前已经跑过 `evaluate`，所以真正会造成失败的是
畸形 subject，而畸形 subject 本来就不该进入决策。

## 6. 对历史票据的影响

`payload_hash` / canonical form / 迁移都没动，其它 action 的 subject 语义也没动。唯一变化是
`scope_expand` 的 subject 形状。T20 尚未有 HTTP 路由（`rg-http` 无 authorization 端点，本任务也禁止新增），
因此不存在已签发的 `scope_expand` 票据。若有测试环境的旧行（纯数字 subject）处于 `pending`，
`evaluate` 会在 `parse_subject` 处以 `invalid_input` 拒绝，无法被批准或执行 —— 是 fail-closed，
不会错误放行。重建一张新票即可。

## 7. 测试设计（5 条必测，第 1 条是核心）

1. `the_target_scope_cannot_be_widened_beyond_what_was_approved`：申请 `10:repo:write`、批准后执行
   `target_scopes="repo"` → 拒绝，token 仍 `repo:read`、票据仍 `pending`；随后用批准时的
   `repo:write` 仍能成功（拒绝没有烧票）。
2. `the_target_scope_cannot_be_narrowed_either`：同一张 `10:repo:write` 票执行 `repo:read` / `repo:collab`
   → 拒绝，token 与票据状态不变。
3. `a_scope_expand_widens_the_same_token_in_place`：既有成功路径改造成新格式后仍绿（`10:repo:write`）。
4. `the_payload_hash_covers_the_target_scope`：直接断言
   `payload_hash(.., "10:repo:write", ..) != payload_hash(.., "10:repo", ..)`（并断言同输入相等）。
5. 格式校验：`a_scope_expand_subject_binds_the_token_id_and_the_target_scope` 与 `scope_expand.rs`
   的单元测试，覆盖 `"10"` / `"10:"` / `":repo:write"` / `"10:repo:write:extra"` / `"0:..."` /
   `"-3:..."` / `"007:..."` / `"1.5:..."` / `"+10:..."` / `"10:user"` / 非 canonical 顺序 → 全部
   `invalid_input`。

附加：`narrowing_or_moving_a_token_sideways_is_refused` 增加「subject 完全匹配但目标不更宽」的用例
（`11:repo:read`），证明第 2 道门仍然生效、不是被第 1 道门顶掉。

T20 的事务性测试 `a_consume_failure_rolls_back_the_scope_change` 保留，只把 subject 改成新格式，
仍然断言 `ALREADY_CONSUMED`（409）且 token 回滚。

## 8. 明确不做

- 不加 `parameters` 列、不写迁移、不改 `payload_hash` canonical form；
- 不改 `payload.rs`；
- 不改其它 action 的 subject 语义（`protected_branch_merge` 等仍绑对象 id）；
- 不做缩权 API、不做 `user`/`admin` 级扩权；
- 不新增 HTTP 路由（`rg-http` 一行未动）。
