# T36 执行记录 — CNB 导入真机验证

> 计划见 `docs/plans/T36-plan.md`，权威约束见 `tasks/T36.md`。
> 基线 `64861cf9`，分支 `agent/t36-cnb-live-verify`，worktree `../ironforge-t36`。

## 1. 开工核对

```
$ git log --oneline -1
64861cf9 chore(tasks): seed batch 4 (T32-T36) for the four production gaps
$ cat ~/.cnb/token | jq -r '.auths["cnb.cool"].access_token | length'
94
$ cnb --version   # 1.18.48
```

没有读取或写入任何 token 文件内容到仓库；端到端测试的 token 只在进程环境变量中传递，
所有落盘产物（数据库、日志、快照）都不含 token。

## 2. 改动的文件

| 文件 | 改动 |
|---|---|
| `crates/rg-core/src/import/cnb_client.rs` | issues 双状态拉取 + 去重；PR 详情；仓库标签；正式评审；429/5xx 退避；`CnbLabel` 补 color/description；7 条新单测 |
| `crates/rg-core/src/import/service.rs`（CNB 段） | PR 正文补取；PR 评论→`pr_reviews`；标签合并/颜色；作者映射；能力 flag 清零；5 条新单测 |

未改：`Cargo.*`、`migrations/`、`rg-http` 路由/OpenAPI/route_meta、`scripts/`、`deploy/`、
`github_client.rs`、`gitlab_client.rs`、共享的 `clone_repo`/`resolve_or_create_target_repo`/`map_users`。
`run_cnb_import` 通过共享导入器的**参数**改变行为，没有改共享实现。

## 3. 缺陷与提交

| # | 提交 | 一句话 |
|---|---|---|
| 1 | `ea1cc2c1` | issues `state=all` 会 400，整个导入死在第一步 |
| 2 | `cedf7b34` | PR 列表正文恒空，34 个 PR 全部丢正文 |
| 3 | `f6b5d738` | PR 评论写进 `issue_comments` 用 PR id 当 `issue_id`，26 条孤儿 |
| 4 | `aa06af70` | 标签只导被引用的、且全部变灰；`/-/labels` 其实存在 |
| 5 | `9f4e6733` | milestones/releases/wiki 被标成「已导」，实际没导 |
| 6 | `5f102cc8` | 429 被 `unwrap_or_default` 静默吞成「0 条评论」 |
| 7 | `13eb3b00` | 作者全部归到 user id 1，而不是导入者 |
| 8 | `60f55cdc` | 正式评审没导；`/pulls/{n}/reviews` 存在且返回 `approved` |
| — | `84995e66` | 标签颜色/去重单测（测试提交） |
| — | `eb24f79a` | 两次并发导入下 6 次重试不够，提到 8 次 |

每条 commit message 都记录了「真实返回 / 代码假设 / 改法 / 验证方式」。

## 4. 端到端导入（最终代码）

实例：`/tmp/t36v5`（clone + 全量）、`/tmp/t36v6`（最终代码复核 + agent2ssh），
独立 SQLite + `--repo-root`，`admin`=id 1，`importer`=id 2。

请求体（token 从环境变量注入，不落盘）：

```json
{"platform":"cnb","source_url":"https://cnb.cool/lengyuqu/ironforge.git",
 "target_owner":"admin","target_name":"cnb-ironforge","auth_token":"<from ~/.cnb/token>",
 "import_repo":true,"import_issues":true,"import_pull_requests":true,"import_labels":true,
 "import_milestones":true,"import_releases":true}
```

返回行（能力 flag 已被清零）：

```
import_repo=true import_issues=true import_pull_requests=true import_labels=true
import_milestones=false import_releases=false import_wiki=false
```

任务结果：

```json
{"repo_cloned":true,"labels_imported":0,"milestones_imported":0,"issues_imported":32,
 "issue_comments_imported":206,"prs_imported":34,"pr_reviews_imported":26,
 "releases_imported":0,"wiki_pages_imported":0}
```

## 5. SQL 核对（不许只看 stats）

