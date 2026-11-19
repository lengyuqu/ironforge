//! Git receive-pack protocol implementation (git push).
//!
//! Supports two modes:
//! 1. Split reader/writer (HTTP mode) — via `handle_receive_pack`
//! 2. Single bidirectional stream (SSH mode) — via `handle_receive_pack_stream`

use std::path::Path;

use anyhow::{Context, Result};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

use crate::pkt_line::{read_pkt_line, write_flush, write_pkt_line, PktLine};
use crate::secret_scan;
use crate::sideband;

/// Result of processing a push for a single ref update.
/// Deletes one branch, or explains why it must not be deleted.
///
/// `rg-git` has no database, so the policy (default branch, protection rules,
/// open pull requests, a token's branch limit) lives with the caller: it passes
/// a guard that runs the same checks as `DELETE /repos/{o}/{n}/branches/{x}`.
/// `Ok(())` means the ref is gone; `Err(reason)` becomes the report-status
/// message the client sees.
pub type BranchDeleter<'a> = dyn Fn(
        String,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>>
    + Send
    + Sync
    + 'a;

/// Result of processing a push for a single ref update.
#[derive(Clone, Debug)]
pub struct RefUpdate {
    pub old_sha: String,
    pub new_sha: String,
    pub refname: String,
    pub status: String,
    pub message: String,
}

/// Handle receive-pack with separate reader and writer (HTTP mode).
/// Returns the list of ref updates that were processed.
pub async fn handle_receive_pack<R, W>(
    repo_path: &Path,
    reader: R,
    writer: W,
) -> Result<Vec<RefUpdate>>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut reader = BufReader::new(reader);
    let mut writer = writer;

    // Send ref advertisement
    let ref_list = build_ref_list(repo_path);
    let ad = build_ref_advertisement(&ref_list, "git-receive-pack");
    for pkt in &ad {
        write_pkt_line(&mut writer, pkt).await?;
    }
    write_flush(&mut writer).await?;

    // Process the push
    let results = process_push(repo_path, &mut reader).await?;

    // Send response
    send_response(&mut writer, &results).await?;
    Ok(results)
}

/// Handle receive-pack with a single bidirectional stream (SSH mode).
/// Takes a mutable reference so the caller can send exit-status before dropping the stream.
/// Returns the list of ref updates that were processed.
pub async fn handle_receive_pack_stream<S>(
    repo_path: &Path,
    stream: &mut S,
) -> Result<Vec<RefUpdate>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    do_receive_pack_stream(repo_path, stream).await
}

/// Handle receive-pack with a caller-provided pre-receive validator.
///
/// The validator receives the parsed ref update commands before pack indexing
/// and before any ref is written. It can mark individual updates as `error`
/// while leaving allowed updates as `ok`.
pub async fn handle_receive_pack_stream_with_rejections<S>(
    repo_path: &Path,
    stream: &mut S,
    rejected_refs: Vec<(String, String)>,
    require_signed_refs: Vec<String>,
    writable_refs: Option<Vec<String>>,
    on_delete: Option<&BranchDeleter<'_>>,
) -> Result<Vec<RefUpdate>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    do_receive_pack_stream_with_rejections(
        repo_path,
        stream,
        rejected_refs,
        require_signed_refs,
        writable_refs,
        on_delete,
    )
    .await
}

/// Handle receive-pack for HTTP mode where ref advertisement is already sent.
/// Returns the list of ref updates that were processed.
pub async fn handle_receive_pack_http<R, W>(
    repo_path: &Path,
    reader: R,
    mut writer: W,
) -> Result<Vec<RefUpdate>>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut reader = BufReader::new(reader);

    let results = process_push(repo_path, &mut reader).await?;
    send_response(&mut writer, &results).await?;
    Ok(results)
}

