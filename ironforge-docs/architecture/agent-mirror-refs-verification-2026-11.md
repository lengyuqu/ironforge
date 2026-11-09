# 镜像同步不再清掉 `refs/pull`（验证记录，2026-11-09）

> 基线 main `04433f8`。问题记录在 [agent-gaps-verification-2026-11.md](agent-gaps-verification-2026-11.md) 第三节：镜像同步用 `git remote update --prune` + `clone --mirror` 带来的 `+refs/*:refs/*`，把本地写入的 `refs/pull/*`（PR 的 head 锚点）当成「上游没有」删掉。

## 一、先复现，再修

用真实 git 复现（`/tmp` 里手工一轮，与单元测试同一套动作）：

```
upstream: main, feat, tag v1
mirror = git clone --mirror upstream; git update-ref refs/pull/1/head <main>
upstream: feat→feat2、删 feat、加 tag v2
git remote update --prune
→ refs/heads/feat2  refs/heads/main  refs/tags/v1  refs/tags/v2      # refs/pull/1/head 没了
```

写成测试（`crates/rg-core/src/mirror/service.rs` 内的单元测试，先红后绿）：

| 测试 | 未修复时 |
|---|---|
| `a_sync_keeps_the_pull_refs_this_instance_wrote` | 红：同步后 `refs/pull/1/head` 消失 |
| `a_clone_does_not_import_the_upstreams_pull_refs` | 红：clone 把上游的 `refs/pull/9/head` 一起搬进来 |

## 二、改法

镜像只跟上游对齐**分支与标签**，其余命名空间不管：

```rust
const MIRROR_REFSPECS: [&str; 2] =
    ["+refs/heads/*:refs/heads/*", "+refs/tags/*:refs/tags/*"];
```

- **同步**（`fetch_with`）：`git [lockdown] fetch --prune origin <两条 refspec>`。`--prune` 只作用于命令行给的 refspec，所以分支/标签跟着上游走（改名、删除都会同步），`refs/pull/*` 不在范围内，既不会被删也不会被拉取。
- **首次镜像**（`clone_mirror_with` → `adopt_heads_and_tags`）：`clone --mirror` 会写死 `remote.origin.fetch=+refs/*:refs/*`，并把上游的 `refs/pull/*` 一起复制过来。克隆完立刻做两件事：
  1. 把 `remote.origin.fetch` 换成上面两条（`--replace-all` + `--add`），这样人工在那个目录里跑 `git remote update --prune` 也不会再踩；
  2. 删掉克隆带进来的 `refs/pull/*`，让这个命名空间只属于本实例（否则上游的 PR 编号会和本实例的冲突）。
- **安全措施没动**：`GIT_TRANSPORT_LOCKDOWN`（`protocol.ext/file/ssh.allow=never`）与 `net_guard::validate_mirror_url` 原样保留，仍在每次 git 调用与每次同步前生效。

### 取舍（写在这里，避免下次再问）

| 疑问 | 结论 |
|---|---|
| 上游也有 `refs/pull/*` 怎么办？ | 不导入，克隆后直接删。镜像不承载「上游的 PR 引用」；这个命名空间在本实例里是 `publish_head_ref` 的地盘，混在一起会按编号互相顶掉。 |
| 上游有 `refs/notes/*`、Gerrit 的 `refs/changes/*` 等命名空间呢？ | **不再镜像**。这是本次改动带来的范围收缩（以前 `refs/*` 会全搬）。代价：这些命名空间不会跟上游同步；收益：不会再误删本地命名空间。需要的话应按命名空间逐个加进 `MIRROR_REFSPECS`，而不是回到 `refs/*`。 |
| 已有的镜像仓库（老配置 `+refs/*:refs/*`）会怎样？ | 同步用命令行 refspec，配置不参与，所以行为立即变正确；配置本身还是老样子，只有人工在那个目录里执行 `git remote update --prune` 才会重现老行为 —— 记录在案，没有做批量改配置。 |

## 三、证据

| 层级 | 结果 |
|---|---|
| 任务验证（SQLite 无关，纯 git） | `cargo test -p rg-core --lib mirror` → 2 项全通过：同步后 `refs/heads` = {`feat2`, `main`}、`refs/tags` = {`v2`}（`feat`/`v1` 已被 prune）、`refs/pull/1/head` 仍在且 sha 不变；克隆后没有任何 `refs/pull/*`，第二次同步也不会带回来 |
| rg-core 全量单测 | `cargo test -p rg-core --lib` → **267 passed**，无失败（含 `net_guard` 的 4 项传输校验） |
| 静态检查 | `cargo fmt --all --check`、`cargo clippy -p rg-core --all-targets -- -D warnings` 干净 |
| 后端 | 本次改动不碰数据库，没有 PG/MySQL 相关验证 |

**测试怎么绕过 transport lockdown**：lockdown 禁止 `file` 传输，而测试的上游就在本地。所以 `clone_mirror_with` / `fetch_with` 把 lockdown 做成参数，生产固定传 `GIT_TRANSPORT_LOCKDOWN`，测试传 `&[]`；断言的是**同一套 refspec 与同一条 git 命令**。lockdown 自身仍由 `net_guard` 的测试覆盖（`mirror_rejects_ext_and_file_transports`）。

## 四、未覆盖

- **没在真实远端上跑过**：只用本地裸仓库（file 传输）验证；`https` 远端的分支/标签同步沿用原有路径，没有新的网络侧证据。
- **没有验证 `refs/notes/*` 等命名空间的实际影响**：范围内已明确不再镜像，但没有测「老镜像仓库里已有的 `refs/notes` 会不会被删」。按 `--prune` 只作用于命令行 refspec 的语义，它们不会被删，只是也不再更新 —— 这一点是推理，不是实测。
- **没有清理老镜像的 `remote.origin.fetch` 配置**（见上表）。
- **没有并发/中断验证**：同步中途失败时 ref 的中间状态沿用原有行为，本次没测。
- **未在 CI 上跑**：以上都是本机结果；`rg-core --lib` 的 mirror 测试会在 CI 的 Rust job 里跑（不需要数据库）。
