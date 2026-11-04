//! P2 X02 acceptance (M01–M05): cross-repository coordination groups.
//!
//! Real HTTP server, SQLite database and bare Git repositories, several
//! repositories and accounts with different permissions. With
//! `IRONFORGE_TEST_DATABASE_URL` the same tests run on PostgreSQL or MySQL.

mod common;
mod collab_common;

use collab_common::{Env, Sess};
use reqwest::Method;
use rg_db::sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde_json::{json, Value};

/// An instance, its owner and helpers for building several repositories.
struct W {
    env: Env,
}

impl W {
    async fn new() -> Self {
        Self { env: Env::new().await }
    }

    /// Repository owned by the fixture owner; returns its id.
    async fn repo(&self, name: &str, private: bool) -> i64 {
        let resp = reqwest::Client::new()
            .post(format!("{}/api/v1/repos", self.env.base))
            .bearer_auth(&self.env.owner_token)
            .json(&json!({"name": name, "is_private": private}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201, "create repo {name}");
        resp.json::<Value>().await.unwrap()["id"].as_i64().unwrap()
    }

    async fn call(&self, method: Method, url: &str, token: &str, sess: Option<&Sess>, body: Option<Value>) -> (u16, Value) {
        self.env.json(method, url, token, sess, body).await
    }

    async fn session(&self, repo: &str, token: &str) -> Sess {
        let (status, body) = self
            .call(
                Method::POST,
                &self.env.url_in(repo, "/sessions"),
                token,
                None,
                Some(json!({"display_name": "agent", "client_kind": "test-client"})),
            )
            .await;
        assert_eq!(status, 201, "{body}");
        Sess {
            id: body["session"]["id"].as_str().unwrap().to_string(),
            secret: body["session_secret"].as_str().unwrap().to_string(),
        }
    }

    /// A collaboration task of `repo`, created by the owner.
    async fn task(&self, repo: &str, title: &str) -> String {
        let s = self.session(repo, &self.env.owner_token).await;
        let (status, task) = self
            .call(
                Method::POST,
                &self.env.url_in(repo, "/tasks"),
                &self.env.owner_token,
                Some(&s),
                Some(json!({"title": title})),
            )
            .await;
        assert_eq!(status, 201, "{task}");
        task["id"].as_str().unwrap().to_string()
    }

    async fn grant(&self, repo: &str, user_id: i64, permission: &str) {
        let resp = reqwest::Client::new()
            .post(format!("{}/api/v1/repos/{}/{}/collaborators", self.env.base, self.env.owner, repo))
            .bearer_auth(&self.env.owner_token)
            .json(&json!({"user_id": user_id, "permission": permission}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201, "grant {permission} on {repo}");
    }

    async fn revoke(&self, repo: &str, user_id: i64) {
        let resp = reqwest::Client::new()
            .delete(format!("{}/api/v1/repos/{}/{}/collaborators/{user_id}", self.env.base, self.env.owner, repo))
            .bearer_auth(&self.env.owner_token)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 204, "revoke on {repo}");
    }

    fn gurl(&self, repo: &str, suffix: &str) -> String {
        self.env.url_in(repo, &format!("/groups{suffix}"))
    }

    /// New group in `repo` as the owner: `(group id, version)`.
    async fn group(&self, repo: &str, title: &str) -> (String, i64) {
        let (status, body) = self
            .call(
                Method::POST,
                &self.gurl(repo, ""),
                &self.env.owner_token,
                None,
                Some(json!({"client_request_id": uuid::Uuid::new_v4().to_string(), "title": title})),
            )
            .await;
        assert_eq!(status, 201, "{body}");
        (body["id"].as_str().unwrap().to_string(), body["version"].as_i64().unwrap())
    }

    async fn add(&self, repo: &str, gid: &str, token: &str, body: Value) -> (u16, Value) {
        self.call(Method::POST, &self.gurl(repo, &format!("/{gid}/links")), token, None, Some(body)).await
    }

    /// Link a task of `target` as the owner and return `(link id, new version)`.
    async fn link_task(&self, repo: &str, gid: &str, version: i64, target_id: i64, task: &str) -> (String, i64) {
        let (status, link) = self
            .add(repo, gid, &self.env.owner_token, task_link(&key(), target_id, task, version))
            .await;
        assert_eq!(status, 201, "{link}");
        (link["id"].as_str().unwrap().to_string(), version + 1)
    }

    async fn links(&self, repo: &str, gid: &str, token: &str, query: &str) -> (u16, Value) {
        self.call(Method::GET, &self.gurl(repo, &format!("/{gid}/links{query}")), token, None, None).await
    }

    async fn version_of(&self, gid: &str) -> i64 {
        use rg_db::entities::collab_coordination_group as g;
        g::Entity::find_by_id(gid.to_string()).one(&self.env.db).await.unwrap().unwrap().version
    }

    async fn active_links(&self, gid: &str) -> u64 {
        rg_db::ops::collab_group_ops::count_active(&self.env.db, gid).await.unwrap()
    }
}

fn key() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn task_link(k: &str, target_repo_id: i64, task: &str, version: i64) -> Value {
    json!({"client_request_id": k, "target_repo_id": target_repo_id, "target_task_id": task, "expected_group_version": version})
}

fn commit_link(k: &str, target_repo_id: i64, sha: &str, version: i64) -> Value {
    json!({"client_request_id": k, "target_repo_id": target_repo_id, "commit_sha": sha, "expected_group_version": version})
}

fn names(page: &Value) -> Vec<String> {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["target_repo"]["name"].as_str().unwrap().to_string())
        .collect()
}

