//! Shared policy for running CI job containers.
//!
//! Both the embedded runner (`rg-ci`) and the standalone agent (`rg-runner`)
//! launch containers, so the hardening flags and image validation live here to
//! keep the two paths from drifting apart. This crate intentionally has no
//! heavy dependencies so the lightweight runner agent can use it.
//!
//! What is defended against:
//!
//! - **Flag injection** — `image` comes from repository content. A value like
//!   `--privileged` would otherwise be consumed by `docker run` as an option,
//!   so it is validated and separated with `--`.
//! - **Privilege escalation inside the container** — `no-new-privileges`
//!   blocks setuid/setgid binaries from gaining privileges.
//! - **Capability abuse** — all capabilities are dropped and only the file
//!   ownership set that ordinary builds need is added back.
//! - **Resource exhaustion** — memory, CPU and PID limits bound a runaway or
//!   hostile job.
//!
//! - **Host ownership of workspace files** — the workspace is mounted
//!   read-write, so a container running as root leaves root-owned files on the
//!   host. [`DockerLimits::default`] therefore runs the container as the
//!   calling process's own `uid:gid`, which owns the workspace by construction.
//!   Set [`RUNNER_USER_ENV`] to `root` to opt back into the image default.

use std::sync::Once;

use anyhow::{bail, Result};

/// Overrides the user the job container runs as.
///
/// - unset / empty — run as the calling process's `uid:gid` (the default)
/// - `root` or `0` — leave the image default (usually root) and pass no
///   `--user`; needed by images whose `run:` step requires root
/// - anything else — passed to `docker run --user` verbatim (e.g. `1000:1000`)
pub const RUNNER_USER_ENV: &str = "IRONFORGE_CI_RUNNER_USER";

/// Default memory cap per job container.
pub const DEFAULT_MEMORY: &str = "4g";
/// Default CPU cap per job container.
pub const DEFAULT_CPUS: &str = "4";
/// Default PID cap per job container.
pub const DEFAULT_PIDS_LIMIT: u32 = 2048;

/// Capabilities re-added after `--cap-drop=ALL`.
///
/// These are the file-ownership capabilities that package managers and build
/// scripts routinely need (`chown`, writing to root-owned paths). Everything
/// else — notably `NET_RAW`, `NET_BIND_SERVICE`, `KILL`, `MKNOD`, `SETFCAP` —
/// stays dropped.
const BUILD_CAPABILITIES: &[&str] = &["CHOWN", "DAC_OVERRIDE", "FOWNER", "SETGID", "SETUID"];

/// Resource and capability limits applied to every job container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerLimits {
    /// `--memory` value, e.g. `"2g"`. `None` disables the cap.
    pub memory: Option<String>,
    /// `--cpus` value, e.g. `"2"`. `None` disables the cap.
    pub cpus: Option<String>,
    /// `--pids-limit` value. `None` disables the cap.
    pub pids_limit: Option<u32>,
    /// Drop all capabilities, re-adding only [`BUILD_CAPABILITIES`].
    pub cap_drop_all: bool,
    /// Pass `--security-opt no-new-privileges`.
    pub no_new_privileges: bool,
    /// Run the container as this `uid:gid` (or name) instead of the image
    /// default. Defaults to the calling process's own `uid:gid` so files
    /// written into the read-write workspace are not left root-owned on the
    /// host; see [`RUNNER_USER_ENV`]. `None` leaves the image default (usually
    /// root).
    pub user: Option<String>,
    /// Detach the container from all networks (`--network none`). Off by
    /// default because builds commonly fetch dependencies.
    pub network_none: bool,
}

/// Fires once, at most, on the first `DockerLimits` built without `--user`.
static IMAGE_DEFAULT_WARNED: Once = Once::new();

/// Counts the warning emitted by [`DockerLimits::default`] so tests can assert
/// that it is emitted at most once.
#[cfg(test)]
static IMAGE_DEFAULT_WARNINGS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

