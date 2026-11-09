# CI 缓存「只写不读」排查（诊断记录，2026-11-09）

> 基线 main `04433f8`。git233（Gitea 28.3.0 + act_runner）上的 Rust job 反复 `No cache found`，而成功的 run 又在 post 步骤保存几百 MB。本文只用 `tea api` 抓到的 job 日志（去 ANSI）与仓库内 workflow 作为证据；需要登录 runner 机器才能回答的问题另列清单，不猜。

## 一、结论先说

原来的描述（「每次 `No cache found`，key 在不同 run 之间完全一致，所以是只写不读」）**不成立**。日志里的实际情况是三条叠加：

1. **缓存是按 runner 各存一份的，不共享。** git233 有两个 runner（`fnos-runner`、`zos-runner`），同一个 job 名会在两者之间来回调度。同一把 key 在 zos 上能命中、在 fnos 上就是 `No cache found`，而 fnos 自己刚保存过的同一把 key，下一次在 fnos 上仍然找不到。
2. **`cargo` 缓存键会随 `Cargo.lock`/环境变。** restore key 的后缀在样本里出现过 `e233010b` → `368e653b` → `d06fa39b`，同一次 run 里不同 job 也不同（clippy 没有 mold linker 那步）。每动一次 lockfile，每个 job 都要重新冷启动一次。
3. **冷启动跑不完就超时，超时的 job 不会保存缓存。** 1713/1714 里所有红色 job 都是在 `timeout-minutes` 上被砍的（clippy 30 分钟整、pg/mysql 20 分钟整、rust-tests 45 分钟整），报 `context deadline exceeded`；post 步骤根本没执行 → 缓存永远种不上 → 下一次还是冷启动。**这是个死锁**，1 和 2 只是让它更容易触发。

`rust-tests` 是最坏的一个：两个 runner 上各保存过 3 次（每次约 638 MB），**从来没有命中过一次**。

## 二、证据

### 命中矩阵（同一批 run，作者用 `tea api …/actions/jobs/<id>/logs` 抓取）

| run | job | runner | 还原 | 保存 | 结果 |
|---|---|---|---|---|---|
| 1693 | Clippy | fnos | **HIT** 423 MB | — | success |
| 1693 | Rust tests | zos | MISS | 638 MB | success |
| 1693 | PostgreSQL smoke | fnos | MISS | 515 MB | success |
| 1693 | MySQL smoke | fnos | MISS | 515 MB | success |
| 1695 | Clippy | fnos | **HIT** 423 MB | — | success |
| 1695 | Real Git protocol | fnos | **HIT** 605 MB | — | success |
| 1695 | PostgreSQL smoke | fnos | MISS | 515 MB | success |
| 1695 | MySQL smoke | fnos | MISS | 515 MB | success |
| 1695 | Rust tests | zos | MISS | 638 MB | success |
| 1696 | MySQL smoke | zos | **HIT** 490 MB | — | success |
| 1697 | Clippy | zos | **HIT** 423 MB | — | success |
| 1697 | Security audit | zos | **HIT** 146 MB | — | success |
| 1697 | Real Git protocol | zos | **HIT** 606 MB | — | success |
| 1697 | PostgreSQL smoke | zos | **HIT** 490 MB | — | success |
| 1697 | **MySQL smoke** | **fnos** | **MISS（key 与 1695 在同 runner 保存的那把**逐字节相同**）** | 517 MB | success |
| 1697 | Rust tests | zos | MISS | 638 MB | success |
| 1698 | MySQL smoke | zos | **HIT** 490 MB | — | success |
| 1699 | MySQL smoke | zos | **HIT** 490 MB | — | success |
| 1705 | MySQL smoke | zos | **HIT** 493 MB | — | success |
| 1713 | Clippy | zos | MISS（新 key） | — | **failure：30 分钟超时** |
| 1713 | Real Git protocol | zos | MISS | — | **failure：30 分钟超时** |
| 1713 | PostgreSQL smoke | zos | MISS | — | **failure：20 分钟超时** |
| 1713 | Rust tests | fnos | MISS（新 key） | — | **failure：45 分钟超时** |
| 1714 | Clippy | fnos | MISS（新 key） | — | **failure：30 分钟超时** |
| 1714 | Real Git protocol | fnos | MISS | 保存 | success（24 分钟） |
| 1714 | PostgreSQL smoke | fnos | MISS | 保存 | success |
| 1714 | MySQL smoke | fnos | MISS | — | failure |

