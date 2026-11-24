# MCP 远程传输（streamable HTTP）：设计取舍、实现与验证记录（2026-11-09）

> **注（2026-11-24）**：文中"没在 git233 Actions 上跑"属于当时的验证边界。git233 已移除，CI 现在全部在 GitHub 托管 runner 上。

> **状态**：已实现，待合并 ｜ **可信度**：实现与任务验证为「确认」，部署行为为「推测/待验证」（见第五节）｜ **来源**：本分支代码与本地测试输出
>
> 接 [agent-gaps-verification-2026-11.md](agent-gaps-verification-2026-11.md) 与 [harness 原生化设计](harness-native-design-2026-11.md) §4.3 的「传输层」。此前各文档里「SSE、streamable-HTTP 未做」的说法，对 streamable-HTTP 到此失效；**旧版 HTTP+SSE（`GET /sse` + `/messages`）仍然没做**。

## 一、做了什么

`ironforge-mcp --http [ADDR]`（默认 `127.0.0.1:8765`，或 `IRONFORGE_MCP_LISTEN`）：同一个二进制多一种传输。stdio 行为不变（分发逻辑抽到 `rg_mcp::server`，两种传输共用同一份 `dispatch`，工具集合因此相同；额外新增 `ping` 方法与 REST 客户端 120 秒总超时，后者对 stdio 也生效）。

| 请求 | 含义 |
|---|---|
| `POST /mcp` + `initialize` | 认证、开 MCP 会话，会话 id 在响应头 `Mcp-Session-Id` |
| `POST /mcp` 其他消息 | 需要 `Mcp-Session-Id` 和**同一个**令牌；通知/客户端回应返回 202；支持 JSON 批量 |
| `DELETE /mcp` | 结束会话（204） |
| `GET /mcp` | 405（服务器不主动发消息） |
| `GET /healthz` | 负载均衡存活探测，不含信息 |

`tools/call` 的响应：客户端 `Accept` 含 `text/event-stream` 时是一个 SSE 流（一个 `message` 事件，等待期间每 15 秒一行 `: heartbeat` 注释，并带 `X-Accel-Buffering: no`）；否则是普通 JSON。其他方法一律 JSON。

### 取舍：独立进程，不挂进 `rg-http`

沿用 harness 设计 §4.3 的决定，写明理由：

| | 独立进程（采用） | 并入 `rg-http` |
|---|---|---|
| 授权 | 不持有任何凭据，不做任何权限判断；每次工具调用带调用者自己的令牌转发给 REST，**scope 阶梯、仓库/分支白名单、幂等键、审计、限流全由服务端既有代码决定**，这里没有可能与之不一致的第二份检查 | 进程内直接调服务层容易旁路 PAT 中间件，需要自己重做一遍检查 |
| 攻击面 | 服务端不多开路由；MCP 进程可单独放 DMZ、单独限流、单独重启 | 服务端多一个长连接入口，长轮询占用服务端连接 |
| 代价 | 多一个进程要部署；多一跳本机 HTTP；`initialize` 多一次探测请求 | 无 |
| 与并行工作的冲突 | 不改 `rg-http/src/lib.rs`（本 PR 对它**零改动**） | 要改路由注册 |

### 认证与会话

- 没有 `Authorization: Bearer <令牌>`（1–512 个可见 ASCII 字符）的请求，在做任何事之前返回 401 + `WWW-Authenticate: Bearer`；Basic 等其他方案同样 401。
- `initialize` 用该令牌探测一次 REST `GET /api/v1/users/me`：401 = 令牌不存在/过期，拒绝（不建会话，垃圾令牌填不满会话表）；200 或 403 = 真实令牌（403 只表示没有 `user` scope，agent 令牌本来就没有）；其他或连不上 = 502。探测依赖服务端「真实 PAT 但 scope 不够 → 403，未知令牌 → 401」的现有行为。可接受任何 Bearer 凭据（PAT 或 JWT），REST 照常判断；推荐 PAT，JWT 一小时过期。
- 会话绑定**令牌的 SHA-256**。别的令牌（含同一账号的另一个令牌）拿着这个会话 id，得到的和「不存在的 id」完全相同的 404（正文逐字节相同）；也不能 `DELETE` 它。令牌被吊销后，会话在**下一次工具调用**失效（REST 返回 401，作为工具错误给出）；会话本身要等空闲过期或 `DELETE` 才从表里消失。
- 每个会话有自己的内存 `SessionStore`（协作会话凭据）。**不读 `IRONFORGE_COLLAB_SESSION_FILE`**：否则一个令牌可能接上另一个令牌写下的协作会话。协作会话密钥仍不出现在工具结果里（测试断言）。
- 上游地址只来自运维配置 `IRONFORGE_URL`，不取自请求，不是开放代理。
- 校验 `Origin`（默认拒绝任何带 `Origin` 的请求；非浏览器 agent 不带该头；`IRONFORGE_MCP_ALLOWED_ORIGINS` 逗号分隔，`*` = 全部）与 `MCP-Protocol-Version`（支持 2025-06-18 / 2025-03-26 / 2024-11-05）。