/// Handle receive-pack for HTTP mode with a caller-provided pre-receive validator.
pub async fn handle_receive_pack_http_with_rejections<R, W>(
    repo_path: &Path,
    reader: R,
    mut writer: W,
    rejected_refs: Vec<(String, String)>,
    require_signed_refs: Vec<String>,
    writable_refs: Option<Vec<String>>,
    on_delete: Option<&BranchDeleter<'_>>,
) -> Result<Vec<RefUpdate>>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut reader = BufReader::new(reader);

    let results = process_push_with_rejections(
        repo_path,
        &mut reader,
        &rejected_refs,
        &require_signed_refs,
        writable_refs.as_deref(),
        on_delete,
    )
    .await?;
    send_response(&mut writer, &results).await?;
    Ok(results)
}

/// Internal: SSH mode implementation with single stream type.
async fn do_receive_pack_stream<S>(repo_path: &Path, stream: &mut S) -> Result<Vec<RefUpdate>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let ref_list = build_ref_list(repo_path);
    let ad = build_ref_advertisement(&ref_list, "git-receive-pack");
    for pkt in &ad {
        write_pkt_line(stream, pkt).await?;
    }
    write_flush(stream).await?;

    // Phase 1: Read push data (wrapped in BufReader for line-reading)
    let results = {
        let mut reader = BufReader::new(&mut *stream);
        process_push(repo_path, &mut reader).await?
    };

    // Phase 2: Write response (BufReader is dropped, stream is available again)
    send_response(stream, &results).await?;
    Ok(results)
}

async fn do_receive_pack_stream_with_rejections<S>(
    repo_path: &Path,
    stream: &mut S,
    rejected_refs: Vec<(String, String)>,
    require_signed_refs: Vec<String>,
    writable_refs: Option<Vec<String>>,
    on_delete: Option<&BranchDeleter<'_>>,
) -> Result<Vec<RefUpdate>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let ref_list = build_ref_list(repo_path);
    let ad = build_ref_advertisement(&ref_list, "git-receive-pack");
    for pkt in &ad {
        write_pkt_line(stream, pkt).await?;
    }
    write_flush(stream).await?;

    let results = {
        let mut reader = BufReader::new(&mut *stream);
        process_push_with_rejections(
            repo_path,
            &mut reader,
            &rejected_refs,
            &require_signed_refs,
            writable_refs.as_deref(),
            on_delete,
        )
        .await?
    };

    send_response(stream, &results).await?;
    Ok(results)
}

/// Build the list of refs with their SHAs for advertisement.
fn build_ref_list(repo_path: &Path) -> Vec<(String, String)> {
    let mut refs = Vec::new();

    // Get all refs using gix API
    if let Ok(repo) = gix::open(repo_path) {
        if let Ok(references) = repo.references() {
            if let Ok(all_refs) = references.all() {
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
            }
        }

        // Also add HEAD if we have it
        if let Ok(head_id) = repo.head_id() {
            let head_entry = refs.iter().find(|(_, name)| name == "HEAD");
            if head_entry.is_none() {
                refs.push((head_id.to_string(), "HEAD".to_string()));
            }
        }
    }

    if refs.is_empty() {
        // Empty repo — add a null ref
        refs.push((
            "0000000000000000000000000000000000000000".to_string(),
            "capabilities^{}".to_string(),
        ));
    }

    refs
}

/// Build ref advertisement pkt-lines for receive-pack.
fn build_ref_advertisement(ref_list: &[(String, String)], _service: &str) -> Vec<PktLine> {
    let mut lines = Vec::new();

    // Capabilities for receive-pack:
    // - report-status: server will send ref update status after receiving the push
    // - report-status-v2: extended status format (we respond in v1-compatible way)
    // - side-band-64k: server can send progress/error on sideband during pack receipt
    // - agent: server identification
    // NOTE: We do NOT advertise atomic (all-or-nothing ref updates) because
    // we process refs sequentially.
    // `delete-refs` is what tells the client `push --delete` is allowed; without
    // it git refuses client-side ("remote does not support deleting refs") and
    // never sends the command at all.
    let caps = format!(
        "report-status report-status-v2 delete-refs side-band-64k {}",
        crate::protocol::v2::AGENT_CAPABILITY
    );

    if let Some((sha, refname)) = ref_list.first() {
        let line = format!("{} {}\0{}", sha, refname, caps);
        lines.push(PktLine::Data(line.into_bytes()));
    }

    for (sha, refname) in ref_list.iter().skip(1) {
        let line = format!("{} {}", sha, refname);
        lines.push(PktLine::Data(line.into_bytes()));
    }

    lines
}

