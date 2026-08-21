# IronForge 架构差异与后续待办（2026-07）

**生成日期**: 2026-07-05  
**最近复核**: 2026-08-20
**来源**: `project-architecture-analysis-notes-2026-07.md` 第 0-10 轮 + 修复波次回填 + 严谨性修缮计划 Q 轨道回填  
**排序原则**: 优先列出安全、权限、可用性、部署失败和文档误导风险。

---

## 已修复（2026-07-05）

| 问题 | 修复范围 | 验证 |
|------|----------|------|
| `GET /users/me` 等接口仍只读 Bearer，不读 HttpOnly cookie | `users/me`、PAT token 管理、Admin helper、SSO refresh/unlink 改为复用 `extract_user_id`，支持 HttpOnly cookie 或 Bearer | `cargo check -p rg-http --tests`；`test_me_accepts_httponly_cookie_without_bearer`；`admin_sso_accepts_httponly_cookie_without_bearer` |
| MFA disable 未检查 `verify_password` 返回的 `bool` | `crates/rg-http/src/api/mfa.rs::disable_mfa` 显式拒绝 `verify_password == false` | `test_disable_mfa_rejects_wrong_password` |
| SSO callback 缺少 state 时继续执行 | `crates/rg-http/src/api/sso.rs::callback` 缺失 query state 时返回 forbidden | `cargo check -p rg-http --tests` |
| Pipeline API 未看到认证或仓库权限检查 | list/get/job 接入 `can_read_repo`，trigger/retry/cancel 接入 `can_write_repo`，并绑定 pipeline/job 到 URL 仓库，修复跨仓库 IDOR 风险 | `cargo check -p rg-http --tests`；`cargo test -p rg-http --test ci_permission_tests -- --nocapture` |
| Package REST/protocol endpoints 权限边界不完整 | REST 读、protocol metadata/download 接入 `can_read_repo`；publish/delete/yank 接入 `can_write_repo`；publish 显式调用 adapter validate；补 package 表名单复数纠偏迁移 | `cargo check -p rg-http --tests`；`cargo test -p rg-http --test package_permission_tests -- --nocapture` |
| OCI anonymous pull 未见 repo visibility 校验 | `/v2` pull/push handlers 改为基于 IronForge repo `can_read_repo` / `can_write_repo`；token endpoint 只签发实际允许的 scope；测试 router 补挂 `/v2` | `cargo check -p rg-http --tests`；`cargo test -p rg-http --test oci_permission_tests -- --nocapture` |
| SSH Git 未看到 repo-level `can_read/can_write` | SSH exec path 在启动 `git-upload-pack` / `git-receive-pack` 前解析 `owner/repo`，并分别接入 `can_read_repo` / `can_write_repo`；无 DB 模式保持 Phase 1 兼容开放 | `cargo check -p rg-ssh -j1`；`git diff --check`；`cargo test -p rg-ssh -j1 -- --nocapture` 两次卡在依赖测试编译阶段后中断 |
| Runner CLI 自动注册不带 Authorization，但后端注册要求 Bearer JWT | `/runners/register` 收紧为 admin-only 且支持 HttpOnly cookie；`ironforge-runner register/run` 与 `ironforge runner` 自动注册新增 `--auth-token` / `IRONFORGE_AUTH_TOKEN`，已有 runner id/token 路径不受影响 | `cargo test -p rg-http --test runner_auth_tests -- --nocapture`；`cargo check -p rg-runner`；`cargo check -p rg-cli` |
| `deploy/docker-compose.yml` 使用被拒绝的默认 secret；Prometheus target 端口不一致 | 新增 `deploy/.env.example`，主 compose 改为读取 `deploy/.env`，移除被拒绝的默认 secret；主服务和观测栈共享 `ironforge-net`，Prometheus 改抓 `ironforge:8080`；部署 README 补生成 secret 步骤 | `docker compose -f deploy/docker-compose.yml config`；`docker compose -f deploy/docker-compose.observability.yml config` |
| 缺少正式 CI workflow | 新增 `.github/workflows/regression.yml`，覆盖 Rust test compile、`rg-http` 集成测试、workspace tests、fresh SQLite migration smoke、前端 `npm run check/build` 和 compose config 静态校验 | `ruby -e "require 'yaml'; YAML.load_file('.github/workflows/regression.yml')"`；`git diff --check` |
| LDAP TLS 证书校验禁用 | `LdapConfig` 新增显式 `insecure_skip_tls_verify`，LDAP 连接默认不再调用 `set_no_tls_verify(true)`；仅 LDAPS 且显式 insecure 时跳过证书校验 | `cargo check -p rg-core`；`cargo test -p rg-core ldap -- --nocapture` 卡在测试 profile 依赖编译阶段后中断 |
| SSO callback 返回 JSON，未见设置 auth cookie 或回跳前端 | OAuth callback URL 改为 `/api/v1/auth/sso/{slug}/callback` 且优先使用 `external_url`；state/PKCE cookies 使用 `append` 且 `Path=/`；callback 成功后设置 `ironforge_token` HttpOnly cookie 并 redirect `/dashboard`；MFA 用户 redirect 到登录页并进入 MFA 表单 | `cargo check -p rg-http --tests`；`cargo test -p rg-http sso_ -- --nocapture`；`npm run build`；`npm run check` 卡在 `svelte-kit sync` 后中断 |
| Rate limit 信任 `X-Forwarded-For` | 限流默认只按 socket IP 计数；新增 `rate_limit.trusted_proxies` / `--rate-limit-trusted-proxies`，只有可信代理来源才读取 `X-Forwarded-For` / `X-Real-IP` | `cargo test -p rg-http rate_limiter -- --nocapture`；`cargo check -p rg-http --tests`；`cargo check -p rg-cli` |
| 外部 runner Docker 不可用时回退 local | `ironforge-runner` 指定 `image` 的 job 在 Docker 不可用时直接失败并上报日志，不再调用 local shell fallback；内置 runner 已保持同样 fail-closed 语义 | `cargo test -p rg-runner docker_unavailable -- --nocapture`；`cargo check -p rg-runner` |
| CI_JOB_TOKEN 生成后 HTTP handler 未接入验证 | `repo_content` / `archive` 读路径接入同仓库 `repo:read` CI token；Package REST/protocol 读路径接入同仓库 `packages:read` CI token；写接口仍只接受用户身份 | `cargo test -p rg-http --test ci_job_token_tests -- --nocapture`；`cargo check -p rg-http --tests` |
| MCP tools/resources 可能无 Tokio runtime | `ironforge-mcp` 启动时创建并进入单 worker Tokio runtime，现有同步 JSON-RPC dispatch 中的 async HTTP 调用不再因缺少 runtime panic | `cargo check -p rg-mcp`；stdio `tools/call list_repos` smoke 返回 JSON-RPC 错误文本而非 panic |
| `/metrics` 未初始化时 panic | `metrics_handler` 在 registry 未初始化时返回 503 文本响应，不再 `expect` panic | `cargo test -p rg-http metrics_handler_returns_503 -- --nocapture`；`cargo check -p rg-http --tests` |
| Artifact API 只管理 metadata | runner artifact raw body 上传会保存到 `{repo_root}/_artifacts` 并落 DB；新增 `/artifacts/{id}/download`；list/get/download/delete 均校验 artifact 所属 pipeline 的 repo read 权限 | `cargo test -p rg-http --test artifact_file_tests -- --nocapture`；`cargo test -p rg-http --test runner_auth_tests -- --nocapture`；`cargo check -p rg-http --tests` |
| 缺少 DB 备份/恢复方案 | 新增 `ironforge backup-db` 和 `ironforge restore-db`；备份使用 SQLite `VACUUM INTO`；恢复默认拒绝覆盖，需服务离线并显式 `--force`；部署文档补容器命令 | `cargo check -p rg-cli`；`cargo build -p rg-cli` 本地链接阶段超时后中断，未完成 runtime smoke |
| Docker 镜像只包含主服务二进制 | Docker runtime 镜像现在同时构建并复制 `ironforge`、`ironforge-runner`、`ironforge-mcp`；compose 可选 runner 示例改用独立 runner 二进制 | `rg -n "ironforge-runner|ironforge-mcp|cargo build --release --bin" Dockerfile deploy/docker-compose.yml deploy/README.md`；`git diff --check` |
| Git receive-pack 分支保护不是 pre-receive 强拦截 | `rg-git` receive-pack 新增 rejected refs 前置拒绝入口；HTTP Git 和 SSH Git 在读 pack/写 ref 前加载 protected branch 规则并把禁止直推的 `refs/heads/*` 标记为 `ng` | 静态检查通过；`cargo check -p rg-git` 本地 `rustc` 睡眠挂起，已中断，需后续环境复跑 |
| MCP `--sse` 参数未实现但文档容易误写为可用能力 | `rg-mcp` 注释改为 stdio-only；`--sse` 现在返回非零错误；AGENT/AGENTS 与 AI Agent 集成文档统一写 stdio 可用、SSE 未实现 | `cargo check -p rg-mcp`；`cargo run -p rg-mcp -- --sse` 返回错误 |
| job log WebSocket 后端有、前端未接入 | 前端 API client 新增 `connectJobLogWebSocket(jobId)`；pipelines 日志弹窗打开时订阅 `/ws/job/{job_id}`，实时追加 runner log chunk，关闭/切换时清理连接 | `npm run build` |
| CSP `connect-src 'self'` 与跨域 API base 不兼容 | CSP `connect-src` 现在会从 `IRONFORGE_CORS_ORIGINS` 和 `IRONFORGE_CSP_CONNECT_SRC` 生成，并自动加入 HTTP(S) origin 对应的 WS(S) origin | `cargo test -p rg-http security::tests::connect_src -- --nocapture`；`cargo check -p rg-http --tests` |
| README/CONTRIBUTING 有旧 E2E 段落 | README 和 CONTRIBUTING 不再引用待创建脚本或长篇临时 shell 片段；统一指向 `full-interface-regression`、OpenAPI smoke、console smoke 和 API client contract check | `rg -n 'scripts/e2e_test\\.sh|端到端测试脚本|待创建' README.md CONTRIBUTING.md` 无结果；`git diff --check` |
| Package 17 种格式与专用协议支持不一致 | 前端 package 格式元数据区分 native adapter 和 Generic fallback；列表/上传页对 `go/conan/conda/alpine/debian/rpm/swift` 标注 Generic fallback，避免误写为完整专用协议 | `npm run build` |
| 旧拆分 API 文件重复实现风险 | `auth.ts/repos.ts/...` 旧领域文件改为从 `client.svelte.ts` 显式 re-export，避免重复实现继续漂移，同时保留老导入路径兼容性 | `npm run build` |
| Markdown sanitizer allowlist | `sanitizeHtml` 改为浏览器 DOMParser + 标签/属性/URL allowlist，非浏览器 fallback 也按 allowlist 收紧，并新增 sanitizer smoke | `npm run smoke:markdown-sanitizer`；`npm run build` |
| API client WebSocket helper 混在聚合文件 | notification/job log WebSocket helper 抽到 `websockets.ts`，`client.svelte.ts` 继续 re-export 保持调用方兼容 | `npm run build` |
| Job log WebSocket 全局广播、连接任务残留 | 后端改为每个 `job_id` 独立 broadcast channel，通知端不再接收 Job 日志；连接退出同步释放 receiver 并回收空 channel，订阅升级前校验 Job 所属仓库读取权限 | `cargo test -p rg-http --lib ws::tests`；`cargo test -p rg-http --test job_websocket_tests` |
| LFS action URL 无签名/过期分级，公有仓库上传未强制写权限 | Batch action URL 改为 HMAC-SHA256 绑定用途、仓库、OID 与过期时间；下载 1h、上传 6h，过期返回 410；上传 Batch/直传对公有仓库同样要求写权限，合法签名可承接 Batch 授权 | `cargo test -p rg-http --test lfs_signed_url_tests` |
| API client Package Registry 混在聚合文件 | Package Registry API 抽到 `packages.ts`，`client.svelte.ts` 继续 re-export `packages` 保持页面导入兼容 | `npm run build` |
| API client Runner 管理混在聚合文件 | Runner 管理 API、响应类型和 labels 归一化抽到 `runners.ts`，`client.svelte.ts` 继续 re-export 保持页面导入兼容 | `npm run build` |
| API client Boards/Time/Search 混在聚合文件 | Boards、Time Tracking、Search API 分别抽到 `boards.ts`、`timeTracking.ts`、`search.ts`，主入口继续 re-export 保持页面导入兼容 | `npm run build` |
| API client Auth/Releases/Issues/PR/CI/Wiki 混在聚合文件 | Auth、Releases、Issues、Pull Requests/Reviews、Pipelines、Wiki API 分别抽到独立领域文件，主入口继续 re-export 保持页面导入兼容 | `npm run build` |
| API client 剩余领域仍在聚合文件 | Repos、Collaborators、Labels、Notifications、Orgs、Branch Protection、Mirrors、Webhooks、Imports、Milestones、Tokens、MFA、Admin 全部抽到独立领域文件，`client.svelte.ts` 只保留兼容 re-export | `npm run build` |
| PR rebase 在 bare 仓库直接执行导致必然失败 | rebase 改为唯一临时工作区执行，成功后以普通 fast-forward push 更新目标分支；冲突或并发推进不会覆盖 base，并统一清理临时目录；同仓库与 fork 共用实现 | `cargo test -p rg-http --test pr_merge_strategy_tests -- --nocapture`（merge/squash/rebase 拓扑与冲突恢复共 2 项通过） |
| SSH push 缺少真实协议回归 | 新增临时 russh 服务 + 数据库注册 Ed25519 密钥 + 系统 OpenSSH/Git 全链路测试，覆盖 push、clone、bare ref/内容和 `last_used_at` | `cargo test -p rg-ssh --test ssh_push_tests -- --nocapture` |
| 通用 OIDC discovery 未用于 token/userinfo，callback 缺 PKCE verifier 时继续请求 | authorize/token/refresh/userinfo 统一解析 discovery document；HTTP 错误 fail closed；callback 缺 verifier 直接 403；新增本地模拟 OIDC 的 S256、Cookie、token、userinfo、账户创建全链路回归 | `cargo test -p rg-http --test oauth_pkce_tests -- --nocapture` |
| Package 9 类原生 REST/协议缺少统一 E2E | 为 Cargo/npm/Maven/PyPI/NuGet/RubyGems/Helm/Composer/Generic 生成最小合法制品，覆盖发布、列表、版本、下载和 8 类专用索引；同时修复 npm publish/list 路由冲突、RubyGems `.json` 查询及 Composer dist URL 缺版本/编码 | `cargo test -p rg-http --test package_format_e2e_tests -- --nocapture` |
| 审计归档器未接入且固定 NDJSON 文件可能覆盖 | 服务启动默认接入可配置归档任务；按 cutoff oldest-first 限量读取，唯一 `.ndjson.zst` 文件、临时文件原子重命名并在落盘后删除 DB；支持关闭、保留天数、间隔和批大小配置 | `cargo test -p rg-core --lib audit::archiver -- --nocapture`；`cargo check -p rg-cli -p rg-core --tests` |

