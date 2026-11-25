# ADR-0003：HA、滚动升级与故障恢复的支持边界

- Status: **Accepted**（2026-11-25 协调者裁决；本轮不改 `deploy/` 与代码，实现另开批次）
- Date: 2026-11-25
- Task: T35（`tasks/T35.md`）
- Evidence: `docs/plans/T35-drill-backup-restore.md`（本机 Docker 实测：SQLite 全流程 + PostgreSQL dump/restore）
- Baseline: `64861cf9`，worktree `../ironforge-t35`

## 1. 决策摘要

**当前只承诺「单实例 + 定期离线全量备份」这一档。** 双实例滚动升级与多活拓扑**不支持**，原因不是
「没上 K8s」，而是四个具体状态没有外置：SQLite 或单写库、`/data/repos` 本地盘、启动时迁移、
以及每个实例各跑一份的周期任务。

本文给出三档支持等级、9 条单机失效模式（每条有代码证据）、单实例升级 checklist、
双实例的 9 个阻塞项与改造量，以及 4 个必须由协调者裁决的架构决策。

## 2. 现状（复核过的代码，不是文档转述）

| 项 | 事实 | 证据 |
|---|---|---|
| 实例数 | 单个 `ironforge` 服务；compose 无副本、无 `deploy.replicas` | `deploy/docker-compose.yml:13-38`；`restart: unless-stopped` 在 `:20` |
| DB | **三后端默认全部编译进二进制**，运行时按 URL 选择；无 `db-postgres`/`db-mysql` cargo feature | `Cargo.toml:43`（正确）；`crates/rg-db/src/lib.rs:182-184` 的注释**写错了**，它说「gated behind the `db-postgres` / `db-mysql` cargo features so the default binary stays SQLite-only」，与 `Cargo.toml:43` 和实际依赖相反。实现者以 `Cargo.toml` 为准 |
| SQLite 参数 | WAL、`synchronous=NORMAL`、`busy_timeout=5s`、`foreign_keys=on`、池上限 5 | `crates/rg-db/src/lib.rs:220-234` |
| 存储 | Git 仓库与 blob（LFS/packages/OCI/artifacts/attachments）都在 `repo_root` 本地盘；S3 只是 blob 的可选后端，Git 仓库不进 S3 | `Dockerfile` CMD `--repo-root /data/repos`；`crates/rg-core/src/blob_storage/s3.rs:1-44` |
| 队列 | `background_jobs` 是**持久表 + 条件 UPDATE 原子 claim**，设计上多 worker 安全；worker 本身在进程内 | `crates/rg-core/src/background_jobs/mod.rs:1-18`、`:100-108` |
| 周期任务 | 每个实例各起一份：镜像同步 60s、删库清理 3600s、LDAP 6h；另有 runner watchdog 60s、审计归档 60min、outbox sweep 60s、repo_events/idempotency/ci_retention/agent_drafts 清理 | `crates/rg-http/src/lib.rs:275-339`、`:4269-4289`、`:4297-4306`；`crates/rg-core/src/audit/archiver.rs:57-67` |
| 登录态 | HS256 JWT，默认 7 天，无服务端 session；校验只设 issuer/audience，未覆盖 leeway | `crates/rg-core/src/auth/jwt.rs:97-108`；`jsonwebtoken 10.3.0`（默认 leeway 60s） |
| 迁移 | 启动时自动 `Migrator::up`，失败即 `?` 退出；**无 schema 版本门禁**（`known_migration_names` 只用于 restore 校验） | `crates/rg-cli/src/main.rs:2749`、`crates/rg-db/src/lib.rs:304-310`、`:164-168` |
| 迁移事务 | 没有迁移自己开事务；SQLite 表重建迁移明确拒绝在事务内运行 | `crates/rg-db/src/migrations/m20261015_000002_repositories_partial_unique_index.rs:168-172` |
| 限流 | 进程内 `Mutex<HashMap>`，按实例计数 | `crates/rg-http/src/rate_limit.rs:35-49` |
| 实例设置 | 进程内 `RwLock<InstanceSettings>`（维护模式/横幅） | `crates/rg-http/src/instance.rs:25` |
| 权限缓存 | 30s TTL，可用 `IRONFORGE_DISABLE_PERM_CACHE=1` 关闭 | `deploy/README.md:60` |

## 3. 单机失效矩阵