// ------------------------------------------------------------------ M01

#[tokio::test]
async fn m01_reader_of_the_anchor_sees_nothing_of_hidden_targets() {
    let w = W::new().await;
    w.repo("anchor-a", true).await;
    let b_id = w.repo("secret-b", true).await;
    let c_id = w.repo("open-c", true).await;
    let (reader, reader_id) = w.env.user("reader").await;
    let (writer, writer_id) = w.env.user("writer").await;
    w.grant("anchor-a", reader_id, "read").await;
    w.grant("anchor-a", writer_id, "write").await;
    w.grant("open-c", reader_id, "read").await;

    let b_task = w.task("secret-b", "TOPSECRET-TITLE").await;
    let c_task = w.task("open-c", "visible task").await;
    let b_sha = w.env.push_commit("secret-b", "secret commit");

    // The owner (write on both) builds the group.
    let (gid, v) = w.group("anchor-a", "release train").await;
    let (_, v) = w.link_task("anchor-a", &gid, v, b_id, &b_task).await;
    let (status, commit_link_body) = w
        .add("anchor-a", &gid, &w.env.owner_token, commit_link(&key(), b_id, &b_sha, v))
        .await;
    assert_eq!(status, 201, "{commit_link_body}");
    assert_eq!(commit_link_body["commit"]["commit_state"], "available");
    let (_, v) = w.link_task("anchor-a", &gid, v + 1, c_id, &c_task).await;

    // The reader sees the anchor group and only the link to the repo they can read.
    let (status, detail) = w.call(Method::GET, &w.gurl("anchor-a", &format!("/{gid}")), &reader, None, None).await;
    assert_eq!(status, 200, "{detail}");
    assert!(detail.get("version").is_none(), "readers get no version counter: {detail}");
    assert_eq!(names(&detail["links"]), ["open-c"], "{detail}");
    assert_eq!(detail["links"]["items"][0]["task"]["title"], "visible task");

    let (_, page) = w.links("anchor-a", &gid, &reader, "").await;
    let raw = page.to_string();
    for needle in ["secret-b", "TOPSECRET-TITLE", b_sha.as_str(), b_task.as_str(), "hidden", "total", "count"] {
        assert!(!raw.contains(needle), "leaked {needle:?}: {raw}");
    }
    assert!(!raw.contains(&format!("\"id\":{b_id}")), "the hidden repository id leaked: {raw}");
    assert!(!detail.to_string().contains("secret-b"));
    assert_eq!(page["has_more"], false);
    assert!(page["next_cursor"].is_null());

    // A reader's page looks the same whether or not hidden links exist.
    let (other, _) = w.group("anchor-a", "empty").await;
    let (_, empty_page) = w.links("anchor-a", &other, &reader, "").await;
    let shape = |v: &Value| {
        let mut keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        keys
    };
    assert_eq!(shape(&page), shape(&empty_page));

    // The group sharing notice is returned on creation.
    let (status, created) = w
        .call(Method::POST, &w.gurl("anchor-a", ""), &w.env.owner_token, None,
              Some(json!({"client_request_id": key(), "title": "t"})))
        .await;
    assert_eq!(status, 201);
    assert!(created["notice"].as_str().unwrap().contains("never copies"), "{created}");

    // The writer of the anchor who cannot read B: "no such repository" is the
    // answer for a hidden repo and for one that does not exist — identical.
    let (s1, hidden) = w.add("anchor-a", &gid, &writer, task_link(&key(), b_id, &b_task, v)).await;
    let (s2, missing) = w.add("anchor-a", &gid, &writer, task_link(&key(), 987_654, &b_task, v)).await;
    assert_eq!((s1, s2), (404, 404), "{hidden} {missing}");
    assert_eq!(hidden["error"]["message"], missing["error"]["message"]);
    assert_eq!(hidden["error"]["code"], missing["error"]["code"]);

    // Read-only on the target (visible, not writable): 403; read-only anchor: 403.
    w.grant("secret-b", writer_id, "read").await;
    let (status, _) = w.add("anchor-a", &gid, &writer, task_link(&key(), b_id, &b_task, v)).await;
    assert_eq!(status, 403);
    let (status, _) = w.add("anchor-a", &gid, &reader, task_link(&key(), c_id, &c_task, v)).await;
    assert_eq!(status, 403);

    // A stranger on a private anchor gets nothing at all.
    let (stranger, _) = w.env.user("stranger").await;
    let (status, _) = w.call(Method::GET, &w.gurl("anchor-a", &format!("/{gid}")), &stranger, None, None).await;
    assert_eq!(status, 403);

    // Removing a link whose target the caller cannot see looks like a missing link.
    let hidden_link_id = {
        use rg_db::entities::collab_coordination_link as l;
        l::Entity::find()
            .filter(l::Column::GroupId.eq(gid.as_str()))
            .filter(l::Column::TargetRepoId.eq(b_id))
            .filter(l::Column::TargetTaskId.is_not_null())
            .one(&w.env.db)
            .await
            .unwrap()
            .unwrap()
            .id
    };
    w.revoke("secret-b", writer_id).await;
    let ver = w.version_of(&gid).await;
    let rm = |id: &str| w.gurl("anchor-a", &format!("/{gid}/links/{id}"));
    let (s1, a) = w
        .call(Method::DELETE, &rm(&hidden_link_id), &writer, None, Some(json!({"client_request_id": key(), "expected_group_version": ver})))
        .await;
    let (s2, b) = w
        .call(Method::DELETE, &rm(&uuid::Uuid::new_v4().to_string()), &writer, None, Some(json!({"client_request_id": key(), "expected_group_version": ver})))
        .await;
    assert_eq!((s1, s2), (404, 404), "{a} {b}");
    assert_eq!(a["error"]["message"], b["error"]["message"]);
    assert_eq!(w.version_of(&gid).await, ver, "nothing happened");
}

