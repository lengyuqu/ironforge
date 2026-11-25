# T35 备份与恢复演练（本机 Docker 实测）

> 任务卡：`tasks/T35.md`。本文件是**可照着执行的演练手册**，第 5 节的输出是 2026-11-25 在本机真实跑出来的。
> 配套设计（失效矩阵、支持等级）见 `docs/plans/T35-adr-ha.md`。

## 0. 演练环境与证据可信度

| 项 | 值 |
|---|---|
| 宿主 | macOS / arm64，Docker 29.9.3 |
| 应用镜像 | `ironforge:local-main-7d27c2d`，image id `sha256:257a6167…`，构建于 **2026-11-09** |
| 源码基线 | `64861cf9`（2026-11-25） |
| 容器 | `t35-drill`（应用 + git + curl；为验证手工 SQL 另装了 `sqlite3`） |
| 数据卷 | `t35-drill-data` → `/data` |
| 数据集 | 1 用户、1 仓库、1 issue、1 协作 session、2 个真实 commit（18 个仓库文件）、SQLite DB 917,504 B |
| PG 演练 | `postgres:16`，105 张表、97 条迁移历史、dump 334,188 B |

**必须说明的版本差**：镜像构建于 11-09，源码 HEAD 是 11-25。镜像内的二进制只认识 **97** 条迁移；
当前源码是 **112** 条（`grep -c 'Box::new(m' crates/rg-db/src/migrations/mod.rs`）。
所以本次演练**没有覆盖最近新增的 15 条迁移**。备份/恢复相关代码路径本身已用 `--help` 与源码逐项核对
（`backup`/`restore`/`restore-db`/`backup-db` 的参数形状与 `crates/rg-cli/src/main.rs:247-345` 一致），
但「HEAD 全量 schema 下的 restore」需要一次用 HEAD 构建镜像的复跑（见 §10）。

## 1. 备份什么，以及三者的原子性关系

`ironforge backup` 写入一个目录（`crates/rg-cli/src/main.rs:752-763`）：

```
<output>/
  manifest.json        # format_version / created_at / includes
  ironforge.db         # SQLite VACUUM INTO 快照
  data/                # repo_root 整棵拷贝：Git 裸仓库 + LFS/packages/OCI/artifacts/attachments
  audit-archive/       # 审计归档（存在才拷）
  config/              # 可选 TOML
  checksums.sha256     # 除 manifest 与自身外每个文件的 SHA-256
```

**原子性事实：数据库快照与文件拷贝是先后做的，不是一个原子操作。** 代码顺序是
`backup_sqlite_db` → `copy_dir_recursive(repo_root)` → 可选 audit/config
（`main.rs:970-1010`）。`backup_instance` **不检查服务是否已停止**——文档说「必须先停服」，
代码只是建议。因此：

- 运行中做全量 `backup`：DB 里已有、`data/` 里还没有的 commit 会变成「数据库有记录、Git 对象不存在」；
- 只恢复 DB、不恢复 `data/`：本次实测 **API 返回 200，`git clone` 返回 404**
  （见 §5.3）。这就是「单独备份 DB 但 repo 目录是旧的」的真实后果：**部分可用，比全挂更危险**。

正确的备份顺序是**停服 → 全量 `backup`**；只有 `backup-db`（纯 DB、`VACUUM INTO`）可以在线跑，
但它不包含 Git 数据，不能单独用于灾难恢复。

## 2. 一致性点：`VACUUM INTO` 还是 `sqlite3 .backup`

代码用的是 `VACUUM INTO ?`（`main.rs:679-692`）：

```rust
db.execute(Statement::from_sql_and_values(
    DatabaseBackend::Sqlite,
    "VACUUM INTO ?",
    [output_str.into()],
))
```

