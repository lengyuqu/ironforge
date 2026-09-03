# IronForge Web 组件单测规范（收官批共用）

仓库根 `D:\vibercodeing\ironforge`，前端在 `web/`，SvelteKit 5 + runes + TypeScript，pnpm 管理。
测试栈：vitest + @testing-library/svelte + happy-dom（**禁止新增任何依赖**）。

每个被测组件 `X.svelte` 对应同目录 `X.test.svelte.ts`。

---

## 1. 写测试前的硬性步骤

1. **先读组件源码**（完整读，别猜行为）。按组件真实行为断言，不要为了通过而写断言。
2. 静态展示组件 → 1-2 例冒烟；交互组件 → 3-5 例契约测试。
3. 每写完一个目录就跑验证（见第 5 节），别攒到最后。

---

## 2. 标准 mock 头

### i18n

```ts
vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
  formatDateTime: (iso: string) => `fmtt(${iso})`,
}));
```

组件用到 `locale` 对象时才加：`locale: { value: 'en', set: vi.fn(), init: () => {} }`。

**易错点**：t() mock 对插值调用返回 **key 原文**。断言用 key（如 `common.created`）或
fallback 英文原文，**不要断言插值结果**。

### API / toast / confirm

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

含路由跳转：`vi.mock('$app/navigation', () => ({ goto: gotoMock }))`（vitest.config.ts 已配别名）。
需要 `$app/environment`：`vi.mock('$app/environment', () => ({ browser: true, dev: false }))`。

---

## 3. 已知坑（逐条读，都是踩过的）

- **Snippet prop（children）组件**：testing-library 无法直接构造 snippet。参照
  `web/src/lib/components/ModalHarness.svelte` 建转发 props + children 的 harness 组件。
- **harness 必须转发组件声明为必需的回调 prop**（如 Modal 的 `onClose`），并在 harness 里给
  `() => {}` 默认值。漏了会导致触发时抛 `$$props.onClose is not a function`。
- **"假绿"陷阱**：vitest 在断言通过但 event handler 抛异常时，仍显示 `Tests N passed`，
  只在末尾单独列 `Errors N errors`。**必须同时看 `Errors` 行为 0**。
  排查口诀：注入 `vi.fn()` 回调并断言调用次数，别只"点了就算过"。
- **prop 名撞挂载选项**（如 `target`）：必须 `render(C, { props: { target: ... } })` 包裹。
- **happy-dom 未实现 option 的 `:checked` 伪类**：Svelte 5 select 绑定不回流 state。
  select 交互类用例改 prop 层注入，别靠 fireEvent.change 下拉框。
- `body.click()` 不生效，用 `fireEvent.click(document.body)`。
- 同文本多元素用 `getAllByText(...)[n]` 计数定位；a 内嵌 div 取 href 用 `.closest('a')`；
  带 "→" 后缀文本用正则；`calc()` 表达式断言原文。
- 时区无关断言：`expect(...).toHaveBeenCalledWith(..., new Date('2026-12-31T23:59').toISOString())` 动态算期望。
- modal 类组件：内层容器应有 `stopPropagation`（本轮已全库修 7 个）。**新写测试时若发现
  同类缺陷，最小修复组件源码并在报告中列出**。

### TS 报错四件套（写完跑 svelte-check，这几类最容易犯）

1. 用了 `vi` 但写成 `import { describe, it, expect } from 'vitest'` → 补 `vi`
   （vitest 运行时有 globals，tsc 不认）
2. 字面量 union 取值错，如 `AttachmentTarget = 'issues' | 'pulls' | ...`，写 `'issue'` 报错
3. 造 mock 数据把 `null` 赋给 `string | undefined` 字段 → 用 `undefined`（`as X` 强转也报错）
4. 被 mock 的函数是定长形参时，包装器不能用 `(...a: unknown[]) => fn(...a)`
   （spread 类型不匹配）→ 显式列出形参

---

## 4. 参考样例

同批已提交的测试是最好的样例，任选一读：

- 交互表单：`web/src/lib/components/labels/LabelFormModal.test.svelte.ts`
- 确认弹窗：`web/src/lib/components/milestones/MilestoneDeleteModal.test.svelte.ts`
- 列表 + 分页：`web/src/lib/components/admin/AuditLogTable.test.svelte.ts`
- API 加载 + 错误态：`web/src/lib/components/home/PublicReposSection.test.svelte.ts`
- harness 用法：`web/src/lib/components/Modal.test.svelte.ts` + `ModalHarness.svelte`

---

## 5. 验证

```bash
cd D:\vibercodeing\ironforge\web
./node_modules/.bin/vitest run src/lib/components/<你的目录>     # Tests 全过 且 Errors 0
./node_modules/.bin/svelte-check --output machine                # 必须 0 ERRORS / 47 WARNINGS
```

svelte-check 的 `ERRORS` 是**整个 web/ 工程**的计数，新测试里一个类型错就会把它从 0 顶起来。
提交/交付前必须回归到 `0 ERRORS 47 WARNINGS`。

多个 agent 并发跑 vitest 是安全的（已实测），但**只改自己目录下的 `*.test.svelte.ts`**，
不要碰别人的目录、不要碰非测试源码（除非发现真实 bug）。

---

## 6. 提交

**只 commit，不 push**（远程 `git233.phynews.com:18443` 无凭证）。
由主 agent 统一分批提交，子 agent **不要自己 commit**。
