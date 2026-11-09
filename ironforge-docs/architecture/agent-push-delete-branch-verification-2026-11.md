# `git push --delete`：与 REST 删分支同一套规则（验证记录，2026-11-10）

> 基线 main `04433f8`。此前 `git push origin :branch` 被 receive-pack 直接拒绝（`deletion not supported`），删分支只有 REST `DELETE /repos/{o}/{n}/branches/{branch}` 一条路。本任务是让 receive-pack 接受删除命令，并**复用同一个守卫**，不是新开一个入口。

## 一、行为

| 情形 | 结果 |
|---|---|
| `refs/heads/<x>` 删除，允许 | ref 删除，report-status `ok`（message `deleted`） |
| 默认分支 / 受保护分支 / 有未关闭 PR 的分支 | 拒绝，report-status `ng`，消息就是 REST 那句（`the default branch cannot be deleted` / `the branch is protected by rule …` / `N open pull request(s) still use this branch`） |
| `refs/tags/*` 与其他命名空间 | 仍然拒绝（`only branches can be deleted`）——标签保护有自己的规则，没有顺手放开 |
| 受限令牌（`refs: ["agent/*"]`） | 只能删白名单内的分支：白名单过滤在删除之前就生效，越界那一条在 report-status 里被拒 |
| 成功后 | HTTP：写审计 `repo.branch_delete`（`details.via = "git_push"`，带 `access_token_id`）+ 触发 `branch.deleted` webhook；SSH：只记日志（无令牌、无请求头可记） |

一个 push 里混着「新建/更新 ref + 删除 ref」也支持：pack 只在新对象存在时才读取，删除在 ref 更新之后逐条走守卫。

## 二、改动

| 文件 | 改动 |
|---|---|
| `rg-git/src/protocol/receive_pack.rs` | 新类型 `BranchDeleter`（异步回调）；四个入口增加 `on_delete` 参数；零值 new SHA 不再直接报错，而是收作「待判定」；应用阶段区分删除（调用守卫）与更新；全部是删除时不读 pack；签名策略跳过删除 |
| `rg-core/src/repo/branch.rs` | 新增 `delete_branch_message(&DeleteBranchError) -> String`：REST、HTTP push、SSH push 三处共用同一句文案 |
| `rg-core/.../service.rs` | 无改动 |
| `rg-http/src/lib.rs` | receive-pack 处理里构造守卫：调 `rg_core::repo::branch::delete_branch`（同一函数、同一守卫），成功后写审计与 webhook |
| `rg-ssh/src/lib.rs` | 同样构造守卫（无令牌、无审计，记日志） |
| `scripts/git-protocol-e2e.sh` | 新增删除用例：HTTP 与 SSH 各删一条、默认分支被拒、受保护分支被拒、标签被拒 |

`rg-git` 不依赖 `rg-core`（方向相反），所以策略通过回调注入；`rg-git` 只负责「解析命令、调用守卫、把拒绝原因写进 report-status」。

## 三、两个真正的根因（第一次跑不通，错在这里）

1. **`delete-refs` 没有被广告**。`push --delete` 只有在服务端的 receive-pack 广告里看到 `delete-refs` 才会发出删除命令；看不到时 git 客户端直接拒绝，**一个 POST 都不会发**（服务端日志里连请求都没有，只有客户端一句没有原因的 `! [remote rejected]`）。而且这两份广告是分开的：rg-git 里给 SSH 用一份 caps 串，`rg-http/src/lib.rs` 里给 Smart HTTP 另写了一份 —— 只改一处等于没改。
2. **git 的删除命令有两种写法**。知道旧值时是 `<old> 0000… <ref>`；不知道时是简写 `:<ref>`（两个 id 都省略）。旧解析器只认三列，简写会被 `parts.len() < 3` 直接丢掉，于是服务端返回空的 report-status，客户端只看到「remote rejected」而没有原因。