- WAL 模式下 `VACUUM INTO` 读的是**一致性快照**，并且会把 WAL 内容合并进输出文件；不需要停服，
  也不需要手工处理 `-wal`/`-shm`。实测：在服务运行中执行 `backup-db`，输出 `PRAGMA integrity_check`
  为 `ok`，issue 行数与源库一致（1=1），耗时 15 ms。
- `sqlite3 .backup` 同样安全，但依赖外部 CLI；镜像里没有 `sqlite3`，CLI 方案反而多一个依赖。
- **不要**直接 `cp ironforge.db`：WAL 里未 checkpoint 的事务会丢，且 `-shm` 状态不一致。

`VACUUM INTO` 的代价：它要求目标文件不存在；CLI 已在调用前 `remove_file`（`main.rs:674-677`），
所以重复备份到同一路径必须带 `--force` 或先删。

## 3. 备份步骤（可执行）

```sh
# 前提：容器已停服；下面在容器内执行
export IRONFORGE_JWT_SECRET='<your secret>'
DB='sqlite:///data/ironforge.db?mode=rwc'

# 3.1 停服（compose 部署时）
docker compose -f deploy/docker-compose.yml stop ironforge

# 3.2 全量备份（注意 output 是位置参数，不是 --output）
docker compose -f deploy/docker-compose.yml run --rm ironforge \
  ironforge backup \
    --db-url "$DB" \
    --repo-root /data/repos \
    --audit-archive-dir /data/audit-archive \
    /data/backups/full-$(date +%Y%m%d-%H%M%S)

# 3.3 把备份复制到另一块盘（同一块盘坏了备份也没了）
cp -a /data/backups/full-* /mnt/backup-disk/

# 3.4 独立校验（不依赖 CLI）
cd /data/backups/full-YYYYmmdd-HHMMSS
sha256sum -c checksums.sha256
```

预期输出（实测）：

```
Verified 19 files against checksums.sha256
...
data/drill/drill-repo.git/objects/pack/pack-….pack: OK
data/drill/drill-repo.git/refs/heads/main: OK
ironforge.db: OK
```

## 4. 恢复步骤与校验

**顺序：先恢复 DB，再恢复文件，最后 migrate，最后起服。** `restore` 的实现就是这个顺序
（`main.rs:1119-1135`），并且**在第一次写入前完成全部校验**（`verify_backup`，`main.rs:896-940`）。

```sh
# 4.1 停服
docker compose -f deploy/docker-compose.yml stop ironforge

# 4.2 恢复（目标已存在时必须 --force；--force 会覆盖 DB 与 sidecar）
docker compose -f deploy/docker-compose.yml run --rm ironforge \
  ironforge restore \
    --db-url "$DB" \
    --repo-root /data/repos \
    --audit-archive-dir /data/audit-archive \
    /data/backups/full-YYYYmmdd-HHMMSS \
    --force

# 4.3 迁移（restore 不会自动跑；通常是 no-op）
docker compose -f deploy/docker-compose.yml run --rm ironforge \
  ironforge migrate --db-url "$DB"

# 4.4 起服并验活
docker compose -f deploy/docker-compose.yml up -d ironforge
curl -fsS http://localhost:8080/health
```

预期输出（实测，节选）：

```
Verified 19 files against checksums.sha256
Restored full instance to:
  database : sqlite:///data/ironforge.db?mode=rwc
  repo root: /data/repos
Next steps:
  1. run `ironforge migrate --db-url ...` (no-op when schema matches)
  2. start the server against the restored paths
Post-restore quiesce (the restored state is older than the world):
  collaboration sessions ended : 1 (clients must register new sessions)
  ...
```

恢复成功的判定（四条全过才算成功）：

1. `restore` 打印 `Verified N files against checksums.sha256`，且 N 与备份目录的文件数一致；
2. `ironforge migrate` 打印 `No pending migrations`；
3. `git --git-dir=/data/repos/<owner>/<repo>.git fsck` 退出码 0；
4. `/health` 的 `database`/`filesystem`/`metrics`/`smtp` 均为 `ok`（`git` 恒为 `builtin`），且用一个真实账号能登录、能列出 issue、能 `git clone`。

