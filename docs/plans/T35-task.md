# T35 执行记录 — HA / 滚动升级 / 故障恢复

> 计划见 `docs/plans/T35-plan.md`，权威约束见 `tasks/T35.md`。
> 基线 `64861cf9`，分支 `agent/t35-ha-drill`，worktree `../ironforge-t35`。

## 1. 开工核对

```
$ git branch --show-current
agent/t35-ha-drill
$ git log --oneline -1
64861cf9 chore(tasks): seed batch 4 (T32-T36) for the four production gaps
$ docker version --format '{{.Server.Version}}'
29.9.3
```

## 2. 改动清单（零代码、零 deploy 改动）

| 文件 | 改动 |
|---|---|
| `docs/plans/T35-adr-ha.md` | **新建**：ADR-0003，失效矩阵 10 条、双实例 9 个阻塞项、三档支持等级 |
| `docs/plans/T35-drill-backup-restore.md` | **新建**：可执行演练手册 + SQLite/PG 实测输出与耗时 |
| `docs/plans/T35-plan.md` | **新建**：执行计划与任务卡偏差 |
| `docs/plans/T35-task.md` | **新建**：本文件 |
| `tasks/T35.md` | `State` → `completed` |

未改：`deploy/`（逐字节未动）、任何 `.rs`、`Cargo.*`、`scripts/`、`migrations/`、`.github/`、`.cnb.yml`、
`CLAUDE.md`、`ironforge-docs/`、`TASKS.md`。

## 3. 实测：SQLite 全流程

容器 `t35-drill`（镜像 `ironforge:local-main-7d27c2d`，数据卷 `t35-drill-data`），
数据：1 用户 / 1 仓库 / 1 issue / 1 协作 session / 2 个 commit / 18 个仓库文件。

| 里程碑 | 实测 |
|---|---:|
| 冷启动到 `/health` 200 | 322 ms |
| 全量备份（1.6 MB，19 文件） | 29 ms |
| 停服 | 2 ms |
| 恢复（含 19 文件校验 + 自动 quiesce） | 26 ms |
| `migrate`（no-op） | 6 ms |
| 启动到 `/health` 200 | 111 ms |
| 在线 `backup-db`（`VACUUM INTO`） | 15 ms，`integrity_check=ok` |
| DB-only 恢复（反例） | 18 ms → `issues=200` 但 `clone_exit=128` |

关键证据（原文）：

```
create_repo=201
create_issue=201
create_collab_session=201
pushed_head=d48955f3815b1cd7836c12113a3221184d5e2775
db_bytes=917504
repo_objects=18 files
Verified 19 files against checksums.sha256
checksums: OK
git fsck: OK
restored_head=d48955f3815b1cd7836c12113a3221184d5e2775
list_issues=200
clone_after_restore=OK
```

损坏备份被拒、且不创建目标：

```
1: error returned from database: (code: 11) database disk image is malformed
refused_target_exists=no
```

## 4. 实测：PostgreSQL dump/restore

同一台机器上的 `postgres:16`，用镜像里的 `ironforge migrate` 建 schema，API 造数后 dump：

| 里程碑 | 实测 |
|---|---:|
| 首次 migrate（PG，97 条迁移） | 405 ms |
| `pg_dump -Fc` | 69 ms，334,188 B |
| `pg_restore` 到新库 | 424 ms |
| 表数量 / 表名列表 | 105 / 完全一致 |
| users、repositories、issues、agent_sessions | 1、1、1、1（两边一致） |
| 迁移历史 | 97 = 97 |
| `migrate`（恢复库） | `No pending migrations`，10 ms |

## 5. 实测：post-restore quiesce

CLI 自动（SQLite，默认 restore）：

```
collaboration sessions ended : 1 (clients must register new sessions)
```

手工 `deploy/post-restore-quiesce.sql`（SQLite `--keep-live-state`）：

```
sessions_before=1
sessions_after=0
reset_tokens_after=0
live_ci_after=0
queued_outbound_after=0
```

手工 `psql -f`（PostgreSQL 恢复库）：

