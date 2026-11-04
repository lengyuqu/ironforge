//! Q4 — Package Registry rigor: yank closed loop (Q4.1) and version
//! deletion compensation (Q4.2).
//!
//! Q4.1: `supports_yank` capability gate — only package types whose
//! protocol gives the flag semantics (cargo sparse index) accept yank;
//! the sparse index must reflect the DB flag so cargo dependency
//! resolution rejects yanked versions. Yanked versions stay
//! downloadable (crates.io semantics: lockfile builds keep working).
//!
//! Q4.2: version deletion is backed by blob backup/restore compensation
//! (unit-tested in `package_registry::storage`); here we verify the
//! happy path removes DB records and blobs together.

mod common;

use std::io::Write;

use common::{create_repo, register_full, spawn_test_app_with_db};
use flate2::write::GzEncoder;
use flate2::Compression;
use reqwest::StatusCode;

fn crate_body(name: &str, version: &str) -> Vec<u8> {
    let cargo_toml = format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\n");
    let mut tar = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    let path = format!("{name}-{version}/Cargo.toml");
    header.set_path(&path).unwrap();
    header.set_size(cargo_toml.len() as u64);
    header.set_mode(0o644);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_cksum();
    tar.append(&header, cargo_toml.as_bytes()).unwrap();
    let tar_bytes = tar.into_inner().unwrap();

    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&tar_bytes).unwrap();
    encoder.finish().unwrap()
}

async fn publish_crate(base: &str, token: &str, owner: &str, repo: &str, name: &str, version: &str) {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/api/v1/repos/{owner}/{repo}/packages/cargo/publish"))
        .bearer_auth(token)
        .header(
            reqwest::header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{name}-{version}.crate\""),
        )
        .body(crate_body(name, version))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "publish failed: {}",
        resp.text().await.unwrap()
    );
}

fn yank_url(base: &str, owner: &str, repo: &str, pkg: &str, version: &str) -> String {
    format!("{base}/api/v1/repos/{owner}/{repo}/packages/cargo/{pkg}/{version}/yank")
}

