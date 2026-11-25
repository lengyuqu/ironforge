//! Git upload-pack protocol implementation (git clone/fetch).
//!
//! Supports two modes:
//! 1. Split reader/writer (HTTP mode) — via `handle_upload_pack`
//! 2. Single bidirectional stream (SSH mode) — via `handle_upload_pack_stream`

use anyhow::{Context, Result};
use std::io::{self, Write};
use std::path::Path;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tracing;

use crate::pkt_line::{read_pkt_line, write_flush, write_pkt_line, PktLine};
use crate::sideband;

/// Bytes handed from the pack encoder to the socket in one go.
///
/// One band byte plus this payload must fit a pkt-line, so it is
/// `MAX_PKT_LINE_LEN - 1` — the same chunk size `write_sideband_data` uses,
/// which keeps the framing on the wire identical to the buffered path.
const PACK_CHUNK: usize = crate::pkt_line::MAX_PKT_LINE_LEN - 1;

/// How many `PACK_CHUNK`s may sit between the encoder and the socket.
///
/// This is the whole memory budget of a streamed pack (4 × 64 KiB): a slow
/// client fills the queue and the producer parks inside `blocking_send`, so the
/// transfer is backpressured instead of buffered. Never make this unbounded.
const PACK_QUEUE_DEPTH: usize = 4;

/// [`std::io::Write`] that hands fixed-size chunks to the async side.
///
/// Used inside `spawn_blocking`, so `blocking_send` is allowed (it panics only
/// on async worker threads).
struct PackSink {
    sender: tokio::sync::mpsc::Sender<Vec<u8>>,
    buffer: Vec<u8>,
}

impl PackSink {
    fn new(sender: tokio::sync::mpsc::Sender<Vec<u8>>) -> Self {
        Self {
            sender,
            buffer: Vec::with_capacity(PACK_CHUNK),
        }
    }

    fn hand_off(&mut self) -> io::Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        let chunk = std::mem::replace(&mut self.buffer, Vec::with_capacity(PACK_CHUNK));
        self.sender.blocking_send(chunk).map_err(|_| {
            // The response stream is gone (client hung up, request cancelled).
            // Failing here is what stops pack generation early instead of
            // computing a pack nobody will read.
            io::Error::new(io::ErrorKind::BrokenPipe, "pack stream consumer is gone")
        })
    }
}

impl Write for PackSink {
    fn write(&mut self, mut data: &[u8]) -> io::Result<usize> {
        let total = data.len();
        while !data.is_empty() {
            let space = PACK_CHUNK - self.buffer.len();
            let take = space.min(data.len());
            self.buffer.extend_from_slice(&data[..take]);
            data = &data[take..];
            if self.buffer.len() == PACK_CHUNK {
                self.hand_off()?;
            }
        }
        Ok(total)
    }

    /// gix flushes once the pack trailer is written; that is when the last
    /// partial chunk leaves.
    fn flush(&mut self) -> io::Result<()> {
        self.hand_off()
    }
}

/// Run `generate` on a blocking thread and stream its output to `writer`.
///
/// `generate` is the pack pipeline: it gets a [`Write`] and must write the
/// pack into it. Chunks cross a bounded channel, so peak memory for the pack is
/// `PACK_QUEUE_DEPTH * PACK_CHUNK` regardless of repository size (T38: a large
/// clone used to hold the whole 1.2 GB pack in a `Vec`).
///
/// With `sideband` each chunk is wrapped in a band-1 pkt-line; without it the
/// raw pack is copied out (V1 clients that did not request sideband).
///
/// Returns the pack byte count. The producer's `JoinHandle` is always awaited:
/// a panic there is reported as an error instead of surfacing to the client as
/// a bare "early EOF" with nothing in the logs.
pub(crate) async fn stream_pack<W, F>(writer: &mut W, sideband: bool, generate: F) -> Result<u64>
where
    W: AsyncWrite + Unpin,
    F: FnOnce(&mut dyn Write) -> Result<()> + Send + 'static,
{
    let (sender, mut receiver) = tokio::sync::mpsc::channel::<Vec<u8>>(PACK_QUEUE_DEPTH);
    let producer = tokio::task::spawn_blocking(move || -> Result<()> {
        let mut sink = PackSink::new(sender);
        generate(&mut sink)?;
        sink.flush()?;
        Ok(())
    });

    let pumped = pump_pack_chunks(&mut receiver, writer, sideband).await;
    // Drop the receiver before awaiting the producer: if the pump stopped early
    // the encoder must see a closed channel, not block on a full one forever.
    drop(receiver);
    let joined = producer.await;

    match (pumped, joined) {
        (Ok(bytes), Ok(Ok(()))) => Ok(bytes),
        (Ok(_), Ok(Err(error))) => Err(error),
        (Ok(_), Err(join_error)) => {
            Err(anyhow::anyhow!("pack generation task failed: {join_error}"))
        }
        // The client is gone; that is the error worth reporting. Keep the
        // producer's own failure in the log so a real object-store problem is
        // not hidden behind a broken pipe.
        (Err(pump_error), Ok(Err(producer_error))) => {
            tracing::warn!(%producer_error, "pack generation failed after the response stream broke");
            Err(pump_error)
        }
        (Err(pump_error), Ok(Ok(()))) => Err(pump_error),
        (Err(pump_error), Err(join_error)) => {
            tracing::warn!(%join_error, "pack generation task ended abnormally after the response stream broke");
            Err(pump_error)
        }
    }
}