两处都修好后删除才通。这也是为什么先按 `scripts/git-protocol-e2e.sh` 的矩阵跑、而不是只写单元测试：这两个问题都在协议握手层，单测碰不到。

## 四、证据（手动实测，HTTP，debug 二进制 + 临时 SQLite）

```
$ git push origin --delete feat
 - [deleted]         feat                      # 删掉了，ls-remote 里只剩 main
$ git push origin --delete main
 ! [remote rejected] main (the default branch cannot be deleted)
$ git push origin --delete v1
 ! [remote rejected] v1 (only branches can be deleted)
```

`scripts/git-protocol-e2e.sh`（release 二进制）整条矩阵 **exit 0**，新增的删除段包含：

| 用例 | 结果 |
|---|---|
| HTTP `push --delete delete-over-http` | 成功，`ls-remote` 里消失 |
| SSH `push --delete delete-over-ssh` | 成功 |
| HTTP 删 `main` | 失败，客户端看到 `default branch` 字样，`main` 仍在 |
| HTTP 删受保护分支 `release/keep`（保护规则经 REST 创建） | 失败，客户端看到 `protected` 字样 |
| HTTP 删标签 `delete-me-tag` | 失败，客户端看到 `only branches can be deleted` |

其它检查：`cargo fmt --all --check` 干净；`cargo clippy --workspace --all-targets -- -D warnings` 干净；`cargo test -p rg-git` 只含 2 个 ignored 的文档测试（该 crate 的 receive-pack 没有单测，协议行为由上面这条真实客户端矩阵覆盖）。

> 证据分层：矩阵是在 `cargo fmt` 之前跑的；之后只做了格式化（不改语义），没有为格式改动重跑 10 分钟的 release 构建。

## 四、回归面（动协议路径，列清楚）

1. **全部是删除时不读 pack**：以前任何 ok 更新都会 `read_to_end` 读 pack。删除命令不带 pack，若仍去读会卡住（SSH）或等 body 结束（HTTP）。现在按「有没有内容更新」决定。
2. **签名策略不再对删除生效**：删除的新 SHA 是零值，解析成 ObjectId 会失败；已跳过。
3. **`writable_refs` 过滤对删除同样生效**：删除命令被收作 `ok` 后走的是同一段过滤，所以令牌的分支白名单没有被绕过。
4. **失败仍是整条 push 失败**：所有 ref 都被拒时 report-status 全 `ng`，客户端退出码非零——与以前「deletion not supported」的表现一致，只是原因变了。
5. **`send_response` 的 sideband 封装没有改**（坑 2）：拒绝原因走的是同一条既有路径。

## 五、未覆盖

- **受限令牌的分支白名单没有用真实 push 覆盖**：e2e 脚本用的是登录 JWT，不是带 `refs: ["agent/*"]` 的 PAT。代码上删除命令走的是同一段 `writable_refs` 过滤（在守卫之前），但没有实测「越界分支被 403 式拒绝」这一条。
- **审计与 webhook 没有断言**：HTTP 路径调用了 `record_write`（`details.via = "git_push"`）与 `trigger_branch_deleted`，没有测试检查数据库里真的多了一行或 webhook 真的投递；SSH 路径按设计**不写审计**（无令牌、无请求头），只记日志 —— 这是一个已知的不对称。
- **一个 push 里同时删多个分支**没有单独测（代码逐条处理，逻辑上没问题）。
- **并发**没测：两个 push 同时删同一分支时，第二个会在守卫里得到 `NotFound`（`no such branch`），没有构造这个竞争。
- **客户端只测了本机 git 2.54**（macOS，Apple 打包版）；其它版本的删除命令拼写（`:ref` vs `<old> 0…`）没有逐一验证，两种拼写都在解析器里支持。
- **没有改 data 层**：删除仍然走 `rg_core::repo::branch::delete_branch`，没有新增迁移、没有新表，因此没有 PG/MySQL 相关验证（该函数的跨库行为已由 `branch_delete_tests` 覆盖）。