// ------------------------------------------------------------------ M02

#[tokio::test]
async fn m02_identical_requests_have_one_effect_and_different_bodies_conflict() {
    let w = W::new().await;
    w.repo("anchor", false).await;
    let b_id = w.repo("target-b", false).await;
    let t1 = w.task("target-b", "one").await;
    let t2 = w.task("target-b", "two").await;
    let (gid, v) = w.group("anchor", "g").await;

    // Eight racing copies of one request.
    let k = key();
    let body = task_link(&k, b_id, &t1, v);
    let calls = (0..8).map(|_| w.add("anchor", &gid, &w.env.owner_token, body.clone()));
    let results = futures_join(calls).await;
    let created = results.iter().filter(|(s, _)| *s == 201).count();
    let replayed = results.iter().filter(|(s, _)| *s == 200).count();
    assert_eq!((created, replayed), (1, 7), "{results:?}");
    let ids: std::collections::HashSet<_> = results.iter().map(|(_, b)| b["id"].as_str().unwrap().to_string()).collect();
    assert_eq!(ids.len(), 1, "every copy answers with the same link");
    assert_eq!(w.version_of(&gid).await, v + 1, "one bump");
    assert_eq!(w.active_links(&gid).await, 1);

    // Same key, different body: 409, no effect.
    let (status, err) = w.add("anchor", &gid, &w.env.owner_token, task_link(&k, b_id, &t2, v)).await;
    assert_eq!(status, 409);
    assert_eq!(err["error"]["code"], "IDEMPOTENCY_KEY_REUSED");
    assert_eq!(w.version_of(&gid).await, v + 1);

    // The same target again under a new key is a duplicate and bumps nothing.
    let v = v + 1;
    let (status, dup) = w.add("anchor", &gid, &w.env.owner_token, task_link(&key(), b_id, &t1, v)).await;
    assert_eq!(status, 409);
    assert_eq!(dup["error"]["code"], "LINK_ALREADY_EXISTS");
    assert_eq!(dup["error"]["details"]["link_id"], results[0].1["id"]);
    assert_eq!(w.version_of(&gid).await, v, "failed add leaves the version alone");

    // Six different links racing on one expected version: exactly one wins.
    let mut tasks = vec![];
    for i in 0..6 {
        tasks.push(w.task("target-b", &format!("race {i}")).await);
    }
    let calls = tasks.iter().map(|t| w.add("anchor", &gid, &w.env.owner_token, task_link(&key(), b_id, t, v)));
    let results = futures_join(calls).await;
    assert_eq!(results.iter().filter(|(s, _)| *s == 201).count(), 1, "{results:?}");
    for (status, body) in results.iter().filter(|(s, _)| *s != 201) {
        assert_eq!(*status, 409, "{body}");
        assert_eq!(body["error"]["code"], "VERSION_CONFLICT");
        assert_eq!(body["error"]["details"]["current_version"], v + 1);
    }
    assert_eq!(w.active_links(&gid).await, 2);
    assert_eq!(w.version_of(&gid).await, v + 1);

    // A failed precondition (commit not in the target) writes nothing.
    let v = v + 1;
    let anchor_sha = w.env.push_commit("anchor", "anchor only");
    let (status, _) = w.add("anchor", &gid, &w.env.owner_token, commit_link(&key(), b_id, &anchor_sha, v)).await;
    assert_eq!(status, 400, "a commit of another repository is not a commit of the target");
    let (status, _) = w
        .add("anchor", &gid, &w.env.owner_token, task_link(&key(), b_id, &uuid::Uuid::new_v4().to_string(), v))
        .await;
    assert_eq!(status, 404);
    assert_eq!(w.version_of(&gid).await, v);
}