/// Process the push: read update commands, packfile, and update refs.
async fn process_push<R: AsyncRead + Unpin>(
    repo_path: &Path,
    reader: &mut BufReader<R>,
) -> Result<Vec<RefUpdate>> {
    process_push_with_rejections(repo_path, reader, &[], &[], None, None).await
}

async fn process_push_with_rejections<R>(
    repo_path: &Path,
    reader: &mut BufReader<R>,
    rejected_refs: &[(String, String)],
    require_signed_refs: &[String],
    writable_refs: Option<&[String]>,
    on_delete: Option<&BranchDeleter<'_>>,
) -> Result<Vec<RefUpdate>>
where
    R: AsyncRead + Unpin,
{
    let mut updates = Vec::new();

    // Read update commands using proper pkt-line parsing.
    // Each line is: `old_sha new_sha refname[\0capabilities]`
    // Terminated by a flush packet ("0000").
    loop {
        let pkt = read_pkt_line(reader).await?;

        // Flush packet or EOF → end of update commands
        // Delim/ResponseEnd are V2-only and shouldn't appear in V1 protocol
        match pkt {
            PktLine::Flush => break,
            PktLine::Delim | PktLine::ResponseEnd => continue,
            PktLine::Data(bytes) => {
                let line = String::from_utf8_lossy(&bytes);
                let line = line.trim_end_matches('\n');

                if line.is_empty() {
                    continue;
                }

                // First update line may include capabilities after NUL
                let clean_line = if line.contains('\0') {
                    line.split('\0').next().unwrap_or(line)
                } else {
                    line
                };

                // Git has two spellings for a deletion: `<old> 0000… <ref>` when
                // it knows the old value, and `:<ref>` when it does not (it omits
                // both ids). The second one used to fall through the
                // three-column check and be dropped, so the client got an empty
                // report-status and reported "remote rejected" with no reason.
                if let Some(refname) = clean_line.strip_prefix(':') {
                    let refname = refname.trim().to_string();
                    if refname.is_empty() {
                        continue;
                    }
                    tracing::info!(refname = %refname, "Receive-pack: delete command (abbreviated form)");
                    let (status, message) = if refname.starts_with("refs/heads/") {
                        ("ok".to_string(), String::new())
                    } else {
                        (
                            "error".to_string(),
                            "only branches can be deleted".to_string(),
                        )
                    };
                    updates.push(RefUpdate {
                        old_sha: String::new(),
                        new_sha: "0".repeat(40),
                        refname,
                        status,
                        message,
                    });
                    continue;
                }

                let parts: Vec<&str> = clean_line.split_whitespace().collect();
                if parts.len() < 3 {
                    continue;
                }

                let old_sha = parts[0].to_string();
                let new_sha = parts[1].to_string();
                let refname = parts[2].to_string();

                tracing::info!(
                    old = %old_sha,
                    new = %new_sha,
                    refname = %refname,
                    "Receive-pack: update command"
                );

                // A null new SHA is a deletion. Only branches can be deleted:
                // tags and the other namespaces keep their own rules and stay
                // refused. The caller's guard runs the rest of the policy, so
                // the command is queued as `ok` and decided at apply time.
                if new_sha.starts_with("0000000") {
                    let (status, message) = if refname.starts_with("refs/heads/") {
                        ("ok".to_string(), String::new())
                    } else {
                        (
                            "error".to_string(),
                            "only branches can be deleted".to_string(),
                        )
                    };
                    updates.push(RefUpdate {
                        old_sha,
                        new_sha,
                        refname,
                        status,
                        message,
                    });
                    continue;
                }

                updates.push(RefUpdate {
                    old_sha: old_sha.clone(),
                    new_sha: new_sha.clone(),
                    refname: refname.clone(),
                    status: "ok".to_string(),
                    message: String::new(),
                });
            }
        }
    }

    if updates.is_empty() {
        return Ok(updates);
    }

    for update in &mut updates {
        if update.status != "ok" {
            continue;
        }

        if let Some((_, message)) = rejected_refs
            .iter()
            .find(|(pattern, _)| ref_matches_rejection_pattern(&update.refname, pattern))
        {
            update.status = "error".to_string();
            update.message = message.clone();
            continue;
        }

        // A credential limited to some refs may write only those: everything
        // else, tags and the other namespaces included, is refused. An empty
        // list therefore refuses every update.
        if let Some(writable) = writable_refs {
            if !writable
                .iter()
                .any(|pattern| ref_matches_rejection_pattern(&update.refname, pattern))
            {
                update.status = "error".to_string();
                update.message = "this access token may not write to this ref".to_string();
            }
        }
    }

    // A null new SHA means "delete this ref"; it carries no pack data.
    let is_delete = |update: &RefUpdate| update.new_sha.starts_with("0000000");
    let has_content_updates = updates
        .iter()
        .any(|update| update.status == "ok" && !is_delete(update));

    if !updates.iter().any(|update| update.status == "ok") {
        drain_pack(reader).await?;
        return Ok(updates);
    }

    if has_content_updates {
        // Receive the pack bytes, then index them gix-natively. This replaces
        // `git index-pack --fix-thin --stdin` including thin-pack completion:
        // delta bases missing from the pack are looked up in the repository's
        // object database before indexing.
        let mut pack_data = Vec::new();
        reader.read_to_end(&mut pack_data).await?;
        let pack_bytes = pack_data.len();
        let repo_dir = repo_path.to_path_buf();
        tokio::task::spawn_blocking(move || crate::ops::index_pack_bytes(&repo_dir, &pack_data))
            .await
            .map_err(|error| anyhow::anyhow!("index-pack task failed: {error}"))?
            .map_err(|error| anyhow::anyhow!("git index-pack failed: {error}"))?;
        tracing::debug!(pack_bytes, "pack indexed");

        enforce_signed_commit_policies(repo_path, &mut updates, require_signed_refs);
        enforce_secret_scan(repo_path, &mut updates);
    }

    // Update the refs. Deletions go through the caller's guard, which applies
    // the same policy as `DELETE /repos/{o}/{n}/branches/{x}`.
    for update in &mut updates {
        if update.status != "ok" {
            continue;
        }
        if is_delete(update) {
            match on_delete {
                Some(guard) => match guard(update.refname.clone()).await {
                    Ok(()) => update.message = "deleted".to_string(),
                    Err(reason) => {
                        update.status = "error".to_string();
                        update.message = reason;
                    }
                },
                None => {
                    update.status = "error".to_string();
                    update.message =
                        "deleting branches is not supported on this transport".to_string();
                }
            }
            continue;
        }
        match update_ref(repo_path, &update.refname, &update.new_sha) {
            Ok(()) => {
                update.message = "ok".to_string();
            }
            Err(e) => {
                update.status = "error".to_string();
                update.message = format!("{}", e);
            }
        }
    }

    Ok(updates)
}

