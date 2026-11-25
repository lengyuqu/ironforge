# T36 执行计划 — CNB 导入真机验证

> 权威约束见 `tasks/T36.md`。基线 `64861cf9`（main），分支 `agent/t36-cnb-live-verify`，worktree `../ironforge-t36`。
> 本文件记录：核对基线、字段级对照全文、发现的缺陷、`supports_metadata` 的最终结论。

## 1. 问题

`run_cnb_import` 的注释写着 CNB 的 PR「have not been verified against a live instance」。
前两次同类任务（`a4c47b81` GitLab、`d7a23c39` Gitea）各炸出 4 个缺陷，共同点是
**统计数字对、数据写错表或写错值**。因此这次不以「导入成功」为判据，而以
**落库行与 CNB 侧真实数据逐字段相等** 为判据。

## 2. 核对基线

工具：本机 `cnb` CLI 1.18.48 + 直接 `curl https://api.cnb.cool`（token 从 `~/.cnb/token`
的 `auths["cnb.cool"].access_token` 解析，全程不落盘）。

| 仓库 | issues | issue 评论 | PR | PR 评论 | 正式评审 | labels | HEAD |
|---|---|---|---|---|---|---|---|
| `lengyuqu/ironforge` | 32（1 open / 31 closed） | 206 | 34（全部 merged） | 26 | 0 | 0 | `64861cf9` |
| `zdzso/agent2ssh` | 32（全部 closed） | 442 | 10（9 merged / 1 closed） | 26 | **2**（#3 两次 `approved`） | 0 | — |
| `cnb/feedback` | （仅用于标签形状） | — | — | — | — | **33** | — |

`lengyuqu/ironforge` 的 issue 号不连续（11–65），全部保留原编号；PR 号 1–66 不连续，同样保留。
`zdzso/agent2ssh` 用于验证「正式评审」这条路径——ironforge 的 34 个 PR 的 `review_count` 都是 0。

认证：`Authorization: Bearer <access_token>`；**必须同时带
`Accept: application/vnd.cnb.api+json`**，否则返回
`406 {"errmsg":"either of 'application/json' or 'application/vnd.cnb.api+json' content type supported"}`。
代码原本就带了这两个头，实测正确。

## 3. 字段级对照（代码期望 vs 真实返回）

### 3.1 Issue 列表 `GET /{owner}/{repo}/-/issues`

| 字段/行为 | 代码期望 | 真实返回 | 结论 |
|---|---|---|---|
| 响应体 | 裸数组 | `[{...}, ...]` | ✅ |
| `state` 过滤 | `state=all` | `state=all` → **400 `{"errcode":400,"errmsg":"bad request"}`**；只接受 `open`/`closed`；不带参数默认 `open`（`x-cnb-total: 1`） | ❌ 缺陷 1 |
| `number` | 字符串 | `"65"` | ✅ |
| `body` | 列表无、详情有 | 列表 JSON 无 `body` key；`/issues/64` 有 `"body": "## 两个测试在 PostgreSQL 上失败..."` | ✅ |
| `state` | `open`/`closed` | `"open"`、`"closed"`（`state_reason` 另有 `completed` 等） | ✅ |
| `closed_at` | 空字符串表示未关闭 | open 的 #65 是 `""`；closed 的 #64 是 `"2026-11-18T06:14:58Z"` | ✅ |
| `labels` | 对象或字符串 | 对象数组：`[{"id":"1838192555308494848","name":"feature request","description":"","color":"#E8FCF4"}]` | ✅ 形状；❌ 颜色/描述被丢弃（缺陷 4） |
| `author` | `{username,...}` | `{"username":"cnb.lengyuqu","nickname":"太难了","is_npc":false}` | ✅ 形状；❌ 作者归属（缺陷 7） |
| `created_at`/`updated_at` | RFC3339 | `"2026-11-24T15:04:04Z"` | ✅ `parse_datetime_or_now` 可解析 |
| `comment_count` | 数字 | `1` | ✅ |
| 分页头 | `x-cnb-total` | `x-cnb-page: 1`、`x-cnb-page-size: 100`、`x-cnb-total: 1`（open）/`31`（closed） | ✅ 与 `total`/`totalPages` 一致 |

### 3.2 Issue 详情 / 评论

