# 通知的用户侧视图设计（E3.2，2026-11-10，待批注）

> 状态：**只出方案，不写代码**。E3 的另一半（用户从模板创建任务）本次不设计 —— 它卡在「所有者会话」没有产品定义（用户不是会话，任务必须有归属），先搁置。E2（邮件通知偏好）同日决定搁置。

## 一、现状（代码事实）

| 项 | 事实 |
|---|---|
| 表 | `notifications`：`user_id`、`event_type`、`title`、`body`、`repo_id`（可空）、`is_read`、`created_at` |
| 端点 | `GET /notifications`（`unread_only` + 分页）、`GET /notifications/unread-count`、`POST /notifications/mark-all-read`、`POST /notifications/{id}/read` |
| 列表返回 | `PaginatedResponse`：`id`、`event_type`、`title`、`body`、`repo_id`、`repo`（`owner/name`，**只有当前可读时才有**）、`is_read`、`created_at` |
| 失去访问 | `event_type` 以 `collab_` 开头且仓库已不可读时，`title` 变成 `"(no longer available)"`、`body` 置空（`api/notifications.rs` 里 `is_collab_event` + `readable_repo_names`） |
| 只有铃铛 | 导航栏未读徽标 + 下拉列表（前端 `unread.svelte.ts`），**没有独立页面**，没有按仓库/类型筛选 |
| 无关的东西 | 协作区的「已读」是**另一套**：`PUT …/collaboration/inbox/user/read` 按「已读游标」记录每个任务的读到哪一条消息，和通知的 `is_read` 不是一回事 |

## 二、页面放哪

新增顶层路由 `/notifications`（登录后可见），导航栏铃铛的「查看全部」跳过去；下拉列表保留（短平快），两者共用同一个 API。

不放进「仓库设置」或「用户设置」：通知是跨仓库的，而协作区已经有自己的页面，塞进去会让「协作」与「通知」的边界更糊。

## 三、API 需要的改动（最小集）

| 改动 | 为什么 |
|---|---|
| `GET /notifications` 增加 `repo_id`、`type`（前缀匹配，如 `collab_`、`pr`）、`before`（keyset 游标，`created_at`+`id`） | 现在只能 `unread_only` + offset 分页；一个通知量大的人翻到第 20 页时 offset 会抖 |
| 响应增加 `hidden: bool` | 现在「失去访问」是靠把标题替换成一句英文；UI 没法区分「这条通知就叫这个」和「它被藏了」。`hidden=true` 时前端渲染成灰色一行 + 说明，而不是假装有个标题 |
| `hidden=true` 时不返回 `repo`、`title`、`body` | 与现有规则一致，只是把规则说清楚 |

`repo` 字段的判定逻辑（逐仓库 `can_read_repo`）保持不变；只在 `hidden` 的语义上补齐。**不加**「按仓库汇总未读数」这种端点 —— 前端拿得到 `repo_id`，聚合放在前端做。

## 四、已读语义（与协作收件箱的关系）

| 动作 | 影响 |
|---|---|
| 在 `/notifications` 点一条 | `POST /notifications/{id}/read`，只改这一条的 `is_read` |
| 「全部标为已读」 | 现有的 `POST /notifications/mark-all-read`，作用于当前筛选范围（**需要新增 `repo_id`/`type` 参数**，否则「我筛出来的这些」和「全部」会打架） |
| 协作任务的消息已读 | 仍然是 `PUT …/collaboration/inbox/user/read` 的游标，**不联动**。任务消息不是通知；把两者耦合会让「我在浏览器里点掉一条通知」变成「我在 agent 那边少看到一条消息」 |

这一点要在 UI 上写明一句（通知页底部一句小字），否则两个「已读」会长期被误解。

## 五、失去访问后的展示

沿用现在的规则，只是显式化：

- `collab_*` 且仓库不可读 → `hidden: true`，显示「该内容已不可见」+ 时间 + 类型徽标；不显示标题、正文、仓库名。
- 非 `collab_*` 的通知：保持现状（标题里有仓库名是常见情况，但那本来就是可读时才生成的）。**待你确认**：仓库转为私有后，普通 `push`/`issue` 通知的标题是否也要藏？现在的实现只藏 `collab_*`。

## 六、前端信息架构

```
/notifications
├── 顶部：标题 + 「全部标为已读」
├── 筛选条：仓库（下拉，只列出现在通知里的可读仓库）、类型（全部 / 协作 / 议题 / 合并请求 / 流水线 / 其他）、只看未读
├── 列表：按日期分组（今天 / 昨天 / 更早），每条 = 类型徽标 + 标题 + 仓库 + 相对时间，未读左侧一条竖线
└── 底部：加载更多（keyset 游标），空态区分「没有通知」和「筛选后为空」
```

- 375px：筛选条折成一行可横滑的 chip；标题两行截断；这是照着 C1 已经验过的模式来（`collab` 页面的筛选条同样处理）。
- i18n：新增 `notifications.*` 一组 key（标题、筛选、空态、隐藏说明、已读提示），中英各一份，走现有 `i18n-key-check`。

## 七、不做（明确划出边界）

- 不发邮件（E2 搁置）、不做浏览器推送、不做聚合去重（「3 个仓库共 12 条 push」）、不做按仓库的未读徽标。
- 不改通知的**写入**侧（谁该收到）——那是协作/仓库 watch 的既有逻辑。
- 不做「通知详情页」：正文最多两行，展开在列表内完成。

## 八、测试与证据计划

| 层级 | 内容 |
|---|---|
| 后端 | 新查询参数的集成测试（`repo_id`/`type`/`before`/`hidden`），PG/MySQL 各一遍（进 `ci-db-tests.sh` agent 列表） |
| 隐私 | 仓库转私有后，`collab_*` 通知必须 `hidden=true` 且无标题正文（沿用现有断言） |
| 前端 | 列表渲染 + 筛选 + 已读的 vitest；375/1440 宽度用 `scripts/ui-width-audit.mjs` 加一条页面 |
| 文档 | 验证记录 + `CLAUDE.md` 的协作通知段落补一句「用户侧视图」 |

## 九、待你批注的点

1. 页面放 `/notifications` 顶层，可以吗？
2. 「全部标为已读」按当前筛选范围生效（而不是永远全部），同意吗？
3. 非 `collab_*` 的通知在仓库转私有后要不要也藏标题？（现在的实现只藏协作类）
4. keyset 游标要现在就做，还是先用 offset 分页、量大再换？