/// Move pack chunks from the encoder to `writer`, framing them when needed.
async fn pump_pack_chunks<W: AsyncWrite + Unpin>(
    receiver: &mut tokio::sync::mpsc::Receiver<Vec<u8>>,
    writer: &mut W,
    sideband: bool,
) -> Result<u64> {
    let mut written = 0u64;
    while let Some(chunk) = receiver.recv().await {
        written += chunk.len() as u64;
        if sideband {
            let mut payload = Vec::with_capacity(chunk.len() + 1);
            payload.push(1u8); // band 1: pack data
            payload.extend_from_slice(&chunk);
            write_pkt_line(writer, &PktLine::Data(payload)).await?;
        } else {
            writer.write_all(&chunk).await?;
        }
    }
    Ok(written)
}

/// Handle upload-pack with separate reader and writer (HTTP mode).
/// This sends the ref advertisement, negotiates, and sends the packfile.
pub async fn handle_upload_pack<R, W>(repo_path: &Path, reader: R, writer: W) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut reader = reader;
    let mut writer = writer;
    upload_pack_refs_and_negotiate(repo_path, &mut reader, &mut writer).await
}

/// Handle upload-pack with a single bidirectional stream (SSH mode).
/// Takes a mutable reference so the caller can send exit-status before dropping the stream.
pub async fn handle_upload_pack_stream<S>(repo_path: &Path, stream: &mut S) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    upload_pack_stream_impl(repo_path, stream).await
}

/// Handle upload-pack for HTTP mode where ref advertisement is already sent.
pub async fn handle_upload_pack_http<R, W>(repo_path: &Path, reader: R, writer: W) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut reader = BufReader::new(reader);
    let mut writer = writer;

    // Read client request and negotiate
    let (wants, haves, client_caps) = read_want_have_split(&mut reader).await?;

    if wants.is_empty() {
        write_flush(&mut writer).await?;
        return Ok(());
    }

    // Send NAK
    write_pkt_line(&mut writer, &PktLine::data(b"NAK")).await?;

    // Send packfile
    let use_sideband = client_caps.contains(&"side-band-64k".to_string())
        || client_caps.contains(&"side-band".to_string());
    send_packfile(repo_path, &wants, &haves, &mut writer, use_sideband).await
}

/// Internal: send ref advertisement and negotiate with separate reader/writer.
async fn upload_pack_refs_and_negotiate<R, W>(
    repo_path: &Path,
    reader: &mut R,
    writer: &mut W,
) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let refs = list_refs(repo_path)?;
    let head_sha = crate::resolve_head_sha(repo_path);
    let ref_list = build_ref_advertisement_vec(refs, head_sha);

    // Send ref advertisement
    let ad = build_ref_advertisement(&ref_list, "git-upload-pack");
    for pkt in &ad {
        write_pkt_line(writer, pkt).await?;
    }
    write_flush(writer).await?;

    // Read client negotiation
    let (wants, haves, client_caps) = read_want_have_split(&mut BufReader::new(reader)).await?;

    if wants.is_empty() {
        write_flush(writer).await?;
        return Ok(());
    }

    // Send NAK
    write_pkt_line(writer, &PktLine::data(b"NAK")).await?;

    // Send packfile
    let use_sideband = client_caps.contains(&"side-band-64k".to_string())
        || client_caps.contains(&"side-band".to_string());
    send_packfile(repo_path, &wants, &haves, writer, use_sideband).await
}

