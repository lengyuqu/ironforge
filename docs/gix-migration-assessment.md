# gix 0.84 → 0.87.1 升级评估与 CLI 替换迁移路线

> 日期：2026-09-06 | 基于 gitoxide 官方 release notes（v0.85.0 / v0.86.0 / v0.87.0 / v0.87.1）
> 复查节奏：CLAUDE.md 约定"每次 gix 版本升级时过一遍"，本文即该次复查的产出。

---

## 一、0.85 → 0.87 更新要点（按对 IronForge 的价值排序）

### 0.87.0 —— 直接命中本项目 CLI 保留清单

| 新能力 | API | 对 IronForge 的意义 |
|---|---|---|
| **Git 兼容提交签名** | `Commit::sign()` | 解析 gpg.format / 每格式程序 / 签名 key / committer 回退 / `gpg.ssh.defaultKeyCommand`，OpenPGP + X.509 + SSH 全支持 |
| **Git 兼容提交验签** | `Commit::verify()` / `commit::SignedData::verify()` | **CLAUDE.md 等待表"GPG 验签"一行就此关闭**——原先计划引入 sequoia-openpgp，现在 gix 内建（签名流式送入 verifier，无需重建 payload） |
| **批量删本地分支** | `Repository::delete_local_branches()` | 事务内一次删 refs+reflog，拒绝任何 worktree 检出中的分支 |
| **Git notes 瓷器层** | `Repository::notes` | notes 读写 + CAS 更新（防并发覆盖） |
| **单 revision clone** | `PrepareFetch::with_revision()` | 对齐 git 2.5x `--revision=` 行为：detach HEAD、不建普通 refs、不持久 refspec |
| **GIT_ALLOW_PROTOCOL** | 传输层 | 网络协议白名单安全语义对齐 git |
| 其他 | `gix::config()`、editor 解析、Git quoting 工具、`commit::Info::generation` | — |

### 0.86.0 —— Windows 性能 + API 工效大版本

| 新能力 | 意义 |
|---|---|
| **Windows dir-cache 加速 status + 懒加载线程局部 `core.fscache`** | IronForge CI runner 本地执行 / 仓库扫描在 Windows 主机上的文件遍历性能直接受益 |
| **gix-config 全 owned 化（lifetime-free）** | 配置值返回 owned `BString`/`PathBuf`/`OsString`，消除 `Cow`/`into_owned()` 样板；错误可自然 `?` 传播 |
| **zlib 压缩级别感知** | 遵循 core.compression / pack.compression（含 git 的 -1 映射）——pack 生成调优可用 |
| **多 remote URL** | `Remote::urls(Direction)` 保留全部 fetch/push URL 并保序 |
| **credentials 强化** | `Connection::configured_credentials_for_current_url()`、`credential.protectProtocol` |
| `Repository::normalize_path()` / `discover_opts()` | 替代 `Pattern + normalize` workaround |

### 0.85.0 —— 正确性修复为主

- clone 采用远端 object format（sha256 兼容）并限定重试次数（`IncompatibleObjectHash`）
- fetch 对远端 symref 的解析对齐 git（`+HEAD:...` 不再错误解析到本地同名分支）
- 松散 ref 路径前缀碰撞（`refs/heads/A` 文件 vs `refs/heads/A/new`）
- 浅 clone 的 tag refspec；相对 worktree gitdir 文件
- tree editor `Editor::remove_leaf()`、`write_object_with_known_id()`

---

## 二、现状盘点：生产代码中经 GitCommandGateway 保留的 CLI

（tests 排除；gateway 自身的 `--version` 健康检查与"no raw git"守卫不计入迁移对象）

