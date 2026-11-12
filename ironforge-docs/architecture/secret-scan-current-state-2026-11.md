# R7 推送时密钥扫描 — 现状核实（2026-11-13）

> 核实对象：issue #38 正文（拆自 #13，设计文档 §5.2 R7，**2026-11-10 快照**）。
> 基线：`cnb/main` = `f91dbf9`。所有 `path:line` 均在该 SHA 上 `sed -n` 打印核对过。
> 本轮边界：**零代码行为改动**（L2 条目只记录，不改 schema / 契约 / 认证语义）；issue #38 保持 open。
> NPC 只读首扫请求 `2120990742129610752`（2026-11-12T21:22:53Z）`60 s × 5` 未回 ⇒ 按正文 + 本机复算推进，未重复发。

## 1. 判定表

| # | 断言（issue 正文 / 设计文档 §5.2 R7） | 判定 | 证据 |
|---|---|---|---|
| 1 | 全仓没有内容扫描实现（`rg-core/src/secret_scan.rs` 不存在） | **成立** | `grep -rn 'secret_scan\|scan_secret\|SecretScan' crates/` = 0 命中；`ls crates/rg-core/src/` 无该文件 |
| 2 | receive-pack 路径零内容扫描 | **成立** | `receive_pack.rs:466-483` 唯一接触 pack 的段落只做 `index_pack_bytes`（`ops.rs:1040`，gix 原生）+ 签名策略，无 blob 内容检查 |
| 3 | REST 提交「`create_or_update_file_with` → `commit_tree_edits`」是唯一链 | **不成立** | `rg-core/src/repo/service.rs` 有 **4 处** `commit_tree_edits`：`:1799`→`:1917`、`update_files_in_commit:1980`→`:2034`、`delete_file_with:2095`→`:2161`、`commit_batch:2362`→`:2574`；入口 handler 另有 `rg-http/src/api/repo_commits.rs:176`。漏 3 条（`delete_file_with` 不产生新 blob，可不算扫描目标） |
| 4 | 「三处与 secret 相关」都不扫内容 | **成立** | ①`collaboration/secret.rs:13` `SECRET_PREFIX`（只生成/摘要）②`entities/ci_secret.rs:12` `encrypted_value`（只加解密）③`rg-http/src/redact.rs:90` `redact_server_paths`（**服务器路径**脱敏，与密钥无关） |
| 5 | 拒绝是纯文本 report-status（`:680` `ng <ref> <message>`） | **成立** | `receive_pack.rs:680` `format!("ng {} {}", result.refname, result.message)` |
| 6 | `reason`/`hint` 是 REST 错误体，位置 `error.rs:29-39` | **部分成立** | 语义对、指针错：`reason`/`hint` 在 `rg-http/src/error.rs:61-74`（`AppError::Coded`）+ `.coded()`（`:231`）/`.with_reason()`（`:245`）；`:29-39` 是 `CoreError::Forbidden/Conflict/InvalidInput` |
| 7 | 扫描器放 `rg-core/src/secret_scan.rs` | **不成立（会返工）** | `rg-git/Cargo.toml:9-25` 依赖只有 tokio/anyhow/tracing/gix/gix-archive/gix-pack/thiserror；`rg-core/Cargo.toml:10-11` 才是反向依赖 `rg-db`/`rg-git` ⇒ push 内（`rg-git`）调不到 `rg-core` 的扫描器 |
| 8 | 超限跳过并「记 `repo_event`」 | **部分成立** | 事件表可写（`repo_event_ops::append`，`rg-db/src/ops/repo_event_ops.rs:8`；`event_type` 自由 64 字符，migration `m20261106_000004:57-60`）；但 `rg-git` **无 DB**（`receive_pack.rs:18` 原文：「`rg-git` has no database, so the policy … lives with the caller」），且 `record_repo_event` 是 `rg-core/src/webhook/service.rs:227` 的**私有** fn ⇒ 只能调用方写 |
| 9 | 命中后 push 侧 `ng <ref> secret detected in <path>:<rule>` | **成立（有通道）** | 在 `:481`（`enforce_signed_commit_policies`）之后、`:486` 起写 ref 之前设 `status="error"` + `message` 即由 `:680` 出到 report-status |
| 10 | REST 侧返回稳定 `reason = SECRET_DETECTED` | **部分成立** | 通道存在（`AppError::Coded`），但**走 commit 路径的错误类型是 `CoreError`**（`rg-core/src/error.rs:24-44`，5 个变体全部只有自由文本、无 code/reason）⇒ 需加变体或 handler 侧映射；`collab` 侧已有桥先例（`collaboration/error.rs:50-56` `CodedConflict` → `rg-http/src/error.rs:318`） |
| 11 | issue 依赖 R6（#18 临时 ref 与预检 CI） | **部分成立（排期，不是技术）** | push 侧硬拒只需要「pack 已 index」+「ref 未写」两个条件，两者今天都成立；预检 CI 只影响「PR 阶段提前告警」。R6 未落地不阻塞 R7 的 push 侧（`receive_pack.rs:481` 与 `:486` 之间的窗口就是插入点） |
| 12 | 完成口径「新 SQL 至少跨后端跑一次」 | **不成立（无对象）** | 本方案零迁移：`event_type` 无枚举/CHECK（migration `:57-60`）、规则集在代码里、允许列表在代码/配置里 ⇒ 除非新增告警表或列 |
| 13 | 单文件 ≤1 MiB、单次 ≤200 文件这类限额有先例可抄 | **不成立** | 现有限额全是 **body 级**：git smart HTTP 4 GiB（`lib.rs:50`，`:451` 施加）、LFS 10 GiB（`:528`/`:965`）、runner cache 1 GiB（`:619`）、附件 101 MiB ×4（`:823-865`）；`grep -rn 'maxItems' crates/` = 0，`commit_files`（`rg-mcp/src/tools/mod.rs:131`）无条数上限 |

