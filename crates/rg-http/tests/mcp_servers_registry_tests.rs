//! The MCP server manifest registry: publish a `server.json`, resolve it, read
//! it back.
//!
//! Pins what an agent relies on when it pins a manifest by digest: a version
//! never changes, a constraint resolves to one exact non-yanked version, what a
//! publisher wrote is labelled `user_content`, nothing is ever called verified,
//! and reads and the write follow the repository's own permissions.

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

    async fn publish_as(&self, token: Option<&str>, server: Value) -> (StatusCode, Value) {
        let mut request = self
            .client
            .post(self.url("/mcp-servers"))
            .json(&json!({ "server": server }));
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        let resp = request.send().await.unwrap();
        let status = resp.status();
        (status, resp.json().await.unwrap_or(Value::Null))
    }

    async fn publish(&self, server: Value) -> (StatusCode, Value) {
        self.publish_as(Some(&self.jwt.clone()), server).await
    }

    async fn get_as(&self, token: Option<&str>, suffix: &str) -> (StatusCode, Value) {
        let mut request = self.client.get(self.url(suffix));
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        let resp = request.send().await.unwrap();
        let status = resp.status();
        (status, resp.json().await.unwrap_or(Value::Null))
    }

    async fn get(&self, suffix: &str) -> (StatusCode, Value) {
        self.get_as(Some(&self.jwt.clone()), suffix).await
    }

    async fn yank(&self, name: &str, version: &str, yank: bool) {
        let resp = self
            .client
            .patch(self.url(&format!(
                "/packages/mcp-server/{}/{version}/yank",
                name.replace('/', "%2F")
            )))
            .bearer_auth(&self.jwt)
            .json(&json!({ "yank": yank }))
            .send()
            .await
            .unwrap();
        assert!(resp.status().is_success(), "yank: {}", resp.status());
    }
}

fn weather(version: &str) -> Value {
    json!({
        "$schema": "https://static.modelcontextprotocol.io/schemas/2025-12-11/server.schema.json",
        "name": "io.github.acme/weather",
        "description": "Weather lookups.",
        "version": version,
        "packages": [{
            "registryType": "npm",
            "identifier": "@acme/weather-mcp",
            "version": version,
            "transport": {"type": "stdio"},
            "environmentVariables": [{"name": "API_KEY", "isSecret": true}],
        }],
        "remotes": [{"type": "streamable-http", "url": "https://mcp.acme.example/weather"}],
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_manifest_is_published_and_read_back() {
    let f = Fixture::new("mcp_roundtrip").await;
    let (status, out) = f.publish(weather("1.0.0")).await;
    assert_eq!(status, StatusCode::CREATED, "{out}");
    assert_eq!(out["name"], "io.github.acme/weather");
    assert_eq!(out["version"], "1.0.0");
    assert_eq!(out["existing"], false);
    assert_eq!(out["ownership"], "unverified");
    let digest = out["digest"].as_str().unwrap();
    assert!(digest.starts_with("sha256:") && digest.len() == 7 + 64);

    let (status, list) = f.get("/mcp-servers").await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let servers = list["servers"].as_array().unwrap();
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0]["name"], "io.github.acme/weather");
    assert_eq!(servers[0]["description"], "Weather lookups.");
    assert_eq!(servers[0]["latest_version"], "1.0.0");
    assert_eq!(servers[0]["ownership"], "unverified");
    assert_eq!(servers[0]["origin"], "user_content");

    let (status, server) = f.get("/mcp-servers/io.github.acme/weather").await;
    assert_eq!(status, StatusCode::OK, "{server}");
    assert_eq!(server["latest_version"], "1.0.0");
    assert_eq!(server["versions"][0]["digest"], digest);
    assert_eq!(server["versions"][0]["yanked"], false);

    let (status, v) = f
        .get("/mcp-servers/io.github.acme/weather/versions/1.0.0")
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["digest"], digest);
    assert_eq!(v["ownership"], "unverified");
    assert_eq!(v["origin"], "user_content");
    assert_eq!(v["manifest"]["name"], "io.github.acme/weather");
    assert_eq!(
        v["manifest"]["remotes"][0]["url"],
        "https://mcp.acme.example/weather"
    );
    assert_eq!(v["endpoints"]["packages"][0]["registry_type"], "npm");
    assert_eq!(v["endpoints"]["packages"][0]["known_registry_type"], true);
    assert_eq!(v["endpoints"]["remotes"][0]["transport"], "streamable-http");
    assert_eq!(v["endpoints"]["remotes"][0]["insecure_local"], false);
    assert!(v["declared_schema"]
        .as_str()
        .unwrap()
        .contains("2025-12-11"));

    // The digest is of the stored bytes: recomputing it from the manifest in
    // its canonical form (sorted keys, no spacing) gives the same value.
    use sha2::{Digest, Sha256};
    let canonical = canonical(&v["manifest"]);
    assert_eq!(
        format!(
            "sha256:{}",
            hex::encode(Sha256::digest(canonical.as_bytes()))
        ),
        digest
    );
    assert_eq!(v["size"].as_u64().unwrap() as usize, canonical.len());

    let (status, v) = f
        .get("/mcp-servers/io.github.acme/weather/versions/9.9.9")
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(v["error"]["code"], "MCP_VERSION_NOT_FOUND");
    let (status, v) = f.get("/mcp-servers/io.github.acme/nothing").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(v["error"]["code"], "MCP_SERVER_NOT_FOUND");
}

