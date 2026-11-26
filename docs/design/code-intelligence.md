# 代码智能：仓库地图与符号索引

> 状态（0.5.1）：已实现。事实来源：`crates/rg-core/src/search/`、`crates/rg-http/src/api/ai.rs`、`crates/rg-db/src/entities/symbol_*.rs`。
> 设计结论见 git 历史中的符号图设计文档；这里只写当前行为。

## 1. 提供的接口

| 接口 | 用途 |
|---|---|
| `GET /ai/repos/{owner}/{repo}/summary` | 仓库摘要（面向 Agent 的精简元数据） |
| `GET /ai/repos/{owner}/{repo}/tree` | 文件树 |
| `GET /ai/repos/{owner}/{repo}/map` | 仓库地图：按 token 预算裁剪的结构化概览 |
| `GET /ai/repos/{owner}/{repo}/search/code` | 代码搜索 |
| `POST /ai/repos/{owner}/{repo}/index` | 为指定 commit 建立/更新符号索引 |
| `GET /ai/repos/{owner}/{repo}/symbols` | 查询符号定义（按 `name`、`path` 或两者） |
| `GET /ai/repos/{owner}/{repo}/pulls/{number}/context` | PR 上下文包（按预算裁剪，见 §4） |

MCP 侧对应 `repo_map`、`find_definition`、`get_pr_context` 等工具，参数与 REST 一致。

## 2. 符号索引

- 只存**定义**，不存逐次引用：`symbol_definitions` 每个 `(repo, commit)` 一份，`symbol_commits` 记录已索引的 commit。
- 索引是**按 commit** 的：查询必须命中已索引的 commit，否则返回 `SYMBOLS_NOT_INDEXED` 并提示先索引，绝不用别的 commit 静默回答。
- 查询结果带 `confidence`：
  - `exact`：名称完全相等，或按 `path` 列出该文件全部定义；
  - `name_match`：仅前缀匹配。
- `partial` 为 true 时 `limitations` 列出静态解析看不到的东西：Svelte 标记、Rust 宏与 `dyn Trait`、TypeScript `obj[key]` / `any`、Python `getattr`、Go 接口调用等。**查不到不等于不存在。**
- Rust `impl` 行的 `name` 是源码首行而不是标识符，查询侧按 `kind = 'impl'` 排除。
- 不持久化引用图；引用侧按需计算或使用文件级 postings（设计结论：逐 commit 全量引用会到 79.8 MiB/commit 量级，超出预算）。

## 3. 仓库地图与预算

`/map` 按调用方的 token 预算裁剪输出：优先保留入口、顶层结构与高价值文件，明确标注被裁掉的部分与计数。裁剪不改变事实，只改变返回量。

## 4. PR 上下文包

`/pulls/{number}/context` 一次返回接手 PR 所需的信息：元数据、head commit 的最新 CI 结果（含每个失败 job 的摘要）、未解决优先的 review threads、变更文件、描述与评审。裁剪优先级：CI 失败 → 未解决评论 → 变更文件 → 描述与评审 → 已解决评论；`truncated` 说明裁掉了什么，`counts` 始终给全量数字。用户文本标 `origin: user_content`。是否可合并与分支保护结果不在包内（见 `not_evaluated`）。

## 5. 不变量

1. 符号索引按 commit 隔离，跨 commit 不混用。
2. 索引只证明「静态解析看到了这个定义」，不证明引用完整或代码可编译。
3. 所有裁剪都带 `truncated` / `counts`，不能把裁剪后的结果伪装成完整。
4. 用户文本只作数据，不作为指令执行。