## 5. 实测记录（2026-11-25）

### 5.1 SQLite：备份 → 删数据 → 恢复 → 验证

| 步骤 | 命令要点 | 实测结果 | 耗时 |
|---|---|---|---:|
| 冷启动（含首次迁移） | `ironforge serve ...` | `/health` 200，`version 0.2.0` | 322 ms |
| 造数 | 注册、建仓、建 issue、建协作 session、2 次 commit + push | `201/201/201`，`pushed_head=d48955f3…`，DB 917,504 B，仓库 18 个文件 | — |
| 停服 | `kill` + `wait` | 退出码 0 | 2 ms |
| 全量备份 | `ironforge backup ...` | 目录 1.6 MB，19 个文件，`checksums` 全 OK | 29 ms |
| 破坏 | `rm -f ironforge.db*; rm -rf repos/*` | `db_exists=no`，`repo_files=0` | — |
| 恢复 | `ironforge restore ... --force` | 校验 19 个文件；quiesce 结束 1 个 session | 26 ms |
| 离线校验 | `sha256sum -c`、`git fsck` | `checksums: OK`、`git fsck: OK`、`restored_head=d48955f3…` | — |
| 起服验证 | 登录 + `GET /issues` + `git clone` | `login=200`、`list_issues=200`、`clone_after_restore=OK`，日志显示 `d48955f3 drill: second commit` | 111 ms |
| migrate | `ironforge migrate` | `No pending migrations` | 6 ms |

恢复后 issue 内容（实测原文，节选）：

```json
{"data":[{"id":1,"repo_id":1,"number":1,"title":"drill issue must survive a restore","body":"t35","state":"open",...}]}
```

### 5.2 损坏的备份会被拒绝（且不动现有实例）

对备份里的 DB 副本写入垃圾字节后，用 `restore-db` 指向一个不存在的新目标：

```
1: error returned from database: (code: 11) database disk image is malformed
refused_target_exists=no
```

结论：`verify_sqlite_file` 在写入前打开的是临时副本（`main.rs:829-894`），
校验失败时目标文件**没有**被创建。这是「回滚到坏备份」不会二次伤害的证据。

### 5.3 只恢复 DB 的后果（原子性反例）

删掉 `/data/repos/drill`，只跑 `restore-db`：

```
restore_db_only_ms=18
api_issues_without_repo_dir=200
clone_exit=128
remote: repository not found
```

**API 健康、Git 不可用。** 运维若只用「`/health` + 列表接口」验活，会把这种半恢复误判为成功。
判定标准必须包含 `git clone` + `git fsck`（见 §4 第 3、4 条）。

### 5.4 PostgreSQL：`pg_dump -Fc` / `pg_restore`

在同一台机器起了 `postgres:16`，用镜像里的 `ironforge migrate` 建 schema（405 ms），
再走真实 API 造数（用户/仓库/issue/session + 一次 push），然后用数据库原生工具备份：

| 步骤 | 实测 |
|---|---:|
| 首次 `ironforge migrate`（PG） | 405 ms |
| `pg_dump -Fc` | 69 ms，334,188 B |
| `pg_restore` 到新库 | 424 ms |
| 表数量 | 105 = 105，表名列表完全一致 |
| 关键表行数 | users 1=1、repositories 1=1、issues 1=1、agent_sessions 1=1 |
| 迁移历史 | 97 = 97，逐条一致 |
| `ironforge migrate`（恢复库） | `No pending migrations`，10 ms |

### 5.5 运行中的 DB 备份（`backup-db`）

服务在线时执行：

```
live_backup_ms=15
live_integrity=ok
live_rows=1
source_rows=1
```

说明 `VACUUM INTO` 在线备份可用；但它**只含 DB**，不含 Git 仓库。

## 6. `deploy/post-restore-quiesce.sql` 是什么，怎么用

