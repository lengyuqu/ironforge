//! Outbound URL guard — SSRF prevention for server-initiated requests.
//!
//! Two policies are provided:
//!
//! - [`validate_webhook_url`]: for webhook delivery (server POSTs
//!   attacker-controlled payloads to a user-supplied URL). Only public
//!   http/https endpoints are allowed — loopback, link-local (cloud
//!   metadata), private, and unique-local addresses are all rejected,
//!   matching Gitea's default `ALLOW_PRIVATE_ADDRESS = false` behaviour.
//! - [`validate_mirror_url`]: for repository mirroring (server pulls from a
//!   user-supplied remote). Self-hosted deployments commonly mirror from
//!   internal git servers, so RFC1918 private addresses remain allowed;
//!   loopback, link-local and unique-local addresses are still rejected.
//!   Dangerous git transports (`ext::` RCE, `file://` local reads) are
//!   rejected by the scheme allowlist; [`crate::mirror::service`] additionally
//!   enforces `protocol.*.allow=never` at the git invocation layer.
//!
//! Validation resolves the hostname and fails closed when resolution fails
//! or returns any blocked address. Callers that perform the actual request
//! (e.g. webhook `deliver`) should re-validate immediately before sending to
//! narrow the DNS-rebinding window.
//!
//! # Escape hatch
//!
//! Setting `IRONFORGE_ALLOW_LOCAL_OUTBOUND=1` disables the address blocking
//! (scheme allowlists still apply) for both policies. This exists for
//! integration tests and local development against loopback receivers;
//! production deployments must not set it.

use std::net::IpAddr;

/// Schemes accepted for webhook endpoints.
const WEBHOOK_SCHEMES: &[&str] = &["http", "https"];
/// Schemes accepted for mirror remotes.
const MIRROR_SCHEMES: &[&str] = &["http", "https", "git"];

/// See module docs — test/dev escape hatch for loopback/private targets.
fn local_outbound_allowed() -> bool {
    std::env::var("IRONFORGE_ALLOW_LOCAL_OUTBOUND")
        .ok()
        .as_deref()
        == Some("1")
}

/// Default port used for DNS resolution when the URL omits one.
fn default_port(scheme: &str) -> u16 {
    match scheme {
        "https" => 443,
        "git" => 9418,
        _ => 80,
    }
}

/// Minimal URL parse: `scheme://[userinfo@]host[:port]/...`.
///
/// Returns `(scheme, host, port)`. URLs without an explicit `scheme://`
/// separator are rejected — this excludes `ext::<cmd>` git transports,
/// scp-like `user@host:path` ssh URLs, and bare hostnames.
fn parse_url(url: &str) -> Result<(String, String, u16), String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("URL is empty".to_string());
    }
    // Reject embedded control characters / whitespace tricks.
    if url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("URL contains control characters".to_string());
    }
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| "URL must be absolute (scheme://host)".to_string())?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme.is_empty() {
        return Err("URL scheme is empty".to_string());
    }

    // Authority ends at the first path/query/fragment separator.
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("");
    // Strip userinfo (`user:pass@`).
    let host_port = match authority.rsplit_once('@') {
        Some((_, host_port)) => host_port,
        None => authority,
    };

    let (host, port) = if let Some(inner) = host_port.strip_prefix('[') {
        // IPv6 literal: `[::1]:port`
        let end = inner
            .find(']')
            .ok_or_else(|| "malformed IPv6 host".to_string())?;
        let host = &inner[..end];
        let port_part = &inner[end + 1..];
        let port = match port_part.strip_prefix(':') {
            Some(p) => p
                .parse::<u16>()
                .map_err(|_| "invalid port".to_string())?,
            None if port_part.is_empty() => default_port(&scheme),
            _ => return Err("malformed port".to_string()),
        };
        (host.to_string(), port)
    } else {
        match host_port.rsplit_once(':') {
            // Only treat as port when the remainder is all digits.
            Some((h, p)) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => (
                h.to_string(),
                p.parse::<u16>()
                    .map_err(|_| "invalid port".to_string())?,
            ),
            _ => (host_port.to_string(), default_port(&scheme)),
        }
    };

    if host.is_empty() {
        return Err("URL host is empty".to_string());
    }
    Ok((scheme, host, port))
}

/// Is this address blocked for *webhook* delivery (all non-public ranges)?
fn is_webhook_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_webhook_blocked_ip(IpAddr::V4(v4));
            }
            v6.is_loopback()
                || v6.is_unspecified()
                || is_ipv6_unique_local(v6)
                || is_ipv6_link_local(v6)
        }
    }
}

/// Is this address blocked for *mirror* remotes? Loopback, link-local
/// (includes cloud metadata endpoints) and unique-local are rejected;
/// RFC1918 private ranges are allowed for internal mirroring.
fn is_mirror_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_link_local() || v4.is_unspecified(),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_mirror_blocked_ip(IpAddr::V4(v4));
            }
            v6.is_loopback() || v6.is_unspecified() || is_ipv6_link_local(v6)
        }
    }
}

