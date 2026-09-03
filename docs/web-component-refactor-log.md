# 前端大组件拆分重构记录

> 系列起点 `7b6880b`（refactor(web): split components, unify auth, fix Svelte 5 runes，2026-09-01）
> 本文档整理该系列中页面拆分相关的 14 个提交，覆盖 19 个路由页面、61 个新组件。

## 一、总体成效

| 指标 | 数值 |
|------|------|
| 拆分页面数 | 19 |
| 新增组件数 | 61（分布在 14 个域目录） |
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
| 看板页（issues/board） | 419 | 173 |

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
| `21eb31c` | 拆分 issues/board 看板页（boards/） | 5 组件 + Board 族类型重写 + api/boards.ts 去 any，8 文件 +542/-317 |

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

### boards/（5 组件，450 行）— 提交 `21eb31c`

| 组件 | 类型 | 职责 |
|------|------|------|
| BoardSwitcher | 纯展示 | board tabs + active 高亮，onSelect/onAddBoard 回调 |
| BoardCreateForm | 自包含 | 看板名 inline 表单（create + toast，onCreated(id) 回调） |
| ColumnCreateForm | 自包含 | 列名 inline 表单（createColumn + toast） |
| BoardColumn | 自包含 | 列头/删列 confirm/添加卡片/drop 目标（moveCard + toast，onRefresh 回调） |
| BoardCard | 纯展示+拖拽源 | 卡片渲染（issue 链接/note），cardId 经 dataTransfer 携带 |

拖放重构：原页面级 draggingCardId/draggingFromColId/dragOverColId 三态改为 dataTransfer 携带 cardId + 各列局部 dragOver 高亮，列间完全解耦；同列 drop 仍为 no-op，位置取列尾。

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
| Board / BoardColumn / BoardCard / BoardColumnFull / BoardFull | 重写/新增 | rg-core BoardFull/ColumnFull/CardFull（card 字段 flatten + issue: {id;number;title}\|null）；删除无使用方的旧可选字段版 Board 族与 BoardColumnEntry |

