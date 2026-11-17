# T26 执行计划 — CI 容器默认以调用进程的 UID 运行（#41 R11）

> 权威约束见 `tasks/T26.md`。基线 `0b7488c`（`tasks/T26.md` 入库提交），分支
> `agent/t26-ci-runner-user`，worktree `../ironforge-t26`。实际执行与验收输出见
> `docs/plans/T26-task.md`。

## 1. 要关的口子

issue #41 剩下的核心项：

> 工作区 rw 挂载 + 容器内 root 仍能在宿主留下 root 属主文件

`rg-ci-isolation` 的其它加固都已就位（`--cap-drop ALL` 后只回补 5 个 capability、
`no-new-privileges`、memory/cpus/pids 限额、镜像名白名单）。缺的只有「容器内进程是谁」：

- `DockerLimits`（`crates/rg-ci-isolation/src/lib.rs:44`）已有 `user: Option<String>`
- `hardening_args()`（`:105`）已会输出 `--user <value>`
- 但 `Default`（`:63`）是 `user: None` → 容器用镜像默认用户（通常 root）

三个使用点（`rg-ci/src/runner.rs:58`/`:78`、`rg-cli/src/main.rs:2643`、
`rg-runner/src/main.rs:598`/`:667`）全部经 `DockerLimits::default()`，因此**只改
`Default` 一处**即可全局生效，调用点一行不动。

## 2. 决策一：默认 `user` = 调用进程自己的 `uid:gid`

不用固定的 `1000:1000`：

| 方案 | 后果 |
| --- | --- |
| `1000:1000` | 只在「宿主用户恰好是 1000」时才对；其它宿主要补齐 `chown`/`chmod 777`，而 `chown` 需要 root，IronForge 未必有，于是只能 `chmod 777`——比 root 属主更糟 |
| **进程 `uid:gid`（采用）** | workspace 目录由 IronForge 进程创建，**属主天然匹配**；容器写出的文件属主 = 该进程 UID，宿主上不再是 root；零 chown/chmod 逻辑 |

格式用**数字** `uid:gid`，不用用户名：Docker 接受任意数字 UID，不依赖镜像里存在同名
`/etc/passwd` 条目（`alpine`、`distroless` 之类常常没有）。

## 3. 决策二：不做 daemon 级 `userns-remap`

`userns-remap` 是 **dockerd 的部署层配置**，影响整台 daemon 上的所有容器（含运维
自己跑的），需要重启 daemon、可能触发已有镜像/卷的属主迁移。它也不解决「宿主上文件属主
是谁」的问题，只是把 root 映射到某个从属 uid 区间。本任务只收紧 IronForge 自己拉起的
容器，部署层方案留给部署文档，不在代码里做。

## 4. 取 uid/gid 的实现选择（依赖说明）

候选：

1. `std::process::Command::new("id")` —— **否**。每次 `Default::default()` 派两个子进程；
   而且极简镜像（distroless/busybox-mini）没有 `id` 时只能静默回退到 root——正好是本次要
   关的那个洞。
2. 新增 `libc` 直接 `getuid()`/`getgid()` —— **采用**。`libc` 自身零依赖，且**早已在
   `Cargo.lock` 里**（`libc 0.2.186`，tokio/mio 等的传递依赖），所以只多一条 direct
   edge，不新增任何编译单元或供应链面。声明在 crate 的
   `[target.'cfg(unix)'.dependencies]` 下，Windows 构建不引入。
3. 新增 `nix`/`users` —— 否，一个 syscall 而已，不值得。

`getuid`/`getgid` 是 `unsafe`（libc 的 extern fn），已加 `// SAFETY:` 说明：无参数、
不会失败、只读调用者自身凭据。

非 unix（Windows）没有 uid/gid 语义，`current_uid_gid()` 返回 `None` → 走镜像默认用户
并触发下面的警告。

> guardrails：新增外部依赖属 🟡 WARN，已在任务报告中登记；`libc` 为既有传递依赖、零自身
> 依赖，风险低。

## 5. 回退开关：`IRONFORGE_CI_RUNNER_USER`

| 取值 | 行为 |
| --- | --- |
| 未设置 / 空串 | 当前进程 `uid:gid`（默认） |
| `root` / `0` | `user = None`，不传 `--user`，回到镜像默认用户 |
| 其它（如 `1000:1000`） | 原样传给 `docker run --user` |

只在 `resolve_runner_user()` 一处读环境变量（唯一的 `std::env::var` 调用点），三个调用点
不重复读。空串/空白视同未设置；非 UTF-8 视同未设置。

## 6. 警告：`Once` 只打一次

最终 `user == None` 时 `tracing::warn!` 说明「容器以镜像默认用户（通常 root）运行，工作区
挂载会让宿主产生 root 属主文件」，用 `static IMAGE_DEFAULT_WARNED: Once` 保证整个进程只
打一次，不会每次 job 刷屏。测试侧用 `#[cfg(test)] static IMAGE_DEFAULT_WARNINGS: AtomicUsize`
计数（生产代码零成本），断言多次构造只 +1。

## 7. 行为变更（默认收紧）

**这是一次默认收紧：CI 容器不再以 root 运行。**

- 绝大多数构建（`cargo build`、`npm ci`、`make`、写 build 目录）不受影响，反而更快暴露
  「镜像里某路径只有 root 可写」这类真实问题。
- **会失败的情况**：镜像的 `run:`/entrypoint 阶段需要 root，例如 `apt-get install`、
  写 `/usr/local`、`adduser`。
- 运维恢复手段：`IRONFORGE_CI_RUNNER_USER=root`（或 `0`）回退到镜像默认用户；也可用
  `1000:1000` 指定任意身份。

## 8. 风险与未覆盖

- 镜像 `USER` 指令被 `--user` 覆盖，这是预期行为（见上）。
- 容器内进程不再是 root 后，依赖 `DAC_OVERRIDE`/`CHOWN` 回补 capability 的步骤在部分脚本里
  可能失效——这正是「需要 root 就用开关回退」的场景，不在代码里猜测。
- 未做：`userns-remap`、workspace `chown`/`chmod`、seccomp/AppArmor 沙箱（均超出任务范围）。

## 9. 验证计划

| # | 验证 | 手段 |
| --- | --- | --- |
| 1 | 默认值 = 当前 `uid:gid` | 测试内用 `id -u`/`id -g` 交叉核对 `DockerLimits::default().user` |
| 2 | `hardening_args()` 含 `--user` + 该值，顺序不变 | 断言 `--user` 紧跟 `no-new-privileges` 之后 |
| 3 | 环境变量回退 `root`/`0` → `None` 且无 `--user`；`4242:4243` 原样用 | 环境变量用全局 `Mutex` 串行化，用后恢复 |
| 4 | 警告只一次 | `IMAGE_DEFAULT_WARNINGS` 计数 + 多次构造 |
| 5 | 既有测试全绿 | `cargo test -p rg-ci-isolation` / `-p rg-ci` / `-p rg-runner` |
| 6 | 环境变量不污染 | `with_runner_user()` 用 RAII 恢复原值；crate 为 edition 2021，`set_var` 非 unsafe |

## 10. 文档

`CLAUDE.md` 现有「CI 容器隔离的剩余面」段落改一句：默认 `--user <进程 uid:gid>`，需要 root
用 `IRONFORGE_CI_RUNNER_USER=root` 回退。不新开章节。