/// Refuse ref updates that would introduce a secret, before any ref is written.
///
/// Only the blobs this push adds are inspected. Blocking rules turn the update
/// into `ng <ref> <message>`; warning-level rules and skipped scans produce
/// audit events and never fail the push. A scan failure is fail-open as well,
/// but it is reported as a `secret_scan_skipped` event instead of passing
/// silently.
fn enforce_secret_scan(repo_path: &Path, updates: &mut [RefUpdate]) {
    let requests: Vec<secret_scan::RefScanRequest<'_>> = updates
        .iter()
        .filter(|update| update.status == "ok" && !update.new_sha.starts_with("0000000"))
        .map(|update| secret_scan::RefScanRequest {
            refname: update.refname.as_str(),
            old_sha: (!update.old_sha.starts_with("0000000")).then_some(update.old_sha.as_str()),
            new_sha: update.new_sha.as_str(),
        })
        .collect();
    if requests.is_empty() {
        return;
    }
    // Owned copies so the immutable borrow of `updates` ends before the refs are
    // updated below.
    let refnames: Vec<String> = requests
        .iter()
        .map(|request| request.refname.to_string())
        .collect();

    let scan = match secret_scan::scan_push(
        repo_path,
        &requests,
        &secret_scan::AllowList::default_rules(),
    ) {
        Ok(scan) => scan,
        Err(error) => {
            tracing::warn!(%error, "secret scan failed; the push continues unscanned");
            for refname in &refnames {
                secret_scan::record(secret_scan::AuditEvent {
                    kind: secret_scan::AuditKind::Skipped,
                    refname: Some(refname.clone()),
                    path: None,
                    rule: None,
                    files: None,
                    reason: Some(format!("scan failed: {error}")),
                });
            }
            return;
        }
    };

    if let Some(skip) = scan.skipped {
        // Over the file budget: the whole push is unscanned, and that is
        // reported for every ref it touches.
        for refname in &refnames {
            secret_scan::record(secret_scan::AuditEvent {
                kind: secret_scan::AuditKind::Skipped,
                refname: Some(refname.clone()),
                path: None,
                rule: None,
                files: skip.files(),
                reason: Some(skip.reason()),
            });
        }
        return;
    }

    for ref_scan in &scan.refs {
        for finding in &ref_scan.report.findings {
            let kind = if finding.rule.is_blocking() {
                secret_scan::AuditKind::Rejected
            } else {
                secret_scan::AuditKind::Warned
            };
            secret_scan::record(secret_scan::AuditEvent {
                kind,
                refname: Some(ref_scan.refname.clone()),
                path: Some(finding.path.clone()),
                rule: Some(finding.rule),
                files: None,
                reason: None,
            });
        }
        for skip in &ref_scan.report.skipped {
            secret_scan::record(secret_scan::AuditEvent {
                kind: secret_scan::AuditKind::Skipped,
                refname: Some(ref_scan.refname.clone()),
                path: skip.path().map(str::to_string),
                rule: None,
                files: skip.files(),
                reason: Some(skip.reason()),
            });
        }
    }

    for ref_scan in &scan.refs {
        let Some(message) = ref_scan.report.rejection_message() else {
            continue;
        };
        if let Some(update) = updates
            .iter_mut()
            .find(|update| update.refname == ref_scan.refname)
        {
            update.status = "error".to_string();
            update.message = message;
        }
    }
}

