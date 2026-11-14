//! Temporary performance probe (not part of the committed suite).

mod common;

use common::{create_repo, register_bootstrap_admin, register_full};

fn git(args: &[&str], dir: Option<&std::path::Path>) {
    let gateway = rg_git::cli_gateway::global_gateway()
        .as_ref()
        .expect("global git gateway");
    let output = gateway.run(args, dir).expect("run git");
    output.ensure_success().expect("git command failed");
}

fn body(index: usize) -> String {
    let mut out = String::new();
    for n in 0..12 {
        out.push_str(&format!(
            "pub struct S{index}_{n} {{ pub a: u32, pub b: String }}\n\nimpl S{index}_{n} {{\n    pub fn new(a: u32) -> Self {{ Self {{ a, b: String::new() }} }}\n    pub fn get(&self) -> u32 {{ self.a }}\n}}\n\n"
        ));
    }
    out
}

#[tokio::test]
#[ignore = "performance probe, run explicitly"]
async fn probe_2000_files() {
    let (db, dir) = common::setup_test_db().await;
    let repo_root = dir.path().join("repos");
    std::fs::create_dir_all(&repo_root).unwrap();
    let app = rg_http::create_router_for_test(common::build_test_app_state(
        db.clone(),
        repo_root.clone(),
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        let _db = db;
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    register_bootstrap_admin(&base).await;
    let (token, _) = register_full(&base, "perf_owner", "perf_owner@example.com").await;
    create_repo(&base, &token, "perf").await;
    let bare = repo_root.join("perf_owner/perf.git");

    let worktree = tempfile::tempdir().unwrap();
    let path = worktree.path();
    git(
        &["init", "--initial-branch=main", &path.to_string_lossy()],
        None,
    );
    git(&["config", "user.email", "t@example.com"], Some(path));
    git(&["config", "user.name", "T"], Some(path));
    let count: usize = std::env::var("MAP_PERF_FILES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2000);
    for index in 0..count {
        let dir = path.join(format!("src/m{}", index % 40));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("f{index}.rs")), body(index)).unwrap();
    }
    git(&["add", "."], Some(path));
    git(&["commit", "-m", "seed"], Some(path));
    git(
        &["remote", "add", "origin", &bare.to_string_lossy()],
        Some(path),
    );
    git(&["push", "origin", "main"], Some(path));

    let client = reqwest::Client::new();
    for budget in [4000, 32000] {
        let url = format!("{base}/api/v1/ai/repos/perf_owner/perf/map?budget={budget}");
        let started = std::time::Instant::now();
        let resp = client.get(&url).bearer_auth(&token).send().await.unwrap();
        let status = resp.status();
        let text = resp.text().await.unwrap();
        eprintln!(
            "files={count} budget={budget} status={status} elapsed={:?} body_chars={}",
            started.elapsed(),
            text.len()
        );
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
            eprintln!(
                "  estimated={} truncated={} listed={} skipped={}",
                value["estimated_tokens"],
                value["truncated"],
                value["files"].as_array().map(|a| a.len()).unwrap_or(0),
                value["skipped"].as_array().map(|a| a.len()).unwrap_or(0)
            );
        }
    }
}
