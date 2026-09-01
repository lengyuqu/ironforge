# 前端大组件拆分重构记录

> 系列起点 `7b6880b`（refactor(web): split components, unify auth, fix Svelte 5 runes，2026-09-01）
> 本文档整理该系列中页面拆分相关的 13 个提交，覆盖 18 个路由页面、56 个新组件。

## 一、总体成效

| 指标 | 数值 |
|------|------|
| 拆分页面数 | 18 |
| 新增组件数 | 56（分布在 13 个域目录） |
| 新增共享工具 | `utils/repoUrls.ts`、`utils/pipelineStatus.ts`、`utils/commitStatus.ts` |
| 页面代码量 | 约 10090 行 → 约 3003 行（编排层） |
| 净变化 | +10329 / -7790 行（含组件新增与页面删除） |
| 验证基线 | 每轮 svelte-check 0 errors、vitest 13/13、vite build 通过 |

### 拆分前后页面行数对比

| 页面 | 拆分前 | 拆分后 |
|------|-------|-------|
| PR 详情页（pulls/[number]） | 873 | 269 |
| 首页（+page） | 723 | 131 |
| 搜索页（search） | 702 | 269 |
| 流水线页（pipelines） | 682 | 277 |
| 标签设置页（settings/labels） | 631 | 138 |
| 仓库首页（[owner]/[repo]） | 629 | 174 |
| 文件查看页（blob） | 608 | 131 |
| Issue 详情页（issues/[number]） | 544 | 264 |
| 实例设置页（admin/settings） | 542 | 95 |
| 分支保护页（settings/branches） | 497 | 126 |
| 发行版页（releases） | 497 | 158 |
| 提交状态页（commits/[sha]） | 494 | 132 |
| 审计日志页（admin/audit） | 468 | 120 |
| 导入页（imports） | 464 | 128 |
| Wiki 详情页（wiki/[title]） | 443 | 220 |
| 新建发行版页（releases/new） | 438 | 102 |
| 仪表盘页（dashboard） | 436 | 118 |
| 安全设置页（settings/security） | 416 | 151 |

## 二、提交清单

| 提交 | 主题 | 范围 |
|------|------|------|
| `61dffab` | 提取页面逻辑为独立组件 | home / repo（第一批）/ pulls / pipelines / labels / search 六页，39 文件 +4466/-3323 |
| `5980222` | 拆分 blob 查看页（repo/ 第二批） | 4 组件 + 类型化，8 文件 +680/-526 |
| `0c622e8` | 拆分 issue 详情页（issues/） | 3 组件 + Issue/IssueComment 类型重写，6 文件 +475/-343 |
| `4dc5641` | 拆分 admin 实例设置页（admin/） | 3 个自包含 section，4 文件 +716/-473 |
| `dbf6353` | 拆分 settings/branches 分支保护页（settings/） | 2 组件，3 文件 +521/-420 |
| `98237f2` | 拆分 releases 发行版页（releases/） | 2 组件 + Release 类型重写，5 文件 +456/-383 |
| `deb2ddc` | 拆分 commit 状态页（repo/ 第三批） | 2 组件 + commitStatus 工具 + CommitStatus/CombinedCommitStatus 类型，6 文件 +499/-390 |
| `bc3e314` | 拆分 admin/audit 审计日志页（admin/ 第二批） | 3 组件，4 文件 +477/-358 |
| `b2b55ce` | 拆分 imports 导入页（imports/） | 2 组件，3 文件 +388/-339 |
| `af352d1` | 拆分 wiki 详情页（wiki/） | 3 组件 + Wiki 类型重写 + api/wiki.ts 全类型化，6 文件 +377/-248 |
| `b691358` | 拆分 releases/new 创建页（releases/ 第二批） | 1 组件，2 文件 +399/-356 |
| `7b5eb4f` | 拆分 dashboard 仪表盘页（dashboard/） | 2 组件，3 文件 +395/-338 |
| `c42752b` | 拆分 settings/security 安全页（settings/ 第三批） | 3 组件，4 文件 +480/-293 |