fn is_ipv6_unique_local(ip: std::net::Ipv6Addr) -> bool {
    (ip.segments()[0] & 0xfe00) == 0xfc00
}

fn is_ipv6_link_local(ip: std::net::Ipv6Addr) -> bool {
    (ip.segments()[0] & 0xffc0) == 0xfe80
}

/// Resolve the host and return an error if any resolved address is blocked.
/// Fails closed when resolution yields no addresses.
async fn resolve_and_check(
    host: &str,
    port: u16,
    blocked: fn(IpAddr) -> bool,
) -> Result<(), String> {
    let addrs = tokio::net::lookup_host((host.to_string(), port))
        .await
        .map_err(|e| format!("cannot resolve host {host:?}: {e}"))?;
    let mut any = false;
    for addr in addrs {
        any = true;
        if blocked(addr.ip()) {
            return Err(format!("host {host:?} resolves to a blocked address ({})", addr.ip()));
        }
    }
    if !any {
        return Err(format!("host {host:?} does not resolve to any address"));
    }
    Ok(())
}

/// Validate a webhook endpoint URL (SSRF guard).
///
/// Only public http/https endpoints are accepted.
pub async fn validate_webhook_url(url: &str) -> Result<(), String> {
    let (scheme, host, port) = parse_url(url)?;
    if !WEBHOOK_SCHEMES.contains(&scheme.as_str()) {
        return Err(format!(
            "webhook URL scheme must be one of {WEBHOOK_SCHEMES:?}, got {scheme:?}"
        ));
    }
    if local_outbound_allowed() {
        return Ok(());
    }
    resolve_and_check(&host, port, is_webhook_blocked_ip).await
}

