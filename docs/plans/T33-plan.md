# T33 执行计划 — Unit 级权限：测绘与 ADR

> 权威约束见 `tasks/T33.md`。基线 `64861cf9`，分支 `agent/t33-unit-permission`，worktree `../ironforge-t33`。
> 实际产出与验收输出见 `docs/plans/T33-task.md`。

## 1. 边界与产出物

本任务是**只出文档**的勘察任务，理由写在 `PLANNING.md` 的两条决策里：

- 2026-11-25：「T33（Unit 权限）与 T35（HA）**只出文档不改码**」——前者属 L2 认证语义变更，
  且必改 `crates/rg-core/src/auth/route_meta.rs`（共享禁区）与 `crates/rg-db/src/migrations/`（序号撞车）。
- `PLANNING.md:27`：「认证/权限语义变更……一律先记账、不动码」。

产出四个文件：

| 文件 | 作用 |
|---|---|
| `docs/plans/T33-authorization-map.md` | 95 行逐条测绘（含 `sed -n` 实证），供实现者核对 |
| `docs/plans/T33-adr-unit-permission.md` | 决策文档：unit 集合、兼容映射、表结构、迁移、子任务拆分 |
| `docs/plans/T33-plan.md` | 本文件 |
| `docs/plans/T33-task.md` | 执行记录与验收输出 |

`tasks/T33.md` 只改 `State`；`TASKS.md` 不动。

## 2. 执行步骤

1. 用任务卡给的同一条 grep 取 95 个命中，**先分类再写表**：区分调用/定义/内部委派/注释/导入/日志。
   这一步决定了后面所有工作量估计，不能跳过。
2. 逐行 `sed -n 'Np'` 打印，粘进表格最后一列；同时读所在函数上下文，判断 action 与 unit。
3. 读 `authorization/{action,evaluator,payload,scope_expand}.rs`，确认：
   - `Action` 枚举**不是** unit action（避免误扩展）；
   - `evaluator` 的 approver 谓词是整仓判断（这是 unit 化的越权点）；
   - `payload_hash` / `authorization_set_id` 只绑定 action+subject，不绑定 unit（无需改哈希）。
4. 统计 `route_meta.rs` 的 `ROUTES`：按构造器与路径前缀分组，得出「需要 unit 标注」的精确条数。
5. 设计表结构前先确认三后端的 DDL 差异（SQLite 的 `ALTER TABLE` 限制、MySQL 的索引长度与派生表别名、
   PG 的 `unnest`），选择「新表」而不是加列。
6. 设计兼容映射时以「行为等价」为第一原则：先给最宽映射（老 `write` → 8 个 unit 的 `write`），
   再让管理员主动收窄。
7. 拆子任务时按 owned paths 是否相交决定串并行；把 `attachments.rs` 这种跨 unit 文件单独指定归属。

## 3. 关键判断（供评审挑战）

| 判断 | 依据 | 影响 |
|---|---|---|
| 95 → 60 个真实调用点 | 类型分类（见 map §0.1） | 工作量估计以 60 为准 |
| unit 集合取 8 个，不抄 Gitea 的 9 个 | `external_wiki` 无对应功能；`skills/mcp` 待裁决 | 第一版迁移规模可控 |
| 新建 `repo_unit_permissions` 而非给 collaborator 加列 | 8 unit × 3 档无法用单列表达；JSON 三后端语义不一 | 回滚只需 `DROP TABLE` |
| 老 `write` 映射到全部 unit 的 `write` | 今天所有这 8 类写 handler 都只查 `can_write_repo` | 迁移不产生权限回归 |
| 读模型（组织级）本轮不动 | 与 unit 迁移耦合会让回滚面爆炸 | 单独立项（ADR D1） |
| `scope_expand` 需额外闸门 | 它扩大的是 token 全域能力，不该由 unit admin 批 | ADR D4 |
| `apply_owner_cap` 必须 unit 化 | 否则 agent 绕过 owner 的 unit 限制 | 验收必测用例 |

## 4. 风险与缓解

| 风险 | 缓解 |
|---|---|
| 缓存键不加 unit → 静默越权 | ADR §2.3 列为实现第一验收项 |
| 回填 24×N 行在大仓库上锁表 | `off/shadow/on` 三态，回填与切换解耦 |
| `route_meta.rs` 151 行手写漏填 | drift test 扩展为「unit 与路径前缀一致」断言 |
| 60 个调用点分给多人时签名冲突 | ADR 把守卫改造（S3）设为串行前置，之后按 unit 并行 |
| 文档行号漂移 | 每条附 `sed -n` 命令；map §7 给出复跑方式 |

## 5. 不做的事

- 不改任何 `.rs`、`Cargo.*`、`migrations/`、`deploy/`、`scripts/`。
- 不写迁移文件，不改 `route_meta.rs`，不给前端提 PR。
- 不把「建议」写成「已支持」；所有推断都标为「建议/待裁决」。
