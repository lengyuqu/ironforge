# 运维：支持边界、备份恢复与升级

> 状态（0.5.1）：单实例部署是唯一支持的形态；多实例与滚动升级**不支持**。本文是 ADR-0003 的当前版本，实测记录见 git 历史。
> 事实来源：`deploy/docker-compose.yml`、`crates/rg-db/src/lib.rs`、`crates/rg-cli/src/main.rs`、`crates/rg-core/src/background_jobs/`。

## 1. 承诺等级

| 等级 | 拓扑 | RPO | RTO | 承诺 |
|---|---|---|---|---|
| **单机（当前唯一支持）** | 1 实例 + SQLite 或 PG/MySQL | 上次成功备份 | 机器时间约百毫秒级；真实恢复是分钟级（含发现、决策、取回备份） | 服务可用性取决于宿主；不承诺自动故障转移 |
| 小团队（未验证） | 1 实例 + PG + S3 | 同上 | 同上 | 需自行验证，文档不承诺 |
| 生产多实例（不支持） | 多实例 + PG + S3 + 外部队列 | — | — | 四个状态未外置，见 §6 |

## 2. 现状事实

| 项 | 事实 |
|---|---|
| 实例数 | compose 单实例，无副本；`restart: unless-stopped` |
| DB | SQLite / PostgreSQL / MySQL 三个驱动都编进二进制，按 URL 选择；不靠 cargo feature |
| SQLite | WAL、`synchronous=NORMAL`、`busy_timeout=5s`、`foreign_keys=on`、连接池上限 5 |
| 存储 | Git 仓库与 Blob 都在本地 `repo_root`；S3 只是 Blob 的可选后端，**Git 仓库不进 S3** |
| 队列 | `background_jobs` 是持久表 + 条件 UPDATE 原子 claim，多 worker 安全；worker 本身在进程内 |
| 周期任务 | 每个实例各跑一份：镜像同步 60s、删库清理 3600s、LDAP 6h、runner watchdog 60s、审计归档 60min、outbox sweep 60s、各清理任务 |
| 登录态 | HS256 JWT，默认 7 天，无服务端 session；时钟偏差 >60s 会误判过期 |
| 迁移 | 启动时自动跑；失败即退出；没有 schema 版本门禁 |
| 限流 / 实例设置 / 权限缓存 | 都在进程内；权限缓存 30s TTL，可 `IRONFORGE_DISABLE_PERM_CACHE=1` 关闭 |

## 3. 单机失效矩阵

| # | 失效 | 现象 | 恢复 | 自动恢复 |
|---|---|---|---|---|
| F1 | DB 损坏 | 启动报 malformed，容器退出 | 用备份 restore；无备份只能重建 | 否（只会重启循环） |
| F2 | SQLite 写锁 | 写请求 5s 后 500 | 等长写结束，客户端重试 | 部分（只有建仓/wiki 有 BUSY 重试） |
| F3 | 磁盘满 | push/上传 500；同盘 DB 也开始失败 | 清盘/扩容，必要时停服防半写 | 否（无 ENOSPC 预检） |
| F4 | OOM 被杀 | 容器退出，在途请求断开 | 自动拉起；`running` job 600s 后回收 | 是（重启）+ 600s |
| F5 | 宿主重启 | 全部断开 | 自动拉起，人工验 `/health` | 是 |
| F6 | S3 不可达 | LFS/包/制品 5xx；Git 仍可用 | 恢复 S3/网络；无本地降级 | 否（有连接/请求超时，无重试） |
| F7 | 迁移中途失败 | 启动即退出，crash loop | 手工修复或恢复备份到旧版本 | 否 |
| F8 | 时钟漂移 >60s | 新 token 被判过期 | 校时 | 是（偏差回落后） |
| F9 | 只恢复 DB | API 200，`git clone` 404 | 必须 DB + data 全量恢复 | 否 |
| F10 | 备份损坏 | restore 写入前拒绝，不创建目标 | 换备份 | 否（设计如此） |

## 4. 备份与恢复

**备份集合**：DB（SQLite `VACUUM INTO` 或 PG `pg_dump`）、`/data/repos`（Git 仓库）、`/data/config`（密钥与配置）、版本/迁移清单。Blob 的 durable namespace 必须一起备（见 [data.md](design/data.md) §5）；CI Cache 与临时文件排除。

**一致性**：SQLite 在 WAL 下用 `VACUUM INTO` 生成一致快照；PG dump 不含 `repo_root`，Git 数据要单独同步。先停写或停服再备份，避免 DB 与文件目录错位。

**恢复顺序**：停服 → 恢复 Blob → 恢复 DB → 恢复 Git 仓库 → 恢复配置 → 起服前手工 `ironforge migrate` → 验活。

**什么算失败**：
- `sha256sum -c` 任一行不是 `OK`；
- `git fsck` 非 0；
- 恢复后 `/health` 非 200 或任一检查不是 `ok`；
- `migrate` 不是 `No pending migrations`；
- 登录失败，或 issue 数 / 仓库 HEAD 与备份前记录不一致。

**注意**：`restore` 不会自动跑 `migrate`，只打印提示；只恢复 DB 会出现「API 200 + clone 404」，验活不能只看 HTTP。

## 5. RPO / RTO

- 系统没有自动备份、没有 WAL 归档、没有 PITR。**RPO = 上次成功备份到故障发生的时间差**，由备份频率决定，不承诺数值。
- RTO 只报实测机器时间：SQLite 在 1.6 MB 数据集上约 145 ms（停服 2 ms + restore 26 ms + migrate 6 ms + 启动 111 ms）；PostgreSQL `pg_restore` 约 424 ms + migrate 10 ms。
- 不含发现故障、确认恢复、从异地拷回、人工决策；真实 RTO 通常是分钟级。数据量变大后不能线性外推，必须在目标数据量上重跑演练。

## 6. 单实例升级

停机窗口 = 停服 + 备份 + 换镜像 + 迁移 + 启动 + 验活。

升级前：确认备份可恢复、记录当前迁移数、确认镜像版本。
升级中：停服 → 备份 → 换镜像 → 启动（自动迁移）→ 验 `/health`、登录、clone。
升级后：检查后台任务、outbox、失败 job。
回滚触发：迁移失败、`/health` 不健康、登录或 clone 失败；回滚用旧镜像 + 备份恢复，**不假设迁移可 down**。

## 7. 多实例的阻塞项

1. **DB**：SQLite 不能两实例共享；PG/MySQL 可用但 CI 只覆盖子集。
2. **仓库存储**：`/data/repos` 是本地盘；需要共享存储或每实例一份 + 路由粘滞。
3. **启动迁移**：每个实例都会跑迁移，需要版本门禁与只由一个实例执行。
4. **周期任务**：镜像同步、清理、LDAP、outbox、审计归档都会在每个实例重复触发；需要选主或分布式锁。
5. **进程内状态**：限流、实例设置、权限缓存都是单进程语义；多实例需要外置或接受不一致。

在这些项解决前，不要部署多副本，也不要把「滚动升级」写进运维承诺。