判定分布：**成立 6 / 部分成立 4 / 不成立 3**（13 条）。

## 2. 三个可用插入点（按「能不能拒」排序）

| 位置 | 能不能拒 | 代价 | 备注 |
|---|---|---|---|
| **A. `receive_pack.rs:481` 之后**（`if has_content_updates` 块内，pack 已 index、ref 未写） | **能**（`status="error"` → `:680` 出 `ng`） | 同步阻塞 push；pack 已在内存（`:472` `read_to_end`） | 唯一硬拒位置。模板 = `enforce_signed_commit_policies`（`:521`，gix 原生、失败即 `error`） |
| **B. `rg-core/src/repo/service.rs` 4 处 `commit_tree_edits` 之前** | 能（返回 `CoreError`） | 每处一行编排 | 3 处有内容（`delete_file_with` 无新 blob） |
| **C. `rg-http/src/lib.rs:3089` `ref_updates` 回来之后** | **不能**（ref 已写） | 低 | 只能事后记账；`spawn_post_push_hooks`（`:3095`）已是这条路的先例 |

`:481` 与 `:486` 之间**没有任何 await 之外的早退**，这是本设计能成立的前提。

## 3. 分层约束（本轮最贵的一条）

- `rg-git` 没有 `rg-db`/`rg-core` 依赖（`Cargo.toml:9-25`），所以 **扫描器不能放 `rg-core/src/secret_scan.rs`**：要么放 `rg-git`，要么由 `rg-http`/`rg-ssh` 以回调形式注入。
- 注入式回调在本文件**已有先例**：`BranchDeleter`（`:18-27`，doc 原文就是「`rg-git` has no database, so the policy … lives with the caller」）+ `rejected_refs` / `require_signed_refs` / `writable_refs` 四个入参。
- `RefUpdate`（`:32-38`）只有 `old_sha/new_sha/refname/status/message` ⇒ **结构化命中（path/rule/severity）无处安放**。要「警告模式记事件」就得扩这个结构（`rg-http` 与 `rg-ssh` 两侧都消费）或走回调。

## 4. 「只扫新增 blob」的现成零件

- `rg-git/src/ops.rs:531` `commits_between(repo, new, old: Option<Id>)` —— `old = None`（新分支）已在 `enforce_signed_commit_policies:562-571` 处理。
- `rg-git/src/ops.rs:50` `blob_at`；树 diff 范式在 `rg-core/src/pull_request/gix_patch.rs:67`（`old_tree.changes()`），`rg-git` 侧可直接用 gix 同款 API。
- 结论：扫描器不需要新依赖（`gix` 已在 `rg-git` 里），也没有现成的「changed paths」函数可复用 ⇒ 这段是净新增代码。

