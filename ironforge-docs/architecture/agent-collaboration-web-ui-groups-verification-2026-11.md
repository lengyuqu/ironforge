# 协作 Web UI：协作组（跨仓）与健康指标页面——实现与验证记录

> 日期：2026-11-06。范围：仓库「协作」标签下新增两个页面——`collaboration/groups`（含 `groups/[id]`）与 `collaboration/health`。后端契约见
> [agent-collaboration-groups-verification-2026-11.md](agent-collaboration-groups-verification-2026-11.md) 与
> [agent-collaboration-health-verification-2026-11.md](agent-collaboration-health-verification-2026-11.md)；首版 UI 约定见
> [agent-collaboration-web-ui-verification-2026-11.md](agent-collaboration-web-ui-verification-2026-11.md)。
> 按证据分层逐层陈述，前一层不能替代后一层；没有做的写在「未验证」。

## 1. 实现

| 文件 | 内容 |
|---|---|
| `web/src/lib/api/collabGroups.ts`、`collabHealth.ts` | 只含登录身份可调用的端点（不发会话头）；路径段已编码 |
| `web/src/lib/utils/collabGroups.ts`、`collabHealth.ts` | 输入规则（`owner/name`、完整提交 id、标题/描述上限）、浏览器本地“打开过的组”、时长格式化、秒字段识别、服务端说明句的本地化映射 |
| `web/src/lib/components/collaboration/{GroupCreateForm,GroupAddLinkForm,HealthDrill}.svelte` | 创建组、添加关联、健康钻取 |
| `web/src/routes/[owner]/[repo]/collaboration/groups/{+page,[id]/+page}.svelte`、`health/+page.svelte` | 三个页面 |
| `web/src/lib/i18n/translations/{en,zh-CN}.json` | 仅新增顶层键 `collab_groups`、`collab_health`（两种语言键集一致） |

### 与任务描述的差异（后端契约决定，不是遗漏）

后端**没有**“列出锚仓所有组”的接口，**也没有**组的重命名 / 软移除 / 恢复接口（groups 文档 §1 已写明“丢了组 id 无法重新发现”）。因此：

- 组页面不是“分页列表”，而是：创建、按组 id 打开、以及“本浏览器创建/打开过的组”（`localStorage`，每次读写都有 try/catch，损坏数据被忽略；页面文字明说这不是全部组的列表）。
- 组详情只有“关联”可改（添加/移除）；没有重命名/移除组/恢复按钮——API 不存在，UI 不伪造。
- “版本 CAS”落在关联的添加/移除上：请求带上读到的 `version`；`VERSION_CONFLICT`（及移除时的 `LINK_ALREADY_REMOVED`）后自动重新加载并提示。

### 行为约定

- **隐藏项零提示**：页面不显示关联总数/计数徽标，空列表文案“没有可显示的关联”对“组本来为空”与“全部不可见”相同；文案目录有测试扫描（中英文），禁止出现隐藏/过滤/总数一类措辞。
- **逐页重新鉴权**：页面说明每页都按当前权限检查、一页可能比请求的短而后面仍有内容；“加载更多”按 `next_cursor` 追加并按 id 去重；显示 `as_of`。
- 只读者拿不到 `version`：页面不显示写入控件，只显示“只读”说明。
- 目标仓库按 `owner/name` 输入，经既有 `GET /repos/{owner}/{name}` 解析为 id；404/403 统一提示“没有找到该仓库，或你无权查看”，不区分不存在与不可见。
- 所有客户端写入的文字（组标题/描述、关联的任务标题与状态、提交、仓库名、健康钻取里的任务标题）都按纯文本渲染；任务状态标为自报。
- 每次写入带新的幂等键；失败后的重试使用**新**键（创建组、添加关联、移除关联均如此）。
- 健康页：覆盖起点之前不是 0——`unknown` 指标无数值行（显示“未知”和原因），`partial` 指标每个数值前加 `≥`，且 partial 的 0 显示为“暂无记录”而不是 0；任务状态来源标为“自报状态”，时间戳来源为“平台事件”；服务端的英文说明句有本地化映射，未知句原样显示；钻取按任务 id 排序并如此说明，页面没有任何排名/按会话视图；非管理员（403）显示“仅管理员可见”，401 显示需要登录；建立基线前有确认对话框，说明它写入什么、不可撤销、只应执行一次，成功后显示服务端返回的新增数量与说明。

## 2. 验证