/// Run the calls concurrently on this task and collect their results.
async fn futures_join<F: std::future::Future<Output = (u16, Value)>>(
    calls: impl Iterator<Item = F>,
) -> Vec<(u16, Value)> {
    futures::future::join_all(calls.collect::<Vec<_>>()).await
}

#[tokio::test]
async fn m02_removal_is_soft_idempotent_and_versioned() {
    let w = W::new().await;
    w.repo("anchor", false).await;
    let b_id = w.repo("target-b", false).await;
    let t1 = w.task("target-b", "one").await;
    let (gid, v) = w.group("anchor", "g").await;
    let (link_id, v) = w.link_task("anchor", &gid, v, b_id, &t1).await;
    let rm = w.gurl("anchor", &format!("/{gid}/links/{link_id}"));

    // Stale version first.
    let (status, err) = w
        .call(Method::DELETE, &rm, &w.env.owner_token, None, Some(json!({"client_request_id": key(), "expected_group_version": v - 1})))
        .await;
    assert_eq!((status, err["error"]["code"].as_str()), (409, Some("VERSION_CONFLICT")), "{err}");
    assert_eq!(w.active_links(&gid).await, 1);

    // Five racing copies of one removal: one effect, the rest replay.
    let k = key();
    let body = json!({"client_request_id": k, "expected_group_version": v});
    let calls = (0..5).map(|_| w.call(Method::DELETE, &rm, &w.env.owner_token, None, Some(body.clone())));
    let results = futures_join(calls).await;
    assert!(results.iter().all(|(s, b)| *s == 200 && b["removed_at"].is_string()), "{results:?}");
    assert_eq!(w.version_of(&gid).await, v + 1, "one bump");
    assert_eq!(w.active_links(&gid).await, 0);

    // Replay later, different body under the same key, different key.
    let (status, _) = w.call(Method::DELETE, &rm, &w.env.owner_token, None, Some(body.clone())).await;
    assert_eq!(status, 200);
    let (status, err) = w
        .call(Method::DELETE, &rm, &w.env.owner_token, None, Some(json!({"client_request_id": k, "expected_group_version": v + 5})))
        .await;
    assert_eq!((status, err["error"]["code"].as_str()), (409, Some("IDEMPOTENCY_KEY_REUSED")));
    let (status, err) = w
        .call(Method::DELETE, &rm, &w.env.owner_token, None, Some(json!({"client_request_id": key(), "expected_group_version": v + 1})))
        .await;
    assert_eq!((status, err["error"]["code"].as_str()), (409, Some("LINK_ALREADY_REMOVED")));

    // The removed link is gone from lists, kept as a row, and the target can be linked again.
    let (_, page) = w.links("anchor", &gid, &w.env.owner_token, "").await;
    assert!(page["items"].as_array().unwrap().is_empty());
    {
        use rg_db::entities::collab_coordination_link as l;
        let row = l::Entity::find_by_id(link_id.clone()).one(&w.env.db).await.unwrap().unwrap();
        assert!(row.removed_at.is_some() && row.active_key.is_none());
        assert_eq!(row.removed_actor_kind.as_deref(), Some("user"));
    }
    let (new_id, v) = w.link_task("anchor", &gid, v + 1, b_id, &t1).await;
    assert_ne!(new_id, link_id);

    // Two different removals racing on one version: one wins.
    let t2 = w.task("target-b", "two").await;
    let (second, v) = w.link_task("anchor", &gid, v, b_id, &t2).await;
    let rm1 = w.gurl("anchor", &format!("/{gid}/links/{new_id}"));
    let rm2 = w.gurl("anchor", &format!("/{gid}/links/{second}"));
    let (a, b) = tokio::join!(
        w.call(Method::DELETE, &rm1, &w.env.owner_token, None, Some(json!({"client_request_id": key(), "expected_group_version": v}))),
        w.call(Method::DELETE, &rm2, &w.env.owner_token, None, Some(json!({"client_request_id": key(), "expected_group_version": v}))),
    );
    let mut statuses = [a.0, b.0];
    statuses.sort();
    assert_eq!(statuses, [200, 409], "{a:?} {b:?}");
    assert_eq!(w.active_links(&gid).await, 1);

    // Audit rows reference ids, never content.
    use rg_db::entities::audit_log;
    let rows = audit_log::Entity::find()
        .filter(audit_log::Column::Action.starts_with("collab.group."))
        .all(&w.env.db)
        .await
        .unwrap();
    assert!(rows.iter().any(|r| r.action == "collab.group.link_remove"), "{:?}", rows.iter().map(|r| &r.action).collect::<Vec<_>>());
    assert!(rows.iter().any(|r| r.action == "collab.group.link_add"));
    assert!(rows.iter().all(|r| !r.details.clone().unwrap_or_default().contains("one")));
}

