# IronForge Web 组件测试收官批 · Agent 交接提示词

## 任务

继续 IronForge 前端组件测试补强的**最终收官批**：为剩余约 36 个未测组件补写单测，跑三项验证，分批提交，更新重构日志，输出收官总结。项目是 Rust 后端 + SvelteKit 5（runes 语法）前端，仓库根 `D:\vibercodeing\ironforge`，前端在 `web/`，pnpm 管理。

## 第 0 步：环境自检（先看这一条，再动手）

**先跑一次全量测试确认环境就绪**，不要一上来就写新测试：

```bash
cd web && ./node_modules/.bin/vitest run
```

**预期结果：`Test Files 93 passed (93)` / `Tests 356 passed (356)`，且 `Errors` 一行为 0**（约 15s）
= 197 已提交基线 + 159 新增（工作区 48 个未提交测试文件）。

⚠️ **判定标准不光是 `Tests passed`**：vitest 对断言通过但 event handler 抛异常的情况会单独列
`Errors N errors`（vite 的 unhandledRejection）。看到 `Errors` 非 0 就是有测试在"假绿"——
断言过了但组件内部抛了错。必须把 `Errors` 归零才算基线健康。
（本轮交接前已修掉一例：`ModalHarness.svelte` 未转发 `onClose`，导致 Escape / 点 × 时
`$$props.onClose is not a function`，报 2 errors 但 356 例仍显示 passed。）

对不上怎么办：
- 用例数明显少于 356 → 工作区文件缺失，先 `git status --porcelain` 核对，别重写任何东西
- `Errors` 非 0 → 按报错栈定位是哪个 harness / 测试文件漏传了必需回调或 prop，修对应文件
- 报 `command not found` 或找不到 `./node_modules/.bin/` → 依赖没装，先 `pnpm install`

自检通过后，再往下读"当前状态"和"剩余清单"。

## 当前状态（先 git status 核对，不要重复劳动）

- 已提交基线：vitest **197/197** 全过（Q-1~Q-16 各批，见 `docs/web-component-refactor-log.md`）。
- 工作区已有**未提交**的 48 个测试文件 + 基建改动，**已跑全量验证通过（356/356，0 errors），不要重写**：
  - 根目录组件：Button/Modal/Dropdown/ToastContainer/Layout/CloneModal/AttachmentPanel/Navbar/RepoHeader 测试，及 `ButtonHarness/ModalHarness/DropdownHarness/LayoutHarness.svelte`、`src/test-stubs/CloneModalStub.svelte`
  - home/ 6 个、InstanceBanner、admin/ 11 个、labels/ 2、milestones/ 2、releases/ 2、packages/ 2、boards/ 3、dashboard/ 2、imports/ 2、orgs/ 2、search/ 1、wiki/ 3
  - 基建：`web/src/test-stubs/app-environment.ts`（$app/environment 桩）、`web/vitest.config.ts` 新增 `$app/environment` 别名
  - **7 个组件 bug 修复（已改未提交）**：AuditDetailModal/UserDeleteModal/UserEditModal/LabelDeleteModal/LabelFormModal/MilestoneDeleteModal/MilestoneFormModal 的内层 modal div 补了 `onclick/onkeydown stopPropagation`（原点击按钮会冒泡到 overlay 误触发 onClose）
- svelte-check 基线：**0 errors / 47 warnings**（warnings 是既有基线，不许新增 errors）。

## 剩余未测组件清单（约 36 个，逐个写 `*.test.svelte.ts`）

- `web/src/lib/components/FileEditor.svelte`（约 722 行，交互大件，重点测：加载内容、脏检测/未保存提示、保存调用、语言切换等）
- `repo/` 7 个：BlobContentView、BlobDeletePanel、BlobFileHeader、CommitInfoCard、EmptyRepoGuide、RepoToolbar、StatusChecksPanel
- `pipelines/` 5 个
- `issues/` 13 个
- `pulls/` 10 个

写测试前**必须先读组件源码**，按组件实际行为断言；静态展示组件 1-2 例冒烟，交互组件 3-5 例契约测试。

## 测试代码模板与规范（与已有批次保持一致，可参考任一已提交的 .test.svelte.ts）

### 1. i18n mock（标准头）

