//! Large-repository performance baseline (T32).
//!
//! These tests are `#[ignore]`d: they build multi-hundred-megabyte datasets and
//! are meant to be run on a quiet machine, not on every `cargo test`:
//!
//! ```text
//! cargo test -p rg-git --test perf_baseline -- --ignored --nocapture
//! ```
//!
//! `LARGE` needs ~2 GiB of disk and is additionally gated behind
//! `IRONFORGE_PERF_LARGE=1`; without it the test prints a skip line and returns.
//!
//! Every measurement goes through the *real* server-side entry points:
//!
//! | # | Probe | Entry point |
//! |---|-------|-------------|
//! | 1 | `push` initial (all objects) | `protocol::receive_pack::handle_receive_pack_http` |
//! | 2 | `push` incremental (1 file)  | `protocol::receive_pack::handle_receive_pack_http` |
//! | 3 | `clone` (all objects)        | `protocol::upload_pack::handle_upload_pack_http` |
//! | 4 | `log` first page (100)       | gix ref walk + commit parse, mirroring `rg-http`'s `get_commit_log` |
//! | 5 | `tree` root                  | `rg_git::ops::list_tree_files` (recursive) |
//! | 6 | `tree` deep directory        | `rg_git::ops::list_tree_files` (subtree) |
//!
//! Pack **generation** on the client side and the preparation of each push
//! target happen before the timer starts; what is measured is the server's
//! work for a request whose bytes are already in memory.
//!
//! Thresholds below are this machine's measured median × 1.5. Different
//! hardware produces different absolute numbers — the environment fingerprint
//! printed at the start of every run (and recorded in
//! `docs/plans/T32-baseline-report.md`) is what makes a number comparable.

#![allow(clippy::print_stdout)]

use std::io;
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Instant;

#[path = "perf_fixture.rs"]
mod perf_fixture;

use perf_fixture::{Dataset, DatasetSpec};
use rg_git::protocol::receive_pack::handle_receive_pack_http;
use rg_git::protocol::upload_pack::handle_upload_pack_http;

// ── Names ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatasetName {
    Small,
    Medium,
    Large,
}

impl DatasetName {
    fn as_str(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    PushInitial,
    PushIncremental,
    Clone,
    LogFirstPage,
    TreeRoot,
    TreeDeep,
}

impl Operation {
    fn as_str(self) -> &'static str {
        match self {
            Self::PushInitial => "push_initial",
            Self::PushIncremental => "push_incremental",
            Self::Clone => "clone",
            Self::LogFirstPage => "log_first_page",
            Self::TreeRoot => "tree_root",
            Self::TreeDeep => "tree_deep",
        }
    }
}

// ── Thresholds ──────────────────────────────────────────────────────

/// `(dataset, operation) -> upper bound in milliseconds`, measured median × 1.5
/// rounded up (to the next multiple of 5 above 50 ms). Calibrated on the
/// machine fingerprinted in `docs/plans/T32-baseline-report.md` with the dev
/// profile, i.e. exactly what the acceptance command runs.
///
/// `LARGE` rows come from a **single** sample (n = 1, ~20 min of work), so they
/// are indicative: they are here so a manual `IRONFORGE_PERF_LARGE=1` run has
/// something to compare against, not because one sample is a distribution.
pub const THRESHOLDS: &[(DatasetName, Operation, u64)] = &[
    (DatasetName::Small, Operation::PushInitial, 195),
    (DatasetName::Small, Operation::PushIncremental, 15),
    (DatasetName::Small, Operation::Clone, 265),
    (DatasetName::Small, Operation::LogFirstPage, 15),
    (DatasetName::Small, Operation::TreeRoot, 110),
    (DatasetName::Small, Operation::TreeDeep, 6),
    (DatasetName::Medium, Operation::PushInitial, 5650),
    (DatasetName::Medium, Operation::PushIncremental, 55),
    (DatasetName::Medium, Operation::Clone, 4730),
    (DatasetName::Medium, Operation::LogFirstPage, 15),
    (DatasetName::Medium, Operation::TreeRoot, 1195),
    (DatasetName::Medium, Operation::TreeDeep, 7),
    (DatasetName::Large, Operation::PushInitial, 111_805),
    (DatasetName::Large, Operation::PushIncremental, 410),
    (DatasetName::Large, Operation::Clone, 69_015),
    (DatasetName::Large, Operation::LogFirstPage, 26),
    (DatasetName::Large, Operation::TreeRoot, 17_350),
    (DatasetName::Large, Operation::TreeDeep, 17),
];