| 层级 | 结论 | 证据（本分支，2026-11-06，`web/` 下） |
|---|---|---|
| 实现证据 | ✅ | 见上表，已提交到本分支 |
| 任务验证 | ✅ | `pnpm run check`：862 文件，0 错误（45 条既有警告，无一来自新文件）；`pnpm run test`：153 个文件 / 805 个用例通过；`pnpm run build` 成功；`node ../scripts/i18n-key-check.mjs`：ok（en = zh-CN = 1647 键） |
| 运行验证（内置浏览器，真实服务 + 真实 SQLite + 真实 Git 仓库） | ✅ 见下 | 预编译 `target/debug/ironforge`；`scripts/collab-e2e.py --scenario p1 --keep` 造数据，另建第二仓 `other`、第三方用户的私有仓 `secret`（授予 `collab-e2e` 写权限）、只读协作者 `ui-viewer`、写协作者 `ui-writer`、一个仅用于本测试的会话与两个 `blocked` 任务 |
| 远端 / 集成 / 发布 | ❌ | 未推送、未合并、未跑 CI（任务要求） |

### 运行验证中实际观察到的

浏览器面板处于隐藏状态，**没有截图**；全部通过 `javascript_tool` 读取 DOM 文字、触发点击并读取结果。

- 组：以含 `<img onerror>` 与 `<script>` 的标题/描述创建 → 跳转到详情页，文字原样显示，`window.__pwned` 未被设置，页面没有 `<img>`。向 `collab-e2e/other` 与 `ui-stranger/secret` 各添加一条提交关联（均成功，版本 1→3）；向不存在的 `nobody/nothing` 添加 → “没有找到该仓库，或你无权查看”。
- **隐藏目标**：第三方撤销 `collab-e2e` 对 `secret` 的权限后刷新 → 该关联静默消失，页面文字中不含 `secret`、隐藏、总数等字样；版本仍显示（写者本来就能看到）。
- **版本冲突**：另一个客户端先添加了一条关联（版本 3→4），本页仍持 3 时点击移除并确认 → 页面提示“该组在你加载之后发生了变化，已重新加载”，版本更新为 4，列表按服务器重新加载。随后的正常移除成功（版本 5，列表只剩一条）。
- **只读者**（`ui-viewer`，只读协作者，且看不到 `other`）：组详情页没有版本、没有移除按钮、没有添加表单，显示只读说明；该组实际有一条指向 `other` 的关联，页面只显示“没有可显示的关联”。
- 健康（管理员）：覆盖起点显示为实例最早事件时间；窗口早于它，顶部提示“部分数字不完整或未知”；两个 `blocked` 任务 → “窗口结束时阻塞 ≥ 2”；无数据的数值显示“暂无记录”；`stale_path_claims` 显示“未知 — 声明时间未被记录”且无数值；钻取“结束时阻塞的任务”列出两个任务（含 `<img onerror>` 标题，按文本显示，状态为“已阻塞”并带自报提示）；无数据的钻取显示“该数字下没有任务”。
- 健康（非管理员，`ui-viewer`）：显示“Admins only / 仅管理员可见”，没有任何指标，没有基线按钮。
- 基线：点击 → 确认对话框（说明写入内容、不可撤销）→ 确认 → 显示“基线已于 … 记录，新增：0 个任务，0 个交接，0 个问题”及服务端说明。
- 中英文：zh-CN 与 English 下两个页面文字均核对过（含服务端说明句的本地化）。

## 3. 未验证 / 已知限制

1. **没有截图**（面板隐藏），布局/样式/窄屏只靠代码审阅，未做视觉检查。
2. **分页的真实场景未跑**：没有造出 >50 条关联，“加载更多 / 一页偏短但仍有游标”只在单元测试里用 mock 验证；服务端对该语义的覆盖见 groups 文档 M04。
3. **基线只验证了“新增 0”的路径**：此数据里没有无事件的对象。`POST baseline` 在新增 0 时，随后的 `baseline.taken_at` 仍为 `null`（页面显示“尚未建立”，但同页上方的报告说已记录）——这是后端行为，UI 如实显示两者；是否应在新增 0 时也写入基线时间是后端待决项，未改动。
4. 健康钻取的 `basis` 字段名（如 `blocked_since`）按服务端原样显示，没有本地化。
5. 仅 SQLite；PostgreSQL / MySQL 与多实例未测。
6. 任务链接的添加/移除（`target_task_id`）只在单元测试中验证请求内容；真实运行只用了提交关联（创建任务需要会话凭据，不属于本页面）。

## 4. 给后续工作的环境提示（非产品缺陷）

内置浏览器的 `127.0.0.1` 上，不同端口的服务**共享同一份 Cookie**（Cookie 不区分端口）。多个会话各自用相同的 JWT 密钥起服务并登录时，浏览器里的登录身份会被别的会话改写（本次实测：身份中途从 `collab-e2e` 变成 `ui-stranger`，同一时段出现过两次短暂的 403“write access denied”，随后恢复；原因是推断而非证实）。改用 `http://localhost:<port>` 访问并显式登录可避免。
