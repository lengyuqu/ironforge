//! Git Smart Protocol V2 implementation.
//!
//! Protocol V2 improves upon V1 with:
//! - Stateless-friendly design
//! - On-demand ref fetching (ls-refs command)
//! - Clearer command/capability negotiation
//! - Support for shallow clone and partial clone
//!
//! Reference: <https://git-scm.com/docs/protocol-v2>

use std::path::Path;

use anyhow::{bail, Context, Result};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, split};

use crate::pkt_line::{write_pkt_line, write_flush, read_pkt_line, PktLine};
use crate::sideband;

/// V2 Protocol constants
pub const PROTOCOL_VERSION: &str = "2";

/// V2 Capability names
pub mod caps {
    /// Agent capability - identifies server version
    pub const AGENT: &str = "agent";
    /// Object format (sha1 for now)
    pub const OBJECT_FORMAT: &str = "object-format";
    /// List refs command
    pub const LS_REFS: &str = "ls-refs";
    /// Fetch command
    pub const FETCH: &str = "fetch";
    /// Server option capability
    pub const SERVER_OPTION: &str = "server-option";
    /// Session identifier
    pub const SESSION_ID: &str = "session-id";
    /// Object info command
    pub const OBJECT_INFO: &str = "object-info";
}

/// Sideband channel constants (inherited from V1)
pub mod sideband_channel {
    pub const DATA: u8 = 1;
    pub const PROGRESS: u8 = 2;
    pub const ERROR: u8 = 3;
}

/// Handle Protocol V2 for a single bidirectional stream (SSH mode).
pub async fn handle_v2_stream<S>(repo_path: &Path, stream: &mut S) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    // We need to use two separate mutable references, so we use RefCell
    // or we can just use the same impl but duplicated for stream mode
    handle_v2_stream_impl(repo_path, stream).await
}

/// Handle Protocol V2 with separate reader/writer (HTTP mode).
pub async fn handle_v2<R, W>(repo_path: &Path, reader: R, writer: W) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    handle_v2_impl(repo_path, reader, writer).await
}

/// Internal: Protocol V2 for single bidirectional stream (SSH mode).
///
/// Uses tokio::io::split to separate the stream into read/write halves,
/// so we can use BufReader on the read half for efficient pkt-line parsing
/// while keeping the write half independent.
async fn handle_v2_stream_impl<S>(repo_path: &Path, stream: &mut S) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    // Split the bidirectional stream into independent read/write halves.
    let (read_half, mut write_half) = split(stream);

    // Send capability advertisement on the write half
    send_capability_advertisement(&mut write_half).await?;

    // BufReader on the read half for efficient pkt-line parsing.
    // We reuse the same BufReader across loop iterations to preserve its buffer.
    let mut reader = BufReader::new(read_half);

    // Command processing loop - V2 allows command multiplexing.
    // We read the command first (storing the result), then match on it,
    // so that the mutable borrow of `reader` ends before the match arms execute.
    loop {
        let command = read_command_request(&mut reader).await?;

        match command {
            CommandRequest::LsRefs {
                ref_patterns,
                peel,
                symrefs,
                unborn,
                server_options,
            } => {
                tracing::debug!(
                    patterns = ?ref_patterns,
                    peel,
                    symrefs,
                    "Processing ls-refs command (SSH V2)"
                );
                handle_ls_refs(
                    repo_path,
                    &mut write_half,
                    &ref_patterns,
                    peel,
                    symrefs,
                    unborn,
                    &server_options,
                )
                .await?;
            }
            CommandRequest::Fetch {
                wants,
                haves,
                shallows,
                deepen,
                filter,
                done,
                client_caps,
            } => {
                tracing::debug!(
                    wants = wants.len(),
                    haves = haves.len(),
                    shallows = shallows.len(),
                    done,
                    "Processing fetch command (SSH V2)"
                );
                handle_fetch(
                    repo_path,
                    &mut write_half,
                    &wants,
                    &haves,
                    &shallows,
                    deepen,
                    &filter,
                    done,
                    &client_caps,
                )
                .await?;
            }
            CommandRequest::ObjectInfo { oid, server_options } => {
                tracing::debug!(oid = %oid, "Processing object-info command (SSH V2)");
                handle_object_info(repo_path, &mut write_half, &oid, &server_options).await?;
            }
            CommandRequest::Flush => {
                // Empty flush packet signals end of commands
                tracing::debug!("Received command flush - closing connection (SSH V2)");
                break;
            }
            CommandRequest::Unknown(cmd) => {
                tracing::warn!(cmd = %cmd, "Unknown command, skipping");
                // Reuse the existing `reader` (BufReader) to skip until flush.
                // The borrow of `reader` for `read_command_request` ended
                // when that function returned, so `reader` is available here.
                skip_until_flush(&mut reader).await?;
                write_flush(&mut write_half).await?;
            }
        }
    }

    Ok(())
}

