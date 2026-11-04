//! X01 — capability discovery and contract compatibility (design R14,
//! acceptance M10–M11).
//!
//! Two kinds of tests:
//!
//! * **Drift guards** (no server): the collaboration routes registered in
//!   `lib.rs`, the OpenAPI document, the capability registry and the MCP tool
//!   table must describe the same surface. Failures say what to add where.
//! * **Behaviour over real HTTP** (SQLite; the other databases are not
//!   available locally): discovery, forward compatibility and rejection of
//!   unknown write fields.

mod collab_common;
mod common;

use std::collections::{BTreeMap, BTreeSet};

use collab_common::{Env, Sess};
use reqwest::Method;
use rg_core::collaboration::capabilities as registry;
use rg_mcp::collab::TOOL_NAMES;
use rg_mcp::collab_contract::{rest_path, TOOL_ROUTES};
use serde_json::{json, Value};

const UUID: &str = "11111111-1111-4111-8111-111111111111";

// ───────────────────────────── drift guards ─────────────────────────────

/// `{x}` → `{}` so differently named path parameters compare equal.
fn normalize(path: &str) -> String {
    let mut out = String::new();
    let mut in_param = false;
    for c in path.chars() {
        match c {
            '{' => {
                in_param = true;
                out.push_str("{}");
            }
            '}' => in_param = false,
            c if !in_param => out.push(c),
            _ => {}
        }
    }
    out
}

/// Every `/…collaboration…` route of `lib.rs` with its HTTP methods.
fn registered_collab_routes() -> BTreeMap<String, BTreeSet<String>> {
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs")).unwrap();
    let mut routes: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut rest = src.as_str();
    while let Some(at) = rest.find(".route(") {
        let after = &rest[at + ".route(".len()..];
        // Segment up to the matching close paren.
        let mut depth = 1usize;
        let mut end = after.len();
        for (i, c) in after.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i;
                        break;
                    }
                }
                _ => {}
            }
        }
        let segment = &after[..end];
        rest = &after[end..];
        let Some(first_quote) = segment.find('"') else {
            continue;
        };
        let Some(len) = segment[first_quote + 1..].find('"') else {
            continue;
        };
        let path = &segment[first_quote + 1..first_quote + 1 + len];
        if !path.contains("collaboration") {
            continue;
        }
        let handlers = &segment[first_quote + 1 + len + 1..];
        let entry = routes.entry(path.to_string()).or_default();
        for method in ["get", "post", "put", "patch", "delete"] {
            let needle = format!("{method}(");
            let mut from = 0;
            while let Some(pos) = handlers[from..].find(&needle) {
                let abs = from + pos;
                let boundary = handlers[..abs]
                    .chars()
                    .next_back()
                    .is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
                if boundary {
                    entry.insert(method.to_uppercase());
                }
                from = abs + needle.len();
            }
        }
    }
    routes
}

fn registry_operations() -> BTreeSet<(String, String)> {
    registry::all()
        .iter()
        .flat_map(|c| c.routes.iter())
        .map(|r| (r.method.to_string(), r.path.to_string()))
        .collect()
}

fn openapi() -> Value {
    serde_json::from_str(&rg_http::openapi::openapi_spec()).unwrap()
}

#[test]
fn registry_and_registered_routes_are_the_same_set() {
    let mut registered = BTreeSet::new();
    for (path, methods) in registered_collab_routes() {
        for method in methods {
            registered.insert((method, path.clone()));
        }
    }
    let listed = registry_operations();
    let missing: Vec<_> = registered.difference(&listed).collect();
    let stale: Vec<_> = listed.difference(&registered).collect();
    assert!(
        missing.is_empty() && stale.is_empty(),
        "capability registry drifted from the router in crates/rg-http/src/lib.rs.\n\
         registered but not in rg_core::collaboration::capabilities::REGISTRY \
         (add a Route to the owning Capability): {missing:#?}\n\
         in the registry but not registered (remove it or register the route): {stale:#?}"
    );
}

#[test]
fn every_registered_operation_is_in_the_openapi_document() {
    let doc = openapi();
    let missing: Vec<_> = registry_operations()
        .into_iter()
        .filter(|(method, path)| doc["paths"][path.as_str()][method.to_lowercase()].is_null())
        .collect();
    assert!(
        missing.is_empty(),
        "operations registered in the router but absent from the OpenAPI document \
         (add the handler to crates/rg-http/src/openapi.rs paths(...)): {missing:#?}"
    );
}