| 命令 | 处数 | 位置 | gix 替代 | 难度 |
|---|---|---|---|---|
| `rev-parse` | 6 | merge_queue / pull_request service / repo service | `repo.rev_parse_single()`（revspec 解析已成熟） | 低 |
| `update-ref` | 2 | merge_queue（group ref） | `repo.edit_reference()` / refs 事务 | 低 |
| `show` | 2 | review/codeowners（`show base:CODEOWNERS`） | `repo.find_object()` + tree 遍历 | 低 |
| `cat-file blob` | 1 | issue_template | 同上 | 低 |
| `init -b` / `add -A` / `commit` | 1+1+4 | repo service（auto_init 种子提交） | gix init + index 更新 + `repo.commit()` | 中 |
| `push` | 2 | auto_init / rebase worktree | `PreparePush`（gix push 已成熟） | 中 |
| `clone --no-checkout` / `fetch` / `checkout --detach` | 2+1+1 | rebase worktree、file 检出 | `PrepareFetch` + worktree checkout（0.85/0.86 大量正确性修复后已稳） | 中 |
| `archive` | 1 | repo_content/archive API | `gix-worktree-stream`（tar 流） | 中 |
| `pack-objects --all --stdout` | 2 | upload_pack / protocol v2（服务端 pack 生成） | gix-pack 可达性遍历 + bundle 写出，但缺 server 端协商高层封装 | 高 |
| `index-pack --fix-thin --stdin` | 1 | receive_pack（thin pack 补全入库） | gix-pack data::input 支持借 ODB 补 base；需自写索引管线 | 高 |
| **rebase** | 整条链 | pull_request service（temp worktree） | **gix 至今无 rebase API**（0.85–0.87 均未涉及） | ❌ 保留 |

---

## 三、迁移路线建议

**Phase A（低风险速赢，~1 天）**：`rev-parse` / `update-ref` / `show` / `cat-file`
→ 纯读操作，gix API 直换，行为可用现有集成测试回归。

**Phase B（0.87 新解锁，~2 天）**：
1. GPG/SSH **验签走 `Commit::verify()`**——若代码中尚有验签 CLI 则替换；新功能可直接用 gix 内建，sequoia-openpgp 依赖不用引
2. auto_init 种子流程 gix 化（init/add/commit/push）
3. rebase worktree 的 clone/fetch/push 环节 gix 化——**仅 rebase 本身保留 CLI**，把本周暴露的
   Windows 子进程环境类 bug 面再压一层

**Phase C（服务端协议，3~5 天）**：`pack-objects` / `index-pack --fix-thin`
→ 用 gix-pack 自研 pack 生成与 thin 补全入库；收益是服务端协议零 git 依赖，但这是性能敏感路径，需基准对比。

**Phase D（等待上游）**：rebase——CLAUDE.md 等待表唯一无法关闭项。

### 建议同步更新 CLAUDE.md

- 依赖速查：`gix = "0.84"` → `"0.87"`（本次已升级）
- 等待表"GPG 验签 | gix 无验签" → 已由 0.87 `Commit::verify()` 内建解决，可关闭
- 保留清单改为：Rebase（等 API）/ Pack 生成 / Thin-pack 索引（Phase C 自研或继续等待高层封装）

---

## 四、升级本身的行为影响

- 仓库内 gix 调用 API 全兼容，`cargo check --workspace` 零错误，无需改代码
- 认证：`cargo test --workspace --no-fail-fast` 全绿（63 个测试二进制 0 failed）
- 0.86 的 owned-config 破坏性变更未波及本项目调用面（我们主要走瓷器层）

---

## 五、Phase A+B 执行结果（2026-09-06）

**Phase A（低风险速赢）— 完成**：
- 新增 `rg_git::ops` 模块（`rev_parse` / `try_rev_parse` / `blob_at` / `update_ref` / `delete_ref` / `verify_commit[_with_repo]`），gix 0.87.1 原生实现，错误语义与 CLI 对齐
- `rev-parse`：merge_queue（base ref）、pull_request/service（rebase worktree HEAD）、repo/service（clone 校验 / blob 校验 / 新提交 SHA）、issue_template（`--verify ^{commit}`）全部迁移
- `update-ref` / `update-ref -d`：merge_queue 的 merge-group ref 写入与清理迁移
- `show` + `rev-parse <rev>:<path>`：review/service 与 codeowners 合并为单次 `blob_at` 调用
- `cat-file blob`：issue_template 模板/配置读取迁移