### 关键对照

- **同 key、同 runner、仍 miss**：1695 的 `mysql-smoke-…-e233010b-07b15e6b` 在 fnos 上保存（515 063 249 字节），下一条 run 1697 的同一个 job 在**同一台 fnos** 上找同一把 key，答 `No cache found.`。
- **同 key、换 runner、反而 hit**：`mysql-smoke-…-e233010b-07b15e6b` 在 zos 上连续 1696/1698/1699/1705 命中 4 次（490–493 MB），fnos 三次全 miss。
- **跨 job 的差**：`clippy` 在 fnos 和 zos 上都能命中（423 MB）—— 说明两台各自都有自己的那一份；`rust-tests` 两边都存过、两边都不命中。
- **大小不是唯一门槛**：605 MB 的 `git-protocol` 缓存能命中，638 MB 的 `rust-tests` 从不命中，515 MB 的 mysql 在 fnos 不落地、490 MB 的在 zos 好好的。

### 超时是「死」的具体样子

| job | 起 | 止 | 时长 | 配置 |
|---|---|---|---|---|
| 6455 Clippy（zos） | 10:51:50 | 11:21:53 | 30:03 | `timeout-minutes: 30` |
| 6464 Clippy（fnos） | 12:15:45 | 12:45:51 | 30:06 | `timeout-minutes: 30` |
| 6465 Rust tests（zos） | 12:30:37 | 13:15:40 | 45:03 | `timeout-minutes: 45` |
| 6458 Real Git protocol（zos） | 11:21:53 | 11:51:57 | 30:04 | `timeout-minutes: 30` |
| 6459 PostgreSQL smoke（zos） | 11:51:57 | 12:12:00 | 20:03 | `timeout-minutes: 20` |

全部带 `##[error]context deadline exceeded`，全部**没有** post 保存。也就是说 1713/1714 的「测试失败」其实是「压根没跑到测试」。

## 三、这次的修复（workflow 侧，只做能做的）

`.github/workflows/regression.yml` 的 `timeout-minutes` 上调，让冷启动能跑完并种下缓存：

| job | 原 | 新 |
|---|---|---|
| clippy | 30 | 60 |
| rust-tests | 45 | 90 |
| security-audit | 20 | 30 |
| git-protocol | 30 | 45 |
| postgres-smoke | 20 | 45 |
| mysql-smoke | 20 | 45 |

头部加了注释说明依据（run 1713/1714 的超时时间戳）以及这是**权宜之计**：缓存不共享的根子在 runner 机器上，workflow 改不了。

不做的事：不设置仓库变量、不把缓存/镜像指到第三方、不动 `Swatinem/rust-cache` 的 key/路径（key 已经是稳定的，问题不在这里）。

## 四、给 runner 负责人的检查清单（fnos / zos 各一遍）

**2026-11-09 业主已确认：fnos 与 zos 的缓存是各自独立的**（不是日志推断）。上面第 1 条因此从「待确认」变成「已知」：两台各存一份，同一个 job 被调度到哪台就只能命中哪台的那份。剩下的待确认项：

1. 缓存目录的磁盘：`du -sh <cache.dir>`、`df -h <cache.dir>`，以及该目录是否在 tmpfs / docker volume / 会自动清理的卷上。fnos 是 NAS，注意卷配额。
2. `act_runner` 自己的日志（不是 job 日志）里有没有缓存写入失败：`no space left on device`、`quota`、`context canceled`、`unexpected EOF` 之类；尤其是保存 500 MB 以上归档的那个时刻。
3. 有没有 cron / systemd timer 在清 `~/.cache/actcache`（或配置的 `cache.dir`）；清理周期是多久。
4. 缓存目录里是否残留大量 `.tar.zst` 临时文件（保存中断的痕迹）。
5. `config.yaml` 的 `cache:` 段具体值（`host`/`port`/`dir`）：给下面方案 A/B 提供依据。
6. Gitea 28.3.0 这套实例本身是否承载 action 缓存（服务端侧），还是完全由 runner 各自承担。**没有查到**，按部署文档确认，不要按猜测改。

