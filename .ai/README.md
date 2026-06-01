# IronForge AI Agent 集成指南

> **本目录为 AI Agent（Claude Code / Cursor / Copilot / Cline 等）提供接入规范。**
> 遵循此规范，AI 工具可直接读取仓库结构、调用 API、理解项目上下文。

---

## 快速开始

### 1. MCP Server（推荐方式）

IronForge 内置 **MCP Server**（`rg-mcp` crate），提供结构化的仓库/Issue/PR 数据访问。

```bash
# 启动 MCP Server（stdio 模式，直接对接 AI 工具）
ironforge mcp --repo /path/to/your/repo

# SSE 模式（HTTP 长连接，适合远程 AI 工具）
ironforge mcp --transport sse --port 4000
```

**Claude Code / Cline 配置**（`~/.claude/mcp.json` 或项目 `.claude/mcp.json`）：

```json
{
  "mcpServers": {
    "ironforge": {
      "command": "ironforge",
      "args": ["mcp", "--repo", ".", "--transport", "stdio"],
      "env": {}
    }
  }
}
```

**Cursor 配置**（`~/.cursor/mcp.json`）：

```json
{
  "mcpServers": {
    "ironforge": {
      "command": "ironforge",
      "args": ["mcp", "--repo", ".", "--transport", "stdio"]
    }
  }
}
```

---

### 2. REST API（AI 友好端点）

IronForge 提供专门为 AI Agent 设计的 REST 端点，路径前缀 `/api/v1/ai/`：

| 端点 | 方法 | 说明 |
|--------|------|------|
| `/api/v1/ai/repos/{owner}/{name}/summary` | GET | 仓库摘要（AI 友好格式） |
| `/api/v1/ai/repos/{owner}/{name}/issues?state=&limit=` | GET | Issue 列表（摘要格式） |
| `/api/v1/ai/repos/{owner}/{name}/prs?state=&limit=` | GET | PR 列表（摘要格式） |
| `/api/v1/ai/repos/{owner}/{name}/tree?ref=&path=` | GET | 文件树（待实现） |
| `/api/v1/ai/repos/{owner}/{name}/search/code?q=` | GET | 代码搜索（待实现） |

**认证**：所有 API 端点（包括 AI 端点）使用 Bearer Token：

```bash
curl -H "Authorization: Bearer <your-token>" \
  http://localhost:3000/api/v1/ai/repos/octocat/Hello-World/summary
```

---

### 3. OpenAPI Spec

完整 OpenAPI 3.0 规范自动生成，可从运行中的服务器获取：

```
GET http://localhost:3000/api-docs/openapi.json
GET http://localhost:3000/api-docs/   （Swagger UI）
```

本目录不存储 openapi.json 副本（文件较大且随版本变化），AI Agent 应实时拉取：

```bash
curl http://localhost:3000/api-docs/openapi.json -o .ai/openapi.json
```

---

## 项目上下文文件

AI Agent **必须**在阅读代码前先读取以下文件：

| 文件 | 用途 | 优先级 |
|------|------|--------|
| `CLAUDE.md` | AI 统一入口：项目结构、关键约定、已实现功能清单、踩坑清单 | ⭐ 最高 |
| `ARCHITECTURE.md` | 完整架构方案：技术选型、模块设计、数据库模型 | ⭐ 高 |
| `CONTRIBUTING.md` | 开发规范：crate 边界规则、编码规范、提交规范 | ⭐ 高 |
| `AGENT.md` | Agent 轻量入口（适合快速上手） | 中 |

---

## Prompt 模板

`.ai/prompts/` 目录包含预设 prompt 模板，AI Agent 可直接使用：

| 模板文件 | 用途 |
|-----------|------|
| `repo-summary.md` | 生成仓库摘要的 prompt |
| `issue-analysis.md` | 分析 Issue 并生成回复建议的 prompt |
| `code-review.md` | 代码审查 prompt（结合 PR diff） |

---

## MCP Tools 清单

通过 MCP Server，AI Agent 可调用以下工具：

### Repository
- `list_repos(owner)` — 列出用户所有仓库
- `get_repo(owner, name)` — 获取仓库详情
- `create_repo(name, description, private)` — 创建新仓库

### Issue
- `list_issues(owner, repo, state)` — 列出 Issue（open/closed/all）
- `get_issue(owner, repo, number)` — 获取 Issue 详情
- `create_issue(owner, repo, title, body, labels)` — 创建 Issue
- `update_issue(owner, repo, number, title, body, state)` — 更新 Issue
- `create_comment(owner, repo, number, body)` — 添加 Issue 评论

### Pull Request
- `list_prs(owner, repo, state)` — 列出 PR
- `get_pr(owner, repo, number)` — 获取 PR 详情
- `create_pr(owner, repo, title, body, head, base)` — 创建 PR
- `merge_pr(owner, repo, number, method)` — 合并 PR

### File Content
- `list_tree(owner, repo, ref, path)` — 浏览文件树
- `get_file(owner, repo, ref, path)` — 读取文件内容

---

## 踩坑经验（AI 必读）

1. **Git 协议**：pkt-line 用 `read_pkt_line`；receive-pack report-status 整体 sideband；thin pack 需 `--fix-thin`；`for-each-ref` 不列 HEAD
2. **Axum**：`nest()` 要求相同 State 类型；TLS 用 `axum-server`；`impl IntoResponse` 不能混用返回类型
3. **SeaORM**：单行更新先 `find` 再 `ActiveModel`；批量删除用 `delete_many().filter()`
4. **SQLite FTS5**：触发器中不要用 `'delete'` 命令语法，用 `DELETE FROM fts WHERE rowid = old.id`

---

## 贡献

当 AI Agent 完成非 trivial 的工作后，应更新以下文件：

1. `CLAUDE.md` — 更新"已实现功能清单"和"踩坑清单"
2. `.ai/prompts/` — 如果有新的有用 prompt 模板，添加到此处
3. `CONTRIBUTING.md` — 如果有新的开发规范或约定

---

*本文件由 WorkBuddy 生成，最后更新：2026-05-26*
