# 协作 Web UI：手机宽度与逐页验证（验证记录，2026-11-09）

> 基线 main `04433f8`。协作 Web UI 的各页此前只在 SQLite + 内置浏览器里手工点过，没有截图，也没有看过手机宽度（375px）。健康基线页最近后端字段有变（`baseline.taken_at` 空仓库建基线后也应是时间字符串）。

## 一、怎么验的

用一个**丢弃式实例**（`ironforge serve` + 临时 SQLite + 主机执行 CI 关掉，只用协作数据），另起 `vite dev`，用 Playwright 驱动本地 Chrome：

- 每页在 **1440** 与 **375** 两个宽度各开一次，等 `networkidle`；
- 判据是**文档本身**有没有横向滚动（`documentElement.scrollWidth > innerWidth`），并单独统计「超出视口但在 `overflow-x: auto/scroll` 容器里」的元素 —— 表格放在可横滑容器里是设计，不算缺陷；
- 每页两种宽度各截一张图；
- 登录走 API（令牌是 HttpOnly cookie），不走登录表单，避免把 UI 状态混进检查。

脚本已放进仓库：`scripts/ui-width-audit.mjs`（不加入构建/CI，需要 Playwright，用法见文件头），这次就是用它跑的。

数据用一次性脚本造：1 个仓库、3 个任务（长标题 + 长描述 + 路径声明 + 分支）、两条消息（一条 note、一条 blocker）、一次健康基线。

## 二、逐页结果

| 页面 | 1440 | 375 | 备注 |
|---|---|---|---|
| 协作总览（任务列表 + 等待你处理 + 概览） | ok | **ok** | 宽表格在自己容器里横滑（11 个元素在 scroller 内），页面本身不横滚 |
| 任务详情（长标题 / 消息 / 依赖 / 决定 / 交接） | ok | **ok** | 长标题在 375 下正常折行，徽标不挤 |
| 协作健康（指标 + 基线） | ok | **ok** | 见下「基线状态」 |
| 协作组列表 | ok | **ok** | — |
| 仓库规则 | ok | **ok** | — |
| 协作搜索 | ok | **ok** | — |
| 协作设置 | ok | **ok** | — |
| 任务模板 | ok | **ok** | — |

截图（375，已提交，共 236 KB）：

- `ironforge-docs/architecture/screenshots/overview-375.png`
- `ironforge-docs/architecture/screenshots/task-375.png`
- `ironforge-docs/architecture/screenshots/health-375.png`

1440 的截图和其余页面留在 `/tmp` 里，没有提交（体积考虑）；重跑脚本会在 `OUT_DIR` 里重新生成全套。

### 交互（375 下）

| 动作 | 结果 |
|---|---|
| 点「健康指标」标签 | 跳到 `/collaboration/health` |
| 点任务表格里的一行 | 跳到 `/collaboration/tasks/<id>`（长 uuid 的链接没有撑破布局） |
| 量表格容器 | `overflow-x: auto`，`wrap.scrollWidth > wrap.clientWidth`（是它在滚，不是页面） |

### 健康基线（本次要确认的字段变化）

- 建基线前：`baseline.taken_at = null`，页面显示「观察基线 尚未建立」；
- 建基线后（`POST …/collaboration/health/baseline` 返回 200，`existed_before: false`、`added` 全 0）：`baseline.taken_at = 2026-11-09T14:58:52.333Z`，`coverage_start` 有值 → **空仓库建基线后也是时间字符串，符合 merged 的修复**。

## 三、修了什么

**零改动。** 8 个页面 × 2 个宽度没有横向滚动、没有溢出到页面、按钮与链接可点、i18n 无缺 key。所以没有「每个修复配单测」这一步 —— 这一节是空的，不是漏了。

有一处**刻意不改**：协作总览的任务表在 375 下宽 415px（视口 375），靠 `.table-wrap { overflow-x: auto }` 横滑。改成手机上的卡片布局是结构性重写，超出「只修问题、不重写页面结构」的范围，留作后续产品决定。

## 四、检查与构建

| 检查 | 结果 |
|---|---|
| `node scripts/i18n-key-check.mjs` | ok：2205 个静态 key、37 个动态前缀，en/zh-CN 各 2071 条（两边等长） |
| `pnpm run test`（vitest） | 通过（exit 0，约 8 秒） |
| `pnpm run check`（svelte-check） | 0 errors，48 warnings（与本次无关的既有告警） |
| `pnpm run build` | 通过，`adapter-static` 写出 `web/build` |

## 五、踩到的一个坑（值得记住）

本机 **8080 端口是你的 `ironforge-local` Docker 容器占着的**，而且那个镜像比 main 旧：里面没有 `collab_health_baselines` 表。我第一次把实例起在 8080 上，实际打的全是那个旧容器 —— 于是「建了基线却仍显示未建立」，看起来像回归，其实是**旧镜像**。

换成自起实例（8099 + 临时 SQLite + `IRONFORGE_JWT_SECRET`）并让 `vite` 的代理指过去之后，`taken_at` 正常。**结论：本机联调别直接信 8080，先确认那个容器的镜像版本。**

## 六、未覆盖

- **只有一个数据规模**：3 个任务、2 条消息；没有造几十行任务或超长分支名/会话名，那种情况下表格与徽标的表现没验。
- **没有真机**：375 是 Playwright 的视口，不是 Safari/iOS 的真实渲染（没有 `-webkit-` 相关差异的验证）。
- **没有看深色/浅色两套主题**：只在默认主题下看了一遍。
- **没有测「等待你处理」的就地回答表单提交**、依赖编辑、决定创建这些写操作在手机宽度下的交互（只验了可达性与布局）。
- **没有做无障碍检查**（对比度、焦点顺序、读屏）。
- **没有跑 E2E 截图对比**（像素级基线），所以「没有回归」只覆盖布局溢出这一类。