/// Internal V2 implementation.
async fn handle_v2_impl<R, W>(repo_path: &Path, reader: R, writer: W) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut reader = BufReader::new(reader);
    let mut writer = writer;

    // Send capability advertisement
    send_capability_advertisement(&mut writer).await?;

    // Command processing loop - V2 allows command multiplexing
    loop {
        // Read command request
        match read_command_request(&mut reader).await? {
            CommandRequest::LsRefs {
                ref_patterns,
                peel,
                symrefs,
                unborn,
                server_options,
            } => {
                tracing::debug!(
                    patterns = ?ref_patterns,
                    peel,
                    symrefs,
                    "Processing ls-refs command"
                );
                handle_ls_refs(
                    repo_path,
                    &mut writer,
                    &ref_patterns,
                    peel,
                    symrefs,
                    unborn,
                    &server_options,
                )
                .await?;
            }
            CommandRequest::Fetch {
                wants,
                haves,
                shallows,
                deepen,
                filter,
                done,
                client_caps,
            } => {
                tracing::debug!(
                    wants = wants.len(),
                    haves = haves.len(),
                    shallows = shallows.len(),
                    done,
                    "Processing fetch command"
                );
                handle_fetch(
                    repo_path,
                    &mut writer,
                    &wants,
                    &haves,
                    &shallows,
                    deepen,
                    &filter,
                    done,
                    &client_caps,
                )
                .await?;
            }
            CommandRequest::ObjectInfo { oid, server_options } => {
                tracing::debug!(oid = %oid, "Processing object-info command");
                handle_object_info(repo_path, &mut writer, &oid, &server_options).await?;
            }
            CommandRequest::Flush => {
                // Empty flush packet signals end of commands
                tracing::debug!("Received command flush - closing connection");
                break;
            }
            CommandRequest::Unknown(cmd) => {
                tracing::warn!(cmd = %cmd, "Unknown command, skipping");
                // Skip until flush
                skip_until_flush(&mut reader).await?;
                write_flush(&mut writer).await?;
            }
        }
    }

    Ok(())
}

/// Send the Protocol V2 capability advertisement.
/// This is the first thing sent after version negotiation.
/// Send the Protocol V2 capability advertisement.
/// This is the first thing sent after version negotiation.
pub async fn send_capability_advertisement<W: AsyncWrite + Unpin>(writer: &mut W) -> Result<()> {
    // Protocol version line
    write_pkt_line(writer, &PktLine::text("version 2")).await?;

    // Capabilities
    write_pkt_line(writer, &PktLine::text("agent=ironforge/0.1")).await?;
    write_pkt_line(writer, &PktLine::text("ls-refs")).await?;
    write_pkt_line(writer, &PktLine::text("fetch=shallow")).await?;
    write_pkt_line(writer, &PktLine::text("object-format=sha1")).await?;
    write_pkt_line(writer, &PktLine::text("server-option")).await?;

    // End of capabilities
    write_flush(writer).await?;

    tracing::debug!("Sent Protocol V2 capability advertisement");
    Ok(())
}

/// Command request types in Protocol V2
#[derive(Debug)]
pub enum CommandRequest {
    LsRefs {
        ref_patterns: Vec<String>,
        peel: bool,
        symrefs: bool,
        unborn: bool,
        server_options: Vec<String>,
    },
    Fetch {
        wants: Vec<String>,
        haves: Vec<String>,
        shallows: Vec<String>,
        deepen: Option<u32>,
        filter: Option<String>,
        done: bool,
        client_caps: Vec<String>,
    },
    ObjectInfo {
        oid: String,
        server_options: Vec<String>,
    },
    /// Empty flush packet signals end of commands
    Flush,
    /// Unknown command type
    Unknown(String),
}