/// Q4.1: yank → sparse index carries `"yanked":true` → un-yank flips it
/// back. The index is what cargo consults, so this is the closed loop
/// that makes `cargo install`/resolution refuse yanked versions.
#[tokio::test]
async fn cargo_yank_flows_into_sparse_index() {
    let (base, _db) = spawn_test_app_with_db().await;
    let (token, _) = register_full(&base, "yank_owner", "yank_owner@example.com").await;
    create_repo(&base, &token, "yank-repo").await;
    publish_crate(&base, &token, "yank_owner", "yank-repo", "yank-crate", "1.0.0").await;
    let client = reqwest::Client::new();

    // Baseline: index shows the version as not yanked
    let index = client
        .get(format!("{base}/api/v1/repos/yank_owner/yank-repo/packages/cargo/index/yank-crate"))
        .send()
        .await
        .unwrap();
    assert_eq!(index.status(), StatusCode::OK);
    let body = index.text().await.unwrap();
    assert!(body.contains("\"yanked\":false"), "baseline index: {body}");

    // Yank
    let resp = client
        .patch(yank_url(&base, "yank_owner", "yank-repo", "yank-crate", "1.0.0"))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "yank": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let index = client
        .get(format!("{base}/api/v1/repos/yank_owner/yank-repo/packages/cargo/index/yank-crate"))
        .send()
        .await
        .unwrap();
    let body = index.text().await.unwrap();
    assert!(body.contains("\"yanked\":true"), "yanked index: {body}");

    // Version detail also reflects the flag
    let detail = client
        .get(format!("{base}/api/v1/repos/yank_owner/yank-repo/packages/cargo/yank-crate/1.0.0"))
        .send()
        .await
        .unwrap();
    let detail = detail.json::<serde_json::Value>().await.unwrap();
    assert_eq!(detail["is_yanked"], serde_json::json!(true));

    // Yanked versions stay downloadable (crates.io semantics: existing
    // lockfiles keep building)
    let download = client
        .get(format!(
            "{base}/api/v1/repos/yank_owner/yank-repo/packages/cargo/yank-crate/1.0.0/yank-crate-1.0.0.crate"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(download.status(), StatusCode::OK);

    // Un-yank flips the index back
    let resp = client
        .patch(yank_url(&base, "yank_owner", "yank-repo", "yank-crate", "1.0.0"))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "yank": false }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let index = client
        .get(format!("{base}/api/v1/repos/yank_owner/yank-repo/packages/cargo/index/yank-crate"))
        .send()
        .await
        .unwrap();
    let body = index.text().await.unwrap();
    assert!(body.contains("\"yanked\":false"), "un-yanked index: {body}");
}

/// Q4.1: yank on a package type without protocol-level yank semantics is
/// rejected with 400 (generic packages must use version deletion).
#[tokio::test]
async fn yank_non_cargo_type_rejected() {
    let (base, _db) = spawn_test_app_with_db().await;
    let (token, _) = register_full(&base, "gen_owner", "gen_owner@example.com").await;
    create_repo(&base, &token, "gen-repo").await;

    let client = reqwest::Client::new();
    let published = client
        .post(format!(
            "{base}/api/v1/repos/gen_owner/gen-repo/packages/generic/publish?name=gen-pkg&version=1.0.0"
        ))
        .bearer_auth(&token)
        .header(
            reqwest::header::CONTENT_DISPOSITION,
            "attachment; filename=\"gen-pkg-1.0.0.bin\"",
        )
        .body(b"generic-bytes".to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(published.status(), StatusCode::CREATED);

    let resp = client
        .patch(format!("{base}/api/v1/repos/gen_owner/gen-repo/packages/generic/gen-pkg/1.0.0/yank"))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "yank": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = resp.json::<serde_json::Value>().await.unwrap();
    assert!(
        body.to_string().contains("not supported"),
        "unexpected error body: {body}"
    );
}

/// Q4.1: yank requires write access.
#[tokio::test]
async fn yank_requires_write_access() {
    let (base, _db) = spawn_test_app_with_db().await;
    let (token, _) = register_full(&base, "perm_owner", "perm_owner@example.com").await;
    create_repo(&base, &token, "perm-repo").await;
    publish_crate(&base, &token, "perm_owner", "perm-repo", "perm-crate", "1.0.0").await;

    let client = reqwest::Client::new();
    let resp = client
        .patch(yank_url(&base, "perm_owner", "perm-repo", "perm-crate", "1.0.0"))
        .json(&serde_json::json!({ "yank": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// Q4.2 happy path: delete removes DB records and blob together —
/// version detail and download both 404 afterwards. (The restore-on-failure
/// branch is unit-tested via `PackageStorage::backup_file`/`restore_file`
/// round-trip in rg-core.)
#[tokio::test]
async fn delete_version_removes_records_and_files() {
    let (base, _db) = spawn_test_app_with_db().await;
    let (token, _) = register_full(&base, "del_owner", "del_owner@example.com").await;
    create_repo(&base, &token, "del-repo").await;
    publish_crate(&base, &token, "del_owner", "del-repo", "del-crate", "1.0.0").await;
    let client = reqwest::Client::new();

    let deleted = client
        .delete(format!(
            "{base}/api/v1/repos/del_owner/del-repo/packages/cargo/del-crate/1.0.0"
        ))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);

    let detail = client
        .get(format!("{base}/api/v1/repos/del_owner/del-repo/packages/cargo/del-crate/1.0.0"))
        .send()
        .await
        .unwrap();
    assert_eq!(detail.status(), StatusCode::NOT_FOUND);

    let download = client
        .get(format!(
            "{base}/api/v1/repos/del_owner/del-repo/packages/cargo/del-crate/1.0.0/del-crate-1.0.0.crate"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(download.status(), StatusCode::NOT_FOUND);

    // Sparse index no longer lists the deleted version
    let index = client
        .get(format!("{base}/api/v1/repos/del_owner/del-repo/packages/cargo/index/del-crate"))
        .send()
        .await
        .unwrap();
    assert_eq!(index.status(), StatusCode::OK);
    let body = index.text().await.unwrap();
    assert!(!body.contains("1.0.0"), "index after delete: {body}");
}
