# 协作 O03b：备份与隔离恢复演练（SQLite）

> 日期：2026-11-05。`scripts/collab-e2e.py --scenario restore`。设计依据：[运维需求 §4](agent-collaboration-operability-requirements-2026-11.md)。**可信度**：确认（运行验证，范围见“没有被证明的”）。

## 演练前的基线（运行验证，已修复）

读代码并实测现有 `ironforge restore`：

- 备份**没有校验和**，恢复也不检查数据库文件是否完好。把备份里的 `ironforge.db` 中段写入随机字节后执行 `restore`，**退出码为 0**，目标库随即是损坏的（`PRAGMA integrity_check` 报 `database disk image is malformed`）。带 `--force` 时它会先覆盖现有数据库、再复制文件，所以损坏的备份会**替换掉一个正常运行的实例**。这不满足“校验失败不替换现有服务数据”。
- 对备份来自更新版本数据库的情形没有检查。

## 改了什么

- `ironforge backup` 在备份目录里写 `checksums.sha256`（逐文件 SHA-256，`sha256sum` 格式），清单里记录它。
- `ironforge restore` 在**第一次写入之前**调用 `verify_backup`：清单与格式版本；每个文件与 `checksums.sha256` 一致，且没有清单外多出的文件；备份数据库的 `PRAGMA integrity_check`（在临时副本上做，不对备份本身写）；数据库里记录的每个迁移都必须被当前二进制认识（否则“备份来自更新版本”，拒绝）。
- 没有 `checksums.sha256` 的旧备份仍可恢复，但会警告“文件无法校验”。`restore-db`（只恢复数据库文件）也先做完整性与迁移检查。
- `rg_db::known_migration_names()` 供上面的检查使用。

## 演练内容与结果（运行验证）

流程：真实服务 + 真实 `ironforge-mcp` + HTTP 客户端 + 真实 Git 仓库，在源实例上造跨表协作数据（2 个任务、5 条含中日文的有序消息、依赖、问题与答复、交接、决定、工作包（带真实 commit）、完成条件及其版本、确认、快照、收件箱已读标记、依赖完成通知）→ 停源服务 → 真实 `ironforge backup` → 恢复到**另一个目录**并 `ironforge migrate`（无操作）→ 在**另一组端口**启动第二个实例。

| 项 | 结果 |
|---|---|
| 备份里不含明文 | ✅ 会话密钥、PAT、JWT 签名密钥均不在任何备份文件中 |
| 协作数据一致 | ✅ 27 个视图（任务、消息顺序与内容、交接、问题、决定、工作包、依赖、条件各版本、快照、完成状态、收件箱、通知）在去掉易变字段后与恢复前**逐字相同**；消息 `seq` 为 1..5 |
| Git | ✅ 两个提交可读，任务分支指向同一提交，`git fsck` 干净，平台观测到的分支头相同 |
| 恢复后的凭据 | **2026-11-05 起默认收敛**（`restore`/`restore-db`，`--keep-live-state` 可跳过）：所有协作会话置为已结束（旧会话密钥得到 409）；密码重置令牌删除；登录令牌取决于配置里的签名密钥，不受影响；个人访问令牌**原样保留并报告数量**（备份无法知道哪些在备份之后被吊销，全部作废会打断所有集成）。演练 `[08]` 用 `--keep-live-state` 观察旧状态仍有效，`[09]` 验证默认恢复后旧会话密钥被拒 |
| 副作用 | 默认收敛把排队中的 `webhook.deliver` / `email.send` 作业标为 `dead`（`last_error: abandoned at restore…`），并取消 pending/running 的 CI 流水线（阶段与作业置 `failed`），恢复实例/Runner 不会补发或重跑它们；单元测试 `quiesce_ends_sessions_and_abandons_only_queued_outbound_jobs` 钉死。**仍未覆盖**：镜像同步、合并队列、以及恢复后新产生的事件照常外发；演练实例本身未配置这些通道，没有端到端验证“配置了通道时不外发” |
| 损坏的数据库（校验和不符） | ✅ 拒绝，现有实例的数据库与仓库逐字节不变 |
| 损坏的数据库但校验和被改成一致（只有完整性检查能发现） | ✅ 拒绝，现有实例不变 |
| 损坏的 Git 对象文件 | ✅ 拒绝（校验和），现有实例不变 |
| 缺少数据库文件 / 格式版本更新 / 来自更新迁移的备份 | ✅ 各自拒绝，现有实例不变 |
| 目标已存在且未给 `--force` | ✅ 拒绝，不变 |
| 无校验和的旧备份 | ✅ 恢复成功并给出警告 |
| 日志泄露 | ✅ 源与恢复实例的服务日志、MCP 进程日志中没有会话密钥 |
| 稳定性 | ✅ 演练脚本连跑 3 遍通过；`rg-cli` 新增 4 项单元测试通过 |

## 没有被证明的

- **CLI 备份只有 SQLite**。`ironforge backup` 对 PostgreSQL/MySQL 明确拒绝，运维用 `pg_dump`/`mysqldump`。2026-11-05 起有 `scripts/db-restore-drill.sh postgres|mysql`（需要 Docker）演练这条链路：用真实测试目标（`collab_condition_tests`）写入数据 → 原生工具转储 → 恢复到第二个库 → 逐表比较行数与内容校验（PostgreSQL 用 `md5(string_agg(行文本))`，MySQL 用 `CHECKSUM TABLE`）→ 对恢复库跑 `ironforge migrate` 必须无待执行迁移且迁移历史一致。实测（PostgreSQL 17、MySQL 8.4，本机 Docker）：103 张表一致（18 张有数据）、93 条迁移历史一致。第 5 步对恢复库应用 `deploy/post-restore-quiesce.sql`（与 SQLite CLI 同一策略的纯 SQL 版本，PostgreSQL/MySQL 手工执行）：演练里 12 个未结束会话全部结束、无重置令牌/排队 webhook·邮件作业/运行中 CI 残留；`rg-cli` 单元测试 `the_quiesce_sql_file_matches_the_cli_path` 保证该文件与 SQLite CLI 路径结果一致。**没覆盖**：Git 仓库与 Blob 存储（仍靠文件备份）、在恢复库上启动服务、时间点恢复；访问令牌、SSH/Deploy Key、Runner 注册、镜像凭据、合并队列仍需手工复核。
- 备份不是原子的：数据库快照和文件复制先后进行，必须先停服务；仍未提供一致性检测之外的在线一致备份。
- 校验和防的是**备份目录在存放中的损坏或篡改**，不能证明备份时数据库与 Git 的内容彼此一致；也不防有人同时改文件和 `checksums.sha256`。
- 单机、单次时间点；没有给出 RPO/RTO 目标（需要更多真实演练再定）。
- 恢复后收敛只处理上面列出的表；个人访问令牌、SSH/Deploy Key、Runner 注册、镜像凭据原样恢复，由运维复核。PostgreSQL/MySQL 用 `deploy/post-restore-quiesce.sql` 手工执行。
- 演练脚本和 `rg-cli` 单元测试之外，没有在 CI 里跑这个演练（手工入口）。
