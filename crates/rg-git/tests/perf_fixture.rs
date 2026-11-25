//! Deterministic datasets for the large-repository performance baseline (T32).
//!
//! This module is **not** a test target of its own: it is pulled into
//! `perf_baseline.rs` with `#[path = "perf_fixture.rs"] mod perf_fixture;`.
//! Cargo also auto-discovers every `tests/*.rs` file as a test target, so the
//! file is compiled twice (once standalone, once as a module); the
//! `#![allow(dead_code)]` below keeps both compilations warning-free.
//!
//! # What is generated
//!
//! A bare repository whose history is fully determined by a [`DatasetSpec`]:
//! `commits` commits, `files` files of exactly `file_bytes` bytes each, spread
//! over `dirs` leaf directories laid out as `g<top>/d<mid>/s<leaf>`.
//!
//! * The first commit creates every file; every later commit rewrites exactly
//!   one file (chosen deterministically from the commit index). Tree objects
//!   for untouched directories are reused, so generation cost is
//!   `O(files + commits)` object writes instead of `O(files * commits)`.
//! * File content is derived from `(path, commit index)` only. Commit
//!   timestamps are `BASE_TIMESTAMP + commit index`, so **two generations of
//!   the same spec produce byte-identical objects and the same tip SHA**.
//! * No `rand`, no `git` CLI: a fixed FNV-1a hash of the path seeds a 64-bit
//!   LCG that fills the file body.
//!
//! `LARGE` is ~1.6 GiB of file content (plus loose-object overhead) and is
//! meant to be run by hand only; see `perf_baseline.rs`.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

// ── Spec ────────────────────────────────────────────────────────────

/// Shape of a dataset. `name` is what shows up in every printed line.
pub struct DatasetSpec {
    pub name: &'static str,
    pub commits: usize,
    pub files: usize,
    /// Average file size; every file is exactly this long.
    pub file_bytes: usize,
    /// Number of leaf directories the files are spread over.
    pub dirs: usize,
}

pub const SMALL: DatasetSpec = DatasetSpec {
    name: "small",
    commits: 100,
    files: 2_000,
    file_bytes: 1024,
    dirs: 20,
};

pub const MEDIUM: DatasetSpec = DatasetSpec {
    name: "medium",
    commits: 2_000,
    files: 20_000,
    file_bytes: 4 * 1024,
    dirs: 200,
};

pub const LARGE: DatasetSpec = DatasetSpec {
    name: "large",
    commits: 20_000,
    files: 200_000,
    file_bytes: 8 * 1024,
    dirs: 1_000,
};

/// Fixed commit clock. A real clock would make two generations produce
/// different commit SHAs, which defeats the determinism check.
const BASE_TIMESTAMP: i64 = 1_700_000_000;

// ── Dataset handle ──────────────────────────────────────────────────

/// A generated dataset. Dropping it deletes the temporary directory.
pub struct Dataset {
    pub spec: &'static DatasetSpec,
    /// Bare repository containing the whole history.
    pub repo_path: PathBuf,
    /// Tip commit (commit `commits - 1`) as a hex SHA.
    pub tip: String,
    /// Parent of the tip (commit `commits - 2`); the "old" side of an
    /// incremental push. Equals `tip` for a single-commit spec.
    pub parent_tip: String,
    /// A leaf directory path (`g000/d01/s7`), used for the deep-tree probe.
    pub deep_dir: String,
    /// Sum of file sizes at the tip, in bytes.
    pub total_file_bytes: u64,
    /// Bytes actually used on disk by the bare repository (loose objects are
    /// zlib-compressed, so this is smaller than `total_file_bytes`).
    pub disk_bytes: u64,
    /// Wall-clock time spent generating the dataset.
    pub build_ms: f64,
    /// `Some` for [`build`] (directory removed on drop), `None` for
    /// [`build_at`] (caller owns the directory).
    _tmp: Option<tempfile::TempDir>,
}

impl Dataset {
    /// Open the generated repository.
    pub fn open(&self) -> gix::Repository {
        gix::open(&self.repo_path).expect("generated dataset must open")
    }
}

// ── Generation ──────────────────────────────────────────────────────

/// Generate `spec` into a fresh temporary directory.
pub fn build(spec: &'static DatasetSpec) -> Dataset {
    let tmp = tempfile::tempdir().expect("failed to create temp dir for dataset");
    let repo_path = tmp.path().join("source.git");
    let mut dataset = build_into(spec, &repo_path);
    dataset._tmp = Some(tmp);
    dataset
}