| # | 失效模式 | 现象 | 数据影响 | 恢复动作 | 会自动恢复吗 | 证据 |
|---|---|---|---|---|---|---|
| F1 | DB 文件损坏 | 启动时迁移/查询报 `database disk image is malformed`，容器退出 | 全库（Git 仓库文件仍在） | `ironforge restore-db` 或全量 `restore`；无备份则只能重建 | **否**，`restart: unless-stopped` 只会循环重启 | 实测见 drill §5（损坏副本被拒收）；`rg-http/src/error.rs:203-208` 映射为 500 |
| F2 | DB 锁死（WAL 写并发） | 写请求等 5s 后 `SQLITE_BUSY` → 500 | 无损坏；未提交事务回滚 | 等待长写结束；客户端重试 | **部分**：仅建仓与 wiki 写有 50 次 BUSY 重试，其余直接 500 | `rg-db/src/lib.rs:220-234`、`:1280-1293`；调用点 `rg-http/src/api/repos.rs:614`、`rg-core/src/wiki/service.rs:158` |
| F3 | `/data/repos` 磁盘满 | push/上传返回 500；DB 若同盘也开始失败 | 新对象写不进；已有数据可读 | 清盘/扩容，必要时先停服防止半写 | **否** | 没有 ENOSPC 预检；`/health` 只看目录可读：`rg-http/src/lib.rs:2118` |
| F4 | 进程 OOM 被杀 | 容器退出；在途请求断开 | 内存态（perm cache、限流桶、维护模式）丢失；DB 靠 WAL 恢复；`running` 的 job 在 600s 后回到 pending | 自动拉起，等 stale 回收 | **是**（重启）；job 回收需 600s | `docker-compose.yml:20`；`background_jobs/mod.rs:195-207`、`:314-316` |
| F5 | 宿主重启 | 全部连接断开，容器停止 | 与 F4 相同；`/data` volume 持久 | 自动拉起，人工验 `/health` | **是**（compose 策略） | `docker-compose.yml:16-26`、`:63-65`（named volume） |
| F6 | S3 不可达 | LFS/packages/artifacts 读写 5xx；Git clone/push 仍可用（走本地盘） | 只有 blob 类操作失败，DB 不受影响 | 恢复 S3/网络；无本地降级 | **否**（连接 10s、请求 120s 超时后报错，无重试） | `blob_storage/s3.rs:43-44`、`:94-96` |
| F7 | 迁移中途失败 | 启动即退出，容器 crash loop | `seaql_migrations` 只记成功项；失败的那条可能留下半成品 DDL（SQLite DDL 无事务回滚） | 手工修复或恢复备份到旧版本 | **否** | `rg-cli/src/main.rs:2749`；`rg-db/src/lib.rs:304-310`；`m20261015_000002...rs:168-172` |
| F8 | 反代/实例时钟漂移 | 偏差 >60s 时，新签发的 token 被判过期；同一 secret 下所有实例同时受影响 | 仅登录态，无数据影响 | 校时（NTP/chrony） | **是**（偏差回落后） | `jwt.rs:97-108`；`Claims.exp` 是绝对 Unix 时间；无 `nbf` |
| F9 | DB 与 repo 目录不一致（只恢复 DB） | API 能列出 issue（200），`git clone` 404 | 数据「看起来在」但 Git 对象丢失 | 必须全量 restore（DB+data 同时） | **否** | 本次实测：`list_issues=200`、`clone_exit=128`（drill §5.3） |
| F10 | 备份文件损坏 | `restore` 在写入前拒绝，目标文件不创建 | 不影响现有实例 | 换一份备份 | **否**（设计如此） | 实测：`database disk image is malformed`，`refused_target_exists=no` |

## 4. 滚动升级

### 4.1 单实例升级（当前唯一支持的形态）

停机窗口 = 停服 + 备份 + 换镜像 + 迁移 + 启动 + 验活。实测里程碑（1.6MB 数据集，见 drill）：

| 步骤 | 实测 |
|---|---:|
| 停服（SIGTERM 到进程退出） | 2 ms |
| 全量备份 | 29 ms |
| `ironforge migrate`（已是最新） | 6 ms |
| 启动到 `/health` 200 | 111 ms |

机器时间合计约 **0.15 s**；真实窗口由「镜像拉取 + 人工确认」决定，不以本文数字承诺。

**迁移能否回滚：不能通过 CLI。** `ironforge` 只有 `migrate`（`main.rs:170-174`），
没有 `migrate --down` / `rollback`；`Migrator::down` 只出现在测试里
（如 `m20261115_000001_create_agent_draft_namespaces.rs:329`）。
因此生产回滚只有一条路：**停服 → 恢复升级前的全量备份 → 用旧镜像启动**。
`down` 窗口有几步：**0 步在线回滚**；离线回滚 3 步（停服、restore、旧镜像起服）。