（`7b6880b` 为本系列起点，确立拆分模式；`b938d31` 为配套的前端测试基建，不在本文档统计范围。）

## 三、各域组件明细

### home/（6 组件，633 行）— 提交 `61dffab`

| 组件 | 类型 | 职责 |
|------|------|------|
| HeroSection | 纯展示 | 落地页 Hero |
| FeaturesSection | 纯展示 | 六特性卡（数组化） |
| StatsSection | 纯展示 | 四格数据带 |
| PublicReposSection | 自包含 | 公开仓库列表（含 retry） |
| SiteFooter | 纯展示 | 页脚 |
| DashboardRepoGrid | 纯展示 | 登录态仓库网格 |

### repo/（11 组件，1531 行）— 提交 `61dffab` + `5980222` + `deb2ddc`

| 组件 | 类型 | 职责 |
|------|------|------|
| EmptyRepoGuide | 自包含 | 空仓库初始化指引（clone URL + 复制按钮 + 三步引导） |
| RepoToolbar | 纯展示 | 分支选择 Dropdown + New file + 面包屑 |
| FileTreePanel | 纯展示 | 文件树（目录点击回调、文件 blob 链接） |
| RecentCommitsPanel | 纯展示 | 最近提交列表 |
| ReadmeSection | 纯展示 | README markdown 渲染 |
| BlobBreadcrumb | 纯展示 | blob 页面包屑 |
| BlobFileHeader | 混合 | 文件头部 + 操作行（复制路径/链接自包含，视图切换/删除回调） |
| BlobDeletePanel | 自包含 | 删除确认（deleteContent API + SHA conflict 友好提示） |
| BlobContentView | 纯展示 | markdown/代码/二进制渲染 + 行号 + highlight.js 高亮 |
| CommitInfoCard | 纯展示 | 提交信息卡（标题/短 sha/作者日期/GPG badge） |
| StatusChecksPanel | 纯展示 | combined 状态 banner + 检查卡片列表（状态映射用 utils/commitStatus.ts） |

### pulls/（7 组件，1197 行）— 提交 `61dffab`

| 组件 | 类型 | 职责 |
|------|------|------|
| SuggestionBlock | 混合 | 共享代码建议块（批量勾选 + 单条应用） |
| PrReviewersBox | 自包含 | 审阅人管理 |
| PrMergeBox | 自包含 | 合并/自动合并/合并队列面板（onChanged 回调） |
| PrTimeline | 纯展示 | 时间线列表 |
| PrThreads | 混合 | 评论线程 + 建议批量应用 |
| PrDiffView | 混合 | diff 视图 + 行内评论表单 |
| PrReviewForm | 混合 | 审查提交表单 |

### pipelines/（3 组件，541 行）— 提交 `61dffab`

| 组件 | 类型 | 职责 |
|------|------|------|
| PipelineList | 纯展示 | 侧边栏流水线列表 |
| PipelineFlow | 纯展示 | 阶段-任务可视化（连接箭头 + job 卡片 + 审批按钮） |
| JobLogModal | 自包含 | 日志弹窗（WebSocket 流，initialError 保留失败态展示） |

### labels/（3 组件，531 行）— 提交 `61dffab`

| 组件 | 类型 | 职责 |
|------|------|------|
| LabelFormModal | 自包含 | 创建/编辑弹窗（预设色板 + 自定义色 + 预览） |
| LabelDeleteModal | 自包含 | 删除确认弹窗 |
| LabelGrid | 纯展示 | 标签卡片网格 |

### search/（2 组件，495 行）— 提交 `61dffab`

| 组件 | 类型 | 职责 |
|------|------|------|
| SearchBox | 纯展示 | 搜索框 + 帮助面板 + 类型 Tab |
| SearchResultsList | 纯展示 | 三种结果卡片 + 分页 |