/// Internal: SSH mode implementation with single stream type.
async fn upload_pack_stream_impl<S>(repo_path: &Path, stream: &mut S) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let refs = list_refs(repo_path)?;
    let head_sha = crate::resolve_head_sha(repo_path);
    let ref_list = build_ref_advertisement_vec(refs, head_sha);

    // Send ref advertisement
    let ad = build_ref_advertisement(&ref_list, "git-upload-pack");
    for pkt in &ad {
        write_pkt_line(stream, pkt).await?;
    }
    write_flush(stream).await?;

    // Negotiation + packfile (single stream type)
    negotiate_and_send_pack_single(repo_path, stream).await
}

/// Internal: negotiate and send pack with single stream type (SSH mode).
async fn negotiate_and_send_pack_single<S>(repo_path: &Path, stream: &mut S) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (wants, haves, client_caps) = read_want_have_stream(stream).await?;

    let use_sideband = client_caps.contains(&"side-band-64k".to_string())
        || client_caps.contains(&"side-band".to_string());

    if wants.is_empty() {
        stream.flush().await?;
        return Ok(());
    }

    write_pkt_line(stream, &PktLine::data(b"NAK")).await?;
    send_packfile(repo_path, &wants, &haves, stream, use_sideband).await
}

/// Read want/have lines from separate reader (HTTP mode).
async fn read_want_have_split<R: AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
) -> Result<(Vec<String>, Vec<String>, Vec<String>)> {
    read_want_have_impl(reader).await
}

/// Read want/have lines from single stream (SSH mode).
/// Wraps stream in a BufReader temporarily, then passes it straight to impl.
async fn read_want_have_stream<S: AsyncRead + Unpin>(
    stream: &mut S,
) -> Result<(Vec<String>, Vec<String>, Vec<String>)> {
    // Safety: BufReader here only pre-reads the negotiation phase.
    // After returning, any remaining bytes in the BufReader internal buffer
    // would be lost, but for pkt-line protocol each read_pkt_line consumes
    // exactly the announced bytes, so there should be no unconsumed buffered data.
    let mut reader = BufReader::new(stream);
    read_want_have_impl(&mut reader).await
}

/// Internal: parse want/have negotiation from a BufReader using proper pkt-line parsing.
///
/// Each pkt-line on the wire is:
///   `<4-hex-length><payload>`
/// where the 4-byte length includes itself. `read_pkt_line` handles this and
/// returns only the payload bytes (or `PktLine::Flush` for "0000").
async fn read_want_have_impl<R: AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
) -> Result<(Vec<String>, Vec<String>, Vec<String>)> {
    let mut wants = Vec::new();
    let mut haves = Vec::new();
    let mut capabilities = Vec::new();

    loop {
        let pkt = read_pkt_line(reader).await?;

        // Flush packet ("0000") or EOF → end of negotiation
        // Delim/ResponseEnd are V2-only and shouldn't appear in V1 protocol
        let raw = match pkt {
            PktLine::Flush => break,
            PktLine::Data(bytes) => bytes,
            PktLine::Delim | PktLine::ResponseEnd => continue, // Skip in V1 context
        };

        // Convert bytes to string (pkt-line payload, no length prefix)
        let line = String::from_utf8_lossy(&raw);
        let line = line.trim_end_matches('\n');

        if line.is_empty() {
            continue;
        }

        // Git want/have lines come in two forms:
        //
        //   Form A (first want line, v1 protocol):
        //     `want <sha1>\0<cap1> <cap2> ...`
        //     NUL separates sha+command from capability list.
        //
        //   Form B (git client sends capabilities space-separated after the SHA,
        //     without a NUL, when the server did NOT advertise them with NUL):
        //     `want <sha1> <cap1> <cap2> ...`
        //
        // In practice the macOS git client sends Form B (space-separated after sha).
        // We handle both by first checking for NUL, then splitting on the second space
        // for commands that start with "want " or "have ".

        let (command, caps_part): (String, Option<&str>) = if line.contains('\0') {
            // Form A: NUL-separated capabilities
            let mut parts = line.splitn(2, '\0');
            let cmd = parts.next().unwrap_or("");
            let caps = parts.next().unwrap_or("");
            (
                cmd.to_string(),
                if caps.is_empty() { None } else { Some(caps) },
            )
        } else if let Some(after_want) = line.strip_prefix("want ") {
            // Form B: `want <sha1> [cap1 cap2 ...]` — space after sha1
            if let Some((sha, caps)) = after_want.split_once(' ') {
                let command = format!("want {sha}");
                (command, if caps.is_empty() { None } else { Some(caps) })
            } else {
                (line.to_string(), None)
            }
        } else {
            (line.to_string(), None)
        };

        if let Some(caps) = caps_part {
            // Parse space-separated capabilities
            capabilities = caps
                .split([' ', '\0'])
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty())
                .collect();
            tracing::debug!(caps = ?capabilities, "Parsed client capabilities");
        }

        if let Some(sha) = command.strip_prefix("want ") {
            let sha = sha.trim().to_string();
            tracing::debug!(sha = %sha, "Client wants");
            wants.push(sha);
        } else if let Some(sha) = command.strip_prefix("have ") {
            let sha = sha.trim().to_string();
            haves.push(sha);
        } else if command == "done" {
            break;
        } else {
            tracing::debug!(line = %command, "Unknown want/have line, ignoring");
        }
    }

    tracing::info!(
        wants = wants.len(),
        haves = haves.len(),
        caps = capabilities.len(),
        "Want/have negotiation complete"
    );

    Ok((wants, haves, capabilities))
}

