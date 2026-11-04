# 多 Agent 协作后续功能：验证记录

> 2026-11-04，分支 `feat/harness-ci-feedback`，基线 `e48c0d0` 加本次未提交改动。结果不是 CI、发布或生产验证。

## 实现和真实运行

- O09 在已有任务、问题、决定、交接搜索上增加消息和工作包；以仓库权限为边界，结果不返回正文、片段或总数。它是按单一类型执行的实时子串查询，分页不保证跨请求快照一致；大小写行为取决于数据库排序规则。
- MCP 增加通知列表、确认通知、确认消息回执及查询投递状态。通知确认不代表同意，回执不移动已读水位。
- `python3 scripts/collab-e2e.py --scenario x04`：真实服务器、真实 MCP 和 REST 客户端、两个私有 Git 仓库、独立会话及已读水位；能力发现、跨仓关联、MCP/REST 双向通知及消息回执、断线接续、管理员健康查询（实际阻塞任务数为 1）、撤权过滤和日志密钥扫描通过。该次运行范围为单实例 SQLite；之后的双实例及其他数据库验证见下方补充。

## 后端验证（临时容器）

- PostgreSQL 17 和 MySQL 8.4：`collab_store` 14 项、`collab_groups_store` 5 项均通过；O09 HTTP 搜索与撤权测试各 2 项通过。
- PostgreSQL：决定、依赖、诊断、提醒、跨仓、健康、通知及 O06–O08 HTTP 测试通过；P0 首次 17/19，失败为 pipeline 表 PostgreSQL `INTEGER` 与 SeaORM 模型 `i64` 不匹配；数据访问层对旧列显式投影为 `bigint` 后复跑 19/19。另在 SQLite、PostgreSQL 和 MySQL 验证流水线创建、查找、状态更新及列表路径；stage/job 等其他 CI 表的旧列类型未在本次修复中覆盖。
- MySQL：P0、决定、依赖、提醒、通知、跨仓及 O06–O08 测试通过。通知和跨仓测试中同秒记录顺序按合同的 `(created_at, id)` 验证，不假设等时间戳下的插入顺序。健康指标首次有 5 项失败：MySQL 秒精度会向上取整，并且同秒 UUID 不保证发生顺序。对新事件及新建任务、交接和问题在写入前截断至秒；复测 16/16，M08 的边界场景额外重复 5 次通过。历史行的亚秒时间不能恢复，旧行若曾向上取整，秒边界附近历史查询仍有误差风险。
- 本次使用临时测试容器和独立数据库，没有对现有运行实例的数据库执行迁移。数据访问层修改会影响事件写入，须在最终基线上重跑受影响测试。

## 未完成

修复后的 `cargo test --workspace --no-fail-fast`：113 个测试目标、972 通过、0 失败、11 忽略；`cargo clippy --workspace --all-targets -- -D warnings` 通过。PostgreSQL 的其他 CI stage/job 路径在当次核验中复现类型问题，后续修复见文末；MySQL 历史数据的秒级边界、跨主机验证和 GitHub CI 均未完成。双实例本机验证见下方补充。本记录不得作为生产发布或完整 P2 阶段门证明。

## 2026-11-04 双实例与 CI 表核验补充

被测对象为分支 `feat/harness-ci-feedback`、HEAD `e48c0d0` 加当时未提交工作区；二进制哈希（SHA-256 前 16 位）服务端 `0eafb3a57437109e`、MCP `73260e3a60f045c5`。这是本机运行验证，不是干净 SHA、CI 或生产发布。

