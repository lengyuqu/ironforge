# OPT-LOOP 账本（IronForge）

> 协议来源：`/Users/yuqu/Documents/mybook/Projects/CNB-NPC-LOOP/README.md`（v0.3）。
> 本文件是这套闭环的**跨轮记忆**。规则：**纯追加**；标为「不成立」的条目禁止删除（否则同一错误会被无限重放）。
> 本文件不是产品文档，不参与 `docs/` 的漂移巡检范围。

## 轮次语义（照抄协议 §3 / §4.2，勿改）

- **issue = 容器**：`open` → 6 轮 → `close` + `BATCH_COMPLETE`
- **评论 = 进度**：以 `## 第 K 轮收尾` 开头的评论条数 `C` = 已完成轮数。单次运行被切断后，下次运行数评论即知从 `C+1` 续跑——进度可从平台侧完整重建，不依赖本地文件。
- **本文件 = 记忆**：「不成立」清单 + 批次索引 + 每轮判定与 SHA。
- **单发模式**（非 `[opt-round]` 容器的存量 issue）回帖首行用 `## 本轮收尾`，不带轮次号，不参与批次计数。

## IronForge 本地化参数

> 协议警告：§2.2 的延迟参数是 agent2ssh 单仓库实测（n=19，p100 20min46s），**不可直接沿用**。本仓库规模大一个量级（10 crate / 191K 行 Rust / 664 个 `.rs` / 20K 行文档），须自行标定。

| 参数 | 当前值 | 状态 |
|---|---|---|
| 轮询间隔 | 60 s | 沿用协议 |
| 等待上限 | 起始 60 s × 30 | **待标定**（本批 n 从 0 起算） |
| 验证命令（文档轮） | `scripts/check.sh --quick`（约 40 s） | 已确认存在 |
| 验证命令（代码轮） | `scripts/check.sh`（约 7 min） | 已确认存在 |
| 验证命令（收批） | `scripts/check.sh --full` | 全 workspace |
| 本机/本仓提交身份 | `cnb.biewen` | 实测（issue #11 作者），F15 判据据此 |
| 已知 flaky | webhook 重试时序、`merge_queue_ci_tests`（环境） | 差分判据须 allowlist |

### 分级口径（本项目专用，覆盖协议 §4.4 的通用表）

| 级别 | 范围 | 动作 |
|---|---|---|
| **L1** | `docs/`、`ironforge-docs/`、`CLAUDE.md`/`AGENT.md`/`ARCHITECTURE.md`/`CONTRIBUTING.md`/`README.md` 的文档与注释；只增不减的断言强度；i18n 键值一致性 | 修 + 验证 + 提交 + 推 cnb |
| **L2** | DB 迁移顺序、Git 协议线格式（pkt-line / sideband / `--fix-thin`）、认证权限语义（LDAP/SSO/TOTP/JWT）、gix↔CLI 替换、OpenAPI 与前端契约、任何改 schema 的 | **不动 main**，写进待拍板清单 |
| **禁区** | `.cnb.yml`、`.github/`、`scripts/`、`Cargo.toml`/`Cargo.lock`、`deploy/` | 只报告 |

---

## 批次索引

### batch#1(issue #11) · 2026-11-10 · git 协议文档与实现一致性

- 开批：`2026-11-10T09:21:41Z`（issue 创建即触发，未额外交触发评论）
- 基线提交：`5624605`（cnb/main HEAD）
- 范围：`docs/git-protocol.md` + `crates/rg-git/src/{pkt_line,sideband,protocol/upload_pack,protocol/receive_pack,protocol/v2}.rs`（约 4300 行）
- 轮次上限：6（本批先跑 1 轮做标定）
- NPC 首条分析：**等待中**
- 6 轮进度：0 / 6

---

## 判定不成立（原文保留，请勿重复提）

（暂无）