---

## 已修复（2026-08-20，严谨性修缮批次 1：Q1/Q2）

来源：`ironforge-严谨性修缮计划.md` Q 轨道。

| 问题 | 修复范围 | 验证 |
|------|----------|------|
| 仓库访问控制存在 4 套并行实现（`repo_access`、`ci.rs`、`ai.rs`、`packages.rs` 各自持有私有授权函数），语义不一致 | `repo_access` 新增 `require_read_with_ci_scope`（CI job token 按 scope 授权匿名读）；`ci.rs` / `ai.rs` / `packages.rs` 私有 `resolve_repo*` / `require_repo_*` 全部迁移至统一 `repo_access::require_read` / `require_write`，旧函数删除；packages 读路径使用 `packages:read` CI scope | `cargo test -p rg-http --no-fail-fast`：`repo_access_matrix_tests` 4/4（含迁移端点语义、CI scope 隔离、读/写矩阵）、`ci_job_token_tests` 2/2、`ci_permission_tests` 4/4、`package_permission_tests` 2/2 |
| 关键多表写入无事务，部分失败会留下不一致状态 | PR merge（`update_pr_merged`：PR 更新 + merged 事件）、Merge Queue（`enqueue` / `finish_entry`）、org 创建（org + owner 成员）、board 创建（board + 3 默认列）改为单事务；涉及 ops 函数（`pull_request_ops::update`、`merge_queue_ops`、`org_ops::add_org_member`、`board_ops`）泛型化为 `ConnectionTrait` | 同上全量运行：`merge_queue_ci_tests` 1/1、`board_tests` 7/7、`org_tests` 3/3、`collaborator_tests` 通过 |
| mirror 同步落库是否原子的疑虑 | 核验 `mirror/service.rs` 现有实现已是原子写入，无需改动 | 代码走读确认（Q2.2 结论：无需改动） |
| 权限矩阵缺统一集成测试 | 新增 `crates/rg-http/tests/repo_access_matrix_tests.rs`，覆盖 public/private 仓库 × 匿名/stranger/owner 的读访问矩阵 | 同第一行 |

