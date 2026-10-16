//! `rg-git` — Git Smart Protocol implementation for IronForge.
//!
//! This crate handles the server side of Git's smart HTTP and SSH protocols.
//! It speaks pkt-line framing, sideband multiplexing, and implements both
//! Protocol V1 (receive-pack / upload-pack) and Protocol V2 (ls-refs, fetch,
//! object-info).
//!
//! # Architecture
//!
//! - **[`pkt_line`]** — low-level Git pkt-line framing (read/write flush, delim,
//!   data packets).  All Git protocol messages are framed in 4-hex + body
//!   format with `0000` as flush.
//! - **[`sideband`]** — Git sideband (band 1 data / band 2 progress / band 3
//!   error) for multiplexing pack data and status messages.
//! - **[`protocol`]** — server-side protocol handlers:
//!   - `receive_pack` — handles `git push` (receive pack + thin pack indexing)
//!   - `upload_pack` — handles `git fetch/clone` (ref advertisement + pack
//!     generation)
//!   - `v2` — Protocol V2 handlers (ls-refs, fetch command, object-info)
//!
//! # Key design decisions
//!
//! - **Bare repositories only.**  IronForge repos live as `{owner}/{repo}.git`
//!   bare repos on disk.
//! - **gix-native core.**  Every protocol path is built on `gix`: ref
//!   traversal, object reading, reference updates, pack generation, thin-pack
//!   indexing, and Protocol V2 packfile streaming in both HTTP and SSH modes.
//!   No `git` CLI is involved in these paths.
//! - **Thin pack handling.**  Clients may send thin (delta-only) packs; they
//!   MUST be completed with `--fix-thin` before the ref is updated.
//!
//! # Usage
//!
//! ```rust,ignore
//! use rg_git::protocol;
//!
//! // Resolve refs for upload-pack advertisement
//! let refs = protocol::upload_pack::list_refs(repo_path)?;
//!
//! // Handle a receive-pack push
//! let updates = protocol::receive_pack::process_pack(repo_path, pack_data)?;
//! ```
//!
//! # Caveats
//!
//! - **Protocol V2** is complete in both HTTP and SSH modes:
//!   `handle_v2_stream_impl` sends the capability advertisement and runs the
//!   V2 command loop (ls-refs / fetch) over a single bidirectional stream.
//! - **`git` CLI usage.**  Within `rg-git`, raw git process spawning is
//!   confined to [`cli_gateway`] and used only by tests (a regression guard
//!   enforces this); all protocol paths are gix-native.  The only production
//!   git-CLI calls live outside this crate — mirror sync (`rg-core` mirror
//!   module), import bare clone, and the CI worktree.

use std::path::Path;

pub mod cli_gateway;
pub mod ops;
pub mod pkt_line;
pub mod protocol;
pub mod sideband;

/// Resolve HEAD to a SHA, or return None if HEAD doesn't point to a valid commit.
pub(crate) fn resolve_head_sha(repo_path: &Path) -> Option<String> {
    let repo = gix::open(repo_path).ok()?;
    let head_id = repo.head_id().ok()?;
    Some(head_id.to_string())
}
