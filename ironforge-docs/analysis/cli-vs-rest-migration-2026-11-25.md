# 数据迁移集成 gh / tea / cnb / glab CLI —— 技术调研

> **状态**：调研完成 ｜ **可信度**：本机实测（非估算）｜ **日期**：2026-11-25
> **实测环境**：macOS / gh 2.105.0 / tea 0.16.0 / cnb 1.18.48 / glab 未安装（据上游 README 核实）
> **被测代码**：`crates/rg-core/src/import/`、`crates/rg-http/src/api/imports.rs`

---

## 0. 结论

**能集成，但不能作为迁移主链路。只在一件事上成立：当 IronForge 与用户跑在同一个 OS 账号下时，用 CLI 读取「已登录凭据」比让用户重新生成 PAT 体验更好。**

三个判断，按确定性排序：

| # | 判断 | 依据 | 严重程度 |
|---|------|------|---------|
| 1 | **CNB 走 CLI 明确更差**——不解决真正的痛点，还多引入 Node 运行时和一层响应信封 | 实测：`list-issues` 与 `get-issues-in-batch` 输出均不含 `body`；body 只在单查端点返回，是 CNB API 的设计约束 | 高 |
| 2 | **CLI 的凭据模型与多租户服务端天然冲突**——四个 CLI 的凭据全部按 `$HOME` 存放，单进程服务只能对应一个身份 | 实测四个凭据文件路径，见 §3.1 | 高 |
| 3 | **错误分类能力会整体坍塌**——现有 `upstream_api_error()` 能区分「用户该修的 4xx」和「运维该看的 5xx」，CLI 只剩 exit=1 | `gh help exit-codes` + 实测 | 中 |

推荐路线见 §5：**不上 CLI，改做「凭据发现」（读 CLI 的凭据文件拿 token，仍走现有 reqwest 链路）**。它吃掉 CLI 唯一的价值，不承担任何 §3 里的成本。

> 补充：调研期间 CNB 导入已随 `6acf6aea` 接线生效，实际改动 **9 行**。相比之下 §5 方案 B（CLI 桥）估算 700–900 行 + 150 MB 镜像 —— 同一目标，成本差两个数量级。详见 §1.2。

---

## 1. 现状基线（实测）

### 1.1 现有架构

```
import_task (DB) → run_import() → match platform
                                    ├─ "github" → run_github_import → GitHubClient(reqwest)
                                    ├─ "gitlab" → run_gitlab_import → GitLabClient(reqwest)
                                    ├─ "gitea"  → run_github_import + gitea_api_base()
                                    └─ "git"    → run_git_import（仅 clone）
```

- **传输层**：`reqwest` 直连，无 CLI、无 subprocess
- **认证**：`resolve_source_auth()` 两级——任务自带 token（AES-GCM 加密存 `auth_token_encrypted`）→ 回退到用户级 `remote_credential` host 字典
- **并发模型**：`start_import` 起 `tokio::spawn` 后台跑，进度写 `import_tasks`
- **错误分类**：`import/mod.rs::upstream_api_error()` —— 4xx 转 `InvalidInput`（消息回显给用户），5xx 记 `tracing::error!` 并对用户脱敏

### 1.2 调研期间的状态变化：CNB 已接线（含关键量化）

调研开始（05:02）时，CNB 导入是**未接线的死代码**：`run_import()` 的 match 无 `"cnb"` 分支（落到 `other =>` 返回 `unsupported platform`），API 白名单也不含 `cnb`，`run_cnb_import()`（154 行）+ `cnb_client.rs`（412 行）无任何调用点，且无 `#[allow(dead_code)]` 豁免。

调研过程中，`6acf6aea merge: bring cnb/main into the release line`（2026-11-25 05:04:38）合入，接线已生效。实际改动量：

| 改动 | 位置 | 行数 |
|------|------|------|
| match 加分派 | `crates/rg-core/src/import/service.rs:168` | 1 |
| 顶部 use 补 `cnb_client` | `crates/rg-core/src/import/service.rs:29-32` | 4 |
| API 白名单放行 `cnb` + 错误文案 | `crates/rg-http/src/api/imports.rs:91-96` | 2 |

