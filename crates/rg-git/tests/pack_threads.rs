//! T38: `IRONFORGE_PACK_THREADS` must only move speed and memory, never
//! change which objects a pack carries.
//!
//! Deliberately a test binary of its own: the test mutates the process
//! environment, and `std::env::set_var` must not race with a `env::var` on
//! another thread. With a single test in the file there is nothing else to race
//! with (the same reason the perf tests keep their env knobs to themselves).
//!
//! What this pins is the contract the operator relies on when they lower the
//! thread budget on a small host: the clone still gets every object, just
//! encoded by fewer threads.

use std::path::{Path, PathBuf};

use rg_git::cli_gateway::GitCommandGateway;
use rg_git::ops::pack_for_wants;

struct Fixture {
    _tmp: tempfile::TempDir,
    gateway: GitCommandGateway,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let tmp = tempfile::tempdir().expect("temp dir");
        let gateway = GitCommandGateway::new().expect("git CLI available");
        let root = tmp.path().to_path_buf();
        let fixture = Self {
            _tmp: tmp,
            gateway,
            root,
        };
        fixture.git(&["init", "-b", "main"]);
        fixture.git(&["config", "user.email", "t38@example.com"]);
        fixture.git(&["config", "user.name", "T38 Thread Budget"]);
        fixture.git(&["config", "commit.gpgsign", "false"]);
        for index in 0..120 {
            let path = fixture.root.join(format!("d{}/f{index:03}.txt", index % 4));
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, format!("file {index}\n{}", "z".repeat(900))).unwrap();
        }
        fixture.git(&["add", "-A"]);
        fixture.git(&["commit", "-m", "initial"]);
        fixture
    }

    fn git(&self, args: &[&str]) -> String {
        let output = self
            .gateway
            .run(args, Some(self.root.as_path()))
            .expect("git runs");
        assert!(
            output.success(),
            "git {args:?} failed: {}",
            output.stderr_str()
        );
        output.stdout_str().trim().to_string()
    }
}

/// A bare copy the server side packs from.
fn bare_server(fixture: &Fixture) -> PathBuf {
    let dir = fixture.root.join("server.git");
    fixture.git(&[
        "clone",
        "--bare",
        "--no-local",
        fixture.root.to_string_lossy().as_ref(),
        dir.to_string_lossy().as_ref(),
    ]);
    dir
}

/// `oid type size` for everything the pack installs, sorted.
fn client_objects(gateway: &GitCommandGateway, dir: &Path, pack: &[u8]) -> Vec<String> {
    std::fs::create_dir_all(dir).expect("create client dir");
    gateway
        .run(&["init", "-b", "main"], Some(dir))
        .expect("git init")
        .ensure_success()
        .expect("git init succeeds");
    let pack_dir = dir.join(".git/objects/pack");
    std::fs::create_dir_all(&pack_dir).expect("create pack dir");
    let path = pack_dir.join("pack-t38-threads.pack");
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

#[test]
fn single_thread_budget_produces_the_same_object_set() {
    let fixture = Fixture::new();
    let server = bare_server(&fixture);
    let tip = fixture.git(&["rev-parse", "HEAD"]);
    let wants = std::slice::from_ref(&tip);

    let default_pack = pack_for_wants(&server, wants, &[], &[]).expect("default pack");
    std::env::set_var("IRONFORGE_PACK_THREADS", "1");
    let single_thread_pack = pack_for_wants(&server, wants, &[], &[]).expect("one-thread pack");
    std::env::remove_var("IRONFORGE_PACK_THREADS");

    let gateway = GitCommandGateway::new().expect("git CLI available");
    let tmp = tempfile::tempdir().expect("temp dir");
    let default_objects = client_objects(&gateway, &tmp.path().join("default"), &default_pack);
    let single_objects = client_objects(&gateway, &tmp.path().join("single"), &single_thread_pack);

    assert!(
        default_objects.len() > 120,
        "expected a few hundred objects, got {}",
        default_objects.len()
    );
    assert_eq!(
        default_objects, single_objects,
        "IRONFORGE_PACK_THREADS=1 changed which objects the pack carries"
    );
    println!(
        "[t38] thread budget 1 vs default: {} objects, {} B vs {} B",
        default_objects.len(),
        default_pack.len(),
        single_thread_pack.len()
    );
}