| 字段/行为 | 代码期望 | 真实返回 | 结论 |
|---|---|---|---|
| 详情 `body` | 字符串 | 1278 字符（#65）、2095 字符（#64） | ✅ |
| 评论 `id` | 字符串 | `"2122936580833869824"`（< 2^63，`parse::<i64>` 成功） | ✅ |
| 评论 `body`/`author`/`created_at` | 有 | `{"id":"...","body":"## 已解决...","author":{"username":"..."},"created_at":"2026-11-24T15:17:51Z","updated_at":"..."}` | ✅ |
| 评论分页默认 | 100 | 默认 `x-cnb-page-size: 10`；显式 `page_size=100` 后为 100 | ✅ 代码传了 100 |
| 评论总数 | 按 issue 汇总 | ironforge 206、agent2ssh 442 | ✅ |
| 评论失败处理 | `unwrap_or_default()` | 429 时静默变 0 条 | ❌ 缺陷 6 |

### 3.3 Pull 列表 / 详情

| 字段/行为 | 代码期望 | 真实返回 | 结论 |
|---|---|---|---|
| `state=all` | 可用 | `200`，`x-cnb-total: 34` | ✅（与 issues 相反） |
| `body` | 列表有正文 | **列表恒为 `"body": ""`**；`/pulls/66` 详情为 918 字符 | ❌ 缺陷 2 |
| 详情字段集 | 与列表同构 | 详情**没有** `created_at`/`updated_at`/`comment_count`，多 `reviewers`/`statuses` | ❌ 只能取 body，不能整行替换 |
| `number` | 字符串 | `"66"` | ✅ |
| `state` + `is_merged` | `closed`+`is_merged:true` 表示已合并 | 34 个全部 `"state":"closed"`、`"is_merged":true`、`"mergeable_state":"merged"` | ✅ 映射为 `merged` |
| `is_wip` | draft | `false` | ✅ |
| `head.ref`/`base.ref` | 全 ref | `"refs/heads/auto/jwt-cve-25537-9fce"` / `"refs/heads/main"` | ✅ 去掉前缀后落库为短名 |
| `head.sha` | sha | `"1160da2f66e72cb695bd25d50a5034eeb8d7f88e"` | ✅ |
| `head.repo` | 无 | 嵌套 `{"id":"...","name":"ironforge","path":"lengyuqu/ironforge",...}` | ✅ serde 忽略未知字段 |
| `labels` | 对象/字符串 | 对象数组 | ✅ |
| `review_count` | 有 | 有（agent2ssh#3 = 2） | ✅ 用于发现缺陷 8 |
| 正式评审 | 「无列表端点」 | **`GET /pulls/3/reviews` → `[{"state":"approved","body":"","author":{...},"created_at":"2026-11-10T18:00:55Z"}]`** | ❌ 缺陷 8 |

### 3.4 Pull 评论 / 评审

| 字段/行为 | 代码期望 | 真实返回 | 结论 |
|---|---|---|---|
| 评论端点 | `/pulls/{n}/comments` | 200，形状同 issue 评论（id/body/author/created_at） | ✅ |
| 评论落库位置 | 共享导入器 `comments` 参数 → `issue_comments` | `issue_id` 写的是 **PR id** | ❌ 缺陷 3（孤儿） |
| 评审 `state` | GitHub 大写 | 小写 `"approved"` | ❌ 需映射（缺陷 8） |
| 评审 `body` | 可空 | `""` | ✅ |
| 评审时间 | `submitted_at` | 字段名是 `created_at` | ❌ 需转换 |

### 3.5 仓库标签 `GET /-/labels`

| 字段/行为 | 代码期望 | 真实返回 | 结论 |
|---|---|---|---|
| 仓库级标签端点 | 注释称不存在；`/issues/labels` 404 | `/-/labels` → **200**（`lengyuqu/ironforge` 为 `[]`，`cnb/feedback` 33 个）；`/-/issues/labels` 确实 404 | ❌ 注释误导，缺陷 4 |
| 标签形状 | 只有 name | `{"id":"1838192555308494848","name":"feature request","description":"","color":"#E8FCF4"}` | ✅ 解析；❌ 颜色/描述未使用 |
| 未被 issue 引用的标签 | 由 issue 汇总即可 | `cnb/feedback` 33 个标签中，首屏 30 个 open issue 之外有 **22 个**未被引用 | ❌ 会丢 |
| 权限 | — | `repo-notes:r` | 记录 |

### 3.6 限流

| 行为 | 代码期望 | 真实返回 | 结论 |
|---|---|---|---|
| 突发请求 | 无特别处理 | ~132 次请求 / 30 秒触发 `429 {"errcode":8,"errmsg":"Too many requests. Please try again later."}` | ❌ 缺陷 6 |
| 单次导入 | — | 1 次完整导入（issues+PR+每条评论+每条评审）约 166 次请求 | 需退避 |
| 两次并发 | — | 6 次重试仍不够；提到 8 次（约 91 秒）后，顺序双导入 0 次 429 | ✅ |

## 4. 发现的缺陷（8 个，全部修复）