impl Default for DockerLimits {
    fn default() -> Self {
        let user = resolve_runner_user();
        if user.is_none() {
            IMAGE_DEFAULT_WARNED.call_once(|| {
                #[cfg(test)]
                IMAGE_DEFAULT_WARNINGS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                tracing::warn!(
                    "CI containers are running as the image's default user (usually root); \
                     the read-write workspace mount will leave root-owned files on the host. \
                     Unset {RUNNER_USER_ENV} to run as the current uid:gid"
                );
            });
        }
        Self {
            memory: Some(DEFAULT_MEMORY.to_string()),
            cpus: Some(DEFAULT_CPUS.to_string()),
            pids_limit: Some(DEFAULT_PIDS_LIMIT),
            cap_drop_all: true,
            no_new_privileges: true,
            user,
            network_none: false,
        }
    }
}

/// Resolves `--user` from [`RUNNER_USER_ENV`], falling back to the calling
/// process. This is the only place the environment variable is read.
fn resolve_runner_user() -> Option<String> {
    match std::env::var(RUNNER_USER_ENV) {
        Ok(value) => {
            let value = value.trim();
            if value.is_empty() {
                current_uid_gid()
            } else if matches!(value, "root" | "0") {
                // Explicit opt-out: keep the image default (often root).
                None
            } else {
                Some(value.to_string())
            }
        }
        // Unset — or set to non-UTF-8 garbage, which is not a usable uid:gid —
        // means "use the caller's identity".
        Err(_) => current_uid_gid(),
    }
}

/// The calling process's numeric `uid:gid`.
///
/// Numeric ids are used deliberately: Docker accepts any uid/gid regardless of
/// whether the image has a matching `/etc/passwd` entry, and the workspace the
/// container writes into is created by this process, so ownership matches
/// without `chown`.
#[cfg(unix)]
fn current_uid_gid() -> Option<String> {
    // SAFETY: getuid/getgid take no arguments, cannot fail and have no
    // preconditions; they only read the caller's own credentials.
    let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
    Some(format!("{uid}:{gid}"))
}

/// Windows has no uid/gid, and Docker Desktop containers do not map the Windows
/// caller's identity, so there is nothing correct to pass here.
#[cfg(not(unix))]
fn current_uid_gid() -> Option<String> {
    None
}

impl DockerLimits {
    /// Flags to insert into `docker run`, before the image argument.
    pub fn hardening_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(memory) = &self.memory {
            args.push("--memory".to_string());
            args.push(memory.clone());
        }
        if let Some(cpus) = &self.cpus {
            args.push("--cpus".to_string());
            args.push(cpus.clone());
        }
        if let Some(pids) = self.pids_limit {
            args.push("--pids-limit".to_string());
            args.push(pids.to_string());
        }
        if self.cap_drop_all {
            args.push("--cap-drop".to_string());
            args.push("ALL".to_string());
            for capability in BUILD_CAPABILITIES {
                args.push("--cap-add".to_string());
                args.push((*capability).to_string());
            }
        }
        if self.no_new_privileges {
            args.push("--security-opt".to_string());
            args.push("no-new-privileges".to_string());
        }
        if let Some(user) = &self.user {
            args.push("--user".to_string());
            args.push(user.clone());
        }
        if self.network_none {
            args.push("--network".to_string());
            args.push("none".to_string());
        }
        args
    }
}