批次 1 验证汇总：`cargo test -p rg-http --no-fail-fast` 仅 5 项失败（issue_template ×2、pr_merge rebase worktree、pr_permission codeowner、runner_workspace auto_init），全部为本机 Windows `//?/` verbatim 临时路径导致 git clone/worktree 失败的既有环境问题（错误信息均为 `does not appear to be a git repository`，非授权类 401/403），与 Q1/Q2 改动无关，Linux CI 可复跑；`cargo clippy -p rg-http -p rg-core -p rg-db --all-targets` 通过（无警告）；前端 `npm run check` + `npm run build` 通过。

---

## 已修复（2026-08-20，严谨性修缮批次 2：ISSUE-104 + Q6.1/Q6.2）

来源：`ironforge-严谨性修缮计划.md` F 轨道 M1（ISSUE-104 Reaction）+ Q 轨道 Q6.1/Q6.2。

| 问题 | 修复范围 | 验证 |
|------|----------|------|
| Issue/评论无表情回应（Reaction）功能（ISSUE-104） | 数据层：`reactions` 表迁移（`m20260820_000001`，唯一约束 target+user+content，`comment_id=0` 表示 issue body）+ entity + `reaction_ops`（含评论删除级联清理、唯一约束转 409）；业务层：`rg-core/src/issue/reactions.rs`（Gitea 兼容 8 种 emoji、聚合计数含 `reacted_by_me`、通知作者但排除自己）；API：issue/评论的 list/add/delete 端点（409 重复、400 非法 content、私有仓库 require_read）；前端：issue 详情页 issue body + 评论 Reaction 栏（svelte snippet 复用）+ i18n（en/zh-CN） | `cargo test -p rg-http --test issue_reaction_tests` 4/4：issue 往返+唯一性+聚合、评论级联清理、私有仓库权限、通知作者不通知 reactor |
| job log WebSocket 断线无重连，日志中断后不可恢复（Q6.1） | 后端 `ws.rs`：`?since=<lines>` 查询参数 + `job_log_catchup`（从 DB 缓冲日志按行偏移重放）；前端 `websockets.ts`：重构为 `JobLogSession`（指数退避重连、已收行数跟踪、connected/reconnecting/closed 状态回调、显式 disconnect）；pipelines 页展示 Live/Reconnecting/Closed 状态并传 `since` 续传 | `cargo test -p rg-http --lib ws::` 4/4（含 `job_log_catchup_replays_from_offset`）；`npm run check`（0 error）+ `npm run build` 通过 |
| i18n 缺 key 无 CI 拦截（Q6.2） | 新增 `scripts/i18n-key-check.mjs`：扫描 `t('...')` 静态 key（985 个）+ 动态前缀白名单（9 个），比对 en/zh-CN 目录（各 874 key）缺 key 即失败；挂入 `regression.yml` frontend job | 脚本本机运行通过；本轮顺带补齐历史缺失 key（errors.save_failed、search.load_failed 等） |

