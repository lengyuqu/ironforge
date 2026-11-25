# T35 执行计划 — HA / 滚动升级 / 故障恢复演练设计

> 权威约束见 `tasks/T35.md`。基线 `64861cf9`，分支 `agent/t35-ha-drill`，worktree `../ironforge-t35`。
> 实测记录见 `docs/plans/T35-drill-backup-restore.md`；结论与支持等级见 `docs/plans/T35-adr-ha.md`。

## 1. 边界

本任务只出文档，不改 `deploy/`、不改代码。理由：`PLANNING.md:25` 把 `deploy/` 列为共享禁区；
2026-11-25 决策条目明确 T33/T35「只出文档不改码」，多实例化涉及 DB/存储/队列三层决策，需协调者裁决。

## 2. 执行步骤

1. **先读现有能力，避免重复发明**：`crates/rg-cli/src/main.rs` 已有 `backup` / `restore` /
   `backup-db` / `restore-db`，`deploy/README.md` 已有备份章节与多实例限制表，`scripts/db-restore-drill.sh`
   已有 PG/MySQL 的 dump/restore 演练。文档必须建立在它们之上，而不是另写一套。
2. **核实现状事实**：逐条对照任务卡的「现状事实」表，发现并记录偏差（见 §3）。
3. **失效矩阵**：每条失效模式都要在代码里找到落点（启动路径、SQLite PRAGMA、健康检查、压缩/超时、
   队列 stale 回收、迁移事务性、JWT 校验参数），不许凭经验写「可能会」。
4. **真跑演练**：用本机 Docker 起一个真实实例，走「造数 → 停服 → backup → 删 DB 和 repo → restore →
   离线校验 → 起服验证」，记录真实耗时；再补一个 PostgreSQL 的 dump/restore。
5. **验证 quiesce**：分别验证 CLI 自动 quiesce（SQLite）与手工 `deploy/post-restore-quiesce.sql`
   （SQLite `--keep-live-state` + PostgreSQL `psql -f`），确认 live 行数归零。
6. **设计滚动升级与多实例前置条件**：先列阻塞项与证据，再给最小改造量；给不出的就写「不支持」。

## 3. 复核任务卡时发现的偏差

| 任务卡说法 | 实际 | 处理 |
|---|---|---|
| DB 可选 pg/mysql（`db-postgres`/`db-mysql` feature） | 三后端在 `Cargo.toml:43` 默认全部编译，**没有这两个 feature** | 写进 ADR §2，提醒实现者不要按不存在的 feature 写 CI |
| 「队列：进程内 background_jobs」 | 队列是**持久表 + 原子 claim**（多 worker 安全），真正进程内的是周期任务与 worker 循环 | ADR 把「队列」与「周期任务」分开评估；队列不算硬阻塞 |
| 「迁移历史表 `_sea_orm_migration_history`」 | 实际表名是 `seaql_migrations` | drill 与 ADR 用实际表名 |
| `/data/repos` 磁盘满的检测 | `/health` 只检查目录可读，不检查可写/剩余空间 | 写进 F3，明确「健康检查发现不了」 |
| 「迁移中途失败」 | 无事务包裹、无版本门禁、失败即进程退出；SQLite 重建迁移明确拒绝事务 | 写进 F7 与 §4.3 |

## 4. 关键判断

| 判断 | 依据 | 影响 |
|---|---|---|
| 只承诺单机档 | 四个硬阻塞项（SQLite/本地 repo/启动迁移/周期任务）都无现成实现 | 支持等级表只给单机 SLO |
| 全量 `backup` 必须停服 | DB 快照与文件拷贝顺序执行，代码不检查服务状态 | 演练与 checklist 都把「停服」列为硬前提 |
| `VACUUM INTO` 是在线 DB 备份的正确选择 | WAL 下一致性快照；实测 integrity ok | 文档明确禁止 `cp` |
| 回滚只能靠备份 | 无 `migrate --down`，`Migrator::down` 仅在测试 | 升级 checklist 的回滚步骤写成 restore + 旧镜像 |
| RTO 只报机器时间 | 实测数据集很小，人工时间不可测 | 明写「不外推」与容量规划要求 |

## 5. 风险

| 风险 | 缓解 |
|---|---|
| 演练镜像比 HEAD 旧 15 条迁移 | drill §0 明写；§10 列为「未实测：用 HEAD 构建镜像复跑」 |
| 有人把 145ms 当生产 RTO | drill §8 明确「不外推」，并给容量规划要求 |
| 只恢复 DB 被误判为成功 | drill §5.3 给出 200/404 反例，验收必须含 git clone/fsck |
| 多实例结论被读成「上个 K8s 就行」 | ADR §5 每项都有代码证据与改造量，明确队列不是阻塞项 |