fn enforce_signed_commit_policies(
    repo_path: &Path,
    updates: &mut [RefUpdate],
    patterns: &[String],
) {
    // Open the repository once for gix-native signature verification.
    let verify_repo = match gix::open(repo_path) {
        Ok(repo) => repo,
        Err(error) => {
            // Fail closed: without repository access we cannot verify anything.
            for update in updates.iter_mut().filter(|update| {
                update.status == "ok"
                    && patterns
                        .iter()
                        .any(|pattern| ref_matches_rejection_pattern(&update.refname, pattern))
            }) {
                update.status = "error".into();
                update.message = format!("unable to verify required commit signatures: {error}");
            }
            return;
        }
    };
    let verify = |sha: &str| -> bool {
        crate::ops::verify_commit_with_repo(&verify_repo, sha).unwrap_or(false)
    };

    for update in updates.iter_mut().filter(|update| {
        update.status == "ok"
            && !update.new_sha.starts_with("0000000")
            && patterns
                .iter()
                .any(|pattern| ref_matches_rejection_pattern(&update.refname, pattern))
    }) {
        let new_id = match gix::hash::ObjectId::from_hex(update.new_sha.as_bytes()) {
            Ok(id) => id,
            Err(error) => {
                update.status = "error".into();
                update.message = format!("invalid new sha for signature verification: {error}");
                continue;
            }
        };
        let old_id = if !update.old_sha.starts_with("0000000") {
            match gix::hash::ObjectId::from_hex(update.old_sha.as_bytes()) {
                Ok(id) => Some(id),
                Err(error) => {
                    update.status = "error".into();
                    update.message = format!("invalid old sha for signature verification: {error}");
                    continue;
                }
            }
        } else {
            None
        };
        let commits = match crate::ops::commits_between(&verify_repo, new_id, old_id) {
            Ok(commits) => commits,
            Err(error) => {
                update.status = "error".into();
                update.message =
                    format!("failed to enumerate commits for signature verification: {error}");
                continue;
            }
        };
        // gix-native replacement for `git verify-commit`: verification shells
        // out to the configured GPG program via gix's `command` feature. Any
        // error (or an unsigned commit) fails closed, matching the CLI where
        // `verify-commit` exiting non-zero marks the commit unsigned.
        if let Some(unsigned) = commits.iter().find(|sha| !verify(sha)) {
            update.status = "error".into();
            update.message =
                format!("commit {unsigned} does not have a cryptographically valid signature");
        }
    }
}

