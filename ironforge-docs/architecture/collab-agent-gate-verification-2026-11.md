# 协作用户门只服务人类（issue #34 / T22）：落地与验证

> **状态**：已落地 ｜ **可信度**：确认（本机测试），未经 CI ｜ **日期**：2026-11-12
> 前置核实见 [collab-user-door-verification-2026-11.md](collab-user-door-verification-2026-11.md)；方案见设计文档 §2.3 条 9 / §4 T22。

## 做了什么

| 位置 | 行为 | 拒绝时的 `error.reason` |
|---|---|---|
| `api/collab.rs::write_actor`（15 个调用点，7 个文件） | 请求不带会话头时，账号是 agent → 403；带会话头走原来的会话校验 | `AGENT_MUST_USE_SESSION` |
| `api/collab_conditions.rs::confirm_condition` | 人作出的确认：agent 一律 403，**带会话也不行** | `AGENT_MAY_NOT_CONFIRM` |
| `api/collab_takeovers.rs::reassign_task` | 用户重指派：agent 403（agent 移交工作的路径是为自己的会话 claim） | `AGENT_MAY_NOT_REASSIGN` |

共用 `collab::ensure_person`：账号读不到、不存在都按拒绝处理（看不清账号类型不能当成"是人"）。错误形态照 `AGENT_MAY_NOT_APPROVE`：`code = FORBIDDEN`，稳定码放 `reason`。

## 刻意没做

- **`mark_user_read`（`PUT …/inbox/user/read`）保留**，agent 仍可调用（issue 正文如此定）。它直接构造 `Actor::user`，不经 `write_actor`；它只写已读标记。若要收紧是单独的决定（R34-2）。
- 没有新增 `Actor::agent` 变体，没有缓存账号类型（每次协作写多一次 `users` 查询）。
- 没有改读路径：`require_collab_read` 不动，agent 仍可读协作数据。
- `create_session` 刻意不拦：它是签发会话的入口。

## 验证（本机，SQLite）

`crates/rg-http/tests/collab_agent_gate_tests.rs`，6 项。修复前的现象：agent 令牌无会话头创建问题，成功（201），记录为 `creator_kind: user`、`creator_id` 是 agent 的账号 id；`confirm_condition` 与 `reassign` 也都成功。修复后：

- agent 无会话头：提问 / 创建工作包 / 写完成条件 → 403 `AGENT_MUST_USE_SESSION`，且没有任何问题被写入
- agent 带自己的会话 → 提问 201，`creator_kind = session`
- 人类无会话头 → 写完成条件 201（回归）
- agent 读任务、问题、完成状态 → 200
- `confirm_condition`：agent 无会话与有会话都 403，人 201
- `reassign`：agent 403，人 201

## 未覆盖

- `write_actor` 的 15 个调用点只测了其中 3 个文件（questions、work_packages、conditions）的写；其余靠它们共用同一函数。
- PostgreSQL / MySQL 没跑（本次没有新增 SQL，只多一次 `users` 主键查询）。
- 没有 CI；Web UI 与 MCP 未验证（MCP 工具走会话，预期不受影响，未单独测）。