fn threshold_for(dataset: DatasetName, operation: Operation) -> Option<u64> {
    THRESHOLDS
        .iter()
        .find(|(d, o, _)| *d == dataset && *o == operation)
        .map(|(_, _, ms)| *ms)
}

// ── Sampling ────────────────────────────────────────────────────────

struct Sample {
    ms: f64,
    bytes: u64,
}

struct OpResult {
    operation: Operation,
    samples: Vec<Sample>,
}

impl OpResult {
    fn sorted_ms(&self) -> Vec<f64> {
        let mut values: Vec<f64> = self.samples.iter().map(|s| s.ms).collect();
        values.sort_by(|a, b| a.partial_cmp(b).expect("timings are never NaN"));
        values
    }

    /// Nearest-rank percentile over the samples. With n = 3, p95 is the max;
    /// that is stated in the report rather than hidden behind a smoother.
    fn percentile(&self, p: f64) -> f64 {
        let values = self.sorted_ms();
        let rank = ((p / 100.0) * values.len() as f64).ceil() as usize;
        values[rank.saturating_sub(1).min(values.len() - 1)]
    }

    fn p50(&self) -> f64 {
        self.percentile(50.0)
    }

    fn p95(&self) -> f64 {
        self.percentile(95.0)
    }

    fn bytes(&self) -> u64 {
        self.samples.first().map(|s| s.bytes).unwrap_or(0)
    }
}

// ── Entry point per dataset ─────────────────────────────────────────

fn run_dataset(spec: &'static DatasetSpec, name: DatasetName, repetitions: usize) {
    println!("[perf] env {}", environment_fingerprint());

    let dataset = perf_fixture::build(spec);
    let (digest, entries) = perf_fixture::content_digest(&dataset.repo_path, "HEAD");
    println!(
        "[perf] dataset={} commits={} files={} file_bytes={} dirs={} build_ms={:.1} \
         disk_mib={:.1} content_mib={:.1} tip={} digest={} entries={}",
        spec.name,
        spec.commits,
        spec.files,
        spec.file_bytes,
        spec.dirs,
        dataset.build_ms,
        perf_fixture::mib(dataset.disk_bytes),
        perf_fixture::mib(dataset.total_file_bytes),
        dataset.tip,
        digest,
        entries,
    );

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");

    let results = vec![
        measure_push_initial(&runtime, &dataset, repetitions),
        measure_push_incremental(&runtime, &dataset, repetitions),
        measure_clone(&runtime, &dataset, repetitions),
        measure_log(&dataset, repetitions),
        measure_tree(&dataset, "", Operation::TreeRoot, repetitions),
        measure_tree(
            &dataset,
            &dataset.deep_dir,
            Operation::TreeDeep,
            repetitions,
        ),
    ];

    print_summary(name, spec, &results);
    check_thresholds(name, &results, repetitions);
}

// ── 1. push: initial ────────────────────────────────────────────────

fn measure_push_initial(
    runtime: &tokio::runtime::Runtime,
    dataset: &Dataset,
    repetitions: usize,
) -> OpResult {
    // Client-side pack generation is not part of the server's cost.
    let pack = rg_git::ops::pack_universe(&dataset.repo_path).expect("failed to build full pack");
    let mut samples = Vec::with_capacity(repetitions);

    for rep in 0..repetitions {
        let tmp = tempfile::tempdir().expect("failed to create push target dir");
        let target = tmp.path().join("target.git");
        perf_fixture::init_bare_with_main(&target);

        let request = push_request(&"0".repeat(40), &dataset.tip, &pack);
        let mut sink = CountingSink::default();
        let started = Instant::now();
        let updates = runtime
            .block_on(handle_receive_pack_http(&target, &request[..], &mut sink))
            .expect("receive-pack (initial) failed");
        let ms = elapsed_ms(started);

        assert_eq!(updates.len(), 1, "expected one ref update");
        assert_eq!(updates[0].status, "ok", "push rejected: {}", updates[0].message);
        let pushed = gix::open(&target).expect("target must open");
        assert_eq!(
            pushed.head_id().expect("target HEAD").to_string(),
            dataset.tip,
            "target HEAD does not match the pushed tip"
        );

        record(&mut samples, dataset.spec.name, Operation::PushInitial, rep, ms, pack.len() as u64);
    }

    OpResult {
        operation: Operation::PushInitial,
        samples,
    }
}

