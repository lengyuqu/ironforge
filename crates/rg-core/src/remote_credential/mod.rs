//! Remote credential dictionary — tokens for *other* Git hosts, keyed by host.
//!
//! A mirror of a private upstream, or a push to one, has to authenticate as the
//! end user against a platform this instance does not control. Rather than
//! asking for the token inside each mirror dialog (and storing it per mirror,
//! where it cannot be reused), the token is configured once per host and looked
//! up automatically from the remote URL:
//!
//! ```text
//! https://github.com/acme/private.git
//!         └──── host key "github.com" ────► remote_credentials row
//! ```
//!
//! # Scope
//!
//! Credentials belong to a user or an organization. A repository resolves to
//! exactly one scope — its org's when it has one, its owner's otherwise — and
//! there is no fallback in either direction. A shared org repository must not
//! silently use whichever member's personal token happens to be configured;
//! that would make an org mirror work or fail depending on who runs the sync.
//!
//! # What is never done
//!
//! The token is never put in the remote URL (`https://user:token@host/...`):
//! that spelling is written verbatim into `.git/config` as
//! `remote.origin.url`, where it outlives the process and leaks through any
//! clone of that mirror. It is also never passed as a `-c` argument, which is
//! visible in `ps` to every local user. See
//! [`service::git_auth_env`] for the mechanism actually used.
pub mod service;