/// List refs in a bare git repository using gix API.
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

/// Build ref advertisement from ref list.
fn build_ref_advertisement_vec(
    refs: Vec<(String, String)>,
    head_sha: Option<String>,
) -> Vec<(String, String)> {
    let mut ref_list = Vec::new();

    // Add HEAD first if we have it
    if let Some(sha) = &head_sha {
        ref_list.push((sha.clone(), "HEAD".to_string()));
    }

    // Add all refs
    for (sha, refname) in refs {
        ref_list.push((sha, refname));
    }

    ref_list
}

/// Capabilities advertised in the V1 ref advertisement.
///
/// The `agent` string is taken from [`v2::AGENT_CAPABILITY`] so HTTP, SSH and
/// V2 all report the same version.
fn capabilities_v1() -> String {
    format!(
        "side-band-64k ofs-delta {}",
        crate::protocol::v2::AGENT_CAPABILITY
    )
}

/// Build ref advertisement pkt-lines.
fn build_ref_advertisement(ref_list: &[(String, String)], service: &str) -> Vec<PktLine> {
    let mut lines = Vec::new();

    // First line includes service announcement and capabilities
    if let Some((sha, refname)) = ref_list.first() {
        // Capabilities: advertise only what we implement.
        // - side-band-64k: packfile in sideband channel 1, messages in channel 2
        // - ofs-delta: server can send OFS_DELTA objects (smaller packs)
        // - agent: server identification
        // NOTE: We do NOT advertise multi_ack / multi_ack_detailed / no-done because
        // our negotiation loop only handles the simple NAK→packfile flow.
        let caps = capabilities_v1();
        let line = format!("{} {}\0{}", sha, refname, caps);
        lines.push(PktLine::Data(line.into_bytes()));
    } else {
        // Empty repo — still need capabilities
        let caps = capabilities_v1();
        let line = format!(
            "0000000000000000000000000000000000000000 capabilities^{}\0{}",
            service, caps
        );
        lines.push(PktLine::Data(line.into_bytes()));
    }

    // Remaining refs
    for (sha, refname) in ref_list.iter().skip(1) {
        let line = format!("{} {}", sha, refname);
        lines.push(PktLine::Data(line.into_bytes()));
    }

    lines
}

