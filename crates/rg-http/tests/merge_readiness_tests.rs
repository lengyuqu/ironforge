//! `GET /repos/{o}/{n}/pulls/{number}/merge-readiness`: what stands between a
//! pull request and a merge, without merging.
//!
//! The endpoint reads the same gates the merge enforces, so the tests that
//! matter most are parity tests: whatever the preview calls a blocker, the
//! real merge refuses with the same sentence, and whatever it calls ready, the
//! merge accepts. Runs against PostgreSQL / MySQL too when
//! `IRONFORGE_TEST_DATABASE_URL` is set.

mod common;
use common::{register_user, spawn_test_app_on_env_db_with_root};

use reqwest::{Client, StatusCode};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde_json::{json, Value};

const PW: &str = "Qz7$wRtm";

struct World {
    base: String,
    db: rg_db::DatabaseConnection,
    root: std::path::PathBuf,
    client: Client,
    owner: String,
    jwt: String,
    repo_id: i64,
}

impl World {
    async fn new(owner: &str) -> World {
        Self::with_visibility(owner, false).await
    }

    async fn with_visibility(owner: &str, private: bool) -> World {
        let (base, db, root) = spawn_test_app_on_env_db_with_root().await;
        let client = Client::new();
        let jwt = register_user(&base, owner, &format!("{owner}@example.com"), PW).await;
        let resp = client
            .post(format!("{base}/api/v1/repos"))
            .bearer_auth(&jwt)
            .json(&json!({"name": "work", "auto_init": true, "is_private": private}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);
        let repo_id = resp.json::<Value>().await.unwrap()["id"].as_i64().unwrap();
        World {
            base,
            db,
            root,
            client,
            owner: owner.to_string(),
            jwt,
            repo_id,
        }
    }

    fn url(&self, suffix: &str) -> String {
        format!("{}/api/v1/repos/{}/work{suffix}", self.base, self.owner)
    }

    fn git_dir(&self) -> std::path::PathBuf {
        self.root.join(format!("{}/work.git", self.owner))
    }

    fn rev(&self, spec: &str) -> Option<String> {
        rg_git::ops::try_rev_parse(&self.git_dir(), spec)
            .ok()
            .flatten()
    }

    /// Write a file on `branch` (created from the default branch when new).
    async fn write(&self, branch: &str, path: &str, content: &str, sha: Option<&str>) -> String {
        let mut body =
            json!({"branch": branch, "message": format!("write {path}"), "content": content});
        if let Some(sha) = sha {
            body["sha"] = json!(sha);
        }
        let resp = self
            .client
            .post(self.url(&format!("/contents/{path}")))
            .bearer_auth(&self.jwt)
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = resp.status();
        let v: Value = resp.json().await.unwrap_or(Value::Null);
        assert_eq!(status, StatusCode::OK, "{v}");
        v["commit_sha"].as_str().unwrap().to_string()
    }

    /// The blob id of `path` on `branch`, read from the repository itself.
    fn blob_sha(&self, branch: &str, path: &str) -> String {
        rg_git::ops::rev_parse(&self.git_dir(), &format!("refs/heads/{branch}:{path}")).unwrap()
    }

    async fn open_pr(&self, head: &str) {
        let resp = self
            .client
            .post(self.url("/pulls"))
            .bearer_auth(&self.jwt)
            .json(&json!({"title": "Change", "head": head, "base": "main"}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    async fn readiness(&self, bearer: Option<&str>, query: &str) -> (StatusCode, Value) {
        let mut req = self
            .client
            .get(self.url(&format!("/pulls/1/merge-readiness{query}")));
        if let Some(token) = bearer {
            req = req.bearer_auth(token);
        }
        let resp = req.send().await.unwrap();
        let status = resp.status();
        (status, resp.json().await.unwrap_or(Value::Null))
    }

    async fn ready(&self) -> Value {
        let (status, v) = self.readiness(Some(&self.jwt), "").await;
        assert_eq!(status, StatusCode::OK, "{v}");
        v
    }

    async fn merge(&self) -> (StatusCode, Value) {
        let resp = self
            .client
            .post(self.url("/pulls/1/merge"))
            .bearer_auth(&self.jwt)
            .json(&json!({"strategy": "merge"}))
            .send()
            .await
            .unwrap();
        let status = resp.status();
        (status, resp.json().await.unwrap_or(Value::Null))
    }

    async fn protect(&self, body: Value) {
        let resp = self
            .client
            .post(self.url("/branches/protection"))
            .bearer_auth(&self.jwt)
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201, "{}", resp.text().await.unwrap());
    }

    /// A pipeline for `sha` with the given `(job name, job status)` pairs.
    async fn pipeline(&self, sha: &str, pipeline_status: &str, jobs: &[(&str, &str)]) -> i64 {
        use rg_db::ops::pipeline_ops as ops;
        // "Latest pipeline for the commit" orders by `created_at`; MySQL keeps whole
        // seconds, so two pipelines made in one second would tie (CLAUDE.md, pitfall 23).
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        let pipeline = ops::create_pipeline(
            &self.db,
            self.repo_id,
            sha,
            "refs/heads/agent/x",
            "push",
            None,
        )
        .await
        .unwrap();
        let stage = ops::create_stage(&self.db, pipeline.id, "ci", 0)
            .await
            .unwrap();
        for (name, status) in jobs {
            let job = ops::create_job(
                &self.db, stage.id, name, "true", None, None, None, None, None, false, None, None,
                None,
            )
            .await
            .unwrap();
            ops::update_job_result(&self.db, job.id, status, Some(0), Some("log"), None, None)
                .await
                .unwrap();
        }
        ops::update_pipeline_status(&self.db, pipeline.id, pipeline_status, None, None)
            .await
            .unwrap();
        pipeline.id
    }
}

fn codes(v: &Value, key: &str) -> Vec<String> {
    v[key]
        .as_array()
        .unwrap_or_else(|| panic!("{key} missing in {v}"))
        .iter()
        .map(|c| c.as_str().unwrap().to_string())
        .collect()
}

fn gate<'a>(v: &'a Value, code: &str) -> &'a Value {
    v["gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["code"] == code)
        .unwrap_or_else(|| panic!("no gate {code} in {}", v["gates"]))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_clean_unprotected_pull_request_is_ready_and_the_merge_agrees() {
    let w = World::new("mr_clean").await;
    let head = w.write("agent/x", "one.txt", "1\n", None).await;
    w.open_pr("agent/x").await;

    let v = w.ready().await;
    assert_eq!(v["consistency"], "live");
    assert_eq!(v["merge"]["state"], "clean");
    assert_eq!(v["merge"]["head_contains_base"], true);
    assert_eq!(v["head"]["sha"], json!(head));
    assert_eq!(v["head"]["state"], "present");
    assert_eq!(v["branch_protection"]["protected"], false);
    assert_eq!(v["ready"], true);
    assert_eq!(v["ready_reason"], json!([]));
    assert!(v["as_of"].as_str().unwrap().ends_with('Z'));
    let merge_action = v["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["path"].as_str().unwrap().ends_with("/merge"))
        .expect("a ready pull request offers the merge");
    assert_eq!(
        merge_action["requires"],
        json!(format!("expected_head_sha={head}"))
    );
    assert!(v["not_evaluated"]
        .as_array()
        .unwrap()
        .contains(&json!("merge_queue_position")));

    let (status, body) = w.merge().await;
    assert_eq!(
        status,
        StatusCode::OK,
        "ready:true means the merge goes through: {body}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_base_that_moved_on_cleanly_offers_update_branch() {
    let w = World::new("mr_behind").await;
    w.write("agent/x", "one.txt", "1\n", None).await;
    w.open_pr("agent/x").await;
    w.write("main", "other.txt", "o\n", None).await;

    let v = w.ready().await;
    assert_eq!(v["merge"]["state"], "clean");
    assert_eq!(v["merge"]["head_contains_base"], false);
    assert_eq!(v["ready"], true);
    let paths: Vec<&str> = v["actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["path"].as_str().unwrap())
        .collect();
    assert!(
        paths.iter().any(|p| p.ends_with("/pulls/1/update-branch")),
        "{paths:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_content_conflict_names_the_file_and_the_block() {
    let w = World::new("mr_content").await;
    w.write("main", "f.txt", "top\nmiddle\nbottom\n", None)
        .await;
    let sha = w.blob_sha("main", "f.txt");
    w.write(
        "agent/x",
        "f.txt",
        "top\nhead one\nhead two\nbottom\n",
        Some(&sha),
    )
    .await;
    w.open_pr("agent/x").await;
    w.write("main", "f.txt", "top\nbase side\nbottom\n", Some(&sha))
        .await;

    let v = w.ready().await;
    assert_eq!(v["merge"]["state"], "conflicts");
    assert_eq!(v["merge"]["reason"], "MERGE_CONFLICTS");
    assert_eq!(v["merge"]["conflict_files"], 1);
    assert_eq!(v["merge"]["conflicts_truncated"], false);
    assert_eq!(v["merge"]["excerpts_origin"], "user_content");
    let file = &v["merge"]["conflicts"][0];
    assert_eq!(file["path"], "f.txt");
    assert_eq!(file["kind"], "content");
    assert_eq!(file["content_available"], true);
    assert_eq!(file["blocks_total"], 1);
    let block = &file["blocks"][0];
    assert_eq!(block["merged_start_line"], 2);
    assert_eq!(block["ours"]["lines"], 1);
    assert_eq!(block["ours"]["excerpt"], "base side\n");
    assert_eq!(block["theirs"]["lines"], 2);
    assert_eq!(block["theirs"]["excerpt"], "head one\nhead two\n");
    assert_eq!(v["ready"], false);
    assert!(codes(&v, "ready_reason").contains(&"MERGE_CONFLICTS".to_string()));

    // The real merge refuses too, and the readiness read changed nothing.
    let main_before = w.rev("refs/heads/main");
    let (status, _) = w.merge().await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(w.rev("refs/heads/main"), main_before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_deleted_on_one_side_and_changed_on_the_other_is_delete_modify() {
    let w = World::new("mr_delmod").await;
    w.write("main", "gone.txt", "keep\n", None).await;
    let sha = w.blob_sha("main", "gone.txt");
    w.write("agent/x", "gone.txt", "changed\n", Some(&sha))
        .await;
    w.open_pr("agent/x").await;
    let resp = w
        .client
        .delete(w.url(&format!(
            "/contents/gone.txt?branch=main&message=drop&sha={sha}"
        )))
        .bearer_auth(&w.jwt)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "{}", resp.status());

    let v = w.ready().await;
    assert_eq!(v["merge"]["state"], "conflicts");
    let file = &v["merge"]["conflicts"][0];
    assert_eq!(file["path"], "gone.txt");
    assert_eq!(file["kind"], "delete_modify");
    assert_eq!(file["content_available"], false);
    assert_eq!(file["blocks"], json!([]));
    assert!(
        file["content_hint"].is_string(),
        "a conflict with no blocks says why"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_large_conflict_is_cut_and_continue_with_walks_the_rest() {
    let w = World::new("mr_many").await;
    w.write("agent/x", "seed.txt", "s\n", None).await;
    w.open_pr("agent/x").await;
    let total = 55;
    // One batch commit per side: the same new paths with different content.
    for (branch, text) in [("agent/x", "head"), ("main", "main")] {
        let tip = w.rev(&format!("refs/heads/{branch}")).unwrap();
        let files: Vec<Value> = (0..total)
            .map(|i| json!({"operation": "create", "path": format!("c/f{i:03}.txt"), "content": format!("{text}\n")}))
            .collect();
        let resp = w
            .client
            .post(w.url("/commits"))
            .bearer_auth(&w.jwt)
            .json(&json!({"branch": branch, "expected_sha": tip, "message": text, "files": files}))
            .send()
            .await
            .unwrap();
        let status = resp.status();
        assert_eq!(status, StatusCode::OK, "{}", resp.text().await.unwrap());
    }

    let v = w.ready().await;
    assert_eq!(v["merge"]["conflict_files"], total);
    assert_eq!(v["merge"]["conflicts"].as_array().unwrap().len(), 50);
    assert_eq!(v["merge"]["conflicts_truncated"], true);
    let next = v["merge"]["continue_with"].as_str().unwrap().to_string();
    assert!(next.contains("conflicts_after=c/f049.txt"), "{next}");

    let resp = w
        .client
        .get(format!("{}{}", w.base, next))
        .bearer_auth(&w.jwt)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let page: Value = resp.json().await.unwrap();
    assert_eq!(page["merge"]["conflicts"].as_array().unwrap().len(), 5);
    assert_eq!(page["merge"]["conflicts_truncated"], false);
    assert_eq!(page["merge"]["conflicts"][0]["path"], "c/f050.txt");
    assert!(page["merge"]["continue_with"].is_null());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_protection_gate_is_reported_and_the_merge_refuses_with_the_same_sentence() {
    let w = World::new("mr_gates").await;
    let head = w.write("agent/x", "one.txt", "1\n", None).await;
    w.open_pr("agent/x").await;
    w.protect(json!({
        "branch_name": "main",
        "require_approval": true,
        "required_approvals": 1,
        "require_status_check": true,
        "required_status_checks": ["lint", "test"],
    }))
    .await;

    // Approval missing, no pipeline at all.
    let v = w.ready().await;
    assert_eq!(v["branch_protection"]["protected"], true);
    assert_eq!(v["branch_protection"]["rule"]["pattern"], "main");
    assert_eq!(v["branch_protection"]["blocked"], true);
    assert_eq!(gate(&v, "REQUIRED_APPROVALS_MISSING")["result"], "FAIL");
    assert_eq!(
        gate(&v, "REQUIRED_APPROVALS_MISSING")["observed"]["current"],
        0
    );
    assert_eq!(
        gate(&v, "REQUIRED_APPROVALS_MISSING")["observed"]["required"],
        1
    );
    assert_eq!(gate(&v, "PIPELINE_NOT_FOUND")["result"], "FAIL");
    assert_eq!(
        gate(&v, "SIGNED_COMMITS_REQUIRED")["result"],
        "NOT_APPLICABLE"
    );
    assert_eq!(v["ready"], false);
    let reasons = codes(&v, "ready_reason");
    assert!(
        reasons.contains(&"REQUIRED_APPROVALS_MISSING".to_string()),
        "{reasons:?}"
    );
    assert!(
        reasons.contains(&"PIPELINE_NOT_FOUND".to_string()),
        "{reasons:?}"
    );
    // Both required checks are missing; the list says so.
    let missing: Vec<&str> = v["required_checks"]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["result"].as_str().unwrap())
        .collect();
    assert_eq!(missing, vec!["MISSING", "MISSING"]);
    // The merge refuses with the first failing gate's sentence.
    let (status, body) = w.merge().await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        body["error"]["message"],
        gate(&v, "REQUIRED_APPROVALS_MISSING")["detail"],
        "{body}"
    );

    // Approved by a person (the author cannot approve their own pull request).
    // Approving a review is an authorization decision since #19/T06: reading the
    // repository is not enough, so the reviewer is given write access first.
    let reviewer = register_user(
        &w.base,
        "mr_gates_reviewer",
        "mr-gates-reviewer@example.com",
        PW,
    )
    .await;
    let granted = w
        .client
        .post(w.url("/collaborators"))
        .bearer_auth(&w.jwt)
        .json(&json!({"username": "mr_gates_reviewer", "permission": "write"}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        granted.status(),
        StatusCode::CREATED,
        "the reviewer needs write access to approve"
    );
    let resp = w
        .client
        .post(w.url("/pulls/1/reviews"))
        .bearer_auth(&reviewer)
        .json(&json!({"action": "approve", "body": "ok"}))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "{}", resp.status());
    let v = w.ready().await;
    assert_eq!(gate(&v, "REQUIRED_APPROVALS_MISSING")["result"], "PASS");
    assert_eq!(
        gate(&v, "REQUIRED_APPROVALS_MISSING")["observed"]["current"],
        1
    );
    let (_, body) = w.merge().await;
    assert_eq!(
        body["error"]["message"],
        gate(&v, "PIPELINE_NOT_FOUND")["detail"]
    );

    // A red pipeline.
    w.pipeline(&head, "failed", &[("lint", "success"), ("test", "failed")])
        .await;
    let v = w.ready().await;
    assert_eq!(gate(&v, "PIPELINE_NOT_SUCCESS")["result"], "FAIL");
    assert_eq!(
        gate(&v, "REQUIRED_CHECK_FAILED")["observed"]["check"],
        "test"
    );
    let by_name = |name: &str| {
        v["required_checks"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == name)
            .unwrap()
            .clone()
    };
    assert_eq!(by_name("lint")["result"], "PASSED");
    assert_eq!(by_name("test")["result"], "FAILED");
    let (_, body) = w.merge().await;
    assert_eq!(
        body["error"]["message"],
        gate(&v, "PIPELINE_NOT_SUCCESS")["detail"]
    );

    // A green pipeline that never ran `test`.
    w.pipeline(&head, "success", &[("lint", "success")]).await;
    let v = w.ready().await;
    assert_eq!(gate(&v, "PIPELINE_NOT_SUCCESS")["result"], "PASS");
    assert_eq!(
        gate(&v, "REQUIRED_CHECK_MISSING")["observed"]["check"],
        "test"
    );
    assert_eq!(by_name_from(&v, "test")["result"], "MISSING");
    let (status, body) = w.merge().await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("missing or failed"),
        "{body}"
    );

    // Everything required passed: ready, and the merge goes through.
    w.pipeline(
        &head,
        "success",
        &[("lint", "success"), ("test", "success")],
    )
    .await;
    let v = w.ready().await;
    assert_eq!(v["branch_protection"]["blocked"], false);
    assert_eq!(gate(&v, "REQUIRED_CHECKS_SATISFIED")["result"], "PASS");
    assert_eq!(v["ready"], true, "{v}");
    let (status, body) = w.merge().await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

fn by_name_from(v: &Value, name: &str) -> Value {
    v["required_checks"]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap()
        .clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn required_signed_commits_can_never_be_met_by_a_server_side_merge() {
    let w = World::new("mr_signed").await;
    w.write("agent/x", "one.txt", "1\n", None).await;
    w.open_pr("agent/x").await;
    w.protect(json!({"branch_name": "main", "require_signed_commits": true}))
        .await;

    let v = w.ready().await;
    assert_eq!(gate(&v, "SIGNED_COMMITS_REQUIRED")["result"], "FAIL");
    assert_eq!(v["ready"], false);
    let (status, body) = w.merge().await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        body["error"]["message"],
        gate(&v, "SIGNED_COMMITS_REQUIRED")["detail"]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rule_that_cannot_be_read_is_unknown_not_a_pass_while_the_merge_keeps_its_old_behaviour()
{
    let w = World::new("mr_malformed").await;
    w.write("agent/x", "one.txt", "1\n", None).await;
    w.open_pr("agent/x").await;
    w.protect(json!({
        "branch_name": "main",
        "require_status_check": true,
        "required_status_checks": ["lint"],
    }))
    .await;
    // Corrupt the stored list the way a bad import or a hand edit would.
    let rule = rg_db::entities::protected_branch::Entity::find()
        .filter(rg_db::entities::protected_branch::Column::RepoId.eq(w.repo_id))
        .one(&w.db)
        .await
        .unwrap()
        .unwrap();
    let mut active: rg_db::entities::protected_branch::ActiveModel = rule.into();
    active.required_status_checks = Set(Some("not json".to_string()));
    active.update(&w.db).await.unwrap();

    let v = w.ready().await;
    let g = gate(&v, "PROTECTION_RULE_MALFORMED");
    assert_eq!(g["result"], "UNKNOWN");
    // Neither blocked nor ready: it could not be judged.
    assert_eq!(v["branch_protection"]["blocked"], false);
    assert_eq!(v["ready"], Value::Null);
    assert_eq!(v["ready_reason"], json!([]));

    // The merge path is unchanged by this work: it still lets this through.
    let (status, body) = w.merge().await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_head_that_moved_past_the_stored_commit_is_refused_not_merged() {
    let w = World::new("mr_stale").await;
    w.write("agent/x", "one.txt", "1\n", None).await;
    w.open_pr("agent/x").await;
    // Move the head branch behind the pull request's back.
    let git_dir = w.git_dir();
    let main = w.rev("refs/heads/main").unwrap();
    rg_git::ops::update_ref(&git_dir, "refs/heads/agent/x", &main, "test").unwrap();

    let v = w.ready().await;
    assert_eq!(v["head"]["sha"], json!(main));
    assert_ne!(v["head"]["stored_sha"], v["head"]["sha"]);
    // The drift is a failing gate under the same code the preview always used,
    // not a fact filed under `unavailable`: approvals and CI were judged for a
    // commit the branch no longer has.
    let g = gate(&v, "HEAD_SHA_STALE");
    assert_eq!(g["result"], "FAIL", "{g}");
    assert_eq!(v["ready"], false);
    assert!(codes(&v, "ready_reason").contains(&"HEAD_SHA_STALE".to_string()));
    assert!(
        !v["unavailable"]
            .as_array()
            .unwrap()
            .iter()
            .any(|u| u["code"] == "HEAD_SHA_STALE"),
        "the same fact must not be reported twice: {}",
        v["unavailable"]
    );

    // The merge refuses with the gate's own sentence.
    let (status, body) = w.merge().await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"]["message"], g["detail"]);
    let after = w.ready().await;
    assert_eq!(after["state"], "open", "the pull request is still open");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_push_moves_the_recorded_head_and_the_old_approval_stops_counting() {
    let w = World::new("mr_sync").await;
    w.write("agent/x", "one.txt", "1\n", None).await;
    w.open_pr("agent/x").await;
    w.protect(json!({
        "branch_name": "main",
        "require_approval": true,
        "required_approvals": 1,
    }))
    .await;

    // A second person with write access approves the commit the PR records now.
    let reviewer = register_user(&w.base, "mr_sync_reviewer", "mr_sync_r@example.com", PW).await;
    let added = w
        .client
        .post(w.url("/collaborators"))
        .bearer_auth(&w.jwt)
        .json(&json!({"username": "mr_sync_reviewer", "permission": "write"}))
        .send()
        .await
        .unwrap();
    assert_eq!(added.status(), StatusCode::CREATED);
    let approved = w
        .client
        .post(w.url("/pulls/1/reviews"))
        .bearer_auth(&reviewer)
        .json(&json!({"action": "approve", "body": "first revision"}))
        .send()
        .await
        .unwrap();
    assert_eq!(approved.status(), StatusCode::CREATED);

    let before = w.ready().await;
    assert_eq!(
        gate(&before, "REQUIRED_APPROVALS_MISSING")["result"],
        "PASS",
        "{before}"
    );

    // Push a new commit to the head branch. The push hook is what moves the
    // pull request's recorded head — it runs after the response, so poll.
    let new_head = w.write("agent/x", "two.txt", "2\n", None).await;
    let mut recorded = None;
    for _ in 0..100 {
        recorded = rg_db::entities::pull_request::Entity::find()
            .filter(rg_db::entities::pull_request::Column::RepoId.eq(w.repo_id))
            .one(&w.db)
            .await
            .unwrap()
            .and_then(|pr| pr.head_sha);
        if recorded.as_deref() == Some(new_head.as_str()) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(
        recorded.as_deref(),
        Some(new_head.as_str()),
        "the push must move the recorded head, not leave it at the creation commit"
    );

    // The approval is still a row (the vote was cast, and the trail keeps it),
    // but it was for the old commit, so it no longer counts.
    let listed = w
        .client
        .get(w.url("/pulls/1/reviews"))
        .bearer_auth(&w.jwt)
        .send()
        .await
        .unwrap();
    let reviews: Value = listed.json().await.unwrap();
    let approvals = reviews
        .as_array()
        .map(|rows| rows.iter().filter(|row| row["action"] == "approve").count())
        .unwrap_or(0);
    assert_eq!(approvals, 1, "the row is kept: {reviews}");

    let after = w.ready().await;
    assert_eq!(
        gate(&after, "HEAD_SHA_STALE")["result"],
        "PASS",
        "the recorded head was synced: {after}"
    );
    assert_eq!(
        gate(&after, "REQUIRED_APPROVALS_MISSING")["result"],
        "FAIL",
        "the old approval must not count for the new commit: {after}"
    );
    let (status, body) = w.merge().await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(
        body["error"]["message"],
        gate(&after, "REQUIRED_APPROVALS_MISSING")["detail"]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_missing_branch_is_a_verdict_not_a_server_error() {
    let w = World::new("mr_gone").await;
    w.write("agent/x", "one.txt", "1\n", None).await;
    w.open_pr("agent/x").await;
    rg_git::ops::delete_ref(&w.git_dir(), "refs/heads/agent/x").unwrap();

    let v = w.ready().await;
    assert_eq!(v["head"]["state"], "deleted");
    assert_eq!(v["head"]["sha"], Value::Null);
    assert_eq!(v["merge"]["state"], "unavailable");
    assert_eq!(v["merge"]["reason"], "HEAD_BRANCH_DELETED");
    assert_eq!(v["ready"], false);
    assert!(codes(&v, "ready_reason").contains(&"HEAD_BRANCH_DELETED".to_string()));
    assert_eq!(gate(&v, "HEAD_BRANCH_DELETED")["result"], "FAIL");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_merged_pull_request_is_not_ready_and_does_not_run_a_merge() {
    let w = World::new("mr_merged").await;
    w.write("agent/x", "one.txt", "1\n", None).await;
    w.open_pr("agent/x").await;
    let (status, body) = w.merge().await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let v = w.ready().await;
    assert_eq!(v["state"], "merged");
    assert_eq!(v["ready"], false);
    assert!(codes(&v, "ready_reason").contains(&"PR_STATE_NOT_OPEN".to_string()));
    assert_eq!(v["merge"]["reason"], "PR_STATE_NOT_OPEN");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rebase_says_it_did_not_replay_and_an_unknown_strategy_is_refused() {
    let w = World::new("mr_strategy").await;
    w.write("agent/x", "one.txt", "1\n", None).await;
    w.open_pr("agent/x").await;

    let (status, v) = w.readiness(Some(&w.jwt), "?strategy=rebase").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(v["strategy"], "rebase");
    assert_eq!(v["merge"]["approximation"], "REBASE_REPLAY_NOT_SIMULATED");
    assert!(v["not_evaluated"]
        .as_array()
        .unwrap()
        .contains(&json!("rebase_replay")));

    let (status, v) = w.readiness(Some(&w.jwt), "?strategy=fast-forward").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_public_repository_reads_without_a_login_and_an_unknown_pull_request_is_404() {
    // Anyone, anonymous included, reads what they could already read (the diff,
    // the reviews, the pipelines).
    let w = World::new("mr_public").await;
    w.write("agent/x", "one.txt", "1\n", None).await;
    w.open_pr("agent/x").await;
    let (status, _) = w.readiness(None, "").await;
    assert_eq!(status, StatusCode::OK);

    let resp = w
        .client
        .get(w.url("/pulls/99/merge-readiness"))
        .bearer_auth(&w.jwt)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_private_repository_keeps_its_readiness_to_those_who_can_read_it() {
    let w = World::with_visibility("mr_private", true).await;
    w.write("agent/x", "one.txt", "1\n", None).await;
    w.open_pr("agent/x").await;

    let stranger = register_user(&w.base, "mr_stranger", "mr-stranger@example.com", PW).await;
    let (status, _) = w.readiness(Some(&stranger), "").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = w.readiness(None, "").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = w.readiness(Some(&w.jwt), "").await;
    assert_eq!(status, StatusCode::OK);
}