/// Match a full ref against a rejection pattern. `*` matches any sequence;
/// patterns without wildcards retain exact-match behavior.
pub fn ref_matches_rejection_pattern(refname: &str, pattern: &str) -> bool {
    if !pattern.contains('*') {
        return refname == pattern;
    }
    let value = refname.as_bytes();
    let pattern = pattern.as_bytes();
    let mut dp = vec![vec![false; value.len() + 1]; pattern.len() + 1];
    dp[0][0] = true;
    for i in 1..=pattern.len() {
        if pattern[i - 1] == b'*' {
            dp[i][0] = dp[i - 1][0];
        }
        for j in 1..=value.len() {
            dp[i][j] = if pattern[i - 1] == b'*' {
                dp[i - 1][j] || dp[i][j - 1]
            } else {
                dp[i - 1][j - 1] && pattern[i - 1] == value[j - 1]
            };
        }
    }
    dp[pattern.len()][value.len()]
}

async fn drain_pack<R: AsyncRead + Unpin>(reader: &mut BufReader<R>) -> Result<()> {
    let mut buf = [0u8; 8192];
    loop {
        let n = reader.read(&mut buf).await?;
        if n == 0 {
            return Ok(());
        }
    }
}

/// Update a ref to point to a new SHA using gix API.
fn update_ref(repo_path: &Path, refname: &str, new_sha: &str) -> Result<()> {
    let repo = gix::open(repo_path).context("failed to open repository")?;
    let object_id = gix::ObjectId::from_hex(new_sha.as_bytes())
        .map_err(|e| anyhow::anyhow!("invalid SHA: {}", e))?;

    // Use repo.reference() to create or update a reference
    // PreviousValue::Any means set unconditionally (like git update-ref)
    repo.reference(
        refname,
        object_id,
        gix::refs::transaction::PreviousValue::Any,
        "update via receive-pack",
    )
    .map_err(|e| anyhow::anyhow!("failed to update ref {}: {}", refname, e))?;

    Ok(())
}

