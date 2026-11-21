//! Cross-repository discovery of skills and MCP server manifests.
//!
//! What is pinned here is what a caller can rely on: a page is built from the
//! entries the caller may read (not cut from a longer list with holes), nothing
//! reveals how many entries the caller cannot see, repositories that are private
//! or deleted never leak, and a token limited to some repositories is refused.

mod common;
use common::{register_user, spawn_test_app_on_env_db};

use reqwest::{Client, StatusCode};
use serde_json::{json, Value};

const PW: &str = "Qz7$wRtm";

/// Every test names its entries `io.github.<tag>/…` (a skill `…-<tag>`) and asks
/// only for entries matching its tag, so the tests give the same answers when
/// they share one database (PostgreSQL, MySQL) as when each has its own.
struct World {
    base: String,
    client: Client,
    tag: String,
}

impl World {
    async fn new(tag: &str) -> World {
        let (base, _db) = spawn_test_app_on_env_db().await;
        World {
            base,
            client: Client::new(),
            tag: tag.to_string(),
        }
    }

    async fn user(&self, name: &str) -> String {
        register_user(&self.base, name, &format!("{name}@example.com"), PW).await
    }

    fn api(&self, path: &str) -> String {
        format!("{}/api/v1{path}", self.base)
    }

    async fn send(
        &self,
        method: reqwest::Method,
        path: &str,
        auth: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut request = self.client.request(method, self.api(path));
        if let Some(auth) = auth {
            request = request.bearer_auth(auth);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let resp = request.send().await.unwrap();
        let status = resp.status();
        (status, resp.json().await.unwrap_or(Value::Null))
    }

    async fn repo(&self, jwt: &str, name: &str, private: bool) {
        let (status, body) = self
            .send(
                reqwest::Method::POST,
                "/repos",
                Some(jwt),
                Some(json!({"name": name, "is_private": private, "auto_init": true})),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "create {name}: {body}");
    }

    async fn publish_mcp(&self, jwt: &str, repo: &str, manifest: Value) -> (StatusCode, Value) {
        self.send(
            reqwest::Method::POST,
            &format!("/repos/{repo}/mcp-servers"),
            Some(jwt),
            Some(json!({ "server": manifest })),
        )
        .await
    }

    /// A listing. Unless `query` has its own `q=`, the test's tag is the query.
    async fn discover(&self, kind: &str, auth: Option<&str>, query: &str) -> (StatusCode, Value) {
        let mut query = query.to_string();
        if !query.starts_with("q=") && !query.contains("&q=") {
            if !query.is_empty() {
                query.push('&');
            }
            query.push_str(&format!("q={}", self.tag));
        }
        let path = format!("/{kind}?{query}");
        self.send(reqwest::Method::GET, &path, auth, None).await
    }

    /// Every page of a listing, following `next_cursor`, as `(names, pages)`.
    async fn walk(&self, kind: &str, auth: Option<&str>, extra: &str) -> (Vec<String>, usize) {
        let mut names = Vec::new();
        let mut cursor: Option<String> = None;
        let mut pages = 0;
        loop {
            let mut query = format!("limit=2{extra}");
            if let Some(cursor) = &cursor {
                query.push_str(&format!("&cursor={cursor}"));
            }
            let (status, page) = self.discover(kind, auth, &query).await;
            assert_eq!(status, StatusCode::OK, "{page}");
            pages += 1;
            assert!(pages < 400, "the cursor must make progress");
            for entry in page["entries"].as_array().unwrap() {
                names.push(entry["name"].as_str().unwrap().to_string());
            }
            match page["next_cursor"].as_str() {
                Some(next) => cursor = Some(next.to_string()),
                None => return (names, pages),
            }
        }
    }
}

fn server(name: &str, version: &str, description: &str) -> Value {
    json!({
        "name": name,
        "description": description,
        "version": version,
        "remotes": [{"type": "streamable-http", "url": "https://mcp.example/x"}],
    })
}

fn names_of(page: &Value) -> Vec<String> {
    page["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_public_entry_is_found_with_what_a_lock_file_needs() {
    let w = World::new("tagpub").await;
    let alice = w.user("disc_alice").await;
    w.repo(&alice, "pub", false).await;
    let (status, out) = w
        .publish_mcp(
            &alice,
            "disc_alice/pub",
            server("io.github.tagpub/weather", "1.0.0", "Weather lookups."),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{out}");

    // Anonymous callers see public repositories.
    let (status, page) = w.discover("mcp-servers", None, "").await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let entries = page["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1, "{page}");
    let e = &entries[0];
    assert_eq!(e["owner"], "disc_alice");
    assert_eq!(e["repo"], "pub");
    assert_eq!(e["name"], "io.github.tagpub/weather");
    assert_eq!(e["description"], "Weather lookups.");
    assert_eq!(e["latest_version"], "1.0.0");
    assert_eq!(e["digest"], out["digest"]);
    assert_eq!(e["ownership"], "unverified");
    assert_eq!(e["origin"], "user_content");
    assert_eq!(e["publisher"]["username"], "disc_alice");
    assert_eq!(e["publisher"]["kind"], "human");
    assert!(page["next_cursor"].is_null());
    // A listing says nothing about totals.
    let mut keys: Vec<&String> = page.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(keys, ["entries", "next_cursor"]);

    // What it names can be fetched from the per-repository route, and resolve
    // agrees with the listing about the digest.
    let (status, resolved) = w
        .send(
            reqwest::Method::GET,
            "/repos/disc_alice/pub/mcp-servers/io.github.tagpub/weather/resolve",
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{resolved}");
    assert_eq!(resolved["digest"], e["digest"]);

    // Skills and MCP servers are separate listings.
    let (_, skills) = w.discover("skills", None, "").await;
    assert!(skills["entries"].as_array().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn private_repositories_are_seen_only_by_those_who_may_read_them() {
    let w = World::new("tagpriv").await;
    let alice = w.user("priv_alice").await;
    let bob = w.user("priv_bob").await;
    let carol = w.user("priv_carol").await;
    w.repo(&alice, "open", false).await;
    w.repo(&alice, "closed", true).await;
    for (repo, name) in [
        ("open", "io.github.tagpriv/open"),
        ("closed", "io.github.tagpriv/closed"),
    ] {
        let (status, out) = w
            .publish_mcp(
                &alice,
                &format!("priv_alice/{repo}"),
                server(name, "1.0.0", "x"),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{out}");
    }
    let (status, body) = w
        .send(
            reqwest::Method::POST,
            "/repos/priv_alice/closed/collaborators",
            Some(&alice),
            Some(json!({"username": "priv_bob", "permission": "read"})),
        )
        .await;
    assert!(status.is_success(), "{body}");

    let both = ["io.github.tagpriv/open", "io.github.tagpriv/closed"];
    let (_, page) = w.discover("mcp-servers", Some(&alice), "").await;
    assert_eq!(names_of(&page), both, "the owner sees both");
    let (_, page) = w.discover("mcp-servers", Some(&bob), "").await;
    assert_eq!(names_of(&page), both, "a collaborator sees both");
    let (_, page) = w.discover("mcp-servers", Some(&carol), "").await;
    assert_eq!(
        names_of(&page),
        ["io.github.tagpriv/open"],
        "a stranger sees the public one"
    );
    let (_, page) = w.discover("mcp-servers", None, "").await;
    assert_eq!(
        names_of(&page),
        ["io.github.tagpriv/open"],
        "so does an anonymous caller"
    );
}

/// The point of authorizing before paginating: hidden entries between visible
/// ones must not cost the caller a slot or a page.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hidden_entries_do_not_take_up_pages() {
    let w = World::new("tagpage").await;
    let alice = w.user("page_alice").await;
    w.repo(&alice, "a", false).await;
    w.repo(&alice, "hidden", true).await;
    w.repo(&alice, "b", false).await;
    for i in 0..2 {
        let name = format!("io.github.tagpage/a{i}");
        w.publish_mcp(&alice, "page_alice/a", server(&name, "1.0.0", "x"))
            .await;
    }
    for i in 0..5 {
        let name = format!("io.github.tagpage/h{i}");
        w.publish_mcp(&alice, "page_alice/hidden", server(&name, "1.0.0", "x"))
            .await;
    }
    for i in 0..2 {
        let name = format!("io.github.tagpage/b{i}");
        w.publish_mcp(&alice, "page_alice/b", server(&name, "1.0.0", "x"))
            .await;
    }

    // Two entries per page. The five hidden ones sit between the pages and must
    // not leave page 2 empty or short.
    let (_, first) = w.discover("mcp-servers", None, "limit=2").await;
    assert_eq!(
        names_of(&first),
        ["io.github.tagpage/a0", "io.github.tagpage/a1"]
    );
    let cursor = first["next_cursor"]
        .as_str()
        .expect("more may follow")
        .to_string();
    let (_, second) = w
        .discover("mcp-servers", None, &format!("limit=2&cursor={cursor}"))
        .await;
    assert_eq!(
        names_of(&second),
        ["io.github.tagpage/b0", "io.github.tagpage/b1"],
        "{second}"
    );
    let text = serde_json::to_string(&second).unwrap();
    assert!(
        !text.contains("io.github.tagpage/h"),
        "a hidden entry must not appear: {text}"
    );

    // Walking to the end yields each visible entry once, and nothing else.
    let (names, _) = w.walk("mcp-servers", None, "").await;
    assert_eq!(
        names,
        [
            "io.github.tagpage/a0",
            "io.github.tagpage/a1",
            "io.github.tagpage/b0",
            "io.github.tagpage/b1"
        ]
    );
    // The owner sees all nine, in the same order.
    let (names, _) = w.walk("mcp-servers", Some(&alice), "").await;
    assert_eq!(names.len(), 9);
}

/// One request reads only so many candidates. When that runs out before the page
/// is full, the page is short but still carries a cursor, and following it
/// finishes the job.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_short_page_keeps_a_cursor_so_the_walk_can_continue() {
    let w = World::new("tagscan").await;
    let alice = w.user("scan_alice").await;
    w.repo(&alice, "hidden", true).await;
    w.repo(&alice, "open", false).await;
    let hidden_count = rg_core::package_registry::discovery::MAX_SCAN + 5;
    for i in 0..hidden_count {
        let name = format!("io.github.tagscan/h{i}");
        let (status, out) = w
            .publish_mcp(&alice, "scan_alice/hidden", server(&name, "1.0.0", "x"))
            .await;
        assert_eq!(status, StatusCode::CREATED, "{out}");
    }
    w.publish_mcp(
        &alice,
        "scan_alice/open",
        server("io.github.tagscan/last", "1.0.0", "x"),
    )
    .await;

    let (_, first) = w.discover("mcp-servers", None, "limit=1").await;
    assert!(first["entries"].as_array().unwrap().is_empty(), "{first}");
    let cursor = first["next_cursor"]
        .as_str()
        .expect("the scan limit was hit")
        .to_string();
    let (_, second) = w
        .discover("mcp-servers", None, &format!("limit=1&cursor={cursor}"))
        .await;
    assert_eq!(names_of(&second), ["io.github.tagscan/last"], "{second}");
    assert!(second["next_cursor"].is_null(), "nothing is left: {second}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_deleted_repository_is_not_listed() {
    let w = World::new("tagdel").await;
    let alice = w.user("del_alice").await;
    w.repo(&alice, "gone", false).await;
    w.repo(&alice, "kept", false).await;
    w.publish_mcp(
        &alice,
        "del_alice/gone",
        server("io.github.tagdel/gone", "1.0.0", "x"),
    )
    .await;
    w.publish_mcp(
        &alice,
        "del_alice/kept",
        server("io.github.tagdel/kept", "1.0.0", "x"),
    )
    .await;
    let (status, body) = w
        .send(
            reqwest::Method::DELETE,
            "/repos/del_alice/gone",
            Some(&alice),
            None,
        )
        .await;
    assert!(status.is_success(), "{status} {body}");

    let (_, page) = w.discover("mcp-servers", None, "").await;
    assert_eq!(names_of(&page), ["io.github.tagdel/kept"], "{page}");
    let (_, page) = w.discover("mcp-servers", Some(&alice), "").await;
    assert_eq!(
        names_of(&page),
        ["io.github.tagdel/kept"],
        "not even for its owner: {page}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_query_is_a_case_insensitive_substring_with_no_wildcards() {
    let w = World::new("tagq").await;
    let alice = w.user("q_alice").await;
    w.repo(&alice, "r", false).await;
    for (name, description) in [
        ("io.github.tagq/pdf-reader", "Reads PDF files"),
        ("io.github.tagq/discount", "Gives 100% off"),
        ("io.github.tagq/under_score", "plain"),
        ("io.github.tagq/other", "nothing to see"),
    ] {
        let (status, out) = w
            .publish_mcp(&alice, "q_alice/r", server(name, "1.0.0", description))
            .await;
        assert_eq!(status, StatusCode::CREATED, "{out}");
    }
    let found = |q: &'static str| {
        let w = &w;
        async move {
            let (status, page) = w
                .discover("mcp-servers", None, &format!("q={}", q.replace('%', "%25")))
                .await;
            assert_eq!(status, StatusCode::OK, "{q}: {page}");
            // Other tests may share the database: look only at this test's entries.
            names_of(&page)
                .into_iter()
                .filter(|n| n.starts_with("io.github.tagq/"))
                .collect::<Vec<_>>()
        }
    };
    assert_eq!(found("PDF").await, ["io.github.tagq/pdf-reader"]);
    assert_eq!(found("reads pdf").await, ["io.github.tagq/pdf-reader"]);
    assert_eq!(found("100%").await, ["io.github.tagq/discount"]);
    // `%` and `_` are ordinary characters, not wildcards.
    assert_eq!(found("%").await, ["io.github.tagq/discount"]);
    assert_eq!(found("_").await, ["io.github.tagq/under_score"]);
    assert!(found("zzz").await.is_empty());
    // A query that matches the description of an entry in a name search.
    assert_eq!(found("to see").await, ["io.github.tagq/other"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bad_parameters_are_refused_with_a_code() {
    let w = World::new("tagbad").await;
    for (query, code) in [
        ("limit=0", "INVALID_LIMIT"),
        ("limit=101", "INVALID_LIMIT"),
        ("cursor=abc", "INVALID_CURSOR"),
        ("cursor=-1", "INVALID_CURSOR"),
    ] {
        let (status, body) = w.discover("skills", None, query).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {body}");
        assert_eq!(body["error"]["code"], code, "{query}: {body}");
    }
    let long = "q".repeat(201);
    let (status, body) = w.discover("mcp-servers", None, &format!("q={long}")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["code"], "INVALID_QUERY");
    let (status, _) = w.discover("mcp-servers", None, "limit=100").await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_latest_non_yanked_version_is_what_is_listed() {
    let w = World::new("tagver").await;
    let alice = w.user("ver_alice").await;
    w.repo(&alice, "r", false).await;
    let name = "io.github.tagver/tool";
    w.publish_mcp(&alice, "ver_alice/r", server(name, "1.0.0", "First words."))
        .await;
    let (_, second) = w
        .publish_mcp(&alice, "ver_alice/r", server(name, "1.1.0", "Later words."))
        .await;

    let (_, page) = w.discover("mcp-servers", None, "").await;
    let e = &page["entries"][0];
    assert_eq!(e["latest_version"], "1.1.0");
    assert_eq!(e["digest"], second["digest"]);
    // The description is the latest version's, not the first one's.
    assert_eq!(e["description"], "Later words.");
    let (_, listed) = w
        .send(
            reqwest::Method::GET,
            "/repos/ver_alice/r/mcp-servers",
            None,
            None,
        )
        .await;
    assert_eq!(listed["servers"][0]["description"], "Later words.");

    let yank = |version: &'static str, yanked: bool| {
        let (w, alice) = (&w, &alice);
        async move {
            let (status, body) = w
                .send(
                    reqwest::Method::PATCH,
                    &format!(
                        "/repos/ver_alice/r/packages/mcp-server/io.github.tagver%2Ftool/{version}/yank"
                    ),
                    Some(alice),
                    Some(json!({"yank": yanked})),
                )
                .await;
            assert!(status.is_success(), "{status} {body}");
        }
    };
    yank("1.1.0", true).await;
    let (_, page) = w.discover("mcp-servers", None, "").await;
    assert_eq!(page["entries"][0]["latest_version"], "1.0.0");
    yank("1.0.0", true).await;
    let (_, page) = w.discover("mcp-servers", None, "").await;
    assert!(
        page["entries"].as_array().unwrap().is_empty(),
        "nothing to resolve, nothing listed: {page}"
    );
    yank("1.0.0", false).await;
    let (_, page) = w.discover("mcp-servers", None, "").await;
    assert_eq!(page["entries"][0]["latest_version"], "1.0.0");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_agent_publisher_is_labelled_as_one() {
    let w = World::new("tagagent").await;
    let alice = w.user("agent_alice").await;
    w.repo(&alice, "r", false).await;
    let (status, agent) = w
        .send(
            reqwest::Method::POST,
            "/agents",
            Some(&alice),
            Some(json!({"name": "bot"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{agent}");
    let agent_name = agent["username"].as_str().unwrap().to_string();
    let (status, token) = w
        .send(
            reqwest::Method::POST,
            &format!("/agents/{agent_name}/tokens"),
            Some(&alice),
            Some(json!({"name": "t"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{token}");
    let token = token["token"].as_str().unwrap().to_string();
    let (status, body) = w
        .send(
            reqwest::Method::POST,
            "/repos/agent_alice/r/collaborators",
            Some(&alice),
            Some(json!({"username": agent_name, "permission": "write"})),
        )
        .await;
    assert!(status.is_success(), "{body}");

    let (status, out) = w
        .publish_mcp(
            &token,
            "agent_alice/r",
            server("io.github.tagagent/bots", "1.0.0", "By a bot."),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{out}");

    let (_, page) = w.discover("mcp-servers", None, "").await;
    let publisher = &page["entries"][0]["publisher"];
    assert_eq!(publisher["kind"], "agent", "{page}");
    assert_eq!(publisher["username"], agent_name);
    assert_eq!(publisher["owner_username"], "agent_alice");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_token_limited_to_some_repositories_cannot_use_the_global_listing() {
    let w = World::new("tagtok").await;
    let alice = w.user("tok_alice").await;
    w.repo(&alice, "r", false).await;
    w.publish_mcp(
        &alice,
        "tok_alice/r",
        server("io.github.tagtok/x", "1.0.0", "x"),
    )
    .await;
    let (status, token) = w
        .send(
            reqwest::Method::POST,
            "/users/tokens",
            Some(&alice),
            Some(json!({"name": "agent", "scopes": "repo:read", "repos": ["tok_alice/r"]})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{token}");
    let token = token["token"].as_str().unwrap().to_string();

    for kind in ["mcp-servers", "skills"] {
        let (status, body) = w.discover(kind, Some(&token), "").await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{kind}: {body}");
    }
    // Its own repository still works.
    let (status, body) = w
        .send(
            reqwest::Method::GET,
            "/repos/tok_alice/r/mcp-servers",
            Some(&token),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // An unrestricted read token may use the global listing.
    let (status, token) = w
        .send(
            reqwest::Method::POST,
            "/users/tokens",
            Some(&alice),
            Some(json!({"name": "wide", "scopes": "repo:read"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{token}");
    let (status, page) = w
        .discover("mcp-servers", Some(token["token"].as_str().unwrap()), "")
        .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(names_of(&page), ["io.github.tagtok/x"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_skill_is_listed_with_its_digests_and_where_it_came_from() {
    let w = World::new("tagskill").await;
    let alice = w.user("skill_disc_alice").await;
    w.repo(&alice, "skills", false).await;
    let (_, log) = w
        .send(
            reqwest::Method::GET,
            "/repos/skill_disc_alice/skills/log?ref=main&limit=1",
            Some(&alice),
            None,
        )
        .await;
    let tip = log["commits"][0]["sha"].as_str().unwrap().to_string();
    let (status, body) = w
        .send(
            reqwest::Method::POST,
            "/repos/skill_disc_alice/skills/commits",
            Some(&alice),
            Some(json!({
                "branch": "main", "expected_sha": tip, "message": "skill",
                "files": [{
                    "operation": "create", "path": "SKILL.md",
                    "content": "---\nname: pdf-tagskill\ndescription: Read and fill PDF forms.\n---\n# PDF\n"
                }],
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, out) = w
        .send(
            reqwest::Method::POST,
            "/repos/skill_disc_alice/skills/skills/publish",
            Some(&alice),
            Some(json!({"ref": "main", "version": "1.2.0"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{out}");

    let (status, page) = w.discover("skills", None, "").await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let e = &page["entries"][0];
    assert_eq!(e["owner"], "skill_disc_alice");
    assert_eq!(e["repo"], "skills");
    assert_eq!(e["name"], "pdf-tagskill");
    assert_eq!(e["description"], "Read and fill PDF forms.");
    assert_eq!(e["latest_version"], "1.2.0");
    assert_eq!(e["content_digest"], out["content_digest"]);
    assert_eq!(e["archive_digest"], out["archive_digest"]);
    assert_eq!(e["source_verified"], true);
    assert_eq!(e["origin"], "user_content");
    assert!(e.get("digest").is_none() && e.get("ownership").is_none());
    assert_eq!(e["publisher"]["kind"], "human");

    // The listing's digests are the ones `resolve` hands out for the same version.
    let (_, resolved) = w
        .send(
            reqwest::Method::GET,
            "/repos/skill_disc_alice/skills/skills/pdf-tagskill/resolve",
            None,
            None,
        )
        .await;
    assert_eq!(resolved["content_digest"], e["content_digest"]);
    assert_eq!(resolved["archive_digest"], e["archive_digest"]);
    let (_, mcp) = w.discover("mcp-servers", None, "").await;
    assert!(mcp["entries"].as_array().unwrap().is_empty());
}
