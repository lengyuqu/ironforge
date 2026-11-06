# Agent 写路径缺口补完：`refs/pull` 评估、PR 导出、创建 PR dry-run、REST 提交的推送钩子（验证记录，2026-11-07）

> 接 [agent-write-path-verification-2026-11.md](agent-write-path-verification-2026-11.md)。那一篇的"未做"清单里有一部分靠核实发现并不成立，另有两个真缺口是在补的过程中被测试揪出来的。**邮件通知偏好与 CI 按负责人安排留到最后，本文不涉及。**

## 一、先说核实结果（和之前文档不一致的地方）

| 之前的说法 | 核实结果 |
|---|---|
| "服务端不写 `refs/pull/<n>/head`"（harness 设计 §2、CLI 设计 §2/§7.2） | **过期**。同仓 PR 创建时（`api/pulls.rs::create_pr`）和推送到 head 分支时（`lib.rs::post_push_hooks`）早就在写，提交 `a8b8103`。**fork PR 不写**（head 分支在 fork 里）。此前没有任何测试覆盖它。 |
| "`wait_pipeline` 成功路径只验证过 stub" | 现在用真实服务 + 主机执行的 CI 验证了成功与失败两条路径（见第四节）。 |
| "PR `export` 依赖 `refs/pull`，要先评估 gc 与 mirror" | 评估完成，见第三节；`export` 已实现。 |

## 二、做了什么

### 1. REST 文件提交现在与 git push 一样跑推送钩子（**真缺口，已修**）

`POST/DELETE …/contents/{path}` 以前只写 git 对象和分支 ref，**不触发**：PR 的 `head_sha` 同步、`refs/pull/<n>/head`、CI 流水线、`push` webhook、`repo_events` 事件日志。后果：agent 用 MCP `commit_file` 提交后，`list_pipelines` 里永远没有这个提交的流水线，`wait_pipeline` 无从等起；PR 的存储 head 落后；`wait_repo_events` 看不到推送。（浏览器里的文件编辑器走的是同一个端点，同样没有触发过 CI。）

修法：把 `post_push_hooks` 的后台任务抽成 `lib.rs::spawn_post_push_hooks`，git push 路径与 contents 写路径共用；contents 写成功后用 `RefUpdate{old, new, refs/heads/<branch>}` 调它（`repo_content.rs::announce_branch_move`，解析不到新提交时跳过）。钩子在后台运行，响应不等它。

**行为变化，需要知情**：此后通过 API 写文件会触发 `push` webhook、`branch.created` webhook（新分支）、CI（有 `.ironforge-ci.yml` 时）、站内通知/邮件（若配置）。这是 git push 一直以来的行为，现在两条路一致。

### 2. 往"还不存在的分支"提交的文件检查（**真缺口，已修**）

分支不存在时，提交会从默认分支 HEAD 建分支，但**检查**读的是不存在的分支：更新已有文件报 `file does not exist`（404），"创建"一个默认分支上已有的文件被放行（在新分支上悄悄覆盖）。修法：分支不存在时检查看 `HEAD`（`create_or_update_file_with` 的 `lookup_ref`）。**行为变化**：以前能"创建"的这种请求现在是 409 `file already exists`。删除不建分支，行为不变。

### 3. `refs/pull` 评估结论（设计文档待决问题 6）

- **gc**：服务端没有 gc/repack 任务（`grep` 全仓库无），对象不会被回收；`refs/pull/<n>/head` 又是普通 ref，所以只要 ref 在，commit 就在。
- **mirror**：mirror 同步是 `git remote update --prune`（refspec `+refs/*:refs/*`）。镜像仓库里本地写入的 `refs/pull/*` 在下一次同步会被当成"上游没有"而删掉——与该仓库里所有本地 ref（包括本地分支）同样的处理，镜像仓库以上游为准。**结论**：不阻止在镜像仓库开 PR（平台现在也没有这个限制），但镜像仓库上的 PR 锚点和分支一样不可靠；这条没有改。
- **fork PR**：head 分支在 fork 里，本仓库没有锚点，`export` 里 `anchors.head_ref` 为 `null`。需要的话得把 fork 的对象取进本仓库再写 ref，未做。
- **分支删除**：服务端**没有删分支的 REST，git push 删除也被 receive-pack 拒绝**（`deletion not supported`）。所以"分支删除后锚点仍在"只能在测试里直接删 ref 来验证（`rg_git::ops::delete_ref`）；这也意味着设计文档里的"分支存续"问题目前是理论上的。

### 4. `GET /repos/{o}/{n}/pulls/{number}/export` 与 MCP `export_pr`

单文件 JSON（`format: ironforge.pull_request_export`，`format_version: 1`）：`pull_request`（元数据与正文）、`anchors`（`head_ref`/`head_ref_sha`、`head_branch_sha`、`base_tip_sha`、`base_sha_at_merge`=合并提交的第一个父、`not_stored: ["base_sha_when_opened"]`）、`reviews`、`review_comments`（含是否已解决、是否带建议）、`ci`（head 提交最新流水线：状态与每个 job 的状态/退出码，**不含日志**）、`counts`/`truncated`（评审 200、评论 1000、job 200 的上限，单段文字 20000 字符）。读权限；用户写的文字带 `origin: user_content`；各段是独立实时读取。

**没有存的东西**：PR 创建时的 base 提交（没有列）。开放 PR 给的是当前 base 分支尖端，已合并 PR 给合并时的 base。要做评测集需要的话要加列并迁移——未做，需要你定。

### 5. 创建 PR 的 `dry_run`

