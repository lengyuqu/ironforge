//! Streaming pack output must be indistinguishable from the buffered path (T38).
//!
//! `ops::pack_for_wants*` used to collect the pack in a `Vec<u8>`; on a large
//! repository that single buffer held 1.2 GB. The `_into` variants write the
//! same pack into any `std::io::Write`, which is what the protocol handlers now
//! hand them. These tests pin the contract: same objects, same content, and
//! output that arrives in pieces rather than one giant allocation.
//!
//! Everything is checked with a real `git`: packs are written to disk, ingested
//! with `git index-pack`, then compared through `git cat-file`. Pack *bytes* are
//! deliberately not compared — delta selection order is not part of the
//! contract; the object set is.

use std::io;
use std::path::{Path, PathBuf};

use rg_git::cli_gateway::GitCommandGateway;
use rg_git::ops::{
    pack_for_wants, pack_for_wants_filtered, pack_for_wants_filtered_into, pack_for_wants_into,
    pack_universe, pack_universe_into, ObjectFilter,
};

// ── harness ─────────────────────────────────────────────────────────

struct Repo {
    _tmp: tempfile::TempDir,
    gateway: GitCommandGateway,
    root: PathBuf,
}

impl Repo {
    fn new() -> Self {
        let tmp = tempfile::tempdir().expect("temp dir");
        let gateway = GitCommandGateway::new().expect("git CLI available");
        let root = tmp.path().to_path_buf();
        Self {
            _tmp: tmp,
            gateway,
            root,
        }
    }

    fn path(&self) -> &Path {
        &self.root
    }

    fn git(&self, args: &[&str]) -> String {
        self.git_in(&self.root, args)
    }

    fn git_in(&self, dir: &Path, args: &[&str]) -> String {
        let output = self.gateway.run(args, Some(dir)).expect("git runs");
        assert!(
            output.success(),
            "git {args:?} failed: {}",
            output.stderr_str()
        );
        output.stdout_str().trim().to_string()
    }

    fn write(&self, relative: &str, content: &str) {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create dir");
        }
        std::fs::write(path, content).expect("write file");
    }

    fn commit(&self, message: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-m", message]);
    }

    fn head(&self) -> String {
        self.git(&["rev-parse", "HEAD"])
    }
}

/// A work tree with `files` files spread over a few directories, one commit.
///
/// Big enough to span several 64 KiB chunks so the "arrives in pieces"
/// assertion is meaningful.
fn seeded_repo(files: usize) -> Repo {
    let repo = Repo::new();
    repo.git(&["init", "-b", "main"]);
    repo.git(&["config", "user.email", "t38@example.com"]);
    repo.git(&["config", "user.name", "T38 Test"]);
    repo.git(&["config", "commit.gpgsign", "false"]);
    for index in 0..files {
        repo.write(
            &format!("g/d{}/f{index:04}.txt", index % 5),
            // ~1 KiB per file so 400 files are well past one chunk.
            &format!("file {index} at commit 0\n{}", "x".repeat(1000)),
        );
    }
    repo.commit("initial");
    repo
}

fn bare_server(source: &Repo) -> PathBuf {
    let dir = source.path().join("server.git");
    source.git(&[
        "clone",
        "--bare",
        "--no-local",
        source.path().to_string_lossy().as_ref(),
        dir.to_string_lossy().as_ref(),
    ]);
    dir
}

/// A `Write` that keeps every `write` call separate, so the test can tell
/// incremental output from one accumulated buffer.
#[derive(Default)]
struct ChunkRecorder {
    chunks: Vec<Vec<u8>>,
}

impl ChunkRecorder {
    fn into_bytes(self) -> Vec<u8> {
        self.chunks.concat()
    }
}