批次 2 验证汇总：`cargo test -p rg-http --no-fail-fast` 仅 issue_template ×2 失败（本机 Windows `//?/` verbatim 临时路径既有环境问题，同批次 1 归因，与改动无关）；`cargo clippy -p rg-http --all-targets -- -D warnings` 通过；`node ../scripts/i18n-key-check.mjs` 通过；前端 `npm run check`（0 error，6 个既有 unused-CSS warning）+ `npm run build` 通过。

批次 2 余量（顺延下一批次）：ISSUE-105 多 Assignee、Q6.3 高频表单校验。

---

## 已修复（2026-08-21，批次 2 代码审查：Q2.6 迁移回归修复）

来源：批次 2 推送后的全量代码审查（review 发现 Q2.6 anyhow→CoreError 迁移引入的 HTTP 语义回归）。

| 问题 | 根因 | 修复 | 验证 |
|------|------|------|------|
| 重复 reaction 返回 500（应 409）；未知 issue/评论返回 500（应 404） | Q2.6（30aa580）把 `reaction_error_response` 改为 CoreError 变体映射，但 service 层仍用 `.context()`（Option 上产生 `Internal` 变体）与 anyhow 传播（`From<anyhow::Error>` → `Internal`），变体映射下全部落 500。批次 2 原实现（anyhow + 字符串匹配）行为正确，回归由迁移引入且迁移验证只跑了权限矩阵测试 | `reactions.rs`：comment 查找改 `ok_or_else(NotFound)`；`insert_reaction` 将 "reaction already exists" anyhow 错误映射为 `Conflict` 变体。`issue/service.rs::get_issue`：`.context` 改 `ok_or_else(NotFound)`（Display 不变，其他调用方无影响） | `issue_reaction_tests` 4/4（含 409/404 断言）；实证检查重复 409 / 未知 issue 404 / 未知评论 404；全量 `cargo test -p rg-http --no-fail-fast` 仅既有 Windows 环境失败（issue_template ×2、pr_merge、pr_permission、runner_workspace，同批次 1 归因）；clippy 通过 |

