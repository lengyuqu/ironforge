//! Server-side file commits can be checked first (`dry_run`) and made conditional
//! on the branch still being where the caller last saw it (`expected_sha`).
//!
//! A dry run runs the real checks and writes nothing; a moved branch is a 409
//! whose `error.reason` is `REF_ADVANCED` and whose `error.details.current_sha`
//! says where it is now.

mod common;
use common::{register_user, spawn_test_app_on_env_db};

use reqwest::{Client, StatusCode};
use serde_json::{json, Value};

const PW: &str = "Qz7$wRtm";

struct Fixture {
    base: String,
    jwt: String,
    owner: &'static str,
    client: Client,
}

impl Fixture {
    async fn new(owner: &'static str) -> Fixture {
        let base = spawn_test_app_on_env_db().await.0;
        let jwt = register_user(&base, owner, &format!("{owner}@example.com"), PW).await;
        let client = Client::new();
        let resp = client
            .post(format!("{base}/api/v1/repos"))
            .bearer_auth(&jwt)
            .json(&json!({"name": "work", "auto_init": true, "is_private": false}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);
        Fixture {
            base,
            jwt,
            owner,
            client,
        }
    }

    fn url(&self, suffix: &str) -> String {
        format!("{}/api/v1/repos/{}/work{suffix}", self.base, self.owner)
    }

    /// The tip of `main`, from the server's own git endpoint.
    fn tip(&self, branch: &str) -> Option<String> {
        let remote = format!("{}/git/{}/work", self.base, self.owner);
        let gateway = rg_git::cli_gateway::global_gateway().as_ref().unwrap();
        let out = gateway
            .run(
                &["ls-remote", &remote, &format!("refs/heads/{branch}")],
                None,
            )
            .unwrap();
        out.stdout_str()
            .split_whitespace()
            .next()
            .map(str::to_string)
    }

    async fn post(
        &self,
        path: &str,
        body: Value,
        header: Option<(&str, &str)>,
    ) -> (StatusCode, Value) {
        let mut req = self
            .client
            .post(self.url(&format!("/contents/{path}")))
            .bearer_auth(&self.jwt)
            .json(&body);
        if let Some((name, value)) = header {
            req = req.header(name, value);
        }
        let resp = req.send().await.unwrap();
        let status = resp.status();
        (status, resp.json().await.unwrap_or(Value::Null))
    }

    async fn delete(&self, query: &str) -> (StatusCode, Value) {
        let resp = self
            .client
            .delete(self.url(&format!("/contents/{query}")))
            .bearer_auth(&self.jwt)
            .send()
            .await
            .unwrap();
        let status = resp.status();
        (status, resp.json().await.unwrap_or(Value::Null))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dry_run_checks_and_reports_but_writes_nothing() {
    let f = Fixture::new("dryrun_user").await;
    let tip = f.tip("main").expect("main exists");

    // Body flag and header both mean dry run.
    for header in [None, Some(("x-dry-run", "1"))] {
        let body = if header.is_some() {
            json!({"branch": "main", "message": "m", "content": "hi\n"})
        } else {
            json!({"branch": "main", "message": "m", "content": "hi\n", "dry_run": true})
        };
        let (status, v) = f.post("note.txt", body, header).await;
        assert_eq!(status, StatusCode::OK, "{v}");
        assert_eq!(v["dry_run"], json!(true));
        let would = v["would"].as_array().unwrap();
        assert_eq!(would[0]["effect"], "ref_update");
        assert_eq!(would[0]["ref"], "refs/heads/main");
        assert_eq!(would[0]["old"], json!(tip));
        assert_eq!(would[0]["creates_branch"], json!(false));
        assert_eq!(would[1]["operation"], "create");
        assert!(v["not_evaluated"].as_array().unwrap().len() >= 3);
    }
    // Nothing moved and the file is not there.
    assert_eq!(f.tip("main"), Some(tip.clone()));
    let tree = f
        .client
        .get(f.url("/blob/note.txt"))
        .bearer_auth(&f.jwt)
        .send()
        .await
        .unwrap();
    assert_eq!(tree.status(), StatusCode::NOT_FOUND);

    // The checks are real: creating over an existing file, or updating with the
    // wrong blob sha, fails the same way as without dry_run.
    let (status, _) = f
        .post(
            "README.md",
            json!({"branch": "main", "message": "m", "content": "x\n", "dry_run": true}),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = f
        .post(
            "README.md",
            json!({"branch": "main", "message": "m", "content": "x\n", "sha": "0000000000000000000000000000000000000000", "dry_run": true}),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    // An invalid branch name is refused, not reported as fine.
    let (status, _) = f
        .post(
            "a.txt",
            json!({"branch": "bad..name", "message": "m", "content": "x\n", "dry_run": true}),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // A dry run on a branch that does not exist reports that it would be created,
    // and does not create it.
    let (status, v) = f
        .post(
            "a.txt",
            json!({"branch": "agent/new", "message": "m", "content": "x\n", "dry_run": true}),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["would"][0]["creates_branch"], json!(true));
    assert_eq!(f.tip("agent/new"), None);

    // The same request for real does write.
    let (status, v) = f
        .post(
            "note.txt",
            json!({"branch": "main", "message": "m", "content": "hi\n"}),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_ne!(f.tip("main"), Some(tip));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_commit_on_a_moved_branch_is_refused_with_the_current_tip() {
    let f = Fixture::new("expected_user").await;
    let tip = f.tip("main").unwrap();

    // At the expected tip: written, and the answer names the new commit.
    let (status, v) = f
        .post(
            "one.txt",
            json!({"branch": "main", "message": "one", "content": "1\n", "expected_sha": tip}),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let new_tip = v["commit_sha"].as_str().unwrap().to_string();
    assert_eq!(f.tip("main"), Some(new_tip.clone()));

    // Using the old tip again: nothing is written; the error says why and where.
    let (status, v) = f
        .post(
            "two.txt",
            json!({"branch": "main", "message": "two", "content": "2\n", "expected_sha": tip}),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(v["error"]["code"], "CONFLICT");
    assert_eq!(v["error"]["reason"], "REF_ADVANCED");
    assert_eq!(v["error"]["retryable"], json!(true));
    assert_eq!(v["error"]["details"]["current_sha"], json!(new_tip));
    assert_eq!(v["error"]["details"]["expected_sha"], json!(tip));
    assert!(v["error"]["hint"]
        .as_str()
        .unwrap()
        .contains("expected_sha"));
    assert_eq!(f.tip("main"), Some(new_tip.clone()));

    // A dry run is held to the same expectation.
    let (status, v) = f
        .post(
            "two.txt",
            json!({"branch": "main", "message": "two", "content": "2\n", "expected_sha": tip, "dry_run": true}),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(v["error"]["reason"], "REF_ADVANCED");

    // A missing branch is not "at" any expected tip.
    let (status, v) = f
        .post("x.txt", json!({"branch": "agent/none", "message": "m", "content": "x\n", "expected_sha": new_tip}), None)
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(v["error"]["reason"], "REF_ADVANCED");
    assert_eq!(v["error"]["details"]["current_sha"], Value::Null);

    // Without expected_sha the old behaviour is unchanged.
    let (status, _) = f
        .post(
            "three.txt",
            json!({"branch": "main", "message": "three", "content": "3\n"}),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_delete_honours_expected_sha_and_dry_run() {
    let f = Fixture::new("delete_user").await;
    let (status, v) = f
        .post(
            "gone.txt",
            json!({"branch": "main", "message": "add", "content": "bye\n"}),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let tip = v["commit_sha"].as_str().unwrap().to_string();
    // The blob sha, from the server.
    let blob: Value = f
        .client
        .get(f.url("/blob/gone.txt"))
        .bearer_auth(&f.jwt)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let blob = blob["sha"].as_str().unwrap().to_string();

    // Wrong expected tip: refused, file still there.
    let stale = "1111111111111111111111111111111111111111";
    let (status, v) = f
        .delete(&format!(
            "gone.txt?branch=main&message=rm&sha={blob}&expected_sha={stale}"
        ))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(v["error"]["reason"], "REF_ADVANCED");
    assert_eq!(v["error"]["details"]["current_sha"], json!(tip));

    // Dry run at the right tip: reports the delete, changes nothing.
    let (status, v) = f
        .delete(&format!(
            "gone.txt?branch=main&message=rm&sha={blob}&expected_sha={tip}&dry_run=true"
        ))
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["would"][1]["operation"], "delete");
    assert_eq!(f.tip("main"), Some(tip.clone()));

    // For real.
    let (status, v) = f
        .delete(&format!(
            "gone.txt?branch=main&message=rm&sha={blob}&expected_sha={tip}"
        ))
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_ne!(f.tip("main"), Some(tip));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_merge_can_be_dry_run_and_pinned_to_the_reviewed_head() {
    let f = Fixture::new("merge_user").await;
    // A branch with one commit, made through the contents API.
    let (status, v) = f
        .post(
            "feature.txt",
            json!({"branch": "agent/feature", "message": "feature", "content": "f\n"}),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let reviewed = v["commit_sha"].as_str().unwrap().to_string();
    let resp = f
        .client
        .post(f.url("/pulls"))
        .bearer_auth(&f.jwt)
        .json(&json!({"title": "feature", "head": "agent/feature", "base": "main"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    let merge = |body: Value, header: Option<(&'static str, &'static str)>| {
        let f = &f;
        async move {
            let mut req = f
                .client
                .post(f.url("/pulls/1/merge"))
                .bearer_auth(&f.jwt)
                .json(&body);
            if let Some((name, value)) = header {
                req = req.header(name, value);
            }
            let resp = req.send().await.unwrap();
            let status = resp.status();
            (status, resp.json::<Value>().await.unwrap_or(Value::Null))
        }
    };
    let main_before = f.tip("main").unwrap();

    // Dry run (body flag and header): reports the merge, merges nothing.
    for (body, header) in [
        (json!({"strategy": "merge", "dry_run": true}), None),
        (json!({"strategy": "merge"}), Some(("x-dry-run", "1"))),
    ] {
        let (status, v) = merge(body, header).await;
        assert_eq!(status, StatusCode::OK, "{v}");
        assert_eq!(v["dry_run"], json!(true));
        assert_eq!(v["would"][0]["effect"], "merge");
        assert_eq!(v["would"][0]["head_sha"], json!(reviewed));
        assert!(v["not_evaluated"]
            .as_array()
            .unwrap()
            .contains(&json!("conflicts")));
    }
    assert_eq!(f.tip("main"), Some(main_before.clone()));

    // The head moves after the review: pinned merges are refused, nothing merges.
    let (status, v) = f
        .post(
            "more.txt",
            json!({"branch": "agent/feature", "message": "more", "content": "m\n"}),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let moved = v["commit_sha"].as_str().unwrap().to_string();
    for dry in [false, true] {
        let (status, v) = merge(
            json!({"strategy": "merge", "expected_head_sha": reviewed, "dry_run": dry}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{v}");
        assert_eq!(v["error"]["reason"], "REF_ADVANCED");
        assert_eq!(v["error"]["details"]["current_sha"], json!(moved));
        assert_eq!(v["error"]["details"]["expected_sha"], json!(reviewed));
    }
    assert_eq!(f.tip("main"), Some(main_before.clone()));

    // Pinned to the new head it merges; without a pin the old behaviour holds.
    let (status, v) = merge(
        json!({"strategy": "merge", "expected_head_sha": moved}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_ne!(f.tip("main"), Some(main_before));
    // A merged PR is a state conflict, with or without a pin.
    let (status, _) = merge(json!({"strategy": "merge"}), None).await;
    assert_eq!(status, StatusCode::CONFLICT);
}

/// A branch that does not exist yet starts from the default branch, so the file
/// checks must look there: an existing file can be updated on a new branch, and
/// "create" of a file that already exists is refused, as on an existing branch.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_new_branch_is_checked_against_the_default_branch() {
    let f = Fixture::new("newbranch_user").await;
    let main_tip = f.tip("main").expect("main exists");

    let blob = f
        .client
        .get(f.url("/blob/README.md?ref=main"))
        .bearer_auth(&f.jwt)
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap()["sha"]
        .as_str()
        .unwrap()
        .to_string();

    // "Create" of a file that main already has: refused, nothing created.
    let (status, v) = f
        .post(
            "README.md",
            json!({"branch": "agent/new", "message": "m", "content": "x\n"}),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{v}");
    assert_eq!(f.tip("agent/new"), None);

    // The wrong blob sha is a conflict, an unknown file is not found.
    let (status, _) = f
        .post(
            "README.md",
            json!({"branch": "agent/new", "message": "m", "content": "x\n", "sha": "0".repeat(40)}),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = f
        .post(
            "missing.md",
            json!({"branch": "agent/new", "message": "m", "content": "x\n", "sha": blob}),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Updating it with its real blob sha works, in a dry run and for real, and the
    // branch starts from main.
    let (status, v) = f
        .post(
            "README.md",
            json!({"branch": "agent/new", "message": "edit", "content": "edited\n",
                   "sha": blob, "dry_run": true}),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["would"][0]["creates_branch"], json!(true));
    assert_eq!(v["would"][1]["blob_before"], json!(blob));
    assert_eq!(f.tip("agent/new"), None, "a dry run creates no branch");
    let (status, v) = f
        .post(
            "README.md",
            json!({"branch": "agent/new", "message": "edit", "content": "edited\n", "sha": blob}),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let new_tip = f.tip("agent/new").expect("branch created");
    assert_ne!(new_tip, main_tip);
    assert_eq!(f.tip("main"), Some(main_tip), "main is untouched");
}