/// Send the response back to the client using the report-status protocol.
///
/// When `side-band-64k` is negotiated (which we always advertise), the entire
/// report-status payload MUST be sideband-encoded as band 1 data.
///
/// Observed correct wire format (verified against real git receive-pack):
///
///   [sideband pkt-line: band=\x01, payload = <report-status pkt-lines concatenated>]
///   [sideband flush: 0000]
///
/// Where the inner report-status pkt-lines payload is:
///   000eunpack ok\n
///   0017ok refs/heads/main\n    (one per ref)
///   0000                        (plain flush — embedded in the band-1 payload)
///
/// The git client reads sideband until it gets a sideband flush `0000`.
/// The band-1 content is then parsed as report-status pkt-lines.
async fn send_response<W: AsyncWrite + Unpin>(writer: &mut W, results: &[RefUpdate]) -> Result<()> {
    // Build the report-status pkt-lines into an in-memory buffer.
    // These will be sent as band-1 sideband data in one shot.
    let mut report_buf: Vec<u8> = Vec::new();

    // 1. unpack status (MUST be first)
    write_pkt_line(&mut report_buf, &PktLine::text("unpack ok")).await?;

    // 2. per-ref update status
    for result in results {
        if result.status == "ok" {
            let line = format!("ok {}", result.refname);
            write_pkt_line(&mut report_buf, &PktLine::text(&line)).await?;
        } else {
            let line = format!("ng {} {}", result.refname, result.message);
            write_pkt_line(&mut report_buf, &PktLine::text(&line)).await?;
        }
    }

    // 3. Flush packet embedded in the band-1 payload
    write_flush(&mut report_buf).await?;

    // Send the entire report as sideband band-1 data
    sideband::write_sideband_data(writer, &report_buf).await?;

    // Send sideband flush to signal end of the sideband stream
    sideband::write_sideband_flush(writer).await?;

    // Ensure everything is flushed to the transport layer
    writer.flush().await?;

    tracing::info!("Receive-pack response sent");
    Ok(())
}

#[cfg(test)]
mod rejection_pattern_tests {
    use super::{enforce_signed_commit_policies, ref_matches_rejection_pattern, RefUpdate};
    #[test]
    fn matches_exact_branches_and_wildcard_tags() {
        assert!(ref_matches_rejection_pattern(
            "refs/heads/main",
            "refs/heads/main"
        ));
        assert!(!ref_matches_rejection_pattern(
            "refs/heads/feature",
            "refs/heads/main"
        ));
        assert!(ref_matches_rejection_pattern(
            "refs/tags/v1.2.3",
            "refs/tags/v*"
        ));
        assert!(ref_matches_rejection_pattern(
            "refs/tags/release/2026/07",
            "refs/tags/release/**"
        ));
        assert!(!ref_matches_rejection_pattern(
            "refs/tags/test-1",
            "refs/tags/v*"
        ));
    }

    #[test]
    fn signed_commit_policy_rejects_unsigned_commit_on_matching_branch() {
        let temp = tempfile::tempdir().unwrap();
        let gateway = crate::cli_gateway::global_gateway().as_ref().unwrap();
        assert!(gateway.run(&["init"], Some(temp.path())).unwrap().success());
        assert!(gateway
            .run(&["config", "user.name", "Test"], Some(temp.path()))
            .unwrap()
            .success());
        assert!(gateway
            .run(
                &["config", "user.email", "test@example.com"],
                Some(temp.path())
            )
            .unwrap()
            .success());
        assert!(gateway
            .run(&["config", "commit.gpgsign", "false"], Some(temp.path()))
            .unwrap()
            .success());
        std::fs::write(temp.path().join("README.md"), "unsigned").unwrap();
        assert!(gateway
            .run(&["add", "README.md"], Some(temp.path()))
            .unwrap()
            .success());
        assert!(gateway
            .run(&["commit", "-m", "unsigned"], Some(temp.path()))
            .unwrap()
            .success());
        let sha = gateway
            .run(&["rev-parse", "HEAD"], Some(temp.path()))
            .unwrap()
            .stdout_str()
            .trim()
            .to_owned();
        let mut updates = vec![RefUpdate {
            old_sha: "0".repeat(40),
            new_sha: sha,
            refname: "refs/heads/main".into(),
            status: "ok".into(),
            message: String::new(),
        }];
        enforce_signed_commit_policies(temp.path(), &mut updates, &["refs/heads/main".into()]);
        assert_eq!(updates[0].status, "error");
        assert!(updates[0]
            .message
            .contains("cryptographically valid signature"));
    }
}

