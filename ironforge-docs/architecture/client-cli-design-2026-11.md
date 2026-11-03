# 客户端 CLI（`ifg`）设计草案

> **状态**：草案（Proposed） ｜ **日期**：2026-11-03 ｜ **可信度**：现状部分 `确认`（附代码证据），设计部分 `推测`（待评审）
> **来源**：仓库代码（`crates/rg-cli`、`crates/rg-mcp`、`crates/rg-http`）+ 本文提出的设计

---

## 1. 背景与目标

IronForge 已有一个 CLI——`ironforge` 二进制（`crates/rg-cli/src/main.rs`），但它是**服务端运维工具**：`serve`、`migrate`、`rebuild-fts`、`backup-db`/`restore-db`、`backup`/`restore`、`create-repo`、`runner`、`import`、`index-repo`、`package`，直接连数据库与磁盘。

缺的是**面向用户的 API 客户端 CLI**（对标 Gitea `tea`、GitHub `gh`）：在任意机器上通过 REST API 操作仓库、Issue、PR、CI、Release。

**目标**

1. 开发者在终端完成日常协作：看/建 Issue 与 PR、合并、看 CI、发 Release。
2. 为脚本与 AI Agent 提供稳定、可解析的接口（`--json`），补足 `rg-mcp`（stdio、偏只读）覆盖不到的写操作场景。
3. 以 CLI 为 REST API 的第二个消费者，暴露 API 一致性问题（分页、错误体、鉴权）。

**非目标**

- 不替代 `ironforge` 运维命令，也不直连数据库。
- 不追求覆盖全部 30+ API 模块；长尾功能走 `ifg api` 透传。
- MVP 不做交互式 TUI、不做 OAuth 设备码登录。

---

## 2. 现状事实（`确认`）

| 事实 | 证据 |
|------|------|
| `rg-mcp` 已有薄 REST 客户端：`/api/v1/` 前缀、Bearer 注入、状态码 → 错误 | `crates/rg-mcp/src/client.rs`（`ApiClient::{get, get_raw, get_bytes}`） |
| `rg-mcp` 用 `IRONFORGE_URL`（默认 `http://localhost:8080`）+ `IRONFORGE_PAT` 配置 | `crates/rg-mcp/src/main.rs` 顶部文档 |
| PAT 格式 `ifp_<64 hex>`，scope ∈ `repo` / `user` / `admin`（`admin` 蕴含全部），创建时默认 `repo` | `rg-http/src/api/users.rs:454`、`rg-core/src/auth/pat_scope.rs:7` |
| REST 中间件按路径要求 PAT scope（如 `/users/me`、`/notifications` 需 `user`） | `rg-http/src/lib.rs::required_pat_scope` |
| Git over HTTP 接受 Bearer 或 Basic 方式的 PAT（需 `repo` scope） | `rg-http/src/lib.rs::extract_actor_id` / `pat_actor` |
| 列表接口统一 `page` / `per_page`（默认 20，上限 100），响应为 `{ data, pagination: { page, per_page, total, total_pages, has_next, has_prev } }` | `rg-http/src/pagination.rs` |
| Git HTTP 路由前缀 `/git/<owner>/<repo>` | `CLAUDE.md`「HTTP 路由前缀」 |
| PR 创建体 `{ title, body?, head, base, draft }`，`head` 支持 `owner:branch`（fork PR） | `rg-http/src/api/pulls.rs::CreatePrRequest` |
| 合并策略 `merge` / `squash` / `rebase` | `rg-core/src/pull_request/service.rs:899` |
| Issue 更新体含 `title? / body? / state?` | `rg-http/src/api/issues.rs::UpdateIssueRequest` |
| CI 接口为 `pipelines`：列表/详情/cancel/retry、`jobs/{job_id}`（响应含 `log` 字段）、approve/rerun/play | `rg-http/src/api/ci.rs` |
| Release 资产上传：原始 body + `Content-Disposition` 或 `x-asset-filename` 头；下载带 `x-checksum-sha256` | `rg-http/src/api/releases.rs::upload_asset` |
| **WebSocket `/ws/job/{job_id}` 只认 JWT，不认 PAT** | `rg-http/src/ws.rs`（`jwt::validate_token`） |
| **服务端不写 `refs/pull/<n>/head`** | `rg-core/src/pull_request/`、`rg-git/src` 中无生产代码写该 ref |