### issues/（3 组件，396 行）— 提交 `0c622e8`

| 组件 | 类型 | 职责 |
|------|------|------|
| ReactionBar | 纯展示 | 表情反应条（8 种 emoji + 计数 + mine 高亮） |
| CommentCard | 纯展示 | issue 正文与评论共用卡片（children snippet 挂附件面板） |
| AssigneesPanel | 自包含 | 负责人面板（加载/行内编辑/保存） |

### admin/（6 组件，1146 行）— 提交 `4dc5641` + `bc3e314`

| 组件 | 类型 | 职责 |
|------|------|------|
| InstanceSettingsSection | 自包含 | 维护模式 + 实例横幅（保存后同步 banner store） |
| SsoProviderSection | 自包含 | SSO 提供商列表（启用/禁用/编辑/删除/LDAP 测试）+ 15 字段表单 |
| LoginAttemptsSection | 自包含 | 登录审计（四维过滤 + 分页） |
| AuditFilters | 混合 | action/resource 过滤下拉（bindable props + onApply/onClear 回调） |
| AuditLogTable | 纯展示 | 日志表格 + action-badge 前缀色 + 分页回调 |
| AuditDetailModal | 自包含 | 详情弹窗（挂载自取 getAuditLog，Escape/Enter/Space 关闭） |

### settings/（5 组件，924 行）— 提交 `dbf6353` + `c42752b`

| 组件 | 类型 | 职责 |
|------|------|------|
| BranchProtectionForm | 自包含 | 分支保护创建/编辑表单（JSON 数组 ↔ 逗号串解析内聚） |
| BranchProtectionList | 自包含 | 规则表格（删除 confirm + toast，编辑 onEdit 回调） |
| MfaStatusSection | 自包含 | MFA 启用状态 + 备份码统计 + 禁用表单（confirm + toast） |
| MfaSetupPanel | 自包含 | QR/secret 展示 + 验证码表单（mfa.enable + toast，onEnabled 回调传备份码） |
| BackupCodesPanel | 自包含 | 备份码网格 + 剪贴板复制 |

### releases/（3 组件，792 行）— 提交 `98237f2` + `b691358`

| 组件 | 类型 | 职责 |
|------|------|------|
| ReleaseCard | 自包含 | 发行版卡片（删除 confirm + 资产下载 + badge/relativeTime/formatBytes 内聚） |
| ReleaseList | 纯展示 | 发行版列表 + 分页回调 |
| ReleaseForm | 自包含 | 创建表单（tag 提示/目标 tag-branch 切换/draft/prerelease，提交 + toast） |

### imports/（2 组件，372 行）— 提交 `b2b55ce`

| 组件 | 类型 | 职责 |
|------|------|------|
| ImportForm | 自包含 | 导入表单（平台切换/内容选项联动/提交 + toast，成功回调刷新） |
| ImportTaskTable | 自包含 | 任务表格（platform/status badge、删除 confirm + toast、onDeleted 同步列表） |

### wiki/（3 组件，311 行）— 提交 `af352d1`

| 组件 | 类型 | 职责 |
|------|------|------|
| WikiSidebar | 纯展示 | 页面导航 + 目录 TOC（scrollToHeading 回调） |
| WikiEditPanel | 自包含 | textarea 编辑（wiki.update + toast，onSaved/onCancel 回调） |
| WikiHistoryPanel | 自包含 | 历史列表（挂载自取 history，版本展开/恢复 + confirm + toast） |

### dashboard/（2 组件，377 行）— 提交 `7b5eb4f`

| 组件 | 类型 | 职责 |
|------|------|------|
| CreateRepoForm | 自包含 | 仓库创建表单（name 正则/长度校验、模板选项自加载、提交 + toast） |
| RepoList | 纯展示 | 仓库卡片列表 + 私有 badge |

## 四、类型层改动（以后端源码为唯一事实来源）