### 4.2 双实例滚动：先列阻塞项

| 阻塞项 | 现状证据 | 为什么阻塞 | 最小改造量 |
|---|---|---|---|
| SQLite 共享 | `journal_mode(Wal)` + `busy_timeout(5s)`（`lib.rs:220-223`），单写者 | WAL 的 `-wal`/`-shm` 需要同机同文件系统；跨主机共享（NFS）语义不受支持。即使同机两容器共享 volume，写仍串行，且迁移会竞争 | **改用 PostgreSQL**（代码已支持），不改造 SQLite |
| `/data/repos` 本地盘 | `--repo-root /data/repos`（Dockerfile CMD） | 两个实例各自本地盘 → clone 到 A、push 到 B 看不到对象；共享 NFS 又没有跨实例的仓库锁 | 共享 POSIX 文件系统 + 每仓库租约，或对象存储网关（两者都不是现有代码） |
| 启动迁移竞争 | 启动即 `Migrator::up`（`main.rs:2749`），无 advisory lock | 两实例同时启动会并发执行 DDL，可能重复建表/写 `seaql_migrations` 冲突 | 迁移前置：加 `pg_advisory_lock` 或独立的 migrate Job，且实例启动只校验不迁移 |
| 周期任务重复 | `register_periodic` 每实例各一份（`lib.rs:4269-4289`） | 镜像同步会重复 fetch；purge/LDAP/watchdog 重复执行 | 选主（DB 租约）或外置调度器 |
| 镜像同步无 claim | `list_due_sync` 后直接 `sync_mirror`，`next_sync_at` 在 fetch **之后**才写（`mirror/service.rs:386-395`、`:341-374`） | 两实例会同时对同一 mirror 做 clone/fetch 到同一路径 | `SELECT ... FOR UPDATE SKIP LOCKED` 先占坑，或单实例执行 |
| 审计归档重复 | 每实例 `time::interval(60min)`（`audit/archiver.rs:62-67`） | 重复扫描/重复写归档文件 | 选主或按 `archived_at` 原子 claim |
| 权限缓存 | 30s 进程内缓存 | 撤销在另一实例最长 30s 后才生效 | `IRONFORGE_DISABLE_PERM_CACHE=1`（已有），或共享缓存/失效广播 |
| 限流 | 进程内 HashMap（`rate_limit.rs:49`） | 实际限额 = `max × 实例数` | 共享限流器或前置换算 |
| 实例设置 | 进程内 `RwLock`（`instance.rs:25`） | 维护模式在一个实例打开，另一个仍接流量 | 放 DB 并每次读取，或广播 |

**结论：队列本身不是阻塞项**（`background_jobs` 的 claim 是原子的，多 worker 安全），
阻塞的是「仓库文件」与「周期任务」两类。

### 4.3 版本对等期

- **API**：只承诺 **1 个次版本**的新旧共存（新增字段/端点向后兼容；删除/改语义必须跨两个版本，
  先 expand 再 contract）。
- **DB schema**：只承诺 **1 个版本**的加列/加表迁移可被旧代码忽略。删列、改类型、加 NOT NULL 无默认值
  都是破坏性变更，必须两阶段。
- **启动不校验 schema 版本**：`known_migration_names` 只在 `restore` 的校验里用
  （`main.rs:866`、`:896-940`），普通启动不会因为「库比二进制新」而拒绝。
  所以**用旧镜像回滚到已被新版本迁移过的库，会静默启动**，这是当前最危险的升级路径。
  回滚前必须恢复备份（见 4.1）。

### 4.4 升级 checklist

**升级前**

1. `curl -fsS http://host:8080/health`：`database` / `filesystem` / `metrics` / `smtp` 为 `ok`，`git` 为 `builtin`。
2. 停服后跑 `ironforge backup`，确认 `checksums.sha256` 与 `manifest.json` 存在，并**把备份复制到另一块盘**。
3. 记录当前镜像 digest 与 `git rev-parse HEAD`（回滚要用旧镜像）。
4. 确认新版本的迁移只有加列/加表；若含删列，拆成两个版本。
5. 预留磁盘 ≥ 当前 `/data` 大小的 2 倍（备份 + 解压余量）。

**升级中**

