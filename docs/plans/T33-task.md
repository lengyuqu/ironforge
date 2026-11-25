# T33 执行记录 — Unit 级权限测绘与 ADR

> 计划见 `docs/plans/T33-plan.md`，权威约束见 `tasks/T33.md`。
> 基线 `64861cf9`，分支 `agent/t33-unit-permission`，worktree `../ironforge-t33`。

## 1. 开工核对

```
$ git branch --show-current
agent/t33-unit-permission
$ git log --oneline -1
64861cf9 chore(tasks): seed batch 4 (T32-T36) for the four production gaps
```

## 2. 改动清单（零代码）

| 文件 | 改动 |
|---|---|
| `docs/plans/T33-authorization-map.md` | **新建**：95 行逐条测绘 + 路由统计 + 复合判断清单 |
| `docs/plans/T33-adr-unit-permission.md` | **新建**：ADR-0002，unit 模型 / 映射 / 迁移 / 拆解 |
| `docs/plans/T33-plan.md` | **新建**：执行计划 |
| `docs/plans/T33-task.md` | **新建**：本文件 |
| `tasks/T33.md` | `State` → `completed` |

未改：任何 `.rs`、`Cargo.*`、`crates/rg-db/src/migrations/`、`crates/rg-core/src/auth/route_meta.rs`、
`deploy/`、`scripts/`、`CLAUDE.md`、`ironforge-docs/`、`TASKS.md`。

## 3. 实测数字

```
$ grep -rn "can_write_repo\|can_read_repo\|can_admin_repo\|fn can_" crates/*/src/ | wc -l
      95
```

95 个 grep 命中的类型分布（从 `T33-authorization-map.md` 的表格逐行统计）：

```
{'调用': 60, '内部委派': 4, '定义': 8, '导入': 2, '注释与文档': 19, '日志': 2}
95
```

| 指标 | 数值 |
|---|---:|
| grep 原始命中 | 95 |
| 真实 handler 级判断点 | **60** |
| 与任务卡估计（95）的差异 | −36.8%（口径不同：命中含注释/定义/`use`/日志） |
| handler 侧通用守卫调用（间接） | 182 次 / 34 文件 |
| `route_meta.rs` `ROUTES` 总条数 | 266（199 `repo` + 3 `other_repos` + 27 `admin` + 38 `user`；第 529 行另有一条非路由模式） |
| 仓库级路由 | 202 |
| 需要加 unit 标注 | **151**（126 单 unit + 13 跨 unit 读 + 12 skills/mcp 待裁决） |
| 保持仓库级/组织级 | 51 |

按 unit 的调用点分布（60 个）：

```
11 code
 8 packages
 6 collaboration
 6 pulls
 4 issues
 4 actions
 4 通用守卫（所有 unit）
 4 仓库设置（mirror）
 3 issues + pulls（复合）
 2 跨 unit（按被通知对象）
 2 跨 unit（审批规则）
 1 跨 unit（所有 unit）
 1 仓库设置（模板）
 1 仓库设置（push mirror）
 1 跨 unit（通知列表）
 1 releases
 1 actions / code（提交状态）
```

`wiki` 与 `boards` 在直接调用点里为 0，全部走 `require_read`/`require_write` 间接判断；
这是「直接调用点清单不足以覆盖全部 surface」的证据，写进了 ADR §9.2 的串行改造项。

## 4. 验收对照（任务卡「验证」小节）

| # | 要求 | 结果 |
|---|---|---|
| 1 | 表格行数与实测 grep 一致，逐行可比对 | ✅ `grep -cE '^\| [0-9]+ \| \`crates/'` = 95；每行末列为 `sed -n 'Np'` 原文 |
| 2 | 每个 unit 的 read/write/admin 至少一个 handler 落点 | ✅ ADR §4 表格 8 个 unit × 3 档全部有 `file:line` |
| 3 | 兼容映射覆盖 read/write/admin，无遗漏 | ✅ ADR §5 矩阵 3×8=24 格全填 |
| 4 | ADR 有「不做会怎样」的量化 | ✅ ADR §10：6 类场景、47/60 判断点、3 个可直接复现的风险例子 |

补充抽样核验：

```
$ sed -n '516p' crates/rg-http/src/api/issues.rs
            rg_core::repo::service::can_write_repo(&state.db, &repo_model, Some(user_id))
$ sed -n '199p' crates/rg-core/src/repo/service.rs
pub async fn can_read_repo(
$ sed -n '351p' crates/rg-core/src/repo/service.rs
pub async fn can_admin_repo(
```

## 5. 本次勘察发现的、任务卡未列的事实

1. **`Action` 枚举不是 unit action。** 它是审批工单的 5 个动作（`ci_environment_release` /
   `protected_branch_merge` / `protected_branch_push` / `release_publish` / `scope_expand`）。
   实现时若直接扩展它，会把「审批」与「读权限」混成一件事。
2. **读是组织级、且团队 `read` 无效**（`repo/service.rs:195-198` 注释 + `:214-224` 实现）。
   这意味着 unit 化不会修复「组织成员可读全部私有仓库」，必须单独立项（ADR D1）。
3. **`apply_owner_cap` 是 unit 化的隐藏越权点**（`repo/service.rs:252-270`）。它用整仓
   `can_write_repo` 判断 owner 能力，若不改，agent 可越过 owner 的 unit 限制。
4. **`evaluator` 的 writers/admins 是整仓谓词**（`authorization/evaluator.rs:69-73`），
   三类 action 共用；unit 化后 `ProtectedBranchMerge` 必须落到 `pulls:write`。
5. **`route_meta.rs` 的 202 条仓库路由里，`skills`/`mcp-servers` 有 12 条**，任务卡给的
   Gitea 九 unit 对照表没有覆盖它们；需要裁决（ADR D2）。
6. **`wiki` / `boards` 没有直接授权调用点**，只走通用守卫；如果只按 grep 清单改，会漏掉它们。

## 6. 遗留与移交

- 本任务不产生可运行产物，验收靠文档比对（任务卡明确「无代码测试」）。
- 实现前需要协调者先裁决 ADR §11 的 D1–D6，其中 D5（迁移号段）是硬阻塞。
- 实现子任务共 **8 组**（S0–S7），其中 S0/S1/S2/S3/S5/S7 串行，S4 拆 8 个并行子任务，
  S6 可并行；每个子任务的 owned paths 与验收命令见 ADR §9.2。
- 建议在实现任务开工前，把 map §7 的 4 条复核命令加进 CI（防止行号漂移后文档失效）。