**Phase B — B1、B3 完成，B2 有意缩减**：
- **B1 auto_init 全量 gix 化**：不再经临时 worktree + init/add/commit/push/symbolic-ref CLI 链，直接在 bare 仓库内 `write_blob` → 构建 Tree → `commit_as`（显式 identity，`refs/heads/<branch>` 创建即校验 MustNotExist）→ `edit_reference` 设 HEAD symbolic。删除约 100 行 CLI 编排
- **B3 验签迁移**：`git verify-commit`（receive_pack 签名策略）替换为 gix `Commit::verify_signature()`；gix features 增加 `command`。未签名提交短路返回 `Ok(None)`，不触发 gpg；验证失败按 fail-closed 处理，与 CLI 语义一致
- **B2 rebase 网络环节缩减**：clone/fetch/checkout/push 保留 CLI。原因：rebase 依赖 `origin/<branch>` 远程跟踪 ref 与 remote 配置，gix `PrepareFetch`/`PreparePush` 单独替换 clone/push 会在 ref 映射与配置写入面引入回归风险，收益不匹配；最终 `rev-parse HEAD` 已随 Phase A 迁移。待 gix rebase API 或 Phase C 一并处理

**认证**：`cargo test --workspace --no-fail-fast` 414 passed / 0 failed；clippy 目标 crate 0 警告。

**注意**：`ops.rs` 文档注释避免出现 regression guard 扫描的裸 git 进程创建字面量（已踩一次：guard 连注释一起扫）。

---

## 六、Phase C-lite：直接可换点位收尾（2026-09-06）

在 Phase C（pack 自研）之前，把评估表中"原语齐全、直接可换"的点位先吃掉：

- **merge_queue 的 `merge-tree --write-tree` + `commit-tree`**：gix `merge_commits` + `tree.write()` + `new_commit_as`（不触引用，等价 commit-tree），冲突仍走 finish_entry Failed。gix Repository 是 !Send，整段放进 `spawn_blocking`
- **issue_template 的 `ls-tree -z --name-only`**：`lookup_entry_by_path` + `Tree::iter()`，缺失/非目录行为与 CLI 对齐（返回空）
- **`git archive`（archive API + runners 产物）**：`repo.worktree_stream` + `repo.worktree_archive`（gix-archive），Tar/TarGz/Zip 三格式齐备；mtime 用提交时间；workspace gix 增加 `worktree-archive` feature，rg-git 直依赖 `gix-archive`（default-features=false + tar/tar_gz/zip）借 feature unification 开启 gix 内 gix-archive 的格式支持

认证：全 workspace 测试通过（唯一失败为 webhook 重试的时序 flaky，单独重跑两次均绿）；rg-git/rg-core/rg-http clippy 无新增警告。

剩余 CLI（生产路径）：rebase 及其 worktree 的 clone/fetch/push、update_files_in_commit 的 clone/add/commit/push、merge_queue fork fetch、receive_pack rev-list/index-pack、upload_pack pack-objects。

---

## 七、Rebase 自研完成 —— CLI 仅剩 pack 链（2026-09-07）

`git rebase`（PR Rebase and merge 策略）以 gix 原语自研实现，替换整条
`clone --no-checkout / fetch / checkout --detach / rebase / push` 临时 worktree 链：

- **重放集合**：`rev_walk(head).with_hidden(base)`（等价 `rev-list base..head`），本地 Kahn 拓扑排序
  （gix walk 无 topo 变体），优先级为最旧提交优先
- **语义对齐 git rebase 默认**：merge commit 线性化丢弃；空提交（重放后树不变）跳过；
  作者身份与消息原样保留，committer=IronForge
- **cherry-pick 核心**：`merge_trees(ancestor=原父树, our=运行头树, their=原提交树)` 显式祖先三方合并
- **并发保护**：`edit_reference` + `PreviousValue::ExistingMustMatch(old_base)`，等价原
  fast-forward push 被拒语义；冲突时 base ref 完全不动
- **切换就绪**：单函数薄适配（`RebaseOutcome::{Rebased, Conflict, BaseAdvanced}`），
  5 个 characterization 测试钉死语义，gix 出原生 rebase API 时按同一断言验收替换
- 删除 `git_rebase_merge` 的临时 worktree 编排（-90 行），fork PR 的 `refs/forks/...` ref 直接可作 head_rev

残余 CLI（生产路径）仅剩：receive_pack `index-pack --fix-thin` + `rev-list`（签名枚举）、
upload_pack `pack-objects`（Phase C 自研）、update_files_in_commit 与 merge_queue fork 的
clone/fetch（PrepareFetch 可达，可选）。

认证：5 个 characterization 测试 + `merge_squash_and_rebase_update_refs_and_pr_state` HTTP 集成测试全绿。
