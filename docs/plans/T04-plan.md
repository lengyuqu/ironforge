# T04 Execution Plan — R7 推送时密钥扫描（issue #38）

> 状态：实现完成，四条验收命令已在本机跑绿。基线 `04e8c87`，分支 `agent/t04-secret-scan`。

## 1. 范围

**做**

1. `crates/rg-git/src/secret_scan.rs`：规则引擎（私钥头 / 云 AK / 平台 PAT 硬拒，JWT / 通用高熵记事件），允许列表（路径 glob + 行内标记），限额（单文件 1 MiB、单次 500 文件）。
2. `receive_pack.rs`：扫描插在签名策略之后、改 ref 之前（原 `:481` 与 `:486` 之间），命中硬规则把该 ref 改成 `ng <ref> <message>`，不写 ref。
3. REST 提交路径（`rg-core/src/repo/service.rs`）：`create_or_update_file_with`、`commit_batch` 在写任何对象前扫描；命中返回 `422 SECRET_DETECTED`。`delete_file_with` 不产新 blob，跳过。
4. 审计：REST 侧写 `audit_log`（`repo.secret_scan_rejected` / `_findings` / `_skipped`）；push 侧 `rg-git` 无 DB，产出结构化事件（`tracing` + 可安装 sink），由调用方持久化。

**不做**

- 全历史扫描（只扫本次推送新增/修改的 blob）。
- push 侧直接写 DB（`rg-git` 无 DB，且 `rg-http/src/lib.rs` 在本任务禁改范围内）。
- 规则可配置化 / 外部规则包 / `.gitleaks.toml` 兼容。

## 2. 规则与处置（协调者口径，逐条实现）

| 规则 | 模式 | 处置 | 误报面 |
| --- | --- | --- | --- |
| `private_key` | `-----BEGIN (?:[A-Z0-9]+ )?PRIVATE KEY(?: BLOCK)?-----` | 硬拒 | 极低；文档/测试里出现由允许列表兜住 |
| `cloud_access_key` | `AKIA`/`ASIA`+16、`LTAI…`、`AKID…` | 硬拒 | 低；示例 key（如 `AKIAIOSFODNN7EXAMPLE`）同样拦住，属预期 |
| `platform_token` | `gh[pousr]_`、`glpat-`、`ifs_` + ≥20 | 硬拒 | 低；测试夹具由允许列表兜住 |
| `jwt` | `eyJ…` 三段 base64url | 记事件 + 警告 | 中（前端常把示例 JWT 写进文档）；不拒 |
| `high_entropy` | token 长度 ≥32 且 Shannon 熵 ≥4.5 | 记事件 + 警告 | 较高（base64 资源、minified JS）；不拒 |

**为什么高熵不会误伤 Git SHA**：十六进制字符集的熵上限是 4 bit/char，40 位 SHA 的实测熵约 3.9，低于 4.5；`Cargo.lock` 的校验和同理。另有回归测试 `git_shas_are_not_high_entropy` 钉死这条。

**同一行的去重**：若该行已命中任一硬规则，不再追加 `high_entropy`（同一个 token 不重复报两条）。

## 3. 允许列表

| 机制 | 生效方式 |
| --- | --- |
| 路径 glob | `AllowList::default_rules()`：`**/*.md`、`**/*.mdx`、`**/*.rst`、`**/*.lock`、`**/*.snap`、`docs/**`、`**/docs/**`、`testdata/**`、`**/testdata/**`、`**/fixtures/**` |
| 行内标记 | 行内出现 `ironforge:allow-secret` → 该行不报；**只影响本行**，有专门测试 |

glob 语义：`*` 不跨 `/`，`**` 跨 `/`，前导 `**/` 同时匹配仓库根（所以 `**/*.md` 能匹配 `README.md`）。翻译为正则后预编译，路径判定是 O(glob 数)。

## 4. 限额与 fail-open（协调者裁决）

| 情况 | 行为 | 可见性 |
| --- | --- | --- |
| 单文件 > 1 MiB | 跳过该文件，其余照扫 | `Skip::FileTooLarge` → `secret_scan_skipped` 事件（含 path、字节数） |
| 单次推送 > 500 文件（跨 ref 合计，按真实文件数计） | **整批跳过**，一个文件都不扫 | `Skip::TooManyFiles{files}` → 每个受影响 ref 一条 `secret_scan_skipped`（含 ref、文件数、原因） |
| 扫描本身失败（对象缺失、树不可读） | fail-open，推送继续 | `secret_scan_skipped`，原因为 `scan failed: …`，日志 `warn` |
| 二进制 blob（前 8 KiB 含 NUL） | 不扫（无文本语义） | 无事件（有测试） |

没有静默跳过路径：四个分支各自产出事件或显式跳过。

## 5. 实现要点

### 5.1 只扫增量

`scan_push` 用 gix 的 tree diff（`old_tree.changes()` → `for_each_to_obtain_tree`）得到本次变化的 blob，**不 walk 历史**。新 ref（`old_sha` 为 0）与空树对比，等价于「整棵新树」，因此首次推送/新分支天然受限额约束。`track_rewrites(None)`：重命名按「删除 + 新增」呈现，不需要 blob 内容。

内存：逐 blob 读取并扫描后即释放，不把 ≤500 个文件全部驻留（最坏 500 × 1 MiB 也不会同时存在）。