#[tokio::test]
async fn m02_inputs_are_validated_and_unknown_fields_rejected() {
    let w = W::new().await;
    w.repo("anchor", false).await;
    let b_id = w.repo("target-b", false).await;
    let t = w.task("target-b", "one").await;
    let (gid, v) = w.group("anchor", "g").await;
    let tok = &w.env.owner_token;

    let (status, _) = w.add("anchor", &gid, tok, json!({"client_request_id": key(), "target_repo_id": b_id, "expected_group_version": v})).await;
    assert_eq!(status, 400, "a link needs a task or a commit");
    let (status, _) = w.add("anchor", &gid, tok, commit_link(&key(), b_id, "abc", v)).await;
    assert_eq!(status, 400, "short sha");
    let mut typo = task_link(&key(), b_id, &t, v);
    typo["target_task"] = json!("x");
    let (status, _) = w.add("anchor", &gid, tok, typo).await;
    assert!(status == 400 || status == 422, "unknown write field rejected, got {status}");
    let (status, _) = w.call(Method::POST, &w.gurl("anchor", ""), tok, None, Some(json!({"client_request_id": key(), "title": "  "}))).await;
    assert_eq!(status, 400);
    let (status, _) = w.call(Method::POST, &w.gurl("anchor", ""), tok, None, Some(json!({"client_request_id": key(), "title": "t", "tittle": "x"}))).await;
    assert!(status == 400 || status == 422);
    // Group ids of other repositories and malformed ids are plain 404s.
    let (status, _) = w.call(Method::GET, &w.gurl("target-b", &format!("/{gid}")), tok, None, None).await;
    assert_eq!(status, 404);
    let (status, _) = w.call(Method::GET, &w.gurl("anchor", "/not-a-uuid"), tok, None, None).await;
    assert_eq!(status, 404);
    // Group creation is idempotent per key.
    let k = key();
    let body = json!({"client_request_id": k, "title": "same"});
    let (s1, g1) = w.call(Method::POST, &w.gurl("anchor", ""), tok, None, Some(body.clone())).await;
    let (s2, g2) = w.call(Method::POST, &w.gurl("anchor", ""), tok, None, Some(body)).await;
    assert_eq!((s1, s2, g1["id"] == g2["id"]), (201, 200, true));
    let (status, _) = w.call(Method::POST, &w.gurl("anchor", ""), tok, None, Some(json!({"client_request_id": k, "title": "other"}))).await;
    assert_eq!(status, 409);
    // A session acts as itself.
    let sess = w.session("anchor", tok).await;
    let (status, g) = w
        .call(Method::POST, &w.gurl("anchor", ""), tok, Some(&sess), Some(json!({"client_request_id": key(), "title": "by agent"})))
        .await;
    assert_eq!((status, g["created_by_kind"].as_str()), (201, Some("session")));
}

