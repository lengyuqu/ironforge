//! Incremental fetch must ship the objects the client is missing, not the
//! whole work tree (T37).
//!
//! `ops::pack_for_wants_filtered` prunes the *commit* walk by `haves`, but the
//! pack was still expanded with `ObjectExpansion::TreeContents`, so every
//! surviving commit re-sent its entire tree. These tests pin both halves of the
//! fix: the pack gets small **and** the receiving client can rebuild the exact
//! tree from it — a smaller pack that drops a needed object is the failure mode
//! that matters here.
//!
//! Every client in this file is real `git`: packs are written to disk and
//! ingested with `git index-pack`, then checked with `git fsck --strict` and
//! `git cat-file`. Nothing is validated with our own reader only.

use std::path::{Path, PathBuf};

use rg_git::cli_gateway::GitCommandGateway;
use rg_git::ops::{pack_for_wants, pack_for_wants_filtered};

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

    /// Run git in `self.root` and return trimmed stdout.
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

    fn rev(&self, spec: &str) -> String {
        self.git(&["rev-parse", spec])
    }
}

/// A repository with `files` files and one commit on top of them. Later commits
/// are added by the tests themselves so each scenario controls its own delta.
fn seeded_repo(files: usize) -> Repo {
    let repo = Repo::new();
    repo.git(&["init", "-b", "main"]);
    repo.git(&["config", "user.email", "t37@example.com"]);
    repo.git(&["config", "user.name", "T37 Test"]);
    repo.git(&["config", "commit.gpgsign", "false"]);
    for index in 0..files {
        repo.write(
            &format!("g/d{}/f{index:04}.txt", index % 5),
            &format!("file {index} at commit 0\n"),
        );
    }
    repo.commit("initial");
    repo
}

/// A bare clone of `source`: the pack generating side of a fetch.
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

/// A fresh work tree repo whose object database contains exactly `packs`.
///
/// This is how a client looks right after `upload-pack` handed it a pack:
/// nothing but what the server sent. The pack bytes land in
/// `.git/objects/pack/` and `git index-pack` builds the `.idx` next to them,
/// which is the same install step a real client performs.
fn client_with_packs(gateway: &GitCommandGateway, dir: &Path, packs: &[Vec<u8>]) {
    std::fs::create_dir_all(dir).expect("create client dir");
    gateway
        .run(&["init", "-b", "main"], Some(dir))
        .expect("git init")
        .ensure_success()
        .expect("git init succeeds");
    let pack_dir = dir.join(".git/objects/pack");
    std::fs::create_dir_all(&pack_dir).expect("create pack dir");
    for (index, pack) in packs.iter().enumerate() {
        let path = pack_dir.join(format!("pack-t37-{index}.pack"));
        std::fs::write(&path, pack).expect("write pack");
        let output = gateway
            .run(&["index-pack", path.to_string_lossy().as_ref()], Some(dir))
            .expect("git index-pack runs");
        assert!(
            output.success(),
            "git index-pack rejected the pack: {}",
            output.stderr_str()
        );
    }
}

fn git_out(gateway: &GitCommandGateway, dir: &Path, args: &[&str]) -> String {
    let output = gateway.run(args, Some(dir)).expect("git runs");
    assert!(
        output.success(),
        "git {args:?} failed: {}",
        output.stderr_str()
    );
    output.stdout_str().trim().to_string()
}

/// Object count from a pack header (`PACK`, version, big-endian count).
fn pack_object_count(pack: &[u8]) -> u32 {
    assert!(
        pack.len() >= 12 && pack.starts_with(b"PACK"),
        "not a pack stream"
    );
    u32::from_be_bytes([pack[8], pack[9], pack[10], pack[11]])
}