| Issue 列表支持 `state` / `labels` / `assignee` 过滤 | `rg-http/src/api/issues.rs::ListQuery` |
| 建仓请求体含 `name / description / is_private / org / auto_init / default_branch` | `rg-http/src/api/repos.rs::CreateRepoRequest` |
| **Release 无按 tag 查询的路由**（只有 `releases/{id}`） | `rg-http/src/api/releases.rs` |

加粗的三条直接影响 CLI 设计，见 §7。

---

## 3. 总体架构

```
crates/
├── rg-client/   ← 新：类型化 REST 客户端（从 rg-mcp/src/client.rs 抽出并扩展）
├── rg-cli-user/ ← 新：二进制 `ifg`，只依赖 rg-client（+ clap / 配置 / 输出）
├── rg-mcp/      ← 改：改为依赖 rg-client，删除自带 client.rs
└── rg-cli/      ← 不变：`ironforge` 服务端/运维二进制
```

### 3.1 为什么是独立二进制

- `ironforge` 依赖 SeaORM、gix、russh、Docker runner 等，体积与编译时间都是服务端量级；用户 CLI 应是几 MB 的纯 HTTP 客户端，可单独发布到 Homebrew / Release 资产。
- 运维命令（直连 DB）与用户命令（走 API + 权限）安全语义不同，混在一个 `--help` 里易误用。

### 3.2 `rg-client` 边界

- **只依赖** `reqwest`、`serde`、`serde_json`、`thiserror`、`tokio`、`url`；**不得依赖** `rg-core` / `rg-db` / `rg-http`（保证客户端可脱离服务端编译）。
- 请求/响应 DTO 在 `rg-client` 内**手写**镜像服务端结构。MVP 不从 OpenAPI 生成：142 个注解未覆盖全部 handler（`CLAUDE.md` 踩坑 #11），生成结果会残缺。API 稳定后再评估 `progenitor` 等生成器。
- 提供：
  - `Client::new(base_url, token: Option<Secret>)`
  - 资源分组：`client.repos()`、`client.issues(owner, repo)`、`client.pulls(..)`、`client.pipelines(..)`、`client.releases(..)`、`client.users()`
  - `Paginator<T>`：基于 `pagination.has_next` 自动翻页，`--limit` 截断
  - `raw(method, path, body)`：给 `ifg api` 与 `rg-mcp` 透传用
- 错误类型：`Api { status, message, body }` / `Transport` / `Decode`；HTTP 401/403/404 映射为可读提示。

---

## 4. 命令设计（MVP）

全局参数：`--host <name|url>`、`-R, --repo <owner/name>`、`--json`、`--no-color`、`-v`。

### 4.1 认证 `ifg auth`

| 命令 | 行为 |
|------|------|
| `auth login [--host URL] [--with-token]` | MVP 只支持粘贴 PAT（交互输入或 stdin），调用 `GET /users/me` 校验后保存。提示用户在 Web 设置页创建 PAT 并选择 scope |
| `auth status` | 列出各 host 的用户名、token 前缀（`ifp_xxxx…`）、scope 是否满足常用命令 |
| `auth logout [--host]` | 删除本地凭据（不吊销服务端 PAT，提示去 Web 端吊销） |
| `auth token` | 打印当前 token（供脚本使用） |
| `auth git-credential` | 实现 git credential helper 协议，让 `git clone/push https://…/git/…` 自动复用 PAT（服务端已支持 Basic 方式的 PAT） |

> 不做账号密码登录：会撞上 MFA（TOTP）与登录限流（10/60s），且需在本地处理密码，收益不大。

### 4.2 仓库 `ifg repo`

| 命令 | API |
|------|-----|
| `repo list [owner] [--limit]` | `GET /repos` 或 `/repos/{owner}` |
| `repo view [owner/name] [--web]` | `GET /repos/{owner}/{name}` |
| `repo create <name> [--private] [--org] [--description] [--init] [--default-branch]` | `POST /repos`（`is_private` / `org` / `auto_init` / `default_branch`） |
| `repo clone <owner/name> [dir] [--ssh]` | 拼 `https://host/git/<owner>/<name>.git` 或 SSH URL，调用系统 `git clone` |

### 4.3 Issue `ifg issue`