// ------------------------------------------------------------------ M03 / M04

/// Six links, targets B1..B6 in creation order; `reader` can read B1, B3, B5.
async fn paged_world(w: &W) -> (String, Vec<i64>, String, i64) {
    w.repo("anchor", true).await;
    let (reader, reader_id) = w.env.user("reader").await;
    w.grant("anchor", reader_id, "read").await;
    let mut ids = vec![];
    let (gid, mut v) = w.group("anchor", "paged").await;
    for i in 1..=6 {
        let name = format!("b{i}");
        let id = w.repo(&name, true).await;
        if i % 2 == 1 {
            w.grant(&name, reader_id, "read").await;
        }
        let t = w.task(&name, &format!("task {i}")).await;
        v = w.link_task("anchor", &gid, v, id, &t).await.1;
        ids.push(id);
    }
    (gid, ids, reader, reader_id)
}

#[tokio::test]
async fn m03_m04_pages_count_only_what_the_reader_can_see() {
    let w = W::new().await;
    let (gid, _ids, reader, _reader_id) = paged_world(&w).await;

    // Equal database timestamps use id as the documented tie-breaker; this
    // order need not match insertion order (notably on MySQL DATETIME).
    let (_, all) = w.links("anchor", &gid, &w.env.owner_token, "").await;
    let ordered = names(&all);
    assert_eq!(ordered.len(), 6);
    let visible: Vec<_> = ordered.iter().filter(|n| matches!(n.as_str(), "b1" | "b3" | "b5")).cloned().collect();
    assert_eq!(visible.len(), 3);
    let (status, p1) = w.links("anchor", &gid, &reader, "?limit=2").await;
    assert_eq!(status, 200, "{p1}");
    assert_eq!(names(&p1), visible[..2]);
    assert_eq!(p1["has_more"], true);
    let (_, p2) = w.links("anchor", &gid, &reader, &format!("?limit=2&cursor={}", p1["next_cursor"].as_str().unwrap())).await;
    assert_eq!(names(&p2), visible[2..]);
    assert_eq!(p2["has_more"], false);
    assert!(p2["next_cursor"].is_null());
    let (_, p) = w.links("anchor", &gid, &reader, "?limit=3").await;
    assert_eq!((names(&p), p["has_more"].as_bool(), p["next_cursor"].is_null()), (visible, Some(false), true));
    assert_eq!(all["items"][0]["target_repo"]["owner"], w.env.owner.as_str());
}

