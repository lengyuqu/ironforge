# `ironforge repair-pull-refs`：给已存在的 PR 补发 `refs/pull/<n>/head`（验证记录，2026-11-09）

> 基线 main `04433f8`。背景：`refs/pull/<n>/head` 只在 PR 创建、head 分支被推送、REST 文件提交时写入（`rg_core::pull_request::publish_head_ref`）。功能上线前开的 PR 一直没有这个锚点，要等它的 head 分支下次被推送；对 fork PR 来说，那个分支可能在别人的仓库里、再也不动。

## 一、行为

```
ironforge repair-pull-refs [--db-url …] [--repo-root …] [--repo owner/name]
                           [--include-closed] [--dry-run]
```

- **范围**：默认只处理 `open` 与 `merging`（`merging` 是瞬态，算开放）；`--include-closed` 把 `merged`/`closed` 也纳入。`--repo` 限定单个仓库，否则遍历实例里所有仓库。
- **判据**：读 PR head 分支的**实时**尖端（`live_head_sha`：fork PR 读 fork，同仓 PR 读目标仓库），再读目标仓库里的 `refs/pull/<n>/head`：
  - 两者相同 → 计入 `current`，不写；
  - ref 不存在 → `missing`，写；
  - ref 指向别处 → `stale`，写。
- **head 分支已不存在**：只报告 `head branch no longer exists`，不报错、不写。分支没了，ref 也就无从重建。
- **`--dry-run`**：只列 `planned`，一个字节都不写。
- **幂等**：跑第二遍全部落进 `current`。
- **副作用**：只写 ref（fork PR 顺带把对象复制进目标仓库，这与正常发布路径一致）。**不改 PR 记录、不发 webhook、不写审计**——命令没有操作人身份可记，影响面就是那个 ref，跑完看一眼输出即可。
- **`base_sha_when_opened` 不回填**：旧 PR 当时没存过，猜一个等于伪造锚点；命令每次都会在输出里说明这一点。

同仓 PR 也在范围内（`--repo` 或全量都算）：同仓 PR 的 ref 是同一批 2026-11-07 的功能加上去的，功能上线前的同仓 PR 一样缺，只是它们下一次推送就会自愈，fork PR 不会。

## 二、改动

| 文件 | 改动 |
|---|---|
| `crates/rg-core/src/pull_request/repair.rs`（新） | `repair_pull_refs` / `RepairOptions` / `RepairReport` / `RefAction` / `Skipped`；复用 `service::publish_head_ref`、`service::live_head_sha`、`service::repository_namespace` |
| `crates/rg-core/src/pull_request/mod.rs` | 登记模块并 re-export |
| `crates/rg-cli/src/main.rs` | 新子命令 `RepairPullRefs` 与输出（汇总行、逐条 `repaired`/`would repair`、跳过原因、base 锚点说明），`--repo` 形状校验 |
| `scripts/ci-db-tests.sh` | agent 目标列表加 `repair_pull_refs_tests`（该测试读 `IRONFORGE_TEST_DATABASE_URL`） |
| `crates/rg-http/tests/repair_pull_refs_tests.rs`（新） | 4 项集成测试 |

**重叠**：`scripts/ci-db-tests.sh` 的同一行也被 A1（PR #32，未合并）改过，两者合并时会有一处文本冲突，取舍是「两边的目标都保留」。

## 三、证据

### 任务验证（SQLite）

`cargo test -p rg-http --test repair_pull_refs_tests` → **4 passed**：

| 用例 | 验的是什么 |
|---|---|
| `the_repair_command_restores_a_missing_ref_and_leaves_the_rest_alone` | 造 fork PR → 删掉 `refs/pull/1/head`（模拟旧 PR）→ dry-run 只报不写 → 真跑补回且指向 fork 尖端、commit 对象也在目标仓库 → 再跑一遍 `current=1`、不写 |
| `a_ref_that_points_at_an_older_commit_is_moved_to_the_live_head` | fork 分支前进后把 ref 强行指回旧提交 → 报告 `stale`，`previous` 是旧 sha，修完等于新尖端 |
| `a_pull_request_whose_head_branch_is_gone_is_reported_not_repaired` | ref 与 fork 分支都删掉 → `skipped` 一条、原因 `head branch no longer exists`、不写、不报错 |
| `closed_pull_requests_are_out_of_scope_unless_asked_for` | 关掉 PR 后默认不扫（`scanned=0`），`--include-closed` 才纳入并修复 |

### CLI 冒烟（真实二进制）

```
$ ironforge repair-pull-refs --help                       # 子命令与参数正确
$ ironforge migrate --db-url sqlite:///tmp/rp-smoke.db?mode=rwc
$ ironforge repair-pull-refs --db-url … --repo-root /tmp/rp-smoke-repos --dry-run
scanned 0 pull request(s): 0 already current, 0 would repair, 0 skipped
note: the base commit recorded at pull-request open (anchors.base_sha_when_opened) is not backfilled …
$ ironforge repair-pull-refs … --repo nobody/nothing     # Error: repository nobody/nothing not found
$ ironforge repair-pull-refs … --repo noslash            # Error: --repo takes the form owner/name
```

### 后端验证（真实 PostgreSQL 17 / MySQL 8.4）

`scripts/ci-db-tests.sh postgres agent` 与 `mysql agent`（每目标全新库）：两个后端均 **exit 0**，9 个目标 **44 项全通过**，其中 `repair_pull_refs_tests` **4 项**在两边都通过。第一次跑时它在两个后端都失败过——不是后端差异，是测试之间共用数据库（CLAUDE.md 坑 23）导致全量遍历也看到别的测试建的仓库；改成按 `--repo` 限定后通过，这也顺带说明「不带 `--repo` 会扫全实例」这个行为是真的。

### 静态检查

`cargo fmt --all --check` 干净；`cargo clippy --workspace --all-targets -- -D warnings` 干净。

## 四、未覆盖

- **没在真实大实例上跑过**：只用测试数据（几十个 PR）验过，没有在 ironforge.db 的真实库上跑（那会改真实 ref，不在本任务范围）。真跑前建议先 `--dry-run`。
- **没有并发保护**：命令运行期间若有推送同时更新同一个 ref，后写者赢；两种结果都指向某个真实提交，不会产生悬空 ref，但没有互斥。
- **没有测中断重跑**：幂等是靠「读实时尖端再比对」保证的，逻辑上可中断重跑，但没有构造 `kill -9` 场景。
- **没有覆盖 `merging` 状态的 PR**：包含在扫描范围里，但没有测试构造这个瞬态。
- **`--repo` 之外的过滤没有**：没有按时间/编号区间/只处理 fork 之类的开关，需要的话再加。
- **CI 未跑**：本记录所有结果都来自本机；`scripts/ci-db-tests.sh` 里新增的目标要等本 PR 的 CI 才有第一份自动化证据。