恢复出来的库是「过去的快照」。如果直接起服：

- 旧的协作 session secret 仍然有效；
- 旧的密码重置 token 仍然有效；
- 队列里 `webhook.deliver` / `email.send` 可能已经由原实例发出过，重跑会二次投递；
- `pending/running` 的 CI 会被恢复的 runner 重新执行（可能是部署任务）。

该文件（`deploy/post-restore-quiesce.sql:23-46`）正是把这四类状态停掉：

| 语句 | 作用 |
|---|---|
| `UPDATE agent_sessions SET ended_at=...` | 结束全部协作会话 |
| `DELETE FROM password_reset_tokens` | 删除重置令牌 |
| `UPDATE background_jobs ... status='dead'` | 把排队中的 webhook/email 标死，`last_error` 写明原因 |
| `UPDATE pipelines/pipeline_stages/pipeline_jobs` | 取消/失败所有在途 CI |

**明确不碰**：PAT、SSH/deploy key、runner 注册、镜像凭据、合并队列（文件头 15-17 行）——
这些在备份之后可能已被吊销/更换，必须人工复核。

SQLite 路径上，`ironforge restore` / `restore-db` 会**自动**执行等价语句（`main.rs:1200-1290`）；
PG/MySQL 没有自动 quiesce，README 要求在 `migrate` 之后、起服之前手工执行这个文件
（`deploy/README.md:144-153`）。

**本次实测（SQLite，`--keep-live-state` 跳过自动 quiesce，再手工执行该文件）**：

```
Skipped the post-restore quiesce step (--keep-live-state).
sessions_before=1
sessions_after=0
reset_tokens_after=0
live_ci_after=0
queued_outbound_after=0
```

**本次实测（PostgreSQL，恢复库上直接 `psql -f`）**：

```
live_sessions_before=1
quiesce_sql=applied
live_sessions_after=0
live_rows_after=0
```

命令：

```sh
psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -f deploy/post-restore-quiesce.sql
mysql ... ironforge < deploy/post-restore-quiesce.sql
```

## 7. 实测中遇到的问题（照抄，供复跑者避坑）

1. **`backup` / `restore` / `restore-db` 的输入输出是位置参数**，不是 `--output` / `--input`。
   第一次跑用了 `--output`，直接：
   `error: unexpected argument '--output' found`。CI 里若照 README 的 `backup-db` 示例写没问题，
   但自己拼参数很容易踩。
2. **镜像只认识 97 条迁移**（11-09 构建），源码 HEAD 是 112 条。用旧镜像恢复「由新版本写出的备份」会
   被 `verify_sqlite_file` 拒绝（`unknown migrations`），这是**正确**行为，但也意味着不能拿旧镜像
   验新 schema。
3. **应用镜像没有 `sqlite3` 和 `python3`**。要做独立校验得在容器里 `apt-get install sqlite3`
   （需要 root，且要 `mkdir -p /var/lib/apt/lists/partial`，否则 apt 报权限错）。
   生产镜像不建议改；演练时可以另起一个工具容器挂同一个 volume。
4. **`restore` 对已存在的目标必须 `--force`**；`--force` 会删除 `-wal`/`-shm`。不停服时执行会拿到
   「target database already exists」，这是保护，不是 bug。
5. **DB-only 恢复会造成「API 200 + clone 404」**（§5.3），验活不能只看 HTTP。
6. **`restore` 不会自动跑 `migrate`**，它只打印提示；但 SQLite 的 quiesce 是在 restore 内部直接做的，
   顺序是「写 DB → 写文件 → quiesce」，所以手工 migrate 要放在起服前。
7. **PG 的 112 vs 97 差异同样存在**；`pg_dump` 不包含 `repo_root`，Git 数据要单独 `rsync`。
   本次 PG 演练只验证了 DB 层往返。

## 8. RPO / RTO 怎么算（只用本次实测，不外推）