## 四之二、两条路，选一条

缓存独立这件事定下来之后，能做的只有三种，按代价从低到高：

| 方案 | 做法 | 代价 | 效果 |
|---|---|---|---|
| **B. 把重活钉到一台** | 给其中一台 runner 追加一个 label（比如 `fnos`），`clippy` / `rust-tests` / `*-smoke` 改成 `runs-on: [ubuntu-24.04, fnos]` | runner 侧加一个 label（一条注册参数或界面操作）+ 三行 workflow；单点风险（那台挂了这些 job 排队） | 命中率从「看运气」变成「基本稳定」；不需要新服务 |
| **A. 两台共用一个缓存** | 起一个共用的缓存服务，两台 runner 的 `cache:` 指过去（自建，不引第三方托管） | 多一个要运维的服务 + 可用性要求 | 命中率 ~100%，且扩容到第三台也不用改 |
| C. 接受 50% 命中 | 什么都不改，只靠超时放宽让冷 run 跑完 | 冷 run 最长 60/90 分钟 | 绿，但慢；lockfile 一动就是两台各冷一次 |

**建议先做 B**：改动面最小、可回退、不需要新组件；等 CI 变快、确认收益后再考虑 A。C 是兜底，已经被本 PR 的超时改动覆盖了一部分。

B 的前置事实：目前两个 job 的 `runs-on` 都只写了 `ubuntu-24.04`，Gitea 返回的 labels 也是这一条，所以现在**没法从 workflow 侧区分**这两台 —— 必须先在 runner 上把机器自己的 label 加上，workflow 才有东西可钉。

B 的开关已经装好（本 PR 的第二部分）：`clippy` / `rust-tests` / `git-protocol` / `postgres-smoke` / `mysql-smoke` 五个 job 的 `runs-on` 改成

```yaml
runs-on: ${{ vars.CI_HEAVY_RUNNER || 'ubuntu-24.04' }}
```

- 变量**不设**时求值成 `ubuntu-24.04`，与改动前完全一致（本仓库已经在用它同一套 `vars.X || 'default'` 形式给镜像用）。
- 某台 runner 有了自己的 label 之后，把 `CI_HEAVY_RUNNER` 设为那个 label，这五个 job 就只落那一台，缓存命中从「看运气」变成稳定。
- 回退：删掉变量即回到今天的行为，不需要改代码。

## 五、未覆盖与不确定

- **没法在本机验证**：以上全部来自 git233 的 job 日志与仓库内 workflow，没有登录过 fnos/zos，没有看过 runner 的 `config.yaml`、磁盘和它自己的日志。runner 侧的成因是「与日志一致的解释」，还不是「看到的配置」。
- **job 日志接口会截断**：`/actions/jobs/<id>/logs` 返回的内容并非总是完整（有的 job 只剩尾部）。本文只用首尾时间戳、`Cache hit for:`/`No cache found.`/`Saving cache` 这些一字不差的输出，以及 `Sent <n> of <n>` 的字节数，不从缺失的行推断「没有发生」。
- **保存是否真的落盘，日志答不了**：所有保存日志都止于 `tar` 打完（`Sent … 100.0%`），后面没有 `Cache Size: ~… MB` / `Cache saved successfully`。但**命中过**的缓存证明至少有一部分保存是成功的，所以这条只能当「日志看不到确认」，不能当「保存失败」的证据。
- **两次 run 的对照没做成**：这个 PR 改了 `.github/workflows/`，按 `scripts/ci-changes.py` 的规则会触发**全量** run，正好可以当第一次观测；要拿到「第二次命中」还得再手动 dispatch 一次（`tea actions workflows dispatch regression.yml --ref <分支>`），本次没有排到（队列里还压着两个 run）。
- 缓存键后缀为什么会在同一 job 上从 `-07b15e6b` 变成 `-073d84b9`：与 lockfile / 环境变量有关，未逐项拆解。
