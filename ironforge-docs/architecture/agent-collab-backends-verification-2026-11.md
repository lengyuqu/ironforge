# 协作套件在 PostgreSQL / MySQL 上补验证（验证记录，2026-11-09）

> **注（2026-11-24）**：文中"没在 git233 Actions 上跑"属于当时的验证边界。git233 已移除，CI 现在全部在 GitHub 托管 runner 上。

> 基线 main `04433f8`。背景：`CLAUDE.md` 里有几处写着「仅 SQLite 实测，PG/MySQL 未验证」（跨仓协调组 X02、协作人类通知、协作 Web UI 依赖的后端）。本任务把整套协作/agent 写路径测试在真实 PostgreSQL 17 与 MySQL 8.4 上跑一遍，暴露问题就修。

## 一、做法

```bash
scripts/ci-db-tests.sh postgres all    # 36 个目标，每目标一个全新库
scripts/ci-db-tests.sh mysql all
```

脚本自带的行为（沿用，不改）：Docker 起一次性容器；每个目标 `DROP/CREATE DATABASE` 后跑；`IRONFORGE_TEST_DATABASE_URL` 传给测试。真后端感知有两条路：`tests/common/mod.rs` 的 `spawn_test_app_on_env_db`（agent 写路径目标）与 `tests/collab_common/mod.rs` 的 `Env`（协作套件），两者都读同一个环境变量。

## 二、结果：36 目标 / 280 用例，两个后端全绿

| 后端 | 目标数 | 通过用例 | 失败 |
|---|---|---|---|
| PostgreSQL 17（Docker，全新库） | 36 | **280** | 0 |
| MySQL 8.4（Docker，全新库） | 36 | **280** | 0 |

关键套件（两个后端同数）：

| 目标 | 用例 |
|---|---|
| `multi_backend_smoke`（迁移 / CRUD / FTS，`--ignored`） | 2 |
| `collab_store` / `collab_groups_store` / `collab_migration` | 15 / 5 / 0（1 ignored） |
| `collab_p0_tests` / `collab_contract_tests` / `collab_health_tests` | 19 / 16 / 17 |
| `collab_human_notify_tests` | 10 |
| `collab_condition_tests` / `collab_dependency_tests` / `collab_ops_tests` | 14 / 14 / 14 |
| `collab_notice_tests` / `collab_decision_tests` / `collab_session_tests` | 13 / 11 / 10 |
| agent 写路径（`pat_ref_allowlist` / `file_commit_semantics` / `idempotency` / `repo_events` / `pr_context` / `pr_export` / `branch_delete` / `fork_pr_ref`） | 7 / 5 / 5 / 5 / 4 / 7 / 5 / 2 |

## 三、修了什么：没有

两个后端**一次跑通，没有暴露需要改代码的后端差异**。没有改 `rg-db/src/ops`，没有改测试。

原因值得记下来，免得下次又从零怀疑：

- `ON CONFLICT DO NOTHING` 这类 SQLite 专有写法在协作写路径里已经没有了（CLAUDE.md 坑 19 是当初踩过的），upsert/去重都走 SeaORM 的跨库构造或「先查再写」。
- 事务与并发相关的用例（版本 CAS、幂等键、one-winner 设置更新）在两个后端上都能过，说明坑 21（SQLite 读开头再写会 BUSY）掩盖的差异没有被测试依赖。
- MySQL 只存整秒（坑 23）没有影响断言：这些套件要么不比较时间戳，要么用「不早于/不晚于」而不是相等。

## 四、CLAUDE.md 改了哪几句（只改有证据的）

| 位置 | 原 | 现 |
|---|---|---|
| 协作组 X02 | 「版本 CAS、软移除 + 审计。**仅 SQLite 实测，PG/MySQL 未验证**。」 | 「版本 CAS、软移除 + 审计。**SQLite、PostgreSQL 17、MySQL 8.4 均通过**（`collab_groups_store` 5 项、`collab_groups_tests` 9 项）。」 |
| 协作人类通知 | 「**无邮件**（无用户偏好、无邮箱验证），**仅 SQLite**。」 | 「**无邮件**（无用户偏好、无邮箱验证），**SQLite、PostgreSQL 17、MySQL 8.4 均通过**（`collab_human_notify_tests` 10 项）。」 |
| 协作 Web UI（首版） | 「**仅 SQLite + 内置浏览器手工验证**。」 | 「**页面只在** SQLite + 内置浏览器手工验证过；它依赖的后端协作套件已在 PostgreSQL 17 与 MySQL 8.4 上通过（见本文）。」 |

没改的两处「仅 SQLite」是它们本来就没变：`scripts/collab-e2e.py` 的双客户端闭环（只跑 SQLite 的手工脚本）、`rg-cli` 备份（CLI 备份确实只支持 SQLite，PG/MySQL 走原生 dump）。

## 五、仍然只在 SQLite 上跑的目标（本次没扩范围）

`crates/rg-http/tests/` 下 111 个测试目标里，35 个能连真后端，**76 个仍只用 SQLite**。按「有没有跨库语义」筛，**值得下一批改成也读 `IRONFORGE_TEST_DATABASE_URL` 的目标**（按价值排序，一次别改太多）：

| 目标 | 为什么值得 |
|---|---|
| `issues_boards_error_semantics_tests` | 唯一约束 / 顺序 / 冲突语义都在断言里，最容易在 PG/MySQL 上分叉 |
| `issue_reaction_tests` | 唯一约束（同一用户同一表情只能一条） |
| `error_semantics_accounts_tests` | 重复注册靠唯一约束，PG/MySQL 报错码不同 |
| `merge_queue_ci_tests` | 并发竞态，SQLite 恰好更宽容 |
| `pr_merge_strategy_tests` | 合并策略依赖事务与引用更新 |
| `release_tests` / `time_tracking_tests` | 分页 + 排序，MySQL 的 `LIMIT/OFFSET` 与排序稳定性不同 |
| `org_tests` | 跑迁移路径，撞到跨库 DDL 差异的概率最高 |

其余 60 多个是纯 HTTP 形状、鉴权矩阵、序列化、错误体之类的断言，换成真后端收益很小，不建议动。

## 六、未覆盖

- **没在 git233 Actions 上跑**：以上是本机 Docker（Docker 29.9.3）的结果，CI 上是否同版本、缓存/超时是否够（见 `ci-cache-diagnosis-2026-11.md`）另说。
- **只跑了 `all`，没跑全量 `cargo test`**：`all` 之外的 SQLite 目标与 `rg-http` 的其余 76 个目标不在本次范围。
- **没有构造后端特有的边界**：比如 MySQL 的 191 字符索引前缀、PG 的 `CITEXT`/大小写排序、时区与整秒截断。跑通不等于这些边界都覆盖了。
- **没有验证迁移在已有数据的库上升级**：`collab_migration` 只有 1 个 `#[ignore]` 的用例，本后端跑的是 `0 passed; 1 ignored`。
- 本记录不覆盖 `collab-e2e.py` 那套真实服务 + 真实 MCP 的闭环（A3 的范围）。