**这组数字本身就是本次调研最硬的论据**：把 CNB 接上线花了 **9 行**，而 §5 方案 B（CLI 桥）估算是 **700–900 行 + 150 MB 镜像**。同一个目标，成本差两个数量级。

`cnb_client.rs` 写对的 API 细节（注释记录的三个坑——bare array / `number` 是 string / list 无 body 需 N+1）已逐条实测复核，见 §2.3。

---

## 2. 四个 CLI 实测能力矩阵

### 2.1 总表

| 维度 | gh 2.105.0 | tea 0.16.0 | cnb 1.18.48 | glab（未装） |
|------|-----------|-----------|-------------|-------------|
| 语言/分发 | Go 单二进制 | Go 单二进制 | **npm 包**（`@cnbcool/cnb-cli` → `bin/cnb.js`） | Go 单二进制 |
| 二进制体积 | **37.3 MB** | **26.3 MB** | 需 Node 运行时 | ~40 MB（估） |
| 通用 API 透传 | ✅ `gh api` | ✅ `tea api` | ❌ 无（按 module/tool 代码生成） | ✅ `glab api` |
| 结构化输出 | ✅ `--json fields` + `--jq` | ⚠️ 领域命令只有 `--fields`（人类可读列）；`tea api` 出 JSON | ⚠️ 默认 YAML 风格伪格式；`-v` 才出 JSON 且带信封 | ✅ `--output json` |
| 分页 | ✅ `--paginate --slurp` | ❌ 仅 `--page/--limit` 手工翻页 | ❌ `--page/--page-size` 手工 | ✅ `--paginate` |
| 凭据位置（实测） | `~/.config/gh/hosts.yml` (84 B, 0600) | `~/Library/Application Support/tea/config.yml` (456 B, 0600) | `~/.cnb/token` (432 B, 0600) | `~/.config/glab-cli/config.yml` |
| 凭据 env 注入 | `GH_TOKEN` / `GH_HOST` | `GITEA_SERVER_URL` / `GITEA_SERVER_TOKEN` | 未确认 | `GITLAB_TOKEN` / `GITLAB_HOST` |
| 多实例 | ✅ `--hostname` (GHES) | ✅ `-l <login>`（天然多实例） | ❌ 单 token | ✅ 自动识别 remote hostname |
| 退出码语义 | 0 / 1 / 2 / 4（官方约定） | 0 / 1 | 未文档化 | 0 / 1 |

### 2.2 退出码：错误分类能力实测

`gh help exit-codes` 官方原文：

```
- 成功 → 0
- 任何失败 → 1
- 被取消 → 2
- 需要认证 → 4
```

实测两例（不可达端点）：

```
gh  (GH_HOST=10.255.255.1 GH_TOKEN=fake) → exit=1   "Get \"https://10.255.255.1/api/v3/user\": dial tcp ...: i/o timeout"
tea (tea api -l git233 /user/repos)      → exit=1   "Error: request failed: ... dial tcp ...: i/o timeout"
```

**结论**：`401 / 403 / 404 / 429 / 5xx / 网络超时` 在 gh 和 tea 里全部塌缩为 `1`。要恢复分类能力只能正则解析 stderr 文本——而 stderr 是给人类看的产品文案，不是契约，跨小版本可改。

现有 `upstream_api_error()` 的核心价值就在这里：**4xx 回显给用户（他自己能修 token/URL），5xx 只记日志并脱敏**。走 CLI 等于主动放弃这个安全边界，把一个「运维事故」和「用户填错 token」都变成同一句 `Error: request failed`。

### 2.3 CNB 专项：CLI 不解决 N+1

`cnb_client.rs` 头注释记录的三个坑，逐条实测复核：

