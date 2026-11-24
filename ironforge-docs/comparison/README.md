# 这份对比清单是什么，不是什么

## 它是「差异清单」

`gitea-gap-list.csv` 回答的问题是：**Gitea 1.26.4 有、IronForge 没有（或做得不一样）的东西有哪些**。它用来判断"还差多少"，不是用来判断"下一步做什么"。

## `weight` 不表示产品优先级

权重反映的是该项在 **Gitea 自身功能面**里的相对体量。一个权重 3 的项，意思是"这一块在 Gitea 里占 3 分"，**不是**"我们对它的投入应该占 3 分"。

把两者混为一谈，会导致一个具体的错误：**按缺口大小排优先级，等于按 Gitea 的形状决定自己的路线**。

## IronForge 的客户端是 `ifg`，不是 `tea`

| | |
|---|---|
| `ifg` | 正式的面向用户的 CLI 客户端，走 IronForge 自有 API（`/api/v1/`、自有分页响应 `{data, pagination}`） |
| 它的契约 | `ironforge-docs/architecture/ifg-server-compat-2026-11.md`，已有对应测试（如 `rg-http/tests/repo_settings_tests.rs` 覆盖 `settings_version` / `expected_version`） |
| Gitea 兼容 | **迁移入口**：让从 Gitea 来的用户、已有的 `tea` 脚本、CI 里的 `curl /api/v1/...`、IDE 插件能直接跑 |

所以：**`ifg` 好用 > Gitea 完整兼容**。两者冲突时以前者为准。

## 因此，这几行的定位需要单独看

| 行 | 定位 |
|---|---|
| `API-301;API-302;API-303` Gitea REST/OpenAPI 兼容 | **迁移用途**。握手端点（`/version`、`/settings/*`、`/user`）已补齐，让工具能连上；**不追**完整的 Gitea 1.26.4 OpenAPI 契约 |
| `CI-206;CI-208` 第三方 uses 执行器 | **对比项**。ADR-0001 已冻结隔离执行边界，是否解冻取决于是否有真实用户需求 |
| `PACKAGE-*` 生态广度 | **对比项**。协议适配已覆盖主流，长尾按实际使用补 |
| 其余 `product` 行 | 自有需求，按缺口推进 |

## 使用这份清单的规则

1. 排优先级看**产品目标与真实使用**，不看 `weight`
2. 每完成一项，更新 `earned_score` 与 `next_acceptance`，**不因为"Gitea 有"就给分**
3. 新增行之前先问：这是产品需求，还是对比差异？后者要明确标注