## 5. 限额与「超限跳过」的口径风险

- 现有限额是 **body 级**（§1 第 13 条），单文件 1 MiB 属**新维度**，没有先例可对齐。
- 「超限跳过」= **fail-open**。叠加「默认只警告不硬拒」后，结果是：超过 200 文件的推送**永远不被扫**，且只在事件流里留一行 ⇒ 攻击者只要一次推 201 个文件即可绕过。本条应与「默认警告」分开决策（建议：超限只跳过**超大文件**，条数超限改为拒绝或分批扫，而不是整批跳过）。

## 6. 无迁移对象

`repo_events.event_type` 是 `string_len(64)` 的自由字符串（migration `m20261106_000004:57-60`，无枚举、无 CHECK），`repo_event_ops::append`（`:8`）直接收 `&str` ⇒ 加 `secret.detected` **不需要迁移**。完成口径里「新 SQL 至少跨后端跑一次（`IRONFORGE_TEST_DATABASE_URL`）」在本方案下**没有对象**；`check.sh --quick` 就是 fmt + clippy `-D warnings`（`scripts/check.sh:36-43`；`:45` 起是 `--quick` 跳过的测试段），完成口径的第一条已由 CI 覆盖。

## 7. 两份材料都没写的一条：`ifs_` 前缀

`rg-core/src/collaboration/secret.rs:12-13`：

```rust
/// Prefix that makes the secret recognisable to secret scanners and log filters.
pub const SECRET_PREFIX: &str = "ifs_";
```

本仓自己的协作会话密钥已经带扫描器可识别前缀 ⇒ 规则集里**必须**有 `ifs_` 规则，否则「本仓自己发的凭据」是扫描器唯一的系统性盲区。反向也成立：仓库里出现 `ifs_` 前缀 = 该凭据泄漏。

## 8. L2 清单（只记录，不改码）

- **R38-1（P1）扫描器位置**：必须落 `rg-git`（或新 crate），`rg-core/src/secret_scan.rs` 会让 push 侧调不到。改位置 = 改设计文档 `:545` 的第一句。
- **R38-2（P1）警告模式的落点**：`RefUpdate`（`:32-38`）无结构化字段，扩结构要同改 `rg-http`/`rg-ssh` 两侧消费点；否则「记事件」只能记自由文本。
- **R38-3（P2）REST 稳定 reason**：`CoreError`（`rg-core/src/error.rs:24-44`）无 code/reason 变体 ⇒ 首个用户要么加变体（契约变更），要么在 handler 映射到 `AppError::coded`（`rg-http/src/error.rs:231`）。
- **R38-4（P2）超限 fail-open**：见 §5，需要单独口径（跳过 vs 拒绝），并把这个决定写进响应（`details` 或响应头），不能只写事件。
- **R38-5（P2）多 ref 语义**：一次 push 可带多个 ref，`:486` 的循环逐个判；同一 commit 被 N 个 ref 指向时是「每个 ref 各出一条 `ng`」还是「一条汇总」需要定。
- **R38-6（P3）成本**：`:472` 已把整个 pack `read_to_end` 进内存，扫描再加一次全量 tree 遍历 + blob 读取，大仓 push 的峰值内存要实测（`MAX_GIT_BODY_BYTES` = 4 GiB，`lib.rs:50`）。
- **R38-7（P3）`ifs_` 规则缺失 = 自漏**（§7）。
- **R38-8（P3）完成口径「新 SQL 跨后端」无对象**（§6）。

## 9. 复核命令

```bash
git -C <repo> rev-parse --short HEAD                     # 应为 f91dbf9 或其后代
grep -rn 'secret_scan\|scan_secret\|SecretScan' crates/   # 0 命中
grep -n 'commit_tree_edits' crates/rg-core/src/repo/service.rs   # 4 处调用 + 定义
grep -rn 'maxItems' crates/                               # 0 命中
sed -n '18p;32,38p;466,483p;521p;680p' crates/rg-git/src/protocol/receive_pack.rs
sed -n '24,44p' crates/rg-core/src/error.rs                # CoreError 5 变体、无 code
sed -n '61,75p;231p;245p' crates/rg-http/src/error.rs      # AppError::Coded + coded/with_reason
sed -n '12,13p' crates/rg-core/src/collaboration/secret.rs # SECRET_PREFIX = "ifs_"
```