/// Sorted-key compact JSON, written independently of the server's code.
fn canonical(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .into_iter()
                .map(|k| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(k).unwrap(),
                        canonical(&map[k])
                    )
                })
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(canonical).collect();
            format!("[{}]", parts.join(","))
        }
        other => serde_json::to_string(other).unwrap(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_published_version_is_idempotent_and_never_changes() {
    let f = Fixture::new("mcp_immutable").await;
    let (status, first) = f.publish(weather("1.0.0")).await;
    assert_eq!(status, StatusCode::CREATED, "{first}");

    // The same manifest with its keys written in another order: same digest, a
    // no-op.
    let reordered: Value = serde_json::from_str(
        r#"{"version":"1.0.0","remotes":[{"url":"https://mcp.acme.example/weather","type":"streamable-http"}],
            "packages":[{"environmentVariables":[{"isSecret":true,"name":"API_KEY"}],
              "transport":{"type":"stdio"},"version":"1.0.0","identifier":"@acme/weather-mcp","registryType":"npm"}],
            "description":"Weather lookups.","name":"io.github.acme/weather",
            "$schema":"https://static.modelcontextprotocol.io/schemas/2025-12-11/server.schema.json"}"#,
    )
    .unwrap();
    let (status, again) = f.publish(reordered).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["existing"], true);
    assert_eq!(again["digest"], first["digest"]);
    assert_eq!(again["version_id"], first["version_id"]);

    // Different content under the same version is refused, with both digests.
    let mut changed = weather("1.0.0");
    changed["remotes"][0]["url"] = json!("https://evil.example/weather");
    let (status, conflict) = f.publish(changed).await;
    assert_eq!(status, StatusCode::CONFLICT, "{conflict}");
    assert_eq!(conflict["error"]["code"], "VERSION_IMMUTABLE");
    assert_eq!(conflict["error"]["reason"], "VERSION_IMMUTABLE");
    assert_eq!(
        conflict["error"]["details"]["existing_digest"],
        first["digest"]
    );
    assert!(conflict["error"]["details"]["new_digest"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));

    // The stored version is still the first one.
    let (_, v) = f
        .get("/mcp-servers/io.github.acme/weather/versions/1.0.0")
        .await;
    assert_eq!(v["digest"], first["digest"]);
    assert_eq!(
        v["manifest"]["remotes"][0]["url"],
        "https://mcp.acme.example/weather"
    );

    // A new version is a new entry.
    let (status, second) = f.publish(weather("1.1.0")).await;
    assert_eq!(status, StatusCode::CREATED, "{second}");
    assert_ne!(second["digest"], first["digest"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_constraint_resolves_to_one_exact_non_yanked_version() {
    let f = Fixture::new("mcp_resolve").await;
    let mut digests = std::collections::HashMap::new();
    for v in ["1.0.0", "1.1.0", "2.0.0-rc.1"] {
        let (status, out) = f.publish(weather(v)).await;
        assert_eq!(status, StatusCode::CREATED, "{out}");
        digests.insert(v, out["digest"].clone());
    }
    let resolve = |constraint: &'static str| {
        let f = &f;
        async move {
            f.get(&format!(
                "/mcp-servers/io.github.acme/weather/resolve?constraint={}",
                constraint.replace(' ', "%20").replace('^', "%5E")
            ))
            .await
        }
    };

    let (status, r) = resolve("^1").await;
    assert_eq!(status, StatusCode::OK, "{r}");
    assert_eq!(r["version"], "1.1.0");
    assert_eq!(r["digest"], digests["1.1.0"]);
    let (_, r) = resolve("=1.0.0").await;
    assert_eq!(r["version"], "1.0.0");

    // `latest` and no constraint: the highest stable version, not the rc.
    let (_, r) = resolve("latest").await;
    assert_eq!(r["version"], "1.1.0");
    let (_, r) = f.get("/mcp-servers/io.github.acme/weather/resolve").await;
    assert_eq!(r["version"], "1.1.0");
    // A pre-release is chosen only when asked for by name.
    let (_, r) = resolve(">=2.0.0-rc.1").await;
    assert_eq!(r["version"], "2.0.0-rc.1");

    // Yanking goes through the generic package route, addressed by the
    // percent-encoded name; the yanked version is no longer chosen.
    f.yank("io.github.acme/weather", "1.1.0", true).await;
    let (_, r) = resolve("^1").await;
    assert_eq!(r["version"], "1.0.0", "a yanked version is skipped");
    let (_, server) = f.get("/mcp-servers/io.github.acme/weather").await;
    assert_eq!(server["latest_version"], "1.0.0");
    let yanked = server["versions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["version"] == "1.1.0")
        .unwrap();
    assert_eq!(yanked["yanked"], true);
    // A yanked version can still be read by its exact version.
    let (status, v) = f
        .get("/mcp-servers/io.github.acme/weather/versions/1.1.0")
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["yanked"], true);

    f.yank("io.github.acme/weather", "1.1.0", false).await;
    let (_, r) = resolve("^1").await;
    assert_eq!(r["version"], "1.1.0");

    // Nothing matches: 404 with what could have been asked for.
    let (status, r) = resolve("^7").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{r}");
    assert_eq!(r["error"]["code"], "NO_MATCHING_VERSION");
    let available = r["error"]["details"]["available"].as_array().unwrap();
    assert!(available.iter().any(|v| v == "1.0.0"));
    let (status, r) = resolve("not a constraint").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{r}");
    assert_eq!(r["error"]["code"], "MCP_CONSTRAINT_INVALID");
    let (status, r) = f.get("/mcp-servers/io.github.acme/other/resolve").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{r}");
    assert_eq!(r["error"]["code"], "MCP_SERVER_NOT_FOUND");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn what_is_not_a_valid_manifest_is_refused_with_a_code() {
    let f = Fixture::new("mcp_refusals").await;
    let case = |edit: &dyn Fn(&mut Value)| {
        let mut m = weather("1.0.0");
        edit(&mut m);
        m
    };
    let refused: Vec<(&str, Value, &str)> = vec![
        (
            "bad name",
            case(&|m| m["name"] = json!("weather")),
            "MCP_NAME_INVALID",
        ),
        (
            "dot name",
            case(&|m| m["name"] = json!("../x")),
            "MCP_NAME_INVALID",
        ),
        (
            "long description",
            case(&|m| m["description"] = json!("d".repeat(101))),
            "MCP_DESCRIPTION_INVALID",
        ),
        (
            "range version",
            case(&|m| m["version"] = json!("^1.0.0")),
            "MCP_VERSION_INVALID",
        ),
        (
            "short version",
            case(&|m| m["version"] = json!("1.0")),
            "MCP_VERSION_INVALID",
        ),
        (
            "nothing to run",
            case(&|m| {
                m.as_object_mut().unwrap().remove("packages");
                m.as_object_mut().unwrap().remove("remotes");
            }),
            "MCP_NO_ENDPOINT",
        ),
        (
            "plain http remote",
            case(&|m| m["remotes"][0]["url"] = json!("http://mcp.acme.example/weather")),
            "MCP_REMOTE_INVALID",
        ),
        (
            "credentials in a remote",
            case(&|m| m["remotes"][0]["url"] = json!("https://u:p@mcp.acme.example/w")),
            "MCP_REMOTE_INVALID",
        ),
        (
            "package without transport",
            case(&|m| {
                m["packages"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("transport");
            }),
            "MCP_PACKAGE_INVALID",
        ),
        (
            "latest package version",
            case(&|m| m["packages"][0]["version"] = json!("latest")),
            "MCP_PACKAGE_INVALID",
        ),
        ("not an object", json!(["a"]), "MCP_MANIFEST_INVALID"),
    ];
    for (label, manifest, code) in refused {
        let (status, body) = f.publish(manifest).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{label}: {body}");
        assert_eq!(body["error"]["code"], code, "{label}: {body}");
    }

    // An oversized manifest is 413, not a silent truncation.
    let big = case(&|m| m["x-big"] = json!("x".repeat(130 * 1024)));
    let (status, body) = f.publish(big).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{body}");
    assert_eq!(body["error"]["code"], "MCP_MANIFEST_TOO_LARGE");

    // A body that is not `{ "server": … }` is refused before any of that.
    let resp = f
        .client
        .post(f.url("/mcp-servers"))
        .bearer_auth(&f.jwt)
        .json(&json!({"server": weather("1.0.0"), "extra": 1}))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_client_error());

    let (_, list) = f.get("/mcp-servers").await;
    assert_eq!(
        list["servers"].as_array().unwrap().len(),
        0,
        "nothing was stored"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_http_remotes_are_allowed_and_marked_and_unknown_things_are_kept() {
    let f = Fixture::new("mcp_local").await;
    let mut m = weather("1.0.0");
    m["remotes"] = json!([
        {"type": "sse", "url": "http://localhost:8080/sse"},
        {"type": "streamable-http", "url": "https://mcp.acme.example/weather"},
    ]);
    m["packages"][0]["registryType"] = json!("brand-new-registry");
    m["x-vendor"] = json!({"note": "kept"});
    let (status, out) = f.publish(m).await;
    assert_eq!(status, StatusCode::CREATED, "{out}");

    let (_, v) = f
        .get("/mcp-servers/io.github.acme/weather/versions/1.0.0")
        .await;
    assert_eq!(v["endpoints"]["remotes"][0]["insecure_local"], true);
    assert_eq!(v["endpoints"]["remotes"][1]["insecure_local"], false);
    assert_eq!(v["endpoints"]["packages"][0]["known_registry_type"], false);
    assert_eq!(
        v["endpoints"]["packages"][0]["registry_type"],
        "brand-new-registry"
    );
    assert_eq!(v["manifest"]["x-vendor"]["note"], "kept");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_generic_package_route_does_not_publish_manifests() {
    let f = Fixture::new("mcp_generic_route").await;
    let resp = f
        .client
        .post(f.url("/packages/mcp-server/publish?name=x%2Fy&version=1.0.0"))
        .bearer_auth(&f.jwt)
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let (_, list) = f.get("/mcp-servers").await;
    assert_eq!(list["servers"].as_array().unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reads_and_publishing_follow_repository_permissions() {
    let f = Fixture::new("mcp_perms").await;
    let (status, out) = f.publish(weather("1.0.0")).await;
    assert_eq!(status, StatusCode::CREATED, "{out}");

    let stranger = register_user(&f.base, "mcp_stranger", "mcp_stranger@example.com", PW).await;
    let reads = [
        "/mcp-servers",
        "/mcp-servers/io.github.acme/weather",
        "/mcp-servers/io.github.acme/weather/resolve",
        "/mcp-servers/io.github.acme/weather/versions/1.0.0",
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

    let (status, _) = f.publish_as(Some(&stranger), weather("9.9.9")).await;
    assert!(
        status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND,
        "{status}"
    );
    let (status, _) = f.publish_as(None, weather("9.9.9")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (_, server) = f.get("/mcp-servers/io.github.acme/weather").await;
    assert_eq!(server["versions"].as_array().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_branch_limited_token_may_read_but_not_publish() {
    let f = Fixture::new("mcp_limited_token").await;
    let (status, _) = f.publish(weather("1.0.0")).await;
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

    let (status, v) = f
        .get_as(Some(&token), "/mcp-servers/io.github.acme/weather/resolve")
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");

    let (status, v) = f.publish_as(Some(&token), weather("2.0.0")).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{v}");
    let (_, server) = f.get("/mcp-servers/io.github.acme/weather").await;
    assert_eq!(server["versions"].as_array().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn publishing_is_audited_once() {
    use rg_db::sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    let (base, db) = spawn_test_app_on_env_db().await;
    let jwt = register_user(&base, "mcp_audit", "mcp_audit@example.com", PW).await;
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
    let publish = || async {
        client
            .post(format!("{base}/api/v1/repos/mcp_audit/work/mcp-servers"))
            .bearer_auth(&jwt)
            .json(&json!({ "server": weather("1.0.0") }))
            .send()
            .await
            .unwrap()
            .status()
    };
    assert_eq!(publish().await, StatusCode::CREATED);
    assert_eq!(publish().await, StatusCode::OK, "the replay");

    let rows = rg_db::entities::audit_log::Entity::find()
        .filter(rg_db::entities::audit_log::Column::Action.eq("mcp_server.publish"))
        // Other tests publish into the same audit table when they share a database.
        .filter(rg_db::entities::audit_log::Column::Username.eq("mcp_audit"))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1, "a replay is not a second publication");
    let details: Value = serde_json::from_str(rows[0].details.as_deref().unwrap()).unwrap();
    assert_eq!(details["version"], "1.0.0");
    assert!(details["digest"].as_str().unwrap().starts_with("sha256:"));
}

/// A manifest close to the 128 KiB limit must publish on every backend: the
/// version's metadata is a `TEXT` column (64 KiB on MySQL), so nothing that
/// scales with the manifest may be stored there.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_manifest_near_the_size_limit_publishes() {
    let f = Fixture::new("mcp_large").await;
    let mut m = weather("1.0.0");
    let one = m["packages"][0].clone();
    let packages: Vec<Value> = (0..50)
        .map(|i| {
            let mut p = one.clone();
            p["identifier"] = json!(format!("@acme/{}{i}", "p".repeat(1000)));
            p
        })
        .collect();
    m["packages"] = Value::Array(packages);
    m["x-big"] = json!("x".repeat(60_000));
    let (status, out) = f.publish(m).await;
    assert_eq!(status, StatusCode::CREATED, "{out}");

    let (status, v) = f
        .get("/mcp-servers/io.github.acme/weather/versions/1.0.0")
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["manifest"]["packages"].as_array().unwrap().len(), 50);
    assert_eq!(v["digest"], out["digest"]);
    assert!(
        v["size"].as_u64().unwrap() > 100 * 1024,
        "the test must really be near the limit: {}",
        v["size"]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skills_and_manifests_do_not_see_each_other() {
    let f = Fixture::new("mcp_vs_skill").await;
    f.publish(weather("1.0.0")).await;
    let (status, skills) = f.get("/skills").await;
    assert_eq!(status, StatusCode::OK, "{skills}");
    assert_eq!(skills["skills"].as_array().unwrap().len(), 0);
    let (_, servers) = f.get("/mcp-servers").await;
    assert_eq!(servers["servers"].as_array().unwrap().len(), 1);
}