// ── 2. push: incremental ────────────────────────────────────────────

fn measure_push_incremental(
    runtime: &tokio::runtime::Runtime,
    dataset: &Dataset,
    repetitions: usize,
) -> OpResult {
    // Everything up to `parent_tip` is what the target must already have;
    // only the last commit's pack is timed. The incremental pack is the real
    // delta (new commit + the trees on its spine + one blob), produced
    // independently of `ops::pack_for_wants` so the timed push hands
    // `receive-pack` a realistic amount of data.
    let history = rg_git::ops::pack_for_wants(
        &dataset.repo_path,
        std::slice::from_ref(&dataset.parent_tip),
        &[],
        &[],
    )
    .expect("failed to build the pre-history pack");
    let increment = perf_fixture::incremental_pack(&dataset.repo_path, &dataset.tip)
        .expect("failed to build the incremental pack");
    // What the server's own `upload-pack` would send for the same one-file
    // fetch. Before T37 this was the whole work tree (20 224 objects on
    // medium); it is now the object-level delta and tracks `increment`.
    let server_side_increment = rg_git::ops::pack_for_wants(
        &dataset.repo_path,
        std::slice::from_ref(&dataset.tip),
        std::slice::from_ref(&dataset.parent_tip),
        &[],
    )
    .expect("failed to build the server-side incremental pack");
    println!(
        "[perf] note dataset={} push_incremental pack_objects={} pack_bytes={} history_pack_bytes={} \
         ops_incremental_pack_bytes={} ops_incremental_objects={}",
        dataset.spec.name,
        perf_fixture::pack_object_count(&increment),
        increment.len(),
        history.len(),
        server_side_increment.len(),
        perf_fixture::pack_object_count(&server_side_increment),
    );

    let mut samples = Vec::with_capacity(repetitions);
    for rep in 0..repetitions {
        let tmp = tempfile::tempdir().expect("failed to create push target dir");
        let target = tmp.path().join("target.git");
        perf_fixture::init_bare_with_main(&target);

        // Prepare: the target already contains the history before the tip.
        let history_request = push_request(&"0".repeat(40), &dataset.parent_tip, &history);
        let mut sink = CountingSink::default();
        runtime
            .block_on(handle_receive_pack_http(
                &target,
                &history_request[..],
                &mut sink,
            ))
            .expect("preparing the incremental target failed");

        let request = push_request(&dataset.parent_tip, &dataset.tip, &increment);
        let mut sink = CountingSink::default();
        let started = Instant::now();
        let updates = runtime
            .block_on(handle_receive_pack_http(&target, &request[..], &mut sink))
            .expect("receive-pack (incremental) failed");
        let ms = elapsed_ms(started);

        assert_eq!(updates.len(), 1, "expected one ref update");
        assert_eq!(updates[0].status, "ok", "push rejected: {}", updates[0].message);
        let pushed = gix::open(&target).expect("target must open");
        assert_eq!(
            pushed.head_id().expect("target HEAD").to_string(),
            dataset.tip,
            "incremental push did not move the branch"
        );

        record(
            &mut samples,
            dataset.spec.name,
            Operation::PushIncremental,
            rep,
            ms,
            increment.len() as u64,
        );
    }

    OpResult {
        operation: Operation::PushIncremental,
        samples,
    }
}

// ── 3. clone ────────────────────────────────────────────────────────

fn measure_clone(
    runtime: &tokio::runtime::Runtime,
    dataset: &Dataset,
    repetitions: usize,
) -> OpResult {
    let request = want_request(&dataset.tip);
    let mut samples = Vec::with_capacity(repetitions);

    for rep in 0..repetitions {
        let mut sink = CountingSink::default();
        let started = Instant::now();
        runtime
            .block_on(handle_upload_pack_http(
                &dataset.repo_path,
                &request[..],
                &mut sink,
            ))
            .expect("upload-pack (clone) failed");
        let ms = elapsed_ms(started);
        assert!(sink.bytes > 0, "clone produced no bytes");

        record(
            &mut samples,
            dataset.spec.name,
            Operation::Clone,
            rep,
            ms,
            sink.bytes,
        );
    }

    OpResult {
        operation: Operation::Clone,
        samples,
    }
}