- **RPO**：系统没有自动备份、没有 WAL 归档、没有 PITR。RPO = 上次成功备份到故障发生的时间差，
  完全由运维的备份频率决定。**不承诺任何 RPO 数值。**
- **RTO（机器时间）**：
  - SQLite：`停服 2ms + restore 26ms + migrate 6ms + 启动 111ms ≈ 145ms`（1.6MB 数据集）；
  - PostgreSQL：`pg_restore 424ms + migrate 10ms + dump 前的停服时间`（334KB dump；启动未计时）。
  - 不含：发现故障、确认要恢复、从异地拷回备份、人工决策。真实 RTO 通常是「分钟级」，
    由人决定，不由这 145ms 决定。
- 数据集变大后不能用这两个数字线性外推：`copy_dir_recursive` 是单线程文件拷贝
  （`main.rs:1290-1330`），Git 仓库多、小文件多时会明显变慢。**要做容量规划，必须在目标数据量上重跑本演练。**

## 9. 什么算演练失败

- `sha256sum -c` 有任何一行不是 `OK`；
- `git fsck` 非 0；
- 恢复后 `/health` 非 200 或任一检查不是 `ok`；
- `migrate` 不是 `No pending migrations`；
- 登录失败，或 issue 数/仓库 HEAD 与备份前记录不一致；
- `git clone` 失败；
- quiesce 后 `live_sessions_after` / `live_rows_after` 不为 0。

任何一条命中：**保留现场**（容器、日志、备份目录），不要在原卷上反复 restore 覆盖证据。

## 10. 未实测的项（明确写出来，不推测）

| 项 | 状态 | 卡在哪 |
|---|---|---|
| MySQL dump/restore | **未实测** | 需要跑 `scripts/db-restore-drill.sh mysql`，它会 `cargo test -p rg-http --test collab_condition_tests`（整套 debug 构建）；本轮未做，避免用未编译的测试结果冒充实测。镜像支持 MySQL，`mysqldump` 命令在 README 里给了 |
| S3 后端下的备份/恢复 | **未实测** | 本次未起 MinIO/S3；`backup` 只拷本地 `repo_root`，S3 里的 blob 不在备份内（代码事实，`main.rs:970-1010` 只 `copy_dir_recursive(repo_root)`） |
| Python 端到端恢复场景 | **未实测** | `scripts/collab-e2e.py --scenario restore` 需要 `python3` + 本地 `target/debug/ironforge`；应用镜像无 python3，宿主是 macOS 不能直接跑 Linux 二进制。该脚本覆盖更细（14 个协作视图、`--keep-live-state` 语义），建议在 Linux CI 上跑一次 |
| 从 HEAD（112 迁移）构建的镜像 | **未实测** | 需要一次完整 `docker build`（前端 pnpm + Rust release）；本轮的镜像是 11-09 的，少 15 条迁移 |
| PITR / 增量备份 | **不支持** | 无 WAL 归档、无 binlog 消费 |

## 11. 复跑本演练的最小脚本

把 §5.1 的步骤写成一个脚本即可复现；本轮实际执行的脚本保存在容器 `/tmp/t35_drill.sh`
（等价内容如下，省略了造数 git 细节）：

```sh
set -euo pipefail
DB='sqlite:///data/ironforge.db?mode=rwc'
ironforge serve --repo-root /data/repos --http-addr 127.0.0.1:8080 \
  --ssh-addr 127.0.0.1:2222 --db-url "$DB" --jwt-secret "$SECRET" &
# ... seed through the API, then:
kill %1; wait
ironforge backup --db-url "$DB" --repo-root /data/repos /data/backups/b1
rm -f /data/ironforge.db*; rm -rf /data/repos/*
ironforge restore --db-url "$DB" --repo-root /data/repos /data/backups/b1 --force
ironforge migrate --db-url "$DB"
git --git-dir=/data/repos/<owner>/<repo>.git fsck
ironforge serve ... & curl -fsS http://127.0.0.1:8080/health
```