### 限制（可用环境变量调）

| 限制 | 默认 | 超限 |
|---|---|---|
| 会话总数 / 每令牌 | 1000 / 16 | 429 + `Retry-After` |
| 空闲过期 | 30 分钟（有调用在跑的会话不算空闲） | 404，客户端重新 `initialize` |
| 同时进行的调用：全局 / 每会话 | 64 / 4 | 429 |
| 并发令牌探测 | 16 | 429 |
| 请求体 | 4 MiB | 413 |

调用占用的名额**在底层调用真正结束后才释放**，客户端断开也一样（测试覆盖），所以上限数的是仍在跑的工作。这些是进程内、粗粒度的限制，不是按 IP 的限流；按 IP/按令牌的限流放在反向代理。

## 二、部署约束（必读）

1. **TLS 在反向代理终止**。本进程只讲明文 HTTP，令牌在请求头里；绑定到非回环地址时启动日志会警告。
2. **单实例**。会话表在进程内（与 `CLAUDE.md`「多实例部署的进程内状态」同类）；多实例需会话粘滞，重启会丢全部会话（客户端收到 404，按规范重新 `initialize`，协作会话需 `collab_start_session` 重新登记）。
3. **反向代理**：对 `/mcp` 关闭响应缓冲（nginx：`proxy_buffering off;`，本进程已发 `X-Accel-Buffering: no`），`proxy_http_version 1.1`，**读超时 ≥ 75 秒**（见 4），不要做响应压缩或把长连接截断；不要给 `/mcp` 设请求体上限低于 4 MiB。
4. **长轮询与超时的关系**：`wait_repo_events` / `wait_pipeline` 单次最多等 55 秒（工具参数上限），底层 REST 的上限是 60 秒，REST 客户端总超时 120 秒。SSE 响应在等待期间每 15 秒发一行心跳，所以空闲超时较短（如 30 秒）的代理也不会断；**不接受 SSE 的客户端拿到的是不带心跳的 JSON**，那时代理的读超时必须 ≥ 60 秒，建议 ≥ 75 秒。agent 一侧的 tool 超时要大于它传的 `timeout` 加几秒。
5. 重试：客户端断开不会取消已发出的底层调用（与 stdio 无取消一样）。写工具重试请带同一个 `idempotency_key`（`commit_file`/`delete_file`/`create_pr` 已支持），否则可能执行两次。
6. 配置示例：

```bash
IRONFORGE_URL=http://127.0.0.1:8080 ironforge-mcp --http 127.0.0.1:8765
```

客户端（支持 streamable HTTP 的 MCP 客户端，如 Claude Code）：

```json
{ "mcpServers": { "ironforge": {
    "type": "http",
    "url": "https://mcp.example.com/mcp",
    "headers": { "Authorization": "Bearer <token>" } } } }
```

令牌仍应最小 scope（`repo:read` / `repo:collab` / `repo:write`，必要时带 `repos`、`refs` 白名单）。

## 三、证据（逐层）

### 实现证据（文件存在于本分支 diff）

`crates/rg-mcp/src/remote.rs`（传输）、`src/server.rs`（共享分发）、`src/main.rs`（`--http`）、`src/lib.rs`、`Cargo.toml`（新增 workspace 已有依赖 `axum`、`futures`、`sha2`）；测试 `crates/rg-http/tests/mcp_remote_tests.rs`。未改 `rg-http/src` 任何文件，未新增 MCP 工具、REST 路由或迁移。

### 任务验证（本分支，本地 macOS，SQLite）