```ts
vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
  formatDateTime: (iso: string) => `fmtt(${iso})`,
}));
```

组件用到 `locale` 对象时才加：`locale: { value: 'en', set: vi.fn(), init: () => {} }`。
注意：t() mock 对**插值调用返回 key 原文**，断言用 key（如 `common.created`）或 fallback 英文原文，不要断言插值结果。

### 2. API / toast / confirm mock

```ts
const apiFn = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({ xxx: { yyy: (...a: unknown[]) => apiFn(...a) } }));

const toastSuccess = vi.fn(); const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: { success: (...a: unknown[]) => toastSuccess(...a), error: (...a: unknown[]) => toastError(...a) },
}));

// 确认流组件：
let confirmMock: ReturnType<typeof vi.fn>;
beforeEach(() => { confirmMock = vi.fn(() => true); vi.stubGlobal('confirm', confirmMock); });
afterEach(() => { vi.unstubAllGlobals(); });
```

含路由跳转的组件：`vi.mock('$app/navigation', () => ({ goto: gotoMock }))`（vitest.config.ts 已配别名）。

### 3. 已知坑（必读）

- **Snippet prop（children）组件**：testing-library 无法直接创建，参照 `ButtonHarness.svelte` 建转发 props + children 的 harness 组件。
- **harness 必须转发组件声明为必需的回调 prop**（如 Modal 的 `onClose`）并在 harness 里给 `() => {}` 默认值。否则用例触发该回调时抛
  `$$props.onClose is not a function` —— vitest 仍显示 passed，只在末尾 `Errors` 行计数，极易漏掉（本轮已修 ModalHarness）。
- **"假绿"排查口诀**：`Tests N passed` 但 `Errors` 非 0 → 有 handler 抛错。用 `vi.fn()` 注入回调并断言调用次数，别只"点了就算过"。
- **TS 报错四件套**（本轮 8 个 error 全是这四类，写完顺手跑 svelte-check）：
  - 用了 `vi` 但 import 写成 `import { describe, it, expect } from 'vitest'` → 补 `vi`（vitest 运行时有 globals，tsc 不认）
  - 字面量 union 取值错，如 AttachmentPanel 的 `target` 是 `'issues' | 'pulls' | ...`，写 `'issue'` 报错
  - 造 mock 数据时 `null` 赋给 `string | undefined` 字段 → 用 `undefined`（`as X` 强转也会报错）
  - 被 mock 的函数有定长形参时，包装器不能用 `(...a: unknown[]) => fn(...a)`（spread 类型不匹配）→ 显式列出形参
- **prop 名撞挂载选项**（如 `target`）：必须 `render(C, { props: { target: ... } })` 包裹（见 AttachmentPanel.test）。
- **happy-dom 未实现 option 的 :checked 伪类**：Svelte 5 select 绑定不回流 state，select 交互类用例改 prop 层注入（见 WebhookList.test）。
- `body.click()` 不生效，用 `fireEvent.click(document.body)`。
- 同文本多元素用 `getAllByText(...)[n]` 计数定位；a 内嵌 div 取 href 用 `.closest('a')`；带 "→" 后缀文本用正则；`calc()` 表达式断言原文。
- 时区无关断言：`expect(...).toHaveBeenCalledWith(..., new Date('2026-12-31T23:59').toISOString())` 动态算期望。
- 新遇到 modal 类组件先检查内层是否有 `stopPropagation`（本轮已全库修复 7 个，新写的组件如有同类问题一并修）。
- **不新增任何依赖**，只用现有 vitest + @testing-library/svelte + happy-dom。

## 验证（每批写完就跑，全部完成后跑全量）

```bash
cd web
./node_modules/.bin/vitest run                                   # 93 files / 356 tests 全过，且 Errors 行为 0
./node_modules/.bin/svelte-check --output machine                 # 0 errors / 47 warnings
./node_modules/.bin/vite build                                    # 构建成功
```

三项已在本轮交接前实测全绿（2026-09-03）。写新测试时**每批写完就跑**，别攒到最后。
注意 svelte-check 的 `ERRORS` 计数是**整个 web/ 工程**的，新写的测试文件里一个类型错都会把它从 0 顶起来；
提交前必须回归到 `0 ERRORS 47 WARNINGS`。

## 提交规范（沿用历史五要素格式，参考 git log）