// ── 4. log first page ───────────────────────────────────────────────

fn measure_log(dataset: &Dataset, repetitions: usize) -> OpResult {
    let mut samples = Vec::with_capacity(repetitions);
    for rep in 0..repetitions {
        let started = Instant::now();
        let count = log_first_page(&dataset.repo_path, 100).expect("commit log failed");
        let ms = elapsed_ms(started);
        assert_eq!(count, 100, "log page must contain 100 commits");

        record(
            &mut samples,
            dataset.spec.name,
            Operation::LogFirstPage,
            rep,
            ms,
            count as u64,
        );
    }

    OpResult {
        operation: Operation::LogFirstPage,
        samples,
    }
}

/// Same shape as `rg-http`'s `repo_content::get_commit_log`: walk from HEAD,
/// take `limit` commit ids, then parse each one (message + author).
fn log_first_page(repo_path: &Path, limit: usize) -> anyhow::Result<usize> {
    let repo = gix::open(repo_path)?;
    let head = repo.rev_parse_single("HEAD")?.detach();
    let mut count = 0;
    for info in repo.rev_walk([head]).all()? {
        let info = info?;
        let commit = repo.find_object(info.id)?.into_commit();
        let _message = commit.message_raw()?;
        let author = commit.author()?;
        let _ = (author.name, author.email, author.time);
        count += 1;
        if count == limit {
            break;
        }
    }
    Ok(count)
}

// ── 5/6. tree listings ──────────────────────────────────────────────

fn measure_tree(
    dataset: &Dataset,
    dir: &str,
    operation: Operation,
    repetitions: usize,
) -> OpResult {
    let mut samples = Vec::with_capacity(repetitions);
    for rep in 0..repetitions {
        let started = Instant::now();
        let listing = rg_git::ops::list_tree_files(&dataset.repo_path, "HEAD", dir, usize::MAX)
            .expect("tree listing failed")
            .expect("directory must exist");
        let ms = elapsed_ms(started);
        assert!(!listing.entries.is_empty(), "tree listing is empty");

        record(
            &mut samples,
            dataset.spec.name,
            operation,
            rep,
            ms,
            listing.entries.len() as u64,
        );
    }

    OpResult {
        operation,
        samples,
    }
}

// ── Wire helpers ────────────────────────────────────────────────────

fn pkt_line(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() + 4);
    out.extend_from_slice(format!("{:04x}", text.len() + 4).as_bytes());
    out.extend_from_slice(text.as_bytes());
    out
}

/// receive-pack request: one update command, flush, then the packfile.
fn push_request(old_sha: &str, new_sha: &str, pack: &[u8]) -> Vec<u8> {
    let mut request = pkt_line(&format!(
        "{old_sha} {new_sha} refs/heads/main\0report-status side-band-64k\n"
    ));
    request.extend_from_slice(b"0000");
    request.extend_from_slice(pack);
    request
}

/// upload-pack request: one `want` line plus a flush, i.e. a full clone.
fn want_request(tip: &str) -> Vec<u8> {
    let mut request = pkt_line(&format!("want {tip} side-band-64k\n"));
    request.extend_from_slice(b"0000");
    request
}

/// Discards everything written and counts it, so a clone of a 100 MiB pack
/// does not need 100 MiB of test-process memory.
#[derive(Default)]
struct CountingSink {
    bytes: u64,
}