6. `docker compose stop ironforge`（必须停，不能边跑边全量备份）。
7. 换镜像后 **先** `ironforge migrate --db-url ...`，再起服务；失败就停在这里，不要起服务。
8. 起服务后等 `/health` 200，再放流量。

**升级后**

9. 抽查：登录、列 issue、`git clone` 一个仓库、`git fsck`、下载一个 release/package。
10. 观察 15 分钟 5xx 率与 `database is locked` 日志。

**回滚触发条件（任一命中即回滚）**

- `/health` 任一检查非 `ok` 且 5 分钟内未恢复；
- 迁移失败（进程退出/crash loop）；
- 登录成功率明显下降或 JWT 全部失效；
- `git clone/push` 5xx 持续 >5 分钟；
- 数据校验不一致（issue 数、仓库 HEAD 与升级前记录不符）。

回滚 = 停服 → `ironforge restore --force` 恢复升级前备份 → 用旧镜像启动 → 验活。

## 5. 多实例化的前置条件（改造量）

| 前置项 | 现状 | 改造量 | 阻塞性 |
|---|---|---|---|
| DB 外置 | 三后端已编译、运行时可选（`Cargo.toml:43`）；CI 有 `scripts/ci-db-tests.sh` 覆盖 PG/MySQL 的部分套件 | 把「服务端集成测试矩阵」补到与 SQLite 同级；无需改代码 | **非阻塞**（但 SQLite 必须换掉） |
| Repo 存储外置 | Git 仓库只在本地盘；S3 仅 blob | 共享文件系统 + 跨实例仓库租约，或 Git 存储网关；工作量以「新子系统」计 | **硬阻塞** |
| 后台任务选主 | 队列 claim 原子；周期任务每实例一份 | 加 DB 租约表 + 每个周期任务先抢锁（1 张表 + 1 个封装 + 4 处调用） | **硬阻塞**（重复副作用） |
| 镜像同步 claim | 无原子 claim（`mirror/service.rs:386-395`） | `FOR UPDATE SKIP LOCKED`（PG）或 `UPDATE ... RETURNING` 占坑 | **硬阻塞** |
| 启动迁移 | 每实例启动都 `up`，无锁 | 独立 migrate 步骤 + 启动只读校验；或 advisory lock | **硬阻塞** |
| 登录态 | JWT 无状态，同一 secret 全实例可用 | 无 | 无阻塞 |
| 限流 | 进程内 | 共享限流器或前置换算；若接受 `max×N` 则零改造 | 视承诺 |
| 权限缓存 | 30s 进程内 | 关缓存（已有开关）或共享缓存 + 失效广播 | 视承诺 |
| 实例设置 | 进程内 | 落库并每请求读 | 视承诺 |
| 审计归档 | 每实例 60min | 选主 | 硬阻塞（重复归档） |

## 6. 支持等级（RPO/RTO 基于实测，不外推）

实测数据集：SQLite 917KB DB + 18–22 个仓库文件，备份目录 1.6MB；PostgreSQL dump 334KB / 105 张表（PG 数字来自旧镜像的 97 条迁移）。
数字含义见 drill §8：RTO 只算「机器执行时间」，不含人工发现、取备份、决策。

| 等级 | 拓扑 | RPO | RTO（实测基线） | 承诺 | 不承诺 |
|---|---|---|---|---|---|
| **单机（当前）** | 1 实例 + SQLite + 本地盘 | **自上次成功备份**（当前无自动备份、无 WAL 归档；备份需停服。内置定时备份已裁决要做，见 §7 D5，但**尚未实现**） | 停服 15ms + restore 42ms + migrate 25ms + 启动 146ms ≈ **0.23s**（HEAD 112 迁移，1.6MB 数据集；旧镜像 97 迁移为 0.15s，见 drill §5.6） | 进程崩溃/宿主重启后自动拉起；`backup`/`restore` 带校验与 quiesce；单机可用性由 `restart: unless-stopped` 提供 | 不承诺零停机升级、自动备份、PITR、多实例、损坏后无损恢复（取决于备份） |
| **小团队** | 1 实例 + PostgreSQL + S3 | 自上次 `pg_dump`（工具支持，**当前没有内置调度**；D5 已批准内置定时备份，未实现） | `pg_dump` 69ms + `pg_restore` 424ms + migrate 10ms ≈ **0.5s**（334KB dump）；启动未计入 | DB 可外置、blob 可进 S3、dump/restore 往返经过实测；**PostgreSQL 是首选后端，MySQL 为 best-effort**（D6） | 不承诺 Git 仓库外置（仍在本地盘）、不承诺自动备份、不承诺 HA、不承诺 MySQL 与 PG 同级（MySQL 的 dump/restore 未实测，见 drill §10） |
| **生产** | 多实例 + PG + 共享 repo 存储 + 选主 | **不支持**（无可承诺值） | **不支持** | 无 | 全部；先完成 §5 的 4 个硬阻塞项。**本轮明确不做 Git 存储外置（D1），因此多实例在可预见的版本内都不支持** |

