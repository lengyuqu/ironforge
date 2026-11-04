# 多 Agent 协作运维三件套（O06 任务模板 / O07 能力声明 / O08 预算用量）：实现与验证记录

> 日期：2026-11-04。范围：[运维需求](agent-collaboration-operability-requirements-2026-11.md) §6 的 O06、O07、O08。证据按[分层](../../CLAUDE.md)逐层陈述，前层不替代后层。

## 1. 实现了什么

- 迁移 `m20261104_000008_create_collab_ops`：**仅新表** `collab_task_templates / collab_task_template_versions / collab_task_template_links / collab_session_declarations / collab_task_budgets / collab_task_usage`；`has_table` 守卫，复合外键保证同仓库，`down` 先删子表。未改 `collab_tasks`。`rg-db` 回滚窗口 8 → 9。
- **O06**：仓库管理员建模板；每次编辑追加不可变新版本（`latest_version` 条件 UPDATE 仲裁）。`POST …/tasks/from-template` 经链接表固定 `template_id+version`，保存快照并把目标/交付/清单渲染进任务描述；可选 `rules_ref` 解析为 `rules_commit`（复用 O01 `rules::discover`，无法解析为 400）。`GET …/tasks/{id}/template` 返回固定版本、快照及“是否已有更新版本”。
- **O07**：会话（凭据）整体替换自己的声明（`expected_version`）；`GET …/sessions/{id}/declaration`、`GET …/declarations`。响应恒带 `source=user_content`、`authorization=none`、`declared_at/age/stale`（默认 24h，`stale_after_seconds` 可调，夹在 60s..30d；会话已结束/过期也标记）。未声明＝`undeclared`（未知，不是“不能”）。
- **O08**：`PUT …/tasks/{id}/budgets`、`GET …/tasks/{id}/budget`、`POST|GET …/tasks/{id}/usage`。unit/source/sampling_period 必填；金额为十进制文本、按整数微单位相加；**只在完全相同的 (unit, source) 内求和**；无用量＝`unknown`+原因、`total:null`、`over_threshold:null`；`over_threshold` 带 `threshold` 数字依据；不匹配任何预算的用量列在 `unmatched_usage`。**不暂停、不阻止任何工作**，仅信息性。
- MCP：`collab_create_template/list_templates/get_template/task_from_template`、`collab_declare_capabilities/get_declaration`、`collab_set_budget/report_usage/get_budget`（共 37 个 `collab_*`）。

## 2. 与设计的取舍

| 设计 | 实际 | 原因 |
|---|---|---|
| 声明/预算 PUT 的幂等键 | 无 client id；`expected_version` 为仲裁，若库中行恰为“本请求应产生的结果”（版本=期望+1 且内容哈希相同）则按重试回放 200 | 自然键 + 版本已能保证幂等；少两列 |
| 模板“清单随任务逐项跟踪” | 清单只是文本，渲染进描述并存快照；**不**逐项跟踪、不是证据 | 逐项状态属 O04 条件，需独立模型 |
| `declared_at` | 服务端写入 | 客户端时钟不可信 |
| 预算修改历史 | 只保留当前行（含 `version`、`set_by_*`） | 用量记录是证据；历史预算需另行设计 |
| 用量更正 | 追加式，不可编辑/删除；不接受负数 | 保持记录不可变；更正按来源约定另报 |
| 单位 | 逐字比较、区分大小写，不做换算或别名 | 避免任何隐含的算术假设 |
| 上限 | 每任务预算 20、用量 2000（`BUDGET_LIMIT_REACHED` / `USAGE_LIMIT_REACHED`） | 汇总在内存中进行，需有界；检查与插入非原子，并发下可能略超 |
| 周期重叠 | 仍按简单求和，同时标 `overlapping_periods` / `total_may_double_count` | 不替调用方猜是累计值还是增量 |

**未做**：Web UI、模板删除/归档、预算删除、自动暂停（明确不在范围）、环境变量级默认阈值、模板清单逐项跟踪。

## 3. 验证

| 层级 | 结论 | 证据 |
|---|---|---|
| 任务验证（SQLite，真实 HTTP） | ✅ | `cargo test -p rg-http --test collab_ops_tests`：O06 6 项、O07 3 项、O08 5 项（编辑不改旧任务、非管理员不可改模板、过期声明被标记、缺失用量不是零、单位/来源不混算、阈值附数字、越线后任务/会话照常、权限与跨仓库 404、版本/幂等/并发竞争） |
| 并发 | ✅ SQLite | 6 路并发新版本→1 胜 5×409；6 路同键建任务→1 个任务；6 路声明/预算替换→各 1 胜；6 路同键用量→1 条 |
| 单元 | ✅ | `rg-core`：模板 3 项、预算 5 项、声明 2 项；`rg-mcp`：3 项新工具映射测试（总 16 项通过）；`rg-db --lib` 26 项通过（含表名一致性、回滚窗口） |
| 门禁 | ✅（除一项） | `cargo clippy --workspace --all-targets -- -D warnings` 无告警（首次运行报 `large_enum_variant` / `too_many_arguments`，已加 `#[allow]` 后通过）；`rg-http --lib`：84 通过、1 失败（`tls_config_tests::loads_every_private_key_format…`，用本机 LibreSSL 生成的测试密钥，报 “failed to parse private key”，与本次改动无关，但**未**在基线上复测）。修复 clippy 后**未**重跑 `collab_ops_tests`（仅加属性，未改逻辑） |
| PostgreSQL / MySQL | ❌ 未验证 | 本机无可用实例。迁移、条件 UPDATE、唯一键竞争只在 SQLite 实测；`IRONFORGE_TEST_DATABASE_URL` 可复用同一套测试 |
| 合并后集成 / CI / 真实 MCP 闭环 | ❌ 未验证 | 未推送；未新增 `scripts/collab-e2e.py` 场景；MCP 只用记录式假 API 验证 |
| 浏览器 / UI | ❌ 未验证 | 无 UI |

## 4. 其他

- 共享文件改动（均为追加）：`migrations/mod.rs`、`rg-db/src/lib.rs`（回滚窗口 8→9）、`entities/mod.rs`、`ops/mod.rs`、`collaboration/mod.rs`、`api/mod.rs`、`openapi.rs`、`rg-http/src/lib.rs`（路由）、`rg-mcp/src/collab.rs`（`TOOL_NAMES` 28→37）、`rg-mcp/tests/collab_mcp.rs`。合并时 `TOOL_NAMES` 长度与回滚窗口数字需与其他分支相加。
- 迁移只在临时 SQLite 上执行；未对任何真实数据库运行 `ironforge migrate`。
