# OPT-LOOP 工作区隔离（IronForge 落地版）

> 来源：协议 `~/Documents/mybook/Projects/CNB-NPC-LOOP/README.md` **§5.1**（v0.9，agent2ssh 实测）。本文只写 **IronForge 的参数与命令**，动机与实测证据看协议正文，不在此重复。
> 适用范围：`/tmp/ironforge-opt-loop.lock` 那两个定时任务（生产者 / 消费者）。
> 版本：2026-11-11 首版（吸收 v0.9）。

## 1. 参数

| 项 | 值 |
|---|---|
| 主工作区 `REPO_DIR` | `/Users/yuqu/vibeCodeing/ironforge`（**只读**） |
| worktree 根 `WTROOT` | `/Users/yuqu/vibeCodeing/ironforge-opt-worktrees`（仓库外） |
| 消费者分支 | `opt/issue-<N>-r<K>-<YYYYMMDD>` |
| 生产者分支 | `opt/prod-batch-<B>-<YYYYMMDD>` |
| 消费者 WT 目录 | `$WTROOT/issue-<N>-r<K>` |
| 生产者 WT 目录 | `$WTROOT/prod-batch-<B>` |
| 唯一远程 | `cnb`（禁 origin / github） |
| 共享重目录 | `CARGO_TARGET_DIR=$REPO_DIR/target`；`web/node_modules` 符号链接；WT 私有 `info/exclude` 追加 `node_modules` |

## 2. 命令骨架（消费者）

```bash
REPO_DIR=/Users/yuqu/vibeCodeing/ironforge
WTROOT=/Users/yuqu/vibeCodeing/ironforge-opt-worktrees
WT="$WTROOT/issue-<N>-r<K>"
BR="opt/issue-<N>-r<K>-$(date +%Y%m%d)"

mkdir -p "$WTROOT"
git -C "$REPO_DIR" fetch -q cnb
git -C "$REPO_DIR" worktree add -q "$WT" -b "$BR" cnb/main \
  || { echo "worktree add 失败，本次不消费"; exit 1; }
ln -s "$REPO_DIR/web/node_modules" "$WT/web/node_modules" 2>/dev/null || true
printf 'node_modules\n' >> "$(git -C "$WT" rev-parse --git-path info/exclude)"
export CARGO_TARGET_DIR="$REPO_DIR/target"     # 复用主工作区 target，避免每轮全量重编
```

之后所有读文件、改文件、跑 `scripts/check.sh`、`git add` / `commit` 都在 `$WT` 内完成。

## 3. 让位判据（开工前，三者任一在动就退出）

1. `git -C "$REPO_DIR" ls-remote cnb refs/heads/main` 的 tip 在最近 5 分钟内变过；
2. 该 issue 最近一条评论新于我方的上一条收尾；
3. `git -C "$REPO_DIR" worktree list` 里其它 worktree 的 `mtime` / `status --porcelain` 行数在动。

判为并发写者且不是本会话 → `git -C "$REPO_DIR" worktree remove "$WT" && git -C "$REPO_DIR" branch -D "$BR"`，零改动退出，输出「让位：有并发写者」。

## 4. 合流（必须在发带轮次号的收尾帖之前）

```bash
git -C "$WT" fetch -q cnb
git -C "$WT" rebase cnb/main           # 撞账本时两段都留，不丢对方内容
git -C "$WT" push cnb HEAD:main
git -C "$WT" merge-base --is-ancestor HEAD cnb/main || { echo "合流失败"; exit 1; }
git -C "$REPO_DIR" fetch -q cnb && { git -C "$REPO_DIR" merge --ff-only cnb/main || true; }
```

**合流失败不得发带轮次号的收尾帖**：评论数就是轮次计数器，发了会让 `C+1` 而改动没进 `main`，该轮被永久跳过。正确动作是发一条**不带轮次号**的通知评论说明状态、保留 worktree，下次运行先补合流。

## 5. 清理（最后一步，顺序不可换）

```bash
git -C "$WT" merge-base --is-ancestor HEAD cnb/main \
  && git -C "$REPO_DIR" worktree remove "$WT" \
  && git -C "$REPO_DIR" branch -d "$BR"
rm -rf /tmp/ironforge-opt-loop.lock
```

`worktree remove` **不用** `--force`；`branch -d` **不用** `-D`。先验祖先再删，否则可能删掉未合流的分支。

## 5.5 本仓 smoke（2026-11-11 首次实测）

| 项 | 结果 |
|---|---|
| `git worktree add -q $WT -b $BR cnb/main` | 成功，分支起点 `7be9a77` |
| `web/node_modules` symlink + WT 私有 `info/exclude` | `git status --porcelain` = **0 行**（不出现的 `?? node_modules` 就是这条 exclude 起的作用） |
| WT 内 `check.sh --quick`（`CARGO_TARGET_DIR` 指主工作区 target） | **104 s，EXIT=0** |
| 主工作区同命令（热缓存） | 2 s —— **差距是 wt 内 cargo 指纹不全导致的局部重编**，共享 target 已把代价从「全量 20 min」压到 2 分钟级 |
| `worktree remove`（无 `--force`）/ `branch -d` | 均成功，残留 `opt/*` 分支 0 |

> 结论：可用。每轮多花约 100 s 编译，换来主工作区不被并发写者污染——比一次 `git add -A` 误提交便宜得多。

## 6. 与锁的关系（正交，不能互相替代）

| 机制 | 解决什么 | 本仓实现 |
|---|---|---|
| 工作区隔离（本文） | 同一棵树被两个写者同时改 | 每轮分支 + worktree，主工作区只读 |
| 锁 | 两个任务重复消费同一条 issue | `/tmp/ironforge-opt-loop.lock`，`pid` 判活，收工手动 `rm -rf` |

锁挡不住「不持锁的并发写者」——那正是本文存在的原因；隔离也挡不住「两个消费者同时消费同一 issue」——那要靠锁。

## 7. IronForge 特有注意

- **`target/` 必须共享**：本仓 `target/` 有 60 万+ 文件，每轮全量重编不可接受。`CARGO_TARGET_DIR` 指主工作区后，cargo 用文件锁串行复用，代价是并发构建会排队（可接受，因为每轮只跑一次 `check.sh`）。
- **`web/node_modules` 用符号链接**：`.gitignore` 里是 `node_modules/`（**带尾斜杠**），只匹配目录、不匹配符号链接，所以要在 WT 私有的 `info/exclude` 里再追加一行，否则 `worktree remove` 会以「包含未跟踪文件」拒绝。
- **账本在仓库内**：消费者每轮都要改 `docs/reports/OPT-LOOP-LEDGER.md`，它是最容易撞车的文件。`rebase` 撞账本时**两段都留**（对方段落在前、我方段落追加在后），绝不 `--ours` 覆盖。