/// Every `$ref`ed component schema reachable from `value`.
fn referenced_schemas(value: &Value, doc: &Value, seen: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(r)) = map.get("$ref") {
                let name = r.rsplit('/').next().unwrap().to_string();
                if seen.insert(name.clone()) {
                    referenced_schemas(&doc["components"]["schemas"][name.as_str()], doc, seen);
                }
            }
            for v in map.values() {
                referenced_schemas(v, doc, seen);
            }
        }
        Value::Array(items) => items.iter().for_each(|v| referenced_schemas(v, doc, seen)),
        _ => {}
    }
}

#[test]
fn write_request_schemas_declare_that_unknown_fields_are_rejected() {
    let doc = openapi();
    let mut checked = 0;
    let mut violations = Vec::new();
    for (method, path) in registry_operations() {
        if !["POST", "PUT", "PATCH", "DELETE"].contains(&method.as_str()) {
            continue;
        }
        let body = &doc["paths"][path.as_str()][method.to_lowercase()]["requestBody"];
        if body.is_null() {
            continue;
        }
        let mut names = BTreeSet::new();
        referenced_schemas(body, &doc, &mut names);
        if names.is_empty() {
            violations.push(format!("{method} {path}: request body has no named schema"));
            continue;
        }
        for name in names {
            if doc["components"]["schemas"][name.as_str()]["additionalProperties"] != json!(false) {
                violations.push(format!("{method} {path}: request schema `{name}` must be #[serde(deny_unknown_fields)]"));
            }
            checked += 1;
        }
    }
    assert!(
        checked >= 10,
        "expected to inspect the write schemas, saw {checked}"
    );
    assert!(
        violations.is_empty(),
        "request schema violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn every_mcp_tool_is_owned_by_a_capability_and_maps_to_a_rest_operation() {
    // Names: TOOL_NAMES == registry's tools == TOOL_ROUTES.
    let advertised: BTreeSet<_> = TOOL_NAMES.iter().copied().collect();
    let owned: BTreeSet<_> = registry::mcp_tools().into_iter().collect();
    let tabled: BTreeSet<_> = TOOL_ROUTES.iter().map(|r| r.tool).collect();
    assert_eq!(
        advertised.difference(&owned).collect::<Vec<_>>(),
        Vec::<&&str>::new(),
        "MCP tools not listed in any capability's mcp_tools (rg_core::collaboration::capabilities::REGISTRY)"
    );
    assert_eq!(
        owned.difference(&advertised).collect::<Vec<_>>(),
        Vec::<&&str>::new(),
        "registry lists MCP tools that rg_mcp::collab::TOOL_NAMES does not have"
    );
    assert_eq!(
        advertised.difference(&tabled).collect::<Vec<_>>(),
        Vec::<&&str>::new(),
        "MCP tools without a row in rg_mcp::collab_contract::TOOL_ROUTES"
    );

    let doc = openapi();
    let operations: BTreeSet<(String, String)> = registry_operations()
        .into_iter()
        .map(|(m, p)| (m, normalize(&p)))
        .collect();

    let mut violations = Vec::new();
    for row in TOOL_ROUTES {
        let path = rest_path(row);
        let key = (row.method.to_string(), normalize(&path));
        assert!(
            operations.contains(&key),
            "{}: {} {} is not a registered collaboration operation",
            row.tool,
            row.method,
            path
        );

        // The owning capability must contain that very operation.
        let owner = registry::all()
            .iter()
            .find(|c| c.mcp_tools.contains(&row.tool))
            .unwrap();
        assert!(
            owner
                .routes
                .iter()
                .any(|r| r.method == row.method && normalize(r.path) == key.1),
            "{}: capability `{}` does not own {} {}",
            row.tool,
            owner.name,
            row.method,
            path
        );

        // OpenAPI: the real (un-normalized) path, parameters and body properties.
        let rest = registry_operations()
            .into_iter()
            .find(|(m, p)| *m == row.method && normalize(p) == key.1)
            .unwrap()
            .1;
        let operation = &doc["paths"][rest.as_str()][row.method.to_lowercase()];
        let params: BTreeSet<_> = operation["parameters"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|p| p["in"] == "query")
                    .filter_map(|p| p["name"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        for q in row.query {
            if !params.contains(*q) {
                violations.push(format!(
                    "{}: query parameter `{q}` is not in OpenAPI for {rest} (has {params:?})",
                    row.tool
                ));
            }
        }
        if row.body.is_empty() {
            continue;
        }
        let mut schemas = BTreeSet::new();
        referenced_schemas(&operation["requestBody"], &doc, &mut schemas);
        let top = schemas.iter().next().cloned().unwrap_or_default();
        let top = operation["requestBody"]["content"]["application/json"]["schema"]["$ref"]
            .as_str()
            .map(|r| r.rsplit('/').next().unwrap().to_string())
            .unwrap_or(top);
        let props = &doc["components"]["schemas"][top.as_str()]["properties"];
        for field in row.body {
            if props[*field].is_null() {
                violations.push(format!(
                    "{}: body property `{field}` is not in request schema `{top}` of {} {rest}",
                    row.tool, row.method
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "MCP/OpenAPI violations:\n{}",
        violations.join("\n")
    );
}

// ─────────────────────────── discovery over HTTP ───────────────────────────

fn instance_url(env: &Env) -> String {
    format!("{}/api/v1/collaboration/capabilities", env.base)
}

async fn probe_methods_allowed(env: &Env, method: &str, path: &str) -> String {
    let path = path
        .replace("{owner}", &env.owner)
        .replace("{name}", &env.repo)
        .replace(
            "{id}",
            if path.starts_with("/admin") {
                "1"
            } else {
                UUID
            },
        )
        .replace("{depends_on_id}", UUID)
        .replace("{group_id}", UUID)
        .replace("{link_id}", UUID)
        .replace("{message_id}", UUID);
    let resp = reqwest::Client::new()
        .request(Method::TRACE, format!("{}/api/v1{path}", env.base))
        .bearer_auth(&env.owner_token)
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        405,
        "{method} {path} is advertised but the server has no route there (TRACE probe answered {})",
        resp.status()
    );
    resp.headers()
        .get("allow")
        .map(|v| v.to_str().unwrap().to_string())
        .unwrap_or_default()
}

#[tokio::test]
async fn m11_capability_list_is_exactly_what_the_server_serves() {
    let env = Env::new().await;
    let (status, body) = env
        .json(
            Method::GET,
            &instance_url(&env),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");

    assert_eq!(body["contract_version"], registry::contract_version());
    assert!(
        body["contract_version"]
            .as_str()
            .unwrap()
            .split('.')
            .count()
            == 2
    );
    assert!(!body["server_version"].as_str().unwrap().is_empty());

    // Names and versions are the registry, nothing invented, nothing omitted.
    let advertised: BTreeSet<String> = body["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap().to_string())
        .collect();
    let expected: BTreeSet<String> = registry::all().iter().map(|c| c.name.to_string()).collect();
    assert_eq!(advertised, expected);
    for c in body["capabilities"].as_array().unwrap() {
        assert!(["stable", "experimental"].contains(&c["status"].as_str().unwrap()));
        assert!(c["version"].as_str().unwrap().split('.').count() == 2);
        assert!(!c["requires"].as_array().unwrap().is_empty());
    }

    // Every advertised operation exists on the live server (a method the route
    // does not serve answers 405 and names the methods it does).
    let mut probed = 0;
    for cap in registry::all() {
        for route in cap.routes {
            let allow = probe_methods_allowed(&env, route.method, route.path).await;
            assert!(
                allow
                    .split(',')
                    .any(|m| m.trim().eq_ignore_ascii_case(route.method)),
                "{} {}: server allows `{allow}`",
                route.method,
                route.path
            );
            probed += 1;
        }
    }
    assert_eq!(probed, registry_operations().len());

    // MCP: stdio only, tool count equals the tools that exist.
    assert_eq!(body["mcp_transport"]["transports"], json!(["stdio"]));
    assert_eq!(body["mcp_transport"]["tool_count"], TOOL_NAMES.len());
    assert_eq!(body["deprecations"], json!([]));
    assert_eq!(
        body["limits"]["page_size_max"],
        rg_core::collaboration::MAX_LIMIT
    );
    let modes: Vec<_> = body["auth_modes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    assert_eq!(modes, ["bearer_token", "session_credentials"]);
}

#[tokio::test]
async fn m11_instance_discovery_needs_authentication_and_names_no_repository() {
    let env = Env::new().await;
    let resp = reqwest::Client::new()
        .get(instance_url(&env))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    let (_, body) = env
        .json(
            Method::GET,
            &instance_url(&env),
            &env.owner_token,
            None,
            None,
        )
        .await;
    let text = body.to_string();
    assert!(
        !text.contains(&env.owner) && !text.contains(&env.repo),
        "instance discovery must carry no repository data"
    );
}

#[tokio::test]
async fn m11_repository_discovery_is_guarded_and_hides_private_repositories() {
    let env = Env::with_repo(json!({"name": "secret-repo", "is_private": true})).await;
    let url = env.url("/capabilities");
    let (stranger, _) = env.user("stranger").await;
    let (reader, reader_id) = env.user("reader").await;
    env.add_collaborator(reader_id, "read").await;
    let (writer, writer_id) = env.user("writer").await;
    env.add_collaborator(writer_id, "write").await;

    // Unauthenticated.
    assert_eq!(
        reqwest::Client::new()
            .get(&url)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );

    // A stranger gets exactly what a missing repository gets.
    let (status, hidden) = env.json(Method::GET, &url, &stranger, None, None).await;
    let missing_url = env.url_in("no-such-repo", "/capabilities");
    let (missing_status, missing) = env
        .json(Method::GET, &missing_url, &stranger, None, None)
        .await;
    assert_eq!((status, missing_status), (404, 404));
    assert_eq!(hidden["error"]["code"], missing["error"]["code"]);
    assert_eq!(hidden["error"]["message"], missing["error"]["message"]);

    // Members see the contract plus their own access.
    let (status, as_owner) = env
        .json(Method::GET, &url, &env.owner_token, None, None)
        .await;
    assert_eq!(status, 200, "{as_owner}");
    assert_eq!(as_owner["repo"], format!("{}/{}", env.owner, env.repo));
    assert_eq!(
        as_owner["caller"],
        json!({"repo_collab_read": true, "repo_write": true})
    );
    assert_eq!(as_owner["contract_version"], registry::contract_version());

    let (status, as_reader) = env.json(Method::GET, &url, &reader, None, None).await;
    assert_eq!(status, 200, "{as_reader}");
    assert_eq!(
        as_reader["caller"],
        json!({"repo_collab_read": true, "repo_write": false})
    );
    let (_, as_writer) = env.json(Method::GET, &url, &writer, None, None).await;
    assert_eq!(as_writer["caller"]["repo_write"], true);
}

#[tokio::test]
async fn m11_public_repository_non_member_is_forbidden_not_hidden() {
    let env = Env::new().await;
    let (stranger, _) = env.user("stranger").await;
    let (status, body) = env
        .json(
            Method::GET,
            &env.url("/capabilities"),
            &stranger,
            None,
            None,
        )
        .await;
    assert_eq!(status, 403, "{body}");
}

// ───────────────────── M10: forward compatibility over HTTP ─────────────────────

/// A client written against contract 1.0: reads only the fields it knows and
/// ignores everything else, treats an unrecognized `status` as experimental.
#[derive(serde::Deserialize)]
struct OldClientCapabilities {
    contract_version: String,
    capabilities: Vec<OldClientCapability>,
}

#[derive(serde::Deserialize)]
struct OldClientCapability {
    name: String,
    status: String,
}

#[derive(serde::Deserialize)]
struct OldClientTask {
    id: String,
    title: String,
    status: String,
    version: i64,
}

#[tokio::test]
async fn m10_an_old_client_reads_responses_that_carry_newer_optional_fields() {
    let env = Env::new().await;
    let s = env.session(&env.owner_token, "A").await;

    // Capabilities: strict-minimum reader.
    let resp = env
        .send(
            Method::GET,
            &instance_url(&env),
            Some(&env.owner_token),
            None,
            None,
        )
        .await;
    let caps: OldClientCapabilities = resp.json().await.unwrap();
    assert_eq!(caps.contract_version, registry::contract_version());
    assert!(caps
        .capabilities
        .iter()
        .any(|c| c.name == "tasks" && c.status == "stable"));

    // A response with fields the old client never heard of still parses.
    let future = json!({
        "contract_version": "1.7",
        "capabilities": [{"name": "dependencies", "status": "preview", "added_in_1_7": true}],
        "brand_new_top_level": {"x": 1}
    });
    let parsed: OldClientCapabilities = serde_json::from_value(future).unwrap();
    // An unknown status is not "stable": the old client must not rely on it.
    assert!(!["stable"].contains(&parsed.capabilities[0].status.as_str()));
    // The server itself is the same-major peer of that client.
    assert!(registry::client_is_compatible("1.0") && !registry::client_is_compatible("1.7"));

    // Real task payload: the old subset parses, extra fields are ignored.
    let (status, task) = env
        .json(
            Method::POST,
            &env.url("/tasks"),
            &env.owner_token,
            Some(&s),
            Some(json!({"title": "t"})),
        )
        .await;
    assert_eq!(status, 201, "{task}");
    let old: OldClientTask = serde_json::from_value(task.clone()).unwrap();
    assert_eq!(
        (old.title.as_str(), old.status.as_str(), old.version),
        ("t", "open", 1)
    );
    assert_eq!(old.id, task["id"]);
}

#[tokio::test]
async fn m10_unknown_safety_critical_enum_values_fail_closed() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let (_, task) = env
        .json(
            Method::POST,
            &env.url("/tasks"),
            &env.owner_token,
            Some(&a),
            Some(json!({"title": "t"})),
        )
        .await;
    let tid = task["id"].as_str().unwrap().to_string();

    // A terminal-state value the server does not know is never stored as success.
    let (status, body) = env
        .json(
            Method::PATCH,
            &env.url(&format!("/tasks/{tid}")),
            &env.owner_token,
            Some(&a),
            Some(json!({"expected_version": 1, "status": "completed"})),
        )
        .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"]["code"], "UNKNOWN_ENUM_VALUE");
    assert_eq!(body["error"]["details"]["field"], "status");
    assert_eq!(body["error"]["details"]["value"], "completed");
    assert!(body["error"]["details"]["allowed"]
        .as_array()
        .unwrap()
        .contains(&json!("done")));
    let (_, after) = env
        .json(
            Method::GET,
            &env.url(&format!("/tasks/{tid}")),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(
        (after["status"].as_str(), after["version"].as_i64()),
        (Some("open"), Some(1)),
        "task unchanged"
    );

    // Filters fail closed too, instead of silently listing everything.
    let (status, body) = env
        .json(
            Method::GET,
            &env.url("/tasks?status=archived"),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (400, Some("UNKNOWN_ENUM_VALUE"))
    );
    let (status, body) = env
        .json(
            Method::GET,
            &env.url(&format!("/tasks/{tid}/questions?status=expired")),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (400, Some("UNKNOWN_ENUM_VALUE"))
    );

    // An unknown recipient kind (authorization scope) creates no question.
    let (status, body) = env
        .json(
            Method::POST,
            &env.url(&format!("/tasks/{tid}/questions")),
            &env.owner_token,
            None,
            Some(json!({
                "client_question_id": "q-unknown-recipient",
                "recipient": {"kind": "everyone"},
                "title": "t", "body": "b"
            })),
        )
        .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (400, Some("UNKNOWN_ENUM_VALUE"))
    );
    let (_, list) = env
        .json(
            Method::GET,
            &env.url(&format!("/tasks/{tid}/questions")),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(list["items"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn m10_a_stored_recipient_kind_from_a_newer_build_cannot_be_answered() {
    use rg_db::sea_orm::{ActiveModelTrait, EntityTrait, Set};
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let (_, task) = env
        .json(
            Method::POST,
            &env.url("/tasks"),
            &env.owner_token,
            Some(&a),
            Some(json!({"title": "t"})),
        )
        .await;
    let tid = task["id"].as_str().unwrap();
    let (status, q) = env
        .json(
            Method::POST,
            &env.url(&format!("/tasks/{tid}/questions")),
            &env.owner_token,
            Some(&a),
            Some(json!({"client_question_id": "q1", "recipient": {"kind": "repo_writers"}, "title": "t", "body": "b"})),
        )
        .await;
    assert_eq!(status, 201, "{q}");
    let qid = q["id"].as_str().unwrap();

    // Simulate a row written by a newer build with a recipient kind we do not know.
    let row = rg_db::entities::collab_question::Entity::find_by_id(qid.to_string())
        .one(&env.db)
        .await
        .unwrap()
        .unwrap();
    let mut active: rg_db::entities::collab_question::ActiveModel = row.into();
    active.recipient_kind = Set("team".to_string());
    active.update(&env.db).await.unwrap();

    let (status, body) = env
        .json(
            Method::POST,
            &env.url(&format!("/questions/{qid}/answer")),
            &env.owner_token,
            None,
            Some(json!({
                "client_answer_id": "a1", "expected_version": q["version"], "body": "yes",
                "acknowledged_context_hash": q["context_hash"]
            })),
        )
        .await;
    assert_eq!(status, 403, "an unknown recipient kind must deny: {body}");
    let (_, after) = env
        .json(
            Method::GET,
            &env.url(&format!("/questions/{qid}")),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(after["status"], "open", "still open, not answered");
}

/// A body that is valid for the endpoint plus one field it does not define.
async fn assert_unknown_field_rejected(
    env: &Env,
    method: Method,
    suffix: &str,
    session: Option<&Sess>,
    body: Value,
) {
    let mut with_extra = body;
    with_extra
        .as_object_mut()
        .unwrap()
        .insert("expected_versoin".into(), json!(1));
    let (status, resp) = env
        .json(
            method.clone(),
            &env.url(suffix),
            &env.owner_token,
            session,
            Some(with_extra),
        )
        .await;
    assert_eq!(status, 400, "{method} {suffix}: {resp}");
    assert_eq!(
        resp["error"]["code"], "UNKNOWN_FIELD",
        "{method} {suffix}: {resp}"
    );
    assert_eq!(
        resp["error"]["details"]["field"], "expected_versoin",
        "{method} {suffix}"
    );
}

#[tokio::test]
async fn m10_unknown_write_fields_are_rejected_by_every_write_endpoint() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let b = env.session(&env.owner_token, "B").await;
    let (_, task) = env
        .json(
            Method::POST,
            &env.url("/tasks"),
            &env.owner_token,
            Some(&a),
            Some(json!({"title": "t"})),
        )
        .await;
    let tid = task["id"].as_str().unwrap();

    // sessions
    assert_unknown_field_rejected(
        &env,
        Method::POST,
        "/sessions",
        None,
        json!({"display_name": "x", "client_kind": "c"}),
    )
    .await;
    // tasks
    assert_unknown_field_rejected(
        &env,
        Method::POST,
        "/tasks",
        Some(&a),
        json!({"title": "t2"}),
    )
    .await;
    assert_unknown_field_rejected(
        &env,
        Method::PATCH,
        &format!("/tasks/{tid}"),
        Some(&a),
        json!({"expected_version": 1, "title": "n"}),
    )
    .await;
    // messages
    assert_unknown_field_rejected(
        &env,
        Method::POST,
        &format!("/tasks/{tid}/messages"),
        Some(&a),
        json!({"client_message_id": "m1", "kind": "note", "body": "hi"}),
    )
    .await;
    assert_unknown_field_rejected(
        &env,
        Method::PUT,
        &format!("/tasks/{tid}/read"),
        Some(&a),
        json!({"last_read_seq": 0}),
    )
    .await;
    // handoffs
    assert_unknown_field_rejected(
        &env,
        Method::POST,
        &format!("/tasks/{tid}/handoffs"),
        Some(&a),
        json!({"client_handoff_id": "h1", "to_session_id": b.id, "summary": "s"}),
    )
    .await;
    assert_unknown_field_rejected(
        &env,
        Method::POST,
        &format!("/handoffs/{UUID}/accept"),
        Some(&b),
        json!({"expected_version": 1}),
    )
    .await;
    assert_unknown_field_rejected(
        &env,
        Method::POST,
        &format!("/handoffs/{UUID}/cancel"),
        Some(&a),
        json!({"expected_version": 1}),
    )
    .await;
    // takeovers
    assert_unknown_field_rejected(
        &env,
        Method::POST,
        &format!("/tasks/{tid}/takeovers"),
        Some(&b),
        json!({"client_takeover_id": "t1", "expected_task_version": 1, "reason": "r"}),
    )
    .await;
    assert_unknown_field_rejected(
        &env, Method::POST, &format!("/tasks/{tid}/reassign"), None,
        json!({"client_takeover_id": "r1", "expected_task_version": 1, "to_session_id": b.id, "reason": "r"}),
    ).await;
    // questions
    assert_unknown_field_rejected(
        &env, Method::POST, &format!("/tasks/{tid}/questions"), None,
        json!({"client_question_id": "q1", "recipient": {"kind": "repo_writers"}, "title": "t", "body": "b"}),
    ).await;
    assert_unknown_field_rejected(
        &env, Method::POST, &format!("/questions/{UUID}/answer"), None,
        json!({"client_answer_id": "a1", "expected_version": 1, "body": "b", "acknowledged_context_hash": "h"}),
    ).await;
    assert_unknown_field_rejected(
        &env,
        Method::POST,
        &format!("/questions/{UUID}/cancel"),
        None,
        json!({"expected_version": 1}),
    )
    .await;
    // work packages
    assert_unknown_field_rejected(
        &env,
        Method::POST,
        &format!("/tasks/{tid}/work-packages"),
        None,
        json!({"client_package_id": "w1", "goal": "g"}),
    )
    .await;
}

#[tokio::test]
async fn m10_new_write_schemas_reject_unknown_fields_over_http() {
    let env = Env::new().await;
    let session = env.session(&env.owner_token, "A").await;
    let (_, task) = env
        .json(
            Method::POST,
            &env.url("/tasks"),
            &env.owner_token,
            Some(&session),
            Some(json!({"title": "t"})),
        )
        .await;
    let tid = task["id"].as_str().unwrap();
    let template = json!({"client_template_id":"x", "name":"n", "title_pattern":"t", "goal":"g"});
    for (method, path, body) in [
        (
            Method::POST,
            format!("/tasks/{tid}/dependencies"),
            json!({"client_request_id":"x", "depends_on_task_id":UUID, "expected_graph_version":0}),
        ),
        (
            Method::DELETE,
            format!("/tasks/{tid}/dependencies/{UUID}"),
            json!({"client_request_id":"x", "expected_graph_version":0}),
        ),
        (
            Method::DELETE,
            format!("/groups/{UUID}/links/{UUID}"),
            json!({"client_request_id":"x", "expected_group_version":1}),
        ),
        (
            Method::POST,
            format!("/tasks/{tid}/decisions"),
            json!({"client_decision_id":"x", "title":"t", "rationale":"r"}),
        ),
        (Method::POST, "/templates".into(), template),
        (
            Method::POST,
            format!("/templates/{UUID}/versions"),
            json!({"client_version_id":"x", "expected_version":1, "title_pattern":"t", "goal":"g"}),
        ),
        (
            Method::PUT,
            format!("/sessions/{}/declaration", session.id),
            json!({"expected_version":0}),
        ),
        (
            Method::PUT,
            format!("/tasks/{tid}/budgets"),
            json!({"expected_version":0,"unit":"USD","source":"invoice","amount":"1","sampling_period":"day"}),
        ),
        (
            Method::POST,
            "/groups".into(),
            json!({"client_request_id":"x","title":"t"}),
        ),
        (
            Method::PUT,
            "/escalation-settings".into(),
            json!({"expected_version":0,"enabled":false,"blocked_after_secs":3600,"question_after_secs":3600,"handoff_after_secs":3600,"repeat_after_secs":0}),
        ),
    ] {
        assert_unknown_field_rejected(&env, method, &path, Some(&session), body).await;
    }
    for (path, body) in [
        (
            "/templates".to_string(),
            json!({"client_template_id":"x","name":"n","title_pattern":"t","goal":"g","checklist":[{"text":"t","unexpected":true}]}),
        ),
        (
            "/escalation-settings".to_string(),
            json!({"expected_version":0,"enabled":false,"blocked_after_secs":3600,"question_after_secs":3600,"handoff_after_secs":3600,"repeat_after_secs":0,"quiet_hours":{"start_minute":10,"end_minute":20,"unexpected":true}}),
        ),
    ] {
        let method = if path == "/templates" {
            Method::POST
        } else {
            Method::PUT
        };
        let (status, resp) = env
            .json(
                method,
                &env.url(&path),
                &env.owner_token,
                Some(&session),
                Some(body),
            )
            .await;
        assert_eq!(
            (
                status,
                resp["error"]["code"].as_str(),
                resp["error"]["details"]["field"].as_str()
            ),
            (400, Some("UNKNOWN_FIELD"), Some("unexpected")),
            "{path}: {resp}"
        );
    }
}

#[tokio::test]
async fn m10_unknown_nested_fields_are_rejected_too() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let (_, task) = env
        .json(
            Method::POST,
            &env.url("/tasks"),
            &env.owner_token,
            Some(&a),
            Some(json!({"title": "t"})),
        )
        .await;
    let tid = task["id"].as_str().unwrap();
    let (status, resp) = env
        .json(
            Method::POST,
            &env.url(&format!("/tasks/{tid}/questions")),
            &env.owner_token,
            None,
            Some(json!({
                "client_question_id": "qn", "title": "t", "body": "b",
                "recipient": {"kind": "repo_writers", "admins_only": true}
            })),
        )
        .await;
    assert_eq!(
        (status, resp["error"]["code"].as_str()),
        (400, Some("UNKNOWN_FIELD")),
        "{resp}"
    );
    assert_eq!(resp["error"]["details"]["field"], "admins_only");
}

#[tokio::test]
async fn m10_a_rejected_request_has_no_effect_and_the_same_body_without_the_typo_works() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let (_, task) = env
        .json(
            Method::POST,
            &env.url("/tasks"),
            &env.owner_token,
            Some(&a),
            Some(json!({"title": "t"})),
        )
        .await;
    let tid = task["id"].as_str().unwrap();

    // The typo'd version guard would have been silently dropped before.
    let (status, resp) = env
        .json(
            Method::PATCH,
            &env.url(&format!("/tasks/{tid}")),
            &env.owner_token,
            Some(&a),
            Some(json!({"expected_versoin": 1, "status": "done"})),
        )
        .await;
    assert_eq!(status, 400, "{resp}");
    let (_, unchanged) = env
        .json(
            Method::GET,
            &env.url(&format!("/tasks/{tid}")),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(
        (unchanged["status"].as_str(), unchanged["version"].as_i64()),
        (Some("open"), Some(1))
    );

    let (status, resp) = env
        .json(
            Method::PATCH,
            &env.url(&format!("/tasks/{tid}")),
            &env.owner_token,
            Some(&a),
            Some(json!({"expected_version": 1, "status": "done"})),
        )
        .await;
    assert_eq!(status, 200, "{resp}");
    assert_eq!(resp["status"], "done");

    // A rejected create makes no object: sessions before/after.
    let (_, before) = env
        .json(
            Method::GET,
            &env.url("/sessions"),
            &env.owner_token,
            None,
            None,
        )
        .await;
    let (status, _) = env
        .json(
            Method::POST,
            &env.url("/sessions"),
            &env.owner_token,
            None,
            Some(json!({"display_name": "n", "client_kind": "c", "color": "red"})),
        )
        .await;
    assert_eq!(status, 400);
    let (_, after) = env
        .json(
            Method::GET,
            &env.url("/sessions"),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert_eq!(
        before["items"].as_array().unwrap().len(),
        after["items"].as_array().unwrap().len()
    );
}

#[tokio::test]
async fn m10_malformed_bodies_are_clean_400s_not_422() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "A").await;
    let client = reqwest::Client::new();
    let post = |body: &'static str| {
        client
            .post(env.url("/tasks"))
            .bearer_auth(&env.owner_token)
            .header("x-ironforge-session", &a.id)
            .header("x-ironforge-session-secret", &a.secret)
            .header("content-type", "application/json")
            .body(body)
            .send()
    };

    let resp = post("{not json").await.unwrap();
    assert_eq!(resp.status(), 400);
    assert_eq!(
        resp.json::<Value>().await.unwrap()["error"]["code"],
        "INVALID_JSON"
    );

    let resp = post("{}").await.unwrap();
    assert_eq!(resp.status(), 400);
    let body = resp.json::<Value>().await.unwrap();
    assert_eq!(body["error"]["code"], "INVALID_REQUEST_BODY");
    assert!(
        body["error"]["message"].as_str().unwrap().contains("title"),
        "{body}"
    );

    let resp = post(r#"{"title": 5}"#).await.unwrap();
    assert_eq!(resp.status(), 400);
    assert_eq!(
        resp.json::<Value>().await.unwrap()["error"]["code"],
        "INVALID_REQUEST_BODY"
    );
}