/// Generate `spec` into `dir/source.git`, **keeping** it after the call.
///
/// This is what makes a same-machine comparison against another server (e.g.
/// Gitea) possible: the reference implementation has to push/clone a dataset
/// that outlives the test process.
pub fn build_at(spec: &'static DatasetSpec, dir: &Path) -> Dataset {
    std::fs::create_dir_all(dir).expect("failed to create dataset directory");
    build_into(spec, &dir.join("source.git"))
}

fn build_into(spec: &'static DatasetSpec, repo_path: &Path) -> Dataset {
    let started = Instant::now();

    let repo = init_bare_repo(repo_path);

    // Directory tree with cached tree ids: only the spine of the changed file
    // is re-written on each commit.
    let mut root = DirNode::default();
    let mut tip: Option<gix::hash::ObjectId> = None;
    let mut parent_tip: Option<gix::hash::ObjectId> = None;

    for index in 0..spec.commits {
        let tree = if index == 0 {
            insert_all_files(&repo, &mut root, spec);
            root.materialize(&repo)
                .expect("failed to write the initial tree")
        } else {
            // One file per commit, deterministic and spread over the tree.
            let file_index = index.wrapping_mul(7919).wrapping_add(13) % spec.files;
            let parts = leaf_parts(file_index, spec);
            let name = file_name(file_index);
            let path = format!("{}/{}", parts.join("/"), name);
            let content = file_content(&path, index, spec.file_bytes);

            let blob = repo
                .write_blob(content.as_slice())
                .expect("failed to write blob")
                .detach();
            let leaf = root.leaf_mut(&parts);
            leaf.files.insert(name, blob);

            root.refresh(&repo, &parts)
                .expect("failed to write tree spine")
        };

        let time = format!("{} +0000", BASE_TIMESTAMP + index as i64);
        let signature = gix::actor::SignatureRef {
            name: "IronForge Perf".as_bytes().into(),
            email: "perf@ironforge.invalid".as_bytes().into(),
            time: time.as_str(),
        };
        let message = format!("perf dataset {} commit {index}", spec.name);
        let parents: Vec<gix::hash::ObjectId> = tip.into_iter().collect();
        let commit = repo
            .new_commit_as(signature, signature, message.as_str(), tree, parents)
            .expect("failed to write commit")
            .id()
            .detach();

        parent_tip = tip;
        tip = Some(commit);
    }

    let tip = tip.expect("a dataset must have at least one commit");
    repo.reference(
        "refs/heads/main",
        tip,
        gix::refs::transaction::PreviousValue::Any,
        format!("perf dataset {} tip", spec.name),
    )
    .expect("failed to write refs/heads/main");
    drop(repo);

    // `gix::init_bare` honours `init.defaultBranch` from the user's git
    // config, so pin HEAD explicitly to keep the dataset machine-independent.
    std::fs::write(repo_path.join("HEAD"), b"ref: refs/heads/main\n")
        .expect("failed to write HEAD");

    let total_file_bytes = (spec.files as u64) * (spec.file_bytes as u64);
    let build_ms = started.elapsed().as_secs_f64() * 1000.0;
    let disk_bytes = disk_usage(repo_path);

    Dataset {
        spec,
        repo_path: repo_path.to_path_buf(),
        tip: tip.to_string(),
        parent_tip: parent_tip
            .map(|id| id.to_string())
            .unwrap_or_else(|| tip.to_string()),
        deep_dir: leaf_parts(spec.files / 2, spec).join("/"),
        total_file_bytes,
        disk_bytes,
        build_ms,
        _tmp: None,
    }
}

/// Create a bare repository whose `HEAD` points at `refs/heads/main`.
pub fn init_bare_with_main(dir: &Path) -> PathBuf {
    let repo = init_bare_repo(dir);
    drop(repo);
    std::fs::write(dir.join("HEAD"), b"ref: refs/heads/main\n").expect("failed to write HEAD");
    dir.to_path_buf()
}

fn init_bare_repo(dir: &Path) -> gix::Repository {
    gix::init_bare(dir).expect("failed to init bare repository")
}

// ── Tree building ───────────────────────────────────────────────────