/// Read a Protocol V2 command request.
/// Format:
///   command=<cmd>
///   capability=<cap>
///   ...
///   0001 (delimiter)
///   command-args...
///   0000 (flush)
async fn read_command_request<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<CommandRequest> {
    let mut command = None;
    let mut capabilities = Vec::new();
    let mut args = Vec::new();
    let mut found_delimiter = false;

    loop {
        let pkt = read_pkt_line(reader).await?;

        match pkt {
            PktLine::Flush => {
                if found_delimiter {
                    // End of request after delimiter
                    break;
                } else {
                    // Empty flush means end of commands
                    return Ok(CommandRequest::Flush);
                }
            }
            PktLine::Delim => {
                found_delimiter = true;
            }
            PktLine::ResponseEnd => {
                // End of stateless response
                return Ok(CommandRequest::Flush);
            }
            PktLine::Data(bytes) => {
                let line = String::from_utf8_lossy(&bytes);
                let line = line.trim_end_matches('\n');

                if !found_delimiter {
                    // Capability negotiation phase
                    if let Some(cmd) = line.strip_prefix("command=") {
                        command = Some(cmd.to_string());
                    } else if !line.is_empty() {
                        capabilities.push(line.to_string());
                    }
                } else {
                    // Command arguments phase
                    args.push(line.to_string());
                }
            }
        }
    }

    let cmd = match command {
        Some(c) => c,
        None => return Ok(CommandRequest::Flush),
    };

    // Parse based on command type
    match cmd.as_str() {
        "ls-refs" => {
            let mut ref_patterns = Vec::new();
            let mut peel = false;
            let mut symrefs = false;
            let mut unborn = false;
            let mut server_options = Vec::new();

            for arg in &args {
                if let Some(pattern) = arg.strip_prefix("ref-prefix ") {
                    ref_patterns.push(pattern.to_string());
                } else if *arg == "peel" {
                    peel = true;
                } else if *arg == "symrefs" {
                    symrefs = true;
                } else if *arg == "unborn" {
                    unborn = true;
                } else if let Some(opt) = arg.strip_prefix("server-option=") {
                    server_options.push(opt.to_string());
                }
            }

            Ok(CommandRequest::LsRefs {
                ref_patterns,
                peel,
                symrefs,
                unborn,
                server_options,
            })
        }
        "fetch" => {
            let mut wants = Vec::new();
            let mut haves = Vec::new();
            let mut shallows = Vec::new();
            let mut deepen = None;
            let mut filter = None;
            let mut done = false;
            let mut client_caps = Vec::new();

            for cap in &capabilities {
                if let Some(want) = cap.strip_prefix("want ") {
                    wants.push(want.to_string());
                } else if let Some(have) = cap.strip_prefix("have ") {
                    haves.push(have.to_string());
                } else if let Some(shallow) = cap.strip_prefix("shallow ") {
                    shallows.push(shallow.to_string());
                } else if let Some(d) = cap.strip_prefix("deepen ") {
                    deepen = d.parse().ok();
                } else if let Some(f) = cap.strip_prefix("filter ") {
                    filter = Some(f.to_string());
                } else if *cap == "done" {
                    done = true;
                } else if !cap.is_empty() {
                    client_caps.push(cap.to_string());
                }
            }

            Ok(CommandRequest::Fetch {
                wants,
                haves,
                shallows,
                deepen,
                filter,
                done,
                client_caps,
            })
        }
        "object-info" => {
            let mut oid = None;
            let mut server_options = Vec::new();

            for arg in &args {
                if let Some(o) = arg.strip_prefix("oid ") {
                    oid = Some(o.to_string());
                } else if let Some(opt) = arg.strip_prefix("server-option=") {
                    server_options.push(opt.to_string());
                }
            }

            match oid {
                Some(o) => Ok(CommandRequest::ObjectInfo {
                    oid: o,
                    server_options,
                }),
                None => Ok(CommandRequest::Unknown(cmd)),
            }
        }
        _ => Ok(CommandRequest::Unknown(cmd)),
    }
}