所有类型均对照 `crates/rg-db/src/entities/` 与 `crates/rg-http/src/api/` 的实际定义重写，重写前先确认旧类型无其他使用方。

| 类型 | 动作 | 对齐依据 |
|------|------|---------|
| PullRequest / RequestedReviewer / PrTimelineEvent / ReviewComment 扩充 | 新增/扩充 | rg-db pull_request 等实体 |
| ExploreRepo | 新增 | explore 接口返回（owner_name/forks_count，无 is_private） |
| Pipeline / PipelineStage / PipelineJob / PipelineDetailResponse / PipelineDetail | 重写 | rg-db pipeline 族实体 |
| Label | 修正 | color 由 `string \| null` 改为 `string`（后端非空 String），补 repo_id/created_at/updated_at |
| RepoInfo / RepoTreeEntry / RepoCommitEntry | 新增 | rg-http repo_content.rs（TreeEntry/CommitEntry，author 为字符串） |
| BlobContent | 新增 | rg-http BlobContent struct |
| Issue / IssueComment | 重写 | rg-http IssueResponse/CommentResponse（author: Option\<String\>、labels: string[]） |
| Release | 重写 | rg-db release::Model 直序列化（无 author/assets_count 字段） |
| CommitStatus 重写 / CombinedCommitStatus 新增 | 重写/新增 | rg-http commit 状态接口（sha/context/state/description/target_url + combined 聚合） |
| WikiPage / WikiPageSummary / WikiRevision | 重写/新增 | rg-http WikiPageResponse / rg-db wiki_revision::Model（message/author_id 可空） |

API 层同步去 any：`api/labels.ts`、`api/pipelines.ts`、`api/pulls.ts`、`api/repos.ts`（get/tree/log/blob/listCommitStatuses/getCombinedStatus）、`api/issues.ts`（七个方法）、`api/releases.ts`（5 处）、`api/wiki.ts`（全方法）。`api/admin.ts`、`api/branchProtections.ts`、`api/mfa.ts`、`api/imports.ts` 原已类型化，直接复用。

## 五、拆分模式约定

1. **页面 = 编排层**：只做数据加载、URL 同步和状态切换；超过 ~300 行的页面视为拆分候选。
2. **组件两类**：
   - **自包含**（操作类）：自己调 API、自己管状态，错误走 `toast.error(toErrorMessage(e))`；
   - **纯展示**（列表/展示类）：props 进、回调出，无 API 依赖。
3. **错误反馈分层**：页面级加载失败保留 error banner；组件内操作失败用 toast；表单校验错误留在表单内。
4. **Svelte 5 runes**：`$props/$state/$derived/$effect`；组件挂载时有意捕获 props 初值的场景用常量快照再初始化 `$state`（会触发 `state_referenced_locally` warning，为已接受模式，见 JobLogModal/LabelFormModal/BranchProtectionForm）。
5. **i18n**：`createT()`，`t(key, fallback)` 重载代替 `t(key) || 'fallback'`。
6. **类型策略**：前端类型与后端不符时，以后端源码为唯一事实来源重写；重写前 grep 确认旧类型无使用方。
7. **样式**：`.btn-primary/.error-banner/.page-container` 等走 `src/lib/app.css` 全局类；组件私有样式随组件走；作用域内无法命中 `{@html}` 输出的死 CSS 不搬运。
8. **验证门槛**：每轮拆分必须通过 `svelte-check --threshold error`（0 errors）、`vitest run`、`vite build` 三项，warnings 只允许既有的 a11y 与快照初始化两类。

## 六、剩余候选拆分预估计划

P1/P2 已全部完成（2026-09-01）。实际执行与预估基本一致，仅两处修正：`settings/webhooks` 与 `settings/+page` 经核实已不存在（预估计划源自旧版页面扫描，实际 P2 拆了 3 页）。各表保留当时的预估内容作为执行记录。

### P1（已完成 ✅，提交 98237f2 → af352d1）