/// Generate and send the packfile for the negotiated want/have set.
///
/// This is the V1 counterpart of `git pack-objects --revs --stdout`: the pack
/// contains everything reachable from `wants` minus everything reachable from
/// `haves`. It previously called `pack_universe`, which ignored `haves` and
/// shipped every object in the repository on **every** fetch — correct output,
/// but an incremental fetch transferred the whole history instead of only the
/// new commits.
///
/// Falls back to `pack_universe` only when the client sent no `want` lines
/// (the negotiation did not narrow the request).
///
/// The pack is generated on a blocking thread and streamed chunk by chunk
/// (T38); it is never collected into a `Vec`, so the response no longer costs
/// one byte of RAM per byte of pack.
async fn send_packfile<W: AsyncWrite + Unpin>(
    repo_path: &Path,
    wants: &[String],
    haves: &[String],
    writer: &mut W,
    use_sideband: bool,
) -> Result<()> {
    let repo_dir = repo_path.to_path_buf();
    let wants_owned = wants.to_vec();
    let haves_owned = haves.to_vec();
    let pack_size = stream_pack(writer, use_sideband, move |out| {
        if wants_owned.is_empty() {
            crate::ops::pack_universe_into(&repo_dir, out)?;
            return Ok(());
        }
        // V1 has no shallow/partial-clone negotiation here, so both are empty.
        crate::ops::pack_for_wants_into(&repo_dir, &wants_owned, &haves_owned, &[], out)?;
        Ok(())
    })
    .await?;

    tracing::info!(
        pack_size,
        wants = wants.len(),
        haves = haves.len(),
        "Packfile generated and streamed"
    );

    if use_sideband {
        // Send "Done." progress message (band 2)
        sideband::write_sideband_progress(writer, "Done.\n").await?;

        // Send flush to end sideband
        sideband::write_sideband_flush(writer).await?;
    } else {
        // Send raw packfile without sideband
        writer.flush().await?;
    }

    tracing::info!(pack_size, objects = wants.len(), "Upload-pack complete");

    Ok(())
}

#[cfg(test)]
mod haves_tests {

    /// Count objects in a pack stream by parsing its header.
    fn pack_object_count(pack: &[u8]) -> u32 {
        assert!(pack.starts_with(b"PACK"), "not a pack stream");
        // 4-byte signature, 4-byte version, then a big-endian object count.
        u32::from_be_bytes([pack[8], pack[9], pack[10], pack[11]])
    }

    fn git(args: &[&str], cwd: Option<&std::path::Path>) -> String {
        let gateway = crate::cli_gateway::global_gateway().as_ref().unwrap();
        let output = gateway.run(args, cwd).unwrap();
        output.ensure_success().unwrap();
        output.stdout_str().trim().to_string()
    }

    /// Regression: the V1 handler ignored `haves` and called `pack_universe`, so
    /// every fetch shipped the entire object database. An incremental fetch must
    /// send only the objects the client does not already have.
    #[tokio::test]
    async fn v1_fetch_prunes_haves_instead_of_sending_everything() {
        let temp = tempfile::tempdir().unwrap();
        let bare = temp.path().join("bare.git");
        let bare_arg = bare.to_string_lossy().to_string();
        git(
            &["init", "--bare", "--initial-branch=main", &bare_arg],
            None,
        );

        let work = tempfile::tempdir().unwrap();
        let path = work.path();
        git(
            &["init", "--initial-branch=main", &path.to_string_lossy()],
            None,
        );
        git(&["config", "user.name", "Haves Test"], Some(path));
        git(&["config", "user.email", "haves@example.com"], Some(path));
        git(&["remote", "add", "origin", &bare_arg], Some(path));

        std::fs::write(path.join("a.txt"), "a\n").unwrap();
        git(&["add", "."], Some(path));
        git(&["commit", "-m", "a"], Some(path));
        git(&["push", "origin", "main"], Some(path));
        let old_tip = git(&["rev-parse", "HEAD"], Some(path));

        std::fs::write(path.join("b.txt"), "b\n").unwrap();
        git(&["add", "."], Some(path));
        git(&["commit", "-m", "b"], Some(path));
        git(&["push", "origin", "main"], Some(path));
        let new_tip = git(&["rev-parse", "HEAD"], Some(path));

        // Full pack (client has nothing) vs incremental (client has `old_tip`).
        let full =
            crate::ops::pack_for_wants(&bare, std::slice::from_ref(&new_tip), &[], &[]).unwrap();
        let incremental = crate::ops::pack_for_wants(
            &bare,
            std::slice::from_ref(&new_tip),
            std::slice::from_ref(&old_tip),
            &[],
        )
        .unwrap();

        let full_count = pack_object_count(&full);
        let incremental_count = pack_object_count(&incremental);
        assert!(
            incremental_count < full_count,
            "incremental fetch must send fewer objects than a full fetch \
             ({incremental_count} vs {full_count})"
        );
        // The incremental pack still carries the new commit, its tree and blob.
        assert!(incremental_count >= 3, "got {incremental_count} objects");
    }