`crates/rg-http/tests/mcp_remote_tests.rs` 起**真实** IronForge 服务（真实 PAT、真实仓库）、在同进程里起真实的远程传输，经真实 HTTP 往返（共 9 个测试，覆盖）：

| 要求 | 测试 |
|---|---|
| 无令牌/Basic/未知令牌/伪造 JWT 被拒；会话建立后每个请求仍要令牌；`GET` 405；`/healthz` 公开 | `a_request_without_a_valid_token_is_refused` |
| 工具集合与 stdio 的 `tools/list` 一致，且包含 `collab::TOOL_NAMES` 全部 | `the_tool_set_is_the_stdio_tool_set` |
| 工具以调用者身份到达 REST；SSE 与 JSON 两种响应；陌生人读不到私有仓库 | `tools_reach_the_rest_api_as_the_caller_over_json_and_sse` |
| `repo:read` 令牌不能 `commit_file`、不能开协作会话（403）；单仓库令牌读别的仓库/`list_repos` 被拒；`agent/*` 分支令牌写 `main` 被拒、写 `agent/one` 成功；令牌被删后下一次调用 401 | `a_limited_token_is_limited_through_mcp_as_over_rest` |
| 别的令牌用/删我的会话得到与未知 id 相同的 404（含同账号另一令牌）；同令牌两个会话不共享协作凭据；秘密不外泄；`DELETE` 后会话失效 | `a_session_belongs_to_the_token_that_opened_it` |
| 新 TCP 连接可续用会话；传输重启后旧会话 404、重新 `initialize` 可用 | `a_client_can_reconnect_and_start_over` |
| 长轮询在事件到达时提前返回（SSE，含心跳）；无事件时在超时返回 `timed_out` | `a_long_poll_returns_on_the_event_and_keeps_the_connection_warm` |
| 客户端中途断开后，名额仍被占到底层调用结束，之后释放 | `a_dropped_call_keeps_its_slot_until_it_has_really_ended` |
| 每令牌会话上限与 `DELETE` 腾位；缺会话头 400；错误协议版本头 400；通知 202；批量；垃圾体 400；批量里的 `initialize` 被拒；`Origin` 默认拒绝/可放行；上游不可达 502 | `protocol_edges_and_limits` |

命令与结果见第四节。**这些证明的是任务分支上的行为，不证明合并后仍通过。**

## 四、本地命令结果

```
cargo test -p rg-http --test mcp_remote_tests   # 9 passed
cargo clippy -p rg-mcp -p rg-http --all-targets -- -D warnings   # 无警告
cargo test -p rg-http -p rg-mcp   # 116 个测试二进制，741 个测试通过，0 失败（含 collab_contract_tests 与 rg-mcp 的 collab_mcp/read_tools_mcp/write_tools_mcp）
```

## 五、没有验证 / 没有做

- **没有**用真实的 MCP 客户端（Claude Code、Cursor 等）连过；协议行为只按规范与本测试的手写客户端验证。SSE 响应只验证了单个 `message` 事件加心跳注释行的形态。
- **没有**经真实反向代理（nginx/Caddy/云负载均衡）验证缓冲、超时与心跳；第二节的代理配置是依据 HTTP 语义与 nginx 文档的**推测，待验证**。
- 没做：旧版 HTTP+SSE 传输（`GET /sse`）、服务器主动消息与可恢复流（`GET /mcp` 返回 405）、TLS、多实例共享会话、按 IP 限流、工具调用的取消。`--sse` 仍报错退出，提示改用 `--http`。
- 仅 SQLite；服务端逻辑在 PostgreSQL/MySQL 上的行为与本传输无关（本传输不碰数据库）。
- 吊销令牌后，已建立的会话在下一次工具调用才失效，会话条目留到空闲过期。
- 契约注册表 `GET …/collaboration/capabilities` 的 `mcp_transport.transports` 仍是 `["stdio"]`：服务端看不到独立运行的 MCP 进程是否在监听，所以不广告它（该测试与注册表未改）；工具表未变，防漂移测试未受影响。
- CI：PR 上 `rg-http` 的集成测试只在 `rg-http` 自己的文件变化时才跑；本 PR 改动了 `rg-http/tests/`，会触发。**未在 git233 的 Gitea Actions 上实跑验证。**