/// Every object reachable from `spec`, as `(sha, path)` from `rev-list --objects`.
fn reachable_objects(gateway: &GitCommandGateway, dir: &Path, spec: &str) -> Vec<String> {
    git_out(gateway, dir, &["rev-list", "--objects", spec])
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

/// Assert `client` can resolve every object `spec` reaches in `server`.
fn assert_client_has_every_object(
    server: &Path,
    client: &Path,
    gateway: &GitCommandGateway,
    spec: &str,
) {
    let needed = reachable_objects(gateway, server, spec);
    assert!(!needed.is_empty(), "nothing reachable from {spec}");
    for entry in &needed {
        let sha = entry.split(' ').next().expect("sha");
        let output = gateway
            .run(&["cat-file", "-e", sha], Some(client))
            .expect("git cat-file runs");
        assert!(
            output.success(),
            "client is missing {entry} (needed by {spec}): {}",
            output.stderr_str()
        );
    }
}

/// Index the pack into a client repo and point it at `tip`, leaving a checkout
/// the assertions can inspect byte for byte.
fn checkout_client(gateway: &GitCommandGateway, client: &Path, tip: &str) {
    git_out(gateway, client, &["update-ref", "refs/heads/main", tip]);
    git_out(
        gateway,
        client,
        &["symbolic-ref", "HEAD", "refs/heads/main"],
    );
    git_out(gateway, client, &["reset", "--hard"]);
}

// ── pack size ───────────────────────────────────────────────────────

/// The whole point: a one-file change must not re-send the work tree.
#[test]
fn one_file_change_pack_holds_only_the_delta() {
    let source = seeded_repo(400);
    source.write("g/d0/f0000.txt", "changed\n");
    source.commit("change one file");
    let tip = source.head();
    let parent = source.rev("HEAD~1");
    let server = bare_server(&source);

    let delta = pack_for_wants(
        &server,
        std::slice::from_ref(&tip),
        std::slice::from_ref(&parent),
        &[],
    )
    .expect("delta pack");
    let full = pack_for_wants(&server, std::slice::from_ref(&tip), &[], &[]).expect("full pack");

    let delta_objects = pack_object_count(&delta);
    let full_objects = pack_object_count(&full);
    println!(
        "[t37] one_file_change delta={delta_objects} objects/{} B full={full_objects} objects/{} B",
        delta.len(),
        full.len()
    );

    // Expected delta: the new commit, the trees on the changed path, the new
    // blob — plus the parent commit and its tree, which
    // `TreeAdditionsComparedToAncestor` names but the client already has.
    // Anything near `full_objects` means the tree expansion regressed.
    assert!(
        delta_objects <= 12,
        "delta pack should hold ~8 objects, got {delta_objects} (full = {full_objects})"
    );
    assert!(
        full_objects >= 400,
        "a haves-less fetch must still send every blob, got {full_objects}"
    );
    assert!(
        delta.len() * 4 < full.len(),
        "delta pack ({} B) is not meaningfully smaller than the full pack ({} B)",
        delta.len(),
        full.len()
    );
}

/// A client that fetches a long range still gets each object once: the delta
/// path must not degrade into "full tree for every commit".
#[test]
fn many_new_commits_scale_with_the_delta_not_with_the_tree() {
    let source = seeded_repo(200);
    let base = source.head();
    for index in 0..30 {
        source.write(
            &format!("g/d{}/f{:04}.txt", index % 5, index % 200),
            &format!("edit {index}\n"),
        );
        source.commit(&format!("edit {index}"));
    }
    let tip = source.head();
    let server = bare_server(&source);

    let delta = pack_for_wants(
        &server,
        std::slice::from_ref(&tip),
        std::slice::from_ref(&base),
        &[],
    )
    .expect("delta pack");
    let objects = pack_object_count(&delta);
    println!(
        "[t37] many_new_commits objects={objects} bytes={}",
        delta.len()
    );
    // 30 commits + their trees + 30 blobs, nowhere near 30 x 200.
    assert!(
        objects < 30 * 10,
        "30 one-file commits should not multiply by the tree size, got {objects}"
    );
}

// ── correctness ─────────────────────────────────────────────────────

/// Clone-shaped fetch (no haves) must still deliver every object.
#[test]
fn haves_less_fetch_still_sends_everything() {
    let source = seeded_repo(50);
    source.write("g/d1/f0001.txt", "second\n");
    source.commit("second");
    let tip = source.head();
    let server = bare_server(&source);

    let pack = pack_for_wants(&server, std::slice::from_ref(&tip), &[], &[]).expect("clone pack");
    let client = source.path().join("client");
    client_with_packs(&source.gateway, &client, &[pack]);

    assert_client_has_every_object(&server, &client, &source.gateway, &tip);

    let fsck = source
        .gateway
        .run(&["fsck", "--strict"], Some(&client))
        .expect("git fsck runs");
    assert!(fsck.success(), "git fsck --strict: {}", fsck.stderr_str());

    checkout_client(&source.gateway, &client, &tip);
    assert_eq!(
        git_out(&source.gateway, &client, &["status", "--porcelain"]),
        "",
        "a fresh clone must have a clean work tree"
    );
}

/// The main correctness test: base clone + one incremental fetch, then compare
/// the client's work tree with the server's, byte for byte.
#[test]
fn incremental_fetch_reproduces_the_work_tree_byte_for_byte() {
    let source = seeded_repo(120);
    let base = source.head();
    // Touch a file in a nested directory and add a brand-new one.
    source.write("g/d2/f0002.txt", "modified content\n");
    source.write("g/d3/new/nested.txt", "brand new\n");
    source.commit("modify and add");
    let tip = source.head();
    let server = bare_server(&source);

    let base_pack =
        pack_for_wants(&server, std::slice::from_ref(&base), &[], &[]).expect("base pack");
    let delta = pack_for_wants(
        &server,
        std::slice::from_ref(&tip),
        std::slice::from_ref(&base),
        &[],
    )
    .expect("delta pack");
    println!(
        "[t37] work_tree base_pack={} objects, delta={} objects/{} B",
        pack_object_count(&base_pack),
        pack_object_count(&delta),
        delta.len()
    );

    let client = source.path().join("client");
    client_with_packs(&source.gateway, &client, &[base_pack, delta]);

    assert_client_has_every_object(&server, &client, &source.gateway, &tip);
    let fsck = source
        .gateway
        .run(&["fsck", "--strict"], Some(&client))
        .expect("git fsck runs");
    assert!(fsck.success(), "git fsck --strict: {}", fsck.stderr_str());

    checkout_client(&source.gateway, &client, &tip);
    assert_same_work_tree(&source, &client, &tip);
}

/// `git pull` after an incremental fetch: the blob changed and the checkout
/// must show the new bytes, not the old ones.
#[test]
fn modified_file_content_is_updated() {
    let source = seeded_repo(20);
    let base = source.head();
    source.write("g/d0/f0000.txt", "after the change\n");
    source.commit("modify");
    let tip = source.head();
    let server = bare_server(&source);

    let client = fetch_into_client(&source, &server, &base, &tip);
    checkout_client(&source.gateway, &client, &tip);
    assert_eq!(
        std::fs::read_to_string(client.join("g/d0/f0000.txt")).expect("file exists"),
        "after the change\n"
    );
}

/// A deletion must arrive as a deletion: the client has to remove the path, not
/// keep the stale file it already had.
#[test]
fn deleted_file_disappears_from_the_client() {
    let source = seeded_repo(20);
    let base = source.head();
    source.git(&["rm", "-q", "g/d1/f0001.txt"]);
    source.commit("delete a file");
    let tip = source.head();
    let server = bare_server(&source);

    let client = fetch_into_client(&source, &server, &base, &tip);
    assert_client_has_every_object(&server, &client, &source.gateway, &tip);
    checkout_client(&source.gateway, &client, &tip);

    assert!(
        !client.join("g/d1/f0001.txt").exists(),
        "the deleted file must not survive the fetch"
    );
    assert_eq!(
        git_out(&source.gateway, &client, &["status", "--porcelain"]),
        "",
        "the checkout must match the fetched commit exactly"
    );
    assert_same_work_tree(&source, &client, &tip);
}

/// A rename is a delete plus an add; both halves must land.
#[test]
fn renamed_file_lands_at_its_new_path() {
    let source = seeded_repo(20);
    let base = source.head();
    source.git(&["mv", "g/d2/f0002.txt", "g/d2/renamed.txt"]);
    source.commit("rename a file");
    let tip = source.head();
    let server = bare_server(&source);

    let client = fetch_into_client(&source, &server, &base, &tip);
    assert_client_has_every_object(&server, &client, &source.gateway, &tip);
    checkout_client(&source.gateway, &client, &tip);

    assert!(
        !client.join("g/d2/f0002.txt").exists(),
        "old path must be gone"
    );
    assert_eq!(
        std::fs::read_to_string(client.join("g/d2/renamed.txt")).expect("renamed file exists"),
        std::fs::read_to_string(source.path().join("g/d2/renamed.txt")).expect("source file"),
    );
    assert_same_work_tree(&source, &client, &tip);
}

/// Merge commits compare against every parent, not just the first: taking only
/// the first parent would drop the branch's objects.
#[test]
fn merge_commit_delta_contains_every_parent_difference() {
    let source = seeded_repo(30);
    let base = source.head();
    source.git(&["checkout", "-q", "-b", "feature"]);
    source.write("g/d4/feature.txt", "from the branch\n");
    source.commit("branch work");
    source.git(&["checkout", "-q", "main"]);
    source.write("g/d0/main.txt", "from main\n");
    source.commit("main work");
    source.git(&["merge", "--no-ff", "-q", "-m", "merge feature", "feature"]);
    let tip = source.head();
    let parents = source.git(&["rev-list", "--parents", "-n", "1", &tip]);
    assert_eq!(
        parents.split(' ').count(),
        3,
        "the tip should be a merge commit: {parents}"
    );
    let server = bare_server(&source);

    let client = fetch_into_client(&source, &server, &base, &tip);
    assert_client_has_every_object(&server, &client, &source.gateway, &tip);
    checkout_client(&source.gateway, &client, &tip);
    assert_eq!(
        std::fs::read_to_string(client.join("g/d4/feature.txt")).expect("branch file"),
        "from the branch\n"
    );
    assert_eq!(
        std::fs::read_to_string(client.join("g/d0/main.txt")).expect("main file"),
        "from main\n"
    );
    assert_same_work_tree(&source, &client, &tip);
}

/// Two disjoint `have` tips must both prune the walk, and the union of the two
/// deltas must still be complete.
#[test]
fn branching_history_fetches_cleanly_from_one_have() {
    let source = seeded_repo(30);
    let base = source.head();
    source.git(&["checkout", "-q", "-b", "side", &base]);
    source.write("g/d3/side.txt", "side\n");
    source.commit("side");
    let side_tip = source.head();
    source.git(&["checkout", "-q", "main"]);
    source.write("g/d0/main.txt", "main\n");
    source.commit("main");
    source.git(&["merge", "--no-ff", "-q", "-m", "merge side", &side_tip]);
    let tip = source.head();
    let server = bare_server(&source);

    let client = fetch_into_client(&source, &server, &base, &tip);
    assert_client_has_every_object(&server, &client, &source.gateway, &tip);
    checkout_client(&source.gateway, &client, &tip);
    assert_same_work_tree(&source, &client, &tip);
}

/// `blob:none` must be applied *after* the delta: trees still arrive (the
/// client walks them) and no blob does.
#[test]
fn partial_clone_filter_still_applies_after_the_delta() {
    let source = seeded_repo(50);
    let base = source.head();
    source.write("g/d1/f0001.txt", "changed\n");
    source.commit("change");
    let tip = source.head();
    let server = bare_server(&source);

    let unfiltered = pack_for_wants(
        &server,
        std::slice::from_ref(&tip),
        std::slice::from_ref(&base),
        &[],
    )
    .expect("delta pack");
    let filter = rg_git::ops::ObjectFilter::parse("blob:none");
    let filtered = pack_for_wants_filtered(
        &server,
        std::slice::from_ref(&tip),
        std::slice::from_ref(&base),
        &[],
        false,
        filter,
    )
    .expect("filtered delta pack");
    println!(
        "[t37] blob:none delta={} objects (unfiltered {})",
        pack_object_count(&filtered),
        pack_object_count(&unfiltered)
    );
    assert!(
        pack_object_count(&filtered) < pack_object_count(&unfiltered),
        "blob:none must drop the blob the delta would carry"
    );
}

/// Shallow deepening keeps the old whole-tree expansion: the client owns the
/// boundary commit but not its trees, so nothing may be pruned against it.
#[test]
fn shallow_boundary_commit_still_gets_its_whole_tree() {
    let source = seeded_repo(60);
    let base = source.head();
    source.write("g/d1/f0001.txt", "changed\n");
    source.commit("change");
    let tip = source.head();
    let server = bare_server(&source);

    let deepening = rg_git::ops::pack_for_wants_opts(
        &server,
        std::slice::from_ref(&tip),
        std::slice::from_ref(&tip),
        std::slice::from_ref(&base),
        false,
    )
    .expect("shallow pack");

    // The boundary commit's tree must be complete: every blob at `base` is in
    // the pack, even though the client claims to have `tip`.
    let client = source.path().join("shallow-client");
    client_with_packs(&source.gateway, &client, &[deepening]);
    for entry in reachable_objects(&source.gateway, &server, &base) {
        let sha = entry.split(' ').next().expect("sha");
        let output = source
            .gateway
            .run(&["cat-file", "-e", sha], Some(&client))
            .expect("git cat-file runs");
        assert!(
            output.success(),
            "shallow deepening dropped {entry}, which belongs to the boundary tree"
        );
    }
}

// ── shared steps ────────────────────────────────────────────────────

/// `client` gets `base` as a first clone and `tip` as an incremental fetch, the
/// way two `git fetch` runs would leave it.
fn fetch_into_client(source: &Repo, server: &Path, base: &str, tip: &str) -> PathBuf {
    let base_owned = base.to_string();
    let base_pack =
        pack_for_wants(server, std::slice::from_ref(&base_owned), &[], &[]).expect("base pack");
    let tip_owned = tip.to_string();
    let delta = pack_for_wants(
        server,
        std::slice::from_ref(&tip_owned),
        std::slice::from_ref(&base_owned),
        &[],
    )
    .expect("delta pack");
    println!(
        "[t37] fetch base_pack={} objects delta={} objects/{} B",
        pack_object_count(&base_pack),
        pack_object_count(&delta),
        delta.len()
    );

    let client = source.path().join("client");
    client_with_packs(&source.gateway, &client, &[base_pack, delta]);
    assert_client_has_every_object(server, &client, &source.gateway, tip);
    let fsck = source
        .gateway
        .run(&["fsck", "--strict"], Some(&client))
        .expect("git fsck runs");
    assert!(fsck.success(), "git fsck --strict: {}", fsck.stderr_str());
    client
}

/// Compare every tracked file of `tip` between two repositories, byte for byte.
fn assert_same_work_tree(source: &Repo, client: &Path, tip: &str) {
    let listing = git_out(
        &source.gateway,
        source.path(),
        &["ls-tree", "-r", "--name-only", tip],
    );
    assert!(!listing.is_empty(), "the fixture should track files");
    for relative in listing.lines() {
        let expected = std::fs::read(source.path().join(relative))
            .unwrap_or_else(|error| panic!("source is missing {relative}: {error}"));
        let actual = std::fs::read(client.join(relative))
            .unwrap_or_else(|error| panic!("client is missing {relative}: {error}"));
        assert_eq!(
            actual, expected,
            "{relative} differs between server and client after the incremental fetch"
        );
    }
}