#[tokio::test]
async fn m03_permissions_are_rechecked_on_every_page() {
    let w = W::new().await;
    let (gid, _ids, reader, reader_id) = paged_world(&w).await;

    let (_, initial) = w.links("anchor", &gid, &reader, "").await;
    let visible = names(&initial);
    assert_eq!(visible.len(), 3);
    let remaining = visible[2].clone();
    let (_, p1) = w.links("anchor", &gid, &reader, "?limit=2").await;
    assert_eq!(names(&p1), visible[..2]);
    let cursor = p1["next_cursor"].as_str().unwrap().to_string();

    // Revalidate permission for the link that would be on the next page.
    w.revoke(&remaining, reader_id).await;
    let (_, p2) = w.links("anchor", &gid, &reader, &format!("?limit=2&cursor={cursor}")).await;
    assert!(p2["items"].as_array().unwrap().is_empty(), "{p2}");
    assert_eq!((p2["has_more"].as_bool(), p2["next_cursor"].is_null()), (Some(false), true));
    w.grant(&remaining, reader_id, "read").await;
    let (_, p2) = w.links("anchor", &gid, &reader, &format!("?limit=2&cursor={cursor}")).await;
    assert_eq!(names(&p2), vec![remaining.clone()]);

    // A newly visible link behind the cursor is only seen on a fresh scan.
    let (_, owner_page) = w.links("anchor", &gid, &w.env.owner_token, "").await;
    let owner_order = names(&owner_page);
    let boundary = owner_order.iter().position(|name| name == &visible[1]).unwrap();
    if let Some(hidden) = owner_order[..boundary].iter().find(|name| !visible.contains(name)) {
        w.grant(hidden, reader_id, "read").await;
        let (_, p2) = w.links("anchor", &gid, &reader, &format!("?limit=2&cursor={cursor}")).await;
        assert!(!names(&p2).contains(hidden));
        let (_, fresh) = w.links("anchor", &gid, &reader, "").await;
        assert!(names(&fresh).contains(hidden));
    }

    // Cursors are bound to the account and to this listing, and cannot be forged.
    let (other, other_id) = w.env.user("other").await;
    w.grant("anchor", other_id, "read").await;
    let (status, _) = w.links("anchor", &gid, &other, &format!("?cursor={cursor}")).await;
    assert_eq!(status, 400, "another account's cursor");
    let (status, _) = w.links("anchor", &gid, &reader, "?cursor=garbage").await;
    assert_eq!(status, 400);
    let tampered = format!("{}x", cursor);
    let (status, _) = w.links("anchor", &gid, &reader, &format!("?cursor={tampered}")).await;
    assert_eq!(status, 400);

    // The cursor survives a restart of the server (no per-process state).
    let base2 = w.env.restart().await;
    let url = w.gurl("anchor", &format!("/{gid}/links?limit=2&cursor={cursor}")).replace(&w.env.base, &base2);
    let (status, p) = w.call(Method::GET, &url, &reader, None, None).await;
    assert_eq!(status, 200, "{p}");
    assert_eq!(names(&p), vec![remaining]);
}

#[tokio::test]
async fn m04_filtering_scans_on_until_the_page_is_full() {
    let w = W::new().await;
    w.repo("anchor", true).await;
    let hidden_id = w.repo("hidden-pile", true).await;
    let seen_id = w.repo("seen", true).await;
    let (reader, reader_id) = w.env.user("reader").await;
    w.grant("anchor", reader_id, "read").await;
    w.grant("seen", reader_id, "read").await;
    let (gid, mut v) = w.group("anchor", "big").await;

    // 105 links the reader cannot see (more than one scan batch), then 3 they can.
    for i in 0..105 {
        let t = w.task("hidden-pile", &format!("h{i}")).await;
        v = w.link_task("anchor", &gid, v, hidden_id, &t).await.1;
    }
    for i in 0..3 {
        let t = w.task("seen", &format!("s{i}")).await;
        v = w.link_task("anchor", &gid, v, seen_id, &t).await.1;
    }

    let (_, p1) = w.links("anchor", &gid, &reader, "?limit=2").await;
    assert_eq!(p1["items"].as_array().unwrap().len(), 2, "page filled despite 105 hidden links in front");
    assert_eq!(p1["has_more"], true);
    let (_, p2) = w.links("anchor", &gid, &reader, &format!("?limit=2&cursor={}", p1["next_cursor"].as_str().unwrap())).await;
    assert_eq!(p2["items"].as_array().unwrap().len(), 1);
    assert_eq!(p2["has_more"], false);

    // A reader who sees none gets an empty, final page — not a "keep going" page.
    let (nobody, nobody_id) = w.env.user("nobody").await;
    w.grant("anchor", nobody_id, "read").await;
    let (_, empty) = w.links("anchor", &gid, &nobody, "?limit=2").await;
    assert!(empty["items"].as_array().unwrap().is_empty());
    assert_eq!((empty["has_more"].as_bool(), empty["next_cursor"].is_null()), (Some(false), true));
    let raw = empty.to_string();
    assert!(!raw.contains("hidden-pile") && !raw.contains("105") && !raw.contains("108"), "{raw}");
}

