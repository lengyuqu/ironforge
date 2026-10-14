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
//! What is *not* solved here: the workspace is mounted read-write, so a
//! container running as root can leave root-owned files on the host. Run the
//! daemon with user-namespace remapping, or set `user` below, if that matters
//! for your threat model.

use anyhow::{bail, Result};

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
    /// default. `None` leaves the image default (usually root).
    pub user: Option<String>,
    /// Detach the container from all networks (`--network none`). Off by
    /// default because builds commonly fetch dependencies.
    pub network_none: bool,
}

impl Default for DockerLimits {
    fn default() -> Self {
        Self {
            memory: Some(DEFAULT_MEMORY.to_string()),
            cpus: Some(DEFAULT_CPUS.to_string()),
            pids_limit: Some(DEFAULT_PIDS_LIMIT),
            cap_drop_all: true,
            no_new_privileges: true,
            user: None,
            network_none: false,
        }
    }
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
    let allowed = |c: char| {
        c.is_ascii_alphanumeric()
            || matches!(c, '/' | '.' | ':' | '_' | '-' | '@' | '+')
    };
    if !image.chars().all(allowed) {
        bail!("container image contains invalid characters");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
            assert!(validate_container_image(bad).is_err(), "must reject {bad:?}");
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
            assert!(validate_container_image(good).is_ok(), "must accept {good:?}");
        }
    }

    #[test]
    fn default_limits_hardened() {
        let limits = DockerLimits::default();
        let args = limits.hardening_args();
        let joined = args.join(" ");
        assert!(joined.contains("--cap-drop ALL"), "{joined}");
        assert!(joined.contains("--security-opt no-new-privileges"), "{joined}");
        assert!(joined.contains("--pids-limit"), "{joined}");
        assert!(joined.contains("--memory 4g"), "{joined}");
        assert!(joined.contains("--cap-add CHOWN"), "{joined}");
        assert!(!joined.contains("--privileged"), "{joined}");
    }
}