| 坑 | 实测结果 | CLI 是否改善 |
|----|---------|-------------|
| 响应是 bare array，不是 `{data: [...]}` | ✅ 属实（REST 直连）；**但 CLI 反而多一层信封**：`{status, trace, header, contentType, data, page, pageSize, total, totalPages}` | ❌ 变差 |
| `number` 是 string（`"65"`） | ✅ 属实，CLI `-v` 输出仍是 `"number": "65"` | ❌ 无改善 |
| list 端点不带 body → N 条 issue 需 N+1 请求 | ✅ **实测确认**：`list-issues -v` 的 issue keys 为 `number/state/state_reason/title/invisible/author/assignees/labels/comment_count/priority/created_at/updated_at/started_at/ended_at/last_acted_at/closed_at`，**无 `body`** | ❌ **完全无改善**（同一 API，同样的 N+1） |

实测命令与输出（issue keys 逐字）：

```bash
cnb issues list-issues --repo lengyuqu/ironforge --page-size 1 -v | jq '.data[0] | keys'
# ["number","state","state_reason","title","invisible","author","assignees","labels",
#  "comment_count","priority","created_at","updated_at","started_at","ended_at",
#  "last_acted_at","closed_at"]
```

**额外成本**：cnb CLI 是 npm 包，运行镜像是 `debian:bookworm-slim`（`Dockerfile` 运行阶段），无 Node。要集成得先往镜像里塞一个 Node 运行时（≈ 50 MB+），只为发几个 HTTP 请求。

### 2.4 tea 的特殊情况

`tea issues list` **没有 JSON 输出**，只有 `--fields index,state,author,title,...` 选列，输出是人类可读表格。想拿 JSON 必须用 `tea api /repos/{owner}/{repo}/issues`。

也就是说：**tea 的领域命令在迁移场景里不可用，只能用 `tea api`。而 `tea api` 就是「用 tea 的身份发一个 HTTP 请求」——绕一圈回到 reqwest。**

---

## 3. 硬约束（按严重程度）

### 3.1 🔴 多租户凭据边界（不可绕过）

四个 CLI 全部把凭据存在**当前 OS 用户的 HOME** 下，实测：

```
~/.config/gh/hosts.yml                                84 B  0600
~/Library/Application Support/tea/config.yml         456 B  0600
~/.cnb/token                                         432 B  0600
~/.config/glab-cli/config.yml                        (未安装)
```

IronForge 是单进程多用户服务（`ironforge serve`，Docker 里以单一 `ironforge` 用户跑）。一个 HOME 只能对应一个平台身份。若用户 A 和用户 B 都要从 GitHub 导入：

- CLI 方案需要在进程内为每个用户伪造 HOME（`tempfile` + `tea logins add --url --token` 逐次写入 + 用完清理）
- 这意味着 **每个导入任务都要 fork 一次子进程、建一次临时目录、写一次 token 到磁盘明文**
- 现有 REST 方案里 token 全程在内存（`SourceAuth`，刻意没实现 `Debug` 防泄漏），磁盘上只有 AES-GCM 密文

这是一次**安全性的净倒退**，换来的只是「少填一个 PAT」。

### 3.2 🔴 测试设施为零

```
$ grep -n "dev-dependencies" -A 15 crates/rg-core/Cargo.toml
[dev-dependencies]
tempfile = "3"
```

全仓无 `wiremock` / `mockito` / `httpmock`。现有 import 链路没有单元测试兜底。

CLI 化之后，测试只有两条路：
1. 造 4 个 fake CLI 脚本（sh/bat 跨平台），模拟 stdout/stderr/exit code —— 维护 4 份假实现，等于把「CLI 输出契约」再写一遍
2. 只能跑真集成测试（要真 token、真网络、真账号）

### 3.3 🟠 输出契约不稳定

| CLI | 稳定性风险 |
|-----|-----------|
| gh | 相对最好：`gh api` 直出 API body；`--json` 字段列表在 `gh help` 里声明（但会随版本增删） |
| tea | 领域命令无 JSON；`tea api` 依赖 Gitea API 版本 |
| cnb | 默认输出是自造的 YAML 风格（`status: 200` / `data[1]:` / `- number: "65"`），非标准；`-v` 才 JSON 且带私有信封 |
| glab | `glab api` → GitLab API v4，相对稳 |

四个 CLI 的「JSON 形态」互不相同，要统一就得写 4 个适配器 + 4 套字段映射——比现在的 4 个 REST client 更厚。

### 3.4 🟡 部署与镜像膨胀

