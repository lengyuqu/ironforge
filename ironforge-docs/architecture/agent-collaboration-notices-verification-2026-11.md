# 协作 U04 / O05 / O10：实现与验证记录（未完成）

> 日期：2026-11-04。分支 `claude/collab-notices-f38afb`。因用量上限中断，**O10 有针对性 SQLite 测试，MCP 工具未做、本文为中途记录**。

## 已实现（实现证据）

- **U04**（提交 ed836f4）：迁移 `m20261104_000005`（`collab_delivery_receipts`、`collab_notices`，唯一键 `(recipient,kind,source_type,source_id,generation)`）；回执仅目标会话可确认，不动 `collab_reads` 水位；交接/定向消息/指向会话的问题/答案/改派离开的通知与业务写入同事务（`*_with_notices` ops，旧签名保留）；`GET notices`（keyset 游标）、`POST notices/{id}/ack`、`POST …/messages/{mid}/read-receipt`、`GET …/messages/{mid}/delivery`（仅消息作者）；过期/结束会话不收通知、不转交后继；`dependency_finished` 仅常量。
- **O05**（提交 1888622）：迁移 `m20261104_000009`（`collab_escalation_settings`，无行=关闭）；管理员 `GET/PUT escalation-settings`；惰性评估（读通知/收件箱时）+ `evaluate_repo`/`evaluate_all_enabled` 供周期任务；条件结束标记 `resolved`；同事件不叠加；静默窗口（UTC）延后不丢；成员判断抽到 `collaboration::access`。
- **O10**：`collaboration/diagnostics.rs` + `GET …/diagnostics?task_id=|session_id=`，每条 finding 带 source 与 `established|unknown`。

## 取舍 / 偏差

- 不做 `supersedes_handoff_id`（按任务要求）。
- 回执接口无 `client_request_id`：主键 `(message_id,target)` 天然幂等。
- 通知的 keyset 游标只保证单次遍历；默认 `pending` 是状态查询，不会漏通知，重新读取即可恢复。
- 任务“阻塞时长”以 `updated_at` 为下界（其他编辑会推迟提醒，不会提前）。
- 配置的静默窗口按 UTC。

## 验证

| 层级 | 结论 |
|---|---|
| U04 任务验证（SQLite） | ✅ `collab_notice_tests` 12 项（含 barrier 并发去重、ack/回执竞争） |
| O05 任务验证（SQLite） | ✅ `collab_escalation_tests` 9 项 |
| 门禁 | U04、O05 提交时 `cargo clippy --workspace --all-targets -D warnings`、`rg-db`、`rg-http --lib`（OpenAPI 守卫）通过；当时 O10 代码仅运行 `cargo check`；后续针对性验证见下行 |
| O10 任务验证（SQLite） | ✅ `cargo test -p rg-http --test collab_diagnostics_tests`：4 项，覆盖访问隔离、离线、回执隐私、交接筛选及截断、完成任务和通知来源；未验证其他数据库 |
| MCP 工具（collab_get_notices/ack_notice/confirm_receipt/get_delivery/get_diagnostics） | ❌ 未做 |
| `.ai/README.md`、CLAUDE.md 表格 | ❌ 未更新 |
| PostgreSQL / MySQL | ❌ 未验证（本机无实例） |
| 合并后集成 / CI / 真实多客户端 | ❌ 未验证 |

O10 修复：通用诊断不披露消息回执；待处理交接先筛选再限量，超限时明确标记；完成任务提示先重开；通知 finding 的来源 ID 指向实际通知，不存在时为 null。