- 使用 `scripts/collab-e2e.py --scenario x04 --multi-instance`，在 SQLite、PostgreSQL 17 和 MySQL 8.4 上分别通过：两个独立服务进程共享同一测试数据库及本机临时 Git 目录；两个不同客户端、两个私有仓库、通知和回执、健康指标、跨进程 CAS/幂等竞争、游标跨实例续页和撤权后的游标重鉴权、跨进程撤权、主实例关闭后备用实例继续服务、主实例重启恢复；扫描两个服务进程与 MCP 日志未发现会话密钥。外部数据库只使用了临时容器内的新建测试库。该结果不覆盖不同物理主机、网络分区、独立 Git 存储或分布式锁。
- 验证流程可通过 `IRONFORGE_E2E_DB_URL` 指向全新**临时测试库**；脚本会迁移并创建数据，不可指向现有实例。命令摘要与二进制哈希可用 `--json-out` 保存；数据库 URL 未写入结果。
- CI 整数列检查：`pipeline_stages.id/pipeline_id`、`pipeline_jobs.id/stage_id` 及 `ci_environment_approvals.job_id` 原始 schema 为 `INTEGER`，实体为 `i64`。在 PostgreSQL 临时库执行 `ci_integer_compat` 六项（stage/job/approval 各创建及读取），**0 通过、6 失败**，错误为 `INT4` 不能解码为 `INT8`；在 MySQL 临时库同六项通过。当次六项测试默认 ignored；修复后已取消忽略并扩展为三数据库回归，见下文。修复前 PostgreSQL 上插入可能已经提交后才在读取返回模型时失败，调用方盲目重试可能产生重复记录。
- `find_latest_by_repo_and_commit` 的 pipeline 同类问题已增加 PostgreSQL 显式投影处理；SQLite、PostgreSQL、MySQL 的 `pipeline_evidence_access_handles_legacy_integer_columns` 均通过。该次 pipeline 专项修复当时不涵盖 stage、job、approval（见下方后续修复）；数据库内 `INTEGER` 的 32 位上限依旧存在，不能把模型中的 `i64` 视为已扩容。


## 2026-11-04 CI 整数类型兼容修复与复测

- 当次已复现的 PostgreSQL `pipeline_stages`、`pipeline_jobs` 和 `ci_environment_approvals` 旧表 INTEGER/i64 解码问题：阶段和任务的读/写路径按 PostgreSQL 显式投影，创建放入事务，解码失败时回滚；审批插入不再依赖错误类型的 `RETURNING`，查询对旧列转换并保持重复审批幂等。`find_active_pipelines_by_ref` 同样兼容旧流水线整数列。没有修改已执行的迁移或表结构，原 INTEGER 的 32 位存储上限仍然存在。
- 扩展 `ci_integer_compat`：SQLite、PostgreSQL 17、MySQL 8.4 各 12 项通过，涵盖阶段/任务创建读取、可选字段、任务调度及日志、手动放行、重复及并发审批、整数溢出不留半成品、重跑/取消、Runner 离线级联。PostgreSQL 额外在非 UTC 会话验证任务更新时间；该专项在 SQLite/MySQL 条件跳过，溢出专项在 SQLite 条件跳过。
- `scripts/collab-e2e.py --scenario ci-compat` 在三个后端各启动真实服务，用外部 Runner 模式验证受保护环境、双人审批、重复审批拒绝和两个同步并发的流水线创建；无需启动 Runner，不执行仓库脚本。三次均通过（SQLite 场景另重复 3 次），PostgreSQL/MySQL 均使用新建的隔离容器数据库；三次服务端二进制 SHA-256 前 16 位均为 `e864a5b43af375ff`，被测代码仍含未提交工作区。
- SQLite CI HTTP 回归：反馈 14、权限及审批 5、重跑 11、Runner 离线 3，合计 33 项通过。此前完整 workspace 测试结果是修复前的工作区证据，不能替代此后的完整重跑；此次还需以最终代码 SHA 跑 CI/发布验证。
- PostgreSQL 的非 UTC 会话已验证；MySQL `TIMESTAMP` 对不同连接使用不同会话时区时的行为尚未验证，建议实例连接使用一致时区。远端 CI、生产环境、历史数据和跨主机测试未覆盖。