`list [--state open|closed|all] [--label] [--assignee]`（对应 `ListQuery` 的 `state` / `labels` / `assignee`）、`view <n> [--comments]`、`create [-t] [-b | --body-file]`、`comment <n> -b`、`close <n>` / `reopen <n>`（`PATCH issues/{n}` 的 `state`）、`edit <n>`。

### 4.4 PR `ifg pr`

| 命令 | 说明 |
|------|------|
| `list [--state]` / `view <n> [--comments]` / `diff <n>` | `pulls`、`pulls/{n}`、`pulls/{n}/diff` |
| `create [--base] [--head] [-t] [-b] [--draft]` | `head` 默认当前分支；未推送时提示先 push |
| `checkout <n>` | 读 PR 的 head 仓库与分支 → `git fetch <head-repo-url> <branch>` → 建本地分支 `pr-<n>`（见 §7.2） |
| `merge <n> [--merge \| --squash \| --rebase] [--auto]` | `pulls/{n}/merge`；`--auto` → `pulls/{n}/auto-merge` |
| `review <n> --approve \| --request-changes \| --comment -b` | `pulls/{n}/reviews` |

### 4.5 CI `ifg ci`

`runs [--branch] [--status]`（`pipelines`）、`view <id>`（含 job 列表）、`logs <id> [--job] [--follow]`、`cancel <id>`、`retry <id>`、`approve <id> <job>`。

`--follow` 的实现见 §7.1。

### 4.6 Release `ifg release`

`list`、`view <tag>`、`create <tag> [-t] [-n | --notes-file] [--draft] [--prerelease] [files…]`（请求体字段为 `tag_name` / `is_draft` / `is_prerelease`）、`upload <tag> <files…>`、`download <tag> [-p pattern]`（用 `x-checksum-sha256` 校验 SHA-256，不匹配则删除文件并报错退出）。

### 4.7 透传 `ifg api`

```
ifg api <path> [-X METHOD] [-f key=value]... [--input file] [--paginate]
```

`path` 相对 `/api/v1/`，自动带 token。`--paginate` 合并所有页的 `data`。长尾功能（wiki、board、time tracking、webhook、secrets 等）在专门命令出来之前都靠它。

---

## 5. 配置与仓库上下文

### 5.1 配置文件

`$XDG_CONFIG_HOME/ironforge/hosts.toml`（macOS 同样用 `~/.config`，与 `gh`/`tea` 习惯一致）：

```toml
default_host = "forge.example.com"

[hosts."forge.example.com"]
url = "https://forge.example.com"
user = "alice"
token = "ifp_…"          # 文件权限 0600；未来可切 OS keychain
git_protocol = "https"   # 或 ssh
```

优先级：命令行 `--host` > 环境变量 `IRONFORGE_URL` + `IRONFORGE_PAT`（与 `rg-mcp` 同名，CI 中无需配置文件）> 仓库 remote 推断 > `default_host`。

写配置时若文件权限比 0600 宽则拒绝读取 token 并告警（参照 `.ai/guardrails.md` 凭据策略）。

### 5.2 从 git remote 推断 `owner/name`

在 git 工作区内，未给 `-R` 时遍历 remote（`origin` 优先）并匹配已配置 host：

- HTTP：`https://host/git/<owner>/<name>(.git)?`（注意 `/git/` 前缀，不能按 GitHub 习惯直接取前两段路径）
- SSH：`ssh://git@host:port/<owner>/<name>.git` 或 `git@host:<owner>/<name>.git`

推断可以只用 `git config --get-regexp remote\..*\.url`，不必为了读 remote 引入 gix。

---

## 6. 输出约定

- 默认输出为人读表格（TTY 下着色、分页器）；非 TTY 自动关色。
- `--json`：输出服务端 JSON 原样（列表为合并后的数组），**字段名即 API 字段名**，不另起一套 schema，降低维护成本。
- `--jq <expr>` 留作后续（需引入 `jaq`），MVP 不做。
- 退出码：`0` 成功；`1` 一般错误；`2` 用法错误；`4` 需认证（401/403）；`8` 资源不存在（404）。数值对齐 `gh`，便于脚本迁移。

---

## 7. 需要服务端配合的缺口

### 7.1 WebSocket 不接受 PAT（`ci logs --follow`）

`/ws/job/{job_id}` 只校验 JWT，持有 PAT 的 CLI 无法订阅实时日志。可选方案：