    /// The full pack still contains every object, and the incremental one is
    /// non-empty (it must include the new commit and its tree/blob).
    #[test]
    fn pack_object_counts_are_sane() {
        let temp = tempfile::tempdir().unwrap();
        let bare = temp.path().join("counts.git");
        git(&["init", "--bare", &bare.to_string_lossy()], None);
        let work = tempfile::tempdir().unwrap();
        let path = work.path();
        git(&["init", &path.to_string_lossy()], None);
        git(&["config", "user.name", "Counts"], Some(path));
        git(&["config", "user.email", "counts@example.com"], Some(path));
        git(
            &["remote", "add", "origin", &bare.to_string_lossy()],
            Some(path),
        );
        std::fs::write(path.join("f.txt"), "1\n").unwrap();
        git(&["add", "."], Some(path));
        git(&["commit", "-m", "1"], Some(path));
        git(&["push", "origin", "HEAD:main"], Some(path));
        let sha = git(&["rev-parse", "HEAD"], Some(path));
        let pack = crate::ops::pack_for_wants(&bare, &[sha], &[], &[]).unwrap();
        assert!(pack_object_count(&pack) >= 3, "commit + tree + blob");
    }
}

/// T38: the streamed pack must produce byte-for-byte the same wire form as the
/// buffered writer it replaces. Framing is where a streaming rewrite silently
/// breaks a client, so it is pinned here without involving a pack at all.
#[cfg(test)]
mod streaming_tests {
    use super::*;
    use crate::sideband::write_sideband_data;

    fn payload(len: usize) -> Vec<u8> {
        (0..len).map(|index| (index % 251) as u8).collect()
    }

    /// Write `source` in deliberately awkward pieces, so the sink's buffering is
    /// what decides the pkt-line boundaries.
    fn write_in_irregular_chunks(out: &mut dyn Write, source: &[u8]) -> Result<()> {
        const SIZES: [usize; 5] = [7, 4096, PACK_CHUNK, 1, 999];
        let mut offset = 0;
        let mut index = 0;
        while offset < source.len() {
            let size = SIZES[index % SIZES.len()].min(source.len() - offset);
            out.write_all(&source[offset..offset + size])?;
            offset += size;
            index += 1;
        }
        Ok(())
    }

    #[tokio::test]
    async fn sideband_framing_matches_the_buffered_writer() {
        // Three full chunks plus a tail: covers the boundary cases.
        let data = payload(PACK_CHUNK * 3 + 1234);

        let mut buffered = Vec::new();
        write_sideband_data(&mut buffered, &data).await.unwrap();

        let mut streamed = Vec::new();
        let source = data.clone();
        let written = stream_pack(&mut streamed, true, move |out| {
            write_in_irregular_chunks(out, &source)
        })
        .await
        .unwrap();

        assert_eq!(written, data.len() as u64);
        assert_eq!(
            streamed, buffered,
            "streamed sideband framing differs from write_sideband_data"
        );
    }

    #[tokio::test]
    async fn raw_stream_is_the_pack_verbatim() {
        let data = payload(PACK_CHUNK + 17);
        let mut streamed = Vec::new();
        let source = data.clone();
        let written = stream_pack(&mut streamed, false, move |out| {
            write_in_irregular_chunks(out, &source)
        })
        .await
        .unwrap();

        assert_eq!(written, data.len() as u64);
        assert_eq!(
            streamed, data,
            "the non-sideband stream must be the raw pack"
        );
    }

    /// A generator that fails must be reported, and the error must be the
    /// generator's, not a framing artefact.
    #[tokio::test]
    async fn generator_failure_is_reported() {
        let mut streamed = Vec::new();
        let error = stream_pack(&mut streamed, true, |_out| {
            anyhow::bail!("object store went away")
        })
        .await
        .expect_err("a failing generator must fail the stream");
        assert!(
            format!("{error:#}").contains("object store went away"),
            "unexpected error: {error:#}"
        );
    }
}
