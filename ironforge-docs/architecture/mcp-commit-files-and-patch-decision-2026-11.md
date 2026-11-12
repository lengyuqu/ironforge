# MCP `commit_files` 与 unified diff 输入（决策记录，2026-11-13）

> **状态**：`commit_files` 已实现（本分支）；unified diff 输入**只有设计，未实现** ｜ **可信度**：确认（实现与运行证据）/ 推测（patch 设计，未写码）｜ **来源**：CNB issue #48 第 1、2 项、仓库代码
> 范围：issue #48 第 3 项（cherry-pick / revert 端点）本会话**未做**，它要新增路由，与 #14 的路由注册表重构冲突。

## 1. `commit_files`（已做）

`ironforge-mcp` 新增 `commit_files`，一次调用 → REST `POST /repos/{owner}/{name}/commits` 一次请求 → 一个 commit、一次 CI。只改了 `rg-mcp/src/tools/mod.rs`，没碰 rg-http。

| 项 | 说明 |
|---|---|
| 入参 | `owner`、`repo`、`branch`、`expected_sha`、`message`、`files[]`、可选 `dry_run`、`idempotency_key`；除后两者外全部必填 |
| `files[]` 每项 | `create{path,content}`、`update{path,content,sha}`、`delete{path,sha}`、`move{from,to,sha}`，用 `operation` 区分，与 REST 的 `BatchCommitFileRequest` 一一对应 |
| 客户端校验（不发请求） | `files` 非空数组；`operation` 必须是四种之一；必填字段缺失或为空（`content` 可为空串，空文件也是文件）；**多余字段直接拒绝**（如把 `sha` 写成 `blob_sha`——REST 侧会静默丢弃它，`update` 会变成无保护的写）。路径合法性、同路径重复、move 成环仍由服务端裁决，返回 409 `PATH_CONFLICT` |
| `branch` / `expected_sha` | 与 REST 一致，必填、不回退 HEAD；分支不存在或已前进 → 409 `REF_ADVANCED`，**批量提交不会创建分支**。要在新分支上批量提交，先用 `commit_file` 建分支（工具描述已写明） |
| 失败形态 | 与 `commit_file` 相同：`isError: true`，文本 `Error: HTTP <status>: <API 响应体>`，模型可直接读 `reason` / `details.current_sha` |
| 幂等 | `commits` 已在服务端 `idempotency_supported` 里，工具透传 `idempotency_key`；e2e 验证了同 key 重放得到同一个 commit |
| capabilities 注册表 | **没有改动**。注册表（`collab_contract.rs` 的 `TOOL_ROUTES`/`TOOL_NAMES`）只登记 `collab_*` 工具，`commit_file` 等非协作工具本来就不在里面；因此没有碰 `capabilities.rs` |
| 工具数量 | 非协作工具 14 → 15。仓库里没有"工具总数"的快照断言；`tools/list` 的断言是按名字和 `required` 列表检查，已把 `commit_files` 加进 `the_write_tools_are_advertised_with_their_required_arguments` |

## 2. unified diff 输入（决策项）

### 结论

**建议做，但不在本会话做；只留设计。** 优先级低于 #14，需要项目负责人先拍板引入依赖（见"成本"）。理由与取舍如下。

价值是真实但有限的：现在 `update` 必须带整文件 `content`，agent 改一个 3000 行文件里的两行，要输出整个文件；而 agent 本地有 `git diff` 时，补丁是它的自然产物。限制同样真实：模型手写的 hunk 行号和上下文经常错，所以价值取决于**失败时能不能给出足够的反馈**让它一次修好——这是设计的重点。

如果目标**只是**省 token，更便宜、更不易出错的是 `update` 增加 `old_string`/`new_string` 替换式编辑（唯一匹配才应用）；补丁适合"agent 已有现成 diff"的场景。两者不互斥，这里不展开。

### 三个问题的回答

**① `patch` 与 `files` 二选一互斥够不够？—— 够，而且应该互斥。**

- 一个 git 风格的多文件补丁本身就能表达 create（`--- /dev/null`）、delete（`+++ /dev/null`）、rename（`rename from/to`）和修改，四类动作都覆盖，不需要再混用 `files`。
- 混用会产生"补丁改了 `a.txt`，`files` 里也写了 `a.txt`"这类难以解释的冲突；互斥让冲突只剩一种来源。
- 规则：`patch` 与 `files` 恰好给一个，两个都给或都不给 → 400 `INVALID_INPUT`。`branch`、`expected_sha`、`message`、`dry_run`、`idempotency_key` 语义不变。需要"补丁 + 个别整文件覆盖"的调用，拆成两次提交，或把整文件覆盖写成补丁里的整文件 hunk。

**② 要不要 `base_sha` 对齐？—— 不要新字段，直接用已有的 `expected_sha`。**

`commit_batch` 先校验分支 tip 等于 `expected_sha`（否则 409 `REF_ADVANCED`），所以补丁是**对 `expected_sha` 那棵树**应用的，这就是它的基底。再加 `base_sha` 只会产生"两个值不一致"的新错误。同理，补丁路径不需要逐文件的 blob `sha` 钉死：tip 相等已经保证每个文件就是调用者看到的那个版本。（这一点和 `files[]` 的 `update.sha` 不同——那里 `sha` 是冗余的第二道保险，补丁没有。）