### 5.2 push 侧拒绝形态

沿用既有 report-status 纯文本：`ng <ref> secret scanning rejected the push: private_key in deploy/id_rsa`。消息只含规则名与路径，**不含匹配文本**；多文件时写成 `<rule> in N files (first: <path>)`。

### 5.3 REST 侧

- `CoreError` 新增 `Coded { status, code, message }` 变体（此前只有 NotFound/Forbidden/Conflict/InvalidInput/Internal，无法表达稳定机器码）。
- `rg-http/src/error.rs` 的 `From<CoreError>` 是穷尽 match，必须补一个映射臂（`AppError::Coded`），否则 workspace 不编译。这是**机械性编译依赖**，只动这一个文件的一处 match 臂。
- `crates/rg-core/src/pull_request/merge_queue.rs` 同样有穷尽 match，补 `Coded` 臂（策略型消息对用户可见，非内部细节）。
- 拒绝发生在写任何对象之前：测试断言 `refs/heads/main` 仍不存在 / 仍指向旧 tip。

## 6. 事件模型

```rust
pub enum AuditKind { Rejected, Warned, Skipped }   // secret_scan_rejected / _findings / _skipped
pub struct AuditEvent { kind, refname, path, rule, files, reason }
pub fn record(event)        // tracing::warn! + 已安装 sink
pub fn set_audit_sink(..)   // rg-core / rg-http 可挂持久化
```

| 路径 | 事件写到哪里 |
| --- | --- |
| REST（`create_or_update_file_with` / `commit_batch`） | `audit_log` 表：`repo.secret_scan_rejected` / `repo.secret_scan_findings` / `repo.secret_scan_skipped`，`details` 只含 path/line/rule 与统计 |
| REST（`update_files_in_commit`，同步、无 DB 句柄） | 拒绝生效，事件降级为 `tracing::warn!`（该函数签名不带 `db`，无法写库；调用方 `rg-http` 不在本任务 owned paths） |
| push（`receive_pack`） | `rg-git` 无 DB：产出结构化 `tracing` 记录 + sink 回调；DB 落地需要 push 后钩子接 sink，见 §9 遗留 |

## 7. 性能实测

`cargo test -p rg-git --release perf_push_scan_is_proportional_to_the_change -- --ignored --nocapture`

```
50-file push scan: 2715 us (repository holds 2000 files)
1-file push scan: 1064 us (repository holds 2001 files)
```

- 仓库 2000 文件、推送改动 50 个文件：**2.7 ms**（含每次调用一次的 `gix::open`，实测约 0.9 ms）。
- 单文件推送：**1.1 ms**，几乎全是 `gix::open` + 一次 tree diff。
- 成本与「改动大小」成正比、与仓库体量无关（2000 与 2001 文件的仓库耗时无差异）。
- 这是 release 数字；debug 构建的 regex 与 gix 会慢 3–10 倍，生产为 release，按此口径报。

## 8. 验收（真实输出）

```
$ cargo test -p rg-git
running 113 tests
test result: ok. 112 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 1.29s

$ cargo test -p rg-core
running 291 tests
test result: ok. 291 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.83s
running 11 tests   (webhook_envelope_tests)
test result: ok. 11 passed; ...
running 1 test     (multi_backend_smoke)
running 0 test     (collab_search_tests，本机未带测试库)

$ cargo clippy --workspace --all-targets -- -D warnings
（无输出，退出码 0）

$ scripts/check.sh --quick
==> cargo fmt --all --check
    (1s)
==> cargo clippy --workspace --all-targets -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 10.30s
==> checks passed in 11s (quick)
```

新增测试 27 项：rg-git 22 项（规则/允许列表/限额/增量/事件/性能）+ 5 项 push 接入（含「消息不含明文」「失败 fail-open」「删除不扫」）；rg-core 5 项（REST 三条路径 + 审计行 + 干净内容放行）。

## 9. 遗留与风险

| 项 | 说明 |
| --- | --- |
| push 侧 DB 事件未落地 | `rg-git` 无 DB，而唯一能在 push 后写库的 `rg-http/src/lib.rs` 在本任务禁改范围。已就位：`AuditEvent` + `set_audit_sink` + 结构化日志。建议后续小任务在 `spawn_post_push_hooks` 里 `set_audit_sink`（约 10 行）。 |
| `update_files_in_commit` 无审计行 | 该函数同步且不带 `db`；拒绝已生效，只是事件降级为日志。 |
| 高熵规则误报 | 设计为「记事件 + 警告」，最坏是多几条审计行，不会阻断推送；若噪音大，后续可调阈值或加 glob。 |
| 私钥头是「头部」匹配 | 只认 PEM/PGP 的 BEGIN 行；裸 base64 私钥体靠高熵规则兜（只警告）。这是分级口径的直接后果，符合协调者裁决。 |
| 扫描失败 fail-open | 有事件与 warn 日志，但确实放过一次。若日后要求 fail-closed，改 `enforce_secret_scan` 的错误分支即可（单点）。 |
| regex 依赖 | 只把已在 `Cargo.lock` 图里的 `regex 1.12.3` 提为 `rg-git` 直接依赖，`Cargo.lock` 只增加 `rg-git` 依赖列表中的 `"regex"` 一行，无新 crate。 |