| # | 真实返回 | 代码假设 | 修复 |
|---|---|---|---|
| 1 | issues `state=all` → 400；不带 state 默认只回 open（1/32） | issues 与 pulls 一样接受 `all` | 分别拉 `open` + `closed`，按 number 去重 |
| 2 | PR 列表 `body` 恒为 `""`，正文在 `/pulls/{n}` | 列表有完整正文 | 正文为空时取详情，只覆盖 body（详情无时间字段） |
| 3 | PR 评论经共享导入器写入 `issue_comments.issue_id = PR id`，LEFT JOIN issues 全部孤儿（26 条） | 评论与 issue 同表即可读 | CNB 段把评论映射为 `COMMENTED` 评审，落 `pr_reviews`（`action=comment`），不改共享函数 |
| 4 | `/-/labels` 200 且分页，含 color/description；22/33 标签未被 issue 引用 | 无仓库级标签端点，只能从 issue 合成灰色标签 | 仓库级拉取 + issue 标签合并去重，对象标签走 `import_github_labels` 保留颜色/描述 |
| 5 | CNB 无 milestone；releases/wiki 无导入路径 | `supports_metadata=true` 即六个 flag 全收，任务 `completed` 但没导 | CNB 的 milestone/release/wiki 在写任务行前清零并记日志 |
| 6 | 429 `errcode 8`；评论调用 `unwrap_or_default()` 静默变空 | 请求不会失败 / 失败可忽略 | `get_json` 对 429/500/502/503/504 指数退避（8 次，最多约 91 秒，尊重 `Retry-After`）；评论失败显式 warn |
| 7 | 空 user map → 作者 fallback 到 **user id 1**（管理员/首个注册账号）；以 user 2 导入仍写 author_id=1 | fallback 到 1 可接受 | 收集 issue/PR/评论/评审作者，走与 GitHub/GitLab 相同的 `map_users` → 归到导入者 |
| 8 | `/pulls/{n}/reviews` 200，`[{"state":"approved"}]` | 「没有 review-list 端点，导评审就是猜」 | 拉取正式评审，`approved`→`APPROVED` 等小写映射；未知状态降级为 comment |

## 5. `supports_metadata` 与 milestone 的最终结论

**PR 能导。** 实测 34/34 个 PR 落库（标题、正文、merged 状态、head/base 短分支、head sha、
评论 26 条、正式评审 2/2 全部对得上），缺陷修复后 PR 路径没有任何「未验证」保留。

`supports_metadata` 对 CNB 继续为 `true`，含义收紧为**已验证的能力集**：

- 支持：issues、labels、pull requests、issue 评论、PR 评论、PR 正式评审；
- 不支持：milestone（CNB 无此概念）、releases、wiki（导入器没有这两条路径）。

处理方式：**在 `start_import` 写任务行之前清零** 不支持的 flag（并 `tracing::info!`），
API 返回的 `import_milestones/import_releases/import_wiki` 如实为 `false`。
不选「报错」：导入对话框默认勾选这些项，报错会让任何 CNB 导入开箱即失败；清零后
任务行本身即诚实记录，调用方看到的就是将要发生的事。

## 6. 验证策略与结果摘要

- 端到端：`lengyuqu/ironforge` 全量（含 clone）→ 32 issues / 206 issue 评论 / 34 PR / 26 PR 评审 /
  0 labels；两个孤儿查询均为 0；34/34 PR 正文非空；HEAD 与源一致 `64861cf9`。
- 逐字段脚本比对：32 issues + 34 PRs，标题/状态/正文/评论/分支/时间 0 处不一致。
- 非管理员（user 2）导入：author_id 全部为 2（修复前为 1）。
- `zdzso/agent2ssh`：32 issues / 442 评论 / 10 PR / **28 pr_reviews = 26 comment + 2 approve**。
- 重复导入同一目标：新增 0 行，行数不变（32/206/34/26），无重复、任务 `completed`。
- 限流：最终代码跑 ironforge 触发 28 次 429，全部重试成功，0 次最终失败。

## 7. 边界与未覆盖项

- **labels 端到端没有真实数据**：本次可访问的仓库标签数都是 0。标签路径由
  `cnb/feedback` 的真实标签载荷 + 单测覆盖（数量合并、颜色保留、空描述转 `None`），
  未对「有标签的仓库」做完整导入，以免改动他人仓库数据。
- CNB 没有 `merged_at`，只有 `closed_at` + `merged_by`，因此 `merged_at` 落 NULL，
  `state` 正确为 `merged`。
- 用户映射与其它平台一致：所有来源作者归到导入者，不做逐用户身份匹配。
- 不做 N+1 并发化（任务明确排除）；本次只让串行路径在真实限流下正确。