```sql
-- 数量
SELECT COUNT(*), SUM(state='open'), SUM(state='closed') FROM issues WHERE repo_id=1;
-- 32 | 1 | 31
SELECT COUNT(*) FROM issue_comments c JOIN issues i ON c.issue_id=i.id WHERE i.repo_id=1;
-- 206
SELECT COUNT(*), SUM(state='merged'), COUNT(DISTINCT number) FROM pull_requests WHERE repo_id=1;
-- 34 | 34 | 34
SELECT COUNT(*) FROM pull_requests WHERE repo_id=1 AND body<>'';
-- 34
SELECT COUNT(*), SUM(action='comment') FROM pr_reviews WHERE repo_id=1;
-- 26 | 26
SELECT COUNT(*) FROM labels WHERE repo_id=1;
-- 0

-- 孤儿外键（任务卡指定，必须为 0）
SELECT COUNT(*) FROM issue_comments c LEFT JOIN issues i ON c.issue_id=i.id WHERE i.id IS NULL;
-- 0
SELECT COUNT(*) FROM pr_reviews r LEFT JOIN pull_requests p ON r.pr_id=p.id WHERE p.id IS NULL;
-- 0
```

编号与内容：

- issue 号 11–65、32 个不重复，1 open / 31 closed；
- PR 号 1–66、34 个不重复，34 个全部 `merged`；head/base 为短分支名，head_sha 与源一致；
- 逐字段脚本（重新请求源 API）比对 32 issues + 34 PRs：
  标题、状态、正文、评论正文、head/base、created_at —— **0 处不一致**；
- clone：`git --git-dir=/tmp/t36v5/repos/admin/cnb-ironforge.git rev-parse HEAD`
  → `64861cf9df821dc0dbfed0cd03e28d923183cdcd`（与源 HEAD 相同）。

## 6. 作者归属（缺陷 7 的实测）

以 `importer`（id 2）导入到另一个目标：

```sql
SELECT DISTINCT author_id FROM issues WHERE repo_id=2;         -- 2
SELECT DISTINCT author_id FROM pull_requests WHERE repo_id=2;  -- 2
SELECT DISTINCT reviewer_id FROM pr_reviews WHERE repo_id=2;   -- 2
```

修复前同一操作写入的全是 `1`。

## 7. 正式评审（缺陷 8 的实测）

```sql
-- zdzso/agent2ssh, repo_id=1 in /tmp/t36v6
SELECT COUNT(*) FROM pr_reviews WHERE repo_id=1;                       -- 28
SELECT action, COUNT(*) FROM pr_reviews WHERE repo_id=1 GROUP BY action; -- approve 2, comment 26
SELECT p.number, r.action FROM pr_reviews r
  JOIN pull_requests p ON r.pr_id=p.id
  WHERE p.repo_id=1 AND p.number=3;                                    -- approve ×2
```

源侧基线：32 issues / 442 issue 评论 / 10 PR / 26 PR 评论 / 2 正式评审 — 逐项相等。

## 8. 限流实测

最终代码导入 ironforge 时：

```
429 响应：28 次
重试日志：28 次（1s/2s/4s/8s/16s/30s/30s）
重试后仍失败：0 次
```

修复前同样的导入曾出现 `pr_reviews 26 → 6`、单个 PR 正文为空，任务仍 `completed`。
两次并发导入是压力场景：6 次重试仍丢过 1 个正文，故提到 8 次。

## 9. 重复导入

同一源、同一目标再导一次（`import_repo=false`）：

```
issues_imported=0, issue_comments_imported=0, prs_imported=0, pr_reviews_imported=0
行数：32 / 206 / 34 / 26 不变
status: completed
```

没有重复行；也没有报冲突，而是按既有行跳过（stats 如实为 0）。
若要「明确报冲突」需要改共享导入器的去重语义，超出 CNB 段，未做；这里满足
「不产生重复行」的判定。

## 10. 测试

```
$ cargo test -p rg-core import
running 21 tests
test result: ok. 21 passed; 0 failed

$ cargo clippy --workspace --all-targets -- -D warnings
Finished `dev` profile (no warnings)
```

新增单测覆盖：双状态去重、PR 详情真实载荷、标签颜色/描述、标签去重映射、429 可重试集合、
评审状态映射、评审真实载荷。

## 11. 未覆盖与说明

- **labels 没有端到端真实数据**：本次所有可访问仓库标签数均为 0。用 `cnb/feedback`
  的真实标签 JSON 做了字段与单测验证（33 个标签、颜色 `#E8FCF4`、22 个未被 issue 引用），
  没有为了测试去给他人的仓库加标签。
- **milestone**：CNB 无此概念，`start_import` 清零并记日志；不报错，因为对话框默认勾选。
- **`merged_at`**：CNB 不提供，只有 `closed_at`；落 NULL，`state=merged` 正确。
- **用户映射**：与 GitHub/GitLab 相同，全部归到导入者，不做逐用户身份匹配。
- **N+1 并发化**：按任务约束未做；本次只保证串行路径在 429 下不丢数据。