| 项 | 增量 |
|----|------|
| gh 二进制 | +37.3 MB |
| tea 二进制 | +26.3 MB |
| glab 二进制 | +~40 MB |
| cnb | +Node 运行时（≈50 MB+），或改 Go 重写 |
| **合计** | **≈ +150 MB**（当前运行镜像为 `debian:bookworm-slim` + strip 过的三进制） |

另外 `Dockerfile` 是多阶段构建，CLI 需要在运行阶段 `apt-get install` 或从外部 COPY 静态二进制，且要处理架构矩阵（amd64/arm64）。

### 3.5 🟡 进程生命周期

- `tokio` 里跑子进程要用 `tokio::process::Command`，必须配 `kill_on_drop(true)` + `tokio::time::timeout`，否则大仓库导入卡死时子进程泄漏
- stdout 需要流式读取（`gh api --paginate` 大响应可能几十 MB，`output()` 一次性读入内存）
- 并发导入 N 个任务 = N 个子进程，需要信号量限流（现有 REST 方案靠 reqwest 连接池天然收敛）

### 3.6 🟢 许可证

四个 CLI 均为 MIT（cnb-cli 待确认）。以**独立进程调用**方式集成不构成衍生作品，无传染风险；若打进镜像分发，保留版权声明即可。此项不是阻碍。

---

## 4. 一件事：CLI 真正比 REST 强的地方

只有一个场景——**「用户本机已经 logged in，不想再生成 PAT」**。

对自托管单机部署（`ironforge serve` 跑在开发者自己机器上，或本地 Docker 挂了 `-v $HOME:/home/ironforge`）：
- 用户 `gh auth login` 过 → IronForge 直接能读 `~/.config/gh/hosts.yml` 拿 token
- 同理 tea / glab

但注意：**此时不需要 CLI 二进制，只需要读那个文件**。凭据文件是简单 YAML/JSON，`serde_yaml` 已在依赖里。

---

## 5. 推荐方案（三档，选一）

### 方案 A：凭据发现（推荐，成本最低）

不做 subprocess，只做「CLI 凭据文件 → token」的读取器，读完仍走现有 reqwest 链路。

```
新增 crates/rg-core/src/import/credential_discovery.rs
  discover(platform, host) -> Option<SourceAuth>
    ├─ gh   → ~/.config/gh/hosts.yml           (hosts.<host>.oauth_token)
    ├─ tea  → <XDG or macOS>/tea/config.yml    (logins[].url/token 匹配 host)
    ├─ glab → ~/.config/glab-cli/config.yml    (hosts.<host>.token)
    └─ cnb  → ~/.cnb/token                     (docker-style auths 文档)
```

**接入点**：`resolve_source_auth()` 的三级回退——任务 token → 用户 host 字典 → **本机 CLI 凭据**。

| 项 | 值 |
|----|----|
| 新增代码 | ≈ 250 行（4 个解析器 + 1 个 trait + 测试） |
| 新增依赖 | 0（`serde_yaml` / `serde_json` 已有） |
| 镜像增量 | 0 |
| 测试 | 纯文件解析，可用 `tempfile` 写字面量 YAML 测，无需网络 |
| 前置条件 | 只能在同一 OS 账号下工作；需在 UI 明确标注「仅本机部署可用」 |
| 安全 | 只读，不落盘，不 fork；读到即用，`SourceAuth` 仍无 `Debug` |

**收益**：吃掉 CLI 全部的真实价值（免 PAT），不承担 §3 的任何成本。

### 方案 B：`*_api` passthrough 子进程（可选，若确需 CLI 语义）

仅当出现「某个平台只有 CLI 能做的事」时才做，且**只允许透传 API 调用**，禁止用领域命令（`gh issue list --json` 之类）。

统一入口：

```rust
// crates/rg-core/src/import/cli_bridge.rs
async fn run_api(
    cli: Cli,                 // Gh | Tea | Glab | Cnb
    home: &Path,              // 每任务独立 HOME
    args: &[&str],
    timeout: Duration,
) -> Result<serde_json::Value>
```

必须同时实现的 4 件事（缺一即事故）：