| 页面 | 行数 | 预估拆分 | 组件数 | 难度 | 备注 |
|------|-----|---------|-------|------|------|
| releases | 497 | ReleaseCard（纯展示，relativeTime/formatBytes 下沉 utils）、ReleaseList（自包含删除 confirm + 资产加载/下载） | 2 | 低 | Release/ReleaseAsset 类型已有，页面 loadReleaseAssets 仍用 any，需顺带类型化 |
| commits/[sha] | 493 | CommitInfoCard（纯展示）、StatusChecksPanel（纯展示，状态 icon/颜色/文案映射内聚） | 2 | 低 | 大部分是样式；getStatusIcon/Color 可下沉 utils/commitStatus.ts |
| admin/audit | 468 | AuditLogFilters（纯展示）、AuditLogTable（列表+分页）、AuditLogDetailModal（自包含详情弹窗，含键盘关闭） | 3 | 中 | 与 admin/LoginAttemptsSection 同构，可参照 |
| imports | 464 | ImportStartForm（自包含：平台切换/选项/提交）、ImportTaskTable（自包含删除+状态展示） | 2 | 低 | ImportTask 类型已存在 |
| wiki/[title] | 443 | WikiContentView（渲染+目录滚动定位）、WikiEditor（自包含保存）、WikiHistoryPanel（自包含：历史列表/查看旧版/恢复） | 3 | 中 | 历史面板与编辑器状态联动，页面需保留编排；viewRevision 用 any 需类型化 |

### P2（已完成 ✅，提交 b691358 / 7b5eb4f / c42752b；原计划 5 页，实际 3 页）

| 页面 | 行数 | 预估拆分 | 组件数 | 难度 | 备注 |
|------|-----|---------|-------|------|------|
| releases/new | 438 | ReleaseForm（自包含：tag 提示/分支与 tag 切换/表单） | 1 | 低 | 单表单页，可整体下沉 |
| settings/webhooks | 437 | WebhookForm（自包含创建，事件勾选矩阵）、WebhookList（自包含启停/删除） | 2 | 低 | 与 labels 页同构 |
| dashboard | 435 | RepoCreateForm（自包含：模板选择/表单）、DashboardRepoList（纯展示） | 2 | 低 | 模板下拉已有数据结构 |
| settings/security | 415 | 待扫（预计密码/TOTP/会话管理分 section，同 admin/settings 三段式） | 2-3 | 中 | 需先核 MFA 相关 API 类型 |
| settings/+page | 415 | 待扫（预计仓库基本信息/危险区等 section） | 2-3 | 低 | 危险区（改名/删除）需自包含确认 |

### P3（交互复杂或需预研）

| 页面 | 行数 | 预估拆分 | 组件数 | 难度 | 备注 |
|------|-----|---------|-------|------|------|
| issues/board | 419 | BoardColumn（含拖放 onDragOver/onDrop）、BoardCard（纯展示）、BoardCreateForm（自包含）；拖放状态留在页面或 BoardColumn 内聚待定 | 3 | 高 | 原生 HTML5 拖放 + 列/卡片两级 CRUD，建议先出拆分方案再动手 |
| orgs / boards / packages 其余 400 行级页面 | ~400 | 待扫 | — | — | 均低于 400 行，可按同样模式迭代 |

### 收尾状态与后续

- ✅ P1 五页、P2 三页均按“每轮 1 页 1 提交”完成，每轮均过三项验证；
- ✅ `web/scripts/_patch_*.py` 共 15 个一次性补丁脚本已清理（本为未跟踪文件，删除后无 git 痕迹）；
- ⚠️ `routes/` 内 400+ 行页面现只剩 `issues/board`（419 行）；
- ⬜ P3：先出 issues/board 拆分方案（HTML5 拖放 + 列/卡片两级 CRUD），确认后再拆分，预计 1-2 轮；
- ⬜ orgs / boards / packages 域内 ~400 行级页面可按同样模式迭代。