/// Reject container image references that are not safe to pass to `docker run`.
///
/// The value originates from repository content. An unvalidated value such as
/// `--privileged` or `-v/:/host` would be parsed by `docker run` as an option,
/// so anything flag-shaped, whitespace-bearing or path-like is refused.
pub fn validate_container_image(image: &str) -> Result<()> {
    if image.is_empty() {
        bail!("container image is empty");
    }
    if image.len() > 255 {
        bail!("container image name is too long ({} bytes)", image.len());
    }
    if image.starts_with('-') {
        bail!("container image must not start with '-' (would be parsed as a docker flag)");
    }
    if image.contains(char::is_whitespace) {
        bail!("container image must not contain whitespace");
    }
    if image.contains("..") || image.contains("//") {
        bail!("container image must not contain path traversal");
    }
    // Allowed grammar: [registry/]name[:tag][@digest]. Characters outside this
    // set (control bytes, quotes, backslashes, shell metacharacters) are refused.
    let allowed =
        |c: char| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | ':' | '_' | '-' | '@' | '+');
    if !image.chars().all(allowed) {
        bail!("container image contains invalid characters");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn rejects_flag_shaped_and_malformed_images() {
        for bad in [
            "",
            "--privileged",
            "-v/:/host",
            "alpine:3.20 --privileged",
            "alpine\n:latest",
            "alpine;rm -rf /",
            "../etc/passwd",
        ] {
            assert!(
                validate_container_image(bad).is_err(),
                "must reject {bad:?}"
            );
        }
    }

    #[test]
    fn accepts_ordinary_image_references() {
        for good in [
            "alpine",
            "alpine:3.20",
            "docker.io/library/node:20-alpine",
            "ghcr.io/owner/image@sha256:abc123",
            "localhost:5000/team/image:tag",
        ] {
            assert!(
                validate_container_image(good).is_ok(),
                "must accept {good:?}"
            );
        }
    }

    #[test]
    fn default_limits_hardened() {
        let limits = DockerLimits::default();
        let args = limits.hardening_args();
        let joined = args.join(" ");
        assert!(joined.contains("--cap-drop ALL"), "{joined}");
        assert!(
            joined.contains("--security-opt no-new-privileges"),
            "{joined}"
        );
        assert!(joined.contains("--pids-limit"), "{joined}");
        assert!(joined.contains("--memory 4g"), "{joined}");
        assert!(joined.contains("--cap-add CHOWN"), "{joined}");
        assert!(!joined.contains("--privileged"), "{joined}");
    }

    /// Env vars are process-global, so every test that reads or writes
    /// [`RUNNER_USER_ENV`] runs under this lock; otherwise a concurrent test
    /// could observe another's override.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Runs `f` with [`RUNNER_USER_ENV`] set to `value` (`None` removes it) and
    /// restores the previous value afterwards, so no other test is polluted.
    fn with_runner_user<T>(value: Option<&str>, f: impl FnOnce() -> T) -> T {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let previous = std::env::var(RUNNER_USER_ENV).ok();
        match value {
            Some(v) => std::env::set_var(RUNNER_USER_ENV, v),
            None => std::env::remove_var(RUNNER_USER_ENV),
        }
        let result = f();
        match previous {
            Some(v) => std::env::set_var(RUNNER_USER_ENV, v),
            None => std::env::remove_var(RUNNER_USER_ENV),
        }
        result
    }

    #[cfg(unix)]
    fn id_output(flag: &str) -> String {
        let out = std::process::Command::new("id")
            .arg(flag)
            .output()
            .expect("`id` must be available to cross-check uid/gid");
        assert!(out.status.success(), "id {flag} failed");
        String::from_utf8(out.stdout)
            .expect("id output must be UTF-8")
            .trim()
            .to_string()
    }

    #[cfg(unix)]
    #[test]
    fn default_user_is_the_calling_process_uid_gid() {
        let expected = format!("{}:{}", id_output("-u"), id_output("-g"));
        let limits = with_runner_user(None, DockerLimits::default);
        assert_eq!(limits.user.as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn hardening_args_carry_the_user_flag() {
        let limits = with_runner_user(Some("4242:4243"), DockerLimits::default);
        assert_eq!(limits.user.as_deref(), Some("4242:4243"));
        let args = limits.hardening_args();
        // `--user <value>` sits right after `no-new-privileges`, matching the
        // order of the other flags.
        let nnp = args
            .iter()
            .position(|a| a == "no-new-privileges")
            .expect("no-new-privileges flag present");
        assert_eq!(args[nnp + 1], "--user");
        assert_eq!(args[nnp + 2], "4242:4243");
    }

    #[test]
    fn root_override_falls_back_to_the_image_default() {
        for value in ["root", "0"] {
            let limits = with_runner_user(Some(value), DockerLimits::default);
            assert_eq!(limits.user, None, "{value} must disable --user");
            assert!(
                !limits.hardening_args().iter().any(|a| a == "--user"),
                "{value} must not emit --user"
            );
        }
    }

    #[test]
    fn empty_override_keeps_the_calling_process_identity() {
        let expected = with_runner_user(None, DockerLimits::default).user;
        let limits = with_runner_user(Some("   "), DockerLimits::default);
        assert_eq!(limits.user, expected);
    }

    #[test]
    fn image_default_warning_is_emitted_at_most_once() {
        // The `Once` is global, so the first test to build a root-fallback
        // `DockerLimits` may already have fired it; it can never exceed one.
        with_runner_user(Some("root"), || {
            for _ in 0..3 {
                assert_eq!(DockerLimits::default().user, None);
            }
        });
        assert_eq!(
            IMAGE_DEFAULT_WARNINGS.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
    }
}