impl io::Write for ChunkRecorder {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.chunks.push(data.to_vec());
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Object listing of everything a client owns after ingesting `pack`, as
/// `oid type size` lines sorted — the same view for either pack shape.
fn client_objects(gateway: &GitCommandGateway, dir: &Path, pack: &[u8]) -> Vec<String> {
    std::fs::create_dir_all(dir).expect("create client dir");
    gateway
        .run(&["init", "-b", "main"], Some(dir))
        .expect("git init")
        .ensure_success()
        .expect("git init succeeds");
    let pack_dir = dir.join(".git/objects/pack");
    std::fs::create_dir_all(&pack_dir).expect("create pack dir");
    let path = pack_dir.join("pack-t38.pack");
    std::fs::write(&path, pack).expect("write pack");
    let output = gateway
        .run(&["index-pack", path.to_string_lossy().as_ref()], Some(dir))
        .expect("git index-pack runs");
    assert!(
        output.success(),
        "git index-pack rejected the pack: {}",
        output.stderr_str()
    );

    let output = gateway
        .run(
            &[
                "cat-file",
                "--batch-all-objects",
                "--batch-check=%(objectname) %(objecttype) %(objectsize)",
            ],
            Some(dir),
        )
        .expect("git cat-file runs");
    assert!(
        output.success(),
        "git cat-file failed: {}",
        output.stderr_str()
    );
    let mut lines: Vec<String> = output
        .stdout_str()
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();
    lines.sort();
    lines
}

fn recorder_chunk_max(recorder: &ChunkRecorder) -> usize {
    recorder.chunks.iter().map(Vec::len).max().unwrap_or(0)
}

fn assert_same_objects(label: &str, buffered: &[u8], streamed: &[u8]) {
    let gateway = GitCommandGateway::new().expect("git CLI available");
    let tmp = tempfile::tempdir().expect("temp dir");
    let buffered_objects = client_objects(&gateway, &tmp.path().join("buffered"), buffered);
    let streamed_objects = client_objects(&gateway, &tmp.path().join("streamed"), streamed);

    assert!(
        !buffered_objects.is_empty(),
        "{label}: buffered pack has no objects"
    );
    assert_eq!(
        buffered_objects.len(),
        streamed_objects.len(),
        "{label}: object count differs (buffered {} vs streamed {})",
        buffered_objects.len(),
        streamed_objects.len()
    );
    assert_eq!(
        buffered_objects, streamed_objects,
        "{label}: streamed pack's objects differ from the buffered pack's"
    );
    println!(
        "[t38] {label}: {} objects, buffered={} B streamed={} B",
        buffered_objects.len(),
        buffered.len(),
        streamed.len()
    );
}

// ── tests ───────────────────────────────────────────────────────────

/// Full clone: the streamed pack must carry exactly the buffered pack's objects.
#[test]
fn streamed_full_clone_pack_has_the_same_objects() {
    let source = seeded_repo(400);
    let tip = source.head();
    let server = bare_server(&source);
    let wants = std::slice::from_ref(&tip);

    let buffered = pack_for_wants(&server, wants, &[], &[]).expect("buffered pack");
    let mut recorder = ChunkRecorder::default();
    pack_for_wants_into(&server, wants, &[], &[], &mut recorder).expect("streamed pack");
    let chunk_count = recorder.chunks.len();
    let largest = recorder_chunk_max(&recorder);
    let streamed = recorder.into_bytes();

    assert_same_objects("full clone", &buffered, &streamed);
    assert!(
        chunk_count > 1,
        "expected the pack to be handed over in several writes, got {chunk_count}"
    );
    // The encoder must feed the writer as it goes, not hand over one giant
    // buffer at the end (which is the allocation T38 removes).
    assert!(
        largest < streamed.len(),
        "one write carried the whole {} byte pack",
        streamed.len()
    );
}

/// Incremental fetch (client already has `parent`): same objects both ways.
#[test]
fn streamed_incremental_fetch_pack_has_the_same_objects() {
    let source = seeded_repo(200);
    let parent = source.head();
    source.write("g/d0/f0000.txt", "changed\n");
    source.commit("change one file");
    let tip = source.head();
    let server = bare_server(&source);

    let buffered = pack_for_wants(
        &server,
        std::slice::from_ref(&tip),
        std::slice::from_ref(&parent),
        &[],
    )
    .expect("buffered pack");
    let mut recorder = ChunkRecorder::default();
    pack_for_wants_into(
        &server,
        std::slice::from_ref(&tip),
        std::slice::from_ref(&parent),
        &[],
        &mut recorder,
    )
    .expect("streamed pack");

    assert_same_objects("incremental fetch", &buffered, &recorder.into_bytes());
}

/// Partial clone (`blob:none`): the filter runs in both paths identically.
#[test]
fn streamed_filtered_pack_has_the_same_objects() {
    let source = seeded_repo(50);
    let tip = source.head();
    let server = bare_server(&source);
    let filter = ObjectFilter::parse("blob:none").expect("filter parses");

    let buffered = pack_for_wants_filtered(
        &server,
        std::slice::from_ref(&tip),
        &[],
        &[],
        false,
        Some(filter.clone()),
    )
    .expect("buffered pack");
    let mut recorder = ChunkRecorder::default();
    pack_for_wants_filtered_into(
        &server,
        std::slice::from_ref(&tip),
        &[],
        &[],
        false,
        Some(filter),
        &mut recorder,
    )
    .expect("streamed pack");

    assert_same_objects("filtered clone", &buffered, &recorder.into_bytes());
}

/// `pack_universe` (the empty-`want` fallback, and `fork_repo`) both ways.
#[test]
fn streamed_universe_pack_has_the_same_objects() {
    let source = seeded_repo(100);
    let server = bare_server(&source);

    let buffered = pack_universe(&server).expect("buffered pack");
    let mut recorder = ChunkRecorder::default();
    pack_universe_into(&server, &mut recorder).expect("streamed pack");

    assert_same_objects("universe", &buffered, &recorder.into_bytes());
}

/// A writer that fails mid-pack must surface the error instead of silently
/// truncating: the HTTP layer relies on that to stop generation when the client
/// hangs up, and the log relies on it to say why.
#[test]
fn streamed_pack_reports_a_failing_writer() {
    let source = seeded_repo(100);
    let tip = source.head();
    let server = bare_server(&source);

    struct FailingWriter {
        remaining: usize,
    }

    impl io::Write for FailingWriter {
        fn write(&mut self, data: &[u8]) -> io::Result<usize> {
            if data.len() > self.remaining {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "client went away",
                ));
            }
            self.remaining -= data.len();
            Ok(data.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    let mut writer = FailingWriter { remaining: 1024 };
    let error =
        match pack_for_wants_into(&server, std::slice::from_ref(&tip), &[], &[], &mut writer) {
            Ok(_) => panic!("a failing writer must fail pack generation"),
            Err(error) => error,
        };
    let message = format!("{error:#}");
    assert!(
        message.contains("client went away") || message.contains("failed to write pack entry"),
        "unexpected error surfaced: {message}"
    );
}