> **上表 RTO 的测量环境（决策时必须一起看）**：`0.23s` 那一行是 HEAD 二进制在 **macOS 宿主**直接运行、
> 数据目录为宿主本地盘测得的；**容器内数字未测量**——overlayfs/volume 的 I/O 语义、`ironforge` 非 root
> 运行用户、`HEALTHCHECK`、镜像层前端资源均未覆盖。真实容器拓扑的 RTO 应视为「上表 + 容器启动开销」，
> **当前数字方向偏乐观**。小团队行的 69/424/10ms 来自旧镜像（97 迁移）的容器内 `pg_dump`/`pg_restore`，
> 与 HEAD 不同版本，不能与 `0.23s` 直接相加或比较。
>
> **D3 落地后 RTO 构成会变**：`migrate`（25ms）移出启动路径，不再是 RTO 的一部分，而是显式前置步骤；
> 启动会新增一次 schema 版本校验（预计 ms 级）。**不要拿当前公式（停服 + restore + migrate + 启动）
> 去套 D3 之后的新拓扑**。

## 7. 裁决记录（2026-11-25，余曲）

| ID | 问题 | 裁决 | 落地要求 |
|---|---|---|---|
| D1 | Git 仓库存储是否外置 | **本轮不做** | 对象存储 + git 层改造是季度级工程。支持等级只到「单机 / 小团队」；**ADR 与 README 都必须明写「不支持多实例」及其代价**（无故障转移、升级有停机、容量受单机限制） |
| D2 | 多实例选主实现 | **DB 租约** | 复用 PG 与 `background_jobs` 的原子 claim 思路；不引 Redis（新依赖 + 新故障面）。实现时先做 `task_leases(name, owner, expires_at)` 与一个 `try_lease` 封装，周期任务逐个接入 |
| D3 | 迁移是否从启动路径拆出 | **是** | `serve` 不再隐式迁移：启动时只校验 schema 版本（缺迁移就拒绝启动并提示）；`ironforge migrate` 作为显式前置步骤（compose init/Job 或运维命令）。**RTO 构成随之变化：migrate 移出 RTO，启动新增 schema 校验；旧公式作废** |
| D4 | 是否提供 `migrate --down` | **不提供** | 回滚靠备份 + 旧镜像。SeaORM 的 `down` 在三后端上没人验证过，半可靠的回滚比没有更危险 |
| D5 | 小团队档是否内置备份调度 | **内置，但范围要小** | 定时 `VACUUM INTO`（SQLite）/ `pg_dump`（PG）+ 保留 N 份 + 每次备份后跑一次恢复校验；不做 PITR、不做异地复制。**当前未实现**，文档不得写成已支持 |
| D6 | MySQL 是否与 PG 同级 | **不同级** | PostgreSQL 优先；MySQL 降级为 best-effort（只保证 `migrate` 可用，dump/restore 未实测）。README/支持表不得把两者并列 |
| D7 | 时钟偏差容忍度 | **显式化 ±60s** | JWT `exp`/`iat` 允许 ±60s；文档要求多实例部署必须 NTP 同步；`/health` 增加实例时间与数据库时间的偏移（实现任务做，本轮只写要求） |

## 8. 本文明确「不支持」的项

- 双实例/多活（任何组合），包括滚动升级时的短暂双实例；
- 零停机升级；
- 自动备份、PITR、WAL 归档；
- SQLite 在线（不停服）全量备份——`backup` 要求停服；只有 `backup-db`（`VACUUM INTO`）可在线；
- PostgreSQL/MySQL 的 Git 仓库备份（CLI 只备份 SQLite 的 DB；PG/MySQL 需 `pg_dump`/`mysqldump` + 单独拷 `repo_root`）；
- 自动故障转移、只读副本、跨机房；
- MySQL 与 PostgreSQL 同等级支持（D6：PG 优先，MySQL best-effort，dump/restore 未实测）；
- 内置定时备份（D5 已批准实现，但当前代码里没有）。
