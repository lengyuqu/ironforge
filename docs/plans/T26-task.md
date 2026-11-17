# T26 执行记录 — CI 容器默认以调用进程的 UID 运行（#41 R11）

> 计划见 `docs/plans/T26-plan.md`，权威约束见 `tasks/T26.md`。
> 基线 `0b7488c`，分支 `agent/t26-ci-runner-user`，worktree `../ironforge-t26`。

## 1. 开工核对

```
$ git fetch cnb && git rebase cnb/main
Current branch agent/t26-ci-runner-user is up to date.
$ git log --oneline -1
0b7488c docs(tasks): add T26 for CI container running as the caller's UID (#41 R11)
$ id -u; id -g
501
20
```

（任务书里写的基线 `0b48b8c` 在仓库中不存在，实际基线是 `0b7488c`，即 `tasks/T26.md` 的入库提交。）

## 2. 改动

| 文件 | 改动 |
| --- | --- |
| `crates/rg-ci-isolation/src/lib.rs` | `Default::user` 改为进程 `uid:gid`；新增 `RUNNER_USER_ENV`、`resolve_runner_user()`、`current_uid_gid()`、一次性警告；crate 文档与 `user` 字段注释同步；7 个新测试 + 1 个既有测试 |
| `crates/rg-ci-isolation/Cargo.toml` | 新增 `tracing`（warn）与 `[target.'cfg(unix)'.dependencies] libc`（`getuid`/`getgid`） |
| `Cargo.lock` | 仅 `rg-ci-isolation` 的依赖表多出 `libc`、`tracing` 两行（两者均早已在锁文件中） |
| `CLAUDE.md` | 「CI 容器隔离的剩余面」段落改写一句 + 回退开关 |
| `tasks/T26.md` | `State` → `completed` |
| `docs/plans/T26-plan.md`、`docs/plans/T26-task.md` | 本任务文档 |

**调用点零改动**：`rg-ci/src/runner.rs`、`rg-cli/src/main.rs`、`rg-runner/src/main.rs` 全部经
`DockerLimits::default()`，自动继承新默认值。

新增依赖说明（guardrails 🟡 WARN 已登记）：`libc` 自身零依赖、早已是 `Cargo.lock` 里的传递依赖
（`0.2.186`），只多一条 direct edge；`tracing` 是 workspace 既有依赖。不用 `Command::new("id")`
是因为极简镜像没有 `id` 时会静默回退到 root——正是本次要关的洞。

## 3. 实现要点

```rust
pub const RUNNER_USER_ENV: &str = "IRONFORGE_CI_RUNNER_USER";

static IMAGE_DEFAULT_WARNED: Once = Once::new();

impl Default for DockerLimits {
    fn default() -> Self {
        let user = resolve_runner_user();   // 唯一读环境变量处
        if user.is_none() {
            IMAGE_DEFAULT_WARNED.call_once(|| { /* tracing::warn! … */ });
        }
        Self { /* … */ user, /* … */ }
    }
}

#[cfg(unix)]
fn current_uid_gid() -> Option<String> {
    // SAFETY: getuid/getgid take no arguments, cannot fail …
    let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
    Some(format!("{uid}:{gid}"))
}

#[cfg(not(unix))]
fn current_uid_gid() -> Option<String> { None }
```

`resolve_runner_user()`：未设置/空白/非 UTF-8 → 当前 `uid:gid`；`root`/`0` → `None`；
其它 → 原样。`hardening_args()` 逻辑未动（`--user` 原本就在 `no-new-privileges` 之后）。

## 4. 验证（6 条映射）

### 1) 默认值 = 当前进程 uid:gid

测试 `default_user_is_the_calling_process_uid_gid` 用 `id -u`/`id -g` 交叉核对：

```
test tests::default_user_is_the_calling_process_uid_gid ... ok
```

本机 `uid=501 gid=20`，即默认 `--user 501:20`。

### 2) `hardening_args()` 含 `--user` + 该值，顺序不变

测试 `hardening_args_carry_the_user_flag` 用 `IRONFORGE_CI_RUNNER_USER=4242:4243` 断言
`args[nnp+1] == "--user"`、`args[nnp+2] == "4242:4243"`：`ok`。

### 3) 环境变量回退

