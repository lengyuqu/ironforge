# 提交作者与推送者归因（T25，issue #36）验证记录

> **状态**：已实现，仅本机验证，**未经 CI** ｜ **可信度**：确认（下列命令输出）｜ **日期**：2026-11-13
> 设计来源：[agent-principals-design-and-roadmap-2026-11.md](agent-principals-design-and-roadmap-2026-11.md) §4 T25。不变量 §2.3 未改动（本变更不碰登录入口、权限上限、令牌 scope）。

## 改了什么

| 面 | 变更 |
|----|------|
| 身份函数 | `rg_core::agent::commit_identity(user) -> (name, email)`（`crates/rg-core/src/agent.rs` 末尾独立区块）。人类 → `(username, 存储的 email)`；agent → `(username, "<username>@agents.invalid")`。**只在提交时派生**：不动库、不回填、无迁移；agent 行里仍是随机 `agent-<uuid>@agents.invalid`。不加 `Co-authored-by`。 |
| REST 提交入口（穷举，4 处） | `repo_content.rs` 文件创建/更新、文件删除；`repo_commits.rs` 批量提交（`commit_batch`）；`rg-core/src/review/service.rs::apply_suggestions`（应用评审建议，**issue 与设计文档都没列，穷举时发现**，同样把 `actor.username/email` 写成 author+committer）。 |
| 不需要改的 | 合并/squash/rebase 的合并提交用服务器身份（`IronForge <noreply@ironforge.local>`，`pull_request/service.rs::commit_as_server`、`merge_queue.rs`）；`rebase_merge` 保留原作者；`repos.rs:204` 的初始提交用仓库所有者，而 agent 不能建仓库（`ensure_human`）；wiki 存数据库，不产生 git 提交；导入/镜像用克隆。 |
| 推送者归因 | `spawn_post_push_hooks` / `PostPushParams` 新增 `pushed_by: Option<i64>`；`push` 事件载荷新增 `pushed_by`（用户 id，未知为 `null`）。git push 传 `extract_git_actor` 得到的 `actor_id`；REST 文件/批量提交经 `announce_branch_move` 传 `user.id`。该载荷同时进 `repo_events` 与 webhook。git push 的作者字段**不改**（客户端控制，影响签名与校验）。 |

## 验证（本机，未经 CI）

| 命令 | 结果 |
|------|------|
| `cargo clippy --workspace --all-targets -- -D warnings` | 干净（`announce_branch_move` 加了 `#[allow(clippy::too_many_arguments)]`，仓库内已有同类用法） |
| `cargo test -p rg-http --test commit_identity_tests`（新增 5 项，SQLite） | 5 passed（改完 clippy 后重跑） |
| `cargo test -p rg-http` 的 `agent_principal_tests`（19）、`batch_commit_tests`（10）、`file_commit_semantics_tests`（5）、`repo_events_tests`（5）、`pr_permission_tests`（6）、`pr_review_error_semantics_tests`（9）、`collab_webhook_tests`（5） | 全部通过 |
| `cargo test -p rg-core --lib agent::` | 5 passed（含新增 2 项 `commit_identity`） |

新测试覆盖：agent 单文件提交 / 删除 / 批量提交 → `git log --format='%an <%ae> | %cn <%ce>'` 为 `alice-agent-bot <alice-agent-bot@agents.invalid>`（author 与 committer 均是）；人类单文件与批量提交不变；agent 行的存储邮箱仍是 uuid 形式；REST 提交与真实 `git push`（带客户端自选作者 `Client Chosen`）的 `push` 事件都带 `pushed_by = agent id`，且推送提交的作者未被服务端改写。`commit_identity` 另有 2 个单元测试（agent / 人类）。

## 没做 / 没跑

- **审计行 `on_behalf_of = 所有者`**：依赖 T23，未实现；**待 T23 合并后补测**。REST 三处写已有 `repo.file_commit` / `repo.file_delete` / `repo.batch_commit` 审计行，`user_id` 为 agent。
- **git push 没有审计行**（现状如此）：本变更只补了 `repo_events` 的 `pushed_by`。若要在审计里查"谁推的"，需另做。
- **SSH push**：`rg-ssh` 路径没有走 `spawn_post_push_hooks`，不产生 `push` 事件，因此也没有 `pushed_by`（本变更前就没有事件）。
- **应用评审建议**路径的改动是同一函数的机械替换，**没有**专门的集成测试（需 PR+评审夹具）。
- PostgreSQL / MySQL：**未跑**（无新 SQL，无新迁移；测试只在 SQLite）。
- 前端、真实 MCP 客户端：未涉及。