```
live_sessions_before=1
quiesce_sql=applied
live_sessions_after=0
live_rows_after=0
```

## 6. 实测中遇到的问题

1. `backup` / `restore` / `restore-db` 的路径是**位置参数**，不是 `--output`/`--input`；
   第一次按 `--output` 写直接报 `unexpected argument`。
2. 镜像构建于 2026-11-09，只认识 **97** 条迁移；源码 HEAD 有 **112** 条。演练未覆盖最近 15 条迁移，
   已在 drill §0 与 §10 明写。
3. 应用镜像没有 `sqlite3` / `python3`；手工验证 quiesce 需要在容器内 `apt-get install sqlite3`
   （非 root 会因 apt 目录权限失败，需 `-u 0`）。
4. `restore` 目标已存在时必须 `--force`，且 `--force` 会删 `-wal`/`-shm`；不停服时会被拒绝。
5. 只恢复 DB 会造成「API 200 + clone 404」；验活必须包含 `git clone` 与 `git fsck`。
6. `restore` 不会自动 `migrate`，只打印提示；SQLite 的 quiesce 在 restore 内部完成。

## 7. 发现的「文档/任务卡说支持但实际不符」

| 项 | 说明 |
|---|---|
| `db-postgres` / `db-mysql` feature | 任务卡与部分说明暗示有这两个 feature；实际 `Cargo.toml:43` 默认把三后端全部编进二进制，**feature 不存在** |
| 迁移历史表名 | 任务卡写 `_sea_orm_migration_history`；实际 `seaql_migrations`（`main.rs:866`、drill 脚本 `scripts/db-restore-drill.sh:115`） |
| 多实例限制 | `deploy/README.md:53-65` 只列了权限缓存/限流/实例设置三项；实际还缺 repo 存储、周期任务选主、镜像同步 claim、审计归档、启动迁移竞争（ADR §4.2 共 9 项） |
| 全量 `backup` 要求停服 | README 说「Stop the server first」，但代码不检查；运行中执行会产出不一致备份（ADR §2、drill §1） |
| `/health` 的 filesystem 检查 | 只 `read_dir`，磁盘满/只读挂载仍报 `ok`（`lib.rs:2118`） |

## 8. 需要协调者裁决（ADR §7 摘要）

1. **D1 Git 仓库存储是否外置**（共享 POSIX + 租约 / 对象存储网关 / 维持单实例）——多实例的头号阻塞。
2. **D2 选主实现**（DB 租约表 / Redis / 外置调度器）。
3. **D3 迁移是否从启动路径拆出**（独立命令 vs advisory lock）。
4. **D4 是否提供 `migrate --down`**（建议不提供，回滚靠备份）。
5. **D5 小团队档是否内置备份调度**（建议只给 cron 模板）。
6. **D6 MySQL 是否与 PG 同级支持**（本轮未实测 MySQL）。
7. **D7 时钟偏差容忍度**（60s 默认是否显式化，并在 `/health` 暴露偏移）。

## 9. 验收对照（任务卡「验证」小节）

| # | 要求 | 结果 |
|---|---|---|
| 1 | 失效矩阵每条有 `file:line` 或配置片段 | ✅ ADR §3 共 10 条，每条有证据列 |
| 2 | 备份恢复演练实际执行过，有真实输出与耗时 | ✅ drill §5（SQLite + PG），本文件 §3/§4 |
| 3 | 多实例前置项每项有 grep 证据与改造量 | ✅ ADR §4.2/§5，9 项 + 4 个硬阻塞 |
| 4 | RPO/RTO 可追溯到 §2 实测 | ✅ drill §8 只用实测数字，并明写「不外推」 |
| 5 | 不支持的地方明写不支持 | ✅ ADR §8、drill §10 |

## 10. 遗留

- 用 HEAD（112 条迁移）构建镜像后复跑一次 drill，覆盖最近 15 条迁移。
- 在 Linux CI 跑 `scripts/collab-e2e.py --scenario restore`（更细的 14 个协作视图对比）。
- 跑 `scripts/db-restore-drill.sh mysql`，把 MySQL 从「未实测」变「已实测」。
