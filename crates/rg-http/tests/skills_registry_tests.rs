//! The skill registry: publish a skill from a Git ref, resolve it, read it back.
//!
//! These pin the properties an agent relies on when it pins a skill by digest:
//! a version is built by the server from one commit and never changes; a
//! constraint resolves to one exact non-yanked version; what a publisher wrote
//! is labelled `user_content`; and the registry's reads and its write follow the
//! repository's own permissions.

mod common;
use common::{register_user, spawn_test_app_on_env_db};

use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const PW: &str = "Qz7$wRtm";

const PDF_SKILL: &str =
    "---\nname: pdf-tools\ndescription: Read and fill PDF forms.\nlicense: MIT\n\
                         allowed-tools: Read Bash(python:*)\n---\n# PDF tools\nUse the script.\n";

struct Fixture {
    base: String,
    jwt: String,
    owner: &'static str,
    client: Client,
}

impl Fixture {
    async fn new(owner: &'static str) -> Fixture {
        let (base, _db) = spawn_test_app_on_env_db().await;
        let jwt = register_user(&base, owner, &format!("{owner}@example.com"), PW).await;
        let client = Client::new();
        let resp = client
            .post(format!("{base}/api/v1/repos"))
            .bearer_auth(&jwt)
            .json(&json!({"name": "work", "auto_init": true, "is_private": true}))
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

    /// The tip of `main`, read through the authenticated API (the repository
    /// is private, so an anonymous `ls-remote` cannot see it).
    async fn tip(&self) -> String {
        let log: Value = self
            .client
            .get(self.url("/log"))
            .query(&[("ref", "main"), ("limit", "1")])
            .bearer_auth(&self.jwt)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        log["commits"][0]["sha"]
            .as_str()
            .expect("main has a commit")
            .to_string()
    }

    async fn blob_sha(&self, path: &str) -> String {
        let body: Value = self
            .client
            .get(self.url(&format!("/blob/{path}")))
            .query(&[("ref", "main")])
            .bearer_auth(&self.jwt)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        body["sha"].as_str().expect("a blob sha").to_string()
    }

    /// One commit on `main` made of `files` (all creates), returning its sha.
    async fn commit_files(&self, files: &[(&str, &str)]) -> String {
        let edits: Vec<Value> = files
            .iter()
            .map(|(path, content)| json!({"operation": "create", "path": path, "content": content}))
            .collect();
        self.commit_edits(edits).await
    }

    async fn commit_edits(&self, edits: Vec<Value>) -> String {
        let resp = self
            .client
            .post(self.url("/commits"))
            .bearer_auth(&self.jwt)
            .json(&json!({
                "branch": "main",
                "expected_sha": self.tip().await,
                "message": "skills",
                "files": edits,
            }))
            .send()
            .await
            .unwrap();
        let status = resp.status();
        let body: Value = resp.json().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{body}");
        body["commit_sha"].as_str().unwrap().to_string()
    }

    /// Seed `skills/pdf` (three files) and return the commit.
    async fn seed_pdf(&self) -> String {
        self.commit_files(&[
            ("skills/pdf/SKILL.md", PDF_SKILL),
            ("skills/pdf/scripts/run.py", "print('hi')\n"),
            ("skills/pdf/reference.md", "Forms reference.\n"),
        ])
        .await
    }

    async fn post_publish(&self, bearer: Option<&str>, body: Value) -> (StatusCode, Value) {
        let mut req = self.client.post(self.url("/skills/publish"));
        if let Some(token) = bearer {
            req = req.bearer_auth(token);
        }
        let resp = req.json(&body).send().await.unwrap();
        let status = resp.status();
        (status, resp.json().await.unwrap_or(Value::Null))
    }

    async fn publish(&self, body: Value) -> (StatusCode, Value) {
        self.post_publish(Some(&self.jwt.clone()), body).await
    }

    async fn get_as(&self, bearer: Option<&str>, suffix: &str) -> (StatusCode, Value) {
        let mut req = self.client.get(self.url(suffix));
        if let Some(token) = bearer {
            req = req.bearer_auth(token);
        }
        let resp = req.send().await.unwrap();
        let status = resp.status();
        (status, resp.json().await.unwrap_or(Value::Null))
    }

    async fn get(&self, suffix: &str) -> (StatusCode, Value) {
        self.get_as(Some(&self.jwt.clone()), suffix).await
    }
}

fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_skill_is_published_from_a_ref_and_read_back() {
    let f = Fixture::new("skill_round_trip").await;
    let commit = f.seed_pdf().await;

    let (status, v) = f
        .publish(json!({"path": "skills/pdf", "ref": "main", "version": "1.0.0"}))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{v}");
    assert_eq!(v["name"], "pdf-tools");
    assert_eq!(v["version"], "1.0.0");
    assert_eq!(v["existing"], json!(false));
    assert_eq!(
        v["commit"],
        json!(commit),
        "the ref was resolved to its commit"
    );
    let content_digest = v["content_digest"].as_str().unwrap().to_string();
    let archive_digest = v["archive_digest"].as_str().unwrap().to_string();
    assert!(content_digest.starts_with("sha256:") && archive_digest.starts_with("sha256:"));

    // Listing: one skill, description from its frontmatter, labelled user content.
    let (status, list) = f.get("/skills").await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let skills = list["skills"].as_array().unwrap();
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0]["name"], "pdf-tools");
    assert_eq!(skills[0]["description"], "Read and fill PDF forms.");
    assert_eq!(skills[0]["latest_version"], "1.0.0");
    assert_eq!(skills[0]["origin"], "user_content");

    // The skill and its versions.
    let (status, skill) = f.get("/skills/pdf-tools").await;
    assert_eq!(status, StatusCode::OK, "{skill}");
    assert_eq!(skill["latest_version"], "1.0.0");
    assert_eq!(skill["versions"][0]["commit"], json!(commit));
    assert_eq!(skill["versions"][0]["yanked"], json!(false));

    // The manifest: frontmatter, file list, digests, and a source the server built.
    let (status, manifest) = f.get("/skills/pdf-tools/versions/1.0.0").await;
    assert_eq!(status, StatusCode::OK, "{manifest}");
    assert_eq!(manifest["origin"], "user_content");
    assert_eq!(manifest["source_verified"], json!(true));
    assert_eq!(manifest["source"]["built_by"], "server");
    assert_eq!(manifest["source"]["commit"], json!(commit));
    assert_eq!(manifest["source"]["path"], "skills/pdf");
    assert_eq!(manifest["source"]["git_ref"], "main");
    assert_eq!(manifest["content_digest"], json!(content_digest));
    assert_eq!(manifest["frontmatter"]["license"], "MIT");
    assert_eq!(
        manifest["frontmatter"]["allowed-tools"], "Read Bash(python:*)",
        "allowed-tools is recorded verbatim, never enforced"
    );
    let paths: Vec<&str> = manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, ["SKILL.md", "reference.md", "scripts/run.py"]);
    assert_eq!(
        manifest["files"][1]["sha256"],
        json!(sha256_hex(b"Forms reference.\n"))
    );

    // SKILL.md as text, without downloading the archive.
    let (status, md) = f.get("/skills/pdf-tools/versions/1.0.0/content").await;
    assert_eq!(status, StatusCode::OK, "{md}");
    assert_eq!(md["content"], PDF_SKILL);
    assert_eq!(md["origin"], "user_content");
    assert_eq!(md["content_digest"], json!(content_digest));

    // One other text file, and the refusals.
    let (status, file) = f
        .get("/skills/pdf-tools/versions/1.0.0/files/scripts/run.py")
        .await;
    assert_eq!(status, StatusCode::OK, "{file}");
    assert_eq!(file["content"], "print('hi')\n");
    let (status, missing) = f
        .get("/skills/pdf-tools/versions/1.0.0/files/nope.md")
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(missing["error"]["code"], "SKILL_FILE_NOT_FOUND");
    // A path the bundle format forbids is refused before any lookup. (A `..`
    // segment cannot be sent through an HTTP client at all - the URL parser
    // resolves it first - so the traversal rules themselves are covered by the
    // `validate_entry_path` unit tests.)
    let (status, forbidden) = f
        .get("/skills/pdf-tools/versions/1.0.0/files/.git/config")
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{forbidden}");
    assert_eq!(forbidden["error"]["code"], "SKILL_PATH_INVALID");

    // The archive: the digest header is the digest of exactly these bytes, and
    // the bundle inside reads back with the recorded content digest.
    let resp = f
        .client
        .get(f.url("/skills/pdf-tools/versions/1.0.0/archive"))
        .bearer_auth(&f.jwt)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()["content-type"], "application/gzip");
    assert_eq!(resp.headers()["x-content-type-options"], "nosniff");
    let header_digest = resp.headers()["x-checksum-sha256"]
        .to_str()
        .unwrap()
        .to_string();
    assert_eq!(resp.headers()["x-content-digest"], content_digest.as_str());
    let bytes = resp.bytes().await.unwrap();
    assert_eq!(format!("sha256:{}", sha256_hex(&bytes)), archive_digest);
    assert_eq!(format!("sha256:{header_digest}"), archive_digest);
    let bundle = rg_core::package_registry::skill_bundle::read_bundle(&bytes).unwrap();
    assert_eq!(bundle.content_digest, content_digest);
    assert_eq!(bundle.files.len(), 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_published_version_is_idempotent_and_never_changes() {
    let f = Fixture::new("skill_immutable").await;
    let first_commit = f.seed_pdf().await;
    let req = json!({"path": "skills/pdf", "ref": "main", "version": "1.0.0"});

    let (status, first) = f.publish(req.clone()).await;
    assert_eq!(status, StatusCode::CREATED, "{first}");

    // Same version, same tree: a no-op that says so.
    let (status, again) = f.publish(req.clone()).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["existing"], json!(true));
    assert_eq!(again["content_digest"], first["content_digest"]);
    assert_eq!(again["version_id"], first["version_id"]);

    // The tree changes on main. The same version must now be refused, naming
    // both digests, and the stored version must be untouched.
    let sha = f.blob_sha("skills/pdf/reference.md").await;
    f.commit_edits(vec![json!({
        "operation": "update", "path": "skills/pdf/reference.md",
        "content": "Changed.\n", "sha": sha,
    })])
    .await;
    let (status, conflict) = f.publish(req.clone()).await;
    assert_eq!(status, StatusCode::CONFLICT, "{conflict}");
    assert_eq!(conflict["error"]["reason"], "VERSION_IMMUTABLE");
    assert_eq!(
        conflict["error"]["details"]["existing_digest"], first["content_digest"],
        "the conflict names the digest that is stored"
    );
    assert_ne!(
        conflict["error"]["details"]["new_digest"],
        conflict["error"]["details"]["existing_digest"]
    );
    let (_, manifest) = f.get("/skills/pdf-tools/versions/1.0.0").await;
    assert_eq!(manifest["content_digest"], first["content_digest"]);
    assert_eq!(manifest["source"]["commit"], json!(first_commit));

    // Re-publishing the *old* commit under the same version is still a no-op.
    let (status, replay) = f
        .publish(json!({"path": "skills/pdf", "ref": first_commit, "version": "1.0.0"}))
        .await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay["existing"], json!(true));

    // The changed content is publishable as a new version.
    let (status, second) = f
        .publish(json!({"path": "skills/pdf", "ref": "main", "version": "1.1.0"}))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{second}");
    assert_ne!(second["content_digest"], first["content_digest"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_constraint_resolves_to_one_exact_non_yanked_version() {
    let f = Fixture::new("skill_resolve").await;
    f.seed_pdf().await;
    let (status, v1) = f
        .publish(json!({"path": "skills/pdf", "ref": "main", "version": "1.0.0"}))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{v1}");

    let sha = f.blob_sha("skills/pdf/reference.md").await;
    f.commit_edits(vec![json!({
        "operation": "update", "path": "skills/pdf/reference.md",
        "content": "v1.1\n", "sha": sha,
    })])
    .await;
    let (_, v11) = f
        .publish(json!({"path": "skills/pdf", "ref": "main", "version": "1.1.0"}))
        .await;
    let sha = f.blob_sha("skills/pdf/reference.md").await;
    f.commit_edits(vec![json!({
        "operation": "update", "path": "skills/pdf/reference.md",
        "content": "v2-rc\n", "sha": sha,
    })])
    .await;
    let (status, rc) = f
        .publish(json!({"path": "skills/pdf", "ref": "main", "version": "2.0.0-rc.1"}))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{rc}");

    let resolve = |constraint: &'static str| {
        let f = &f;
        async move {
            f.get(&format!(
                "/skills/pdf-tools/resolve?constraint={}",
                constraint.replace(' ', "%20").replace('^', "%5E")
            ))
            .await
        }
    };

    let (status, r) = resolve("^1").await;
    assert_eq!(status, StatusCode::OK, "{r}");
    assert_eq!(r["version"], "1.1.0");
    assert_eq!(r["content_digest"], v11["content_digest"]);
    assert_eq!(r["archive_digest"], v11["archive_digest"]);
    assert_eq!(r["commit"], v11["commit"]);

    let (_, r) = resolve("=1.0.0").await;
    assert_eq!(r["version"], "1.0.0");

    // `latest` and no constraint: the highest *stable* version, not the rc.
    let (_, r) = resolve("latest").await;
    assert_eq!(r["version"], "1.1.0");
    let (_, r) = f.get("/skills/pdf-tools/resolve").await;
    assert_eq!(r["version"], "1.1.0");
    let (_, skill) = f.get("/skills/pdf-tools").await;
    assert_eq!(skill["latest_version"], "1.1.0");

    // A pre-release is chosen only when asked for by name.
    let (_, r) = resolve(">=2.0.0-rc.1").await;
    assert_eq!(r["version"], "2.0.0-rc.1");

    // Yank 1.1.0 through the generic package route: it is no longer chosen.
    let resp = f
        .client
        .patch(f.url("/packages/skill/pdf-tools/1.1.0/yank"))
        .bearer_auth(&f.jwt)
        .json(&json!({"yank": true}))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "yank: {}", resp.status());
    let (_, r) = resolve("^1").await;
    assert_eq!(r["version"], "1.0.0", "a yanked version is skipped");
    let (_, skill) = f.get("/skills/pdf-tools").await;
    assert_eq!(skill["latest_version"], "1.0.0");
    let yanked = skill["versions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["version"] == "1.1.0")
        .unwrap();
    assert_eq!(
        yanked["yanked"],
        json!(true),
        "but it is still listed, marked"
    );

    // Nothing matches: a 404 that lists what would.
    let (status, none) = resolve("^3").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(none["error"]["code"], "NO_MATCHING_VERSION");
    let available = none["error"]["details"]["available"].as_array().unwrap();
    assert!(available.iter().any(|v| v == "1.0.0"));
    assert!(
        !available.iter().any(|v| v == "1.1.0"),
        "yanked is not offered"
    );

    let (status, bad) = resolve("not a version").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(bad["error"]["code"], "SKILL_CONSTRAINT_INVALID");

    let (status, unknown) = f.get("/skills/nothing/resolve").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(unknown["error"]["code"], "SKILL_NOT_FOUND");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn what_is_not_a_valid_skill_is_refused_with_a_code() {
    let f = Fixture::new("skill_refusals").await;
    f.commit_files(&[
        ("skills/pdf/SKILL.md", PDF_SKILL),
        ("skills/nodesc/SKILL.md", "---\nname: nodesc\n---\nbody\n"),
        (
            "skills/badname/SKILL.md",
            "---\nname: Bad_Name\ndescription: x\n---\n",
        ),
        (
            "skills/reserved/SKILL.md",
            "---\nname: publish\ndescription: x\n---\n",
        ),
        ("skills/empty/notes.md", "no skill here\n"),
    ])
    .await;

    let code = |v: &Value| v["error"]["code"].as_str().unwrap_or_default().to_string();

    // No SKILL.md in that directory.
    let (status, v) = f
        .publish(json!({"path": "skills/empty", "ref": "main", "version": "1.0.0"}))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(code(&v), "SKILL_MISSING_SKILL_MD");

    let (status, v) = f
        .publish(json!({"path": "skills/nodesc", "ref": "main", "version": "1.0.0"}))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(code(&v), "SKILL_DESCRIPTION_INVALID");

    let (status, v) = f
        .publish(json!({"path": "skills/badname", "ref": "main", "version": "1.0.0"}))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(code(&v), "SKILL_NAME_INVALID");

    let (status, v) = f
        .publish(json!({"path": "skills/reserved", "ref": "main", "version": "1.0.0"}))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(code(&v), "SKILL_NAME_RESERVED");

    // The path or ref does not exist.
    let (status, v) = f
        .publish(json!({"path": "skills/missing", "ref": "main", "version": "1.0.0"}))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{v}");
    assert_eq!(code(&v), "SKILL_PATH_NOT_FOUND");
    // A file is not a directory.
    let (status, v) = f
        .publish(json!({"path": "skills/pdf/SKILL.md", "ref": "main", "version": "1.0.0"}))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{v}");
    assert_eq!(code(&v), "SKILL_PATH_NOT_FOUND");
    let (status, v) = f
        .publish(json!({"path": "skills/pdf", "ref": "no-such-branch", "version": "1.0.0"}))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{v}");
    assert_eq!(code(&v), "SKILL_REF_NOT_FOUND");

    // A ref cannot be a revision expression.
    for bad in ["main^", "main~1", "main:skills", "HEAD^{tree}", "-x"] {
        let (status, v) = f
            .publish(json!({"path": "skills/pdf", "ref": bad, "version": "1.0.0"}))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}: {v}");
        assert_eq!(code(&v), "SKILL_REF_INVALID", "{bad}");
    }
    for bad in ["../x", "skills//pdf", ".git/x"] {
        let (status, v) = f
            .publish(json!({"path": bad, "ref": "main", "version": "1.0.0"}))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}: {v}");
        assert_eq!(code(&v), "SKILL_PATH_INVALID", "{bad}");
    }

    // Versions: required, canonical, not contradicting the frontmatter.
    let (status, v) = f
        .publish(json!({"path": "skills/pdf", "ref": "main"}))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(code(&v), "SKILL_VERSION_REQUIRED");
    for bad in ["1.0", "v1.0.0", "1.0.0+build"] {
        let (status, v) = f
            .publish(json!({"path": "skills/pdf", "ref": "main", "version": bad}))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}: {v}");
        assert_eq!(code(&v), "SKILL_VERSION_INVALID", "{bad}");
    }

    // An unknown request field is an error, not ignored.
    let (status, _) = f
        .publish(json!({"path": "skills/pdf", "ref": "main", "version": "1.0.0", "force": true}))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // Nothing above published anything.
    let (_, list) = f.get("/skills").await;
    assert_eq!(list["skills"].as_array().unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_version_in_the_frontmatter_is_used_and_must_agree_with_the_request() {
    let f = Fixture::new("skill_version_src").await;
    f.commit_files(&[(
        "SKILL.md",
        "---\nname: root-skill\ndescription: At the repository root.\nmetadata:\n  version: \"0.3.0\"\n---\nbody\n",
    )])
    .await;

    // The skill sits at the repository root (no `path`), and its version comes
    // from `metadata.version`.
    let (status, v) = f.publish(json!({"ref": "main"})).await;
    assert_eq!(status, StatusCode::CREATED, "{v}");
    assert_eq!(v["name"], "root-skill");
    assert_eq!(v["version"], "0.3.0");

    let (status, v) = f.publish(json!({"ref": "main", "version": "0.4.0"})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(v["error"]["code"], "SKILL_VERSION_MISMATCH");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_generic_package_route_does_not_publish_skills() {
    let f = Fixture::new("skill_generic_route").await;
    let resp = f
        .client
        .post(f.url("/packages/skill/publish?name=x&version=1.0.0"))
        .bearer_auth(&f.jwt)
        .body("anything")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let (_, list) = f.get("/skills").await;
    assert_eq!(list["skills"].as_array().unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reads_and_publishing_follow_repository_permissions() {
    let f = Fixture::new("skill_perms").await;
    f.seed_pdf().await;
    let (status, v) = f
        .publish(json!({"path": "skills/pdf", "ref": "main", "version": "1.0.0"}))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{v}");

    let stranger = register_user(&f.base, "skill_stranger", "skill_stranger@example.com", PW).await;
    let reads = [
        "/skills",
        "/skills/pdf-tools",
        "/skills/pdf-tools/resolve",
        "/skills/pdf-tools/versions/1.0.0",
        "/skills/pdf-tools/versions/1.0.0/content",
        "/skills/pdf-tools/versions/1.0.0/files/reference.md",
    ];
    for path in reads {
        let (status, body) = f.get_as(Some(&stranger), path).await;
        assert!(
            status == StatusCode::NOT_FOUND || status == StatusCode::FORBIDDEN,
            "a stranger must not read {path} of a private repository: {status} {body}"
        );
        let (status, _) = f.get_as(None, path).await;
        assert!(
            status == StatusCode::NOT_FOUND
                || status == StatusCode::FORBIDDEN
                || status == StatusCode::UNAUTHORIZED,
            "an anonymous caller must not read {path}: {status}"
        );
    }
    let archive = f
        .client
        .get(f.url("/skills/pdf-tools/versions/1.0.0/archive"))
        .bearer_auth(&stranger)
        .send()
        .await
        .unwrap();
    assert_ne!(archive.status(), StatusCode::OK);

    // Publishing needs write access; a stranger and an anonymous caller get none.
    let body = json!({"path": "skills/pdf", "ref": "main", "version": "9.9.9"});
    let (status, _) = f.post_publish(Some(&stranger), body.clone()).await;
    assert!(
        status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND,
        "{status}"
    );
    let (status, _) = f.post_publish(None, body).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (_, skill) = f.get("/skills/pdf-tools").await;
    assert_eq!(skill["versions"].as_array().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_branch_limited_token_may_read_but_not_publish() {
    let f = Fixture::new("skill_limited_token").await;
    f.seed_pdf().await;
    let (status, _) = f
        .publish(json!({"path": "skills/pdf", "ref": "main", "version": "1.0.0"}))
        .await;
    assert_eq!(status, StatusCode::CREATED);

    let resp = f
        .client
        .post(format!("{}/api/v1/users/tokens", f.base))
        .bearer_auth(&f.jwt)
        .json(&json!({
            "name": "agent", "scopes": "repo:write",
            "repos": [format!("{}/work", f.owner)], "refs": ["agent/*"],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let token = resp.json::<Value>().await.unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string();

    // Reads are not narrowed by a branch limit.
    let (status, v) = f.get_as(Some(&token), "/skills/pdf-tools/resolve").await;
    assert_eq!(status, StatusCode::OK, "{v}");

    // Publishing builds from a ref the token has no declared right to: refused.
    let (status, v) = f
        .post_publish(
            Some(&token),
            json!({"path": "skills/pdf", "ref": "main", "version": "2.0.0"}),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{v}");
    let (_, skill) = f.get("/skills/pdf-tools").await;
    assert_eq!(skill["versions"].as_array().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn publishing_is_audited_once() {
    use rg_db::sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    let (base, db) = common::spawn_test_app_on_env_db().await;
    let jwt = register_user(&base, "skill_audit", "skill_audit@example.com", PW).await;
    let client = Client::new();
    assert_eq!(
        client
            .post(format!("{base}/api/v1/repos"))
            .bearer_auth(&jwt)
            .json(&json!({"name": "work", "auto_init": true}))
            .send()
            .await
            .unwrap()
            .status(),
        201
    );
    let tip = {
        let remote = format!("{base}/git/skill_audit/work");
        rg_git::cli_gateway::global_gateway()
            .as_ref()
            .unwrap()
            .run(&["ls-remote", &remote, "refs/heads/main"], None)
            .unwrap()
            .stdout_str()
            .split_whitespace()
            .next()
            .unwrap()
            .to_string()
    };
    let resp = client
        .post(format!("{base}/api/v1/repos/skill_audit/work/commits"))
        .bearer_auth(&jwt)
        .json(&json!({
            "branch": "main", "expected_sha": tip, "message": "skill",
            "files": [{"operation": "create", "path": "SKILL.md", "content": PDF_SKILL}],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let publish = || async {
        client
            .post(format!(
                "{base}/api/v1/repos/skill_audit/work/skills/publish"
            ))
            .bearer_auth(&jwt)
            .json(&json!({"ref": "main", "version": "1.0.0"}))
            .send()
            .await
            .unwrap()
            .status()
    };
    assert_eq!(publish().await, StatusCode::CREATED);
    assert_eq!(publish().await, StatusCode::OK, "the replay");

    let rows = rg_db::entities::audit_log::Entity::find()
        .filter(rg_db::entities::audit_log::Column::Action.eq("skill.publish"))
        // Other tests publish into the same audit table when they share a database.
        .filter(rg_db::entities::audit_log::Column::Username.eq("skill_audit"))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1, "a replay is not a second publication");
    let details: Value = serde_json::from_str(rows[0].details.as_deref().unwrap()).unwrap();
    assert_eq!(details["version"], "1.0.0");
    assert!(details["commit"].as_str().unwrap().len() == 40);
    assert!(details["content_digest"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
}

/// A skill at the file-count limit, with long paths, must publish on every
/// backend. The version's metadata is stored in a `TEXT` column, which on MySQL
/// holds 64 KiB: a manifest with every file's path and digest stored there would
/// overflow it, so what is stored must stay small however large the skill is.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_skill_at_the_file_limit_with_long_paths_publishes() {
    let f = Fixture::new("skill_large").await;
    let dir = format!(
        "{}/{}/{}/{}",
        "a".repeat(60),
        "b".repeat(60),
        "c".repeat(60),
        "d".repeat(60)
    );
    let mut files: Vec<(String, String)> = vec![("skills/big/SKILL.md".into(), PDF_SKILL.into())];
    for i in 0..199 {
        files.push((format!("skills/big/{dir}/{i:03}.md"), format!("file {i}\n")));
    }
    let borrowed: Vec<(&str, &str)> = files
        .iter()
        .map(|(p, c)| (p.as_str(), c.as_str()))
        .collect();
    f.commit_files(&borrowed).await;

    let (status, v) = f
        .publish(json!({"path": "skills/big", "ref": "main", "version": "1.0.0"}))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{v}");

    // The manifest still lists every file, and the archive reads back whole.
    let (status, manifest) = f.get("/skills/pdf-tools/versions/1.0.0").await;
    assert_eq!(status, StatusCode::OK, "{manifest}");
    assert_eq!(manifest["files"].as_array().unwrap().len(), 200);
    assert_eq!(manifest["content_digest"], v["content_digest"]);
    let (status, md) = f.get("/skills/pdf-tools/versions/1.0.0/content").await;
    assert_eq!(status, StatusCode::OK, "{md}");

    // One more file than the limit is refused with a code, not truncated.
    let sha = f.blob_sha("skills/big/SKILL.md").await;
    let _ = sha;
    f.commit_files(&[("skills/big/one-too-many.md", "x\n")])
        .await;
    let (status, over) = f
        .publish(json!({"path": "skills/big", "ref": "main", "version": "1.1.0"}))
        .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{over}");
    assert_eq!(over["error"]["code"], "SKILL_TOO_MANY_FILES");
}

/// A frontmatter too large to store whole is refused, with a code.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_oversized_frontmatter_is_refused() {
    let f = Fixture::new("skill_big_frontmatter").await;
    let padding: String = (0..400)
        .map(|i| format!("key{i}: {}\n", "v".repeat(60)))
        .collect();
    let md = format!("---\nname: heavy\ndescription: Too much metadata.\n{padding}---\nbody\n");
    f.commit_files(&[("SKILL.md", &md)]).await;
    let (status, v) = f.publish(json!({"ref": "main", "version": "1.0.0"})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(v["error"]["code"], "SKILL_INVALID_FRONTMATTER");
}