#[cfg(test)]
mod secret_scan_push_tests {
    use super::{enforce_secret_scan, RefUpdate};
    use crate::ops::{commit_tree_edits, CommitTreeOutcome, TreeEdit};

    const PRIVATE_KEY: &str = "-----BEGIN OPENSSH PRIVATE KEY-----";

    fn init_bare() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        gix::init_bare(dir.path()).unwrap();
        dir
    }

    fn commit(repo: &std::path::Path, parent: Option<&str>, files: &[(&str, &str)]) -> String {
        let edits: Vec<TreeEdit> = files
            .iter()
            .map(|(path, content)| TreeEdit::SetFile {
                path: (*path).to_string(),
                content: content.as_bytes().to_vec(),
                expected_blob_sha: None,
            })
            .collect();
        match commit_tree_edits(
            repo,
            "main",
            parent,
            &edits,
            "test commit",
            "Test",
            "test@example.invalid",
            parent.is_none(),
        )
        .unwrap()
        {
            CommitTreeOutcome::Committed(sha) => sha,
            CommitTreeOutcome::TipAdvanced { actual } => panic!("tip advanced to {actual:?}"),
        }
    }

    fn update(old_sha: &str, new_sha: &str) -> RefUpdate {
        RefUpdate {
            old_sha: old_sha.to_string(),
            new_sha: new_sha.to_string(),
            refname: "refs/heads/main".into(),
            status: "ok".into(),
            message: String::new(),
        }
    }

    #[test]
    fn a_blocking_rule_turns_the_ref_update_into_an_error() {
        let dir = init_bare();
        let sha = commit(dir.path(), None, &[("deploy/id_rsa", PRIVATE_KEY)]);
        let mut updates = vec![update(&"0".repeat(40), &sha)];

        enforce_secret_scan(dir.path(), &mut updates);

        assert_eq!(updates[0].status, "error");
        assert!(
            updates[0].message.contains("deploy/id_rsa"),
            "{:?}",
            updates[0]
        );
        assert!(
            updates[0].message.contains("private_key"),
            "{:?}",
            updates[0]
        );
        // The report-status line goes to the client: never the matched text.
        assert!(
            !updates[0].message.contains("PRIVATE KEY"),
            "{:?}",
            updates[0]
        );
    }

    #[test]
    fn a_clean_update_is_left_alone() {
        let dir = init_bare();
        let sha = commit(dir.path(), None, &[("src/main.rs", "fn main() {}\n")]);
        let mut updates = vec![update(&"0".repeat(40), &sha)];

        enforce_secret_scan(dir.path(), &mut updates);

        assert_eq!(updates[0].status, "ok");
        assert!(updates[0].message.is_empty());
    }

    #[test]
    fn warning_rules_do_not_block_the_push() {
        let dir = init_bare();
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
        let sha = commit(dir.path(), None, &[("src/auth.rs", jwt)]);
        let mut updates = vec![update(&"0".repeat(40), &sha)];

        enforce_secret_scan(dir.path(), &mut updates);

        assert_eq!(updates[0].status, "ok", "{:?}", updates[0]);
    }

    #[test]
    fn an_unreadable_repository_fails_open_without_blocking() {
        let dir = init_bare();
        let mut updates = vec![update(&"0".repeat(40), &"f".repeat(40))];

        enforce_secret_scan(dir.path(), &mut updates);

        assert_eq!(updates[0].status, "ok");
    }

    #[test]
    fn deletions_are_not_scanned() {
        let dir = init_bare();
        let sha = commit(dir.path(), None, &[("deploy/id_rsa", PRIVATE_KEY)]);
        let mut updates = vec![update(&sha, &"0".repeat(40))];

        enforce_secret_scan(dir.path(), &mut updates);

        assert_eq!(updates[0].status, "ok");
    }
}