#[derive(Default)]
struct DirNode {
    files: BTreeMap<String, gix::hash::ObjectId>,
    dirs: BTreeMap<String, DirNode>,
    /// Tree id, valid as long as nothing below this node changed.
    cached: Option<gix::hash::ObjectId>,
}

impl DirNode {
    /// Write this node's tree object, recursing into every directory first.
    fn materialize(&mut self, repo: &gix::Repository) -> anyhow::Result<gix::hash::ObjectId> {
        for child in self.dirs.values_mut() {
            child.materialize(repo)?;
        }
        self.write(repo)
    }

    /// Re-write only the spine from the root down to `path`, reusing the
    /// cached tree ids of every untouched sibling.
    fn refresh(
        &mut self,
        repo: &gix::Repository,
        path: &[String],
    ) -> anyhow::Result<gix::hash::ObjectId> {
        if let Some((head, rest)) = path.split_first() {
            let child = self
                .dirs
                .get_mut(head)
                .ok_or_else(|| anyhow::anyhow!("missing directory '{head}' while writing tree"))?;
            child.refresh(repo, rest)?;
        }
        self.write(repo)
    }

    fn write(&mut self, repo: &gix::Repository) -> anyhow::Result<gix::hash::ObjectId> {
        // Git sorts tree entries as if directory names carried a trailing `/`,
        // so the sort key is built that way; the real name is kept separately.
        let mut entries: Vec<(Vec<u8>, String, gix::hash::ObjectId, bool)> = Vec::new();
        for (name, oid) in &self.files {
            entries.push((sort_key(name, false), name.clone(), *oid, false));
        }
        for (name, child) in &self.dirs {
            let id = child
                .cached
                .ok_or_else(|| anyhow::anyhow!("directory '{name}' was not materialised"))?;
            entries.push((sort_key(name, true), name.clone(), id, true));
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        let mut git_entries = Vec::with_capacity(entries.len());
        for (_, name, oid, is_tree) in entries {
            let mode = if is_tree { 0o040000u32 } else { 0o100644u32 };
            git_entries.push(gix::objs::tree::Entry {
                mode: gix::objs::tree::EntryMode::try_from(mode).expect("valid tree entry mode"),
                filename: name.into_bytes().into(),
                oid,
            });
        }

        let id = repo
            .write_object(gix::objs::Tree {
                entries: git_entries,
            })?
            .detach();
        self.cached = Some(id);
        Ok(id)
    }

    fn leaf_mut(&mut self, path: &[String]) -> &mut DirNode {
        let mut node = self;
        for part in path {
            node = node.dirs.entry(part.clone()).or_default();
        }
        node
    }
}

/// Sort key that reproduces git's tree ordering: the name, plus `/` when the
/// entry is a directory.
fn sort_key(name: &str, is_tree: bool) -> Vec<u8> {
    let mut key = name.as_bytes().to_vec();
    if is_tree {
        key.push(b'/');
    }
    key
}

fn insert_all_files(repo: &gix::Repository, root: &mut DirNode, spec: &'static DatasetSpec) {
    for index in 0..spec.files {
        let parts = leaf_parts(index, spec);
        let name = file_name(index);
        let path = format!("{}/{}", parts.join("/"), name);
        let content = file_content(&path, 0, spec.file_bytes);
        let blob = repo
            .write_blob(content.as_slice())
            .expect("failed to write blob")
            .detach();
        let leaf = root.leaf_mut(&parts);
        leaf.files.insert(name, blob);
    }
}

// ── Paths and content ───────────────────────────────────────────────

fn file_name(index: usize) -> String {
    format!("f{index:06}.txt")
}

/// Leaf directory (`g000`, `d01`, `s7`) for a file index. Files are spread
/// evenly over `spec.dirs` leaves, and leaves are spread over at most
/// `10 * 10` (mid, leaf) buckets under a top-level group.
fn leaf_parts(index: usize, spec: &'static DatasetSpec) -> Vec<String> {
    let dir_index = index.saturating_mul(spec.dirs) / spec.files.max(1);
    let leaf = dir_index % 10;
    let mid = (dir_index / 10) % 10;
    let top = dir_index / 100;
    vec![format!("g{top:03}"), format!("d{mid:02}"), format!("s{leaf}")]
}

/// File body derived only from `(path, commit index)`, so two runs produce
/// identical bytes. Printable ASCII keeps the objects compressible like real
/// source files instead of incompressible random data.
fn file_content(path: &str, commit_index: usize, len: usize) -> Vec<u8> {
    let mut state = fnv1a(path.as_bytes())
        ^ (commit_index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        | 1;
    let mut out = Vec::with_capacity(len);
    while out.len() < len {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let mut value = state;
        for _ in 0..8 {
            if out.len() == len {
                break;
            }
            out.push(b'a' + (value % 26) as u8);
            value /= 26;
        }
    }
    out
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

// ── Pack helpers ────────────────────────────────────────────────────

/// Pack containing only the objects added by `commit` relative to its first
/// parent — what `git push` of a one-file change actually sends.
///
/// `rg_git::ops::pack_for_wants` always expands commits with `TreeContents`,
/// which re-sends the whole tree of the new commit even when the client
/// already has it; a real client sends the delta, so the fixture builds the
/// delta itself. (The `TreeContents` behaviour is recorded as a known
/// bottleneck in `docs/plans/T32-baseline-report.md`.)
pub fn incremental_pack(repo_path: &Path, commit: &str) -> anyhow::Result<Vec<u8>> {
    use gix::odb::pack::data::output;
    use gix::progress::Discard;

    let repo = gix::open(repo_path)?;
    let id = repo.rev_parse_single(commit)?.detach();

    let should_interrupt = std::sync::atomic::AtomicBool::new(false);
    // Same requirement as `ops::generate_pack`: `count::objects` reaches
    // `Find::location_by_oid`, which asserts the handle prevents pack unload.
    let mut db = repo.objects.clone().into_inner();
    db.prevent_pack_unload();

    let input: Box<dyn Iterator<Item = Result<gix::hash::ObjectId, gix::Error>> + Send> =
        Box::new(std::iter::once(Ok(id)));
    let (counts, _outcome) = output::count::objects(
        db.clone(),
        input,
        &Discard,
        &should_interrupt,
        output::count::objects::Options {
            thread_limit: None,
            chunk_size: 10,
            input_object_expansion:
                output::count::objects::ObjectExpansion::TreeAdditionsComparedToAncestor,
        },
    )
    .map_err(|error| anyhow::anyhow!("failed to count incremental objects: {error}"))?;

    let num_entries = counts.len() as u32;
    let entries = output::entry::iter_from_counts(
        counts,
        db,
        Box::new(Discard),
        output::entry::iter_from_counts::Options {
            allow_thin_pack: false,
            ..Default::default()
        },
    )
    .map_err(|error| anyhow::anyhow!("failed to start incremental pack: {error}"))?;

    let mut pack = Vec::new();
    {
        use gix::parallel::InOrderIter;
        let in_order = InOrderIter::from(entries);
        let mut writer = output::bytes::FromEntriesIter::new(
            in_order,
            &mut pack,
            num_entries,
            gix::odb::pack::data::Version::V2,
            repo.object_hash(),
        );
        for chunk in &mut writer {
            chunk.map_err(|error| anyhow::anyhow!("failed to write incremental pack: {error}"))?;
        }
    }
    Ok(pack)
}

/// Object count from a pack header (`PACK`, version, big-endian count).
pub fn pack_object_count(pack: &[u8]) -> u32 {
    assert!(pack.len() >= 12 && pack.starts_with(b"PACK"), "not a pack stream");
    u32::from_be_bytes([pack[8], pack[9], pack[10], pack[11]])
}

// ── Verification helpers ────────────────────────────────────────────

/// Digest over `(path, blob sha)` of every file in `commitish`'s tree.
/// Blob SHAs are content-addressed, so an equal digest means equal content
/// for the whole working set. Returns the digest and the file count.
pub fn content_digest(repo_path: &Path, commitish: &str) -> (String, usize) {
    let listing = rg_git::ops::list_tree_files(repo_path, commitish, "", usize::MAX)
        .expect("failed to list tree for digest")
        .expect("commitish must have a tree");
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for entry in &listing.entries {
        for byte in entry.path.as_bytes().iter().chain([0u8].iter()) {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        for byte in entry.blob_sha.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    (format!("{hash:016x}"), listing.entries.len())
}

/// Bytes on disk under `path` (sum of file lengths, i.e. what the objects
/// actually occupy, not the apparent size of the tree).
pub fn disk_usage(path: &Path) -> u64 {
    walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.metadata().ok())
        .filter(|meta| meta.is_file())
        .map(|meta| meta.len())
        .sum()
}

/// MiB with one decimal, for the report.
pub fn mib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}