1. **HOME 隔离**：`tempfile::TempDir` + 逐次 `tea logins add` / 每次写 token 文件，`Drop` 时删除
2. **错误状态还原**：解析 stderr 中的 `(HTTP 4xx)` / `dial tcp` 等模式，映射回 `CoreError::InvalidInput` vs `internal`，恢复 §2.2 的分类
3. **显式分页循环**：gh 用 `--paginate --slurp`；tea/glab 手工翻页；cnb 读信封里的 `totalPages`
4. **进程护栏**：`kill_on_drop(true)` + `timeout()` + `Semaphore(1)` 限流 + stdout 流式读取

| 项 | 值 |
|----|----|
| 新增代码 | ≈ 700–900 行（含 4 个适配器 + 4 个 fake CLI 测试桩） |
| 镜像增量 | ≈ +150 MB |
| 维护 | 4 个 CLI 的输出契约 × 各自的升级节奏 |

### 方案 C：用领域命令替代 REST（不推荐）

`gh issue list --json number,title,body --paginate` 这类。问题：
- 字段由 CLI 白名单决定，取不到的关系数据（reviews 里的行内评论、reactions）无法补齐
- tea 根本没有 JSON 领域输出
- cnb 的 `-v` 信封私有

---

## 6. 对当前 CNB 工作的具体建议

`cnb_client.rs` 已写对（三个坑都记在注释里且实测复核通过），接线也已随 `6acf6aea` 生效。**剩余动作只有两项**：

| # | 动作 | 位置 | 价值 |
|---|------|------|------|
| 1 | **把 body/评论拉取改成并发**：`run_cnb_import` 目前是 `for issue in issues` 串行 `await`，每条 issue 要 2 次请求（`get_issue` 拿 body + `list_issue_comments`），总耗时 ≈ 2N × RTT。改成 `futures::stream::iter(...).buffer_unordered(8)` 可把墙钟时间压到 1/8。**这是当前 CNB 导入真正的瓶颈** | `service.rs:1387-1420` | 高 |
| 2 | 明确 CNB 能力边界并写进注释：无 milestone 概念、PR 导入未经真实实例验证 | `service.rs:1966` `supports_metadata` | 中——避免 UI 勾选后静默无数据 |
| 3 | 前端导入表单补 `cnb` 选项 | `web/src/` | 中——后端已放行，前端无选项则能力不可见 |

**关于 `get-issues-in-batch`：实测证伪，不要走这条路。**

当初的判断是「批量端点或许能干掉 N+1」，实测结果否定了它：

```bash
cnb issues get-issues-in-batch --repo lengyuqu/ironforge --numbers 65 -v | jq '.data[0] | keys'
# ["number","state","state_reason","title","invisible","author","assignees","labels",
#  "comment_count","priority","created_at","updated_at","started_at","ended_at",
#  "last_acted_at","closed_at"]        ← 与 list 端点完全一致，仍无 body
```

**结论：`body` 只在单查端点返回，这是 CNB OpenAPI 的设计约束，任何客户端（CLI 或 REST）都逃不掉 N 次请求。** 这条实测同时关掉了两个门：批量端点优化，以及「CLI 或许能省请求」的最后一点可能性。

---

## 7. 调研中唯一值得单独记住的一条

> CNB 的 issue 列表与详情是**字段割裂**的：列表给元数据不给正文，详情给正文。这条设计约束把「N+1」变成不可优化的常数项，只能靠并发摊薄，不能靠换客户端消除。
>
> 任何声称「换 CLI 就能解决 CNB 导入慢」的方案，都可以用上面那行 `jq '.data[0] | keys'` 三秒证伪。

---

## 8. 给决策的一句话

> CLI 集成是**体验优化**，不是**能力扩展**。它能解决的唯一问题是「用户不想填 token」，而这个问题的正解是读凭据文件（方案 A，0 依赖 0 镜像增量），不是拉 4 个二进制 + 150 MB 镜像 + 一套子进程护栏进来。
>
> CNB 已经接上线（9 行），N+1 实测不可消除（批量端点同样不带 body）。当前该做的是把 `run_cnb_import` 里的串行 `for` 循环改成并发，那才是导入慢的真正原因。
