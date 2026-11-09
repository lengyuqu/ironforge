# 协作健康基线：「已建立」有了自己的记录（验证记录，2026-11-09）

> 解决 CLAUDE.md「协作 Web UI（首版）」一行里记的后端待定项：健康基线在新增 0 条时 `taken_at` 为空，页面显示「尚未建立」，与同页的基线报告矛盾。

## 一、问题

基线过去只以「给没有事件的对象写一条观察事件」的形式存在。仓库里没有这样的对象（空仓库，或每个对象都已有事件）时，`POST …/health/baseline` 成功返回 `added: 0`，却没留下任何痕迹，`GET …/health` 的 `baseline.taken_at` 仍是 `null`。

## 二、改动

- 迁移 `m20261109_000001_create_collab_health_baselines`：表 `collab_health_baselines(repo_id PK, taken_at)`，`repo_id` 外键到 `repositories`（级联删除），`up` 幂等、`down` 删表。实体 `collab_health_baseline`。
- `collab_metric_ops::take_baseline` 末尾记录这一行（先查再插，唯一键冲突视为别人抢先；不用 `ON CONFLICT DO NOTHING`，CLAUDE.md 踩坑 19）。
- `baseline_taken_at` 取该行与旧的基线事件中**较早者**：本迁移之前已建立的基线只有事件，仍被识别。
- `rg-db` 的软删除回滚测试里 `Migrator::down` 的迁移数从 23 改为 24（该测试注释要求追加迁移时同步）。

**刻意没改**：覆盖起点（`coverage_start`）仍是第一条事件。基线只表示「观察开始了」，不为指标发明起点，所以空仓库建基线后各指标仍是 `unknown`（`NO_EVENTS_BEFORE_WINDOW_END`），直到出现第一个事件。我先试过用一条无任务的标记事件，被 `(task_id, repo_id)` 外键拒绝（500），因此改成独立的表。

## 三、证据

| 层级 | 结果 |
|---|---|
| 任务验证（SQLite） | `collab_health_tests` 17 项（新增 `m08_a_baseline_with_nothing_to_observe_is_still_a_baseline`：空仓库建基线前 `taken_at` 为 null，之后为字符串；重复调用 `existed_before` 为 true、`taken_at` 不变、该仓库只有一行；覆盖起点仍为 null）；原有基线测试（含五个并发基线、被拒绝的基线不写任何东西）全部通过；`rg-db` 27 项。 |
| 后端验证 | `collab_health_tests` 17 项在 PostgreSQL 17 与 MySQL 8.4（全新数据库）通过（迁移与外键在两个后端上建出）。第一次在 MySQL 上跑时新测试有 1 项失败，是**测试**数了整张表（同文件的测试共用一个库），已改为只数本仓库的行。 |
| 全量（SQLite） | 见 PR 描述。 |

## 四、未覆盖

- Web 页面没有重新看过（后端字段从 null 变成时间字符串，页面逻辑本来就按「有无 `taken_at`」分支）；没做浏览器验证。
- `scripts/collab-e2e.py --scenario restore` 的 27 个视图没有加入新表（备份是整库，新表随之备份/恢复；`scripts/db-restore-drill.sh` 按 CLAUDE.md 的说明逐表对比，应会覆盖，但没有重跑）。