`root_override_falls_back_to_the_image_default`：`root` 与 `0` 均为 `user == None` 且
`hardening_args()` 不含 `--user`；`4242:4243` 原样用；`"   "`（空白）走默认 `501:20`：

```
test tests::root_override_falls_back_to_the_image_default ... ok
test tests::hardening_args_carry_the_user_flag ... ok
test tests::empty_override_keeps_the_calling_process_identity ... ok
```

### 4) 警告只触发一次

`image_default_warning_is_emitted_at_most_once`：在 `root` 下连续构造 3 次
`DockerLimits::default()`，`IMAGE_DEFAULT_WARNINGS` 仍为 1：`ok`。

### 5) 既有测试全绿

```
$ cargo test -p rg-ci-isolation
running 8 tests
test tests::accepts_ordinary_image_references ... ok
test tests::default_limits_hardened ... ok
test tests::empty_override_keeps_the_calling_process_identity ... ok
test tests::hardening_args_carry_the_user_flag ... ok
test tests::rejects_flag_shaped_and_malformed_images ... ok
test tests::image_default_warning_is_emitted_at_most_once ... ok
test tests::root_override_falls_back_to_the_image_default ... ok
test tests::default_user_is_the_calling_process_uid_gid ... ok
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

$ cargo test -p rg-ci
test result: ok. 46 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 9.48s
test counters_and_gates_agree_on_every_backend ... ok        （tests/quota_backends.rs，1 passed）

$ cargo test -p rg-runner
running 5 tests
test tests::docker_unavailable_message_is_fail_closed ... ok
test tests::resolves_cache_key_from_polled_environment_and_rejects_escape ... ok
test tests::flag_shaped_images_are_refused_before_docker_runs ... ok
test tests::runner_applies_hardening_flags ... ok
test tests::local_executor_injects_polled_variables_with_a_clean_environment ... ok
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

### 6) 环境变量不污染

`with_runner_user()` 在全局 `ENV_LOCK: Mutex<()>` 下设置变量，执行完用 RAII 恢复原值（未设置则
`remove_var`）；所有读写 `IRONFORGE_CI_RUNNER_USER` 的测试都走它，避免并发下互相观察。
crate 是 edition 2021，`std::env::set_var`/`remove_var` 在本版本仍是安全函数，无需 `unsafe`
（若将来升到 edition 2024 需包 `unsafe` 并说明「互斥锁保证无并发读写」）。

### 全量门禁

```
$ cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 53.76s

$ scripts/check.sh --quick
==> cargo fmt --all --check
    (1s)
==> cargo clippy --workspace --all-targets -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.99s
    (1s)
==> checks passed in 2s (quick)
```

## 5. 行为变更（运维须知）

容器默认**不再以 root 运行**，改用 IronForge 进程自己的 `uid:gid`：

- 正常构建不受影响；容器写进 workspace 的文件属主 = IronForge 用户，宿主上不再出现 root 文件。
- **会失败**：镜像 `run:`/entrypoint 需要 root 的工程（`apt-get install`、写 `/usr/local`、
  `adduser` 等）。回退开关：`IRONFORGE_CI_RUNNER_USER=root`（或 `0`）。
- 也可用 `IRONFORGE_CI_RUNNER_USER=1000:1000` 指定任意身份。

## 6. `CLAUDE.md` 改了哪一句

改前：

```
**CI 容器隔离的剩余面**：工作区 rw 挂载 + 容器内 root 仍可在宿主留下 root 属主文件；需靠 daemon 的
userns-remap 或 `rg-ci-isolation::DockerLimits::user` 收紧。镜像/资源/capability 已加固。
```

改后：

```
**CI 容器隔离的剩余面**：容器默认以 `--user <IronForge 进程 uid:gid>` 运行（`rg-ci-isolation::DockerLimits::default`），
工作区由该进程创建、属主天然匹配，宿主不再产生 root 属主文件；需要 root 的镜像（`run:` 里 `apt-get install`、
写 `/usr/local`）用 `IRONFORGE_CI_RUNNER_USER=root` 回退到镜像默认用户。镜像/资源/capability 已加固。
```

## 7. 未做（按任务范围排除）

`userns-remap`、workspace `chown`/`chmod` 逻辑、seccomp/AppArmor 沙箱；三个调用点未改动。