#[tokio::test]
async fn m03_rename_soft_delete_and_restore_follow_the_repository_id() {
    let w = W::new().await;
    w.repo("anchor", false).await;
    let b_id = w.repo("moving", false).await;
    let c_id = w.repo("doomed", false).await;
    let tb = w.task("moving", "tb").await;
    let tc = w.task("doomed", "tc").await;
    let sha = w.env.push_commit("moving", "c1");
    let (gid, v) = w.group("anchor", "g").await;
    let (_, v) = w.link_task("anchor", &gid, v, b_id, &tb).await;
    let (_, v) = w.link_task("anchor", &gid, v, c_id, &tc).await;
    let (status, _) = w.add("anchor", &gid, &w.env.owner_token, commit_link(&key(), b_id, &sha, v)).await;
    assert_eq!(status, 201);

    // "Rename" by moving the repository into an organization.
    let client = reqwest::Client::new();
    let org = format!("acme{}", &key()[..6]);
    let resp = client.post(format!("{}/api/v1/orgs", w.env.base)).bearer_auth(&w.env.owner_token)
        .json(&json!({"name": org, "display_name": "Acme"})).send().await.unwrap();
    assert_eq!(resp.status(), 201);
    let resp = client.post(format!("{}/api/v1/repos/{}/moving/transfer", w.env.base, w.env.owner))
        .bearer_auth(&w.env.owner_token).json(&json!({"new_owner": org})).send().await.unwrap();
    assert_eq!(resp.status(), 200, "transfer");

    let (_, page) = w.links("anchor", &gid, &w.env.owner_token, "").await;
    let moved: Vec<_> = page["items"].as_array().unwrap().iter().filter(|l| l["target_repo"]["id"] == b_id).collect();
    assert_eq!(moved.len(), 2);
    assert!(moved.iter().all(|l| l["target_repo"]["owner"] == org.as_str()), "resolved at read time: {page}");
    assert!(moved.iter().any(|l| l["commit"]["commit_state"] == "available" && l["commit"]["sha"] == sha.as_str()));

    // Soft delete: the link disappears from the visible list; nothing says why.
    let resp = client.delete(format!("{}/api/v1/repos/{}/doomed", w.env.base, w.env.owner))
        .bearer_auth(&w.env.owner_token).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let (_, page) = w.links("anchor", &gid, &w.env.owner_token, "").await;
    assert_eq!(page["items"].as_array().unwrap().len(), 2);
    assert!(!page.to_string().contains("doomed"));
    assert_eq!(w.active_links(&gid).await, 3, "the row is kept");

    // Restore (simulated: the instance has no restore endpoint): visible again.
    use rg_db::entities::repository;
    let row = repository::Entity::find_by_id(c_id).one(&w.env.db).await.unwrap().unwrap();
    let mut active: repository::ActiveModel = row.into();
    active.deleted_at = Set(None);
    active.update(&w.env.db).await.unwrap();
    let (_, page) = w.links("anchor", &gid, &w.env.owner_token, "").await;
    assert_eq!(page["items"].as_array().unwrap().len(), 3);

    // A deleted anchor is not reachable through its group routes.
    let resp = client.delete(format!("{}/api/v1/repos/{}/anchor", w.env.base, w.env.owner))
        .bearer_auth(&w.env.owner_token).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let (status, _) = w.links("anchor", &gid, &w.env.owner_token, "").await;
    assert_eq!(status, 404);
}

// ------------------------------------------------------------------ M05

#[tokio::test]
async fn m05_a_collected_commit_becomes_unavailable_and_keeps_its_sha() {
    let w = W::new().await;
    w.repo("anchor", false).await;
    let b_id = w.repo("target-b", false).await;
    let sha = w.env.push_commit("target-b", "to be collected");
    let (gid, v) = w.group("anchor", "g").await;
    let k = key();
    let (status, link) = w.add("anchor", &gid, &w.env.owner_token, commit_link(&k, b_id, &sha, v)).await;
    assert_eq!(status, 201, "{link}");
    assert_eq!(link["commit"]["commit_state"], "available");

    // Remove the (loose) objects the way a prune would.
    let objects = w.env.bare_path("target-b").join("objects");
    let loose = objects.join(&sha[..2]).join(&sha[2..]);
    assert!(loose.exists(), "the pushed commit is a loose object in this test");
    std::fs::remove_file(&loose).unwrap();

    let (_, page) = w.links("anchor", &gid, &w.env.owner_token, "").await;
    let item = &page["items"][0];
    assert_eq!(item["commit"]["commit_state"], "unavailable");
    assert_eq!(item["commit"]["sha"], sha.as_str(), "never another sha");

    // A retry of the successful request still replays, even though the commit is gone now.
    let (status, again) = w
        .add("anchor", &gid, &w.env.owner_token, commit_link(&k, b_id, &sha, v))
        .await;
    assert_eq!((status, again["id"] == link["id"]), (200, true), "{again}");
    // ...but a new request for it is refused.
    let (status, _) = w.add("anchor", &gid, &w.env.owner_token, commit_link(&key(), b_id, &sha, v + 1)).await;
    assert_eq!(status, 400);
}