提交标题：`test(web): <批次内容概述>，<目录/主题>（质量补强收官批）`
正文五要素，逐文件列出测试用例要点 + 用例数变化 + 验证结果。示例：

```
test(web): 分支保护列表与 MFA 组件单测，settings 目录全覆盖（质量补强第十五批）

- BranchProtectionList.test.svelte.ts（5 用例）：空态、行渲染仅显示
  启用中的保护徽章、编辑回调透传整条 rule、确认后删除走
  branchProtections.remove + onDeleted + toast、confirm 取消零调用
- ...（每个文件一条）
- vitest 183 → 197 用例，组件层 40/50 覆盖
- 验证：vitest 197/197 · svelte-check 0 errors / 47 warnings（基线不变）
  · vite build ✔
```

分批建议（提交前 `git show --stat HEAD` 核对文件清单）：
0. **交接清理批（已改未提交，建议首个提交）**：见下方"交接前清理清单"，这批改动让基线从
   "356 passed + 2 errors + svelte-check 8 errors" 变成真正全绿，提交信息里说明是测试基建修复
1. 先提交：工作区已有的 48 个测试 + 基建（test-stubs、vitest.config.ts、harness）+ 7 个 stopPropagation bug 修复——可拆 2-3 个提交，bug 修复单独说明"测试发现的生产缺陷"
2. 之后每完成一个目录批次提交一次（repo/ → pipelines/ → issues/ → pulls/ → FileEditor）
3. 最后一个提交：更新 `docs/web-component-refactor-log.md` 补收官批（Q-17 起）记录，格式参照已有 Q 条目，注明累计用例数与全量验证结果

## 交接前清理清单（2026-09-03 已完成，改动在工作区未提交）

跑第 0 步自检时发现基线并非真正全绿，已修掉下面 3 类共 10 个问题。**理解这些改动再接手**，别回退：

| # | 文件 | 问题 | 处理 |
|---|---|---|---|
| 1 | `ModalHarness.svelte` | 未转发 `onClose`（组件里是必需 prop），Escape / 点 × 抛 `$$props.onClose is not a function`，vitest 报 2 errors 但用例仍显示 passed | harness 加 `onClose = () => {}` 默认值并透传 |
| 2 | `Modal.test.svelte.ts` | 第 2、3 例只是"点了"，没有断言 | 注入 `vi.fn()` 断言调用次数 1/2/3，禁用时断言 0 次 |
| 3 | `AttachmentPanel.test.svelte.ts` | `target: 'issue'` 不是合法 `AttachmentTarget`（应为 `'issues'`），3 处断言同步写错 | prop 与 3 处断言一起改成 `'issues'` |
| 4 | `RepoList` / `DashboardRepoGrid` / `FeaturesSection` / `SiteFooter` / `StatsSection` 5 个 test | 用了 `vi` 但没 import | 补 `vi` 到 vitest 导入 |
| 5 | `DashboardRepoGrid.test.svelte.ts` | mock 数据 `owner_name: null` 赋给 `string \| undefined` | 改 `undefined` |
| 6 | `PackageVersions.test.svelte.ts` | mock 包装器 `(...a: unknown[]) => fn(...a)`，被 mock 函数是 6 个定长形参 | 显式列出形参 |

清理后实测：`vitest 356/356，Errors 0` · `svelte-check 0 ERRORS 47 WARNINGS` · `vite build ✔`。

## 约束

- 远程 git233.phynews.com:18443 无凭证，**只 commit 不 push**，收官总结里列出待推送提交清单即可
- 不重构、不删除、不改动与任务无关的代码；组件源码只在测试暴露真实 bug 时最小修复并在提交信息注明
- 已提交的 197 例测试不许改坏；全量 vitest 必须全绿才算收官
- **"全绿"的定义**：`Tests` 全 passed **且** `Errors` 为 0 **且** svelte-check `0 ERRORS`。三者缺一不可

## 交付

收官总结：累计用例数（当前 356，剩余约 36 个组件补完后预计 450+）、覆盖组件数、本轮发现并修复的
bug 清单（生产侧：7 个 modal stopPropagation + 1 个 Pagination key 重复；测试基建侧：见上方清理清单 6 项）、
三项验证结果、待推送提交区间。