impl tokio::io::AsyncWrite for CountingSink {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.bytes += buf.len() as u64;
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

// ── Reporting ───────────────────────────────────────────────────────

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn record(
    samples: &mut Vec<Sample>,
    dataset: &str,
    operation: Operation,
    rep: usize,
    ms: f64,
    bytes: u64,
) {
    println!(
        "[perf] sample dataset={dataset} op={} rep={} ms={ms:.1} bytes={bytes}",
        operation.as_str(),
        rep + 1
    );
    samples.push(Sample { ms, bytes });
}

fn print_summary(name: DatasetName, spec: &DatasetSpec, results: &[OpResult]) {
    println!("[perf] summary begin dataset={}", name.as_str());
    println!("| dataset | operation | median_ms | p50_ms | p95_ms | bytes | n |");
    println!("| --- | --- | --- | --- | --- | --- | --- |");
    for result in results {
        let median = result.p50();
        println!(
            "| {} | {} | {:.1} | {:.1} | {:.1} | {} | {} |",
            name.as_str(),
            result.operation.as_str(),
            median,
            result.p50(),
            result.p95(),
            result.bytes(),
            result.samples.len()
        );
        println!(
            "[perf] summary dataset={} op={} n={} median_ms={:.1} p50_ms={:.1} p95_ms={:.1} bytes={}",
            name.as_str(),
            result.operation.as_str(),
            result.samples.len(),
            median,
            result.p50(),
            result.p95(),
            result.bytes()
        );
    }
    println!(
        "[perf] summary end dataset={} commits={} files={} file_bytes={} dirs={}",
        name.as_str(),
        spec.commits,
        spec.files,
        spec.file_bytes,
        spec.dirs
    );
}

fn check_thresholds(name: DatasetName, results: &[OpResult], repetitions: usize) {
    let mut violations = Vec::new();
    for result in results {
        let measured = result.p50();
        match threshold_for(name, result.operation) {
            Some(threshold) => {
                let verdict = if measured <= threshold as f64 { "ok" } else { "FAIL" };
                println!(
                    "[perf] threshold dataset={} op={} measured_ms={:.1} threshold_ms={} verdict={verdict} \
                     n={} p95_ms={:.1}",
                    name.as_str(),
                    result.operation.as_str(),
                    measured,
                    threshold,
                    repetitions,
                    result.p95()
                );
                if measured > threshold as f64 {
                    violations.push(format!(
                        "{}/{}: measured {:.1} ms (p95 {:.1} ms, n={}) > threshold {} ms",
                        name.as_str(),
                        result.operation.as_str(),
                        measured,
                        result.p95(),
                        repetitions,
                        threshold
                    ));
                }
            }
            None => {
                println!(
                    "[perf] threshold dataset={} op={} measured_ms={:.1} verdict=uncalibrated (no THRESHOLDS row)",
                    name.as_str(),
                    result.operation.as_str(),
                    measured
                );
                if name != DatasetName::Large {
                    violations.push(format!(
                        "{}/{}: measured {:.1} ms but THRESHOLDS has no row — add median×1.5",
                        name.as_str(),
                        result.operation.as_str(),
                        measured
                    ));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "performance baseline failed:\n{}",
        violations.join("\n")
    );
}

// ── Environment fingerprint ─────────────────────────────────────────

fn environment_fingerprint() -> String {
    format!(
        "cpu=\"{}\" cores={} os={} arch={} profile={} rustc=\"{}\" disk=\"{}\"",
        cpu_model(),
        std::thread::available_parallelism()
            .map(|value| value.get())
            .unwrap_or(0),
        std::env::consts::OS,
        std::env::consts::ARCH,
        if cfg!(debug_assertions) { "dev" } else { "release" },
        command_first_line("rustc", &["-V"]).unwrap_or_else(|| "unknown".to_string()),
        disk_kind(),
    )
}

fn command_first_line(program: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(|line| line.trim().to_string())
}

fn cpu_model() -> String {
    if cfg!(target_os = "macos") {
        if let Some(model) = command_first_line("sysctl", &["-n", "machdep.cpu.brand_string"]) {
            if !model.is_empty() {
                return model;
            }
        }
    }
    if let Ok(cpuinfo) = std::fs::read_to_string("/proc/cpuinfo") {
        for line in cpuinfo.lines() {
            if let Some(value) = line.strip_prefix("model name") {
                if let Some(name) = value.split(':').nth(1) {
                    return name.trim().to_string();
                }
            }
        }
    }
    "unknown".to_string()
}

fn disk_kind() -> String {
    if cfg!(target_os = "macos") {
        if let Ok(output) = std::process::Command::new("diskutil")
            .args(["info", "/"])
            .output()
        {
            let text = String::from_utf8_lossy(&output.stdout);
            let solid_state = text
                .lines()
                .find(|line| line.trim_start().starts_with("Solid State:"))
                .map(|line| line.trim().to_string())
                .unwrap_or_else(|| "Solid State: unknown".to_string());
            return format!("macOS APFS ({solid_state})");
        }
    }
    if let Ok(output) = std::process::Command::new("df").args(["-T", "."]).output() {
        if let Some(line) = String::from_utf8_lossy(&output.stdout).lines().nth(1) {
            if let Some(fs) = line.split_whitespace().next() {
                return fs.to_string();
            }
        }
    }
    "unknown".to_string()
}

// ── Tests ───────────────────────────────────────────────────────────

#[test]
#[ignore = "performance baseline: run explicitly with --ignored --nocapture"]
fn perf_baseline_small() {
    run_dataset(&perf_fixture::SMALL, DatasetName::Small, 3);
}

#[test]
#[ignore = "performance baseline: run explicitly with --ignored --nocapture"]
fn perf_baseline_medium() {
    run_dataset(&perf_fixture::MEDIUM, DatasetName::Medium, 3);
}

/// ~2 GiB of loose objects and several minutes of work. Manual only.
#[test]
#[ignore = "large dataset: manual only, set IRONFORGE_PERF_LARGE=1"]
fn perf_baseline_large() {
    if std::env::var("IRONFORGE_PERF_LARGE").as_deref() != Ok("1") {
        println!(
            "[perf] skipped dataset=large reason=manual-only (set IRONFORGE_PERF_LARGE=1 to run, ~2 GiB)"
        );
        return;
    }
    run_dataset(&perf_fixture::LARGE, DatasetName::Large, 1);
}

/// Materialise a dataset on disk so another server (e.g. Gitea) can be fed
/// exactly the same objects. Used by the cross-server section of
/// `docs/plans/T32-baseline-report.md`; not part of the baseline itself.
///
/// ```text
/// IRONFORGE_PERF_KEEP_DIR=/tmp/t32-medium IRONFORGE_PERF_DATASET=medium \
///   cargo test -p rg-git --test perf_baseline -- --ignored --nocapture perf_materialize_dataset
/// ```
#[test]
#[ignore = "manual: materialise a dataset for a cross-server comparison"]
fn perf_materialize_dataset() {
    let Ok(dir) = std::env::var("IRONFORGE_PERF_KEEP_DIR") else {
        println!(
            "[perf] skipped materialise reason=no-target (set IRONFORGE_PERF_KEEP_DIR and \
             IRONFORGE_PERF_DATASET=small|medium|large)"
        );
        return;
    };
    let requested =
        std::env::var("IRONFORGE_PERF_DATASET").unwrap_or_else(|_| "small".to_string());
    let spec = match requested.as_str() {
        "small" => &perf_fixture::SMALL,
        "medium" => &perf_fixture::MEDIUM,
        "large" => &perf_fixture::LARGE,
        other => panic!("unknown IRONFORGE_PERF_DATASET={other} (small|medium|large)"),
    };

    let dataset = perf_fixture::build_at(spec, Path::new(&dir));
    let (digest, entries) = perf_fixture::content_digest(&dataset.repo_path, "HEAD");
    println!(
        "[perf] materialised dataset={} repo={} tip={} parent_tip={} entries={} digest={} \
         build_ms={:.1} disk_mib={:.1}",
        spec.name,
        dataset.repo_path.display(),
        dataset.tip,
        dataset.parent_tip,
        entries,
        digest,
        dataset.build_ms,
        perf_fixture::mib(dataset.disk_bytes)
    );
}

/// Two generations of the same spec must be byte-identical, or the numbers
/// above are not reproducible.
#[test]
#[ignore = "performance baseline: run explicitly with --ignored --nocapture"]
fn perf_dataset_is_deterministic() {
    let first = perf_fixture::build(&perf_fixture::SMALL);
    let second = perf_fixture::build(&perf_fixture::SMALL);

    let (first_digest, first_files) = perf_fixture::content_digest(&first.repo_path, "HEAD");
    let (second_digest, second_files) = perf_fixture::content_digest(&second.repo_path, "HEAD");

    println!(
        "[perf] determinism first tip={} digest={} entries={} build_ms={:.1}",
        first.tip, first_digest, first_files, first.build_ms
    );
    println!(
        "[perf] determinism second tip={} digest={} entries={} build_ms={:.1}",
        second.tip, second_digest, second_files, second.build_ms
    );

    assert_eq!(first.tip, second.tip, "tip SHA differs between generations");
    assert_eq!(
        first_digest, second_digest,
        "content digest differs between generations"
    );
    assert_eq!(first_files, perf_fixture::SMALL.files);
    assert_eq!(first_files, second_files);
}