/// Validate a mirror remote URL (RCE/SSRF guard).
///
/// Accepts http/https/git schemes; rejects loopback, link-local and
/// unique-local targets. Private (RFC1918) addresses are allowed because
/// internal mirroring is a supported self-hosted use case.
pub async fn validate_mirror_url(url: &str) -> Result<(), String> {
    let (scheme, host, port) = parse_url(url)?;
    if !MIRROR_SCHEMES.contains(&scheme.as_str()) {
        return Err(format!(
            "mirror URL scheme must be one of {MIRROR_SCHEMES:?}, got {scheme:?} \
             (ext:: and file:// transports are not allowed)"
        ));
    }
    if local_outbound_allowed() {
        return Ok(());
    }
    resolve_and_check(&host, port, is_mirror_blocked_ip).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The escape hatch reads a process-global env var; tests that validate
    /// addresses and tests that mutate the var must not interleave.
    static ENV_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    // ── parse_url ─────────────────────────────────────────────────────────

    #[test]
    fn parse_rejects_ext_transport() {
        assert!(parse_url("ext::sh -c 'evil'").is_err());
    }

    #[test]
    fn parse_rejects_scp_like_ssh_url() {
        assert!(parse_url("git@github.com:owner/repo.git").is_err());
    }

    #[test]
    fn parse_rejects_file_url() {
        // `file:///path` has an empty authority — rejected at parse level.
        assert!(parse_url("file:///etc/passwd").is_err());
        // Even a host-bearing file:// URL parses but is rejected by the
        // mirror scheme allowlist.
        let (scheme, ..) = parse_url("file://evilhost/etc/passwd").unwrap();
        assert_eq!(scheme, "file");
        assert!(!MIRROR_SCHEMES.contains(&"file"));
        assert!(!WEBHOOK_SCHEMES.contains(&"file"));
    }

    #[test]
    fn parse_rejects_relative_and_empty() {
        assert!(parse_url("").is_err());
        assert!(parse_url("/etc/passwd").is_err());
        assert!(parse_url("javascript:alert(1)").is_err());
    }

    #[test]
    fn parse_extracts_userinfo_host_port() {
        let (scheme, host, port) = parse_url("http://user:pass@example.com:8080/x").unwrap();
        assert_eq!(scheme, "http");
        assert_eq!(host, "example.com");
        assert_eq!(port, 8080);
    }

    #[test]
    fn parse_defaults_ports_by_scheme() {
        let (_, _, p1) = parse_url("https://example.com/x").unwrap();
        let (_, _, p2) = parse_url("http://example.com").unwrap();
        let (_, _, p3) = parse_url("git://example.com/repo").unwrap();
        assert_eq!(p1, 443);
        assert_eq!(p2, 80);
        assert_eq!(p3, 9418);
    }

    #[test]
    fn parse_ipv6_literal() {
        let (scheme, host, port) = parse_url("http://[::1]:9000/x").unwrap();
        assert_eq!(scheme, "http");
        assert_eq!(host, "::1");
        assert_eq!(port, 9000);
    }

    #[test]
    fn parse_rejects_control_chars() {
        assert!(parse_url("http://example.com/\t evil").is_err());
        assert!(parse_url("http://exa mple.com/").is_err());
    }

    // ── webhook policy ───────────────────────────────────────────────────

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn webhook_rejects_dangerous_schemes() {
        let _g = ENV_GUARD.lock().unwrap();
        assert!(validate_webhook_url("ext::sh -c x").await.is_err());
        assert!(validate_webhook_url("file:///etc/passwd").await.is_err());
        assert!(validate_webhook_url("ftp://example.com").await.is_err());
        assert!(validate_webhook_url("javascript:alert(1)").await.is_err());
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn webhook_rejects_loopback_literals() {
        let _g = ENV_GUARD.lock().unwrap();
        assert!(validate_webhook_url("http://127.0.0.1/x").await.is_err());
        assert!(validate_webhook_url("http://127.0.0.1:8080/x").await.is_err());
        assert!(validate_webhook_url("http://[::1]/x").await.is_err());
        assert!(validate_webhook_url("http://0.0.0.0/x").await.is_err());
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn webhook_rejects_metadata_and_private_literals() {
        let _g = ENV_GUARD.lock().unwrap();
        // AWS/GCP/Azure metadata endpoints are link-local.
        assert!(
            validate_webhook_url("http://169.254.169.254/latest/meta-data/")
                .await
                .is_err()
        );
        assert!(validate_webhook_url("http://10.0.0.5/x").await.is_err());
        assert!(validate_webhook_url("http://192.168.1.1/x").await.is_err());
        assert!(validate_webhook_url("http://172.16.0.1/x").await.is_err());
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn webhook_rejects_ipv6_mapped_loopback() {
        let _g = ENV_GUARD.lock().unwrap();
        assert!(validate_webhook_url("http://[::ffff:127.0.0.1]/x").await.is_err());
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn webhook_accepts_public_literal() {
        let _g = ENV_GUARD.lock().unwrap();
        // Literal IP avoids DNS dependency in tests.
        assert!(validate_webhook_url("http://8.8.8.8/x").await.is_ok());
        assert!(validate_webhook_url("https://1.1.1.1/hook").await.is_ok());
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn webhook_fails_closed_on_unresolvable_host() {
        let _g = ENV_GUARD.lock().unwrap();
        let err = validate_webhook_url("http://definitely-not-a-real-host.invalid/x")
            .await
            .unwrap_err();
        assert!(err.contains("cannot resolve"), "{err}");
    }

    // ── mirror policy ────────────────────────────────────────────────────

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn mirror_rejects_ext_and_file_transports() {
        let _g = ENV_GUARD.lock().unwrap();
        assert!(validate_mirror_url("ext::sh -c 'evil'").await.is_err());
        assert!(
            validate_mirror_url("file:///var/lib/ironforge/repo")
                .await
                .is_err()
        );
        assert!(validate_mirror_url("ssh://git@github.com/o/r.git").await.is_err());
        assert!(validate_mirror_url("git@github.com:o/r.git").await.is_err());
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn mirror_rejects_loopback_and_metadata() {
        let _g = ENV_GUARD.lock().unwrap();
        assert!(validate_mirror_url("http://127.0.0.1/repo.git").await.is_err());
        assert!(validate_mirror_url("http://169.254.169.254/repo.git").await.is_err());
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn mirror_allows_private_for_internal_use() {
        let _g = ENV_GUARD.lock().unwrap();
        assert!(validate_mirror_url("http://10.0.0.5/repo.git").await.is_ok());
        assert!(validate_mirror_url("https://192.168.1.10/o/r.git").await.is_ok());
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn mirror_accepts_public_literal() {
        let _g = ENV_GUARD.lock().unwrap();
        assert!(validate_mirror_url("https://8.8.8.8/o/r.git").await.is_ok());
        assert!(validate_mirror_url("git://8.8.8.8/o/r.git").await.is_ok());
    }

    // ── escape hatch ─────────────────────────────────────────────────────

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn escape_hatch_allows_loopback_but_keeps_scheme_allowlist() {
        let _g = ENV_GUARD.lock().unwrap();
        std::env::set_var("IRONFORGE_ALLOW_LOCAL_OUTBOUND", "1");
        let r = validate_webhook_url("http://127.0.0.1:9930/hook").await;
        let r_ext = validate_webhook_url("ext::sh -c x").await;
        let r_file = validate_mirror_url("file:///etc/passwd").await;
        std::env::remove_var("IRONFORGE_ALLOW_LOCAL_OUTBOUND");
        assert!(r.is_ok());
        assert!(r_ext.is_err(), "scheme allowlist must survive the override");
        assert!(r_file.is_err(), "scheme allowlist must survive the override");
    }
}