| 方案 | 代价 | 建议 |
|------|------|------|
| A. CLI 轮询 `GET pipelines/{id}/jobs/{job_id}`（2s），按已打印长度增量输出 `log` | 零服务端改动；长日志每次全量传输 | **MVP 采用** |
| B. WS 鉴权增加 PAT（`bearer.ifp_…` 子协议），要求 `repo` scope，并按 job 所属仓库复用 `can_read` | 小改动；需补测试 | 第二阶段 |
| C. 新增 `GET …/jobs/{job_id}/log?offset=N` 增量接口 | 新接口 + OpenAPI | 若日志体积成问题再做 |

### 7.2 无 `refs/pull/<n>/head`

`pr checkout` 只能从 PR 元数据取 head 仓库与分支再 fetch；head 分支删除或 fork 已删除后就无法 checkout。是否在服务端维护 `refs/pull/<n>/head`（Gitea/GitHub 均有）应单独立项评估，CLI 届时切换实现。

### 7.3 Release 无按 tag 查询接口

Release 路由只有 `releases/{id}`，没有按 tag 查询的路由。CLI 的 `<tag>` 参数先翻页 `GET releases` 再按 `tag_name` 匹配；Release 多了以后可以考虑加 `GET releases/tags/{tag}`。

### 7.4 错误体一致性

错误响应体为 `{ "error": { "code", "message", "request_id" } }`（`rg-http/src/error.rs::ErrorBody`），CLI 从 `error.message` 取展示原因、从 `error.code` 做分支（不是顶层 `message`）。实现时应抽查各模块错误体是否统一走 `AppError`，发现不一致的登记修复。

---

## 8. 测试策略

| 层级 | 做法 |
|------|------|
| `rg-client` 单元 | 对 DTO 做反序列化测试，用真实响应样本做 fixture |
| 端到端 | 起真实 HTTP 服务（随机端口）后跑 CLI。`rg-http/tests/common` 是测试私有模块，别的 crate 无法引用，因此要么把 `build_test_app_state` 挪到 `rg-http` 的 `test-support` feature 下导出，要么把 CLI 的端到端测试放进 `rg-http/tests/`（以 dev-dependency 方式依赖 CLI 库）。链路： `auth login --with-token` → `repo create` → `issue create/close` → `pr create/merge` → `release create/upload/download` 全链路 |
| remote 推断 | 用表驱动测试覆盖 HTTP `/git/` 前缀、SSH scp 写法、自定义端口、`.git` 后缀 |
| CI 环境 | 遵守「CI 无 git identity」：测试里涉及 commit 的步骤显式设置 `GIT_AUTHOR_*` / `GIT_COMMITTER_*` 环境变量，不依赖全局 git 配置 |

---

## 9. 实施分期

| 阶段 | 内容 | 验收 |
|------|------|------|
| P0 | 抽 `rg-client`，`rg-mcp` 改为依赖它（行为不变） | `rg-mcp` 现有测试通过；`rg-client` 不依赖任何服务端 crate（`cargo tree` 校验） |
| P1 | `ifg` 骨架：`auth`、`api`、配置文件、remote 推断、`--json` | `ifg api /users/me` 在测试服务上通过 |
| P2 | `repo` / `issue` / `pr`（不含 checkout） | 端到端链路测试 |
| P3 | `ci`（轮询日志）、`release`（含校验和）、`pr checkout`、`auth git-credential` | 端到端 + 手工对真实实例 |
| P4 | 服务端：WS 接受 PAT（§7.1-B）；评估 `refs/pull`（§7.2）；打包与发布（Release 资产 / Homebrew tap） | 发布流程跑通 |

---

## 10. 待决问题

1. **二进制名**：`ifg`（短，可能与别的工具重名）还是 `ironforge-cli` / `forge`？需在发布前查重。
2. **crate 名**：`rg-cli-user` 不太好听，可改为 `rg-client-cli`；或把现有 `rg-cli` 改名 `rg-server-cli`（会动到部署文档，代价大，不建议）。
3. **token 存储**：MVP 用 0600 明文文件；是否在 P3 引入 `keyring` crate 接 OS keychain？
4. **是否兼容 `tea`**：若 IronForge 计划提供 Gitea 兼容 API，用户可直接用 `tea`，本 CLI 的定位需重新权衡。目前未见兼容层，按「不兼容」设计。