API 层同步去 any：`api/labels.ts`、`api/pipelines.ts`、`api/pulls.ts`、`api/repos.ts`（get/tree/log/blob/listCommitStatuses/getCombinedStatus）、`api/issues.ts`（七个方法）、`api/releases.ts`（5 处）、`api/wiki.ts`（全方法）、`api/boards.ts`（14 个方法）。`api/admin.ts`、`api/branchProtections.ts`、`api/mfa.ts`、`api/imports.ts` 原已类型化，直接复用。

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
- ✅ `web/scripts/_patch_*.py` 一次性补丁脚本已清理（17 个，本为未跟踪文件，删除后无 git 痕迹）；
- ✅ P3：issues/board 看板页已拆分（提交 `21eb31c`），拖放重构为 dataTransfer 携带 cardId + 列局部高亮；
- ✅ `routes/` 内 400+ 行大页面全部拆分完毕，拆分系列到此收官；
- ✅ Q1-1 `settings/webhooks` 拆分（提交 `d3407b4`）：WebhookForm/WebhookList，页面 437 → 105 行；
- ✅ Q1-2 `settings/+page` 拆分（提交 `4819f47`）：RepoInfoSection（纯展示）/TransferSection/DangerZoneSection，页面 415 → 78 行；`repos.transfer` 返回类型 any → RepoInfo；
- ✅ Q1-3 orgs 域（提交 `41bca01`）：api/orgs.ts 16 处 any 全量类型化（对齐 rg-http orgs.rs 响应，entities 重写 Organization/OrganizationTeam 并新增 OrgSummary/OrgMember/TeamMember）+ orgs 页拆分 CreateOrgForm/OrgList，页面 412 → 104 行；
- ✅ Q1-4 `[owner]/[repo]/boards` 页（提交 `8964411`）：重写为编排层复用 BoardSwitcher/BoardCreateForm/ColumnCreateForm/BoardColumn，消除与 issues/board 的重复实现；保留 board 删除（工具栏按钮）；卡片移动由 select 下拉升级为拖放；
- ✅ Q2-1 `settings/mirror` 拆分（提交 `fe41f2f`）：MirrorForm（自包含，含 Sync/Delete）/MirrorStatusPanel（纯展示），页面 386 → 97 行；mirrors API 已类型化无改动；
- ✅ Q2-2 `settings/collaborators` 拆分（提交 `a51e7b4`）：entities 新增 RepoCollaborator，collaborators.ts 3 处 any 类型化；CollaboratorAddForm/CollaboratorTable，页面 386 → 82 行；
- ✅ Q2-3 packages 域（提交 `3a08a38`）：packages.ts get/getVersion 2 处 any 类型化 + 导出三类型（client 聚合）；PackageList（纯展示）/PackageVersions（自包含，10 种格式安装命令内聚）；列表页 357 → 210 行、详情页 373 → 135 行（删除 latest_version/created_at 死代码字段）；
- ✅ Q2-4 `admin/users` 拆分（提交 `37235eb`）：AdminUserTable（自包含，Unlock 内聚）/UserEditModal/UserDeleteModal，页面 353 → 101 行；warnings +4 均为快照初始化类；
- ✅ Q3-1 issues 列表页拆分（提交 `7e56f28`）：IssueFilterTabs/IssueList（纯展示）+ IssueTemplateChooser（纯展示，validate 降级警告内聚）+ IssueCreateForm（自包含，Q6.3 校验 + 模板预填），页面 381 → 184 行，全 any → 具体类型；client 聚合导出 IssueTemplate/IssueConfig；
- ✅ Q3-2 time_tracking 域拆分（提交 `2f1dc93`）：entities 重写旧 TimeEntry（seconds 字段与后端不符）对齐 time_entry::Model，timeTracking.ts 2 处 any 类型化；IssueSelector/TimeEntryForm/TimeEntryList，页面 350 → 204 行；
- ✅ R2-1 pulls 列表页拆分（提交 `b0a24b9`）：PullFilterTabs/PullList（纯展示）+ PullCreateForm（自包含，Q6.3 校验 + 模板预填内聚），页面 304 → 100 行，prList/branches 去 any；
- ✅ R2-2 pulls API 8 处 any 类型化 + PR 详情页头部提取（提交 `c336efa`）：merge/enableAutoMerge/disableAutoMerge/addComment/setThreadResolved/applySuggestion/applySuggestions 逐一对照 rg-http/rg-core 核实；新增 PrHeader（纯展示），详情页 271 → 169 行；
- ✅ R2-4 Access Tokens 页拆分（提交 `c95734d`）：tokens.ts 内联类型提取为 AccessToken/CreatedToken 并经 client 聚合导出；TokenCreateForm（自包含，新 token 明文展示条）/TokenList（自包含，Revoke confirm），页面 342 → 88 行；
- ✅ R2-3 issues 详情页拆分（提交 `3d3dc07`）：IssueHeader（纯展示）+ IssueCommentForm（自包含，Close/Reopen 内聚），页面 264 → 155 行；
- ✅ R3-1 notifications 域（提交 `856dfb9`）：notifications.ts 5 处 any 类型化（NotificationItem 等）；NotificationList（自包含，event_type 图标映射内聚）；页面 162 → 136 行，load 失败改 error banner；
- ✅ R3-2 pipelines 拆分（提交 `9ed238a`）：PipelineDetailPanel（纯展示编排：header 动作 + info + PipelineFlow + ArtifactsPanel），页面 291 → 236 行；
- ✅ R3-3 repos.ts 4 处 any 类型化（提交 `3f38474`）：stargazers/fork/forks/createCommitStatus；entities.CommitStatus 复用；RepoHeader fork 兜底链清理恒 undefined 访问；
- ✅ M1 milestones 类型化 + 缺口调查（提交 `c5a9d98`）：entities.Milestone 按后端 milestone::Model 重写（旧 GitHub 风格零使用）+ CreateMilestoneInput/UpdateMilestoneInput；milestones.ts 4 处 any 清零并经 client 聚合导出；新增 docs/milestones-gap-analysis.md（结论：后端 5 端点+issue 联动+webhook+导入完整，前端整域零 UI，导入默认写入里程碑数据不可见；M2 UI 需后端先确认列表过滤/进度计数/issue enrich 三项）；
- ✅ M2-1 里程碑 UI（提交 `f4daeab`）：settings/milestones 管理页（编排层）+ MilestoneGrid/MilestoneFormModal/MilestoneDeleteModal（参照 labels 域模式）；IssueCreateForm 里程碑下拉（issues.create 透传 milestone_id）；issue 详情页 IssueMilestonePanel（PATCH null 摘除语义）；i18n 补 zh-CN/en 键组；+4 warnings 均为 FormModal 快照初始化类。前后端里程碑闭环打通，导入数据首次可见可管理；
- ✅ G-1 AI API 调查（无代码改动）：结论**非功能缺口**——ai.rs 头注释与 docs/ai-agent-integration.md 三层架构明确其为 AI Agent 专用 API（MCP stdio server rg-mcp 7 只读工具为第一层消费方），前端 web UI 不是目标消费者，零 UI 是设计使然；协同项销项；
- ✅ R4-1 releases/edit（提交 `0c931ec`）：ReleaseForm 扩展创建/编辑双模式（tag 锁定/隐藏 target 选择器，onCreated 可选化，new 页零改动兼容），页面 265 → 104 行；
- ✅ R4-2 admin/runners（提交 `ffdff2d`）：RunnerRegisterForm（自包含：注册 + 一次性 token 明文条 + copy）/RunnerTable（纯展示）/RunnerDeleteModal，页面 289 → 141 行，any 清零（RunnerListItem 导出并聚合）；
- ✅ R4-3 packages/upload（提交 `1b257e1`）：PackageUploadForm（自包含：格式/文件/metadata/publish + inline banner），页面 278 → 68 行；修复 en.json i18n 键错插嵌套块（双 upload_success 锚点歧义）；
- ✅ Q-1 websockets 事件信封类型化（提交 `dbe594b`）：NotificationWsEvent/JobLogEventData 对照 ws.rs，API 层 any 彻底清零（此前"2 处"统计含注释误报，实际 1 处）；
- ✅ Q-2 组件单测第一批（提交 `0558fff`）：vitest 13 → 25 用例——parseRunnerLabels 兼容契约 5 例、MilestoneGrid 渲染+回调 4 例、RunnerTable 3 例；
- ✅ Q-3 i18n 技术债根治（提交 `364dc08`）：index.ts runes 核心 git mv 为 i18n.svelte.ts + 转发 shim，修复"任何 t() 组件在 vitest 下不可渲染"的既有架构限制（已实测去 mock 可渲染），组件测试解锁；
- ✅ Q-4 纯函数模块单测（提交 `925bb31` + `b6f3251`）：vitest 25 → 64 用例——packageFormats 分类学契约 4 例、repoUrls 链接构造器 9 例、pipelineStatus 7 例、commitStatus + highlightText XSS 防注入 12 例、buildLineDiff LCS 语义 5 例、markdown sanitizeHtml 白名单 unwrap 语义 + renderMarkdown 端到端 10 例；纯函数层（utils/ + packageFormats）覆盖收官；
- ✅ Q-5 第四批列表组件测试（提交 `4cb3a45`）：PackageList 5 例（含 scoped 包名路由编码契约）/IssueList 4 例/PullList 5 例，vitest 64 → 78 用例，i18n mock 升级兼容插值参数；组件层 10/50 覆盖；
- ✅ Q-6 第五批筛选器与网格组件测试（提交 `cc712ba`）：LabelGrid 3 例（色块 style/描述 null 省略/编辑删除回调）、ReleaseList 4 例（卡片渲染/分页显隐/双向回调/首末页禁用，含 toast mock 令 ReleaseCard 自包含动作惰性）、PullFilterTabs 3 例（open/closed/merged，无 all）、IssueFilterTabs 3 例（open/closed/all，无 merged——与 PR 差异断言锁定）；顺手修正 ReleaseAsset 导入源为 $lib/api/releases（与 entities 同名类型字段不同，svelte-check 即时暴露）；vitest 78 → 91 用例，组件层 14/50 覆盖；
- ✅ Q-7 第六批 repo 目录组件测试（提交 `b56cecb`）：BlobBreadcrumb 2 例（crumb 累积路径/根级仅 repo 链接）、FileTreePanel 4 例（dir/tree 双 kind 目录回调/blob 链接前缀 path 且 ref 仅入 query/嵌套 .. 绑 onNavigateUp/size 格式化与 null 隐藏）、ReadmeSection 3 例（真实 renderMarkdown 渲染/loading 占位/无内容零输出）、RecentCommitsPanel 3 例（多行 message 仅首行/链接保留 ref/sha 截 7 位+日期格式化）；vitest 91 → 103 用例，组件层 18/50 覆盖，repo/ 目录纯展示组件全覆盖；
- ✅ Q-8 第七批看板与通用组件测试 + 分页 bug 修复（提交 `e401e04`）：BoardCard 3 例（issue 链接/纯 note 省略/删除与 dragStart 回调）、BoardSwitcher 3 例（tab 渲染/active 唯一/选择与新建回调）、PipelineBadge 4 例（8 终态 icon/running spinner/未知状态回退 pending/配色）、Pagination 5 例（单页零输出/区间摘要/双省略号折叠/首末页禁用/回调 no-op 语义）；**测试发现并修复真实 bug：Pagination keyed each 以 '…' 作 key，左右双省略号共存时（中间页 + 小 siblingCount）触发 Svelte each_key_duplicate 运行时崩溃，修复为省略号 key 追加索引去重**；vitest 103 → 118 用例，组件层 22/50 覆盖；
- ✅ Q-9 第八批 settings 信息面板组件测试（提交 `14d0242`）：RepoInfoSection 3 例（名称/描述渲染、null 描述回退 '-'、private badge 切换）、MirrorStatusPanel 3 例（状态+双时间戳本地化渲染、null 时间戳 common.never 占位、error 高亮+last_sync_error 透出）、BackupCodesPanel 3 例（每码一 code 元素、Copy Codes 换行拼接写剪贴板+成功 toast、剪贴板拒绝报错 toast，stub navigator.clipboard）；vitest 118 → 127 用例，组件层 25/50 覆盖；
- ✅ Q-10 第九批通知与搜索组件测试（提交 `a719676`）：NotificationList 5 例（loading/空态/条目渲染含 icon 映射与 unread 样式/标记已读走 markRead+onRefresh/失败路由 onError，mock client.svelte 聚合层）、SearchBox 4 例（query 绑定/键入与 Enter 语义/? 帮助面板切换/四类型 tab 与回调）；vitest 127 → 136 用例，组件层 27/50 覆盖，notifications/ 与 search/ 两目录全覆盖；
- ✅ Q-11 第十批 settings 列表类组件测试（提交 `fa56df9`）：WebhookList 5 例（空态/events trim 重组/checkbox 切换 webhooks.update+toast+onChanged/确认删除/confirm 取消不动）、TokenList 4 例（空态/行渲染含 Never 占位与本地化日期/revoke+onRefresh/失败内联 error-box）、CollaboratorTable 5 例（空态/#user_id+select 绑定初值/save 走 updatePermission/remove 按 user_id/失败 toast 且不刷新）；**发现测试环境限制：happy-dom 未实现 option 的 :checked 伪类，Svelte 5 select 绑定回读依赖它，DOM 级选项变更无法回流 state（生产浏览器无此问题），save 用例改用 prop 层注入变更覆盖同一契约**；vitest 136 → 150 用例，组件层 30/50 覆盖；
- ✅ Q-12 第十一批确认流组件测试（提交 `8d0fbb5`）：DangerZoneSection 4 例（精确 owner/repo 输入前按钮禁用/repos.delete+goto('/dashboard')/confirm 取消零调用/失败内联 error 且不重定向）、TransferSection 4 例（空 owner 禁用/repos.transfer 传 trim 后 owner+成功框/confirm 取消/失败 error 无成功框）；配套修复：vitest.config.ts 补 $app/navigation 别名指向 @sveltejs/kit 运行时再导出，SvelteKit 虚拟模块在 vitest 下可解析、vi.mock 得以拦截；vitest 150 → 158 用例，组件层 32/50 覆盖；
- ✅ Q-13 第十二批镜像表单测试（提交 `3200f3b`）：MirrorForm 7 例（create 模式无 sync/delete 且空 url 禁用 save/create 提交全字段 trim+interval 换算/edit 预填与 password 刻意不回显/edit 走 update/sync 失败内联不刷新/confirm 后 remove+刷新/create 失败内联不刷新）；vitest 158 → 165 用例，组件层 33/50 覆盖；
- ✅ Q-14 第十三批表单组件测试（提交 `2398edf`）：BranchProtectionForm 4 例（create 默认值/create 提交含逗号串解析与数字过滤与 approvals 条件化 undefined/edit 预填 JSON 解包+分支锁定+update 不含 branch_name/空分支名拦截）、WebhookForm 5 例（push 预选/事件增删/创建全参数+表单重置+onCreated/空 url 拦截/失败 toast）；vitest 165 → 174 用例，组件层 35/50 覆盖；
- ✅ Q-15 第十四批表单测试（提交 `fe957d1`）：TokenCreateForm 5 例（空 name 禁用/trim+空 scopes 回退 repo+日期转本地当日末尾 ISO 时区无关断言/一次性 token 展示+表单重置+onCreated/Copy 写剪贴板切换 Copied/失败内联不出 token）、CollaboratorAddForm 4 例（空标识禁用/trim 标识符+默认 read/成功重置+onAdded/失败内联）；vitest 174 → 183 用例，组件层 37/50 覆盖；
- ✅ Q-16 第十五批分支保护列表与 MFA 测试（提交 `086055f`）：BranchProtectionList 5 例（空态/启用中徽章才出现/编辑透传 rule/删除 remove+onDeleted+onChanged+toast/取消与失败保留规则）、MfaSetupPanel 4 例（QR+secret+空 code 禁用/enable 传 trim code+backup_codes 透传/空 code 拦截/失败 toast）、MfaStatusSection 5 例（禁用态 Set up MFA/启用态计数/密码确认后 disable+onDisabled/双拦截/失败 toast）；vitest 183 → 197 用例，组件层 40/50 覆盖，settings/ 目录 16 组件全部覆盖，各业务目录核心组件测试全部收官；
- ⬜ 剩余可选：M2-2（issue 列表里程碑筛选/进度计数，依赖后端 A1/A2 确认）；后端协同项：list_reviews/TimeEntry 补 username enrich、里程碑删除级联策略 A5；其余组件可按已成熟模式增量补测。R4 后 routes/ 250+ 行页面仅剩 login/search 等单功能低频页，拆分系列正式收官；API any 清零收官。