附带审查结论（无改动）：Q6.1 前后端行数对齐在"整段空 chunk"时会漂移一行（`log_write_queue` 对空 chunk 仍追加 `\n` 分隔），重连时最多重复一个空行、无数据丢失，可接受；comment reaction 通知 `repo_id=None`（前端通知页不消费该字段，无影响）。

---

## 已修复（2026-08-21，严谨性修缮批次 3：Q2.4–Q2.6 + Q3.1/Q3.3 + CI-201 + Q6.3）

来源：`ironforge-严谨性修缮计划.md` Q 轨道 + F 轨道 CI-201（提交 `7ee7ffb` + Q2.6 `30aa580` + 回归修复 `877fa68`）。

| 问题 | 修复范围 | 验证 |
|------|----------|------|
| 写端点无输入长度/格式校验，直接依赖后端 500/DB 截断（Q2.5） | 新增 `rg-http/src/api/validation.rs` 集中校验 helper；issue 创建/更新、PR 创建、wiki 创建/更新、webhook 创建（URL + events）、label 创建统一加上限 | `cargo test -p rg-http --no-fail-fast`（批次 3 全量，见下方汇总） |
| webhook/email 投递失败仅静默 warn，不可观测（Q2.4） | webhook：reqwest 30s 超时 + 结构化 tracing（webhook_id/event/status 成败均记录），redeliver 接入 `metrics::recorder::webhook_delivery()`；email：SMTP 30s 超时 + 发送成败 tracing | 同上（tracing/metrics 走代码走读 + 编译验证；自动重试按决策 4 等 QUEUE-001） |
| 服务重启后遗留 `running` job 永久卡死或重复执行（Q3.1） | 新增 `recover_orphaned_jobs()`：启动时先于 watchdog 扫描全部 assigned/running job 标记 failed（原因 "runner restart"，fail-closed 防重复执行）；watchdog 卡死阈值 600s→300s | 同上 |
| docker/spawn 报错路径可泄漏 secret 值到 job log（Q3.3） | `run_job()` Err 路径返回前用 `secret_values` 脱敏错误消息 | 同上 |
| 失败 job 无法单独重跑（CI-201） | `pipeline_ops::rerun_failed_job()`：failed/errored/canceled → pending，级联重置 stage/pipeline 状态；新增 `POST /repos/{owner}/{name}/pipelines/{pipeline_id}/jobs/{job_id}/rerun`（写权限 + pipeline 归属校验） | 同上 |
| 高频表单（建仓/Issue/PR）无前端即时校验（Q6.3） | Issue/PR 创建：标题必填 + 255 上限、正文 65536 上限、校验失败禁用提交；建仓：名称必填 + 100 上限 + 正则（字母数字/点/横线/下划线、首尾字母数字）、描述 255 上限；校验消息 i18n（en/zh-CN） | `npm run check` + `npm run build` |
| rg-core 业务路径残留 `anyhow`（Q2.6） | auth（11 文件）/issue/repo/pull_request/email/notification/lib 校验器全面迁移 `CoreError`/`CoreResult`；HTTP 层新增 `From<CoreError>` 语义映射（NotFound→404/Forbidden→403/Conflict→409/InvalidInput→400）；rg-ssh 新增 `From<CoreError>`；迁移引入的状态码回归已于 `877fa68` 修复（见上节） | 迁移验证（权限矩阵 4/4）+ 回归修复验证（`issue_reaction_tests` 4/4 含 409/404 断言） |