/// Skip packets until flush (for unknown commands).
///
/// Accepts any `AsyncRead + Unpin` directly.
async fn skip_until_flush<R: AsyncRead + Unpin>(reader: &mut R) -> Result<()> {
    loop {
        let pkt = read_pkt_line(reader).await?;
        if matches!(pkt, PktLine::Flush) {
            break;
        }
    }
    Ok(())
}

/// Handle the ls-refs command.
/// Sends ref advertisements based on client request.
async fn handle_ls_refs<W: AsyncWrite + Unpin>(
    repo_path: &Path,
    writer: &mut W,
    _ref_patterns: &[String],
    peel: bool,
    symrefs: bool,
    unborn: bool,
    _server_options: &[String],
) -> Result<()> {
    // Get all refs
    let refs = list_refs(repo_path)?;
    let head_sha = crate::resolve_head_sha(repo_path);

    // Handle unborn HEAD
    if unborn {
        // If HEAD points to an unborn branch, we could send that info
        // For simplicity, we just skip it if HEAD doesn't resolve
    }

    // Build ref advertisement
    for (i, (sha, refname)) in refs.iter().enumerate() {
        let mut line = format!("{} {}", sha, refname);

        // Add symref if requested (simplified - just show direct refs)
        if symrefs && refname.starts_with("HEAD") {
            // In real implementation, we'd look up the target
        }

        // Add peel info for tags if requested
        if peel && refname.starts_with("refs/tags/") {
            if let Some(peel_sha) = get_tag_peel(repo_path, sha) {
                line.push_str(&format!(" {}", peel_sha));
            }
        }

        // First ref can include capabilities (but ls-refs is simpler)
        if i == 0 && refs.len() == 1 && head_sha.is_none() {
            // Single ref in empty repo
        }

        write_pkt_line(writer, &PktLine::text(&line)).await?;
    }

    // Also send HEAD if we have it
    if let Some(sha) = &head_sha {
        write_pkt_line(writer, &PktLine::text(&format!("{} HEAD", sha))).await?;
    }

    // End of refs
    write_flush(writer).await?;

    tracing::debug!(refs = refs.len(), "Sent ls-refs response");
    Ok(())
}

/// Handle the fetch command.
/// Negotiates common commits and sends packfile.
async fn handle_fetch<W: AsyncWrite + Unpin>(
    repo_path: &Path,
    writer: &mut W,
    wants: &[String],
    haves: &[String],
    _shallows: &[String],
    _deepen: Option<u32>,
    _filter: &Option<String>,
    done: bool,
    client_caps: &[String],
) -> Result<()> {
    use sideband::{write_sideband_data, write_sideband_flush, write_sideband_progress};

    let use_sideband = client_caps.iter().any(|c| c.contains("side-band"));

    if wants.is_empty() {
        write_flush(writer).await?;
        return Ok(());
    }

    // Simple negotiation: if client sent "done", we just send packfile
    // For more complex negotiation, we'd compare haves/wants
    if done || !haves.is_empty() {
        // Client is ready for packfile
        write_pkt_line(writer, &PktLine::data(b"ACK\n")).await?;
    } else {
        // Client wants more negotiation
        write_pkt_line(writer, &PktLine::data(b"NAK")).await?;
    }

    // Send packfile
    let pack_data = generate_packfile(repo_path, wants).await?;

    if use_sideband {
        // Send progress message
        write_sideband_progress(
            writer,
            &format!("counting {} objects\n", wants.len()),
        )
        .await?;

        // Send packfile through sideband
        write_sideband_data(writer, &pack_data).await?;

        // Send done progress
        write_sideband_progress(writer, "Done.\n").await?;

        // End sideband
        write_sideband_flush(writer).await?;
    } else {
        writer.write_all(&pack_data).await?;
        writer.flush().await?;
    }

    tracing::info!(pack_size = pack_data.len(), objects = wants.len(), "Sent V2 fetch packfile");
    Ok(())
}

/// Handle the object-info command.
async fn handle_object_info<W: AsyncWrite + Unpin>(
    repo_path: &Path,
    writer: &mut W,
    oid: &str,
    _server_options: &[String],
) -> Result<()> {
    // Get object size
    let size = get_object_size(repo_path, oid)?;

    write_pkt_line(writer, &PktLine::text("size")).await?;
    write_pkt_line(writer, &PktLine::text(&format!("{} {}", oid, size))).await?;
    write_flush(writer).await?;

    Ok(())
}