**③ 冲突怎么报？**

- 应用方式：**严格应用**。允许 hunk 起始行号有偏移（模型数行常错），**不允许模糊匹配**（fuzz = 0），上下文行逐字相等（行尾换行与 `\ No newline at end of file` 按字节处理）。任何 hunk 失败 → **整批不落**，分支不动，复用现有"全部成功或不动"的语义。
- 错误：HTTP 409，稳定码 `PATCH_DOES_NOT_APPLY`，`retryable: false`，`hint` 指向"重新读文件，针对 `expected_sha` 重新生成补丁"。`details` 里列出**所有**失败的 hunk（不是只报第一个，省一轮往返；总量封顶），每项含 `path`、`hunk_index`、补丁要求的上下文、文件里最接近位置的实际行（各截断到固定行数/字节）、该文件在 `expected_sha` 处的 blob sha。这些内容来自仓库文件和客户端补丁，按不可信文本输出，不带指令语义。
- 其他错误：补丁语法错误 → 400 `PATCH_MALFORMED`（带行号）；二进制补丁、仅改文件模式、符号链接、子模块 → 400 `PATCH_UNSUPPORTED`（本期不支持，明确拒绝而不是忽略）；路径走现有 `validate_repo_file_path`；补丁里同一路径出现两次、rename 成环、写+move 同一路径 → 沿用 409 `PATH_CONFLICT`。
- 上限：补丁字节数与涉及文件数设上限（建议 1 MiB / 200 个文件，实现时再按 `BatchCommitRequest` 现状定），超限 400，不截断。

### 实现草案（未写码）

1. **服务端先做，MCP 后做**。MCP 只是转发器：`BatchCommitRequest` 加 `patch: Option<String>`，`files` 改为可选并做互斥校验；服务层把补丁解析并应用成 `Vec<BatchEdit>`（create/update/delete/move，内容为应用后的全文），然后走**同一个** `commit_batch`。这样分支保护、`require_signed_commits`、令牌分支白名单（`ensure_token_may_write_branch`）、`dry_run`、审计、幂等键、`announce_branch_move`（CI/webhook/事件）全部原样共用，补丁路径不会成为绕过它们的新入口。
2. **审计**：沿用 `repo.batch_commit`，`details` 里记补丁的 SHA-256 与路径列表，不存补丁正文（与整文件内容不入审计一致）。
3. **dry_run**：每个文件返回一条 `would`，含 `hunks_applied`、`max_offset`，让模型在不写入时就能看到偏移是否异常。
4. **幂等键**：`commits` 已支持；请求指纹包含补丁全文。
5. **MCP**：`commit_files` 增加 `patch`（字符串），与 `files` 互斥，客户端先校验（都给/都不给直接报错，不发请求），其余原样转发。工具描述要写明"针对 `expected_sha` 生成、不容忍上下文差异、失败时看 `details` 并重新读文件"。
6. **能力注册表与 OpenAPI**：`BatchCommitRequest` 变动要同步 OpenAPI 注解；`capabilities.rs` 若登记该路由的请求字段，需同步（涉及 #14，需在其合并后做）。

### 成本与风险（需负责人确认）

- **没有现成的补丁应用器**。仓库里 `gix_patch::unified_patch` 只会**生成**补丁，gix 没有 apply API。选项：① 新增依赖（如 `diffy`）——`.ai/guardrails.md` 把新增外部依赖列为 🟡 WARN，且要评估其对偏移、`\ No newline`、CRLF 的行为是否满足上面的"零 fuzz"；② 自研（约几百行，要有对拍测试：与 `git apply --check` 比较接受/拒绝结果，仿 `gix_patch::parity_tests`）；③ 调 `git apply` CLI——**不建议**：CLAUDE.md 已把 git CLI 退出生产热路径（仅 mirror 例外），且 `git apply` 要工作区/索引，在裸仓库上要造临时索引。
- 补丁解析是新的不可信输入面（大小、行数、路径、恶意 hunk 头里的巨大行号），必须有边界测试。
- 不涉及迁移、权限逻辑或认证，因此不触发 🟠 CONFIRM；引入依赖是 🟡 WARN。

### 验收（若以后实现）

补丁应用成功并只产生 1 个 commit/1 次 CI；偏移 ≤ 容忍范围时成功、上下文不符时 409 `PATCH_DOES_NOT_APPLY` 且分支 tip 不动；`patch`+`files` 同给/都不给 400；二进制/模式变更 400 `PATCH_UNSUPPORTED`；受分支限制的令牌写范围外分支 403；与 `git apply` 的对拍测试；MCP 侧桩测试与 `scripts/agent-e2e.py` 增加补丁场景。

## 3. 未做

- issue #48 第 3 项（cherry-pick / revert 端点）：要加路由，且与 #14 路由注册表重构冲突，本会话没碰。
- unified diff 输入：只有上面的设计，没有代码。