批次 3 验证汇总：全量 `cargo test -p rg-http --no-fail-fast` 仅既有 Windows 环境失败（issue_template ×2、pr_merge、pr_permission、runner_workspace，同批次 1 归因）；`cargo clippy` 通过；前端 check/build + `i18n-key-check` 通过。

批次 3 余量（顺延下一批次）：ISSUE-105 多 Assignee（批次 2 顺延后仍未完成）、Q3.2 runner 心跳与断连。

---

## 已修复（2026-08-22，严谨性修缮批次 4：ISSUE-105 + Q3.2）

来源：`ironforge-严谨性修缮计划.md` 批次 3 顺延项。

| 问题 | 修复范围 | 验证 |
|------|----------|------|
| Issue 仅支持单一 Assignee（ISSUE-105） | 后端：新表 `issue_assignees`（迁移 `m20260821_000001`，含 issues/users 级联 FK + 唯一索引 + 存量 assignee_id 回填）、entity + `issue_assignee_ops`；`rg-core/src/issue/assignees.rs` 多 Assignee 业务层（set/list/按 assignee 筛选，首列为 primary 镜像回写 legacy `assignee_id`）；API：`GET/PUT /repos/{owner}/{name}/issues/{number}/assignees`（PUT 需写权限）、创建 Issue 支持 `assignees`、列表支持 `?assignee=` 筛选；PATCH legacy `assignee_id` 同步 junction 表。前端：详情页 Assignees 面板（展示/编辑，primary 徽标高亮）、创建表单 assignees 字段、模板预填、`issues.ts` API client 扩展、i18n（en/zh-CN） | `cargo test -p rg-http --test issue_assignee_tests`（4/4：round-trip 去重+primary 镜像、写权限 403、创建+筛选、PATCH 同步）；`npm run check` + `npm run build` |
| Runner 心跳断连后 job 被重新排队导致重复执行风险（Q3.2） | 按决策改为标记失败：`pipeline_ops::fail_runner_jobs()`（runner 的 assigned/running job → error/exit_code=-1/finished_at，并级联 stage→pipeline 状态，复用 `mark_job_timeout`）；watchdog（`rg-http/src/lib.rs`）断连 runner 处理从 reset-to-pending 改为 fail-closed，并删除 300s stuck-job 重置循环（长任务误杀 + 重复执行隐患；job 超时由 rg-runner 侧 `tokio::time::timeout` 执行，重启孤儿由 Q3.1 `recover_orphaned_jobs` 兜底）；deregister 同样改为标记失败；删除 `reset_stuck_job`/`reset_runner_jobs` 死代码。心跳参数维持 30s 上报/90s 判定（3 次丢失）| `cargo test -p rg-http --test runner_disconnect_tests`（3/3：断连检测+job 失败+stage/pipeline 级联、心跳新鲜不误判、deregister 标记失败）；`cargo test -p rg-db -p rg-core` 全量通过 |