// ─── Git Operations ───────────────────────────────────────────────────────────

/// List refs using gix API.
///
/// CRITICAL: HEAD reference handling (踩坑经验 #5)
///
/// `git for-each-ref` does NOT list HEAD by default.
/// gix `repo.references().all()` correctly returns ALL references including HEAD.
/// See CLAUDE.md "常见错误排查" for details.
///
/// CRITICAL: HEAD reference handling (踩坑经验 #5)
///
/// The `git for-each-ref` command does NOT list HEAD by default.
/// This is a common gotcha when migrating from git CLI to gix API.
///
/// For example:
///   $ git for-each-ref refs/heads/
///   Will NOT include HEAD even if HEAD points to refs/heads/main
///
/// Our solution: Use gix API (`repo.references().all()`) which correctly
/// returns ALL references including HEAD. We then handle symbolic refs
/// (like HEAD) separately by resolving them to their target object ID.
///
/// This approach is more reliable than shelling out to `git for-each-ref`
/// and avoids the HEAD-missing bug.
fn list_refs(repo_path: &Path) -> Result<Vec<(String, String)>> {
    let repo = gix::open(repo_path).context("failed to open repository")?;
    let mut refs = Vec::new();

    let references = repo.references().context("failed to list references")?;
    let all_refs = references.all()?;

    for reference in all_refs {
        let reference = match reference {
            Ok(r) => r,
            Err(_) => continue,
        };
        let refname = reference.name().as_bstr().to_string();
        let target = reference.target();

        match target {
            gix::refs::TargetRef::Object(id) => {
                refs.push((id.to_string(), refname));
            }
            gix::refs::TargetRef::Symbolic(_) => {
                // For symbolic refs like HEAD, try to resolve to the actual object
                if refname == "HEAD" {
                    if let Ok(head_id) = repo.head_id() {
                        refs.push((head_id.to_string(), refname));
                    }
                }
            }
        }
    }

    Ok(refs)
}

/// Get the peel (dereferenced) SHA of a tag using gix API.
fn get_tag_peel(repo_path: &Path, sha: &str) -> Option<String> {
    let repo = gix::open(repo_path).ok()?;
    let object_id = gix::ObjectId::from_hex(sha.as_bytes()).ok()?;

    // Find the object
    let object = repo.find_object(object_id).ok()?;

    // Check if it's a tag and get the peeled object
    if let Ok(tag) = object.try_into_tag() {
        // The tag points to another object - that's the peeled SHA
        let target_id = tag.target_id().ok()?;
        return Some(target_id.to_string());
    }

    // Not a tag or can't peel, return the original SHA
    Some(sha.to_string())
}

/// Get the size of a git object using gix API.
fn get_object_size(repo_path: &Path, oid: &str) -> Result<u64> {
    let repo = gix::open(repo_path).context("failed to open repository")?;
    let object_id = gix::ObjectId::from_hex(oid.as_bytes())
        .map_err(|e| anyhow::anyhow!("invalid object ID: {}", e))?;

    let object = repo
        .find_object(object_id)
        .map_err(|_| anyhow::anyhow!("object {} not found", oid))?;

    // Get the size of the object data
    let size = object.data.len() as u64;
    Ok(size)
}

/// Generate a packfile for the given wants (async version).
/// TODO(gix): Replace with gix pack generation when available.
/// Currently using git pack-objects CLI as gix doesn't have a direct replacement.
async fn generate_packfile(repo_path: &Path, _wants: &[String]) -> Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    use tokio::process::Command;
    use std::process::Stdio;

    let mut cmd = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["pack-objects", "--all", "--stdout"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to spawn git pack-objects")?;

    let stdout = cmd.stdout.take().context("no stdout")?;
    let mut reader = BufReader::new(stdout);
    let mut pack_data = Vec::new();
    reader.read_to_end(&mut pack_data).await.context("failed to read packfile")?;

    let status = cmd.wait().await.context("git pack-objects failed")?;
    if !status.success() {
        bail!("git pack-objects failed");
    }

    Ok(pack_data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_advertisement_format() {
        // Just verify constants are defined correctly
        assert_eq!(PROTOCOL_VERSION, "2");
        assert_eq!(caps::LS_REFS, "ls-refs");
        assert_eq!(caps::FETCH, "fetch");
    }
}