`POST …/pulls` 带 `dry_run: true` 或 `X-Dry-Run: 1`：跑与真实创建相同的检查（标题、head≠base、milestone 归属、解析 head），返回会创建什么（`number`、head/base 的提交、是否会发布 `refs/pull`、`existing_open_pull_requests`），什么都不写。实现是 `rg_core::pull_request::plan_pr`，`create_pr` 自己也调它（检查与取值只有一份）。**不预测**：webhook、通知、CODEOWNERS 评审请求（`not_evaluated`）。MCP `create_pr` 新增 `dry_run` 参数。注意：平台**不拒绝**同一对分支的重复开放 PR，dry-run 只是报告。

### 6. 审计覆盖

新增共享的 `api::audit::record_write`（带 IP/UA 和 `access_token_id`），用于：`repo.file_commit`、`repo.file_delete`、`pull_request.create`、`pull_request.merge`、`pull_request.review`。原有 5 处审计写入各自保留自己的拷贝（未合并，改动面太大）。**仍然没有审计的写入**：issue 与评论、标签、wiki、releases、分支保护、webhook 配置、git push 本身等。这些是否要审计是产品决定。

## 三、不做什么 / 未覆盖

- **fork PR 没有 `refs/pull`**，`export` 对它的锚点为 `null`。
- **镜像仓库**上的 `refs/pull` 会被同步清掉（见上）。
- **SSE、streamable-HTTP MCP 传输没做**。当前只有 stdio 与长轮询，对本机/同机 agent 足够；远程 agent 才需要后两者，是单独的任务（需要认证、会话、反向代理不缓冲等部署约束）。
- `expected_head_sha` 的合并前检查仍然非原子（见前一篇）。
- 删除分支没有 REST，也没有 git 删除。
- `base_sha_when_opened` 没存。
- 真实运行用主机执行的 CI（`--allow-host-execution`），没有 Docker runner；脚本只在 SQLite、单机跑过。

## 四、证据

| 层级 | 结果 |
|---|---|
| 任务验证（SQLite） | `pr_export_tests` 7 项（真实 HTTP、真实 `git`）：同仓 PR 的 `refs/pull/1/head` 指向 head、`git push` 推进后跟着走、直接删掉 head 分支后仍在且可 `git fetch`、export 仍给出锚点；**REST 提交推进分支时 `refs/pull` 与 PR 存储的 head 一起走**（这条在修复前失败）；REST 提交进入事件日志；export 含评论、CI job（无日志）、锚点，不存在的 PR 404，私有仓库对匿名/陌生人拒绝；创建 PR dry-run 不写 PR 也不发布 `refs/pull`、头部与体两种开关、相同的拒绝（同分支、空标题 400）、缺失 head 如实报告、已有开放 PR 时给出重复与下一个编号；令牌写入的 `repo.file_commit`/`pull_request.create`/`pull_request.merge` 审计带真实 `access_token_id`，普通登录的不带。`file_commit_semantics_tests` 新增 1 项：新分支上更新已有文件成功（dry-run 与真实）、"创建"已有文件 409、错 blob 409、未知文件 404、`main` 不动（修复前更新用例 404）。`write_tools_mcp` 新增 1 项（`create_pr` 的 `dry_run`、`export_pr` 路由）。 |
| 运行验证（真实服务 + 真实 `ironforge-mcp` + 主机执行的 CI） | `scripts/agent-e2e.py`：受限令牌（`repos` + `refs: [agent/*]`）下 ① `commit_file` dry-run 不建分支、真提交建 `agent/pass`、同 `idempotency_key` 重试回放同一提交；② 该提交出现流水线，`wait_pipeline` 返回成功；③ 把 CI 脚本改成 `exit 3` 的提交，`wait_pipeline` 返回失败且摘要含脚本打印的标记；④ `create_pr` dry-run 计划 #1、真调用与带 key 重试只得到一个 PR；⑤ `get_pr_context` 的 head 与 CI 与 export 一致，export 的 `anchors.head_ref` 是 `refs/pull/1/head` 且指向 head；⑥ `wait_repo_events` 取到两次提交的 `push` 事件；⑦ `commit_file` 写 `main` 被 403。脚本在第一次真实跑时就发现了上面第 2 条（新分支更新文件 404）。 |
| 后端验证（PostgreSQL 17 / MySQL 8.4，真实容器，每个目标全新数据库） | `scripts/ci-db-tests.sh postgres agent` 与 `mysql agent`：`pat_ref_allowlist_tests` 7、`file_commit_semantics_tests` 5、`idempotency_tests` 5、`repo_events_tests` 5、`pr_context_tests` 4、`pr_export_tests` 7，两个后端全部通过。这 6 个目标是 `rg-http` 里仅有的会读 `IRONFORGE_TEST_DATABASE_URL` 的（其余目标一律 SQLite），脚本新增 `agent` 子集并把它们并入 `all`。 |
| 全量（SQLite） | `cargo test --workspace --no-fail-fast`：133 个测试程序、1172 项通过、0 失败、11 项 ignored（需外部数据库的冒烟测试）；`cargo clippy --workspace --all-targets -- -D warnings` 干净，`cargo fmt` 已应用。前端未改动，未重跑。 |

## 五、脚本里踩到的两个坑（供以后写脚本/测试的人）

- `GET /repos/{o}/{n}/pulls?state=all` **不是**"所有状态"：`state` 按字面值过滤，`all` 匹配不到任何 PR，返回空列表。要全部就别带 `state`。（我自己的第一版测试因此有一条空洞的断言，已改成能看到真实 PR 的版本。）
- 读文件的 blob sha 用 `GET …/blob/{path}?ref=`；`GET …/contents/{path}` 只有写方法（405）。