批次 4 验证汇总：`cargo test -p rg-db -p rg-core` 全量通过；`cargo test -p rg-http --test issue_assignee_tests`（4/4）、`--test runner_disconnect_tests`（3/4→3/3）、`--test issue_reaction_tests` 回归通过；`cargo clippy -p rg-db -p rg-core -p rg-http -p rg-runner --all-targets` 无警告；前端 check/build 通过。`runner_workspace_tests` 在 Windows 下仍因 auto_init 的 `git push` 无法识别 `\\?\` 前缀临时路径而失败（批次 1 起已知环境问题，非本批次引入，新测试已绕开该路径）。

---

## P0：应优先修复

当前 P0 已清零。

---

## P1：高优先级增强

当前 P1 安全/部署强优先级项已清零。剩余体验、文档口径和中长期生产化任务见 P2。

---

## P2：中期整理

| 问题 | 影响 | 建议 |
|------|------|------|
| API client 新增领域回归到聚合入口 | 前端 API 维护成本可能再次抬升 | 当前 `client.svelte.ts` 已降至 38 行纯 re-export；后续新增领域应优先放独立模块，再由主入口 re-export |
| 多数据库生产化余量 | SQLite/PostgreSQL/MySQL 已完成首轮实库迁移、CRUD、FTS、并发认证与服务启动验证，但尚未覆盖版本矩阵、备份恢复演练、HA 和长期压测 | 以支持版本矩阵和恢复目标为边界补生产验证；不再把“实现 PostgreSQL/MySQL 基础支持”列为待办 |
| MCP SSE transport | 当前 MCP 仅 stdio 可用，网页端 Agent 场景仍缺 transport | 等 HTTP/SSE server 设计明确后再实现；现阶段继续保持 `--sse` fail-fast |
| Package 专用协议补全 | `go/conan/conda/alpine/debian/rpm/swift` 仍按 Generic fallback 展示 | 根据真实用户需求逐个补 adapter，不把 type 枚举写成完整协议支持 |
| gix 后续迁移 | pack/rebase/archive/unified diff 等仍依赖 git CLI 或等待上游能力 | 每次 gix 升级复查阻塞表，优先迁移可对拍验证的只读路径 |

---

## 文档口径修正

以下表述在最终文档中应避免或修正：

| 旧口径 | 当前应写成 |
|--------|------------|
| Phase 1-21 全部完成即可代表功能闭环 | 以当前代码路径为准，部分安全/权限/运维闭环仍有缺口 |
| PostgreSQL/MySQL 支持 | 默认 SQLite，同时支持 PostgreSQL/MySQL URL；2026-07-13 已完成真实服务首轮 smoke，版本矩阵、备份恢复、HA 与压测仍是生产化余量 |
| MCP 支持 stdio/SSE | 当前 stdio 可用，`--sse` 未实现 |
| 前端登录态已完全迁移到 HttpOnly cookie | 关键浏览器用户 API 已支持 cookie；其他领域接口仍需按功能逐步复查 |
| Package Registry 支持 17 种完整协议 | 17 种 type 枚举存在；其中 Cargo/npm/Maven/PyPI/Docker/NuGet/RubyGems/Helm/Composer/Generic 有 native adapter，其余 type 走 Generic fallback |
| Artifact 管理完整 | ✅ 已补 runner raw 上传、服务端文件保存、下载端点和 repo read 权限 |
| Docker compose 可直接启动 | 需要先从 `deploy/.env.example` 生成 `deploy/.env` 并设置强 `IRONFORGE_JWT_SECRET`；镜像内已包含 `ironforge` / `ironforge-runner` / `ironforge-mcp` |
| API docs 不支持 PAT | 本轮核验有测试覆盖 JWT 和 PAT 均可访问 docs |

---

## 建议执行顺序

1. 先冻结本轮 P0/P1 修复成果：保留现有回归测试入口，避免认证、权限、部署和 API client 结构再次漂移。
2. 回填最终架构文档：把“已修复事实”和“长期方向”分开写，避免 Phase 完成状态被误读为生产化全闭环。
3. 再推进长期能力：数据库版本矩阵与恢复演练、MCP SSE、Package 专用协议补全、gix 后续迁移。
