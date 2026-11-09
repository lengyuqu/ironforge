# 真实端到端：受限令牌、远程 MCP、fork PR（验证记录，2026-11-09）

> 基线 main `04433f8`。`scripts/agent-e2e.py` 覆盖了 agent 的写路径（`commit_file` → CI → `wait_pipeline` → `create_pr` / `export_pr` / `wait_repo_events`），但后来加的三件事没被它覆盖：MCP `delete_branch` 的分支白名单、`ironforge-mcp --http`（streamable HTTP 远程传输）、fork PR 的锚点与 `base_sha_when_opened`。

## 一、这次加了什么

新增 `scripts/agent-e2e-remote.py`（不改服务代码），沿用 `collab-e2e.py` 的助手（`Server` / `http` / `say` / `check` / `free_port`），自己带一个**手写的 streamable-HTTP MCP 客户端**（一次 POST 一个 JSON-RPC 消息，会话在 `Mcp-Session-Id` 头里，`Accept: application/json` 不走 SSE）。

跑法（两步：先建二进制，再跑）：

```bash
cargo build -p rg-cli -p rg-mcp
python3 scripts/agent-e2e-remote.py        # --keep 保留工作目录
```

脚本自己起 `ironforge serve --allow-host-execution`（一次性 SQLite + 主机执行 CI，仅因为实例是丢弃式的），结束时清理。

## 二、覆盖的三块

### 1. 分支白名单下的 `delete_branch`

| 场景 | 期望 | 结果 |
|---|---|---|
| 白名单内分支 `agent/target` 带 `dry_run` | 200，`dry_run: true`，分支还在 | ✅ |
| 同一分支真删（带 `expected_sha`） | 200，分支消失 | ✅ |
| 白名单外分支 `feature/outside` | **403** | ✅ |
| `POST /branches/protection`（受限令牌） | **403** | ✅ |
| 默认分支 `main`（owner 身份） | 409 `DEFAULT_BRANCH` | ✅ |
| 受保护分支 `release/1.0`（owner 建了保护规则后） | 409 `BRANCH_PROTECTED` | ✅ |
| 有未关闭 PR 的 `agent/busy` | 409 `OPEN_PULL_REQUESTS`，`details.pull_requests` 里点名 PR 编号 | ✅ |
| `expected_sha` 传过期值 | 409 `REF_ADVANCED` | ✅ |

### 2. `ironforge-mcp --http`（远程传输）

| 场景 | 期望 | 结果 |
|---|---|---|
| 无令牌 `initialize` | 401，不给会话 | ✅ |
| 持令牌 `initialize` → `tools/list` | 200，会话 id 在响应头，73 个工具（含 `commit_file`/`wait_pipeline`/`delete_branch`/`export_pr`） | ✅ |
| `tools/call`：`commit_file` → `list_pipelines` / `wait_pipeline` → `delete_branch` | 提交成功、流水线跑到 success、分支被删 | ✅ |
| 另一个令牌 | 拿到**不同**的会话 id | ✅ |
| 用 A 的会话 id + B 的令牌 | 404（与未知会话一致，不泄露存在性） | ✅ |
| 重新 `initialize` | 新会话可用，旧会话仍可用 | ✅ |

### 3. fork PR

| 场景 | 期望 | 结果 |
|---|---|---|
| bob fork → 在 fork 的 `feat` 上提交 → 向 alice 仓库开 PR | 201；`refs/pull/<n>/head` 出现在**目标仓库** | ✅ |
| `export_pr` 锚点 | `head_ref` = `refs/pull/<n>/head`，`head_ref_sha` = fork 的提交 | ✅ |
| 推进 fork 分支 | 锚点跟着走 | ✅ |
| 推进 base（`main`） | `base_sha_when_opened` **不变**，`base_tip_sha` 变成新尖端 | ✅ |

## 三、脚本输出（最后一次干净运行）

```
[01] server started — http://127.0.0.1:60672 (sqlite, host-executed CI)
[02] delete_branch — inside the limit: dry run kept it, the real call removed it
[03] delete_branch — outside the limit and the protection path are both 403
[04] delete_branch — default/protected/open-PR/stale-sha all refused with the right reason
[05] remote MCP started — POST http://127.0.0.1:60702/mcp
[06] remote MCP — initialize + tools/list ok (73 tools)
[07] remote MCP — commit_file -> wait_pipeline -> delete_branch all work over HTTP
[08] remote MCP — sessions are per token; a reconnect opens a new one without breaking the old
[09] fork PR — #2 anchored at refs/pull/2/head -> f25659b1ccac
[10] fork PR — anchor follows the fork; base_sha_when_opened stays put while the base moves
OK: branch limit, remote MCP transport and fork pull requests verified
```

## 四、写脚本时踩到的四处（都不是服务端 bug）

1. `GET /repos/{o}/{n}/branches` 返回的是**分支名字符串列表**，不是带 commit 的对象 —— 所以 `base_sha_when_opened` 的基线改成从 `export_pr` 自己的锚点读，反而更贴近要验的东西。
2. `POST /repos/{o}/{n}/fork` 会返回 **202**（异步建仓），必须等 `GET …/branches` 变 200 才能用。
3. `delete_branch` 的 409 里，PR 编号在 `error.details.pull_requests`（不是 `pulls`）。
4. `delete_branch` 的 `dry_run` / `expected_sha` 是**查询参数**，不是请求体。

服务端一个字没改 —— 这次没有暴露需要修的缺陷。

## 五、未覆盖

- **只在 SQLite、单机、主机执行 CI 下验证**：没有 Docker、没有外部 runner、没有 PostgreSQL/MySQL（脚本不碰数据库层）。
- **MCP 客户端是手写的**：没有用官方 inspector 或第三方 MCP 客户端库（环境里没有）。它只覆盖 `initialize` / `tools/list` / `tools/call` 三个方法，`notifications/*`、SSE 流（`Accept: text/event-stream`）与心跳都没走。
- **没有真实反向代理**：TLS、按 IP 限流、代理缓冲/读超时这些部署前提没有验（`mcp-remote-verification-2026-11.md` 里也记为未做）。
- **fork 场景只有一个 fork、一次 PR**：没有覆盖 fork 被删、head 分支被删后再导出、多个 fork 同一分支名。
- **没有测限流/并发**：远程 MCP 的会话上限、并发上限、请求体上限只在实现里有，脚本没打到边界。
- **不在 CI 里跑**：`--allow-host-execution` 是安全敏感开关，脚本只在丢弃式实例上手工跑。
